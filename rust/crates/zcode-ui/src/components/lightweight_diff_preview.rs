//! `LightweightDiffPreview` —— Rust 移植自
//! `packages/ui/src/components/ui/lightweight-diff-preview.tsx`
//!
//! 轻量 diff 预览：不用富渲染器，直接把 unified diff 的每一行铺成
//! 「行号槽 + 代码」两列，用**行背景、左侧状态条、行号颜色**表达增删。
//!
//! ## 关键移植点 1：不能把 `+` / `-` / 空格当内容显示
//!
//! unified diff 的行首标记是**协议**，不是代码。所以每一行都要先经过
//! [`get_lightweight_diff_line_parts`] 把标记剥掉，只把 `content` 交给 `<code>`；
//! 增删信息由样式承担。这和富 `DiffViewer` 的观感保持一致。
//!
//! ## 关键移植点 2：不换行时由**内层滚动面**统一算宽度
//!
//! 见 TS 里那段注释：如果每一行各自 `w-max`，横向滚动到右侧时短行的背景会提前
//! 结束，露出黑色断层。所以 `w-max` 只加在内层滚动容器上，行本身是 `w-full`。
//!
//! ## 关键移植点 3：行号槽是 sticky 的
//!
//! `sticky left-0` + `z-[1]`：横向滚动时行号要钉在左边不跟着走，
//! 所以它必须有不透明背景（见 [`get_lightweight_diff_line_styles`] 里的
//! `backgroundColor`），否则代码会从行号底下透出来。
//!
//! ## 与 TS 的差异
//!
//! 1. **`renderLineContent` → `tokens`**：TS 用 render prop 让
//!    `HighlightedLightweightDiffPreview` 把高亮 token 注入每一行。Dioxus 的
//!    `EventHandler` 不能返回值，所以改成传**数据**（`Option<Vec<Vec<ThemedToken>>>`），
//!    由本组件按行取用。语义等价 —— TS 里那个 render prop 唯一做的事就是
//!    「按 `index` 取 `tokens`」。
//! 2. **`codePreviewSettings` 拆成三个 prop**：`fontSizePx` / `showLineNumbers` /
//!    `wrapLongLines`。TS 收的是一个 `Pick<CodePreviewSettings, ...>`，Rust 侧没有
//!    全局 store，拆平更直接。
//! 3. **`{...props}` 透传改为显式 prop**：本移植没有通用的属性透传机制，
//!    只把真正用到的三个 `data-lightweight-diff-highlight-*` 做成 prop。
//! 4. **截断文案改为 prop**：TS 走 `intl.formatMessage`，默认值与 `en-US` 一致。

use crate::cn::cn;
use crate::patch_diff_preview::parse_truncated_marker_omitted_line_count;
use crate::shiki_highlighter::ThemedToken;
use dioxus::prelude::*;

/// diff 行的三种语义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LightweightDiffLineKind {
    Added,
    Removed,
    Context,
}

/// 拆解后的 diff 行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LightweightDiffLineParts {
    /// 剥掉协议标记后的代码内容。
    pub content: String,
    pub kind: LightweightDiffLineKind,
    /// 协议标记本身（`"+"` / `"-"` / `" "` / `""`）。
    pub marker: &'static str,
    /// 原始行。
    pub raw: String,
}

/// 一行的内联样式。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LightweightDiffLineStyles {
    /// 行号槽的内联样式。
    pub gutter_style: String,
    /// 行号的文字颜色类名。
    pub line_number_class: &'static str,
    /// 整行的内联样式（上下文行为空串）。
    pub row_style: String,
}

/// 根节点基础类名。
pub const LIGHTWEIGHT_DIFF_PREVIEW_BASE: &str = "w-full min-w-0 overflow-auto bg-background";

/// 内层滚动面基础类名。
pub const LIGHTWEIGHT_DIFF_SCROLL_CONTENT_BASE: &str =
    "min-w-full font-mono leading-relaxed text-foreground";

/// 不换行时给内层滚动面加的宽度类名（见模块文档关键移植点 2）。
pub const LIGHTWEIGHT_DIFF_SCROLL_CONTENT_NOWRAP: &str = "w-max";

