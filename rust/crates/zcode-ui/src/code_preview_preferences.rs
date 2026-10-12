//! `code_preview_preferences` —— Rust 移植自
//! `packages/ui/src/lib/codePreviewPreferences.ts`
//!
//! 代码预览的主题偏好：可选主题清单 + 「这个主题是不是深色」。
//!
//! ## 关键移植点：明暗语义靠**白名单**，不靠主题名正则
//!
//! 见 TS 里的原注释：`github-light-high-contrast` 与 `catppuccin-latte` 这种
//! 名字里既没有 `light` 也没有 `dark` 的主题，正则猜不出来。所以必须跟着
//! 设置页**显式支持**的选项走 —— 新增主题时两处一起改。
//!
//! 用白名单的另一个好处：`github-dark` 之类的名字恰好含有 `dark`，
//! 一旦哪天出现 `solarized-dark` 这种「名字含 dark 但实际是浅色背景」的
//! 变体，正则方案会静默出错。
//!
//! ## 与 TS 的差异
//!
//! **主题标识用 `&'static str` 而不是 `BundledTheme` 联合类型**：
//! shiki 的主题联合有上百个成员，Rust 侧没有必要穷举；
//! 本模块只维护「设置页真正暴露的那 10 个」+ 5 个深色判定，
//! 其余主题名一律按浅色处理（与 `includes` 的行为一致）。

/// 一个可选主题（`value` 是 shiki 主题名，`label` 是设置页展示名）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodePreviewThemeOption {
    pub value: &'static str,
    pub label: &'static str,
}

/// 设置页暴露的主题清单（顺序与 TS 一致）。
pub const CODE_PREVIEW_THEME_OPTIONS: [CodePreviewThemeOption; 10] = [
    CodePreviewThemeOption {
        value: "github-light",
        label: "GitHub Light",
    },
    CodePreviewThemeOption {
        value: "github-dark",
        label: "GitHub Dark",
    },
    CodePreviewThemeOption {
        value: "vitesse-light",
        label: "Vitesse Light",
    },
    CodePreviewThemeOption {
        value: "vitesse-dark",
        label: "Vitesse Dark",
    },
    CodePreviewThemeOption {
        value: "min-light",
        label: "Minimal Light",
    },
    CodePreviewThemeOption {
        value: "min-dark",
        label: "Minimal Dark",
    },
    CodePreviewThemeOption {
        value: "github-light-high-contrast",
        label: "GitHub HC Light",
    },
    CodePreviewThemeOption {
        value: "github-dark-high-contrast",
        label: "GitHub HC Dark",
    },
    CodePreviewThemeOption {
        value: "catppuccin-latte",
        label: "Catppuccin Latte",
    },
    CodePreviewThemeOption {
        value: "catppuccin-mocha",
        label: "Catppuccin Mocha",
    },
];

/// 深色主题白名单。
pub const DARK_CODE_PREVIEW_THEMES: [&str; 5] = [
    "github-dark",
    "vitesse-dark",
    "min-dark",
    "github-dark-high-contrast",
    "catppuccin-mocha",
];

/// `isDarkCodePreviewTheme`。
///
/// 不在白名单里（含未知主题名）一律按浅色。
pub fn is_dark_code_preview_theme(theme: &str) -> bool {
    DARK_CODE_PREVIEW_THEMES.contains(&theme)
}

/// 设置页的预览代码片段。
pub const SETTINGS_PREVIEW_CODE: &str = r##"const themePreview: ThemeConfig = {
  surface: "sidebar",
  accent: "#339CFF",
  contrast: 45,
};"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_option_list_matches_the_ts_order() {
        let values: Vec<&str> = CODE_PREVIEW_THEME_OPTIONS
            .iter()
            .map(|option| option.value)
            .collect();
        assert_eq!(
            values,
            vec![
                "github-light",
                "github-dark",
                "vitesse-light",
                "vitesse-dark",
                "min-light",
                "min-dark",
                "github-light-high-contrast",
                "github-dark-high-contrast",
                "catppuccin-latte",
                "catppuccin-mocha",
            ]
        );
        assert_eq!(CODE_PREVIEW_THEME_OPTIONS[0].label, "GitHub Light");
        assert_eq!(CODE_PREVIEW_THEME_OPTIONS[6].label, "GitHub HC Light");
        assert_eq!(CODE_PREVIEW_THEME_OPTIONS[9].label, "Catppuccin Mocha");
    }

    #[test]
    fn every_dark_theme_is_an_offered_option() {
        for theme in DARK_CODE_PREVIEW_THEMES {
            assert!(
                CODE_PREVIEW_THEME_OPTIONS
                    .iter()
                    .any(|option| option.value == theme),
                "{theme} 不在设置页选项里"
            );
        }
    }

    #[test]
    fn the_dark_set_is_exactly_the_five_ts_entries() {
        let dark: Vec<&str> = CODE_PREVIEW_THEME_OPTIONS
            .iter()
            .filter(|option| is_dark_code_preview_theme(option.value))
            .map(|option| option.value)
            .collect();
        assert_eq!(
            dark,
            vec![
                "github-dark",
                "vitesse-dark",
                "min-dark",
                "github-dark-high-contrast",
                "catppuccin-mocha",
            ]
        );
    }

    /// 白名单而不是正则：这两个名字里既没有 `light` 也没有 `dark`。
    #[test]
    fn names_without_a_light_or_dark_hint_are_still_classified() {
        assert!(is_dark_code_preview_theme("catppuccin-mocha"));
        assert!(!is_dark_code_preview_theme("catppuccin-latte"));
        assert!(!is_dark_code_preview_theme("github-light-high-contrast"));
    }

    #[test]
    fn an_unknown_theme_is_treated_as_light() {
        assert!(!is_dark_code_preview_theme(""));
        assert!(!is_dark_code_preview_theme("solarized-dark"));
        assert!(!is_dark_code_preview_theme("dracula"));
    }

    #[test]
    fn the_preview_snippet_is_verbatim() {
        assert!(SETTINGS_PREVIEW_CODE.starts_with("const themePreview: ThemeConfig = {"));
        assert!(SETTINGS_PREVIEW_CODE.contains("accent: \"#339CFF\","));
        assert!(SETTINGS_PREVIEW_CODE.ends_with("};"));
    }
}
