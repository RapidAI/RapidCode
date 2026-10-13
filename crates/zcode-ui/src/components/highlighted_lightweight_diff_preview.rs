//! `HighlightedLightweightDiffPreview` —— Rust 移植自
//! `packages/ui/src/components/ui/highlighted-lightweight-diff-preview.tsx`
//!
//! 在 [`LightweightDiffPreview`](crate::components::lightweight_diff_preview::LightweightDiffPreview)
//! 之上叠一层语法高亮：先把所有行拼成一段代码交给高亮器，再把逐行 token 铺回去。
//!
//! ## 关键移植点 1：为什么要「先拼再切」
//!
//! 高亮器是按**整段代码**理解语法的（多行字符串、块注释、模板字面量都跨行）。
//! 逐行独立高亮会把一个跨行字符串的后半截当成新语句。所以必须先
//! [`build_highlighted_lightweight_diff_code`] 拼成整段，再按行取回
//! `tokens[index]`。
//!
//! 拼接用的是**剥掉协议标记后**的 `content`（不是原始行），否则 `+` / `-`
//! 会被当成代码参与高亮，甚至把整行染成运算符色。
//!
//! ## 关键移植点 2：超长代码直接放弃高亮
//!
//! [`HIGHLIGHTED_LIGHTWEIGHT_DIFF_MAX_CHARS`] 之上的代码不送进高亮器 ——
//! 那是几万 token 的同步解析，比不高亮更糟。
//!
//! ## 关键移植点 3：纯文本语言仍然「算高亮过」
//!
//! TS 的 `highlightCode` 对 `text` / `log` 这类语言会**同步返回 raw token**
//! （见 `shiki_highlighter` 模块）。所以 `data-lightweight-diff-highlighted`
//! 对纯文本也是 `"true"` —— 只有「代码为空」或「超长」才是 `"false"`。
//! 这个区别被 [`should_tokenize_highlighted_lightweight_diff`] 与
//! [`tokenize_highlighted_lightweight_diff`] 的返回 `Option` 表达出来。
//!
//! ## 与 TS 的差异
//!
//! 1. **异步高亮搬到调用方**：TS 在 `useEffect` 里调 `highlightCode`（带缓存与
//!    订阅），本移植不引入 shiki，把「拿 token」这一步交给调用方 —— 用
//!    [`tokenize_highlighted_lightweight_diff`] 算出 `Option<TokenizedCode>`，
//!    作为 `tokens` prop 传进来。组件本身只负责铺 token + 挂 `data-*` 属性。
//!    这样做还有一个好处：纯函数可以被完整测试，不必依赖渲染器。
//! 2. **`useMemo` 改为普通计算**：TS 用 `useMemo(build..., [lines])`，
//!    Rust 侧 `build_highlighted_lightweight_diff_code` 只是一次 join，
//!    每次渲染重算的代价可以忽略。
//! 3. **`code.length` 按字节而不是 UTF-16 码元**：见 `shiki_highlighter` 模块文档差异 2。
//! 4. **`path` 只用于日志**：TS 把它传给 `logger.debug`，本移植没有 logger，
//!    因此不接收该 prop。

use crate::components::lightweight_diff_preview::{
    get_lightweight_diff_line_parts, LightweightDiffPreview, LIGHTWEIGHT_DIFF_TRUNCATED_LABEL,
};
use crate::shiki_highlighter::{
    highlight_code, CodeHighlighter, ShikiLanguageCatalog, ThemedToken, TokenizedCode,
};
use dioxus::prelude::*;

/// 超过这么多字符就放弃语法高亮。
pub const HIGHLIGHTED_LIGHTWEIGHT_DIFF_MAX_CHARS: usize = 120_000;

/// 一行代码 + 它的协议标记。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HighlightedLightweightDiffLine {
    /// 剥掉协议标记后的代码。
    pub code: String,
    /// 协议标记（`"+"` / `"-"` / `" "` / `""`）。
    pub marker: &'static str,
}

/// 拆出一行的代码与标记。
pub fn get_highlighted_lightweight_diff_line(line: &str) -> HighlightedLightweightDiffLine {
    let parts = get_lightweight_diff_line_parts(line);
    HighlightedLightweightDiffLine {
        code: parts.content,
        marker: parts.marker,
    }
}

/// 把所有行拼成一段代码（用 `\n` 连接）。
///
/// 见模块文档关键移植点 1：必须是剥掉标记后的内容。
pub fn build_highlighted_lightweight_diff_code(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| get_highlighted_lightweight_diff_line(line).code)
        .collect::<Vec<_>>()
        .join("\n")
}

/// 这段代码是否值得送进高亮器。
///
/// 空代码与超长代码都返回 `false`（对应 TS `!code || code.length > MAX`）。
pub fn should_tokenize_highlighted_lightweight_diff(code: &str) -> bool {
    !code.is_empty() && code.len() <= HIGHLIGHTED_LIGHTWEIGHT_DIFF_MAX_CHARS
}

