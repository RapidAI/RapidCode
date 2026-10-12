//! `PdfViewer` —— Rust 移植自 `packages/ui/src/components/ui/pdf-viewer.tsx`
//!
//! PDF 预览：翻页 + 缩放 + 分段拉取。TS 侧把渲染整个交给 `react-pdf`
//! （`<Document>` / `<Page>`，底层是 pdf.js 的 worker + canvas），这个文件
//! 负责**页码状态机、缩放策略、以及控制条的全部样式**。
//!
//! ## 关键移植点 1：`ArrayBuffer` 必须复制，`Uint8Array` 不必
//!
//! pdf.js 会把二进制数据 **transfer** 给 worker，原 buffer 随即 detached。
//! 所以 TS 里 `ArrayBuffer` 走 `new Uint8Array(source.slice(0))` 复制一份，
//! 保证调用方复用同一份数据重开预览时不会报错。这个区别在 Rust 侧用
//! 两个枚举变体保留（[`PdfViewerSource::ArrayBuffer`] vs
//! [`PdfViewerSource::Bytes`]），由 [`PdfViewerSource::needs_copy`] 表达。
//!
//! ## 关键移植点 2：两套倍率 + 停顿提交 + 双缓冲覆盖层
//!
//! - `displayScale`：用户看到的目标倍率，缩放手势期间**只**更新它，
//!   走 CSS `transform: scale()`（合成器出画面，不掉帧）；
//! - `renderScale`：真正传给渲染器的倍率，手势停顿 [`ZOOM_COMMIT_DELAY_MS`]
//!   之后才提交，避免连续滚轮触发几十次整页重绘；
//! - 提交瞬间旧 canvas 被卸载、新 canvas 还没画完，中间会闪白 —— 所以先
//!   复制当前位图当覆盖层，等新 canvas `onRenderSuccess` 到了再撤掉。
//!
//! 数学与覆盖层策略都在 [`crate::pdf_zoom_overlay`]，本文件只做编排。
//!
//! 还有一个容易漏掉的守卫：**旧 render task 的回调可能在新倍率提交后才到**。
//! [`is_stale_page_render`] 用 [`PDF_VIEWER_RENDER_SCALE_EPSILON`] 判定，
//! 命中就直接丢弃，否则旧回调会提前把覆盖层撤掉。
//!
//! ## 关键移植点 3：滚轮缩放必须挂**非 passive** 监听
//!
//! React 的 `onWheel` 委托在 root 上是 passive 监听，`preventDefault()` 不生效，
//! 缩放时页面会跟着滚。TS 因此绕过 React，直接
//! `container.addEventListener("wheel", h, { passive: false })`。
//! Rust 侧这个能力取决于渲染器（见模块差异 2），宿主需要自己挂原生监听并
//! 调用 [`PdfViewerWheelZoom::resolve`]。
//!
//! 修饰键**按平台二选一**：Apple 用 `⌘`，其余平台用 `Ctrl`（复用
//! [`crate::keyboard_shortcuts::is_apple_keyboard_platform`]）。
//!
//! ## 关键移植点 4：页码输入框内不吃方向键
//!
//! TS 的 `handleKeyDown` 先看 `event.target.tagName === "INPUT"` 就返回，
//! 把方向键的光标语义还给输入框。[`resolve_pdf_viewer_key_action`] 保留这个
//! guard；组件里则在输入框自身 `stop_propagation()`，语义等价（见模块差异 3）。
//!
//! ## 与 TS 的差异
//!
//! 1. **不引入 `react-pdf` / `pdf.js`**：`<Document>` / `<Page>` 无对应物，
//!    改为由宿主以 `children` **注入**。本组件渲染除页面画布之外的全部
//!    chrome（滚动容器、控制条、提示文案），因此可以被完整测试。
//! 2. **`addEventListener("wheel", …, { passive: false })` → 宿主职责**：
//!    Dioxus 的 `onwheel` 是否 passive 由渲染器决定，本组件提供
//!    [`PdfViewerWheelZoom`] 让宿主在自己的原生监听里做同样的判定。
//! 3. **输入框用 `stop_propagation()` 代替 `tagName` 判定**：纯函数
//!    [`resolve_pdf_viewer_key_action`] 仍保留 guard 以便逐字对照与测试。
//! 4. **`labels` 由 `Partial<PdfViewerLabels>` 改为显式的
//!    [`PdfViewerLabelOverrides`]**：Rust 侧没有 `{...DEFAULT, ...labels}` 的
//!    结构展开，用「8 个 `Option` + [`PdfViewerLabels::merged`]」表达同一语义。
//! 5. **`style` 对象改为样式字符串**：由
//!    [`pdf_viewer_page_viewport_style`] / [`pdf_viewer_page_preview_style`] 生成。

use crate::cn::cn;
use crate::icons::{IconChevronLeft, IconChevronRight, IconZoomIn, IconZoomOut};
use crate::keyboard_shortcuts::is_apple_keyboard_platform;
use crate::pdf_js_assets::{create_pdf_js_document_options, PdfJsDocumentOptions};
use crate::pdf_zoom_overlay::{
    clamp_scale, get_pdf_page_display_size, get_pdf_page_preview_style, wheel_zoom_target,
    PdfDisplaySize, PdfPagePreviewStyle, PdfPageSize, PdfZoomPointer, MAX_SCALE, MIN_SCALE,
    ZOOM_STEP,
};
use crate::components::button::{Button, ButtonSize, ButtonVariant};
use dioxus::prelude::*;

/// 与 service 层 `readFileRange` 的默认分段大小对齐：一次 range 请求 = 一次 RPC。
pub const PDF_VIEWER_RANGE_CHUNK_BYTES: usize = 256 * 1024;

/// 判定「旧 render task 回调」的倍率容差。
///
/// 浮点误差不可能超过它，而真实的倍率变化（最小 [`ZOOM_STEP`] = 0.25）
/// 远大于它。
pub const PDF_VIEWER_RENDER_SCALE_EPSILON: f64 = 0.001;

/// 总页数未知时控制条上显示的占位符。
pub const PDF_VIEWER_PAGE_COUNT_PLACEHOLDER: &str = "-";

// ---- 类名 ----

/// 根节点基础类名。
pub const PDF_VIEWER_BASE: &str = "flex h-full min-h-0 flex-col outline-none";

/// 滚动容器。
pub const PDF_VIEWER_SCROLL_CONTAINER: &str = "min-h-0 flex-1 overflow-auto";

/// 内容居中包装（`w-max` 让页面比容器宽时可以横向滚动）。
pub const PDF_VIEWER_CONTENT_WRAPPER: &str = "mx-auto w-max p-4";

/// 页面视口（`position: relative` 供覆盖层绝对定位）。
pub const PDF_VIEWER_PAGE_VIEWPORT: &str = "relative";

/// 页面画布的投影。
pub const PDF_VIEWER_PAGE: &str = "shadow-md";

/// 错误文案。
pub const PDF_VIEWER_ERROR_MESSAGE: &str = "p-3 text-ui-base text-destructive";

/// 弱化提示文案（加载中 / 无数据）。
pub const PDF_VIEWER_SUBTLE_MESSAGE: &str = "p-3 text-ui-base text-foreground-subtle";

/// 底部控制条。
pub const PDF_VIEWER_CONTROLS: &str =
    "flex shrink-0 items-center justify-center gap-1 border-t border-border px-2 py-1.5";

/// 页码区（`12 / 30`）。
pub const PDF_VIEWER_PAGE_COUNTER: &str = "flex items-center gap-1 text-ui-base text-foreground-subtle";

