//! `Checkbox` —— Rust 移植自 `packages/ui/src/components/ui/checkbox.tsx`
//!
//! TS 用 Radix `Checkbox.Root` + `Checkbox.Indicator`，图标来自 lucide-react
//! （`Check` / `Minus`）。Rust 侧渲染同样的 `button role="checkbox"`，内联同一份
//! lucide 路径，并显式输出 `data-state="checked|unchecked|indeterminate"`。
//!
//! Radix 的 `Checkbox.Indicator` 只在非 unchecked 时挂载；这里改为**始终挂载**，
//! 由 TS 里原有的 `[&[data-state=indeterminate]_[data-slot=...]]:hidden/block`
//! 类名负责显隐 —— 语义等价，且省掉一次挂载/卸载。

use crate::cn::cn;
use dioxus::prelude::*;

/// 勾选状态（对应 Radix 的 `checked: boolean | "indeterminate"`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CheckboxState {
    #[default]
    Unchecked,
    Checked,
    /// 半选（部分子项被选中）。
    Indeterminate,
}

impl CheckboxState {
    pub fn as_str(&self) -> &'static str {
        match self {
            CheckboxState::Unchecked => "unchecked",
            CheckboxState::Checked => "checked",
            CheckboxState::Indeterminate => "indeterminate",
        }
    }

    /// 对应 `aria-checked` 的取值（半选是 `"mixed"`）。
    pub fn aria_checked(&self) -> &'static str {
        match self {
            CheckboxState::Checked => "true",
            CheckboxState::Indeterminate => "mixed",
            CheckboxState::Unchecked => "false",
        }
    }

    pub fn is_checked(&self) -> bool {
        !matches!(self, CheckboxState::Unchecked)
    }

    /// 点击后的下一个状态：非 checked 一律变 checked，checked 变 unchecked。
    pub fn toggled(&self) -> Self {
        match self {
            CheckboxState::Checked => CheckboxState::Unchecked,
            _ => CheckboxState::Checked,
        }
    }
}

/// 根容器基础类名。
pub const CHECKBOX_BASE: &str = "peer size-4 shrink-0 rounded-sm border border-input-border bg-input text-primary-foreground outline-none transition-colors focus-visible:ring-2 focus-visible:ring-input-border-focused/30 disabled:cursor-not-allowed disabled:opacity-50 data-[state=checked]:border-primary data-[state=checked]:bg-primary data-[state=indeterminate]:border-primary data-[state=indeterminate]:bg-primary";

/// 指示器基础类名。
pub const CHECKBOX_INDICATOR_BASE: &str = "flex items-center justify-center text-current [&[data-state=indeterminate]_[data-slot=checkbox-checked-icon]]:hidden [&[data-state=indeterminate]_[data-slot=checkbox-indeterminate-icon]]:block";

/// 勾号图标基础类名。
pub const CHECKBOX_CHECKED_ICON_BASE: &str = "size-3";

/// 减号图标基础类名（默认隐藏，半选时由指示器规则显示）。
pub const CHECKBOX_INDETERMINATE_ICON_BASE: &str = "hidden size-3";

/// 指定 `checkIconStrokeWidth` 时附加的类名（保持描边不随缩放变粗）。
pub const CHECKBOX_NON_SCALING_STROKE: &str = "[&_path]:[vector-effect:non-scaling-stroke]";

pub fn checkbox_classes(class: &str) -> String {
    cn(&[CHECKBOX_BASE, class])
}

pub fn checkbox_indicator_classes(class: &str) -> String {
    cn(&[CHECKBOX_INDICATOR_BASE, class])
}

/// 组合图标类名（对应 TS `cn("size-3", checkIconStrokeWidth !== undefined && "...")`）。
pub fn checkbox_icon_classes(base: &str, class: &str, stroke_width: Option<f64>) -> String {
    cn(&[
        base,
        class,
        if stroke_width.is_some() {
            CHECKBOX_NON_SCALING_STROKE
        } else {
            ""
        },
    ])
}

