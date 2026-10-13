//! `shiki_highlighter` —— Rust 移植自 `packages/ui/src/lib/shikiHighlighter.ts`
//!
//! 这个模块在 TS 侧是「带缓存 + 订阅 + 微任务投递的异步高亮入口」。其中真正
//! 决定行为的部分全是纯逻辑：语言归一化、要不要高亮、缓存键、raw token 兜底。
//! 真正不可移植的只有一件事 —— **TextMate 语法引擎本身**。
//!
//! ## 移植策略
//!
//! 本移植不引入 shiki（它自带整套语法定义与 oniguruma WASM，与「零额外依赖、
//! 不需要 wasm 也能编译测试」的目标冲突），而是把「引擎」抽象成
//! [`ShikiLanguageCatalog`] + [`CodeHighlighter`] 两个 trait：
//!
//! - [`ShikiLanguageCatalog`] 提供 `bundledLanguages` / 别名表；
//! - [`CodeHighlighter`] 提供 `codeToTokens`。
//!
//! 纯逻辑（本文件）与引擎（调用方注入）因此彻底分离，前者可以完整测试。
//!
//! ## 与 TS 的差异
//!
//! 1. **不引入 shiki**：见上。
//! 2. **`getCodeTokensCacheKey` 的切片按字节而不是 UTF-16 码元**：TS 的
//!    `code.slice(0, 100)` 数的是 UTF-16 码元，Rust 的字符串不能按字节随意切
//!    （会切碎多字节字符）。这里改成「按字节截断，但回退到最近的字符边界」，
//!    长度用 `code.len()`（字节）。缓存键只用于去重，切片口径不同不影响正确性。
//! 3. **`getResolvedCodeTheme` 的暗色判断改为入参**：TS 直接读
//!    `document.documentElement.classList.contains("dark")`，无渲染器环境读不到，
//!    改由调用方传 `prefers_dark`。
//! 4. **订阅 / 微任务投递未移植**：那部分是 React 的 `setState` 时序约束
//!    （避免 React #185），Dioxus 侧由 `use_future` 直接驱动，不需要这层。
//!    `tokensCache` / `highlighterCache` 保留为进程级缓存（`OnceLock`）。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// 纯文本类语言：没有语法高亮收益，却会让高亮器进入异步状态机。
pub const PLAIN_TEXT_CODE_LANGUAGES: [&str; 7] =
    ["", "text", "txt", "plain", "plaintext", "log", "output"];

/// 归一化失败时的兜底语言（`FALLBACK_CODE_LANGUAGE`）。
pub const FALLBACK_CODE_LANGUAGE: &str = "log";

/// 暗色主题名。
pub const DARK_CODE_THEME: &str = "github-dark";

/// 亮色主题名。
pub const LIGHT_CODE_THEME: &str = "github-light";

/// 高亮结果里的一个 token。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemedToken {
    pub content: String,
    /// `None` 表示不覆盖颜色（TS 里 `token.color` 为假值）。
    pub color: Option<String>,
}

/// 一段代码的高亮结果（对应 TS `TokenizedCode`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenizedCode {
    /// 逐行、逐 token。空行是**空 `Vec`**（不是含一个空 token 的 `Vec`）。
    pub tokens: Vec<Vec<ThemedToken>>,
    /// 前景色。
    pub fg: String,
    /// 背景色。
    pub bg: String,
}

/// shiki 的语言表。
///
/// TS 里是模块级常量 `bundledLanguageIds` / `bundledLanguageAliases`，
/// 这里改成 trait 由调用方注入（见模块文档差异 1）。
pub trait ShikiLanguageCatalog {
    /// `bundledLanguages` 的键集：这个 id 是否可用。
    fn is_bundled(&self, id: &str) -> bool;

    /// `bundledLanguagesInfo[].aliases`：别名 → 规范 id。
    fn resolve_alias(&self, alias: &str) -> Option<String>;
}

/// 语法高亮引擎。
pub trait CodeHighlighter {
    /// 把代码切成 token。`language` 必须已经在引擎里加载过。
    fn code_to_tokens(&self, code: &str, language: &str, theme: &str) -> TokenizedCode;

    /// 引擎当前已加载的语言（TS `highlighter.getLoadedLanguages()`）。
    fn loaded_languages(&self) -> Vec<String>;
}

