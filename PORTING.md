# TypeScript → Rust 移植说明（PORTING.md）

本文件说明 Rust 移植的**范围、映射、设计取舍与剩余工作**。

> **分支布局**：本分支 `rust-only` 只保留 Rust 代码，仓库根即 Cargo workspace 根。
> 移植所依据的 TypeScript 源码保留在 `rust` 分支（= `main` + 本目录内容）。
> 下文出现的 `packages/**`、`apps/**` 路径都是**移植来源**，
> 用于追溯「这段 Rust 是从哪个 TS 文件翻译过来的」，需在 `rust` 分支上查看。

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
> 并为 UI 层选定 Rust 等价框架（Dioxus）从原语组件开始迁移，  
> 最后从 `packages/services` 里挑自包含的链路接上真实 IO。**

产出保证：`cargo build` 与 `cargo test` 全部通过（零警告、1028 个测试全绿）。

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
| `git.ts`                               | `git.rs`                   | ✅ 类型层完整（见 §2.4） |

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

### 2.4 `crates/zcode-services` —— 起步：git 工作区状态链路

`packages/services`（674 个文件 / 224k 行）不可能一次全量移植。这里挑的是
**自包含、可测、且能立刻让界面显示真实数据**的一条链路：git 工作区状态。

| TS 源文件 | Rust 目标文件 | 状态 |
| --- | --- | --- |
| `git/config.ts` | `git/config.rs` | ✅ 完整（环境净化 / 二进制探测 / 路径归一） |
| `providers/gitEnvironmentProvider.ts` | `git/config.rs`（`apply_git_command_env`） | ✅ 完整 |
| `providers/gitCommandProvider.ts` | `git/provider.rs` | ✅ 语义等价（见下方差异） |
| `repo/gitCliTypes.ts` | `git/types.rs` | ✅ 完整 |
| `repo/gitCliHelpers.ts` | `git/cli_helpers.rs` | ✅ 完整（porcelain v2 / numstat 解析） |
| `repo/gitCliRepo.ts` | `git/repo.rs` | ✅ 读路径（`resolve` / `getStatus` / `listLocalBranches`）+ 写路径（`stage` / `unstage` / `discard` / `commit` / `push`） |
| `gitService.ts` 的 `getChanges` | `git/changes.rs` | ✅ 完整（按暂存区分组 + 汇总） |
| `packages/shared/src/git.ts` | `crates/zcode-shared/src/git.rs` | ✅ 类型层完整 |

**有意未移植**（不是遗漏，是为了不让「已移植」这句话失真）：

- `gitCommitMessageGenerator*` —— 依赖 LLM provider；
- `gitCheckpoint*` —— 依赖检查点存储（另一个未移植的子系统）；
- `commitMessageFileScope`、`getIgnoredPaths` / `getDiff` / `getCommitGraph` / `getIdentity`；
- `commit` 的 **`stagedOnly` 部分提交变体** —— 上游靠临时 `GIT_INDEX_FILE`
  只提交索引里的内容；本移植的 `commit` 走的是「整索引 + 可选 pathspec」那条路，
  所以调用方**不要**期待这个能力。

清单与理由也写在 `crates/zcode-services/src/lib.rs` 的模块文档里。

**与上游的两处刻意差异**：

1. **杀进程只做一次**。上游分两阶段（先 `SIGTERM` 再超时后 `SIGKILL`）；Rust 的
   `Child::kill` 在 Windows 上就是 `TerminateProcess`，没有「先礼后兵」的中间态，
   所以直接一次强杀，并在结果里如实记录 `force_kill_attempted` / `orphaned`。
2. **spawn 失败用类型区分，不靠 stderr 文本**。上游靠匹配错误文本判断
   「git 不存在」还是「工作目录不存在」；Rust 侧改成 `GitSpawnErrorKind`
   枚举。顺带发现一个上游没有覆盖的真实行为：Windows 上 `CreateProcessW`
   遇到不存在的 `cwd` 返回 `ERROR_DIRECTORY`(267)，Rust 把它映射成
   `ErrorKind::Other` 而**不是** `NotFound` —— 所以 `classify_spawn_error`
   必须**先查 `cwd.is_dir()`**，否则会把「目录不存在」误判成「git 没装」。

### 2.5 写路径（`stage` / `unstage` / `discard` / `commit` / `push`）

对应上游 `gitCliRepo.ts` 的同名方法（`stage` 1423 / `unstage` 1444 /
`discard` 1465 / `commit` 1488 / `push` 1638 行附近）。五条约定：

1. **前置检查**：没有可用的 git、或 workspace 不在仓库里 → `RepositoryUnavailable`。
   顺序是先 git 后仓库 —— 「没装 git」比「这不是仓库」更根本，报错也更该先说。
2. **路径归一**：`normalizeInputPath` 的等价物把用户给的相对路径**以 workspace
   （不是 repo 根）为基准**解析，`realpath` 后转成仓库相对路径，越界报
   `PathOutsideRepository`。`realpath` 必须带兜底 —— `discard` 要处理的正是
   **被删掉的文件**，对它 `canonicalize` 必然失败。
