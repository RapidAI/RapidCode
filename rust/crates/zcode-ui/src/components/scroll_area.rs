//! `ScrollArea` 家族 —— Rust 移植自 `packages/ui/src/components/ui/scroll-area.tsx`
//!
//! ## 与 TS 的重要差异（功能可用性取舍）
//!
//! Radix 的 `ScrollArea` 会**接管原生滚动**：把 viewport 设为
//! `overflow: scroll` + 隐藏原生滚动条，再用 JS 测量 scrollHeight 并把
//! `transform` 写到自绘的 Thumb 上。这套机制依赖 Radix 自己注入的 CSS 与
//! 运行时测量，本移植没有对应的运行时，照搬只会得到一个**不会滚动的容器**。
//!
//! 因此这里的做法是：
//!
//! 1. **viewport 直接使用原生滚动**（类名追加 `overflow-auto`）。这是让组件
//!    「能跑」的最小改动 —— 原生滚动在 webview 里完全可用，性能也更好。
//! 2. `ScrollBar` / `ScrollThumb` **仍然导出**且类名逐字保留，供需要自绘滚动条
//!    的调用方复用（配合自己的测量逻辑）；但 [`ScrollArea`] **不再自动渲染**它们
//!    —— 渲染一个不会被驱动的 Thumb 只会得到一条假的灰色竖条。
//! 3. `hide_scrollbar` 用于隐藏原生滚动条（ZCode 里大量使用这种「无滚动条但可滚」
//!    的容器），通过任意 variant 实现，不需要 JS。
//!
//! `rounded-[inherit]` 与 `focus-visible:ring-*` 等其余类名保持与 TS 一致。

use crate::cn::cn;
use dioxus::prelude::*;

/// 滚动条方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ScrollOrientation {
    #[default]
    Vertical,
    Horizontal,
}

impl ScrollOrientation {
    pub fn as_str(&self) -> &'static str {
        match self {
            ScrollOrientation::Vertical => "vertical",
            ScrollOrientation::Horizontal => "horizontal",
        }
    }
}

/// `ScrollArea` 根容器基础类名。
pub const SCROLL_AREA_BASE: &str = "relative";

/// `ScrollArea` viewport 基础类名（TS 原文）。
pub const SCROLL_AREA_VIEWPORT_BASE: &str = "size-full rounded-[inherit] transition-[color,box-shadow] outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-1";

/// viewport 追加类名：让原生滚动生效（见模块文档）。
pub const SCROLL_AREA_VIEWPORT_SCROLLABLE: &str = "overflow-auto";

/// 隐藏原生滚动条的类名（同时覆盖 Firefox 的 `scrollbar-width` 与 WebKit 伪元素）。
pub const SCROLL_AREA_HIDE_SCROLLBAR: &str =
    "[scrollbar-width:none] [&::-webkit-scrollbar]:hidden";

/// `ScrollBar` 基础类名。
pub const SCROLL_AREA_SCROLLBAR_BASE: &str = "flex touch-none select-none transition-colors data-horizontal:h-2 data-horizontal:flex-col data-horizontal:p-[2px] data-vertical:h-full data-vertical:w-2 data-vertical:p-[2px]";

/// `ScrollThumb` 基础类名。
pub const SCROLL_AREA_THUMB_BASE: &str = "relative min-h-2 min-w-2 flex-1 rounded-full bg-border";

pub fn scroll_area_classes(class: &str) -> String {
    cn(&[SCROLL_AREA_BASE, class])
}

pub fn scroll_area_viewport_classes(hide_scrollbar: bool, class: &str) -> String {
    cn(&[
        SCROLL_AREA_VIEWPORT_BASE,
        SCROLL_AREA_VIEWPORT_SCROLLABLE,
        if hide_scrollbar {
            SCROLL_AREA_HIDE_SCROLLBAR
        } else {
            ""
        },
        class,
    ])
}

