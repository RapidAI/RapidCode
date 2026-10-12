//! # zcode-rpc
//!
//! VS Code 风格 IPC 通信抽象框架的 **Rust 移植**。
//!
//! 原实现为 TypeScript：`packages/rpc/src/*`（约 3.3k 行）。本 crate 逐层对应：
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │  Layer 6: Remote 远程连接            (remote)                 │
//! │  Layer 5: ProxyChannel 服务自动代理  (proxy_channel)          │
//! │  Layer 4: IPCServer / IPCClient      (ipc)                    │
//! │  Layer 3: ChannelServer / ChannelClient (channel_server/client)│
//! │  Layer 2: MessagePassingProtocol     (protocol)               │
//! │  Layer 1: 序列化 (VQL + 类型标签)     (serialization)          │
//! │  Layer 0: 基础设施 Event/Disposable   (foundation)            │
//! └──────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 设计取舍
//!
//! - **零外部依赖**：仅用 `std`（`Mutex` / `Condvar` / 线程 / mpsc）。
//!   异步结果用回调式 [`Promise`](promise::Promise) 承载，语义对齐 JS Promise。
//! - **动态类型**：TS 的 `any` 载荷用 [`Value`](value::Value) 枚举表示，
//!   并自带 JSON / Base64 编解码，复刻原 `DataType::Object` 回退分支。
//! - **TContext**：统一用 [`Value`](value::Value) 承载（本身即可序列化）。

pub mod buffer;
pub mod channel_client;
pub mod channel_server;
pub mod channels;
pub mod channels_shared;
pub mod delayed_channel;
pub mod error;
pub mod foundation;
pub mod ipc;
pub mod logging_middleware;
pub mod network_telemetry_middleware;
pub mod persistent_protocol;
pub mod promise;
pub mod protocol;
pub mod proxy_channel;
pub mod remote;
pub mod serialization;
pub mod value;

// ---- Layer 0: 基础设施 ----
pub use foundation::{
    cancellation_token_none, to_disposable, CancellationToken, CancellationTokenSource,
    Disposable, DisposableStore, Event, EventMultiplexer, Emitter, Listener, Relay,
};

// ---- Layer 0.5: Buffer ----
pub use buffer::VSBuffer;

// ---- Layer 1: 序列化 ----
pub use serialization::{deserialize, serialize, BufferReader, BufferWriter, Reader, Writer};

// ---- 动态值 & 错误 ----
pub use error::RpcError;
pub use promise::{Promise, PromiseResolver};
pub use value::Value;

// ---- Layer 2: 协议 ----
pub use protocol::{
    create_queue_pair, ConnectionFlowControl, MessagePassingProtocol, MessagePortFlowState,
    MessagePortLike, MessagePortProtocol, ProtocolMessage, ProtocolMessageType, Socket,
    SocketProtocol, HEADER_SIZE,
};

// ---- Layer 3: 通道 ----
pub use channels_shared::{Channel, RawResponse, RequestType, ResponseType, ServerChannel};
pub use channel_client::ChannelClient;
pub use channel_server::ChannelServer;
pub use delayed_channel::get_delayed_channel;

// ---- Layer 4: 连接管理 ----
pub use ipc::{ChannelSelector, ClientRouter, Connection, ConnectionHub, IPCClient, IPCServer, StaticRouter};

// ---- Layer 5: ProxyChannel ----
pub use proxy_channel::{is_dynamic_event, is_event, ProxyChannel, ServiceClient, ServiceDefinition};

// ---- Layer 6: Remote ----
pub use remote::{
    create_uri_transformer, RemoteAgentConnection, RemoteAuthorityResolver,
    RemoteAuthorityResolverService, RemoteConnection, RemoteConnectionState, RemoteConnectionType,
    ResolvedAuthority, SimpleUri, SocketFactory, UriTransformer,
};

// ---- 中间件 ----
pub use logging_middleware::{LoggingChannelClient, LoggingChannelServer, RpcLogger};
pub use network_telemetry_middleware::{
    emit_network_telemetry_observation, set_network_telemetry_sink, NetworkObservation,
    NetworkTelemetryChannelClient, NetworkTelemetryChannelServer, NetworkTelemetrySink,
    NetworkTransportKind,
};

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[test]
    fn end_to_end_call_over_queue_pair() {
        let (client_protocol, server_protocol) = create_queue_pair();
        // 先建客户端（订阅 on_message），再建服务端（发送 Initialize）。
        let client = ChannelClient::new(client_protocol);
        let server = ChannelServer::new(
            server_protocol,
            Value::String("test-ctx".into()),
            Duration::from_millis(500),
            false,
        );

        let mut def = ServiceDefinition::new();
        def.sync_method("add", |args| {
            let a = args.first().and_then(|v| v.as_int()).unwrap_or(0);
            let b = args.get(1).and_then(|v| v.as_int()).unwrap_or(0);
            Value::Int(a + b)
        });
        def.sync_method("echo", |args| args.first().cloned().unwrap_or(Value::Undefined));
        server.register_channel("math", ProxyChannel::from_service(def));

        let channel = client.get_channel("math");
        let result = channel
            .call(
                "add",
                Value::Array(vec![Value::Int(2), Value::Int(3)]),
                cancellation_token_none(),
            )
            .wait()
            .expect("call should succeed");
        assert_eq!(result, Value::Int(5));

        let echoed = channel
            .call(
                "echo",
                Value::String("hi".into()),
                cancellation_token_none(),
            )
            .wait()
            .unwrap();
        assert_eq!(echoed, Value::String("hi".into()));
    }

    #[test]
    fn end_to_end_event_listen() {
        let (client_protocol, server_protocol) = create_queue_pair();
        let client = ChannelClient::new(client_protocol);
        let server = ChannelServer::new(
            server_protocol,
            Value::String("ctx".into()),
            Duration::from_millis(500),
            false,
        );

        let source = Arc::new(Emitter::<Value>::new());
        let mut def = ServiceDefinition::new();
        def.event("onTick", source.event());
        server.register_channel("clock", ProxyChannel::from_service(def));

        let channel = client.get_channel("clock");
        let received = Arc::new(Mutex::new(Vec::<i64>::new()));
        let received_clone = received.clone();
        let _sub = channel.listen("onTick", Value::Undefined).subscribe(move |v: Value| {
            if let Some(n) = v.as_int() {
                received_clone.lock().unwrap().push(n);
            }
        });

        // 给 EventListen 请求一点时间到达服务端。
        std::thread::sleep(Duration::from_millis(50));
        source.fire(Value::Int(7));
        source.fire(Value::Int(8));
        std::thread::sleep(Duration::from_millis(50));

        assert_eq!(*received.lock().unwrap(), vec![7, 8]);
    }

    #[test]
    fn unknown_method_rejects() {
        let (client_protocol, server_protocol) = create_queue_pair();
        let client = ChannelClient::new(client_protocol);
        let server = ChannelServer::new(
            server_protocol,
            Value::String("ctx".into()),
            Duration::from_millis(500),
            false,
        );
        let def = ServiceDefinition::new();
        server.register_channel("empty", ProxyChannel::from_service(def));

        let channel = client.get_channel("empty");
        let err = channel
            .call("nope", Value::Undefined, cancellation_token_none())
            .wait()
            .expect_err("should reject");
        assert!(err.message.contains("Method not found"));
    }
}