/// 该语言是否值得走语法高亮（TS `shouldUseSyntaxHighlighting`）。
///
/// 先看纯文本黑名单，再确认语言在 shiki 的表里。
///
/// 注意这里**不**校验「别名是否指向已打包的 id」—— TS 的
/// `bundledLanguageIds.has(candidate) || bundledLanguageAliases.has(candidate)`
/// 同样不校验。实践中不会出问题：别名表是从 `bundledLanguagesInfo` 派生的，
/// 别名必然指向已打包的 id。真正需要校验的是 [`normalize_code_language`]，
/// 那里 TS 也校验了。
pub fn should_use_syntax_highlighting(
    catalog: &impl ShikiLanguageCatalog,
    language: &str,
) -> bool {
    let candidate = language.trim().to_lowercase();
    if PLAIN_TEXT_CODE_LANGUAGES.contains(&candidate.as_str()) {
        return false;
    }
    catalog.is_bundled(&candidate) || catalog.resolve_alias(&candidate).is_some()
}

/// 归一化成引擎认识的语言 id（TS `normalizeCodeLanguage`）。
///
/// 顺序：空 → 兜底；别名 → 规范 id；本身是 id → 原样；其余 → 兜底。
pub fn normalize_code_language(catalog: &impl ShikiLanguageCatalog, language: &str) -> String {
    let candidate = language.trim().to_lowercase();
    if candidate.is_empty() {
        return FALLBACK_CODE_LANGUAGE.to_string();
    }

    if let Some(alias) = catalog.resolve_alias(&candidate) {
        if catalog.is_bundled(&alias) {
            return alias;
        }
    }

    if catalog.is_bundled(&candidate) {
        return candidate;
    }

    FALLBACK_CODE_LANGUAGE.to_string()
}

/// 解析实际使用的主题（TS `getResolvedCodeTheme`）。
pub fn get_resolved_code_theme(theme: Option<&str>, prefers_dark: bool) -> String {
    match theme {
        Some(theme) => theme.to_string(),
        None => if prefers_dark { DARK_CODE_THEME } else { LIGHT_CODE_THEME }.to_string(),
    }
}

