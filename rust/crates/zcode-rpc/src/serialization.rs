//! Layer 1: 二进制序列化
//!
//! Rust 移植自 `packages/rpc/src/serialization.ts`。
//!
//! RPC 消息需要编码为二进制才能通过 [`IMessagePassingProtocol`] 传输。
//! 这里实现了与 VS Code 一致的自定义序列化协议：
//!
//! ```text
//! 格式: [1 byte 类型标签] [VQL 编码的长度] [数据]
//! ```
//!
//! VQL (Variable-Length Quantity) 用 7 bit 存数据，最高位标记是否还有后续字节。
//! 小数字只需 1 byte，大数字按需扩展，比固定 4 byte 更紧凑。

use crate::buffer::VSBuffer;
use crate::value::Value;
use std::collections::BTreeMap;
use std::sync::Mutex;

// ============================================================================
// Reader / Writer 接口
// ============================================================================

/// 顺序读取接口。
pub trait Reader: Send + Sync {
    fn read(&self, bytes: usize) -> VSBuffer;
}

/// 顺序写入接口。
pub trait Writer: Send + Sync {
    fn write(&self, buffer: VSBuffer);
}

/// 从一个 [`VSBuffer`] 中按顺序读取数据，内部维护游标。
pub struct BufferReader {
    buffer: VSBuffer,
    pos: Mutex<usize>,
}

impl BufferReader {
    pub fn new(buffer: VSBuffer) -> Self {
        BufferReader {
            buffer,
            pos: Mutex::new(0),
        }
    }
}

impl Reader for BufferReader {
    fn read(&self, bytes: usize) -> VSBuffer {
        let mut pos = self.pos.lock().unwrap();
        let result = self.buffer.slice(*pos, Some(*pos + bytes));
        *pos += result.byte_length();
        result
    }
}

/// 收集多个写入的 buffer，最后通过 [`BufferWriter::buffer`] 一次性拼接。
///
/// 避免写入时频繁的内存拷贝。
#[derive(Default)]
pub struct BufferWriter {
    buffers: Mutex<Vec<VSBuffer>>,
}

impl BufferWriter {
    pub fn new() -> Self {
        BufferWriter {
            buffers: Mutex::new(Vec::new()),
        }
    }

    /// 拼接已写入的所有 buffer。
    pub fn buffer(&self) -> VSBuffer {
        let buffers = self.buffers.lock().unwrap();
        VSBuffer::concat(&buffers)
    }
}

impl Writer for BufferWriter {
    fn write(&self, buffer: VSBuffer) {
        self.buffers.lock().unwrap().push(buffer);
    }
}

// ============================================================================
// VQL 编码
// ============================================================================

/// 读取 VQL 编码的无符号整数。
///
/// @see <https://en.wikipedia.org/wiki/Variable-length_quantity>
fn read_int_vql(reader: &dyn Reader) -> u32 {
    let mut value: u32 = 0;
    let mut shift: u32 = 0;
    loop {
        let next = reader.read(1);
        let byte = next.read_u8(0);
        value |= ((byte & 0b0111_1111) as u32) << shift;
        if byte & 0b1000_0000 == 0 {
            return value;
        }
        shift += 7;
        if shift >= 35 {
            // 防御：非法/超长 VQL 输入。
            return value;
        }
    }
}

/// 写入 VQL 编码的整数。
///
/// 例: 0 → `[0x00]`, 127 → `[0x7F]`, 128 → `[0x80, 0x01]`
fn write_int32_vql(writer: &dyn Writer, value: u32) {
    if value == 0 {
        writer.write(create_one_byte_buffer(0));
        return;
    }
    let mut len = 0usize;
    let mut v = value;
    while v != 0 {
        len += 1;
        v >>= 7;
    }

    let mut scratch = VSBuffer::alloc(len);
    let mut value = value;
    let mut i = 0usize;
    while value != 0 {
        scratch.write_u8((value & 0b0111_1111) as u8, i);
        value >>= 7;
        if value > 0 {
            let cur = scratch.read_u8(i);
            scratch.write_u8(cur | 0b1000_0000, i);
        }
        i += 1;
    }
    writer.write(scratch);
}

// ============================================================================
// 数据类型标签
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum DataType {
    Undefined = 0,
    String = 1,
    Buffer = 2,
    VSBuffer = 3,
    Array = 4,
    Object = 5, // JSON fallback
    Int = 6,    // VQL 编码的整数
}

fn create_one_byte_buffer(value: u8) -> VSBuffer {
    let mut result = VSBuffer::alloc(1);
    result.write_u8(value, 0);
    result
}

fn tag(data_type: DataType) -> VSBuffer {
    create_one_byte_buffer(data_type as u8)
}

// ============================================================================
// serialize / deserialize
// ============================================================================

