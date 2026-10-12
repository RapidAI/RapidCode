//! `cn` —— 类名合并工具
//!
//! Rust 移植自 `packages/ui/src/components/lib/utils.ts`：
//!
//! ```ts
//! const mergeUiClasses = extendTailwindMerge({
//!   extend: { classGroups: { "font-size": ["text-ui-*", ...] } },
//! });
//! export function cn(...inputs: ClassValue[]) {
//!   return mergeUiClasses(clsx(inputs));
//! }
//! ```
//!
//! 即：`clsx`（拼接 / 过滤空值）+ `tailwind-merge`（解决同类 Tailwind 类冲突，
//! 后者覆盖前者）。
//!
//! ## 移植说明
//!
//! 完整版 `tailwind-merge` 依赖一张巨大的类组配置表。这里实现了一个
//! **务实子集**：
//! - `clsx` 部分完全等价：按空白切分、过滤空串、保序拼接；
//! - 冲突消解按「修饰符前缀（如 `hover:` / `dark:` / `data-[...]:`）+
//!   类组」作为键，**保留最后一个**；
//! - 未识别的类不做任何合并（安全：宁可少合并，也不错合并）。
//!
//! 关键点与 TS 一致：`text-ui-*` 是**字号**而非文字颜色，因此显式单独成组，
//! 不会和 `text-foreground` 等颜色类互相覆盖。

/// 把一个 token 拆成（修饰符列表，基类）。
///
/// 修饰符是开头的 `xxx:` 段，但 `[` `]` 内的 `:` 不算分隔符
/// （如 `[&_svg:not([class*='size-'])]:size-4`）。
fn split_modifiers(token: &str) -> (Vec<&str>, &str) {
    let bytes = token.as_bytes();
    let mut depth = 0i32;
    let mut last_split = 0usize;
    let mut modifiers = Vec::new();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'[' => depth += 1,
            b']' => depth -= 1,
            b':' if depth <= 0 => {
                modifiers.push(&token[last_split..i]);
                last_split = i + 1;
            }
            _ => {}
        }
    }
    (modifiers, &token[last_split..])
}

