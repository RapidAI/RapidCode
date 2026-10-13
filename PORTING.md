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

产出保证：`cargo build` 与 `cargo test` 全部通过（零警告、1116 个测试全绿）。

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
| `repo/gitCliHelpers.ts` | `git/cli_helpers.rs` | ✅ 完整（porcelain v2 / numstat 解析 + `git config --show-scope` 解析） |
| `repo/gitCliRepo.ts`（`git log` 解析那一组纯函数） | `git/graph.rs` | ✅ 完整（提交图 / 装饰 ref 解析） |
| `repo/gitCliRepo.ts`（diff 结果构造那一组纯函数） | `git/diff.rs` | ✅ 完整（可用性判定 / 二进制探测 / 未跟踪文件最小补丁） |
| `repo/gitCliRepo.ts` | `git/repo.rs` | ✅ 读路径（`resolve` / `getStatus` / `listLocalBranches` / `getIdentity` / `getIgnoredPaths` / `getCommitGraph` / `getDiff`）+ 写路径（`stage` / `unstage` / `discard` / `commit` / `push`） |
| `gitService.ts` 的 `getChanges` | `git/changes.rs` | ✅ 完整（按暂存区分组 + 汇总） |
| `packages/shared/src/git.ts` | `crates/zcode-shared/src/git.rs` | ✅ 类型层完整 |

> `graph.rs` / `diff.rs` 在上游**并不是独立文件** —— 它们是 `gitCliRepo.ts` /
> `gitCliHelpers.ts` 里的模块级纯函数。拆出来只是因为「拼命令行」和
> 「解析输出」是两件可分别验证的事：解析层不碰进程，就能用字符串直接喂进
> 单测（`parse_git_graph_records` / `to_diff_result`），不必真起仓库。
> 逻辑逐行对应，没有合并或改写。

**有意未移植**（不是遗漏，是为了不让「已移植」这句话失真）：

- `gitCommitMessageGenerator*` —— 依赖 LLM provider；
- `gitCheckpoint*` —— 依赖检查点存储（另一个未移植的子系统）；
- `commitMessageFileScope`；
- `getBranchComparison` —— 分支比较的「提交列表 + 文件列表」聚合视图，
  需要先有提交图之外的 merge-base 计算链路；
- `reuseInFlightRequest` —— 上游用它把并发发起的同一个查询合并成一次。
  本移植的读接口是**无状态**的，调用方自己决定要不要合并；
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

### 2.6 读接口补全（`getIdentity` / `getIgnoredPaths` / `getCommitGraph` / `getDiff`）

对应上游 `gitCliRepo.ts` 的同名方法。这一批的目的不只是「多几个查询」，
而是让已经移植好的 `diff_viewer` / `lightweight_diff_preview` 原语**有真实数据可喂** ——
「变更」页因此从「列出改了哪些文件」变成「点开就能看补丁」。

**`getIdentity`** 读 `user.name` / `user.email`。两条容易踩的约定：

1. `git config` 的退出码 **1 表示「这个 key 没配」，不是错误**。当成失败会
   让「用户没设邮箱」这种完全正常的状态变成红色报错。
2. 解析 `--show-scope` 输出时**只能剥掉行尾那一个换行**（`/\r?\n$/`），
   不能 `trim()` —— 否则配置值两端的空格会被吃掉，而 git 允许值里有空格。
   同理 `split('\t')` 之后要**把第 3 段之后重新拼回 `\t`**，因为值本身可能含制表符。

**`getIgnoredPaths`** 走 `git check-ignore`。两处细节：

- **`-z` 只和 `--stdin` 搭配时才有效**。本移植把路径放在 argv 上，
  所以必须按**换行**解析输出（上游在这里踩过并修过）；
- 输入里**不在仓库内的路径会被静默跳过**（等价于上游的 `try/catch → null`），
  而不是让整次查询失败 —— 一个越界路径不该毁掉其余结果。

**`getCommitGraph`** 走 `git log --format=%H%x00%P%x00%an%x00%at%x00%s%x00%D%x1e`。
四条刻意的取舍：

