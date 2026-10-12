//! Promise —— 回调式异步结果
//!
//! TS 的 RPC 框架基于 `Promise`。Rust 标准库没有异步运行时，
//! 为了保证「零依赖即可编译」，这里用一个**回调式 Promise** 承载异步结果：
//!
//! - 注册回调后，一旦结果落定，回调在**触发落定的线程**上同步执行；
//! - 不依赖任何 executor，语义与 JS Promise 的 `then` 一致；
//! - 额外提供阻塞式 [`Promise::wait`]，便于测试与同步调用方。
//!
//! 这既保持了原框架「Promise 一旦 settle 就通知所有 handler」的行为，
//! 又完全用 `std`（`Mutex` + `Condvar`）实现。

use crate::error::RpcError;
use std::sync::{Arc, Condvar, Mutex};

type SettleCallback<T> = Box<dyn FnOnce(Result<T, RpcError>) + Send>;

enum PromiseState<T> {
    Pending(Vec<SettleCallback<T>>),
    Settled(Result<T, RpcError>),
}

struct PromiseInner<T> {
    state: Mutex<PromiseState<T>>,
    condvar: Condvar,
}

/// 异步结果句柄。
pub struct Promise<T> {
    inner: Arc<PromiseInner<T>>,
}

impl<T> Clone for Promise<T> {
    fn clone(&self) -> Self {
        Promise {
            inner: self.inner.clone(),
        }
    }
}

impl<T: Clone + Send + Sync + 'static> Promise<T> {
    /// 创建一对 (resolver, promise)。
    pub fn new() -> (PromiseResolver<T>, Promise<T>) {
        let inner = Arc::new(PromiseInner {
            state: Mutex::new(PromiseState::Pending(Vec::new())),
            condvar: Condvar::new(),
        });
        (
            PromiseResolver {
                inner: inner.clone(),
            },
            Promise { inner },
        )
    }

    /// 已成功落定的 Promise。
    pub fn resolved(value: T) -> Self {
        let inner = Arc::new(PromiseInner {
            state: Mutex::new(PromiseState::Settled(Ok(value))),
            condvar: Condvar::new(),
        });
        Promise { inner }
    }

    /// 已失败落定的 Promise。
    pub fn rejected(error: RpcError) -> Self {
        let inner = Arc::new(PromiseInner {
            state: Mutex::new(PromiseState::Settled(Err(error))),
            condvar: Condvar::new(),
        });
        Promise { inner }
    }

    /// 注册落定回调（成功 / 失败都会触发）。
    pub fn on_settled<F>(&self, callback: F)
    where
        F: FnOnce(Result<T, RpcError>) + Send + 'static,
    {
        let mut state = self.inner.state.lock().unwrap();
        match &mut *state {
            PromiseState::Pending(callbacks) => callbacks.push(Box::new(callback)),
            PromiseState::Settled(result) => {
                let result = result.clone();
                drop(state);
                callback(result);
            }
        }
    }

    /// 映射成功值，返回新的 Promise。
    pub fn map<U, F>(&self, f: F) -> Promise<U>
    where
        U: Clone + Send + Sync + 'static,
        F: Fn(T) -> U + Send + Sync + 'static,
    {
        self.then(move |v| Ok(f(v)))
    }

    /// `then`：成功时经 `f` 变换，失败时把错误透传给结果 Promise。
    pub fn then<U, F>(&self, f: F) -> Promise<U>
    where
        U: Clone + Send + Sync + 'static,
        F: Fn(T) -> Result<U, RpcError> + Send + Sync + 'static,
    {
        let (resolver, promise) = Promise::<U>::new();
        let f = Arc::new(f);
        self.on_settled(move |result| match result {
            Ok(v) => match f(v) {
                Ok(u) => resolver.resolve(u),
                Err(e) => resolver.reject(e),
            },
            Err(e) => resolver.reject(e),
        });
        promise
    }

    /// 阻塞等待结果（供同步调用方与测试使用）。
    pub fn wait(&self) -> Result<T, RpcError> {
        let mut state = self.inner.state.lock().unwrap();
        loop {
            match &*state {
                PromiseState::Settled(result) => return result.clone(),
                PromiseState::Pending(_) => {
                    state = self.inner.condvar.wait(state).unwrap();
                }
            }
        }
    }

    /// 是否已落定。
    pub fn is_settled(&self) -> bool {
        matches!(&*self.inner.state.lock().unwrap(), PromiseState::Settled(_))
    }
}

/// Promise 落定控制器。
pub struct PromiseResolver<T> {
    inner: Arc<PromiseInner<T>>,
}

impl<T: Clone> PromiseResolver<T> {
    /// 以成功值落定。
    pub fn resolve(&self, value: T) {
        self.settle(Ok(value));
    }

    /// 以错误落定。
    pub fn reject(&self, error: RpcError) {
        self.settle(Err(error));
    }

    fn settle(&self, result: Result<T, RpcError>) {
        let callbacks = {
            let mut state = self.inner.state.lock().unwrap();
            match &mut *state {
                PromiseState::Pending(_) => {
                    let previous = std::mem::replace(&mut *state, PromiseState::Settled(result));
                    match previous {
                        PromiseState::Pending(callbacks) => callbacks,
                        PromiseState::Settled(_) => unreachable!(),
                    }
                }
                // 已落定：忽略重复落定（Promise 语义：只认第一次）。
                PromiseState::Settled(_) => return,
            }
        };
        self.inner.condvar.notify_all();
        let settled = self.inner.state.lock().unwrap();
        let snapshot = match &*settled {
            PromiseState::Settled(r) => r.clone(),
            PromiseState::Pending(_) => unreachable!(),
        };
        drop(settled);
        for callback in callbacks {
            callback(snapshot.clone());
        }
    }
}

impl<T> Clone for PromiseResolver<T> {
    fn clone(&self) -> Self {
        PromiseResolver {
            inner: self.inner.clone(),
        }
    }
}
