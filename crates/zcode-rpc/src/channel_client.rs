//! ChannelClient —— 单连接 RPC 客户端
//!
//! Rust 移植自 `packages/rpc/src/channelClient.ts`。

use crate::buffer::VSBuffer;
use crate::channels_shared::{Channel, ChannelClientApi, Handler, RawResponse, RequestType, ResponseType};
use crate::error::RpcError;
use crate::foundation::{
    disposable_slot, CancellationToken, Disposable, DisposableSlot, Event, Emitter, SlotDisposable,
};
use crate::promise::{Promise, PromiseResolver};
use crate::protocol::MessagePassingProtocol;
use crate::serialization::{deserialize, serialize, BufferReader, BufferWriter};
use crate::value::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Uninitialized,
    Idle,
}

/// 单连接 RPC 客户端。
pub struct ChannelClient {
    protocol: Arc<dyn MessagePassingProtocol>,
    state: Mutex<State>,
    is_disposed: AtomicBool,
    active_requests: Mutex<Vec<Box<dyn Disposable>>>,
    handlers: Mutex<HashMap<u32, Handler>>,
    /// Promise 请求的 reject 器；只有 Promise 请求需要在连接终结时 reject。
    pending_rejections: Mutex<HashMap<u32, PromiseResolver<Value>>>,
    last_request_id: AtomicU32,
    protocol_listener: Mutex<Option<Box<dyn Disposable>>>,
    did_initialize_emitter: Arc<Emitter<()>>,
    self_weak: OnceLock<Weak<ChannelClient>>,
}

impl ChannelClient {
    pub fn new(protocol: Arc<dyn MessagePassingProtocol>) -> Arc<Self> {
        let client = Arc::new(ChannelClient {
            protocol: protocol.clone(),
            state: Mutex::new(State::Uninitialized),
            is_disposed: AtomicBool::new(false),
            active_requests: Mutex::new(Vec::new()),
            handlers: Mutex::new(HashMap::new()),
            pending_rejections: Mutex::new(HashMap::new()),
            last_request_id: AtomicU32::new(0),
            protocol_listener: Mutex::new(None),
            did_initialize_emitter: Arc::new(Emitter::new()),
            self_weak: OnceLock::new(),
        });
        let _ = client.self_weak.set(Arc::downgrade(&client));

        let weak = Arc::downgrade(&client);
        let listener = protocol.on_message().subscribe(move |msg: VSBuffer| {
            if let Some(c) = weak.upgrade() {
                c.on_buffer(msg);
            }
        });
        *client.protocol_listener.lock().unwrap() = Some(listener);
        client
    }

    fn self_weak(&self) -> Weak<ChannelClient> {
        self.self_weak.get().cloned().unwrap_or_else(Weak::new)
    }

    /// 订阅初始化完成。
    pub fn on_did_initialize(&self) -> Event<()> {
        self.did_initialize_emitter.event()
    }

    /// 获取一个通道代理。
    pub fn get_channel(&self, channel_name: &str) -> Arc<dyn Channel> {
        Arc::new(ClientChannel {
            client: self.self_weak(),
            channel_name: channel_name.to_string(),
        })
    }

