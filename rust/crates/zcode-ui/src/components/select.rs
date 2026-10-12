//! `Select` 家族 —— Rust 移植自 `packages/ui/src/components/ui/select.tsx`
//!
//! TS 用 Radix `Select.*`（Portal + floating-ui + roving tabindex + 类型搜索）。
//! Rust 侧沿用 [`crate::components::overlay`] 的纯 CSS 自管理定位，
//! 并把「当前选中值」与「当前高亮项」都放进 context，让选项自己读。
//!
//! ## 关键移植点 1：`cva` 的两条轴
//!
//! `selectTriggerVariants` 是**两个** variant 轴（`variant` × `size`），
//! 因此这里是两个独立枚举（[`SelectTriggerVariant`] / [`SelectTriggerSize`]），
//! 而不是一个。两个枚举各自 `as_str()` 写进 `data-variant` / `data-size`。
//!
//! 注意默认值：`defaultVariants` 是 `{ variant: "input", size: "default" }`，
//! 所以 `#[default]` 落在 `Input` 上，**不是**叫 `Default` 的那个变体。
//!
//! ## 关键移植点 2：`data-highlighted` 与 `data-placeholder`
//!
//! 这两族类名（`data-[highlighted]:bg-menu-hover`、`data-placeholder:text-foreground-subtlest`）
//! 在 TS 里由 Radix 运行时写入。Rust 侧必须自己提供：
//!
//! - `data-highlighted`：鼠标 hover **或** 键盘导航命中该索引时为真。
//! - `data-placeholder`：`SelectTrigger` 在「没有选中值」时挂上。
//!
//! ## 关键移植点 3：`position` 决定定位路径
//!
//! Radix 的 `item-aligned`（默认）会把**选中项**对齐到触发器位置，
//! 需要运行时测量；`popper` 则挂在触发器上下方。Rust 侧：
//!
//! - [`SelectContentPosition::ItemAligned`] → 用 `top-0 left-0 w-full` 近似
//!   「覆盖在触发器上」（无测量，见下方差异说明）。
//! - [`SelectContentPosition::Popper`] → 走 [`side_align_classes`]。
//!
//! 同时原样输出 `data-align-trigger="true|false"`，因为类名里有
//! `data-[align-trigger=true]:animate-none`。
//!
//! ## 与 TS 的差异
//!
//! 1. 没有 Portal：内容用 `absolute` 相对锚点定位，因此**要求祖先链上没有
//!    `overflow: hidden` / `transform`**。
//! 2. `item-aligned` 无测量：用「覆盖触发器」近似，选中项不会精确压在触发器
//!    文字上。选项需要解释后果时请用 `position = Popper`（与 TS 注释同建议）。
//! 3. 没有类型搜索（typeahead）；键盘只支持 ↑ / ↓ / Home / End / Enter / Escape。
//! 4. `--radix-select-content-available-height` 这类 CSS 变量无人写入，
//!    类名保留但高度上限交给调用方的 `max-h-*`。
//! 5. `SelectValue` 用 [`element_is_empty`] 区分「显式覆盖」与「自动回填」，
//!    对应 TS 里 Radix 的自动回填行为。自动回填显示的是**选中值字符串**
//!    （Radix 显示的是选项文字，我们不做 label 注册表）。

use crate::cn::cn;
use crate::components::overlay::{side_align_classes, Align, Side};
use crate::icons::{IconCheck, IconChevronDown, IconChevronUp};
use dioxus::prelude::*;

/// `SelectTrigger` 的 `variant` 轴（TS `selectTriggerVariants.variant`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SelectTriggerVariant {
    #[default]
    Input,
    Default,
    Outline,
    Secondary,
    Ghost,
    Destructive,
}

impl SelectTriggerVariant {
    pub fn as_str(&self) -> &'static str {
        match self {
            SelectTriggerVariant::Input => "input",
            SelectTriggerVariant::Default => "default",
            SelectTriggerVariant::Outline => "outline",
            SelectTriggerVariant::Secondary => "secondary",
            SelectTriggerVariant::Ghost => "ghost",
            SelectTriggerVariant::Destructive => "destructive",
        }
    }

    /// 变体附加类名（TS 原文，逐字保留）。
    pub fn classes(&self) -> &'static str {
        match self {
            SelectTriggerVariant::Input => "border-input-border bg-input text-foreground hover:border-input-border-hover focus-visible:border-input-border-focused focus-visible:bg-input-focused focus-visible:ring-0",
            SelectTriggerVariant::Default => {
                "border-transparent bg-primary text-primary-foreground hover:bg-primary/80"
            }
            SelectTriggerVariant::Outline => "border-border bg-transparent text-foreground hover:bg-input/50 hover:text-foreground aria-expanded:bg-selected aria-expanded:text-foreground",
            SelectTriggerVariant::Secondary => "border-transparent bg-secondary text-foreground hover:bg-secondary/80 aria-expanded:bg-secondary aria-expanded:text-foreground",
            SelectTriggerVariant::Ghost => "border-transparent bg-transparent text-foreground hover:bg-hover hover:text-foreground aria-expanded:bg-hover aria-expanded:text-foreground",
            SelectTriggerVariant::Destructive => "border-transparent bg-destructive text-destructive-foreground hover:bg-destructive/90 focus-visible:border-destructive/40 focus-visible:ring-destructive/20 aria-expanded:bg-destructive aria-expanded:text-destructive-foreground",
        }
    }
}

/// `SelectTrigger` 的 `size` 轴（TS `selectTriggerVariants.size`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SelectTriggerSize {
    Xs,
    Sm,
    #[default]
    Default,
    Lg,
}

