//! RPC 调用网络遥测
//!
//! Rust 移植自 `packages/rpc/src/network-telemetry-middleware.ts`。
//!
//! 记录 `channel.command` 级成功率与耗时，供上层聚合上报。

use crate::channel_client::ChannelClient;
use crate::channel_server::ChannelServer;
use crate::channels_shared::{Channel, ServerChannel};
use crate::foundation::{CancellationToken, Event};
use crate::promise::Promise;
use crate::value::Value;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

/// 传输类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkTransportKind {
    Http,
    WebSocket,
    Rpc,
}

impl NetworkTransportKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            NetworkTransportKind::Http => "http",
            NetworkTransportKind::WebSocket => "websocket",
            NetworkTransportKind::Rpc => "rpc",
        }
    }
}

/// 一次网络观测。
#[derive(Clone, Debug)]
pub struct NetworkObservation {
    pub transport: NetworkTransportKind,
    pub interface: String,
    pub duration_ms: f64,
    pub ok: bool,
    pub status_code: Option<u16>,
    pub error_kind: Option<String>,
    pub attempt: Option<u32>,
}

/// 遥测接收器。
pub type NetworkTelemetrySink = Arc<dyn Fn(&NetworkObservation) + Send + Sync>;

fn sink_slot() -> &'static Mutex<Option<NetworkTelemetrySink>> {
    static SLOT: OnceLock<Mutex<Option<NetworkTelemetrySink>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// 设置全局遥测接收器。
pub fn set_network_telemetry_sink(sink: Option<NetworkTelemetrySink>) {
    *sink_slot().lock().unwrap() = sink;
}

/// 上报一次观测。
pub fn emit_network_telemetry_observation(observation: NetworkObservation) {
    let sink = sink_slot().lock().unwrap().clone();
    if let Some(sink) = sink {
        sink(&observation);
    }
}

/// 根据错误信息归类。
fn classify_error_kind(error: &str) -> String {
    let message = error.to_lowercase();
    if message.contains("timeout") || message.contains("timed out") {
        return "timeout".to_string();
    }
    if message.contains("dns") || message.contains("getaddrinfo") || message.contains("enotfound") {
        return "dns_failure".to_string();
    }
    if message.contains("econnreset")
        || message.contains("connection reset")
        || message.contains("econnrefused")
    {
        return "connection_reset".to_string();
    }
    "other".to_string()
}

fn emit_rpc_observation(channel_name: &str, command: &str, duration_ms: f64, ok: bool, error: Option<&str>) {
    emit_network_telemetry_observation(NetworkObservation {
        transport: NetworkTransportKind::Rpc,
        interface: format!("{channel_name}.{command}"),
        duration_ms: duration_ms.max(0.0).round(),
        ok,
        status_code: None,
        error_kind: if ok {
            None
        } else {
            Some(classify_error_kind(error.unwrap_or("")))
        },
        attempt: Some(1),
    });
}

struct NetworkTelemetryServerChannel {
    inner: Arc<dyn ServerChannel>,
    channel_name: String,
}

impl ServerChannel for NetworkTelemetryServerChannel {
    fn call(
        &self,
        ctx: &Value,
        command: &str,
        arg: Value,
        token: Arc<dyn CancellationToken>,
    ) -> Promise<Value> {
        let start = Instant::now();
        let channel_name = self.channel_name.clone();
        let command = command.to_string();
        let (resolver, promise) = Promise::<Value>::new();
        self.inner.call(ctx, &command, arg, token).on_settled(move |result| {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            match result {
                Ok(v) => {
                    emit_rpc_observation(&channel_name, &command, elapsed, true, None);
                    resolver.resolve(v);
                }
                Err(e) => {
                    emit_rpc_observation(&channel_name, &command, elapsed, false, Some(&e.message));
                    resolver.reject(e);
                }
            }
        });
        promise
    }

    fn listen(&self, ctx: &Value, event: &str, arg: Value) -> Event<Value> {
        self.inner.listen(ctx, event, arg)
    }
}

struct NetworkTelemetryChannel {
    inner: Arc<dyn Channel>,
    channel_name: String,
}

impl Channel for NetworkTelemetryChannel {
    fn call(&self, command: &str, arg: Value, token: Arc<dyn CancellationToken>) -> Promise<Value> {
        let start = Instant::now();
        let channel_name = self.channel_name.clone();
        let command = command.to_string();
        let (resolver, promise) = Promise::<Value>::new();
        self.inner.call(&command, arg, token).on_settled(move |result| {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            match result {
                Ok(v) => {
                    emit_rpc_observation(&channel_name, &command, elapsed, true, None);
                    resolver.resolve(v);
                }
                Err(e) => {
                    emit_rpc_observation(&channel_name, &command, elapsed, false, Some(&e.message));
                    resolver.reject(e);
                }
            }
        });
        promise
    }

    fn listen(&self, event: &str, arg: Value) -> Event<Value> {
        self.inner.listen(event, arg)
    }
}

/// 装饰 [`ChannelServer`]，为 RPC call 写入网络遥测。
pub struct NetworkTelemetryChannelServer {
    inner: Arc<ChannelServer>,
}

impl NetworkTelemetryChannelServer {
    pub fn new(inner: Arc<ChannelServer>) -> Self {
        NetworkTelemetryChannelServer { inner }
    }

    pub fn register_channel(&self, channel_name: &str, channel: Arc<dyn ServerChannel>) {
        let wrapped: Arc<dyn ServerChannel> = Arc::new(NetworkTelemetryServerChannel {
            inner: channel,
            channel_name: channel_name.to_string(),
        });
        self.inner.register_channel(channel_name, wrapped);
    }

    pub fn ready(&self) {
        self.inner.ready();
    }
}

/// 装饰 [`ChannelClient`]（renderer 侧可选，与 server 侧二选一即可避免双计）。
pub struct NetworkTelemetryChannelClient {
    inner: Arc<ChannelClient>,
}

impl NetworkTelemetryChannelClient {
    pub fn new(inner: Arc<ChannelClient>) -> Self {
        NetworkTelemetryChannelClient { inner }
    }

    pub fn get_channel(&self, channel_name: &str) -> Arc<dyn Channel> {
        let channel = self.inner.get_channel(channel_name);
        Arc::new(NetworkTelemetryChannel {
            inner: channel,
            channel_name: channel_name.to_string(),
        })
    }
}
