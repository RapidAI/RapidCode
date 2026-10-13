//! PersistentProtocol —— 可靠性层
//!
//! Rust 移植自 `packages/rpc/src/persistent-protocol.ts`。
//!
//! 在 [`SocketProtocol`](crate::protocol::SocketProtocol) 之上添加：
//! - 消息 ACK 确认机制
//! - 心跳保活 (keepAlive)
//! - 断线重连时重放未确认的消息（有界：字节上限 + 时间宽限窗）
//! - 拥塞信号：`unacknowledgedBytes` 水位 + `onSaturated` / `onDrained`
//!
//! 关键：Channel RPC 层完全不知道 PersistentProtocol 的存在，
//! 它只看到 [`MessagePassingProtocol`] 接口。这就是分层抽象的威力。

use crate::buffer::VSBuffer;
use crate::foundation::{Disposable, DisposableStore, Event, Emitter};
use crate::promise::Promise;
use crate::protocol::{
    write_protocol_message, ChunkStream, ConnectionFlowControl, MessagePassingProtocol, ProtocolMessage,
    ProtocolMessageType, Socket, HEADER_SIZE,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// PersistentProtocol 可调参数（v4 通道层补丁）。
#[derive(Clone, Debug)]
pub struct PersistentProtocolOptions {
    /// 未 ACK 字节高水位：越过即 onSaturated。
    pub saturation_high_water_mark_bytes: usize,
    /// 未 ACK 字节低水位：饱和后回落至此即 onDrained。
    pub saturation_low_water_mark_bytes: usize,
    /// 重放缓冲字节上限：越界 = 放弃协议会话（onClose）。
    pub replay_buffer_max_bytes: usize,
    /// 重放缓冲时间宽限窗（ms）：最老未 ACK 消息超龄同样放弃会话。
    pub replay_buffer_grace: Duration,
}

impl Default for PersistentProtocolOptions {
    fn default() -> Self {
        PersistentProtocolOptions {
            saturation_high_water_mark_bytes: 1024 * 1024,
            saturation_low_water_mark_bytes: 1024 * 1024 / 4,
            replay_buffer_max_bytes: 8 * 1024 * 1024,
            replay_buffer_grace: Duration::from_millis(45_000),
        }
    }
}

struct UnackEntry {
    msg: ProtocolMessage,
    queued_at: Instant,
}

const KEEP_ALIVE_INTERVAL: Duration = Duration::from_millis(5_000);
const ACK_TIMEOUT: Duration = Duration::from_millis(20_000);

/// 可靠性协议实现。
pub struct PersistentProtocol {
    message_emitter: Arc<Emitter<VSBuffer>>,
    close_emitter: Arc<Emitter<()>>,
    socket_close_emitter: Arc<Emitter<()>>,
    saturated_emitter: Arc<Emitter<()>>,
    drained_emitter: Arc<Emitter<()>>,

    socket: Mutex<Arc<dyn Socket>>,
    chunk_stream: Mutex<ChunkStream>,
    disposables: Mutex<DisposableStore>,

    // ACK 机制（队列有界化，条目携带入队时间用于宽限窗判定）
    outgoing_msg_id: Mutex<u32>,
    outgoing_unack: Mutex<Vec<UnackEntry>>,
    incoming_ack_id: Mutex<u32>,
    unack_bytes: Mutex<usize>,
    saturated: AtomicBool,
    overflowed: AtomicBool,
    last_ack_time: Mutex<Instant>,

    options: PersistentProtocolOptions,

    // 定时器线程控制
    stop_flag: Arc<AtomicBool>,
    keep_alive_handle: Mutex<Option<std::thread::JoinHandle<()>>>,
    ack_check_handle: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl PersistentProtocol {
    /// 创建并启动（绑定 socket、开启心跳与 ACK 检查）。
    pub fn new(socket: Arc<dyn Socket>, options: PersistentProtocolOptions) -> Arc<Self> {
        let protocol = Arc::new(PersistentProtocol {
            message_emitter: Arc::new(Emitter::new()),
            close_emitter: Arc::new(Emitter::new()),
            socket_close_emitter: Arc::new(Emitter::new()),
            saturated_emitter: Arc::new(Emitter::new()),
            drained_emitter: Arc::new(Emitter::new()),
            socket: Mutex::new(socket),
            chunk_stream: Mutex::new(ChunkStream::new()),
            disposables: Mutex::new(DisposableStore::new()),
            outgoing_msg_id: Mutex::new(0),
            outgoing_unack: Mutex::new(Vec::new()),
            incoming_ack_id: Mutex::new(0),
            unack_bytes: Mutex::new(0),
            saturated: AtomicBool::new(false),
            overflowed: AtomicBool::new(false),
            last_ack_time: Mutex::new(Instant::now()),
            options,
            stop_flag: Arc::new(AtomicBool::new(false)),
            keep_alive_handle: Mutex::new(None),
            ack_check_handle: Mutex::new(None),
        });

        protocol.bind_socket();
        protocol.start_keep_alive();
        protocol.start_ack_check();
        protocol
    }

    /// 订阅协议级关闭（放弃会话 / 对端 Disconnect）。
    pub fn on_close(&self) -> Event<()> {
        self.close_emitter.event()
    }

    /// 订阅底层 socket 关闭。
    pub fn on_socket_close(&self) -> Event<()> {
        self.socket_close_emitter.event()
    }

    fn bind_socket(self: &Arc<Self>) {
        let socket = self.socket.lock().unwrap().clone();

        let weak = Arc::downgrade(self);
        let d = socket.on_data().subscribe(move |data: VSBuffer| {
            if let Some(p) = weak.upgrade() {
                p.chunk_stream.lock().unwrap().accept_chunk(data);
                p.read_messages();
            }
        });
        self.disposables.lock().unwrap().add_boxed(d);

        let weak = Arc::downgrade(self);
        let d = socket.on_close().subscribe(move |_: ()| {
            if let Some(p) = weak.upgrade() {
                p.socket_close_emitter.fire(());
            }
        });
        self.disposables.lock().unwrap().add_boxed(d);
    }

    fn write_message(&self, msg: &ProtocolMessage) {
        let socket = self.socket.lock().unwrap().clone();
        socket.write(write_protocol_message(msg));
    }

    fn read_messages(&self) {
        loop {
            if self.chunk_stream.lock().unwrap().byte_length() < HEADER_SIZE {
                break;
            }

            let header = match self.chunk_stream.lock().unwrap().peek(HEADER_SIZE) {
                Some(h) => h,
                None => break,
            };

            let msg_type = ProtocolMessageType::from_u8(header.read_u8(0));
            let id = header.read_u32_be(1);
            let ack = header.read_u32_be(5);
            let length = header.read_u32_be(9) as usize;

            let total_frame_length = HEADER_SIZE + length;
            if self.chunk_stream.lock().unwrap().byte_length() < total_frame_length {
                // 流式传输必须等整帧到齐后再消费 header，避免分片时把消息头吃掉。
                break;
            }

            self.chunk_stream.lock().unwrap().skip(HEADER_SIZE);

            let mut body = VSBuffer::alloc(0);
            if length > 0 {
                match self.chunk_stream.lock().unwrap().read(length) {
                    Some(read_body) => body = read_body,
                    None => panic!("PersistentProtocol 读取到完整帧长度后 body 不应为空"),
                }
            }

            // 处理对方的 ACK：清除已确认的发送队列
            self.process_ack(ack);

            match msg_type {
                ProtocolMessageType::Regular => {
                    *self.incoming_ack_id.lock().unwrap() = id;
                    self.message_emitter.fire(body);
                }
                ProtocolMessageType::Ack | ProtocolMessageType::KeepAlive => {
                    // 纯 ACK / 心跳，只更新活跃时间
                }
                ProtocolMessageType::Disconnect => {
                    self.close_emitter.fire(());
                }
                _ => {}
            }

            *self.last_ack_time.lock().unwrap() = Instant::now();
        }
    }

    /// 根据对方的 ACK 号清除已确认的消息。
    fn process_ack(&self, ack: u32) {
        {
            let mut queue = self.outgoing_unack.lock().unwrap();
            let mut bytes = self.unack_bytes.lock().unwrap();
            while let Some(front) = queue.first() {
                if front.msg.id <= ack {
                    let entry = queue.remove(0);
                    *bytes -= entry.msg.data.byte_length();
                } else {
                    break;
                }
            }
        }
        // 饱和后回落到低水位 → onDrained（边沿触发）。
        let bytes = *self.unack_bytes.lock().unwrap();
        if self.saturated.load(Ordering::SeqCst)
            && bytes <= self.options.saturation_low_water_mark_bytes
        {
            self.saturated.store(false, Ordering::SeqCst);
            self.drained_emitter.fire(());
        }
    }

    /// 重放缓冲越界（字节/宽限窗）：这条协议会话已不可能无损续传，
    /// 主动断开走 onClose。
    fn abandon_session(&self) {
        if self.overflowed.swap(true, Ordering::SeqCst) {
            return;
        }
        let incoming_ack = *self.incoming_ack_id.lock().unwrap();
        self.write_message(&ProtocolMessage::new(
            ProtocolMessageType::Disconnect,
            0,
            incoming_ack,
            VSBuffer::alloc(0),
        ));
        self.close_emitter.fire(());
    }

    /// 重连时替换底层 socket，并重放所有未被确认的消息。
    pub fn replace_socket(self: &Arc<Self>, new_socket: Arc<dyn Socket>) {
        self.disposables.lock().unwrap().dispose();
        *self.disposables.lock().unwrap() = DisposableStore::new();
        *self.chunk_stream.lock().unwrap() = ChunkStream::new();
        *self.socket.lock().unwrap() = new_socket;
        self.bind_socket();

        // 重放未确认的消息
        let queue = self.outgoing_unack.lock().unwrap();
        for entry in queue.iter() {
            self.write_message(&entry.msg);
        }
    }

    fn start_keep_alive(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        let stop = self.stop_flag.clone();
        let handle = std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(KEEP_ALIVE_INTERVAL);
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                if let Some(p) = weak.upgrade() {
                    let incoming_ack = *p.incoming_ack_id.lock().unwrap();
                    p.write_message(&ProtocolMessage::new(
                        ProtocolMessageType::KeepAlive,
                        0,
                        incoming_ack,
                        VSBuffer::alloc(0),
                    ));
                } else {
                    break;
                }
            }
        });
        *self.keep_alive_handle.lock().unwrap() = Some(handle);
    }

    fn start_ack_check(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        let stop = self.stop_flag.clone();
        let handle = std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(ACK_TIMEOUT);
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let Some(p) = weak.upgrade() else { break };

                let queue_len = p.outgoing_unack.lock().unwrap().len();
                let elapsed_since_ack = p.last_ack_time.lock().unwrap().elapsed();
                if queue_len > 0 && elapsed_since_ack > ACK_TIMEOUT {
                    p.socket_close_emitter.fire(());
                }

                // 最老未 ACK 消息超过宽限窗 → 放弃会话。
                let oldest_age = {
                    let queue = p.outgoing_unack.lock().unwrap();
                    queue.first().map(|e| e.queued_at.elapsed())
                };
                if let Some(age) = oldest_age {
                    if age > p.options.replay_buffer_grace {
                        p.abandon_session();
                    }
                }
            }
        });
        *self.ack_check_handle.lock().unwrap() = Some(handle);
    }
}

