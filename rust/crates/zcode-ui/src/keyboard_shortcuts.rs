//! `keyboard_shortcuts` —— Rust 移植自 `packages/ui/src/lib/keyboardShortcuts.ts`
//!
//! 快捷键的**匹配**与**显示标签**。两件事都很容易被做错，这里逐条复刻：
//!
//! ## 关键移植点 1：主快捷键按平台隔离
//!
//! macOS 的 `Ctrl` 保留给系统 Emacs 风格文本编辑，只有 Windows/Linux 才用 `Ctrl`。
//! 同时按下 `Ctrl` 与 `Command` **不算**主快捷键（避免误触发）。
//!
//! ## 关键移植点 2：`event.key` 会被键盘布局改写
//!
//! macOS 上按下 Option 参与组合键时，`event.key` 可能变成别的字符，
//! 直接按 `key` 比较会把 `⌥⌘B` 这类快捷键误判成未命中。所以匹配时补一层
//! `event.code` 兜底（[`get_expected_shortcut_code`]）—— 它不受输入法/布局影响。
//!
//! ## 关键移植点 3：三处大小写口径不一致
//!
//! 上游在拼标签时混用了「原样大写」和「`formatShortcutKeyLabel`」：
//!
//! | 函数 | 非 Apple 分支用的是 |
//! |---|---|
//! | `formatCommandShortcutLabel` | `key.toUpperCase()` |
//! | `formatAltCommandShortcutLabel` | `key.toUpperCase()` |
//! | `formatShiftCommandShortcutLabel` | `formatShortcutKeyLabel(key)` |
//!
//! `formatShortcutKeyLabel` 对 `[` / `]` 不做事（其它一律大写），所以对普通字母
//! 两者等价，只有 `[` / `]` 之外的非字母键才会看出差别。本移植原样保留这个不一致，
//! 并用测试钉住。

/// 平台信息（TS `KeyboardShortcutPlatformInfo`）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyboardShortcutPlatformInfo {
    pub platform: Option<String>,
    pub user_agent: Option<String>,
}

/// 一个按键事件（TS `PrimaryShortcutKeyboardEvent`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutKeyboardEvent {
    pub key: String,
    pub code: Option<String>,
    pub meta_key: bool,
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
}

impl ShortcutKeyboardEvent {
    /// 只给 `key` 的构造器，修饰键全为假。
    pub fn from_key(key: &str) -> Self {
        Self {
            key: key.to_string(),
            code: None,
            meta_key: false,
            ctrl_key: false,
            shift_key: false,
            alt_key: false,
        }
    }
}

/// 是否是 Apple 键盘平台（TS `isAppleKeyboardPlatform`）。
///
/// `platform` 小写后含 `mac` / `iphone` / `ipad` / `ipod`，
/// 或者 `userAgent` 匹配 `/Mac|iPhone|iPad|iPod/`（**大小写敏感**）。
pub fn is_apple_keyboard_platform(platform_info: &KeyboardShortcutPlatformInfo) -> bool {
    let platform = platform_info
        .platform
        .as_deref()
        .unwrap_or("")
        .to_lowercase();
    let user_agent = platform_info.user_agent.as_deref().unwrap_or("");

    if platform.contains("mac")
        || platform.contains("iphone")
        || platform.contains("ipad")
        || platform.contains("ipod")
    {
        return true;
    }

    // 注意：这一路是大小写敏感的（TS 用的是不带 `i` 标志的正则）
    ["Mac", "iPhone", "iPad", "iPod"]
        .iter()
        .any(|needle| user_agent.contains(needle))
}

fn get_command_modifier_label(platform_info: &KeyboardShortcutPlatformInfo) -> &'static str {
    if is_apple_keyboard_platform(platform_info) {
        "⌘"
    } else {
        "Ctrl"
    }
}

/// 键的显示标签（TS `formatShortcutKeyLabel`）。
///
/// `[` / `]` 原样，其余转大写。
pub fn format_shortcut_key_label(key: &str) -> String {
    match key {
        "[" => "[".to_string(),
        "]" => "]".to_string(),
        other => other.to_uppercase(),
    }
}

/// `⌘ B`（Apple）/ `Ctrl+B`（其它）。
///
/// 注意非 Apple 分支用的是 `key.toUpperCase()` 而不是 [`format_shortcut_key_label`]
/// —— 见模块文档关键移植点 3。
pub fn format_command_shortcut_label(
    key: &str,
    platform_info: &KeyboardShortcutPlatformInfo,
) -> String {
    let modifier = get_command_modifier_label(platform_info);
    if is_apple_keyboard_platform(platform_info) {
        return format!("{modifier} {}", format_shortcut_key_label(key));
    }
    format!("{modifier}+{}", key.to_uppercase())
}

