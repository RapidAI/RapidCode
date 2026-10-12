//! `Dialog` 家族 —— Rust 移植自 `packages/ui/src/components/ui/dialog.tsx`
//!
//! TS 用 Radix `Dialog.Root/Trigger/Portal/Overlay/Content/Close/Title/Description`，
//! 状态由 Root 持有并通过 Context 下发。
//!
//! ## 为什么这次不需要「自管理定位」
//!
//! Dialog 的 `Overlay` 与 `Content` 用的都是 **`position: fixed`**，其包含块是
//! **视口**而不是最近的定位祖先 —— 只要没有任何祖先元素带 `transform` /
//! `filter` / `backdrop-filter` / `contain`（这些会重新建立包含块），
//! 不搬 DOM 也能得到与 Portal 相同的视觉效果。这比 tooltip/popover 那类
//! `absolute` 浮层幸运得多。
//!
//! 因此本模块**保留 TS 的类名逐字不变**，只做两件事：
//!
//! - 用 [`DialogContext`] 复刻 Root → 子组件的状态下发；
//! - 把 Radix 的 `Portal` 退化为**透传容器**（[`DialogPortal`] 只输出 `children`，
//!   不产生 DOM，因此也没有 `data-slot="dialog-portal"` 节点）。
//!
//! ## 与 TS 的其余差异
//!
//! 1. **没有焦点陷阱 / 滚动锁定 / Esc 关闭**（Radix 的 `DismissableLayer` +
//!    `FocusScope`）。`DialogContent` 保留了 `outline-none`，
//!    但调用方需要自己处理 Esc 与焦点。桌面端模态大多由按钮触发、由按钮关闭，
//!    实际影响有限；接入渲染器后可补。
//! 2. **[`DialogClose`] 直接渲染 `button`**（TS 是 `Close` + `asChild` 包 `Button`）。
//!    Radix 的 Slot 会把 props 合并到 `Button` 上，最终 DOM 就是一个带
//!    Button 类名的 `button` —— 所以这里直接渲染 `button` + `button_classes()`，
//!    DOM 与 TS **完全一致**，而且避免了 Dioxus 里「嵌套 button」的问题。
//! 3. `DialogTrigger` 同理渲染 `button`（Radix 默认就是 `button`）。

use crate::cn::cn;
use crate::components::button::{button_classes, ButtonSize, ButtonVariant};
use crate::icons::IconX;
use dioxus::prelude::*;

/// `DialogOverlay` 基础类名（TS 原文，逐字保留）。
pub const DIALOG_OVERLAY_BASE: &str = "fixed inset-0 isolate z-50 bg-black/60 duration-100 supports-backdrop-filter:backdrop-blur-xs data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0";

/// `DialogContent` 基础类名（TS 原文，逐字保留）。
pub const DIALOG_CONTENT_BASE: &str = "fixed top-1/2 left-1/2 z-50 grid w-full max-w-[calc(100%-2rem)] -translate-x-1/2 -translate-y-1/2 gap-4 rounded-2xl border border-popover-border bg-popover p-4 text-ui-base/relaxed text-foreground shadow-md duration-100 outline-none data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95 [app-region:no-drag]";

/// `DialogHeader` 基础类名。
pub const DIALOG_HEADER_BASE: &str = "flex flex-col gap-1";

/// `DialogFooter` 基础类名。
pub const DIALOG_FOOTER_BASE: &str = "flex flex-col-reverse gap-2 sm:flex-row sm:justify-end";

/// `DialogTitle` 基础类名。
pub const DIALOG_TITLE_BASE: &str = "text-ui-base font-medium text-foreground";

/// `DialogDescription` 基础类名。
pub const DIALOG_DESCRIPTION_BASE: &str = "text-ui-base/relaxed text-foreground-subtle *:[a]:underline *:[a]:underline-offset-3 *:[a]:hover:text-foreground";

/// `DialogContent` 内置关闭按钮的追加类名。
pub const DIALOG_CONTENT_CLOSE_BASE: &str =
    "absolute top-4 right-4 z-20 hover:bg-surface-hover [app-region:no-drag]";

