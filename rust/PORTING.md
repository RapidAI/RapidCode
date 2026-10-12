# TypeScript → Rust 移植说明（PORTING.md）

本文件说明 `rust/` 分支上 Rust 移植的**范围、映射、设计取舍与剩余工作**。

> UI 层的移植（`packages/ui` 的 shadcn 风格原语 → Rust + Dioxus）
> 单独写在 **[PORTING-UI.md](./PORTING-UI.md)**。

---

## 1. 范围现实（务必先读）

`main` 分支是一个完整产品（ZCode：桌面端 + 浏览器端 + 服务端 + 终端 Agent）。  
其 TypeScript 规模如下（`node_modules`、`.git` 除外）：

| 目录                  |              行数 | 说明                                  |
| ------------------- | --------------: | ----------------------------------- |
| `packages/ui`       |         663,605 | React 前端（.tsx / .ts）→ 已用 Dioxus 移植原语子集 |
| `apps/zcode-cli`    |         691,712 | 终端 Agent 应用                         |
| `packages/desktop`  |         299,419 | Electron 桌面端（含主进程与 UI）              |
| `packages/services` |         224,275 | 服务层业务逻辑                             |
| `packages/shared`   |          82,754 | 共享类型 / 逻辑                           |
| 其余包                 |         ~90,000 | server / provider / rpc / web / …   |
| **合计**              | **≈ 2,057,000** | 8,462 个 `.ts` / `.tsx` 文件           |

**结论**：把约 205 万行 TypeScript（其中 66 万行是 React UI）在单次迭代内做  
1:1 全量重写，并保证编译通过，在工程上不可行。

因此本分支采取**务实、可验证**的策略：

> **先完整移植项目的「逻辑骨架」——IPC/RPC 框架（`packages/rpc`），  
> 再逐步迁移其它纯逻辑模块（`packages/shared`），  
> 并为 UI 层选定 Rust 等价框架（Dioxus）从原语组件开始迁移。**

产出保证：`cargo build` 与 `cargo test` 全部通过。

---

## 2. 已完成移植

### 2.1 `crates/zcode-rpc` —— 完整移植 `packages/rpc`

`packages/rpc` 是整个产品的通信骨架（VS Code 风格 IPC 框架，3,295 行 TS）。  
本 crate **逐文件、逐层**完成了移植：

| TS 源文件                              | Rust 目标文件                            | 状态                                                           |
| ----------------------------------- | ------------------------------------ | ------------------------------------------------------------ |
| `foundation.ts`                     | `foundation.rs`                      | ✅ 完整                                                         |
| `buffer.ts`                         | `buffer.rs`                          | ✅ 完整                                                         |
| `serialization.ts`                  | `serialization.rs`                   | ✅ 完整（含 JSON 回退 / 嵌套字节标记）                                     |
| `protocol.ts`                       | `protocol.rs`                        | ✅ 完整（ChunkStream / SocketProtocol / QueuePair / MessagePort） |
| `persistent-protocol.ts`            | `persistent_protocol.rs`             | ✅ 完整（ACK / 心跳 / 重放 / 流控）                                     |
| `channels.shared.ts`                | `channels_shared.rs`                 | ✅ 完整                                                         |
| `channelServer.ts`                  | `channel_server.rs`                  | ✅ 完整                                                         |
| `channelClient.ts`                  | `channel_client.rs`                  | ✅ 完整                                                         |
| `channels.ts` / `delayedChannel.ts` | `channels.rs` / `delayed_channel.rs` | ✅ 完整                                                         |
| `ipc.ts`                            | `ipc.rs`                             | ✅ 完整（IPCServer / IPCClient / StaticRouter / 多播）              |
| `proxy-channel.ts`                  | `proxy_channel.rs`                   | ✅ 语义等价（见 §3.2）                                               |
| `remote.ts`                         | `remote.rs`                          | ✅ 完整（Resolver / SocketFactory / URITransformer / 重连）         |
| `logging-middleware.ts`             | `logging_middleware.rs`              | ✅ 完整                                                         |
| `network-telemetry-middleware.ts`   | `network_telemetry_middleware.rs`    | ✅ 完整                                                         |
| `index.ts`                          | `lib.rs`（re-exports）                 | ✅ 完整                                                         |