impl MessagePassingProtocol for PersistentProtocol {
    fn send(&self, buffer: VSBuffer) {
        let id = {
            let mut outgoing = self.outgoing_msg_id.lock().unwrap();
            *outgoing += 1;
            *outgoing
        };
        let incoming_ack = *self.incoming_ack_id.lock().unwrap();
        let msg = ProtocolMessage::new(ProtocolMessageType::Regular, id, incoming_ack, buffer.clone());

        self.outgoing_unack.lock().unwrap().push(UnackEntry {
            msg: msg.clone(),
            queued_at: Instant::now(),
        });
        *self.unack_bytes.lock().unwrap() += buffer.byte_length();

        self.write_message(&msg);

        let bytes = *self.unack_bytes.lock().unwrap();
        // 重放缓冲字节越界 → 立即放弃会话（同步判定，不等定时器）。
        if bytes > self.options.replay_buffer_max_bytes {
            self.abandon_session();
            return;
        }
        // 越过高水位进入饱和态（边沿触发，不重复通知）。
        if !self.saturated.load(Ordering::SeqCst)
            && bytes > self.options.saturation_high_water_mark_bytes
        {
            self.saturated.store(true, Ordering::SeqCst);
            self.saturated_emitter.fire(());
        }
    }

    fn on_message(&self) -> Event<VSBuffer> {
        self.message_emitter.event()
    }

    fn drain(&self) -> Promise<()> {
        self.socket.lock().unwrap().drain()
    }
}

impl ConnectionFlowControl for PersistentProtocol {
    fn unacknowledged_bytes(&self) -> usize {
        *self.unack_bytes.lock().unwrap()
    }
    fn on_saturated(&self) -> Event<()> {
        self.saturated_emitter.event()
    }
    fn on_drained(&self) -> Event<()> {
        self.drained_emitter.event()
    }
}

impl Disposable for PersistentProtocol {
    fn dispose(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Some(h) = self.keep_alive_handle.lock().unwrap().take() {
            let _ = h.join();
        }
        if let Some(h) = self.ack_check_handle.lock().unwrap().take() {
            let _ = h.join();
        }
        self.disposables.lock().unwrap().dispose();
        self.message_emitter.dispose();
        self.close_emitter.dispose();
        self.socket_close_emitter.dispose();
        self.saturated_emitter.dispose();
        self.drained_emitter.dispose();
        self.socket.lock().unwrap().dispose();
    }
}
