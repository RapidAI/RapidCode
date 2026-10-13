# ZCode Rust 移植

> **本分支（`rust-only`）只保留 Rust 代码。**
> 移植所依据的 TypeScript 源码**不在这里** —— 它保留在 `rust` 分支
> （= `main` + 本目录内容）。本文档与 `PORTING.md` / `PORTING-UI.md` 里出现的
> `packages/**`、`apps/**` 路径都是**移植来源**，用于追溯「这段 Rust 是从哪个
> TS 文件翻译过来的」，在 `rust` 分支上才能打开。

本仓库是 **RapidCode / ZCode 的 Rust 移植**。分三批推进：

1. **核心逻辑层**（移植自 `packages/rpc` + `packages/shared`）—— 见 **[PORTING.md](./PORTING.md)**
2. **UI 层**（移植自 `packages/ui` 的 shadcn 风格原语，**47 / 47**）—— 见 **[PORTING-UI.md](./PORTING-UI.md)**
3. **可运行的应用**（`crates/zcode-app`）—— 用移植出的原语拼出的 ZCode 风格桌面外壳

目标是：把项目的核心逻辑与 UI 用 Rust 重写，并保证 **可编译、可测试、能跑起来**。

## 快速开始

```bash
cargo build                          # 编译整个工作区
cargo test                           # 运行全部测试（871 个）
cargo run -p zcode-app               # 开一个 1360×860 的桌面窗口
cargo build --release

# UI 预览（生成一份可直接在浏览器打开的 HTML）
cargo run -p zcode-ui --example preview       # → ./preview.html      （原语清单）
cargo run -p zcode-app --example app_preview  # → ./app-preview.html  （应用整屏静态快照）
```

> `zcode-rpc` / `zcode-shared` **零外部依赖**（只用 `std`），离线也能编译。
> `zcode-ui` 依赖 Dioxus，但默认关掉了所有渲染器 feature，因此同样离线可编译、
> 可测试（`cargo test` 首次需联网拉取 dev-dependency `dioxus-ssr`）。
> `zcode-app` 默认走 `dioxus/desktop`（wry + tao 原生窗口），不需要 wasm 工具链。
> 开窗依赖系统已安装 **WebView2 Runtime**（Windows 10/11 通常自带）。
>
> ⚠️ 已知环境问题：在 WebView2 Runtime **154.0.4258.62** 上，dioxus-desktop 0.6.3
> 自带的 wry 0.45 会在 `WebView::new` 处报 `WebView2Error(E_INVALIDARG)`，开不了窗。
> 这与本仓库代码无关（一个只含 `div { "hello" }` 的最小 Dioxus 程序同样失败），
> 编译与测试不受影响。详见「已知问题」一节。

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

## 已知问题

### WebView2 Runtime 154 上开不了窗（环境问题，与本仓库代码无关）

在 WebView2 Runtime `154.0.4258.62` 上运行 `cargo run -p zcode-app`，会在
wry 建 WebView 时 panic：

```
thread 'main' panicked at dioxus-desktop-0.6.3/src/webview.rs:400:39:
called `Result::unwrap()` on an `Err` value:
  WebView2Error(WindowsError(Error { code: HRESULT(0x80070057), message: "参数错误。" }))
```

**已排除的原因**（都实测过）：

| 假设 | 结果 |
| --- | --- |
| 沙箱限制 GUI 创建 | 非沙箱下同样失败 |
| 本仓库改动引入 | **一个只含 `div { "hello" }` 的最小 Dioxus 程序同样失败** |
| WebView2 Runtime 缺失 | 已安装 `154.0.4258.62`，且系统上有 32 个进程正在使用它 |
| 用户数据目录无效 | 显式 `with_data_directory(临时目录)` 同样失败 |
| `tao` 开不了窗口 | 调用栈显示 `tao` 事件循环正常，失败点在 `wry::WebView::new` |

**结论**：是 dioxus-desktop 0.6.3 自带的 **wry 0.45.0** 与较新 WebView2 Runtime
之间的兼容性问题。`wry` 0.53.5 / 0.54.2 的对应代码与 0.45 逐字相同，
所以不能确认新版已修复。

**影响范围**：只影响「开原生窗口」这一步。`cargo build` / `cargo test` 全部不受影响，
**871 个测试照常全绿**。

**后续可尝试**：升级 dioxus 到 0.7（换用更新的 wry）；或在另一台
WebView2 版本较旧的机器上验证；或用 `--features web` 走浏览器渲染器
（需要 `wasm32-unknown-unknown` target）。
