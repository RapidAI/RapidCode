//! `DiffViewer` —— Rust 移植自 `packages/ui/src/components/ui/diff-viewer.tsx`
//!
//! 富 diff 视图：TS 侧把渲染整个交给 `@pierre/diffs` 的 `<PatchDiff>` /
//! `<MultiFileDiff>`（跑在 Shadow DOM 里），这个文件只负责
//! **参数、样式变量、以及「是 patch 还是双文件」的分流**。
//!
//! ## 关键移植点 1：两套输入形态，靠 `patch` 是不是字符串区分
//!
//! 入参是互斥联合：要么给 `patch`，要么给 `oldFile` + `newFile`。
//! TS 用 `typeof props.patch === "string"` 判断；Rust 侧用一个枚举表达，
//! 类型上就不可能出现「两个都给了」。
//!
//! ## 关键移植点 2：`hunkSeparators` 随形态变化
//!
//! `PatchDiff` 只有 patch 里的局部上下文，**没有**完整的 before/after 内容，
//! 所以展示「未修改行」的展开提示点了也没东西可展开。因此：
//!
//! | 形态 | `hunkSeparators` |
//! |---|---|
//! | patch | `"simple"` |
//! | 双文件 | `"line-info"`（保留可展开提示） |
//!
//! ## 关键移植点 3：字体不会被继承，必须显式给
//!
//! 完整 diff 跑在 Shadow DOM 里，**不会**自动继承外层的 `font-mono` class。
//! 所以 `--diffs-font-family` 必须通过 CSS 变量显式注入。
//!
//! ## 与 TS 的差异
//!
//! 1. **不引入 `@pierre/diffs`**：`<PatchDiff>` / `<MultiFileDiff>` 无对应物。
//!    本移植产出**渲染参数**（[`DiffViewerRenderPlan`]），由宿主接上真实渲染器。
//!    这也是本文件能被完整测试的原因。
//! 2. **`style` 改为 `extra_style` 字符串**：TS 的 `style` 是 `CSSProperties`
//!    对象且**最后展开**（覆盖上面的 CSS 变量）。Rust 侧用一段内联样式字符串表达，
//!    追加在末尾，覆盖顺序一致。
//! 3. **`disableWorkerPool` / `selectedLines` / `themeType` 只做透传**：
//!    它们是给 `@pierre/diffs` 的开关，本移植只保留字段与默认值。
//! 4. **`className` 落在**根节点**上**（`h-full w-full overflow-auto`），
//!    而 `diffClassName` 落在**内层渲染器**上（`min-h-full w-full`）。
//!    两者不要混。

use crate::cn::cn;
use crate::diffs_highlighter_engine::{DiffsWorkerHighlighterOptions, DIFFS_PREFERRED_HIGHLIGHTER};
use dioxus::prelude::*;

/// 根节点基础类名。
pub const DIFF_VIEWER_BASE: &str = "h-full w-full overflow-auto";

/// 内层渲染器的默认类名（TS `diffClassName` 的默认值）。
pub const DIFF_VIEWER_DIFF_CLASS: &str = "min-h-full w-full";

/// 默认字号（px）。
pub const DIFF_VIEWER_DEFAULT_FONT_SIZE_PX: f64 = 12.0;

/// `unsafeCSS` 的默认值。
///
/// TS 里它是空串，注释说明：需要覆盖 Shadow DOM 内部样式时走 `unsafeCSS` 注入，
/// **且只做最小覆盖，不做大范围样式重写**。
pub const DIFF_VIEWER_UNSAFE_CSS: &str = "";

/// `diffStyle` 的取值。
pub const DIFF_VIEWER_DIFF_STYLE: &str = "unified";

/// `diffIndicators` 的取值。
pub const DIFF_VIEWER_DIFF_INDICATORS: &str = "bars";

/// `lineDiffType` 的取值。
pub const DIFF_VIEWER_LINE_DIFF_TYPE: &str = "word-alt";

/// `overflow` 的取值。
pub const DIFF_VIEWER_OVERFLOW: &str = "scroll";

/// patch 形态的 `hunkSeparators`。
pub const DIFF_VIEWER_HUNK_SEPARATORS_PATCH: &str = "simple";

