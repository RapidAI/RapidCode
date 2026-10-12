//! `CodeViewer` —— Rust 移植自 `packages/ui/src/components/ui/code-viewer.tsx`
//!
//! 代码预览 + 行内评论：行选/ gutter 选择 → 写评论 → 提交成引用。
//! TS 侧把代码渲染整个交给 `@pierre/diffs` 的 `<File>`（跑在 Shadow DOM 里），
//! 这个文件负责**评论状态机、行号坐标换算、快捷键、以及样式变量**。
//!
//! ## 关键移植点 1：三套行号坐标系，换算必须显式
//!
//! | 坐标系 | 基准 | 出现在哪 |
//! |---|---|---|
//! | 文件行号 | `firstLineNumber` | `CodeCommentRange`、`markedLines` |
//! | 选中行 | `1` | `SelectedLineRange`（`@pierre/diffs` 的入参） |
//! | 注释锚点 | `1` | `LineAnnotation.lineNumber` |
//!
//! 三者之间的换算是这个文件里最容易出错的部分，所以全部抽成纯函数：
//! [`selected_range_to_code_comment_range`] /
//! [`code_comment_range_to_selected_range`] /
//! [`build_line_annotations`]。注释锚点用的是 `endLine`（**不是** `startLine`）——
//! 评论要挂在区间的最后一行下面，挂在第一行会把代码顶开。
//!
//! ## 关键移植点 2：评论能力同时决定「行 hover」和「gutter 按钮」
//!
//! 见 TS 里的原注释：markdown 代码块也复用 `CodeViewer`，之前只关掉评论按钮
//! 但保留行 hover，会让**只读代码看起来仍可进入评论交互**。所以两个交互开关
//! 都收敛到 [`CodeViewerCapabilities`] 上，由 `onSubmitCodeComment` 是否存在决定。
//!
//! ## 关键移植点 3：`cacheKey` 必须带主题
//!
//! `@pierre/diffs` 按 `file.cacheKey` 复用 token。旧 key 没带主题，导致**同一
//! task 内切换 app 明暗主题时继续命中旧高亮**，只有切换 task 触发重建后才恢复。
//! [`create_code_viewer_file`] 把主题放进 key 的第一段。
//!
//! ## 关键移植点 4：`color-scheme` 必须跟着**代码主题**，不是系统偏好
//!
//! 手机远控页面可能是深色主题，但浏览器系统偏好仍是浅色。`@pierre/diffs` 的
//! token 颜色依赖 `color-scheme` / `light-dark()`，不显式指定会让 `text`
//! 代码块在深色卡片上渲染成浅色主题的深色文字，看起来像正文丢失。
//!
//! ## 关键移植点 5：提交 / 取消快捷键是「平台主修饰键 + Enter」
//!
//! `⌘ + Enter`（Apple）/ `Ctrl + Enter`（其它）提交，`Esc` 取消。
//! 两者都 `preventDefault`，且**提交判断在前**。
//!
//! ## 与 TS 的差异
//!
//! 1. **不引入 `@pierre/diffs`**：`<File>` 无对应物，改由宿主以 `children`
//!    注入。本组件渲染除代码画布之外的全部 chrome（顶部提示、顶部评论、
//!    评论草稿、样式变量），因此可以被完整测试。
//! 2. **`style` 对象改为样式字符串**（[`code_viewer_style`]），与
//!    `diff_viewer` / `pdf_viewer` 同一约定。
//! 3. **Shadow DOM 相关的三件事改为宿主职责**：`unsafeCSS` 里的
//!    gutter 按钮位置补丁（[`CODE_VIEWER_UNSAFE_CSS`] 原样导出）、
//!    按钮 `title` / `aria-label` 补丁（`patchCodeCommentUtilityButtons`）、
//!    以及 `findCodeViewerLineElement` / `findCodeViewerCommentElement`
//!    的滚动定位（都是 DOM 查询，无对应原语）。
//! 4. **`onSubmitCodeComment(params)` 拆成三个参数**：Rust 的
//!    `EventHandler` 只能收一个参数，所以定义 [`CodeCommentSubmission`] 结构体。
//! 5. **`theme` 用 `Option<&str>`**：TS 是 shiki 的 `BundledTheme` 联合，
//!    Rust 侧不穷举（见 [`crate::code_preview_preferences`]）。

use crate::cn::cn;
use crate::code_preview_preferences::is_dark_code_preview_theme;
use crate::components::button::{Button, ButtonVariant};
use crate::components::textarea::Textarea;
use crate::diffs_highlighter_engine::DIFFS_PREFERRED_HIGHLIGHTER;
use crate::icons::IconTrash;
use crate::keyboard_shortcuts::{
    format_command_shortcut_label, is_apple_keyboard_platform, KeyboardShortcutPlatformInfo,
};
use dioxus::prelude::*;

// ---- 文案 ----

/// 评论相关文案（TS `CodeCommentLabels`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeCommentLabels {
    pub add_comment: String,
    pub add_comment_tooltip: String,
    pub comment_placeholder: String,
    pub submit_comment: String,
    pub cancel_comment: String,
    pub delete_comment: String,
    /// 单行：`Line {line}`。
    pub comment_line: String,
    /// 多行：`Lines {startLine}-{endLine}`。
    pub comment_range: String,
}

impl Default for CodeCommentLabels {
    /// 逐字对应 TS 的 `DEFAULT_CODE_COMMENT_LABELS`（`en-US`）。
    fn default() -> Self {
        Self {
            add_comment: "Add comment".to_string(),
            add_comment_tooltip: "Click or drag to comment".to_string(),
            comment_placeholder: "Comment".to_string(),
            submit_comment: "Add comment".to_string(),
            cancel_comment: "Cancel".to_string(),
            delete_comment: "Delete comment".to_string(),
            comment_line: "Line {line}".to_string(),
            comment_range: "Lines {startLine}-{endLine}".to_string(),
        }
    }
}

/// 文案覆盖（TS 的 `Partial<CodeCommentLabels>`）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeCommentLabelOverrides {
    pub add_comment: Option<String>,
    pub add_comment_tooltip: Option<String>,
    pub comment_placeholder: Option<String>,
    pub submit_comment: Option<String>,
    pub cancel_comment: Option<String>,
    pub delete_comment: Option<String>,
    pub comment_line: Option<String>,
    pub comment_range: Option<String>,
}

impl CodeCommentLabels {
    /// `{ ...DEFAULT_CODE_COMMENT_LABELS, ...labelOverrides }`。
    pub fn merged(overrides: Option<&CodeCommentLabelOverrides>) -> Self {
        let mut labels = Self::default();
        if let Some(o) = overrides {
            labels.add_comment = o.add_comment.clone().unwrap_or(labels.add_comment);
            labels.add_comment_tooltip = o
                .add_comment_tooltip
                .clone()
                .unwrap_or(labels.add_comment_tooltip);
            labels.comment_placeholder = o
                .comment_placeholder
                .clone()
                .unwrap_or(labels.comment_placeholder);
            labels.submit_comment = o.submit_comment.clone().unwrap_or(labels.submit_comment);
            labels.cancel_comment = o.cancel_comment.clone().unwrap_or(labels.cancel_comment);
            labels.delete_comment = o.delete_comment.clone().unwrap_or(labels.delete_comment);
            labels.comment_line = o.comment_line.clone().unwrap_or(labels.comment_line);
            labels.comment_range = o.comment_range.clone().unwrap_or(labels.comment_range);
        }
        labels
    }
}

// ---- 类名与样式 ----

