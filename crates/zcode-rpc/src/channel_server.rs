//! ChannelServer —— 单连接 RPC 服务端
//!
//! Rust 移植自 `packages/rpc/src/channelServer.ts`。
//!
//! 与 TS 不同，Rust 侧用 `Weak<Self>` 安全地在异步回调/定时线程中回引服务端，
//! 避免 `Arc` 循环引用。

use crate::buffer::VSBuffer;
use crate::channels_shared::{RawResponse, RequestType, ResponseType, ServerChannel};
use crate::error::RpcError;
use crate::foundation::{CancellationTokenSource, Disposable, DisposableHandle};
use crate::protocol::MessagePassingProtocol;
use crate::serialization::{deserialize, serialize, BufferReader, BufferWriter};
use crate::value::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

/// 未找到通道时缓存的请求。
struct PendingRequest {
    request_type: RequestType,
    id: u32,
    channel_name: String,
    name: String,
    arg: Value,
    cancelled: Arc<AtomicBool>,
}

/// 单连接 RPC 服务端。
pub struct ChannelServer {
    protocol: Arc<dyn MessagePassingProtocol>,
    ctx: Value,
    timeout_delay: Duration,
    channels: Mutex<HashMap<String, Arc<dyn ServerChannel>>>,
    active_requests: Mutex<HashMap<u32, Box<dyn Disposable>>>,
    pending_requests: Mutex<HashMap<String, Vec<PendingRequest>>>,
    protocol_listener: Mutex<Option<Box<dyn Disposable>>>,
    disposed: AtomicBool,
    self_weak: OnceLock<Weak<ChannelServer>>,
}

impl ChannelServer {
    /// 创建服务端。`defer_init` 为 true 时不立即发送 Initialize。
    pub fn new(
        protocol: Arc<dyn MessagePassingProtocol>,
        ctx: Value,
        timeout_delay: Duration,
        defer_init: bool,
    ) -> Arc<Self> {
        let server = Arc::new(ChannelServer {
            protocol: protocol.clone(),
            ctx,
            timeout_delay,
            channels: Mutex::new(HashMap::new()),
            active_requests: Mutex::new(HashMap::new()),
            pending_requests: Mutex::new(HashMap::new()),
            protocol_listener: Mutex::new(None),
            disposed: AtomicBool::new(false),
            self_weak: OnceLock::new(),
        });
        let _ = server.self_weak.set(Arc::downgrade(&server));

        let weak = Arc::downgrade(&server);
        let listener = protocol.on_message().subscribe(move |msg: VSBuffer| {
            if let Some(s) = weak.upgrade() {
                s.on_raw_message(msg);
            }
        });
        *server.protocol_listener.lock().unwrap() = Some(listener);

        if !defer_init {
            server.send_response(RawResponse::Initialize);
        }
        server
    }

    fn self_weak(&self) -> Weak<ChannelServer> {
        self.self_weak.get().cloned().unwrap_or_else(Weak::new)
    }

    /// 发送 Initialize（对应 TS 的 `ready()`）。
    pub fn ready(&self) {
        self.send_response(RawResponse::Initialize);
    }

    /// 注册通道，并异步 flush 等待该通道的挂起请求。
    pub fn register_channel(&self, channel_name: &str, channel: Arc<dyn ServerChannel>) {
        self.channels
            .lock()
            .unwrap()
            .insert(channel_name.to_string(), channel);
        let weak = self.self_weak();
        let name = channel_name.to_string();
        std::thread::spawn(move || {
            if let Some(server) = weak.upgrade() {
                server.flush_pending_requests(&name);
            }
        });
    }

    fn send_response(&self, response: RawResponse) {
        let header = |response_type: ResponseType, id: Option<u32>| {
            let mut arr = vec![Value::Int(response_type as i64)];
            if let Some(id) = id {
                arr.push(Value::Int(id as i64));
            }
            Value::Array(arr)
        };
        match response {
            RawResponse::Initialize => self.send(header(ResponseType::Initialize, None), Value::Undefined),
            RawResponse::PromiseSuccess { id, data } => {
                self.send(header(ResponseType::PromiseSuccess, Some(id)), data)
            }
            RawResponse::PromiseError { id, data } => {
                self.send(header(ResponseType::PromiseError, Some(id)), data.to_value())
            }
            RawResponse::PromiseErrorObj { id, data } => {
                self.send(header(ResponseType::PromiseErrorObj, Some(id)), data)
            }
            RawResponse::EventFire { id, data } => {
                self.send(header(ResponseType::EventFire, Some(id)), data)
            }
        }
    }

    fn send(&self, header: Value, body: Value) {
        let writer = BufferWriter::new();
        serialize(&writer, &header);
        serialize(&writer, &body);
        // 传输失败静默忽略（对应 TS 的 try/catch noop）。
        self.protocol.send(writer.buffer());
    }