/// 双文件形态的 `hunkSeparators`。
pub const DIFF_VIEWER_HUNK_SEPARATORS_MULTI_FILE: &str = "line-info";

/// 被选中的行区间落在哪一侧。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffViewerSelectedSide {
    Additions,
    Deletions,
}

impl DiffViewerSelectedSide {
    pub fn as_str(&self) -> &'static str {
        match self {
            DiffViewerSelectedSide::Additions => "additions",
            DiffViewerSelectedSide::Deletions => "deletions",
        }
    }
}

/// 选中的行区间。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiffViewerSelectedLineRange {
    pub start: usize,
    pub end: usize,
    pub side: DiffViewerSelectedSide,
}

/// 一个文件的内容（对应 `@pierre/diffs` 的 `FileContents`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffViewerFileContents {
    pub name: String,
    pub contents: String,
}

/// 渲染器形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffViewerKind {
    /// 只有一份 patch。
    Patch,
    /// 有完整的旧 / 新文件内容。
    MultiFile,
}

/// 视图入参（TS 的互斥联合）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffViewerInput {
    Patch {
        patch: String,
    },
    MultiFile {
        old_file: DiffViewerFileContents,
        new_file: DiffViewerFileContents,
    },
}

impl DiffViewerInput {
    pub fn kind(&self) -> DiffViewerKind {
        match self {
            DiffViewerInput::Patch { .. } => DiffViewerKind::Patch,
            DiffViewerInput::MultiFile { .. } => DiffViewerKind::MultiFile,
        }
    }

    /// 对应 TS 的 `isPatchDiffProps`（`typeof props.patch === "string"`）。
    pub fn is_patch(&self) -> bool {
        matches!(self, DiffViewerInput::Patch { .. })
    }
}

/// 主题类型（亮 / 暗）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffViewerThemeType {
    Light,
    Dark,
}

impl DiffViewerThemeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            DiffViewerThemeType::Light => "light",
            DiffViewerThemeType::Dark => "dark",
        }
    }
}

/// 按形态挑 `hunkSeparators`（见模块文档关键移植点 2）。
pub fn resolve_hunk_separators(kind: DiffViewerKind) -> &'static str {
    match kind {
        DiffViewerKind::Patch => DIFF_VIEWER_HUNK_SEPARATORS_PATCH,
        DiffViewerKind::MultiFile => DIFF_VIEWER_HUNK_SEPARATORS_MULTI_FILE,
    }
}

/// 亮暗两套主题。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffViewerTheme {
    pub light: String,
    pub dark: String,
}

/// 解析主题：**必须两套都给**才生效（TS `lightTheme && darkTheme ? {...} : undefined`）。
pub fn resolve_diff_viewer_theme(
    light_theme: Option<&str>,
    dark_theme: Option<&str>,
) -> Option<DiffViewerTheme> {
    match (light_theme, dark_theme) {
        (Some(light), Some(dark)) => Some(DiffViewerTheme {
            light: light.to_string(),
            dark: dark.to_string(),
        }),
        _ => None,
    }
}

/// 渲染参数（`FileDiffOptions` 的 Rust 表达）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffViewerOptions {
    pub diff_style: String,
    pub diff_indicators: String,
    pub disable_file_header: bool,
    pub hunk_separators: String,
    pub line_diff_type: String,
    pub overflow: String,
    pub unsafe_css: String,
    pub theme: Option<DiffViewerTheme>,
    pub theme_type: Option<DiffViewerThemeType>,
    pub preferred_highlighter: String,
}

impl DiffViewerOptions {
    /// TS 里那套写死的默认值。
    pub fn new(kind: DiffViewerKind) -> Self {
        Self {
            diff_style: DIFF_VIEWER_DIFF_STYLE.to_string(),
            diff_indicators: DIFF_VIEWER_DIFF_INDICATORS.to_string(),
            disable_file_header: true,
            hunk_separators: resolve_hunk_separators(kind).to_string(),
            line_diff_type: DIFF_VIEWER_LINE_DIFF_TYPE.to_string(),
            overflow: DIFF_VIEWER_OVERFLOW.to_string(),
            unsafe_css: DIFF_VIEWER_UNSAFE_CSS.to_string(),
            theme: None,
            theme_type: None,
            preferred_highlighter: DIFFS_PREFERRED_HIGHLIGHTER.to_string(),
        }
    }
}

