# ZCode Rust 移植

> **本分支（`rust-only`）只保留 Rust 代码。**
> 移植所依据的 TypeScript 源码**不在这里** —— 它保留在 `rust` 分支
> （= `main` + 本目录内容）。本文档与 `PORTING.md` / `PORTING-UI.md` 里出现的
> `packages/**`、`apps/**` 路径都是**移植来源**，用于追溯「这段 Rust 是从哪个
> TS 文件翻译过来的」，在 `rust` 分支上才能打开。

本仓库是 **RapidCode / ZCode 的 Rust 移植**。分三批推进：

1. **核心逻辑层**（移植自 `packages/rpc` + `packages/shared`）—— 见 **[PORTING.md](./PORTING.md)**
2. **UI 层**（移植自 `packages/ui` 的 shadcn 风格原语，**47 / 47**）—— 见 **[PORTING-UI.md](./PORTING-UI.md)**
3. **应用外壳**（`crates/zcode-app`）—— 用移植出的原语拼出的 ZCode 风格界面

**当前状态，请先读这一句**：**可编译、可测试**已经做到（`cargo build` 零警告、
871 个测试全绿），但**桌面窗口渲染不出内容**（环境层面的依赖不兼容，见「已知问题」），
而且**应用外壳目前只是静态演示**（硬编码演示数据，没有任何真实业务逻辑）。
上面第 3 条**不等于**「应用已经能用」。已移植的是 **UI 原语这一层**；
上游 8,462 个 `.ts` / `.tsx` 里真正承载业务逻辑的那部分基本还没动，
范围明细见「已知问题 · 移植范围」。

## 快速开始

```bash
cargo build                          # 编译整个工作区
cargo test                           # 运行全部测试（871 个）
cargo run -p zcode-app               # 桌面窗口（⚠️ 本机开得出窗口但内容是空白，见「已知问题」）
cargo build --release

# UI 预览（生成一份可直接在浏览器打开的 HTML）—— 这是目前唯一能看到 UI 的路径
cargo run -p zcode-ui --example preview       # → ./preview.html      （原语清单）
cargo run -p zcode-app --example app_preview  # → ./app-preview.html  （应用整屏静态快照）
```

> `zcode-rpc` / `zcode-shared` **零外部依赖**（只用 `std`），离线也能编译。
> `zcode-ui` 依赖 Dioxus，但默认关掉了所有渲染器 feature，因此同样离线可编译、
> 可测试（`cargo test` 首次需联网拉取 dev-dependency `dioxus-ssr`）。
> `zcode-app` 默认走 `dioxus/desktop`（wry + tao 原生窗口），不需要 wasm 工具链。
> 开窗依赖系统已安装 **WebView2 Runtime**（Windows 10/11 通常自带）。
>
> 实测环境：WebView2 Runtime `154.0.4258.62`（Windows）。
>
> ⚠️ **`cargo run -p zcode-app` 能开出窗口，但窗口内容区是空白的** —— UI 渲染不出来。
> 这是 dioxus-desktop 0.6.3（wry 0.45）与较新 WebView2 Runtime 的兼容性问题，
> **与本仓库代码无关**（`cargo build` 零警告、871 个测试全绿，SSR 渲染出的组件树
> 也完整正确）。完整证据链与可选出路见「已知问题」一节。
> 想直接看 UI，用浏览器打开 `app-preview.html`（SSR 静态快照）。

## 工作区结构

