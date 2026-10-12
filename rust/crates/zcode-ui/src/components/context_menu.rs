//! `ContextMenu` 家族 —— Rust 移植自
//! `packages/ui/src/components/ui/context-menu.tsx`。
//!
//! 结构与 [`crate::components::dropdown_menu`] 几乎同构（同样的菜单语言、
//! 同样的 item / sub 类名），唯一实质差别是**锚点**：
//!
//! - `DropdownMenu` 锚在触发按钮上 → 走
//!   [`crate::components::overlay::side_align_classes`]（`absolute`）；
//! - `ContextMenu` 锚在**右键光标**上，没有可包裹的元素 → 走
//!   [`crate::components::overlay::cursor_overlay_classes`]（`fixed` +
//!   内联 `left/top`），并用
//!   [`clamp_to_viewport`](crate::components::overlay::clamp_to_viewport)
//!   做边界避让。
//!
//! 与 TS 的另一处差别：Radix 的 `ContextMenu.Root` 自己监听 `contextmenu`
//! 事件；这里由 `ContextMenuTrigger` 上的 `oncontextmenu` 承担，并把
//! `clientX/clientY` 写进 context，`ContextMenuContent` 从 context 取坐标。
//!
//! ## 类名对齐
//!
//! Content 用的是 `min-w-44`（DropdownMenu 是 `min-w-32`）与普通 `shadow-md`
//! （DropdownMenu 是 `!shadow-md`），这两处差异在 TS 里就是有意的，逐字保留。

use crate::cn::cn;
use crate::components::overlay::{clamp_to_viewport, cursor_overlay_classes, cursor_position_style};
use crate::icons::{IconCheck, IconChevronRight};
use dioxus::prelude::*;

/// `ContextMenuContent` 基础类名（TS 原文，逐字保留）。
pub const CONTEXT_MENU_CONTENT_BASE: &str = "z-[60] flex flex-col gap-0.5 min-w-44 origin-(--radix-context-menu-content-transform-origin) overflow-hidden rounded-lg border border-popover-border bg-menu p-1 text-foreground shadow-md duration-100 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95";

/// `ContextMenuSubContent` 基础类名（TS 原文，逐字保留）。
pub const CONTEXT_MENU_SUB_CONTENT_BASE: &str = "z-[60] flex flex-col gap-0.5 min-w-32 origin-(--radix-context-menu-content-transform-origin) overflow-hidden rounded-lg border border-popover-border bg-menu p-1 text-foreground shadow-md duration-100 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95";

/// `ContextMenuItem` 基础类名（与 `DropdownMenuItem` 完全一致）。
pub const CONTEXT_MENU_ITEM_BASE: &str = "group/context-menu-item relative flex min-h-7 cursor-default items-center gap-2 rounded-md px-2 py-1 text-ui-base/relaxed text-foreground outline-hidden select-none data-[highlighted]:bg-menu-hover data-[highlighted]:text-foreground data-inset:pl-7.5 data-[variant=destructive]:text-destructive data-[variant=destructive]:data-[highlighted]:bg-destructive data-[variant=destructive]:data-[highlighted]:text-destructive-foreground data-disabled:pointer-events-none data-disabled:text-foreground-subtlest data-disabled:opacity-100 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4 data-[variant=destructive]:*:[svg]:text-current data-[variant=destructive]:data-[highlighted]:*:[svg]:text-destructive-foreground";

/// `ContextMenuSubTrigger` 基础类名。
pub const CONTEXT_MENU_SUB_TRIGGER_BASE: &str = "group/context-menu-sub-trigger flex min-h-7 cursor-default items-center gap-2 rounded-md px-2 py-1 text-ui-base text-foreground outline-hidden select-none data-[highlighted]:bg-menu-hover data-[highlighted]:text-foreground data-inset:pl-7.5 data-[state=open]:bg-menu-hover data-[state=open]:text-foreground data-disabled:pointer-events-none data-disabled:text-foreground-subtlest data-disabled:opacity-100 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4";

/// `ContextMenuSubTrigger` 右侧箭头类名。
pub const CONTEXT_MENU_SUB_TRIGGER_CHEVRON: &str =
    "ml-auto size-4 text-foreground-subtle group-data-[highlighted]/context-menu-sub-trigger:text-foreground-subtle";

/// `ContextMenuSeparator` 基础类名。
pub const CONTEXT_MENU_SEPARATOR_BASE: &str = "-mx-1 my-1 h-px bg-border";

/// 菜单项变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ContextMenuItemVariant {
    #[default]
    Default,
    Destructive,
}

impl ContextMenuItemVariant {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContextMenuItemVariant::Default => "default",
            ContextMenuItemVariant::Destructive => "destructive",
        }
    }
}

/// 光标位置（视口坐标，对应 `MouseEvent.clientX/clientY`）。
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct CursorPoint {
    pub x: f64,
    pub y: f64,
}

