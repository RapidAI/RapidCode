//! `ScrollFadeViewport` —— Rust 移植自
//! `packages/ui/src/components/ui/scroll-fade-viewport.tsx`。
//!
//! TS 版本在滚动容器上挂 `scroll` / `ResizeObserver` 监听，按「上下是否还有
//! 被遮住的内容」切换一组 `mask-image` 类名，做出「内容滚出视口时渐隐」的效果。
//!
//! ## 与 TS 的差异：测量与渲染分离
//!
//! TS 直接读 `viewport.scrollTop` / `scrollHeight` / `clientHeight` 三个 DOM 属性。
//! Dioxus 0.6.3 的 `ScrollEvent` 是**空载荷**（`ScrollData` 没有任何访问器，
//! 见 `dioxus-html/src/events/scroll.rs`），拿不到这些数字；`ResizeObserver`
//! 更是浏览器专有。所以这里把职责切开：
//!
//! - **判定逻辑**做成纯函数 [`scroll_mask_state`] —— 逐字复刻 TS 里的阈值
//!   （`maxScrollTop <= 1`、`scrollTop > 1`、`scrollTop < maxScrollTop - 1`），
//!   可单测、与渲染器无关；
//! - **组件**接受受控的 `mask_state` prop，只负责把状态映射成类名与
//!   `data-scroll-mask` 属性；
//! - 谁来读 DOM 数字由调用方决定（在桌面/Web 上接 `onscroll` + 自己的测量）。
//!
//! 这样做的代价是「开箱即用」性下降，收益是这块逻辑在无渲染器环境下**可验证** ——
//! 否则它只能靠肉眼在浏览器里试。

use crate::cn::cn;
use dioxus::prelude::*;

/// 滚动遮罩的四种状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ScrollMaskState {
    /// 内容没超出，或恰好停在两端 —— 不需要遮罩。
    #[default]
    None,
    /// 上方有被遮住的内容。
    Top,
    /// 下方有被遮住的内容。
    Bottom,
    /// 上下都有。
    Both,
}

impl ScrollMaskState {
    /// 写进 `data-scroll-mask` 的取值。
    pub fn as_str(&self) -> &'static str {
        match self {
            ScrollMaskState::None => "none",
            ScrollMaskState::Top => "top",
            ScrollMaskState::Bottom => "bottom",
            ScrollMaskState::Both => "both",
        }
    }

    /// 对应的 `mask-image` 类名（TS 原文，逐字保留）。
    pub fn classes(&self) -> &'static str {
        match self {
            ScrollMaskState::None => "",
            ScrollMaskState::Top => {
                "[mask-image:linear-gradient(to_bottom,transparent_0,black_24px,black_100%)] \
                 [-webkit-mask-image:linear-gradient(to_bottom,transparent_0,black_24px,black_100%)]"
            }
            // Tailwind arbitrary value 里用下划线表示 calc 的空格，
            // 避免生成无效的 `calc(100%-24px)`。
            ScrollMaskState::Bottom => {
                "[mask-image:linear-gradient(to_bottom,black_0,black_calc(100%_-_24px),transparent_100%)] \
                 [-webkit-mask-image:linear-gradient(to_bottom,black_0,black_calc(100%_-_24px),transparent_100%)]"
            }
            ScrollMaskState::Both => {
                "[mask-image:linear-gradient(to_bottom,transparent_0,black_24px,black_calc(100%_-_24px),transparent_100%)] \
                 [-webkit-mask-image:linear-gradient(to_bottom,transparent_0,black_24px,black_calc(100%_-_24px),transparent_100%)]"
            }
        }
    }
}

/// 视口基础类名（TS 原文）。
pub const SCROLL_FADE_VIEWPORT_BASE: &str = "min-h-0 flex-1 overflow-y-auto";

/// 判定当前该用哪种遮罩 —— TS `updateMaskState` 的纯函数化。
///
/// 三个入参就是 TS 读的那三个 DOM 属性：
/// `scroll_top` = `scrollTop`、`scroll_height` = `scrollHeight`、
/// `client_height` = `clientHeight`。
///
/// 用 `f64` 而不是整数：`scrollTop` 在缩放/高 DPI 下会是小数，
/// TS 侧的 `> 1` / `< max - 1` 阈值本身就是为亚像素抖动留的余量。
pub fn scroll_mask_state(scroll_top: f64, scroll_height: f64, client_height: f64) -> ScrollMaskState {
    let max_scroll_top = scroll_height - client_height;
    if max_scroll_top <= 1.0 {
        return ScrollMaskState::None;
    }

    let has_hidden_top = scroll_top > 1.0;
    let has_hidden_bottom = scroll_top < max_scroll_top - 1.0;

    match (has_hidden_top, has_hidden_bottom) {
        (true, true) => ScrollMaskState::Both,
        (true, false) => ScrollMaskState::Top,
        (false, true) => ScrollMaskState::Bottom,
        (false, false) => ScrollMaskState::None,
    }
}