pub fn scroll_bar_classes(orientation: ScrollOrientation, class: &str) -> String {
    cn(&[
        SCROLL_AREA_SCROLLBAR_BASE,
        // TS 里 ScrollBar 通过 `data-orientation={orientation}` 驱动语义 variant；
        // 这里额外补上显式的方向类名，避免只依赖 attribute 映射。
        match orientation {
            ScrollOrientation::Vertical => "data-vertical:w-2",
            ScrollOrientation::Horizontal => "data-horizontal:h-2",
        },
        class,
    ])
}

pub fn scroll_thumb_classes(class: &str) -> String {
    cn(&[SCROLL_AREA_THUMB_BASE, class])
}

/// 可滚动容器（使用原生滚动，见模块文档）。
#[component]
pub fn ScrollArea(
    /// 隐藏原生滚动条，仅保留滚动能力。
    #[props(default)] hide_scrollbar: bool,
    #[props(default, into)] class: String,
    /// viewport 的类名（对应 TS 里写死在组件内部的 viewport className 之外的补充）。
    #[props(default, into)] viewport_class: String,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: scroll_area_classes(&class),
            "data-slot": "scroll-area",
            div {
                class: scroll_area_viewport_classes(hide_scrollbar, &viewport_class),
                "data-slot": "scroll-area-viewport",
                tabindex: "0",
                {children}
            }
        }
    }
}

/// 自绘滚动条容器（需调用方自行驱动 Thumb 的位移）。
#[component]
pub fn ScrollBar(
    #[props(default)] orientation: ScrollOrientation,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: scroll_bar_classes(orientation, &class),
            "data-slot": "scroll-area-scrollbar",
            "data-orientation": orientation.as_str(),
            {children}
        }
    }
}

/// 自绘滚动条的滑块。
#[component]
pub fn ScrollThumb(
    #[props(default, into)] class: String,
) -> Element {
    rsx! {
        div {
            class: scroll_thumb_classes(&class),
            "data-slot": "scroll-area-thumb",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientation_strings() {
        assert_eq!(ScrollOrientation::Vertical.as_str(), "vertical");
        assert_eq!(ScrollOrientation::Horizontal.as_str(), "horizontal");
        assert_eq!(ScrollOrientation::default(), ScrollOrientation::Vertical);
    }

    #[test]
    fn root_is_relative() {
        assert_eq!(scroll_area_classes(""), "relative");
        assert!(scroll_area_classes("h-full").contains("h-full"));
    }

    #[test]
    fn viewport_is_scrollable() {
        let c = scroll_area_viewport_classes(false, "");
        assert!(c.contains("overflow-auto"));
        assert!(c.contains("size-full"));
        assert!(c.contains("rounded-[inherit]"));
        assert!(c.contains("focus-visible:ring-[3px]"));
    }

    #[test]
    fn hide_scrollbar_adds_arbitrary_variants() {
        let hidden = scroll_area_viewport_classes(true, "");
        assert!(hidden.contains("[scrollbar-width:none]"));
        assert!(hidden.contains("[&::-webkit-scrollbar]:hidden"));
        let shown = scroll_area_viewport_classes(false, "");
        assert!(!shown.contains("[scrollbar-width:none]"));
    }

    #[test]
    fn scrollbar_orientation_rules() {
        let v = scroll_bar_classes(ScrollOrientation::Vertical, "");
        assert!(v.contains("data-vertical:w-2"));
        assert!(v.contains("data-vertical:h-full"));
        let h = scroll_bar_classes(ScrollOrientation::Horizontal, "");
        assert!(h.contains("data-horizontal:h-2"));
        assert!(h.contains("data-horizontal:flex-col"));
    }

    #[test]
    fn thumb_base() {
        let c = scroll_thumb_classes("");
        assert!(c.contains("min-h-2"));
        assert!(c.contains("min-w-2"));
        assert!(c.contains("rounded-full"));
        assert!(c.contains("bg-border"));
    }
}