impl SelectTriggerSize {
    pub fn as_str(&self) -> &'static str {
        match self {
            SelectTriggerSize::Xs => "xs",
            SelectTriggerSize::Sm => "sm",
            SelectTriggerSize::Default => "default",
            SelectTriggerSize::Lg => "lg",
        }
    }

    /// 尺寸附加类名（TS 原文，逐字保留）。
    pub fn classes(&self) -> &'static str {
        match self {
            SelectTriggerSize::Xs => {
                "h-5 rounded-sm pl-2 pr-1 text-ui-base [&_svg:not([class*='size-'])]:size-2.5"
            }
            SelectTriggerSize::Sm => {
                "h-6 rounded-md pl-2 pr-1 text-ui-base/relaxed [&_svg:not([class*='size-'])]:size-3"
            }
            SelectTriggerSize::Default => {
                "h-7 rounded-md pl-2 pr-1 text-ui-base/relaxed [&_svg:not([class*='size-'])]:size-3.5"
            }
            SelectTriggerSize::Lg => {
                "h-8 rounded-lg pl-3 pr-2 text-ui-base [&_svg:not([class*='size-'])]:size-4"
            }
        }
    }
}

/// `SelectContent` 的 `position`（Radix `item-aligned` | `popper`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SelectContentPosition {
    #[default]
    ItemAligned,
    Popper,
}

impl SelectContentPosition {
    pub fn as_str(&self) -> &'static str {
        match self {
            SelectContentPosition::ItemAligned => "item-aligned",
            SelectContentPosition::Popper => "popper",
        }
    }

    /// 是否是 `item-aligned`（写进 `data-align-trigger`）。
    pub fn is_item_aligned(&self) -> bool {
        matches!(self, SelectContentPosition::ItemAligned)
    }
}

/// 键盘导航方向（纯数据，便于穷举测试）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectNav {
    Next,
    Prev,
    First,
    Last,
}

/// `SelectTrigger` 基础类名（TS 原文，逐字保留）。
pub const SELECT_TRIGGER_BASE: &str = "flex w-fit items-center justify-between gap-1.5 border whitespace-nowrap transition-colors outline-none disabled:cursor-not-allowed disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-2 aria-invalid:ring-destructive/20 data-placeholder:text-foreground-subtlest *:data-[slot=select-value]:line-clamp-1 *:data-[slot=select-value]:flex *:data-[slot=select-value]:items-center *:data-[slot=select-value]:gap-1.5 dark:aria-invalid:border-destructive/50 dark:aria-invalid:ring-destructive/40 [&_svg]:pointer-events-none [&_svg]:shrink-0";

/// `SelectTrigger` 里指示图标的类名（TS 原文，逐字保留）。
pub const SELECT_TRIGGER_ICON: &str = "pointer-events-none size-3.5 text-foreground-subtle";

/// `SelectContent` 基础类名（TS 原文，逐字保留）。
///
/// 注意 `z-[60]`：可操作的 Select 必须高于 `z-50` 的 tooltip，避免提示遮住选项。
pub const SELECT_CONTENT_BASE: &str = "relative z-[60] max-h-(--radix-select-content-available-height) min-w-32 origin-(--radix-select-content-transform-origin) overflow-x-hidden overflow-y-auto rounded-lg border border-popover-border bg-menu p-1 text-foreground shadow-md duration-100 data-[align-trigger=true]:animate-none data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95 [app-region:no-drag]";

/// `SelectPrimitive.Viewport` 基础类名（TS 原文，逐字保留）。
///
/// 注意 `data-[position=popper]:` 前缀的类名是**无条件输出**的：它们是否生效
/// 由元素上的 `data-position` 属性决定（TS 里 `position === "popper" && ""`
/// 那一段是空操作）。因此本函数不需要 `position` 参数。
pub const SELECT_VIEWPORT_BASE: &str = "flex flex-col gap-0.5 data-[position=popper]:w-full data-[position=popper]:min-w-(--radix-select-trigger-width)";

/// `SelectGroup` 基础类名（TS 原文，逐字保留）。
pub const SELECT_GROUP_BASE: &str = "flex flex-col gap-0.5 scroll-my-1 p-1";

/// `SelectLabel` 基础类名（TS 原文，逐字保留）。
pub const SELECT_LABEL_BASE: &str = "px-2 py-1.5 text-ui-base text-foreground-subtlest";

/// `SelectItem` 基础类名（TS 原文，逐字保留）。
pub const SELECT_ITEM_BASE: &str = "relative flex min-h-7 w-full cursor-default items-center gap-2 rounded-md px-2 py-1 text-ui-base/relaxed text-foreground outline-hidden select-none data-[highlighted]:bg-menu-hover data-[highlighted]:text-foreground data-disabled:pointer-events-none data-disabled:text-foreground-subtlest data-disabled:opacity-100 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4 *:[span]:last:flex *:[span]:last:items-center *:[span]:last:gap-2";

/// `SelectRichItem` 基础类名（TS 原文，逐字保留）。
///
/// 与 [`SELECT_ITEM_BASE`] 的三处差别：`min-h-13`（两行）、`items-start`、
/// `pr-8 pl-2`；并且**没有** `data-disabled:opacity-100`。
pub const SELECT_RICH_ITEM_BASE: &str = "relative flex min-h-13 w-full cursor-default items-start gap-3 rounded-md py-2 pr-8 pl-2 text-ui-base text-foreground outline-hidden select-none data-[highlighted]:bg-menu-hover data-[highlighted]:text-foreground data-disabled:pointer-events-none data-disabled:text-foreground-subtlest [&_svg]:pointer-events-none [&_svg]:shrink-0";

