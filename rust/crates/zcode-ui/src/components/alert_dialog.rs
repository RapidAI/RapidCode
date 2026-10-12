//! `AlertDialog` 家族 —— Rust 移植自 `packages/ui/src/components/ui/alert-dialog.tsx`
//!
//! 与 [`crate::components::dialog`] 同构（都是 `fixed` 居中模态，因此**不需要
//! Portal 搬运 DOM**），差异只在：
//!
//! - 语义：AlertDialog 是「必须做出选择」的确认框，Radix 因此**不允许**点击遮罩
//!   或 Esc 关闭；
//! - 视觉：`gap-5 rounded-2xl bg-popover/98 p-5 ring-border shadow-2xl`，
//!   标题字号更大（`text-ui-lg font-semibold`）；
//! - 多了 `Action` / `Cancel` 两个语义化按钮（直接用 `buttonVariants` 类名渲染
//!   原生 `button`，与 TS 一致 —— TS 里它们就是 Radix 的 `Action`/`Cancel`
//!   primitive 加 button 类名，不是 `<Button>` 组件）。
//!
//! ## 与 TS 的差异
//!
//! 1. 没有 Portal 节点（`AlertDialogPortal` 是透传容器）。
//! 2. 没有焦点陷阱 / 滚动锁定；Esc 关闭**刻意不实现**（与 Radix 语义一致）。
//! 3. `AlertDialogAction` / `AlertDialogCancel` 点击后会**自动关闭**（Radix 的
//!    Action/Cancel 行为），同时回调调用方传入的 `onclick`。

use crate::cn::cn;
use crate::components::button::{button_classes, ButtonSize, ButtonVariant};
use dioxus::prelude::*;

/// `AlertDialogOverlay` 基础类名（TS 原文，逐字保留）。
pub const ALERT_DIALOG_OVERLAY_BASE: &str = "fixed inset-0 isolate z-50 bg-black/60 duration-100 supports-backdrop-filter:backdrop-blur-xs data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0";

/// `AlertDialogContent` 基础类名（TS 原文，逐字保留）。
pub const ALERT_DIALOG_CONTENT_BASE: &str = "fixed top-1/2 left-1/2 z-50 grid w-full max-w-[calc(100%-2rem)] -translate-x-1/2 -translate-y-1/2 gap-5 rounded-2xl border-none bg-popover/98 p-5 text-ui-base/relaxed text-foreground ring-border shadow-2xl duration-100 outline-none data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95 sm:max-w-md";

/// `AlertDialogHeader` 基础类名。
pub const ALERT_DIALOG_HEADER_BASE: &str = "flex flex-col gap-2 text-left";

/// `AlertDialogFooter` 基础类名。
pub const ALERT_DIALOG_FOOTER_BASE: &str = "flex flex-col-reverse gap-2 sm:flex-row sm:justify-end";

/// `AlertDialogTitle` 基础类名。
pub const ALERT_DIALOG_TITLE_BASE: &str = "text-ui-lg font-semibold text-foreground";

/// `AlertDialogDescription` 基础类名。
pub const ALERT_DIALOG_DESCRIPTION_BASE: &str =
    "whitespace-pre-line pt-0.5 text-ui-base leading-6 text-foreground-subtle";

pub fn alert_dialog_overlay_classes(class: &str) -> String {
    cn(&[ALERT_DIALOG_OVERLAY_BASE, class])
}

pub fn alert_dialog_content_classes(class: &str) -> String {
    cn(&[ALERT_DIALOG_CONTENT_BASE, class])
}

pub fn alert_dialog_header_classes(class: &str) -> String {
    cn(&[ALERT_DIALOG_HEADER_BASE, class])
}

pub fn alert_dialog_footer_classes(class: &str) -> String {
    cn(&[ALERT_DIALOG_FOOTER_BASE, class])
}

pub fn alert_dialog_title_classes(class: &str) -> String {
    cn(&[ALERT_DIALOG_TITLE_BASE, class])
}

pub fn alert_dialog_description_classes(class: &str) -> String {
    cn(&[ALERT_DIALOG_DESCRIPTION_BASE, class])
}

/// 由 `AlertDialog` 提供、其余子组件消费的共享状态。
#[derive(Clone, Copy)]
struct AlertDialogContext {
    open: Signal<bool>,
}

/// 读取当前 alert dialog 的展开状态。
pub fn use_alert_dialog_open() -> Signal<bool> {
    use_context::<AlertDialogContext>().open
}

/// alert dialog 根容器。与 Radix 一致，**不产生任何 DOM**（同 [`crate::components::dialog::Dialog`]）。
#[component]
pub fn AlertDialog(
    #[props(default)] open: Option<Signal<bool>>,
    children: Element,
) -> Element {
    let internal = use_signal(|| false);
    let open = open.unwrap_or(internal);
    use_context_provider(|| AlertDialogContext { open });
    rsx! { {children} }
}

/// alert dialog 触发器（渲染 `button`）。
#[component]
pub fn AlertDialogTrigger(
    #[props(default)] variant: ButtonVariant,
    #[props(default)] size: ButtonSize,
    #[props(default, into)] class: String,
    #[props(default)] disabled: bool,
    #[props(default)] onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    let mut ctx = use_context::<AlertDialogContext>();
    let cls = button_classes(variant, size, &class);

    rsx! {
        button {
            class: cls,
            r#type: "button",
            "data-slot": "alert-dialog-trigger",
            "data-state": if (ctx.open)() { "open" } else { "closed" },
            disabled: disabled,
            onclick: move |e| {
                onclick.call(e);
                if !disabled {
                    ctx.open.set(true);
                }
            },
            {children}
        }
    }
}

