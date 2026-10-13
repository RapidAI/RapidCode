//! `Progress` —— Rust 移植自 `packages/ui/src/components/ui/progress.tsx`
//!
//! TS 用 Radix `Progress.Root` + `Progress.Indicator`（Radix 负责 `role="progressbar"`
//! 与 `aria-valuenow` 等无障碍属性）。Rust 侧手写等价的 `div` 结构，**归一化逻辑
//! 与 TS 完全一致**：
//!
//! - `value` 夹取到 `[0, 100]`；
//! - 每段 `percent` 夹取到 `[0, 1]`，并过滤掉 `percent <= 0` 的段；
//! - 存在有效分段时指示条切换为 `flex overflow-hidden bg-transparent` 的分段容器。

use crate::cn::cn;
use dioxus::prelude::*;

/// 一个进度分段。
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ProgressSegment {
    /// 分段唯一 key（对应 TS 的 `key`）。
    pub id: String,
    /// 相对整条进度条的占比，`0.0 ~ 1.0`。
    pub percent: f64,
    /// 追加类名。
    pub class: String,
    /// 追加内联样式。
    pub style: String,
}

impl ProgressSegment {
    pub fn new(id: impl Into<String>, percent: f64) -> Self {
        Self {
            id: id.into(),
            percent,
            class: String::new(),
            style: String::new(),
        }
    }

    pub fn with_class(mut self, class: impl Into<String>) -> Self {
        self.class = class.into();
        self
    }

    pub fn with_style(mut self, style: impl Into<String>) -> Self {
        self.style = style.into();
        self
    }
}

/// 根容器基础类名。
pub const PROGRESS_BASE: &str =
    "relative flex h-1 w-full items-center overflow-x-hidden rounded-md bg-muted";

/// 指示条基础类名。
pub const PROGRESS_INDICATOR_BASE: &str = "h-full rounded-full transition-[width]";

/// 分段模式下指示条的额外类名。
pub const PROGRESS_INDICATOR_SEGMENTED: &str = "flex overflow-hidden bg-transparent";

/// 分段模式下指示条的默认背景。
pub const PROGRESS_INDICATOR_SOLID: &str = "bg-primary";

/// 分段本身的基础类名。
pub const PROGRESS_SEGMENT_BASE: &str = "h-full shrink-0";

/// 把 `value` 归一化到 `[0, 100]`（与 TS `Math.min(Math.max(value ?? 0, 0), 100)` 等价）。
pub fn normalize_value(value: Option<f64>) -> f64 {
    value.unwrap_or(0.0).clamp(0.0, 100.0)
}

/// 归一化分段列表（与 TS 的 `map(...).filter(...)` 等价）。
pub fn normalize_segments(segments: Option<&[ProgressSegment]>) -> Vec<ProgressSegment> {
    segments
        .unwrap_or(&[])
        .iter()
        .map(|s| {
            let mut s = s.clone();
            s.percent = s.percent.clamp(0.0, 1.0);
            s
        })
        .filter(|s| s.percent > 0.0)
        .collect()
}

pub fn progress_classes(class: &str) -> String {
    cn(&[PROGRESS_BASE, class])
}

/// 组合指示条类名（对应 TS 里的 `cn("h-full rounded-full transition-[width]", ...)`）。
pub fn progress_indicator_classes(
    normalized_value: f64,
    has_segments: bool,
    indicator_class: &str,
) -> String {
    cn(&[
        PROGRESS_INDICATOR_BASE,
        if has_segments {
            PROGRESS_INDICATOR_SEGMENTED
        } else {
            PROGRESS_INDICATOR_SOLID
        },
        if normalized_value > 0.0 { indicator_class } else { "" },
    ])
}

/// 百分比 → CSS 宽度字符串。
pub fn width_style(percent_of_100: f64) -> String {
    format!("width: {percent_of_100}%")
}

/// 分段占比 → CSS `flex-basis` 字符串。
pub fn flex_basis_style(percent_of_1: f64) -> String {
    format!("flex-basis: {}%", percent_of_1 * 100.0)
}