    fn on_raw_message(&self, message: VSBuffer) {
        let reader = BufferReader::new(message);
        let header = deserialize(&reader);
        let body = deserialize(&reader);

        let arr = header.as_array();
        let type_value = arr.and_then(|a| a.first()).and_then(|v| v.as_int()).unwrap_or(-1);
        let Some(request_type) = RequestType::from_i64(type_value) else {
            return;
        };
        let id = arr.and_then(|a| a.get(1)).and_then(|v| v.as_int()).unwrap_or(0) as u32;
        let channel_name = arr
            .and_then(|a| a.get(2))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let name = arr
            .and_then(|a| a.get(3))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        match request_type {
            RequestType::Promise => self.on_promise(id, channel_name, name, body),
            RequestType::EventListen => self.on_event_listen(id, channel_name, name, body),
            RequestType::PromiseCancel | RequestType::EventDispose => self.dispose_active_request(id),
        }
    }

    fn on_promise(&self, id: u32, channel_name: String, name: String, arg: Value) {
        let channel = self.channels.lock().unwrap().get(&channel_name).cloned();
        let Some(channel) = channel else {
            self.collect_pending_request(RequestType::Promise, id, channel_name, name, arg);
            return;
        };

        let cts = Arc::new(CancellationTokenSource::new());
        let promise = channel.call(&self.ctx, &name, arg, cts.token());

        let cancel_disposable: Box<dyn Disposable> = Box::new(DisposableHandle::new({
            let cts = cts.clone();
            move || cts.cancel()
        }));
        self.active_requests.lock().unwrap().insert(id, cancel_disposable);

        let weak = self.self_weak();
        promise.on_settled(move |result| {
            let Some(server) = weak.upgrade() else { return };
            match result {
                Ok(data) => server.send_response(RawResponse::PromiseSuccess { id, data }),
                Err(err) => server.send_response(RawResponse::PromiseError { id, data: err }),
            }
            // finally：释放取消句柄并移除活跃请求。
            let removed = server.active_requests.lock().unwrap().remove(&id);
            if let Some(d) = removed {
                d.dispose();
            }
        });
    }

    fn on_event_listen(&self, id: u32, channel_name: String, name: String, arg: Value) {
        let channel = self.channels.lock().unwrap().get(&channel_name).cloned();
        let Some(channel) = channel else {
            self.collect_pending_request(RequestType::EventListen, id, channel_name, name, arg);
            return;
        };

        let weak = self.self_weak();
        let disposable = channel.listen(&self.ctx, &name, arg).subscribe(move |data: Value| {
            if let Some(server) = weak.upgrade() {
                server.send_response(RawResponse::EventFire { id, data });
            }
        });
        self.active_requests.lock().unwrap().insert(id, disposable);
    }

    fn dispose_active_request(&self, id: u32) {
        if let Some(d) = self.active_requests.lock().unwrap().remove(&id) {
            d.dispose();
        }
    }

    fn collect_pending_request(
        &self,
        request_type: RequestType,
        id: u32,
        channel_name: String,
        name: String,
        arg: Value,
    ) {
        let cancelled = Arc::new(AtomicBool::new(false));
        self.pending_requests
            .lock()
            .unwrap()
            .entry(channel_name.clone())
            .or_default()
            .push(PendingRequest {
                request_type,
                id,
                channel_name: channel_name.clone(),
                name,
                arg,
                cancelled: cancelled.clone(),
            });

        // 超时后报错（对应 TS 的 setTimeout(timeoutDelay)）。
        let weak = self.self_weak();
        let timeout = self.timeout_delay;
        std::thread::spawn(move || {
            std::thread::sleep(timeout);
            if cancelled.load(Ordering::SeqCst) {
                return;
            }
            let Some(server) = weak.upgrade() else { return };
            if server.disposed.load(Ordering::SeqCst) {
                return;
            }
            eprintln!("Unknown channel: {channel_name}");
            if request_type != RequestType::Promise {
                return;
            }
            let err = RpcError::new(format!(
                "Channel name '{channel_name}' timed out after {}ms",
                timeout.as_millis()
            ))
            .with_name("Unknown channel");
            server.send_response(RawResponse::PromiseError { id, data: err });
        });
    }

    fn flush_pending_requests(&self, channel_name: &str) {
        let requests = self.pending_requests.lock().unwrap().remove(channel_name);
        let Some(requests) = requests else {
            return;
        };
        for req in requests {
            req.cancelled.store(true, Ordering::SeqCst);
            match req.request_type {
                RequestType::Promise => self.on_promise(req.id, req.channel_name, req.name, req.arg),
                RequestType::EventListen => {
                    self.on_event_listen(req.id, req.channel_name, req.name, req.arg)
                }
                _ => {}
            }
        }
    }
}

impl Disposable for ChannelServer {
    fn dispose(&self) {
        if self.disposed.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(d) = self.protocol_listener.lock().unwrap().take() {
            d.dispose();
        }
        let active = std::mem::take(&mut *self.active_requests.lock().unwrap());
        for (_, d) in active {
            d.dispose();
        }
    }
}
