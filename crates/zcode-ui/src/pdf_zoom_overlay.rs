//! `pdf_zoom_overlay` —— Rust 移植自
//! `packages/ui/src/components/ui/usePdfZoomOverlay.ts`
//!
//! PDF 预览的缩放数学与「双缓冲覆盖层」策略。
//!
//! ## 关键移植点 1：两套倍率（预览倍率 vs 渲染倍率）
//!
//! 缩放手势期间**只**更新 `displayScale`（走 CSS `transform: scale()`，
//! 由合成器直接出画面，不掉帧）；手势停顿 [`ZOOM_COMMIT_DELAY_MS`] 之后才把
//! 倍率提交给真正的渲染器重画。这样连续捏合/滚轮不会触发几十次整页重绘。
//!
//! 提交时旧画布会被卸载、新画布尚未画完，中间会闪一下空白 —— 所以要先
//! 把当前位图复制成覆盖层（[`PdfZoomOverlayPlacement`]），等新画布
//! `onRenderSuccess` 到了再撤掉。
//!
//! ## 关键移植点 2：缩放锚点
//!
//! 缩放要「指针下的那个内容点不动」，所以提交前先
//! [`capture_pdf_zoom_anchor`] 记下指针在页面上的比例坐标，布局提交后再
//! [`restore_pdf_zoom_anchor`] 反算出滚动量把那个点拉回原位。
//! 指针没给时退回容器中心（对应「按钮缩放」）。
//!
//! ## 与 TS 的差异
//!
//! 1. **DOM 参数换成几何量**：TS 的 `capturePdfZoomAnchor(container, pageViewport,
//!    pointer)` 直接调 `getBoundingClientRect()`；Rust 侧改成收 [`PdfRect`]，
//!    由调用方测量。纯数学因此可以完整测试。
//! 2. **`restorePdfZoomAnchor` 返回增量而不是直接改 DOM**：TS 里是
//!    `container.scrollLeft += ...`；Rust 侧返回 [`PdfScrollAdjustment`]，
//!    由调用方加回滚动容器。
//! 3. **`getPdfPageDisplaySize` / `getPdfPagePreviewStyle` 返回结构体而不是
//!    `CSSProperties`**：由调用方拼样式串。

/// 一次缩放的步进（按钮缩放用）。
pub const ZOOM_STEP: f64 = 0.25;

/// 最小倍率。
pub const MIN_SCALE: f64 = 0.25;

/// 最大倍率。
pub const MAX_SCALE: f64 = 4.0;

/// 默认倍率。
pub const DEFAULT_SCALE: f64 = 1.0;

/// 修饰键 + 滚轮的连续缩放灵敏度。
///
/// 选这个值是为了同时兼容触控板的高频小 `deltaY` 和鼠标滚轮的大 `deltaY`。
pub const WHEEL_ZOOM_SENSITIVITY: f64 = 0.002;

/// 手势停顿多久后提交真实渲染。
pub const ZOOM_COMMIT_DELAY_MS: u64 = 200;

/// 把倍率夹到 `[MIN_SCALE, MAX_SCALE]`。
pub fn clamp_scale(scale: f64) -> f64 {
    if scale.is_nan() {
        return DEFAULT_SCALE;
    }
    MAX_SCALE.min(MIN_SCALE.max(scale))
}

/// 一个视口坐标点。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PdfZoomPointer {
    pub client_x: f64,
    pub client_y: f64,
}

/// 一个矩形（`getBoundingClientRect()` 的结果）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PdfRect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub width: f64,
    pub height: f64,
}

impl PdfRect {
    /// 从左上角 + 尺寸构造（`right` / `bottom` 由 `left` / `top` 推出）。
    pub fn from_origin(left: f64, top: f64, width: f64, height: f64) -> Self {
        Self {
            left,
            top,
            right: left + width,
            bottom: top + height,
            width,
            height,
        }
    }
}

