//! `PptxPreviewViewer` —— Rust 移植自
//! `packages/ui/src/components/ui/pptx-preview-viewer.tsx`
//!
//! PPTX 预览：缩略图列 + 主视图 + 页码/缩放控制条 + 元素选择浮层。
//! TS 侧把幻灯片渲染整个交给 `@/presentation/pptxRendererPreviewEngine`
//! （内部是 `pptx-renderer`，把 OOXML 画进 DOM），这个文件负责**状态机、
//! 键盘、缩放、以及元素选择浮层的几何**。
//!
//! ## 关键移植点 1：只有「可编辑目标」才豁免方向键
//!
//! TS 里那个 guard 同时排除 `INPUT` / `TEXTAREA` / `isContentEditable`。
//! 原注释点明了原因：**评论草稿的 `Textarea` 经 Popover Portal 渲染，
//! 但仍在 React 树内向此处冒泡**，只豁免 `INPUT` 会让方向键在评论框里翻页，
//! 并连带清空未提交的评论。所以三者必须一起豁免。
//!
//! ## 关键移植点 2：Esc 在元素选择态下有两层含义
//!
//! | 状态 | Esc 的语义 |
//! |---|---|
//! | 非选择态 | 什么都不做 |
//! | 选择态 + 正在写评论 | 只回退到 AI 编辑条（**不清空已输入内容**） |
//! | 选择态 | 退出元素选择 |
//!
//! 顺序也很关键：**Esc 的判断在方向键之前**，且 `Escape` 只在选择态下被拦截。
//!
//! ## 关键移植点 3：元素选择浮层的 `zIndex` 是 `zIndex * 2 + 单元格位`
//!
//! 因为每个元素后面还要叠一层「命中区 + 高亮框」，`* 2` 给两层留位置；
//! 表格单元格再 `+1`，保证同一表格内单元格压在表格自身之上。
//!
//! ## 关键移植点 4：文字划选与元素选择是互斥的两条路径
//!
//! - `textSelectionEnabled = selectionMode && selectedElement?.text?.trim()`
//!   —— 注意是真值判断：只有**非空文本**（trim 后）才开启文字划选；
//! - 开启后，元素按钮变成 `pointer-events-none`（把指针让给底层文本），
//!   渲染面变成 `pointer-events-auto select-text`；
//! - 未开启时，渲染面 `pointer-events-none`（避免点到 OOXML 里的超链接），
//!   元素按钮 `pointer-events-auto`。
//!
//! ## 与 TS 的差异
//!
//! 1. **不引入 `pptx-renderer`**：`PresentationPreviewDocument` 的
//!    `open` / `renderPage` / `getPageElements` 全部由宿主注入。
//!    本组件收的是它的**投影**（`page_count` + `page_size`）与
//!    `children`（主页面的渲染结果）；缩略图列同理用 `thumbnails` 注入。
//!    组件自身渲染除画布之外的全部 chrome，因此可以被完整测试。
//! 2. **缩略图宽度改为 prop**：TS 在组件内用 `ResizeObserver` 测，
//!    Dioxus 的测量是异步的（`onmounted` + `get_client_rect()`），
//!    所以由宿主测好传进来（[`pptx_thumbnail_scale`] 仍逐字移植）。
//! 3. **IntersectionObserver 懒渲染改为宿主职责**：`visible` 标记在
//!    TS 里控制「滚到附近才渲染缩略图」，Rust 侧由宿主的缩略图元素决定。
//! 4. **`installDocumentLinkSafety` / `sanitizeDocumentHref` 由宿主负责**：
//!    它们是 DOM 层的拦截，见 `officeFilePreview`。
//! 5. **`labels` 里的 4 个函数型文案改为模板串**：TS 是
//!    `(n: number) => string`，Rust 侧用 `{n}` / `{path}` 占位符 +
//!    [`format_pptx_label`]。默认值不提供 —— TS 里 `labels` 是必填 prop。

use crate::cn::cn;
use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::components::input::{input_classes, InputSize};
use crate::components::pptx_selection_action_bar::PptxSelectionActionBar;
use crate::icons::{
    IconChevronLeft, IconChevronRight, IconFileDown, IconLoader2, IconMousePointer2, IconZoomIn,
    IconZoomOut,
};
use crate::presentation_element_model::{
    PresentationElementBounds, PresentationPageElement, PresentationPoint, PresentationSize,
};
use dioxus::prelude::*;

// ---- 缩放常量（逐字移植 `presentationPreviewControls.ts`） ----

pub const PPTX_MIN_ZOOM_PERCENT: i64 = 25;
pub const PPTX_MAX_ZOOM_PERCENT: i64 = 300;
pub const PPTX_ZOOM_STEP_PERCENT: i64 = 25;

/// 主视图的四周留白（`viewportSize - 32`）。
pub const PPTX_VIEWPORT_PADDING_PX: f64 = 32.0;

/// 缩略图按钮的内边距（`button.clientWidth - 16`）。
pub const PPTX_THUMBNAIL_PADDING_PX: f64 = 16.0;

/// 元素选择浮层的 `zIndex` 基数。
pub const PPTX_SELECTION_OVERLAY_Z_INDEX_FACTOR: i64 = 2;

// ---- 类名 ----

pub const PPTX_VIEWER_BASE: &str = "flex h-full min-h-0 outline-none";
pub const PPTX_THUMBNAIL_ASIDE: &str =
    "w-24 shrink-0 overflow-y-auto border-r border-border bg-surface/30 p-2 sm:w-32";
pub const PPTX_THUMBNAIL_LIST: &str = "flex flex-col gap-2";
pub const PPTX_THUMBNAIL_BUTTON_BASE: &str = "flex w-full flex-col items-center gap-1 rounded-lg border p-2 text-ui-xs text-foreground-subtle outline-none transition-colors";
pub const PPTX_THUMBNAIL_BUTTON_SELECTED: &str = "border-border-hover bg-selected text-foreground";
pub const PPTX_THUMBNAIL_BUTTON_IDLE: &str =
    "border-transparent hover:border-border hover:bg-surface-hover";
pub const PPTX_THUMBNAIL_FRAME: &str =
    "flex w-full items-center justify-center overflow-hidden bg-surface";
pub const PPTX_THUMBNAIL_NUMBER: &str = "tabular-nums";
pub const PPTX_MAIN_COLUMN: &str = "flex min-w-0 flex-1 flex-col bg-background";
pub const PPTX_MAIN_VIEWPORT: &str = "min-h-0 flex-1 overflow-auto";
pub const PPTX_MAIN_GRID: &str = "grid min-h-full min-w-full place-items-center p-4";
pub const PPTX_PAGE_FRAME: &str = "relative shrink-0 overflow-hidden bg-background shadow-md";
pub const PPTX_RENDER_SURFACE_FRAME: &str = "absolute left-0 top-0 origin-top-left";
pub const PPTX_SELECTION_OVERLAY: &str =
    "pointer-events-none absolute inset-0";
pub const PPTX_TOOLBAR: &str = "flex min-h-10 shrink-0 flex-wrap items-center justify-center gap-1 border-t border-border bg-surface/30 px-2 py-1.5 text-ui-base";
pub const PPTX_TOOLBAR_DIVIDER: &str = "mx-1 h-5 w-px bg-border";
pub const PPTX_PAGE_INPUT: &str =
    "w-12 text-center text-mobile-input-safe tabular-nums md:text-ui-base";
pub const PPTX_PAGE_COUNTER: &str = "min-w-10 text-center tabular-nums text-foreground-subtle";
pub const PPTX_ZOOM_LABEL: &str = "w-12 text-center tabular-nums text-foreground-subtle";
pub const PPTX_ERROR_MESSAGE: &str = "p-3 text-ui-base text-destructive";
pub const PPTX_SUBTLE_MESSAGE: &str = "p-3 text-ui-base text-foreground-subtle";

/// 元素浮层基础类名。
pub const PPTX_ELEMENT_BUTTON_BASE: &str = "absolute border outline-none transition-colors";
pub const PPTX_ELEMENT_BUTTON_SELECTED: &str = "border-primary bg-accent/30";
pub const PPTX_ELEMENT_BUTTON_IDLE: &str =
    "border-transparent bg-transparent hover:border-primary hover:bg-accent/20";

