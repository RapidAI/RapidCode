//! 通道契约 (Channels Shared)
//!
//! Rust 移植自 `packages/rpc/src/channels.shared.ts`。
//!
//! 定义 RPC 的请求/响应类型、通道接口。
//!
//! 说明：TS 的 `TContext` 泛型在 Rust 侧统一用 [`Value`] 承载
//! （上下文本身是可序列化的动态数据）。

use crate::error::RpcError;
use crate::foundation::{CancellationToken, Event};
use crate::promise::Promise;
use crate::value::Value;
use std::sync::Arc;

/// 客户端通道：调用命令 / 监听事件。
pub trait Channel: Send + Sync {
    /// 调用远端命令。
    fn call(&self, command: &str, arg: Value, token: Arc<dyn CancellationToken>) -> Promise<Value>;
    /// 监听远端事件。
    fn listen(&self, event: &str, arg: Value) -> Event<Value>;
}

/// 服务端通道：处理来自客户端的命令 / 事件订阅。
pub trait ServerChannel: Send + Sync {
    fn call(&self, ctx: &Value, command: &str, arg: Value, token: Arc<dyn CancellationToken>) -> Promise<Value>;
    fn listen(&self, ctx: &Value, event: &str, arg: Value) -> Event<Value>;
}

/// 客户端请求类型（对应 TS 的 `RequestType`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i64)]
pub enum RequestType {
    Promise = 100,
    PromiseCancel = 101,
    EventListen = 102,
    EventDispose = 103,
}

impl RequestType {
    pub fn from_i64(value: i64) -> Option<RequestType> {
        match value {
            100 => Some(RequestType::Promise),
            101 => Some(RequestType::PromiseCancel),
            102 => Some(RequestType::EventListen),
            103 => Some(RequestType::EventDispose),
            _ => None,
        }
    }
}

/// 服务端响应类型（对应 TS 的 `ResponseType`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i64)]
pub enum ResponseType {
    Initialize = 200,
    PromiseSuccess = 201,
    PromiseError = 202,
    PromiseErrorObj = 203,
    EventFire = 204,
}

impl ResponseType {
    pub fn from_i64(value: i64) -> Option<ResponseType> {
        match value {
            200 => Some(ResponseType::Initialize),
            201 => Some(ResponseType::PromiseSuccess),
            202 => Some(ResponseType::PromiseError),
            203 => Some(ResponseType::PromiseErrorObj),
            204 => Some(ResponseType::EventFire),
            _ => None,
        }
    }
}

/// 原始响应。
#[derive(Clone, Debug)]
pub enum RawResponse {
    Initialize,
    PromiseSuccess { id: u32, data: Value },
    PromiseError { id: u32, data: RpcError },
    PromiseErrorObj { id: u32, data: Value },
    EventFire { id: u32, data: Value },
}

impl RawResponse {
    pub fn response_type(&self) -> ResponseType {
        match self {
            RawResponse::Initialize => ResponseType::Initialize,
            RawResponse::PromiseSuccess { .. } => ResponseType::PromiseSuccess,
            RawResponse::PromiseError { .. } => ResponseType::PromiseError,
            RawResponse::PromiseErrorObj { .. } => ResponseType::PromiseErrorObj,
            RawResponse::EventFire { .. } => ResponseType::EventFire,
        }
    }

    pub fn id(&self) -> Option<u32> {
        match self {
            RawResponse::Initialize => None,
            RawResponse::PromiseSuccess { id, .. }
            | RawResponse::PromiseError { id, .. }
            | RawResponse::PromiseErrorObj { id, .. }
            | RawResponse::EventFire { id, .. } => Some(*id),
        }
    }
}

/// 响应处理器。
pub type Handler = Arc<dyn Fn(&RawResponse) + Send + Sync>;

/// 服务端：注册通道。
pub trait ChannelServerApi: Send + Sync {
    fn register_channel(&self, channel_name: &str, channel: Arc<dyn ServerChannel>);
    fn ready(&self) {}
}

/// 客户端：按名字获取通道。
pub trait ChannelClientApi: Send + Sync {
    fn get_channel(&self, channel_name: &str) -> Arc<dyn Channel>;
}
