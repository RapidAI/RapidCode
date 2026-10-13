//! Layer 4: 连接管理 —— IPCServer 和 IPCClient
//!
//! Rust 移植自 `packages/rpc/src/ipc.ts`。
//!
//! `ChannelServer` / `ChannelClient` 是单连接的 RPC 实现。
//! `IPCServer` / `IPCClient` 在其上构建连接管理能力：
//!
//! - `IPCServer` (1:N): 一个服务端接受多个客户端连接，每个连接独立创建
//!   ChannelServer + ChannelClient，支持通过 Router 选择目标客户端进行调用。
//! - `IPCClient` (1:1 双向): 既是客户端又是服务端。
//!
//! 关键协议：客户端连接后发送的第一条消息是 ctx（上下文/客户端 ID），
//! 服务端据此识别客户端身份。

use crate::channel_client::ChannelClient;
use crate::channel_server::ChannelServer;
use crate::channels_shared::{Channel, ServerChannel};
use crate::delayed_channel::get_delayed_channel;
use crate::foundation::{
    CancellationToken, Disposable, DisposableStore, Event, EventMultiplexer, Emitter, NoopDisposable,
};
use crate::promise::Promise;
use crate::protocol::MessagePassingProtocol;
use crate::serialization::{deserialize, serialize, BufferReader, BufferWriter};
use crate::value::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

// ============================================================================
// Connection 相关接口
// ============================================================================

/// 客户端连接事件。
pub struct ClientConnectionEvent {
    pub protocol: Arc<dyn MessagePassingProtocol>,
    pub on_did_client_disconnect: Event<()>,
}

/// 一条活跃连接 = 客户端标识 + 双向 channel。
pub struct Connection {
    pub channel_server: Arc<ChannelServer>,
    pub channel_client: Arc<ChannelClient>,
    pub ctx: Value,
}

/// 连接中心——暴露所有活跃连接。
pub trait ConnectionHub: Send + Sync {
    fn connections(&self) -> Vec<Arc<Connection>>;
    fn on_did_add_connection(&self) -> Event<Arc<Connection>>;
    fn on_did_remove_connection(&self) -> Event<Arc<Connection>>;
}

/// 路由器——在多客户端场景中选择目标客户端。
pub trait ClientRouter: Send + Sync {
    fn route_call(
        &self,
        hub: &dyn ConnectionHub,
        command: &str,
        arg: Value,
        token: Arc<dyn CancellationToken>,
    ) -> Promise<Arc<Connection>>;
    fn route_event(&self, hub: &dyn ConnectionHub, event: &str, arg: Value) -> Promise<Arc<Connection>>;
}

/// 通道选择器：静态过滤函数或路由器。
pub enum ChannelSelector {
    Filter(Arc<dyn Fn(&Value) -> bool + Send + Sync>),
    Router(Arc<dyn ClientRouter>),
}

// ============================================================================
// IPCServer —— 一对多服务端
// ============================================================================

/// IPCServer 是整个通信架构中的「大脑」。
///
/// 它同时是：
/// - `ChannelServer`: 注册 channel 供客户端调用
/// - 路由客户端: 可以反向调用客户端的 channel（通过 Router / Filter 选择目标）
/// - `ConnectionHub`: 暴露所有活跃连接，支持连接增删事件
pub struct IPCServer {
    channels: Mutex<HashMap<String, Arc<dyn ServerChannel>>>,
    connections: Mutex<Vec<Arc<Connection>>>,
    add_emitter: Arc<Emitter<Arc<Connection>>>,
    remove_emitter: Arc<Emitter<Arc<Connection>>>,
    disposables: Mutex<DisposableStore>,
    disposed: AtomicBool,
    self_weak: OnceLock<Weak<IPCServer>>,
    default_timeout: Duration,
}

