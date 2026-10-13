//! 动态数据模型 (Dynamic Value)
//!
//! RPC 载荷是动态类型的（对应 TS 里的 `any`）。Rust 是静态类型语言，
//! 因此这里用一个 [`Value`] 枚举承载所有可序列化的数据形态，
//! 与 `serialization.ts` 中的数据类型标签一一对应。
//!
//! 另外附带一个零依赖的 JSON 编解码器与 Base64 实现，用于
//! `DataType::Object` 分支（原 TS 实现里用 `JSON.stringify` / `JSON.parse`）。

use crate::buffer::VSBuffer;
use std::collections::BTreeMap;

/// 动态数据值。变体顺序与 `serialization.ts` 的 `DataType` 对齐。
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// 未定义（DataType::Undefined = 0）
    Undefined,
    /// 字符串（DataType::String = 1）
    String(String),
    /// 原始字节（DataType::Buffer = 2）
    Buffer(Vec<u8>),
    /// VSBuffer（DataType::VSBuffer = 3）
    VSBuffer(VSBuffer),
    /// 数组（DataType::Array = 4）
    Array(Vec<Value>),
    /// 对象（DataType::Object = 5，走 JSON 回退）
    Object(BTreeMap<String, Value>),
    /// 整数（DataType::Int = 6，VQL 编码）
    Int(i64),
    /// JSON 回退分支里出现的布尔
    Bool(bool),
    /// JSON 回退分支里出现的空值
    Null,
    /// JSON 回退分支里出现的浮点
    Float(f64),
}

impl Value {
    /// 便捷构造：从 `&str`。
    pub fn str(s: impl Into<String>) -> Value {
        Value::String(s.into())
    }

    /// 便捷构造：从字节。
    pub fn buffer(bytes: impl Into<Vec<u8>>) -> Value {
        Value::Buffer(bytes.into())
    }

    /// 是否为「未定义」。
    pub fn is_undefined(&self) -> bool {
        matches!(self, Value::Undefined)
    }

    /// 取出字符串引用（若为字符串）。
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// 取出整数（若为整数）。
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }

    /// 取出数组引用（若为数组）。
    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    /// 取出对象引用（若为对象）。
    pub fn as_object(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }
}

// ============================================================================
// 嵌套 Uint8Array 标记（与 TS 端协议保持一致）
// ============================================================================

const RPC_NESTED_UINT8_ARRAY_MARKER: &str = "__zcode_rpc_nested_uint8array_v1";
const RPC_NESTED_UINT8_ARRAY_BASE64_KEY: &str = "base64";

