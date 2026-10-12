//! 把移植后的原语组件渲染成一份**可直接在浏览器打开的 HTML 页面**。
//!
//! ```bash
//! cargo run -p zcode-ui --example preview              # 生成 ./preview.html
//! cargo run -p zcode-ui --example preview -- out.html  # 指定输出路径
//! ```
//!
//! ## 这份页面里的类名从哪来
//!
//! 页面的 DOM 结构是手写的，但**每一个 `class` 都来自移植后组件的
//! `*_classes()` 纯函数**（`button_classes` / `card_header_classes` / …）。
//! 也就是说：如果移植过程中类名拼错了、variant 映射写错了，页面会立刻暴露，
//! 因为这里没有第二份「手抄的类名」可以掩盖问题。
//!
//! ## 样式为什么能生效
//!
//! 页面通过 CDN 加载 Tailwind v4 浏览器版，并用 `@theme` / `@custom-variant`
//! 复刻 `packages/ui/src/styles.css` 里与这批组件相关的那部分语义令牌：
//!
//! - `--color-*` 取 `styles.css` 中 `@theme` 的真实定义；
//! - `--text-ui-*` 字号刻度同样照搬；
//! - `data-checked:` / `data-active:` / `data-horizontal:` 等语义 variant
//!   按 `styles.css` 注释所述，从 `data-state` / `data-orientation` 映射而来。
//!
//! 少数几个令牌（`--color-muted` / `--color-muted-foreground` / `--color-ring`
//! / `--color-secondary-foreground`）在本仓库里由 `shadcn/tailwind.css` 提供，
//! 源码不在本仓库中，这里取 shadcn 默认量级的等价回退值并就地标注。
//!
//! > 需要联网加载 Tailwind CDN。离线打开会退化成无样式 HTML（结构仍然正确）。

use dioxus::prelude::*;
use std::fmt::Write as _;
use zcode_ui::*;

// ---------------------------------------------------------------------------
// 页面骨架
// ---------------------------------------------------------------------------

/// 复刻 `packages/ui/src/styles.css` 中与本批组件相关的主题令牌。
const THEME_CSS: &str = r#"
  /* Tailwind v4 默认的 dark: 跟随 prefers-color-scheme。本页只定义浅色令牌，
     这里把它改成 class 驱动（与 packages/ui 的 .dark 约定一致），
     避免系统处于深色模式时页面半深半浅。 */
  @custom-variant dark (&:where(.dark, .dark *));

  /* 语义 variant：把 Radix 的 data-state / data-orientation 映射过来。
     见 packages/ui/src/styles.css 顶部注释与 shadcn/tailwind.css。 */
  @custom-variant data-checked   (&[data-state="checked"]);
  @custom-variant data-unchecked (&[data-state="unchecked"]);
  @custom-variant data-active    (&[data-state="active"]);
  @custom-variant data-horizontal (&[data-orientation="horizontal"]);
  @custom-variant data-vertical   (&[data-orientation="vertical"]);
  @custom-variant data-disabled   (&[data-disabled]);

  @theme {
    /* ---- 字号刻度（styles.css: --text-ui-*）---- */
    --text-ui-xl: calc(var(--ui-font-size) + 4px);
    --text-ui-lg: calc(var(--ui-font-size) + 2px);
    --text-ui-base: var(--ui-font-size);
    --text-ui-caption: calc(var(--ui-font-size) - 1px);
    --text-ui-sm: calc(var(--ui-font-size) - 2px);
    --text-ui-xs: calc(var(--ui-font-size) - 4px);

    /* ---- 颜色（styles.css: @theme 原值）---- */
    --color-brand: var(--color-sky-400);
    --color-background: var(--color-neutral-50);
    --color-border: color-mix(in oklab, var(--color-neutral-950) 10%, transparent);
    --color-border-hover: color-mix(in oklab, var(--color-neutral-950) 20%, transparent);
    --color-hover: var(--color-neutral-200);
    --color-card: var(--color-white);
    --color-card-border: var(--color-border);
    --color-input: var(--color-white);
    --color-input-focused: var(--color-neutral-50);
    --color-input-border: var(--color-border);
    --color-input-border-hover: var(--color-border-hover);
    --color-input-border-focused: var(--color-brand);
    --color-accent: var(--color-sky-50);
    --color-primary: var(--color-neutral-950);
    --color-primary-foreground: var(--color-neutral-50);
    --color-secondary: var(--color-neutral-300);
    --color-foreground: var(--color-neutral-700);
    --color-foreground-subtle: color-mix(in oklab, var(--color-neutral-700) 60%, transparent);
    --color-foreground-subtlest: color-mix(in oklab, var(--color-neutral-700) 40%, transparent);
    --color-destructive: var(--color-red-600);
    --color-destructive-foreground: var(--color-white);
    --color-warning: var(--color-yellow-600);
    --color-warning-foreground: var(--color-white);

    /* ---- 以下 4 个令牌由 shadcn/tailwind.css 提供（源码不在本仓库），
            此处取 shadcn 默认量级的等价回退值 ---- */
    --color-muted: var(--color-neutral-100);
    --color-muted-foreground: color-mix(in oklab, var(--color-neutral-700) 60%, transparent);
    --color-secondary-foreground: var(--color-neutral-900);
    --color-ring: var(--color-neutral-400);
  }