impl IPCServer {
    pub fn new(on_did_client_connect: Event<ClientConnectionEvent>) -> Arc<Self> {
        let server = Arc::new(IPCServer {
            channels: Mutex::new(HashMap::new()),
            connections: Mutex::new(Vec::new()),
            add_emitter: Arc::new(Emitter::new()),
            remove_emitter: Arc::new(Emitter::new()),
            disposables: Mutex::new(DisposableStore::new()),
            disposed: AtomicBool::new(false),
            self_weak: OnceLock::new(),
            default_timeout: Duration::from_millis(1000),
        });
        let _ = server.self_weak.set(Arc::downgrade(&server));

        let weak = Arc::downgrade(&server);
        let d = on_did_client_connect.subscribe(move |event: ClientConnectionEvent| {
            if let Some(server) = weak.upgrade() {
                server.on_client_connect(event);
            }
        });
        server.disposables.lock().unwrap().add_boxed(d);
        server
    }

    fn self_weak(&self) -> Weak<IPCServer> {
        self.self_weak.get().cloned().unwrap_or_else(Weak::new)
    }

    fn on_client_connect(&self, event: ClientConnectionEvent) {
        let protocol = event.protocol.clone();
        let weak = self.self_weak();

        // 等待客户端发来的第一条消息：ctx（客户端身份标识）
        let d = event.protocol.on_message().once().subscribe(move |msg| {
            let Some(server) = weak.upgrade() else { return };
            let reader = BufferReader::new(msg);
            let ctx = deserialize(&reader);

            let channel_server = ChannelServer::new(
                protocol.clone(),
                ctx.clone(),
                server.default_timeout,
                false,
            );
            let channel_client = ChannelClient::new(protocol.clone());

            // 把已注册的 channel 推送给新连接
            let channels: Vec<(String, Arc<dyn ServerChannel>)> = server
                .channels
                .lock()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for (name, channel) in channels {
                channel_server.register_channel(&name, channel);
            }

            let connection = Arc::new(Connection {
                channel_server,
                channel_client,
                ctx,
            });
            server.connections.lock().unwrap().push(connection.clone());
            server.add_emitter.fire(connection.clone());

            // 客户端断开时清理
            let weak_disconnect = server.self_weak();
            let d = event.on_did_client_disconnect.subscribe(move |_: ()| {
                let Some(server) = weak_disconnect.upgrade() else { return };
                connection.channel_server.dispose();
                connection.channel_client.dispose();
                server
                    .connections
                    .lock()
                    .unwrap()
                    .retain(|c| !Arc::ptr_eq(c, &connection));
                server.remove_emitter.fire(connection.clone());
            });
            server.disposables.lock().unwrap().add_boxed(d);
        });
        self.disposables.lock().unwrap().add_boxed(d);
    }

    /// 注册通道并推送给所有已连接客户端。
    pub fn register_channel(&self, channel_name: &str, channel: Arc<dyn ServerChannel>) {
        self.channels
            .lock()
            .unwrap()
            .insert(channel_name.to_string(), channel.clone());
        for connection in self.connections.lock().unwrap().iter() {
            connection.channel_server.register_channel(channel_name, channel.clone());
        }
    }

    /// 获取客户端的通道（反向调用）。
    pub fn get_channel(&self, channel_name: &str, selector: ChannelSelector) -> Arc<dyn Channel> {
        let weak = self.self_weak();
        let channel_name = channel_name.to_string();
        let selector = Arc::new(selector);

        Arc::new(IpcServerChannel {
            server: weak,
            channel_name,
            selector,
        })
    }

    fn find_or_wait_connection(
        &self,
        filter: Arc<dyn Fn(&Value) -> bool + Send + Sync>,
    ) -> Promise<Arc<Connection>> {
        let (resolver, promise) = Promise::<Arc<Connection>>::new();
        if let Some(matched) = self
            .connections
            .lock()
            .unwrap()
            .iter()
            .find(|c| filter(&c.ctx))
            .cloned()
        {
            resolver.resolve(matched);
            return promise;
        }
        let resolver_for_sub = resolver.clone();
        self.on_did_add_connection().subscribe(move |connection: Arc<Connection>| {
            if filter(&connection.ctx) {
                resolver_for_sub.resolve(connection.clone());
            }
        });
        promise
    }

