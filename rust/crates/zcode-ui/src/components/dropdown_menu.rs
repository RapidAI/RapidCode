//! `DropdownMenu` 家族 —— Rust 移植自 `packages/ui/src/components/ui/dropdown-menu.tsx`
//!
//! TS 用 Radix `DropdownMenu.*`（Portal + floating-ui + roving tabindex + 键盘导航）。
//! Rust 侧改用 [`crate::components::overlay`] 约定的纯 CSS 自管理定位。
//!
//! ## 关键移植点：`data-[highlighted]` 是怎么活过来的
//!
//! TS 的菜单项样式几乎全靠 `data-[highlighted]:bg-menu-hover` 这一族类名，而
//! `data-highlighted` 是 Radix 在运行时写上的（鼠标移入或键盘导航到该项）。
//! 如果只是把类名抄过来，**高亮永远不会出现**。
//!
//! 这里给每个菜单项一个自己的 `Signal<bool>`，在 `onmouseenter` / `onmouseleave`
//! 时切换，并把它写成 `data-highlighted` 属性 —— 于是那族类名**原样生效**，
//! 不需要改任何类名，也不需要全局的高亮索引管理。
//!
//! ## 与 TS 的差异
//!
//! 1. 没有 Portal：菜单用 `absolute` 相对锚点包裹元素定位，因此**要求祖先链上没有
//!    `overflow: hidden` / `transform`**（详见 [`crate::components::overlay`]）。
//!    菜单内容保留 `overflow-y-auto`，超出锚点父容器时不会自动避让视口。
//! 2. 没有键盘导航（上下键 / 左右键 / 类型搜索 / `Escape`），也没有焦点陷阱。
//! 3. 没有「点击外部关闭」；改用**触发器切换** + 鼠标离开内容区不关闭
//!    （菜单是点击语义，不是 hover 语义）。
//! 4. `DropdownMenuTrigger` 直接渲染 `button`（等价 TS 的 `asChild` + `Button`）。
//! 5. `DropdownMenuSub` 的子菜单用 hover 展开（Radix 用 hover + 键盘）。

use crate::cn::cn;
use crate::components::button::{button_classes, ButtonSize, ButtonVariant};
use crate::components::overlay::{overlay_anchor_classes, side_align_classes, Align, Side};
use crate::icons::{IconCheck, IconChevronRight};
use dioxus::prelude::*;

/// `DropdownMenuContent` 基础类名（TS 原文，逐字保留）。
pub const DROPDOWN_MENU_CONTENT_BASE: &str = "z-[60] flex flex-col gap-0.5 max-h-(--radix-dropdown-menu-content-available-height) max-w-(--radix-dropdown-menu-content-available-width) min-w-32 origin-(--radix-dropdown-menu-content-transform-origin) overflow-x-hidden overflow-y-auto rounded-lg border border-popover-border bg-menu p-1 text-foreground !shadow-md focus:!shadow-md focus-visible:!shadow-md duration-100 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 data-[state=closed]:overflow-hidden data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95";

/// `DropdownMenuSubContent` 基础类名（TS 原文，逐字保留）。
pub const DROPDOWN_MENU_SUB_CONTENT_BASE: &str = "z-[60] flex flex-col gap-0.5 min-w-32 origin-(--radix-dropdown-menu-content-transform-origin) overflow-hidden rounded-lg border border-popover-border bg-menu p-1 text-foreground !shadow-md focus:!shadow-md focus-visible:!shadow-md duration-100 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95";

/// `DropdownMenuItem` 基础类名。
pub const DROPDOWN_MENU_ITEM_BASE: &str = "group/dropdown-menu-item relative flex min-h-7 cursor-default items-center gap-2 rounded-md px-2 py-1 text-ui-base/relaxed text-foreground outline-hidden select-none data-[highlighted]:bg-menu-hover data-[highlighted]:text-foreground data-inset:pl-7.5 data-[variant=destructive]:text-destructive data-[variant=destructive]:data-[highlighted]:bg-destructive data-[variant=destructive]:data-[highlighted]:text-destructive-foreground data-disabled:pointer-events-none data-disabled:text-foreground-subtlest data-disabled:opacity-100 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4 data-[variant=destructive]:*:[svg]:text-current data-[variant=destructive]:data-[highlighted]:*:[svg]:text-destructive-foreground";