/// 截断提示行的类名。
pub const LIGHTWEIGHT_DIFF_TRUNCATED_ROW: &str = "px-3 py-1 text-foreground-subtle";

/// 普通行外框。
pub const LIGHTWEIGHT_DIFF_ROW: &str = "flex min-w-full w-full";

/// 行号槽（`sticky` 钉在左边）。
pub const LIGHTWEIGHT_DIFF_GUTTER: &str = "sticky left-0 z-[1] w-12 shrink-0 select-none border-r border-border px-2 text-right tabular-nums";

/// `<code>` 基础类名。
pub const LIGHTWEIGHT_DIFF_CODE: &str = "block flex-1 px-3";

/// 换行模式。
pub const LIGHTWEIGHT_DIFF_CODE_WRAP: &str = "whitespace-pre-wrap break-words";

/// 不换行模式。
pub const LIGHTWEIGHT_DIFF_CODE_NOWRAP: &str = "whitespace-pre";

/// 新增行的行号颜色。
pub const LIGHTWEIGHT_DIFF_LINENO_ADDED: &str = "text-diff-added";

/// 删除行的行号颜色。
pub const LIGHTWEIGHT_DIFF_LINENO_REMOVED: &str = "text-diff-removed";

/// 上下文行的行号颜色。
pub const LIGHTWEIGHT_DIFF_LINENO_CONTEXT: &str = "text-foreground-subtlest";

/// 截断提示的默认模板（同 `en-US` 语言包，`{count}` 会被替换）。
pub const LIGHTWEIGHT_DIFF_TRUNCATED_LABEL: &str =
    "Diff preview truncated: {count} lines omitted to keep UI responsive.";

/// 把截断模板里的 `{count}` 换成实际数字。
pub fn format_truncated_label(template: &str, omitted_line_count: usize) -> String {
    template.replace("{count}", &omitted_line_count.to_string())
}

pub fn lightweight_diff_preview_classes(class: &str) -> String {
    cn(&[LIGHTWEIGHT_DIFF_PREVIEW_BASE, class])
}

pub fn lightweight_diff_scroll_content_classes(wrap_long_lines: bool) -> String {
    if wrap_long_lines {
        LIGHTWEIGHT_DIFF_SCROLL_CONTENT_BASE.to_string()
    } else {
        cn(&[
            LIGHTWEIGHT_DIFF_SCROLL_CONTENT_BASE,
            LIGHTWEIGHT_DIFF_SCROLL_CONTENT_NOWRAP,
        ])
    }
}

pub fn lightweight_diff_code_classes(wrap_long_lines: bool) -> String {
    let wrap = if wrap_long_lines {
        LIGHTWEIGHT_DIFF_CODE_WRAP
    } else {
        LIGHTWEIGHT_DIFF_CODE_NOWRAP
    };
    cn(&[LIGHTWEIGHT_DIFF_CODE, wrap])
}

pub fn lightweight_diff_gutter_classes(kind: LightweightDiffLineKind) -> String {
    cn(&[
        LIGHTWEIGHT_DIFF_GUTTER,
        get_lightweight_diff_line_styles(kind).line_number_class,
    ])
}

/// 拆解一行 unified diff。
///
/// 只有 `+` / `-` / 空格三种前缀会带 `marker`；其余（含空行）算上下文且
/// `marker` 为空串。
pub fn get_lightweight_diff_line_parts(line: &str) -> LightweightDiffLineParts {
    let marker = if line.starts_with('+') {
        Some(("+", LightweightDiffLineKind::Added))
    } else if line.starts_with('-') {
        Some(("-", LightweightDiffLineKind::Removed))
    } else if line.starts_with(' ') {
        Some((" ", LightweightDiffLineKind::Context))
    } else {
        None
    };

    let (marker, kind) = marker.unwrap_or(("", LightweightDiffLineKind::Context));
    LightweightDiffLineParts {
        content: crate::patch_diff_preview::get_patch_preview_line_content(line),
        kind,
        marker,
        raw: line.to_string(),
    }
}