/// 宿主交给渲染器的完整计划。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffViewerRenderPlan {
    pub kind: DiffViewerKind,
    pub options: DiffViewerOptions,
    /// 内层渲染器的类名。
    pub diff_class: String,
    /// 内层渲染器的内联样式（CSS 变量）。
    pub style: String,
    pub disable_worker_pool: bool,
    pub selected_lines: Option<DiffViewerSelectedLineRange>,
    /// `Patch` 形态下是 patch 原文；`MultiFile` 形态下为空。
    pub patch: String,
    /// `MultiFile` 形态下的旧 / 新文件。
    pub old_file: Option<DiffViewerFileContents>,
    pub new_file: Option<DiffViewerFileContents>,
}

/// 生成 CSS 变量样式串（TS 的 `viewerStyle`）。
///
/// `extra_style` **最后**追加，所以它能覆盖上面的变量（与 TS 的 `...style` 一致）。
pub fn diff_viewer_style(font_size_px: f64, extra_style: &str) -> String {
    let mut style = String::new();
    // Shadow DOM 不继承外层 CSS 变量，这三条必须显式给
    style.push_str("--diffs-bg: var(--color-background);");
    style.push_str("--diffs-light-bg: var(--color-background);");
    style.push_str("--diffs-dark-bg: var(--color-background);");
    // 完整 diff 跑在 Shadow DOM 中，不会自动继承外层 font-mono class
    style.push_str("--diffs-font-family: var(--font-mono);");
    style.push_str(&format!("--diffs-font-size: {font_size_px}px;"));
    style.push_str(extra_style);
    style
}

pub fn diff_viewer_classes(class: &str) -> String {
    cn(&[DIFF_VIEWER_BASE, class])
}

/// 内层渲染器的类名（TS `cn("min-h-full w-full", diffClassName)`）。
pub fn diff_viewer_diff_classes(diff_class_name: &str) -> String {
    cn(&[DIFF_VIEWER_DIFF_CLASS, diff_class_name])
}

/// 组装渲染计划 —— TS `DiffViewerComponent` 的函数体。
#[allow(clippy::too_many_arguments)]
pub fn build_diff_viewer_render_plan(
    input: &DiffViewerInput,
    light_theme: Option<&str>,
    dark_theme: Option<&str>,
    theme_type: Option<DiffViewerThemeType>,
    diff_class_name: &str,
    font_size_px: f64,
    extra_style: &str,
    disable_worker_pool: bool,
    selected_lines: Option<DiffViewerSelectedLineRange>,
    options_override: Option<DiffViewerOptions>,
) -> DiffViewerRenderPlan {
    let kind = input.kind();

    let mut options = DiffViewerOptions::new(kind);
    options.theme = resolve_diff_viewer_theme(light_theme, dark_theme);
    options.theme_type = theme_type;
    // TS 把 `...optionsOverride` 放在最后展开 → 调用方能覆盖任意一项
    if let Some(override_options) = options_override {
        options = override_options;
    }

    let (patch, old_file, new_file) = match input {
        DiffViewerInput::Patch { patch } => (patch.clone(), None, None),
        DiffViewerInput::MultiFile { old_file, new_file } => {
            (String::new(), Some(old_file.clone()), Some(new_file.clone()))
        }
    };

    DiffViewerRenderPlan {
        kind,
        options,
        diff_class: diff_viewer_diff_classes(diff_class_name),
        style: diff_viewer_style(font_size_px, extra_style),
        disable_worker_pool,
        selected_lines,
        patch,
        old_file,
        new_file,
    }
}

