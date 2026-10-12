//! `overlay` —— 浮层组件的**共享定位约定**。
//!
//! ## 为什么需要这一层
//!
//! TS 侧所有浮层（Tooltip / HoverCard / Popover / DropdownMenu / Select /
//! Dialog …）都依赖 Radix 的 `Portal` + `@floating-ui`：
//!
//! - `Portal` 把浮层 DOM **搬到 `<body>` 下**，从而不受祖先 `overflow: hidden` /
//!   `transform` 的裁剪与层叠上下文影响；
//! - `floating-ui` 在运行时测量锚点与浮层尺寸，算出 `top/left` 并写入内联样式，
//!   还会自动翻转（flip）与平移（shift）以避免越界。
//!
//! Rust 侧**两者都没有**（无渲染器时不可测；有渲染器时也需要大量 JS 桥接）。
//! 因此这里统一采用一套**纯 CSS 的自管理定位**约定，分两条路径：
//!
//! ### 路径 A：相对锚点（`side_align_classes`）
//!
//! 1. 每个浮层组件渲染一个 `position: relative` 的**锚点包裹元素**
//!    （[`OVERLAY_ANCHOR_BASE`]），浮层内容以 `position: absolute` 相对它定位；
//! 2. 方向与对齐由 [`Side`] / [`Align`] 枚举映射成固定的 inset + translate 类名
//!    （[`side_align_classes`]），不依赖运行时测量；
//! 3. 层级用 `z-*` 类名静态指定（沿用 TS 的取值，如 tooltip `z-50`、
//!    popover / menu `z-[60]`）。
//!
//! 适用于 Tooltip / HoverCard / Popover / DropdownMenu / Select ——
//! 它们的锚点是一个**元素**。
//!
//! ### 路径 B：跟随光标（`cursor_overlay_classes` + `cursor_position_style`）
//!
//! 右键菜单的锚点是**鼠标位置**，没有可包裹的元素，所以走
//! `position: fixed` + 内联 `left/top`。`fixed` 是必须的：`clientX` / `clientY`
//! 是视口坐标，只有 `fixed` 的包含块才是视口。另外提供
//! [`clamp_to_viewport`]（`floating-ui` 的 `shift` 等价物），
//! 让贴着屏幕边缘右键时菜单不会跑出去。
//!
//! ## 与 TS 的已知差异
//!
//! | 能力 | TS（Radix） | 本移植 |
//! | --- | --- | --- |
//! | 脱离祖先裁剪 | Portal 到 body | ⚠️ 路径 A 仍在原位置（需祖先无 `overflow:hidden`）；路径 B 用 `fixed` 天然脱离 |
//! | 自动翻转 | floating-ui flip | ❌ 路径 A 固定方向，可能越界 |
//! | 自动平移 | floating-ui shift | ⚠️ 路径 B 有 [`clamp_to_viewport`]；路径 A 无 |
//! | 箭头指向 | floating-ui arrow | ❌ 未实现 |
//! | 定位计算 | 运行时 JS | ✅ 纯 CSS + 可测的纯函数，零运行时测量 |

use crate::cn::cn;

/// 浮层相对锚点的方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Side {
    #[default]
    Top,
    Right,
    Bottom,
    Left,
}

impl Side {
    pub fn as_str(&self) -> &'static str {
        match self {
            Side::Top => "top",
            Side::Right => "right",
            Side::Bottom => "bottom",
            Side::Left => "left",
        }
    }

    /// 反向的边（用于 `slide-in-from-*` 类名：浮层从锚点方向滑入）。
    pub fn slide_from(&self) -> &'static str {
        match self {
            Side::Top => "slide-in-from-bottom-2",
            Side::Right => "slide-in-from-left-2",
            Side::Bottom => "slide-in-from-top-2",
            Side::Left => "slide-in-from-right-2",
        }
    }
}

/// 浮层沿交叉轴的对齐方式。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Align {
    Start,
    #[default]
    Center,
    End,
}

impl Align {
    pub fn as_str(&self) -> &'static str {
        match self {
            Align::Start => "start",
            Align::Center => "center",
            Align::End => "end",
        }
    }
}

/// 锚点包裹元素的类名。
///
/// TS 里浮层组件**不产生额外 DOM**（Portal 是逻辑容器）；本移植为了给
/// `position: absolute` 提供包含块，必须插入这一层，见模块文档。
pub const OVERLAY_ANCHOR_BASE: &str = "relative inline-flex";

/// 浮层内容通用的定位类名（`absolute` 由这里统一给出）。
pub const OVERLAY_POSITION_BASE: &str = "absolute";

