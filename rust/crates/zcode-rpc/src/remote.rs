//! Layer 6: Remote 远程连接抽象
//!
//! Rust 移植自 `packages/rpc/src/remote.ts`。
//!
//! 核心思路：
//! 1. `RemoteAuthority`: `"ssh+myserver"` 这样的字符串标识远程环境
//! 2. `RemoteAuthorityResolver`: 由扩展注册，把 authority 解析为实际连接地址
//! 3. `SocketFactory`: 根据连接类型创建不同的 socket
//! 4. `URITransformer`: 在客户端和服务端之间转换文件路径
//!
//! 连接建立流程：
//! ```text
//!   authority "ssh+myserver"
//!       ↓ (RemoteAuthorityResolver)
//!   { connectTo, connectionToken }
//!       ↓ (SocketFactory)
//!   ISocket (TCP/WebSocket)
//!       ↓ (PersistentProtocol)
//!   MessagePassingProtocol (带 ACK + 重连)
//!       ↓ (IPCClient)
//!   可以调用远端的 channel 了！
//! ```

use crate::foundation::{to_disposable, Disposable, Event, Emitter};
use crate::ipc::IPCClient;
use crate::persistent_protocol::{PersistentProtocol, PersistentProtocolOptions};
use crate::promise::Promise;
use crate::protocol::Socket;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

// ============================================================================
// Remote Authority
// ============================================================================

/// 远程连接类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RemoteConnectionType {
    WebSocket = 0,
    Managed = 1,
}

/// 远程连接描述。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteConnection {
    /// WebSocket 连接：直接连到 host:port
    WebSocket { host: String, port: u16 },
    /// 托管连接：通过 ID 引用已建立的连接（如 tunnel）
    Managed { id: u64 },
}

impl RemoteConnection {
    pub fn connection_type(&self) -> RemoteConnectionType {
        match self {
            RemoteConnection::WebSocket { .. } => RemoteConnectionType::WebSocket,
            RemoteConnection::Managed { .. } => RemoteConnectionType::Managed,
        }
    }
}

impl std::fmt::Display for RemoteConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoteConnection::WebSocket { host, port } => write!(f, "WebSocket({host}:{port})"),
            RemoteConnection::Managed { id } => write!(f, "Managed({id})"),
        }
    }
}

/// 解析后的远程地址。
#[derive(Clone, Debug)]
pub struct ResolvedAuthority {
    pub authority: String,
    pub connect_to: RemoteConnection,
    pub connection_token: Option<String>,
}

// ============================================================================
// Remote Authority Resolver
// ============================================================================

/// 负责把 authority 字符串解析为实际连接地址。
pub trait RemoteAuthorityResolver: Send + Sync {
    fn resolve(&self, authority: &str) -> Promise<ResolvedAuthority>;
}

/// 管理多个 resolver。
#[derive(Default)]
pub struct RemoteAuthorityResolverService {
    resolvers: Arc<Mutex<HashMap<String, Arc<dyn RemoteAuthorityResolver>>>>,
}

impl RemoteAuthorityResolverService {
    pub fn new() -> Self {
        RemoteAuthorityResolverService::default()
    }

    /// 注册一个 authority 类型的 resolver，返回可注销句柄。
    pub fn register_resolver(
        &self,
        resolver_type: &str,
        resolver: Arc<dyn RemoteAuthorityResolver>,
    ) -> Box<dyn Disposable> {
        self.resolvers
            .lock()
            .unwrap()
            .insert(resolver_type.to_string(), resolver);
        let resolvers = self.resolvers.clone();
        let resolver_type = resolver_type.to_string();
        to_disposable(move || {
            resolvers.lock().unwrap().remove(&resolver_type);
        })
    }

    /// 解析 authority（如 `"ssh+myserver"`, `"wsl+Ubuntu"`）。
    pub fn resolve_authority(&self, authority: &str) -> Promise<ResolvedAuthority> {
        let resolver_type = match authority.find('+') {
            Some(idx) => &authority[..idx],
            None => authority,
        };
        let resolver = self.resolvers.lock().unwrap().get(resolver_type).cloned();
        match resolver {
            Some(r) => r.resolve(authority),
            None => Promise::rejected(crate::error::RpcError::new(format!(
                "No resolver registered for remote type: {resolver_type}"
            ))),
        }
    }
}

// ============================================================================
// Remote Socket Factory
// ============================================================================

/// 创建特定类型的 socket 连接。
pub trait SocketFactory: Send + Sync {
    fn supports(&self, connect_to: &RemoteConnection) -> bool;
    fn connect(&self, connect_to: &RemoteConnection, path: &str, query: &str)
        -> Promise<Arc<dyn Socket>>;
}