/// 拼装视口类名：基础 + 遮罩 + 调用方覆盖（与 TS 的 `cn()` 参数顺序一致）。
pub fn scroll_fade_viewport_classes(state: ScrollMaskState, class: &str) -> String {
    cn(&[SCROLL_FADE_VIEWPORT_BASE, state.classes(), class])
}

/// 渐隐滚动视口。
///
/// `mask_state` 不传时按 [`ScrollMaskState::None`] 渲染（即不加遮罩）。
#[component]
pub fn ScrollFadeViewport(
    /// 受控的遮罩状态。由调用方测量后传入，见模块文档。
    #[props(default)] mask_state: Option<ScrollMaskState>,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let state = mask_state.unwrap_or_default();
    rsx! {
        div {
            class: scroll_fade_viewport_classes(state, &class),
            "data-scroll-mask": state.as_str(),
            // 内层包裹 div：TS 里它只是 `ResizeObserver` 的观察目标，
            // 在受控模型下没有额外职责，保留是为了 DOM 结构与 TS 一致。
            div { {children} }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_class_matches_ts() {
        assert_eq!(SCROLL_FADE_VIEWPORT_BASE, "min-h-0 flex-1 overflow-y-auto");
    }

    #[test]
    fn content_shorter_than_viewport_has_no_mask() {
        // maxScrollTop = 100 - 200 = -100 <= 1
        assert_eq!(scroll_mask_state(0.0, 100.0, 200.0), ScrollMaskState::None);
    }

    #[test]
    fn exact_fit_has_no_mask() {
        assert_eq!(scroll_mask_state(0.0, 200.0, 200.0), ScrollMaskState::None);
        // maxScrollTop = 1 仍在阈值内
        assert_eq!(scroll_mask_state(0.0, 201.0, 200.0), ScrollMaskState::None);
    }

    #[test]
    fn at_top_only_bottom_is_hidden() {
        assert_eq!(
            scroll_mask_state(0.0, 1000.0, 200.0),
            ScrollMaskState::Bottom
        );
    }

    #[test]
    fn at_bottom_only_top_is_hidden() {
        assert_eq!(
            scroll_mask_state(800.0, 1000.0, 200.0),
            ScrollMaskState::Top
        );
    }

    #[test]
    fn in_the_middle_both_are_hidden() {
        assert_eq!(
            scroll_mask_state(400.0, 1000.0, 200.0),
            ScrollMaskState::Both
        );
    }

    #[test]
    fn subpixel_jitter_does_not_flip_state() {
        // 0.5px 的抖动不该让「顶端」误判成「上下都有」
        assert_eq!(
            scroll_mask_state(0.5, 1000.0, 200.0),
            ScrollMaskState::Bottom
        );
        // 距底 0.5px 同理
        assert_eq!(
            scroll_mask_state(799.5, 1000.0, 200.0),
            ScrollMaskState::Top
        );
    }

    #[test]
    fn state_names_match_ts_data_attribute() {
        assert_eq!(ScrollMaskState::None.as_str(), "none");
        assert_eq!(ScrollMaskState::Top.as_str(), "top");
        assert_eq!(ScrollMaskState::Bottom.as_str(), "bottom");
        assert_eq!(ScrollMaskState::Both.as_str(), "both");
    }

    #[test]
    fn none_has_no_mask_classes() {
        assert_eq!(ScrollMaskState::None.classes(), "");
    }

    #[test]
    fn mask_classes_use_underscores_for_calc_spaces() {
        // Tailwind arbitrary value 里空格要写成下划线，否则生成的是无效的 calc(100%-24px)
        assert!(ScrollMaskState::Bottom.classes().contains("calc(100%_-_24px)"));
        assert!(!ScrollMaskState::Bottom.classes().contains("calc(100% - 24px)"));
    }

    #[test]
    fn both_includes_both_edges() {
        let c = ScrollMaskState::Both.classes();
        assert!(c.contains("transparent_0"));
        assert!(c.contains("black_24px"));
        assert!(c.contains("transparent_100%"));
    }

    #[test]
    fn user_class_wins_over_base() {
        // 同组覆盖：调用方给 overflow-y-scroll 时不该再留着 overflow-y-auto
        let c = scroll_fade_viewport_classes(ScrollMaskState::None, "overflow-y-scroll");
        assert!(c.contains("overflow-y-scroll"));
        assert!(!c.contains("overflow-y-auto"));
        // 没被覆盖的部分保留
        assert!(c.contains("min-h-0"));
        assert!(c.contains("flex-1"));
    }

    #[test]
    fn mask_classes_sit_between_base_and_user_class() {
        let c = scroll_fade_viewport_classes(ScrollMaskState::Both, "");
        assert!(c.starts_with("min-h-0"));
        assert!(c.contains("[mask-image:"));
    }
}