/// 把 `side` / `align` / `side_offset` 映射成定位类名。
///
/// 偏移量以像素为单位（对应 Radix 的 `sideOffset`），直接生成
/// `mt-[4px]` 这类任意值，避免依赖主题里的 spacing 刻度。
pub fn side_align_classes(side: Side, align: Align, side_offset: f64) -> String {
    let offset = if side_offset.fract() == 0.0 {
        format!("{}px", side_offset as i64)
    } else {
        format!("{side_offset}px")
    };

    let placement = match (side, align) {
        // 上
        (Side::Top, Align::Start) => "bottom-full left-0",
        (Side::Top, Align::Center) => "bottom-full left-1/2 -translate-x-1/2",
        (Side::Top, Align::End) => "bottom-full right-0",
        // 下
        (Side::Bottom, Align::Start) => "top-full left-0",
        (Side::Bottom, Align::Center) => "top-full left-1/2 -translate-x-1/2",
        (Side::Bottom, Align::End) => "top-full right-0",
        // 左
        (Side::Left, Align::Start) => "right-full top-0",
        (Side::Left, Align::Center) => "right-full top-1/2 -translate-y-1/2",
        (Side::Left, Align::End) => "right-full bottom-0",
        // 右
        (Side::Right, Align::Start) => "left-full top-0",
        (Side::Right, Align::Center) => "left-full top-1/2 -translate-y-1/2",
        (Side::Right, Align::End) => "left-full bottom-0",
    };

    let gap = match side {
        Side::Top => format!("mb-[{offset}]"),
        Side::Bottom => format!("mt-[{offset}]"),
        Side::Left => format!("mr-[{offset}]"),
        Side::Right => format!("ml-[{offset}]"),
    };

    cn(&[OVERLAY_POSITION_BASE, placement, &gap])
}

/// 锚点包裹元素的最终类名。
pub fn overlay_anchor_classes(class: &str) -> String {
    cn(&[OVERLAY_ANCHOR_BASE, class])
}

// ---------------------------------------------------------------------------
// 光标定位（右键菜单）
// ---------------------------------------------------------------------------

/// 光标定位浮层的定位基准类名。
///
/// 用 `fixed` 而不是 `absolute`：`clientX` / `clientY` 是**视口坐标**，
/// 只有 `position: fixed` 的包含块才是视口。用 `absolute` 会把浮层相对最近的
/// 定位祖先偏移，右键菜单就会跑偏 —— 这是 Radix 用 `Portal` 到 `body`
/// 之外还需要额外处理的原因，我们靠 `fixed` 一步到位。
pub const OVERLAY_CURSOR_BASE: &str = "fixed";

/// 由光标坐标生成内联定位样式。
pub fn cursor_position_style(x: f64, y: f64) -> String {
    format!("left: {x}px; top: {y}px;")
}

/// 光标定位浮层的最终类名。
pub fn cursor_overlay_classes(class: &str) -> String {
    cn(&[OVERLAY_CURSOR_BASE, class])
}

