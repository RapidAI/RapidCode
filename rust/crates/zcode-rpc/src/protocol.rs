//! Layer 2: 传输协议抽象
//!
//! Rust 移植自 `packages/rpc/src/protocol.ts`。
//!
//! [`MessagePassingProtocol`] 是整个 IPC 框架的「腰部」——
//! 它上面是 Channel RPC 层（完全传输无关），
//! 它下面是各种具体传输实现（Electron / MessagePort / Socket / ChildProcess）。
//!
//! 只要实现 `send` 和 `on_message`，就能接入整个 RPC 框架。

use crate::buffer::VSBuffer;
use crate::foundation::{Disposable, DisposableStore, Event, Emitter};
use crate::promise::Promise;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

// ============================================================================
// 核心传输接口
// ============================================================================

/// 整个框架的核心抽象。
///
/// 上层代码只看到 `send` / `on_message`，
/// 不管底层是 Electron IPC、MessagePort、WebSocket 还是 TCP Socket。
pub trait MessagePassingProtocol: Send + Sync {
    fn send(&self, buffer: VSBuffer);
    fn on_message(&self) -> Event<VSBuffer>;
    /// 可选：等待发送侧 drain 完成。默认立即完成。
    fn drain(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

/// 连接级只读流控观察面。
///
/// transport 负责维护未确认字节与状态边沿；业务层只能订阅，
/// 不能伪造 ACK 或直接改水位。
pub trait ConnectionFlowControl: Send + Sync {
    fn unacknowledged_bytes(&self) -> usize;
    fn on_saturated(&self) -> Event<()>;
    fn on_drained(&self) -> Event<()>;
}

/// MessagePort 流控状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessagePortFlowState {
    Saturated,
    Drained,
}

// ============================================================================
// Socket 接口（用于 TCP/WebSocket 等流式传输）
// ============================================================================

/// 抽象底层网络 socket。
///
/// 在 Node.js 环境是 `net.Socket`，在浏览器环境是 `WebSocket`。
pub trait Socket: Disposable {
    fn on_data(&self) -> Event<VSBuffer>;
    fn on_close(&self) -> Event<()>;
    fn on_end(&self) -> Event<()>;
    fn write(&self, buffer: VSBuffer);
    fn end(&self);
    fn drain(&self) -> Promise<()>;
}

// ============================================================================
// ChunkStream —— 处理 TCP 的分片和粘包
// ============================================================================

/// TCP 是流式协议，一次 write 不代表对面一次 read 就能完整收到。
///
/// `ChunkStream` 把收到的碎片攒起来，按需读取指定字节数。
#[derive(Default)]
pub struct ChunkStream {
    chunks: Vec<VSBuffer>,
    total_length: usize,
}

impl ChunkStream {
    pub fn new() -> Self {
        ChunkStream {
            chunks: Vec::new(),
            total_length: 0,
        }
    }

    pub fn byte_length(&self) -> usize {
        self.total_length
    }

    pub fn accept_chunk(&mut self, chunk: VSBuffer) {
        self.total_length += chunk.byte_length();
        self.chunks.push(chunk);
    }

    /// 预览前 `byte_count` 字节，但不消费底层缓冲。
    ///
    /// Socket/stdio 传输天然可能分片。若在 body 还没收全时先 read 掉 header，
    /// 后续再来的 body 会失去对应的帧头，导致消息永远卡住。
    /// 这里提供 peek，让调用方先判断「整帧是否已经到齐」，确认足够后再真正消费。
    pub fn peek(&self, byte_count: usize) -> Option<VSBuffer> {
        if self.total_length < byte_count {
            return None;
        }

        if self.chunks[0].byte_length() >= byte_count {
            return Some(self.chunks[0].slice(0, Some(byte_count)));
        }

        let mut result = VSBuffer::alloc(byte_count);
        let mut offset = 0usize;
        for chunk in &self.chunks {
            if offset >= byte_count {
                break;
            }
            let remaining = byte_count - offset;
            let copy_length = chunk.byte_length().min(remaining);
            let piece = if copy_length == chunk.byte_length() {
                chunk.clone()
            } else {
                chunk.slice(0, Some(copy_length))
            };
            result.set(piece.as_bytes(), offset);
            offset += copy_length;
        }
        Some(result)
    }