/// 页码输入框。
pub const PDF_VIEWER_PAGE_INPUT: &str = "h-6 w-10 rounded-md border border-input-border bg-input px-1 text-center text-ui-base text-foreground outline-none transition-colors hover:border-input-border-hover focus-visible:border-input-border-focused focus-visible:bg-input-focused disabled:pointer-events-none disabled:opacity-50";

/// 控制条中间的分隔竖线。
pub const PDF_VIEWER_CONTROLS_DIVIDER: &str = "mx-1 h-4 w-px bg-border";

/// 缩放百分比标签（`tabular-nums` 防抖动）。
pub const PDF_VIEWER_ZOOM_LABEL: &str =
    "w-11 text-center text-ui-base text-foreground-subtle tabular-nums";

// ---- 文案 ----

/// 控制条与提示的全部文案（TS `PdfViewerLabels`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdfViewerLabels {
    pub loading: String,
    pub load_error: String,
    pub no_data: String,
    pub previous_page: String,
    pub next_page: String,
    pub page_input: String,
    pub zoom_in: String,
    pub zoom_out: String,
}

impl Default for PdfViewerLabels {
    /// 逐字对应 TS 的 `DEFAULT_LABELS`（`en-US`）。
    fn default() -> Self {
        Self {
            loading: "Loading PDF…".to_string(),
            load_error: "Failed to load PDF".to_string(),
            no_data: "No PDF data".to_string(),
            previous_page: "Previous page".to_string(),
            next_page: "Next page".to_string(),
            page_input: "Page number".to_string(),
            zoom_in: "Zoom in".to_string(),
            zoom_out: "Zoom out".to_string(),
        }
    }
}

/// 文案覆盖（对应 TS 的 `Partial<PdfViewerLabels>`）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PdfViewerLabelOverrides {
    pub loading: Option<String>,
    pub load_error: Option<String>,
    pub no_data: Option<String>,
    pub previous_page: Option<String>,
    pub next_page: Option<String>,
    pub page_input: Option<String>,
    pub zoom_in: Option<String>,
    pub zoom_out: Option<String>,
}

impl PdfViewerLabels {
    /// `{ ...DEFAULT_LABELS, ...labels }`。
    pub fn merged(overrides: Option<&PdfViewerLabelOverrides>) -> Self {
        let mut labels = Self::default();
        if let Some(o) = overrides {
            labels.loading = o.loading.clone().unwrap_or(labels.loading);
            labels.load_error = o.load_error.clone().unwrap_or(labels.load_error);
            labels.no_data = o.no_data.clone().unwrap_or(labels.no_data);
            labels.previous_page = o.previous_page.clone().unwrap_or(labels.previous_page);
            labels.next_page = o.next_page.clone().unwrap_or(labels.next_page);
            labels.page_input = o.page_input.clone().unwrap_or(labels.page_input);
            labels.zoom_in = o.zoom_in.clone().unwrap_or(labels.zoom_in);
            labels.zoom_out = o.zoom_out.clone().unwrap_or(labels.zoom_out);
        }
        labels
    }
}

// ---- 数据源 ----

/// 分段拉取源的数据部分。
///
/// TS 的 `PdfViewerRangeSource.requestRange(offset, length)` 是一个
/// **回调**；Rust 侧不在这个结构里持有它（那会让结构体无法 `PartialEq`，
/// 也就无法当 prop），改由宿主在挂 wheel/range 监听时自己闭包捕获。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdfViewerRangeSource {
    /// 文档总字节数。
    pub total_bytes: usize,
    /// 首个分段（pdf.js 用它立刻解析 trailer）。
    pub initial_data: Option<Vec<u8>>,
}

impl PdfViewerRangeSource {
    pub fn new(total_bytes: usize) -> Self {
        Self {
            total_bytes,
            initial_data: None,
        }
    }

    pub fn with_initial_data(mut self, data: Vec<u8>) -> Self {
        self.initial_data = Some(data);
        self
    }
}

/// 文档来源（TS `string | Blob | ArrayBuffer | Uint8Array | PdfViewerRangeSource`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PdfViewerSource {
    /// 字符串：`http(s)` URL，或 `blob:` 对象 URL。
    Url(String),
    /// `Blob`：浏览器侧表达为对象 URL（`URL.createObjectURL(blob)`）。
    ///
    /// 与 [`PdfViewerSource::Url`] 分开是因为语义不同 —— 前者是服务端资源，
    /// 后者是调用方自己造的内存对象，生命周期归调用方（要记得 `revokeObjectURL`）。
    BlobUrl(String),
    /// `Uint8Array`：pdf.js 直接持有，**不需要**复制。
    Bytes(Vec<u8>),
    /// `ArrayBuffer`：**必须**复制，见模块文档移植点 1。
    ArrayBuffer(Vec<u8>),
    /// 分段拉取（大文件只拉需要的页）。
    Range(PdfViewerRangeSource),
}

impl PdfViewerSource {
    /// 是否是分段拉取源（决定 `Document` 走哪套 options）。
    pub fn is_range(&self) -> bool {
        matches!(self, PdfViewerSource::Range(_))
    }

    /// 二进制数据是否需要在交给渲染器前复制一份。
    pub fn needs_copy(&self) -> bool {
        matches!(self, PdfViewerSource::ArrayBuffer(_))
    }

    /// 交给渲染器的字节（`Url` / `BlobUrl` 没有内联字节）。
    pub fn inline_bytes(&self) -> Option<&[u8]> {
        match self {
            PdfViewerSource::Bytes(b) | PdfViewerSource::ArrayBuffer(b) => Some(b),
            _ => None,
        }
    }

    /// 按模块移植点 1 的规则取出**可以安全 transfer** 的字节。
    ///
    /// `ArrayBuffer` 会克隆一份，`Uint8Array` 原样返回。
    pub fn bytes_for_document(&self) -> Option<Vec<u8>> {
        match self {
            PdfViewerSource::Bytes(b) => Some(b.clone()),
            PdfViewerSource::ArrayBuffer(b) => Some(b.clone()),
            _ => None,
        }
    }

    /// 分段拉取源；不是该形态时返回 `None`。
    pub fn as_range(&self) -> Option<&PdfViewerRangeSource> {
        match self {
            PdfViewerSource::Range(source) => Some(source),
            _ => None,
        }
    }
}

// ---- 文档参数 ----

/// 传给 `pdf.js` 的加载参数（`cMapUrl` 必需，见 [`crate::pdf_js_assets`]）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdfViewerDocumentOptions {
    pub c_map_packed: bool,
    pub c_map_url: String,
    /// range 模式关闭整档预取。
    pub disable_auto_fetch: bool,
    /// range 模式关闭流式加载。
    pub disable_stream: bool,
    /// range 模式的分段大小（非 range 模式为 0）。
    pub range_chunk_size: usize,
}

impl PdfViewerDocumentOptions {
    fn from_base(base: PdfJsDocumentOptions) -> Self {
        Self {
            c_map_packed: base.c_map_packed,
            c_map_url: base.c_map_url,
            disable_auto_fetch: false,
            disable_stream: false,
            range_chunk_size: 0,
        }
    }
}

/// 整档加载的 `DOCUMENT_OPTIONS`。
pub fn create_pdf_viewer_document_options(
    vite_base_url: &str,
    document_url: &str,
) -> PdfViewerDocumentOptions {
    PdfViewerDocumentOptions::from_base(create_pdf_js_document_options(
        vite_base_url,
        document_url,
    ))
}