/// 普通项 / 富项共用的「尾部插槽」包裹层类名（`trailing` 与勾号都用它）。
pub const SELECT_ITEM_TRAILING_BASE: &str =
    "pointer-events-none absolute right-2 flex items-center justify-center";

/// 普通项的 `trailing` 插槽：可点击，因此**不带** `pointer-events-none`。
pub const SELECT_ITEM_TRAILING_INTERACTIVE: &str =
    "pointer-events-auto absolute right-2 flex items-center justify-center";

/// 富项勾号的包裹层（顶部对齐，`top-2.5`）。
pub const SELECT_RICH_ITEM_INDICATOR_BASE: &str =
    "pointer-events-none absolute top-2.5 right-2 flex items-center justify-center";

/// 勾号图标类名（普通项 / 富项相同）。
pub const SELECT_ITEM_INDICATOR_ICON: &str = "pointer-events-none size-4 text-foreground-subtle";

/// `SelectSeparator` 基础类名（TS 原文，逐字保留）。
pub const SELECT_SEPARATOR_BASE: &str = "pointer-events-none my-1 h-px bg-border";

/// `SelectScrollUpButton` / `SelectScrollDownButton` 基础类名（TS 里两处相同）。
pub const SELECT_SCROLL_BUTTON_BASE: &str = "z-10 flex cursor-default items-center justify-center bg-menu py-1 text-foreground-subtle [&_svg:not([class*='size-'])]:size-3.5";

/// 触发器类名拼装（`cva` 的等价物）。
pub fn select_trigger_classes(
    variant: SelectTriggerVariant,
    size: SelectTriggerSize,
    class: &str,
) -> String {
    cn(&[SELECT_TRIGGER_BASE, variant.classes(), size.classes(), class])
}

/// 内容面板定位类名。
///
/// - `ItemAligned`：`absolute left-0 top-0 w-full` —— 近似 Radix 的
///   「把选中项压在触发器上」（无测量，见模块文档差异 2）。
/// - `Popper`：走 [`side_align_classes`]，挂 `side_offset` 像素。
pub fn select_content_position_classes(
    position: SelectContentPosition,
    side: Side,
    align: Align,
    side_offset: f64,
) -> String {
    match position {
        SelectContentPosition::ItemAligned => "absolute left-0 top-0 w-full".to_string(),
        SelectContentPosition::Popper => side_align_classes(side, align, side_offset),
    }
}

pub fn select_content_classes(
    position: SelectContentPosition,
    side: Side,
    align: Align,
    side_offset: f64,
    class: &str,
) -> String {
    cn(&[
        SELECT_CONTENT_BASE,
        &select_content_position_classes(position, side, align, side_offset),
        class,
    ])
}

pub fn select_viewport_classes(class: &str) -> String {
    cn(&[SELECT_VIEWPORT_BASE, class])
}

pub fn select_group_classes(class: &str) -> String {
    cn(&[SELECT_GROUP_BASE, class])
}

pub fn select_label_classes(class: &str) -> String {
    cn(&[SELECT_LABEL_BASE, class])
}

pub fn select_item_classes(class: &str) -> String {
    cn(&[SELECT_ITEM_BASE, class])
}

pub fn select_rich_item_classes(class: &str) -> String {
    cn(&[SELECT_RICH_ITEM_BASE, class])
}

pub fn select_separator_classes(class: &str) -> String {
    cn(&[SELECT_SEPARATOR_BASE, class])
}

pub fn select_scroll_button_classes(class: &str) -> String {
    cn(&[SELECT_SCROLL_BUTTON_BASE, class])
}

/// 键盘导航的纯逻辑：算出新的高亮索引。
///
/// Radix 的 `Select` 在列表首尾**环绕**（`↑` 从第一项跳到末项），这里保持一致。
/// `len == 0` 时恒为 `None`。
pub fn select_navigate(current: Option<usize>, len: usize, nav: SelectNav) -> Option<usize> {
    if len == 0 {
        return None;
    }
    match nav {
        SelectNav::First => Some(0),
        SelectNav::Last => Some(len - 1),
        SelectNav::Next => Some(match current {
            Some(i) if i + 1 < len => i + 1,
            Some(_) => 0,
            None => 0,
        }),
        SelectNav::Prev => Some(match current {
            Some(0) | None => len - 1,
            Some(i) => i - 1,
        }),
    }
}

/// `Select` 的共享状态：展开态 + 选中值 + 键盘高亮索引。
///
/// 注意**项计数不在这里** —— 见 [`SelectItemsContext`]。
#[derive(Clone, Copy)]
struct SelectContext {
    open: Signal<bool>,
    /// 选中值；`None` 表示未选中（触发器挂 `data-placeholder`）。
    value: Signal<Option<String>>,
    /// 键盘高亮的项索引（`None` = 无高亮）。
    highlight: Signal<Option<usize>>,
}

/// 项注册表：由 [`SelectContent`] 在**每次展开时新建**，供选项自报索引。
///
/// 为什么必须每次展开新建：内容关闭时各项会被卸载，重新展开会**重新挂载**并
/// 再次执行 `use_hook`。若计数器挂在根上且从不重置，第二次展开时索引会从
/// `n` 开始继续累加，`count` 变成 `2n` —— 键盘环绕要按 2n 次才回到首项。
/// 把计数器放在「只有展开时才挂载」的组件里，生命周期自然对齐。
#[derive(Clone, Copy)]
struct SelectItemsContext {
    /// 下一个待分配的索引。
    next_index: Signal<usize>,
    /// 已注册的项数（= 最后一次分配的索引 + 1）。
    count: Signal<usize>,
}

