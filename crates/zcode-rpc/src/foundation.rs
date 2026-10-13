//! Layer 0: 基础设施 (Foundation)
//!
//! Rust 移植自 `packages/rpc/src/foundation.ts`。
//!
//! - [`Disposable`] / [`DisposableStore`]: 资源生命周期管理
//! - [`Event`] / [`Emitter`]: 事件系统
//! - [`CancellationToken`]: 取消令牌
//!
//! 这些是整个 IPC 框架的地基，所有上层模块都依赖它们。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

// ============================================================================
// Disposable - 资源释放模式
// ============================================================================

/// 可释放资源。`dispose` 必须幂等（重复调用无副作用）。
pub trait Disposable: Send + Sync {
    fn dispose(&self);
}

/// 无操作释放器，对应 TS 里的 `{ dispose() {} }`。
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopDisposable;

impl Disposable for NoopDisposable {
    fn dispose(&self) {}
}

/// 由闭包构造的一次性释放器，对应 TS 的 `toDisposable(fn)`。
pub struct DisposableHandle {
    inner: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl DisposableHandle {
    /// 用闭包创建一个只执行一次的释放器。
    pub fn new<F>(f: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        DisposableHandle {
            inner: Mutex::new(Some(Box::new(f))),
        }
    }
}

impl Disposable for DisposableHandle {
    fn dispose(&self) {
        // 取出后置 None，保证只执行一次（对应 TS 的 `once` 包装）。
        let taken = self.inner.lock().unwrap().take();
        if let Some(f) = taken {
            f();
        }
    }
}

/// 对应 TS 的 `toDisposable(fn)`。
pub fn to_disposable<F>(f: F) -> Box<dyn Disposable>
where
    F: FnOnce() + Send + 'static,
{
    Box::new(DisposableHandle::new(f))
}

/// 可共享的释放槽：多处持有同一个句柄，谁先释放谁生效。
pub type DisposableSlot = Arc<Mutex<Option<Box<dyn Disposable>>>>;

/// 创建一个空的释放槽。
pub fn disposable_slot() -> DisposableSlot {
    Arc::new(Mutex::new(None))
}

/// 释放槽的 [`Disposable`] 包装。
pub struct SlotDisposable(pub DisposableSlot);

impl Disposable for SlotDisposable {
    fn dispose(&self) {
        if let Some(d) = self.0.lock().unwrap().take() {
            d.dispose();
        }
    }
}

/// 把 `Arc<D>` 提升为可共享的 [`Disposable`]。
pub struct ArcDisposable<D: Disposable + ?Sized>(pub Arc<D>);

impl<D: Disposable + ?Sized> Disposable for ArcDisposable<D> {
    fn dispose(&self) {
        self.0.dispose();
    }
}

/// 收集多个 [`Disposable`]，统一释放。
///
/// VS Code 里几乎每个类都有一个 DisposableStore 来管理子资源。
#[derive(Default)]
pub struct DisposableStore {
    items: Mutex<Vec<Box<dyn Disposable>>>,
    is_disposed: AtomicBool,
}

impl DisposableStore {
    pub fn new() -> Self {
        DisposableStore {
            items: Mutex::new(Vec::new()),
            is_disposed: AtomicBool::new(false),
        }
    }

    /// 添加一个子资源。若 store 已释放，则立刻释放该资源并返回 `None`。
    pub fn add<D: Disposable + 'static>(&self, item: D) -> Option<Arc<D>> {
        if self.is_disposed.load(Ordering::SeqCst) {
            item.dispose();
            return None;
        }
        let arc = Arc::new(item);
        self.items
            .lock()
            .unwrap()
            .push(Box::new(ArcDisposable(arc.clone())));
        Some(arc)
    }

    /// 添加一个已装箱的子资源。
    pub fn add_boxed(&self, item: Box<dyn Disposable>) {
        if self.is_disposed.load(Ordering::SeqCst) {
            item.dispose();
            return;
        }
        self.items.lock().unwrap().push(item);
    }
}

impl Disposable for DisposableStore {
    fn dispose(&self) {
        if self.is_disposed.swap(true, Ordering::SeqCst) {
            return;
        }
        let items = std::mem::take(&mut *self.items.lock().unwrap());
        for item in items {
            item.dispose();
        }
    }
}

// ============================================================================
// Event System - 事件系统
// ============================================================================