/// 复选 / 单选菜单项共用的基础类名（TS 里两处字符串完全相同）。
pub const DROPDOWN_MENU_CHECK_ITEM_BASE: &str = "relative flex min-h-7 cursor-default items-center gap-2 rounded-md py-1.5 pr-8 pl-2 text-ui-base text-foreground outline-hidden select-none data-[highlighted]:bg-menu-hover data-[highlighted]:text-foreground data-inset:pl-7.5 data-disabled:pointer-events-none data-disabled:text-foreground-subtlest data-disabled:opacity-100 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4";

/// 复选 / 单选指示器包裹层类名。
pub const DROPDOWN_MENU_ITEM_INDICATOR_BASE: &str =
    "pointer-events-none absolute right-2 flex items-center justify-center";

/// 指示器内的勾号类名。
pub const DROPDOWN_MENU_ITEM_INDICATOR_ICON: &str = "size-4 text-foreground-subtle";

/// `DropdownMenuGroup` / `DropdownMenuRadioGroup` 基础类名。
pub const DROPDOWN_MENU_GROUP_BASE: &str = "flex flex-col gap-0.5";

/// `DropdownMenuLabel` 基础类名。
pub const DROPDOWN_MENU_LABEL_BASE: &str =
    "px-2 py-1.5 text-ui-base font-medium text-foreground-subtlest data-inset:pl-7.5";

/// `DropdownMenuSeparator` 基础类名。
pub const DROPDOWN_MENU_SEPARATOR_BASE: &str = "-mx-1 my-1 h-px bg-border";

/// `DropdownMenuShortcut` 基础类名。
pub const DROPDOWN_MENU_SHORTCUT_BASE: &str = "ml-auto text-ui-xs tracking-widest text-foreground-subtlest group-data-[highlighted]/dropdown-menu-item:text-foreground-subtle";

/// `DropdownMenuSubTrigger` 基础类名。
pub const DROPDOWN_MENU_SUB_TRIGGER_BASE: &str = "group/dropdown-menu-sub-trigger flex min-h-7 cursor-default items-center gap-2 rounded-md px-2 py-1 text-ui-base text-foreground outline-hidden select-none data-[highlighted]:bg-menu-hover data-[highlighted]:text-foreground data-inset:pl-7.5 data-[state=open]:bg-menu-hover data-[state=open]:text-foreground [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4";

/// `DropdownMenuSubTrigger` 右侧箭头类名。
pub const DROPDOWN_MENU_SUB_TRIGGER_CHEVRON: &str =
    "ml-auto size-4 text-foreground-subtle group-data-[highlighted]/dropdown-menu-sub-trigger:text-foreground-subtle";

/// 菜单项语义变体（对应 TS 的 `variant`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DropdownMenuItemVariant {
    #[default]
    Default,
    Destructive,
}

impl DropdownMenuItemVariant {
    pub fn as_str(&self) -> &'static str {
        match self {
            DropdownMenuItemVariant::Default => "default",
            DropdownMenuItemVariant::Destructive => "destructive",
        }
    }

    /// 变体附加类名。TS 里变体完全由 `data-[variant=...]` 选择器驱动
    /// （类名都在 `DROPDOWN_MENU_ITEM_BASE` 里），所以这里恒为空字符串。
    /// 保留这个方法是为了让调用形状与 `cva` 一致，将来若新增需要附加类名的
    /// 变体不必改调用点。
    pub fn classes(&self) -> &'static str {
        ""
    }
}

pub fn dropdown_menu_content_classes(
    side: Side,
    align: Align,
    side_offset: f64,
    class: &str,
) -> String {
    cn(&[
        DROPDOWN_MENU_CONTENT_BASE,
        &side_align_classes(side, align, side_offset),
        class,
    ])
}

pub fn dropdown_menu_sub_content_classes(
    side: Side,
    align: Align,
    side_offset: f64,
    class: &str,
) -> String {
    cn(&[
        DROPDOWN_MENU_SUB_CONTENT_BASE,
        &side_align_classes(side, align, side_offset),
        class,
    ])
}

pub fn dropdown_menu_item_classes(
    variant: DropdownMenuItemVariant,
    inset: bool,
    class: &str,
) -> String {
    cn(&[
        DROPDOWN_MENU_ITEM_BASE,
        variant.classes(),
        if inset { "pl-7.5" } else { "" },
        class,
    ])
}