1. **绝不用 `--all`**。本仓库内部有 `refs/zcode/checkpoints` 这类隐藏 ref，
   `--all` 会把检查点提交拉进提交图。上游用的是显式的
   `HEAD --branches --tags --remotes`，照搬。
2. **`%at` 按 JS `Number.parseInt` 语义解析**，而不是 Rust 的 `str::parse`。
   `parseInt` 是**宽松**的：允许前导空白、允许尾部垃圾（`"12abc"` → `12`）、
   遇到小数点就截断（`"12.9"` → `12`）。`parse_leading_i64` 逐字符复刻了这套行为。
   解析不出来时是 `None`（**不是 0**）—— 0 会被界面渲染成 1970 年。
3. **装饰 ref 的解析规则**（全部保留）：`HEAD -> refs/heads/main` 要产出
   **两个** ref（`HEAD` 是 head，`main` 是 branch）；`tag: v1.0` 与
   `tag: refs/tags/v1.0` 是同一个 tag；没有前缀时按「含 `/` ⇒ remote，
   否则 branch」兜底。
   上游是**先 trim 再匹配 `"tag: "`**，所以裸的 `"tag: "` 会变成 `"tag:"`、
   匹配不上前缀、落进兜底分支变成名为 `tag:` 的 branch。这个怪行为**故意保留**，
   并有测试把它钉住。
4. **`has_more` 靠多取一条**：实际跑 `--max-count = 归一后的值 + 1`，
   `has_more = 解析条数 > 归一后的值`，再 `.take(归一后的值)`。
   `normalize_git_graph_max_count` 的语义是：`None` → 100，否则
   **clamp 到 `[1, 200]`** —— 注意 `0` 和负数是被**抬到 1**，不是「不限量」。

**`getDiff`** 是本批最需要小心的一条。核心是一个**顺序不能调换**的判定链：

```
超时 → 输出被截断 → 退出码 → stdout 为空 → 二进制 → 有补丁
```

前两项必须在看退出码之前定，因为它们的含义是「命令**没正常跑完**」——
拿一个被杀掉的进程的退出码去判断「有没有改动」是没有意义的。

其余约定：

- **`is_binary_diff` 匹配 `"GIT binary patch"` 或 `"Binary files "`** ——
  后面那个**尾随空格是模式的一部分**，少了它 `Binary files` 开头的普通文本
  也会被误判；
- **`--no-index` 用退出码 1 表示「两边有差异」**，这是正常结果，
  所以 `allowed_exit_codes` 传 `[0, 1]`；
- **未跟踪文件不走 `git diff --no-index`**，而是**手工拼一个最小 unified diff**
  （`--- /dev/null` / `+++ b/<path>` / `@@ -0,0 +1,N @@`）。上游这么做是因为
  `--no-index` 在 Windows 上会吐平台相关的头，而 `@pierre/diffs` 解析某些
  `diff --git` 头会直接抛异常、把界面展开动作打崩。手工拼的那份输出
  平台无关，且**二进制判定要排在大小截断之前** —— 否则一个巨大的二进制文件
  会报「被截断」，而它其实是「不可预览」。
- **两侧全文预览要么都给、要么都不给**（`to_complete_diff_contents`）。
  如果只读成功一侧、另一侧回填 `""`，界面会把「读不出来」当成「空文件」，
  于是整份文件被渲染成「全是新增/删除」。宁可整对降级。
  同理 `with_diff_contents`：**补丁成功、全文预览失败时保留补丁** ——
  预览只是喂给 MultiFileDiff 的，丢掉它不该连正确的补丁一起丢。
- **`git show ":path"`（空 ref）读的是索引版本**，这正是暂存区 diff 的「后」侧；
  而 `git show "<ref>:<path>"` **不能**用于 `--show-scope` / `--show-origin`
  的输出（那是 `scope\tfile:<path>\tvalue` 格式），所以两者各走各的解析。

应用层这一侧有一条与写路径**刻意相反**的约定：

> **`load_diff` 不共用 `busy` 锁。** 读 diff 既不修改仓库、也不碰 `index.lock`，
> 没有理由把它和刷新 / 写操作串起来 —— 那只会让「点开看一眼」也被排队。
> 代价是**并发结果必须能分辨新旧**：每次请求带一个单调递增的 `id`，
> 轮询端只采纳 id 等于当前请求的那条。用户快速点了 A 又点 B 时，
> A 的结果可能后到，直接丢弃，否则会把 B 的内容盖成 A 的。

