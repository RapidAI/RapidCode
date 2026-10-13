//! `FlipMetricValue` —— Rust 移植自 `packages/ui/src/components/ui/flip-metric-value.tsx`
//!
//! TS 用 `motion/react` 的 `AnimatePresence` + `motion.span` 给每个数字做
//! 「绕 X 轴翻进来」的动画。Rust 侧没有动画库，改用**按字符 key 重建元素 +
//! CSS 关键帧**：字符一变，旧元素卸载、新元素挂载并播放一次进入动画。
//!
//! ## 关键移植点 1：槽位宽度表
//!
//! 每个字符占一个固定宽度的槽（数字 `0.66em`、`:` / `.` `0.34em`、其余 `0.7em`），
//! 这样数字滚动时整体宽度不抖。TS 注释点名了原因：早先用 baseline + 负偏移，
//! 冒号和 K/M 单位走静态 inline，混排时**视觉中线不一致**；现在所有字符统一
//! `h-[1.15em]` 并居中，翻页只发生在槽内。
//!
//! ## 关键移植点 2：只有数字槽会翻转
//!
//! `!isDigitCharacter(character) || reducedMotion` → 走**静态**槽（没有
//! `overflow-hidden` / `[perspective:8em]`）。所以 `:`、`.`、`K`、`M` 永远不翻，
//! 只有 `0-9` 翻。`reduced_motion` 为真时连数字也不翻（无障碍）。
//!
//! ## 与 TS 的差异
//!
//! 1. **没有退出动画**：`AnimatePresence` 会同时保留旧字符播放 exit（`rotateX: 90`
//!    往下翻出）。没有动画库时保留两份节点代价太高，这里改为新字符直接替换旧字符
//!    并播进入动画 —— 在 `overflow-hidden` 的槽里，滚动计数器的观感与 TS 基本一致。
//! 2. **`animate_initial` 由 Rust 侧自己实现**：TS 用 `AnimatePresence initial={false}`
//!    抑制首帧动画。这里用「是否已渲染过一次」的信号达到同样效果（见 [`FlipMetricValue`]）。
//! 3. **`prefers-reduced-motion` 改为显式 prop**：TS 内部 `matchMedia` 查询，
//!    Rust 侧无法在无渲染器环境读取，因此由调用方传入（`reduced_motion`）。
//! 4. `role="text"` 不是合法的 ARIA role，TS 里就有；这里**原样保留**以免偏离
//!    上游，同时补上 `aria-label` / `title`（TS 也有）。

use crate::cn::cn;
use dioxus::prelude::*;

/// 翻转动画时长（毫秒），对应 TS `FLIP_TRANSITION.duration = 0.16`。
pub const FLIP_TRANSITION_DURATION_MS: f64 = 160.0;

/// 翻转动画缓动，对应 TS `FLIP_TRANSITION.ease = [0.4, 0, 0.2, 1]`。
pub const FLIP_TRANSITION_EASE: &str = "cubic-bezier(0.4, 0, 0.2, 1)";

/// `FlipMetricValue` 根节点基础类名（TS 原文，逐字保留）。
pub const FLIP_METRIC_VALUE_BASE: &str = "inline-flex max-w-full items-center overflow-hidden whitespace-nowrap align-middle leading-none";

/// 所有字符槽共用的类名（TS 里两处都有这一段）。
pub const METRIC_SLOT_SHARED: &str =
    "inline-flex h-[1.15em] shrink-0 items-center justify-center leading-none";

/// 数字槽独有的类名（多出 `relative` / `overflow-hidden` / 透视）。
pub const METRIC_DIGIT_SLOT_EXTRA: &str =
    "relative overflow-hidden [perspective:8em]";

/// 数字槽内层「翻页层」的类名（TS 原文，逐字保留）。
pub const METRIC_DIGIT_INNER: &str = "absolute inset-0 flex items-center justify-center leading-none";

/// 进入动画类名。由 `rust/.tailwind-build/build.sh` 里的 `@utility` 提供
/// （关键帧参数逐字对应 TS 的 `initial` / `animate` / `transition`）。
pub const METRIC_FLIP_IN: &str = "zcode-flip-metric-in";

/// 判断字符是否是 `0-9`（TS `isDigitCharacter`，用 `/^[0-9]$/`）。
///
/// 注意只认 ASCII 数字 —— 全角 `０` 不匹配 TS 的正则，这里保持一致。
pub fn is_digit_character(character: char) -> bool {
    character.is_ascii_digit()
}