pub fn dropdown_menu_check_item_classes(inset: bool, class: &str) -> String {
    cn(&[
        DROPDOWN_MENU_CHECK_ITEM_BASE,
        if inset { "pl-7.5" } else { "" },
        class,
    ])
}

pub fn dropdown_menu_label_classes(inset: bool, class: &str) -> String {
    cn(&[
        DROPDOWN_MENU_LABEL_BASE,
        if inset { "pl-7.5" } else { "" },
        class,
    ])
}

pub fn dropdown_menu_separator_classes(class: &str) -> String {
    cn(&[DROPDOWN_MENU_SEPARATOR_BASE, class])
}

pub fn dropdown_menu_shortcut_classes(class: &str) -> String {
    cn(&[DROPDOWN_MENU_SHORTCUT_BASE, class])
}

pub fn dropdown_menu_sub_trigger_classes(inset: bool, class: &str) -> String {
    cn(&[
        DROPDOWN_MENU_SUB_TRIGGER_BASE,
        if inset { "pl-7.5" } else { "" },
        class,
    ])
}

/// 由 `DropdownMenu` 提供、内容区消费的共享状态。
#[derive(Clone, Copy)]
struct DropdownMenuContext {
    open: Signal<bool>,
}

/// 由 `DropdownMenuSub` 提供、子菜单消费的共享状态。
#[derive(Clone, Copy)]
struct DropdownMenuSubContext {
    open: Signal<bool>,
    side: Side,
    side_offset: f64,
}

/// 读取当前下拉菜单的展开状态。
pub fn use_dropdown_menu_open() -> Signal<bool> {
    use_context::<DropdownMenuContext>().open
}

/// 下拉菜单根容器（状态持有者 + 定位锚点）。
#[component]
pub fn DropdownMenu(
    #[props(default)] open: Option<Signal<bool>>,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let internal = use_signal(|| false);
    let open = open.unwrap_or(internal);
    use_context_provider(|| DropdownMenuContext { open });

    rsx! {
        div {
            class: overlay_anchor_classes(&class),
            "data-slot": "dropdown-menu",
            "data-state": if open() { "open" } else { "closed" },
            {children}
        }
    }
}

/// 菜单触发器（渲染 `button`，等价 TS 的 `asChild` + `Button`）。
#[component]
pub fn DropdownMenuTrigger(
    #[props(default)] variant: ButtonVariant,
    #[props(default)] size: ButtonSize,
    #[props(default, into)] class: String,
    #[props(default)] disabled: bool,
    #[props(default)] onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    let mut ctx = use_context::<DropdownMenuContext>();
    let cls = button_classes(variant, size, &class);
    let state = if (ctx.open)() { "open" } else { "closed" };

    rsx! {
        button {
            class: cls,
            r#type: "button",
            "data-slot": "dropdown-menu-trigger",
            "data-state": state,
            "aria-expanded": if (ctx.open)() { "true" } else { "false" },
            disabled: disabled,
            onclick: move |e| {
                // 菜单触发器通常嵌在可点击的行/卡片里（会话列表项、表格行）。
                // Radix 的 Trigger 也会吃掉 pointerdown，否则点「⋯」会连带触发
                // 整行的 onclick。这里显式阻止冒泡，保持与 Radix 一致。
                e.stop_propagation();
                onclick.call(e);
                if !disabled {
                    let next = !(ctx.open)();
                    ctx.open.set(next);
                }
            },
            {children}
        }
    }
}

/// Radix `Portal` 的等价物：透传容器，不产生 DOM。
#[component]
pub fn DropdownMenuPortal(children: Element) -> Element {
    rsx! { {children} }
}

/// 菜单内容面板。`side` / `align` / `side_offset` 放在 Content 上（同 TS）。
#[component]
pub fn DropdownMenuContent(
    /// 对应 Radix `side`，默认 `bottom`。
    #[props(default = Side::Bottom)] side: Side,
    /// 对应 Radix `align`，默认 `start`（与 TS 一致）。
    #[props(default = Align::Start)] align: Align,
    /// 对应 Radix `sideOffset`，默认 `2`（与 TS 一致）。
    #[props(default = 2.0)] side_offset: f64,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let ctx = use_context::<DropdownMenuContext>();
    if !(ctx.open)() {
        return rsx! {};
    }

    rsx! {
        div {
            class: dropdown_menu_content_classes(side, align, side_offset, &class),
            "data-slot": "dropdown-menu-content",
            "data-state": "open",
            "data-side": side.as_str(),
            "data-align": align.as_str(),
            role: "menu",
            {children}
        }
    }
}

