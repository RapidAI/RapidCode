//! `Popover` 家族 —— Rust 移植自 `packages/ui/src/components/ui/popover.tsx`
//!
//! TS 用 Radix `Popover.Root/Trigger/Anchor/Content`，`Content` 包在
//! `Popover.Portal` 里。Rust 侧改用 [`crate::components::overlay`] 约定的
//! 纯 CSS 自管理定位。
//!
//! ## 与 TS 的差异
//!
//! 1. **没有 Portal 层**（同 [`crate::components::hover_card`]）。
//! 2. **[`Popover`] 多渲染一层锚点包裹元素**；**[`PopoverTrigger`] 渲染 `span`**
//!    而非 Radix 默认的 `<button>`（避免与内层 `Button` 形成嵌套按钮）。
//! 3. **点击外部关闭**（Radix 的 `DismissableLayer`）**未实现**：需要全局
//!    pointerdown 监听 + 命中判断。这里改为**点击触发器切换** + 显式传入的
//!    `on_open_change`；调用方可在自己的遮罩层上处理「点外部关闭」。
//!    `PopoverAnchor` 因此只作为语义占位（`span`），不参与定位计算。
//! 4. 默认 `side` 为 `bottom`、`align` 为 `center`、`sideOffset` 为 `4`，与 Radix 一致。

use crate::cn::cn;
use crate::components::overlay::{overlay_anchor_classes, side_align_classes, Align, Side};
use dioxus::prelude::*;

/// `PopoverContent` 基础类名（TS 原文，逐字保留）。
pub const POPOVER_CONTENT_BASE: &str = "z-[60] flex w-72 origin-(--radix-popover-content-transform-origin) flex-col gap-4 rounded-xl bg-popover p-2.5 text-ui-base text-popover-foreground shadow-md border border-border overflow-hidden outline-hidden duration-100 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95";

/// `PopoverHeader` 基础类名。
pub const POPOVER_HEADER_BASE: &str = "flex flex-col gap-1 text-ui-base";

/// `PopoverTitle` 基础类名。
pub const POPOVER_TITLE_BASE: &str = "text-ui-base font-medium";

/// `PopoverDescription` 基础类名。
pub const POPOVER_DESCRIPTION_BASE: &str = "text-muted-foreground";

pub fn popover_content_classes(
    side: Side,
    align: Align,
    side_offset: f64,
    class: &str,
) -> String {
    cn(&[
        POPOVER_CONTENT_BASE,
        &side_align_classes(side, align, side_offset),
        class,
    ])
}

pub fn popover_header_classes(class: &str) -> String {
    cn(&[POPOVER_HEADER_BASE, class])
}

pub fn popover_title_classes(class: &str) -> String {
    cn(&[POPOVER_TITLE_BASE, class])
}

pub fn popover_description_classes(class: &str) -> String {
    cn(&[POPOVER_DESCRIPTION_BASE, class])
}

/// 由 `Popover` 提供、`PopoverContent` 消费的共享状态。
#[derive(Clone, Copy)]
struct PopoverContext {
    open: Signal<bool>,
}

/// 读取当前 popover 的展开状态。
pub fn use_popover_open() -> Signal<bool> {
    use_context::<PopoverContext>().open
}

/// popover 根容器。
#[component]
pub fn Popover(
    #[props(default)] open: Option<Signal<bool>>,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let internal = use_signal(|| false);
    let open = open.unwrap_or(internal);
    use_context_provider(|| PopoverContext { open });

    rsx! {
        span {
            class: overlay_anchor_classes(&class),
            "data-slot": "popover",
            {children}
        }
    }
}

/// popover 的触发区（渲染 `span`，点击切换展开）。
#[component]
pub fn PopoverTrigger(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let mut ctx = use_context::<PopoverContext>();
    rsx! {
        span {
            class: class,
            "data-slot": "popover-trigger",
            "data-state": if (ctx.open)() { "open" } else { "closed" },
            onclick: move |_| {
                let next = !(ctx.open)();
                ctx.open.set(next);
            },
            {children}
        }
    }
}

/// popover 的锚点占位（Radix 用 `Anchor` 让浮层对齐到非触发元素）。
///
/// 本移植不区分锚点与触发点（都相对同一层包裹元素定位），因此这里只保留
/// `data-slot` 语义，见模块文档差异 3。
#[component]
pub fn PopoverAnchor(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        span {
            class: class,
            "data-slot": "popover-anchor",
            {children}
        }
    }
}

/// popover 浮层内容。`side` / `align` / `side_offset` 放在 Content 上（同 TS）。
#[component]
pub fn PopoverContent(
    /// 对应 Radix `side`，默认 `bottom`。
    #[props(default = Side::Bottom)] side: Side,
    #[props(default)] align: Align,
    /// 对应 Radix `sideOffset`，默认 `4`（与 TS 一致）。
    #[props(default = 4.0)] side_offset: f64,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let ctx = use_context::<PopoverContext>();
    if !(ctx.open)() {
        return rsx! {};
    }

    rsx! {
        div {
            class: popover_content_classes(side, align, side_offset, &class),
            "data-slot": "popover-content",
            "data-state": "open",
            "data-side": side.as_str(),
            "data-align": align.as_str(),
            {children}
        }
    }
}

/// popover 头部（标题 + 描述）。
#[component]
pub fn PopoverHeader(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: popover_header_classes(&class),
            "data-slot": "popover-header",
            {children}
        }
    }
}

/// popover 标题。
#[component]
pub fn PopoverTitle(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: popover_title_classes(&class),
            "data-slot": "popover-title",
            {children}
        }
    }
}

/// popover 描述。
#[component]
pub fn PopoverDescription(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        p {
            class: popover_description_classes(&class),
            "data-slot": "popover-description",
            {children}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_uses_z60_to_beat_tooltip() {
        let c = popover_content_classes(Side::Bottom, Align::Center, 4.0, "");
        assert!(c.contains("z-[60]"));
    }

    #[test]
    fn content_visual_tokens() {
        let c = popover_content_classes(Side::Bottom, Align::Center, 4.0, "");
        assert!(c.contains("w-72"));
        assert!(c.contains("rounded-xl"));
        assert!(c.contains("bg-popover"));
        assert!(c.contains("gap-4"));
        assert!(c.contains("border-border"));
        assert!(c.contains("overflow-hidden"));
    }

    #[test]
    fn content_positioning() {
        let c = popover_content_classes(Side::Bottom, Align::Center, 4.0, "");
        assert!(c.contains("absolute"));
        assert!(c.contains("top-full"));
        assert!(c.contains("left-1/2"));
        assert!(c.contains("-translate-x-1/2"));
        assert!(c.contains("mt-[4px]"));
    }

    #[test]
    fn header_title_description() {
        assert!(popover_header_classes("").contains("flex flex-col gap-1"));
        assert!(popover_title_classes("").contains("font-medium"));
        assert!(popover_description_classes("").contains("text-muted-foreground"));
    }

    #[test]
    fn user_class_overrides_width() {
        let c = popover_content_classes(Side::Bottom, Align::Center, 4.0, "w-[420px]");
        assert!(c.contains("w-[420px]"));
        assert!(!c.contains("w-72"));
    }
}