/// 页面的固有尺寸（PDF 点数 × 倍率之前的原始尺寸）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PdfPageSize {
    pub width: f64,
    pub height: f64,
}

/// 页面在容器里的显示尺寸。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PdfDisplaySize {
    pub width: f64,
    pub height: f64,
}

/// 缩放锚点：指针落在页面上的**比例**坐标。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PdfZoomAnchor {
    pub client_x: f64,
    pub client_y: f64,
    pub page_x_ratio: f64,
    pub page_y_ratio: f64,
}

/// 需要加回滚动容器的偏移量。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PdfScrollAdjustment {
    pub left: f64,
    pub top: f64,
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    max.min(min.max(value))
}

/// 页面尺寸没变时**复用原值**（对应 TS 里的 `current?.width === width && … ? current : {...}`）。
///
/// 这一点很重要：返回同一个值意味着上层状态不变、不会触发重渲染。
pub fn resolve_pdf_page_size(
    current: Option<PdfPageSize>,
    width: f64,
    height: f64,
) -> PdfPageSize {
    match current {
        Some(size) if size.width == width && size.height == height => size,
        _ => PdfPageSize { width, height },
    }
}

/// 页面显示尺寸：固有尺寸 × 倍率，**向下取整**。
pub fn get_pdf_page_display_size(
    intrinsic_size: Option<PdfPageSize>,
    display_scale: f64,
) -> Option<PdfDisplaySize> {
    intrinsic_size.map(|size| PdfDisplaySize {
        width: (size.width * display_scale).floor(),
        height: (size.height * display_scale).floor(),
    })
}

/// 页面预览层的布局策略。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PdfPagePreviewStyle {
    /// 是否脱离普通文档流（`position: absolute; inset: 0`）。
    pub absolute: bool,
    /// 预览倍率；`1.0` 表示不加 `transform`。
    pub preview_scale: f64,
    /// `transform-origin` 恒为 `top left`。
    pub transform_origin: &'static str,
}

/// 页面预览样式（TS `getPdfPagePreviewStyle`）。
///
/// 返回 `None` 表示「既没有稳定布局盒、倍率又是 1」→ 不需要任何样式。
///
/// 注释点明的原因：**已知页面尺寸后要脱离普通文档流**，只让目标布局盒决定滚动范围，
/// 否则旧画布的尺寸会干扰缩放后的滚动条。
pub fn get_pdf_page_preview_style(
    has_stable_layout_box: bool,
    preview_scale: f64,
) -> Option<PdfPagePreviewStyle> {
    if !has_stable_layout_box && preview_scale == 1.0 {
        return None;
    }
    Some(PdfPagePreviewStyle {
        absolute: has_stable_layout_box,
        preview_scale,
        transform_origin: "top left",
    })
}

/// 记录缩放锚点。
///
/// - `container`：滚动容器；`page`：页面视口。
/// - `pointer` 为 `None` 时取容器中心（按钮缩放）。
/// - 页面尺寸为 0（还没渲染）→ `None`。
///
/// 指针先被夹进容器可视区，再换算成页面上的比例坐标（再夹到 `[0, 1]`），
/// 所以指针在容器外时锚点会落到页面边缘而不是飞出去。
pub fn capture_pdf_zoom_anchor(
    container: PdfRect,
    page: PdfRect,
    pointer: Option<PdfZoomPointer>,
) -> Option<PdfZoomAnchor> {
    if page.width == 0.0 || page.height == 0.0 {
        return None;
    }

    let requested_client_x = pointer
        .map(|p| p.client_x)
        .unwrap_or(container.left + container.width / 2.0);
    let requested_client_y = pointer
        .map(|p| p.client_y)
        .unwrap_or(container.top + container.height / 2.0);
    let visible_client_x = clamp(requested_client_x, container.left, container.right);
    let visible_client_y = clamp(requested_client_y, container.top, container.bottom);
    let page_x_ratio = clamp((visible_client_x - page.left) / page.width, 0.0, 1.0);
    let page_y_ratio = clamp((visible_client_y - page.top) / page.height, 0.0, 1.0);

    Some(PdfZoomAnchor {
        client_x: page.left + page.width * page_x_ratio,
        client_y: page.top + page.height * page_y_ratio,
        page_x_ratio,
        page_y_ratio,
    })
}

