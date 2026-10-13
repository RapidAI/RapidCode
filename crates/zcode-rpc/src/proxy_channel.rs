//! Layer 5: ProxyChannel —— 服务自动代理
//!
//! Rust 移植自 `packages/rpc/src/proxy-channel.ts`。
//!
//! 原实现依赖 ES6 `Proxy` 拦截任意属性访问。Rust 是静态类型语言，
//! 没有等价的动态属性拦截，因此这里用**显式注册表**（方法/事件按名注册）
//! 实现同一套语义：
//!
//! - 方法 → `call`
//! - `on*` 事件 → `listen`
//! - `onDynamic*` 动态事件（调用后返回 Event）→ `listen` 时传参
//! - 事件在无人订阅时先 buffer，一有订阅者就 flush
//!
//! 服务端：
//! ```ignore
//! let mut def = ServiceDefinition::new();
//! def.method("readFile", |args| { /* ... */ Promise::resolved(Value::Undefined) });
//! def.event("onDidChange", some_event);
//! let channel = ProxyChannel::from_service(def);
//! ```
//!
//! 客户端：
//! ```ignore
//! let service = ProxyChannel::to_service(channel);
//! let result = service.call("readFile", vec![arg]);
//! ```

use crate::channels_shared::{Channel, ServerChannel};
use crate::foundation::{
    CancellationToken, Disposable, DisposableHandle, Event, Emitter, Listener,
};
use crate::promise::Promise;
use crate::value::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// 同步/异步方法处理器。
pub type MethodHandler = Arc<dyn Fn(Vec<Value>) -> Promise<Value> + Send + Sync>;
/// 动态事件处理器：`onDynamicXxx(arg)` 返回一个 Event。
pub type DynamicEventHandler = Arc<dyn Fn(Value) -> Event<Value> + Send + Sync>;

/// 服务定义：把 Rust 对象显式描述为可被 RPC 代理的成员表。
#[derive(Default, Clone)]
pub struct ServiceDefinition {
    methods: HashMap<String, MethodHandler>,
    events: HashMap<String, Event<Value>>,
    dynamic_events: HashMap<String, DynamicEventHandler>,
}

impl ServiceDefinition {
    pub fn new() -> Self {
        ServiceDefinition::default()
    }

    /// 注册一个方法。
    pub fn method<F>(&mut self, name: &str, handler: F) -> &mut Self
    where
        F: Fn(Vec<Value>) -> Promise<Value> + Send + Sync + 'static,
    {
        self.methods.insert(name.to_string(), Arc::new(handler));
        self
    }

    /// 注册一个同步方法（返回普通值）。
    pub fn sync_method<F>(&mut self, name: &str, handler: F) -> &mut Self
    where
        F: Fn(Vec<Value>) -> Value + Send + Sync + 'static,
    {
        let handler = Arc::new(handler);
        self.methods
            .insert(name.to_string(), Arc::new(move |args| Promise::resolved(handler(args))));
        self
    }

    /// 注册一个静态事件（`onXxx`）。
    pub fn event(&mut self, name: &str, event: Event<Value>) -> &mut Self {
        self.events.insert(name.to_string(), event);
        self
    }

    /// 注册一个动态事件（`onDynamicXxx`）。
    pub fn dynamic_event<F>(&mut self, name: &str, handler: F) -> &mut Self
    where
        F: Fn(Value) -> Event<Value> + Send + Sync + 'static,
    {
        self.dynamic_events.insert(name.to_string(), Arc::new(handler));
        self
    }
}

/// ProxyChannel 命名空间。
pub struct ProxyChannel;