/// 菜单根状态。
#[derive(Clone, Copy)]
struct ContextMenuContext {
    open: Signal<bool>,
    cursor: Signal<CursorPoint>,
}

/// 子菜单共享状态（由 `ContextMenuSub` 提供）。
#[derive(Clone, Copy)]
struct ContextMenuSubContext {
    open: Signal<bool>,
}

/// 读取当前右键菜单的展开状态。
pub fn use_context_menu_open() -> Signal<bool> {
    use_context::<ContextMenuContext>().open
}

/// 右键菜单根容器（状态持有者，不产生 DOM）。
///
/// 与 TS 一致：Radix 的 `Root` 只提供 context，不渲染元素。
#[component]
pub fn ContextMenu(
    #[props(default)] open: Option<Signal<bool>>,
    children: Element,
) -> Element {
    let internal = use_signal(|| false);
    let open = open.unwrap_or(internal);
    let cursor = use_signal(CursorPoint::default);
    use_context_provider(|| ContextMenuContext { open, cursor });
    rsx! { {children} }
}

/// 右键触发区。命中 `contextmenu` 时记录光标坐标并展开菜单。
#[component]
pub fn ContextMenuTrigger(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let mut ctx = use_context::<ContextMenuContext>();
    rsx! {
        div {
            class: class,
            "data-slot": "context-menu-trigger",
            "data-state": if (ctx.open)() { "open" } else { "closed" },
            oncontextmenu: move |e| {
                e.prevent_default();
                let c = e.client_coordinates();
                ctx.cursor.set(CursorPoint { x: c.x, y: c.y });
                ctx.open.set(true);
            },
            {children}
        }
    }
}

/// 菜单内容面板。关闭时整体不渲染（Portal + Presence 语义）。
#[component]
pub fn ContextMenuContent(
    /// 视口尺寸，用于边界避让。传 `None` 时不做夹取。
    #[props(default)] viewport: Option<(f64, f64)>,
    /// 浮层自身尺寸（估算值即可，用于避让）。
    #[props(default = (176.0, 240.0))] content_size: (f64, f64),
    /// 距视口边缘的安全间距。
    #[props(default = 8.0)] padding: f64,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let ctx = use_context::<ContextMenuContext>();
    if !(ctx.open)() {
        return rsx! {};
    }

    let cursor = (ctx.cursor)();
    let (x, y) = match viewport {
        Some((vw, vh)) => clamp_to_viewport(
            cursor.x,
            cursor.y,
            vw,
            vh,
            content_size.0,
            content_size.1,
            padding,
        ),
        None => (cursor.x, cursor.y),
    };

    rsx! {
        div {
            class: cursor_overlay_classes(&cn(&[CONTEXT_MENU_CONTENT_BASE, &class])),
            "data-slot": "context-menu-content",
            "data-state": "open",
            role: "menu",
            style: cursor_position_style(x, y),
            {children}
        }
    }
}

/// 普通菜单项。
#[component]
pub fn ContextMenuItem(
    #[props(default)] variant: ContextMenuItemVariant,
    #[props(default)] inset: bool,
    #[props(default)] disabled: bool,
    #[props(default, into)] class: String,
    #[props(default)] onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    let ctx = use_context::<ContextMenuContext>();
    // Radix 的高亮态由内部漫游焦点驱动；这里用逐项 Signal + hover 事件替代，
    // 样式表里的 `data-[highlighted]:` 才有输入（同 DropdownMenuItem）。
    let mut highlighted = use_signal(|| false);
    let close = ctx.open;

    rsx! {
        div {
            class: cn(&[CONTEXT_MENU_ITEM_BASE, &class]),
            "data-slot": "context-menu-item",
            "data-variant": variant.as_str(),
            "data-inset": inset.then_some(""),
            "data-disabled": disabled.then_some(""),
            "data-highlighted": highlighted().then_some(""),
            role: "menuitem",
            tabindex: "-1",
            onmouseenter: move |_| highlighted.set(true),
            onmouseleave: move |_| highlighted.set(false),
            onclick: move |e| {
                if !disabled {
                    onclick.call(e);
                    let mut c = close;
                    c.set(false);
                }
            },
            {children}
        }
    }
}

/// 分隔线。
#[component]
pub fn ContextMenuSeparator(#[props(default, into)] class: String) -> Element {
    rsx! {
        div {
            class: cn(&[CONTEXT_MENU_SEPARATOR_BASE, &class]),
            "data-slot": "context-menu-separator",
            role: "separator",
            "aria-orientation": "horizontal",
        }
    }
}