    /// 丢弃前 `byte_count` 字节。
    pub fn skip(&mut self, byte_count: usize) {
        if self.read(byte_count).is_none() {
            panic!("ChunkStream.skip({byte_count}) 超出可读范围");
        }
    }

    /// 读取 `byte_count` 字节，不够就返回 `None`。
    pub fn read(&mut self, byte_count: usize) -> Option<VSBuffer> {
        if self.total_length < byte_count {
            return None;
        }

        if self.chunks[0].byte_length() == byte_count {
            let result = self.chunks.remove(0);
            self.total_length -= byte_count;
            return Some(result);
        }

        if self.chunks[0].byte_length() > byte_count {
            let result = self.chunks[0].slice(0, Some(byte_count));
            self.chunks[0] = self.chunks[0].slice(byte_count, None);
            self.total_length -= byte_count;
            return Some(result);
        }

        // 需要跨多个 chunk 拼接
        let mut result = VSBuffer::alloc(byte_count);
        let mut offset = 0usize;
        while offset < byte_count {
            let chunk = self.chunks[0].clone();
            let needed = byte_count - offset;
            if chunk.byte_length() <= needed {
                result.set(chunk.as_bytes(), offset);
                offset += chunk.byte_length();
                self.chunks.remove(0);
            } else {
                result.set(chunk.slice(0, Some(needed)).as_bytes(), offset);
                self.chunks[0] = chunk.slice(needed, None);
                offset += needed;
            }
        }
        self.total_length -= byte_count;
        Some(result)
    }
}

// ============================================================================
// Protocol —— 在 Socket 上实现 MessagePassingProtocol
// ============================================================================

/// 消息帧格式 (13 bytes header):
///
/// ```text
/// ┌─────────┬──────────┬──────────┬──────────────┐
/// │ type(1) │  id(4)   │  ack(4)  │  length(4)   │
/// └─────────┴──────────┴──────────┴──────────────┘
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ProtocolMessageType {
    None = 0,
    Regular = 1,
    Control = 2,
    Ack = 3,
    Disconnect = 5,
    ReplayRequest = 6,
    Pause = 7,
    Resume = 8,
    KeepAlive = 9,
}

impl ProtocolMessageType {
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => ProtocolMessageType::Regular,
            2 => ProtocolMessageType::Control,
            3 => ProtocolMessageType::Ack,
            5 => ProtocolMessageType::Disconnect,
            6 => ProtocolMessageType::ReplayRequest,
            7 => ProtocolMessageType::Pause,
            8 => ProtocolMessageType::Resume,
            9 => ProtocolMessageType::KeepAlive,
            _ => ProtocolMessageType::None,
        }
    }
}

/// 帧头长度：1 + 4 + 4 + 4。
pub const HEADER_SIZE: usize = 13;

/// 一条协议消息。
#[derive(Clone, Debug)]
pub struct ProtocolMessage {
    pub message_type: ProtocolMessageType,
    pub id: u32,
    pub ack: u32,
    pub data: VSBuffer,
}

impl ProtocolMessage {
    pub fn new(message_type: ProtocolMessageType, id: u32, ack: u32, data: VSBuffer) -> Self {
        ProtocolMessage {
            message_type,
            id,
            ack,
            data,
        }
    }

    pub fn byte_length(&self) -> usize {
        HEADER_SIZE + self.data.byte_length()
    }
}

/// 把消息编码为字节帧。
pub fn write_protocol_message(msg: &ProtocolMessage) -> VSBuffer {
    let mut result = VSBuffer::alloc(HEADER_SIZE + msg.data.byte_length());
    result.write_u8(msg.message_type as u8, 0);
    result.write_u32_be(msg.id, 1);
    result.write_u32_be(msg.ack, 5);
    result.write_u32_be(msg.data.byte_length() as u32, 9);
    result.set(msg.data.as_bytes(), HEADER_SIZE);
    result
}