// ---- 文案 ----

/// 全部文案（TS `PptxPreviewViewerLabels`）。
///
/// 4 个函数型文案用 `{n}` / `{path}` 占位符表达，见模块差异 5。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PptxPreviewViewerLabels {
    pub loading: String,
    pub load_error: String,
    pub no_slides: String,
    pub previous_page: String,
    pub next_page: String,
    pub page_input: String,
    pub zoom_in: String,
    pub zoom_out: String,
    pub thumbnails: String,
    /// `labels.thumbnail(pageNumber)` 的模板，占位符 `{n}`。
    pub thumbnail: String,
    pub export_pdf: String,
    pub exporting_pdf: String,
    /// `labels.exportPdfSuccess(path)` 的模板，占位符 `{path}`。
    pub export_pdf_success: String,
    pub export_pdf_failed: String,
    pub select_element: String,
    pub exit_element_selection: String,
    pub ai_edit: String,
    pub comment_placeholder: String,
    pub cancel_ai_edit: String,
    pub add_to_conversation: String,
    /// `labels.referencedPageMissing(pageNumber)` 的模板，占位符 `{n}`。
    pub referenced_page_missing: String,
    /// `labels.referencedSourceChanged(pageNumber)` 的模板，占位符 `{n}`。
    pub referenced_source_changed: String,
}

/// 把模板里的占位符全部替换掉。
///
/// TS 的文案是函数（`(n) => ...`），同一参数可能被引用多次，
/// 所以用**全部替换**而不是只替换第一处。
pub fn format_pptx_label(template: &str, placeholder: &str, value: &str) -> String {
    template.replace(placeholder, value)
}

/// `labels.thumbnail(pageNumber)`。
pub fn format_pptx_thumbnail_label(labels: &PptxPreviewViewerLabels, page_number: usize) -> String {
    format_pptx_label(&labels.thumbnail, "{n}", &page_number.to_string())
}

/// `labels.exportPdfSuccess(path)`。
pub fn format_pptx_export_success_label(
    labels: &PptxPreviewViewerLabels,
    path: &str,
) -> String {
    format_pptx_label(&labels.export_pdf_success, "{path}", path)
}

/// `labels.referencedPageMissing(pageNumber)`。
pub fn format_pptx_referenced_page_missing_label(
    labels: &PptxPreviewViewerLabels,
    page_number: usize,
) -> String {
    format_pptx_label(&labels.referenced_page_missing, "{n}", &page_number.to_string())
}

/// `labels.referencedSourceChanged(pageNumber)`。
pub fn format_pptx_referenced_source_changed_label(
    labels: &PptxPreviewViewerLabels,
    page_number: usize,
) -> String {
    format_pptx_label(
        &labels.referenced_source_changed,
        "{n}",
        &page_number.to_string(),
    )
}

// ---- 纯逻辑 ----

/// `clampPresentationPageNumber`。
///
/// `pageCount` 先 `max(1, floor(...))`；`target` 非有限 → `1`；
/// 否则 `min(max(1, round(target)), safePageCount)`。
pub fn clamp_presentation_page_number(target: f64, page_count: f64) -> usize {
    let safe_page_count = if page_count.is_nan() {
        // `Math.max(1, NaN)` = NaN，后续比较全为 false；这里直接退回 1
        1.0
    } else {
        1.0_f64.max(page_count.floor())
    };
    if !target.is_finite() {
        return 1;
    }
    // `Math.round` 半数向 +∞
    let rounded = if target.is_nan() {
        f64::NAN
    } else {
        (target + 0.5).floor()
    };
    1.0_f64.max(rounded).min(safe_page_count) as usize
}

/// `clampPresentationZoomPercent`。
pub fn clamp_presentation_zoom_percent(target: f64) -> i64 {
    if !target.is_finite() {
        return 100;
    }
    let rounded = (target + 0.5).floor();
    (PPTX_MIN_ZOOM_PERCENT as f64)
        .max(rounded)
        .min(PPTX_MAX_ZOOM_PERCENT as f64) as i64
}

/// 主视图的缩放倍率（TS 的 `mainScale` memo）。
///
/// 视口没量到尺寸（宽或高为 0）→ `1`。
/// 否则 `min((w-32)/pw, (h-32)/ph) * (zoomPercent / 100)`。
///
/// 注意 `Math.max(1, viewportSize.width - 32)`：视口比留白还小时按 `1` 算，
/// 避免出现 `0/…` 或负数倍率。
pub fn pptx_main_scale(
    page_size: PresentationSize,
    viewport_width: f64,
    viewport_height: f64,
    zoom_percent: i64,
) -> f64 {
    if viewport_width == 0.0 || viewport_height == 0.0 {
        return 1.0;
    }
    let fit_width = 1.0_f64.max(viewport_width - PPTX_VIEWPORT_PADDING_PX);
    let fit_height = 1.0_f64.max(viewport_height - PPTX_VIEWPORT_PADDING_PX);
    let fit_scale = (fit_width / page_size.w).min(fit_height / page_size.h);
    fit_scale * (zoom_percent as f64 / 100.0)
}

/// 缩略图的缩放倍率（`width > 0 ? width / pageSize.width : 0`）。
pub fn pptx_thumbnail_scale(content_width: f64, page_width: f64) -> f64 {
    if content_width > 0.0 {
        content_width / page_width
    } else {
        0.0
    }
}

/// `Math.max(0, button.clientWidth - 16)`。
pub fn pptx_thumbnail_content_width(button_width: f64) -> f64 {
    0.0_f64.max(button_width - PPTX_THUMBNAIL_PADDING_PX)
}

/// 根容器 `keydown` 的语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PptxViewerKeyAction {
    None,
    PreviousPage,
    NextPage,
    /// 选择态 + 正在写评论：只回退到 AI 编辑条。
    ClearAiEditDraft,
    /// 退出元素选择。
    ExitSelection,
}

/// 键盘事件的目标类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PptxKeyTarget {
    Input,
    Textarea,
    ContentEditable,
    Other,
}

impl PptxKeyTarget {
    /// 是否会豁免按键（见模块移植点 1）。
    pub fn is_editable(&self) -> bool {
        matches!(
            self,
            PptxKeyTarget::Input | PptxKeyTarget::Textarea | PptxKeyTarget::ContentEditable
        )
    }
}

/// `handleKeyDown`。
///
/// 顺序：可编辑目标早退 → Esc（仅选择态）→ 左 / 右方向键。
pub fn resolve_pptx_viewer_key_action(
    key: Key,
    target: PptxKeyTarget,
    selection_mode: bool,
    has_ai_edit_draft: bool,
) -> PptxViewerKeyAction {
    if target.is_editable() {
        return PptxViewerKeyAction::None;
    }
    if matches!(key, Key::Escape) && selection_mode {
        if has_ai_edit_draft {
            return PptxViewerKeyAction::ClearAiEditDraft;
        }
        return PptxViewerKeyAction::ExitSelection;
    }
    match key {
        Key::ArrowLeft => PptxViewerKeyAction::PreviousPage,
        Key::ArrowRight => PptxViewerKeyAction::NextPage,
        _ => PptxViewerKeyAction::None,
    }
}

/// 加载阶段（TS 的三段早退 + 正常态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PptxLoadStage {
    Loading,
    Error,
    NoSlides,
    Ready,
}

/// `if (loadError) … if (!document) … if (pageCount === 0) …` 的顺序。
pub fn resolve_pptx_load_stage(load_error: bool, document_loaded: bool, page_count: usize) -> PptxLoadStage {
    if load_error {
        return PptxLoadStage::Error;
    }
    if !document_loaded {
        return PptxLoadStage::Loading;
    }
    if page_count == 0 {
        return PptxLoadStage::NoSlides;
    }
    PptxLoadStage::Ready
}

/// 元素引用跳转请求。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PptxReferenceNavigationRequest {
    pub request_id: String,
    pub page_index: i64,
    pub expected_source_fingerprint: String,
}

/// 元素引用跳转的判定结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PptxReferenceNavigationDecision {
    /// 什么都不做（没有请求 / 未就绪 / 同一个请求已应用过）。
    Ignore,
    /// `pageIndex` 越界或不是整数 → 提示「引用页不存在」。
    MissingPage { page_number: i64 },
    Navigate {
        page_number: usize,
        /// 指纹不匹配 → 额外提示「引用源已变化」。
        source_changed: bool,
    },
}

