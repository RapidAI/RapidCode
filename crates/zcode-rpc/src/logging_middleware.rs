//! RPC 日志拦截中间件
//!
//! Rust 移植自 `packages/rpc/src/logging-middleware.ts`。
//!
//! 装饰 [`ChannelServer`] / [`ChannelClient`]，在不侵入核心逻辑的前提下
//! 统一记录所有 RPC 调用和事件订阅。

use crate::channel_client::ChannelClient;
use crate::channel_server::ChannelServer;
use crate::channels_shared::{Channel, ServerChannel};
use crate::foundation::{CancellationToken, Event};
use crate::promise::Promise;
use crate::value::Value;
use std::sync::Arc;
use std::time::Instant;

/// 日志函数类型。
pub type RpcLogger = Arc<dyn Fn(&str) + Send + Sync>;

/// 装饰单个 [`ServerChannel`]，记录 call/listen。
struct LoggingServerChannel {
    inner: Arc<dyn ServerChannel>,
    channel_name: String,
    logger: RpcLogger,
}

impl ServerChannel for LoggingServerChannel {
    fn call(
        &self,
        ctx: &Value,
        command: &str,
        arg: Value,
        token: Arc<dyn CancellationToken>,
    ) -> Promise<Value> {
        let start = Instant::now();
        let logger = self.logger.clone();
        let channel_name = self.channel_name.clone();
        let command = command.to_string();
        let inner = self.inner.clone();
        let ctx = ctx.clone();

        let (resolver, promise) = Promise::<Value>::new();
        inner.call(&ctx, &command, arg, token).on_settled(move |result| {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            match result {
                Ok(v) => {
                    logger(&format!("[rpc:call] {channel_name}.{command} OK ({elapsed:.1}ms)"));
                    resolver.resolve(v);
                }
                Err(e) => {
                    logger(&format!("[rpc:call] {channel_name}.{command} FAIL ({elapsed:.1}ms)"));
                    resolver.reject(e);
                }
            }
        });
        promise
    }

    fn listen(&self, ctx: &Value, event: &str, arg: Value) -> Event<Value> {
        (self.logger)(&format!(
            "[rpc:listen] {}.{} subscribed",
            self.channel_name, event
        ));
        self.inner.listen(ctx, event, arg)
    }
}

/// 包装 [`ChannelServer`]，为每个注册的频道自动加上日志。
pub struct LoggingChannelServer {
    inner: Arc<ChannelServer>,
    logger: RpcLogger,
}

impl LoggingChannelServer {
    pub fn new(inner: Arc<ChannelServer>, logger: RpcLogger) -> Self {
        LoggingChannelServer { inner, logger }
    }

    pub fn register_channel(&self, channel_name: &str, channel: Arc<dyn ServerChannel>) {
        (self.logger)(&format!("[rpc:register] channel \"{channel_name}\""));
        let wrapped: Arc<dyn ServerChannel> = Arc::new(LoggingServerChannel {
            inner: channel,
            channel_name: channel_name.to_string(),
            logger: self.logger.clone(),
        });
        self.inner.register_channel(channel_name, wrapped);
    }

    pub fn ready(&self) {
        self.inner.ready();
    }

    pub fn inner(&self) -> Arc<ChannelServer> {
        self.inner.clone()
    }
}

/// 装饰单个 [`Channel`]（客户端侧），记录 call/listen。
struct LoggingChannel {
    inner: Arc<dyn Channel>,
    channel_name: String,
    logger: RpcLogger,
}

impl Channel for LoggingChannel {
    fn call(&self, command: &str, arg: Value, token: Arc<dyn CancellationToken>) -> Promise<Value> {
        let start = Instant::now();
        let logger = self.logger.clone();
        let channel_name = self.channel_name.clone();
        let command = command.to_string();
        let inner = self.inner.clone();

        let (resolver, promise) = Promise::<Value>::new();
        inner.call(&command, arg, token).on_settled(move |result| {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            match result {
                Ok(v) => {
                    logger(&format!(
                        "[rpc:call] {channel_name}.{command} → OK ({elapsed:.1}ms)"
                    ));
                    resolver.resolve(v);
                }
                Err(e) => {
                    logger(&format!(
                        "[rpc:call] {channel_name}.{command} → FAIL ({elapsed:.1}ms)"
                    ));
                    resolver.reject(e);
                }
            }
        });
        promise
    }

    fn listen(&self, event: &str, arg: Value) -> Event<Value> {
        (self.logger)(&format!(
            "[rpc:listen] {}.{} → subscribed",
            self.channel_name, event
        ));
        self.inner.listen(event, arg)
    }
}

/// 包装 [`ChannelClient`]，为每个获取的频道自动加上日志。
pub struct LoggingChannelClient {
    inner: Arc<ChannelClient>,
    logger: RpcLogger,
}

impl LoggingChannelClient {
    pub fn new(inner: Arc<ChannelClient>, logger: RpcLogger) -> Self {
        LoggingChannelClient { inner, logger }
    }

    pub fn get_channel(&self, channel_name: &str) -> Arc<dyn Channel> {
        let channel = self.inner.get_channel(channel_name);
        Arc::new(LoggingChannel {
            inner: channel,
            channel_name: channel_name.to_string(),
            logger: self.logger.clone(),
        })
    }

    pub fn inner(&self) -> Arc<ChannelClient> {
        self.inner.clone()
    }
}