/// 组合分段内联样式（等价 TS 的 `{ ...segment.style, flexBasis: ... }`：
/// 自定义样式在前，`flex-basis` 恒在最后覆盖）。
pub fn segment_style(segment: &ProgressSegment) -> String {
    let base = flex_basis_style(segment.percent);
    let custom = segment.style.trim().trim_end_matches(';').trim();
    if custom.is_empty() {
        base
    } else {
        format!("{custom}; {base}")
    }
}

/// 进度条。
#[component]
pub fn Progress(
    #[props(default)] value: Option<f64>,
    #[props(default)] segments: Option<Vec<ProgressSegment>>,
    #[props(default, into)] class: String,
    #[props(default, into)] indicator_class: String,
) -> Element {
    let normalized_value = normalize_value(value);
    let normalized_segments = normalize_segments(segments.as_deref());
    let indicator_cls = progress_indicator_classes(
        normalized_value,
        !normalized_segments.is_empty(),
        &indicator_class,
    );
    let indicator_style = width_style(normalized_value);

    rsx! {
        div {
            class: progress_classes(&class),
            "data-slot": "progress",
            role: "progressbar",
            "aria-valuemin": "0",
            "aria-valuemax": "100",
            "aria-valuenow": format!("{normalized_value}"),
            div {
                class: indicator_cls,
                style: indicator_style,
                "data-slot": "progress-indicator",
                for segment in normalized_segments {
                    span {
                        key: "{segment.id}",
                        class: cn(&[PROGRESS_SEGMENT_BASE, segment.class.as_str()]),
                        style: segment_style(&segment),
                        "aria-hidden": "true",
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_is_clamped() {
        assert_eq!(normalize_value(None), 0.0);
        assert_eq!(normalize_value(Some(-20.0)), 0.0);
        assert_eq!(normalize_value(Some(42.5)), 42.5);
        assert_eq!(normalize_value(Some(180.0)), 100.0);
    }

    #[test]
    fn segments_are_clamped_and_filtered() {
        let segs = vec![
            ProgressSegment::new("a", 0.5),
            ProgressSegment::new("b", -1.0),
            ProgressSegment::new("c", 2.0),
            ProgressSegment::new("d", 0.0),
        ];
        let out = normalize_segments(Some(&segs));
        // b -> 0（被过滤），d -> 0（被过滤），c -> 1.0，a 保持 0.5
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].id, "a");
        assert_eq!(out[0].percent, 0.5);
        assert_eq!(out[1].id, "c");
        assert_eq!(out[1].percent, 1.0);
    }

    #[test]
    fn no_segments_uses_solid_indicator() {
        let c = progress_indicator_classes(30.0, false, "");
        assert!(c.contains("bg-primary"));
        assert!(!c.contains("bg-transparent"));
    }

    #[test]
    fn with_segments_uses_transparent_flex() {
        let c = progress_indicator_classes(30.0, true, "");
        assert!(c.contains("flex"));
        assert!(c.contains("overflow-hidden"));
        assert!(c.contains("bg-transparent"));
        assert!(!c.contains("bg-primary"));
    }

    #[test]
    fn indicator_class_only_when_value_positive() {
        assert!(progress_indicator_classes(0.0, false, "bg-warning").contains("bg-primary"));
        assert!(!progress_indicator_classes(0.0, false, "bg-warning").contains("bg-warning"));
        let c = progress_indicator_classes(10.0, false, "bg-warning");
        assert!(c.contains("bg-warning"));
        assert!(!c.contains("bg-primary"));
    }

    #[test]
    fn styles() {
        assert_eq!(width_style(33.0), "width: 33%");
        assert_eq!(flex_basis_style(0.25), "flex-basis: 25%");
    }

    #[test]
    fn segment_style_composition() {
        let plain = ProgressSegment::new("a", 0.5);
        assert_eq!(segment_style(&plain), "flex-basis: 50%");

        let custom = ProgressSegment::new("b", 0.25).with_style("background: red;");
        assert_eq!(segment_style(&custom), "background: red; flex-basis: 25%");
    }
}
