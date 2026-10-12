//! HTML 快照测试 —— 用 `dioxus-ssr` 把组件**真正序列化成 HTML 字符串**再断言。
//!
//! 与 `tests/render.rs` 的分工：
//!
//! - `render.rs` 走 `WriteMutations` 记录渲染指令，粒度更细（能区分静态属性 /
//!   动态属性 / 文本节点），但可读性差；
//! - 本文件走 `dioxus_ssr::render_element`，输出就是浏览器会看到的 HTML，
//!   断言读起来跟看页面一样，适合做**回归快照**。
//!
//! 两者都不需要 wasm / 窗口。

use dioxus::prelude::*;
use zcode_ui::*;

/// 把一个组件渲染成 HTML 字符串。
///
/// 参数故意收 `fn() -> Element`（而不是 `Element`）：`rsx!` 会在**调用方**的
/// 上下文里构造 props，而 `#[props(default)] onclick: EventHandler<..>` 这类
/// 默认值需要在 Dioxus runtime 里创建。若把 `rsx!{…}` 直接写在测试函数体里，
/// 就会撞上 `Must be called from inside a Dioxus runtime`。
/// 收成闭包后，`rsx!` 在 runtime 内执行，顺带 `use_signal` 之类的 hook 也合法。
///
/// 注意 `VirtualDom::new` 收的是 **fn 指针**，所以这里只能用不捕获变量的闭包；
/// 需要传参时把参数做成组件的 props（见下方 `CheckboxCase`）。
fn html(app: fn() -> Element) -> String {
    let mut dom = VirtualDom::new(app);
    dom.rebuild_in_place();
    dioxus_ssr::render(&dom)
}

// ---------------------------------------------------------------------------
// Button
// ---------------------------------------------------------------------------