/// 事件监听器：接收事件值。
pub type Listener<T> = Arc<dyn Fn(T) + Send + Sync>;

/// [`Event<T>`] 对应 TS 的 `type Event<T> = (listener) => IDisposable`。
///
/// 内部是一个可克隆的订阅函数；订阅时传入 listener，返回用于取消订阅的释放器。
#[derive(Clone)]
pub struct Event<T> {
    subscribe_fn: Arc<dyn Fn(Listener<T>) -> Box<dyn Disposable> + Send + Sync>,
}

impl<T> Event<T> {
    /// 用一个订阅函数构造事件。
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(Listener<T>) -> Box<dyn Disposable> + Send + Sync + 'static,
    {
        Event {
            subscribe_fn: Arc::new(f),
        }
    }

    /// 订阅事件，返回取消订阅句柄。
    pub fn subscribe<F>(&self, listener: F) -> Box<dyn Disposable>
    where
        F: Fn(T) + Send + Sync + 'static,
    {
        (self.subscribe_fn)(Arc::new(listener))
    }

    /// 订阅事件（listener 已是 `Arc`）。
    pub fn subscribe_listener(&self, listener: Listener<T>) -> Box<dyn Disposable> {
        (self.subscribe_fn)(listener)
    }

    /// 永远不触发的事件。
    pub fn none() -> Self
    where
        T: 'static,
    {
        Event::new(|_| Box::new(NoopDisposable))
    }

    /// 只触发一次就自动取消订阅。
    pub fn once(self) -> Self
    where
        T: 'static,
    {
        Event::new(move |listener| {
            let fired = Arc::new(AtomicBool::new(false));
            // 共享槽：保存真实订阅句柄，便于触发时自动释放 / 外部取消。
            let slot: Arc<Mutex<Option<Box<dyn Disposable>>>> = Arc::new(Mutex::new(None));

            let fired_inner = fired.clone();
            let slot_inner = slot.clone();
            let listener_inner = listener.clone();
            let real = self.subscribe(move |e: T| {
                if !fired_inner.swap(true, Ordering::SeqCst) {
                    if let Some(d) = slot_inner.lock().unwrap().take() {
                        d.dispose();
                    }
                    listener_inner(e);
                }
            });
            *slot.lock().unwrap() = Some(real);

            Box::new(DisposableHandle::new(move || {
                if let Some(d) = slot.lock().unwrap().take() {
                    d.dispose();
                }
            }))
        })
    }

    /// 过滤事件。
    pub fn filter<F>(self, predicate: F) -> Self
    where
        T: 'static,
        F: Fn(&T) -> bool + Send + Sync + 'static,
    {
        let predicate = Arc::new(predicate);
        Event::new(move |listener| {
            let predicate = predicate.clone();
            let listener = listener.clone();
            self.subscribe(move |e: T| {
                if predicate(&e) {
                    listener(e);
                }
            })
        })
    }

    /// 映射事件。
    pub fn map<U, F>(self, mapper: F) -> Event<U>
    where
        T: 'static,
        U: 'static,
        F: Fn(T) -> U + Send + Sync + 'static,
    {
        let mapper = Arc::new(mapper);
        Event::new(move |listener| {
            let mapper = mapper.clone();
            self.subscribe(move |e: T| listener(mapper(e)))
        })
    }
}

/// 内部：带唯一 id 的监听器，保证按身份精确移除。
struct RegisteredListener<T> {
    id: u64,
    listener: Listener<T>,
}

/// [`Emitter<T>`] 是事件的发射器。
///
/// 关键设计（与 TS 一致）：
/// - `on_will_add_first_listener`: 第一个订阅者到来时触发（懒初始化资源）
/// - `on_did_remove_last_listener`: 最后一个订阅者离开时触发（释放资源）
///
/// 这个「懒订阅」机制在 IPC 框架中至关重要 —— ChannelClient 的
/// `requestEvent` 正是利用它来实现「有人监听才发送 EventListen 请求」。
pub struct Emitter<T> {
    state: Arc<EmitterState<T>>,
}

struct EmitterState<T> {
    listeners: Mutex<Vec<RegisteredListener<T>>>,
    next_id: AtomicU64,
    disposed: AtomicBool,
    on_first: Option<Box<dyn Fn() + Send + Sync>>,
    on_last: Option<Box<dyn Fn() + Send + Sync>>,
}

impl<T> Emitter<T> {
    pub fn new() -> Self
    where
        T: 'static,
    {
        Emitter::with_options(None, None)
    }