/// 菜单分组。
#[component]
pub fn DropdownMenuGroup(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: cn(&[DROPDOWN_MENU_GROUP_BASE, &class]),
            "data-slot": "dropdown-menu-group",
            role: "group",
            {children}
        }
    }
}

/// 普通菜单项。
#[component]
pub fn DropdownMenuItem(
    #[props(default)] variant: DropdownMenuItemVariant,
    #[props(default)] inset: bool,
    #[props(default)] disabled: bool,
    #[props(default, into)] class: String,
    #[props(default)] onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    // `data-highlighted` 由本项的 hover 状态驱动（见模块文档）
    let mut hovered = use_signal(|| false);

    rsx! {
        div {
            class: dropdown_menu_item_classes(variant, inset, &class),
            "data-slot": "dropdown-menu-item",
            "data-variant": variant.as_str(),
            "data-inset": inset.then_some(""),
            "data-disabled": disabled.then_some(""),
            "data-highlighted": hovered().then_some(""),
            role: "menuitem",
            tabindex: "-1",
            onmouseenter: move |_| hovered.set(true),
            onmouseleave: move |_| hovered.set(false),
            onclick: move |e| {
                if !disabled {
                    onclick.call(e);
                }
            },
            {children}
        }
    }
}

/// 带勾选标记的菜单项（勾号在右侧）。
#[component]
pub fn DropdownMenuCheckboxItem(
    #[props(default)] checked: bool,
    #[props(default)] inset: bool,
    #[props(default)] disabled: bool,
    #[props(default, into)] class: String,
    #[props(default)] onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    let mut hovered = use_signal(|| false);

    rsx! {
        div {
            class: dropdown_menu_check_item_classes(inset, &class),
            "data-slot": "dropdown-menu-checkbox-item",
            "data-inset": inset.then_some(""),
            "data-disabled": disabled.then_some(""),
            "data-highlighted": hovered().then_some(""),
            "aria-checked": if checked { "true" } else { "false" },
            role: "menuitemcheckbox",
            tabindex: "-1",
            onmouseenter: move |_| hovered.set(true),
            onmouseleave: move |_| hovered.set(false),
            onclick: move |e| {
                if !disabled {
                    onclick.call(e);
                }
            },
            span {
                class: DROPDOWN_MENU_ITEM_INDICATOR_BASE,
                "data-slot": "dropdown-menu-checkbox-item-indicator",
                // Radix 的 `ItemIndicator` 只在选中时挂载
                if checked {
                    IconCheck { class: DROPDOWN_MENU_ITEM_INDICATOR_ICON }
                }
            }
            {children}
        }
    }
}

/// 单选菜单分组。
#[component]
pub fn DropdownMenuRadioGroup(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: cn(&[DROPDOWN_MENU_GROUP_BASE, &class]),
            "data-slot": "dropdown-menu-radio-group",
            role: "group",
            {children}
        }
    }
}

/// 单选菜单项（勾号在右侧）。
#[component]
pub fn DropdownMenuRadioItem(
    #[props(default)] checked: bool,
    #[props(default)] inset: bool,
    #[props(default)] disabled: bool,
    #[props(default, into)] class: String,
    #[props(default)] onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    let mut hovered = use_signal(|| false);

    rsx! {
        div {
            class: dropdown_menu_check_item_classes(inset, &class),
            "data-slot": "dropdown-menu-radio-item",
            "data-inset": inset.then_some(""),
            "data-disabled": disabled.then_some(""),
            "data-highlighted": hovered().then_some(""),
            "aria-checked": if checked { "true" } else { "false" },
            role: "menuitemradio",
            tabindex: "-1",
            onmouseenter: move |_| hovered.set(true),
            onmouseleave: move |_| hovered.set(false),
            onclick: move |e| {
                if !disabled {
                    onclick.call(e);
                }
            },
            span {
                class: DROPDOWN_MENU_ITEM_INDICATOR_BASE,
                "data-slot": "dropdown-menu-radio-item-indicator",
                if checked {
                    IconCheck { class: DROPDOWN_MENU_ITEM_INDICATOR_ICON }
                }
            }
            {children}
        }
    }
}