// ============================================================================
// Base64
// ============================================================================

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// 标准 Base64 编码。
pub fn bytes_to_base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(BASE64_ALPHABET[((triple >> 18) & 0x3f) as usize] as char);
        out.push(BASE64_ALPHABET[((triple >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(BASE64_ALPHABET[((triple >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(BASE64_ALPHABET[(triple & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// 标准 Base64 解码。
pub fn base64_to_bytes(s: &str) -> Vec<u8> {
    let mut lookup = [255u8; 256];
    for (i, &c) in BASE64_ALPHABET.iter().enumerate() {
        lookup[c as usize] = i as u8;
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for &c in s.as_bytes() {
        if c == b'=' {
            break;
        }
        let v = lookup[c as usize];
        if v == 255 {
            continue;
        }
        buffer = (buffer << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    out
}

// ============================================================================
// JSON 编码（对应 TS 的 JSON.stringify + replacer）
// ============================================================================

/// 把 [`Value`] 编码为 JSON 字符串。
///
/// 嵌套的 [`Value::Buffer`] / [`Value::VSBuffer`] 会被编码为带标记的对象，
/// 与 TS 端 `encodeRpcJsonValue` 行为一致。
pub fn value_to_json(value: &Value) -> String {
    let mut out = String::new();
    write_json(value, &mut out);
    out
}

fn write_json(value: &Value, out: &mut String) {
    match value {
        Value::Undefined | Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(n) => out.push_str(&n.to_string()),
        Value::Float(f) => write_json_number(*f, out),
        Value::String(s) => write_json_string(s, out),
        Value::Buffer(bytes) => write_nested_buffer_marker(bytes, out),
        Value::VSBuffer(buf) => write_nested_buffer_marker(buf.as_bytes(), out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (i, (k, v)) in map.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json_string(k, out);
                out.push(':');
                write_json(v, out);
            }
            out.push('}');
        }
    }
}

fn write_json_number(f: f64, out: &mut String) {
    if f.is_finite() {
        // 整数形式保留整数写法。
        if f.fract() == 0.0 && f.abs() < 1e15 {
            out.push_str(&(f as i64).to_string());
        } else {
            out.push_str(&format!("{f}"));
        }
    } else {
        // JSON 不支持 NaN/Infinity，回退为 null。
        out.push_str("null");
    }
}

fn write_json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_nested_buffer_marker(bytes: &[u8], out: &mut String) {
    out.push('{');
    write_json_string(RPC_NESTED_UINT8_ARRAY_MARKER, out);
    out.push_str(":true,");
    write_json_string(RPC_NESTED_UINT8_ARRAY_BASE64_KEY, out);
    out.push(':');
    write_json_string(&bytes_to_base64(bytes), out);
    out.push('}');
}

// ============================================================================
// JSON 解码（对应 TS 的 JSON.parse + reviver）
// ============================================================================

/// 解析 JSON 字符串为 [`Value`]，并恢复嵌套的字节标记。
pub fn json_to_value(s: &str) -> Result<Value, String> {
    let mut parser = JsonParser {
        bytes: s.as_bytes(),
        pos: 0,
    };
    parser.skip_ws();
    let v = parser.parse_value()?;
    parser.skip_ws();
    Ok(v)
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn parse_value(&mut self) -> Result<Value, String> {
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b'"') => Ok(Value::String(self.parse_string()?)),
            Some(b't') => self.parse_literal("true", Value::Bool(true)),
            Some(b'f') => self.parse_literal("false", Value::Bool(false)),
            Some(b'n') => self.parse_literal("null", Value::Null),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.parse_number(),
            _ => Err(format!("JSON 解析错误：位置 {}", self.pos)),
        }
    }

    fn parse_literal(&mut self, lit: &str, value: Value) -> Result<Value, String> {
        if self.bytes[self.pos..].starts_with(lit.as_bytes()) {
            self.pos += lit.len();
            Ok(value)
        } else {
            Err(format!("JSON 字面量错误：位置 {}", self.pos))
        }
    }

    fn parse_number(&mut self) -> Result<Value, String> {
        let start = self.pos;
        let mut is_float = false;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while let Some(c) = self.peek() {
            match c {
                b'0'..=b'9' => self.pos += 1,
                b'.' | b'e' | b'E' | b'+' | b'-' => {
                    is_float = true;
                    self.pos += 1;
                }
                _ => break,
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap_or("");
        if is_float {
            text.parse::<f64>()
                .map(Value::Float)
                .map_err(|e| e.to_string())
        } else {
            text.parse::<i64>()
                .map(Value::Int)
                .map_err(|e| e.to_string())
        }
    }

    fn parse_string(&mut self) -> Result<String, String> {
        // 跳过开头引号
        self.pos += 1;
        let mut out = String::new();
        while let Some(c) = self.peek() {
            self.pos += 1;
            match c {
                b'"' => return Ok(out),
                b'\\' => {
                    let esc = self.peek().ok_or("JSON 转义不完整")?;
                    self.pos += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'b' => out.push('\u{08}'),
                        b'f' => out.push('\u{0c}'),
                        b'u' => {
                            let hex = self
                                .bytes
                                .get(self.pos..self.pos + 4)
                                .ok_or("JSON unicode 转义不完整")?;
                            let code = u32::from_str_radix(
                                std::str::from_utf8(hex).unwrap_or(""),
                                16,
                            )
                            .map_err(|e| e.to_string())?;
                            self.pos += 4;
                            if let Some(ch) = char::from_u32(code) {
                                out.push(ch);
                            }
                        }
                        other => out.push(other as char),
                    }
                }
                _ => {
                    // UTF-8 多字节：重新组装
                    let start = self.pos - 1;
                    let extra = utf8_extra_bytes(c);
                    self.pos += extra;
                    if let Ok(s) = std::str::from_utf8(&self.bytes[start..self.pos]) {
                        out.push_str(s);
                    }
                }
            }
        }
        Err("JSON 字符串未闭合".to_string())
    }

    fn parse_array(&mut self) -> Result<Value, String> {
        self.pos += 1; // '['
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Value::Array(items));
        }
        loop {
            let v = self.parse_value()?;
            items.push(v);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err("JSON 数组格式错误".to_string()),
            }
        }
    }

    fn parse_object(&mut self) -> Result<Value, String> {
        self.pos += 1; // '{'
        let mut map = BTreeMap::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Object(map));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err("JSON 对象键必须是字符串".to_string());
            }
            let key = self.parse_string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err("JSON 对象缺少冒号".to_string());
            }
            self.pos += 1;
            let value = self.parse_value()?;
            map.insert(key, value);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(restore_nested_uint8_array(map));
                }
                _ => return Err("JSON 对象格式错误".to_string()),
            }
        }
    }
}

fn utf8_extra_bytes(first: u8) -> usize {
    if first < 0x80 {
        0
    } else if first >> 5 == 0b110 {
        1
    } else if first >> 4 == 0b1110 {
        2
    } else if first >> 3 == 0b11110 {
        3
    } else {
        0
    }
}

/// 识别并还原 `{marker: true, base64: "..."}` 形态的嵌套字节。
fn restore_nested_uint8_array(mut map: BTreeMap<String, Value>) -> Value {
    if map.len() == 2 {
        let is_marker = matches!(
            map.get(RPC_NESTED_UINT8_ARRAY_MARKER),
            Some(Value::Bool(true))
        );
        let base64 = map
            .get(RPC_NESTED_UINT8_ARRAY_BASE64_KEY)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if is_marker {
            if let Some(b64) = base64 {
                return Value::Buffer(base64_to_bytes(&b64));
            }
        }
    }
    // 清掉潜在的借用告警（保持原对象）
    let _ = &mut map;
    Value::Object(map)
}