/// 基础 Protocol: 在 [`Socket`] 上加消息帧，实现 [`MessagePassingProtocol`]。
///
/// 只做消息分帧，不做 ACK/重连（那是 `PersistentProtocol` 的事）。
pub struct SocketProtocol {
    emitter: Arc<Emitter<VSBuffer>>,
    chunk_stream: Mutex<ChunkStream>,
    disposables: Mutex<DisposableStore>,
    socket: Arc<dyn Socket>,
    disposed: AtomicBool,
}

impl SocketProtocol {
    pub fn new(socket: Arc<dyn Socket>) -> Arc<Self> {
        let emitter = Arc::new(Emitter::new());
        let disposables = Mutex::new(DisposableStore::new());

        let protocol = Arc::new(SocketProtocol {
            emitter: emitter.clone(),
            chunk_stream: Mutex::new(ChunkStream::new()),
            disposables,
            socket: socket.clone(),
            disposed: AtomicBool::new(false),
        });

        let weak = Arc::downgrade(&protocol);
        let d = socket.on_data().subscribe(move |data: VSBuffer| {
            if let Some(p) = weak.upgrade() {
                p.chunk_stream.lock().unwrap().accept_chunk(data);
                p.read_messages();
            }
        });
        protocol.disposables.lock().unwrap().add_boxed(d);

        protocol
    }

    fn read_messages(&self) {
        loop {
            let header = match self.chunk_stream.lock().unwrap().peek(HEADER_SIZE) {
                Some(h) => h,
                None => break,
            };

            let msg_type = ProtocolMessageType::from_u8(header.read_u8(0));
            let length = header.read_u32_be(9) as usize;
            let total_frame_length = HEADER_SIZE + length;

            if self.chunk_stream.lock().unwrap().byte_length() < total_frame_length {
                // 不能在 body 未到齐时提前消费 header，否则下一段数据拼上来后
                // 已经找不到这帧的长度信息，调用方就会一直等待一个永远不会完成的 Promise。
                break;
            }

            self.chunk_stream.lock().unwrap().skip(HEADER_SIZE);

            if length == 0 {
                if msg_type == ProtocolMessageType::Regular {
                    self.emitter.fire(VSBuffer::alloc(0));
                }
                continue;
            }

            let body = self.chunk_stream.lock().unwrap().read(length);
            match body {
                Some(body) => {
                    if msg_type == ProtocolMessageType::Regular {
                        self.emitter.fire(body);
                    }
                }
                None => panic!("SocketProtocol 读取到完整帧长度后 body 不应为空"),
            }
        }
    }
}

impl MessagePassingProtocol for SocketProtocol {
    fn send(&self, buffer: VSBuffer) {
        let msg = ProtocolMessage::new(ProtocolMessageType::Regular, 0, 0, buffer);
        self.socket.write(write_protocol_message(&msg));
    }

    fn on_message(&self) -> Event<VSBuffer> {
        self.emitter.event()
    }

    fn drain(&self) -> Promise<()> {
        self.socket.drain()
    }
}

impl Disposable for SocketProtocol {
    fn dispose(&self) {
        if self.disposed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.disposables.lock().unwrap().dispose();
        self.emitter.dispose();
    }
}

// ============================================================================
// QueueProtocol —— 内存中的协议对，用于测试
// ============================================================================

struct QueueProtocol {
    tx: Mutex<Sender<VSBuffer>>,
    emitter: Arc<Emitter<VSBuffer>>,
}

impl MessagePassingProtocol for QueueProtocol {
    fn send(&self, buffer: VSBuffer) {
        // A 发送的消息 → B 收到（异步投递，模拟 setTimeout(..., 0)）。
        let _ = self.tx.lock().unwrap().send(buffer);
    }

    fn on_message(&self) -> Event<VSBuffer> {
        self.emitter.event()
    }
}