/// 分段加载的 `RANGE_DOCUMENT_OPTIONS`。
///
/// 注释点明的原因：range 模式下要**关掉整档预取与流式加载**，
/// 保持「只拉需要的页」；`rangeChunkSize` 与 service 层的分段大小对齐，
/// 一次 range 请求正好对应一次 RPC。
pub fn create_pdf_viewer_range_document_options(
    vite_base_url: &str,
    document_url: &str,
) -> PdfViewerDocumentOptions {
    PdfViewerDocumentOptions {
        disable_auto_fetch: true,
        disable_stream: true,
        range_chunk_size: PDF_VIEWER_RANGE_CHUNK_BYTES,
        ..create_pdf_viewer_document_options(vite_base_url, document_url)
    }
}

// ---- 纯逻辑 ----

/// `Math.round`：**半数向 +∞ 取整**（`-2.5` → `-2`），与 Rust 的
/// `f64::round`（半数远离零）不同，所以不能直接用。
fn js_round(value: f64) -> f64 {
    if !value.is_finite() {
        return value;
    }
    (value + 0.5).floor()
}

/// `Number.parseInt(value.trim(), 10)` 的语义：
///
/// - 忽略前导空白；
/// - 允许一个 `+` / `-`；
/// - 只吃连续数字，后面的字符一律忽略（`"12.5"` → `12`、`"1e3"` → `1`）；
/// - 一个数字都没有 → `None`（`parseInt` 返回 `NaN`）。
///
/// 与 [`crate::components::smooth_metric_value`] 里的同名实现相比，
/// 这里**不做** `max(0, …)` 夹取 —— 页码允许负数，由 `goToPage` 夹到 `1`。
pub fn parse_page_input_int(value: &str) -> Option<i64> {
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
    // parseInt 用 IEEE double 累加，超大整数会丢精度；这里用 f64 复刻
    let magnitude: f64 = digits.parse().unwrap_or(f64::INFINITY);
    let signed = if negative { -magnitude } else { magnitude };
    Some(signed as i64)
}

/// `goToPage` 的结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdfViewerPageNavigation {
    /// 新的输入框文本（**总是**会被写入，即使页码没变）。
    pub page_input: String,
    /// `Some` 表示页码真的变了。
    pub page_number: Option<usize>,
    /// 换页后要把滚动容器拉回顶部。
    pub scroll_to_top: bool,
    /// 换页后页面固有尺寸作废（新页尺寸可能不同）。
    pub reset_page_intrinsic_size: bool,
    /// 换页后撤掉缩放覆盖层（覆盖的是上一页的位图）。
    pub clear_zoom_overlay: bool,
}

/// `goToPage(target)`。
///
/// - 总页数未知（`num_pages == None`）→ `None`，**什么都不做**（TS 里直接 return）；
/// - 目标夹到 `[1, num_pages]`，并**总是**回写输入框；
/// - 夹取后的页码与当前相同时只回写输入框，不重置滚动/尺寸/覆盖层。
pub fn go_to_page(
    num_pages: Option<usize>,
    current_page: usize,
    target: f64,
) -> Option<PdfViewerPageNavigation> {
    let total = num_pages?;
    if total == 0 {
        // `Math.min(x, 0)` 会得到 0，页码 0 无意义；TS 里 numPages 不会为 0
        return None;
    }
    let clamped = (js_round(target).max(1.0).min(total as f64)) as usize;
    let page_input = clamped.to_string();
    if clamped != current_page {
        Some(PdfViewerPageNavigation {
            page_input,
            page_number: Some(clamped),
            scroll_to_top: true,
            reset_page_intrinsic_size: true,
            clear_zoom_overlay: true,
        })
    } else {
        Some(PdfViewerPageNavigation {
            page_input,
            page_number: None,
            scroll_to_top: false,
            reset_page_intrinsic_size: false,
            clear_zoom_overlay: false,
        })
    }
}

/// `commitPageInput` 的结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PdfViewerPageInputCommit {
    /// 解析失败 → 输入框回滚到当前页码，页码不变。
    Revert { page_input: String },
    /// 解析成功 → 走 `goToPage`。
    Navigate(PdfViewerPageNavigation),
}

/// `commitPageInput()`（Enter / blur）。
///
/// 返回 `None` 表示**完全无操作**：解析成功但 `goToPage` 早退（总页数未知）。
pub fn commit_page_input(
    page_input: &str,
    current_page: usize,
    num_pages: Option<usize>,
) -> Option<PdfViewerPageInputCommit> {
    let Some(parsed) = parse_page_input_int(page_input) else {
        return Some(PdfViewerPageInputCommit::Revert {
            page_input: current_page.to_string(),
        });
    };
    go_to_page(num_pages, current_page, parsed as f64).map(PdfViewerPageInputCommit::Navigate)
}

/// 根容器 `keydown` 的语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdfViewerKeyAction {
    /// 不处理（含「焦点在输入框内」的情况）。
    None,
    PreviousPage,
    NextPage,
}

/// `handleKeyDown`：**只有** 左 / 右方向键翻页。
///
/// `target_is_input` 为真时一律 `None` —— 把方向键的光标语义还给页码输入框。
pub fn resolve_pdf_viewer_key_action(key: Key, target_is_input: bool) -> PdfViewerKeyAction {
    if target_is_input {
        return PdfViewerKeyAction::None;
    }
    match key {
        Key::ArrowLeft => PdfViewerKeyAction::PreviousPage,
        Key::ArrowRight => PdfViewerKeyAction::NextPage,
        _ => PdfViewerKeyAction::None,
    }
}

/// 滚轮缩放是否触发：**按平台二选一**的修饰键。
///
/// Apple 用 `⌘`，其余平台用 `Ctrl`；同时按下不算（避免和浏览器手势冲突）。
pub fn pdf_viewer_zoom_modifier_pressed(
    apple_keyboard: bool,
    meta_key: bool,
    ctrl_key: bool,
) -> bool {
    if apple_keyboard {
        meta_key
    } else {
        ctrl_key
    }
}

/// 按当前平台判定修饰键（`isAppleKeyboardPlatform()` 的等价入口）。
pub fn pdf_viewer_zoom_modifier_pressed_for(
    platform: &crate::keyboard_shortcuts::KeyboardShortcutPlatformInfo,
    meta_key: bool,
    ctrl_key: bool,
) -> bool {
    pdf_viewer_zoom_modifier_pressed(is_apple_keyboard_platform(platform), meta_key, ctrl_key)
}

/// 一次滚轮缩放请求（判定 + 计算 + 指针）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PdfViewerWheelZoom {
    pub scale: f64,
    pub pointer: PdfZoomPointer,
}

impl PdfViewerWheelZoom {
    /// `handleWheelZoom` 的纯函数版本。
    ///
    /// 返回 `None` 的三种情况：
    /// 1. 修饰键没按（正常滚动，不拦截）；
    /// 2. `deltaY == 0`（`preventDefault` 已经做了，但没有缩放量）；
    /// 3. 夹取后倍率没变（已经在上下限上）。
    ///
    /// 调用方需要先 `preventDefault()` —— 见模块移植点 3。
    pub fn resolve(
        current_display_scale: f64,
        delta_y: f64,
        modifier_pressed: bool,
        pointer: PdfZoomPointer,
    ) -> Option<Self> {
        if !modifier_pressed || delta_y == 0.0 {
            return None;
        }
        let target = wheel_zoom_target(current_display_scale, delta_y);
        let scale = resolve_pdf_viewer_zoom_target(current_display_scale, target)?;
        Some(Self { scale, pointer })
    }
}