3. **空列表是 no-op**：不报错，也不跑 git。
4. **写操作不返回新状态**：调用方自己重新扫描。界面因此在动作完成后
   先收到一条 `Notice`（成功 / 失败），再收到刷新后的 `Loaded`。
5. **`discard` 是唯一会丢数据的操作**（`git restore` 之后 git 也找不回），
   所以在应用层被标成 `is_destructive()`，必须过二次确认弹窗。

平台相关的一处坑：Windows 上 `std::fs::canonicalize` 返回 `\\?\C:\...` 这种
verbatim 前缀，它永远 `strip_prefix` 匹配不上普通路径，于是所有路径比较都会
静默失败。`config.rs` 的 `canonicalize_best_effort` 负责剥掉前缀
（`\\?\UNC\server\share` → `\\server\share`），**两边都必须走它**。

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

### 3.5 同步阻塞的服务调用怎么接进 dioxus（不引 tokio、不轮询）

`zcode-services` 的调用是**同步阻塞**的（起进程、读管道、等退出）。直接在
dioxus 的渲染线程上跑会把窗口冻住 —— 而「窗口卡死」和「窗口空白」在肉眼上
几乎分不出来，正是上一轮误判的成因。所以必须挪到后台线程。

问题在于：dioxus 0.6 用的是自己的**本地 executor**（不是 tokio），
`spawn` 只能从组件体内调用，而线程跑完之后需要**主动唤醒** executor 才会
重新 poll 那个 future。

做法是 `futures-channel` 的 `mpsc::unbounded()`：

```rust
// 后台线程：send 会注册 dioxus 的 waker，从而触发 VirtualDom 重新 poll
let _ = sender.unbounded_send(WorkspaceMessage::Loaded(Box::new(data)));

// 组件内：
while let Some(message) = next_workspace_message(receiver).await { … }
// 其中 next_workspace_message = poll_fn(|cx| Pin::new(&mut *receiver).poll_next(cx))
```

要点：

- **不需要轮询定时器**，也不需要 tokio —— 唤醒是 `send` 的副作用；
- `UnboundedReceiver` **不是 `Clone`**，而 `use_hook` 要求闭包 `Clone`，
  所以包一层 `Rc<RefCell<Option<_>>>`（`WorkspaceChannel`），提供
  `requester()` / `take_receiver()`；接收端只能被取走一次（有测试守着）；
- 只多引了两个**纯 trait/通道** crate（`futures-channel` / `futures-core`），
  没有引入任何运行时；
- `AppCtx` 必须保持 `Copy`（各 view 用 `move |_|` 按字段捕获它），
  所以持有 `Arc` 的 `WorkspaceRequester` 走**另一个** context
  （`use_context_provider`），不能塞进 `AppCtx`。

写操作（`run`）复用同一条通道，但有两点不同：

- **所有 git 操作共用同一个 `busy` 原子量，刻意串行化**。并发的 `git add`
  与 `git commit` 会争 `index.lock`，失败信息还很难懂；而且并发写同一个仓库
  本来就没什么意义。
- **被拒绝的操作要说明原因**。`refresh` 被拒时静默返回 `false` 就够了（用户
  只是多点了一下刷新）；但写操作被拒时如果也静默，用户看到的就是「按钮点了
  没反应」，会以为界面卡死 —— 所以这时会补一条
  `Notice`（`上一个 Git 操作还在进行中，「提交」没有执行`）。这是「失败必须
  看得见」那条教训的直接延续。

消息顺序也是刻意的：`Loading` → `Notice` → `Loaded`。先让用户看到
「提交成功：1b79077」，再刷新列表；反过来会变成「列表刷新了，但刚才那下
到底成没成？」。

副作用是：SSR 快照测试**不会**驱动 `spawn` 出来的 future（`rebuild_in_place`
只跑同步代码）。这反而帮了忙 —— 快照测试因此永远不会真的去跑 git。
`AppShell` 只在存在 `WorkspaceBootstrap`（真实 `App` 根）时才发起扫描，
测试里用 `try_consume_context` 拿不到它，于是完全无副作用。

---

## 4. 构建与验证

```bash
cargo build                              # 编译整个工作区
cargo test                               # 工作区 1028 个测试
cargo run -p zcode-app                   # 桌面窗口（⚠️ 本机能开窗但内容空白，见 README「已知问题」）
cargo run -p zcode-ui --example preview  # 生成 UI 预览页 ./preview.html
cargo build --release
```

测试分布：