/// 子菜单根（hover 展开）。
#[component]
pub fn ContextMenuSub(
    #[props(default)] open: Option<Signal<bool>>,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let internal = use_signal(|| false);
    let open = open.unwrap_or(internal);
    use_context_provider(|| ContextMenuSubContext { open });

    rsx! {
        div {
            class: cn(&["relative", &class]),
            "data-slot": "context-menu-sub",
            "data-state": if open() { "open" } else { "closed" },
            onmouseenter: move |_| {
                let mut o = open;
                o.set(true);
            },
            onmouseleave: move |_| {
                let mut o = open;
                o.set(false);
            },
            {children}
        }
    }
}

/// 子菜单触发器（自动追加右侧 chevron）。
#[component]
pub fn ContextMenuSubTrigger(
    #[props(default)] inset: bool,
    #[props(default)] disabled: bool,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let ctx = use_context::<ContextMenuSubContext>();
    let mut highlighted = use_signal(|| false);

    rsx! {
        div {
            class: cn(&[CONTEXT_MENU_SUB_TRIGGER_BASE, &class]),
            "data-slot": "context-menu-sub-trigger",
            "data-state": if (ctx.open)() { "open" } else { "closed" },
            "data-inset": inset.then_some(""),
            "data-disabled": disabled.then_some(""),
            "data-highlighted": highlighted().then_some(""),
            role: "menuitem",
            tabindex: "-1",
            onmouseenter: move |_| highlighted.set(true),
            onmouseleave: move |_| highlighted.set(false),
            {children}
            IconChevronRight { class: CONTEXT_MENU_SUB_TRIGGER_CHEVRON }
        }
    }
}

/// 子菜单内容（相对触发器从右侧展开）。
#[component]
pub fn ContextMenuSubContent(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let ctx = use_context::<ContextMenuSubContext>();
    if !(ctx.open)() {
        return rsx! {};
    }

    rsx! {
        div {
            class: cn(&[
                "absolute left-full top-0 ml-[2px]",
                CONTEXT_MENU_SUB_CONTENT_BASE,
                &class,
            ]),
            "data-slot": "context-menu-sub-content",
            "data-state": "open",
            "data-side": "right",
            role: "menu",
            {children}
        }
    }
}

/// 选中标记（供调用方放进 `ContextMenuItem`）。
#[component]
pub fn ContextMenuCheckIcon(#[props(default, into)] class: String) -> Element {
    rsx! {
        span {
            class: "pointer-events-none absolute right-2 flex items-center justify-center",
            "data-slot": "context-menu-item-indicator",
            IconCheck { class: cn(&["size-4 text-foreground-subtle", &class]) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_base_matches_ts() {
        // 与 DropdownMenu 的两处有意差异：min-w-44（不是 32）、普通 shadow-md（不是 !shadow-md）
        assert!(CONTEXT_MENU_CONTENT_BASE.contains("min-w-44"));
        assert!(CONTEXT_MENU_CONTENT_BASE.contains(" shadow-md "));
        assert!(!CONTEXT_MENU_CONTENT_BASE.contains("!shadow-md"));
        // 层级：右键菜单与 Dropdown 同档，高于 tooltip
        assert!(CONTEXT_MENU_CONTENT_BASE.contains("z-[60]"));
    }

    #[test]
    fn sub_content_restarts_at_min_w_32() {
        // TS 注释：子菜单重新起算圆角/宽度
        assert!(CONTEXT_MENU_SUB_CONTENT_BASE.contains("min-w-32"));
        assert!(!CONTEXT_MENU_SUB_CONTENT_BASE.contains("min-w-44"));
    }

    #[test]
    fn item_keeps_inset_and_destructive_variants() {
        assert!(CONTEXT_MENU_ITEM_BASE.contains("data-inset:pl-7.5"));
        assert!(CONTEXT_MENU_ITEM_BASE.contains("data-[variant=destructive]:text-destructive"));
        assert!(CONTEXT_MENU_ITEM_BASE.contains("data-[highlighted]:bg-menu-hover"));
        assert!(CONTEXT_MENU_ITEM_BASE.contains("group/context-menu-item"));
    }

    #[test]
    fn item_group_name_matches_the_ts_scope() {
        // 分组名必须是 context-menu-item，否则 sub-trigger 的 group-data-* 选择器错位
        assert!(CONTEXT_MENU_SUB_TRIGGER_BASE.contains("group/context-menu-sub-trigger"));
        assert!(
            CONTEXT_MENU_SUB_TRIGGER_CHEVRON
                .contains("group-data-[highlighted]/context-menu-sub-trigger:text-foreground-subtle")
        );
    }

    #[test]
    fn variant_strings() {
        assert_eq!(ContextMenuItemVariant::Default.as_str(), "default");
        assert_eq!(ContextMenuItemVariant::Destructive.as_str(), "destructive");
        assert_eq!(
            ContextMenuItemVariant::default(),
            ContextMenuItemVariant::Default
        );
    }

    #[test]
    fn separator_matches_dropdown_separator() {
        assert_eq!(CONTEXT_MENU_SEPARATOR_BASE, "-mx-1 my-1 h-px bg-border");
    }
}