/// 管理不同连接类型的 socket 工厂（策略模式）。
#[derive(Default)]
pub struct RemoteSocketFactoryService {
    factories: Mutex<HashMap<RemoteConnectionType, Vec<Arc<dyn SocketFactory>>>>,
}

impl RemoteSocketFactoryService {
    pub fn new() -> Self {
        RemoteSocketFactoryService::default()
    }

    /// 注册一个工厂。
    pub fn register(&self, factory_type: RemoteConnectionType, factory: Arc<dyn SocketFactory>) {
        self.factories
            .lock()
            .unwrap()
            .entry(factory_type)
            .or_default()
            .push(factory);
    }

    /// 按 supports 选择工厂并建立连接。
    pub fn connect(
        &self,
        connect_to: &RemoteConnection,
        path: &str,
        query: &str,
    ) -> Promise<Arc<dyn Socket>> {
        let factories = self.factories.lock().unwrap();
        let list = factories.get(&connect_to.connection_type());
        let factory = list.and_then(|l| l.iter().find(|f| f.supports(connect_to)).cloned());
        drop(factories);
        match factory {
            Some(f) => f.connect(connect_to, path, query),
            None => Promise::rejected(crate::error::RpcError::new(format!(
                "No socket factory found for {connect_to}"
            ))),
        }
    }
}

// ============================================================================
// URI Transformer
// ============================================================================

/// 简化的 URI 表示。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimpleUri {
    pub scheme: String,
    pub authority: String,
    pub path: String,
}

/// URI 转换器——在客户端和服务端之间转换文件路径。
pub trait UriTransformer: Send + Sync {
    /// 客户端 URI → 服务端 URI
    fn transform_incoming(&self, uri: SimpleUri) -> SimpleUri;
    /// 服务端 URI → 客户端 URI
    fn transform_outgoing(&self, uri: SimpleUri) -> SimpleUri;
}

/// 创建一个 URI 转换器。
pub fn create_uri_transformer(remote_authority: &str) -> Arc<dyn UriTransformer> {
    Arc::new(SimpleUriTransformer {
        remote_authority: remote_authority.to_string(),
    })
}

struct SimpleUriTransformer {
    remote_authority: String,
}

impl UriTransformer for SimpleUriTransformer {
    fn transform_incoming(&self, uri: SimpleUri) -> SimpleUri {
        // vscode-remote://authority/path → file:///path
        if uri.scheme == "vscode-remote" && uri.authority == self.remote_authority {
            return SimpleUri {
                scheme: "file".into(),
                authority: String::new(),
                path: uri.path,
            };
        }
        // file:///local → vscode-local:///local
        if uri.scheme == "file" {
            return SimpleUri {
                scheme: "vscode-local".into(),
                authority: String::new(),
                path: uri.path,
            };
        }
        uri
    }

    fn transform_outgoing(&self, uri: SimpleUri) -> SimpleUri {
        // file:///path → vscode-remote://authority/path
        if uri.scheme == "file" {
            return SimpleUri {
                scheme: "vscode-remote".into(),
                authority: self.remote_authority.clone(),
                path: uri.path,
            };
        }
        // vscode-local:///local → file:///local
        if uri.scheme == "vscode-local" {
            return SimpleUri {
                scheme: "file".into(),
                authority: String::new(),
                path: uri.path,
            };
        }
        uri
    }
}

// ============================================================================
// Remote Agent Connection —— 把所有 Remote 抽象串起来
// ============================================================================

/// 重连延迟策略（秒）。
const RECONNECT_DELAYS: [u64; 9] = [0, 5, 5, 10, 10, 10, 10, 10, 30];

/// 远程连接状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteConnectionState {
    Connected,
    Reconnecting,
    Disconnected,
}

/// 远程连接的完整生命周期管理器。
pub struct RemoteAgentConnection {
    authority: String,
    resolver_service: Arc<RemoteAuthorityResolverService>,
    socket_factory: Arc<RemoteSocketFactoryService>,
    protocol: Mutex<Option<Arc<PersistentProtocol>>>,
    client: Mutex<Option<Arc<IPCClient>>>,
    state_emitter: Arc<Emitter<RemoteConnectionState>>,
    self_weak: OnceLock<Weak<RemoteAgentConnection>>,
}