/// `⌥ ⌘ B`（Apple）/ `Ctrl+Alt+B`（其它）。
pub fn format_alt_command_shortcut_label(
    key: &str,
    platform_info: &KeyboardShortcutPlatformInfo,
) -> String {
    let is_apple = is_apple_keyboard_platform(platform_info);
    let command = get_command_modifier_label(platform_info);
    if is_apple {
        return format!("⌥ {command} {}", format_shortcut_key_label(key));
    }
    format!("Ctrl+Alt+{}", key.to_uppercase())
}

/// `⇧ ⌘ B`（Apple）/ `Ctrl+Shift+B`（其它）。
pub fn format_shift_command_shortcut_label(
    key: &str,
    platform_info: &KeyboardShortcutPlatformInfo,
) -> String {
    let is_apple = is_apple_keyboard_platform(platform_info);
    let command = get_command_modifier_label(platform_info);
    let display_key = format_shortcut_key_label(key);
    if is_apple {
        return format!("⇧ {command} {display_key}");
    }
    format!("Ctrl+Shift+{display_key}")
}

/// `⇧ Tab`（Apple）/ `Shift+Tab`（其它）。
pub fn format_shift_tab_shortcut_label(
    platform_info: &KeyboardShortcutPlatformInfo,
) -> String {
    if is_apple_keyboard_platform(platform_info) {
        "⇧ Tab".to_string()
    } else {
        "Shift+Tab".to_string()
    }
}

/// 按键的 `event.code` 期望值（TS `getExpectedShortcutCode`）。
pub fn get_expected_shortcut_code(key: &str) -> Option<String> {
    if key.chars().count() == 1 {
        let lower = key.to_lowercase();
        if lower.len() == 1 {
            let character = lower.chars().next()?;
            if character.is_ascii_lowercase() {
                return Some(format!("Key{}", character.to_ascii_uppercase()));
            }
            if character.is_ascii_digit() {
                return Some(format!("Digit{character}"));
            }
        }
    }

    match key {
        "[" => Some("BracketLeft".to_string()),
        "]" => Some("BracketRight".to_string()),
        _ => None,
    }
}

/// 键是否匹配（TS `matchesShortcutKey`）。
///
/// 先比 `key`（都小写化），不中再看 `code` —— 后者不受键盘布局影响。
pub fn matches_shortcut_key(event: &ShortcutKeyboardEvent, key: &str) -> bool {
    let normalized_key = key.to_lowercase();
    if event.key.to_lowercase() == normalized_key {
        return true;
    }

    match get_expected_shortcut_code(&normalized_key) {
        Some(expected_code) => event.code.as_deref() == Some(expected_code.as_str()),
        None => false,
    }
}

/// 主修饰键是否按下（TS `matchesPrimaryModifier`）。
///
/// Apple → `meta && !ctrl`；其它 → `ctrl && !meta`。两者同时按下都不算。
pub fn matches_primary_modifier(
    event: &ShortcutKeyboardEvent,
    platform_info: &KeyboardShortcutPlatformInfo,
) -> bool {
    if is_apple_keyboard_platform(platform_info) {
        event.meta_key && !event.ctrl_key
    } else {
        event.ctrl_key && !event.meta_key
    }
}

/// `⌘/Ctrl + <key>`（不带 Shift / Alt）。
pub fn matches_primary_shortcut(
    event: &ShortcutKeyboardEvent,
    key: &str,
    platform_info: &KeyboardShortcutPlatformInfo,
) -> bool {
    matches_primary_modifier(event, platform_info)
        && !event.shift_key
        && !event.alt_key
        && matches_shortcut_key(event, key)
}

/// `⌘/Ctrl + Shift + <key>`。
pub fn matches_primary_shift_shortcut(
    event: &ShortcutKeyboardEvent,
    key: &str,
    platform_info: &KeyboardShortcutPlatformInfo,
) -> bool {
    matches_primary_modifier(event, platform_info)
        && event.shift_key
        && !event.alt_key
        && matches_shortcut_key(event, key)
}

/// `⌘/Ctrl + Alt + <key>`。
pub fn matches_primary_alt_shortcut(
    event: &ShortcutKeyboardEvent,
    key: &str,
    platform_info: &KeyboardShortcutPlatformInfo,
) -> bool {
    matches_primary_modifier(event, platform_info)
        && !event.shift_key
        && event.alt_key
        && matches_shortcut_key(event, key)
}

