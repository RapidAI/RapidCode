//! Layer 0.5: 跨平台 Buffer 抽象
//!
//! Rust 移植自 `packages/rpc/src/buffer.ts`。
//!
//! 原实现是对 `Uint8Array` 的薄封装，用于在 Node.js (Buffer) 和浏览器
//! (Uint8Array) 之间统一二进制操作。Rust 侧用 `Vec<u8>` 承载，
//! 提供完全一致的读写接口，作为序列化层与传输层的基础。

/// 对字节序列的薄封装，对应 TS 的 `VSBuffer`。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VSBuffer {
    buffer: Vec<u8>,
}

impl VSBuffer {
    /// 分配指定大小的空 buffer。
    pub fn alloc(byte_length: usize) -> Self {
        VSBuffer {
            buffer: vec![0u8; byte_length],
        }
    }

    /// 包装已有的字节序列。
    pub fn wrap(buffer: Vec<u8>) -> Self {
        VSBuffer { buffer }
    }

    /// 从字符串创建 buffer (UTF-8)。
    pub fn from_string(s: &str) -> Self {
        VSBuffer {
            buffer: s.as_bytes().to_vec(),
        }
    }

    /// 拼接多个 buffer。
    pub fn concat(buffers: &[VSBuffer]) -> Self {
        let total: usize = buffers.iter().map(|b| b.byte_length()).sum();
        let mut out = Vec::with_capacity(total);
        for b in buffers {
            out.extend_from_slice(&b.buffer);
        }
        VSBuffer { buffer: out }
    }

    /// 底层字节只读视图。
    pub fn as_bytes(&self) -> &[u8] {
        &self.buffer
    }

    /// 消耗自身，取出底层字节。
    pub fn into_bytes(self) -> Vec<u8> {
        self.buffer
    }

    /// 字节长度。
    pub fn byte_length(&self) -> usize {
        self.buffer.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// 转为 UTF-8 字符串（非法字节用 U+FFFD 替换）。
    pub fn to_string(&self) -> String {
        String::from_utf8_lossy(&self.buffer).into_owned()
    }

    /// 切片 `[start, end)`。`end` 为 `None` 时切到末尾。
    pub fn slice(&self, start: usize, end: Option<usize>) -> VSBuffer {
        let len = self.buffer.len();
        let end = end.unwrap_or(len).min(len);
        let start = start.min(end);
        VSBuffer {
            buffer: self.buffer[start..end].to_vec(),
        }
    }

    /// 拷贝数据到本 buffer 的指定位置。
    pub fn set(&mut self, source: &[u8], offset: usize) {
        let end = offset + source.len();
        if end > self.buffer.len() {
            self.buffer.resize(end, 0);
        }
        self.buffer[offset..end].copy_from_slice(source);
    }

    /// 读取 1 字节。
    pub fn read_u8(&self, offset: usize) -> u8 {
        self.buffer[offset]
    }

    /// 写入 1 字节。
    pub fn write_u8(&mut self, value: u8, offset: usize) {
        self.buffer[offset] = value;
    }

    /// 大端读取 32 位无符号整数。
    pub fn read_u32_be(&self, offset: usize) -> u32 {
        ((self.buffer[offset] as u32) << 24)
            | ((self.buffer[offset + 1] as u32) << 16)
            | ((self.buffer[offset + 2] as u32) << 8)
            | (self.buffer[offset + 3] as u32)
    }

    /// 大端写入 32 位无符号整数。
    pub fn write_u32_be(&mut self, value: u32, offset: usize) {
        self.buffer[offset] = ((value >> 24) & 0xff) as u8;
        self.buffer[offset + 1] = ((value >> 16) & 0xff) as u8;
        self.buffer[offset + 2] = ((value >> 8) & 0xff) as u8;
        self.buffer[offset + 3] = (value & 0xff) as u8;
    }
}

impl From<&[u8]> for VSBuffer {
    fn from(value: &[u8]) -> Self {
        VSBuffer::wrap(value.to_vec())
    }
}

impl From<Vec<u8>> for VSBuffer {
    fn from(value: Vec<u8>) -> Self {
        VSBuffer::wrap(value)
    }
}

impl From<&str> for VSBuffer {
    fn from(value: &str) -> Self {
        VSBuffer::from_string(value)
    }
}