/// `zoomTo(target)` 的早退：夹取后与当前相同就什么都不做。
pub fn resolve_pdf_viewer_zoom_target(current_display_scale: f64, target: f64) -> Option<f64> {
    let next = clamp_scale(target);
    if next == current_display_scale {
        None
    } else {
        Some(next)
    }
}

/// 这次 `onRenderSuccess` 是不是**旧** render task 的回调。
///
/// 命中就要丢弃：否则旧回调会提前撤掉覆盖层，露出还没画完的新 canvas。
pub fn is_stale_page_render(completed_scale: f64, current_render_scale: f64) -> bool {
    (completed_scale - current_render_scale).abs() > PDF_VIEWER_RENDER_SCALE_EPSILON
}

/// 手势停顿后是否需要把预览倍率提交给渲染器。
pub fn should_commit_zoom(display_scale: f64, render_scale: f64) -> bool {
    display_scale != render_scale
}

// ---- 样式 ----

/// 页面视口的尺寸样式（`pageDisplaySize`）。
///
/// 返回 `None` 表示还没有已知尺寸 → 不设样式，交给内容撑开。
pub fn pdf_viewer_page_viewport_style(size: Option<PdfDisplaySize>) -> Option<String> {
    size.map(|s| format!("width: {}px; height: {}px;", s.width, s.height))
}

/// 页面预览层样式（`pagePreviewStyle`）。
///
/// `absolute` 为真时脱离文档流（`position: absolute; inset: 0`），
/// 只让目标布局盒决定滚动范围；倍率不等于 1 时再加 `transform: scale()`。
pub fn pdf_viewer_page_preview_style(style: Option<PdfPagePreviewStyle>) -> Option<String> {
    let style = style?;
    let mut parts: Vec<String> = Vec::new();
    if style.absolute {
        parts.push("position: absolute;".to_string());
        parts.push("inset: 0;".to_string());
    }
    parts.push(format!("transform-origin: {};", style.transform_origin));
    if style.preview_scale != 1.0 {
        parts.push(format!("transform: scale({});", style.preview_scale));
    }
    Some(parts.join(" "))
}

// ---- 控制条状态 ----

/// 控制条 + 页面布局的全部派生值。
///
/// 这是一个**纯投影**：同样的输入永远得到同样的输出，组件只负责渲染它。
#[derive(Clone, Debug, PartialEq)]
pub struct PdfViewerControlsState {
    /// 文档总页数（`None` = 还没加载完）。
    pub num_pages: Option<usize>,
    /// 文档还没加载完 → 所有控制项禁用。
    pub controls_disabled: bool,
    pub previous_disabled: bool,
    pub next_disabled: bool,
    pub zoom_out_disabled: bool,
    pub zoom_in_disabled: bool,
    /// 用户看到的倍率（`displayScale`）。
    pub display_scale: f64,
    /// 渲染器正在用的倍率（`renderScale`）。
    pub render_scale: f64,
    /// 当前页码。
    pub page_number: usize,
    /// `Math.round(displayScale * 100)`。
    pub zoom_percent: i64,
    /// 覆盖层的 `transform: scale()` 倍率（`displayScale / renderScale`）。
    pub zoom_preview_scale: f64,
    pub page_display_size: Option<PdfDisplaySize>,
    pub page_preview_style: Option<PdfPagePreviewStyle>,
    /// `12 / 30` 里的 `/ 30`（未知时是 `/ -`）。
    pub page_counter: String,
    /// `125%`。
    pub zoom_label: String,
}

/// 由原始状态算出控制条需要的一切。
pub fn build_pdf_viewer_controls(
    num_pages: Option<usize>,
    page_number: usize,
    display_scale: f64,
    render_scale: f64,
    page_intrinsic_size: Option<PdfPageSize>,
) -> PdfViewerControlsState {
    let controls_disabled = num_pages.is_none();
    let page_display_size = get_pdf_page_display_size(page_intrinsic_size, display_scale);
    let zoom_preview_scale = display_scale / render_scale;
    let zoom_percent = js_round(display_scale * 100.0) as i64;
    PdfViewerControlsState {
        num_pages,
        controls_disabled,
        previous_disabled: controls_disabled || page_number <= 1,
        next_disabled: controls_disabled || num_pages.is_none_or(|total| page_number >= total),
        zoom_out_disabled: controls_disabled || display_scale <= MIN_SCALE,
        zoom_in_disabled: controls_disabled || display_scale >= MAX_SCALE,
        display_scale,
        render_scale,
        page_number,
        zoom_percent,
        zoom_preview_scale,
        page_display_size,
        page_preview_style: get_pdf_page_preview_style(
            page_display_size.is_some(),
            zoom_preview_scale,
        ),
        page_counter: format!(
            "/ {}",
            num_pages
                .map(|total| total.to_string())
                .unwrap_or_else(|| PDF_VIEWER_PAGE_COUNT_PLACEHOLDER.to_string())
        ),
        zoom_label: format!("{zoom_percent}%"),
    }
}

// ---- 组件 ----

/// 缩放请求（按钮缩放无指针；滚轮缩放带指针）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PdfViewerZoomRequest {
    pub scale: f64,
    pub pointer: Option<PdfZoomPointer>,
}

/// 提交输入框：解析成功派发导航、失败回滚文本。
///
/// 单独抽成函数是为了能在「Enter」与「blur」两处复用同一个 `Fn` 逻辑 ——
/// 直接在 rsx 里写同一个闭包会被 move 两次。
fn dispatch_page_input_commit(
    text: &str,
    page_number: usize,
    num_pages: Option<usize>,
    onpageinputchange: EventHandler<String>,
    onnavigate: EventHandler<PdfViewerPageNavigation>,
) {
    match commit_page_input(text, page_number, num_pages) {
        // 总页数未知 → goToPage 早退，完全无操作
        None => {}
        Some(PdfViewerPageInputCommit::Revert { page_input }) => {
            onpageinputchange.call(page_input);
        }
        Some(PdfViewerPageInputCommit::Navigate(navigation)) => onnavigate.call(navigation),
    }
}