/// 字符槽的宽度类名（逐字移植 TS `getCharacterWidthClass`）。
pub fn character_width_class(character: char) -> &'static str {
    if is_digit_character(character) {
        "w-[0.66em]"
    } else if character == ':' || character == '.' {
        "w-[0.34em]"
    } else {
        "w-[0.7em]"
    }
}

/// 该字符是否走「会翻转」的数字槽。
///
/// TS：`!isDigitCharacter(character) || reducedMotion` → 静态槽。
pub fn uses_flip_slot(character: char, reduced_motion: bool) -> bool {
    is_digit_character(character) && !reduced_motion
}

/// 单个字符槽的类名。
pub fn metric_slot_classes(character: char, reduced_motion: bool) -> String {
    let width = character_width_class(character);
    if uses_flip_slot(character, reduced_motion) {
        cn(&[METRIC_SLOT_SHARED, METRIC_DIGIT_SLOT_EXTRA, width])
    } else {
        cn(&[METRIC_SLOT_SHARED, width])
    }
}

/// 根节点类名。
pub fn flip_metric_value_classes(class: &str) -> String {
    cn(&[FLIP_METRIC_VALUE_BASE, class])
}

/// 把值拆成字符序列。
///
/// TS 用 `Array.from(value)` —— 它按 **Unicode 码点**拆，所以 emoji / 中文
/// 不会被劈成两半。Rust 的 `chars()` 语义相同。
pub fn metric_characters(value: &str) -> Vec<char> {
    value.chars().collect()
}

/// 单个字符槽。数字槽会翻转，其余静态。
#[component]
fn MetricCharacter(
    character: char,
    /// 槽位序号（TS 用它 + 字符本身构成 `AnimatePresence` 的 key）。
    index: usize,
    /// 无障碍：为真时所有槽都走静态路径。
    #[props(default)] reduced_motion: bool,
    /// 首帧是否播放动画（对应 `AnimatePresence initial={animateInitial}`）。
    #[props(default)] animate_initial: bool,
) -> Element {
    // 首帧是否要抑制动画（对应 `AnimatePresence initial={animateInitial}`）。
    //
    // 用 `use_hook` + `Cell` 而不是 `Signal`：这个标记只影响本组件自己的渲染，
    // 不需要反应式订阅，写 `Signal` 反而会多触发一次重渲染。
    // `use_hook` 的闭包只在首次渲染执行，之后每次拿到的是同一个 `Cell`。
    let first_render = use_hook(|| std::cell::Cell::new(true));
    let is_first_render = first_render.get();
    first_render.set(false);

    let cls = metric_slot_classes(character, reduced_motion);

    if !uses_flip_slot(character, reduced_motion) {
        return rsx! {
            span { class: cls, "aria-hidden": "true", {character.to_string()} }
        };
    }

    // 首帧且未要求首帧动画 → 不挂动画类（对应 `initial={false}`）
    let animate = animate_initial || !is_first_render;

    rsx! {
        span {
            class: cls,
            "aria-hidden": "true",
            span {
                // key 里带字符：字符一变就重建元素，动画随之重播。
                // 对应 TS 的 `key={`{index}-{character}`}`。
                key: "{index}-{character}",
                class: if animate {
                    cn(&[METRIC_DIGIT_INNER, METRIC_FLIP_IN])
                } else {
                    METRIC_DIGIT_INNER.to_string()
                },
                {character.to_string()}
            }
        }
    }
}

