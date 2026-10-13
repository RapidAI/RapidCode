//! `ButtonGroup` 家族 —— Rust 移植自 `packages/ui/src/components/ui/button-group.tsx`
//!
//! TS 用 `cva` 描述 `orientation`（horizontal / vertical）到类名的映射，
//! 并通过 `[&>*:not(:first-child)]:rounded-l-none` 这类**子选择器**把相邻按钮的
//! 圆角/边框缝合起来。Rust 侧把 cva 换成枚举 + `classes()`，其余类名逐字保留。
//!
//! ## 与 TS 的差异
//!
//! - `ButtonGroupText` 的 `asChild`（Radix `Slot.Root`）**未移植**：Dioxus 没有
//!   「把 props 合并到唯一子元素上」的等价机制（子元素类型在编译期就固定了）。
//!   需要「文本 + 图标」时直接用 [`ButtonGroupText`] 包裹即可，视觉一致。
//! - `ButtonGroupSeparator` 复用 [`crate::components::separator::Separator`]，
//!   通过 `slot` / `class` 覆盖出 `data-slot="button-group-separator"`。

use crate::cn::cn;
use crate::components::separator::{separator_classes, Separator, SeparatorOrientation};
use dioxus::prelude::*;

/// 按钮组方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ButtonGroupOrientation {
    #[default]
    Horizontal,
    Vertical,
}

impl ButtonGroupOrientation {
    pub fn as_str(&self) -> &'static str {
        match self {
            ButtonGroupOrientation::Horizontal => "horizontal",
            ButtonGroupOrientation::Vertical => "vertical",
        }
    }

    /// 对应 `buttonGroupVariants.variants.orientation[orientation]`。
    pub fn classes(&self) -> &'static str {
        match self {
            ButtonGroupOrientation::Horizontal => BUTTON_GROUP_HORIZONTAL,
            ButtonGroupOrientation::Vertical => BUTTON_GROUP_VERTICAL,
        }
    }
}

/// `cva` 的第一个参数（所有方向共用）。
pub const BUTTON_GROUP_BASE: &str = "flex w-fit items-stretch *:focus-visible:relative *:focus-visible:z-10 has-[>[data-slot=button-group]]:gap-2 has-[select[aria-hidden=true]:last-child]:[&>[data-slot=select-trigger]:last-of-type]:rounded-r-md [&>[data-slot=select-trigger]:not([class*='w-'])]:w-fit [&>input]:flex-1";

/// 水平方向的缝合规则。
pub const BUTTON_GROUP_HORIZONTAL: &str = "[&>*:not(:first-child)]:rounded-l-none [&>*:not(:first-child)]:border-l-0 [&>*:not(:last-child)]:rounded-r-none [&>[data-slot]:not(:has(~[data-slot]))]:rounded-r-md!";

/// 垂直方向的缝合规则。
pub const BUTTON_GROUP_VERTICAL: &str = "flex-col [&>*:not(:first-child)]:rounded-t-none [&>*:not(:first-child)]:border-t-0 [&>*:not(:last-child)]:rounded-b-none [&>[data-slot]:not(:has(~[data-slot]))]:rounded-b-md!";

/// `ButtonGroupText` 基础类名。
pub const BUTTON_GROUP_TEXT_BASE: &str = "flex items-center gap-2 rounded-md border bg-muted px-2.5 text-ui-base/relaxed font-medium [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4";

/// `ButtonGroupSeparator` 追加类名。
pub const BUTTON_GROUP_SEPARATOR_BASE: &str = "relative self-stretch bg-input data-horizontal:mx-px data-horizontal:w-auto data-vertical:my-px data-vertical:h-auto";

pub fn button_group_classes(orientation: ButtonGroupOrientation, class: &str) -> String {
    cn(&[BUTTON_GROUP_BASE, orientation.classes(), class])
}

pub fn button_group_text_classes(class: &str) -> String {
    cn(&[BUTTON_GROUP_TEXT_BASE, class])
}

/// 按钮组分隔线的完整类名（在 `Separator` 基类之上叠加）。
pub fn button_group_separator_classes(class: &str) -> String {
    separator_classes(&cn(&[BUTTON_GROUP_SEPARATOR_BASE, class]))
}

/// 把若干按钮缝成一组。
#[component]
pub fn ButtonGroup(
    #[props(default)] orientation: ButtonGroupOrientation,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: button_group_classes(orientation, &class),
            role: "group",
            "data-slot": "button-group",
            "data-orientation": orientation.as_str(),
            {children}
        }
    }
}

/// 按钮组里的文本/图标片段。
#[component]
pub fn ButtonGroupText(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: button_group_text_classes(&class),
            "data-slot": "button-group-text",
            {children}
        }
    }
}

/// 按钮组里的分隔线（默认竖直）。
#[component]
pub fn ButtonGroupSeparator(
    #[props(default = SeparatorOrientation::Vertical)] orientation: SeparatorOrientation,
    #[props(default, into)] class: String,
) -> Element {
    rsx! {
        Separator {
            orientation,
            class: button_group_separator_classes(&class),
            slot: "button-group-separator",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientation_strings() {
        assert_eq!(ButtonGroupOrientation::Horizontal.as_str(), "horizontal");
        assert_eq!(ButtonGroupOrientation::Vertical.as_str(), "vertical");
        assert_eq!(ButtonGroupOrientation::default(), ButtonGroupOrientation::Horizontal);
    }

    #[test]
    fn horizontal_stitches_left_edges() {
        let c = button_group_classes(ButtonGroupOrientation::Horizontal, "");
        assert!(c.contains("[&>*:not(:first-child)]:rounded-l-none"));
        assert!(c.contains("[&>*:not(:first-child)]:border-l-0"));
        assert!(c.contains("[&>*:not(:last-child)]:rounded-r-none"));
        assert!(!c.contains("flex-col"));
    }

    #[test]
    fn vertical_adds_flex_col() {
        let c = button_group_classes(ButtonGroupOrientation::Vertical, "");
        assert!(c.contains("flex-col"));
        assert!(c.contains("[&>*:not(:first-child)]:rounded-t-none"));
        assert!(c.contains("[&>*:not(:last-child)]:rounded-b-none"));
    }

    #[test]
    fn text_base_has_svg_sizing() {
        let c = button_group_text_classes("");
        assert!(c.contains("bg-muted"));
        assert!(c.contains("[&_svg]:pointer-events-none"));
        assert!(c.contains("[&_svg:not([class*='size-'])]:size-4"));
    }

    #[test]
    fn separator_overrides_bg_and_orientation_rules() {
        let c = button_group_separator_classes("");
        assert!(c.contains("bg-input"));
        // tailwind-merge：后写的 bg-input 覆盖 Separator 基类的 bg-border
        assert!(!c.contains("bg-border"));
        assert!(c.contains("data-horizontal:mx-px"));
        assert!(c.contains("data-vertical:my-px"));
        // Separator 基类仍在
        assert!(c.contains("data-horizontal:h-px"));
    }
}
