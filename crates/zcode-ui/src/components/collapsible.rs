//! `Collapsible` 家族 —— Rust 移植自 `packages/ui/src/components/ui/collapsible.tsx`
//!
//! TS 用 Radix `Collapsible.Root/Trigger/Content`，父子之间通过 React Context 共享
//! 展开状态。Rust 侧用 Dioxus 的 `use_context_provider` / `use_context` 复刻同一套
//! 结构（与 [`crate::components::tabs`] 完全一致的做法）。
//!
//! ## 状态归属：同时支持受控与非受控
//!
//! Radix 的 `Root` 接受 `open`（受控）或 `defaultOpen`（非受控）。Rust 侧：
//!
//! - 传 `open: Signal<bool>` → **受控**，状态由调用方持有；
//! - 不传 `open` → **非受控**，组件内部 `use_signal(|| default_open)` 兜底。
//!
//! 两种模式下 context 里存的都是 `Signal<bool>` **句柄**（generational box），
//! 其标识跨渲染稳定，因此不存在「context 读到陈旧值」的问题。
//!
//! ## 与 TS 的行为差异
//!
//! Radix 的 `CollapsibleContent` 走 `Presence`：关闭后**卸载** DOM（退场动画期间
//! 才临时保留）。这里改为**保持挂载 + `hidden` 切换**：
//!
//! - 好处：子树内部状态（输入框内容、滚动位置）在反复折叠时不丢失，
//!   桌面端长驻面板更符合直觉；
//! - 代价：`data-[state=closed]:animate-collapsible-up` 的**退场动画不播放**
//!   （`hidden` 会直接 `display:none`），展开时的淡入仍由内层 div 的
//!   `group-data-[state=open]/collapsible-content:animate-in` 正常触发。
//!
//! 这是刻意的取舍：无渲染器环境下无法验证关键帧动画，而「状态不丢」是可测的。
//!
//! ### `id` 由调用方提供
//!
//! Radix 内部用 `useId()` 生成稳定的 `triggerId` / `contentId` 并自动接上
//! `aria-controls`。Dioxus 0.6 没有等价的「每实例稳定 id」原语（`use_hook` 能给
//! 稳定值，但 Trigger 与 Content 是两个独立组件，拿不到同一个 id），所以这里
//! 把 `id` 做成显式 prop：**给了才发属性**，`aria-controls` 固定拼 `-content`。
//! 不传时两个属性都不出现，不会留下 `id=""` 这种无效 HTML。

use crate::cn::cn;
use dioxus::prelude::*;

/// `CollapsibleContent` 基础类名。
pub const COLLAPSIBLE_CONTENT_BASE: &str = "group/collapsible-content overflow-hidden data-[state=open]:animate-collapsible-down data-[state=closed]:animate-collapsible-up data-[state=closed]:[animation-fill-mode:forwards] transition-none duration-300 ease-in-out";

/// `CollapsibleContent` 内层包裹 div 的类名。
///
/// 注意内层 div 自身没有 `data-state`，所以类名全部读**父层**的 state
/// （`group-data-[state=...]/collapsible-content:`），让透明度动画与高度动画分层。
pub const COLLAPSIBLE_CONTENT_INNER: &str = "transition-none duration-300 ease-in-out group-data-[state=open]/collapsible-content:animate-in group-data-[state=open]/collapsible-content:fade-in-0 group-data-[state=closed]/collapsible-content:animate-out group-data-[state=closed]/collapsible-content:fade-out-0 group-data-[state=closed]/collapsible-content:[animation-fill-mode:forwards]";

pub fn collapsible_content_classes(class: &str) -> String {
    cn(&[COLLAPSIBLE_CONTENT_BASE, class])
}

pub fn collapsible_content_inner_classes(class: &str) -> String {
    cn(&[COLLAPSIBLE_CONTENT_INNER, class])
}

/// 由 `Collapsible` 提供、`CollapsibleTrigger` / `CollapsibleContent` 消费的共享状态。
#[derive(Clone, Copy)]
struct CollapsibleContext {
    open: Signal<bool>,
    disabled: bool,
}

/// 读取当前展开状态（供自定义触发器复用）。
pub fn use_collapsible_open() -> Signal<bool> {
    use_context::<CollapsibleContext>().open
}