界面状态也据此设计成**单个信号** `Option<DiffPreviewState>`，
其中 `path` 就是行主键 —— 于是「A 行展开着、显示的却是 B 的内容」这种状态
在结构上不可能出现。

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
cargo test                               # 工作区 1116 个测试
cargo run -p zcode-app                   # 桌面窗口（⚠️ 本机能开窗但内容空白，见 README「已知问题」）
cargo run -p zcode-ui --example preview  # 生成 UI 预览页 ./preview.html
cargo build --release
```

测试分布：

| crate | 单元测试 | 集成测试 | 覆盖内容 |
| --- | ---: | ---: | --- |
| `zcode-rpc` | 10 | — | 序列化 / 事件 / 端到端 RPC |
| `zcode-shared` | 19 | — | 行级变更统计 / 供应商族 / 套餐身份 / git 类型（含提交图 ref、改动来源枚举） |
| `zcode-services` | 144 | 37 | git 解析 / 路径归一 / 提交图 / diff 结果构造纯函数，**真起临时仓库**的端到端（读 17 + 写 20） |
| `zcode-ui` | 742 | 75 + 11 | 类名 / 几何 / 键盘纯函数、渲染指令、HTML 快照 |
| `zcode-app` | 33 | 36 + 9 | 应用外壳 + 整树冒烟（含写操作入口与 diff 预览四种状态）+ 跨线程读 / 写 / diff 链路 |

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
- `git config --show-scope` 解析：三列拆解、**key 不存在（退出码 1）不算错**、
  值内含制表符要拼回、列数不足时整行当值、**只剥行尾换行**（值两端空格要保留）、
  CRLF 输出、空值保留 scope；
- 提交图解析：装饰 ref 去重、`HEAD -> refs/heads/main` 产出两个 ref、
  `tag: ` 前缀拆解、**裸 `"tag: "` 落进兜底分支**（复刻上游行为）、
  记录之间的换行容错、时间戳解析不出来时是 `None` 而不是 0、
  `parseInt` 宽松语义（前导空白 / 尾部垃圾 / 小数点截断）、
  subject 里的逗号与空格不被吃掉；
- diff 结果构造：判定链顺序（**超时优先于退出码** / 截断优先于退出码）、
  二进制标记探测、**未跟踪文件二进制判定排在大小截断之前**、
  未跟踪补丁标记缺失的结尾换行、`with_diff_contents` 在预览失败时
  **保留补丁**；
- 超时诊断信息：断言 `timed out after 15000ms` / `elapsed=` / `killAt=` /
  `cleanup=` / `forceKill=` / `orphaned=` 都出现在消息里；
- **读接口端到端**（`tests/git_workspace.rs`，真建临时仓库，读 17 条）：
  解析仓库根 / 子目录作用域 / 本地分支列表（当前分支排最前）/ 普通目录不是错误 /
  工作目录不存在时降级而不是报错 / 读出已配置的 `user.name` 与 `user.email` /
  非仓库时身份为空 / `check-ignore` 认出被忽略的路径（空输入与普通目录返回空）/
  提交图按「新到旧」列出真实提交 / `max_count` 生效且 `has_more` 如实报告 /
  无提交的仓库返回空图 / 未暂存改动的 diff **两侧内容都有** /
  已暂存改动同样补齐两侧 / 新暂存的文件保留补丁（没有「前」侧）/
  未跟踪文件走手工最小补丁 / 二进制内容报「不可预览」/
  文件不存在或不在仓库时是「不可用」/ 没有上游时分支比较给出明确错误 /
  有上游时按 upstream 比较 / 越界路径被拒；
- **写操作端到端**（同一文件，写 20 条）：`stage` 把未跟踪文件推进索引、
  `unstage` 退回工作区（并断言 `M.` → `.M` 的往返）、`discard` 还原未暂存改动 /
  连带清索引 / 处理**已被删除**的文件、`commit` 真的产出带指定信息的提交 /
  拒绝空信息 / 只提交指定 pathspec、`push` 在没有 remote 或 detached HEAD 时
  给出明确错误、以及写操作统一拒绝「非仓库」与「越界路径」、空路径列表是 no-op。

  > 「有上游时按 upstream 比较」那条用的是**本地目录当 remote**
  > （`remote add origin <temp>` → `fetch` → `branch --set-upstream-to`），
  > 不碰网络，所以离线也能跑。

  > 这一组踩过一个值得记的坑：`CARGO_TARGET_TMPDIR` 落在仓库自己的 `target/`
  > 里，而 `target/` 只是被 `.gitignore` 掉了 —— `git rev-parse` 依然会解析到
  > **真实仓库**。于是「拒绝非仓库」那条测试其实对着真实工作区跑了一次
  > `git push`。现在改用 `std::env::temp_dir()` 并先探一次
  > `git rev-parse --show-toplevel` 确认真的不在任何仓库里，测试从 93 秒
  > 降到 9.7 秒。

应用层测试覆盖（`crates/zcode-app`）：

- **整树冒烟**（`tests/smoke.rs`，36 条 + 1 条 `#[ignore]`）：各页面渲染、
  写操作入口的可见性、**diff 预览的四种状态**（有补丁 / 加载中 /
  不可用且给出原因 / 命令失败）、以及「展开预览不会遮掉页面其余部分」；
