# UI 层移植说明（PORTING-UI.md）

> 配套阅读：[PORTING.md](./PORTING.md)（核心逻辑层移植）、[README.md](./README.md)（快速开始）。

## 1. 结论先行

`packages/ui` 的 shadcn 风格原语组件（React + Radix + Tailwind）已经用
**Rust + [Dioxus](https://dioxuslabs.com/) 0.6** 重写，落在 `rust/crates/zcode-ui`。

- **15 / 47** 个原语完成移植：`748` 行 TSX → `2,459` 行 Rust（含 65 个内联单测）
  + `984` 行集成测试 + `1,007` 行预览生成器；
- `cargo build` / `cargo test` **全绿**（工作区 123 个测试），且**不需要 wasm、不需要窗口、不需要浏览器**；
- 类名（`class`）输出与 TS 版本**逐字符对齐** —— 移植后仍复用 `packages/ui` 的
  同一套 Tailwind 样式与 `@custom-variant` 语义，不需要改一行 CSS。

```
cargo build -p zcode-ui                  # 纯组件模型，任意环境可编译
cargo test  -p zcode-ui                  # 65 单元 + 23 HTML 快照 + 11 mutation 记录 = 99 个测试
cargo run   -p zcode-ui --example preview # 生成可直接打开的 preview.html
cargo build -p zcode-ui --features web    # 额外挂上 dioxus-web 渲染器（真机用）
```

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

## 5. 测试策略

三层，**全部在 host 上跑**（无 wasm / 无窗口 / 无浏览器）：

| 层 | 文件 | 数量 | 断言粒度 |
| --- | --- | --- | --- |
| 类名纯函数单测 | `src/**` 内联 `#[cfg(test)]` | 65 | 字符串包含 / 相等 |
| mutation 记录式渲染 | `tests/render.rs` | 11 | 区分静态属性 / 动态属性 / 文本节点 |
| HTML 快照 | `tests/html_snapshot.rs` | 23 | 序列化后的 HTML 字符串 |

合计 **99** 个；加上 `zcode-rpc`（10）与 `zcode-shared`（14），工作区共 **123** 个测试。

### 5.1 类名纯函数单测（65 个）

`*_classes()` 与 `cn()` 的直接断言。覆盖 variant/size 映射、用户 class 覆盖
组件默认、`text-ui-*` 与文字颜色不互相覆盖、方括号修饰符切分、
`ring` 宽度 vs 颜色、`shadow` 尺寸 vs 颜色等。

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

### 5.3 HTML 快照（23 个）

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
- 布尔属性渲染成 `disabled=true`，为 `false` 时**整个属性省略**；
- `BUTTON_BASE` 的 `text-ui-base/relaxed` 会被 size 的 `text-ui-base` 覆盖
  （同属 `font-size` 组，tailwind-merge 语义下后者胜）—— 与 TS 版一致。

### 5.4 预览页生成器（`examples/preview.rs`）

```bash
cargo run -p zcode-ui --example preview            # → ./preview.html
```

生成一份可直接在浏览器打开的页面。页面里每个 `class` 都来自移植后组件的
`*_classes()` 纯函数（没有第二份「手抄类名」可以掩盖拼写错误），顶部那组控件
更是走 `dioxus_ssr::render_element` 的**真渲染**结果。样式由 Tailwind v4
浏览器版 + 从 `packages/ui/src/styles.css` 复刻的 `@theme` / `@custom-variant`
令牌驱动（需联网加载 CDN；离线打开会退化为无样式但结构正确的 HTML）。

## 6. 未移植组件与原因

`packages/ui/src/components/ui` 下还有 32 个 TSX 未移植，分三类：

**(a) 浮层类（需要 Portal + 定位计算）**
`dialog` / `alert-dialog` / `popover` / `hover-card` / `tooltip` /
`dropdown-menu` / `context-menu` / `select` / `command` / `toast` / `menubar`。
它们的「类名表 + 状态机」是可移植的，但 Radix 的核心价值在于**焦点陷阱、
层级管理、碰撞检测、键盘导航**，这些要配合 Dioxus 的 Portal 与定位逻辑
单独设计，不能机械翻译。

**(b) 需要浏览器运行时能力**
`carousel`（滚动吸附 + 手势）、`resizable`（Pointer Events）、
`scroll-area`（ResizeObserver）、`chart`（canvas/SVG 绘制）、
`code-viewer` / `diff-viewer` / `pdf-viewer` / `pptx-preview-viewer`（重型渲染器）、
`floating-particles` / `smooth-metric-value` / `flip-metric-value`（requestAnimationFrame 动画）。

**(c) 组合型（由 (a) 派生）**
`button-group` / `input-group` / `radio-group` / `collapsible` / `accordion` /
`scroll-fade-viewport` / `highlighted-lightweight-diff-preview` /
`pptx-selection-action-bar` / `ZCodeAboutLogo` / `GlmMonochromeIcon`。

## 7. 剩余工作

1. 按 (a) → (c) 顺序推进，先做 **Portal + 浮层定位** 这层基础设施，
   再批量移植浮层组件；
2. 为 `packages/ui` 的 **组合型业务组件**（非 `components/ui` 下的原语）
   建立第二批移植清单；
3. 把 `packages/ui/src/styles.css` 的 `@theme` 令牌导出成 Rust 常量，
   让颜色/间距不再只存在于 CSS 里（当前 CSS 仍是唯一真源，组件只负责类名）；
4. 补上 `--color-muted` / `--color-ring` / `--color-secondary-foreground`
   的权威取值 —— 它们由 `shadcn/tailwind.css` 提供，而该文件不在本仓库中，
   预览页目前用的是等价回退值。