    /// 聚合所有匹配客户端的同名事件为一个事件。
    fn get_multicast_event(
        &self,
        channel_name: String,
        filter: Arc<dyn Fn(&Value) -> bool + Send + Sync>,
        event_name: String,
        arg: Value,
    ) -> Event<Value> {
        let weak = self.self_weak();
        Event::new(move |listener| {
            let Some(server) = weak.upgrade() else {
                return Box::new(NoopDisposable);
            };
            let store = DisposableStore::new();
            let multiplexer = Arc::new(EventMultiplexer::<Value>::new());

            // 已连接客户端的事件
            for connection in server.connections() {
                if filter(&connection.ctx) {
                    let channel = connection.channel_client.get_channel(&channel_name);
                    let event = channel.listen(&event_name, arg.clone());
                    let _ = multiplexer.add(event);
                }
            }

            // 后续新连接的事件
            let mux_for_add = multiplexer.clone();
            let channel_for_add = channel_name.clone();
            let event_for_add = event_name.clone();
            let arg_for_add = arg.clone();
            let filter_for_add = filter.clone();
            let d = server
                .on_did_add_connection()
                .filter(move |c: &Arc<Connection>| filter_for_add(&c.ctx))
                .subscribe(move |connection: Arc<Connection>| {
                    let channel = connection.channel_client.get_channel(&channel_for_add);
                    let event = channel.listen(&event_for_add, arg_for_add.clone());
                    let _ = mux_for_add.add(event);
                });
            store.add_boxed(d);

            // 多路复用事件 → 外层 listener
            let d = multiplexer.event().subscribe_listener(listener.clone());
            store.add_boxed(d);

            // 保持 multiplexer 存活至 store 释放
            store.add_boxed(Box::new(crate::foundation::ArcDisposable(multiplexer)));

            Box::new(store)
        })
    }
}

impl ConnectionHub for IPCServer {
    fn connections(&self) -> Vec<Arc<Connection>> {
        self.connections.lock().unwrap().clone()
    }
    fn on_did_add_connection(&self) -> Event<Arc<Connection>> {
        self.add_emitter.event()
    }
    fn on_did_remove_connection(&self) -> Event<Arc<Connection>> {
        self.remove_emitter.event()
    }
}

impl Disposable for IPCServer {
    fn dispose(&self) {
        if self.disposed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.disposables.lock().unwrap().dispose();
        for connection in self.connections.lock().unwrap().iter() {
            connection.channel_client.dispose();
            connection.channel_server.dispose();
        }
        self.connections.lock().unwrap().clear();
        self.channels.lock().unwrap().clear();
        self.add_emitter.dispose();
        self.remove_emitter.dispose();
    }
}

/// IPCServer 反向通道代理。
struct IpcServerChannel {
    server: Weak<IPCServer>,
    channel_name: String,
    selector: Arc<ChannelSelector>,
}

impl Channel for IpcServerChannel {
    fn call(&self, command: &str, arg: Value, token: Arc<dyn CancellationToken>) -> Promise<Value> {
        let Some(server) = self.server.upgrade() else {
            return Promise::rejected(crate::error::RpcError::new("IPCServer disposed"));
        };
        let channel_name = self.channel_name.clone();
        let command = command.to_string();

        let connection_promise = match &*self.selector {
            ChannelSelector::Filter(filter) => server.find_or_wait_connection(filter.clone()),
            ChannelSelector::Router(router) => router.route_call(&*server, &command, arg.clone(), token.clone()),
        };

        let channel_promise = connection_promise
            .map(move |connection: Arc<Connection>| connection.channel_client.get_channel(&channel_name));
        get_delayed_channel(channel_promise).call(&command, arg, token)
    }