- **后台通道**（`tests/workspace_loader.rs`，9 条）：真起临时仓库，
  断言刷新链路的消息顺序、**diff 请求只回一条消息**（不发 `Loading` / `Loaded`）、
  并发 diff 的 id 互不相同、失败时**带着原因回来**而不是静默。

---

## 5. 剩余工作（Roadmap）

按价值/可行性排序：

1. **继续迁移 `packages/shared` 纯逻辑模块**（zod schema 可用 `serde` + 手写校验替代）：  
   `errors.ts`、`hooks.ts`、`command-types.ts`、`conversation-message-projection-policy.ts`、  
   `background-task-controls.ts`、`oauth.ts`、`zcode-protocol-v4/*` 等。
2. **`packages/rpc` 的补充**：真实 `Socket` 实现（TCP / WebSocket）、  
   `MessagePortLike` 的 Electron/浏览器绑定、与 `packages/provider*` 的对接。
3. **UI 层**（见 [PORTING-UI.md](./PORTING-UI.md) §6/§7）：  
   **47 / 47 个原语已全部移植完毕**，并拼出了可运行的 `zcode-app`（1116 个测试全绿）。
   剩余的是**宿主接线**而非移植：`overlay.rs` 的边缘翻转碰撞检测，以及为
   `code_viewer` / `diff_viewer` / `pdf_viewer` / `pptx_preview_viewer` / `chart` /
   `carousel` 接上各自的 JS 引擎等价物（这些属于宿主应用职责，见 §4.4）。
4. **服务层核心逻辑**（`packages/services`，224k 行）：按领域切分，优先迁移  
   纯计算/状态机模块；I/O 与第三方 SDK 调用保留边界。  
   **已完成三条**：git 工作区状态的**读路径**（§2.4）、**写路径**（§2.5，
   `stage` / `unstage` / `discard` / `commit` / `push`）与**读接口补全**
   （§2.6，`getIdentity` / `getIgnoredPaths` / `getCommitGraph` / `getDiff`）。
   「变更」页因此走完了「列出改动 → 可操作 → 点开看补丁」三级。
   下一步的自然延伸有两支：`getBranchComparison`（需要先补 merge-base 计算），
   以及 `packages/services` 里其它自包含链路（如 `gitCheckpoint*`，
   但那条依赖检查点存储，得先把存储一起移植）。
5. **不可移植部分**：桌面/浏览器外壳（Electron / Vite / Node 主进程）不迁移；
   UI 层则**改用 Dioxus 重写**而不是机械翻译（React 运行时无法移植，但组件语义可以）。

> 若需要「整仓全量 Rust 化」，建议改为**分阶段、按模块**推进，  
> 并为每个阶段设定可编译、可测试的验收标准——本分支即第一阶段的可运行样板。