/// 把浮层左上角夹进视口内。
///
/// 这是 `floating-ui` 的 `shift` 中间件里最常用的一条（只平移、不翻方向）的
/// 等价物：右键贴着屏幕右/下边缘时，菜单仍然完整可见。
///
/// 参数都用视口坐标系（`clientX` / `clientY` 那一套），
/// `padding` 是距视口边缘的安全间距。
///
/// 浮层比视口还大时（`content_* + 2 * padding > viewport_*`）优先保证左上角
/// 不越界，即退化为 `padding`。
pub fn clamp_to_viewport(
    x: f64,
    y: f64,
    viewport_w: f64,
    viewport_h: f64,
    content_w: f64,
    content_h: f64,
    padding: f64,
) -> (f64, f64) {
    let clamp_axis = |pos: f64, viewport: f64, content: f64| {
        let max = viewport - content - padding;
        if max < padding {
            // 放不下：退到左/上边界，至少不把起点挤出屏幕
            padding
        } else {
            pos.clamp(padding, max)
        }
    };
    (
        clamp_axis(x, viewport_w, content_w),
        clamp_axis(y, viewport_h, content_h),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_and_align_strings() {
        assert_eq!(Side::Top.as_str(), "top");
        assert_eq!(Side::Right.as_str(), "right");
        assert_eq!(Side::Bottom.as_str(), "bottom");
        assert_eq!(Side::Left.as_str(), "left");
        assert_eq!(Side::default(), Side::Top);

        assert_eq!(Align::Start.as_str(), "start");
        assert_eq!(Align::Center.as_str(), "center");
        assert_eq!(Align::End.as_str(), "end");
        assert_eq!(Align::default(), Align::Center);
    }

    #[test]
    fn slide_from_is_opposite_of_side() {
        assert_eq!(Side::Top.slide_from(), "slide-in-from-bottom-2");
        assert_eq!(Side::Bottom.slide_from(), "slide-in-from-top-2");
        assert_eq!(Side::Left.slide_from(), "slide-in-from-right-2");
        assert_eq!(Side::Right.slide_from(), "slide-in-from-left-2");
    }

    #[test]
    fn top_center_centers_horizontally() {
        let c = side_align_classes(Side::Top, Align::Center, 4.0);
        assert!(c.contains("absolute"));
        assert!(c.contains("bottom-full"));
        assert!(c.contains("left-1/2"));
        assert!(c.contains("-translate-x-1/2"));
        assert!(c.contains("mb-[4px]"));
        assert!(!c.contains("mt-"));
    }

    #[test]
    fn bottom_start_anchors_left() {
        let c = side_align_classes(Side::Bottom, Align::Start, 0.0);
        assert!(c.contains("top-full"));
        assert!(c.contains("left-0"));
        assert!(c.contains("mt-[0px]"));
        assert!(!c.contains("translate-x"));
    }

    #[test]
    fn left_center_centers_vertically() {
        let c = side_align_classes(Side::Left, Align::Center, 8.0);
        assert!(c.contains("right-full"));
        assert!(c.contains("top-1/2"));
        assert!(c.contains("-translate-y-1/2"));
        assert!(c.contains("mr-[8px]"));
    }

    #[test]
    fn fractional_offset_is_preserved() {
        let c = side_align_classes(Side::Right, Align::End, 1.5);
        assert!(c.contains("ml-[1.5px]"));
        assert!(c.contains("bottom-0"));
    }

    #[test]
    fn anchor_is_relative() {
        let c = overlay_anchor_classes("");
        assert!(c.contains("relative"));
        assert!(c.contains("inline-flex"));
        assert!(overlay_anchor_classes("w-full").contains("w-full"));
    }

    #[test]
    fn user_class_overrides_offset() {
        // 调用方传入的类名在最后，应覆盖默认偏移
        let c = cn(&[&side_align_classes(Side::Top, Align::Center, 4.0), "mb-2"]);
        assert!(c.contains("mb-2"));
        assert!(!c.contains("mb-[4px]"));
    }

    #[test]
    fn cursor_overlay_is_fixed() {
        // clientX/clientY 是视口坐标 → 必须用 fixed，absolute 会相对祖先偏移
        assert_eq!(OVERLAY_CURSOR_BASE, "fixed");
        let c = cursor_overlay_classes("min-w-44");
        assert!(c.contains("fixed"));
        assert!(c.contains("min-w-44"));
    }

    #[test]
    fn cursor_style_uses_pixels() {
        assert_eq!(cursor_position_style(120.0, 340.5), "left: 120px; top: 340.5px;");
    }

    #[test]
    fn clamp_leaves_roomy_positions_alone() {
        let (x, y) = clamp_to_viewport(100.0, 200.0, 1280.0, 800.0, 200.0, 300.0, 8.0);
        assert_eq!((x, y), (100.0, 200.0));
    }

    #[test]
    fn clamp_pulls_back_from_the_far_edges() {
        // 光标在右下角 → 菜单要整体左移/上移，保证完整可见
        let (x, y) = clamp_to_viewport(1270.0, 790.0, 1280.0, 800.0, 200.0, 300.0, 8.0);
        assert_eq!(x, 1280.0 - 200.0 - 8.0);
        assert_eq!(y, 800.0 - 300.0 - 8.0);
    }

    #[test]
    fn clamp_keeps_the_near_edges_off_screen() {
        // 负坐标（多显示器）也不该贴到 0
        let (x, y) = clamp_to_viewport(-50.0, -50.0, 1280.0, 800.0, 200.0, 300.0, 8.0);
        assert_eq!((x, y), (8.0, 8.0));
    }

    #[test]
    fn clamp_degrades_gracefully_when_content_is_too_big() {
        // 浮层比视口还大：退到 padding，而不是算出负的 max 再夹出负数
        let (x, y) = clamp_to_viewport(500.0, 500.0, 300.0, 200.0, 400.0, 300.0, 8.0);
        assert_eq!((x, y), (8.0, 8.0));
    }
}