/// 富 diff 视图。
#[component]
pub fn DiffViewer(
    /// 渲染入参（patch 或双文件）。
    input: DiffViewerInput,
    /// 字号（px），默认 12。
    #[props(default = DIFF_VIEWER_DEFAULT_FONT_SIZE_PX)] font_size_px: f64,
    /// 亮色主题（必须与暗色成对出现才生效）。
    #[props(default)] light_theme: Option<String>,
    /// 暗色主题。
    #[props(default)] dark_theme: Option<String>,
    /// 强制亮 / 暗（`None` 表示跟随）。
    #[props(default)] theme_type: Option<DiffViewerThemeType>,
    /// 内层渲染器的附加类名。
    #[props(default, into)] diff_class_name: String,
    /// 追加到内联样式末尾（可覆盖 CSS 变量）。
    #[props(default, into)] extra_style: String,
    /// 关闭 worker 池（主线程兜底渲染）。
    #[props(default)] disable_worker_pool: bool,
    /// 选中的行区间。
    #[props(default)] selected_lines: Option<DiffViewerSelectedLineRange>,
    #[props(default, into)] class: String,
) -> Element {
    let plan = build_diff_viewer_render_plan(
        &input,
        light_theme.as_deref(),
        dark_theme.as_deref(),
        theme_type,
        &diff_class_name,
        font_size_px,
        &extra_style,
        disable_worker_pool,
        selected_lines,
        None,
    );

    rsx! {
        div {
            class: diff_viewer_classes(&class),
            // TS 里是裸属性 `data-diff-viewer=""`
            "data-diff-viewer": "",
            "data-diff-kind": match plan.kind {
                DiffViewerKind::Patch => "patch",
                DiffViewerKind::MultiFile => "multi-file",
            },
            "data-diff-hunk-separators": plan.options.hunk_separators.clone(),
            style: plan.style.clone(),
            // 真正的 diff 渲染在宿主侧（`@pierre/diffs` 的 Shadow DOM 里），
            // 这里挂一个占位节点承载类名与参数，见模块文档差异 1。
            div {
                class: plan.diff_class.clone(),
                "data-slot": "diff-viewer-body",
                "data-preferred-highlighter": plan.options.preferred_highlighter.clone(),
            }
        }
    }
}