#[test]
fn button_html() {
    let out = html(|| rsx! { Button { variant: ButtonVariant::Destructive, "删除" } });

    assert!(out.starts_with("<button "), "根节点不是 <button>：{out}");
    assert!(out.ends_with(">删除</button>"), "文本 children 没渲染：{out}");
    assert!(out.contains(r#"data-slot="button""#));
    assert!(out.contains(r#"data-variant="destructive""#));
    assert!(out.contains(r#"data-size="default""#));
    assert!(out.contains(r#"type="button""#));
    assert!(out.contains("bg-destructive"));
    // BUTTON_BASE 里的 text-ui-base/relaxed 会被 size 的 text-ui-base 覆盖：
    // 二者同属 font-size 组，tailwind-merge 语义下后者胜 —— 与 TS 版 cn() 一致。
    assert!(out.contains("text-ui-base"));
    assert!(!out.contains("text-ui-base/relaxed"));
    // 未传 disabled 时不应出现 disabled 属性（Dioxus 会直接省略 false 的布尔属性）
    // 注意：类名里本来就有 disabled:pointer-events-none，所以只能查 `=`
    assert!(!out.contains("disabled="), "{out}");
}

#[test]
fn button_disabled_and_type_override() {
    let out = html(|| rsx! {
        Button { disabled: true, html_type: "submit".to_string(), "提交" }
    });
    assert!(out.contains(r#"type="submit""#));
    // 布尔属性渲染成 `disabled=true`（无引号，合法 HTML5；HTML 布尔属性只看存在与否）
    assert!(out.contains("disabled=true"), "{out}");
}

#[test]
fn button_size_lg_and_user_class_override() {
    let out = html(|| rsx! { Button { size: ButtonSize::Lg, class: "w-full", "x" } });
    assert!(out.contains("h-8"), "Lg 尺寸没生效：{out}");
    assert!(out.contains("w-full"));
    assert!(!out.contains("h-7"), "默认尺寸没被覆盖：{out}");
}

// ---------------------------------------------------------------------------
// 简单原语
// ---------------------------------------------------------------------------

#[test]
fn separator_html_is_exact() {
    // 最简单的原语：无 children、无动态内容 —— 适合做逐字符快照
    let out = html(|| rsx! { Separator {} });
    assert_eq!(
        out,
        r#"<div class="shrink-0 bg-border data-horizontal:h-px data-horizontal:w-full data-vertical:w-px data-vertical:self-stretch" data-slot="separator" data-orientation="horizontal" role="none" aria-hidden="true"></div>"#
    );
}

#[test]
fn separator_vertical_non_decorative() {
    let out = html(|| rsx! {
        Separator { orientation: SeparatorOrientation::Vertical, decorative: false }
    });
    assert!(out.contains(r#"data-orientation="vertical""#));
    assert!(out.contains(r#"role="separator""#));
    assert!(out.contains(r#"aria-orientation="vertical""#));
    // 非装饰模式不应输出 aria-hidden
    assert!(!out.contains("aria-hidden"));
}

#[test]
fn label_html() {
    let out = html(|| rsx! { Label { html_for: "email".to_string(), "邮箱" } });
    assert!(out.starts_with("<label "));
    assert!(out.contains(r#"for="email""#));
    assert!(out.contains(r#"data-slot="label""#));
    assert!(out.ends_with(">邮箱</label>"));
}

#[test]
fn kbd_and_group_html() {
    let out = html(|| rsx! {
        KbdGroup {
            Kbd { "⌘" }
            Kbd { "K" }
        }
    });
    assert!(out.contains(r#"data-slot="kbd-group""#));
    assert_eq!(out.matches(r#"data-slot="kbd""#).count(), 2);
    assert!(out.contains("<kbd "));
    assert!(out.contains("h-5"));
    assert!(out.contains("min-w-5"));
    assert!(out.contains(">⌘</kbd>"));
}

#[test]
fn input_and_textarea_html() {
    let input = html(|| rsx! { Input { size: InputSize::Lg, placeholder: "搜索".to_string() } });
    assert!(input.starts_with("<input "));
    assert!(input.contains(r#"data-slot="input""#));
    assert!(input.contains(r#"placeholder="搜索""#));
    assert!(input.contains("h-8"));

    let ta = html(|| rsx! { Textarea { rows: 4, placeholder: "描述".to_string() } });
    assert!(ta.starts_with("<textarea "));
    assert!(ta.contains(r#"data-slot="textarea""#));
    // 数值属性 dioxus-ssr 不加引号（合法 HTML5）
    assert!(ta.contains("rows=4"), "{ta}");
    assert!(ta.contains("field-sizing-content"));
}

#[test]
fn spinner_html_uses_svg() {
    let out = html(|| rsx! { Spinner {} });
    assert!(out.starts_with("<svg "));
    assert!(out.contains(r#"viewBox="0 0 24 24""#), "viewBox 丢了：{out}");
    assert!(out.contains(r#"stroke-linecap="round""#));
    assert!(out.contains(r#"role="status""#));
    assert!(out.contains(r#"aria-label="Loading""#));
    assert_eq!(out.matches("<line ").count(), 8, "lucide loader 应有 8 条线");
    assert!(out.contains("animate-spin"));
}

// ---------------------------------------------------------------------------
// 状态驱动
// ---------------------------------------------------------------------------

#[test]
fn switch_html_state_driven() {
    let on = html(|| rsx! { Switch { checked: true } });
    assert!(on.contains(r#"role="switch""#));
    assert!(on.contains(r#"data-state="checked""#));
    assert!(on.contains(r#"aria-checked="true""#));
    assert!(on.contains(r#"data-slot="switch-thumb""#));

    let off = html(|| rsx! { Switch { checked: false } });
    assert!(off.contains(r#"data-state="unchecked""#));
    assert!(off.contains(r#"aria-checked="false""#));
    // 未 disabled 时不应输出 data-disabled 属性
    // （注意：类名里本来就有 data-disabled: 变体，所以只能查 `=`）
    assert!(!off.contains("data-disabled="), "{off}");

    let disabled = html(|| rsx! { Switch { checked: true, disabled: true } });
    assert!(disabled.contains("data-disabled="), "{disabled}");
    assert!(disabled.contains("disabled=true"), "{disabled}");
}

/// 把 `CheckboxState` 提升成 props，好让测试里的闭包保持「不捕获变量」。
#[component]
fn CheckboxCase(checked: CheckboxState) -> Element {
    rsx! { Checkbox { checked } }
}

#[test]
fn checkbox_html_three_states() {
    let cases = [
        (
            html(|| rsx! { CheckboxCase { checked: CheckboxState::Unchecked } }),
            "unchecked",
            "false",
        ),
        (
            html(|| rsx! { CheckboxCase { checked: CheckboxState::Checked } }),
            "checked",
            "true",
        ),
        (
            html(|| rsx! { CheckboxCase { checked: CheckboxState::Indeterminate } }),
            "indeterminate",
            "mixed",
        ),
    ];
    for (out, data_state, aria) in cases {
        assert!(out.contains(r#"role="checkbox""#));
        assert!(out.contains(&format!(r#"data-state="{data_state}""#)), "{out}");
        assert!(out.contains(&format!(r#"aria-checked="{aria}""#)), "{out}");
        assert_eq!(out.matches("<svg ").count(), 2, "勾号 + 减号两个图标：{out}");
        assert!(out.contains(r#"data-slot="checkbox-checked-icon""#));
        assert!(out.contains(r#"data-slot="checkbox-indeterminate-icon""#));
        // lucide path 数据
        assert!(out.contains(r#"d="M20 6 9 17l-5-5""#));
        assert!(out.contains(r#"d="M5 12h14""#));
    }
}

#[test]
fn checkbox_stroke_width_is_forwarded() {
    let out = html(|| rsx! { Checkbox { check_icon_stroke_width: Some(3.0) } });
    // 数值属性 dioxus-ssr 不加引号（合法 HTML5）
    assert!(out.contains("stroke-width=3"), "{out}");
    assert!(out.contains("vector-effect:non-scaling-stroke"));
}

// ---------------------------------------------------------------------------
// Tabs：context 串联
// ---------------------------------------------------------------------------

#[test]
fn tabs_context_wires_triggers_and_panels() {
    let out = html(|| rsx! {
        Tabs { value: use_signal(|| "general".to_string()),
            TabsList { variant: TabsListVariant::Line,
                TabsTrigger { value: "general", "常规" }
                TabsTrigger { value: "advanced", disabled: true, "高级" }
            }
            TabsContent { value: "general", "常规内容" }
            TabsContent { value: "advanced", "高级内容" }
        }
    });

    assert!(out.contains(r#"data-slot="tabs""#));
    assert!(out.contains(r#"data-orientation="horizontal""#));
    assert!(out.contains(r#"data-slot="tabs-list""#));
    assert!(out.contains(r#"data-variant="line""#));
    assert!(out.contains("gap-1 bg-transparent"));

    // 当前值 "general" → 第一个 trigger 激活，第二个未激活
    assert_eq!(out.matches(r#"data-slot="tabs-trigger""#).count(), 2);
    assert_eq!(out.matches(r#"data-state="active""#).count(), 2); // trigger + panel
    assert!(out.contains(r#"data-value="general""#));
    assert!(out.contains(r#"data-value="advanced""#));
    assert!(out.contains(r#"aria-selected="true""#));
    assert!(out.contains(r#"aria-selected="false""#));

    // 两个面板都挂载，非激活的带 hidden
    assert!(out.contains("常规内容"));
    assert!(out.contains("高级内容"));
    assert_eq!(out.matches("hidden").count(), 1);
}

#[test]
fn tabs_vertical_orientation() {
    let out = html(|| rsx! {
        Tabs { value: use_signal(|| "a".to_string()), orientation: TabsOrientation::Vertical,
            TabsList { TabsTrigger { value: "a", "A" } }
        }
    });
    assert!(out.contains(r#"data-orientation="vertical""#));
}

// ---------------------------------------------------------------------------
// Progress：归一化 + 分段
// ---------------------------------------------------------------------------

#[test]
fn progress_html_simple() {
    let out = html(|| rsx! { Progress { value: 42.0 } });
    assert!(out.contains(r#"role="progressbar""#));
    assert!(out.contains(r#"aria-valuenow="42""#));
    assert!(out.contains(r#"style="width: 42%""#));
    assert!(out.contains("bg-primary"));
    // 没有分段时不应出现 flex 容器
    assert!(!out.contains("bg-transparent"));
}

#[test]
fn progress_html_clamps_out_of_range_value() {
    let out = html(|| rsx! { Progress { value: 180.0 } });
    assert!(out.contains(r#"style="width: 100%""#));
    assert!(out.contains(r#"aria-valuenow="100""#));

    let out = html(|| rsx! { Progress { value: -5.0 } });
    assert!(out.contains(r#"style="width: 0%""#));
}

#[test]
fn progress_html_segments() {
    let out = html(|| rsx! {
        Progress {
            value: 60.0,
            segments: vec![
                ProgressSegment::new("a", 0.3).with_class("bg-sky-500"),
                ProgressSegment::new("b", 0.7),
                // percent = 0 应被过滤掉
                ProgressSegment::new("c", 0.0),
            ],
        }
    });
    assert!(out.contains("bg-transparent"));
    assert!(out.contains("overflow-hidden"));
    assert!(out.contains(r#"style="flex-basis: 30%""#));
    assert!(out.contains(r#"style="flex-basis: 70%""#));
    assert_eq!(out.matches("aria-hidden").count(), 2, "percent=0 的段应被过滤：{out}");
    assert!(out.contains("bg-sky-500"));
}

// ---------------------------------------------------------------------------
// Avatar
// ---------------------------------------------------------------------------

#[test]
fn avatar_html() {
    let out = html(|| rsx! {
        Avatar { size: AvatarSize::Lg,
            AvatarImage { src: "https://example.com/a.png".to_string(), alt: "用户".to_string() }
            AvatarFallback { "A" }
            AvatarBadge {}
        }
    });
    assert!(out.contains(r#"data-slot="avatar""#));
    assert!(out.contains(r#"data-size="lg""#));
    assert!(out.contains(r#"data-slot="avatar-image""#));
    assert!(out.contains(r#"src="https://example.com/a.png""#));
    assert!(out.contains(r#"alt="用户""#));
    assert!(out.contains("object-cover"));
    assert!(out.contains(r#"data-slot="avatar-fallback""#));
    assert!(out.contains(r#"data-slot="avatar-badge""#));
    assert!(out.contains("data-[size=lg]:size-10"));
}

#[test]
fn avatar_group_html() {
    let out = html(|| rsx! {
        AvatarGroup {
            Avatar { AvatarFallback { "A" } }
            AvatarGroupCount { "+3" }
        }
    });
    assert!(out.contains(r#"data-slot="avatar-group""#));
    assert!(out.contains("-space-x-2"));
    assert!(out.contains(r#"data-slot="avatar-group-count""#));
    assert!(out.contains("+3"));
}

// ---------------------------------------------------------------------------
// Card / Alert / Badge 组合
// ---------------------------------------------------------------------------

#[test]
fn card_composition_html() {
    let out = html(|| rsx! {
        Card { size: CardSize::Sm,
            CardHeader {
                CardTitle { "标题" }
                CardDescription { "描述" }
                CardAction { "操作" }
            }
            CardContent { "内容" }
            CardFooter { "页脚" }
        }
    });
    for slot in [
        "card",
        "card-header",
        "card-title",
        "card-description",
        "card-action",
        "card-content",
        "card-footer",
    ] {
        assert!(out.contains(&format!(r#"data-slot="{slot}""#)), "缺少 {slot}");
    }
    assert!(out.contains(r#"data-size="sm""#));
    // size=sm 只写属性，不改类名
    assert!(out.contains("data-[size=sm]:gap-3"));
    assert!(out.contains("@container/card-header"));
}

#[test]
fn alert_html() {
    let out = html(|| rsx! {
        Alert { variant: AlertVariant::Warning,
            AlertTitle { "注意" }
            AlertDescription { "这是一条警告" }
            AlertAction { "关闭" }
        }
    });
    assert!(out.contains(r#"data-slot="alert""#));
    assert!(out.contains(r#"data-variant="warning""#));
    assert!(out.contains("bg-warning"));
    assert!(out.contains(r#"data-slot="alert-title""#));
    assert!(out.contains(r#"data-slot="alert-description""#));
    assert!(out.contains(r#"data-slot="alert-action""#));
    assert!(out.contains("absolute top-1.5 right-2"));
}

#[test]
fn badge_html() {
    let out = html(|| rsx! { Badge { variant: BadgeVariant::Outline, "v3.15.1" } });
    assert!(out.starts_with("<span "));
    assert!(out.contains(r#"data-slot="badge""#));
    assert!(out.contains(r#"data-variant="outline""#));
    assert!(out.contains("border-border"));
    assert!(out.contains("text-ui-xs"));
    assert!(out.ends_with(">v3.15.1</span>"));
}

// ---------------------------------------------------------------------------
// 整棵树
// ---------------------------------------------------------------------------

#[component]
fn FullTree() -> Element {
    let tab = use_signal(|| "general".to_string());
    let mut on = use_signal(|| true);
    rsx! {
        div { class: "app",
            Button { variant: ButtonVariant::Ghost, "按钮" }
            Badge { "徽标" }
            Card { CardContent { "卡片" } }
            Alert { AlertDescription { "提示" } }
            Input { placeholder: "输入".to_string() }
            Textarea { placeholder: "多行".to_string() }
            Label { "标签" }
            Separator {}
            Progress { value: 25.0 }
            Kbd { "K" }
            Spinner {}
            Avatar { AvatarFallback { "A" } }
            AvatarGroup { AvatarGroupCount { "+1" } }
            Switch { checked: *on.read(), on_checked_change: move |v| on.set(v) }
            Checkbox { checked: CheckboxState::Checked }
            Tabs { value: tab,
                TabsList { TabsTrigger { value: "general", "常规" } }
                TabsContent { value: "general", "面板" }
            }
        }
    }
}

#[test]
fn full_tree_renders_to_html() {
    let out = html(|| rsx! { FullTree {} });

    // 标签闭合完整（粗略但有效的结构校验）
    assert_eq!(out.matches("<div ").count(), out.matches("</div>").count());
    assert_eq!(out.matches("<button ").count(), out.matches("</button>").count());
    assert_eq!(out.matches("<span ").count(), out.matches("</span>").count());
    assert_eq!(out.matches("<svg ").count(), out.matches("</svg>").count());

    // 所有 data-slot 都出现了
    for slot in [
        "button",
        "badge",
        "card",
        "card-content",
        "alert",
        "alert-description",
        "input",
        "textarea",
        "label",
        "separator",
        "progress",
        "progress-indicator",
        "kbd",
        "spinner",
        "avatar",
        "avatar-fallback",
        "avatar-group",
        "avatar-group-count",
        "switch",
        "switch-thumb",
        "checkbox",
        "checkbox-indicator",
        "tabs",
        "tabs-list",
        "tabs-trigger",
        "tabs-content",
    ] {
        assert!(out.contains(&format!(r#"data-slot="{slot}""#)), "缺少 {slot}");
    }

    // 文本全部到位
    for text in ["按钮", "徽标", "卡片", "提示", "标签", "常规", "面板"] {
        assert!(out.contains(text), "缺少文本 {text}");
    }

    // 没有残留的未解析占位
    assert!(!out.contains("data-slot=\"dynamic"));
    assert!(!out.contains("{{"));
}