/// 类组前缀表：`(前缀, 组名)`。用**最长前缀**匹配。
///
/// 注意：`border-*` / `ring-*` / `shadow-*` / `outline-*` / `divide-*` 这些
/// 「同前缀但语义不同」（宽度 vs 颜色）的族**不在这里**，由
/// [`size_family_group`] 单独判定，否则 `ring-2 ring-background` 会被误判成
/// 同组而丢掉 `ring-2`。
const GROUP_PREFIXES: &[(&str, &str)] = &[
    // ---- 字号（必须排在 text-* 颜色之前）----
    ("text-ui-", "font-size"),
    ("text-mobile-", "font-size"),
    // ---- 文本对齐 / 换行 / 溢出 ----
    ("text-left", "text-align"),
    ("text-center", "text-align"),
    ("text-right", "text-align"),
    ("text-justify", "text-align"),
    ("text-start", "text-align"),
    ("text-end", "text-align"),
    ("text-balance", "text-wrap"),
    ("text-pretty", "text-wrap"),
    ("text-nowrap", "text-wrap"),
    ("text-ellipsis", "text-overflow"),
    ("text-clip", "text-overflow"),
    // ---- 文字颜色 ----
    ("text-", "text-color"),
    // ---- 背景 ----
    ("bg-clip-", "bg-clip"),
    ("bg-origin-", "bg-origin"),
    ("bg-blend-", "bg-blend"),
    ("bg-gradient-", "bg-image"),
    ("bg-repeat", "bg-repeat"),
    ("bg-no-repeat", "bg-repeat"),
    ("bg-cover", "bg-size"),
    ("bg-contain", "bg-size"),
    ("bg-auto", "bg-size"),
    ("bg-center", "bg-position"),
    ("bg-top", "bg-position"),
    ("bg-bottom", "bg-position"),
    ("bg-left", "bg-position"),
    ("bg-right", "bg-position"),
    ("bg-", "bg-color"),
    // ---- 字体族 / 字重 ----
    ("font-sans", "font-family"),
    ("font-serif", "font-family"),
    ("font-mono", "font-family"),
    ("font-", "font-weight"),
    // ---- 圆角 ----
    ("rounded", "rounded"),
    // ---- 内边距 ----
    ("px-", "px"),
    ("py-", "py"),
    ("pt-", "pt"),
    ("pr-", "pr"),
    ("pb-", "pb"),
    ("pl-", "pl"),
    ("p-", "p"),
    // ---- 外边距 ----
    ("-mx-", "-mx"),
    ("-my-", "-my"),
    ("-mt-", "-mt"),
    ("-mb-", "-mb"),
    ("-ml-", "-ml"),
    ("-mr-", "-mr"),
    ("-m-", "-m"),
    ("mx-", "mx"),
    ("my-", "my"),
    ("mt-", "mt"),
    ("mr-", "mr"),
    ("mb-", "mb"),
    ("ml-", "ml"),
    ("m-", "m"),
    // ---- 尺寸 ----
    ("min-w-", "min-w"),
    ("max-w-", "max-w"),
    ("min-h-", "min-h"),
    ("max-h-", "max-h"),
    ("size-", "size"),
    ("w-", "w"),
    ("h-", "h"),
    // ---- 间距 ----
    ("gap-x-", "gap-x"),
    ("gap-y-", "gap-y"),
    ("gap-", "gap"),
    ("space-x-", "space-x"),
    ("space-y-", "space-y"),
    // ---- 定位 ----
    ("inset-x-", "inset-x"),
    ("inset-y-", "inset-y"),
    ("inset-", "inset"),
    ("top-", "top"),
    ("right-", "right"),
    ("bottom-", "bottom"),
    ("left-", "left"),
    ("z-", "z"),
    // ---- 布局 ----
    ("items-", "items"),
    ("justify-", "justify"),
    ("content-", "content"),
    ("self-", "self"),
    ("grid-cols-", "grid-cols"),
    ("grid-rows-", "grid-rows"),
    ("grid-", "grid"),
    ("col-start-", "col-start"),
    ("col-span-", "col-span"),
    ("col-", "col"),
    ("row-start-", "row-start"),
    ("row-span-", "row-span"),
    ("row-", "row"),
    ("flex-", "flex"),
    ("basis-", "basis"),
    ("order-", "order"),
    ("auto-cols-", "auto-cols"),
    ("auto-rows-", "auto-rows"),
    ("overflow-x-", "overflow-x"),
    ("overflow-y-", "overflow-y"),
    ("overflow-", "overflow"),
    ("place-items-", "place-items"),
    ("place-content-", "place-content"),
    ("place-self-", "place-self"),
    // ---- 排版 ----
    ("leading-", "leading"),
    ("tracking-", "tracking"),
    ("whitespace-", "whitespace"),
    ("underline-offset-", "underline-offset"),
    ("list-", "list"),
    ("object-", "object"),
    // ---- 视觉 ----
    ("opacity-", "opacity"),
    ("mix-blend-", "mix-blend"),
    ("backdrop-", "backdrop"),
    ("blur", "blur"),
    ("brightness-", "brightness"),
    ("contrast-", "contrast"),
    ("grayscale", "grayscale"),
    ("invert", "invert"),
    ("saturate-", "saturate"),
    ("sepia", "sepia"),
    // ---- 动画 / 过渡 ----
    ("transition-", "transition"),
    ("duration-", "duration"),
    ("delay-", "delay"),
    ("ease-", "ease"),
    ("animate-", "animate"),
    ("translate-x-", "translate-x"),
    ("translate-y-", "translate-y"),
    ("scale-", "scale"),
    ("rotate-", "rotate"),
    ("origin-", "origin"),
    // ---- 交互 ----
    ("cursor-", "cursor"),
    ("select-", "select"),
    ("pointer-events-", "pointer-events"),
    ("shrink", "shrink"),
    ("grow", "grow"),
    ("resize", "resize"),
    ("field-sizing-", "field-sizing"),
];

/// `shadow` 的尺寸关键字（`shadow-{关键字}` 是投影强度，不是颜色）。
const SHADOW_SIZE_KEYWORDS: &[&str] = &[
    "2xs", "xs", "sm", "md", "lg", "xl", "2xl", "none", "inner",
];