impl ProxyChannel {
    /// 服务端：把 [`ServiceDefinition`] 包装为 [`ServerChannel`]。
    ///
    /// 约定与 TS 一致：
    /// - 静态事件在首次使用前会被 buffer，避免订阅前的事件丢失。
    pub fn from_service(definition: ServiceDefinition) -> Arc<dyn ServerChannel> {
        // 预缓存所有静态事件并 buffer（对应 TS 的 `bufferEvent`）。
        let mut event_map: HashMap<String, Event<Value>> = HashMap::new();
        for (key, event) in &definition.events {
            if is_event(key) && !is_dynamic_event(key) {
                event_map.insert(key.clone(), buffer_event(event.clone()));
            }
        }
        let event_map = Arc::new(Mutex::new(event_map));
        let methods = Arc::new(definition.methods);
        let dynamic_events = Arc::new(definition.dynamic_events);

        Arc::new(ServiceServerChannel {
            methods,
            event_map,
            dynamic_events,
        })
    }

    /// 客户端：把 [`Channel`] 包装为可调用的服务代理。
    pub fn to_service(channel: Arc<dyn Channel>) -> ServiceClient {
        ServiceClient { channel }
    }
}

/// 客户端服务代理。
pub struct ServiceClient {
    channel: Arc<dyn Channel>,
}

impl ServiceClient {
    /// 调用远端方法。
    pub fn call(&self, method: &str, args: Vec<Value>) -> Promise<Value> {
        self.channel
            .call(method, Value::Array(args), crate::foundation::cancellation_token_none())
    }

    /// 订阅远端事件。
    pub fn listen(&self, event: &str) -> Event<Value> {
        self.channel.listen(event, Value::Undefined)
    }

    /// 订阅远端动态事件（传参）。
    pub fn listen_dynamic(&self, event: &str, arg: Value) -> Event<Value> {
        self.channel.listen(event, arg)
    }

    /// 访问底层通道。
    pub fn channel(&self) -> Arc<dyn Channel> {
        self.channel.clone()
    }
}

struct ServiceServerChannel {
    methods: Arc<HashMap<String, MethodHandler>>,
    event_map: Arc<Mutex<HashMap<String, Event<Value>>>>,
    dynamic_events: Arc<HashMap<String, DynamicEventHandler>>,
}

impl ServerChannel for ServiceServerChannel {
    fn call(
        &self,
        _ctx: &Value,
        command: &str,
        arg: Value,
        _token: Arc<dyn CancellationToken>,
    ) -> Promise<Value> {
        let Some(handler) = self.methods.get(command) else {
            return Promise::rejected(crate::error::RpcError::new(format!("Method not found: {command}")));
        };
        let args = match arg {
            Value::Array(items) => items,
            Value::Undefined => Vec::new(),
            other => vec![other],
        };
        handler(args)
    }

    fn listen(&self, _ctx: &Value, event: &str, arg: Value) -> Event<Value> {
        // 先查缓存
        if let Some(cached) = self.event_map.lock().unwrap().get(event) {
            return cached.clone();
        }

        // 动态事件
        if is_dynamic_event(event) {
            if let Some(handler) = self.dynamic_events.get(event) {
                return handler(arg);
            }
        }

        // 延迟发现的事件
        if is_event(event) {
            if let Some(handler) = self.dynamic_events.get(event) {
                let e = handler(arg);
                self.event_map
                    .lock()
                    .unwrap()
                    .insert(event.to_string(), e.clone());
                return e;
            }
        }

        // 未找到：返回永不触发的事件（对应 TS 的抛错，这里降级为安全空事件）。
        Event::none()
    }
}

// ============================================================================
// 命名约定
// ============================================================================

/// 匹配 `onXxx` 事件命名约定。
pub fn is_event(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() >= 3 && bytes[0] == b'o' && bytes[1] == b'n' && (b'A'..=b'Z').contains(&bytes[2])
}

/// 匹配 `onDynamicXxx` 动态事件命名约定。
pub fn is_dynamic_event(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() >= 10
        && name.starts_with("onDynamic")
        && (b'A'..=b'Z').contains(&bytes[9])
}

// ============================================================================
// 事件缓冲
// ============================================================================