新增的支撑模块（TS 中由运行时/第三方提供，Rust 侧自实现）：  
`value.rs`（动态值 + 手写 JSON + Base64）、`error.rs`（`RpcError`）、  
`promise.rs`（零依赖回调式 Promise）。

### 2.2 `crates/zcode-shared` —— 部分移植 `packages/shared`

| TS 源文件                                 | Rust 目标文件                  | 状态              |
| -------------------------------------- | -------------------------- | --------------- |
| `lineChangeStat.ts`                    | `line_change_stat.rs`      | ✅ 完整（行级 LCS 统计） |
| `model-provider-family.ts`（+ 相关常量）     | `model_provider_family.rs` | ✅ 完整            |
| `plan-identity.ts`（+ `usage-stats` 子集） | `plan_identity.rs`         | ✅ 完整            |

### 2.3 `crates/zcode-ui` —— 部分移植 `packages/ui`（UI 层）

Rust 侧与 React 语义最贴近的等价框架是 **Dioxus 0.6**（RSX ≈ JSX、
`#[component]` ≈ 函数组件、`use_signal` ≈ `useState`、
`use_context(_provider)` ≈ `useContext`/`Provider`）。

`packages/ui/src/components/ui` 下共 **47** 个 shadcn 风格原语，**已全部移植（47 / 47）**：

| 批次 | 数量 | 内容 |
| --- | ---: | --- |
| 第一批 | 15 | `button` `badge` `card` `alert` `input` `textarea` `label` `separator` `progress` `kbd` `spinner` `avatar` `switch` `checkbox` `tabs`（+ `cn.rs` ← `lib/utils.ts`） |
| 第二批 | 12 | 折叠类 `collapsible` `accordion`；组合型 `radio_group` `button_group` `input_group` `scroll_area`；浮层类 `tooltip` `hover_card` `popover` `dialog` `alert_dialog` `dropdown_menu` |
| 第三批 | 11 | 展示 / 动画 / 组合类 `brand` `glm_icon` `scroll_fade_viewport` `toast` `select` `command` `context_menu` `resizable` `floating_particles` `smooth_metric_value` `flip_metric_value` |
| 第四批 | 9 | 重型预览（注入式移植）`carousel` `chart` `code_viewer` `diff_viewer` `pdf_viewer` `pptx_preview_viewer` `lightweight_diff_preview` `highlighted_lightweight_diff_preview` `pptx_selection_action_bar` |

> 第四批背后的 `@pierre/diffs` / `pdf.js` / `pptx-renderer` / `recharts` / `embla` /
> shiki 都无法移植到 Rust。做法不是丢掉这些组件，而是**不 vendor 引擎、把引擎注入**：
> 抽出全部纯逻辑并加单测，引擎经 `children` / trait / 回调交给宿主。
> 详见 [PORTING-UI.md](./PORTING-UI.md) §3.8 与 §4.4。

关键点（细节见 [PORTING-UI.md](./PORTING-UI.md)）：

- 关掉 Dioxus 全部渲染器 feature，只保留组件模型 → 无 wasm / 无窗口也能编译测试；
- `cva` → 枚举 + `classes()`；`clsx` + `tailwind-merge` → `cn()`；
- **原样保留 `data-slot` / `data-state` / `data-orientation` 属性**，
  因为 `packages/ui/src/styles.css` 的 `@custom-variant` 全靠它们生效；
- 类名输出与 TS 逐字符对齐，**CSS 一行不用改**。

---

## 3. 关键设计取舍

### 3.1 零外部依赖 / 回调式 Promise

原框架全程基于 `Promise` / `async`。Rust 标准库没有异步运行时。  
为了「**保证能编译**」且不受网络/依赖版本影响，本移植：

- **不使用** tokio / futures 等运行时；
- 用 `promise.rs` 中的回调式 `Promise<T>` 表达异步：结果落定时，  
  回调在**触发落定的线程**上同步执行，语义对齐 JS Promise 的 `then`；
- 额外提供阻塞式 `wait()`，便于同步调用方与测试；
- 定时器（心跳 / ACK 检查 / 延迟 flush）用 `std::thread` + `sleep` + 停止标志实现。

### 3.2 ProxyChannel：ES6 Proxy → 显式注册表