    fn request_promise(
        &self,
        channel_name: &str,
        name: &str,
        arg: Value,
        token: Arc<dyn CancellationToken>,
    ) -> Promise<Value> {
        let id = self.last_request_id.fetch_add(1, Ordering::SeqCst);

        if token.is_cancellation_requested() {
            return Promise::rejected(RpcError::new("Cancelled"));
        }

        let (resolver, promise) = Promise::<Value>::new();
        self.pending_rejections.lock().unwrap().insert(id, resolver.clone());

        let weak = self.self_weak();
        let channel_name = channel_name.to_string();
        let name = name.to_string();

        // 取消订阅句柄放进共享槽，settle 时统一释放。
        let cancel_slot: DisposableSlot = disposable_slot();

        let do_request = {
            let weak = weak.clone();
            let channel_name = channel_name.clone();
            let name = name.clone();
            let arg = arg.clone();
            let resolver = resolver.clone();
            let cancel_slot = cancel_slot.clone();
            move || {
                let Some(client) = weak.upgrade() else { return };
                // dispose/cancel 可能发生在 Initialize 之前；此时不能再把已经
                // rejected 的请求发送到新连接或已终结的传输上。
                if client.is_disposed.load(Ordering::SeqCst)
                    || !client.pending_rejections.lock().unwrap().contains_key(&id)
                {
                    return;
                }

                let weak_for_handler = weak.clone();
                let handler: Handler = Arc::new(move |response: &RawResponse| {
                    let Some(client) = weak_for_handler.upgrade() else { return };
                    match response {
                        RawResponse::PromiseSuccess { data, .. } => {
                            client.handlers.lock().unwrap().remove(&id);
                            client.pending_rejections.lock().unwrap().remove(&id);
                            if let Some(d) = cancel_slot.lock().unwrap().take() {
                                d.dispose();
                            }
                            resolver.resolve(data.clone());
                        }
                        RawResponse::PromiseError { data, .. } => {
                            client.handlers.lock().unwrap().remove(&id);
                            client.pending_rejections.lock().unwrap().remove(&id);
                            if let Some(d) = cancel_slot.lock().unwrap().take() {
                                d.dispose();
                            }
                            resolver.reject(data.clone());
                        }
                        RawResponse::PromiseErrorObj { data, .. } => {
                            client.handlers.lock().unwrap().remove(&id);
                            client.pending_rejections.lock().unwrap().remove(&id);
                            if let Some(d) = cancel_slot.lock().unwrap().take() {
                                d.dispose();
                            }
                            resolver.reject(RpcError::from_value(data));
                        }
                        _ => {}
                    }
                });

                client.handlers.lock().unwrap().insert(id, handler);
                client.send_request(RequestType::Promise, id, &channel_name, &name, arg.clone());
            }
        };

        if *self.state.lock().unwrap() == State::Idle {
            do_request();
        } else {
            let do_request = do_request.clone();
            self.when_initialized().on_settled(move |_| do_request());
        }

        // 订阅取消。
        let weak_cancel = weak.clone();
        let resolver_cancel = resolver.clone();
        let cancel_slot_for_sub = cancel_slot.clone();
        let cancel_disposable = token.on_cancellation_requested().subscribe(move |_: ()| {
            let Some(client) = weak_cancel.upgrade() else { return };
            if !client.pending_rejections.lock().unwrap().contains_key(&id) {
                return;
            }
            client.send_cancel_or_dispose(RequestType::PromiseCancel, id);
            client.handlers.lock().unwrap().remove(&id);
            client.pending_rejections.lock().unwrap().remove(&id);
            resolver_cancel.reject(RpcError::new("Cancelled"));
            let _ = &cancel_slot_for_sub;
        });
        *cancel_slot.lock().unwrap() = Some(cancel_disposable);

        self.active_requests
            .lock()
            .unwrap()
            .push(Box::new(SlotDisposable(cancel_slot)));

        promise
    }

    fn request_event(&self, channel_name: &str, name: &str, arg: Value) -> Event<Value> {
        let id = self.last_request_id.fetch_add(1, Ordering::SeqCst);
        let weak = self.self_weak();
        let channel_name = channel_name.to_string();
        let name = name.to_string();

        let weak_first = weak.clone();
        let channel_for_first = channel_name.clone();
        let name_for_first = name.clone();
        let arg_for_first = arg.clone();
        let weak_last = weak.clone();

        let emitter = Arc::new(Emitter::<Value>::with_options(
            Some(Box::new(move || {
                let Some(client) = weak_first.upgrade() else { return };
                let do_request = {
                    let weak = weak_first.clone();
                    let channel_name = channel_for_first.clone();
                    let name = name_for_first.clone();
                    let arg = arg_for_first.clone();
                    move || {
                        let Some(client) = weak.upgrade() else { return };
                        client.send_request(RequestType::EventListen, id, &channel_name, &name, arg.clone());
                    }
                };
                if *client.state.lock().unwrap() == State::Idle {
                    do_request();
                } else {
                    client.when_initialized().on_settled(move |_| do_request());
                }
            })),
            Some(Box::new(move || {
                let Some(client) = weak_last.upgrade() else { return };
                client.send_cancel_or_dispose(RequestType::EventDispose, id);
                client.handlers.lock().unwrap().remove(&id);
            })),
        ));

        let emitter_for_handler = emitter.clone();
        self.handlers.lock().unwrap().insert(
            id,
            Arc::new(move |response: &RawResponse| {
                if let RawResponse::EventFire { data, .. } = response {
                    emitter_for_handler.fire(data.clone());
                }
            }),
        );

        emitter.event()
    }

    fn send_request(&self, request_type: RequestType, id: u32, channel_name: &str, name: &str, arg: Value) {
        let writer = BufferWriter::new();
        serialize(
            &writer,
            &Value::Array(vec![
                Value::Int(request_type as i64),
                Value::Int(id as i64),
                Value::String(channel_name.to_string()),
                Value::String(name.to_string()),
            ]),
        );
        serialize(&writer, &arg);
        self.protocol.send(writer.buffer());
    }

    fn send_cancel_or_dispose(&self, request_type: RequestType, id: u32) {
        let writer = BufferWriter::new();
        serialize(
            &writer,
            &Value::Array(vec![
                Value::Int(request_type as i64),
                Value::Int(id as i64),
            ]),
        );
        serialize(&writer, &Value::Undefined);
        self.protocol.send(writer.buffer());
    }

