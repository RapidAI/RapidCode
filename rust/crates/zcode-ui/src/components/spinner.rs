//! `Spinner` —— Rust 移植自 `packages/ui/src/components/ui/spinner.tsx`
//!
//! TS 用 lucide-react 的 `LoaderIcon`。Rust 侧没有图标库依赖，直接内联同一份
//! lucide `loader` 路径数据，视觉与 TS 完全一致。
//!
//! 差异：TS 通过 `useZCodeIntl()` 取 `common.loading` 文案；本 crate 不引入 i18n
//! 依赖，改为 `label` prop（默认 `"Loading"`），由调用方传入本地化文案。

use crate::cn::cn;
use dioxus::prelude::*;

/// 基础类名。
pub const SPINNER_BASE: &str = "size-4 animate-spin";

pub fn spinner_classes(class: &str) -> String {
    cn(&[SPINNER_BASE, class])
}

/// 加载中指示器（内联 lucide `loader` 图标）。
#[component]
pub fn Spinner(
    #[props(default, into)] class: String,
    /// 对应 TS 的 `aria-label={intl.formatMessage({ id: "common.loading" })}`。
    #[props(default = "Loading".to_string())] label: String,
) -> Element {
    rsx! {
        svg {
            class: spinner_classes(&class),
            "data-slot": "spinner",
            role: "status",
            // svg 元素不在 dioxus-html 的 aria 属性组里，用自定义属性名透传
            "aria-label": label,
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            line { x1: "12", x2: "12", y1: "2", y2: "6" }
            line { x1: "12", x2: "12", y1: "18", y2: "22" }
            line { x1: "4.93", x2: "7.76", y1: "4.93", y2: "7.76" }
            line { x1: "16.24", x2: "19.07", y1: "16.24", y2: "19.07" }
            line { x1: "2", x2: "6", y1: "12", y2: "12" }
            line { x1: "18", x2: "22", y1: "12", y2: "12" }
            line { x1: "4.93", x2: "7.76", y1: "19.07", y2: "16.24" }
            line { x1: "16.24", x2: "19.07", y1: "7.76", y2: "4.93" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_classes() {
        let c = spinner_classes("");
        assert!(c.contains("size-4"));
        assert!(c.contains("animate-spin"));
    }

    #[test]
    fn size_override() {
        let c = spinner_classes("size-6");
        assert!(c.contains("size-6"));
        assert!(!c.contains("size-4"));
    }
}