/// 复选框（受控组件）。
#[component]
pub fn Checkbox(
    #[props(default)] checked: CheckboxState,
    #[props(default, into)] class: String,
    #[props(default)] disabled: bool,
    /// 对应 TS 的 `checkIconStrokeWidth`。
    #[props(default)] check_icon_stroke_width: Option<f64>,
    /// 对应 TS 的 `onCheckedChange`。
    #[props(default)] on_checked_change: EventHandler<CheckboxState>,
) -> Element {
    let checked_icon = checkbox_icon_classes(
        CHECKBOX_CHECKED_ICON_BASE,
        "",
        check_icon_stroke_width,
    );
    let minus_icon = checkbox_icon_classes(
        CHECKBOX_INDETERMINATE_ICON_BASE,
        "",
        check_icon_stroke_width,
    );

    rsx! {
        button {
            class: checkbox_classes(&class),
            r#type: "button",
            role: "checkbox",
            "data-slot": "checkbox",
            "data-state": checked.as_str(),
            "aria-checked": checked.aria_checked(),
            "data-disabled": disabled.then_some(""),
            disabled: disabled,
            onclick: move |_| {
                if !disabled {
                    on_checked_change.call(checked.toggled());
                }
            },
            span {
                class: checkbox_indicator_classes(""),
                "data-slot": "checkbox-indicator",
                // lucide `check`
                svg {
                    class: checked_icon,
                    "data-slot": "checkbox-checked-icon",
                    view_box: "0 0 24 24",
                    fill: "none",
                    stroke: "currentColor",
                    stroke_width: check_icon_stroke_width,
                    stroke_linecap: "round",
                    stroke_linejoin: "round",
                    path { d: "M20 6 9 17l-5-5" }
                }
                // lucide `minus`
                svg {
                    class: minus_icon,
                    "data-slot": "checkbox-indeterminate-icon",
                    view_box: "0 0 24 24",
                    fill: "none",
                    stroke: "currentColor",
                    stroke_width: check_icon_stroke_width,
                    stroke_linecap: "round",
                    stroke_linejoin: "round",
                    path { d: "M5 12h14" }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_strings() {
        assert_eq!(CheckboxState::Unchecked.as_str(), "unchecked");
        assert_eq!(CheckboxState::Checked.as_str(), "checked");
        assert_eq!(CheckboxState::Indeterminate.as_str(), "indeterminate");
        assert_eq!(CheckboxState::default(), CheckboxState::Unchecked);
    }

    #[test]
    fn aria_checked_mapping() {
        assert_eq!(CheckboxState::Checked.aria_checked(), "true");
        assert_eq!(CheckboxState::Unchecked.aria_checked(), "false");
        assert_eq!(CheckboxState::Indeterminate.aria_checked(), "mixed");
    }

    #[test]
    fn toggling() {
        assert_eq!(CheckboxState::Unchecked.toggled(), CheckboxState::Checked);
        assert_eq!(CheckboxState::Indeterminate.toggled(), CheckboxState::Checked);
        assert_eq!(CheckboxState::Checked.toggled(), CheckboxState::Unchecked);
    }

    #[test]
    fn is_checked() {
        assert!(!CheckboxState::Unchecked.is_checked());
        assert!(CheckboxState::Checked.is_checked());
        assert!(CheckboxState::Indeterminate.is_checked());
    }

    #[test]
    fn base_has_state_variants() {
        let c = checkbox_classes("");
        assert!(c.contains("data-[state=checked]:bg-primary"));
        assert!(c.contains("data-[state=indeterminate]:bg-primary"));
    }

    #[test]
    fn stroke_width_adds_non_scaling_stroke() {
        let with = checkbox_icon_classes(CHECKBOX_CHECKED_ICON_BASE, "", Some(3.0));
        assert!(with.contains("size-3"));
        assert!(with.contains("vector-effect:non-scaling-stroke"));
        let without = checkbox_icon_classes(CHECKBOX_CHECKED_ICON_BASE, "", None);
        assert!(!without.contains("vector-effect:non-scaling-stroke"));
    }

    #[test]
    fn indeterminate_icon_is_hidden_by_default() {
        assert!(checkbox_icon_classes(CHECKBOX_INDETERMINATE_ICON_BASE, "", None).contains("hidden"));
    }
}