pub fn dialog_overlay_classes(class: &str) -> String {
    cn(&[DIALOG_OVERLAY_BASE, class])
}

pub fn dialog_content_classes(class: &str) -> String {
    cn(&[DIALOG_CONTENT_BASE, class])
}

pub fn dialog_header_classes(class: &str) -> String {
    cn(&[DIALOG_HEADER_BASE, class])
}

pub fn dialog_footer_classes(class: &str) -> String {
    cn(&[DIALOG_FOOTER_BASE, class])
}

pub fn dialog_title_classes(class: &str) -> String {
    cn(&[DIALOG_TITLE_BASE, class])
}

pub fn dialog_description_classes(class: &str) -> String {
    cn(&[DIALOG_DESCRIPTION_BASE, class])
}

/// 由 `Dialog` 提供、其余子组件消费的共享状态。
#[derive(Clone, Copy)]
struct DialogContext {
    open: Signal<bool>,
}

/// 读取当前 dialog 的展开状态。
pub fn use_dialog_open() -> Signal<bool> {
    use_context::<DialogContext>().open
}

/// dialog 根容器（状态持有者）。
///
/// 与 Radix 的 `Dialog.Root` 一致，**不产生任何 DOM**：只做 context 注入后原样
/// 输出 `children`。这一点很重要 —— 如果这里包一层 `div`，在 flex 布局里它就会
/// 变成一个 flex item，把触发器和浮层的位置全带偏。
#[component]
pub fn Dialog(
    /// 受控模式：由调用方持有展开状态。不传则使用内部状态。
    #[props(default)] open: Option<Signal<bool>>,
    children: Element,
) -> Element {
    let internal = use_signal(|| false);
    let open = open.unwrap_or(internal);
    use_context_provider(|| DialogContext { open });
    rsx! { {children} }
}

/// dialog 触发器（渲染 `button`）。
#[component]
pub fn DialogTrigger(
    #[props(default)] variant: ButtonVariant,
    #[props(default)] size: ButtonSize,
    #[props(default, into)] class: String,
    #[props(default)] disabled: bool,
    #[props(default)] onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    let mut ctx = use_context::<DialogContext>();
    let cls = button_classes(variant, size, &class);

    rsx! {
        button {
            class: cls,
            r#type: "button",
            "data-slot": "dialog-trigger",
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

/// Radix `Portal` 的等价物：**透传容器**，不产生 DOM（见模块文档）。
#[component]
pub fn DialogPortal(children: Element) -> Element {
    rsx! { {children} }
}

/// 模态遮罩。
#[component]
pub fn DialogOverlay(
    #[props(default, into)] class: String,
    #[props(default)] onclick: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        div {
            class: dialog_overlay_classes(&class),
            "data-slot": "dialog-overlay",
            "data-state": "open",
            onclick: move |e| onclick.call(e),
        }
    }
}

/// dialog 关闭按钮（渲染 `button` + Button 类名，见模块文档差异 2）。
#[component]
pub fn DialogClose(
    #[props(default)] variant: ButtonVariant,
    #[props(default)] size: ButtonSize,
    #[props(default, into)] class: String,
    #[props(default)] onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    let mut ctx = use_context::<DialogContext>();
    let cls = button_classes(variant, size, &class);

    rsx! {
        button {
            class: cls,
            r#type: "button",
            "data-slot": "dialog-close",
            onclick: move |e| {
                onclick.call(e);
                ctx.open.set(false);
            },
            {children}
        }
    }
}

/// dialog 内容容器（自带遮罩与右上角关闭按钮）。
///
/// 关闭时**整体不渲染** —— 对应 Radix 的 `Portal` + `Presence`（关闭即卸载），
/// 而不是渲染出来再 `hidden`。这样遮罩不会挡住底下的点击，也不需要调用方
/// 自己加 `if` 判断。
#[component]
pub fn DialogContent(
    /// 是否渲染右上角关闭按钮（对应 TS `showCloseButton`，默认 `true`）。
    #[props(default = true)] show_close_button: bool,
    /// 是否渲染遮罩（对应 TS `showOverlay`，默认 `true`）。
    #[props(default = true)] show_overlay: bool,
    #[props(default, into)] class: String,
    #[props(default, into)] overlay_class: String,
    /// 关闭按钮的提示文案（对应 TS 里的 `<span class="sr-only">Close</span>`）。
    #[props(default = "Close".to_string())] close_label: String,
    children: Element,
) -> Element {
    let ctx = use_context::<DialogContext>();
    if !(ctx.open)() {
        return rsx! {};
    }

    rsx! {
        if show_overlay {
            DialogOverlay { class: overlay_class }
        }
        div {
            class: dialog_content_classes(&class),
            "data-slot": "dialog-content",
            "data-state": "open",
            role: "dialog",
            "aria-modal": "true",
            tabindex: "-1",
            {children}
            if show_close_button {
                DialogClose {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::IconSm,
                    class: DIALOG_CONTENT_CLOSE_BASE,
                    IconX {}
                    span { class: "sr-only", {close_label} }
                }
            }
        }
    }
}

/// dialog 头部。
#[component]
pub fn DialogHeader(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: dialog_header_classes(&class),
            "data-slot": "dialog-header",
            {children}
        }
    }
}