```
.                                   # 仓库根 = Cargo workspace 根
├── Cargo.toml                      # workspace 根
├── Cargo.lock
├── LICENSE                         # Apache 2.0（沿用上游）
├── PORTING.md                      # 逻辑层移植说明
├── PORTING-UI.md                   # UI 层移植说明
├── preview.html                    # 原语清单预览页（由 zcode-ui 的 example 生成）
├── app-preview.html                # 应用整屏静态快照（由 zcode-app 的 example 生成，自包含）
├── .tailwind-build/                # 生成 app.css（见 PORTING-UI.md §4.8）
│   ├── build.sh                    #   拼装入口 CSS + 调 Tailwind CLI
│   ├── tokens.css                  #   设计令牌（vendored 自上游 styles.css 的 @theme / .dark）
│   └── package.json                #   只用 @tailwindcss/cli + tw-animate-css
└── crates/
    ├── zcode-rpc/                  # 核心：VS Code 风格 IPC/RPC 框架（完整移植）
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
    ├── zcode-shared/               # 共享核心逻辑（部分移植）
    │   └── src/
    │       ├── line_change_stat.rs       行级 LCS 变更统计
    │       ├── model_provider_family.rs  模型供应商族归一 / 可见性
    │       └── plan_identity.rs          套餐身份解析
    ├── zcode-ui/                   # UI 原语组件库（Rust + Dioxus 移植自 packages/ui）
    │   ├── src/
    │   │   ├── cn.rs                     类名合并（clsx + tailwind-merge 子集）
    │   │   ├── icons.rs                  内联 lucide 图标（零图标库依赖）
    │   │   ├── code_preview_preferences.rs  代码预览主题白名单
    │   │   ├── keyboard_shortcuts.rs     快捷键平台判定（mac / 非 mac）
    │   │   ├── presentation_element_model.rs  PPTX 元素命中测试模型
    │   │   ├── patch_diff_preview.rs     diff 预览的纯数据层
    │   │   ├── shiki_highlighter.rs      shiki 的 trait 抽象
    │   │   ├── pdf_js_assets.rs          pdf.js 资源路径解析
    │   │   ├── pdf_zoom_overlay.rs       缩放浮层的双缓冲状态
    │   │   ├── diffs_highlighter_engine.rs  高亮引擎的纯决策层
    │   │   └── components/               47 个 shadcn 风格原语 + overlay.rs 定位约定
    │   │       ├── 基础     button  badge  card  alert  input  textarea  label
    │   │       │            separator  progress  kbd  spinner  avatar  switch  checkbox  tabs
    │   │       ├── 折叠类   collapsible  accordion
    │   │       ├── 组合型   radio_group  button_group  input_group  scroll_area
    │   │       ├── 浮层类   tooltip  hover_card  popover  dialog  alert_dialog  dropdown_menu
    │   │       ├── 展示/动画 brand  glm_icon  scroll_fade_viewport  toast  select  command
    │   │       │            context_menu  resizable  floating_particles
    │   │       │            smooth_metric_value  flip_metric_value
    │   │       └── 重型预览 carousel  chart  code_viewer  diff_viewer  pdf_viewer
    │   │                    pptx_preview_viewer  lightweight_diff_preview
    │   │                    highlighted_lightweight_diff_preview  pptx_selection_action_bar
    │   ├── tests/
    │   │   ├── render.rs                 mutation 记录式渲染测试（11）
    │   │   └── html_snapshot.rs          dioxus-ssr HTML 快照测试（75）
    │   └── examples/preview.rs           预览页生成器
    └── zcode-app/                  # 可运行的桌面应用（用上面两个 crate 拼出来）
        ├── assets/app.css                由 .tailwind-build/build.sh 生成（116,737 字节）
        ├── src/
        │   ├── lib.rs                    App / AppShell，内嵌样式表
        │   ├── main.rs                   桌面入口（LaunchBuilder::desktop）
        │   ├── state.rs                  AppState / AppCtx / demo_state
        │   └── views/                    sidebar / topbar / conversation / composer
        │                                 detail / settings / gallery
        ├── examples/app_preview.rs       整屏静态快照生成器（自包含 HTML）
        └── tests/smoke.rs                整树冒烟测试（17）
```

## 设计要点（逻辑层）

| 主题 | TypeScript 原实现 | Rust 移植 |
| --- | --- | --- |
| 事件系统 | `Event<T>` = 回调函数 | `Event<T>` 结构体（`Arc<dyn Fn>` 订阅） |
| 异步结果 | 原生 `Promise` | 零依赖回调式 `Promise<T>`（`Mutex` + `Condvar`） |
| 动态载荷 | `any` | `Value` 枚举（含 JSON / Base64 编解码） |
| 定时器 | `setTimeout` / `setInterval` | `std::thread` + `sleep` + 停止标志 |
| 传输 | 接口 | `MessagePassingProtocol` trait；`create_queue_pair` 供测试 |
| 上下文 | 泛型 `TContext` | 统一用 `Value` 承载（本身可序列化） |

## 设计要点（UI 层）

| React 生态 | Rust / Dioxus 等价物 |
| --- | --- |
| JSX | RSX（`rsx! { ... }`） |
| 函数组件 / `useState` | `#[component] fn` / `use_signal` |
| `useContext` / `Provider` | `use_context` / `use_context_provider` |
| `cva`（class-variance-authority） | 枚举 + `classes()` |
| `clsx` + `tailwind-merge` | `cn()` / `cn_opt()` |
| Radix `data-state` / `data-orientation` | 手写同名属性（保住 `@custom-variant` 语义） |
| Radix `Portal` + `@floating-ui` | 锚点包裹层 + `absolute` 类名（见 `components/overlay.rs`） |
| lucide-react | `icons.rs` 内联 SVG（零依赖） |