pub const CODE_VIEWER_BASE: &str = "h-full w-full overflow-auto";
pub const CODE_VIEWER_RENDERER_CLASS: &str = "min-h-full w-full";
pub const CODE_VIEWER_NOTICE: &str = "px-3 pt-3 text-ui-sm text-warning";
pub const CODE_VIEWER_ANNOTATION: &str =
    "m-2 rounded-lg border border-border bg-background p-3 font-sans shadow-sm";
pub const CODE_VIEWER_ANNOTATION_RANGE: &str = "mb-2 text-ui-base text-foreground-subtle";
pub const CODE_VIEWER_ANNOTATION_TEXT: &str =
    "whitespace-pre-wrap text-ui-base leading-relaxed text-foreground";
pub const CODE_VIEWER_ANNOTATION_ACTIONS: &str = "mt-2 flex items-center justify-end";
pub const CODE_VIEWER_DELETE_BUTTON: &str =
    "text-foreground-subtle hover:text-foreground";
pub const CODE_VIEWER_DRAFT_ACTIONS: &str = "mt-2 flex items-center justify-end gap-2";
pub const CODE_VIEWER_DRAFT_TEXTAREA: &str = "min-h-16 w-full resize-none rounded-lg border-input-border bg-input text-ui-base text-foreground placeholder:text-foreground-subtlest hover:border-input-border-hover focus-visible:border-input-border-focused focus-visible:bg-input-focused focus-visible:ring-0 md:text-ui-base";

/// 默认字号（px）。
pub const CODE_VIEWER_DEFAULT_FONT_SIZE_PX: f64 = 12.0;

/// 评论草稿的默认行数。
pub const CODE_VIEWER_DRAFT_ROWS: i64 = 3;

/// 内建 comment 按钮的位置补丁（逐字移植 TS 的 `CODE_VIEWER_UNSAFE_CSS`）。
///
/// 原注释点明的原因：内建 `comment +` 默认贴在行号右侧，用户拖拽时容易和
/// 代码起点混在一起。这里**只调整 Shadow DOM 内 gutter utility 的位置，
/// 不接管 pointer 事件**，避免破坏多行拖拽 range。
pub const CODE_VIEWER_UNSAFE_CSS: &str = concat!(
    "[data-gutter-utility-slot]{left:auto;right:4px;justify-content:flex-end;align-items:center;}",
    "[data-utility-button]{margin-left:0;margin-right:0;}",
    "@media (hover:hover) and (pointer:fine){",
    "[data-utility-button]::after{content:var(--code-comment-add-tooltip);position:absolute;left:calc(100% + 6px);top:50%;transform:translateY(-50%);max-width:16rem;white-space:nowrap;pointer-events:none;opacity:0;z-index:5;border:1px solid var(--color-border);border-radius:8px;background:var(--color-tooltip);color:var(--color-tooltip-foreground);padding:4px 8px;font-family:var(--diffs-header-font-family,var(--diffs-header-font-fallback));font-size:12px;line-height:16px;box-shadow:0 4px 12px rgb(0 0 0 / 0.14);}",
    "[data-utility-button]:hover::after,[data-utility-button]:focus-visible::after{opacity:1;}",
    "}",
);

// ---- 区间与评论 ----

/// 评论区间（1 起的**文件行号**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodeCommentRange {
    pub start_line: i64,
    pub end_line: i64,
}

/// 一条评论（TS `CodeCommentPreview`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeCommentPreview {
    pub id: String,
    pub start_line: i64,
    pub end_line: i64,
    pub source_path: Option<String>,
    pub source_title: String,
    pub selected_text: String,
    pub comment: String,
}

/// 选中行区间（1 起的**文件内相对行号**，`@pierre/diffs` 的坐标）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectedLineRange {
    pub start: i64,
    pub end: i64,
}

/// `normalizeRange`：把区间摆正（`start <= end`）。
pub fn normalize_code_comment_range(range: CodeCommentRange) -> CodeCommentRange {
    CodeCommentRange {
        start_line: range.start_line.min(range.end_line),
        end_line: range.start_line.max(range.end_line),
    }
}

/// `selectedRangeToCodeCommentRange`。
pub fn selected_range_to_code_comment_range(
    range: SelectedLineRange,
    first_line_number: i64,
) -> CodeCommentRange {
    normalize_code_comment_range(CodeCommentRange {
        start_line: first_line_number + range.start - 1,
        end_line: first_line_number + range.end - 1,
    })
}

/// `codeCommentRangeToSelectedRange`。
pub fn code_comment_range_to_selected_range(
    range: CodeCommentRange,
    first_line_number: i64,
) -> SelectedLineRange {
    let normalized = normalize_code_comment_range(range);
    SelectedLineRange {
        start: normalized.start_line - first_line_number + 1,
        end: normalized.end_line - first_line_number + 1,
    }
}

/// `getTextForLineRange`：按区间取出代码文本。
///
/// 区间外的行**夹到文件范围内**；区间完全在文件外时返回空串
/// （提交时据此拒绝空选区）。
pub fn get_text_for_line_range(
    code: &str,
    first_line_number: i64,
    range: CodeCommentRange,
) -> String {
    let lines: Vec<&str> = code.split('\n').collect();
    let normalized = normalize_code_comment_range(range);
    let start_index = 0_i64.max(normalized.start_line - first_line_number);
    let end_index = (lines.len() as i64 - 1).min(normalized.end_line - first_line_number);
    if start_index > end_index {
        return String::new();
    }
    lines[start_index as usize..=end_index as usize].join("\n")
}

/// `formatCommentRange`：单行用 `comment_line`，多行用 `comment_range`。
///
/// `replaceAll` 语义：占位符出现多次要全部替换。
pub fn format_comment_range(labels: &CodeCommentLabels, range: CodeCommentRange) -> String {
    let normalized = normalize_code_comment_range(range);
    if normalized.start_line == normalized.end_line {
        return labels
            .comment_line
            .replace("{line}", &normalized.start_line.to_string());
    }
    labels
        .comment_range
        .replace("{startLine}", &normalized.start_line.to_string())
        .replace("{endLine}", &normalized.end_line.to_string())
}

/// 一条行注释（TS 的 `LineAnnotation<CodeViewerAnnotationMetadata>`）。
///
/// `line_number` 是**算好的锚点**（`@pierre/diffs` 的坐标），不是原始的文件行号 ——
/// 见模块移植点 1 的坐标系表。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodeViewerLineAnnotation {
    Comment {
        comment: CodeCommentPreview,
        line_number: i64,
    },
    Draft {
        range: CodeCommentRange,
        line_number: i64,
    },
}

impl CodeViewerLineAnnotation {
    /// 注释锚点行（`@pierre/diffs` 的坐标）。
    pub fn line_number(&self) -> i64 {
        match self {
            CodeViewerLineAnnotation::Comment { line_number, .. }
            | CodeViewerLineAnnotation::Draft { line_number, .. } => *line_number,
        }
    }
}

/// `lineAnnotations` memo。
///
/// 注意：已存在的评论会跳过锚点行小于 `1` 的（早于 `firstLineNumber` 的评论
/// 挂不上去），而**草稿分支没有这个检查** —— 逐字保留。
pub fn build_line_annotations(
    comments: &[CodeCommentPreview],
    active_draft_range: Option<CodeCommentRange>,
    can_create_comment: bool,
    first_line_number: i64,
) -> Vec<CodeViewerLineAnnotation> {
    let mut annotations = Vec::new();
    for comment in comments {
        let line_number = normalize_code_comment_range(CodeCommentRange {
            start_line: comment.start_line,
            end_line: comment.end_line,
        })
        .end_line
            - first_line_number
            + 1;
        if line_number < 1 {
            continue;
        }
        annotations.push(CodeViewerLineAnnotation::Comment {
            comment: comment.clone(),
            line_number,
        });
    }
    if can_create_comment {
        if let Some(range) = active_draft_range {
            annotations.push(CodeViewerLineAnnotation::Draft {
                line_number: normalize_code_comment_range(range).end_line - first_line_number + 1,
                range,
            });
        }
    }
    annotations
}