/// 按行语义取样式。
pub fn get_lightweight_diff_line_styles(kind: LightweightDiffLineKind) -> LightweightDiffLineStyles {
    match kind {
        LightweightDiffLineKind::Added => LightweightDiffLineStyles {
            line_number_class: LIGHTWEIGHT_DIFF_LINENO_ADDED,
            row_style: "background-color: color-mix(in srgb, var(--color-diff-added) 14%, transparent); box-shadow: inset 3px 0 0 var(--color-diff-added);".to_string(),
            gutter_style: "background-color: color-mix(in srgb, var(--color-diff-added) 18%, var(--color-background)); box-shadow: inset 3px 0 0 var(--color-diff-added); color: var(--color-diff-added);".to_string(),
        },
        LightweightDiffLineKind::Removed => LightweightDiffLineStyles {
            line_number_class: LIGHTWEIGHT_DIFF_LINENO_REMOVED,
            row_style: "background-color: color-mix(in srgb, var(--color-diff-removed) 14%, transparent); box-shadow: inset 3px 0 0 var(--color-diff-removed);".to_string(),
            gutter_style: "background-color: color-mix(in srgb, var(--color-diff-removed) 18%, var(--color-background)); box-shadow: inset 3px 0 0 var(--color-diff-removed); color: var(--color-diff-removed);".to_string(),
        },
        LightweightDiffLineKind::Context => LightweightDiffLineStyles {
            line_number_class: LIGHTWEIGHT_DIFF_LINENO_CONTEXT,
            // 上下文行没有行背景，只有行号槽要盖住代码
            gutter_style: "background-color: var(--color-background);".to_string(),
            row_style: String::new(),
        },
    }
}

/// 把一行代码渲染成 `<code>` 的内容。
///
/// - 有高亮 token 且非空 → 逐个渲染 `<span>`，带 `color` 时写内联样式；
/// - 否则 → 原文；原文为空时渲染一个空格（保住行高）。
fn render_code_line(content: &str, tokens: Option<&Vec<ThemedToken>>) -> Element {
    match tokens {
        Some(tokens) if !tokens.is_empty() => rsx! {
            for (index, token) in tokens.iter().enumerate() {
                span {
                    // TS 的 key 是 `{index}:{token.content}`
                    key: "{index}:{token.content}",
                    style: token.color.as_ref().map(|color| format!("color: {color};")),
                    {token.content.clone()}
                }
            }
        },
        _ => {
            let text = if content.is_empty() {
                " ".to_string()
            } else {
                content.to_string()
            };
            rsx! { {text} }
        }
    }
}

