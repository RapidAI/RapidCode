//! `GlmMonochromeIcon` —— Rust 移植自
//! `packages/ui/src/components/ui/GlmMonochromeIcon.tsx`。
//!
//! 组件本身很简单：一张 `<img>`，按主题换图源 + 换一组 CSS `filter`。
//! 真正值得移植的是**那两条 filter 表达式**（TS 注释解释了为什么不用线框图：
//! 线框化会破坏原始 logo 的识别度，视觉上也偏轻；这里恢复原始位图，
//! 只做去色和明度压缩）。
//!
//! ## 与 TS 的差异：图源由调用方提供
//!
//! TS 用 Vite 的 `import x from "....png"` 拿到带 hash 的 URL，再由打包器处理。
//! Rust 侧没有等价的资源管线（`dioxus::asset!` 需要渲染器 feature，会让本 crate
//! 失去「任意环境可编译」的性质），因此 `src` 做成显式 prop，
//! 由调用方决定怎么加载（`include_bytes!` + data URI、或交给宿主的资源系统）。
//!
//! 主题同理：TS 里读的是全局 store（`useZCodeStore`），这里做成 `is_dark` prop，
//! 保持组件库不依赖任何 store。

use crate::cn::cn;
use dioxus::prelude::*;

/// `<img>` 的基础类名（TS 原文）。
pub const GLM_MONOCHROME_BASE: &str = "shrink-0 object-contain";

/// 深色主题下的 filter（抬到偏白，压掉自带蓝色，但不丢轮廓）。
pub const GLM_FILTER_DARK: &str = "grayscale(1) brightness(1.9) contrast(0.8)";

/// 浅色主题下的 filter（保留偏灰效果）。
pub const GLM_FILTER_LIGHT: &str = "grayscale(1) brightness(0.74) contrast(1.05)";

/// 按主题取 filter —— 移植的核心逻辑，纯函数。
pub fn glm_monochrome_filter(is_dark: bool) -> &'static str {
    if is_dark {
        GLM_FILTER_DARK
    } else {
        GLM_FILTER_LIGHT
    }
}

/// GLM 的单色图标。
///
/// `src` 必传（见模块文档的差异说明）；`alt` 为空时不发 `aria-hidden`，
/// 非空时标 `aria-hidden="true"` —— 与 TS 的 `aria-hidden={alt ? undefined : true}` 一致。
#[component]
pub fn GlmMonochromeIcon(
    /// 图标源。深色/浅色两张图的取值由调用方决定。
    #[props(into)] src: String,
    #[props(default, into)] alt: String,
    /// 是否深色主题。默认 `false`（浅色）。
    #[props(default)] is_dark: bool,
    #[props(default, into)] class: String,
) -> Element {
    let filter = glm_monochrome_filter(is_dark);
    rsx! {
        img {
            class: cn(&[GLM_MONOCHROME_BASE, &class]),
            src: src,
            alt: alt.clone(),
            // 空 alt 表示纯装饰 → 对读屏隐藏；有 alt 就让它被读出来
            "aria-hidden": alt.is_empty().then_some("true"),
            style: "filter: {filter}",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_class_matches_ts() {
        assert_eq!(GLM_MONOCHROME_BASE, "shrink-0 object-contain");
    }

    #[test]
    fn filters_match_ts_verbatim() {
        assert_eq!(
            glm_monochrome_filter(true),
            "grayscale(1) brightness(1.9) contrast(0.8)"
        );
        assert_eq!(
            glm_monochrome_filter(false),
            "grayscale(1) brightness(0.74) contrast(1.05)"
        );
    }

    #[test]
    fn dark_filter_brightens_and_light_filter_darkens() {
        // 两条表达式的方向必须相反，否则深色主题下图标会糊在背景里
        assert!(GLM_FILTER_DARK.contains("brightness(1.9)"));
        assert!(GLM_FILTER_LIGHT.contains("brightness(0.74)"));
    }
}