    fn on_buffer(&self, message: VSBuffer) {
        let reader = BufferReader::new(message);
        let header = deserialize(&reader);
        let body = deserialize(&reader);
        let arr = header.as_array();
        let type_value = arr.and_then(|a| a.first()).and_then(|v| v.as_int()).unwrap_or(-1);
        let Some(response_type) = ResponseType::from_i64(type_value) else {
            return;
        };

        match response_type {
            ResponseType::Initialize => self.on_response(RawResponse::Initialize),
            ResponseType::PromiseSuccess => {
                let id = arr.and_then(|a| a.get(1)).and_then(|v| v.as_int()).unwrap_or(0) as u32;
                self.on_response(RawResponse::PromiseSuccess { id, data: body });
            }
            ResponseType::PromiseError => {
                let id = arr.and_then(|a| a.get(1)).and_then(|v| v.as_int()).unwrap_or(0) as u32;
                self.on_response(RawResponse::PromiseError {
                    id,
                    data: RpcError::from_value(&body),
                });
            }
            ResponseType::PromiseErrorObj => {
                let id = arr.and_then(|a| a.get(1)).and_then(|v| v.as_int()).unwrap_or(0) as u32;
                self.on_response(RawResponse::PromiseErrorObj { id, data: body });
            }
            ResponseType::EventFire => {
                let id = arr.and_then(|a| a.get(1)).and_then(|v| v.as_int()).unwrap_or(0) as u32;
                self.on_response(RawResponse::EventFire { id, data: body });
            }
        }
    }

    fn on_response(&self, response: RawResponse) {
        if matches!(response, RawResponse::Initialize) {
            *self.state.lock().unwrap() = State::Idle;
            self.did_initialize_emitter.fire(());
            return;
        }
        if let Some(id) = response.id() {
            let handler = self.handlers.lock().unwrap().get(&id).cloned();
            if let Some(handler) = handler {
                handler(&response);
            }
        }
    }

    fn when_initialized(&self) -> Promise<()> {
        if *self.state.lock().unwrap() == State::Idle {
            return Promise::resolved(());
        }
        let (resolver, promise) = Promise::<()>::new();
        let resolver_sub = resolver.clone();
        self.did_initialize_emitter.event().once().subscribe(move |_: ()| {
            resolver_sub.resolve(());
        });
        // 订阅后重新检查状态，避免「检查 → 订阅」之间 Initialize 已到达而错过信号。
        if *self.state.lock().unwrap() == State::Idle {
            resolver.resolve(());
        }
        promise
    }

    /// 关闭客户端。`reason` 非空时作为 reject 原因，否则用默认原因。
    pub fn dispose_with_reason(&self, reason: Option<RpcError>) {
        if self.is_disposed.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(d) = self.protocol_listener.lock().unwrap().take() {
            d.dispose();
        }

        let rejection = reason.unwrap_or_else(|| RpcError::new("ChannelClient disposed").with_name("ConnectionClosed"));

        // 传输已终结时，所有已发出以及排队等待 Initialize 的 Promise 请求都必须
        // fail-closed，否则上层的 in-flight 去重 Promise 会永久占用 workspace key。
        let pending = std::mem::take(&mut *self.pending_rejections.lock().unwrap());
        for (id, resolver) in pending {
            self.handlers.lock().unwrap().remove(&id);
            resolver.reject(rejection.clone());
        }
        let active = std::mem::take(&mut *self.active_requests.lock().unwrap());
        for d in active {
            d.dispose();
        }
        self.did_initialize_emitter.dispose();
    }
}

impl Disposable for ChannelClient {
    fn dispose(&self) {
        self.dispose_with_reason(None);
    }
}

impl ChannelClientApi for ChannelClient {
    fn get_channel(&self, channel_name: &str) -> Arc<dyn Channel> {
        ChannelClient::get_channel(self, channel_name)
    }
}

/// 通道代理：把 `call` / `listen` 转发到客户端。
struct ClientChannel {
    client: Weak<ChannelClient>,
    channel_name: String,
}

impl Channel for ClientChannel {
    fn call(&self, command: &str, arg: Value, token: Arc<dyn CancellationToken>) -> Promise<Value> {
        let Some(client) = self.client.upgrade() else {
            return Promise::rejected(RpcError::new("ChannelClient is disposed"));
        };
        if client.is_disposed.load(Ordering::SeqCst) {
            return Promise::rejected(RpcError::new("ChannelClient is disposed"));
        }
        client.request_promise(&self.channel_name, command, arg, token)
    }

    fn listen(&self, event: &str, arg: Value) -> Event<Value> {
        let Some(client) = self.client.upgrade() else {
            return Event::none();
        };
        if client.is_disposed.load(Ordering::SeqCst) {
            return Event::none();
        }
        client.request_event(&self.channel_name, event, arg)
    }
}
