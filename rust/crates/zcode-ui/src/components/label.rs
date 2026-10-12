//! `Label` —— Rust 移植自 `packages/ui/src/components/ui/label.tsx`
//!
//! TS 直接渲染原生 `<label>`（没有 Radix 原语），Rust 侧一一对应。

use crate::cn::cn;
use dioxus::prelude::*;

/// 基础类名。
pub const LABEL_BASE: &str =
    "text-ui-base font-medium leading-none peer-disabled:cursor-not-allowed peer-disabled:opacity-70";

/// 组合标签最终类名。
pub fn label_classes(class: &str) -> String {
    cn(&[LABEL_BASE, class])
}

/// 表单标签。
#[component]
pub fn Label(
    #[props(default, into)] class: String,
    /// 对应 TS 透传的 `htmlFor`。
    #[props(default)] html_for: Option<String>,
    #[props(default)] disabled: bool,
    #[props(default)] onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    rsx! {
        label {
            class: label_classes(&class),
            "data-slot": "label",
            "for": html_for,
            "aria-disabled": disabled.then_some("true"),
            onclick: move |e| onclick.call(e),
            {children}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_label_classes() {
        let c = label_classes("");
        assert!(c.contains("text-ui-base"));
        assert!(c.contains("font-medium"));
        assert!(c.contains("leading-none"));
    }

    #[test]
    fn user_class_is_appended() {
        let c = label_classes("text-foreground");
        assert!(c.contains("text-foreground"));
        // text-ui-base 是字号、text-foreground 是颜色，两者不应互相覆盖
        assert!(c.contains("text-ui-base"));
    }
}