"#;

fn page(body: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>ZCode UI —— Rust + Dioxus 移植预览</title>
<script src="https://cdn.jsdelivr.net/npm/@tailwindcss/browser@4"></script>
<style>
  :root {{ --ui-font-size: 14px; }}
  body {{ margin: 0; }}
</style>
<style type="text/tailwindcss">{THEME_CSS}</style>
</head>
<body class="bg-background text-foreground antialiased">
{body}
</body>
</html>
"#
    )
}

// ---------------------------------------------------------------------------
// 小工具
// ---------------------------------------------------------------------------

fn section(title: &str, hint: &str, inner: &str) -> String {
    format!(
        r#"<section class="border-b border-border px-6 py-6">
  <h2 class="mb-1 text-ui-lg font-medium text-foreground">{title}</h2>
  <p class="mb-4 text-ui-sm text-foreground-subtle">{hint}</p>
  {inner}
</section>"#
    )
}

fn row(inner: &str) -> String {
    format!(r#"<div class="flex flex-wrap items-center gap-3">{inner}</div>"#)
}

fn code(inner: &str) -> String {
    format!(
        r#"<code class="rounded-sm bg-muted px-1 py-0.5 font-mono text-ui-xs">{inner}</code>"#
    )
}

/// 把组件输出的类名贴到指定标签上，生成一段静态 HTML。
fn el(tag: &str, class: &str, attrs: &str, inner: &str) -> String {
    format!(r#"<{tag} class="{class}"{attrs}>{inner}</{tag}>"#)
}

// ---------------------------------------------------------------------------
// 各区块
// ---------------------------------------------------------------------------

fn buttons_section() -> String {
    let variants = [
        ("Default", ButtonVariant::Default),
        ("Outline", ButtonVariant::Outline),
        ("Secondary", ButtonVariant::Secondary),
        ("Ghost", ButtonVariant::Ghost),
        ("Destructive", ButtonVariant::Destructive),
        ("Warning", ButtonVariant::Warning),
        ("Link", ButtonVariant::Link),
    ];

    let mut all_variants = String::new();
    for (label, variant) in variants {
        let cls = button_classes(variant, ButtonSize::Default, "");
        all_variants.push_str(&el(
            "button",
            &cls,
            &format!(
                r#" type="button" data-slot="button" data-variant="{}" data-size="default""#,
                variant.as_str()
            ),
            label,
        ));
    }

    let sizes = [
        ("xs", ButtonSize::Xs),
        ("sm", ButtonSize::Sm),
        ("default", ButtonSize::Default),
        ("lg", ButtonSize::Lg),
    ];
    let mut all_sizes = String::new();
    for (label, size) in sizes {
        let cls = button_classes(ButtonVariant::Default, size, "");
        all_sizes.push_str(&el(
            "button",
            &cls,
            &format!(
                r#" type="button" data-slot="button" data-variant="default" data-size="{}""#,
                size.as_str()
            ),
            &format!("size={label}"),
        ));
    }

    let icons = [
        ("icon-xs", ButtonSize::IconXs),
        ("icon-sm", ButtonSize::IconSm),
        ("icon", ButtonSize::Icon),
        ("icon-md", ButtonSize::IconMd),
        ("icon-lg", ButtonSize::IconLg),
    ];
    let mut all_icons = String::new();
    for (label, size) in icons {
        let cls = button_classes(ButtonVariant::Outline, size, "");
        all_icons.push_str(&el(
            "button",
            &cls,
            &format!(
                r#" type="button" data-slot="button" data-variant="outline" data-size="{}" title="{label}""#,
                size.as_str()
            ),
            // 内联 lucide `plus`
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14"/><path d="M12 5v14"/></svg>"#,
        ));
    }

    let disabled = {
        let cls = button_classes(ButtonVariant::Default, ButtonSize::Default, "");
        el(
            "button",
            &cls,
            r#" type="button" data-slot="button" disabled"#,
            "disabled",
        )
    };

    let overridden = {
        // 用户传入的 class 覆盖组件默认（tailwind-merge 语义）
        let cls = button_classes(ButtonVariant::Default, ButtonSize::Default, "w-full justify-start");
        el(
            "button",
            &cls,
            r#" type="button" data-slot="button""#,
            "w-full justify-start（用户 class 覆盖）",
        )
    };

    section(
        "Button",
        &format!(
            "7 个 variant · 4 个尺寸 · 5 个图标尺寸。类名来自 {}",
            code("button_classes(variant, size, class)")
        ),
        &format!(
            r#"<div class="space-y-4">
  <div>{}</div>
  <div>{}</div>
  <div>{}</div>
  <div class="flex flex-wrap items-center gap-3">{}{}</div>
</div>"#,
            row(&all_variants),
            row(&all_sizes),
            row(&all_icons),
            disabled,
            overridden
        ),
    )
}

fn badges_section() -> String {
    let variants = [
        ("Default", BadgeVariant::Default),
        ("Secondary", BadgeVariant::Secondary),
        ("Destructive", BadgeVariant::Destructive),
        ("Outline", BadgeVariant::Outline),
        ("Ghost", BadgeVariant::Ghost),
        ("Link", BadgeVariant::Link),
    ];
    let mut out = String::new();
    for (label, variant) in variants {
        let cls = badge_classes(variant, "");
        out.push_str(&el(
            "span",
            &cls,
            &format!(
                r#" data-slot="badge" data-variant="{}""#,
                variant.as_str()
            ),
            label,
        ));
    }
    section("Badge", "6 个 variant，类名来自 badge_classes()", &row(&out))
}

fn cards_section() -> String {
    let card = |inner: &str| {
        el(
            "div",
            &card_classes(""),
            r#" data-slot="card" data-size="default""#,
            inner,
        )
    };

    let default_card = card(&format!(
        r#"{}{}{}"#,
        el(
            "div",
            &cn(&[CARD_HEADER_BASE, ""]),
            r#" data-slot="card-header""#,
            &format!(
                r#"{}{}{}"#,
                el(
                    "div",
                    &cn(&[CARD_TITLE_BASE, ""]),
                    r#" data-slot="card-title""#,
                    "默认尺寸卡片"
                ),
                el(
                    "div",
                    &cn(&[CARD_DESCRIPTION_BASE, ""]),
                    r#" data-slot="card-description""#,
                    "CardSize::Default —— gap-4 / py-4"
                ),
                el(
                    "div",
                    &cn(&[CARD_ACTION_BASE, ""]),
                    r#" data-slot="card-action""#,
                    &el(
                        "button",
                        &button_classes(ButtonVariant::Ghost, ButtonSize::IconXs, ""),
                        r#" type="button""#,
                        r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>"#,
                    )
                ),
            ),
        ),
        el(
            "div",
            &cn(&[CARD_CONTENT_BASE, ""]),
            r#" data-slot="card-content""#,
            "CardAction 的存在会触发 <code>has-data-[slot=card-action]:grid-cols-[1fr_auto]</code>，标题与操作按钮自动分列。",
        ),
        el(
            "div",
            &cn(&[CARD_FOOTER_BASE, ""]),
            r#" data-slot="card-footer""#,
            &row(&format!(
                "{}{}",
                el(
                    "button",
                    &button_classes(ButtonVariant::Default, ButtonSize::Sm, ""),
                    r#" type="button""#,
                    "确认"
                ),
                el(
                    "button",
                    &button_classes(ButtonVariant::Outline, ButtonSize::Sm, ""),
                    r#" type="button""#,
                    "取消"
                )
            )),
        ),
    ));

    let sm_card = el(
        "div",
        &card_classes(""),
        r#" data-slot="card" data-size="sm""#,
        &format!(
            r#"{}{}"#,
            el(
                "div",
                &cn(&[CARD_HEADER_BASE, ""]),
                r#" data-slot="card-header""#,
                &format!(
                    r#"{}{}"#,
                    el(
                        "div",
                        &cn(&[CARD_TITLE_BASE, ""]),
                        r#" data-slot="card-title""#,
                        "小尺寸卡片"
                    ),
                    el(
                        "div",
                        &cn(&[CARD_DESCRIPTION_BASE, ""]),
                        r#" data-slot="card-description""#,
                        "CardSize::Sm —— 只改 data-size，由 group-data-[size=sm]/card:* 生效"
                    ),
                ),
            ),
            el(
                "div",
                &cn(&[CARD_CONTENT_BASE, ""]),
                r#" data-slot="card-content""#,
                "注意这里没有任何「手写的小尺寸类」——尺寸完全由 data-size 属性驱动。",
            ),
        ),
    );

    section(
        "Card",
        "Card / CardHeader / CardTitle / CardDescription / CardAction / CardContent / CardFooter",
        &format!(r#"<div class="grid gap-4 md:grid-cols-2">{default_card}{sm_card}</div>"#),
    )
}

fn alerts_section() -> String {
    let mk = |variant: AlertVariant, title: &str, desc: &str| {
        el(
            "div",
            &alert_classes(variant, ""),
            &format!(
                r#" data-slot="alert" data-variant="{}" role="alert""#,
                variant.as_str()
            ),
            &format!(
                r#"{}{}"#,
                el(
                    "div",
                    &cn(&[ALERT_TITLE_BASE, ""]),
                    r#" data-slot="alert-title""#,
                    title
                ),
                el(
                    "div",
                    &cn(&[ALERT_DESCRIPTION_BASE, ""]),
                    r#" data-slot="alert-description""#,
                    desc
                ),
            ),
        )
    };

    section(
        "Alert",
        "3 个 variant；has-data-[slot=alert-action] 会让右侧留出操作位",
        &format!(
            r#"<div class="space-y-3">{}{}{}</div>"#,
            mk(
                AlertVariant::Default,
                "默认提示",
                "这是一条普通信息，用于说明当前状态。"
            ),
            mk(
                AlertVariant::Warning,
                "注意",
                "危险操作前请确认已备份数据。"
            ),
            mk(
                AlertVariant::Destructive,
                "执行失败",
                "命令返回了非零退出码，请检查日志。"
            ),
        ),
    )
}

fn forms_section() -> String {
    let label = |text: &str, for_id: &str| {
        el(
            "label",
            &label_classes(""),
            &format!(r#" data-slot="label" for="{for_id}""#),
            text,
        )
    };

    let mut inputs = String::new();
    for (name, size) in [
        ("xs", InputSize::Xs),
        ("sm", InputSize::Sm),
        ("default", InputSize::Default),
        ("lg", InputSize::Lg),
    ] {
        inputs.push_str(&el(
            "input",
            &input_classes(size, ""),
            &format!(
                r#" type="text" data-slot="input" placeholder="InputSize::{name}""#
            ),
            "",
        ));
    }

    let disabled = el(
        "input",
        &input_classes(InputSize::Default, ""),
        r#" type="text" data-slot="input" placeholder="disabled" disabled"#,
        "",
    );

    let textarea = el(
        "textarea",
        &textarea_classes(""),
        r#" data-slot="textarea" rows="3" placeholder="Textarea —— min-h-16 resize-none field-sizing-content""#,
        "",
    );

    section(
        "Input / Textarea / Label",
        "4 个输入尺寸；Textarea 使用 field-sizing-content 自适应高度",
        &format!(
            r#"<div class="grid max-w-xl gap-4">
  <div class="grid gap-1.5">{}{}</div>
  <div class="grid gap-2">{}</div>
  <div class="grid gap-2">{}</div>
  <div class="grid gap-1.5">{}</div>
</div>"#,
            label("Input 尺寸阶梯", "input-sizes"),
            inputs,
            disabled,
            label("描述", "desc"),
            textarea,
        ),
    )
}

fn progress_section() -> String {
    let simple = el(
        "div",
        &progress_classes(""),
        r#" data-slot="progress" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="42""#,
        &el(
            "div",
            &progress_indicator_classes(42.0, false, ""),
            r#" data-slot="progress-indicator" style="width: 42%""#,
            "",
        ),
    );

    let with_segments = |segments: &[(&str, f64, &str)], value: f64| {
        let mut spans = String::new();
        for (id, percent, extra) in segments {
            spans.push_str(&format!(
                r#"<span class="{}" style="flex-basis: {}%" aria-hidden="true"></span>"#,
                cn(&[PROGRESS_SEGMENT_BASE, extra]),
                percent * 100.0
            ));
            let _ = id;
        }
        el(
            "div",
            &progress_classes(""),
            &format!(
                r#" data-slot="progress" role="progressbar" aria-valuenow="{value}""#
            ),
            &el(
                "div",
                &progress_indicator_classes(value, true, ""),
                &format!(r#" data-slot="progress-indicator" style="width: {value}%""#),
                &spans,
            ),
        )
    };

    let hint = el(
        "div",
        &progress_classes("h-2"),
        r#" data-slot="progress" role="progressbar" aria-valuenow="70""#,
        &el(
            "div",
            &progress_indicator_classes(70.0, false, "bg-warning"),
            r#" data-slot="progress-indicator" style="width: 70%""#,
            "",
        ),
    );

    section(
        "Progress",
        "value 夹取到 [0,100]；存在有效分段时指示条切为 flex 容器，每段用 flex-basis 定位",
        &format!(
            r#"<div class="max-w-xl space-y-4">
  <div>{simple}</div>
  <div>{multi}</div>
  <div>{hint}</div>
</div>"#,
            multi = with_segments(
                &[
                    ("a", 0.3, "bg-primary"),
                    ("b", 0.45, "bg-sky-500"),
                    ("c", 0.25, "bg-teal-500"),
                ],
                88.0
            ),
        ),
    )
}

fn kbd_section() -> String {
    let kbd = |s: &str| el("kbd", &kbd_classes(""), r#" data-slot="kbd""#, s);

    let combo = el(
        "div",
        &kbd_group_classes(""),
        r#" data-slot="kbd-group""#,
        &format!("{}{}{}", kbd("⌘"), kbd("⇧"), kbd("K")),
    );
    let single = row(&format!("{}{}{}", kbd("⌘"), kbd("K"), kbd("Esc")));

    let spinner = el(
        "svg",
        &spinner_classes("text-foreground-subtle"),
        r#" data-slot="spinner" role="status" aria-label="Loading" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="12" x2="12" y1="2" y2="6"/><line x1="12" x2="12" y1="18" y2="22"/><line x1="4.93" x2="7.76" y1="4.93" y2="7.76"/><line x1="16.24" x2="19.07" y1="16.24" y2="19.07"/><line x1="2" x2="6" y1="12" y2="12"/><line x1="18" x2="22" y1="12" y2="12"/><line x1="4.93" x2="7.76" y1="19.07" y2="16.24"/><line x1="16.24" x2="19.07" y1="7.76" y2="4.93"/></svg>"#,
        "",
    );

    section(
        "Kbd / KbdGroup / Spinner",
        "键帽固定 h-5 + min-w-5 保证单符号键与字母键等宽；Spinner 内联 lucide `loader` 路径",
        &format!(
            r#"<div class="space-y-4">
  <div>{combo}</div>
  <div>{single}</div>
  <div class="flex items-center gap-2 text-ui-sm text-foreground-subtle">{spinner} 加载中…</div>
</div>"#
        ),
    )
}

fn avatar_section() -> String {
    let fallback = |text: &str, class: &str| {
        el(
            "span",
            &avatar_fallback_classes(class),
            r#" data-slot="avatar-fallback""#,
            text,
        )
    };

    let mk = |size: AvatarSize, text: &str| {
        el(
            "span",
            &avatar_classes(""),
            &format!(r#" data-slot="avatar" data-size="{}""#, size.as_str()),
            &format!(
                r#"{}{}"#,
                fallback(text, ""),
                el(
                    "span",
                    &avatar_badge_classes(""),
                    r#" data-slot="avatar-badge""#,
                    ""
                ),
            ),
        )
    };

    let group = el(
        "div",
        &avatar_group_classes(""),
        r#" data-slot="avatar-group""#,
        &format!(
            r#"{}{}{}"#,
            el(
                "span",
                &avatar_classes(""),
                r#" data-slot="avatar" data-size="default""#,
                &fallback("A", "")
            ),
            el(
                "span",
                &avatar_classes(""),
                r#" data-slot="avatar" data-size="default""#,
                &fallback("B", "")
            ),
            el(
                "div",
                &avatar_group_count_classes(""),
                r#" data-slot="avatar-group-count""#,
                "+3"
            ),
        ),
    );

    let image = el(
        "span",
        &avatar_classes(""),
        r#" data-slot="avatar" data-size="lg""#,
        &format!(
            r#"<img class="{}" data-slot="avatar-image" src="data:image/svg+xml;utf8,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 40 40'%3E%3Crect width='40' height='40' fill='%230ea5e9'/%3E%3C/svg%3E" alt="用户" />"#,
            avatar_image_classes("")
        ),
    );

    section(
        "Avatar",
        "data-size 驱动尺寸（sm/default/lg）；AvatarGroup 用 -space-x-2 叠放",
        &format!(
            r#"<div class="flex flex-wrap items-center gap-6">
  <div class="flex items-center gap-3">{}{}{}</div>
  <div class="flex items-center gap-3">{group}</div>
  <div class="flex items-center gap-3">{image}</div>
</div>"#,
            mk(AvatarSize::Sm, "S"),
            mk(AvatarSize::Default, "M"),
            mk(AvatarSize::Lg, "L"),
        ),
    )
}

fn toggle_section() -> String {
    let switch = |checked: bool, size: SwitchSize| {
        el(
            "button",
            &switch_classes(""),
            &format!(
                r#" type="button" role="switch" data-slot="switch" data-size="{}" data-state="{}" aria-checked="{}""#,
                size.as_str(),
                if checked { "checked" } else { "unchecked" },
                checked
            ),
            &el(
                "span",
                &switch_thumb_classes(""),
                r#" data-slot="switch-thumb""#,
                "",
            ),
        )
    };

    let checkbox = |state: CheckboxState| {
        let check = format!(
            r#"<svg class="{}" data-slot="checkbox-checked-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>"#,
            checkbox_icon_classes(CHECKBOX_CHECKED_ICON_BASE, "", None)
        );
        let minus = format!(
            r#"<svg class="{}" data-slot="checkbox-indeterminate-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14"/></svg>"#,
            checkbox_icon_classes(CHECKBOX_INDETERMINATE_ICON_BASE, "", None)
        );
        el(
            "button",
            &checkbox_classes(""),
            &format!(
                r#" type="button" role="checkbox" data-slot="checkbox" data-state="{}" aria-checked="{}""#,
                state.as_str(),
                state.aria_checked()
            ),
            &el(
                "span",
                &checkbox_indicator_classes(""),
                r#" data-slot="checkbox-indicator""#,
                &format!("{check}{minus}"),
            ),
        )
    };

    let tag = |s: &str| {
        format!(
            r#"<span class="text-ui-sm text-foreground-subtle">{s}</span>"#
        )
    };

    section(
        "Switch / Checkbox",
        "data-state 驱动全部视觉状态（data-checked: / data-unchecked: / data-[state=indeterminate]:）",
        &format!(
            r#"<div class="flex flex-wrap items-center gap-10">
  <div class="flex items-center gap-3">{}{}{}{}</div>
  <div class="flex items-center gap-3">{}{}{}</div>
</div>"#,
            tag("Switch 关"),
            switch(false, SwitchSize::Default),
            tag("开"),
            switch(true, SwitchSize::Default),
            tag("未选"),
            checkbox(CheckboxState::Unchecked),
            checkbox(CheckboxState::Checked),
        ),
    ) + &format!(
        r#"<div class="mt-4 flex flex-wrap items-center gap-10">
  <div class="flex items-center gap-3">{}{}{}</div>
  <div class="flex items-center gap-3">{}{}{}</div>
</div>"#,
        tag("半选"),
        checkbox(CheckboxState::Indeterminate),
        tag("小号开关"),
        switch(true, SwitchSize::Sm),
        tag("小号关"),
        switch(false, SwitchSize::Sm),
    )
}

fn tabs_section() -> String {
    let trigger = |value: &str, active: bool, label: &str| {
        el(
            "button",
            &tabs_trigger_classes(""),
            &format!(
                r#" type="button" role="tab" data-slot="tabs-trigger" data-state="{}" data-value="{value}" aria-selected="{}""#,
                if active { "active" } else { "inactive" },
                active
            ),
            label,
        )
    };

    let list = |variant: TabsListVariant, inner: &str| {
        el(
            "div",
            &tabs_list_classes(variant, ""),
            &format!(
                r#" data-slot="tabs-list" data-variant="{}" role="tablist""#,
                variant.as_str()
            ),
            inner,
        )
    };

    let panel = |active: bool, inner: &str| {
        el(
            "div",
            &tabs_content_classes(""),
            &format!(
                r#" data-slot="tabs-content" data-state="{}" role="tabpanel"{}"#,
                if active { "active" } else { "inactive" },
                if active { "" } else { " hidden" }
            ),
            inner,
        )
    };

    let default_tabs = el(
        "div",
        &tabs_classes(""),
        r#" data-slot="tabs" data-orientation="horizontal""#,
        &format!(
            r#"{}{}"#,
            list(
                TabsListVariant::Default,
                &format!(
                    "{}{}",
                    trigger("general", true, "常规"),
                    trigger("advanced", false, "高级")
                )
            ),
            &format!(
                r#"<div class="pt-3 text-ui-base/relaxed">默认变体：<code class="rounded-sm bg-muted px-1 text-ui-xs">bg-muted</code> 底 + 选中项 <code class="rounded-sm bg-muted px-1 text-ui-xs">data-active:bg-background</code>。</div>"#
            ),
        ),
    );

    let line_tabs = el(
        "div",
        &tabs_classes(""),
        r#" data-slot="tabs" data-orientation="horizontal""#,
        &format!(
            r#"{}{}"#,
            list(
                TabsListVariant::Line,
                &format!(
                    "{}{}{}",
                    trigger("a", true, "已激活"),
                    trigger("b", false, "未激活"),
                    trigger("c", false, "另一个")
                )
            ),
            &format!(
                r#"<div class="pt-3 text-ui-base/relaxed">line 变体：透明底 + <code class="rounded-sm bg-muted px-1 text-ui-xs">after:</code> 下划线，由 <code class="rounded-sm bg-muted px-1 text-ui-xs">group-data-[variant=line]/tabs-list:data-active:after:opacity-100</code> 点亮。</div>"#
            ),
        ),
    );

    let vertical = el(
        "div",
        &tabs_classes("w-fit"),
        r#" data-slot="tabs" data-orientation="vertical""#,
        &format!(
            r#"{}{}"#,
            list(
                TabsListVariant::Default,
                &format!(
                    "{}{}{}",
                    trigger("v1", true, "垂直一"),
                    trigger("v2", false, "垂直二"),
                    trigger("v3", false, "垂直三")
                )
            ),
            panel(true, "垂直方向的 TabsList 会变成 flex-col。"),
        ),
    );

    section(
        "Tabs",
        "Tabs / TabsList / TabsTrigger / TabsContent；data-orientation 驱动横竖布局",
        &format!(
            r#"<div class="space-y-6">{default_tabs}{line_tabs}{vertical}</div>"#
        ),
    )
}

fn separator_section() -> String {
    let h = el(
        "div",
        &separator_classes(""),
        r#" data-slot="separator" data-orientation="horizontal" role="none" aria-hidden="true""#,
        "",
    );
    let v = el(
        "div",
        &separator_classes("h-6"),
        r#" data-slot="separator" data-orientation="vertical" role="none" aria-hidden="true""#,
        "",
    );

    section(
        "Separator",
        "data-orientation 驱动 data-horizontal: / data-vertical: 两套尺寸规则",
        &format!(
            r#"<div class="max-w-md space-y-4">
  <div>横向分隔线（data-horizontal:h-px data-horizontal:w-full）</div>
  {h}
  <div class="flex items-center gap-3">纵向分隔线{v}同一行</div>
</div>"#
        ),
    )
}

// ---------------------------------------------------------------------------
// 组装
// ---------------------------------------------------------------------------

/// 用 Dioxus 真渲染一遍所有组件（证明 `#[component]` 定义在真实渲染路径下可用），
/// 把结果作为「运行期自检」嵌进页面顶部。
#[component]
fn RuntimeCheck() -> Element {
    let mut on = use_signal(|| true);
    let tab = use_signal(|| "general".to_string());

    rsx! {
        div { class: "flex flex-wrap items-center gap-3",
            Button { variant: ButtonVariant::Default, size: ButtonSize::Sm, "Dioxus 真渲染" }
            Badge { variant: BadgeVariant::Secondary, "use_signal" }
            Switch { checked: *on.read(), on_checked_change: move |v| on.set(v) }
            Checkbox { checked: CheckboxState::Indeterminate }
            Progress { value: 30.0 }
            Kbd { "⌘" }
            Spinner {}
            Tabs { value: tab,
                TabsList {
                    TabsTrigger { value: "general", "常规" }
                    TabsTrigger { value: "advanced", "高级" }
                }
            }
        }
    }
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "preview.html".to_string());

    // 真渲染：走一遍 Dioxus 的组件树 + dioxus-ssr 的 HTML 序列化
    let runtime_html = dioxus_ssr::render_element(rsx! { RuntimeCheck {} });

    let mut body = String::new();
    let _ = write!(
        body,
        r#"<header class="border-b border-border bg-card px-6 py-8">
  <h1 class="text-ui-xl font-medium text-foreground">ZCode UI · Rust + Dioxus 移植预览</h1>
  <p class="mt-2 max-w-3xl text-ui-base/relaxed text-foreground-subtle">
    这一页的每个 <code class="rounded-sm bg-muted px-1 font-mono text-ui-xs">class</code>
    都来自 <code class="rounded-sm bg-muted px-1 font-mono text-ui-xs">rust/crates/zcode-ui</code>
    里移植后组件的 <code class="rounded-sm bg-muted px-1 font-mono text-ui-xs">*_classes()</code> 纯函数，
    样式由 Tailwind v4 + 从 <code class="rounded-sm bg-muted px-1 font-mono text-ui-xs">packages/ui/src/styles.css</code>
    复刻的 <code class="rounded-sm bg-muted px-1 font-mono text-ui-xs">@theme</code> 令牌驱动。
  </p>
  <div class="mt-4 rounded-lg border border-border bg-background p-3">
    <div class="mb-2 text-ui-sm text-foreground-subtle">
      下面这组控件是 <strong>Dioxus 组件真渲染</strong>（<code class="rounded-sm bg-muted px-1 font-mono text-ui-xs">dioxus_ssr::render_element(rsx! {{ … }})</code>）的结果，
      不是手写 HTML：
    </div>
    {runtime_html}
  </div>
</header>
"#
    );

    body.push_str(&buttons_section());
    body.push_str(&badges_section());
    body.push_str(&cards_section());
    body.push_str(&alerts_section());
    body.push_str(&forms_section());
    body.push_str(&progress_section());
    body.push_str(&kbd_section());
    body.push_str(&avatar_section());
    body.push_str(&toggle_section());
    body.push_str(&tabs_section());
    body.push_str(&separator_section());

    let _ = write!(
        body,
        r#"<footer class="px-6 py-8 text-ui-sm text-foreground-subtle">
  由 <code class="rounded-sm bg-muted px-1 font-mono text-ui-xs">cargo run -p zcode-ui --example preview</code> 生成 ·
  移植说明见 <code class="rounded-sm bg-muted px-1 font-mono text-ui-xs">rust/PORTING-UI.md</code>
</footer>
"#
    );

    let html = page(&body);
    match std::fs::write(&out, html.as_bytes()) {
        Ok(()) => println!(
            "已生成 {out}（{} 字节 / {} 行）",
            html.len(),
            html.lines().count()
        ),
        Err(e) => {
            eprintln!("写入 {out} 失败：{e}");
            std::process::exit(1);
        }
    }
}