/// Radix `Portal` 的等价物：透传容器，不产生 DOM。
#[component]
pub fn AlertDialogPortal(children: Element) -> Element {
    rsx! { {children} }
}

/// 模态遮罩。
#[component]
pub fn AlertDialogOverlay(
    #[props(default, into)] class: String,
) -> Element {
    rsx! {
        div {
            class: alert_dialog_overlay_classes(&class),
            "data-slot": "alert-dialog-overlay",
            "data-state": "open",
        }
    }
}

/// alert dialog 内容容器（自带遮罩）。关闭时整体不渲染（同 `DialogContent`）。
#[component]
pub fn AlertDialogContent(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let ctx = use_context::<AlertDialogContext>();
    if !(ctx.open)() {
        return rsx! {};
    }

    rsx! {
        AlertDialogOverlay {}
        div {
            class: alert_dialog_content_classes(&class),
            "data-slot": "alert-dialog-content",
            "data-state": "open",
            role: "alertdialog",
            "aria-modal": "true",
            tabindex: "-1",
            {children}
        }
    }
}

/// 确认按钮（点击后自动关闭）。
#[component]
pub fn AlertDialogAction(
    #[props(default)] variant: ButtonVariant,
    #[props(default)] size: ButtonSize,
    #[props(default, into)] class: String,
    #[props(default)] disabled: bool,
    #[props(default)] onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    let mut ctx = use_context::<AlertDialogContext>();
    let cls = button_classes(variant, size, &class);

    rsx! {
        button {
            class: cls,
            r#type: "button",
            "data-slot": "alert-dialog-action",
            disabled: disabled,
            onclick: move |e| {
                onclick.call(e);
                if !disabled {
                    ctx.open.set(false);
                }
            },
            {children}
        }
    }
}

/// 取消按钮（默认 `outline`，点击后自动关闭）。
#[component]
pub fn AlertDialogCancel(
    /// 默认 `outline`（对应 TS 的 `variant: variant ?? "outline"`）。
    #[props(default = ButtonVariant::Outline)] variant: ButtonVariant,
    #[props(default)] size: ButtonSize,
    #[props(default, into)] class: String,
    #[props(default)] disabled: bool,
    #[props(default)] onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    let mut ctx = use_context::<AlertDialogContext>();
    let cls = button_classes(variant, size, &class);

    rsx! {
        button {
            class: cls,
            r#type: "button",
            "data-slot": "alert-dialog-cancel",
            disabled: disabled,
            onclick: move |e| {
                onclick.call(e);
                if !disabled {
                    ctx.open.set(false);
                }
            },
            {children}
        }
    }
}

/// alert dialog 头部。
#[component]
pub fn AlertDialogHeader(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: alert_dialog_header_classes(&class),
            "data-slot": "alert-dialog-header",
            {children}
        }
    }
}

/// alert dialog 底部操作区。
#[component]
pub fn AlertDialogFooter(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: alert_dialog_footer_classes(&class),
            "data-slot": "alert-dialog-footer",
            {children}
        }
    }
}

/// alert dialog 标题。
#[component]
pub fn AlertDialogTitle(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        h2 {
            class: alert_dialog_title_classes(&class),
            "data-slot": "alert-dialog-title",
            {children}
        }
    }
}

/// alert dialog 描述。
#[component]
pub fn AlertDialogDescription(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        p {
            class: alert_dialog_description_classes(&class),
            "data-slot": "alert-dialog-description",
            {children}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_matches_dialog_overlay() {
        assert_eq!(
            alert_dialog_overlay_classes(""),
            super::super::dialog::dialog_overlay_classes("")
        );
    }

    #[test]
    fn content_visual_tokens() {
        let c = alert_dialog_content_classes("");
        assert!(c.contains("fixed"));
        assert!(c.contains("rounded-2xl"));
        assert!(c.contains("bg-popover/98"));
        assert!(c.contains("ring-border"));
        assert!(c.contains("shadow-2xl"));
        assert!(c.contains("gap-5"));
        assert!(c.contains("sm:max-w-md"));
    }

    #[test]
    fn content_is_centered() {
        let c = alert_dialog_content_classes("");
        assert!(c.contains("-translate-x-1/2"));
        assert!(c.contains("-translate-y-1/2"));
        assert!(c.contains("top-1/2"));
        assert!(c.contains("left-1/2"));
    }

    #[test]
    fn title_is_larger_than_dialog_title() {
        assert!(alert_dialog_title_classes("").contains("text-ui-lg"));
        assert!(alert_dialog_title_classes("").contains("font-semibold"));
        assert!(super::super::dialog::dialog_title_classes("").contains("text-ui-base"));
    }

    #[test]
    fn description_preserves_newlines() {
        let c = alert_dialog_description_classes("");
        assert!(c.contains("whitespace-pre-line"));
        assert!(c.contains("leading-6"));
    }

    #[test]
    fn cancel_defaults_to_outline() {
        let cls = button_classes(ButtonVariant::Outline, ButtonSize::Default, "");
        assert!(cls.contains("border-border"));
        assert!(cls.contains("hover:bg-input/50"));
    }

    #[test]
    fn action_defaults_to_primary() {
        let cls = button_classes(ButtonVariant::Default, ButtonSize::Default, "");
        assert!(cls.contains("bg-primary"));
        assert!(cls.contains("text-primary-foreground"));
    }
}