/// 反算把锚点拉回原位需要的滚动增量（TS `restorePdfZoomAnchor`）。
pub fn restore_pdf_zoom_anchor(page: PdfRect, anchor: PdfZoomAnchor) -> PdfScrollAdjustment {
    PdfScrollAdjustment {
        left: page.left + page.width * anchor.page_x_ratio - anchor.client_x,
        top: page.top + page.height * anchor.page_y_ratio - anchor.client_y,
    }
}

/// 修饰键 + 滚轮的一次缩放目标倍率。
///
/// `scale * e^(-deltaY * SENSITIVITY)` —— 指数形式让「往上滚」与「往下滚」
/// 对称，且小 delta 的变化量也小（触控板手感）。
pub fn wheel_zoom_target(current_scale: f64, delta_y: f64) -> f64 {
    clamp_scale(current_scale * (-delta_y * WHEEL_ZOOM_SENSITIVITY).exp())
}

/// 按 `delta_y` 的符号决定放大还是缩小（`delta_y == 0` 时返回 `None`）。
pub fn resolve_wheel_zoom(current_scale: f64, delta_y: f64) -> Option<f64> {
    if delta_y == 0.0 {
        return None;
    }
    Some(wheel_zoom_target(current_scale, delta_y))
}

/// 覆盖层的摆放参数（TS `stageZoomOverlay` 里设置的那几条样式）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PdfZoomOverlayPlacement {
    pub target_width: f64,
    pub target_height: f64,
    pub position: &'static str,
    pub inset: &'static str,
    pub z_index: &'static str,
    pub pointer_events: &'static str,
    pub user_select: &'static str,
    pub aria_hidden: bool,
}

impl PdfZoomOverlayPlacement {
    pub fn new(target_width: f64, target_height: f64) -> Self {
        Self {
            target_width,
            target_height,
            position: "absolute",
            inset: "0",
            z_index: "1",
            pointer_events: "none",
            user_select: "none",
            aria_hidden: true,
        }
    }

