//! 延迟通道 (Delayed Channel)
//!
//! Rust 移植自 `packages/rpc/src/delayedChannel.ts`。
//!
//! 当通道本身来自一个尚未落定的 Promise 时（如 IPCServer 等待客户端连接），
//! 用一个「延迟通道」先占位：一旦 promise 落定就把调用转发到真实通道。

use crate::channels_shared::Channel;
use crate::error::RpcError;
use crate::foundation::{CancellationToken, Event, Relay};
use crate::promise::Promise;
use crate::value::Value;
use std::sync::Arc;

/// 把一个 `Promise<Channel>` 包装成可立即使用的 [`Channel`]。
pub fn get_delayed_channel(promise: Promise<Arc<dyn Channel>>) -> Arc<dyn Channel> {
    Arc::new(DelayedChannel { promise })
}

struct DelayedChannel {
    promise: Promise<Arc<dyn Channel>>,
}

impl Channel for DelayedChannel {
    fn call(&self, command: &str, arg: Value, token: Arc<dyn CancellationToken>) -> Promise<Value> {
        let (resolver, out) = Promise::<Value>::new();
        let command = command.to_string();
        let promise = self.promise.clone();
        promise.on_settled(move |result| match result {
            Ok(channel) => {
                let resolver = resolver.clone();
                channel.call(&command, arg, token).on_settled(move |r| match r {
                    Ok(v) => resolver.resolve(v),
                    Err(e) => resolver.reject(e),
                });
            }
            Err(e) => resolver.reject(e),
        });
        out
    }

    fn listen(&self, event: &str, arg: Value) -> Event<Value> {
        let relay = Arc::new(Relay::<Value>::new());
        let relay_for_set = relay.clone();
        let event_name = event.to_string();
        let promise = self.promise.clone();
        promise.on_settled(move |result| {
            if let Ok(channel) = result {
                relay_for_set.set_input(channel.listen(&event_name, arg));
            }
        });

        // 让返回的 Event 持有 Relay，保证输入订阅在 promise 落定后持续有效。
        let base = relay.event();
        Event::new(move |listener| {
            let _keep_alive = &relay;
            base.subscribe_listener(listener)
        })
    }
}

/// 便于在测试中构造一个立即落定的延迟通道。
pub fn resolved_channel(channel: Arc<dyn Channel>) -> Arc<dyn Channel> {
    get_delayed_channel(Promise::resolved(channel))
}

/// 便于构造一个失败的延迟通道。
pub fn rejected_channel(error: RpcError) -> Arc<dyn Channel> {
    get_delayed_channel(Promise::rejected(error))
}