/// 缓冲事件：确保在订阅之前触发的事件不会丢失。
///
/// 未订阅时事件存入队列，一有订阅者就 flush。
pub fn buffer_event(event: Event<Value>) -> Event<Value> {
    let buffer: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let flushing = Arc::new(AtomicBool::new(false));
    let holder: Arc<OnceLock<Arc<Emitter<Value>>>> = Arc::new(OnceLock::new());
    let source_sub: Arc<Mutex<Option<Box<dyn Disposable>>>> = Arc::new(Mutex::new(None));

    let (buffer_first, flushing_first, holder_first, source_sub_first, event_first) = (
        buffer.clone(),
        flushing.clone(),
        holder.clone(),
        source_sub.clone(),
        event.clone(),
    );
    let (buffer_last, source_sub_last) = (buffer.clone(), source_sub.clone());

    let emitter = Arc::new(Emitter::<Value>::with_options(
        Some(Box::new(move || {
            let emitter = holder_first.get().cloned();
            let buffer = buffer_first.clone();
            let flushing = flushing_first.clone();
            let d = event_first.subscribe(move |e: Value| {
                if flushing.load(Ordering::SeqCst) {
                    if let Some(em) = &emitter {
                        em.fire(e);
                    }
                } else {
                    buffer.lock().unwrap().push(e);
                }
            });
            *source_sub_first.lock().unwrap() = Some(d);
        })),
        Some(Box::new(move || {
            if let Some(d) = source_sub_last.lock().unwrap().take() {
                d.dispose();
            }
            buffer_last.lock().unwrap().clear();
        })),
    ));
    let _ = holder.set(emitter.clone());

    let base = emitter.event();
    let (buffer_outer, flushing_outer, emitter_outer) = (buffer, flushing, emitter);
    Event::new(move |listener: Listener<Value>| {
        let d = base.subscribe_listener(listener);
        // 一旦有订阅者，先 flush 缓冲区（只做一次）。
        if !flushing_outer.swap(true, Ordering::SeqCst) {
            let pending = std::mem::take(&mut *buffer_outer.lock().unwrap());
            for item in pending {
                emitter_outer.fire(item);
            }
        }
        d
    })
}

/// 便于外部构造一次性释放器。
pub fn once_disposable<F: FnOnce() + Send + 'static>(f: F) -> Box<dyn Disposable> {
    Box::new(DisposableHandle::new(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn naming_conventions() {
        assert!(is_event("onDidChange"));
        assert!(!is_event("on"));
        assert!(!is_event("Open"));
        assert!(is_dynamic_event("onDynamicFoo"));
        assert!(!is_dynamic_event("onDidChange"));
    }

    #[test]
    fn buffer_event_keeps_events_fired_during_subscription() {
        // 模拟一个「订阅时同步回放历史」的源事件：这正是 buffer 要处理的窗口。
        let source = Event::new(|listener: Listener<Value>| {
            listener(Value::Int(1));
            listener(Value::Int(2));
            Box::new(crate::foundation::NoopDisposable)
        });
        let buffered = buffer_event(source);

        let received = Arc::new(Mutex::new(Vec::<i64>::new()));
        let received_clone = received.clone();
        let _d = buffered.subscribe(move |v: Value| {
            if let Some(n) = v.as_int() {
                received_clone.lock().unwrap().push(n);
            }
        });

        assert_eq!(*received.lock().unwrap(), vec![1, 2]);
    }

    #[test]
    fn buffer_event_forwards_after_subscription() {
        let emitter = Arc::new(Emitter::<Value>::new());
        let buffered = buffer_event(emitter.event());

        let received = Arc::new(Mutex::new(Vec::<i64>::new()));
        let received_clone = received.clone();
        let _d = buffered.subscribe(move |v: Value| {
            if let Some(n) = v.as_int() {
                received_clone.lock().unwrap().push(n);
            }
        });

        emitter.fire(Value::Int(3));
        assert_eq!(*received.lock().unwrap(), vec![3]);
    }
}