/// `referenceNavigation` effect 的判定。
///
/// 注意：**只要 `referenceNavigation` 存在就会先清空选择态**，
/// 与「是否真的跳转」无关（见 TS 里那段注释：旧选择浮层继续存在会让用户
/// 误以为引用页定位同时恢复了元素）。所以调用方要在
/// `decision != Ignore` 与 `== Ignore` 两种情况下都执行清空 ——
/// 本函数只负责「跳不跳、跳去哪」。
pub fn resolve_pptx_reference_navigation(
    reference: Option<&PptxReferenceNavigationRequest>,
    ready: bool,
    document_loaded: bool,
    applied_request_id: Option<&str>,
    page_count: usize,
    source_fingerprint: &str,
) -> PptxReferenceNavigationDecision {
    let Some(reference) = reference else {
        return PptxReferenceNavigationDecision::Ignore;
    };
    if !ready || !document_loaded {
        return PptxReferenceNavigationDecision::Ignore;
    }
    if applied_request_id == Some(reference.request_id.as_str()) {
        return PptxReferenceNavigationDecision::Ignore;
    }
    let page_number = reference.page_index + 1;
    if reference.page_index < 0 || reference.page_index as usize >= page_count {
        return PptxReferenceNavigationDecision::MissingPage { page_number };
    }
    PptxReferenceNavigationDecision::Navigate {
        page_number: page_number as usize,
        source_changed: source_fingerprint != reference.expected_source_fingerprint,
    }
}

/// `Number.parseInt(pageInput.trim(), 10)` 的语义（同 `pdf_viewer`）。
pub fn parse_pptx_page_input(value: &str) -> Option<i64> {
    let trimmed = value.trim_start();
    let mut chars = trimmed.chars().peekable();
    let negative = match chars.peek() {
        Some('+') => {
            chars.next();
            false
        }
        Some('-') => {
            chars.next();
            true
        }
        _ => false,
    };
    let digits: String = chars.take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let magnitude: f64 = digits.parse().unwrap_or(f64::INFINITY);
    Some(if negative { -magnitude } else { magnitude } as i64)
}

/// `commitPageInput` 的结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PptxPageInputCommit {
    /// 解析失败 → 输入框回滚到当前页码。
    Revert { page_input: String },
    /// 解析成功 → 跳到这一页。
    Navigate { page_number: usize },
}

/// `commitPageInput`：解析失败回滚输入框，成功走 `goToPage`。
///
/// 返回 `None` 表示文档还没加载完（`goToPage` 早退）。
pub fn commit_pptx_page_input(
    page_input: &str,
    current_page: usize,
    document_loaded: bool,
    page_count: usize,
) -> Option<PptxPageInputCommit> {
    let Some(parsed) = parse_pptx_page_input(page_input) else {
        return Some(PptxPageInputCommit::Revert {
            page_input: current_page.to_string(),
        });
    };
    if !document_loaded || page_count == 0 {
        return None;
    }
    Some(PptxPageInputCommit::Navigate {
        page_number: clamp_presentation_page_number(parsed as f64, page_count as f64),
    })
}

// ---- 样式 ----

/// 缩略图按钮类名。
pub fn pptx_thumbnail_class(selected: bool) -> String {
    cn(&[
        PPTX_THUMBNAIL_BUTTON_BASE,
        if selected {
            PPTX_THUMBNAIL_BUTTON_SELECTED
        } else {
            PPTX_THUMBNAIL_BUTTON_IDLE
        },
    ])
}

/// `aria-current={selected ? "page" : undefined}`。
pub fn pptx_thumbnail_aria_current(selected: bool) -> Option<&'static str> {
    selected.then_some("page")
}

/// 缩略图内框的宽高比样式（`aspectRatio: "w / h"`）。
pub fn pptx_thumbnail_aspect_ratio_style(page_size: PresentationSize) -> String {
    format!("aspect-ratio: {} / {};", page_size.w, page_size.h)
}

/// 页面外框样式（`width: w * scale; height: h * scale`）。
pub fn pptx_page_frame_style(page_size: PresentationSize, scale: f64) -> String {
    format!(
        "width: {}px; height: {}px;",
        page_size.w * scale,
        page_size.h * scale
    )
}

/// 渲染面样式：原始尺寸 + `transform: scale(scale)`。
pub fn pptx_render_surface_style(page_size: PresentationSize, scale: f64) -> String {
    format!(
        "width: {}px; height: {}px; transform: scale({});",
        page_size.w, page_size.h, scale
    )
}

/// 渲染面的类名。
///
/// 见模块移植点 4：只有「可交互 且（非选择态 或 已开启文字划选）」才放行指针。
pub fn pptx_render_surface_class(
    interactive: bool,
    selection_mode: bool,
    text_selection_enabled: bool,
) -> String {
    let pointer = if interactive && (!selection_mode || text_selection_enabled) {
        "pointer-events-auto"
    } else {
        "pointer-events-none"
    };
    cn(&[
        "absolute inset-0",
        pointer,
        if text_selection_enabled {
            "select-text"
        } else {
            ""
        },
    ])
}

/// 元素浮层按钮的类名。
pub fn pptx_element_button_class(selected: bool, text_selection_enabled: bool) -> String {
    cn(&[
        PPTX_ELEMENT_BUTTON_BASE,
        if text_selection_enabled {
            "pointer-events-none"
        } else {
            "pointer-events-auto"
        },
        if selected {
            PPTX_ELEMENT_BUTTON_SELECTED
        } else {
            PPTX_ELEMENT_BUTTON_IDLE
        },
    ])
}

/// 元素浮层的 `zIndex`（见模块移植点 3）。
pub fn pptx_element_overlay_z_index(element: &PresentationPageElement) -> i64 {
    element.z_index * PPTX_SELECTION_OVERLAY_Z_INDEX_FACTOR
        + if element.node_type == crate::presentation_element_model::PresentationElementNodeType::TableCell {
            1
        } else {
            0
        }
}

/// 元素浮层的定位样式。
pub fn pptx_element_overlay_style(element: &PresentationPageElement) -> String {
    format!(
        "left: {}px; top: {}px; width: {}px; height: {}px; z-index: {};",
        element.bounds.x,
        element.bounds.y,
        element.bounds.width,
        element.bounds.height,
        pptx_element_overlay_z_index(element)
    )
}

/// React 的 `key={elementKey}`（`[nodeId, nodeType, rowIndex ?? "", cellIndex ?? ""].join(":")`）。
pub fn pptx_element_key(element: &PresentationPageElement) -> String {
    format!(
        "{}:{}:{}:{}",
        element.node_id,
        element.node_type.as_str(),
        element
            .row_index
            .map(|value| value.to_string())
            .unwrap_or_default(),
        element
            .cell_index
            .map(|value| value.to_string())
            .unwrap_or_default()
    )
}

/// `aria-label={element.text || element.nodeName || element.nodeType}`。
pub fn pptx_element_aria_label(element: &PresentationPageElement) -> String {
    element
        .text
        .as_ref()
        .filter(|text| !text.is_empty())
        .cloned()
        .unwrap_or_else(|| {
            if element.node_name.is_empty() {
                element.node_type.as_str().to_string()
            } else {
                element.node_name.clone()
            }
        })
}

/// 文字划选是否开启（真值判断：`selectedElement?.text?.trim()`）。
pub fn pptx_text_selection_enabled(
    selection_mode: bool,
    selected_element: Option<&PresentationPageElement>,
) -> bool {
    selection_mode
        && selected_element
            .and_then(|element| element.text.as_ref())
            .is_some_and(|text| !text.trim().is_empty())
}

/// `toPresentationPoint`：视口坐标 → 页面坐标。
///
/// 渲染面尺寸为 0（还没布局）→ `None`。
pub fn to_presentation_point(
    surface: PresentationElementBounds,
    page_size: PresentationSize,
    client_x: f64,
    client_y: f64,
) -> Option<PresentationPoint> {
    if surface.width <= 0.0 || surface.height <= 0.0 {
        return None;
    }
    Some(PresentationPoint {
        x: ((client_x - surface.x) / surface.width) * page_size.w,
        y: ((client_y - surface.y) / surface.height) * page_size.h,
    })
}