impl RemoteAgentConnection {
    pub fn new(
        authority: &str,
        resolver_service: Arc<RemoteAuthorityResolverService>,
        socket_factory: Arc<RemoteSocketFactoryService>,
    ) -> Arc<Self> {
        let connection = Arc::new(RemoteAgentConnection {
            authority: authority.to_string(),
            resolver_service,
            socket_factory,
            protocol: Mutex::new(None),
            client: Mutex::new(None),
            state_emitter: Arc::new(Emitter::new()),
            self_weak: OnceLock::new(),
        });
        let _ = connection.self_weak.set(Arc::downgrade(&connection));
        connection
    }

    /// 订阅连接状态变化。
    pub fn on_did_state_change(&self) -> Event<RemoteConnectionState> {
        self.state_emitter.event()
    }

    fn self_weak(&self) -> Weak<RemoteAgentConnection> {
        self.self_weak.get().cloned().unwrap_or_else(Weak::new)
    }

    /// 建立连接并返回 IPCClient。
    pub fn connect(&self) -> Promise<Arc<IPCClient>> {
        let (resolver, promise) = Promise::<Arc<IPCClient>>::new();
        let weak = self.self_weak();
        let resolver_outer = resolver.clone();

        self.resolver_service
            .resolve_authority(&self.authority)
            .on_settled(move |resolved| {
                let resolved = match resolved {
                    Ok(r) => r,
                    Err(e) => {
                        resolver_outer.reject(e);
                        return;
                    }
                };
                let Some(connection) = weak.upgrade() else {
                    resolver_outer.reject(crate::error::RpcError::new("RemoteAgentConnection dropped"));
                    return;
                };

                let query = resolved
                    .connection_token
                    .as_ref()
                    .map(|t| format!("token={t}"))
                    .unwrap_or_default();

                let weak_connect = weak.clone();
                connection
                    .socket_factory
                    .connect(&resolved.connect_to, "/", &query)
                    .on_settled(move |socket| {
                        let socket = match socket {
                            Ok(s) => s,
                            Err(e) => {
                                resolver_outer.reject(e);
                                return;
                            }
                        };
                        let Some(connection) = weak_connect.upgrade() else {
                            resolver_outer.reject(crate::error::RpcError::new("RemoteAgentConnection dropped"));
                            return;
                        };

                        let protocol = PersistentProtocol::new(socket, PersistentProtocolOptions::default());
                        let client = IPCClient::new(protocol.clone(), crate::value::Value::String(connection.authority.clone()));

                        // 监听断线，触发重连
                        let weak_reconnect = connection.self_weak();
                        let resolved_for_reconnect = resolved.clone();
                        protocol.on_socket_close().subscribe(move |_: ()| {
                            if let Some(c) = weak_reconnect.upgrade() {
                                c.state_emitter.fire(RemoteConnectionState::Reconnecting);
                                c.reconnect(resolved_for_reconnect.clone(), 0);
                            }
                        });

                        *connection.protocol.lock().unwrap() = Some(protocol);
                        *connection.client.lock().unwrap() = Some(client.clone());
                        connection.state_emitter.fire(RemoteConnectionState::Connected);
                        resolver_outer.resolve(client);
                    });
            });

        promise
    }

    fn reconnect(&self, resolved: ResolvedAuthority, attempt: usize) {
        if attempt >= RECONNECT_DELAYS.len() {
            self.state_emitter.fire(RemoteConnectionState::Disconnected);
            return;
        }
        let delay = Duration::from_secs(RECONNECT_DELAYS[attempt]);
        let query = resolved
            .connection_token
            .as_ref()
            .map(|t| format!("token={t}"))
            .unwrap_or_default();

        let weak = self.self_weak();
        let factory = self.socket_factory.clone();
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            let Some(connection) = weak.upgrade() else { return };
            let resolved_clone = resolved.clone();
            factory
                .connect(&resolved.connect_to, "/", &query)
                .on_settled(move |socket| match socket {
                    Ok(new_socket) => {
                        let protocol = connection.protocol.lock().unwrap().clone();
                        if let Some(protocol) = protocol {
                            // 用新 socket 替换，PersistentProtocol 会自动重放未确认的消息
                            protocol.replace_socket(new_socket);
                            connection.state_emitter.fire(RemoteConnectionState::Connected);
                        }
                    }
                    Err(_) => connection.reconnect(resolved_clone, attempt + 1),
                });
        });
    }
}

impl Disposable for RemoteAgentConnection {
    fn dispose(&self) {
        if let Some(client) = self.client.lock().unwrap().take() {
            client.dispose();
        }
        if let Some(protocol) = self.protocol.lock().unwrap().take() {
            protocol.dispose();
        }
        self.state_emitter.dispose();
    }
}