/// 读取当前 Select 的展开状态。
pub fn use_select_open() -> Signal<bool> {
    use_context::<SelectContext>().open
}

/// 读取当前 Select 的选中值。
pub fn use_select_value() -> Signal<Option<String>> {
    use_context::<SelectContext>().value
}

/// 根容器（状态持有者 + 定位锚点）。
#[component]
pub fn Select(
    /// 受控展开态；不传则由组件自持。
    #[props(default)] open: Option<Signal<bool>>,
    /// 受控选中值；不传则由组件自持。
    #[props(default)] value: Option<Signal<Option<String>>>,
    /// 选中值变化时的回调。
    #[props(default)] on_value_change: EventHandler<String>,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let internal_open = use_signal(|| false);
    let open = open.unwrap_or(internal_open);
    let internal_value = use_signal(|| None::<String>);
    let value = value.unwrap_or(internal_value);
    let highlight = use_signal(|| None::<usize>);

    use_context_provider(|| SelectContext {
        open,
        value,
        highlight,
    });

    rsx! {
        div {
            // 与 DropdownMenu 同款锚点：`relative inline-flex`，供 absolute 内容定位。
            class: cn(&["relative inline-flex", &class]),
            "data-slot": "select",
            "data-state": if open() { "open" } else { "closed" },
            // 键盘处理在内容面板上（那里才知道项数）；这里只兜 `Escape`，
            // 保证触发器获得焦点时也能关掉菜单。
            onkeydown: move |e| {
                if e.key() == Key::Escape {
                    let mut open = open;
                    open.set(false);
                }
            },
            {children}
        }
    }
}

/// 选项分组。
#[component]
pub fn SelectGroup(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: select_group_classes(&class),
            "data-slot": "select-group",
            role: "group",
            {children}
        }
    }
}

/// 判断一个 `Element` 是否「没有实际内容」。
///
/// 为什么不能简单判空：`Element` 在 Dioxus 0.6 里是 `Result<VNode, RenderError>`，
/// 且 `rsx! {}` 与 `rsx! { {"x"} }` 的 VNode 形状**完全一样**
/// （都是 `roots = 1, dynamic_nodes = 1`），所以 `is_ok()` 或「看节点数」都区分不出来。
///
/// 实测四种形状（见本模块测试 `empty_children_are_detected`）：
///
/// | 写法 | `template.roots` | `dynamic_nodes` |
/// |---|---|---|
/// | `rsx! {}` | 1 | `[Placeholder]` |
/// | `rsx! { {"x"} }` | 1 | `[Text]` |
/// | `rsx! { span { "x" } }` | 1 | `[]` |
/// | `VNode::empty()` | 0 | `[]` |
///
/// 因此判据是：
///
/// - **有**动态节点时：全部是 [`DynamicNode::Placeholder`]（Dioxus 对
///   「不渲染任何东西的 fragment」正是用它表示）才算空。
/// - **没有**动态节点时：只有空模板（`roots` 也为空）才算空；`roots` 非空说明
///   模板里含静态节点，那就是有内容。
///
/// ⚠️ 这依赖 `dioxus-core` 的内部表示。升级 Dioxus 后若行为变化，
/// `empty_children_are_detected` 会失败，届时需重新核对上表。
pub fn element_is_empty(el: &Element) -> bool {
    use dioxus::dioxus_core::DynamicNode;

    match el {
        Ok(vnode) => {
            let dynamic = &vnode.dynamic_nodes;
            if dynamic.is_empty() {
                vnode.template.roots.is_empty()
            } else {
                dynamic
                    .iter()
                    .all(|n| matches!(n, DynamicNode::Placeholder(_)))
            }
        }
        // 渲染错误 / 挂起占位：当作「有内容」，不要覆盖调用方给的东西
        Err(_) => false,
    }
}

/// 触发器里的「当前值」占位。
///
/// - 传了非空 `children` → 原样渲染（对应 TS 里显式传 children 覆盖 Radix 自动回填）。
/// - 没传 → 从 context 读选中值自动回填（Radix 的默认行为）。
#[component]
pub fn SelectValue(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let ctx = use_context::<SelectContext>();
    let cls = cn(&["flex items-center gap-1.5", &class]);

    if !element_is_empty(&children) {
        return rsx! {
            span { class: cls, "data-slot": "select-value", {children} }
        };
    }

    let text = (ctx.value)().unwrap_or_default();
    rsx! {
        span { class: cls, "data-slot": "select-value", {text} }
    }
}

/// 触发器（渲染 `button`）。
#[component]
pub fn SelectTrigger(
    #[props(default)] variant: SelectTriggerVariant,
    #[props(default)] size: SelectTriggerSize,
    /// 自定义指示图标；不传则用 `ChevronDown`。
    #[props(default)] indicator: Option<Element>,
    #[props(default)] disabled: bool,
    #[props(default, into)] class: String,
    #[props(default)] onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    let mut ctx = use_context::<SelectContext>();
    let cls = select_trigger_classes(variant, size, &class);
    let is_open = (ctx.open)();
    // Radix 在无选中值时给触发器挂 `data-placeholder`，驱动 `data-placeholder:text-foreground-subtlest`。
    let is_placeholder = (ctx.value)().is_none();

    rsx! {
        button {
            class: cls,
            r#type: "button",
            "data-slot": "select-trigger",
            "data-variant": variant.as_str(),
            "data-size": size.as_str(),
            "data-placeholder": is_placeholder.then_some(""),
            "data-state": if is_open { "open" } else { "closed" },
            "aria-expanded": if is_open { "true" } else { "false" },
            "aria-haspopup": "listbox",
            disabled: disabled,
            onclick: move |e| {
                e.stop_propagation();
                onclick.call(e);
                if !disabled {
                    let next = !(ctx.open)();
                    ctx.open.set(next);
                    if next {
                        ctx.highlight.set(None);
                    }
                }
            },
            {children}
            match indicator {
                Some(el) => rsx! { {el} },
                None => rsx! { IconChevronDown { class: SELECT_TRIGGER_ICON } },
            }
        }
    }
}

