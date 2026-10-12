//! `Switch` —— Rust 移植自 `packages/ui/src/components/ui/switch.tsx`
//!
//! TS 用 Radix `Switch.Root/Thumb`。Radix 输出 `<button role="switch" data-state>`，
//! Rust 侧渲染同样的 `button`，并把 Radix 的 `data-state="checked"|"unchecked"`
//! 显式写出来，使 `data-checked:` / `data-unchecked:` 语义 variant 继续生效。

use crate::cn::cn;
use dioxus::prelude::*;

/// 开关尺寸。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SwitchSize {
    Sm,
    #[default]
    Default,
}

impl SwitchSize {
    pub fn as_str(&self) -> &'static str {
        match self {
            SwitchSize::Sm => "sm",
            SwitchSize::Default => "default",
        }
    }
}

/// 根容器基础类名。
pub const SWITCH_BASE: &str = "p-px peer group/switch relative inline-flex shrink-0 items-center rounded-full transition-all outline-none after:absolute after:-inset-x-3 after:-inset-y-2 focus-visible:ring-2 focus-visible:ring-input-border-focused/30 aria-invalid:ring-2 aria-invalid:ring-destructive/20 data-[size=default]:h-[18px] data-[size=default]:w-[32px] data-[size=sm]:h-[16px] data-[size=sm]:w-[28px] dark:aria-invalid:ring-destructive/40 data-checked:bg-primary data-unchecked:bg-primary/30 data-disabled:cursor-not-allowed data-disabled:opacity-50";

/// 滑块基础类名。
pub const SWITCH_THUMB_BASE: &str = "pointer-events-none block rounded-full bg-primary-foreground ring-0 transition-transform group-data-[size=default]/switch:size-4 group-data-[size=sm]/switch:size-3.5 group-data-[size=default]/switch:data-checked:translate-x-[calc(100%-2px)] group-data-[size=sm]/switch:data-checked:translate-x-[calc(100%-2px)] group-data-[size=default]/switch:data-unchecked:translate-x-0 group-data-[size=sm]/switch:data-unchecked:translate-x-0";

pub fn switch_classes(class: &str) -> String {
    cn(&[SWITCH_BASE, class])
}

pub fn switch_thumb_classes(class: &str) -> String {
    cn(&[SWITCH_THUMB_BASE, class])
}

/// 开关（受控组件：`checked` + `on_checked_change`）。
#[component]
pub fn Switch(
    #[props(default)] checked: bool,
    #[props(default)] size: SwitchSize,
    #[props(default, into)] class: String,
    #[props(default)] disabled: bool,
    /// 对应 TS 的 `onCheckedChange`。
    #[props(default)] on_checked_change: EventHandler<bool>,
    #[props(default, into)] thumb_class: String,
) -> Element {
    let state = if checked { "checked" } else { "unchecked" };
    rsx! {
        button {
            class: switch_classes(&class),
            r#type: "button",
            role: "switch",
            "data-slot": "switch",
            "data-size": size.as_str(),
            "data-state": state,
            "aria-checked": if checked { "true" } else { "false" },
            "data-disabled": disabled.then_some(""),
            disabled: disabled,
            onclick: move |_| {
                if !disabled {
                    on_checked_change.call(!checked);
                }
            },
            span {
                class: switch_thumb_classes(&thumb_class),
                "data-slot": "switch-thumb",
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_size_strings() {
        assert_eq!(SwitchSize::Sm.as_str(), "sm");
        assert_eq!(SwitchSize::Default.as_str(), "default");
        assert_eq!(SwitchSize::default(), SwitchSize::Default);
    }

    #[test]
    fn base_has_state_and_size_variants() {
        let c = switch_classes("");
        assert!(c.contains("data-checked:bg-primary"));
        assert!(c.contains("data-unchecked:bg-primary/30"));
        assert!(c.contains("data-[size=default]:h-[18px]"));
        assert!(c.contains("data-[size=sm]:w-[28px]"));
    }

    #[test]
    fn thumb_has_translate_variants() {
        let c = switch_thumb_classes("");
        assert!(c.contains("group-data-[size=default]/switch:size-4"));
        assert!(c.contains("translate-x-[calc(100%-2px)]"));
    }

    #[test]
    fn user_class_overrides_bg() {
        let c = switch_classes("data-checked:bg-accent");
        assert!(c.contains("data-checked:bg-accent"));
        assert!(!c.contains("data-checked:bg-primary"));
    }
}