/// PDF 预览外壳。
///
/// **不含**页面画布本身 —— 那由宿主以 `children` 注入
/// （对应 TS 里的 `<Document>` + `<Page>`）。
#[component]
pub fn PdfViewer(
    #[props(default, into)] class: String,
    /// 文案覆盖。
    #[props(default)] labels: Option<PdfViewerLabelOverrides>,
    /// 控制条的派生状态（[`build_pdf_viewer_controls`] 的产物）。
    state: PdfViewerControlsState,
    /// 输入框的受控文本。
    #[props(default, into)] page_input: String,
    /// range 传输出错 → 整块换成错误文案，不再渲染页面。
    #[props(default)] range_error: bool,
    /// 注入的文档渲染器。
    children: Element,
    onpageinputchange: EventHandler<String>,
    /// 提交输入框（Enter / blur）。宿主把结果应用到自己的状态上。
    onnavigate: EventHandler<PdfViewerPageNavigation>,
    /// 缩放请求。
    onzoomto: EventHandler<PdfViewerZoomRequest>,
    /// 根容器上的方向键语义。
    onkeyaction: EventHandler<PdfViewerKeyAction>,
    /// 页面视口挂载（宿主用它拿 ref 做锚点测量与覆盖层）。
    #[props(default)] onviewportmounted: EventHandler<MountedEvent>,
) -> Element {
    let labels = PdfViewerLabels::merged(labels.as_ref());
    let display_scale = state.display_scale;
    let page_number = state.page_number;
    let num_pages = state.num_pages;

    let viewport_style = pdf_viewer_page_viewport_style(state.page_display_size);
    let preview_style = pdf_viewer_page_preview_style(state.page_preview_style);

    let enter_text = page_input.clone();
    let blur_text = page_input.clone();

    rsx! {
        div {
            tabindex: "0",
            class: cn(&[PDF_VIEWER_BASE, &class]),
            "data-slot": "pdf-viewer",
            onkeydown: move |e: KeyboardEvent| {
                match resolve_pdf_viewer_key_action(e.key(), false) {
                    PdfViewerKeyAction::None => {}
                    action => {
                        e.prevent_default();
                        onkeyaction.call(action);
                    }
                }
            },

            div { class: PDF_VIEWER_SCROLL_CONTAINER, "data-slot": "pdf-viewer-scroll",
                div { class: PDF_VIEWER_CONTENT_WRAPPER,
                    if range_error {
                        div { class: PDF_VIEWER_ERROR_MESSAGE, "{labels.load_error}" }
                    } else {
                        div {
                            class: PDF_VIEWER_PAGE_VIEWPORT,
                            "data-slot": "pdf-viewer-page-viewport",
                            style: viewport_style,
                            onmounted: move |e| onviewportmounted.call(e),
                            div { style: preview_style, {children} }
                        }
                    }
                }
            }

            div { class: PDF_VIEWER_CONTROLS, "data-slot": "pdf-viewer-controls",
                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::IconSm,
                    aria_label: labels.previous_page.clone(),
                    disabled: state.previous_disabled,
                    onclick: move |_| onkeyaction.call(PdfViewerKeyAction::PreviousPage),
                    IconChevronLeft { aria_hidden: true }
                }
                div { class: PDF_VIEWER_PAGE_COUNTER,
                    input {
                        value: "{page_input}",
                        "inputmode": "numeric",
                        disabled: state.controls_disabled,
                        "aria-label": labels.page_input.clone(),
                        class: PDF_VIEWER_PAGE_INPUT,
                        oninput: move |e: FormEvent| onpageinputchange.call(e.value()),
                        onkeydown: move |e: KeyboardEvent| {
                            // 输入框内保留方向键的光标语义（见模块移植点 4）：
                            // 纯函数在这里必定返回 None，事件也不再冒泡到根容器。
                            let _ = resolve_pdf_viewer_key_action(e.key(), true);
                            if matches!(e.key(), Key::Enter) {
                                e.prevent_default();
                            }
                            e.stop_propagation();
                            if matches!(e.key(), Key::Enter) {
                                dispatch_page_input_commit(
                                    &enter_text,
                                    page_number,
                                    num_pages,
                                    onpageinputchange,
                                    onnavigate,
                                );
                            }
                        },
                        onblur: move |_| {
                            dispatch_page_input_commit(
                                &blur_text,
                                page_number,
                                num_pages,
                                onpageinputchange,
                                onnavigate,
                            );
                        },
                    }
                    span { "{state.page_counter}" }
                }
                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::IconSm,
                    aria_label: labels.next_page.clone(),
                    disabled: state.next_disabled,
                    onclick: move |_| onkeyaction.call(PdfViewerKeyAction::NextPage),
                    IconChevronRight { aria_hidden: true }
                }

                div { class: PDF_VIEWER_CONTROLS_DIVIDER }

                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::IconSm,
                    aria_label: labels.zoom_out.clone(),
                    disabled: state.zoom_out_disabled,
                    onclick: move |_| {
                        if let Some(scale) =
                            resolve_pdf_viewer_zoom_target(display_scale, display_scale - ZOOM_STEP)
                        {
                            onzoomto.call(PdfViewerZoomRequest { scale, pointer: None });
                        }
                    },
                    IconZoomOut { aria_hidden: true }
                }
                span { class: PDF_VIEWER_ZOOM_LABEL, "{state.zoom_label}" }
                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::IconSm,
                    aria_label: labels.zoom_in.clone(),
                    disabled: state.zoom_in_disabled,
                    onclick: move |_| {
                        if let Some(scale) =
                            resolve_pdf_viewer_zoom_target(display_scale, display_scale + ZOOM_STEP)
                        {
                            onzoomto.call(PdfViewerZoomRequest { scale, pointer: None });
                        }
                    },
                    IconZoomIn { aria_hidden: true }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyboard_shortcuts::KeyboardShortcutPlatformInfo;

    fn apple() -> KeyboardShortcutPlatformInfo {
        KeyboardShortcutPlatformInfo {
            platform: Some("MacIntel".to_string()),
            user_agent: None,
        }
    }

    fn windows() -> KeyboardShortcutPlatformInfo {
        KeyboardShortcutPlatformInfo {
            platform: Some("Win32".to_string()),
            user_agent: None,
        }
    }

    // ---- 文案 ----

    #[test]
    fn default_labels_match_the_ts_defaults() {
        let l = PdfViewerLabels::default();
        assert_eq!(l.loading, "Loading PDF…");
        assert_eq!(l.load_error, "Failed to load PDF");
        assert_eq!(l.no_data, "No PDF data");
        assert_eq!(l.previous_page, "Previous page");
        assert_eq!(l.next_page, "Next page");
        assert_eq!(l.page_input, "Page number");
        assert_eq!(l.zoom_in, "Zoom in");
        assert_eq!(l.zoom_out, "Zoom out");
    }

    #[test]
    fn no_overrides_yields_the_defaults() {
        assert_eq!(PdfViewerLabels::merged(None), PdfViewerLabels::default());
    }

    #[test]
    fn label_overrides_only_replace_what_is_given() {
        let overrides = PdfViewerLabelOverrides {
            previous_page: Some("上一页".to_string()),
            zoom_in: Some("放大".to_string()),
            ..Default::default()
        };
        let merged = PdfViewerLabels::merged(Some(&overrides));
        assert_eq!(merged.previous_page, "上一页");
        assert_eq!(merged.zoom_in, "放大");
        // 其余保持默认
        assert_eq!(merged.next_page, "Next page");
        assert_eq!(merged.zoom_out, "Zoom out");
        assert_eq!(merged.load_error, "Failed to load PDF");
    }

    // ---- 数据源 ----

    #[test]
    fn range_source_defaults_and_builder() {
        let plain = PdfViewerRangeSource::new(1_000);
        assert_eq!(plain.total_bytes, 1_000);
        assert_eq!(plain.initial_data, None);

        let seeded = PdfViewerRangeSource::new(2_000).with_initial_data(vec![1, 2, 3]);
        assert_eq!(seeded.total_bytes, 2_000);
        assert_eq!(seeded.initial_data, Some(vec![1, 2, 3]));
    }

    #[test]
    fn source_kind_detection() {
        assert!(PdfViewerSource::Range(PdfViewerRangeSource::new(1)).is_range());
        assert!(!PdfViewerSource::Url("a.pdf".into()).is_range());
        assert!(!PdfViewerSource::BlobUrl("blob:x".into()).is_range());
        assert!(!PdfViewerSource::Bytes(vec![1]).is_range());
        assert!(!PdfViewerSource::ArrayBuffer(vec![1]).is_range());
    }

    /// 移植点 1：只有 `ArrayBuffer` 需要复制。
    #[test]
    fn array_buffer_needs_a_copy_but_bytes_do_not() {
        assert!(PdfViewerSource::ArrayBuffer(vec![1, 2]).needs_copy());
        assert!(!PdfViewerSource::Bytes(vec![1, 2]).needs_copy());
        // URL 形态没有内联字节，也就无所谓复制
        assert!(!PdfViewerSource::Url("a.pdf".into()).needs_copy());
        assert!(!PdfViewerSource::BlobUrl("blob:x".into()).needs_copy());
        assert!(!PdfViewerSource::Range(PdfViewerRangeSource::new(1)).needs_copy());
    }

    #[test]
    fn inline_bytes_only_exist_for_the_binary_variants() {
        assert_eq!(
            PdfViewerSource::Bytes(vec![7, 8]).inline_bytes(),
            Some(&[7u8, 8u8][..])
        );
        assert_eq!(
            PdfViewerSource::ArrayBuffer(vec![9]).inline_bytes(),
            Some(&[9u8][..])
        );
        assert_eq!(PdfViewerSource::Url("a.pdf".into()).inline_bytes(), None);
        assert_eq!(
            PdfViewerSource::Range(PdfViewerRangeSource::new(1)).inline_bytes(),
            None
        );
    }

    #[test]
    fn bytes_for_document_copies_the_buffer() {
        let bytes = PdfViewerSource::Bytes(vec![1, 2, 3]);
        assert_eq!(bytes.bytes_for_document(), Some(vec![1, 2, 3]));

        // ArrayBuffer 也返回字节，但语义上是「复制后交给 worker」
        let buffer = PdfViewerSource::ArrayBuffer(vec![4, 5]);
        assert_eq!(buffer.bytes_for_document(), Some(vec![4, 5]));

        assert_eq!(PdfViewerSource::Url("a.pdf".into()).bytes_for_document(), None);
    }

    #[test]
    fn as_range_returns_the_source() {
        let source = PdfViewerSource::Range(PdfViewerRangeSource::new(42));
        assert_eq!(source.as_range().unwrap().total_bytes, 42);
        assert!(PdfViewerSource::Bytes(vec![]).as_range().is_none());
    }

    // ---- 文档参数 ----

    #[test]
    fn document_options_are_not_range_by_default() {
        let options = create_pdf_viewer_document_options("./", "http://localhost/index.html");
        assert!(options.c_map_packed);
        assert!(!options.disable_auto_fetch);
        assert!(!options.disable_stream);
        assert_eq!(options.range_chunk_size, 0);
    }

    /// 注释点明的原因：range 模式要关掉整档预取与流式加载。
    #[test]
    fn range_document_options_turn_off_prefetching() {
        let options = create_pdf_viewer_range_document_options("./", "http://localhost/index.html");
        assert!(options.disable_auto_fetch);
        assert!(options.disable_stream);
        assert_eq!(options.range_chunk_size, PDF_VIEWER_RANGE_CHUNK_BYTES);
        assert_eq!(PDF_VIEWER_RANGE_CHUNK_BYTES, 256 * 1024);
        // cMap 配置不受影响
        assert!(options.c_map_packed);
    }

    #[test]
    fn document_options_carry_the_cmap_url() {
        let options = create_pdf_viewer_document_options("/app/", "http://host/app/index.html");
        assert_eq!(options.c_map_url, "http://host/app/pdfjs/cmaps/");
        // range 形态的 cMap 与整档形态一致
        assert_eq!(
            create_pdf_viewer_range_document_options("/app/", "http://host/app/index.html")
                .c_map_url,
            options.c_map_url
        );
    }

    // ---- parseInt ----

    #[test]
    fn parse_page_input_follows_parse_int() {
        assert_eq!(parse_page_input_int("12"), Some(12));
        assert_eq!(parse_page_input_int("  7 "), Some(7));
        assert_eq!(parse_page_input_int("+3"), Some(3));
        assert_eq!(parse_page_input_int("-4"), Some(-4));
        // 只吃连续数字，后面的字符忽略
        assert_eq!(parse_page_input_int("12.5"), Some(12));
        assert_eq!(parse_page_input_int("1e3"), Some(1));
        assert_eq!(parse_page_input_int("3页"), Some(3));
        assert_eq!(parse_page_input_int("0"), Some(0));
    }

    #[test]
    fn parse_page_input_rejects_when_there_are_no_digits() {
        assert_eq!(parse_page_input_int(""), None);
        assert_eq!(parse_page_input_int("   "), None);
        assert_eq!(parse_page_input_int("abc"), None);
        assert_eq!(parse_page_input_int("-"), None);
        assert_eq!(parse_page_input_int("+"), None);
    }

    // ---- Math.round ----

    #[test]
    fn js_round_rounds_half_toward_positive_infinity() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(2.4), 2.0);
        assert_eq!(js_round(0.5), 1.0);
        // 与 Rust 的 f64::round 不同：-2.5 → -2（Rust 会给 -3）
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(-0.5), 0.0);
        assert_eq!(-2.5_f64.round(), -3.0);
    }

    // ---- goToPage ----

    #[test]
    fn go_to_page_does_nothing_without_a_page_count() {
        assert_eq!(go_to_page(None, 3, 5.0), None);
    }

    #[test]
    fn go_to_page_clamps_into_range() {
        // 下越界 → 1
        assert_eq!(go_to_page(Some(10), 5, 0.0).unwrap().page_number, Some(1));
        assert_eq!(go_to_page(Some(10), 5, -7.0).unwrap().page_number, Some(1));
        // 上越界 → 总页数
        assert_eq!(go_to_page(Some(10), 5, 999.0).unwrap().page_number, Some(10));
        // 界内原样
        assert_eq!(go_to_page(Some(10), 5, 8.0).unwrap().page_number, Some(8));
    }

    #[test]
    fn go_to_page_rounds_fractional_targets() {
        assert_eq!(go_to_page(Some(10), 5, 7.4).unwrap().page_number, Some(7));
        assert_eq!(go_to_page(Some(10), 5, 7.5).unwrap().page_number, Some(8));
        // 半数向 +∞：1.5 → 2
        assert_eq!(go_to_page(Some(10), 5, 1.5).unwrap().page_number, Some(2));
    }

    /// TS 里 `setPageInput` 在早退之前，所以**总是**会被执行。
    #[test]
    fn go_to_page_always_rewrites_the_input() {
        let same = go_to_page(Some(10), 4, 4.0).unwrap();
        assert_eq!(same.page_input, "4");
        // 越界时写回夹取后的值，而不是原始输入
        let clamped = go_to_page(Some(10), 4, 500.0).unwrap();
        assert_eq!(clamped.page_input, "10");
        assert_eq!(go_to_page(Some(10), 4, -1.0).unwrap().page_input, "1");
    }

    #[test]
    fn go_to_page_on_the_same_page_only_rewrites_the_input() {
        let nav = go_to_page(Some(10), 4, 4.0).unwrap();
        assert_eq!(nav.page_number, None);
        assert!(!nav.scroll_to_top);
        assert!(!nav.reset_page_intrinsic_size);
        assert!(!nav.clear_zoom_overlay);
    }

    #[test]
    fn go_to_page_resets_scroll_and_overlay_only_when_the_page_changes() {
        let nav = go_to_page(Some(10), 4, 5.0).unwrap();
        assert_eq!(nav.page_number, Some(5));
        assert!(nav.scroll_to_top);
        assert!(nav.reset_page_intrinsic_size);
        // 覆盖层缓冲的是上一页的位图，换页必须撤掉
        assert!(nav.clear_zoom_overlay);
    }

    #[test]
    fn go_to_page_treats_an_empty_document_as_no_op() {
        assert_eq!(go_to_page(Some(0), 1, 1.0), None);
    }

    // ---- commitPageInput ----

    #[test]
    fn commit_reverts_on_unparseable_input() {
        let commit = commit_page_input("abc", 7, Some(20)).unwrap();
        assert_eq!(
            commit,
            PdfViewerPageInputCommit::Revert {
                page_input: "7".to_string()
            }
        );
        // 空串同样回滚
        assert_eq!(
            commit_page_input("", 3, Some(20)).unwrap(),
            PdfViewerPageInputCommit::Revert {
                page_input: "3".to_string()
            }
        );
    }

    #[test]
    fn commit_navigates_on_parseable_input() {
        let commit = commit_page_input(" 12 ", 3, Some(20)).unwrap();
        match commit {
            PdfViewerPageInputCommit::Navigate(nav) => {
                assert_eq!(nav.page_number, Some(12));
                assert_eq!(nav.page_input, "12");
            }
            other => panic!("expected Navigate, got {other:?}"),
        }
    }

    /// 解析成功但总页数未知 → `goToPage` 早退，整条链路无操作。
    #[test]
    fn commit_is_a_no_op_when_the_document_is_not_loaded() {
        assert_eq!(commit_page_input("5", 1, None), None);
    }

    #[test]
    fn commit_of_the_current_page_does_not_change_the_page_number() {
        match commit_page_input("4", 4, Some(10)).unwrap() {
            PdfViewerPageInputCommit::Navigate(nav) => assert_eq!(nav.page_number, None),
            other => panic!("expected Navigate, got {other:?}"),
        }
    }

    // ---- 键盘 ----

    #[test]
    fn key_action_only_handles_arrows() {
        assert_eq!(
            resolve_pdf_viewer_key_action(Key::ArrowLeft, false),
            PdfViewerKeyAction::PreviousPage
        );
        assert_eq!(
            resolve_pdf_viewer_key_action(Key::ArrowRight, false),
            PdfViewerKeyAction::NextPage
        );
        assert_eq!(
            resolve_pdf_viewer_key_action(Key::ArrowUp, false),
            PdfViewerKeyAction::None
        );
        assert_eq!(
            resolve_pdf_viewer_key_action(Key::Enter, false),
            PdfViewerKeyAction::None
        );
        assert_eq!(
            resolve_pdf_viewer_key_action(Key::PageDown, false),
            PdfViewerKeyAction::None
        );
    }

    /// 输入框内保留方向键的光标语义。
    #[test]
    fn key_action_is_suppressed_inside_the_input() {
        assert_eq!(
            resolve_pdf_viewer_key_action(Key::ArrowLeft, true),
            PdfViewerKeyAction::None
        );
        assert_eq!(
            resolve_pdf_viewer_key_action(Key::ArrowRight, true),
            PdfViewerKeyAction::None
        );
    }

    // ---- 滚轮修饰键 ----

    #[test]
    fn zoom_modifier_is_platform_specific() {
        // Apple：只看 ⌘
        assert!(pdf_viewer_zoom_modifier_pressed(true, true, false));
        assert!(!pdf_viewer_zoom_modifier_pressed(true, false, true));
        // 其余平台：只看 Ctrl
        assert!(pdf_viewer_zoom_modifier_pressed(false, false, true));
        assert!(!pdf_viewer_zoom_modifier_pressed(false, true, false));
        // 都没按
        assert!(!pdf_viewer_zoom_modifier_pressed(true, false, false));
        assert!(!pdf_viewer_zoom_modifier_pressed(false, false, false));
    }

    #[test]
    fn zoom_modifier_for_platform_uses_the_platform_probe() {
        assert!(pdf_viewer_zoom_modifier_pressed_for(&apple(), true, false));
        assert!(!pdf_viewer_zoom_modifier_pressed_for(&apple(), false, true));
        assert!(pdf_viewer_zoom_modifier_pressed_for(&windows(), false, true));
        assert!(!pdf_viewer_zoom_modifier_pressed_for(&windows(), true, false));
    }

    // ---- 滚轮缩放 ----

    fn pointer() -> PdfZoomPointer {
        PdfZoomPointer {
            client_x: 10.0,
            client_y: 20.0,
        }
    }

    #[test]
    fn wheel_zoom_requires_the_modifier() {
        assert_eq!(PdfViewerWheelZoom::resolve(1.0, -100.0, false, pointer()), None);
    }

    #[test]
    fn wheel_zoom_ignores_a_zero_delta() {
        // 修饰键按了、deltaY 为 0：preventDefault 已做，但没有缩放量
        assert_eq!(PdfViewerWheelZoom::resolve(1.0, 0.0, true, pointer()), None);
    }

    #[test]
    fn wheel_zoom_scales_and_carries_the_pointer() {
        let up = PdfViewerWheelZoom::resolve(1.0, -100.0, true, pointer()).unwrap();
        assert!(up.scale > 1.0, "{up:?}");
        assert_eq!(up.pointer, pointer());

        let down = PdfViewerWheelZoom::resolve(1.0, 100.0, true, pointer()).unwrap();
        assert!(down.scale < 1.0, "{down:?}");

        // 与底层公式一致
        assert_eq!(up.scale, wheel_zoom_target(1.0, -100.0));
    }

    #[test]
    fn wheel_zoom_stops_at_the_bounds() {
        // 已经在上下限上时，再滚也不产生请求
        assert_eq!(
            PdfViewerWheelZoom::resolve(MAX_SCALE, -100.0, true, pointer()),
            None
        );
        assert_eq!(
            PdfViewerWheelZoom::resolve(MIN_SCALE, 100.0, true, pointer()),
            None
        );
        // 巨大 delta 会被夹到边界（仍然产生一次请求）
        let clamped = PdfViewerWheelZoom::resolve(1.0, -100_000.0, true, pointer()).unwrap();
        assert_eq!(clamped.scale, MAX_SCALE);
    }

    // ---- zoomTo ----

    #[test]
    fn zoom_target_is_none_when_nothing_changes() {
        assert_eq!(resolve_pdf_viewer_zoom_target(1.0, 1.0), None);
        // 夹取后等于当前值 → 也是「没变化」
        assert_eq!(resolve_pdf_viewer_zoom_target(MAX_SCALE, 99.0), None);
        assert_eq!(resolve_pdf_viewer_zoom_target(MIN_SCALE, 0.0), None);
    }

    #[test]
    fn zoom_target_clamps_and_returns_the_change() {
        assert_eq!(resolve_pdf_viewer_zoom_target(1.0, 1.25), Some(1.25));
        assert_eq!(resolve_pdf_viewer_zoom_target(1.0, 99.0), Some(MAX_SCALE));
        assert_eq!(resolve_pdf_viewer_zoom_target(1.0, 0.0), Some(MIN_SCALE));
        // 步进就是 ZOOM_STEP
        assert_eq!(resolve_pdf_viewer_zoom_target(1.0, 1.0 + ZOOM_STEP), Some(1.25));
    }

    // ---- 提交与陈旧回调 ----

    #[test]
    fn stale_render_detection() {
        assert!(!is_stale_page_render(1.0, 1.0));
        // 容差之内不算陈旧
        assert!(!is_stale_page_render(1.0 + PDF_VIEWER_RENDER_SCALE_EPSILON / 2.0, 1.0));
        assert!(!is_stale_page_render(1.0 - PDF_VIEWER_RENDER_SCALE_EPSILON / 2.0, 1.0));
        // 边界是严格大于：正好等于容差时不算陈旧
        assert!(!is_stale_page_render(1.0 + PDF_VIEWER_RENDER_SCALE_EPSILON, 1.0));
        // 超过容差就是旧回调
        assert!(is_stale_page_render(1.25, 1.0));
        assert!(is_stale_page_render(1.0, 1.25));
        // 注意：`1.0 - 0.001` 的浮点结果离 1.0 有 0.0010000000000000009，
        // 因此**会**被判为陈旧 —— 这正是「用容差而不是相等」要兜住的情况
        assert!(is_stale_page_render(1.0 - PDF_VIEWER_RENDER_SCALE_EPSILON, 1.0));
    }

    /// 真实倍率变化（最小 0.25）远大于容差。
    #[test]
    fn the_epsilon_is_far_below_a_single_zoom_step() {
        assert!(PDF_VIEWER_RENDER_SCALE_EPSILON < ZOOM_STEP / 100.0);
    }

    #[test]
    fn commit_zoom_only_when_the_scales_differ() {
        assert!(!should_commit_zoom(1.0, 1.0));
        assert!(should_commit_zoom(1.25, 1.0));
        assert!(should_commit_zoom(1.0, 1.25));
    }

    // ---- 样式 ----

    #[test]
    fn viewport_style_is_absent_until_the_size_is_known() {
        assert_eq!(pdf_viewer_page_viewport_style(None), None);
    }

    #[test]
    fn viewport_style_carries_the_floor_size() {
        let style = pdf_viewer_page_viewport_style(Some(PdfDisplaySize {
            width: 612.0,
            height: 792.0,
        }))
        .unwrap();
        assert_eq!(style, "width: 612px; height: 792px;");
    }

    #[test]
    fn preview_style_is_absent_when_there_is_nothing_to_do() {
        assert_eq!(pdf_viewer_page_preview_style(None), None);
    }

    #[test]
    fn preview_style_goes_absolute_and_scales() {
        // 有稳定布局盒、倍率 1 → 只脱离文档流
        let absolute_only = pdf_viewer_page_preview_style(Some(PdfPagePreviewStyle {
            absolute: true,
            preview_scale: 1.0,
            transform_origin: "top left",
        }))
        .unwrap();
        assert_eq!(absolute_only, "position: absolute; inset: 0; transform-origin: top left;");
        assert!(!absolute_only.contains("transform: scale"));

        // 手势期间没有布局盒，只有 transform
        let gesture = pdf_viewer_page_preview_style(Some(PdfPagePreviewStyle {
            absolute: false,
            preview_scale: 1.25,
            transform_origin: "top left",
        }))
        .unwrap();
        assert_eq!(gesture, "transform-origin: top left; transform: scale(1.25);");
        assert!(!gesture.contains("position"));
    }

    // ---- 控制条 ----

    fn page_size() -> Option<PdfPageSize> {
        Some(PdfPageSize {
            width: 612.0,
            height: 792.0,
        })
    }

    #[test]
    fn controls_are_all_disabled_while_loading() {
        let state = build_pdf_viewer_controls(None, 1, 1.0, 1.0, None);
        assert!(state.controls_disabled);
        assert!(state.previous_disabled);
        assert!(state.next_disabled);
        assert!(state.zoom_in_disabled);
        assert!(state.zoom_out_disabled);
    }

    #[test]
    fn previous_is_disabled_on_the_first_page() {
        assert!(build_pdf_viewer_controls(Some(10), 1, 1.0, 1.0, None).previous_disabled);
        assert!(!build_pdf_viewer_controls(Some(10), 2, 1.0, 1.0, None).previous_disabled);
    }

    #[test]
    fn next_is_disabled_on_the_last_page() {
        assert!(build_pdf_viewer_controls(Some(10), 10, 1.0, 1.0, None).next_disabled);
        assert!(!build_pdf_viewer_controls(Some(10), 9, 1.0, 1.0, None).next_disabled);
        // 单页文档：两头都禁用
        let single = build_pdf_viewer_controls(Some(1), 1, 1.0, 1.0, None);
        assert!(single.previous_disabled);
        assert!(single.next_disabled);
    }

    #[test]
    fn zoom_buttons_are_disabled_at_the_bounds() {
        let low = build_pdf_viewer_controls(Some(10), 1, MIN_SCALE, MIN_SCALE, None);
        assert!(low.zoom_out_disabled);
        assert!(!low.zoom_in_disabled);

        let high = build_pdf_viewer_controls(Some(10), 1, MAX_SCALE, MAX_SCALE, None);
        assert!(high.zoom_in_disabled);
        assert!(!high.zoom_out_disabled);

        let mid = build_pdf_viewer_controls(Some(10), 1, 1.0, 1.0, None);
        assert!(!mid.zoom_in_disabled);
        assert!(!mid.zoom_out_disabled);
    }

    #[test]
    fn page_counter_uses_a_dash_when_unknown() {
        assert_eq!(
            build_pdf_viewer_controls(None, 1, 1.0, 1.0, None).page_counter,
            "/ -"
        );
        assert_eq!(
            build_pdf_viewer_controls(Some(30), 1, 1.0, 1.0, None).page_counter,
            "/ 30"
        );
    }

    #[test]
    fn zoom_percent_rounds_like_math_round() {
        let state = build_pdf_viewer_controls(Some(10), 1, 1.25, 1.25, None);
        assert_eq!(state.zoom_percent, 125);
        assert_eq!(state.zoom_label, "125%");

        // 1.005 * 100 = 100.49999999999999 → 100
        let rounding = build_pdf_viewer_controls(Some(10), 1, 1.005, 1.005, None);
        assert_eq!(rounding.zoom_percent, 100);
    }

    #[test]
    fn controls_expose_both_scales_and_their_ratio() {
        let state = build_pdf_viewer_controls(Some(10), 3, 1.5, 1.0, page_size());
        assert_eq!(state.display_scale, 1.5);
        assert_eq!(state.render_scale, 1.0);
        assert_eq!(state.zoom_preview_scale, 1.5);
        assert_eq!(state.page_number, 3);
    }

    #[test]
    fn controls_derive_the_page_box_from_the_display_scale() {
        let state = build_pdf_viewer_controls(Some(10), 1, 2.0, 2.0, page_size());
        let size = state.page_display_size.unwrap();
        // 612 * 2 = 1224、792 * 2 = 1584（floor 无影响）
        assert_eq!((size.width, size.height), (1224.0, 1584.0));
        // 有稳定布局盒且倍率一致 → 只脱离文档流
        let style = state.page_preview_style.unwrap();
        assert!(style.absolute);
        assert_eq!(style.preview_scale, 1.0);
    }

    #[test]
    fn controls_leave_the_page_box_open_before_the_first_render() {
        let state = build_pdf_viewer_controls(Some(10), 1, 1.0, 1.0, None);
        assert_eq!(state.page_display_size, None);
        assert_eq!(state.page_preview_style, None);
        assert_eq!(pdf_viewer_page_viewport_style(state.page_display_size), None);
    }

    #[test]
    fn controls_carry_the_gesture_preview_scale_into_the_style() {
        // 手势期间 displayScale 变了、renderScale 还没跟上
        let state = build_pdf_viewer_controls(Some(10), 1, 1.5, 1.0, page_size());
        let style = pdf_viewer_page_preview_style(state.page_preview_style).unwrap();
        // 有布局盒 → 脱离文档流；预览倍率 1.5
        assert!(style.contains("position: absolute;"));
        assert!(style.contains("transform: scale(1.5)"));
    }

    #[test]
    fn zoom_request_carries_an_optional_pointer() {
        let button = PdfViewerZoomRequest {
            scale: 1.25,
            pointer: None,
        };
        assert!(button.pointer.is_none());
        let wheel = PdfViewerZoomRequest {
            scale: 1.1,
            pointer: Some(pointer()),
        };
        assert_eq!(wheel.pointer, Some(pointer()));
    }
}
