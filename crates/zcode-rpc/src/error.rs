//! RPC 错误模型
//!
//! 对应 `channels.shared.ts` 里 `ResponseType::PromiseError` 携带的
//! `{ message, name, stack, code?, kind?, status?, ... }` 结构。
//!
//! TS 里错误对象可以挂任意附加字段并在跨进程传输时透传；Rust 侧用
//! [`RpcError::extras`] 承载这些「透传键」。

use crate::value::Value;
use std::collections::BTreeMap;

/// 跨进程透传的错误附加字段名（与 TS 端 `passthroughKeys` 保持一致）。
pub const PASSTHROUGH_KEYS: [&str; 9] = [
    "code",
    "kind",
    "status",
    "retryAfterMs",
    "data",
    "detail",
    "details",
    "taskId",
    "traceId",
];

/// RPC 错误。
#[derive(Clone, Debug, PartialEq)]
pub struct RpcError {
    pub message: String,
    pub name: String,
    pub stack: Option<Vec<String>>,
    /// 透传附加字段（code / kind / status / ...）。
    pub extras: BTreeMap<String, Value>,
}

impl RpcError {
    /// 以消息构造，name 默认 `Error`。
    pub fn new(message: impl Into<String>) -> Self {
        RpcError {
            message: message.into(),
            name: "Error".to_string(),
            stack: None,
            extras: BTreeMap::new(),
        }
    }

    /// 指定 name。
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// 指定堆栈。
    pub fn with_stack(mut self, stack: Vec<String>) -> Self {
        self.stack = Some(stack);
        self
    }

    /// 追加一个透传字段。
    pub fn with_extra(mut self, key: impl Into<String>, value: Value) -> Self {
        self.extras.insert(key.into(), value);
        self
    }

    /// 序列化为 [`Value::Object`]，用于 `ResponseType::PromiseError`。
    pub fn to_value(&self) -> Value {
        let mut map = BTreeMap::new();
        map.insert("message".to_string(), Value::String(self.message.clone()));
        map.insert("name".to_string(), Value::String(self.name.clone()));
        map.insert(
            "stack".to_string(),
            match &self.stack {
                Some(lines) => Value::Array(lines.iter().cloned().map(Value::String).collect()),
                None => Value::Undefined,
            },
        );
        for (k, v) in &self.extras {
            map.insert(k.clone(), v.clone());
        }
        Value::Object(map)
    }

    /// 从反序列化得到的 [`Value`] 还原。
    pub fn from_value(value: &Value) -> Self {
        let map = value.as_object();
        let get_str = |key: &str| -> Option<String> {
            map.and_then(|m| m.get(key)).and_then(|v| v.as_str()).map(String::from)
        };
        let message = get_str("message").unwrap_or_default();
        let name = get_str("name").unwrap_or_else(|| "Error".to_string());
        let stack = map
            .and_then(|m| m.get("stack"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>()
            });
        let mut extras = BTreeMap::new();
        if let Some(m) = map {
            for key in PASSTHROUGH_KEYS {
                if let Some(v) = m.get(key) {
                    if !v.is_undefined() {
                        extras.insert(key.to_string(), v.clone());
                    }
                }
            }
        }
        RpcError {
            message,
            name,
            stack,
            extras,
        }
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.name, self.message)
    }
}

impl std::error::Error for RpcError {}

impl From<String> for RpcError {
    fn from(value: String) -> Self {
        RpcError::new(value)
    }
}

impl From<&str> for RpcError {
    fn from(value: &str) -> Self {
        RpcError::new(value)
    }
}