/// 创建一对通过内存队列连接的 protocol。
///
/// 一端 `send` 的消息会异步出现在另一端的 `on_message`。
/// 非常适合单元测试，不需要真正的网络连接。
pub fn create_queue_pair() -> (Arc<dyn MessagePassingProtocol>, Arc<dyn MessagePassingProtocol>) {
    let (tx_a, rx_a) = channel::<VSBuffer>();
    let (tx_b, rx_b) = channel::<VSBuffer>();

    let emitter_a = Arc::new(Emitter::<VSBuffer>::new());
    let emitter_b = Arc::new(Emitter::<VSBuffer>::new());

    // A 发出的消息（tx_a → rx_a）投递到 B 的 emitter
    {
        let emitter_b = emitter_b.clone();
        std::thread::spawn(move || {
            for msg in rx_a {
                emitter_b.fire(msg);
            }
        });
    }
    // B 发出的消息（tx_b → rx_b）投递到 A 的 emitter
    {
        let emitter_a = emitter_a.clone();
        std::thread::spawn(move || {
            for msg in rx_b {
                emitter_a.fire(msg);
            }
        });
    }

    let protocol_a: Arc<dyn MessagePassingProtocol> = Arc::new(QueueProtocol {
        tx: Mutex::new(tx_a),
        emitter: emitter_a,
    });
    let protocol_b: Arc<dyn MessagePassingProtocol> = Arc::new(QueueProtocol {
        tx: Mutex::new(tx_b),
        emitter: emitter_b,
    });

    (protocol_a, protocol_b)
}

// ============================================================================
// MessagePort Protocol —— 用于 Web Worker / Electron sandbox
// ============================================================================

/// MessagePort 的最小抽象，使该 Protocol 可同时用于浏览器和 Electron。
pub trait MessagePortLike: Send + Sync {
    fn add_message_listener(&self, listener: Arc<dyn Fn(Vec<u8>) + Send + Sync>);
    fn post_message(&self, message: Vec<u8>);
    fn start(&self);
    fn close(&self);
}

/// 在 [`MessagePortLike`] 上实现 [`MessagePassingProtocol`]。
///
/// 这是最简单的传输实现——不需要分帧，因为 MessagePort 本身就是消息边界的。
pub struct MessagePortProtocol {
    emitter: Arc<Emitter<VSBuffer>>,
    flow_emitter: Arc<Emitter<MessagePortFlowState>>,
    port: Arc<dyn MessagePortLike>,
}

impl MessagePortProtocol {
    pub fn new(port: Arc<dyn MessagePortLike>) -> Arc<Self> {
        let emitter = Arc::new(Emitter::new());
        let flow_emitter = Arc::new(Emitter::new());
        let protocol = Arc::new(MessagePortProtocol {
            emitter: emitter.clone(),
            flow_emitter,
            port: port.clone(),
        });

        let emitter_for_listener = emitter.clone();
        port.add_message_listener(Arc::new(move |data: Vec<u8>| {
            // MessagePort control object 不能进入 Channel deserialize；
            // 只有真实字节才是 RPC binary。
            emitter_for_listener.fire(VSBuffer::wrap(data));
        }));
        port.start();
        protocol
    }

    /// 订阅流控状态变化。
    pub fn on_flow_state(&self) -> Event<MessagePortFlowState> {
        self.flow_emitter.event()
    }

    /// 主动发送流控状态。
    pub fn send_flow_state(&self, state: MessagePortFlowState) {
        self.flow_emitter.fire(state);
    }

    /// 断开连接。
    pub fn disconnect(&self) {
        self.port.close();
        self.emitter.dispose();
        self.flow_emitter.dispose();
    }
}

impl MessagePassingProtocol for MessagePortProtocol {
    fn send(&self, buffer: VSBuffer) {
        self.port.post_message(buffer.into_bytes());
    }

    fn on_message(&self) -> Event<VSBuffer> {
        self.emitter.event()
    }
}