/// 内容面板（含上下滚动按钮与 viewport）。
///
/// 关闭时渲染空 `rsx!{}`；展开时把实际内容交给 [`SelectContentInner`]，
/// 让项注册表随每次展开重建（原因见 [`SelectItemsContext`]）。
#[component]
pub fn SelectContent(
    #[props(default)] position: SelectContentPosition,
    /// 仅 `popper` 生效。
    #[props(default = Side::Bottom)] side: Side,
    /// 仅 `popper` 生效；TS 默认 `center`。
    #[props(default = Align::Center)] align: Align,
    /// 仅 `popper` 生效。
    #[props(default = 4.0)] side_offset: f64,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let ctx = use_context::<SelectContext>();
    if !(ctx.open)() {
        return rsx! {};
    }

    rsx! {
        SelectContentInner { position, side, align, side_offset, class, {children} }
    }
}

/// 内容面板的实际渲染体。**只在展开时挂载**，因此这里的 `Signal` 生命周期
/// 与「一次展开」对齐。
#[component]
fn SelectContentInner(
    position: SelectContentPosition,
    side: Side,
    align: Align,
    side_offset: f64,
    class: String,
    children: Element,
) -> Element {
    let mut ctx = use_context::<SelectContext>();
    let items = SelectItemsContext {
        next_index: use_signal(|| 0usize),
        count: use_signal(|| 0usize),
    };
    use_context_provider(|| items);

    rsx! {
        div {
            class: select_content_classes(position, side, align, side_offset, &class),
            "data-slot": "select-content",
            "data-state": "open",
            "data-position": position.as_str(),
            "data-align-trigger": if position.is_item_aligned() { "true" } else { "false" },
            "data-side": side.as_str(),
            role: "listbox",
            // 内容自身可聚焦以接收键盘事件；选项用 `data-highlighted` 表达焦点，
            // 因此不把 tabindex 放到每一项上（与 Radix 的 roving tabindex 效果等价）。
            tabindex: "-1",
            onkeydown: move |e| {
                let nav = match e.key() {
                    Key::Escape => {
                        ctx.open.set(false);
                        return;
                    }
                    Key::ArrowDown => SelectNav::Next,
                    Key::ArrowUp => SelectNav::Prev,
                    Key::Home => SelectNav::First,
                    Key::End => SelectNav::Last,
                    // Enter 交给选项自己的 onclick（浏览器会把 Enter 转成 click）
                    _ => return,
                };
                e.prevent_default();
                let len = (items.count)();
                let next = select_navigate((ctx.highlight)(), len, nav);
                ctx.highlight.set(next);
            },
            SelectScrollUpButton {}
            div {
                class: select_viewport_classes(""),
                "data-slot": "select-viewport",
                "data-position": position.as_str(),
                {children}
            }
            SelectScrollDownButton {}
        }
    }
}

/// 选项分组标题。
#[component]
pub fn SelectLabel(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: select_label_classes(&class),
            "data-slot": "select-label",
            {children}
        }
    }
}

/// 单行选项。
#[component]
pub fn SelectItem(
    /// 该项的值；选中时写回 context。
    #[props(default, into)] value: String,
    #[props(default)] disabled: bool,
    /// 右侧插槽（可交互，例如快捷键提示）；勾号会另占一层。
    #[props(default)] trailing: Option<Element>,
    #[props(default, into)] class: String,
    #[props(default)] onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    let mut ctx = use_context::<SelectContext>();
    let mut items = use_context::<SelectItemsContext>();
    let mut hovered = use_signal(|| false);

    // 项在列表里的序号：按挂载顺序递增（`use_hook` 只在首次渲染跑一次）。
    // 计数器由 `SelectContentInner` 持有，每次展开都从 0 重新开始。
    let index = use_hook(|| {
        let mut next = items.next_index;
        let i = next();
        next.set(i + 1);
        items.count.set(i + 1);
        i
    });

    let is_selected = (ctx.value)().as_deref() == Some(value.as_str());
    let is_highlighted = hovered() || (ctx.highlight)() == Some(index);
    let item_value = value.clone();

    rsx! {
        div {
            class: select_item_classes(&class),
            "data-slot": "select-item",
            "data-value": value,
            "data-index": "{index}",
            "data-disabled": disabled.then_some(""),
            "data-highlighted": is_highlighted.then_some(""),
            "data-state": if is_selected { "checked" } else { "unchecked" },
            "aria-selected": if is_selected { "true" } else { "false" },
            role: "option",
            onmouseenter: move |_| {
                hovered.set(true);
                ctx.highlight.set(Some(index));
            },
            onmouseleave: move |_| hovered.set(false),
            onclick: move |e| {
                e.stop_propagation();
                onclick.call(e);
                if !disabled {
                    ctx.value.set(Some(item_value.clone()));
                    ctx.open.set(false);
                }
            },
            // `trailing` 在前（`pointer-events-auto`），勾号在后，与 TS 的 DOM 顺序一致。
            if let Some(el) = trailing {
                span {
                    class: SELECT_ITEM_TRAILING_INTERACTIVE,
                    "data-slot": "select-item-trailing",
                    {el}
                }
            }
            span {
                class: SELECT_ITEM_TRAILING_BASE,
                "data-slot": "select-item-indicator",
                // Radix 的 `ItemIndicator` 只在选中时挂载
                if is_selected {
                    IconCheck { class: SELECT_ITEM_INDICATOR_ICON }
                }
            }
            span {
                "data-slot": "select-item-text",
                class: "flex items-center gap-2",
                {children}
            }
        }
    }
}