    /// 带懒订阅回调的构造。
    pub fn with_options(
        on_first: Option<Box<dyn Fn() + Send + Sync>>,
        on_last: Option<Box<dyn Fn() + Send + Sync>>,
    ) -> Self
    where
        T: 'static,
    {
        Emitter {
            state: Arc::new(EmitterState {
                listeners: Mutex::new(Vec::new()),
                next_id: AtomicU64::new(1),
                disposed: AtomicBool::new(false),
                on_first,
                on_last,
            }),
        }
    }

    /// 获取可订阅的事件句柄。
    pub fn event(&self) -> Event<T>
    where
        T: 'static,
    {
        let state = self.state.clone();
        Event::new(move |listener| {
            if state.disposed.load(Ordering::SeqCst) {
                return Box::new(NoopDisposable);
            }

            let id = state.next_id.fetch_add(1, Ordering::SeqCst);
            let is_first = {
                let mut listeners = state.listeners.lock().unwrap();
                let is_first = listeners.is_empty();
                listeners.push(RegisteredListener { id, listener });
                is_first
            };

            if is_first {
                if let Some(cb) = &state.on_first {
                    cb();
                }
            }

            let state_for_dispose = state.clone();
            Box::new(DisposableHandle::new(move || {
                let became_empty = {
                    let mut listeners = state_for_dispose.listeners.lock().unwrap();
                    listeners.retain(|entry| entry.id != id);
                    listeners.is_empty()
                };
                if became_empty {
                    if let Some(cb) = &state_for_dispose.on_last {
                        cb();
                    }
                }
            }))
        })
    }

    /// 发射事件。
    pub fn fire(&self, event: T)
    where
        T: Clone,
    {
        if self.state.disposed.load(Ordering::SeqCst) {
            return;
        }
        // 复制一份 listener 列表，允许监听过程中增删。
        let listeners: Vec<Listener<T>> = self
            .state
            .listeners
            .lock()
            .unwrap()
            .iter()
            .map(|entry| entry.listener.clone())
            .collect();
        for listener in listeners {
            listener(event.clone());
        }
    }

    /// 是否已释放。
    pub fn is_disposed(&self) -> bool {
        self.state.disposed.load(Ordering::SeqCst)
    }
}

impl<T> Default for Emitter<T>
where
    T: 'static,
{
    fn default() -> Self {
        Emitter::new()
    }
}

impl<T> Disposable for Emitter<T> {
    fn dispose(&self) {
        self.state.disposed.store(true, Ordering::SeqCst);
        self.state.listeners.lock().unwrap().clear();
    }
}

/// [`Relay`] 是一个「事件中继器」，可以动态切换输入源。
///
/// 用于 `getDelayedChannel` 中：先创建 Relay，等 channel promise resolve 后切换 input。
pub struct Relay<T> {
    emitter: Arc<Emitter<T>>,
    input_disposable: Mutex<Box<dyn Disposable>>,
}

impl<T> Relay<T> {
    pub fn new() -> Self
    where
        T: 'static + Clone,
    {
        Relay {
            emitter: Arc::new(Emitter::new()),
            input_disposable: Mutex::new(Box::new(NoopDisposable)),
        }
    }

    /// 订阅中继后的事件。
    pub fn event(&self) -> Event<T>
    where
        T: 'static,
    {
        self.emitter.event()
    }

    /// 切换输入源。
    pub fn set_input(&self, event: Event<T>)
    where
        T: 'static + Clone,
    {
        self.input_disposable.lock().unwrap().dispose();
        let emitter = self.emitter.clone();
        let d = event.subscribe(move |e: T| emitter.fire(e));
        *self.input_disposable.lock().unwrap() = d;
    }
}

impl<T> Default for Relay<T>
where
    T: 'static + Clone,
{
    fn default() -> Self {
        Relay::new()
    }
}

impl<T> Disposable for Relay<T> {
    fn dispose(&self) {
        self.input_disposable.lock().unwrap().dispose();
        self.emitter.dispose();
    }
}

/// [`EventMultiplexer`] 聚合多个事件源为一个事件。
///
/// IPCServer 的 `getMulticastEvent` 用它来聚合所有客户端的同名事件。
pub struct EventMultiplexer<T> {
    emitter: Arc<Emitter<T>>,
    disposables: Mutex<Vec<Box<dyn Disposable>>>,
}