/// 等宽槽位排列的指标数值。
///
/// 每个字符占一个固定宽度的槽，数字变化时在槽内翻转，整体宽度不抖。
#[component]
pub fn FlipMetricValue(
    /// 要显示的文本（通常是已经格式化好的数字串）。
    #[props(default, into)] value: String,
    /// 无障碍：为真时不播放翻转动画（对应 TS 内部的 `prefers-reduced-motion`）。
    #[props(default)] reduced_motion: bool,
    /// 首帧是否也播放动画（TS 默认 `false`）。
    #[props(default)] animate_initial: bool,
    #[props(default, into)] class: String,
) -> Element {
    let characters = metric_characters(&value);

    rsx! {
        span {
            class: flip_metric_value_classes(&class),
            "data-slot": "flip-metric-value",
            // TS 是 `animateInitial ? "true" : undefined`：false 时属性整个不出现
            "data-animate-initial": animate_initial.then_some("true"),
            "aria-label": value.clone(),
            title: value,
            for (index, character) in characters.into_iter().enumerate() {
                MetricCharacter {
                    key: "{index}",
                    character,
                    index,
                    reduced_motion,
                    animate_initial,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_ascii_digits_are_digits() {
        for c in '0'..='9' {
            assert!(is_digit_character(c), "{c} 应被认作数字");
        }
        assert!(!is_digit_character('a'));
        assert!(!is_digit_character('K'));
        assert!(!is_digit_character(':'));
        assert!(!is_digit_character('.'));
        assert!(!is_digit_character(' '));
        assert!(!is_digit_character('-'));
        // TS 的正则 `/^[0-9]$/` 不匹配全角数字，这里保持一致
        assert!(!is_digit_character('０'));
        assert!(!is_digit_character('一'));
    }

    #[test]
    fn width_classes_match_the_three_buckets() {
        assert_eq!(character_width_class('7'), "w-[0.66em]");
        assert_eq!(character_width_class(':'), "w-[0.34em]");
        assert_eq!(character_width_class('.'), "w-[0.34em]");
        assert_eq!(character_width_class('K'), "w-[0.7em]");
        assert_eq!(character_width_class('M'), "w-[0.7em]");
        assert_eq!(character_width_class('%'), "w-[0.7em]");
        assert_eq!(character_width_class(' '), "w-[0.7em]");
    }

    /// 只有数字槽会翻转；`reduced_motion` 时连数字也不翻。
    #[test]
    fn only_digit_slots_flip() {
        assert!(uses_flip_slot('5', false));
        assert!(!uses_flip_slot('5', true));
        assert!(!uses_flip_slot(':', false));
        assert!(!uses_flip_slot('.', false));
        assert!(!uses_flip_slot('K', false));
        assert!(!uses_flip_slot('K', true));
    }

    /// 数字槽多出 `relative` / `overflow-hidden` / 透视；静态槽没有。
    #[test]
    fn digit_slot_adds_overflow_and_perspective() {
        let digit = metric_slot_classes('5', false);
        assert!(digit.contains("relative"));
        assert!(digit.contains("overflow-hidden"));
        assert!(digit.contains("[perspective:8em]"));
        assert!(digit.contains("w-[0.66em]"));

        let static_slot = metric_slot_classes('5', true);
        assert!(!static_slot.contains("overflow-hidden"));
        assert!(!static_slot.contains("[perspective:8em]"));
        assert!(!static_slot.contains("relative"));
        // 宽度类名仍然保留
        assert!(static_slot.contains("w-[0.66em]"));
    }

    /// TS 注释点名的坑：所有槽必须统一高度并居中，否则混排时中线不一致。
    #[test]
    fn every_slot_shares_height_and_centering() {
        for c in ['0', ':', '.', 'K', '%', ' '] {
            for reduced in [false, true] {
                let cls = metric_slot_classes(c, reduced);
                assert!(
                    cls.contains("h-[1.15em]"),
                    "{c} (reduced={reduced}) 缺固定高度：{cls}"
                );
                assert!(cls.contains("items-center"), "{c} 缺垂直居中：{cls}");
                assert!(cls.contains("justify-center"), "{c} 缺水平居中：{cls}");
                assert!(cls.contains("leading-none"), "{c} 缺 leading-none：{cls}");
                assert!(cls.contains("shrink-0"), "{c} 缺 shrink-0：{cls}");
            }
        }
    }

    #[test]
    fn root_classes() {
        let c = flip_metric_value_classes("");
        assert!(c.contains("inline-flex"));
        assert!(c.contains("max-w-full"));
        assert!(c.contains("overflow-hidden"));
        assert!(c.contains("whitespace-nowrap"));
        assert!(c.contains("align-middle"));
        assert!(c.contains("leading-none"));
    }

    #[test]
    fn user_class_wins_on_root() {
        let c = flip_metric_value_classes("text-2xl");
        assert!(c.contains("text-2xl"));
        assert!(c.contains("inline-flex"));
    }

    #[test]
    fn transition_matches_ts() {
        assert_eq!(FLIP_TRANSITION_DURATION_MS, 160.0);
        assert_eq!(FLIP_TRANSITION_EASE, "cubic-bezier(0.4, 0, 0.2, 1)");
        // 动画类名要与 build.sh 里 @utility 定义的名字一致
        assert_eq!(METRIC_FLIP_IN, "zcode-flip-metric-in");
    }

    /// `Array.from(value)` 按码点拆 —— emoji 不能被劈成两半。
    #[test]
    fn characters_split_by_code_point() {
        assert_eq!(metric_characters("12.5K"), vec!['1', '2', '.', '5', 'K']);
        assert_eq!(metric_characters(""), Vec::<char>::new());
        assert_eq!(metric_characters("中"), vec!['中']);
        // 一个 emoji 是单个 char（U+1F600），不是两个 UTF-16 码元
        assert_eq!(metric_characters("😀"), vec!['😀']);
        assert_eq!(metric_characters("1😀2").len(), 3);
    }
}