// ---- 导出文件名 ----

/// `suggestedPdfName`：把 `.ppt` / `.pptx` / `.pps` / `.ppsx` 换成 `.pdf`。
///
/// `fileName.split(/[\\/]/).pop()?.trim()` —— 空串（含「以分隔符结尾」）
/// 会落到 `"presentation.pdf"`。
pub fn suggested_pptx_pdf_name(file_name: Option<&str>) -> String {
    let base = file_name
        .map(|name| {
            name.split(['\\', '/'])
                .next_back()
                .unwrap_or("")
                .trim()
                .to_string()
        })
        .unwrap_or_default();
    if base.is_empty() {
        return "presentation.pdf".to_string();
    }
    match strip_ppt_extension(&base) {
        Some(stem) => format!("{stem}.pdf"),
        None => format!("{base}.pdf"),
    }
}

/// `\.(pptx?|ppsx?)$/i` 的匹配：命中则返回去掉后缀的部分。
///
/// 只在**结尾**匹配，所以 `"a.pptx.pptx"` 去的是最后一段。
fn strip_ppt_extension(base: &str) -> Option<String> {
    let lower = base.to_lowercase();
    for suffix in [".pptx", ".ppsx", ".ppt", ".pps"] {
        if lower.ends_with(suffix) {
            return Some(base[..base.len() - suffix.len()].to_string());
        }
    }
    None
}

// ---- 组件 ----

/// 视口尺寸（宿主用 `ResizeObserver` 测出来）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PptxViewportSize {
    pub width: f64,
    pub height: f64,
}

/// 单个缩略图。
///
/// 只负责外壳（按钮 + 宽高比内框 + 页码），画布由 `children` 注入。
#[component]
pub fn PptxThumbnail(
    /// 0 基的页下标。
    page_index: usize,
    #[props(default)] selected: bool,
    labels: PptxPreviewViewerLabels,
    page_size: PresentationSize,
    /// 内框可用宽度（TS 用 `ResizeObserver` 在组件内测，见模块差异 2）。
    #[props(default)] content_width: f64,
    #[props(default)] onselect: EventHandler<()>,
    /// 注入的缩略图渲染结果。
    children: Element,
) -> Element {
    let scale = pptx_thumbnail_scale(content_width, page_size.w);
    let page_number = page_index + 1;
    rsx! {
        button {
            r#type: "button",
            "aria-current": pptx_thumbnail_aria_current(selected),
            "aria-label": format_pptx_thumbnail_label(&labels, page_number),
            class: pptx_thumbnail_class(selected),
            onclick: move |_| onselect.call(()),
            div {
                class: PPTX_THUMBNAIL_FRAME,
                style: pptx_thumbnail_aspect_ratio_style(page_size),
                if scale > 0.0 {
                    {children}
                }
            }
            span { class: PPTX_THUMBNAIL_NUMBER, "{page_number}" }
        }
    }
}

/// 提交输入框（Enter / blur）。
///
/// TS 里 `commitPageInput` 直接改内部 state；Rust 侧把结果派发出去，
/// 由宿主决定写回 `page_input` 还是跳页。
fn dispatch_pptx_page_commit(
    text: &str,
    current_page: usize,
    document_loaded: bool,
    page_count: usize,
    onpageinputchange: EventHandler<String>,
    onnavigate: EventHandler<usize>,
) {
    match commit_pptx_page_input(text, current_page, document_loaded, page_count) {
        None => {}
        Some(PptxPageInputCommit::Revert { page_input }) => onpageinputchange.call(page_input),
        Some(PptxPageInputCommit::Navigate { page_number }) => onnavigate.call(page_number),
    }
}

