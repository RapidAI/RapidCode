# UI 层移植说明（PORTING-UI.md）

> 配套阅读：[PORTING.md](./PORTING.md)（核心逻辑层移植）、[README.md](./README.md)（快速开始）。

## 1. 结论先行

`packages/ui` 的 shadcn 风格原语组件（React + Radix + Tailwind）已经用
**Rust + [Dioxus](https://dioxuslabs.com/) 0.6** 重写，落在 `rust/crates/zcode-ui`；
在此之上又用移植出来的原语搭了一个**真能开窗口跑**的桌面应用 `rust/crates/zcode-app`。

- **27 / 47** 个原语完成移植（`6,569` 行 Rust，含 147 个内联单测）；
- **应用外壳**：侧栏 / 会话流 / 输入区 / 详情面板 / 设置对话框 / 组件画廊，
  `2,719` 行 + `645` 行冒烟测试；
- `cargo build` / `cargo test` **全绿**（工作区 **242** 个测试），
  且测试层**不需要 wasm、不需要窗口、不需要浏览器**；
- 类名（`class`）输出与 TS 版本**逐字符对齐** —— 移植后仍复用 `packages/ui` 的
  同一套 Tailwind 样式与 `@custom-variant` 语义，不需要改一行 CSS。

```
cargo run   -p zcode-app                  # 开一个 1360×860 的桌面窗口（默认 feature = desktop）
cargo build -p zcode-ui                   # 纯组件模型，任意环境可编译
cargo test  --workspace                   # 242 个测试，全部在 host 上跑
cargo run   -p zcode-ui --example preview # 生成可直接打开的 preview.html
```

> 「能正常运行」的判定方式见 §5.5：除编译与测试外，还核对了进程存活、
> 窗口句柄非零、`MainWindowTitle` 与 `WindowBuilder` 一致、`Responding=True`。

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
| 浮层定位 | Radix `Portal` + `@floating-ui`（碰撞检测 / 自动翻转） | 锚点包裹层 + `absolute` 类名 | 见 §4.3；不做碰撞检测，位置由调用方给的 `side` / `align` / `side_offset` 决定 |
| `DropdownMenu` 高亮态 | Radix 内部 roving focus | 每项一个 `Signal<bool>` 写 `data-highlighted` | 只做 hover 高亮，不做键盘漫游（见 `dropdown_menu.rs` 模块文档） |
| `Collapsible` / `Accordion` 的 `id` | `useId()` 自动生成，并自动接 `aria-controls` | 显式 prop，**不传则不发该属性** | Dioxus 0.6 没有「每实例稳定 id」原语；Trigger 与 Content 是两个组件，拿不到同一个 id。发 `id=""` 是无效 HTML，所以宁可整个不发 |
| `InputGroupButton` | 委托给 `Button` | 渲染裸 `button` + `button_classes()` | 与 TS 一致（TS 里它就是 `<button className={button({...})}>`，不是 `<Button>`） |

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

### 4.3 支撑模块

| Rust 模块 | 作用 |
| --- | --- |
| `icons.rs` | **内联 lucide 图标**，零图标库依赖。`lucide!` 宏把 `<svg>` 的公共属性（`viewBox` / `stroke` / `stroke-width` / `stroke-linecap` / `stroke-linejoin`）固化，手写几何；`data-slot` 默认取图标名（`plus` / `check` / `chevron-down`…），这样 TS 里 `**:data-[slot=accordion-trigger-icon]:size-4` 这类选择器能原样命中 |
| `components/overlay.rs` | **浮层定位约定**。Radix 的 `Portal` + `@floating-ui` 在这里被替换成「锚点包裹层 + `absolute` 类名」：`Side`（Top/Right/Bottom/Left）、`Align`（Start/Center/End）、`side_align_classes(side, align, side_offset)` 生成形如 `absolute bottom-full left-1/2 -translate-x-1/2 mb-[4px]` 的类名，`OVERLAY_ANCHOR_BASE = "relative inline-flex"` 是锚点基准 |

> **为什么 Dialog 不需要这套约定**：Dialog / AlertDialog 的内容用 `position: fixed`，
> containing block 是**视口**而不是最近的定位祖先，所以没有 Portal 也能居中盖满整屏。
> 只有相对锚点定位的浮层（Tooltip / HoverCard / Popover / DropdownMenu）才需要 §4.3 的约定。

### 4.4 可运行的应用：`crates/zcode-app`
把移植出的原语拼成一个 ZCode 风格的外壳，用来证明「组件库真的能用」。

| 文件 | 内容 |
| --- | --- |
| `src/lib.rs` | `App` / `AppShell`；注入 `document::Title` + `document::Style`；`APP_CSS` 用 `include_str!` 内嵌 `assets/app.css` |
| `src/main.rs` | 桌面入口：`LaunchBuilder::desktop().with_cfg(WindowBuilder::new().with_title(..).with_inner_size(1360×860))` |
| `src/state.rs` | `AppState` / `AppCtx`（全部 `Signal`）/ `use_app()` / `demo_state()` |
| `src/views/sidebar.rs` | 品牌行 + 搜索（`InputGroup`）+ 按「今天/昨天/更早」分组的会话列表 + 右键菜单（`DropdownMenu`，destructive 删除）+ 上下文用量（`Progress`）+ 头像 |
| `src/views/topbar.rs` | 会话标题 + 分支 `Badge` + 视图 `Tabs` + 模型选择器（`DropdownMenuRadioItem`）+ 带 `Tooltip` 的图标按钮 |
| `src/views/conversation.rs` | 消息流；助手头像挂 `HoverCard`；工具调用卡片（分段 `Progress`）；流式提示用受控 `Collapsible` + `Alert` |
| `src/views/composer.rs` | `InputGroup`（`InlineStart` 回形针 / `Textarea` / `InlineEnd` 发送或停止）+ 审批模式选择器 |
| `src/views/detail.rs` | 执行计划（受控 `Collapsible`）+ 变更文件 / 工具调用 / 上下文来源（`Accordion`）+ 权限 `RadioGroup` + 分段 `Progress` + 图例 + `AvatarGroup` + `ButtonGroup` |
| `src/views/settings.rs` | 设置 `Dialog`（常规/外观/关于 三个 `Tabs`）+ 删除确认 `AlertDialog` |
| `src/views/gallery.rs` | 组件画廊 `Dialog`，六个分页把所有原语和变体都渲染一遍 |

### 4.5 样式表怎么来的

仓库里**没有** Tailwind 安装，`packages/ui` 的样式来自 `shadcn/tailwind.css`
（一个不在本仓库的依赖）。所以 `rust/.tailwind-build/build.sh` 手工拼了一份入口 CSS：

1. `@import "tailwindcss" source(none)` —— 关掉自动扫描，只扫我们指定的路径；
2. `@import "tw-animate-css"` —— 提供 `animate-in` / `fade-in-0` / `animate-collapsible-down` 等；
3. `@source "../crates/**/*.rs"` —— 让 Tailwind 从 **Rust 源码**里提取类名
   （注意路径相对 `.tailwind-build/`，写成 `../../crates` 会扫到空目录，产出的 CSS 会静默变空）；
4. 手写补上 `shadcn/tailwind.css` 里那组 `@custom-variant`
   （`data-open` / `data-closed` / `data-active` / `data-checked` / `data-horizontal` /
   `data-vertical` / `data-disabled` / `data-highlighted` / `data-inset`）；
5. 用 `sed` 从 `packages/ui/src/styles.css` 里抠出 `@theme { … }` 与 `.dark { … }` 两块原样粘贴。

```bash
cd rust/.tailwind-build && ./build.sh   # → rust/crates/zcode-app/assets/app.css（93,588 字节）
```

> ⚠️ Tailwind v4 会**按需裁剪** `@theme`：只有被实际用到的令牌才会作为 CSS 变量输出。
> 所以「样式表里没有 `--text-ui-2xs`」不代表 `@theme` 没被导入 —— 只是当前代码没用它。
> 冒烟测试因此既查具体令牌，也查 `--color-*` 的总量（>200）来确认整块 `@theme` 确实进来了。

## 5. 测试策略

四层，**全部在 host 上跑**（无 wasm / 无窗口 / 无浏览器）：

| 层 | 文件 | 数量 | 断言粒度 |
| --- | --- | --- | --- |
| 类名纯函数单测 | `zcode-ui/src/**` 内联 `#[cfg(test)]` | 147 | 字符串包含 / 相等 |
| mutation 记录式渲染 | `zcode-ui/tests/render.rs` | 11 | 区分静态属性 / 动态属性 / 文本节点 |
| HTML 快照 | `zcode-ui/tests/html_snapshot.rs` | 41 | 序列化后的 HTML 字符串 |
| 整树冒烟 | `zcode-app/tests/smoke.rs` | 17 | 把整个应用（含浮层）真渲染一遍 |

各 crate 明细：

| crate | 单元 | 集成 | 小计 |
| --- | --- | --- | --- |
| `zcode-rpc` | 10 | — | 10 |
| `zcode-shared` | 14 | — | 14 |
| `zcode-ui` | 147 | 41 + 11 | 199 |
| `zcode-app` | 2 | 17 | 19 |
| **合计** | **173** | **69** | **242** |

### 5.1 类名纯函数单测（147 个）

`*_classes()` 与 `cn()` 的直接断言。覆盖 variant/size 映射、用户 class 覆盖
组件默认、`text-ui-*` 与文字颜色不互相覆盖、方括号修饰符切分、
`ring` 宽度 vs 颜色、`shadow` 尺寸 vs 颜色、负向定位前缀
（`-translate-x-1/2` / `-inset-x-1` 的 tailwind-merge 分组）等。

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

### 5.3 HTML 快照（41 个）

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

### 5.5 整树冒烟 + 「能正常运行」的判定（17 个）

`zcode-app/tests/smoke.rs` 把 [`AppShell`] 整棵树、以及三个浮层（画廊 / 设置 /
删除确认）**真渲染一遍**，断言：

- 整棵树能挂载（任何 `use_context` 缺失或 hook 顺序错误都会在这里 panic）；
- 各区域文案真的出现（说明分支没走空）；
- 关键原语的 `data-slot` 锚点与 `data-*` 语义属性真的在 DOM 里 ——
  这是 shadcn `@custom-variant` 生效的前提，也是移植中最容易漏的东西；
- 浮层**默认关闭**时内容整体不渲染（`dialog-content` / `alert-dialog-content` 都不在）；
- 需要展开才有 DOM 的浮层另建 `OverlaysOpen` 夹具，用受控
  `open: Some(signal(true))` 强制打开后再断言内容标记。

其中一条专门盯着「类名在 DOM 上、但样式表里没编译出规则」这个坑：
断言 `[scrollbar-width:none]` 既出现在 DOM 里，也出现在内嵌 CSS 里。

**开窗口**这一层没法在无头环境自动验证，所以用外部探针核对了四件事：

```bash
cargo run -p zcode-app            # 保持运行
# 另一个终端：
#   Get-Process zcode-app | Select Id, MainWindowHandle, MainWindowTitle, Responding
```

| 观测项 | 期望 | 实测 |
| --- | --- | --- |
| 进程存活 | 事件循环不退出 | 存活 12s 无输出 |
| `MainWindowHandle` | 非 0（真的有窗口） | `44435456` |
| `MainWindowTitle` | 与 `WindowBuilder` 一致 | `ZCode — Rust + Dioxus 移植` |
| `Responding` | `True`（消息泵在跑） | `True`，工作集 36 MB |

> ⚠️ `MainWindowTitle` 首次读可能是空串（.NET 侧缓存），`$p.Refresh()` 之后才对。

## 6. 未移植组件与原因

`packages/ui/src/components/ui` 下还有 **20 / 47** 个 TSX 未移植，分三类：

**(a) 浮层类（需要焦点陷阱 / 键盘导航）** —— 4 个
`context-menu` / `select` / `command` / `toast`。

`dialog` / `alert-dialog` / `popover` / `hover-card` / `tooltip` / `dropdown-menu`
已经在第二批里用 §4.3 的「锚点包裹层 + `absolute`」约定做掉了，证明这层不需要
Portal 也能落地。剩下的 4 个卡在别处：

- `select` / `command` 需要**键盘漫游 + 列表虚拟化 + 输入过滤**，
  Radix 的 roving focus 没有 Rust 等价物；
- `context-menu` 需要**光标坐标**定位（我们的约定是「相对锚点」，而右键菜单
  的锚点就是鼠标位置），要额外一套 `position: fixed` + clientX/clientY 的路径；
- `toast` 需要**全局队列 + 自动消失计时器**，属于运行时服务而不是纯组件。

**(b) 需要浏览器运行时能力** —— 10 个
`carousel`（滚动吸附 + 手势）、`resizable`（Pointer Events）、
`chart`（canvas/SVG 绘制）、`code-viewer` / `diff-viewer` / `pdf-viewer` /
`pptx-preview-viewer`（重型渲染器）、
`floating-particles` / `smooth-metric-value` / `flip-metric-value`
（`requestAnimationFrame` 动画）。

> `scroll-area` 本来也在这类（Radix 用 `ResizeObserver` 自绘滚动条），
> 第二批改用原生滚动绕开了（见 §3.7），所以从清单里划掉。

**(c) 组合型 / 装饰型** —— 6 个
`scroll-fade-viewport` / `lightweight-diff-preview` /
`highlighted-lightweight-diff-preview` / `pptx-selection-action-bar` /
`ZCodeAboutLogo` / `GlmMonochromeIcon`。

`button-group` / `input-group` / `radio-group` / `collapsible` / `accordion`
原先也在这类，第二批已完成。

## 7. 剩余工作

1. **浮层定位的第二步**：给 `overlay.rs` 补 `position: fixed` + 光标坐标的路径，
   用来接 `context-menu`；再补碰撞检测（视口边缘自动翻转），
   这一步做完 `select` 之外的浮层就齐了；
2. **键盘导航**：给 `DropdownMenu` 补 roving focus（当前只有 hover 高亮），
   这是 `select` / `command` 的前置；
3. **`toast` 的运行时服务**：需要一个挂在 `AppShell` 顶层的队列 + 定时器；
4. **重型渲染器**（`code-viewer` / `diff-viewer` / `pdf-viewer`）建议独立评估，
   它们是「换一个 Rust 库」而不是「移植 TSX」；
5. 把 `packages/ui/src/styles.css` 的 `@theme` 令牌导出成 Rust 常量，
   让颜色/间距不再只存在于 CSS 里（当前 CSS 仍是唯一真源，组件只负责类名）；
6. 补上 `--color-muted` / `--color-ring` / `--color-secondary-foreground`
   的权威取值 —— 它们由 `shadcn/tailwind.css` 提供，而该文件不在本仓库中，
   预览页目前用的是等价回退值；
7. `packages/ui` 里 **`components/` 下非 `ui/` 的组合型业务组件**
   （工作流时间线、文件树、diff 面板等）还没建立移植清单。