## 测试

```
cargo test                 # 工作区 871 个测试
cargo test -p zcode-ui     # UI 层 828 个测试
cargo test -p zcode-app    # 应用层 19 个测试
```

| crate | 单元 | 集成 | 小计 |
| --- | --- | --- | --- |
| `zcode-rpc` | 10 | — | 10 |
| `zcode-shared` | 14 | — | 14 |
| `zcode-ui` | 742 | 75 + 11 | 828 |
| `zcode-app` | 2 | 17 | 19 |
| **合计** | **768** | **103** | **871** |

- **逻辑层**：VQL 序列化往返、嵌套 `Uint8Array` 的 JSON 回退、Base64、
  事件缓冲、基于内存队列（`create_queue_pair`）的端到端 RPC 调用 / 事件订阅。
- **UI 层**：类名 / 几何 / 键盘 / 格式化的纯函数断言（742）、mutation 记录式
  渲染（11）、`dioxus-ssr` HTML 快照（75）—— 全部不需要 wasm / 窗口 / 浏览器。
- **应用层**：整棵应用树（含三个浮层）真渲染一遍，断言区域文案、
  `data-slot` 锚点、`data-*` 语义属性、浮层默认关闭、内嵌样式表覆盖到了
  所需的 `@custom-variant` 与设计令牌。

> 「能正常运行」的判定方法（进程存活 / `MainWindowHandle` 非零 /
> `MainWindowTitle` 与 `WindowBuilder` 一致 / `Responding=True`）见
> [PORTING-UI.md](./PORTING-UI.md) §5.5。

## 已知问题：原生窗口渲染不出内容（WebView2 运行时崩溃）

**结论先说**：`cargo run -p zcode-app` 能开出一个标题正确的窗口，但**窗口内容区一片空白**
—— UI 渲染不出来。这是 **dioxus-desktop 0.6.3（内置 wry 0.45）与 WebView2 Runtime
`154.0.4258.62` 之间的兼容性问题，不是本仓库代码的问题**。同一份代码
`cargo build` 零警告、871 个测试全绿；SSR 渲染出的组件树也完整正确（见下方
「能看的东西」）。

### 证据链

1. **页面本身加载正常**。往 `</head>` 注入自检脚本后用
   `window.ipc.postMessage` 回读（这条通道可靠，`initialize` 已证明可用）：
   `parse` / `domready` / `load` 三条都在页面时间 200 ms 内到达，
   且此时 `window.interpreter` 已存在。

2. **解释器启动正常**。包住解释器方法后可以看到
   `initialize()` 与 `waitForRequest()` 都正常进入并返回
   （`waitForRequest-enter h=true`）。顺带确认 `headless=true` 是 dioxus 的设计：
   `webview.rs:228` 里 `headless = !cfg.window.window.visible`，而
   `app.rs::handle_start_cause_init` 总是先 `with_visible(false)` 避免白屏闪烁。

3. **DOM 变更送不到页面**。`#main` 始终 0 个子节点；`run_from_bytes`（解释器应用
   DOM 变更的唯一入口）要么根本不被调用，要么被调用后页面立刻死亡。
   页面在「收不到变更」时能一直存活（心跳每秒正常回读），说明**不是 JS 线程被阻塞**。

4. **WebView2 会自行崩溃**。`<exe>.WebView2/EBWebView/Crashpad/reports/` 下会**自行**
   出现崩溃转储（没有任何外力 kill），元数据为：

   ```
   ApplicationName=msedgewebview2.exe
   ApplicationVersion=154.0.4258.62
   ModuleName=msedge.dll
   ProcessType=browser
   SubCode=0x80000003        ← STATUS_BREAKPOINT，msedge.dll 里的 CHECK 失败
   ```

5. **宿主进程不受影响**。渲染进程死掉后 `zcode-app.exe` 仍然存活、`Responding=True`、
   `MainWindowHandle` 非零。所以「进程活着 + 有窗口句柄 + 标题正确」**都不能**用来
   判定「能正常运行」—— 之前正是只看了这几项，才把「空白窗口」误判成「已跑通」。

### 为什么在本机修不了