/// 两行选项：图标 + 标题（+ 标记）+ 说明，选中态用尾部对勾。
///
/// 只有 `title` 会进触发器（对应 TS 里只有 `ItemText` 参与 `SelectValue` 回填）。
#[component]
pub fn SelectRichItem(
    #[props(default, into)] value: String,
    /// 左侧 16–18px 图标。
    #[props(default)] icon: Option<Element>,
    /// 标题同一行的标记（例如「默认」）。
    #[props(default)] title_adornment: Option<Element>,
    /// 标题下一行的说明。
    #[props(default)] description: Option<String>,
    #[props(default)] disabled: bool,
    #[props(default, into)] class: String,
    #[props(default)] onclick: EventHandler<MouseEvent>,
    /// 进入 ItemText 的那段文字（只有它会被触发器回填）。
    title: Element,
) -> Element {
    let mut ctx = use_context::<SelectContext>();
    let mut items = use_context::<SelectItemsContext>();
    let mut hovered = use_signal(|| false);

    let index = use_hook(|| {
        let mut next = items.next_index;
        let i = next();
        next.set(i + 1);
        items.count.set(i + 1);
        i
    });

    let is_selected = (ctx.value)().as_deref() == Some(value.as_str());
    let is_highlighted = hovered() || (ctx.highlight)() == Some(index);
    let item_value = value.clone();

    rsx! {
        div {
            class: select_rich_item_classes(&class),
            "data-slot": "select-item",
            "data-value": value,
            "data-index": "{index}",
            "data-disabled": disabled.then_some(""),
            "data-highlighted": is_highlighted.then_some(""),
            "data-state": if is_selected { "checked" } else { "unchecked" },
            "aria-selected": if is_selected { "true" } else { "false" },
            role: "option",
            onmouseenter: move |_| {
                hovered.set(true);
                ctx.highlight.set(Some(index));
            },
            onmouseleave: move |_| hovered.set(false),
            onclick: move |e| {
                e.stop_propagation();
                onclick.call(e);
                if !disabled {
                    ctx.value.set(Some(item_value.clone()));
                    ctx.open.set(false);
                }
            },
            if let Some(el) = icon {
                span {
                    class: "mt-0.5 flex size-4.5 items-center justify-center",
                    "data-slot": "select-rich-item-icon",
                    {el}
                }
            }
            span {
                class: "flex min-w-0 flex-col gap-0.5",
                span {
                    class: "flex min-w-0 items-center gap-1.5",
                    span { "data-slot": "select-item-text", {title} }
                    {title_adornment}
                }
                if let Some(desc) = description {
                    span {
                        class: "text-ui-sm text-foreground-subtle",
                        "data-slot": "select-rich-item-description",
                        {desc}
                    }
                }
            }
            span {
                class: SELECT_RICH_ITEM_INDICATOR_BASE,
                "data-slot": "select-item-indicator",
                if is_selected {
                    IconCheck { class: SELECT_ITEM_INDICATOR_ICON }
                }
            }
        }
    }
}

/// 选项分隔线。
#[component]
pub fn SelectSeparator(
    #[props(default, into)] class: String,
) -> Element {
    rsx! {
        div {
            class: select_separator_classes(&class),
            "data-slot": "select-separator",
            role: "separator",
            "aria-orientation": "horizontal",
        }
    }
}

/// 列表顶部的「向上滚动」按钮。
#[component]
pub fn SelectScrollUpButton(
    #[props(default, into)] class: String,
    #[props(default)] onclick: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        div {
            class: select_scroll_button_classes(&class),
            "data-slot": "select-scroll-up-button",
            onclick: move |e| onclick.call(e),
            IconChevronUp {}
        }
    }
}