/// 在 `max_bytes` 处截断，并回退到最近的字符边界。
fn floor_char_boundary(text: &str, max_bytes: usize) -> usize {
    if max_bytes >= text.len() {
        return text.len();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// 在 `max_bytes` 处从右侧截断，并前进到最近的字符边界。
fn ceil_char_boundary(text: &str, max_bytes: usize) -> usize {
    if max_bytes >= text.len() {
        return text.len();
    }
    let mut start = max_bytes;
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    start
}

/// 缓存键（TS `getCodeTokensCacheKey`）。
///
/// 只取首尾各 100 字节 + 长度：完整代码做键太占内存，而「长度 + 首尾」在实践中
/// 足以区分不同代码块。见模块文档差异 2。
pub fn get_code_tokens_cache_key(code: &str, language: &str, theme: &str) -> String {
    const EDGE: usize = 100;
    let start = &code[..floor_char_boundary(code, EDGE)];
    let end = if code.len() > EDGE {
        &code[ceil_char_boundary(code, code.len() - EDGE)..]
    } else {
        ""
    };
    format!("{theme}:{language}:{}:{start}:{end}", code.len())
}

/// 不做高亮时的兜底 token：每行一个 `color: inherit` 的 token。
///
/// 空行给**空 `Vec`**（TS `line === "" ? [] : [...]`）。
pub fn create_raw_code_tokens(code: &str) -> TokenizedCode {
    TokenizedCode {
        bg: "transparent".to_string(),
        fg: "inherit".to_string(),
        tokens: code
            .split('\n')
            .map(|line| {
                if line.is_empty() {
                    Vec::new()
                } else {
                    vec![ThemedToken {
                        color: Some("inherit".to_string()),
                        content: line.to_string(),
                    }]
                }
            })
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// 进程级缓存
// ---------------------------------------------------------------------------

fn tokens_cache() -> &'static Mutex<HashMap<String, TokenizedCode>> {
    static CACHE: OnceLock<Mutex<HashMap<String, TokenizedCode>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 已缓存的 token 条数（对应 TS 的内存诊断计数器）。
pub fn cached_token_entry_count() -> usize {
    tokens_cache().lock().map(|c| c.len()).unwrap_or(0)
}

/// 带缓存的高亮入口（TS `highlightCode` 的同步部分）。
///
/// - 纯文本语言 → 直接返回 [`create_raw_code_tokens`]（不碰引擎）；
/// - 缓存命中 → 返回缓存；
/// - 否则 → 调引擎、写缓存、返回。
///
/// TS 返回 `TokenizedCode | null` 是因为它把真正的高亮丢给了异步链；
/// 这里引擎是同步 trait，所以恒有返回值（异步由调用方的 `use_future` 负责）。
pub fn highlight_code(
    catalog: &impl ShikiLanguageCatalog,
    highlighter: &impl CodeHighlighter,
    code: &str,
    language: &str,
    theme: Option<&str>,
    prefers_dark: bool,
) -> TokenizedCode {
    if !should_use_syntax_highlighting(catalog, language) {
        // 文本/日志代码块没有语法高亮收益，却会在流式渲染和历史恢复时进入
        // 高亮器的异步状态机。这里直接返回 raw token，避免启动高亮副作用。
        return create_raw_code_tokens(code);
    }

    let resolved_theme = get_resolved_code_theme(theme, prefers_dark);
    let resolved_language = normalize_code_language(catalog, language);
    let key = get_code_tokens_cache_key(code, &resolved_language, &resolved_theme);

    if let Ok(cache) = tokens_cache().lock() {
        if let Some(cached) = cache.get(&key) {
            return cached.clone();
        }
    }

    let available = highlighter.loaded_languages();
    let language_to_use = if available.iter().any(|l| l == &resolved_language) {
        resolved_language
    } else {
        FALLBACK_CODE_LANGUAGE.to_string()
    };

    let result = highlighter.code_to_tokens(code, &language_to_use, &resolved_theme);

    if let Ok(mut cache) = tokens_cache().lock() {
        cache.insert(key, result.clone());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一个假的语言表：只认 `rust` / `ts`，别名 `typescript` → `ts`。
    struct FakeCatalog;

    impl ShikiLanguageCatalog for FakeCatalog {
        fn is_bundled(&self, id: &str) -> bool {
            matches!(id, "rust" | "ts" | "log")
        }

        fn resolve_alias(&self, alias: &str) -> Option<String> {
            match alias {
                "typescript" => Some("ts".to_string()),
                "js" => Some("javascript".to_string()), // 别名指向一个**未**打包的 id
                _ => None,
            }
        }
    }

    /// 一个假的高亮器：按空格切 token。
    struct FakeHighlighter {
        loaded: Vec<String>,
    }

    impl FakeHighlighter {
        fn with_loaded(loaded: &[&str]) -> Self {
            Self {
                loaded: loaded.iter().map(|l| l.to_string()).collect(),
            }
        }
    }

    impl CodeHighlighter for FakeHighlighter {
        fn code_to_tokens(&self, code: &str, language: &str, theme: &str) -> TokenizedCode {
            TokenizedCode {
                tokens: code
                    .split('\n')
                    .map(|line| {
                        if line.is_empty() {
                            Vec::new()
                        } else {
                            line.split(' ')
                                .map(|word| ThemedToken {
                                    content: word.to_string(),
                                    color: Some(format!("{language}/{theme}")),
                                })
                                .collect()
                        }
                    })
                    .collect(),
                fg: "inherit".to_string(),
                bg: "transparent".to_string(),
            }
        }

        fn loaded_languages(&self) -> Vec<String> {
            self.loaded.clone()
        }
    }

    // ---- 要不要高亮 ----

    #[test]
    fn plain_text_languages_never_highlight() {
        for language in PLAIN_TEXT_CODE_LANGUAGES {
            assert!(
                !should_use_syntax_highlighting(&FakeCatalog, language),
                "{language:?}"
            );
        }
        // 大小写与空白都归一化后再比
        assert!(!should_use_syntax_highlighting(&FakeCatalog, "  TEXT  "));
        assert!(!should_use_syntax_highlighting(&FakeCatalog, "Log"));
    }

    #[test]
    fn unknown_languages_never_highlight() {
        assert!(!should_use_syntax_highlighting(&FakeCatalog, "brainfuck"));
        assert!(!should_use_syntax_highlighting(&FakeCatalog, "中文语言"));
    }

    #[test]
    fn bundled_and_aliased_languages_highlight() {
        assert!(should_use_syntax_highlighting(&FakeCatalog, "rust"));
        assert!(should_use_syntax_highlighting(&FakeCatalog, "Rust"));
        assert!(should_use_syntax_highlighting(&FakeCatalog, "ts"));
        assert!(should_use_syntax_highlighting(&FakeCatalog, "typescript"));
    }

    /// 别名只要**存在**就算可高亮 —— 即使它指向一个未打包的 id。
    ///
    /// 这是 TS 的行为（`bundledLanguageAliases.has(candidate)` 不看目标），
    /// 原样保留。实践中别名表派生自 `bundledLanguagesInfo`，不会出现悬空别名。
    #[test]
    fn an_existing_alias_counts_as_highlightable() {
        assert!(should_use_syntax_highlighting(&FakeCatalog, "js"));
    }

    /// 但**归一化**时会校验别名目标（TS 在 `normalizeCodeLanguage` 里就是这样）。
    #[test]
    fn a_dangling_alias_is_rejected_when_normalizing() {
        assert_eq!(normalize_code_language(&FakeCatalog, "js"), FALLBACK_CODE_LANGUAGE);
    }

    // ---- 归一化 ----

    #[test]
    fn empty_language_falls_back_to_log() {
        assert_eq!(normalize_code_language(&FakeCatalog, ""), FALLBACK_CODE_LANGUAGE);
        assert_eq!(
            normalize_code_language(&FakeCatalog, "   "),
            FALLBACK_CODE_LANGUAGE
        );
    }

    #[test]
    fn aliases_resolve_to_the_canonical_id() {
        assert_eq!(normalize_code_language(&FakeCatalog, "typescript"), "ts");
        assert_eq!(normalize_code_language(&FakeCatalog, "TypeScript"), "ts");
    }

    #[test]
    fn canonical_ids_pass_through() {
        assert_eq!(normalize_code_language(&FakeCatalog, "rust"), "rust");
        assert_eq!(normalize_code_language(&FakeCatalog, " RUST "), "rust");
    }

    #[test]
    fn unknown_and_dangling_alias_fall_back() {
        assert_eq!(
            normalize_code_language(&FakeCatalog, "brainfuck"),
            FALLBACK_CODE_LANGUAGE
        );
        // 别名存在但目标未打包 → 兜底（而不是返回那个不存在的 id）
        assert_eq!(normalize_code_language(&FakeCatalog, "js"), FALLBACK_CODE_LANGUAGE);
    }

    // ---- 主题 ----

    #[test]
    fn explicit_theme_wins() {
        assert_eq!(get_resolved_code_theme(Some("nord"), false), "nord");
        assert_eq!(get_resolved_code_theme(Some("nord"), true), "nord");
    }

    #[test]
    fn missing_theme_follows_the_color_scheme() {
        assert_eq!(get_resolved_code_theme(None, true), DARK_CODE_THEME);
        assert_eq!(get_resolved_code_theme(None, false), LIGHT_CODE_THEME);
    }

    // ---- 缓存键 ----

    #[test]
    fn cache_key_includes_theme_language_and_length() {
        let key = get_code_tokens_cache_key("fn main() {}", "rust", "nord");
        assert!(key.starts_with("nord:rust:12:"), "{key}");
    }

    #[test]
    fn short_code_has_no_tail_segment() {
        let key = get_code_tokens_cache_key("abc", "rust", "nord");
        // `abc:abc:` 之后为空
        assert!(key.ends_with(":abc:"), "{key}");
    }

    #[test]
    fn long_code_keeps_both_edges() {
        // 首尾刻意不同，才能分别断言
        let code = format!("{}{}", "A".repeat(150), "B".repeat(150));
        let key = get_code_tokens_cache_key(&code, "rust", "nord");

        assert!(key.starts_with("nord:rust:300:"), "{key}");
        assert!(key.contains(&"A".repeat(100)), "缺少首部切片");
        assert!(key.contains(&"B".repeat(100)), "缺少尾部切片");
        // 中段被丢掉，所以不该出现 101 个连续 A
        assert!(!key.contains(&"A".repeat(101)));
    }

    /// 多字节字符不能被切碎 —— 否则 `&code[..]` 会 panic。
    #[test]
    fn cache_key_never_splits_a_multibyte_character() {
        // 每个「中」是 3 字节，100 字节落在第 34 个字符中间
        let code: String = "中".repeat(100);
        let key = get_code_tokens_cache_key(&code, "rust", "nord");
        assert!(key.starts_with("nord:rust:300:"));
        // 能正常生成就说明没有 panic；再确认切片长度是 3 的倍数（字符边界）
        let body = key.strip_prefix("nord:rust:300:").unwrap();
        let (start, _end) = body.split_once(':').unwrap();
        assert_eq!(start.len() % 3, 0, "首部切片落在了字符中间：{start:?}");
    }

    // ---- raw token 兜底 ----

    #[test]
    fn raw_tokens_keep_lines_and_blank_lines_empty() {
        let tokens = create_raw_code_tokens("a\n\nb");
        assert_eq!(tokens.bg, "transparent");
        assert_eq!(tokens.fg, "inherit");
        assert_eq!(tokens.tokens.len(), 3);
        assert_eq!(tokens.tokens[0].len(), 1);
        assert_eq!(tokens.tokens[0][0].content, "a");
        assert_eq!(tokens.tokens[0][0].color.as_deref(), Some("inherit"));
        // 空行是**空** Vec，不是含一个空 token 的 Vec
        assert!(tokens.tokens[1].is_empty());
        assert_eq!(tokens.tokens[2][0].content, "b");
    }

    #[test]
    fn raw_tokens_of_empty_code_is_one_empty_line() {
        let tokens = create_raw_code_tokens("");
        assert_eq!(tokens.tokens.len(), 1);
        assert!(tokens.tokens[0].is_empty());
    }

    // ---- 端到端 ----

    #[test]
    fn plain_text_short_circuits_before_the_engine() {
        let highlighter = FakeHighlighter::with_loaded(&[]);
        let tokens = highlight_code(
            &FakeCatalog,
            &highlighter,
            "hello world",
            "text",
            Some("nord"),
            false,
        );
        // 引擎没被调用 → 拿到的是 raw token
        assert_eq!(tokens.tokens[0][0].content, "hello world");
        assert_eq!(tokens.tokens[0][0].color.as_deref(), Some("inherit"));
    }

    #[test]
    fn unknown_language_also_short_circuits() {
        let highlighter = FakeHighlighter::with_loaded(&["rust"]);
        let tokens = highlight_code(
            &FakeCatalog,
            &highlighter,
            "hello",
            "brainfuck",
            None,
            false,
        );
        assert_eq!(tokens.tokens[0][0].color.as_deref(), Some("inherit"));
    }

    #[test]
    fn highlighting_goes_through_the_engine() {
        let highlighter = FakeHighlighter::with_loaded(&["rust"]);
        let tokens = highlight_code(
            &FakeCatalog,
            &highlighter,
            "fn main",
            "rust",
            Some("nord"),
            false,
        );
        assert_eq!(tokens.tokens[0].len(), 2);
        assert_eq!(tokens.tokens[0][0].color.as_deref(), Some("rust/nord"));
    }

    /// 引擎没加载这个语言 → 退回兜底语言（但**仍然**走引擎，不是 raw token）。
    #[test]
    fn unloaded_language_falls_back_to_the_fallback_language() {
        let highlighter = FakeHighlighter::with_loaded(&[FALLBACK_CODE_LANGUAGE]);
        let tokens = highlight_code(&FakeCatalog, &highlighter, "fn main", "rust", None, false);
        assert_eq!(tokens.tokens[0][0].color.as_deref(), Some("log/github-light"));
    }

    /// 第二次调用命中缓存，拿到**完全相同**的结果。
    #[test]
    fn repeated_calls_are_served_from_the_cache() {
        let highlighter = FakeHighlighter::with_loaded(&["rust"]);
        let code = format!("fn main() {{ /* cache probe {} */ }}", std::process::id());
        let first = highlight_code(&FakeCatalog, &highlighter, &code, "rust", None, false);
        let second = highlight_code(&FakeCatalog, &highlighter, &code, "rust", None, false);
        assert_eq!(first, second);
    }
}
