# ZCode Rust 核心逻辑（`rust/` 工作区）

本目录是 **RapidCode / ZCode 核心逻辑层的 Rust 移植**，对应 `main` 分支里的
TypeScript 源码。目标是：把项目的核心业务逻辑用 Rust 重写，并保证 **可编译、可测试**。

> 完整范围说明、逐文件映射与剩余工作，请先阅读 **[PORTING.md](./PORTING.md)**。

## 快速开始

```bash
cd rust
cargo build          # 编译整个工作区
cargo test           # 运行全部单元 / 集成测试
cargo build --release
```

> 本工作区 **零外部依赖**（只用 `std`），因此离线也能编译：
> `CARGO_NET_OFFLINE=true cargo build` 同样通过。

## 工作区结构

```
rust/
├── Cargo.toml                      # workspace 根
└── crates/
    ├── zcode-rpc/                  # ← 核心：VS Code 风格 IPC/RPC 框架（完整移植）
    │   └── src/
    │       ├── foundation.rs            Layer 0  Event / Emitter / Disposable / CancellationToken
    │       ├── buffer.rs                Layer 0.5 VSBuffer
    │       ├── value.rs                 动态值 + JSON/Base64 编解码
    │       ├── error.rs                 RpcError
    │       ├── promise.rs               回调式 Promise（零依赖异步）
    │       ├── serialization.rs         Layer 1  VQL 序列化
    │       ├── protocol.rs              Layer 2  IMessagePassingProtocol / SocketProtocol / ChunkStream
    │       ├── persistent_protocol.rs   Layer 2+ ACK / 心跳 / 重放 / 流控
    │       ├── channels_shared.rs       Layer 3  通道契约
    │       ├── channel_server.rs        Layer 3  ChannelServer
    │       ├── channel_client.rs        Layer 3  ChannelClient
    │       ├── delayed_channel.rs       Layer 3  延迟通道
    │       ├── ipc.rs                   Layer 4  IPCServer / IPCClient / StaticRouter
    │       ├── proxy_channel.rs         Layer 5  ProxyChannel
    │       ├── remote.rs                Layer 6  远程连接
    │       ├── logging_middleware.rs    中间件：RPC 日志
    │       └── network_telemetry_middleware.rs  中间件：网络遥测
    └── zcode-shared/               # 共享核心逻辑（部分移植）
        └── src/
            ├── line_change_stat.rs       行级 LCS 变更统计
            ├── model_provider_family.rs  模型供应商族归一 / 可见性
            └── plan_identity.rs          套餐身份解析
```

## 设计要点

| 主题 | TypeScript 原实现 | Rust 移植 |
| --- | --- | --- |
| 事件系统 | `Event<T>` = 回调函数 | `Event<T>` 结构体（`Arc<dyn Fn>` 订阅） |
| 异步结果 | 原生 `Promise` | 零依赖回调式 `Promise<T>`（`Mutex` + `Condvar`） |
| 动态载荷 | `any` | `Value` 枚举（含 JSON / Base64 编解码） |
| 定时器 | `setTimeout` / `setInterval` | `std::thread` + `sleep` + 停止标志 |
| 传输 | 接口 | `MessagePassingProtocol` trait；`create_queue_pair` 供测试 |
| 上下文 | 泛型 `TContext` | 统一用 `Value` 承载（本身可序列化） |

## 测试

```
cargo test
```

覆盖：VQL 序列化往返、嵌套 `Uint8Array` 的 JSON 回退、Base64、
事件缓冲、以及基于内存队列（`create_queue_pair`）的 **端到端 RPC 调用 / 事件订阅**。