/// 「同前缀、不同语义」的族表：`(前缀, 宽度组, 颜色组)`。
///
/// 顺序敏感：更长的前缀必须排在前面（`ring-offset-` 先于 `ring-`，
/// `border-x-` 先于 `border-`）。
const SIZE_FAMILIES: &[(&str, &str, &str)] = &[
    ("inset-ring-offset-", "inset-ring-offset-w", "inset-ring-offset-color"),
    ("ring-offset-", "ring-offset-w", "ring-offset-color"),
    ("inset-ring-", "inset-ring-w", "inset-ring-color"),
    ("ring-", "ring-w", "ring-color"),
    ("outline-", "outline-w", "outline-color"),
    ("divide-x-", "divide-x-w", "divide-x-color"),
    ("divide-y-", "divide-y-w", "divide-y-color"),
    ("divide-", "divide-w", "divide-color"),
    ("border-x-", "border-x-w", "border-x-color"),
    ("border-y-", "border-y-w", "border-y-color"),
    ("border-t-", "border-t-w", "border-t-color"),
    ("border-r-", "border-r-w", "border-r-color"),
    ("border-b-", "border-b-w", "border-b-color"),
    ("border-l-", "border-l-w", "border-l-color"),
    ("border-", "border-w", "border-color"),
    ("shadow-", "shadow", "shadow-color"),
];

/// 判断 `{family}-{rest}` 的 `rest` 表示「宽度 / 尺寸」还是「颜色」。
fn is_size_value(family: &str, rest: &str) -> bool {
    // 任意值（`ring-[3px]` / `shadow-[0_1px_2px_rgb(0_0_0/0.1)]`）一律按宽度处理
    if rest.starts_with('[') {
        return true;
    }
    // 带透明度斜杠的只可能是颜色（`ring-ring/50`）
    if rest.contains('/') {
        return false;
    }
    // 裸族名（`ring` / `border` / `shadow`）等价于默认宽度
    if rest.is_empty() {
        return true;
    }
    if family == "shadow" {
        return SHADOW_SIZE_KEYWORDS.contains(&rest);
    }
    // 纯数字 / 小数 → 宽度
    rest.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// 处理 `border` / `ring` / `shadow` / `outline` / `divide` 这类
/// 「前缀相同、组不同」的族。命中返回组名，否则返回 `None` 交给前缀表。
fn size_family_group(base: &str) -> Option<&'static str> {
    for (prefix, size_group, color_group) in SIZE_FAMILIES {
        let bare = prefix.trim_end_matches('-');
        if base == bare {
            return Some(size_group);
        }
        if let Some(rest) = base.strip_prefix(prefix) {
            return Some(if is_size_value(bare, rest) {
                size_group
            } else {
                color_group
            });
        }
    }
    None
}

/// 求基类所属的类组（族判定优先，其次最长前缀匹配）。
fn group_of(base: &str) -> Option<&'static str> {
    if let Some(group) = size_family_group(base) {
        return Some(group);
    }
    let mut best: Option<(&str, &str)> = None;
    for (prefix, group) in GROUP_PREFIXES {
        if base.starts_with(prefix) {
            if best.map_or(true, |(p, _)| prefix.len() > p.len()) {
                best = Some((prefix, group));
            }
        }
    }
    best.map(|(_, g)| g)
}

/// 合并一组类名：`clsx` 语义 + 同类冲突消解（后者覆盖前者）。
pub fn cn(classes: &[&str]) -> String {
    // 1) clsx：切分、过滤空值、保序
    let mut tokens: Vec<&str> = Vec::new();
    for input in classes {
        for token in input.split_whitespace() {
            if !token.is_empty() {
                tokens.push(token);
            }
        }
    }

    // 2) tailwind-merge（子集）：同 (修饰符, 组) 只保留最后一个
    let mut keep = vec![true; tokens.len()];
    let mut last_index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (i, token) in tokens.iter().enumerate() {
        let (modifiers, base) = split_modifiers(token);
        if let Some(group) = group_of(base) {
            let key = format!("{}|{}", modifiers.join(":"), group);
            if let Some(prev) = last_index.insert(key, i) {
                keep[prev] = false;
            }
        }
    }

    tokens
        .iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|(t, _)| *t)
        .collect::<Vec<_>>()
        .join(" ")
}

