//! `Separator` —— Rust 移植自 `packages/ui/src/components/ui/separator.tsx`
//!
//! TS 用 Radix `Separator.Root`；Radix 会渲染成 `div[role=separator|none]` 并输出
//! `data-orientation`。Rust 侧直接渲染同样的 `div`，保持 **属性完全一致**，
//! 这样 `data-horizontal:` / `data-vertical:` 这类语义 variant（由 shadcn
//! `tailwind.css` 的 `@custom-variant` 从 `data-orientation` 映射而来）继续生效。

use crate::cn::cn;
use dioxus::prelude::*;

/// 分隔线方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SeparatorOrientation {
    #[default]
    Horizontal,
    Vertical,
}

impl SeparatorOrientation {
    pub fn as_str(&self) -> &'static str {
        match self {
            SeparatorOrientation::Horizontal => "horizontal",
            SeparatorOrientation::Vertical => "vertical",
        }
    }
}

/// 基础类名。
pub const SEPARATOR_BASE: &str = "shrink-0 bg-border data-horizontal:h-px data-horizontal:w-full data-vertical:w-px data-vertical:self-stretch";

/// 组合分隔线最终类名。
pub fn separator_classes(class: &str) -> String {
    cn(&[SEPARATOR_BASE, class])
}

/// 分隔线。
#[component]
pub fn Separator(
    #[props(default)] orientation: SeparatorOrientation,
    /// 对应 Radix 的 `decorative`（默认 `true`，即纯装饰、对无障碍树不可见）。
    #[props(default = true)] decorative: bool,
    #[props(default, into)] class: String,
    /// 覆盖 `data-slot`。TS 里 `<Separator data-slot="button-group-separator" />`
    /// 是在 `{...props}` 中覆盖的（props 展开在 `data-slot="separator"` 之后），
    /// 所以这里也必须可覆盖，否则 `ButtonGroupSeparator` 的锚点会错。
    #[props(default = "separator")] slot: &'static str,
) -> Element {
    rsx! {
        div {
            class: separator_classes(&class),
            "data-slot": slot,
            "data-orientation": orientation.as_str(),
            role: if decorative { "none" } else { "separator" },
            "aria-hidden": decorative.then_some("true"),
            "aria-orientation": (!decorative).then_some(orientation.as_str()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_classes() {
        let c = separator_classes("");
        assert!(c.contains("bg-border"));
        assert!(c.contains("data-horizontal:h-px"));
        assert!(c.contains("data-vertical:w-px"));
    }

    #[test]
    fn orientation_strings() {
        assert_eq!(SeparatorOrientation::Horizontal.as_str(), "horizontal");
        assert_eq!(SeparatorOrientation::Vertical.as_str(), "vertical");
        assert_eq!(SeparatorOrientation::default(), SeparatorOrientation::Horizontal);
    }
}
