//! `HoverCard` 家族 —— Rust 移植自 `packages/ui/src/components/ui/hover-card.tsx`
//!
//! TS 用 Radix `HoverCard.Root/Trigger/Content`，`Content` 包在
//! `HoverCard.Portal`（带 `data-slot="hover-card-portal"`）里。Rust 侧改用
//! [`crate::components::overlay`] 约定的纯 CSS 自管理定位。
//!
//! ## 与 TS 的差异
//!
//! 1. **没有 Portal 层**，因此不存在 `data-slot="hover-card-portal"` 这个节点。
//!    浮层直接渲染在锚点包裹元素内（`absolute` 定位）。
//! 2. **[`HoverCard`] 多渲染一层锚点包裹元素**（TS 的 Root 不产生 DOM）。
//! 3. **[`HoverCardTrigger`] 渲染 `span`**，与 `asChild` 用法等价（同
//!    [`crate::components::tooltip`] 的取舍）。
//! 4. **`openDelay` / `closeDelay` 未实现**：Radix 默认 700ms / 300ms 延迟，
//!    需要定时器原语；这里 hover 立即开、leave 立即关。
//! 5. 默认 `side` 与 Radix 一致为 `bottom`（TS 未显式传 `side`）。

use crate::cn::cn;
use crate::components::overlay::{overlay_anchor_classes, side_align_classes, Align, Side};
use dioxus::prelude::*;

/// `HoverCardContent` 基础类名（TS 原文，逐字保留）。
pub const HOVER_CARD_CONTENT_BASE: &str = "z-50 w-72 origin-(--radix-hover-card-content-transform-origin) rounded-lg bg-popover p-2.5 text-ui-base/relaxed text-popover-foreground shadow-md ring-1 ring-foreground/10 outline-hidden duration-100 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95";

/// 组合浮层内容最终类名。
pub fn hover_card_content_classes(
    side: Side,
    align: Align,
    side_offset: f64,
    class: &str,
) -> String {
    cn(&[
        HOVER_CARD_CONTENT_BASE,
        &side_align_classes(side, align, side_offset),
        class,
    ])
}

/// 由 `HoverCard` 提供、`HoverCardContent` 消费的共享状态。
#[derive(Clone, Copy)]
struct HoverCardContext {
    open: Signal<bool>,
}

/// 读取当前 hover card 的展开状态。
pub fn use_hover_card_open() -> Signal<bool> {
    use_context::<HoverCardContext>().open
}

/// hover card 根容器。
#[component]
pub fn HoverCard(
    #[props(default)] open: Option<Signal<bool>>,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let internal = use_signal(|| false);
    let open = open.unwrap_or(internal);
    use_context_provider(|| HoverCardContext { open });

    let mut hover_open = open;
    rsx! {
        span {
            class: overlay_anchor_classes(&class),
            "data-slot": "hover-card",
            onmouseenter: move |_| hover_open.set(true),
            onmouseleave: move |_| hover_open.set(false),
            onfocusin: move |_| hover_open.set(true),
            onfocusout: move |_| hover_open.set(false),
            {children}
        }
    }
}

/// hover card 的触发区（渲染 `span`）。
#[component]
pub fn HoverCardTrigger(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        span {
            class: class,
            "data-slot": "hover-card-trigger",
            {children}
        }
    }
}

/// hover card 浮层内容。`side` / `align` / `side_offset` 放在 Content 上（同 TS）。
#[component]
pub fn HoverCardContent(
    /// 对应 Radix `side`，默认 `bottom`（与 TS 未显式传值时的 Radix 默认一致）。
    #[props(default = Side::Bottom)] side: Side,
    #[props(default)] align: Align,
    /// 对应 Radix `sideOffset`，默认 `4`（与 TS 一致）。
    #[props(default = 4.0)] side_offset: f64,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let ctx = use_context::<HoverCardContext>();
    if !(ctx.open)() {
        return rsx! {};
    }

    rsx! {
        div {
            class: hover_card_content_classes(side, align, side_offset, &class),
            "data-slot": "hover-card-content",
            "data-state": "open",
            "data-side": side.as_str(),
            "data-align": align.as_str(),
            {children}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_visual_tokens() {
        let c = hover_card_content_classes(Side::Bottom, Align::Center, 4.0, "");
        assert!(c.contains("z-50"));
        assert!(c.contains("w-72"));
        assert!(c.contains("rounded-lg"));
        assert!(c.contains("bg-popover"));
        assert!(c.contains("text-popover-foreground"));
        assert!(c.contains("shadow-md"));
        assert!(c.contains("ring-1"));
        assert!(c.contains("ring-foreground/10"));
        assert!(c.contains("duration-100"));
    }

    #[test]
    fn base_state_variants() {
        let c = hover_card_content_classes(Side::Bottom, Align::Center, 4.0, "");
        assert!(c.contains("data-[side=bottom]:slide-in-from-top-2"));
        assert!(c.contains("data-open:animate-in"));
        assert!(c.contains("data-open:zoom-in-95"));
        assert!(c.contains("data-closed:animate-out"));
        assert!(c.contains("data-closed:zoom-out-95"));
    }

    #[test]
    fn default_side_is_bottom_with_4px_offset() {
        let c = hover_card_content_classes(Side::Bottom, Align::Center, 4.0, "");
        assert!(c.contains("absolute"));
        assert!(c.contains("top-full"));
        assert!(c.contains("mt-[4px]"));
    }

    #[test]
    fn user_class_overrides_width() {
        let c = hover_card_content_classes(Side::Bottom, Align::Center, 4.0, "w-96");
        assert!(c.contains("w-96"));
        assert!(!c.contains("w-72"));
    }
}