/// dialog 底部操作区。
#[component]
pub fn DialogFooter(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: dialog_footer_classes(&class),
            "data-slot": "dialog-footer",
            {children}
        }
    }
}

/// dialog 标题。
#[component]
pub fn DialogTitle(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        h2 {
            class: dialog_title_classes(&class),
            "data-slot": "dialog-title",
            {children}
        }
    }
}

/// dialog 描述。
#[component]
pub fn DialogDescription(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        p {
            class: dialog_description_classes(&class),
            "data-slot": "dialog-description",
            {children}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_covers_viewport() {
        let c = dialog_overlay_classes("");
        assert!(c.contains("fixed"));
        assert!(c.contains("inset-0"));
        assert!(c.contains("z-50"));
        assert!(c.contains("bg-black/60"));
        assert!(c.contains("supports-backdrop-filter:backdrop-blur-xs"));
        assert!(c.contains("data-open:fade-in-0"));
    }

    #[test]
    fn content_is_centered_fixed() {
        let c = dialog_content_classes("");
        assert!(c.contains("fixed"));
        assert!(c.contains("top-1/2"));
        assert!(c.contains("left-1/2"));
        assert!(c.contains("-translate-x-1/2"));
        assert!(c.contains("-translate-y-1/2"));
        assert!(c.contains("rounded-2xl"));
        assert!(c.contains("bg-popover"));
    }

    #[test]
    fn content_keeps_no_drag_region() {
        let c = dialog_content_classes("");
        assert!(c.contains("[app-region:no-drag]"));
    }

    #[test]
    fn content_user_class_overrides_max_width() {
        let c = dialog_content_classes("max-w-lg");
        assert!(c.contains("max-w-lg"));
        assert!(!c.contains("max-w-[calc(100%-2rem)]"));
    }

    #[test]
    fn header_footer_title_description() {
        assert!(dialog_header_classes("").contains("flex flex-col gap-1"));
        assert!(dialog_footer_classes("").contains("sm:flex-row sm:justify-end"));
        assert!(dialog_title_classes("").contains("font-medium"));
        assert!(dialog_description_classes("").contains("text-foreground-subtle"));
        assert!(dialog_description_classes("").contains("*:[a]:underline"));
    }

    #[test]
    fn close_button_uses_icon_sm_ghost() {
        let cls = button_classes(
            ButtonVariant::Ghost,
            ButtonSize::IconSm,
            DIALOG_CONTENT_CLOSE_BASE,
        );
        assert!(cls.contains("size-6"));
        // ghost 的 hover:bg-hover 与追加的 hover:bg-surface-hover 同组（bg-color +
        // hover 修饰符），后者覆盖前者
        assert!(cls.contains("hover:bg-surface-hover"));
        assert!(!cls.contains("hover:bg-hover"));
        assert!(cls.contains("absolute top-4 right-4"));
    }
}