/// `selectedLines` memo。
pub fn build_selected_lines(
    active_draft_range: Option<CodeCommentRange>,
    can_create_comment: bool,
    focused_range: Option<CodeCommentRange>,
    first_line_number: i64,
) -> Option<SelectedLineRange> {
    if can_create_comment {
        if let Some(range) = active_draft_range {
            return Some(code_comment_range_to_selected_range(range, first_line_number));
        }
    }
    focused_range.map(|range| code_comment_range_to_selected_range(range, first_line_number))
}

// ---- 能力开关 ----

/// 评论能力（见模块移植点 2）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodeViewerCapabilities {
    /// `Boolean(onSubmitCodeComment)`。
    pub can_create_comment: bool,
    /// `canCreateComment && enableLineSelection`。
    pub can_use_comment_line_selection: bool,
    /// `canCreateComment && enableGutterUtility`。
    pub can_use_comment_gutter_utility: bool,
}

impl CodeViewerCapabilities {
    pub fn resolve(can_create_comment: bool, enable_line_selection: bool, enable_gutter_utility: bool) -> Self {
        Self {
            can_create_comment,
            can_use_comment_line_selection: can_create_comment && enable_line_selection,
            can_use_comment_gutter_utility: can_create_comment && enable_gutter_utility,
        }
    }

    /// `lineHoverHighlight`：两个交互开关都没开时是 `"disabled"`。
    pub fn line_hover_highlight(&self) -> &'static str {
        if self.can_use_comment_line_selection || self.can_use_comment_gutter_utility {
            "both"
        } else {
            "disabled"
        }
    }
}

// ---- 渲染参数 ----

/// 传给 `@pierre/diffs` 的渲染参数（本移植只产出，不消费）。
#[derive(Clone, Debug, PartialEq)]
pub struct CodeViewerRenderOptions {
    pub disable_file_header: bool,
    pub disable_line_numbers: bool,
    pub overflow: &'static str,
    pub theme: Option<String>,
    pub preferred_highlighter: &'static str,
    pub enable_line_selection: bool,
    pub enable_gutter_utility: bool,
    pub line_hover_highlight: &'static str,
    /// `CODE_VIEWER_UNSAFE_CSS + markedLinesCss`。
    pub unsafe_css: String,
}

/// `options` memo。
pub fn build_code_viewer_options(
    capabilities: CodeViewerCapabilities,
    show_line_numbers: bool,
    wrap_long_lines: bool,
    theme: Option<&str>,
    marked_lines_css: &str,
) -> CodeViewerRenderOptions {
    CodeViewerRenderOptions {
        // 文件头被禁用：外层已经有语言/文件名信息
        disable_file_header: true,
        disable_line_numbers: !show_line_numbers,
        overflow: if wrap_long_lines { "wrap" } else { "scroll" },
        theme: theme.map(|value| value.to_string()),
        preferred_highlighter: DIFFS_PREFERRED_HIGHLIGHTER,
        enable_line_selection: capabilities.can_use_comment_line_selection,
        enable_gutter_utility: capabilities.can_use_comment_gutter_utility,
        line_hover_highlight: capabilities.line_hover_highlight(),
        unsafe_css: format!("{CODE_VIEWER_UNSAFE_CSS}{marked_lines_css}"),
    }
}

/// `@pierre/diffs` 的 `FileContents`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeViewerFile {
    pub name: String,
    pub contents: String,
    pub lang: String,
    pub cache_key: String,
}

/// `JSON.stringify(value)`（用于把文案塞进 CSS 自定义属性）。
pub fn to_css_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `hashCodeViewerContent`：djb2 变体（`hash * 33 ^ codeUnit`），结果取无符号 32 位后转 36 进制。
///
/// 注意 `code.charCodeAt(i)` 取的是 **UTF-16 码元**，所以用 `encode_utf16()`
/// 而不是 `chars()` —— 否则非 BMP 字符（emoji 等）算出来的 hash 与 TS 不同，
/// `cacheKey` 也就对不上。
pub fn hash_code_viewer_content(code: &str) -> String {
    let mut hash: u32 = 5381;
    for unit in code.encode_utf16() {
        hash = hash.wrapping_mul(33) ^ (unit as u32);
    }
    to_base36(hash)
}

/// `Number.prototype.toString(36)`（非负整数）。
fn to_base36(mut value: u32) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_string();
    }
    let mut buffer = Vec::new();
    while value > 0 {
        buffer.push(DIGITS[(value % 36) as usize]);
        value /= 36;
    }
    buffer.reverse();
    String::from_utf8(buffer).expect("base36 字母表是 ASCII")
}

/// `createCodeViewerFile`（见模块移植点 3）。
pub fn create_code_viewer_file(
    code: &str,
    enable_syntax_highlighting: bool,
    language: &str,
    theme: Option<&str>,
) -> CodeViewerFile {
    let lang = if enable_syntax_highlighting {
        language.to_string()
    } else {
        "text".to_string()
    };
    let name = if language.is_empty() {
        "preview.txt".to_string()
    } else {
        format!("preview.{language}")
    };
    let theme_cache_key = theme.unwrap_or("auto");
    // `code.length` 是 JS 字符串长度 = UTF-16 码元数
    let code_length = code.encode_utf16().count();
    CodeViewerFile {
        cache_key: format!(
            "{theme_cache_key}:{name}:{lang}:{code_length}:{}",
            hash_code_viewer_content(code)
        ),
        name,
        contents: code.to_string(),
        lang,
    }
}

/// `codeViewerMarkedLinesCss`。
///
/// 被点名行的行号格是 `@pierre/diffs` 在 Shadow DOM 里画的
/// `[data-column-number="<行号>"]`，外层 class 进不去，只能随 `unsafeCSS` 注入；
/// 颜色走应用 token（自定义属性能穿透 shadow root），深浅主题自动跟随。
///
/// 非正整数与重复值丢弃，空集合返回空串。
pub fn code_viewer_marked_lines_css(lines: Option<&[i64]>) -> String {
    let mut unique: Vec<i64> = Vec::new();
    for line in lines.unwrap_or(&[]) {
        if *line > 0 && !unique.contains(line) {
            unique.push(*line);
        }
    }
    if unique.is_empty() {
        return String::new();
    }
    let selector = unique
        .iter()
        .map(|line| format!("[data-column-number=\"{line}\"]"))
        .collect::<Vec<_>>()
        .join(",");
    format!("{selector}{{color:var(--color-warning);}}")
}

/// `resolveCodeViewerColorScheme`。
///
/// `theme` 为 `None` 时看文档根节点有没有 `dark` 类；给了主题则**只看主题**
/// （见模块移植点 4）。
pub fn resolve_code_viewer_color_scheme(
    theme: Option<&str>,
    document_is_dark: bool,
) -> &'static str {
    match theme {
        None => {
            if document_is_dark {
                "dark"
            } else {
                "light"
            }
        }
        Some(theme) => {
            if is_dark_code_preview_theme(theme) {
                "dark"
            } else {
                "light"
            }
        }
    }
}