/// `Ctrl + <key>`（**任何平台**都认 Ctrl，且要求没按 Meta）。
pub fn matches_ctrl_shortcut(event: &ShortcutKeyboardEvent, key: &str) -> bool {
    event.ctrl_key
        && !event.meta_key
        && !event.shift_key
        && !event.alt_key
        && matches_shortcut_key(event, key)
}

/// `Ctrl + Shift + <key>`。
pub fn matches_ctrl_shift_shortcut(event: &ShortcutKeyboardEvent, key: &str) -> bool {
    event.ctrl_key
        && !event.meta_key
        && event.shift_key
        && !event.alt_key
        && matches_shortcut_key(event, key)
}

/// 纯 `Shift+Tab`（不含 Ctrl / Cmd / Alt）。
///
/// 历史上聊天工具栏 mode 曾用这个组合键；保留给仍需要区分反向 Tab 导航的调用方。
pub fn matches_shift_tab(event: &ShortcutKeyboardEvent) -> bool {
    event.shift_key
        && !event.ctrl_key
        && !event.meta_key
        && !event.alt_key
        && (event.key == "Tab" || event.code.as_deref() == Some("Tab"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mac() -> KeyboardShortcutPlatformInfo {
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

    // ---- 平台判定 ----

    #[test]
    fn apple_platforms_are_detected_from_the_platform_field() {
        for platform in ["MacIntel", "MAC", "iPhone", "iPad", "iPod", "mac"] {
            let info = KeyboardShortcutPlatformInfo {
                platform: Some(platform.to_string()),
                user_agent: None,
            };
            assert!(is_apple_keyboard_platform(&info), "{platform}");
        }
        assert!(!is_apple_keyboard_platform(&windows()));
        assert!(!is_apple_keyboard_platform(&KeyboardShortcutPlatformInfo::default()));
    }

    #[test]
    fn apple_platforms_are_detected_from_the_user_agent() {
        let info = KeyboardShortcutPlatformInfo {
            platform: None,
            user_agent: Some("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)".to_string()),
        };
        assert!(is_apple_keyboard_platform(&info));

        let iphone = KeyboardShortcutPlatformInfo {
            platform: None,
            user_agent: Some("Mozilla/5.0 (iPhone; CPU iPhone OS 17_0)".to_string()),
        };
        assert!(is_apple_keyboard_platform(&iphone));
    }

    /// userAgent 那一路是**大小写敏感**的（上游正则没有 `i` 标志）。
    #[test]
    fn the_user_agent_check_is_case_sensitive() {
        let lower = KeyboardShortcutPlatformInfo {
            platform: None,
            user_agent: Some("mozilla/5.0 (macintosh)".to_string()),
        };
        // platform 是空串 → 两路都不中
        assert!(!is_apple_keyboard_platform(&lower));
    }

    // ---- 标签 ----

    #[test]
    fn shortcut_key_labels() {
        assert_eq!(format_shortcut_key_label("b"), "B");
        assert_eq!(format_shortcut_key_label("B"), "B");
        // `[` / `]` 不转大写
        assert_eq!(format_shortcut_key_label("["), "[");
        assert_eq!(format_shortcut_key_label("]"), "]");
        assert_eq!(format_shortcut_key_label("Enter"), "ENTER");
    }

    #[test]
    fn command_labels_per_platform() {
        assert_eq!(format_command_shortcut_label("b", &mac()), "⌘ B");
        assert_eq!(format_command_shortcut_label("b", &windows()), "Ctrl+B");
        assert_eq!(format_command_shortcut_label("B", &windows()), "Ctrl+B");
    }

    #[test]
    fn alt_command_labels_per_platform() {
        assert_eq!(format_alt_command_shortcut_label("b", &mac()), "⌥ ⌘ B");
        assert_eq!(
            format_alt_command_shortcut_label("b", &windows()),
            "Ctrl+Alt+B"
        );
    }

    #[test]
    fn shift_command_labels_per_platform() {
        assert_eq!(format_shift_command_shortcut_label("b", &mac()), "⇧ ⌘ B");
        assert_eq!(
            format_shift_command_shortcut_label("b", &windows()),
            "Ctrl+Shift+B"
        );
        // `[` 在 Apple 侧走 `formatShortcutKeyLabel`（保持 `[`）
        assert_eq!(format_shift_command_shortcut_label("[", &mac()), "⇧ ⌘ [");
        assert_eq!(
            format_shift_command_shortcut_label("[", &windows()),
            "Ctrl+Shift+["
        );
    }

    #[test]
    fn shift_tab_labels_per_platform() {
        assert_eq!(format_shift_tab_shortcut_label(&mac()), "⇧ Tab");
        assert_eq!(format_shift_tab_shortcut_label(&windows()), "Shift+Tab");
    }

    // ---- code 兜底 ----

    #[test]
    fn expected_codes_for_letters_digits_and_brackets() {
        assert_eq!(get_expected_shortcut_code("b").as_deref(), Some("KeyB"));
        assert_eq!(get_expected_shortcut_code("B").as_deref(), Some("KeyB"));
        assert_eq!(get_expected_shortcut_code("7").as_deref(), Some("Digit7"));
        assert_eq!(get_expected_shortcut_code("[").as_deref(), Some("BracketLeft"));
        assert_eq!(get_expected_shortcut_code("]").as_deref(), Some("BracketRight"));
        // 其它多字符键没有 code 映射
        assert_eq!(get_expected_shortcut_code("Enter"), None);
        assert_eq!(get_expected_shortcut_code("/"), None);
        assert_eq!(get_expected_shortcut_code(""), None);
    }

    #[test]
    fn key_matching_is_case_insensitive() {
        let event = ShortcutKeyboardEvent::from_key("B");
        assert!(matches_shortcut_key(&event, "b"));
        assert!(matches_shortcut_key(&event, "B"));
        assert!(!matches_shortcut_key(&event, "c"));
    }

    /// macOS 上 Option 会把 `event.key` 改写成别的字符 —— `code` 兜底要救回来。
    #[test]
    fn the_code_fallback_survives_a_rewritten_key() {
        let event = ShortcutKeyboardEvent {
            // 键盘布局把 `b` 改写成了 `∫`
            key: "∫".to_string(),
            code: Some("KeyB".to_string()),
            meta_key: true,
            ctrl_key: false,
            shift_key: false,
            alt_key: true,
        };
        assert!(matches_shortcut_key(&event, "b"));
        assert!(matches_primary_alt_shortcut(&event, "b", &mac()));
    }

    #[test]
    fn a_wrong_code_does_not_match() {
        let event = ShortcutKeyboardEvent {
            key: "∫".to_string(),
            code: Some("KeyC".to_string()),
            ..ShortcutKeyboardEvent::from_key("∫")
        };
        assert!(!matches_shortcut_key(&event, "b"));
    }

    // ---- 主修饰键 ----

    #[test]
    fn the_primary_modifier_is_platform_specific() {
        let command = ShortcutKeyboardEvent {
            meta_key: true,
            ..ShortcutKeyboardEvent::from_key("b")
        };
        let ctrl = ShortcutKeyboardEvent {
            ctrl_key: true,
            ..ShortcutKeyboardEvent::from_key("b")
        };

        // macOS 认 Command，不认 Ctrl
        assert!(matches_primary_modifier(&command, &mac()));
        assert!(!matches_primary_modifier(&ctrl, &mac()));
        // Windows 认 Ctrl，不认 Command
        assert!(matches_primary_modifier(&ctrl, &windows()));
        assert!(!matches_primary_modifier(&command, &windows()));
    }

    /// 两个都按下不视作主快捷键（避免误触发）。
    #[test]
    fn both_modifiers_together_do_not_count() {
        let both = ShortcutKeyboardEvent {
            meta_key: true,
            ctrl_key: true,
            ..ShortcutKeyboardEvent::from_key("b")
        };
        assert!(!matches_primary_modifier(&both, &mac()));
        assert!(!matches_primary_modifier(&both, &windows()));
    }

    #[test]
    fn no_modifier_does_not_count() {
        let bare = ShortcutKeyboardEvent::from_key("b");
        assert!(!matches_primary_modifier(&bare, &mac()));
        assert!(!matches_primary_modifier(&bare, &windows()));
    }

    // ---- 组合匹配 ----

    #[test]
    fn primary_shortcut_rejects_shift_and_alt() {
        let base = ShortcutKeyboardEvent {
            meta_key: true,
            ..ShortcutKeyboardEvent::from_key("b")
        };
        assert!(matches_primary_shortcut(&base, "b", &mac()));

        for extra in [
            ShortcutKeyboardEvent {
                shift_key: true,
                ..base.clone()
            },
            ShortcutKeyboardEvent {
                alt_key: true,
                ..base.clone()
            },
        ] {
            assert!(!matches_primary_shortcut(&extra, "b", &mac()));
        }
    }

    #[test]
    fn primary_shift_shortcut_requires_shift_but_not_alt() {
        let event = ShortcutKeyboardEvent {
            meta_key: true,
            shift_key: true,
            ..ShortcutKeyboardEvent::from_key("b")
        };
        assert!(matches_primary_shift_shortcut(&event, "b", &mac()));
        assert!(!matches_primary_shortcut(&event, "b", &mac()));

        let with_alt = ShortcutKeyboardEvent {
            alt_key: true,
            ..event
        };
        assert!(!matches_primary_shift_shortcut(&with_alt, "b", &mac()));
    }

    #[test]
    fn primary_alt_shortcut_requires_alt_but_not_shift() {
        let event = ShortcutKeyboardEvent {
            meta_key: true,
            alt_key: true,
            ..ShortcutKeyboardEvent::from_key("b")
        };
        assert!(matches_primary_alt_shortcut(&event, "b", &mac()));

        let with_shift = ShortcutKeyboardEvent {
            shift_key: true,
            ..event
        };
        assert!(!matches_primary_alt_shortcut(&with_shift, "b", &mac()));
    }

    /// `matchesCtrlShortcut` 不看平台，任何平台都认 Ctrl（且要求没按 Meta）。
    #[test]
    fn ctrl_shortcuts_are_platform_agnostic() {
        let event = ShortcutKeyboardEvent {
            ctrl_key: true,
            ..ShortcutKeyboardEvent::from_key("b")
        };
        assert!(matches_ctrl_shortcut(&event, "b"));
        // 即使平台是 macOS 也照样匹配
        assert!(matches_ctrl_shortcut(&event, "b"));

        let with_meta = ShortcutKeyboardEvent {
            meta_key: true,
            ..event
        };
        assert!(!matches_ctrl_shortcut(&with_meta, "b"));
    }

    #[test]
    fn ctrl_shortcuts_reject_shift_and_alt() {
        let base = ShortcutKeyboardEvent {
            ctrl_key: true,
            ..ShortcutKeyboardEvent::from_key("b")
        };
        for extra in [
            ShortcutKeyboardEvent {
                shift_key: true,
                ..base.clone()
            },
            ShortcutKeyboardEvent {
                alt_key: true,
                ..base.clone()
            },
        ] {
            assert!(!matches_ctrl_shortcut(&extra, "b"));
        }

        let ctrl_shift = ShortcutKeyboardEvent {
            ctrl_key: true,
            shift_key: true,
            ..ShortcutKeyboardEvent::from_key("b")
        };
        assert!(matches_ctrl_shift_shortcut(&ctrl_shift, "b"));
        assert!(!matches_ctrl_shift_shortcut(&base, "b"));
    }

    // ---- Shift+Tab ----

    #[test]
    fn shift_tab_matches_by_key_or_code() {
        let by_key = ShortcutKeyboardEvent {
            shift_key: true,
            ..ShortcutKeyboardEvent::from_key("Tab")
        };
        assert!(matches_shift_tab(&by_key));

        // `key` 被改写，但 `code` 还是 Tab
        let by_code = ShortcutKeyboardEvent {
            key: "".to_string(),
            code: Some("Tab".to_string()),
            shift_key: true,
            ..ShortcutKeyboardEvent::from_key("")
        };
        assert!(matches_shift_tab(&by_code));
    }

    #[test]
    fn shift_tab_rejects_extra_modifiers() {
        for extra in [
            ShortcutKeyboardEvent {
                ctrl_key: true,
                ..ShortcutKeyboardEvent::from_key("Tab")
            },
            ShortcutKeyboardEvent {
                meta_key: true,
                ..ShortcutKeyboardEvent::from_key("Tab")
            },
            ShortcutKeyboardEvent {
                alt_key: true,
                ..ShortcutKeyboardEvent::from_key("Tab")
            },
        ] {
            let event = ShortcutKeyboardEvent {
                shift_key: true,
                ..extra
            };
            assert!(!matches_shift_tab(&event));
        }
        // 没按 Shift 也不是
        assert!(!matches_shift_tab(&ShortcutKeyboardEvent::from_key("Tab")));
    }

    /// 平台判定只影响主快捷键，不影响 `Ctrl` / `Shift+Tab` 这两族。
    #[test]
    fn the_platform_only_changes_the_primary_modifier() {
        let command = ShortcutKeyboardEvent {
            meta_key: true,
            ..ShortcutKeyboardEvent::from_key("b")
        };
        assert!(matches_primary_shortcut(&command, "b", &mac()));
        assert!(!matches_primary_shortcut(&command, "b", &windows()));
        // Ctrl 那族两边一样
        let ctrl = ShortcutKeyboardEvent {
            ctrl_key: true,
            ..ShortcutKeyboardEvent::from_key("b")
        };
        assert!(matches_ctrl_shortcut(&ctrl, "b"));
    }
}