TS 用 `Proxy` 拦截任意属性访问，自动把 `onXxx` 映射为事件、其余映射为方法。  
Rust 无等价的动态属性拦截，故改为**显式注册**的 `ServiceDefinition`  
（`method` / `sync_method` / `event` / `dynamic_event`），并保留完全一致的：

- `onXxx` / `onDynamicXxx` 命名约定（`is_event` / `is_dynamic_event`）；
- 事件「订阅窗口缓冲」语义（`buffer_event`）。

### 3.3 TContext → `Value`

TS 的 `TContext` 泛型在 Rust 侧统一用 `Value` 承载（上下文本身就是可序列化的动态数据），  
避免了为每个上下文类型引入泛型参数与 trait 约束。

### 3.4 动态载荷 `any` → `Value`

`serialize` / `deserialize` 的 `any` 用 `Value` 枚举表达，其变体与 TS 的  
`DataType` 标签一一对应（`Undefined/String/Buffer/VSBuffer/Array/Object/Int`）。  
`Object` 分支所需 JSON 与嵌套 `Uint8Array` 标记用**手写编解码**复刻，  
不引入 `serde`。

---

## 4. 构建与验证

```bash
cd rust
cargo build                              # 编译整个工作区
cargo test                               # 工作区 871 个测试
cargo run -p zcode-app                   # 开桌面窗口（UI 层见 PORTING-UI.md）
cargo run -p zcode-ui --example preview  # 生成 UI 预览页 ./preview.html
cargo build --release
```

测试分布：

| crate | 单元测试 | 集成测试 | 覆盖内容 |
| --- | ---: | ---: | --- |
| `zcode-rpc` | 10 | — | 序列化 / 事件 / 端到端 RPC |
| `zcode-shared` | 14 | — | 行级变更统计 / 供应商族 / 套餐身份 |
| `zcode-ui` | 742 | 75 + 11 | 类名 / 几何 / 键盘纯函数、渲染指令、HTML 快照 |
| `zcode-app` | 2 | 17 | 应用外壳 + 整树冒烟 |

逻辑层测试覆盖：

- VQL 整数往返（含大数 / 边界）；
- 字符串 / 数组 / 嵌套数组往返；
- 对象 JSON 回退中**嵌套字节**的还原；
- Base64 往返；
- 事件缓冲语义；
- 基于 `create_queue_pair`（内存队列传输）的**端到端**：  
  RPC 调用、事件订阅、未知方法报错。

---

## 5. 剩余工作（Roadmap）

按价值/可行性排序：

1. **继续迁移 `packages/shared` 纯逻辑模块**（zod schema 可用 `serde` + 手写校验替代）：  
   `errors.ts`、`hooks.ts`、`command-types.ts`、`conversation-message-projection-policy.ts`、  
   `background-task-controls.ts`、`oauth.ts`、`zcode-protocol-v4/*` 等。
2. **`packages/rpc` 的补充**：真实 `Socket` 实现（TCP / WebSocket）、  
   `MessagePortLike` 的 Electron/浏览器绑定、与 `packages/provider*` 的对接。
3. **UI 层**（见 [PORTING-UI.md](./PORTING-UI.md) §6/§7）：  
   **47 / 47 个原语已全部移植完毕**，并拼出了可运行的 `zcode-app`（871 个测试全绿）。
   剩余的是**宿主接线**而非移植：`overlay.rs` 的边缘翻转碰撞检测，以及为
   `code_viewer` / `diff_viewer` / `pdf_viewer` / `pptx_preview_viewer` / `chart` /
   `carousel` 接上各自的 JS 引擎等价物（这些属于宿主应用职责，见 §4.4）。
4. **服务层核心逻辑**（`packages/services`，224k 行）：按领域切分，优先迁移  
   纯计算/状态机模块；I/O 与第三方 SDK 调用保留边界。
5. **不可移植部分**：桌面/浏览器外壳（Electron / Vite / Node 主进程）不迁移；
   UI 层则**改用 Dioxus 重写**而不是机械翻译（React 运行时无法移植，但组件语义可以）。

> 若需要「整仓全量 Rust 化」，建议改为**分阶段、按模块**推进，  
> 并为每个阶段设定可编译、可测试的验收标准——本分支即第一阶段的可运行样板。