/// 阅读器的样式字符串（`color-scheme` + `--diffs-*` 变量）。
///
/// 见模块移植点 4 与 TS 里关于字体 fallback 的注释：
/// `@pierre/diffs` 在 Shadow DOM 内用自己的等宽 fallback，
/// Windows 中文会落到宋体，所以要显式透传 `--font-mono`。
pub fn code_viewer_style(
    theme: Option<&str>,
    document_is_dark: bool,
    font_size_px: f64,
    add_comment_tooltip: Option<&str>,
    extra_style: &str,
) -> String {
    format!(
        "color-scheme: {}; --diffs-bg: var(--color-background); --diffs-light-bg: var(--color-background); --diffs-dark-bg: var(--color-background); --diffs-font-family: var(--font-mono); --diffs-font-size: {}px; --code-comment-add-tooltip: {}; {}",
        resolve_code_viewer_color_scheme(theme, document_is_dark),
        font_size_px,
        to_css_string(add_comment_tooltip.unwrap_or("")),
        extra_style
    )
}

// ---- 快捷键 ----

/// `isCodeCommentSubmitShortcut`：`Enter` + 平台主修饰键。
pub fn is_code_comment_submit_shortcut(
    key: Key,
    ctrl_key: bool,
    meta_key: bool,
    apple_keyboard: bool,
) -> bool {
    if !matches!(key, Key::Enter) {
        return false;
    }
    if apple_keyboard {
        meta_key
    } else {
        ctrl_key
    }
}

/// `isCodeCommentCancelShortcut`。
pub fn is_code_comment_cancel_shortcut(key: Key) -> bool {
    matches!(key, Key::Escape)
}

/// `formatCodeCommentSubmitShortcutLabel`。
pub fn format_code_comment_submit_shortcut_label(
    platform_info: &KeyboardShortcutPlatformInfo,
) -> String {
    format_command_shortcut_label("↵", platform_info)
}

/// `formatCodeCommentCancelShortcutLabel`。
pub fn format_code_comment_cancel_shortcut_label() -> &'static str {
    "Esc"
}

// ---- 提交 ----

/// `onSubmitCodeComment` 的参数（TS 的入参对象）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeCommentSubmission {
    pub range: CodeCommentRange,
    pub selected_text: String,
    pub comment: String,
}

/// `handleSubmitDraft`：选区取不到文本就**不提交**。
///
/// 返回 `None` 表示不满足提交条件（没有草稿区间，或选中的代码是空白）。
pub fn build_code_comment_submission(
    code: &str,
    first_line_number: i64,
    active_draft_range: Option<CodeCommentRange>,
    draft_text: &str,
) -> Option<CodeCommentSubmission> {
    let range = active_draft_range?;
    let selected_text = get_text_for_line_range(code, first_line_number, range);
    if selected_text.trim().is_empty() {
        return None;
    }
    Some(CodeCommentSubmission {
        range: normalize_code_comment_range(range),
        selected_text,
        comment: draft_text.trim().to_string(),
    })
}

// ---- 组件 ----

/// 一条已提交评论的展示（TS `CodeCommentAnnotation`）。
#[component]
pub fn CodeCommentAnnotation(
    comment: CodeCommentPreview,
    labels: CodeCommentLabels,
    #[props(default = true)] show_range: bool,
    #[props(default)] on_delete: EventHandler<String>,
) -> Element {
    let comment_id = comment.id.clone();
    let has_text = !comment.comment.trim().is_empty();
    let has_delete = on_delete != EventHandler::default();
    rsx! {
        div {
            "data-code-comment-id": comment.id.clone(),
            class: CODE_VIEWER_ANNOTATION,
            if show_range {
                div { class: CODE_VIEWER_ANNOTATION_RANGE,
                    {format_comment_range(
                        &labels,
                        CodeCommentRange {
                            start_line: comment.start_line,
                            end_line: comment.end_line,
                        },
                    )}
                }
            }
            if has_text {
                div { class: CODE_VIEWER_ANNOTATION_TEXT, "{comment.comment}" }
            }
            if has_delete {
                div { class: CODE_VIEWER_ANNOTATION_ACTIONS,
                    Button {
                        variant: ButtonVariant::Ghost,
                        class: CODE_VIEWER_DELETE_BUTTON.to_string(),
                        aria_label: labels.delete_comment.clone(),
                        onclick: move |_| on_delete.call(comment_id.clone()),
                        IconTrash { class: "size-4", aria_hidden: true }
                        "{labels.delete_comment}"
                    }
                }
            }
        }
    }
}

/// 写评论的草稿（TS `CommentDraft`）。
#[component]
pub fn CodeCommentDraft(
    range: CodeCommentRange,
    labels: CodeCommentLabels,
    #[props(default, into)] value: String,
    #[props(default)] platform_info: Option<KeyboardShortcutPlatformInfo>,
    #[props(default)] onvaluechange: EventHandler<String>,
    #[props(default)] onsubmit: EventHandler<()>,
    #[props(default)] oncancel: EventHandler<()>,
) -> Element {
    let platform = platform_info.unwrap_or_default();
    let cancel_label = format!(
        "{} ({})",
        labels.cancel_comment,
        format_code_comment_cancel_shortcut_label()
    );
    let submit_label = format!(
        "{} ({})",
        labels.submit_comment,
        format_code_comment_submit_shortcut_label(&platform)
    );
    let apple = is_apple_keyboard_platform(&platform);
    let submit_label_for_key = submit_label.clone();

    rsx! {
        div { class: CODE_VIEWER_ANNOTATION,
            div { class: CODE_VIEWER_ANNOTATION_RANGE, {format_comment_range(&labels, range)} }
            Textarea {
                value: Some(value),
                placeholder: Some(labels.comment_placeholder.clone()),
                rows: Some(CODE_VIEWER_DRAFT_ROWS),
                aria_label: Some(labels.comment_placeholder.clone()),
                autofocus: true,
                class: CODE_VIEWER_DRAFT_TEXTAREA.to_string(),
                oninput: move |e: FormEvent| onvaluechange.call(e.value()),
                onkeydown: move |e: KeyboardEvent| {
                    let modifiers = e.modifiers();
                    // 提交判断在取消之前（见模块移植点 5）
                    if is_code_comment_submit_shortcut(
                        e.key(),
                        modifiers.contains(Modifiers::CONTROL),
                        modifiers.contains(Modifiers::META),
                        apple,
                    ) {
                        e.prevent_default();
                        onsubmit.call(());
                        return;
                    }
                    if is_code_comment_cancel_shortcut(e.key()) {
                        e.prevent_default();
                        oncancel.call(());
                    }
                },
            }
            div { class: CODE_VIEWER_DRAFT_ACTIONS,
                Button {
                    variant: ButtonVariant::Ghost,
                    aria_label: cancel_label.clone(),
                    onclick: move |_| oncancel.call(()),
                    "{cancel_label}"
                }
                Button {
                    aria_label: submit_label_for_key.clone(),
                    onclick: move |_| onsubmit.call(()),
                    "{submit_label_for_key}"
                }
            }
        }
    }
}