/// PPTX 预览外壳。
///
/// **不含**幻灯片画布本身 —— 主页面由 `children` 注入，缩略图列由
/// `thumbnails` 注入（对应 TS 里的 `pptxRendererPreviewEngine`）。
#[component]
pub fn PptxPreviewViewer(
    #[props(default, into)] class: String,
    labels: PptxPreviewViewerLabels,
    /// 文档元信息：总页数。
    #[props(default)] page_count: usize,
    /// 文档元信息：页面尺寸。
    page_size: PresentationSize,
    /// 文档是否已加载（`None` 态的等价物）。
    #[props(default)] document_loaded: bool,
    #[props(default)] load_error: bool,
    #[props(default = 1)] page_number: usize,
    #[props(default = "1".to_string(), into)] page_input: String,
    #[props(default = 100)] zoom_percent: i64,
    #[props(default)] selection_mode: bool,
    /// 是否正在写评论（`aiEditDraft !== null`）。
    #[props(default)] ai_edit_editing: bool,
    /// 是否处于元素选择态且已选中元素（决定浮层是否展开）。
    #[props(default)] selection_open: bool,
    #[props(default)] adding_reference: bool,
    #[props(default)] exporting_pdf: bool,
    /// 平台是否支持导出 PDF（`printPageToPdf && saveFile`）。
    #[props(default)] can_export_pdf: bool,
    /// 是否提供元素引用能力（决定「选择元素」按钮是否出现）。
    #[props(default)] reference_enabled: bool,
    #[props(default)] viewport_size: PptxViewportSize,
    /// 注入的缩略图列内容（宿主按页数自行渲染）。
    thumbnails: Element,
    /// 注入的主页面渲染器。
    children: Element,
    #[props(default)] onpageinputchange: EventHandler<String>,
    /// `goToPage` 之后的新页码。
    #[props(default)] onnavigate: EventHandler<usize>,
    #[props(default)] onzoomchange: EventHandler<i64>,
    #[props(default)] onkeyaction: EventHandler<PptxViewerKeyAction>,
    #[props(default)] onselectelement: EventHandler<PresentationPageElement>,
    #[props(default)] on_toggle_selection: EventHandler<()>,
    #[props(default)] on_ai_edit: EventHandler<()>,
    #[props(default)] on_cancel_ai_edit: EventHandler<()>,
    #[props(default)] on_add_to_conversation: EventHandler<String>,
    #[props(default)] on_exit_selection: EventHandler<()>,
    #[props(default)] on_export_pdf: EventHandler<()>,
) -> Element {
    let stage = resolve_pptx_load_stage(load_error, document_loaded, page_count);
    match stage {
        PptxLoadStage::Error => {
            return rsx! { div { class: PPTX_ERROR_MESSAGE, "{labels.load_error}" } };
        }
        PptxLoadStage::Loading => {
            return rsx! { div { class: PPTX_SUBTLE_MESSAGE, "{labels.loading}" } };
        }
        PptxLoadStage::NoSlides => {
            return rsx! { div { class: PPTX_SUBTLE_MESSAGE, "{labels.no_slides}" } };
        }
        PptxLoadStage::Ready => {}
    }

    let main_scale = pptx_main_scale(
        page_size,
        viewport_size.width,
        viewport_size.height,
        zoom_percent,
    );
    let selection_button_label = if selection_mode {
        labels.exit_element_selection.clone()
    } else {
        labels.select_element.clone()
    };
    let export_button_label = if exporting_pdf {
        labels.exporting_pdf.clone()
    } else {
        labels.export_pdf.clone()
    };
    let enter_text = page_input.clone();
    let blur_text = page_input.clone();
    let can_go_previous = page_number > 1;
    let can_go_next = page_number < page_count;
    let can_zoom_out = zoom_percent > PPTX_MIN_ZOOM_PERCENT;
    let can_zoom_in = zoom_percent < PPTX_MAX_ZOOM_PERCENT;
    let current_page = page_number;
    let total_pages = page_count;
    let input_class = input_classes(InputSize::Default, PPTX_PAGE_INPUT);

    rsx! {
        div {
            tabindex: "0",
            class: cn(&[PPTX_VIEWER_BASE, &class]),
            "data-slot": "pptx-preview-viewer",
            onkeydown: move |e: KeyboardEvent| {
                // 焦点在浮层的 Textarea 里时，事件也会冒泡到这里；
                // 由浮层自己 stop_propagation，所以这里固定按 Other 处理。
                let action = resolve_pptx_viewer_key_action(
                    e.key(),
                    PptxKeyTarget::Other,
                    selection_mode,
                    ai_edit_editing,
                );
                if action == PptxViewerKeyAction::None {
                    return;
                }
                e.prevent_default();
                onkeyaction.call(action);
            },

            aside {
                "aria-label": labels.thumbnails.clone(),
                class: PPTX_THUMBNAIL_ASIDE,
                div { class: PPTX_THUMBNAIL_LIST, {thumbnails} }
            }

            div { class: PPTX_MAIN_COLUMN,
                div { class: PPTX_MAIN_VIEWPORT, "data-slot": "pptx-preview-viewport",
                    div { class: PPTX_MAIN_GRID,
                        PptxSelectionActionBar {
                            open: selection_mode && selection_open,
                            label: labels.ai_edit.clone(),
                            comment_placeholder: labels.comment_placeholder.clone(),
                            cancel_label: labels.cancel_ai_edit.clone(),
                            add_to_conversation_label: labels.add_to_conversation.clone(),
                            editing: ai_edit_editing,
                            disabled: adding_reference,
                            on_ai_edit: move |_| on_ai_edit.call(()),
                            on_cancel_ai_edit: move |_| on_cancel_ai_edit.call(()),
                            on_add_to_conversation: move |comment: String| on_add_to_conversation.call(comment),
                            on_exit_selection: move |_| on_exit_selection.call(()),
                            div {
                                class: PPTX_PAGE_FRAME,
                                style: pptx_page_frame_style(page_size, main_scale),
                                {children}
                            }
                        }
                    }
                }

                div { class: PPTX_TOOLBAR, "data-slot": "pptx-preview-toolbar",
                    if reference_enabled {
                        Button {
                            variant: if selection_mode { ButtonVariant::Secondary } else { ButtonVariant::Ghost },
                            size: ButtonSize::Sm,
                            aria_label: selection_button_label.clone(),
                            onclick: move |_| on_toggle_selection.call(()),
                            IconMousePointer2 { class: "size-4", aria_hidden: true }
                            "{selection_button_label}"
                        }
                    }
                    Button {
                        variant: ButtonVariant::Ghost,
                        size: ButtonSize::IconMd,
                        aria_label: labels.previous_page.clone(),
                        disabled: !can_go_previous,
                        onclick: move |_| onkeyaction.call(PptxViewerKeyAction::PreviousPage),
                        IconChevronLeft { aria_hidden: true }
                    }
                    input {
                        r#type: "text",
                        "inputmode": "numeric",
                        "aria-label": labels.page_input.clone(),
                        value: "{page_input}",
                        class: input_class,
                        oninput: move |e: FormEvent| onpageinputchange.call(e.value()),
                        onkeydown: move |e: KeyboardEvent| {
                            if matches!(e.key(), Key::Enter) {
                                e.prevent_default();
                                dispatch_pptx_page_commit(
                                    &enter_text,
                                    current_page,
                                    document_loaded,
                                    total_pages,
                                    onpageinputchange,
                                    onnavigate,
                                );
                            }
                            e.stop_propagation();
                        },
                        onblur: move |_| {
                            dispatch_pptx_page_commit(
                                &blur_text,
                                current_page,
                                document_loaded,
                                total_pages,
                                onpageinputchange,
                                onnavigate,
                            );
                        },
                    }
                    span { class: PPTX_PAGE_COUNTER, "/ {total_pages}" }
                    Button {
                        variant: ButtonVariant::Ghost,
                        size: ButtonSize::IconMd,
                        aria_label: labels.next_page.clone(),
                        disabled: !can_go_next,
                        onclick: move |_| onkeyaction.call(PptxViewerKeyAction::NextPage),
                        IconChevronRight { aria_hidden: true }
                    }
                    div { class: PPTX_TOOLBAR_DIVIDER, "aria-hidden": "true" }
                    Button {
                        variant: ButtonVariant::Ghost,
                        size: ButtonSize::IconMd,
                        aria_label: labels.zoom_out.clone(),
                        disabled: !can_zoom_out,
                        onclick: move |_| {
                            onzoomchange.call(clamp_presentation_zoom_percent(
                                (zoom_percent - PPTX_ZOOM_STEP_PERCENT) as f64,
                            ));
                        },
                        IconZoomOut { aria_hidden: true }
                    }
                    span { class: PPTX_ZOOM_LABEL, "{zoom_percent}%" }
                    Button {
                        variant: ButtonVariant::Ghost,
                        size: ButtonSize::IconMd,
                        aria_label: labels.zoom_in.clone(),
                        disabled: !can_zoom_in,
                        onclick: move |_| {
                            onzoomchange.call(clamp_presentation_zoom_percent(
                                (zoom_percent + PPTX_ZOOM_STEP_PERCENT) as f64,
                            ));
                        },
                        IconZoomIn { aria_hidden: true }
                    }
                    if can_export_pdf {
                        div { class: PPTX_TOOLBAR_DIVIDER, "aria-hidden": "true" }
                        Button {
                            variant: ButtonVariant::Ghost,
                            size: ButtonSize::IconMd,
                            aria_label: export_button_label.clone(),
                            disabled: exporting_pdf,
                            onclick: move |_| on_export_pdf.call(()),
                            if exporting_pdf {
                                IconLoader2 { class: "animate-spin", aria_hidden: true }
                            } else {
                                IconFileDown { aria_hidden: true }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presentation_element_model::PresentationElementNodeType;

    fn labels() -> PptxPreviewViewerLabels {
        PptxPreviewViewerLabels {
            loading: "Loading presentation…".to_string(),
            load_error: "Failed to load presentation".to_string(),
            no_slides: "No slides".to_string(),
            previous_page: "Previous page".to_string(),
            next_page: "Next page".to_string(),
            page_input: "Page number".to_string(),
            zoom_in: "Zoom in".to_string(),
            zoom_out: "Zoom out".to_string(),
            thumbnails: "Slides".to_string(),
            thumbnail: "Slide {n}".to_string(),
            export_pdf: "Export as PDF".to_string(),
            exporting_pdf: "Exporting…".to_string(),
            export_pdf_success: "Saved to {path}".to_string(),
            export_pdf_failed: "Export failed".to_string(),
            select_element: "Select element".to_string(),
            exit_element_selection: "Exit selection".to_string(),
            ai_edit: "AI edit".to_string(),
            comment_placeholder: "Add a comment".to_string(),
            cancel_ai_edit: "Cancel".to_string(),
            add_to_conversation: "Add to chat".to_string(),
            referenced_page_missing: "Page {n} not found".to_string(),
            referenced_source_changed: "Page {n} source changed".to_string(),
        }
    }

    fn page_size() -> PresentationSize {
        PresentationSize {
            w: 960.0,
            h: 540.0,
        }
    }

    fn element(z_index: i64, node_type: PresentationElementNodeType) -> PresentationPageElement {
        PresentationPageElement {
            slide_index: 0,
            slide_part: "slide1.xml".to_string(),
            node_id: "n1".to_string(),
            node_path: None,
            node_name: "Title 1".to_string(),
            node_type,
            text: None,
            bounds: PresentationElementBounds {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 50.0,
            },
            row_index: None,
            cell_index: None,
            z_index,
        }
    }

    // ---- 常量 ----

    #[test]
    fn zoom_constants_match_the_ts_module() {
        assert_eq!(PPTX_MIN_ZOOM_PERCENT, 25);
        assert_eq!(PPTX_MAX_ZOOM_PERCENT, 300);
        assert_eq!(PPTX_ZOOM_STEP_PERCENT, 25);
        assert_eq!(PPTX_VIEWPORT_PADDING_PX, 32.0);
        assert_eq!(PPTX_THUMBNAIL_PADDING_PX, 16.0);
        assert_eq!(PPTX_SELECTION_OVERLAY_Z_INDEX_FACTOR, 2);
    }

    // ---- 文案模板 ----

    #[test]
    fn templates_replace_every_occurrence() {
        assert_eq!(format_pptx_label("Page {n} of {n}", "{n}", "3"), "Page 3 of 3");
        // 没有占位符时原样返回
        assert_eq!(format_pptx_label("no placeholder", "{n}", "3"), "no placeholder");
    }

    #[test]
    fn label_helpers_fill_the_right_placeholder() {
        let l = labels();
        assert_eq!(format_pptx_thumbnail_label(&l, 2), "Slide 2");
        assert_eq!(format_pptx_export_success_label(&l, "/tmp/a.pdf"), "Saved to /tmp/a.pdf");
        assert_eq!(format_pptx_referenced_page_missing_label(&l, 7), "Page 7 not found");
        assert_eq!(
            format_pptx_referenced_source_changed_label(&l, 7),
            "Page 7 source changed"
        );
    }

    // ---- 页码夹取 ----

    #[test]
    fn page_number_clamps_into_range() {
        assert_eq!(clamp_presentation_page_number(0.0, 10.0), 1);
        assert_eq!(clamp_presentation_page_number(-5.0, 10.0), 1);
        assert_eq!(clamp_presentation_page_number(999.0, 10.0), 10);
        assert_eq!(clamp_presentation_page_number(4.0, 10.0), 4);
    }

    #[test]
    fn page_number_rounds_half_toward_positive_infinity() {
        assert_eq!(clamp_presentation_page_number(1.5, 10.0), 2);
        assert_eq!(clamp_presentation_page_number(1.4, 10.0), 1);
        assert_eq!(clamp_presentation_page_number(2.5, 10.0), 3);
    }

    #[test]
    fn a_non_finite_target_falls_back_to_the_first_page() {
        assert_eq!(clamp_presentation_page_number(f64::NAN, 10.0), 1);
        assert_eq!(clamp_presentation_page_number(f64::INFINITY, 10.0), 1);
        assert_eq!(clamp_presentation_page_number(f64::NEG_INFINITY, 10.0), 1);
    }

    /// `pageCount` 会先被 `max(1, floor(...))` 兜住。
    #[test]
    fn the_page_count_is_floored_and_at_least_one() {
        assert_eq!(clamp_presentation_page_number(5.0, 3.9), 3);
        assert_eq!(clamp_presentation_page_number(5.0, 0.0), 1);
        assert_eq!(clamp_presentation_page_number(5.0, -10.0), 1);
        assert_eq!(clamp_presentation_page_number(5.0, f64::NAN), 1);
    }

    // ---- 缩放夹取 ----

    #[test]
    fn zoom_percent_clamps_between_the_bounds() {
        assert_eq!(clamp_presentation_zoom_percent(10.0), PPTX_MIN_ZOOM_PERCENT);
        assert_eq!(clamp_presentation_zoom_percent(999.0), PPTX_MAX_ZOOM_PERCENT);
        assert_eq!(clamp_presentation_zoom_percent(100.0), 100);
        assert_eq!(clamp_presentation_zoom_percent(150.0), 150);
    }

    #[test]
    fn a_non_finite_zoom_falls_back_to_one_hundred() {
        assert_eq!(clamp_presentation_zoom_percent(f64::NAN), 100);
        assert_eq!(clamp_presentation_zoom_percent(f64::INFINITY), 100);
        assert_eq!(clamp_presentation_zoom_percent(f64::NEG_INFINITY), 100);
    }

    #[test]
    fn zoom_percent_rounds_like_math_round() {
        assert_eq!(clamp_presentation_zoom_percent(149.5), 150);
        assert_eq!(clamp_presentation_zoom_percent(149.4), 149);
    }

    /// 步进 25：从 100 往上 / 往下各走一步都落在界内。
    #[test]
    fn a_zoom_step_moves_by_twenty_five() {
        assert_eq!(
            clamp_presentation_zoom_percent((100 - PPTX_ZOOM_STEP_PERCENT) as f64),
            75
        );
        assert_eq!(
            clamp_presentation_zoom_percent((100 + PPTX_ZOOM_STEP_PERCENT) as f64),
            125
        );
        // 已经在边界上时不动
        assert_eq!(
            clamp_presentation_zoom_percent((PPTX_MIN_ZOOM_PERCENT - PPTX_ZOOM_STEP_PERCENT) as f64),
            PPTX_MIN_ZOOM_PERCENT
        );
        assert_eq!(
            clamp_presentation_zoom_percent((PPTX_MAX_ZOOM_PERCENT + PPTX_ZOOM_STEP_PERCENT) as f64),
            PPTX_MAX_ZOOM_PERCENT
        );
    }

    // ---- 主视图倍率 ----

    #[test]
    fn main_scale_is_one_before_the_viewport_is_measured() {
        assert_eq!(pptx_main_scale(page_size(), 0.0, 500.0, 100), 1.0);
        assert_eq!(pptx_main_scale(page_size(), 500.0, 0.0, 100), 1.0);
        assert_eq!(pptx_main_scale(page_size(), 0.0, 0.0, 200), 1.0);
    }

    /// 取宽高两个方向的**较小**值，保证整页放得下。
    #[test]
    fn main_scale_fits_the_whole_page() {
        // 视口 992×572 → (992-32)/960 = 1.0、(572-32)/540 = 1.002… → 取 1.0
        let scale = pptx_main_scale(page_size(), 992.0, 572.0, 100);
        assert!((scale - 1.0).abs() < 1e-9, "{scale}");

        // 高受限：视口 2000×300 → 宽 2.05、高 0.4962… → 取高
        let tall = pptx_main_scale(page_size(), 2000.0, 300.0, 100);
        assert!((tall - (300.0 - 32.0) / 540.0).abs() < 1e-9, "{tall}");
    }

    /// `Math.max(1, viewportSize - 32)`：视口比留白还小时按 1 算。
    #[test]
    fn main_scale_never_goes_negative_or_zero_on_a_tiny_viewport() {
        let scale = pptx_main_scale(page_size(), 10.0, 10.0, 100);
        // (1/960).min(1/540) = 1/960
        assert!((scale - 1.0 / 960.0).abs() < 1e-12, "{scale}");
        assert!(scale > 0.0);
    }

    #[test]
    fn main_scale_multiplies_by_the_zoom_percent() {
        let base = pptx_main_scale(page_size(), 992.0, 572.0, 100);
        let doubled = pptx_main_scale(page_size(), 992.0, 572.0, 200);
        assert!((doubled - base * 2.0).abs() < 1e-9);
        let quarter = pptx_main_scale(page_size(), 992.0, 572.0, 25);
        assert!((quarter - base * 0.25).abs() < 1e-9);
    }

    // ---- 缩略图尺寸 ----

    #[test]
    fn thumbnail_scale_needs_a_positive_width() {
        assert_eq!(pptx_thumbnail_scale(0.0, 960.0), 0.0);
        assert_eq!(pptx_thumbnail_scale(-5.0, 960.0), 0.0);
        assert_eq!(pptx_thumbnail_scale(96.0, 960.0), 0.1);
    }

    #[test]
    fn thumbnail_content_width_subtracts_the_padding() {
        assert_eq!(pptx_thumbnail_content_width(112.0), 96.0);
        // 按钮比内边距还窄 → 夹到 0
        assert_eq!(pptx_thumbnail_content_width(10.0), 0.0);
        assert_eq!(pptx_thumbnail_content_width(16.0), 0.0);
    }

    // ---- 键盘 ----

    #[test]
    fn editable_targets_are_exempt_from_every_key() {
        for target in [
            PptxKeyTarget::Input,
            PptxKeyTarget::Textarea,
            PptxKeyTarget::ContentEditable,
        ] {
            assert!(target.is_editable());
            assert_eq!(
                resolve_pptx_viewer_key_action(Key::ArrowLeft, target, true, true),
                PptxViewerKeyAction::None
            );
            assert_eq!(
                resolve_pptx_viewer_key_action(Key::Escape, target, true, false),
                PptxViewerKeyAction::None
            );
        }
        assert!(!PptxKeyTarget::Other.is_editable());
    }

    #[test]
    fn arrows_are_the_only_navigation_keys() {
        assert_eq!(
            resolve_pptx_viewer_key_action(Key::ArrowLeft, PptxKeyTarget::Other, false, false),
            PptxViewerKeyAction::PreviousPage
        );
        assert_eq!(
            resolve_pptx_viewer_key_action(Key::ArrowRight, PptxKeyTarget::Other, false, false),
            PptxViewerKeyAction::NextPage
        );
        assert_eq!(
            resolve_pptx_viewer_key_action(Key::ArrowUp, PptxKeyTarget::Other, false, false),
            PptxViewerKeyAction::None
        );
        assert_eq!(
            resolve_pptx_viewer_key_action(Key::Enter, PptxKeyTarget::Other, false, false),
            PptxViewerKeyAction::None
        );
    }

    /// 移植点 2：Esc 只在选择态下有意义。
    #[test]
    fn escape_is_ignored_outside_selection_mode() {
        assert_eq!(
            resolve_pptx_viewer_key_action(Key::Escape, PptxKeyTarget::Other, false, false),
            PptxViewerKeyAction::None
        );
        // 非选择态下即便有草稿也不处理
        assert_eq!(
            resolve_pptx_viewer_key_action(Key::Escape, PptxKeyTarget::Other, false, true),
            PptxViewerKeyAction::None
        );
    }

    #[test]
    fn escape_clears_the_draft_before_exiting_the_selection() {
        // 选择态 + 有草稿 → 只回退编辑条
        assert_eq!(
            resolve_pptx_viewer_key_action(Key::Escape, PptxKeyTarget::Other, true, true),
            PptxViewerKeyAction::ClearAiEditDraft
        );
        // 选择态 + 无草稿 → 退出选择
        assert_eq!(
            resolve_pptx_viewer_key_action(Key::Escape, PptxKeyTarget::Other, true, false),
            PptxViewerKeyAction::ExitSelection
        );
    }

    /// 选择态下方向键仍然翻页。
    #[test]
    fn arrows_still_work_in_selection_mode() {
        assert_eq!(
            resolve_pptx_viewer_key_action(Key::ArrowLeft, PptxKeyTarget::Other, true, true),
            PptxViewerKeyAction::PreviousPage
        );
    }

    // ---- 加载阶段 ----

    #[test]
    fn load_stage_follows_the_early_return_order() {
        // 出错优先于一切
        assert_eq!(resolve_pptx_load_stage(true, false, 0), PptxLoadStage::Error);
        assert_eq!(resolve_pptx_load_stage(true, true, 5), PptxLoadStage::Error);
        // 未加载
        assert_eq!(resolve_pptx_load_stage(false, false, 0), PptxLoadStage::Loading);
        // 加载了但没页
        assert_eq!(resolve_pptx_load_stage(false, true, 0), PptxLoadStage::NoSlides);
        assert_eq!(resolve_pptx_load_stage(false, true, 3), PptxLoadStage::Ready);
    }

    // ---- 元素引用跳转 ----

    fn request(id: &str, page_index: i64, fingerprint: &str) -> PptxReferenceNavigationRequest {
        PptxReferenceNavigationRequest {
            request_id: id.to_string(),
            page_index,
            expected_source_fingerprint: fingerprint.to_string(),
        }
    }

    #[test]
    fn reference_navigation_ignores_the_absent_case() {
        assert_eq!(
            resolve_pptx_reference_navigation(None, true, true, None, 10, "fp"),
            PptxReferenceNavigationDecision::Ignore
        );
    }

    #[test]
    fn reference_navigation_waits_for_readiness_and_the_document() {
        let req = request("r1", 2, "fp");
        assert_eq!(
            resolve_pptx_reference_navigation(Some(&req), false, true, None, 10, "fp"),
            PptxReferenceNavigationDecision::Ignore
        );
        assert_eq!(
            resolve_pptx_reference_navigation(Some(&req), true, false, None, 10, "fp"),
            PptxReferenceNavigationDecision::Ignore
        );
    }

    #[test]
    fn reference_navigation_is_applied_at_most_once() {
        let req = request("r1", 2, "fp");
        assert_eq!(
            resolve_pptx_reference_navigation(Some(&req), true, true, Some("r1"), 10, "fp"),
            PptxReferenceNavigationDecision::Ignore
        );
        // 换了 requestId 就可以再跳
        assert_eq!(
            resolve_pptx_reference_navigation(Some(&req), true, true, Some("r0"), 10, "fp"),
            PptxReferenceNavigationDecision::Navigate {
                page_number: 3,
                source_changed: false
            }
        );
    }

    #[test]
    fn reference_navigation_reports_a_missing_page() {
        let negative = request("r1", -1, "fp");
        assert_eq!(
            resolve_pptx_reference_navigation(Some(&negative), true, true, None, 10, "fp"),
            PptxReferenceNavigationDecision::MissingPage { page_number: 0 }
        );
        // 等于 pageCount 也越界（下标是 0 基）
        let past_end = request("r2", 10, "fp");
        assert_eq!(
            resolve_pptx_reference_navigation(Some(&past_end), true, true, None, 10, "fp"),
            PptxReferenceNavigationDecision::MissingPage { page_number: 11 }
        );
    }

    /// `pageIndex` 是 0 基，`pageNumber` 是 1 基。
    #[test]
    fn reference_navigation_converts_the_page_index() {
        let first = request("r1", 0, "fp");
        assert_eq!(
            resolve_pptx_reference_navigation(Some(&first), true, true, None, 10, "fp"),
            PptxReferenceNavigationDecision::Navigate {
                page_number: 1,
                source_changed: false
            }
        );
        let last = request("r2", 9, "fp");
        assert_eq!(
            resolve_pptx_reference_navigation(Some(&last), true, true, None, 10, "fp"),
            PptxReferenceNavigationDecision::Navigate {
                page_number: 10,
                source_changed: false
            }
        );
    }

    #[test]
    fn reference_navigation_flags_a_changed_source() {
        let req = request("r1", 4, "old");
        assert_eq!(
            resolve_pptx_reference_navigation(Some(&req), true, true, None, 10, "new"),
            PptxReferenceNavigationDecision::Navigate {
                page_number: 5,
                source_changed: true
            }
        );
    }

    // ---- 输入框提交 ----

    #[test]
    fn page_input_parse_follows_parse_int() {
        assert_eq!(parse_pptx_page_input("12"), Some(12));
        assert_eq!(parse_pptx_page_input("  7 "), Some(7));
        assert_eq!(parse_pptx_page_input("+3"), Some(3));
        assert_eq!(parse_pptx_page_input("-4"), Some(-4));
        assert_eq!(parse_pptx_page_input("12.5"), Some(12));
        assert_eq!(parse_pptx_page_input("abc"), None);
        assert_eq!(parse_pptx_page_input(""), None);
    }

    #[test]
    fn page_input_commit_reverts_when_unparseable() {
        assert_eq!(
            commit_pptx_page_input("abc", 7, true, 20),
            Some(PptxPageInputCommit::Revert {
                page_input: "7".to_string()
            })
        );
    }

    #[test]
    fn page_input_commit_navigates_and_clamps() {
        assert_eq!(
            commit_pptx_page_input("12", 1, true, 20),
            Some(PptxPageInputCommit::Navigate { page_number: 12 })
        );
        assert_eq!(
            commit_pptx_page_input("999", 1, true, 20),
            Some(PptxPageInputCommit::Navigate { page_number: 20 })
        );
        assert_eq!(
            commit_pptx_page_input("-3", 1, true, 20),
            Some(PptxPageInputCommit::Navigate { page_number: 1 })
        );
    }

    #[test]
    fn page_input_commit_is_a_no_op_before_the_document_loads() {
        assert_eq!(commit_pptx_page_input("5", 1, false, 20), None);
        assert_eq!(commit_pptx_page_input("5", 1, true, 0), None);
    }

    // ---- 样式 ----

    #[test]
    fn thumbnail_class_switches_on_selection() {
        let selected = pptx_thumbnail_class(true);
        assert!(selected.contains(PPTX_THUMBNAIL_BUTTON_SELECTED));
        assert!(!selected.contains(PPTX_THUMBNAIL_BUTTON_IDLE));

        let idle = pptx_thumbnail_class(false);
        assert!(idle.contains(PPTX_THUMBNAIL_BUTTON_IDLE));
        assert!(!idle.contains(PPTX_THUMBNAIL_BUTTON_SELECTED));
    }

    #[test]
    fn thumbnail_aria_current_is_only_set_when_selected() {
        assert_eq!(pptx_thumbnail_aria_current(true), Some("page"));
        assert_eq!(pptx_thumbnail_aria_current(false), None);
    }

    #[test]
    fn thumbnail_aspect_ratio_uses_the_page_size() {
        assert_eq!(
            pptx_thumbnail_aspect_ratio_style(page_size()),
            "aspect-ratio: 960 / 540;"
        );
    }

    #[test]
    fn page_frame_style_scales_both_dimensions() {
        assert_eq!(
            pptx_page_frame_style(page_size(), 0.5),
            "width: 480px; height: 270px;"
        );
    }

    #[test]
    fn render_surface_style_keeps_the_intrinsic_size_and_scales() {
        assert_eq!(
            pptx_render_surface_style(page_size(), 0.25),
            "width: 960px; height: 540px; transform: scale(0.25);"
        );
    }

    /// 移植点 4 的四种组合。
    #[test]
    fn render_surface_pointer_events_depend_on_the_mode() {
        // 非交互（缩略图）→ 永远不吃指针
        assert!(pptx_render_surface_class(false, false, false).contains("pointer-events-none"));
        assert!(pptx_render_surface_class(false, true, true).contains("pointer-events-none"));

        // 交互 + 非选择态 → 吃指针（为了点超链接）
        let idle = pptx_render_surface_class(true, false, false);
        assert!(idle.contains("pointer-events-auto"));
        assert!(!idle.contains("select-text"));

        // 交互 + 选择态但没开文字划选 → 不吃指针
        assert!(
            pptx_render_surface_class(true, true, false).contains("pointer-events-none")
        );

        // 交互 + 选择态 + 文字划选 → 吃指针并且可选文本
        let selecting = pptx_render_surface_class(true, true, true);
        assert!(selecting.contains("pointer-events-auto"));
        assert!(selecting.contains("select-text"));
    }

    #[test]
    fn element_button_class_switches_on_both_flags() {
        let selected_text = pptx_element_button_class(true, true);
        assert!(selected_text.contains(PPTX_ELEMENT_BUTTON_SELECTED));
        // 文字划选开启时元素按钮让出指针
        assert!(selected_text.contains("pointer-events-none"));

        let idle = pptx_element_button_class(false, false);
        assert!(idle.contains(PPTX_ELEMENT_BUTTON_IDLE));
        assert!(idle.contains("pointer-events-auto"));
    }

    // ---- 浮层几何 ----

    #[test]
    fn overlay_z_index_doubles_and_prioritises_cells() {
        assert_eq!(
            pptx_element_overlay_z_index(&element(0, PresentationElementNodeType::Shape)),
            0
        );
        assert_eq!(
            pptx_element_overlay_z_index(&element(3, PresentationElementNodeType::Shape)),
            6
        );
        // 单元格 +1
        assert_eq!(
            pptx_element_overlay_z_index(&element(3, PresentationElementNodeType::TableCell)),
            7
        );
        assert_eq!(
            pptx_element_overlay_z_index(&element(0, PresentationElementNodeType::TableCell)),
            1
        );
    }

    #[test]
    fn overlay_style_carries_bounds_and_z_index() {
        assert_eq!(
            pptx_element_overlay_style(&element(2, PresentationElementNodeType::Shape)),
            "left: 10px; top: 20px; width: 100px; height: 50px; z-index: 4;"
        );
    }

    #[test]
    fn element_key_joins_four_segments() {
        let shape = element(0, PresentationElementNodeType::Shape);
        // rowIndex / cellIndex 缺省时是空串，但分隔符仍在
        assert_eq!(pptx_element_key(&shape), "n1:shape::");

        let mut cell = element(0, PresentationElementNodeType::TableCell);
        cell.row_index = Some(1);
        cell.cell_index = Some(2);
        assert_eq!(pptx_element_key(&cell), "n1:table-cell:1:2");
    }

    #[test]
    fn element_aria_label_prefers_text_then_name_then_type() {
        let mut el = element(0, PresentationElementNodeType::Shape);
        // 有 nodeName → 用它
        assert_eq!(pptx_element_aria_label(&el), "Title 1");

        // 有 text → 优先
        el.text = Some("标题".to_string());
        assert_eq!(pptx_element_aria_label(&el), "标题");

        // 空 text 视为没有
        el.text = Some(String::new());
        assert_eq!(pptx_element_aria_label(&el), "Title 1");

        // 名字也空 → 退回 nodeType
        el.node_name = String::new();
        assert_eq!(pptx_element_aria_label(&el), "shape");
    }

    #[test]
    fn text_selection_needs_selection_mode_and_non_blank_text() {
        let mut el = element(0, PresentationElementNodeType::Shape);
        // 非选择态：即使有文本也不开
        el.text = Some("abc".to_string());
        assert!(!pptx_text_selection_enabled(false, Some(&el)));
        assert!(pptx_text_selection_enabled(true, Some(&el)));

        // 空白文本不算
        el.text = Some("   \n ".to_string());
        assert!(!pptx_text_selection_enabled(true, Some(&el)));
        el.text = None;
        assert!(!pptx_text_selection_enabled(true, Some(&el)));
        assert!(!pptx_text_selection_enabled(true, None));
    }

    // ---- 视口 → 页面坐标 ----

    #[test]
    fn a_zero_sized_surface_cannot_be_mapped() {
        let surface = PresentationElementBounds {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 100.0,
        };
        assert_eq!(
            to_presentation_point(surface, page_size(), 10.0, 10.0),
            None
        );
    }

    #[test]
    fn presentation_point_scales_into_page_coordinates() {
        // 渲染面 480×270（页面的一半），左上角在 (10, 20)
        let surface = PresentationElementBounds {
            x: 10.0,
            y: 20.0,
            width: 480.0,
            height: 270.0,
        };
        // 点正好在渲染面中心 → 页面中心
        let center = to_presentation_point(surface, page_size(), 250.0, 155.0).unwrap();
        assert_eq!(center, PresentationPoint { x: 480.0, y: 270.0 });

        // 点正好在渲染面左上角 → 页面原点
        let origin = to_presentation_point(surface, page_size(), 10.0, 20.0).unwrap();
        assert_eq!(origin, PresentationPoint { x: 0.0, y: 0.0 });
    }

    // ---- 导出文件名 ----

    #[test]
    fn suggested_pdf_name_replaces_ppt_extensions() {
        assert_eq!(suggested_pptx_pdf_name(Some("deck.pptx")), "deck.pdf");
        assert_eq!(suggested_pptx_pdf_name(Some("deck.ppt")), "deck.pdf");
        assert_eq!(suggested_pptx_pdf_name(Some("deck.pps")), "deck.pdf");
        assert_eq!(suggested_pptx_pdf_name(Some("deck.ppsx")), "deck.pdf");
        // 大小写不敏感
        assert_eq!(suggested_pptx_pdf_name(Some("Deck.PPTX")), "Deck.pdf");
        // 只换最后一段
        assert_eq!(
            suggested_pptx_pdf_name(Some("a.pptx.pptx")),
            "a.pptx.pdf"
        );
    }

    #[test]
    fn suggested_pdf_name_appends_when_there_is_no_ppt_extension() {
        assert_eq!(suggested_pptx_pdf_name(Some("notes.txt")), "notes.txt.pdf");
        assert_eq!(suggested_pptx_pdf_name(Some("slides")), "slides.pdf");
    }

    #[test]
    fn suggested_pdf_name_strips_the_directory() {
        assert_eq!(
            suggested_pptx_pdf_name(Some("/home/me/deck.pptx")),
            "deck.pdf"
        );
        assert_eq!(
            suggested_pptx_pdf_name(Some(r"C:/work/deck.pptx")),
            "deck.pdf"
        );
        // 混合分隔符
        assert_eq!(
            suggested_pptx_pdf_name(Some(r"C:/work/sub/deck.pptx")),
            "deck.pdf"
        );
    }

    #[test]
    fn suggested_pdf_name_falls_back_when_there_is_no_file_name() {
        assert_eq!(suggested_pptx_pdf_name(None), "presentation.pdf");
        assert_eq!(suggested_pptx_pdf_name(Some("")), "presentation.pdf");
        assert_eq!(suggested_pptx_pdf_name(Some("   ")), "presentation.pdf");
        // 以分隔符结尾 → 最后一段是空串 → 同样回退
        assert_eq!(suggested_pptx_pdf_name(Some("a/b/")), "presentation.pdf");
    }
}
