//! `diffs_highlighter_engine` —— Rust 移植自
//! `packages/ui/src/lib/diffsHighlighterEngine.ts`
//!
//! 这个模块只做一件事：**决定 diff 高亮用哪个引擎，并把参数固定下来**。
//! 里面的常量都是配置数据，没有行为，所以移植后是纯数据 + 一个构造器。
//!
//! ## 关键移植点：为什么必须是 `shiki-wasm`
//!
//! 见 TS 里那段长注释。要点：`@pierre/diffs` 默认用 shiki 的 **JavaScript 正则引擎**
//! （`shiki-js`）。它把 TextMate 语法逐条翻译成巨型 `RegExp` 并在引擎级缓存里永久持有，
//! V8 会把执行过的正则编译成原生代码放进 **code space**；双字节文本（中文注释等）
//! 还会再编译一份。主窗口 + 4 个 diff worker 共用同一个 256MB code range，长时间运行
//! 后会被这些正则占满，触发 renderer 的 V8 OOM（`CALL_AND_RETRY_LAST`，
//! 此时 old-space 仍有大量空闲，code cage「ran out of reservation」）。
//!
//! `oniguruma` WASM 引擎的正则活在 wasm 线性内存里，不占 V8 代码区，
//! 而且它是 TextMate 语法的参考实现。
//!
//! ## 与 TS 的差异
//!
//! 1. **不引入 `@pierre/diffs`**：这里只保留「参数是什么」，不构造它的对象。
//!    [`DiffsWorkerHighlighterOptions`] 是一个普通结构体，由宿主适配成真实库参数。
//! 2. **`lineDiffType` / `maxLineDiffLength` / `tokenizeMaxLineLength` 是库默认值**：
//!    TS 注释说明「先沿用库默认阈值，后续可基于慢日志再单独收紧，避免一次改动引入
//!    『高亮信息突然消失』的回归」。本移植把这些默认值显式写出来，方便日后对照。

/// 高亮引擎类型。**必须**用 `shiki-wasm`，理由见模块文档。
pub const DIFFS_PREFERRED_HIGHLIGHTER: &str = "shiki-wasm";

/// 逐词差异计算的类型（库默认值）。
pub const DIFFS_LINE_DIFF_TYPE: &str = "word-alt";

/// 参与逐词差异计算的最大行长（库默认值）。
pub const DIFFS_MAX_LINE_DIFF_LENGTH: usize = 1_000;

/// 参与 tokenize 的最大行长（库默认值）。
pub const DIFFS_TOKENIZE_MAX_LINE_LENGTH: usize = 1_000;

/// 是否启用 token transformer（库默认值）。
pub const DIFFS_USE_TOKEN_TRANSFORMER: bool = false;

/// 亮 / 暗两套高亮主题。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffsHighlighterThemeSettings {
    pub light_theme: String,
    pub dark_theme: String,
}

/// diff worker 池的初始化参数。
///
/// 主线程兜底渲染与 worker **必须使用同一个引擎选择**，否则同一个文件在
/// 「worker 还没起来」和「worker 已起来」两种状态下会呈现不同的高亮结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffsWorkerHighlighterOptions {
    pub light_theme: String,
    pub dark_theme: String,
    pub line_diff_type: String,
    pub max_line_diff_length: usize,
    pub tokenize_max_line_length: usize,
    pub use_token_transformer: bool,
    pub preferred_highlighter: String,
}

/// 构造 worker 池初始化参数（TS `createDiffsWorkerHighlighterOptions`）。
pub fn create_diffs_worker_highlighter_options(
    settings: &DiffsHighlighterThemeSettings,
) -> DiffsWorkerHighlighterOptions {
    DiffsWorkerHighlighterOptions {
        light_theme: settings.light_theme.clone(),
        dark_theme: settings.dark_theme.clone(),
        // 不同 patch 的逐词差异计算会额外占用主线程；沿用库默认阈值，
        // 避免一次改动引入「高亮信息突然消失」的回归。
        line_diff_type: DIFFS_LINE_DIFF_TYPE.to_string(),
        max_line_diff_length: DIFFS_MAX_LINE_DIFF_LENGTH,
        tokenize_max_line_length: DIFFS_TOKENIZE_MAX_LINE_LENGTH,
        use_token_transformer: DIFFS_USE_TOKEN_TRANSFORMER,
        preferred_highlighter: DIFFS_PREFERRED_HIGHLIGHTER.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engine_is_always_shiki_wasm() {
        let options = create_diffs_worker_highlighter_options(&DiffsHighlighterThemeSettings {
            light_theme: "github-light".to_string(),
            dark_theme: "github-dark".to_string(),
        });
        // 换成 `shiki-js` 会重新引入 V8 code space 耗尽的问题
        assert_eq!(options.preferred_highlighter, "shiki-wasm");
        assert_eq!(options.preferred_highlighter, DIFFS_PREFERRED_HIGHLIGHTER);
    }

    #[test]
    fn themes_are_passed_through() {
        let options = create_diffs_worker_highlighter_options(&DiffsHighlighterThemeSettings {
            light_theme: "nord".to_string(),
            dark_theme: "dracula".to_string(),
        });
        assert_eq!(options.light_theme, "nord");
        assert_eq!(options.dark_theme, "dracula");
    }

    #[test]
    fn library_defaults_are_pinned() {
        let options = create_diffs_worker_highlighter_options(&DiffsHighlighterThemeSettings {
            light_theme: "l".to_string(),
            dark_theme: "d".to_string(),
        });
        assert_eq!(options.line_diff_type, "word-alt");
        assert_eq!(options.max_line_diff_length, 1_000);
        assert_eq!(options.tokenize_max_line_length, 1_000);
        assert!(!options.use_token_transformer);
    }

    /// 主线程与 worker 必须拿到同一套参数（否则高亮结果会不一致）。
    #[test]
    fn repeated_calls_are_identical() {
        let settings = DiffsHighlighterThemeSettings {
            light_theme: "l".to_string(),
            dark_theme: "d".to_string(),
        };
        assert_eq!(
            create_diffs_worker_highlighter_options(&settings),
            create_diffs_worker_highlighter_options(&settings)
        );
    }
}