/// 把 [`DiffViewerOptions`] 与 worker 池参数对齐（宿主启动 worker 时用）。
pub fn diff_viewer_worker_options(
    options: &DiffViewerOptions,
    fallback_light: &str,
    fallback_dark: &str,
) -> DiffsWorkerHighlighterOptions {
    let (light, dark) = match &options.theme {
        Some(theme) => (theme.light.clone(), theme.dark.clone()),
        None => (fallback_light.to_string(), fallback_dark.to_string()),
    };
    DiffsWorkerHighlighterOptions {
        light_theme: light,
        dark_theme: dark,
        line_diff_type: options.line_diff_type.clone(),
        max_line_diff_length: crate::diffs_highlighter_engine::DIFFS_MAX_LINE_DIFF_LENGTH,
        tokenize_max_line_length: crate::diffs_highlighter_engine::DIFFS_TOKENIZE_MAX_LINE_LENGTH,
        use_token_transformer: crate::diffs_highlighter_engine::DIFFS_USE_TOKEN_TRANSFORMER,
        preferred_highlighter: options.preferred_highlighter.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch_input() -> DiffViewerInput {
        DiffViewerInput::Patch {
            patch: "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b".to_string(),
        }
    }

    fn multi_input() -> DiffViewerInput {
        DiffViewerInput::MultiFile {
            old_file: DiffViewerFileContents {
                name: "x".to_string(),
                contents: "a\n".to_string(),
            },
            new_file: DiffViewerFileContents {
                name: "x".to_string(),
                contents: "b\n".to_string(),
            },
        }
    }

    // ---- 形态判别 ----

    #[test]
    fn the_input_kind_follows_the_variant() {
        assert!(patch_input().is_patch());
        assert_eq!(patch_input().kind(), DiffViewerKind::Patch);
        assert!(!multi_input().is_patch());
        assert_eq!(multi_input().kind(), DiffViewerKind::MultiFile);
    }

    /// 双文件形态必须有完整的 before/after —— 这是它和 patch 的本质区别。
    #[test]
    fn multi_file_carries_both_versions() {
        match multi_input() {
            DiffViewerInput::MultiFile { old_file, new_file } => {
                assert_eq!(old_file.contents, "a\n");
                assert_eq!(new_file.contents, "b\n");
            }
            DiffViewerInput::Patch { .. } => panic!("形态错了"),
        }
    }

    // ---- hunkSeparators ----

    #[test]
    fn hunk_separators_follow_the_kind() {
        // patch 只有局部上下文，展开提示点了也没东西可展 → simple
        assert_eq!(resolve_hunk_separators(DiffViewerKind::Patch), "simple");
        // 双文件有完整内容 → 保留可展开提示
        assert_eq!(
            resolve_hunk_separators(DiffViewerKind::MultiFile),
            "line-info"
        );
    }

    // ---- 主题 ----

    #[test]
    fn both_themes_are_required() {
        assert_eq!(
            resolve_diff_viewer_theme(Some("l"), Some("d")),
            Some(DiffViewerTheme {
                light: "l".to_string(),
                dark: "d".to_string()
            })
        );
        // 只给一套 → 整体不生效
        assert_eq!(resolve_diff_viewer_theme(Some("l"), None), None);
        assert_eq!(resolve_diff_viewer_theme(None, Some("d")), None);
        assert_eq!(resolve_diff_viewer_theme(None, None), None);
    }

    #[test]
    fn theme_type_strings() {
        assert_eq!(DiffViewerThemeType::Light.as_str(), "light");
        assert_eq!(DiffViewerThemeType::Dark.as_str(), "dark");
    }

    #[test]
    fn selected_side_strings() {
        assert_eq!(DiffViewerSelectedSide::Additions.as_str(), "additions");
        assert_eq!(DiffViewerSelectedSide::Deletions.as_str(), "deletions");
    }

    // ---- 样式 ----

    #[test]
    fn style_carries_every_diffs_variable() {
        let style = diff_viewer_style(12.0, "");
        for variable in [
            "--diffs-bg: var(--color-background);",
            "--diffs-light-bg: var(--color-background);",
            "--diffs-dark-bg: var(--color-background);",
            "--diffs-font-family: var(--font-mono);",
            "--diffs-font-size: 12px;",
        ] {
            assert!(style.contains(variable), "缺少 {variable}：{style}");
        }
    }

    #[test]
    fn font_size_is_parameterized() {
        assert!(diff_viewer_style(14.0, "").contains("--diffs-font-size: 14px;"));
        assert!(diff_viewer_style(11.5, "").contains("--diffs-font-size: 11.5px;"));
    }

    /// 调用方给的样式**最后**追加，所以能覆盖上面的变量（同 TS 的 `...style`）。
    #[test]
    fn extra_style_wins_by_coming_last() {
        let style = diff_viewer_style(12.0, "--diffs-font-size: 20px;");
        let custom = style.find("20px").expect("自定义样式应在");
        let default = style.find("12px").expect("默认样式应在");
        assert!(custom > default, "自定义样式必须排在后面：{style}");
    }

    #[test]
    fn classes() {
        assert_eq!(diff_viewer_classes(""), DIFF_VIEWER_BASE);
        let root = diff_viewer_classes("h-64");
        assert!(root.contains("overflow-auto"));
        assert!(root.contains("h-64"));

        // 内层渲染器类名与根节点是两回事
        let inner = diff_viewer_diff_classes("");
        assert_eq!(inner, DIFF_VIEWER_DIFF_CLASS);
        assert!(diff_viewer_diff_classes("text-xs").contains("text-xs"));
    }

    // ---- 渲染计划 ----

    #[test]
    fn plan_defaults_match_the_ts_options() {
        let plan = build_diff_viewer_render_plan(
            &patch_input(),
            None,
            None,
            None,
            "",
            12.0,
            "",
            false,
            None,
            None,
        );
        assert_eq!(plan.options.diff_style, "unified");
        assert_eq!(plan.options.diff_indicators, "bars");
        assert!(plan.options.disable_file_header);
        assert_eq!(plan.options.line_diff_type, "word-alt");
        assert_eq!(plan.options.overflow, "scroll");
        assert_eq!(plan.options.unsafe_css, "");
        assert_eq!(plan.options.preferred_highlighter, "shiki-wasm");
        assert_eq!(plan.options.theme, None);
        assert_eq!(plan.options.theme_type, None);
        assert!(!plan.disable_worker_pool);
        assert_eq!(plan.selected_lines, None);
    }

    #[test]
    fn plan_routes_the_payload_by_kind() {
        let patch_plan = build_diff_viewer_render_plan(
            &patch_input(),
            None,
            None,
            None,
            "",
            12.0,
            "",
            false,
            None,
            None,
        );
        assert!(patch_plan.patch.contains("@@ -1 +1 @@"));
        assert_eq!(patch_plan.old_file, None);
        assert_eq!(patch_plan.new_file, None);
        assert_eq!(patch_plan.options.hunk_separators, "simple");

        let multi_plan = build_diff_viewer_render_plan(
            &multi_input(),
            None,
            None,
            None,
            "",
            12.0,
            "",
            false,
            None,
            None,
        );
        assert_eq!(multi_plan.patch, "");
        assert_eq!(multi_plan.old_file.unwrap().contents, "a\n");
        assert_eq!(multi_plan.new_file.unwrap().contents, "b\n");
        assert_eq!(multi_plan.options.hunk_separators, "line-info");
    }

    #[test]
    fn plan_applies_themes_and_selection() {
        let plan = build_diff_viewer_render_plan(
            &patch_input(),
            Some("github-light"),
            Some("github-dark"),
            Some(DiffViewerThemeType::Dark),
            "text-xs",
            14.0,
            "",
            true,
            Some(DiffViewerSelectedLineRange {
                start: 3,
                end: 7,
                side: DiffViewerSelectedSide::Additions,
            }),
            None,
        );

        assert_eq!(
            plan.options.theme,
            Some(DiffViewerTheme {
                light: "github-light".to_string(),
                dark: "github-dark".to_string()
            })
        );
        assert_eq!(plan.options.theme_type, Some(DiffViewerThemeType::Dark));
        assert!(plan.disable_worker_pool);
        let selected = plan.selected_lines.expect("应带上选区");
        assert_eq!((selected.start, selected.end), (3, 7));
        assert_eq!(selected.side, DiffViewerSelectedSide::Additions);
        assert!(plan.diff_class.contains("text-xs"));
        assert!(plan.style.contains("--diffs-font-size: 14px;"));
    }

    /// `optionsOverride` 整体替换默认值（TS 里是 `...optionsOverride` 最后展开）。
    #[test]
    fn an_options_override_replaces_the_defaults_wholesale() {
        let override_options = DiffViewerOptions {
            diff_style: "split".to_string(),
            hunk_separators: "custom".to_string(),
            ..DiffViewerOptions::new(DiffViewerKind::Patch)
        };
        let plan = build_diff_viewer_render_plan(
            &patch_input(),
            Some("l"),
            Some("d"),
            None,
            "",
            12.0,
            "",
            false,
            None,
            Some(override_options),
        );
        assert_eq!(plan.options.diff_style, "split");
        assert_eq!(plan.options.hunk_separators, "custom");
        // 覆盖是整体替换，所以主题也回到默认（None）—— 与 TS 的展开顺序一致
        assert_eq!(plan.options.theme, None);
    }

    // ---- worker 参数对齐 ----

    #[test]
    fn worker_options_reuse_the_plan_theme() {
        let options = DiffViewerOptions {
            theme: Some(DiffViewerTheme {
                light: "nord".to_string(),
                dark: "dracula".to_string(),
            }),
            ..DiffViewerOptions::new(DiffViewerKind::Patch)
        };
        let worker = diff_viewer_worker_options(&options, "fallback-l", "fallback-d");
        assert_eq!(worker.light_theme, "nord");
        assert_eq!(worker.dark_theme, "dracula");
        assert_eq!(worker.preferred_highlighter, "shiki-wasm");
        assert_eq!(worker.line_diff_type, "word-alt");
    }

    #[test]
    fn worker_options_fall_back_when_no_theme_is_set() {
        let options = DiffViewerOptions::new(DiffViewerKind::Patch);
        let worker = diff_viewer_worker_options(&options, "fallback-l", "fallback-d");
        assert_eq!(worker.light_theme, "fallback-l");
        assert_eq!(worker.dark_theme, "fallback-d");
    }

    #[test]
    fn component_is_constructible() {
        let _ = DiffViewer as fn(DiffViewerProps) -> Element;
    }
}