/// 算出整段代码的高亮 token。
///
/// 返回 `None` 表示「没有高亮结果」，对应 TS 里 `tokenizedCode` 停留在 `null`
/// 的状态（只有空代码与超长代码会这样）。纯文本语言**不会**返回 `None` ——
/// 它会拿到 raw token，见模块文档关键移植点 3。
pub fn tokenize_highlighted_lightweight_diff(
    catalog: &impl ShikiLanguageCatalog,
    highlighter: &impl CodeHighlighter,
    lines: &[String],
    language: &str,
    theme: &str,
    prefers_dark: bool,
) -> Option<TokenizedCode> {
    let code = build_highlighted_lightweight_diff_code(lines);
    if !should_tokenize_highlighted_lightweight_diff(&code) {
        return None;
    }
    Some(highlight_code(
        catalog,
        highlighter,
        &code,
        language,
        Some(theme),
        prefers_dark,
    ))
}

/// 带语法高亮的轻量 diff 预览。
///
/// `tokens` 由调用方用 [`tokenize_highlighted_lightweight_diff`] 算好后传入。
#[component]
pub fn HighlightedLightweightDiffPreview(
    /// 要渲染的行（unified diff 原文）。
    lines: Vec<String>,
    /// 高亮语言（写到 `data-lightweight-diff-highlight-language`）。
    #[props(default, into)] language: String,
    /// 高亮主题（写到 `data-lightweight-diff-highlight-theme`）。
    #[props(default, into)] theme: String,
    /// 逐行 token；`None` 表示未高亮（`data-...-highlighted="false"`）。
    #[props(default)] tokens: Option<Vec<Vec<ThemedToken>>>,
    /// 字号（px）。
    #[props(default = 12.0)] font_size_px: f64,
    /// 是否显示行号槽。
    #[props(default = true)] show_line_numbers: bool,
    /// 长行是否折行。
    #[props(default = false)] wrap_long_lines: bool,
    /// 截断提示模板。
    #[props(default = LIGHTWEIGHT_DIFF_TRUNCATED_LABEL.to_string(), into)]
    truncated_label: String,
    #[props(default, into)] class: String,
) -> Element {
    let highlighted = tokens.is_some();

    rsx! {
        LightweightDiffPreview {
            lines,
            tokens,
            font_size_px,
            show_line_numbers,
            wrap_long_lines,
            truncated_label,
            class,
            highlight_language: Some(language),
            highlight_theme: Some(theme),
            highlighted: Some(highlighted),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shiki_highlighter::{
        create_raw_code_tokens, ShikiLanguageCatalog, TokenizedCode,
    };

    fn owned(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|l| l.to_string()).collect()
    }

    struct FakeCatalog;
    impl ShikiLanguageCatalog for FakeCatalog {
        fn is_bundled(&self, id: &str) -> bool {
            matches!(id, "rust" | "log")
        }
        fn resolve_alias(&self, _alias: &str) -> Option<String> {
            None
        }
    }

    struct CountingHighlighter {
        calls: std::cell::Cell<usize>,
    }
    impl CountingHighlighter {
        fn new() -> Self {
            Self {
                calls: std::cell::Cell::new(0),
            }
        }
    }
    impl CodeHighlighter for CountingHighlighter {
        fn code_to_tokens(&self, code: &str, language: &str, theme: &str) -> TokenizedCode {
            self.calls.set(self.calls.get() + 1);
            TokenizedCode {
                tokens: code
                    .split('\n')
                    .map(|line| {
                        if line.is_empty() {
                            Vec::new()
                        } else {
                            vec![ThemedToken {
                                content: line.to_string(),
                                color: Some(format!("{language}:{theme}")),
                            }]
                        }
                    })
                    .collect(),
                fg: "inherit".to_string(),
                bg: "transparent".to_string(),
            }
        }
        fn loaded_languages(&self) -> Vec<String> {
            vec!["rust".to_string()]
        }
    }

    // ---- 单行拆解 ----

    #[test]
    fn line_exposes_code_and_marker() {
        let added = get_highlighted_lightweight_diff_line("+let x = 1;");
        assert_eq!(added.code, "let x = 1;");
        assert_eq!(added.marker, "+");

        let context = get_highlighted_lightweight_diff_line(" keep");
        assert_eq!(context.code, "keep");
        assert_eq!(context.marker, " ");

        let bare = get_highlighted_lightweight_diff_line("raw");
        assert_eq!(bare.code, "raw");
        assert_eq!(bare.marker, "");
    }

    // ---- 拼接 ----

    /// 拼接的是**剥掉标记**的内容 —— 否则 `+` / `-` 会被当成代码参与高亮。
    #[test]
    fn the_joined_code_drops_the_protocol_markers() {
        let code = build_highlighted_lightweight_diff_code(&owned(&[
            " let x = 1;",
            "-let y = 2;",
            "+let z = 3;",
        ]));
        assert_eq!(code, "let x = 1;\nlet y = 2;\nlet z = 3;");
        assert!(!code.contains('+'));
        assert!(!code.starts_with(' '));
    }

    #[test]
    fn the_joined_code_preserves_line_count() {
        let lines = owned(&["+a", "+b", "+c"]);
        let code = build_highlighted_lightweight_diff_code(&lines);
        // 3 行 → 2 个换行 → 高亮器切回来还是 3 行
        assert_eq!(code.matches('\n').count(), 2);
        assert_eq!(code.split('\n').count(), lines.len());
    }

    /// 空行会变成连续换行，**不会**被吞掉 —— 行号才能跟原文对齐。
    #[test]
    fn blank_lines_are_preserved_in_the_joined_code() {
        let code = build_highlighted_lightweight_diff_code(&owned(&["+a", "+", "+b"]));
        assert_eq!(code, "a\n\nb");
        assert_eq!(code.split('\n').count(), 3);
    }

    #[test]
    fn an_empty_line_list_joins_to_an_empty_string() {
        assert_eq!(build_highlighted_lightweight_diff_code(&[]), "");
        assert_eq!(build_highlighted_lightweight_diff_code(&owned(&["+"])), "");
    }

    /// 截断标记行也是「一行代码」，会原样进入拼接结果。
    ///
    /// 这不影响正确性：超长预览本来就会被
    /// [`HIGHLIGHTED_LIGHTWEIGHT_DIFF_MAX_CHARS`] 拦下，而截断标记只在超长时才出现。
    #[test]
    fn a_truncated_marker_line_is_joined_verbatim() {
        let code =
            build_highlighted_lightweight_diff_code(&owned(&["+a", "\\ __ZCODE_DIFF_TRUNCATED__:5"]));
        assert!(code.contains("__ZCODE_DIFF_TRUNCATED__:5"), "{code}");
    }

    // ---- 高亮闸门 ----

    #[test]
    fn empty_code_is_not_tokenized() {
        assert!(!should_tokenize_highlighted_lightweight_diff(""));
    }

    #[test]
    fn code_at_the_limit_is_still_tokenized() {
        let at_limit = "a".repeat(HIGHLIGHTED_LIGHTWEIGHT_DIFF_MAX_CHARS);
        assert!(should_tokenize_highlighted_lightweight_diff(&at_limit));
        // 超一个字节就放弃
        let over = "a".repeat(HIGHLIGHTED_LIGHTWEIGHT_DIFF_MAX_CHARS + 1);
        assert!(!should_tokenize_highlighted_lightweight_diff(&over));
    }

    // ---- 端到端 ----

    #[test]
    fn empty_lines_produce_no_tokens_at_all() {
        let highlighter = CountingHighlighter::new();
        let out = tokenize_highlighted_lightweight_diff(
            &FakeCatalog,
            &highlighter,
            &[],
            "rust",
            "nord",
            false,
        );
        assert!(out.is_none(), "空输入不该产生 token");
        // 引擎完全没被调用
        assert_eq!(highlighter.calls.get(), 0);
    }

    #[test]
    fn oversized_input_skips_the_engine() {
        let highlighter = CountingHighlighter::new();
        // 注意 `+` 会被剥掉，所以这里要多给一个字符才真的超限
        let lines = owned(&[&format!(
            "+{}",
            "a".repeat(HIGHLIGHTED_LIGHTWEIGHT_DIFF_MAX_CHARS + 1)
        )]);
        let out = tokenize_highlighted_lightweight_diff(
            &FakeCatalog,
            &highlighter,
            &lines,
            "rust",
            "nord",
            false,
        );
        assert!(out.is_none(), "超长输入不该产生 token");
        assert_eq!(highlighter.calls.get(), 0, "超长输入不该进引擎");
    }

    #[test]
    fn highlightable_code_goes_through_the_engine() {
        let highlighter = CountingHighlighter::new();
        let out = tokenize_highlighted_lightweight_diff(
            &FakeCatalog,
            &highlighter,
            &owned(&["+fn main() {}", "+// hi"]),
            "rust",
            "nord",
            false,
        )
        .expect("应拿到 token");

        assert_eq!(highlighter.calls.get(), 1);
        // 引擎收到的是剥掉 `+` 之后的整段代码
        assert_eq!(out.tokens.len(), 2);
        assert_eq!(out.tokens[0][0].content, "fn main() {}");
        assert_eq!(out.tokens[0][0].color.as_deref(), Some("rust:nord"));
    }

    /// 纯文本语言**仍然**返回 `Some`（raw token），因此 `highlighted` 是 `"true"`。
    ///
    /// 见模块文档关键移植点 3 —— 这与「未高亮」不是一回事。
    #[test]
    fn plain_text_still_counts_as_highlighted() {
        let highlighter = CountingHighlighter::new();
        let out = tokenize_highlighted_lightweight_diff(
            &FakeCatalog,
            &highlighter,
            &owned(&["+hello", "+world"]),
            "text",
            "nord",
            false,
        )
        .expect("纯文本也应拿到 raw token");

        // 引擎没被调用，拿到的是 raw token
        assert_eq!(highlighter.calls.get(), 0);
        assert_eq!(out, create_raw_code_tokens("hello\nworld"));
    }

    #[test]
    fn component_is_constructible() {
        let _ = HighlightedLightweightDiffPreview
            as fn(HighlightedLightweightDiffPreviewProps) -> Element;
    }
}