/// 代码预览外壳。
///
/// **不含**代码画布本身 —— 那由宿主以 `children` 注入
/// （对应 TS 里的 `@pierre/diffs` 的 `<File>`）。
#[component]
pub fn CodeViewer(
    #[props(default, into)] class: String,
    code: String,
    #[props(default)] language: String,
    #[props(default = true)] enable_syntax_highlighting: bool,
    #[props(default = true)] show_line_numbers: bool,
    #[props(default)] theme: Option<String>,
    #[props(default)] wrap_long_lines: bool,
    #[props(default = CODE_VIEWER_DEFAULT_FONT_SIZE_PX)] font_size_px: f64,
    #[props(default = 1)] first_line_number: i64,
    #[props(default)] comments: Vec<CodeCommentPreview>,
    #[props(default)] top_comment: Option<CodeCommentPreview>,
    #[props(default = true)] top_comment_show_range: bool,
    #[props(default)] top_comment_notice: Option<String>,
    #[props(default)] focused_range: Option<CodeCommentRange>,
    #[props(default)] marked_lines: Option<Vec<i64>>,
    #[props(default)] enable_line_selection: bool,
    #[props(default)] enable_gutter_utility: bool,
    #[props(default)] labels: Option<CodeCommentLabelOverrides>,
    /// 当前草稿区间（`None` = 没有草稿）。
    #[props(default)] draft_range: Option<CodeCommentRange>,
    #[props(default, into)] draft_text: String,
    #[props(default)] platform_info: Option<KeyboardShortcutPlatformInfo>,
    /// 文档根节点是否有 `dark` 类（宿主探测）。
    #[props(default)] document_is_dark: bool,
    /// 追加到样式串末尾（覆盖上面的 CSS 变量，顺序与 TS 一致）。
    #[props(default, into)] extra_style: String,
    /// 注入的代码渲染器。
    children: Element,
    #[props(default)] on_submit_code_comment: EventHandler<CodeCommentSubmission>,
    #[props(default)] on_delete_code_comment: EventHandler<String>,
    #[props(default)] on_draft_change: EventHandler<Option<CodeCommentRange>>,
    #[props(default)] on_draft_text_change: EventHandler<String>,
) -> Element {
    let labels = CodeCommentLabels::merged(labels.as_ref());
    let capabilities = CodeViewerCapabilities::resolve(
        on_submit_code_comment != EventHandler::default(),
        enable_line_selection,
        enable_gutter_utility,
    );
    let file = create_code_viewer_file(
        &code,
        enable_syntax_highlighting,
        &language,
        theme.as_deref(),
    );
    let marked_lines_css = code_viewer_marked_lines_css(marked_lines.as_deref());
    let options = build_code_viewer_options(
        capabilities,
        show_line_numbers,
        wrap_long_lines,
        theme.as_deref(),
        &marked_lines_css,
    );
    let style = code_viewer_style(
        theme.as_deref(),
        document_is_dark,
        font_size_px,
        capabilities
            .can_use_comment_gutter_utility
            .then_some(labels.add_comment_tooltip.as_str()),
        &extra_style,
    );

    let annotations = build_line_annotations(
        &comments,
        draft_range,
        capabilities.can_create_comment,
        first_line_number,
    );
    let selected_lines = build_selected_lines(
        draft_range,
        capabilities.can_create_comment,
        focused_range,
        first_line_number,
    );
    let annotation_count = annotations.len();
    let code_for_submit = code.clone();
    let draft_text_for_submit = draft_text.clone();
    let submit = move |_| {
        if let Some(submission) = build_code_comment_submission(
            &code_for_submit,
            first_line_number,
            draft_range,
            &draft_text_for_submit,
        ) {
            on_submit_code_comment.call(submission);
        }
    };
    let cancel = move |_| {
        on_draft_change.call(None);
        on_draft_text_change.call(String::new());
    };

    rsx! {
        div {
            class: cn(&[CODE_VIEWER_BASE, &class]),
            "data-slot": "code-viewer",
            "data-language": language.clone(),
            style: style,
            // `options` / `file` / `annotations` / `selected_lines` 是给注入渲染器的
            // 参数；宿主通过 `data-*` 读不到，这里挂成只读属性方便宿主与测试断言。
            "data-code-viewer-cache-key": file.cache_key.clone(),
            "data-code-viewer-lang": file.lang.clone(),
            "data-code-viewer-overflow": options.overflow,
            "data-code-viewer-line-hover": options.line_hover_highlight,
            "data-code-viewer-highlighter": options.preferred_highlighter,
            "data-code-viewer-line-selection": options.enable_line_selection.then_some("true"),
            "data-code-viewer-gutter-utility": options.enable_gutter_utility.then_some("true"),
            "data-code-viewer-selected-lines": selected_lines
                .map(|range| format!("{}-{}", range.start, range.end)),
            "data-code-viewer-annotation-count": annotation_count.to_string(),

            if let Some(notice) = top_comment_notice {
                div { "data-code-review-target-warning": "true", class: CODE_VIEWER_NOTICE, "{notice}" }
            }
            if let Some(comment) = top_comment {
                CodeCommentAnnotation {
                    comment: comment,
                    labels: labels.clone(),
                    show_range: top_comment_show_range,
                    on_delete: on_delete_code_comment,
                }
            }

            {children}

            if let Some(range) = draft_range {
                CodeCommentDraft {
                    range: range,
                    labels: labels.clone(),
                    value: draft_text.clone(),
                    platform_info: platform_info,
                    onvaluechange: move |text: String| on_draft_text_change.call(text),
                    onsubmit: submit,
                    oncancel: cancel,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment(id: &str, start: i64, end: i64, text: &str) -> CodeCommentPreview {
        CodeCommentPreview {
            id: id.to_string(),
            start_line: start,
            end_line: end,
            source_path: None,
            source_title: String::new(),
            selected_text: String::new(),
            comment: text.to_string(),
        }
    }

    // ---- 文案 ----

    #[test]
    fn default_labels_match_the_ts_defaults() {
        let l = CodeCommentLabels::default();
        assert_eq!(l.add_comment, "Add comment");
        assert_eq!(l.add_comment_tooltip, "Click or drag to comment");
        assert_eq!(l.comment_placeholder, "Comment");
        assert_eq!(l.submit_comment, "Add comment");
        assert_eq!(l.cancel_comment, "Cancel");
        assert_eq!(l.delete_comment, "Delete comment");
        assert_eq!(l.comment_line, "Line {line}");
        assert_eq!(l.comment_range, "Lines {startLine}-{endLine}");
    }

    #[test]
    fn label_overrides_only_replace_what_is_given() {
        let overrides = CodeCommentLabelOverrides {
            add_comment: Some("添加评论".to_string()),
            comment_line: Some("第 {line} 行".to_string()),
            ..Default::default()
        };
        let merged = CodeCommentLabels::merged(Some(&overrides));
        assert_eq!(merged.add_comment, "添加评论");
        assert_eq!(merged.comment_line, "第 {line} 行");
        // 其余保持默认
        assert_eq!(merged.delete_comment, "Delete comment");
        assert_eq!(merged.comment_range, "Lines {startLine}-{endLine}");
        assert_eq!(CodeCommentLabels::merged(None), CodeCommentLabels::default());
    }

    // ---- 区间换算 ----

    #[test]
    fn normalize_range_orders_the_bounds() {
        let normalized = normalize_code_comment_range(CodeCommentRange {
            start_line: 9,
            end_line: 3,
        });
        assert_eq!(normalized.start_line, 3);
        assert_eq!(normalized.end_line, 9);
        // 已经有序时不动
        let same = normalize_code_comment_range(CodeCommentRange {
            start_line: 3,
            end_line: 9,
        });
        assert_eq!(same.start_line, 3);
        assert_eq!(same.end_line, 9);
    }

    /// `firstLineNumber` 是文件行号与「文件内相对行号」之间的偏移。
    #[test]
    fn selected_range_to_comment_range_applies_the_file_offset() {
        // 文件从第 100 行开始：相对 1..3 → 文件 100..102
        let range = selected_range_to_code_comment_range(
            SelectedLineRange { start: 1, end: 3 },
            100,
        );
        assert_eq!(range.start_line, 100);
        assert_eq!(range.end_line, 102);
        // 反向选择会被摆正
        let reversed =
            selected_range_to_code_comment_range(SelectedLineRange { start: 5, end: 2 }, 1);
        assert_eq!(reversed.start_line, 2);
        assert_eq!(reversed.end_line, 5);
    }

    #[test]
    fn comment_range_to_selected_range_is_the_inverse() {
        let selected = SelectedLineRange { start: 4, end: 7 };
        let comment_range = selected_range_to_code_comment_range(selected, 50);
        let back = code_comment_range_to_selected_range(comment_range, 50);
        assert_eq!(back, selected);
    }

    #[test]
    fn the_selected_range_conversion_normalizes_first() {
        let range = code_comment_range_to_selected_range(
            CodeCommentRange {
                start_line: 9,
                end_line: 5,
            },
            1,
        );
        assert_eq!(range.start, 5);
        assert_eq!(range.end, 9);
    }

    // ---- 取文本 ----

    #[test]
    fn text_for_line_range_is_inclusive_on_both_ends() {
        let code = "a\nb\nc\nd";
        let text = get_text_for_line_range(
            code,
            1,
            CodeCommentRange {
                start_line: 2,
                end_line: 3,
            },
        );
        assert_eq!(text, "b\nc");
        // 单行
        assert_eq!(
            get_text_for_line_range(
                code,
                1,
                CodeCommentRange {
                    start_line: 4,
                    end_line: 4
                }
            ),
            "d"
        );
    }

    #[test]
    fn text_for_line_range_respects_the_first_line_number() {
        let code = "x\ny\nz";
        // 文件从第 10 行开始，区间 11..12 → 索引 1..2
        let text = get_text_for_line_range(
            code,
            10,
            CodeCommentRange {
                start_line: 11,
                end_line: 12,
            },
        );
        assert_eq!(text, "y\nz");
    }

    #[test]
    fn text_for_line_range_clamps_outside_the_file() {
        let code = "a\nb\nc";
        // 起点早于文件开头 → 夹到 0
        assert_eq!(
            get_text_for_line_range(
                code,
                1,
                CodeCommentRange {
                    start_line: -5,
                    end_line: 2
                }
            ),
            "a\nb"
        );
        // 终点超过文件末尾 → 夹到最后一行
        assert_eq!(
            get_text_for_line_range(
                code,
                1,
                CodeCommentRange {
                    start_line: 2,
                    end_line: 99
                }
            ),
            "b\nc"
        );
    }

    #[test]
    fn text_for_line_range_is_empty_when_entirely_outside() {
        let code = "a\nb\nc";
        assert_eq!(
            get_text_for_line_range(
                code,
                1,
                CodeCommentRange {
                    start_line: 10,
                    end_line: 12
                }
            ),
            ""
        );
    }

    /// JS 的 `split("\n")` 会为结尾的换行留一个空串。
    #[test]
    fn text_for_line_range_keeps_a_trailing_empty_line() {
        let code = "a\nb\n";
        // 3 行：["a", "b", ""]
        assert_eq!(
            get_text_for_line_range(
                code,
                1,
                CodeCommentRange {
                    start_line: 3,
                    end_line: 3
                }
            ),
            ""
        );
        assert_eq!(
            get_text_for_line_range(
                code,
                1,
                CodeCommentRange {
                    start_line: 1,
                    end_line: 3
                }
            ),
            "a\nb\n"
        );
    }

    // ---- 区间文案 ----

    #[test]
    fn format_comment_range_picks_single_or_multi_template() {
        let labels = CodeCommentLabels::default();
        assert_eq!(
            format_comment_range(
                &labels,
                CodeCommentRange {
                    start_line: 7,
                    end_line: 7
                }
            ),
            "Line 7"
        );
        assert_eq!(
            format_comment_range(
                &labels,
                CodeCommentRange {
                    start_line: 7,
                    end_line: 9
                }
            ),
            "Lines 7-9"
        );
    }

    #[test]
    fn format_comment_range_normalizes_and_replaces_every_placeholder() {
        let labels = CodeCommentLabels {
            comment_range: "{startLine}/{endLine} ({startLine}-{endLine})".to_string(),
            ..Default::default()
        };
        assert_eq!(
            format_comment_range(
                &labels,
                CodeCommentRange {
                    start_line: 9,
                    end_line: 2
                }
            ),
            "2/9 (2-9)"
        );
    }

    // ---- 行注释 ----

    #[test]
    fn annotations_anchor_on_the_end_line() {
        let annotations = build_line_annotations(&[comment("c1", 3, 7, "x")], None, false, 1);
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].line_number(), 7);
    }

    #[test]
    fn annotations_skip_comments_before_the_first_line() {
        // 评论在第 2 行，但文件从第 10 行开始 → 锚点 2-10+1 = -7 < 1 → 跳过
        let annotations = build_line_annotations(&[comment("c1", 1, 2, "x")], None, false, 10);
        assert!(annotations.is_empty());
        // 正好落在第一行则保留
        let kept = build_line_annotations(&[comment("c1", 10, 10, "x")], None, false, 10);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].line_number(), 1);
    }

    #[test]
    fn annotations_are_ordered_comments_then_draft() {
        let annotations = build_line_annotations(
            &[comment("c1", 1, 1, "a"), comment("c2", 2, 2, "b")],
            Some(CodeCommentRange {
                start_line: 5,
                end_line: 6,
            }),
            true,
            1,
        );
        assert_eq!(annotations.len(), 3);
        assert!(matches!(annotations[0], CodeViewerLineAnnotation::Comment { .. }));
        assert!(matches!(annotations[1], CodeViewerLineAnnotation::Comment { .. }));
        assert!(matches!(annotations[2], CodeViewerLineAnnotation::Draft { .. }));
        assert_eq!(annotations[2].line_number(), 6);
    }

    /// 草稿分支**没有**锚点下界检查（逐字保留 TS 行为）。
    #[test]
    fn the_draft_annotation_has_no_lower_bound_check() {
        let annotations = build_line_annotations(
            &[],
            Some(CodeCommentRange {
                start_line: 1,
                end_line: 2,
            }),
            true,
            10,
        );
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].line_number(), -7);
    }

    #[test]
    fn the_draft_annotation_requires_the_capability() {
        let range = Some(CodeCommentRange {
            start_line: 1,
            end_line: 1,
        });
        assert!(build_line_annotations(&[], range, false, 1).is_empty());
        assert_eq!(build_line_annotations(&[], range, true, 1).len(), 1);
    }

    // ---- 选中行 ----

    #[test]
    fn selected_lines_prefers_the_draft_over_the_focused_range() {
        let draft = Some(CodeCommentRange {
            start_line: 3,
            end_line: 4,
        });
        let focused = Some(CodeCommentRange {
            start_line: 8,
            end_line: 9,
        });
        assert_eq!(
            build_selected_lines(draft, true, focused, 1),
            Some(SelectedLineRange { start: 3, end: 4 })
        );
        // 没有草稿能力时退回 focused
        assert_eq!(
            build_selected_lines(draft, false, focused, 1),
            Some(SelectedLineRange { start: 8, end: 9 })
        );
    }

    #[test]
    fn selected_lines_is_none_without_a_draft_or_a_focus() {
        assert_eq!(build_selected_lines(None, true, None, 1), None);
    }

    // ---- 能力开关 ----

    #[test]
    fn capabilities_gate_both_interactions_on_the_comment_handler() {
        let all_off = CodeViewerCapabilities::resolve(false, true, true);
        assert!(!all_off.can_create_comment);
        assert!(!all_off.can_use_comment_line_selection);
        assert!(!all_off.can_use_comment_gutter_utility);

        let all_on = CodeViewerCapabilities::resolve(true, true, true);
        assert!(all_on.can_create_comment);
        assert!(all_on.can_use_comment_line_selection);
        assert!(all_on.can_use_comment_gutter_utility);

        let selection_only = CodeViewerCapabilities::resolve(true, true, false);
        assert!(selection_only.can_use_comment_line_selection);
        assert!(!selection_only.can_use_comment_gutter_utility);
    }

    #[test]
    fn line_hover_highlight_is_disabled_when_nothing_is_enabled() {
        assert_eq!(
            CodeViewerCapabilities::resolve(false, true, true).line_hover_highlight(),
            "disabled"
        );
        assert_eq!(
            CodeViewerCapabilities::resolve(true, false, false).line_hover_highlight(),
            "disabled"
        );
        assert_eq!(
            CodeViewerCapabilities::resolve(true, true, false).line_hover_highlight(),
            "both"
        );
        assert_eq!(
            CodeViewerCapabilities::resolve(true, false, true).line_hover_highlight(),
            "both"
        );
    }

    // ---- 渲染参数 ----

    #[test]
    fn options_disable_the_file_header_and_flip_line_numbers() {
        let caps = CodeViewerCapabilities::resolve(true, false, false);
        let shown = build_code_viewer_options(caps, true, false, None, "");
        assert!(shown.disable_file_header);
        assert!(!shown.disable_line_numbers);

        let hidden = build_code_viewer_options(caps, false, false, None, "");
        assert!(hidden.disable_line_numbers);
    }

    #[test]
    fn options_switch_the_overflow_mode() {
        let caps = CodeViewerCapabilities::resolve(false, false, false);
        assert_eq!(
            build_code_viewer_options(caps, true, false, None, "").overflow,
            "scroll"
        );
        assert_eq!(
            build_code_viewer_options(caps, true, true, None, "").overflow,
            "wrap"
        );
    }

    #[test]
    fn options_carry_the_highlighter_and_theme() {
        let caps = CodeViewerCapabilities::resolve(false, false, false);
        let options = build_code_viewer_options(caps, true, false, Some("github-dark"), "");
        assert_eq!(options.preferred_highlighter, DIFFS_PREFERRED_HIGHLIGHTER);
        assert_eq!(options.theme, Some("github-dark".to_string()));
        assert_eq!(
            build_code_viewer_options(caps, true, false, None, "").theme,
            None
        );
    }

    #[test]
    fn options_append_the_marked_lines_css_after_the_base_patch() {
        let caps = CodeViewerCapabilities::resolve(false, false, false);
        let options = build_code_viewer_options(caps, true, false, None, "[data-column-number=\"3\"]{}");
        assert!(options.unsafe_css.starts_with(CODE_VIEWER_UNSAFE_CSS));
        assert!(options.unsafe_css.ends_with("[data-column-number=\"3\"]{}"));
        // 顺序很重要：markedLines 必须能覆盖基础补丁
        let base_len = CODE_VIEWER_UNSAFE_CSS.len();
        assert!(options.unsafe_css.len() > base_len);
    }

    // ---- 行号着色 CSS ----

    #[test]
    fn marked_lines_css_dedupes_and_filters() {
        let css = code_viewer_marked_lines_css(Some(&[3, 3, 1, -2, 0, 5]));
        assert_eq!(
            css,
            "[data-column-number=\"3\"],[data-column-number=\"1\"],[data-column-number=\"5\"]{color:var(--color-warning);}"
        );
    }

    #[test]
    fn marked_lines_css_is_empty_when_nothing_qualifies() {
        assert_eq!(code_viewer_marked_lines_css(None), "");
        assert_eq!(code_viewer_marked_lines_css(Some(&[])), "");
        assert_eq!(code_viewer_marked_lines_css(Some(&[0, -1, -99])), "");
    }

    // ---- JSON.stringify ----

    #[test]
    fn to_css_string_wraps_and_escapes() {
        assert_eq!(to_css_string("plain"), "\"plain\"");
        assert_eq!(to_css_string(""), "\"\"");
        assert_eq!(to_css_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(to_css_string("a\\b"), "\"a\\\\b\"");
        assert_eq!(to_css_string("a\nb"), "\"a\\nb\"");
        assert_eq!(to_css_string("a\tb"), "\"a\\tb\"");
        // 控制字符走 \uXXXX
        assert_eq!(to_css_string("\u{1}"), "\"\\u0001\"");
        // 中文原样（JSON.stringify 不转义非 ASCII）
        assert_eq!(to_css_string("点击或拖拽"), "\"点击或拖拽\"");
    }

    // ---- hash 与 cacheKey ----

    #[test]
    fn base36_matches_the_js_radix() {
        assert_eq!(to_base36(0), "0");
        assert_eq!(to_base36(10), "a");
        assert_eq!(to_base36(35), "z");
        assert_eq!(to_base36(36), "10");
        assert_eq!(to_base36(5381), "45h");
    }

    #[test]
    fn hash_is_stable_for_the_same_content() {
        assert_eq!(
            hash_code_viewer_content("const a = 1;"),
            hash_code_viewer_content("const a = 1;")
        );
        assert_ne!(
            hash_code_viewer_content("a"),
            hash_code_viewer_content("b")
        );
    }

    /// djb2 走的是 UTF-16 码元，非 BMP 字符会拆成代理对。
    #[test]
    fn hash_uses_utf16_code_units() {
        // "😀" = U+1F600 → 代理对 0xD83D, 0xDE00
        let mut expected: u32 = 5381;
        for unit in "😀".encode_utf16() {
            expected = expected.wrapping_mul(33) ^ (unit as u32);
        }
        assert_eq!(hash_code_viewer_content("😀"), to_base36(expected));
        // 单个代理码元与整体不同（证明不是按 char 走）
        assert_ne!(hash_code_viewer_content("😀"), {
            let mut single: u32 = 5381;
            single = single.wrapping_mul(33) ^ (0x1F600_u32);
            to_base36(single)
        });
    }

    #[test]
    fn file_switches_lang_to_text_without_highlighting() {
        let highlighted = create_code_viewer_file("x", true, "rust", None);
        assert_eq!(highlighted.lang, "rust");
        assert_eq!(highlighted.name, "preview.rust");

        let plain = create_code_viewer_file("x", false, "rust", None);
        assert_eq!(plain.lang, "text");
        // 文件名仍然带语言（方便 @pierre/diffs 判断扩展名）
        assert_eq!(plain.name, "preview.rust");
    }

    #[test]
    fn file_name_falls_back_when_the_language_is_empty() {
        let file = create_code_viewer_file("x", true, "", None);
        assert_eq!(file.name, "preview.txt");
        assert_eq!(file.lang, "");
    }

    #[test]
    fn file_carries_the_contents_verbatim() {
        let file = create_code_viewer_file("a\nb\n", true, "text", None);
        assert_eq!(file.contents, "a\nb\n");
    }

    /// 移植点 3：`cacheKey` 必须带主题。
    #[test]
    fn cache_key_starts_with_the_theme() {
        let auto = create_code_viewer_file("x", true, "rust", None);
        assert!(auto.cache_key.starts_with("auto:"), "{}", auto.cache_key);

        let light = create_code_viewer_file("x", true, "rust", Some("github-light"));
        let dark = create_code_viewer_file("x", true, "rust", Some("github-dark"));
        assert!(light.cache_key.starts_with("github-light:"));
        assert!(dark.cache_key.starts_with("github-dark:"));
        // 同一份代码换主题 → key 必须不同，否则会命中旧高亮
        assert_ne!(light.cache_key, dark.cache_key);
    }

    #[test]
    fn cache_key_layout_is_theme_name_lang_length_hash() {
        let file = create_code_viewer_file("abcd", true, "rust", Some("min-dark"));
        // 4 是 UTF-16 码元数
        assert_eq!(
            file.cache_key,
            format!(
                "min-dark:preview.rust:rust:4:{}",
                hash_code_viewer_content("abcd")
            )
        );
    }

    #[test]
    fn cache_key_changes_with_the_content() {
        let a = create_code_viewer_file("a", true, "text", None);
        let b = create_code_viewer_file("b", true, "text", None);
        assert_ne!(a.cache_key, b.cache_key);
        // 长度相同、内容不同也要区分（hash 兜住）
        let long_a = create_code_viewer_file("aaaa", true, "text", None);
        let long_b = create_code_viewer_file("bbbb", true, "text", None);
        assert_ne!(long_a.cache_key, long_b.cache_key);
    }

    /// `code.length` 是 UTF-16 码元数，不是 `char` 数。
    #[test]
    fn cache_key_length_counts_utf16_units() {
        let file = create_code_viewer_file("😀", true, "text", None);
        assert_eq!(file.lang, "text");
        assert!(file.cache_key.contains(":2:"), "{}", file.cache_key);
        assert_eq!("😀".chars().count(), 1);
        assert_eq!("😀".encode_utf16().count(), 2);
    }

    // ---- color-scheme ----

    /// 移植点 4：给了代码主题就**只看主题**。
    #[test]
    fn color_scheme_follows_the_theme_over_the_document() {
        // 文档是深色、主题是浅色 → 浅色
        assert_eq!(
            resolve_code_viewer_color_scheme(Some("github-light"), true),
            "light"
        );
        // 文档是浅色、主题是深色 → 深色
        assert_eq!(
            resolve_code_viewer_color_scheme(Some("github-dark"), false),
            "dark"
        );
    }

    #[test]
    fn color_scheme_uses_the_document_when_the_theme_is_absent() {
        assert_eq!(resolve_code_viewer_color_scheme(None, true), "dark");
        assert_eq!(resolve_code_viewer_color_scheme(None, false), "light");
    }

    #[test]
    fn an_unknown_theme_is_treated_as_light() {
        assert_eq!(
            resolve_code_viewer_color_scheme(Some("dracula"), true),
            "light"
        );
    }

    // ---- 样式串 ----

    #[test]
    fn style_string_carries_the_tokens_and_font() {
        let style = code_viewer_style(None, false, 12.0, None, "");
        assert!(style.starts_with("color-scheme: light;"));
        assert!(style.contains("--diffs-bg: var(--color-background);"));
        assert!(style.contains("--diffs-light-bg: var(--color-background);"));
        assert!(style.contains("--diffs-dark-bg: var(--color-background);"));
        // Shadow DOM 不会继承 font-mono，必须显式透传
        assert!(style.contains("--diffs-font-family: var(--font-mono);"));
        assert!(style.contains("--diffs-font-size: 12px;"));
        // 没有 gutter 能力时 tooltip 是空串
        assert!(style.contains("--code-comment-add-tooltip: \"\";"));
    }

    #[test]
    fn style_string_quotes_the_tooltip_and_appends_the_extra_style() {
        let style = code_viewer_style(None, true, 14.5, Some("点击或拖拽"), "color: red;");
        assert!(style.contains("color-scheme: dark;"));
        assert!(style.contains("--diffs-font-size: 14.5px;"));
        assert!(style.contains("--code-comment-add-tooltip: \"点击或拖拽\";"));
        // 追加样式在最后，可以覆盖上面的变量
        assert!(style.trim_end().ends_with("color: red;"));
    }

    // ---- 快捷键 ----

    #[test]
    fn submit_shortcut_is_enter_plus_the_platform_modifier() {
        // Apple：⌘ + Enter
        assert!(is_code_comment_submit_shortcut(Key::Enter, false, true, true));
        assert!(!is_code_comment_submit_shortcut(Key::Enter, true, false, true));
        // 其它平台：Ctrl + Enter
        assert!(is_code_comment_submit_shortcut(Key::Enter, true, false, false));
        assert!(!is_code_comment_submit_shortcut(Key::Enter, false, true, false));
        // 没有修饰键不算
        assert!(!is_code_comment_submit_shortcut(Key::Enter, false, false, false));
        assert!(!is_code_comment_submit_shortcut(Key::Enter, false, false, true));
        // 不是 Enter 一律不算
        assert!(!is_code_comment_submit_shortcut(Key::Escape, true, true, false));
        assert!(!is_code_comment_submit_shortcut(Key::ArrowLeft, true, true, false));
    }

    #[test]
    fn cancel_shortcut_is_escape() {
        assert!(is_code_comment_cancel_shortcut(Key::Escape));
        assert!(!is_code_comment_cancel_shortcut(Key::Enter));
    }

    #[test]
    fn submit_shortcut_label_follows_the_platform() {
        let apple = KeyboardShortcutPlatformInfo {
            platform: Some("MacIntel".to_string()),
            user_agent: None,
        };
        let windows = KeyboardShortcutPlatformInfo {
            platform: Some("Win32".to_string()),
            user_agent: None,
        };
        assert_eq!(format_code_comment_submit_shortcut_label(&apple), "⌘ ↵");
        assert_eq!(format_code_comment_submit_shortcut_label(&windows), "Ctrl+↵");
        assert_eq!(format_code_comment_cancel_shortcut_label(), "Esc");
    }

    // ---- 提交 ----

    #[test]
    fn submission_requires_a_draft_range() {
        assert_eq!(
            build_code_comment_submission("a\nb", 1, None, "hi"),
            None
        );
    }

    #[test]
    fn submission_rejects_a_blank_selection() {
        let code = "a\n\nb";
        // 第 2 行是空行 → trim 后为空 → 不提交
        assert_eq!(
            build_code_comment_submission(
                code,
                1,
                Some(CodeCommentRange {
                    start_line: 2,
                    end_line: 2
                }),
                "hi"
            ),
            None
        );
        // 只有空白字符也一样
        assert_eq!(
            build_code_comment_submission(
                code,
                1,
                Some(CodeCommentRange {
                    start_line: 2,
                    end_line: 2
                }),
                "   "
            ),
            None
        );
    }

    #[test]
    fn submission_trims_the_comment_and_keeps_the_selection_verbatim() {
        let submission = build_code_comment_submission(
            "  let a = 1;  ",
            1,
            Some(CodeCommentRange {
                start_line: 1,
                end_line: 1,
            }),
            "  改个名字  ",
        )
        .unwrap();
        // comment 会 trim
        assert_eq!(submission.comment, "改个名字");
        // selectedText 原样（保留缩进，供引用上下文使用）
        assert_eq!(submission.selected_text, "  let a = 1;  ");
    }

    #[test]
    fn submission_normalizes_the_range() {
        let submission = build_code_comment_submission(
            "a\nb\nc",
            1,
            Some(CodeCommentRange {
                start_line: 3,
                end_line: 1,
            }),
            "x",
        )
        .unwrap();
        assert_eq!(submission.range.start_line, 1);
        assert_eq!(submission.range.end_line, 3);
        assert_eq!(submission.selected_text, "a\nb\nc");
    }

    #[test]
    fn submission_can_carry_an_empty_comment() {
        // 只选代码不写评论是合法的（引用上下文本身就够用）
        let submission = build_code_comment_submission(
            "a",
            1,
            Some(CodeCommentRange {
                start_line: 1,
                end_line: 1,
            }),
            "",
        )
        .unwrap();
        assert_eq!(submission.comment, "");
        assert_eq!(submission.selected_text, "a");
    }
}