/// 菜单分组标题。
#[component]
pub fn DropdownMenuLabel(
    #[props(default)] inset: bool,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: dropdown_menu_label_classes(inset, &class),
            "data-slot": "dropdown-menu-label",
            "data-inset": inset.then_some(""),
            {children}
        }
    }
}

/// 菜单分隔线。
#[component]
pub fn DropdownMenuSeparator(
    #[props(default, into)] class: String,
) -> Element {
    rsx! {
        div {
            class: dropdown_menu_separator_classes(&class),
            "data-slot": "dropdown-menu-separator",
            role: "separator",
            "aria-orientation": "horizontal",
        }
    }
}

/// 菜单项右侧的快捷键提示。
#[component]
pub fn DropdownMenuShortcut(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        span {
            class: dropdown_menu_shortcut_classes(&class),
            "data-slot": "dropdown-menu-shortcut",
            {children}
        }
    }
}

/// 子菜单容器（hover 展开）。
#[component]
pub fn DropdownMenuSub(
    /// 受控模式：由调用方持有子菜单展开状态（Radix 的 `Sub` 同样支持受控）。
    #[props(default)] open: Option<Signal<bool>>,
    #[props(default = Side::Right)] side: Side,
    /// 子菜单与父项的间距，默认 `2`。
    #[props(default = 2.0)] side_offset: f64,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let internal = use_signal(|| false);
    let mut open = open.unwrap_or(internal);
    use_context_provider(|| DropdownMenuSubContext {
        open,
        side,
        side_offset,
    });

    rsx! {
        div {
            class: cn(&["relative", &class]),
            "data-slot": "dropdown-menu-sub",
            "data-state": if open() { "open" } else { "closed" },
            onmouseenter: move |_| open.set(true),
            onmouseleave: move |_| open.set(false),
            {children}
        }
    }
}

/// 子菜单触发项（右侧带箭头）。
#[component]
pub fn DropdownMenuSubTrigger(
    #[props(default)] inset: bool,
    #[props(default)] disabled: bool,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let mut ctx = use_context::<DropdownMenuSubContext>();
    let mut hovered = use_signal(|| false);
    let state = if (ctx.open)() { "open" } else { "closed" };

    rsx! {
        div {
            class: dropdown_menu_sub_trigger_classes(inset, &class),
            "data-slot": "dropdown-menu-sub-trigger",
            "data-inset": inset.then_some(""),
            "data-disabled": disabled.then_some(""),
            "data-highlighted": hovered().then_some(""),
            "data-state": state,
            role: "menuitem",
            tabindex: "-1",
            onmouseenter: move |_| {
                hovered.set(true);
                ctx.open.set(true);
            },
            onmouseleave: move |_| hovered.set(false),
            onclick: move |_| {
                if !disabled {
                    let next = !(ctx.open)();
                    ctx.open.set(next);
                }
            },
            {children}
            IconChevronRight { class: DROPDOWN_MENU_SUB_TRIGGER_CHEVRON }
        }
    }
}