/// 合并可选的类名（`None` / 空串自动跳过）。
pub fn cn_opt(classes: &[Option<&str>]) -> String {
    let owned: Vec<&str> = classes.iter().filter_map(|c| *c).collect();
    cn(&owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_and_filters_empty() {
        assert_eq!(cn(&["a  b", "", "  c "]), "a b c");
    }

    #[test]
    fn later_class_wins_in_same_group() {
        // 用户传入的 class 应覆盖组件默认
        assert_eq!(cn(&["bg-primary", "bg-secondary"]), "bg-secondary");
        assert_eq!(cn(&["p-2", "p-4"]), "p-4");
        assert_eq!(cn(&["size-4", "size-6"]), "size-6");
    }

    #[test]
    fn text_ui_is_font_size_not_color() {
        // text-ui-base（字号）与 text-foreground（颜色）不应互相覆盖
        assert_eq!(cn(&["text-ui-base", "text-foreground"]), "text-ui-base text-foreground");
        assert_eq!(cn(&["text-ui-base", "text-ui-lg"]), "text-ui-lg");
    }

    #[test]
    fn bg_clip_not_merged_with_bg_color() {
        // button.tsx 同时用 bg-clip-padding 与 bg-primary
        assert_eq!(
            cn(&["bg-clip-padding", "bg-primary"]),
            "bg-clip-padding bg-primary"
        );
    }

    #[test]
    fn modifiers_scope_the_conflict() {
        // 不同修饰符下不算冲突
        assert_eq!(
            cn(&["hover:bg-primary", "bg-secondary"]),
            "hover:bg-primary bg-secondary"
        );
        assert_eq!(cn(&["hover:bg-primary", "hover:bg-secondary"]), "hover:bg-secondary");
    }

    #[test]
    fn bracket_colons_are_not_modifier_separators() {
        // [&_svg:not([class*='size-'])]:size-4 —— 方括号内的冒号不是修饰符分隔
        let out = cn(&["[&_svg:not([class*='size-'])]:size-4", "[&_svg:not([class*='size-'])]:size-3"]);
        assert_eq!(out, "[&_svg:not([class*='size-'])]:size-3");
    }

    #[test]
    fn border_color_vs_width() {
        assert_eq!(cn(&["border", "border-border"]), "border border-border");
        assert_eq!(cn(&["border-border", "border-input-border"]), "border-input-border");
    }

    #[test]
    fn ring_width_not_merged_with_ring_color() {
        // avatar-badge / checkbox / switch 都写 ring-2 + ring-background 这种组合，
        // 若误判成同组会把宽度丢掉（focus ring 直接消失）
        assert_eq!(
            cn(&["ring-2", "ring-background"]),
            "ring-2 ring-background"
        );
        assert_eq!(
            cn(&["focus-visible:ring-2", "focus-visible:ring-ring/50"]),
            "focus-visible:ring-2 focus-visible:ring-ring/50"
        );
        // 同组才合并
        assert_eq!(cn(&["ring-2", "ring-4"]), "ring-4");
        assert_eq!(cn(&["ring-background", "ring-ring"]), "ring-ring");
        // 任意值按宽度处理
        assert_eq!(
            cn(&["focus-visible:ring-[3px]", "focus-visible:ring-ring/50"]),
            "focus-visible:ring-[3px] focus-visible:ring-ring/50"
        );
    }

    #[test]
    fn ring_offset_is_its_own_family() {
        // ring-offset- 不能被 ring- 抢先匹配
        assert_eq!(
            cn(&["ring-offset-2", "ring-2", "ring-offset-background"]),
            "ring-offset-2 ring-2 ring-offset-background"
        );
    }

    #[test]
    fn shadow_size_not_merged_with_shadow_color() {
        assert_eq!(
            cn(&["shadow-md", "shadow-foreground/10"]),
            "shadow-md shadow-foreground/10"
        );
        assert_eq!(cn(&["shadow-sm", "shadow-lg"]), "shadow-lg");
        assert_eq!(cn(&["shadow-none", "shadow-inner"]), "shadow-inner");
    }

    #[test]
    fn border_side_width_vs_color() {
        assert_eq!(
            cn(&["border-b-2", "border-b-border"]),
            "border-b-2 border-b-border"
        );
        assert_eq!(cn(&["border-b-2", "border-b-4"]), "border-b-4");
    }

    #[test]
    fn outline_width_vs_color() {
        assert_eq!(
            cn(&["outline-1", "outline-ring"]),
            "outline-1 outline-ring"
        );
    }

    #[test]
    fn cn_opt_skips_none() {
        assert_eq!(cn_opt(&[Some("a"), None, Some("b")]), "a b");
    }
}