/// 轻量 diff 预览。
#[component]
pub fn LightweightDiffPreview(
    /// 要渲染的行（unified diff 原文，含协议标记）。
    lines: Vec<String>,
    /// 字号（px）。
    #[props(default = 12.0)] font_size_px: f64,
    /// 是否显示行号槽。
    #[props(default = true)] show_line_numbers: bool,
    /// 长行是否折行（为假时走横向滚动）。
    #[props(default = false)] wrap_long_lines: bool,
    /// 语法高亮 token（按行）。见模块文档差异 1。
    #[props(default)] tokens: Option<Vec<Vec<ThemedToken>>>,
    /// 截断提示模板，`{count}` 会被替换。
    #[props(default = LIGHTWEIGHT_DIFF_TRUNCATED_LABEL.to_string(), into)]
    truncated_label: String,
    /// 写到 `data-lightweight-diff-highlight-language`。
    #[props(default)] highlight_language: Option<String>,
    /// 写到 `data-lightweight-diff-highlight-theme`。
    #[props(default)] highlight_theme: Option<String>,
    /// 写到 `data-lightweight-diff-highlighted`（`"true"` / `"false"`）。
    #[props(default)] highlighted: Option<bool>,
    #[props(default, into)] class: String,
) -> Element {
    rsx! {
        div {
            class: lightweight_diff_preview_classes(&class),
            // TS 里是裸属性 `data-lightweight-diff-preview` → 空串
            "data-lightweight-diff-preview": "",
            "data-lightweight-diff-highlight-language": highlight_language,
            "data-lightweight-diff-highlight-theme": highlight_theme,
            "data-lightweight-diff-highlighted": highlighted
                .map(|v| if v { "true" } else { "false" }),
            div {
                class: lightweight_diff_scroll_content_classes(wrap_long_lines),
                "data-lightweight-diff-scroll-content": "",
                style: format!("font-size: {font_size_px}px;"),
                for (index, line) in lines.iter().enumerate() {
                    if let Some(omitted) =
                        parse_truncated_marker_omitted_line_count(line)
                    {
                        div {
                            key: "{index}",
                            class: LIGHTWEIGHT_DIFF_TRUNCATED_ROW,
                            {format_truncated_label(&truncated_label, omitted)}
                        }
                    } else {
                        {
                            let parts = get_lightweight_diff_line_parts(line);
                            let styles = get_lightweight_diff_line_styles(parts.kind);
                            let line_tokens = tokens.as_ref().and_then(|t| t.get(index));
                            rsx! {
                                div {
                                    key: "{index}",
                                    class: LIGHTWEIGHT_DIFF_ROW,
                                    style: if styles.row_style.is_empty() {
                                        None
                                    } else {
                                        Some(styles.row_style.clone())
                                    },
                                    // 见模块文档关键移植点 2：不换行时宽度由内层滚动面统一算
                                    if show_line_numbers {
                                        span {
                                            "aria-hidden": "true",
                                            class: lightweight_diff_gutter_classes(parts.kind),
                                            style: styles.gutter_style.clone(),
                                            // 行号是 1-based（TS `{index + 1}`）
                                            {(index + 1).to_string()}
                                        }
                                    }
                                    code {
                                        class: lightweight_diff_code_classes(wrap_long_lines),
                                        {render_code_line(&parts.content, line_tokens)}
                                    }
                                }
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

    // ---- 行拆解 ----

    #[test]
    fn added_line_strips_the_marker() {
        let parts = get_lightweight_diff_line_parts("+let x = 1;");
        assert_eq!(parts.kind, LightweightDiffLineKind::Added);
        assert_eq!(parts.marker, "+");
        assert_eq!(parts.content, "let x = 1;");
        assert_eq!(parts.raw, "+let x = 1;");
    }

    #[test]
    fn removed_line_strips_the_marker() {
        let parts = get_lightweight_diff_line_parts("-let x = 2;");
        assert_eq!(parts.kind, LightweightDiffLineKind::Removed);
        assert_eq!(parts.marker, "-");
        assert_eq!(parts.content, "let x = 2;");
    }

    #[test]
    fn context_line_keeps_its_leading_space_as_marker() {
        let parts = get_lightweight_diff_line_parts(" unchanged");
        assert_eq!(parts.kind, LightweightDiffLineKind::Context);
        assert_eq!(parts.marker, " ");
        // 剥掉那个空格后是真正的代码
        assert_eq!(parts.content, "unchanged");
    }

    /// 没有协议前缀的行算上下文，`marker` 为空串（不是空格）。
    #[test]
    fn unprefixed_line_is_context_without_a_marker() {
        for line in ["@@ -1 +1 @@", "", "raw text"] {
            let parts = get_lightweight_diff_line_parts(line);
            assert_eq!(parts.kind, LightweightDiffLineKind::Context, "{line:?}");
            assert_eq!(parts.marker, "", "{line:?}");
            assert_eq!(parts.content, line, "{line:?}");
        }
    }

    /// 空内容行（patch 里的 `+` 单独一行）剥完标记后是空串。
    #[test]
    fn marker_only_line_has_empty_content() {
        assert_eq!(get_lightweight_diff_line_parts("+").content, "");
        assert_eq!(get_lightweight_diff_line_parts("-").content, "");
        assert_eq!(get_lightweight_diff_line_parts(" ").content, "");
    }

    // ---- 样式 ----

    #[test]
    fn added_and_removed_use_opposite_color_tokens() {
        let added = get_lightweight_diff_line_styles(LightweightDiffLineKind::Added);
        let removed = get_lightweight_diff_line_styles(LightweightDiffLineKind::Removed);

        assert_eq!(added.line_number_class, LIGHTWEIGHT_DIFF_LINENO_ADDED);
        assert_eq!(removed.line_number_class, LIGHTWEIGHT_DIFF_LINENO_REMOVED);
        assert!(added.row_style.contains("--color-diff-added"));
        assert!(removed.row_style.contains("--color-diff-removed"));
        assert!(!added.row_style.contains("--color-diff-removed"));
    }

    #[test]
    fn added_and_removed_rows_carry_the_status_bar() {
        for kind in [
            LightweightDiffLineKind::Added,
            LightweightDiffLineKind::Removed,
        ] {
            let styles = get_lightweight_diff_line_styles(kind);
            // 左侧 3px 状态条
            assert!(styles.row_style.contains("inset 3px 0 0"), "{styles:?}");
            assert!(styles.gutter_style.contains("inset 3px 0 0"), "{styles:?}");
            // 行背景是 14%，行号槽是 18%（行号槽要不透明才能盖住代码）
            assert!(styles.row_style.contains("14%"), "{styles:?}");
            assert!(styles.gutter_style.contains("18%"), "{styles:?}");
        }
    }

    #[test]
    fn context_line_has_no_row_style_but_an_opaque_gutter() {
        let styles = get_lightweight_diff_line_styles(LightweightDiffLineKind::Context);
        assert_eq!(styles.line_number_class, LIGHTWEIGHT_DIFF_LINENO_CONTEXT);
        // 上下文行不画行背景
        assert!(styles.row_style.is_empty());
        // 但行号槽必须不透明，否则横向滚动时代码会从它底下透出来
        assert!(styles.gutter_style.contains("background-color: var(--color-background)"));
    }

    #[test]
    fn gutter_classes_append_the_line_number_color() {
        let c = lightweight_diff_gutter_classes(LightweightDiffLineKind::Added);
        assert!(c.contains("sticky"));
        assert!(c.contains("left-0"));
        assert!(c.contains("z-[1]"));
        assert!(c.contains("w-12"));
        assert!(c.contains("tabular-nums"));
        assert!(c.contains(LIGHTWEIGHT_DIFF_LINENO_ADDED));
    }

    // ---- 类名 ----

    #[test]
    fn root_and_scroll_content_classes() {
        let root = lightweight_diff_preview_classes("");
        assert!(root.contains("overflow-auto"));
        assert!(root.contains("bg-background"));

        // 换行时不加 `w-max`
        let wrap = lightweight_diff_scroll_content_classes(true);
        assert!(!wrap.contains("w-max"));
        // 不换行时加 `w-max`（宽度交给内层滚动面统一算）
        let nowrap = lightweight_diff_scroll_content_classes(false);
        assert!(nowrap.contains("w-max"));
        assert!(nowrap.contains("font-mono"));
    }

    #[test]
    fn code_classes_follow_the_wrap_setting() {
        let wrap = lightweight_diff_code_classes(true);
        assert!(wrap.contains("whitespace-pre-wrap"));
        assert!(wrap.contains("break-words"));
        assert!(!wrap.contains("whitespace-pre "));

        let nowrap = lightweight_diff_code_classes(false);
        assert!(nowrap.contains("whitespace-pre"));
        assert!(!nowrap.contains("whitespace-pre-wrap"));
    }

    // ---- 截断文案 ----

    #[test]
    fn truncated_label_substitutes_the_count() {
        let out = format_truncated_label(LIGHTWEIGHT_DIFF_TRUNCATED_LABEL, 42);
        assert!(out.contains("42 lines omitted"), "{out}");
        assert!(!out.contains("{count}"));
        // 自定义模板同样支持占位
        assert_eq!(
            format_truncated_label("省略了 {count} 行", 7),
            "省略了 7 行"
        );
        // 没有占位符时原样返回
        assert_eq!(format_truncated_label("无占位符", 7), "无占位符");
    }

    #[test]
    fn component_is_constructible() {
        let _ = LightweightDiffPreview as fn(LightweightDiffPreviewProps) -> Element;
    }
}