/// 列表底部的「向下滚动」按钮。
#[component]
pub fn SelectScrollDownButton(
    #[props(default, into)] class: String,
    #[props(default)] onclick: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        div {
            class: select_scroll_button_classes(&class),
            "data-slot": "select-scroll-down-button",
            onclick: move |e| onclick.call(e),
            IconChevronDown {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_and_size_strings() {
        assert_eq!(SelectTriggerVariant::Input.as_str(), "input");
        assert_eq!(SelectTriggerVariant::Default.as_str(), "default");
        assert_eq!(SelectTriggerVariant::Outline.as_str(), "outline");
        assert_eq!(SelectTriggerVariant::Secondary.as_str(), "secondary");
        assert_eq!(SelectTriggerVariant::Ghost.as_str(), "ghost");
        assert_eq!(SelectTriggerVariant::Destructive.as_str(), "destructive");

        assert_eq!(SelectTriggerSize::Xs.as_str(), "xs");
        assert_eq!(SelectTriggerSize::Sm.as_str(), "sm");
        assert_eq!(SelectTriggerSize::Default.as_str(), "default");
        assert_eq!(SelectTriggerSize::Lg.as_str(), "lg");
    }

    /// `defaultVariants` 是 `{ variant: "input", size: "default" }` —— 默认 variant
    /// 不是叫 `Default` 的那个，容易抄错。
    #[test]
    fn defaults_are_input_and_default_size() {
        assert_eq!(SelectTriggerVariant::default(), SelectTriggerVariant::Input);
        assert_eq!(SelectTriggerSize::default(), SelectTriggerSize::Default);
        assert_eq!(
            SelectContentPosition::default(),
            SelectContentPosition::ItemAligned
        );
    }

    #[test]
    fn trigger_base_keeps_placeholder_and_value_slot_rules() {
        let c = select_trigger_classes(
            SelectTriggerVariant::Input,
            SelectTriggerSize::Default,
            "",
        );
        assert!(c.contains("data-placeholder:text-foreground-subtlest"));
        assert!(c.contains("*:data-[slot=select-value]:line-clamp-1"));
        assert!(c.contains("[&_svg]:pointer-events-none"));
        // 注释里点名的理由：颜色过渡而非 transition-all
        assert!(c.contains("transition-colors"));
        assert!(!c.contains("transition-all"));
    }

    #[test]
    fn trigger_size_axis_changes_height_and_icon_size() {
        let xs = select_trigger_classes(SelectTriggerVariant::Input, SelectTriggerSize::Xs, "");
        assert!(xs.contains("h-5"));
        assert!(xs.contains("[&_svg:not([class*='size-'])]:size-2.5"));

        let lg = select_trigger_classes(SelectTriggerVariant::Input, SelectTriggerSize::Lg, "");
        assert!(lg.contains("h-8"));
        assert!(lg.contains("rounded-lg"));
        assert!(lg.contains("[&_svg:not([class*='size-'])]:size-4"));
    }

    #[test]
    fn trigger_variant_axis_is_orthogonal_to_size() {
        // 同一个 variant 配不同 size，variant 类名必须都在
        let a = select_trigger_classes(SelectTriggerVariant::Outline, SelectTriggerSize::Sm, "");
        let b = select_trigger_classes(SelectTriggerVariant::Outline, SelectTriggerSize::Lg, "");
        assert!(a.contains("aria-expanded:bg-selected"));
        assert!(b.contains("aria-expanded:bg-selected"));
        assert_ne!(a, b);

        // destructive 独有 focus-visible:ring-destructive/20
        let d = select_trigger_classes(
            SelectTriggerVariant::Destructive,
            SelectTriggerSize::Default,
            "",
        );
        assert!(d.contains("focus-visible:ring-destructive/20"));
        assert!(!a.contains("focus-visible:ring-destructive/20"));
    }

    #[test]
    fn content_is_above_tooltip() {
        let c = select_content_classes(
            SelectContentPosition::ItemAligned,
            Side::Bottom,
            Align::Center,
            4.0,
            "",
        );
        // 注释里点名：必须高于 z-50 的 tooltip
        assert!(c.contains("z-[60]"));
        assert!(c.contains("bg-menu"));
        assert!(c.contains("border-popover-border"));
        assert!(c.contains("rounded-lg"));
        assert!(c.contains("min-w-32"));
        assert!(c.contains("shadow-md"));
        assert!(c.contains("[app-region:no-drag]"));
    }

    #[test]
    fn content_position_switches_the_positioning_path() {
        let aligned = select_content_classes(
            SelectContentPosition::ItemAligned,
            Side::Bottom,
            Align::Center,
            4.0,
            "",
        );
        assert!(aligned.contains("absolute"));
        assert!(aligned.contains("top-0"));
        assert!(aligned.contains("left-0"));
        assert!(aligned.contains("w-full"));
        assert!(!aligned.contains("top-full"));

        let popper = select_content_classes(
            SelectContentPosition::Popper,
            Side::Bottom,
            Align::Center,
            4.0,
            "",
        );
        assert!(popper.contains("absolute"));
        assert!(popper.contains("top-full"));
        assert!(popper.contains("mt-[4px]"));
        assert!(!popper.contains("w-full"));
    }

    #[test]
    fn align_trigger_attribute_tracks_position() {
        assert!(SelectContentPosition::ItemAligned.is_item_aligned());
        assert!(!SelectContentPosition::Popper.is_item_aligned());
        assert_eq!(SelectContentPosition::ItemAligned.as_str(), "item-aligned");
        assert_eq!(SelectContentPosition::Popper.as_str(), "popper");
    }

    #[test]
    fn viewport_only_caps_width_in_popper_mode() {
        let c = select_viewport_classes("");
        assert!(c.contains("flex flex-col gap-0.5"));
        assert!(c.contains("data-[position=popper]:w-full"));
        assert!(c.contains("data-[position=popper]:min-w-(--radix-select-trigger-width)"));
        // 注释里点名：不要把 viewport 高度锁成 trigger 高度
        assert!(!c.contains("h-(--radix-select-trigger-height)"));
        assert!(!c.contains("max-h"));
    }

    /// 钉住 [`element_is_empty`] 的判据 —— 它依赖 `dioxus-core` 的内部表示，
    /// 升级 Dioxus 后本测试是第一道警报。
    #[test]
    fn empty_children_are_detected() {
        // `rsx! {}` → roots=1, dynamic_nodes=[Placeholder]
        let empty: Element = rsx! {};
        assert!(element_is_empty(&empty));

        // `VNode::empty()` → roots=0, dynamic_nodes=[]
        assert!(element_is_empty(&VNode::empty()));

        // `rsx! { {"x"} }` → roots=1, dynamic_nodes=[Text]（与空 fragment 形状相同，靠变体区分）
        let dynamic_only: Element = rsx! { { "dynamic" } };
        assert!(!element_is_empty(&dynamic_only));

        // `rsx! { span { "x" } }` → roots=1, dynamic_nodes=[]
        let with_text: Element = rsx! { span { "hello" } };
        assert!(!element_is_empty(&with_text));

        // 只有静态元素、没有文本也要算「非空」
        let empty_div: Element = rsx! { div {} };
        assert!(!element_is_empty(&empty_div));

        // 动态节点是 `None` 时退化成 Placeholder，应当按「空」处理（正是想要的行为）
        let none_child: Element = rsx! { { None::<Element> } };
        assert!(element_is_empty(&none_child));

        // 渲染错误当作「有内容」，避免覆盖调用方给的东西
        let captured: dioxus::CapturedError = "boom".parse().unwrap();
        let err: Element = Err(RenderError::from(captured));
        assert!(!element_is_empty(&err));
    }

    #[test]
    fn item_base_has_highlight_and_disabled_rules() {
        let c = select_item_classes("");
        assert!(c.contains("min-h-7"));
        assert!(c.contains("data-[highlighted]:bg-menu-hover"));
        assert!(c.contains("data-disabled:text-foreground-subtlest"));
        assert!(c.contains("data-disabled:opacity-100"));
        assert!(c.contains("[&_svg:not([class*='size-'])]:size-4"));
    }

    /// 富项与普通项的三处差别必须保持，否则两行版式会塌。
    #[test]
    fn rich_item_differs_from_plain_item() {
        let plain = select_item_classes("");
        let rich = select_rich_item_classes("");

        assert!(plain.contains("min-h-7"));
        assert!(rich.contains("min-h-13"));

        assert!(plain.contains("items-center"));
        assert!(rich.contains("items-start"));

        assert!(plain.contains("px-2"));
        assert!(rich.contains("pr-8 pl-2"));

        // 富项刻意没有 data-disabled:opacity-100
        assert!(plain.contains("data-disabled:opacity-100"));
        assert!(!rich.contains("data-disabled:opacity-100"));
    }

    #[test]
    fn item_trailing_slots_differ_in_pointer_events() {
        // 勾号层不可交互；`trailing` 插槽可交互
        assert!(SELECT_ITEM_TRAILING_BASE.contains("pointer-events-none"));
        assert!(SELECT_ITEM_TRAILING_INTERACTIVE.contains("pointer-events-auto"));
        assert!(!SELECT_ITEM_TRAILING_INTERACTIVE.contains("pointer-events-none"));
        // 富项勾号顶部对齐
        assert!(SELECT_RICH_ITEM_INDICATOR_BASE.contains("top-2.5"));
    }

    #[test]
    fn label_group_separator_and_scroll_buttons() {
        let g = select_group_classes("");
        assert!(g.contains("gap-0.5"));
        assert!(g.contains("scroll-my-1"));
        assert!(g.contains("p-1"));

        let l = select_label_classes("");
        assert!(l.contains("px-2 py-1.5"));
        assert!(l.contains("text-foreground-subtlest"));

        let s = select_separator_classes("");
        assert!(s.contains("my-1"));
        assert!(s.contains("h-px"));
        assert!(s.contains("bg-border"));
        assert!(s.contains("pointer-events-none"));

        // 上下滚动按钮共用同一个常量（TS 里两处字符串相同）
        let b = select_scroll_button_classes("");
        assert!(b.contains("z-10"));
        assert!(b.contains("bg-menu"));
        assert!(b.contains("[&_svg:not([class*='size-'])]:size-3.5"));
    }

    #[test]
    fn user_class_overrides_min_width() {
        let c = select_content_classes(
            SelectContentPosition::Popper,
            Side::Bottom,
            Align::Center,
            4.0,
            "min-w-56",
        );
        assert!(c.contains("min-w-56"));
        assert!(!c.contains("min-w-32"));
    }

    #[test]
    fn navigate_wraps_at_both_ends() {
        // 前进：末尾回到 0
        assert_eq!(select_navigate(None, 3, SelectNav::Next), Some(0));
        assert_eq!(select_navigate(Some(0), 3, SelectNav::Next), Some(1));
        assert_eq!(select_navigate(Some(2), 3, SelectNav::Next), Some(0));

        // 后退：0 回到末尾；无高亮时从末尾进
        assert_eq!(select_navigate(Some(0), 3, SelectNav::Prev), Some(2));
        assert_eq!(select_navigate(Some(1), 3, SelectNav::Prev), Some(0));
        assert_eq!(select_navigate(None, 3, SelectNav::Prev), Some(2));
    }

    #[test]
    fn navigate_home_and_end() {
        assert_eq!(select_navigate(Some(2), 5, SelectNav::First), Some(0));
        assert_eq!(select_navigate(Some(0), 5, SelectNav::Last), Some(4));
        assert_eq!(select_navigate(None, 5, SelectNav::First), Some(0));
        assert_eq!(select_navigate(None, 5, SelectNav::Last), Some(4));
    }

    #[test]
    fn navigate_on_empty_list_is_none() {
        for nav in [SelectNav::Next, SelectNav::Prev, SelectNav::First, SelectNav::Last] {
            assert_eq!(select_navigate(None, 0, nav), None);
            assert_eq!(select_navigate(Some(7), 0, nav), None);
        }
    }

    #[test]
    fn navigate_single_item_stays_put() {
        assert_eq!(select_navigate(Some(0), 1, SelectNav::Next), Some(0));
        assert_eq!(select_navigate(Some(0), 1, SelectNav::Prev), Some(0));
        assert_eq!(select_navigate(None, 1, SelectNav::Next), Some(0));
    }
}