impl<T> EventMultiplexer<T> {
    pub fn new() -> Self
    where
        T: 'static + Clone,
    {
        EventMultiplexer {
            emitter: Arc::new(Emitter::new()),
            disposables: Mutex::new(Vec::new()),
        }
    }

    pub fn event(&self) -> Event<T>
    where
        T: 'static,
    {
        self.emitter.event()
    }

    /// 加入一个事件源，返回可单独移除的句柄。
    pub fn add(&self, event: Event<T>) -> Box<dyn Disposable>
    where
        T: 'static + Clone,
    {
        let emitter = self.emitter.clone();
        let d = event.subscribe(move |e: T| emitter.fire(e));
        let shared = Arc::new(SharedDisposable(Mutex::new(Some(d))));
        self.disposables
            .lock()
            .unwrap()
            .push(Box::new(SharedDisposableRef(shared.clone())));
        Box::new(SharedDisposableRef(shared))
    }
}

impl<T> Default for EventMultiplexer<T>
where
    T: 'static + Clone,
{
    fn default() -> Self {
        EventMultiplexer::new()
    }
}

impl<T> Disposable for EventMultiplexer<T> {
    fn dispose(&self) {
        let items = std::mem::take(&mut *self.disposables.lock().unwrap());
        for d in items {
            d.dispose();
        }
        self.emitter.dispose();
    }
}

/// 可共享的释放句柄（多处持有，只释放一次）。
struct SharedDisposable(Mutex<Option<Box<dyn Disposable>>>);

impl Disposable for SharedDisposable {
    fn dispose(&self) {
        if let Some(d) = self.0.lock().unwrap().take() {
            d.dispose();
        }
    }
}

/// `SharedDisposable` 的引用形式。
struct SharedDisposableRef(Arc<SharedDisposable>);

impl Disposable for SharedDisposableRef {
    fn dispose(&self) {
        self.0.dispose();
    }
}

// ============================================================================
// CancellationToken - 取消令牌
// ============================================================================

/// 取消令牌。与 TS 一致：只读状态 + 取消事件。
pub trait CancellationToken: Send + Sync {
    fn is_cancellation_requested(&self) -> bool;
    fn on_cancellation_requested(&self) -> Event<()>;
}

/// 永不取消的令牌，对应 `CancellationToken.None`。
#[derive(Debug, Default, Clone, Copy)]
pub struct NoneCancellationToken;

impl CancellationToken for NoneCancellationToken {
    fn is_cancellation_requested(&self) -> bool {
        false
    }
    fn on_cancellation_requested(&self) -> Event<()> {
        Event::none()
    }
}

/// 便捷函数：永不取消的令牌。
pub fn cancellation_token_none() -> Arc<dyn CancellationToken> {
    Arc::new(NoneCancellationToken)
}

/// [`CancellationTokenSource`] 创建并触发取消。
///
/// 令牌与 source 共享同一个取消标志，保证 `is_cancellation_requested` 一致。
pub struct CancellationTokenSource {
    emitter: Arc<Emitter<()>>,
    is_cancelled: Arc<AtomicBool>,
}

impl CancellationTokenSource {
    pub fn new() -> Self {
        CancellationTokenSource {
            emitter: Arc::new(Emitter::new()),
            is_cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 获取令牌。
    pub fn token(&self) -> Arc<dyn CancellationToken> {
        Arc::new(SourcedToken {
            emitter: self.emitter.clone(),
            is_cancelled: self.is_cancelled.clone(),
        })
    }

    /// 触发取消。
    pub fn cancel(&self) {
        if !self.is_cancelled.swap(true, Ordering::SeqCst) {
            self.emitter.fire(());
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.is_cancelled.load(Ordering::SeqCst)
    }
}

impl Default for CancellationTokenSource {
    fn default() -> Self {
        CancellationTokenSource::new()
    }
}

impl Disposable for CancellationTokenSource {
    fn dispose(&self) {
        self.emitter.dispose();
    }
}

/// 由 source 派生的令牌，与 source 共享取消标志。
struct SourcedToken {
    emitter: Arc<Emitter<()>>,
    is_cancelled: Arc<AtomicBool>,
}

impl CancellationToken for SourcedToken {
    fn is_cancellation_requested(&self) -> bool {
        self.is_cancelled.load(Ordering::SeqCst)
    }

    fn on_cancellation_requested(&self) -> Event<()> {
        self.emitter.event()
    }
}