    /// 是否可以摆放（源画布必须有非零尺寸，否则复制出来是空白）。
    pub fn can_stage(source_width: f64, source_height: f64, source_pixels_w: f64, source_pixels_h: f64) -> bool {
        source_width != 0.0
            && source_height != 0.0
            && source_pixels_w != 0.0
            && source_pixels_h != 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- 倍率夹取 ----

    #[test]
    fn clamp_scale_bounds() {
        assert_eq!(clamp_scale(1.0), 1.0);
        assert_eq!(clamp_scale(MIN_SCALE), MIN_SCALE);
        assert_eq!(clamp_scale(MAX_SCALE), MAX_SCALE);
        assert_eq!(clamp_scale(0.01), MIN_SCALE);
        assert_eq!(clamp_scale(100.0), MAX_SCALE);
        assert_eq!(clamp_scale(-3.0), MIN_SCALE);
    }

    #[test]
    fn clamp_scale_recovers_from_nan() {
        // NaN 穿过 min/max 会传染，必须显式兜住
        assert_eq!(clamp_scale(f64::NAN), DEFAULT_SCALE);
        assert_eq!(clamp_scale(f64::INFINITY), MAX_SCALE);
        assert_eq!(clamp_scale(f64::NEG_INFINITY), MIN_SCALE);
    }

    // ---- 页面尺寸 ----

    /// 尺寸没变时返回**同一个值**，上层状态因此不会更新、不触发重渲染。
    #[test]
    fn unchanged_page_size_is_reused() {
        let current = Some(PdfPageSize {
            width: 612.0,
            height: 792.0,
        });
        assert_eq!(
            resolve_pdf_page_size(current, 612.0, 792.0),
            current.unwrap()
        );
    }

    #[test]
    fn changed_page_size_produces_a_new_value() {
        let current = Some(PdfPageSize {
            width: 612.0,
            height: 792.0,
        });
        let next = resolve_pdf_page_size(current, 595.0, 842.0);
        assert_eq!((next.width, next.height), (595.0, 842.0));
        // 只有一边变也要更新
        assert_eq!(
            resolve_pdf_page_size(current, 612.0, 1000.0),
            PdfPageSize {
                width: 612.0,
                height: 1000.0
            }
        );
    }

    #[test]
    fn no_previous_size_yields_the_new_one() {
        assert_eq!(
            resolve_pdf_page_size(None, 10.0, 20.0),
            PdfPageSize {
                width: 10.0,
                height: 20.0
            }
        );
    }

    #[test]
    fn display_size_floors_the_product() {
        let size = Some(PdfPageSize {
            width: 612.5,
            height: 792.9,
        });
        let displayed = get_pdf_page_display_size(size, 1.0).unwrap();
        assert_eq!(displayed.width, 612.0);
        assert_eq!(displayed.height, 792.0);

        let doubled = get_pdf_page_display_size(size, 2.0).unwrap();
        assert_eq!(doubled.width, 1225.0);
        assert_eq!(doubled.height, 1585.0);
    }

    #[test]
    fn no_intrinsic_size_means_no_display_size() {
        assert_eq!(get_pdf_page_display_size(None, 2.0), None);
    }

    // ---- 预览样式 ----

    #[test]
    fn preview_style_is_omitted_when_there_is_nothing_to_do() {
        // 没有稳定布局盒 + 倍率 1 → 什么都不用加
        assert_eq!(get_pdf_page_preview_style(false, 1.0), None);
    }

    #[test]
    fn preview_style_goes_absolute_once_the_layout_box_is_stable() {
        let style = get_pdf_page_preview_style(true, 1.0).unwrap();
        // 脱离文档流，只让目标布局盒决定滚动范围
        assert!(style.absolute);
        assert_eq!(style.preview_scale, 1.0);
        assert_eq!(style.transform_origin, "top left");
    }

    #[test]
    fn preview_style_carries_the_transform_during_a_gesture() {
        let style = get_pdf_page_preview_style(false, 1.25).unwrap();
        // 还没有稳定布局盒 → 不脱离文档流，但要有 transform
        assert!(!style.absolute);
        assert_eq!(style.preview_scale, 1.25);

        let both = get_pdf_page_preview_style(true, 0.5).unwrap();
        assert!(both.absolute);
        assert_eq!(both.preview_scale, 0.5);
    }

    // ---- 锚点 ----

    fn container() -> PdfRect {
        PdfRect::from_origin(100.0, 50.0, 400.0, 300.0)
    }

    fn page() -> PdfRect {
        PdfRect::from_origin(200.0, 100.0, 200.0, 400.0)
    }

    #[test]
    fn a_zero_sized_page_cannot_be_anchored() {
        let empty = PdfRect::from_origin(0.0, 0.0, 0.0, 0.0);
        assert_eq!(capture_pdf_zoom_anchor(container(), empty, None), None);
        // 只有一边为 0 也不行
        let flat = PdfRect::from_origin(0.0, 0.0, 10.0, 0.0);
        assert_eq!(capture_pdf_zoom_anchor(container(), flat, None), None);
    }

    /// 不给指针时锚点落在**容器中心**（按钮缩放）。
    #[test]
    fn no_pointer_anchors_at_the_container_center() {
        let anchor = capture_pdf_zoom_anchor(container(), page(), None).unwrap();
        // 容器中心 (300, 200)；页面左上角 (200, 100)、尺寸 200×400
        assert_eq!(anchor.page_x_ratio, 0.5);
        assert_eq!(anchor.page_y_ratio, 0.25);
        // 归一化后的 client 坐标就是「页面左上 + 比例 × 尺寸」
        assert_eq!(anchor.client_x, 300.0);
        assert_eq!(anchor.client_y, 200.0);
    }

    #[test]
    fn a_pointer_inside_the_page_keeps_its_ratio() {
        let anchor = capture_pdf_zoom_anchor(
            container(),
            page(),
            Some(PdfZoomPointer {
                client_x: 250.0,
                client_y: 300.0,
            }),
        )
        .unwrap();
        assert_eq!(anchor.page_x_ratio, 0.25);
        assert_eq!(anchor.page_y_ratio, 0.5);
        assert_eq!((anchor.client_x, anchor.client_y), (250.0, 300.0));
    }

    /// 指针在容器外时先夹进容器，再换算 —— 锚点落在页面边缘而不是飞出去。
    #[test]
    fn a_pointer_outside_the_container_is_clamped_into_it() {
        let anchor = capture_pdf_zoom_anchor(
            container(),
            page(),
            Some(PdfZoomPointer {
                client_x: 9_999.0,
                client_y: -9_999.0,
            }),
        )
        .unwrap();
        // 容器右边界 500 → 页面内比例 (500-200)/200 = 1.5 → 夹到 1
        assert_eq!(anchor.page_x_ratio, 1.0);
        // 容器上边界 50 → 页面内比例 (50-100)/400 = -0.125 → 夹到 0
        assert_eq!(anchor.page_y_ratio, 0.0);
        assert_eq!((anchor.client_x, anchor.client_y), (400.0, 100.0));
    }

    /// 指针落在页面外（但在容器内）时比例夹到 `[0,1]`。
    #[test]
    fn a_pointer_beside_the_page_is_clamped_to_its_edge() {
        // 容器左侧 (100, 200) 在页面左边 (200) 之外
        let anchor = capture_pdf_zoom_anchor(
            container(),
            page(),
            Some(PdfZoomPointer {
                client_x: 100.0,
                client_y: 200.0,
            }),
        )
        .unwrap();
        assert_eq!(anchor.page_x_ratio, 0.0);
        assert_eq!(anchor.page_y_ratio, 0.25);
    }

    #[test]
    fn restoring_an_anchor_that_did_not_move_needs_no_scroll() {
        let anchor = capture_pdf_zoom_anchor(container(), page(), None).unwrap();
        // 页面没动 → 增量为 0
        let adjustment = restore_pdf_zoom_anchor(page(), anchor);
        assert!(adjustment.left.abs() < 1e-9, "{adjustment:?}");
        assert!(adjustment.top.abs() < 1e-9, "{adjustment:?}");
    }

    /// 页面整体右下移动 40px 时，滚动量要往回收 40px 才能把内容点钉住。
    #[test]
    fn restoring_compensates_for_the_page_moving() {
        let anchor = capture_pdf_zoom_anchor(container(), page(), None).unwrap();
        let moved = PdfRect::from_origin(240.0, 140.0, 200.0, 400.0);
        let adjustment = restore_pdf_zoom_anchor(moved, anchor);
        assert!((adjustment.left - 40.0).abs() < 1e-9, "{adjustment:?}");
        assert!((adjustment.top - 40.0).abs() < 1e-9, "{adjustment:?}");
    }

    /// 页面放大一倍时，锚点比例不变但像素位置变化 —— 增量要把差值补回来。
    #[test]
    fn restoring_handles_a_scaled_page() {
        let anchor = capture_pdf_zoom_anchor(
            container(),
            page(),
            Some(PdfZoomPointer {
                client_x: 250.0,
                client_y: 300.0,
            }),
        )
        .unwrap();
        // 页面以左上角为原点放大一倍：200×400 → 400×800
        let scaled = PdfRect::from_origin(200.0, 100.0, 400.0, 800.0);
        let adjustment = restore_pdf_zoom_anchor(scaled, anchor);
        // 新位置 = 200 + 400*0.25 = 300；原 client_x = 250 → 增量 +50
        assert!((adjustment.left - 50.0).abs() < 1e-9, "{adjustment:?}");
        // 新位置 = 100 + 800*0.5 = 500；原 client_y = 300 → 增量 +200
        assert!((adjustment.top - 200.0).abs() < 1e-9, "{adjustment:?}");
    }

    // ---- 滚轮缩放 ----

    #[test]
    fn a_zero_delta_does_not_zoom() {
        assert_eq!(resolve_wheel_zoom(1.0, 0.0), None);
    }

    #[test]
    fn scrolling_up_zooms_in_and_down_zooms_out() {
        // deltaY < 0（向上滚）→ 放大
        let zoomed_in = resolve_wheel_zoom(1.0, -100.0).unwrap();
        assert!(zoomed_in > 1.0, "{zoomed_in}");
        // deltaY > 0（向下滚）→ 缩小
        let zoomed_out = resolve_wheel_zoom(1.0, 100.0).unwrap();
        assert!(zoomed_out < 1.0, "{zoomed_out}");
    }

    /// 指数形式：同样大小的正负 delta 互逆（对称）。
    #[test]
    fn opposite_deltas_are_symmetric() {
        let in_scale = resolve_wheel_zoom(1.0, -100.0).unwrap();
        let out_scale = resolve_wheel_zoom(1.0, 100.0).unwrap();
        assert!((in_scale * out_scale - 1.0).abs() < 1e-9, "{in_scale} {out_scale}");
    }

    #[test]
    fn wheel_zoom_respects_the_bounds() {
        // 疯狂往上滚也超不过上限
        assert_eq!(resolve_wheel_zoom(3.0, -100_000.0).unwrap(), MAX_SCALE);
        assert_eq!(resolve_wheel_zoom(0.5, 100_000.0).unwrap(), MIN_SCALE);
    }

    #[test]
    fn sensitivity_matches_the_constant() {
        // 公式是 `scale * e^(-deltaY * SENSITIVITY)`；
        // deltaY = -100 时指数是 `+100 * SENSITIVITY = +0.2`
        let expected = (100.0 * WHEEL_ZOOM_SENSITIVITY).exp();
        assert!((wheel_zoom_target(1.0, -100.0) - expected).abs() < 1e-12);
        // 换个起点也成立（且不撞上下限）
        let expected_from_half = 0.5 * (100.0 * WHEEL_ZOOM_SENSITIVITY).exp();
        assert!((wheel_zoom_target(0.5, -100.0) - expected_from_half).abs() < 1e-12);
    }

    // ---- 覆盖层 ----

    #[test]
    fn overlay_placement_styles() {
        let placement = PdfZoomOverlayPlacement::new(320.0, 480.0);
        assert_eq!(placement.target_width, 320.0);
        assert_eq!(placement.target_height, 480.0);
        assert_eq!(placement.position, "absolute");
        assert_eq!(placement.inset, "0");
        assert_eq!(placement.z_index, "1");
        // 覆盖层只是视觉缓冲，不能吃指针事件
        assert_eq!(placement.pointer_events, "none");
        assert_eq!(placement.user_select, "none");
        assert!(placement.aria_hidden);
    }

    #[test]
    fn overlay_requires_a_non_zero_source() {
        assert!(PdfZoomOverlayPlacement::can_stage(100.0, 200.0, 100.0, 200.0));
        // 任一维为 0 都复制不出内容
        assert!(!PdfZoomOverlayPlacement::can_stage(0.0, 200.0, 100.0, 200.0));
        assert!(!PdfZoomOverlayPlacement::can_stage(100.0, 0.0, 100.0, 200.0));
        assert!(!PdfZoomOverlayPlacement::can_stage(100.0, 200.0, 0.0, 200.0));
        assert!(!PdfZoomOverlayPlacement::can_stage(100.0, 200.0, 100.0, 0.0));
    }
}