    fn listen(&self, event: &str, arg: Value) -> Event<Value> {
        let Some(server) = self.server.upgrade() else {
            return Event::none();
        };
        let channel_name = self.channel_name.clone();
        let event_name = event.to_string();

        match &*self.selector {
            ChannelSelector::Filter(filter) => server.get_multicast_event(
                channel_name,
                filter.clone(),
                event_name,
                arg,
            ),
            ChannelSelector::Router(router) => {
                let channel_promise = router
                    .route_event(&*server, &event_name, arg.clone())
                    .map(move |connection: Arc<Connection>| {
                        connection.channel_client.get_channel(&channel_name)
                    });
                get_delayed_channel(channel_promise).listen(&event_name, arg)
            }
        }
    }
}

// ============================================================================
// IPCClient —— 一对一双向
// ============================================================================

/// IPCClient 是双向的：
/// - 可以调远端的 channel (`ChannelClient`)
/// - 也可以注册自己的 channel 供远端调用 (`ChannelServer`)
///
/// 第一条消息发送 ctx（自己的身份标识），这样服务端能识别你是谁。
pub struct IPCClient {
    channel_client: Arc<ChannelClient>,
    channel_server: Arc<ChannelServer>,
}

impl IPCClient {
    pub fn new(protocol: Arc<dyn MessagePassingProtocol>, ctx: Value) -> Arc<Self> {
        // 第一条消息：发送自己的身份标识
        let writer = BufferWriter::new();
        serialize(&writer, &ctx);
        protocol.send(writer.buffer());

        let channel_client = ChannelClient::new(protocol.clone());
        let channel_server = ChannelServer::new(protocol, ctx, Duration::from_millis(1000), false);
        Arc::new(IPCClient {
            channel_client,
            channel_server,
        })
    }

    pub fn get_channel(&self, channel_name: &str) -> Arc<dyn Channel> {
        self.channel_client.get_channel(channel_name)
    }

    pub fn register_channel(&self, channel_name: &str, channel: Arc<dyn ServerChannel>) {
        self.channel_server.register_channel(channel_name, channel);
    }
}

impl Disposable for IPCClient {
    fn dispose(&self) {
        self.channel_client.dispose();
        self.channel_server.dispose();
    }
}

// ============================================================================
// StaticRouter —— 简单路由器
// ============================================================================

/// 根据静态条件选择客户端的路由器。
///
/// 例: `StaticRouter::new(|ctx| ctx.as_str() == Some("main-window"))`
pub struct StaticRouter {
    predicate: Arc<dyn Fn(&Value) -> bool + Send + Sync>,
}

impl StaticRouter {
    pub fn new<F>(predicate: F) -> Self
    where
        F: Fn(&Value) -> bool + Send + Sync + 'static,
    {
        StaticRouter {
            predicate: Arc::new(predicate),
        }
    }

    fn route(&self, hub: &dyn ConnectionHub) -> Promise<Arc<Connection>> {
        let (resolver, promise) = Promise::<Arc<Connection>>::new();
        for connection in hub.connections() {
            if (self.predicate)(&connection.ctx) {
                resolver.resolve(connection);
                return promise;
            }
        }
        // 等待新连接到来
        let predicate = self.predicate.clone();
        hub.on_did_add_connection().subscribe(move |connection: Arc<Connection>| {
            if predicate(&connection.ctx) {
                resolver.resolve(connection.clone());
            }
        });
        promise
    }
}

impl ClientRouter for StaticRouter {
    fn route_call(
        &self,
        hub: &dyn ConnectionHub,
        _command: &str,
        _arg: Value,
        _token: Arc<dyn CancellationToken>,
    ) -> Promise<Arc<Connection>> {
        self.route(hub)
    }

    fn route_event(
        &self,
        hub: &dyn ConnectionHub,
        _event: &str,
        _arg: Value,
    ) -> Promise<Arc<Connection>> {
        self.route(hub)
    }
}