dioxus-desktop 在 Windows 上靠**「http 伪装的自定义协议」**传 DOM 变更：
`protocol.rs` 里 `EDITS_PATH = "http://dioxus.index.html/__edits"`，由 wry 的
`AddWebResourceRequestedFilter("http://dioxus.*")` 拦截后转交 dioxus 的处理器
（wry `webview2/mod.rs:794` 与 `:940-943`：注册 `http://<name>.*` 过滤器，命中后把
`http://<name>.` 前缀重写回 `<name>://`）。解释器用长轮询不断 fetch 这个端点。
这条链路在 WebView2 `154.0.4258.62` 上不可靠，且会触发 `msedge.dll` 崩溃。

可选出路，都受本机条件限制：

| 出路 | 本机可行性 |
| --- | --- |
| 换一个 WebView2 Runtime 版本 | ❌ 只装了 `154.0.4258.62` 一个版本 |
| 升级 dioxus 到 0.7（换掉这套协议实现） | ❌ 离线缓存只有 `dioxus 0.6.3`，且无网络 |
| 改走 `--features web`（浏览器渲染器） | ❌ 只装了 `x86_64-pc-windows-msvc` target，无 wasm 工具链 |
| 把 `dioxus-desktop` 的 wry 依赖换成缓存的 `0.53.5 / 0.54.2 / 0.55.1` | ⚠️ 缓存里有，但 0.45 → 0.53 的 API 改动很大，需要 vendor 后改写 |

### 调试过程中被证伪的三个假设

**「WebView2 154 与 wry 0.45 不兼容，所以建不出 WebView」** —— 不成立。裸
`wry 0.45 + tao 0.30` 建 WebView 在全部变体下都成功，完整复刻 dioxus 的参数链也成功。
建窗这一步没问题，问题在**建完之后传 DOM 变更**。

**「`dioxus://` 协议没注册导致 `E_INVALIDARG`」** —— 只在一个刻意构造的二分变体里成立
（保留 `dioxus://` URL 但去掉协议注册）。`LaunchBuilder::desktop()` 会自动注册，
正常路径不会遇到。另外反复快速启停时偶发的 `E_INVALIDARG` 属 WebView2 侧进程竞争
（上一次实例被强杀、用户数据目录还没释放），确认无残留 `msedgewebview2.exe` 后重试即可。

**「窗口打开约 20 秒后静默退出」** —— 是渲染进程崩溃的**伴生现象**，不是独立问题。
用 `Config::with_custom_event_handler` 抓事件流可见全程没有 `CloseRequested` /
`Destroyed`，但崩溃转储会自行出现；崩溃时机不确定，所以进程存活时长在 20 秒到
数小时之间随机。

### 移植范围（避免误解）

「47 / 47 已移植」指的是 **UI 原语这一层**，不是整个应用：

| 层 | 状态 |
| --- | --- |
| `packages/ui/src/components/ui`（shadcn 原语，47 个 TSX） | ✅ 全部移植到 `crates/zcode-ui` |
| 逻辑原语（`zcode-rpc` / `zcode-shared`） | ✅ 移植了一部分（VQL 序列化往返、Base64、事件缓冲、内存队列 RPC） |
| `crates/zcode-app`（应用外壳） | ⚠️ **只是静态演示**：`state.rs` 里是硬编码演示数据，整层没有任何文件 IO / 网络 / 进程调用 |
| `packages/shared`（419 个模块） | ⚠️ 只移植了 3 个 |
| `packages/services`（674 个文件） | ❌ 未移植 |
| `packages/desktop`（1,232 个文件） | ❌ 未移植 |
| `apps/`（5,060 个文件） | ❌ 未移植 |

上游 `main` 分支共 8,462 个 `.ts` / `.tsx` 文件。所以「移植完成」只在
**UI 原语这一层**成立；让它变成可用产品的那部分基本还没动。

### 能看的东西：SSR 静态快照

窗口渲染不出来，但**整棵组件树本身是正确的**。用 SSR 把它渲染成 HTML 就能看到：

```bash
cargo run -p zcode-ui  --example preview       # → ./preview.html      （47 个原语清单）
cargo run -p zcode-app --example app_preview   # → ./app-preview.html  （应用整屏快照）
```

`app-preview.html` 含 10,000+ 字可见文本、347 个 `data-slot` 锚点，
配色 / 圆角 / 字号都来自上游抽取并 vendored 的设计令牌 —— 用浏览器打开即所见即所得。

判断「能正常运行」的完整观测清单见 [PORTING-UI.md](./PORTING-UI.md) §5.5。