| crate | 单元测试 | 集成测试 | 覆盖内容 |
| --- | ---: | ---: | --- |
| `zcode-rpc` | 10 | — | 序列化 / 事件 / 端到端 RPC |
| `zcode-shared` | 18 | — | 行级变更统计 / 供应商族 / 套餐身份 / git 类型 |
| `zcode-services` | 91 | 20 | git 解析 / 路径归一纯函数、**真起临时仓库**的端到端（读 6 + 写 14） |
| `zcode-ui` | 742 | 75 + 11 | 类名 / 几何 / 键盘纯函数、渲染指令、HTML 快照 |
| `zcode-app` | 25 | 30 + 6 | 应用外壳 + 整树冒烟（含写操作入口）+ 跨线程读 / 写链路 |

逻辑层测试覆盖：

- VQL 整数往返（含大数 / 边界）；
- 字符串 / 数组 / 嵌套数组往返；
- 对象 JSON 回退中**嵌套字节**的还原；
- Base64 往返；
- 事件缓冲语义；
- 基于 `create_queue_pair`（内存队列传输）的**端到端**：  
  RPC 调用、事件订阅、未知方法报错。

服务层测试覆盖：

- `--porcelain=v2 -z` 解析：`MM` 条目同时产出两条变更、重命名记录吃掉
  下一条空记录、路径含空格、Windows 反斜杠归一、未跟踪目录记录；
- `--numstat -z` 解析：二进制显示 `-` 时记 0、重命名取 `original_path`；
- 超时诊断信息：断言 `timed out after 15000ms` / `elapsed=` / `killAt=` /
  `cleanup=` / `forceKill=` / `orphaned=` 都出现在消息里；
- **端到端**（`tests/git_workspace.rs`，真建临时仓库）：解析仓库根 / 子目录
  作用域 / 本地分支列表（当前分支排最前）/ 普通目录不是错误 / 工作目录不存在时
  降级而不是报错（读 6 条）；
- **写操作端到端**（同一文件，写 14 条）：`stage` 把未跟踪文件推进索引、
  `unstage` 退回工作区（并断言 `M.` → `.M` 的往返）、`discard` 还原未暂存改动 /
  连带清索引 / 处理**已被删除**的文件、`commit` 真的产出带指定信息的提交 /
  拒绝空信息 / 只提交指定 pathspec、`push` 在没有 remote 或 detached HEAD 时
  给出明确错误、以及写操作统一拒绝「非仓库」与「越界路径」、空路径列表是 no-op。

  > 这一组踩过一个值得记的坑：`CARGO_TARGET_TMPDIR` 落在仓库自己的 `target/`
  > 里，而 `target/` 只是被 `.gitignore` 掉了 —— `git rev-parse` 依然会解析到
  > **真实仓库**。于是「拒绝非仓库」那条测试其实对着真实工作区跑了一次
  > `git push`。现在改用 `std::env::temp_dir()` 并先探一次
  > `git rev-parse --show-toplevel` 确认真的不在任何仓库里，测试从 93 秒
  > 降到 9.7 秒。

---

## 5. 剩余工作（Roadmap）

按价值/可行性排序：

1. **继续迁移 `packages/shared` 纯逻辑模块**（zod schema 可用 `serde` + 手写校验替代）：  
   `errors.ts`、`hooks.ts`、`command-types.ts`、`conversation-message-projection-policy.ts`、  
   `background-task-controls.ts`、`oauth.ts`、`zcode-protocol-v4/*` 等。
2. **`packages/rpc` 的补充**：真实 `Socket` 实现（TCP / WebSocket）、  
   `MessagePortLike` 的 Electron/浏览器绑定、与 `packages/provider*` 的对接。
3. **UI 层**（见 [PORTING-UI.md](./PORTING-UI.md) §6/§7）：  
   **47 / 47 个原语已全部移植完毕**，并拼出了可运行的 `zcode-app`（1028 个测试全绿）。
   剩余的是**宿主接线**而非移植：`overlay.rs` 的边缘翻转碰撞检测，以及为
   `code_viewer` / `diff_viewer` / `pdf_viewer` / `pptx_preview_viewer` / `chart` /
   `carousel` 接上各自的 JS 引擎等价物（这些属于宿主应用职责，见 §4.4）。
4. **服务层核心逻辑**（`packages/services`，224k 行）：按领域切分，优先迁移  
   纯计算/状态机模块；I/O 与第三方 SDK 调用保留边界。  
   **已完成两条**：git 工作区状态的**读路径**（§2.4）与**写路径**（§2.5，
   `stage` / `unstage` / `discard` / `commit` / `push`），「变更」页因此从只读
   变成了可操作。下一步的自然延伸是 `getDiff` / `getCommitGraph` /
   `getIdentity` 这几个读接口 —— 它们同样只是拼命令行 + 复用同一套
   `GitCommandProvider`，而且能直接喂给已经移植好的 `diff_viewer` 原语。
5. **不可移植部分**：桌面/浏览器外壳（Electron / Vite / Node 主进程）不迁移；
   UI 层则**改用 Dioxus 重写**而不是机械翻译（React 运行时无法移植，但组件语义可以）。

> 若需要「整仓全量 Rust 化」，建议改为**分阶段、按模块**推进，  
> 并为每个阶段设定可编译、可测试的验收标准——本分支即第一阶段的可运行样板。