/// 折叠面板根容器。
#[component]
pub fn Collapsible(
    /// 受控模式：由调用方持有展开状态。不传则使用内部状态（对应 TS 的 `defaultOpen`）。
    #[props(default)] open: Option<Signal<bool>>,
    /// 非受控模式下的初始展开状态（对应 TS 的 `defaultOpen`）。
    #[props(default)] default_open: bool,
    #[props(default)] disabled: bool,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let internal = use_signal(|| default_open);
    let open = open.unwrap_or(internal);
    use_context_provider(|| CollapsibleContext { open, disabled });

    let state = if open() { "open" } else { "closed" };
    rsx! {
        div {
            class: class,
            "data-slot": "collapsible",
            "data-state": state,
            "data-disabled": disabled.then_some(""),
            {children}
        }
    }
}

/// 折叠触发器（渲染为 `button`，输出 `aria-expanded` / `aria-controls`）。
#[component]
pub fn CollapsibleTrigger(
    #[props(default, into)] class: String,
    /// 对应 Radix 的 `id`；`aria-controls` 会拼上 `-content`。
    #[props(default, into)] id: String,
    children: Element,
) -> Element {
    let mut ctx = use_context::<CollapsibleContext>();
    let expanded = (ctx.open)();
    let state = if expanded { "open" } else { "closed" };
    // Radix 用 `useId()` 自动生成 id；本移植没有等价物，改由调用方显式给。
    // 没给就整个属性不发 —— 发 `id=""` 是无效 HTML，而且会让 `aria-controls` 指向 `-content`。
    let controls = (!id.is_empty()).then(|| format!("{id}-content"));
    let id_attr = (!id.is_empty()).then_some(id);

    rsx! {
        button {
            class: class,
            r#type: "button",
            "data-slot": "collapsible-trigger",
            "data-state": state,
            "data-disabled": ctx.disabled.then_some(""),
            disabled: ctx.disabled,
            id: id_attr,
            "aria-expanded": if expanded { "true" } else { "false" },
            "aria-controls": controls,
            onclick: move |_| {
                if !ctx.disabled {
                    let next = !(ctx.open)();
                    ctx.open.set(next);
                }
            },
            {children}
        }
    }
}

/// 折叠内容区。
#[component]
pub fn CollapsibleContent(
    #[props(default, into)] class: String,
    /// 对应 Radix 的 `id`；不传时由 `CollapsibleTrigger` 的 `id` 派生（这里需显式给）。
    #[props(default, into)] id: String,
    /// 内层包裹 div 的类名（对应 TS 里写死在 `CollapsibleContent` 内部的第二个 `cn()`）。
    #[props(default, into)] inner_class: String,
    children: Element,
) -> Element {
    let ctx = use_context::<CollapsibleContext>();
    let expanded = (ctx.open)();
    let state = if expanded { "open" } else { "closed" };
    let id_attr = (!id.is_empty()).then_some(id);

    rsx! {
        div {
            class: collapsible_content_classes(&class),
            "data-slot": "collapsible-content",
            "data-state": state,
            "data-disabled": ctx.disabled.then_some(""),
            id: id_attr,
            role: "region",
            hidden: !expanded,
            div {
                class: collapsible_content_inner_classes(&inner_class),
                {children}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_base_has_state_variants() {
        let c = collapsible_content_classes("");
        assert!(c.contains("group/collapsible-content"));
        assert!(c.contains("overflow-hidden"));
        assert!(c.contains("data-[state=open]:animate-collapsible-down"));
        assert!(c.contains("data-[state=closed]:animate-collapsible-up"));
        assert!(c.contains("transition-none"));
    }

    #[test]
    fn inner_reads_parent_state_via_group() {
        let c = collapsible_content_inner_classes("");
        assert!(c.contains("group-data-[state=open]/collapsible-content:animate-in"));
        assert!(c.contains("group-data-[state=open]/collapsible-content:fade-in-0"));
        assert!(c.contains("group-data-[state=closed]/collapsible-content:animate-out"));
        assert!(c.contains("group-data-[state=closed]/collapsible-content:fade-out-0"));
    }

    #[test]
    fn user_class_is_appended() {
        let c = collapsible_content_classes("p-2");
        assert!(c.contains("p-2"));
        assert!(c.contains("overflow-hidden"));
    }
}
