# UI 层移植说明（PORTING-UI.md）

> 配套阅读：[PORTING.md](./PORTING.md)（核心逻辑层移植）、[README.md](./README.md)（快速开始）。

> **分支布局**：本分支 `rust-only` 只保留 Rust 代码，仓库根即 Cargo workspace 根。
> 移植所依据的 TypeScript 源码保留在 `rust` 分支（= `main` + 本目录内容）。
> 下文出现的 `packages/**` 路径都是**移植来源**，用于追溯「这段 Rust 是从哪个
> TSX 文件翻译过来的」，需在 `rust` 分支上查看。设计令牌已 vendored 到
> `.tailwind-build/tokens.css`（见 §4.8），因此本分支不依赖上游 CSS 即可重新生成样式。

## 1. 结论先行

`packages/ui` 的 shadcn 风格原语组件（React + Radix + Tailwind）已经用
**Rust + [Dioxus](https://dioxuslabs.com/) 0.6** 全量重写，落在 `crates/zcode-ui`；
在此之上又用移植出来的原语搭了一个 ZCode 风格的桌面应用 `crates/zcode-app`。

- **47 / 47** 个原语完成移植（`26,397` 行 Rust，含 **742** 个内联单测）；
- **应用外壳**：侧栏 / 会话流 / 输入区 / 详情面板 / 设置对话框 / 组件画廊，
  `5,088` 行 + `1,814` 行测试；
- **「变更」页已接真实数据，并且可操作**：走 `crates/zcode-services` 里移植过来的
  git 工作区状态链路（见 [PORTING.md](./PORTING.md) §2.4 / §2.5 / §2.6 与 §3.5），
  界面上列出的分支、变更文件、`+N/-M` 都来自真实 `git status` / `git diff`，
  不是硬编码；`暂存` / `取消暂存` / `丢弃` / `提交` / `推送` 五个动作也是真的在跑 `git`；
  **点开任意一行会展开真实补丁**（行号、增删着色都来自 `git diff` 本身）；
- `cargo build` / `cargo test` **全绿**（工作区 **1116** 个测试，零警告），
  且测试层**不需要 wasm、不需要窗口、不需要浏览器**；
- 类名（`class`）输出与 TS 版本**逐字符对齐** —— 移植后仍复用 `packages/ui` 的
  同一套 Tailwind 样式与 `@custom-variant` 语义，不需要改一行 CSS。

> **重型组件怎么落的**：`code-viewer` / `diff-viewer` / `pdf-viewer` /
> `pptx-preview-viewer` / `chart` / `carousel` 这类组件背后是
> `@pierre/diffs`、`react-pdf`、`pptx-renderer`、`recharts`、`embla` 等
> **无法移植的 JS 引擎**。处理方式是**不 vendor 引擎、而是把引擎注入**：
> 把可移植的部分（状态机、坐标换算、类名、渲染参数、缓存键）全部抽成
> **纯函数**并完整测试，引擎本身由宿主以 `children` / 参数 / trait 的形式注入。
> 详见 §4.4 与 §3.8。

```
cargo run   -p zcode-app                  # 开一个 1360×860 的桌面窗口（默认 feature = desktop）
cargo build -p zcode-ui                   # 纯组件模型，任意环境可编译
cargo test  --workspace                   # 1116 个测试，全部在 host 上跑
cargo run   -p zcode-ui --example preview # 生成可直接打开的 preview.html
```

> 「能正常运行」的判定方式见 §5.5：除编译与测试外，还核对了进程存活、
> 窗口句柄非零、`MainWindowTitle` 与 `WindowBuilder` 一致、`Responding=True`。
> **注意**：这几项**都成立也不代表界面渲染出来了** —— 本机就出现过「四项全过但
> 窗口空白」的误判，所以判定清单里还必须有「`#main` 子节点数非 0」这一条。

## 2. 为什么选 Dioxus

React 生态里 `packages/ui` 依赖三件事：**组件模型**、**原语库（Radix）**、
**类名工具链（cva / clsx / tailwind-merge）**。Rust 侧的等价物对应关系：

| React / 前端生态 | Rust / Dioxus | 本仓库落地位置 |
| --- | --- | --- |
| JSX | RSX（`rsx! { ... }`） | 每个组件 |
| 函数组件 | `#[component] fn` | 每个组件 |
| `useState` | `use_signal` | `Tabs` |
| `useContext` / `Provider` | `use_context` / `use_context_provider` | `Tabs` 家族 |
| `props.children` | `children: Element` | 每个容器组件 |
| `className` | `class`（Dioxus 内置属性） | 每个组件 |
| `cva(...)` | 枚举 + `classes()` | `ButtonVariant`、`BadgeVariant`… |
| `clsx` + `tailwind-merge` | `cn()` / `cn_opt()` | `src/cn.rs` |
| Radix `data-state` / `data-orientation` | 手写同名属性 | `Switch`、`Checkbox`、`Tabs`、`Separator` |

Dioxus 是当前 Rust 生态里与 React **语义距离最短**的框架：RSX 就是 JSX 的
Rust 版（同一套属性/子节点语法），组件就是函数，状态就是信号。这让我们可以
**逐行对照** TSX 移植，而不是「重新设计一个 UI 框架」。

## 3. 关键工程决策

### 3.1 关掉渲染器，让 UI 库在任意环境可编译可测试

这是整个移植里最重要的一条。`Cargo.toml`：

```toml
dioxus = { version = "0.6", default-features = false, features = [
    "macro", "hooks", "html", "signals",
] }
```

Dioxus 的默认 feature 会拉上 `web`（wasm-bindgen / web-sys）或 `desktop`
（tauri / wry）。全关之后只剩**组件模型**：`rsx!`、`#[component]`、
hooks、`Element`。带来三个直接好处：

1. `cargo build` / `cargo test` 在 Linux CI、无图形界面的容器、Windows 主机上
   **行为完全一致**，不依赖 wasm32 target，也不需要 `dioxus-cli`；
2. 组件库不绑定渲染后端 —— 上层应用自己选 `dioxus-web` / `dioxus-desktop`；
3. 编译产物小、构建快（本 crate 全量测试 < 1s，仅首次编译 Dioxus 需要时间）。

需要真机挂载时按需打开：

```bash
cargo build -p zcode-ui --features web
```

`dioxus-ssr` 只作为 **dev-dependency** 引入（用于 HTML 快照测试与预览生成），
所以 `cargo build -p zcode-ui` 仍然不需要它。

### 3.2 `cva` → 枚举 + `classes()`

TS 的 `cva` 是一张「variant → 类名」的运行时映射表：

```ts
const buttonVariants = cva("group/button inline-flex …", {
  variants: {
    variant: { default: "bg-primary …", outline: "border-border …", … },
    size:    { default: "h-7 …", xs: "h-5 …", … },
  },
  defaultVariants: { variant: "default", size: "default" },
});
```

Rust 侧用枚举把同一张表**编译期固化**：

```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ButtonVariant { #[default] Default, Outline, Secondary, /* … */ }

impl ButtonVariant {
    pub fn as_str(&self) -> &'static str { /* → "default" / "outline" / … */ }
    pub fn classes(&self) -> &'static str { /* → "bg-primary …" */ }
}

pub fn button_classes(variant: ButtonVariant, size: ButtonSize, class: &str) -> String {
    cn(&[BUTTON_BASE, variant.classes(), size.classes(), class])
}
```

收益：

- **拼错 variant 编译不过**（TS 里是运行时 undefined class）；
- `as_str()` 同时供 `data-variant` 属性使用，保证属性与类名同源；
- 类名是 `&'static str`，零分配、零查找。

### 3.3 `cn()` —— `clsx` + `tailwind-merge` 的务实子集

`packages/ui/src/components/lib/utils.ts` 的实现是：

```ts
const mergeUiClasses = extendTailwindMerge({
  extend: { classGroups: { "font-size": ["text-ui-*", …] } },
});
export function cn(...inputs: ClassValue[]) { return mergeUiClasses(clsx(inputs)); }
```

Rust 版（`src/cn.rs`）实现了两段：

1. **`clsx` 语义**：按空白切分、过滤空串、保序拼接 —— 与 TS 完全等价；
2. **冲突消解**：以「修饰符前缀 + 类组」为键，**后者覆盖前者**。

两个必须做对、否则会静默出错的点：

- **修饰符要按方括号配对切分。** `[&_svg:not([class*='size-'])]:size-4` 里
  方括号内的 `:` **不是**修饰符分隔符。朴素 `split(':')` 会把它切成 4 段，
  导致 `size-4` 被归错组、被用户传入的 `size-*` 误覆盖。实现见
  `split_modifiers()`（跟踪 `[` / `]` 深度）。
- **`text-ui-*` 是字号不是文字颜色。** `text-ui-base`（字号）与
  `text-foreground`（颜色）必须落在不同类组，否则二者会互相吞掉。
  前缀表里 `text-ui-` / `text-mobile-` 显式排在 `text-` 之前。

此外还专门处理了「**同前缀、不同语义**」的族（`size_family_group()`）：

| 族 | 宽度组 | 颜色组 | 不处理会怎样 |
| --- | --- | --- | --- |
| `ring-` | `ring-2`、`ring-[3px]` | `ring-background`、`ring-ring/50` | **focus ring 宽度整条丢失** |
| `border-` / `border-x-`… | `border`、`border-2` | `border-border`、`border-transparent` | 边框宽度丢失 |
| `shadow-` | `shadow-md`、`shadow-none` | `shadow-foreground/10` | 投影强度丢失 |
| `outline-` | `outline-1` | `outline-ring` | 轮廓宽度丢失 |
| `divide-` | `divide-2` | `divide-border` | 分隔线宽度丢失 |

> 这个 bug 是在移植 `AvatarBadge` 时被单测抓出来的：`ring-2 ring-background`
> 被判成同组，`ring-2` 被丢掉。tailwind-merge 真实实现也把它们分成两个组。

未识别的类**不做任何合并** —— 宁可少合并，也不错合并。

### 3.4 保留 `data-*` 锚点，CSS 一行不用改

`packages/ui/src/styles.css` 顶部有一段关键注释：

> 当前 UI 直接使用了 shadcn 的 radix-mira 风格组件源码，这些组件大量依赖
> `data-active` / `data-horizontal` / `data-vertical` 这类语义 variant。
> 上游并不是靠单个组件手写 `data-[state=active]`，而是通过
> `shadcn/tailwind.css` 里的 `@custom-variant` 把 Radix 的 `data-state`、
> `data-orientation` 统一映射过来。

也就是说：**类名里的 `data-active:` / `data-checked:` / `data-horizontal:`
之所以能生效，全靠元素上真实存在的 `data-state` / `data-orientation` 属性。**
Rust 侧没有 Radix 帮我们写这些属性，所以必须**手写**：

| 组件 | 输出的属性 | 让哪些类名生效 |
| --- | --- | --- |
| `Switch` | `data-state="checked\|unchecked"`、`data-size` | `data-checked:` / `data-unchecked:` / `data-[size=…]:` |
| `Checkbox` | `data-state="checked\|unchecked\|indeterminate"` | `data-[state=checked]:` / `data-[state=indeterminate]:` |
| `Tabs` | `data-orientation="horizontal\|vertical"` | `data-horizontal:` / `data-vertical:` |
| `TabsList` | `data-variant="default\|line"` | `group-data-[variant=line]/tabs-list:` |
| `TabsTrigger` | `data-state="active\|inactive"` | `data-active:` |
| `Separator` | `data-orientation`、`role`、`aria-*` | `data-horizontal:` / `data-vertical:` |
| `Avatar` | `data-size="default\|sm\|lg"` | `data-[size=lg]:` / `group-data-[size=sm]/avatar:` |

每个元素还保留了 `data-slot="…"`（`button` / `card-header` / `tabs-trigger` …），
因为 `packages/ui` 里有大量 `has-data-[slot=card-action]:` 这类**父选择器变体**
依赖它。

### 3.5 类名拼装与组件渲染解耦

每个模块都导出**纯函数** `*_classes()`，组件只是把它贴到元素上：

```rust
pub fn badge_classes(variant: BadgeVariant, class: &str) -> String {
    cn(&[BADGE_BASE, variant.classes(), class])
}

#[component]
pub fn Badge(variant: BadgeVariant, class: String, children: Element) -> Element {
    rsx! { span { class: badge_classes(variant, &class), "data-slot": "badge", … } }
}
```

于是「variant 表有没有拼错」这件事可以用**普通单测**验证，
不需要渲染器、不需要 wasm、不需要快照文件。

### 3.6 `Tabs` 的 context：为什么不用 `defaultValue`

Radix 的 `Tabs.Root` 同时支持受控（`value`）与非受控（`defaultValue`），靠内部
state 兜底。Dioxus 的 `use_context_provider` 只在**首次渲染**建立 context，
把内部 `Signal` 塞进去在受控模式下会读到陈旧值。

因此 `Tabs` 统一采用**调用方持有状态**：

```rust
let tab = use_signal(|| "general".to_string());
rsx! {
    Tabs { value: tab,
        TabsList { variant: TabsListVariant::Line,
            TabsTrigger { value: "general", "常规" }
        }
        TabsContent { value: "general", "…" }
    }
}
```

`Signal<T>` 是句柄（generational box），标识跨渲染稳定，所以 context 里存的
句柄永远指向最新值 —— 既没有陈旧问题，也天然同时覆盖受控 / 非受控两种用法。

### 3.7 有意的行为差异

| 组件 | Radix / TS | 本移植 | 原因 |
| --- | --- | --- | --- |
| `TabsContent` | 默认**卸载**非激活面板 | 保持挂载 + `hidden` | 桌面端长驻面板不该因切 tab 丢子组件状态 |
| `AvatarImage` | `onload`/`onerror` + 延迟挂载判定 | `onerror` → 隐藏自身 | 复刻「加载失败回退 Fallback」，但不引入 Radix 的 delayMs 逻辑 |
| `Checkbox` 指示器 | 非 unchecked 才挂载 | 始终挂载，靠类名显隐 | TS 里本来就有 `[&[data-state=indeterminate]_…]:hidden` 规则，语义等价且省一次挂载 |
| `Spinner` 文案 | `useZCodeIntl()` 取 `common.loading` | `label` prop（默认 `"Loading"`） | 不把 i18n 运行时依赖拖进组件库 |
| `Button` `asChild` | Radix `Slot` 把样式委托给子元素 | 未移植 | Dioxus 0.6 无对应原语；需要时改用 `button_classes()` 自行贴类名 |
| `CollapsibleContent` / `AccordionContent` | `Presence`：关闭后**卸载** DOM | 保持挂载 + `hidden` | 子树状态（输入框内容、滚动位置）在反复折叠时不丢；代价是退场动画不播放 |
| `ScrollArea` | Radix 的 JS 滚动条（`ResizeObserver` 测量 + 自绘 thumb） | 原生 `overflow-auto`，可选隐藏滚动条 | 桌面端原生滚动更顺、可访问性更好；自绘滚动条需要浏览器运行时，无头环境无法验证 |
| 浮层定位 | Radix `Portal` + `@floating-ui`（碰撞检测 / 自动翻转） | 锚点包裹层 + `absolute` 类名 | 见 §4.5；不做碰撞检测，位置由调用方给的 `side` / `align` / `side_offset` 决定 |
| `DropdownMenu` 高亮态 | Radix 内部 roving focus | 每项一个 `Signal<bool>` 写 `data-highlighted` | 只做 hover 高亮，不做键盘漫游（见 `dropdown_menu.rs` 模块文档） |
| `Collapsible` / `Accordion` 的 `id` | `useId()` 自动生成，并自动接 `aria-controls` | 显式 prop，**不传则不发该属性** | Dioxus 0.6 没有「每实例稳定 id」原语；Trigger 与 Content 是两个组件，拿不到同一个 id。发 `id=""` 是无效 HTML，所以宁可整个不发 |
| `InputGroupButton` | 委托给 `Button` | 渲染裸 `button` + `button_classes()` | 与 TS 一致（TS 里它就是 `<button className={button({...})}>`，不是 `<Button>`） |

### 3.8 不 vendor 引擎，而是把引擎注入

最后 9 个组件（`carousel` / `chart` / `code-viewer` / `diff-viewer` /
`pdf-viewer` / `pptx-preview-viewer` / `lightweight-diff-preview` /
`highlighted-lightweight-diff-preview` / `pptx-selection-action-bar`）
全部卡在同一件事上：**它们背后是一个无法移植的 JS 引擎**。

| 组件 | 背后的引擎 | 为什么不能移植 |
| --- | --- | --- |
| `diff-viewer` | `@pierre/diffs` | Shadow DOM 里自绘，无 Rust 等价物 |
| `code-viewer` | `@pierre/diffs` + shiki | 同上；shiki 的 tokenizer 是 oniguruma WASM |
| `pdf-viewer` | `react-pdf` / `pdf.js` | worker + canvas + 原生 PDF 解析 |
| `pptx-preview-viewer` | `pptx-renderer` | 直接写 DOM 的 OOXML 渲染器 |
| `chart` | `recharts` | 依赖真实 SVG 测量与响应式容器 |
| `carousel` | `embla-carousel-react` | 依赖真实滚动吸附与手势测量 |

面对这种情况有两条路：**vendor 一个 Rust 引擎**（等于换库重写，且质量不可控），
或者**把引擎当外部依赖注入**。这里选后者，做法固定成三步：

1. **抽出纯逻辑**：状态机（`CarouselState`）、坐标换算
   （`pdf_zoom_overlay` 的锚点、`presentation_element_model` 的单元格矩形）、
   缓存键（`create_code_viewer_file` 的 `cacheKey`）、
   渲染参数（`DiffViewerRenderPlan` / `CodeViewerRenderOptions`）、
   类名（全部 `*_classes()`）；
2. **定义注入点**：能用 `children: Element` 就用它（`pdf-viewer` /
   `pptx-preview-viewer` / `code-viewer` / `diff-viewer` 的画布），
   需要「每项一份数据」时用参数（`thumbnails` / `PptxThumbnail::content_width`），
   需要调用引擎时用 trait（`shiki_highlighter` 的 `ShikiLanguageCatalog` /
   `CodeHighlighter`，因为 **Dioxus 的 props 需要 `PartialEq`，
   而 `Rc<dyn Trait>` 不是 `PartialEq`**）；
3. **把「不能移植」写进模块文档**：每个模块的「与 TS 的差异」一节都逐条列出
   哪一段交给了宿主、为什么，以及 `unsafeCSS` / Shadow DOM 查询这类
   DOM 层职责的归属。

这样做的收益是**每个模块都仍然可以被完整测试** —— 纯函数层有单测，
外壳层有 `dioxus-ssr` 快照，而引擎的缺席只影响「画面」，不影响「逻辑」。
代价是宿主需要多写一点接线代码，这部分记在 §4.4 的「宿主职责」列里。

## 4. 已移植组件清单

| Rust 模块 | TSX 来源 | 导出 |
| --- | --- | --- |
| `components/button.rs` | `button.tsx` | `Button`、`ButtonVariant`、`ButtonSize`、`button_classes` |
| `components/badge.rs` | `badge.tsx` | `Badge`、`BadgeVariant`、`badge_classes` |
| `components/card.rs` | `card.tsx` | `Card` / `CardHeader` / `CardTitle` / `CardDescription` / `CardAction` / `CardContent` / `CardFooter`、`CardSize` |
| `components/alert.rs` | `alert.tsx` | `Alert` / `AlertTitle` / `AlertDescription` / `AlertAction`、`AlertVariant` |
| `components/input.rs` | `input.tsx` | `Input`、`InputSize`、`input_classes` |
| `components/textarea.rs` | `textarea.tsx` | `Textarea`、`textarea_classes` |
| `components/label.rs` | `label.tsx` | `Label`、`label_classes` |
| `components/separator.rs` | `separator.tsx` | `Separator`、`SeparatorOrientation` |
| `components/progress.rs` | `progress.tsx` | `Progress`、`ProgressSegment`、`normalize_value`、`normalize_segments` |
| `components/kbd.rs` | `kbd.tsx` | `Kbd`、`KbdGroup` |
| `components/spinner.rs` | `spinner.tsx` | `Spinner`（内联 lucide `loader` 路径） |
| `components/avatar.rs` | `avatar.tsx` | `Avatar` / `AvatarImage` / `AvatarFallback` / `AvatarBadge` / `AvatarGroup` / `AvatarGroupCount`、`AvatarSize` |
| `components/switch.rs` | `switch.tsx` | `Switch`、`SwitchSize` |
| `components/checkbox.rs` | `checkbox.tsx` | `Checkbox`、`CheckboxState`（内联 lucide `check` / `minus`） |
| `components/tabs.rs` | `tabs.tsx` | `Tabs` / `TabsList` / `TabsTrigger` / `TabsContent`、`TabsListVariant`、`TabsOrientation`、`use_tabs_value` |
| `cn.rs` | `lib/utils.ts` | `cn`、`cn_opt` |

### 4.1 第二批：折叠类 / 表单组合

| Rust 模块 | TSX 来源 | 导出 |
| --- | --- | --- |
| `components/collapsible.rs` | `collapsible.tsx` | `Collapsible` / `CollapsibleTrigger` / `CollapsibleContent` |
| `components/accordion.rs` | `accordion.tsx` | `Accordion` / `AccordionItem` / `AccordionTrigger` / `AccordionContent`、`AccordionType`、`AccordionOrientation` |
| `components/radio_group.rs` | `radio-group.tsx` | `RadioGroup` / `RadioGroupItem` |
| `components/button_group.rs` | `button-group.tsx` | `ButtonGroup` / `ButtonGroupText` / `ButtonGroupSeparator`、`ButtonGroupOrientation` |
| `components/input_group.rs` | `input-group.tsx` | `InputGroup` / `InputGroupAddon` / `InputGroupButton` / `InputGroupText` / `InputGroupInput` / `InputGroupTextarea`、`InputGroupAddonAlign`、`InputGroupButtonSize` |
| `components/scroll_area.rs` | `scroll-area.tsx` | `ScrollArea` / `ScrollBar` / `ScrollThumb`、`ScrollOrientation` |

### 4.2 第二批：浮层类

| Rust 模块 | TSX 来源 | 导出 |
| --- | --- | --- |
| `components/tooltip.rs` | `tooltip.tsx` | `TooltipProvider` / `Tooltip` / `TooltipTrigger` / `TooltipContent` |
| `components/hover_card.rs` | `hover-card.tsx` | `HoverCard` / `HoverCardTrigger` / `HoverCardContent` |
| `components/popover.rs` | `popover.tsx` | `Popover` / `PopoverTrigger` / `PopoverAnchor` / `PopoverContent` / `PopoverHeader` / `PopoverTitle` / `PopoverDescription` |
| `components/dialog.rs` | `dialog.tsx` | `Dialog` / `DialogTrigger` / `DialogPortal` / `DialogOverlay` / `DialogContent` / `DialogClose` / `DialogHeader` / `DialogFooter` / `DialogTitle` / `DialogDescription` |
| `components/alert_dialog.rs` | `alert-dialog.tsx` | `AlertDialog` / `AlertDialogTrigger` / `AlertDialogPortal` / `AlertDialogOverlay` / `AlertDialogContent` / `AlertDialogAction` / `AlertDialogCancel` / `AlertDialogHeader` / `AlertDialogFooter` / `AlertDialogTitle` / `AlertDialogDescription` |
| `components/dropdown_menu.rs` | `dropdown-menu.tsx` | `DropdownMenu` / `…Trigger` / `…Portal` / `…Content` / `…Group` / `…Item` / `…CheckboxItem` / `…RadioGroup` / `…RadioItem` / `…Label` / `…Separator` / `…Shortcut` / `…Sub` / `…SubTrigger` / `…SubContent`、`DropdownMenuItemVariant` |

### 4.3 第三批：展示 / 动画 / 组合类（11 个）

| Rust 模块 | TSX 来源 | 导出 |
| --- | --- | --- |
| `components/brand.rs` | `ZCodeAboutLogo.tsx` | `ZCodeAboutLogo` / `ZCodeWordmarkLogo`、`ZCODE_LOGO_BASE` |
| `components/glm_icon.rs` | `GlmMonochromeIcon.tsx` | `GlmMonochromeIcon`、`glm_monochrome_filter`、`GLM_FILTER_DARK` / `GLM_FILTER_LIGHT` |
| `components/scroll_fade_viewport.rs` | `scroll-fade-viewport.tsx` | `ScrollFadeViewport`、`ScrollMaskState`、`scroll_mask_state` |
| `components/toast.rs` | `toast.tsx` | `ToastProvider` / `ToastHost` / `ToastMessage` / `AnchoredToastStack`、`ToastStore`、`use_toast_store`、`toast` / `toast_with` / `update_toast` / `dismiss_toast`、`ToastPosition` / `ToastVariant`、`resolve_toast_stack_classes` |
| `components/select.rs` | `select.tsx` | `Select` / `SelectTrigger` / `SelectValue` / `SelectContent` / `SelectGroup` / `SelectLabel` / `SelectItem` / `SelectRichItem` / `SelectSeparator` / `SelectScrollUpButton` / `SelectScrollDownButton`、`SelectNav` / `select_navigate`、`SelectTriggerVariant` / `SelectTriggerSize` / `SelectContentPosition` |
| `components/command.rs` | `command.tsx` | `Command` / `CommandDialog` / `CommandInput` / `CommandList` / `CommandEmpty` / `CommandGroup` / `CommandItem` / `CommandSeparator` / `CommandShortcut`、`CommandNav` / `command_navigate`、`command_filter_matches` / `command_visible_indices` |
| `components/context_menu.rs` | `context-menu.tsx` | `ContextMenu` / `ContextMenuTrigger` / `ContextMenuContent` / `ContextMenuItem` / `ContextMenuSeparator` / `ContextMenuSub` / `ContextMenuSubTrigger` / `ContextMenuSubContent`、`CursorPoint`、`ContextMenuItemVariant` |
| `components/resizable.rs` | `resizable.tsx` | `ResizablePanelGroup` / `ResizablePanel` / `ResizableHandle`、`clamp_size` / `normalize_sizes` / `resize_pair`、`MemoryLayoutStore`、`ResizableOrientation`、`ArrowKey` / `key_to_arrow` / `arrow_to_delta` / `px_to_percent` |
| `components/floating_particles.rs` | `floating-particles.tsx` | `FloatingParticles`、`particles` / `seeded_unit` / `lerp`、`particle_frame` / `particle_frame_draws` / `should_draw_frame`、`PARTICLE_CHANNEL_TIMES` |
| `components/smooth_metric_value.rs` | `smooth-metric-value.tsx` | `SmoothMetricValue`、`SmoothMetricState`、`MetricValue`、`normalize_smooth_metric_target` / `normalize_smooth_metric_target_str` / `clamp_metric_speed` / `estimate_smooth_metric_lines_per_second` / `resolve_next_smooth_metric_display` |
| `components/flip_metric_value.rs` | `flip-metric-value.tsx` | `FlipMetricValue`、`metric_characters` / `metric_slot_classes` / `uses_flip_slot` / `is_digit_character` / `character_width_class`、`FLIP_TRANSITION_DURATION_MS` / `FLIP_TRANSITION_EASE` |

### 4.4 第四批：重型预览（注入式移植，9 个）

这 9 个背后都是不可移植的 JS 引擎，按 §3.8 的三步落地。
「宿主职责」列列出**没有**被移植、需要调用方接上的部分。

| Rust 模块 | TSX 来源 | 导出 | 宿主职责 |
| --- | --- | --- | --- |
| `components/carousel.rs` | `carousel.tsx` | `Carousel` / `CarouselContent` / `CarouselItem` / `CarouselPrevious` / `CarouselNext`、`CarouselState`、`CarouselOrientation`、`resolve_carousel_key_action` | 替换 `embla` 的滚动吸附（`basis-full` 下 `CarouselState` 的索引机与 embla 等价） |
| `components/chart.rs` | `chart.tsx` | `ChartContainer` / `ChartTooltipContent` / `ChartLegendContent`、`ChartConfig` / `ChartConfigItem`、`ChartPayloadObject`、`ChartIndicator`、`ChartValue` / `format_chart_value`、`build_chart_style` | 把配置交给 `recharts`；本模块只产出配置数据与类名 |
| `components/code_viewer.rs` | `code-viewer.tsx` | `CodeViewer` / `CodeCommentAnnotation` / `CodeCommentDraft`、`CodeCommentLabels` / `CodeCommentRange` / `CodeCommentPreview`、`create_code_viewer_file` / `hash_code_viewer_content`、`build_line_annotations` / `build_selected_lines`、`code_viewer_marked_lines_css`、`code_viewer_style` | 渲染 `@pierre/diffs` 的 `<File>`（`children`）；Shadow DOM 内的行元素查询与 gutter 按钮 `title` 补丁；`installDocumentLinkSafety` |
| `components/diff_viewer.rs` | `diff-viewer.tsx` | `DiffViewer`、`DiffViewerRenderPlan`、`DiffViewerOptions`、`DiffViewerInput` / `DiffViewerKind`、`DiffViewerSelectedLineRange` / `DiffViewerTheme`、`build_diff_viewer_render_plan` / `diff_viewer_worker_options` | 渲染 `<PatchDiff>` / `<MultiFileDiff>`（`children`） |
| `components/pdf_viewer.rs` | `pdf-viewer.tsx` | `PdfViewer`、`PdfViewerSource` / `PdfViewerRangeSource`、`PdfViewerLabels`、`PdfViewerControlsState` / `build_pdf_viewer_controls`、`go_to_page` / `commit_page_input` / `resolve_pdf_viewer_key_action`、`PdfViewerWheelZoom`、`create_pdf_viewer_range_document_options` | 渲染 `<Document>` + `<Page>`（`children`）；`wheel` 的**非 passive** 原生监听；range 传输的 `requestRange` 回调 |
| `components/pptx_preview_viewer.rs` | `pptx-preview-viewer.tsx` | `PptxPreviewViewer` / `PptxThumbnail`、`PptxPreviewViewerLabels`、`clamp_presentation_page_number` / `clamp_presentation_zoom_percent`、`pptx_main_scale`、`resolve_pptx_viewer_key_action`、`resolve_pptx_reference_navigation`、`suggested_pptx_pdf_name` | `pptxRendererPreviewEngine` 的 `open` / `renderPage` / `getPageElements`；缩略图列的 `IntersectionObserver` 懒渲染；缩略图宽度测量；导出 PDF 的 `printPageToPdf` / `saveFile` |
| `components/lightweight_diff_preview.rs` | `lightweight-diff-preview.tsx` | `LightweightDiffPreview`、`LightweightDiffLineParts` / `get_lightweight_diff_line_parts`、`LightweightDiffLineStyles` / `get_lightweight_diff_line_styles`、`format_truncated_label` | 无需引擎（纯 DOM 渲染，本模块自己画） |
| `components/highlighted_lightweight_diff_preview.rs` | `highlighted-lightweight-diff-preview.tsx` | `HighlightedLightweightDiffPreview`、`get_highlighted_lightweight_diff_line`、`build_highlighted_lightweight_diff_code`、`should_tokenize_highlighted_lightweight_diff`、`tokenize_highlighted_lightweight_diff` | 提供 `CodeHighlighter`（shiki）实现，见 `shiki_highlighter.rs` |
| `components/pptx_selection_action_bar.rs` | `pptx-selection-action-bar.tsx` | `PptxSelectionActionBar`、`resolve_pptx_selection_escape`、`is_ime_composing_key_event`、`pptx_selection_resets_draft`、`pptx_selection_content_class` | 无需引擎（纯浮层） |

> 这三个「无需引擎」的组件仍然被归在第四批，是因为它们和另外 6 个共享同一批
> 支撑模块（`patch_diff_preview` / `shiki_highlighter` / `keyboard_shortcuts`），
> 拆开列反而看不出依赖关系。

### 4.5 支撑模块

| Rust 模块 | 作用 |
| --- | --- |
| `icons.rs` | **内联 lucide 图标**，零图标库依赖。`lucide!` 宏把 `<svg>` 的公共属性（`viewBox` / `stroke` / `stroke-width` / `stroke-linecap` / `stroke-linejoin`）固化，手写几何；`data-slot` 默认取图标名（`plus` / `check` / `chevron-down`…），这样 TS 里 `**:data-[slot=accordion-trigger-icon]:size-4` 这类选择器能原样命中 |
| `components/overlay.rs` | **浮层定位约定**。Radix 的 `Portal` + `@floating-ui` 在这里被替换成「锚点包裹层 + `absolute` 类名」：`Side`（Top/Right/Bottom/Left）、`Align`（Start/Center/End）、`side_align_classes(side, align, side_offset)` 生成形如 `absolute bottom-full left-1/2 -translate-x-1/2 mb-[4px]` 的类名，`OVERLAY_ANCHOR_BASE = "relative inline-flex"` 是锚点基准 |

第四批还引入了 8 个**crate 根模块**，它们镜像的是 TS 的 `lib/*` 与
`presentation/*`（纯逻辑，与具体组件解耦）：

| Rust 模块 | TS 来源 | 作用 |
| --- | --- | --- |
| `code_preview_preferences.rs` | `lib/codePreviewPreferences.ts` | 代码主题白名单。明暗判定靠**白名单**不靠正则 —— `catppuccin-mocha` / `github-light-high-contrast` 这种名字里没有 `light`/`dark` 的主题正则猜不出来 |
| `keyboard_shortcuts.rs` | `lib/keyboardShortcuts.ts` | 平台主修饰键（`⌘` / `Ctrl`）、`event.code` 回退（macOS 的 Option 会改写 `event.key`）、快捷键文案 |
| `diffs_highlighter_engine.rs` | `lib/diffsHighlighterEngine.ts` | 高亮器配置。记录为什么选 oniguruma WASM 而不是 shiki-js：后者的巨型正则常驻 V8 **code space**（主窗口 + 4 个 diff worker 共享 256 MB），会 OOM |
| `shiki_highlighter.rs` | `lib/shikiHighlighter.ts` | 语言归一化、token 缓存键、`create_raw_code_tokens`；引擎通过 `ShikiLanguageCatalog` / `CodeHighlighter` 两个 trait 注入 |
| `patch_diff_preview.rs` | `lib/patchDiffPreview.ts` | 「这段 patch 能不能进富渲染器」的判定（行数 / 字符数 / hunk 行号上限 + lockfile 强制纯文本），以及纯文本回退的行收集 |
| `pdf_zoom_overlay.rs` | `components/ui/usePdfZoomOverlay.ts` | PDF 缩放的数学：两套倍率（手势走 CSS transform、停顿后提交真实渲染）、指针锚点、滚轮指数缩放 |
| `pdf_js_assets.rs` | `lib/pdfJsAssets.ts` | 算 `cMapUrl`。ReportLab 的 `STSong-Light` 等 Type0 字体只声明预定义 CMap、未内嵌字符映射，不给这个路径就是一片空白 |
| `presentation_element_model.rs` | `presentation/presentationElementModel.ts` | PPTX 元素模型：节点树 → 可命中矩形表（含表格单元格的比例换算与合并单元格推进规则）+ 命中测试 |

> **为什么 Dialog 不需要这套约定**：Dialog / AlertDialog 的内容用 `position: fixed`，
> containing block 是**视口**而不是最近的定位祖先，所以没有 Portal 也能居中盖满整屏。
> 只有相对锚点定位的浮层（Tooltip / HoverCard / Popover / DropdownMenu）才需要 §4.3 的约定。

### 4.6 可运行的应用：`crates/zcode-app`
把移植出的原语拼成一个 ZCode 风格的外壳，用来证明「组件库真的能用」。

| 文件 | 内容 |
| --- | --- |
| `src/lib.rs` | `App` / `AppShell` / `WorkspaceBootstrap` / `WorkspaceSeed`；注入 `document::Title` + `document::Style`；`APP_CSS` 用 `include_str!` 内嵌 `assets/app.css` |
| `src/main.rs` | 桌面入口：`LaunchBuilder::desktop().with_cfg(WindowBuilder::new().with_title(..).with_inner_size(1360×860))` |
| `src/state.rs` | `AppState` / `AppCtx`（全部 `Signal`）/ `use_app()` / `demo_state()`；`diff_preview` 是**单个信号**（`Option<DiffPreviewState>`），其 `path` 就是变更行的主键 |
| `src/workspace.rs` | **真实数据桥**：`WorkspaceRequester`（后台线程 + `futures-channel`）、`use_workspace_requester()`、`WorkspaceStatus` 四态；`refresh()` 读、`run(WorkspaceAction)` 写，两者共用同一个 `busy` 原子量**串行化**；`load_diff()` 读补丁，**刻意不共用 `busy`**（读 diff 不碰 `index.lock`），代价是每次请求带一个单调递增 `id`，轮询端只采纳当前那次的结果；`WorkspaceNotice` 回传单次写操作的成败 |
| `src/views/sidebar.rs` | 品牌行 + 搜索（`InputGroup`）+ 按「今天/昨天/更早」分组的会话列表 + 右键菜单（`DropdownMenu`，destructive 删除）+ 上下文用量（`Progress`）+ 头像；**底部显示真实工作区目录名与「分支 · N 个变更」** |
| `src/views/topbar.rs` | 会话标题 + **`WorkspaceBadge`**（五态：失败 / 加载中 / 未扫描 / 非仓库 / 真实分支 + ahead-behind + 变更数）+ 视图 `Tabs` + 模型选择器（`DropdownMenuRadioItem`）+ 带 `Tooltip` 的图标按钮（刷新按钮接 `requester.refresh()`） |
| `src/views/changes.rs` | **「变更」页**：`WorkspaceBar`（分支 / 仓库根 / 汇总 / 刷新 + **推送按钮** + **提交输入框与提交按钮**）+ 按「已暂存 / 未暂存 / 未跟踪 / 冲突」分节（每组带 **全部暂存 / 全部丢弃**）+ `ActionableChangeRow`（`+N/-M` + 行内 **暂存** 按钮 + 垃圾桶按钮，默认 `opacity-40`、hover 才完全显形；**整行可点，点开就地展开真实补丁**）+ `DiffPreviewPane`（四种状态：有补丁 / 加载中 / 不可用且说明原因 / 命令失败）+ `NoticeBanner`（成功 / 失败提示条）+ 丢弃前的 `AlertDialog` 二次确认 + 加载 / 干净 / 非仓库 / 失败 / 空 五种落空态 + `NotPortedPane` |
| `src/views/conversation.rs` | 消息流；助手头像挂 `HoverCard`；工具调用卡片（分段 `Progress`）；流式提示用受控 `Collapsible` + `Alert` |
| `src/views/composer.rs` | `InputGroup`（`InlineStart` 回形针 / `Textarea` / `InlineEnd` 发送或停止）+ 审批模式选择器 |
| `src/views/detail.rs` | 执行计划（受控 `Collapsible`）+ **变更文件（`ChangesSection`，真实数据）** + 工具调用 / 上下文来源（`Accordion`）+ 权限 `RadioGroup` + 分段 `Progress` + 图例 + `AvatarGroup` + `ButtonGroup` |
| `src/views/settings.rs` | 设置 `Dialog`（常规/外观/关于 三个 `Tabs`）+ 删除确认 `AlertDialog` |
| `src/views/gallery.rs` | 组件画廊 `Dialog`，六个分页把所有原语和变体都渲染一遍 |
| `src/views/mod.rs` | `WorkspacePane()` —— 按 `workspace_tab` 切主区（`changes` → `ChangesView`、`tasks` → `NotPortedPane`、其余 → `ConversationView`），抽出来是为了让快照测试能注入 tab |

> **为什么把「失败」做成一个显式状态**：上一轮的问题是窗口看起来像「空壳」，
> 而真实原因是一个被吞掉的错误。现在 `WorkspaceStatus::Failed(String)` 会渲染
> 一个 destructive 的 `FailurePane`，顶栏徽标也变红 —— **宁可显示报错，也不要显示空白**。

### 4.7 整屏静态快照

```bash
cargo run -p zcode-app --example app_preview
# → ./app-preview.html          （对话页，自包含，约 249 KB）
# → ./app-preview-changes.html  （变更页，自包含，约 321 KB，含一行展开的补丁）
```

把 [`AppShell`] 用 `dioxus-ssr` 渲染成 HTML，再把 `APP_CSS` 原样包进 `<style>`，
产出**单文件、无外部依赖**的页面（不需要 CDN / 本地服务 / wasm）。
它和 `cargo run -p zcode-app` 开原生窗口是两条独立的验证路径：
前者能进 CI、能被 diff，后者本应证明窗口真的能开 —— 但在本机
**后者只开得出空白窗口**（原因见 [README.md](./README.md) 的「已知问题」），
所以这个静态快照是目前唯一能看到 UI 的路径，也是「组件树本身是对的」的直接证据。

**这个 example 会真的跑一次 git 查询**（同步调用 `zcode-services` 那套移植过来的
代码），把结果通过 `WorkspaceSeed` 注入界面 —— 所以快照里的分支名、文件名、
`+N/-M` 都是你当前仓库的真实状态，换 `ZCODE_WORKSPACE` 就会换一批内容。
变更页还会**再挑一行真的去读补丁**并展开（`scan_first_diff` 优先挑一个不超过
80 行、且每行不超过 200 字符的补丁 —— 否则会挑中压缩成单行的 `app.css`，
截图上就只剩一条横线）。
之所以同步扫而不是等后台线程：SSR 只渲染一帧，没有事件循环去驱动 `spawn`
出来的 future。真实窗口里走的是后台线程那条路（见
`crates/zcode-app/tests/workspace_loader.rs`）。

> 渲染的是 `AppShell` 而不是 `App` —— `App` 用 `document::Style` 注入样式，
> 而那需要真实渲染器，SSR 下没有实现（这也是两者被拆开的原因）。
> `WorkspaceSeed` 让 `AppShell` 在测试 / SSR 下拿到现成数据，
> 于是**永远不会**在快照测试里真的去跑 git。

### 4.8 样式表怎么来的

上游 `packages/ui` 的样式来自 `shadcn/tailwind.css`（一个不在本仓库的依赖）。
本分支不装 Tailwind、也不含上游 CSS，所以 `.tailwind-build/build.sh` 手工拼一份入口 CSS：

1. `@import "tailwindcss" source(none)` —— 关掉自动扫描，只扫我们指定的路径；
2. `@import "tw-animate-css"` —— 提供 `animate-in` / `fade-in-0` / `animate-collapsible-down` 等；
3. `@source "../crates/**/*.rs"` —— 让 Tailwind 从 **Rust 源码**里提取类名
   （注意路径相对 `.tailwind-build/`，写成 `../../crates` 会扫到空目录，产出的 CSS 会静默变空）；
4. 手写补上 `shadcn/tailwind.css` 里那组 `@custom-variant`
   （`data-open` / `data-closed` / `data-active` / `data-checked` / `data-horizontal` /
   `data-vertical` / `data-disabled` / `data-highlighted` / `data-inset`）；
5. `cat .tailwind-build/tokens.css` —— 设计令牌（`@theme { … }` + `.dark { … }` 两块）。

第 5 步的 `tokens.css` 是**从上游 `packages/ui/src/styles.css` 逐字抽取后 vendored 进来的**。
之所以 vendor 而不是每次去读上游文件：本分支只有 Rust 代码，上游 CSS 不在这里
（参照源在 `rust` 分支）。上游若更新令牌，重新抽取并覆盖即可，方法写在
`tokens.css` 顶部注释里。

> 验证方式：把 vendored 版本生成的 `app.css` 与改动前逐字节比对。
> 注意**每次新增组件或改类名都要重新跑一遍 build.sh** —— 上一轮的「变更」页
> 就引入了 6 个此前没被扫到的类名（`border-destructive` / `break-all` / `h-11` /
> `last:border-b-0` / `p-8` / `py-12`）；接写操作时又多了 5 个
> （`group` / `group-hover:opacity-100` / `opacity-40` / `pr-1.5` / `transition-opacity`），
> 漏掉它们界面会「类名在、效果不在」—— 行内按钮会**永远半透明**。
> 这一轮接 diff 预览时还踩到一个**不是漏生成、而是令牌根本不存在**的例子：
> 面板底色原本写成 `bg-muted/30`，但本设计系统里**根本没有 `--color-muted`
> 这个令牌**（`tokens.css` 里连 `muted` 这个词都搜不到），于是类名进了 DOM、
> 样式表里一条规则都没有，面板会保持透明。
> 改用真实存在的 `bg-background-alt` 才对。
> 排查办法是脚本化比对：把 `changes.rs` 里所有 `class:` 里的 token 抽出来，
> 逐个到 `app.css` 里查有没有规则，剩下的就是这类「静默失效」的类名。

```bash
bash .tailwind-build/build.sh   # → crates/zcode-app/assets/app.css（117,806 字节）
```

> ⚠️ Tailwind v4 会**按需裁剪** `@theme`：只有被实际用到的令牌才会作为 CSS 变量输出。
> 所以「样式表里没有 `--text-ui-2xs`」不代表 `@theme` 没被导入 —— 只是当前代码没用它。
> 冒烟测试因此既查具体令牌，也查 `--color-*` 的总量（>200）来确认整块 `@theme` 确实进来了。

## 5. 测试策略

六层，**全部在 host 上跑**（无 wasm / 无窗口 / 无浏览器）：

| 层 | 文件 | 数量 | 断言粒度 |
| --- | --- | --- | --- |
| 纯函数单测 | `zcode-ui/src/**` 内联 `#[cfg(test)]` | 742 | 字符串包含 / 相等 / 数值 |
| mutation 记录式渲染 | `zcode-ui/tests/render.rs` | 11 | 区分静态属性 / 动态属性 / 文本节点 |
| HTML 快照 | `zcode-ui/tests/html_snapshot.rs` | 75 | 序列化后的 HTML 字符串 |
| 整树冒烟（真实数据注入） | `zcode-app/tests/smoke.rs` | 36（+1 ignored） | 把整个应用（含浮层）真渲染一遍，变更页用 fixture 注入五种数据状态 + 写操作入口与提示条 + **diff 预览的四种状态** |
| 跨线程读 / 写 / diff 链路 | `zcode-app/tests/workspace_loader.rs` | 9 | 手写 executor 驱动 `refresh` / `run` / `load_diff → 后台线程 → 通道 → 结果`，不依赖 dioxus 运行时 |
| git 端到端 | `zcode-services/tests/git_workspace.rs` | 37 | 真起临时仓库，跑真实 `git status` / `git log` / `git config` / `git diff`（读 17）与 `stage` / `unstage` / `discard` / `commit` / `push`（写 20） |

各 crate 明细：

| crate | 单元 | 集成 | 小计 |
| --- | --- | --- | --- |
| `zcode-rpc` | 10 | — | 10 |
| `zcode-shared` | 19 | — | 19 |
| `zcode-services` | 144 | 37 | 181 |
| `zcode-ui` | 742 | 75 + 11 | 828 |
| `zcode-app` | 33 | 36 + 9 | 78 |
| **合计** | **948** | **168** | **1116** |

> **为什么「变更」页的冒烟测试不跑真 git**：快照测试只跑同步代码，
> `spawn` 出来的 future 根本不会被 poll，所以 `AppShell` 在拿不到
> `WorkspaceBootstrap` 时不会发起扫描。测试用 `WorkspaceHarness` 直接注入
> `WorkspaceSeed`（`changes` / `clean` / `not-repo` / `failed` / `loading` 五个 fixture）
> 与 `WorkspaceNotice`（成功 / 失败两种），
> 于是**渲染层测试完全确定**，真 git 的行为交给 `zcode-services` 那两层去测。
>
> 写操作那条路因此被拆成三段各自验证：**渲染**（smoke 断言按钮 / 提交框 /
> 确认弹窗在不在）、**通道语义**（loader 断言 `Loading → Notice → Loaded`
> 的顺序与内容）、**git 行为**（services 断言文件真的进了索引 / 真的回到 HEAD）。
>
> **diff 预览同理，也是三段**：smoke 注入四种 `DiffPreviewState` 断言面板长什么样、
> loader 断言「diff 请求**只回一条消息**」（它不发 `Loading` / `Loaded`，
> 因为读 diff 不刷新整个工作区）且并发请求的 `id` 互不相同、services 断言
> 未暂存 / 已暂存 / 未跟踪 / 二进制四种来源各自产出正确的补丁与可用性。
>
> ⚠️ 有一条断言值得单独记：**不能断言 DOM 里有 `-let a = 1;` 这样的字面量**。
> `LightweightDiffPreview` 会把行首的协议标记（`+` / `-` / 空格）**取出来**
> 用于着色，`<code>` 里只剩内容本身，标记体现在行元素的 `text-diff-removed` /
> `text-diff-added` 类名上。所以正确写法是断言内容
> （`let a = 1;` / `let a = 2;`）**加上**两个类名同时存在。

### 5.1 纯函数单测（742 个）

`*_classes()` 与 `cn()` 的直接断言。覆盖 variant/size 映射、用户 class 覆盖
组件默认、`text-ui-*` 与文字颜色不互相覆盖、方括号修饰符切分、
`ring` 宽度 vs 颜色、`shadow` 尺寸 vs 颜色、负向定位前缀
（`-translate-x-1/2` / `-inset-x-1` 的 tailwind-merge 分组）等。

第四批之后这一层扩大到**所有被抽出来的纯逻辑**，包括：

- **几何**：`pdf_zoom_overlay` 的锚点捕获/还原（指针在容器外要先夹进容器再算比例）、
  `presentation_element_model` 的表格单元格矩形与合并单元格的**不对称推进**
  （纵向续格推进列号、横向续格不推进）；
- **坐标系换算**：`code_viewer` 的「文件行号 ↔ 选中行 ↔ 注释锚点」三套坐标；
- **JS 数值语义**：`Math.round` 半数向 `+∞`（与 Rust 的 `f64::round` 不同）、
  `Number.parseInt` 的前缀解析、`JSON.stringify` 的转义、
  `Number.prototype.toString(36)`、`Math.max(0, NaN) = NaN` 的传染性；
- **UTF-16 而非 `char`**：`hash_code_viewer_content` 的 djb2 与 `code.length`
  都按 **UTF-16 码元**走，否则非 BMP 字符算出的 `cacheKey` 与 TS 不一致；
- **`Array.prototype.join` 把 `null` 变成 `""`**：`chart.rs` 里
  「主题没有颜色时输出一行空白」是**上游行为**，被单独钉成一条测试。

### 5.2 mutation 记录式渲染（11 个）

实现一个**记录型渲染器**（`impl WriteMutations`），把 Dioxus 下发的
「加载模板」（含编译期静态属性/文本）与「写入属性」（运行期动态属性）
全部收集起来，于是可以断言「DOM 长什么样」：

```rust
let mut vdom = VirtualDom::new(Showcase);
let mut rec = Recorder::default();
vdom.rebuild(&mut rec);          // ← 不能是 rebuild_in_place()，见下
```

> ⚠️ 踩坑记录：`rebuild_in_place()` 等价于 `rebuild(&mut NoOpMutations)`，
> 初始挂载的全部 mutation 会被丢进空实现 —— 什么也记录不到。
> 另一个坑：`rsx!` 里的**字面量文本**（含 `Button { "删除" }` 这种作为
> children 的字面量）会被编译进模板走 `TemplateNode::Text`，
> 而**不是** `create_text_node` mutation。断言文本必须查模板。

断言覆盖：`data-slot` 锚点（38 个）、variant/size 是否真的进了 `class`、
`data-state` / `data-orientation` / `role` / `aria-*`、
SVG 命名空间与 `viewBox` / `stroke-*`、`Progress` 的宽度与分段 `flex-basis`、
文本节点透传、受控 prop 在 DOM 上的反映。

### 5.3 HTML 快照（75 个）

`dioxus-ssr`（**仅 dev-dependency**，库本身不依赖它）把组件序列化成
浏览器会看到的 HTML，断言读起来跟看页面一样，适合做回归快照：

```rust
fn html(app: fn() -> Element) -> String {
    let mut dom = VirtualDom::new(app);
    dom.rebuild_in_place();
    dioxus_ssr::render(&dom)
}

assert_eq!(html(|| rsx! { Separator {} }), r#"<div class="shrink-0 bg-border …" …></div>"#);
```

> ⚠️ 踩坑记录：参数必须是 `fn() -> Element`（闭包）而不是 `Element`。
> `rsx!` 会在**调用方**上下文构造 props，而 `#[props(default)] onclick:
> EventHandler<..>` 的默认值需要 Dioxus runtime；直接传 `rsx!{…}` 会撞上
> `Must be called from inside a Dioxus runtime`。另外 `VirtualDom::new` 收的是
> **fn 指针**，所以测试闭包不能捕获变量 —— 需要传参就把参数提升成组件 props。

这一层还抓出了几个「序列化细节」并固化成断言：

- 数值属性不带引号：`rows=4`、`stroke-width=3`；
- 布尔属性渲染成 `disabled=true` / `hidden=true`，为 `false` 时**整个属性省略**；
- `BUTTON_BASE` 的 `text-ui-base/relaxed` 会被 size 的 `text-ui-base` 覆盖
  （同属 `font-size` 组，tailwind-merge 语义下后者胜）—— 与 TS 版一致；
- `dioxus-ssr` **不转义**属性值里的 `&`，所以 `[&::-webkit-scrollbar]:hidden`
  在 HTML 里就是原样，断言时不要写成 `&amp;`。

### 5.4 预览页生成器（`examples/preview.rs`）

```bash
cargo run -p zcode-ui --example preview            # → ./preview.html
```

生成一份可直接在浏览器打开的页面。页面里每个 `class` 都来自移植后组件的
`*_classes()` 纯函数（没有第二份「手抄类名」可以掩盖拼写错误），顶部那组控件
更是走 `dioxus_ssr::render_element` 的**真渲染**结果。样式由 Tailwind v4
浏览器版 + 从 `packages/ui/src/styles.css` 复刻的 `@theme` / `@custom-variant`
令牌驱动（需联网加载 CDN；离线打开会退化为无样式但结构正确的 HTML）。

### 5.5 整树冒烟 + 「能正常运行」的判定（36 + 1 ignored）

`zcode-app/tests/smoke.rs` 把 [`AppShell`] 整棵树、以及三个浮层（画廊 / 设置 /
删除确认）**真渲染一遍**，断言：

- 整棵树能挂载（任何 `use_context` 缺失或 hook 顺序错误都会在这里 panic）；
- 各区域文案真的出现（说明分支没走空）；
- 关键原语的 `data-slot` 锚点与 `data-*` 语义属性真的在 DOM 里 ——
  这是 shadcn `@custom-variant` 生效的前提，也是移植中最容易漏的东西；
- 浮层**默认关闭**时内容整体不渲染（`dialog-content` / `alert-dialog-content` 都不在）；
- 需要展开才有 DOM 的浮层另建 `OverlaysOpen` 夹具，用受控
  `open: Some(signal(true))` 强制打开后再断言内容标记；
- **「变更」页在五种数据状态下分别渲染出正确的内容**（真实变更列表 / 干净 /
  非仓库 / 加载失败 / 加载中），且失败态**必须显示错误信息而不是空白**；
- **写操作入口真的在**：每行有 `暂存` 与垃圾桶按钮、每个分组有 `全部暂存` /
  `全部丢弃`、`WorkspaceBar` 有提交输入框与提交按钮；非仓库时不渲染提交框
  （因为提交必然失败，留着只会误导）；
- **丢弃必须先确认**：默认渲染里 `alert-dialog-content` **不在** DOM 中，
  点垃圾桶才会出现 —— 「唯一会丢数据的操作」不能靠一次误点触发；
- **提示条能读出内容**：成功（绿）与失败（红）两种 `WorkspaceNotice`
  都渲染出完整文案，且在「非仓库」状态下也照样显示；
- **diff 预览的四种状态各自有落点**：默认**不展开**（`diff-preview` 不在 DOM 里）、
  有补丁时渲染真实行、加载中显示提示、二进制 / 截断 / 不可用 / 命令失败
  **都必须说明原因**（`data-slot="diff-unavailable"` + `data-diff-availability`），
  且展开预览**不会遮掉页面其余部分**（顶栏、其它分组仍在）；
- 顶栏 `WorkspaceBadge` 的五个分支各命中一次；「任务」tab 渲染 `NotPortedPane`
  而不是静默空白。

其中一条专门盯着「类名在 DOM 上、但样式表里没编译出规则」这个坑：
断言 `[scrollbar-width:none]` 既出现在 DOM 里，也出现在内嵌 CSS 里。

**跨线程读 / 写 / diff 链路**（`workspace_loader.rs`）另起一组：不启动 dioxus 运行时，
用手写的 `FlagWaker` + `poll_fn` 驱动 `WorkspaceRequester`，断言：

- `refresh`：「加载成功」「不是仓库也是一种正常状态而不是失败」
  「上一次没跑完时重复 `refresh` 会被忽略（`busy` 标志）」；
- `run`：`Loading → Notice → Loaded` 的**顺序**（先报结果、再刷新）、暂存后
  重新扫描拿到的是**新状态**（文件从「未跟踪」挪到「已暂存」）、
  失败时提示里带得上 git 给的原因**且仍然刷新**、`discard` 之后
  **磁盘上的内容真的回到了 HEAD 版本**；
- `load_diff`：**只回一条消息**（读 diff 不刷新工作区，所以不发
  `Loading` / `Loaded`）、并发两次请求拿到的 `id` 互不相同、
  命令失败时**把原因带回来**而不是静默 —— 这是「不共用 `busy` 锁」
  这个决定必须配上的代价。

**开窗口**这一层没法在无头环境自动验证，只能上外部探针。下面这张表记的是一次
**踩坑**：起初只核对前四项就下了「跑通了」的结论，其实**窗口是空白的**。

| 观测项 | 期望 | 实测 |
| --- | --- | --- |
| 进程存活 | 事件循环不退出 | 存活（时长随机，20 s 到数小时） |
| `MainWindowHandle` | 非 0（真的有窗口） | 非 0 |
| `MainWindowTitle` | 与 `WindowBuilder` 一致 | `ZCode — Rust + Dioxus 移植` |
| `Responding` | `True`（消息泵在跑） | `True`，工作集 36 MB |
| **`#main` 子节点数** | **非 0（UI 真的渲染出来了）** | **0 —— 空白** |
| **WebView2 崩溃转储** | **无新增** | **自行新增 `msedge.dll` 崩溃转储** |

> ⚠️ **前四项全部满足，也不代表能正常运行。** 渲染进程崩掉后，宿主进程照样存活、
> 窗口句柄照样非零、消息泵照样响应。要判定「真的渲染了」，必须去看 DOM
> （或直接看窗口截图）。
>
> 实测结论：dioxus-desktop 0.6.3 在 WebView2 `154.0.4258.62` 上**渲染不出内容**，
> 成因与完整证据链见 [README.md](./README.md) 的「已知问题」。那一节还记录了
> 排查中被证伪的三个假设，以及本机为什么修不了。
>
> 想做同类排查，可复用的两件工具：
> - `Config::with_custom_event_handler` —— 把事件循环收到的每个 tao 事件落盘，
>   用来判断进程是「被事件驱动退出」还是「别的原因」。
> - `Config::with_custom_head` / `with_custom_index` + `window.ipc.postMessage` ——
>   往页面里注入自检脚本，把 `#main` 子节点数、`window.interpreter` 是否存在、
>   JS 报错等状态**回读**到 Rust 侧（消息会以
>   `UserEvent(Ipc { msg: IpcMessage { method: "…" } })` 出现在事件流里）。
>   注意回读别用 `setTimeout` —— 页面一旦停摆定时器就不再触发，
>   会让人误判成「通道不通」。

## 6. 曾经的未移植清单与最终归宿

**结论：`packages/ui/src/components/ui` 下 47 / 47 个 TSX 全部已移植，未移植 0 个。**

下面这张表保留历史记录 —— 这些组件一度被判定为「不可能移植」，后来各自用
不同的方式落地了。保留它的价值在于：如果你要移植 `packages/ui` 之外的业务
组件，会再次遇到同样的三类问题，这里的解法可以直接复用。

| 曾经的分类 | 当时的判断 | 最终解法 | 对应章节 |
| --- | --- | --- | --- |
| **(a) 浮层类** 4 个：`context-menu` / `select` / `command` / `toast` | Radix 的 roving focus、Portal、全局队列都没有 Rust 等价物 | 拆成「纯状态机 + 宿主渲染」：roving focus 变成 `resolve_*_key_action` 纯函数，Portal 变成「锚点包裹层 + `absolute`」，`toast` 队列变成 `Vec<ToastItem>` + `futures-timer::Delay` | §4.2 / §4.3 |
| **(b) 需要浏览器运行时** 10 个：`carousel` / `resizable` / `chart` / `code-viewer` / `diff-viewer` / `pdf-viewer` / `pptx-preview-viewer` / `floating-particles` / `smooth-metric-value` / `flip-metric-value` | 「换一个 Rust 库」而不是「移植 TSX」，宿主无 wasm / 无浏览器时无法验证 | **不 vendor 引擎，而是把引擎注入**（§3.8）：抽出全部纯逻辑并加单测，引擎经 `children` / trait / 回调注入 | §3.8 / §4.4 |
| **(c) 组合型 / 装饰型** 6 个：`scroll-fade-viewport` / `lightweight-diff-preview` / `highlighted-lightweight-diff-preview` / `pptx-selection-action-bar` / `ZCodeAboutLogo` / `GlmMonochromeIcon` | 依赖 (a) 或 (b) 的产出，属于收尾 | 先做 (a)(b) 的公共件，再拼装 | §4.3 / §4.4 |

> `scroll-area` 也曾在 (b) 类（Radix 用 `ResizeObserver` 自绘滚动条），
> 第二批改用原生滚动绕开了（见 §3.7），所以提前从清单里划掉。
> `button-group` / `input-group` / `radio-group` / `collapsible` / `accordion`
> 曾在 (c) 类，第二批已完成。

**这套解法的可复用结论**：判断一个组件「能不能移植」，问的不是「它用了什么
浏览器 API」，而是「**它的纯逻辑边界在哪里**」。把 `Math` / 坐标 / 键盘 /
格式化规则抽出来之后，剩下真正必须留在浏览器里的部分，往往只有一个
`<canvas>` 或一个第三方库的实例 —— 而那部分本来也不该由 UI 库负责。

## 7. 剩余工作

47 个 `ui/` 组件的移植已经收敛，下面是**尚未完成、但已明确**的事项，
按「是否阻塞使用」排序：

**A. 影响预览页观感（不阻塞编译 / 测试）**

1. **补上 `--color-muted` / `--color-ring` / `--color-secondary-foreground`
   的权威取值** —— 它们由 `shadcn/tailwind.css` 提供，而该文件不在本仓库中。
   预览页目前用的是等价回退值（见 §4.8），视觉上与上游可能有细微色差。
   拿到该文件后重新生成 `app.css` 即可。
2. **把 `packages/ui/src/styles.css` 的 `@theme` 令牌导出成 Rust 常量** ——
   当前 CSS 仍是颜色 / 间距的唯一真源，组件只负责拼类名。如果将来需要
   在 Rust 侧读色值（例如给 `chart` 传主题色），需要一层导出。

**B. 宿主接线（组件已就绪，等真实宿主）**

3. **`overlay.rs` 的碰撞检测**：当前浮层定位是「相对锚点 + `absolute`」，
   视口边缘不会自动翻转。`context-menu` 的 `position: fixed` + 光标坐标
   路径已在 §4.2 落地，但边缘翻转还没做。
4. **重型预览的引擎注入**：`code-viewer` / `diff-viewer` / `pdf-viewer` /
   `pptx-preview-viewer` / `chart` / `carousel` 的 Rust 壳已全部完成且带单测，
   但宿主需要按 §4.4 的「宿主职责」列自行接上 `@pierre/diffs` / `pdf.js` /
   `pptx-renderer` / `recharts` / shiki 的等价物。这一步是**宿主应用**的工作，
   不是 UI 库的。

**C. 范围之外**

5. **`packages/ui` 中 `components/` 下非 `ui/` 的组合型业务组件**
   （工作流时间线、文件树、diff 面板等）还没建立移植清单 ——
   它们不属于本次「UI primitives 移植」的范围，需要单独盘点。

**D. 应用层（本轮新增）**

6. **桌面窗口仍然渲染不出内容** —— dioxus-desktop 0.6.3 与 WebView2
   `154.0.4258.62` 的兼容性问题，成因与出路见 [README.md](./README.md)
   「已知问题」。这是目前**唯一**挡住「真的在窗口里看到界面」的东西；
   组件树本身已被 SSR 快照与 36 条冒烟断言证明是对的。
7. **「变更」页的读写都已接上，但还缺几块** —— `stage` / `unstage` / `discard` /
   `commit` / `push` 五个动作已可用（见 [PORTING.md](./PORTING.md) §2.5），
   点开任意一行也能看到真实补丁（§2.6）；
   还没有的是：**右键菜单 / 勾选框式多选**（现在只能逐行或整组操作）、
   **提交信息生成**（`gitCommitMessageGenerator` 依赖 LLM，属未移植子系统）、
   **部分提交**（上游的 `stagedOnly` 变体靠临时 `GIT_INDEX_FILE`，本移植有意未做）、
   **提交图 / 分支比较视图**（`getCommitGraph` 与 `getDiff` 的服务层已经就位，
   界面上还没有消费它们的页面 —— 上游那部分叫 `getBranchComparison`，未移植）、
   以及**冲突解决入口**（冲突现在只在「冲突」分组里列出来，点不了）。
   另外补丁现在**只读**：上游能在 diff 上留评论 / 选中行区间，
   `code_viewer` 的注释原语已移植好，但「变更」页还没接上。
8. **其余页面仍是演示数据** —— 会话流 / 输入区 / 设置里的内容还是 `demo_state()`
   的硬编码值。它们要接的是会话与消息存储，那是 `packages/services` 里
   体量大得多的另一块。