/// 序列化任意数据到 writer。
///
/// 格式：`[1 byte 类型] [VQL 长度(如果需要)] [数据]`
///
/// 每条 RPC 消息 = `serialize(header) + serialize(body)`。
/// header 通常是 `[RequestType, id, channelName, methodName]`，
/// body 是方法参数或返回值。
pub fn serialize(writer: &dyn Writer, data: &Value) {
    match data {
        Value::Undefined => {
            writer.write(tag(DataType::Undefined));
        }
        Value::String(s) => {
            let buffer = VSBuffer::from_string(s);
            writer.write(tag(DataType::String));
            write_int32_vql(writer, buffer.byte_length() as u32);
            writer.write(buffer);
        }
        Value::VSBuffer(buf) => {
            writer.write(tag(DataType::VSBuffer));
            write_int32_vql(writer, buf.byte_length() as u32);
            writer.write(buf.clone());
        }
        Value::Buffer(bytes) => {
            let buffer = VSBuffer::wrap(bytes.clone());
            writer.write(tag(DataType::Buffer));
            write_int32_vql(writer, buffer.byte_length() as u32);
            writer.write(buffer);
        }
        Value::Array(items) => {
            writer.write(tag(DataType::Array));
            write_int32_vql(writer, items.len() as u32);
            for el in items {
                serialize(writer, el);
            }
        }
        Value::Int(n) => {
            // 整数用 VQL 编码，比 JSON 更紧凑。按 32 位截断，与 TS 的 `| 0` 语义一致。
            writer.write(tag(DataType::Int));
            write_int32_vql(writer, *n as u32);
        }
        // 其余（Bool / Null / Float / Object）走 JSON 回退分支。
        other => {
            let buffer = VSBuffer::from_string(&crate::value::value_to_json(other));
            writer.write(tag(DataType::Object));
            write_int32_vql(writer, buffer.byte_length() as u32);
            writer.write(buffer);
        }
    }
}

/// 从 reader 反序列化数据。
pub fn deserialize(reader: &dyn Reader) -> Value {
    let type_byte = reader.read(1).read_u8(0);

    match type_byte {
        0 => Value::Undefined,
        1 => {
            let len = read_int_vql(reader) as usize;
            Value::String(reader.read(len).to_string())
        }
        2 => {
            let len = read_int_vql(reader) as usize;
            Value::Buffer(reader.read(len).into_bytes())
        }
        3 => {
            let len = read_int_vql(reader) as usize;
            Value::VSBuffer(reader.read(len))
        }
        4 => {
            let length = read_int_vql(reader) as usize;
            let mut result = Vec::with_capacity(length);
            for _ in 0..length {
                result.push(deserialize(reader));
            }
            Value::Array(result)
        }
        5 => {
            let len = read_int_vql(reader) as usize;
            let text = reader.read(len).to_string();
            crate::value::json_to_value(&text).unwrap_or(Value::Null)
        }
        6 => Value::Int(read_int_vql(reader) as i64),
        // 未知标签：返回 Undefined（容错）。
        _ => Value::Undefined,
    }
}

// ============================================================================
// 便捷构造：把常见 Rust 类型转成 Value
// ============================================================================

impl From<String> for Value {
    fn from(value: String) -> Self {
        Value::String(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::String(value.to_string())
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Value::Int(value)
    }
}

impl From<i32> for Value {
    fn from(value: i32) -> Self {
        Value::Int(value as i64)
    }
}

impl From<u32> for Value {
    fn from(value: u32) -> Self {
        Value::Int(value as i64)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Value::Bool(value)
    }
}

impl From<Vec<Value>> for Value {
    fn from(value: Vec<Value>) -> Self {
        Value::Array(value)
    }
}

impl From<BTreeMap<String, Value>> for Value {
    fn from(value: BTreeMap<String, Value>) -> Self {
        Value::Object(value)
    }
}

impl From<VSBuffer> for Value {
    fn from(value: VSBuffer) -> Self {
        Value::VSBuffer(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(value: Value) -> Value {
        let writer = BufferWriter::new();
        serialize(&writer, &value);
        let reader = BufferReader::new(writer.buffer());
        deserialize(&reader)
    }

    #[test]
    fn vql_roundtrip_small_and_large() {
        for n in [0i64, 1, 127, 128, 300, 100000, 4_000_000_000] {
            assert_eq!(roundtrip(Value::Int(n)), Value::Int(n), "n={n}");
        }
    }

    #[test]
    fn string_and_array_roundtrip() {
        let v = Value::Array(vec![
            Value::String("hello".into()),
            Value::Int(42),
            Value::Undefined,
            Value::Array(vec![Value::Int(1), Value::Int(2)]),
        ]);
        assert_eq!(roundtrip(v.clone()), v);
    }

    #[test]
    fn nested_bytes_survive_object_json_fallback() {
        let mut map = BTreeMap::new();
        map.insert("payload".to_string(), Value::Buffer(vec![1, 2, 3, 250, 255]));
        map.insert("name".to_string(), Value::String("x".into()));
        let v = Value::Object(map);
        let out = roundtrip(v.clone());
        // Buffer 应被还原为 Buffer（而非被 JSON 展开成对象）。
        match out {
            Value::Object(m) => {
                assert_eq!(m.get("payload"), Some(&Value::Buffer(vec![1, 2, 3, 250, 255])));
            }
            other => panic!("期望 Object，得到 {other:?}"),
        }
    }

    #[test]
    fn base64_roundtrip() {
        for bytes in [vec![], vec![1], vec![1, 2], vec![1, 2, 3], vec![255; 100]] {
            let b64 = crate::value::bytes_to_base64(&bytes);
            assert_eq!(crate::value::base64_to_bytes(&b64), bytes);
        }
    }
}
