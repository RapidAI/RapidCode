//! `Tooltip` 家族 —— Rust 移植自 `packages/ui/src/components/ui/tooltip.tsx`
//!
//! TS 用 Radix `Tooltip.Provider/Root/Trigger/Content`，`Content` 被包在
//! `Tooltip.Portal` 里并由 floating-ui 计算位置。Rust 侧改用
//! [`crate::components::overlay`] 约定的**纯 CSS 自管理定位**（详见该模块文档）。
//!
//! ## 与 TS 的差异
//!
//! 1. **多一层锚点包裹元素**。[`Tooltip`] 渲染一个 `relative inline-flex` 的
//!    `span` 作为定位包含块（TS 的 Root 不产生 DOM）。这层只影响布局的
//!    「盒模型参与方式」，不改变视觉。
//! 2. **[`TooltipTrigger`] 渲染 `span` 而不是 `button`**。Radix 的 Trigger 默认是
//!    `<button>`，但 shadcn 的用法几乎都是 `asChild` 包一个 `Button`；渲染 `button`
//!    会与内层 `Button` 形成嵌套按钮（非法 DOM）。改用 `span` 后
//!    `<TooltipTrigger><Button …/></TooltipTrigger>` 与 TS 的 `asChild` 用法等价。
//! 3. **不实现 `delayDuration` 的延迟**。Radix 用定时器实现延迟显示；Dioxus 的
//!    hover 事件没有等价的「延迟派发」原语（需要 `use_coroutine` + 取消逻辑）。
//!    这里 hover 立即显示 —— 对 0 延迟的默认值（`delayDuration = 0`）行为一致。
//! 4. `data-state` 用 `open` / `closed` 表达（Radix 还有 `delayed-open` 等中间态，
//!    TS 的类名里 `data-[state=delayed-open]:*` 与 `data-open:*` 两组都写了，
//!    这里统一走 `data-open` / `data-closed` 这一组，视觉结果相同）。

use crate::cn::cn;
use crate::components::overlay::{overlay_anchor_classes, side_align_classes, Align, Side};
use dioxus::prelude::*;

/// `TooltipContent` 基础类名（TS 原文，逐字保留）。
pub const TOOLTIP_CONTENT_BASE: &str = "z-50 inline-flex w-fit max-w-xs origin-(--radix-tooltip-content-transform-origin) items-center gap-1.5 rounded-lg border border-border bg-tooltip px-3 py-1.5 text-ui-sm text-tooltip-foreground has-data-[slot=kbd]:pr-1.5 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 **:data-[slot=kbd]:relative **:data-[slot=kbd]:isolate **:data-[slot=kbd]:z-50 **:data-[slot=kbd]:rounded-sm data-[state=delayed-open]:animate-in data-[state=delayed-open]:fade-in-0 data-[state=delayed-open]:zoom-in-95 data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95";

/// 组合浮层内容最终类名。
pub fn tooltip_content_classes(side: Side, align: Align, side_offset: f64, class: &str) -> String {
    cn(&[
        TOOLTIP_CONTENT_BASE,
        &side_align_classes(side, align, side_offset),
        class,
    ])
}

/// 由 `TooltipProvider` 提供、`Tooltip` 消费的共享配置。
#[derive(Clone, Copy)]
struct TooltipProviderContext {
    /// 保留字段：Radix 用它控制延迟显示，本移植暂不实现（见模块文档差异 3）。
    #[allow(dead_code)]
    delay_duration: u64,
}

/// 由 `Tooltip` 提供、`TooltipTrigger` / `TooltipContent` 消费的共享状态。
#[derive(Clone, Copy)]
struct TooltipContext {
    open: Signal<bool>,
}

/// 读取当前 tooltip 的展开状态。
pub fn use_tooltip_open() -> Signal<bool> {
    use_context::<TooltipContext>().open
}

/// 全局 tooltip 配置（Radix 的 `Tooltip.Provider`）。
///
/// Radix 的 Provider 不渲染 DOM，这里同样只做 context 注入后原样输出 `children`。
#[component]
pub fn TooltipProvider(
    /// 对应 Radix 的 `delayDuration`（默认 `0`）。见模块文档差异 3。
    #[props(default = 0_u64)] delay_duration: u64,
    children: Element,
) -> Element {
    use_context_provider(|| TooltipProviderContext { delay_duration });
    rsx! { {children} }
}