/// 子菜单内容面板。
#[component]
pub fn DropdownMenuSubContent(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let ctx = use_context::<DropdownMenuSubContext>();
    if !(ctx.open)() {
        return rsx! {};
    }

    rsx! {
        div {
            class: dropdown_menu_sub_content_classes(
                ctx.side,
                Align::Start,
                ctx.side_offset,
                &class,
            ),
            "data-slot": "dropdown-menu-sub-content",
            "data-state": "open",
            "data-side": ctx.side.as_str(),
            role: "menu",
            {children}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_strings() {
        assert_eq!(DropdownMenuItemVariant::Default.as_str(), "default");
        assert_eq!(DropdownMenuItemVariant::Destructive.as_str(), "destructive");
        assert_eq!(
            DropdownMenuItemVariant::default(),
            DropdownMenuItemVariant::Default
        );
    }

    #[test]
    fn content_uses_z60_and_menu_surface() {
        let c = dropdown_menu_content_classes(Side::Bottom, Align::Start, 2.0, "");
        assert!(c.contains("z-[60]"));
        assert!(c.contains("bg-menu"));
        assert!(c.contains("border-popover-border"));
        assert!(c.contains("rounded-lg"));
        assert!(c.contains("min-w-32"));
        assert!(c.contains("!shadow-md"));
    }

    #[test]
    fn content_positioning() {
        let c = dropdown_menu_content_classes(Side::Bottom, Align::Start, 2.0, "");
        assert!(c.contains("absolute"));
        assert!(c.contains("top-full"));
        assert!(c.contains("left-0"));
        assert!(c.contains("mt-[2px]"));
    }

    #[test]
    fn item_base_has_highlight_and_variant_rules() {
        let c = dropdown_menu_item_classes(DropdownMenuItemVariant::Default, false, "");
        assert!(c.contains("data-[highlighted]:bg-menu-hover"));
        assert!(c.contains("data-[variant=destructive]:text-destructive"));
        assert!(c.contains("data-disabled:text-foreground-subtlest"));
        assert!(c.contains("[&_svg:not([class*='size-'])]:size-4"));
    }

    #[test]
    fn item_inset_adds_padding() {
        let with = dropdown_menu_item_classes(DropdownMenuItemVariant::Default, true, "");
        // `inset` 时追加一个裸的 pl-7.5（在类名末尾，覆盖基类的 px-2 里的左内边距）
        assert!(with.split_whitespace().any(|t| t == "pl-7.5"));
        // px-2 与 pl-7.5 分属不同组（px vs pl），tailwind-merge 语义下两者都保留
        assert!(with.split_whitespace().any(|t| t == "px-2"));

        let without = dropdown_menu_item_classes(DropdownMenuItemVariant::Default, false, "");
        assert!(!without.split_whitespace().any(|t| t == "pl-7.5"));
        assert!(without.split_whitespace().any(|t| t == "px-2"));
        // 无论 inset 与否，基类的 data-inset 规则都保留
        assert!(without.contains("data-inset:pl-7.5"));
    }

    #[test]
    fn check_item_base_matches_radio_item_base() {
        // TS 里两处字符串完全相同，这里共享同一个常量
        let c = dropdown_menu_check_item_classes(false, "");
        assert!(c.contains("pr-8"));
        assert!(c.contains("pl-2"));
        assert!(c.contains("data-[highlighted]:bg-menu-hover"));
    }

    #[test]
    fn label_and_shortcut() {
        let l = dropdown_menu_label_classes(false, "");
        assert!(l.contains("font-medium"));
        assert!(l.contains("text-foreground-subtlest"));
        assert!(dropdown_menu_label_classes(true, "").contains("pl-7.5"));

        let s = dropdown_menu_shortcut_classes("");
        assert!(s.contains("ml-auto"));
        assert!(s.contains("tracking-widest"));
        assert!(s.contains("group-data-[highlighted]/dropdown-menu-item:text-foreground-subtle"));
    }

    #[test]
    fn separator_is_negative_inset_line() {
        let c = dropdown_menu_separator_classes("");
        assert!(c.contains("-mx-1"));
        assert!(c.contains("my-1"));
        assert!(c.contains("h-px"));
        assert!(c.contains("bg-border"));
    }

    #[test]
    fn sub_trigger_has_open_state_and_chevron() {
        let c = dropdown_menu_sub_trigger_classes(false, "");
        assert!(c.contains("data-[state=open]:bg-menu-hover"));
        assert!(c.contains("group/dropdown-menu-sub-trigger"));
        assert!(DROPDOWN_MENU_SUB_TRIGGER_CHEVRON.contains("ml-auto"));
    }

    #[test]
    fn sub_content_has_no_scroll_cap() {
        let c = dropdown_menu_sub_content_classes(Side::Right, Align::Start, 2.0, "");
        assert!(c.contains("min-w-32"));
        assert!(c.contains("overflow-hidden"));
        assert!(!c.contains("overflow-y-auto"));
        assert!(c.contains("left-full"));
        assert!(c.contains("ml-[2px]"));
    }

    #[test]
    fn user_class_overrides_min_width() {
        let c = dropdown_menu_content_classes(Side::Bottom, Align::Start, 2.0, "min-w-48");
        assert!(c.contains("min-w-48"));
        assert!(!c.contains("min-w-32"));
    }
}