/// 单个 tooltip 的根容器（负责 hover 状态与锚点定位上下文）。
#[component]
pub fn Tooltip(
    /// 受控模式：由调用方持有展开状态。不传则内部管理（对应非受控用法）。
    #[props(default)] open: Option<Signal<bool>>,
    /// 锚点包裹元素的类名。
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    // 让 Provider 的存在被「使用」一次：缺失 Provider 时 Radix 会告警，这里保持同样的
    // 契约（读取一次即可，值本身暂未参与计算）。
    let _provider = try_use_context::<TooltipProviderContext>();

    let internal = use_signal(|| false);
    let open = open.unwrap_or(internal);
    use_context_provider(|| TooltipContext { open });

    let mut hover_open = open;
    rsx! {
        span {
            class: overlay_anchor_classes(&class),
            "data-slot": "tooltip",
            onmouseenter: move |_| hover_open.set(true),
            onmouseleave: move |_| hover_open.set(false),
            onfocusin: move |_| hover_open.set(true),
            onfocusout: move |_| hover_open.set(false),
            {children}
        }
    }
}

/// tooltip 的触发区（渲染 `span`，见模块文档差异 2）。
#[component]
pub fn TooltipTrigger(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        span {
            class: class,
            "data-slot": "tooltip-trigger",
            {children}
        }
    }
}

/// tooltip 浮层内容。
///
/// `side` / `align` / `side_offset` 与 TS 一致地放在 **Content** 上
/// （对应 Radix `Tooltip.Content` 的 props），而不是 Root 上。
#[component]
pub fn TooltipContent(
    /// 浮层方向（对应 Radix `side`，默认 `top`）。
    #[props(default)] side: Side,
    /// 浮层对齐（对应 Radix `align`，默认 `center`）。
    #[props(default)] align: Align,
    /// 与锚点的间距（对应 Radix `sideOffset`，默认 `0`）。
    #[props(default = 0.0)] side_offset: f64,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let ctx = use_context::<TooltipContext>();
    if !(ctx.open)() {
        return rsx! {};
    }

    rsx! {
        div {
            class: tooltip_content_classes(side, align, side_offset, &class),
            "data-slot": "tooltip-content",
            "data-state": "open",
            "data-side": side.as_str(),
            "data-align": align.as_str(),
            role: "tooltip",
            {children}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_has_visual_tokens() {
        let c = tooltip_content_classes(Side::Top, Align::Center, 0.0, "");
        assert!(c.contains("z-50"));
        assert!(c.contains("max-w-xs"));
        assert!(c.contains("rounded-lg"));
        assert!(c.contains("bg-tooltip"));
        assert!(c.contains("text-tooltip-foreground"));
        assert!(c.contains("text-ui-sm"));
    }

    #[test]
    fn base_has_side_and_state_variants() {
        let c = tooltip_content_classes(Side::Top, Align::Center, 0.0, "");
        assert!(c.contains("data-[side=top]:slide-in-from-bottom-2"));
        assert!(c.contains("data-open:animate-in"));
        assert!(c.contains("data-open:zoom-in-95"));
        assert!(c.contains("data-closed:animate-out"));
        assert!(c.contains("data-[state=delayed-open]:fade-in-0"));
    }

    #[test]
    fn base_has_kbd_nesting_rules() {
        let c = tooltip_content_classes(Side::Top, Align::Center, 0.0, "");
        assert!(c.contains("has-data-[slot=kbd]:pr-1.5"));
        assert!(c.contains("**:data-[slot=kbd]:rounded-sm"));
    }

    #[test]
    fn positioning_is_absolute_and_side_aware() {
        let top = tooltip_content_classes(Side::Top, Align::Center, 0.0, "");
        assert!(top.contains("absolute"));
        assert!(top.contains("bottom-full"));
        let right = tooltip_content_classes(Side::Right, Align::Start, 6.0, "");
        assert!(right.contains("left-full"));
        assert!(right.contains("ml-[6px]"));
    }

    #[test]
    fn user_class_wins_over_visual_defaults() {
        let c = tooltip_content_classes(Side::Top, Align::Center, 0.0, "bg-accent rounded-md");
        assert!(c.contains("bg-accent"));
        assert!(!c.contains("bg-tooltip"));
        assert!(c.contains("rounded-md"));
    }
}
