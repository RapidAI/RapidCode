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

// ===========================================================================
// 第二批原语：折叠类 / 表单组合 / 浮层
// ===========================================================================

/// 取出包含 `data-slot="<slot>"` 的那个开标签的完整文本。
///
/// 断言「某个变体类名落在哪个元素上」时用得上 —— 直接对整棵树的字符串做
/// `contains` 会误判（同一个类名可能在别处也出现）。
fn tag_with_slot<'a>(out: &'a str, slot: &str) -> &'a str {
    let needle = format!(r#"data-slot="{slot}""#);
    let end = out
        .find(&needle)
        .unwrap_or_else(|| panic!("找不到 data-slot=\"{slot}\""))
        + needle.len();
    let start = out[..end].rfind('<').expect("slot 不在标签内");
    let close = out[end..].find('>').expect("标签未闭合") + end;
    &out[start..=close]
}

/// `hidden` 是布尔属性，dioxus-ssr 输出的是不带引号的 `hidden=true`。
fn has_hidden(out: &str) -> bool {
    out.contains("hidden=true") || out.contains(r#"hidden="true""#)
}

// ---------------------------------------------------------------------------
// Collapsible
// ---------------------------------------------------------------------------

#[component]
fn CollapsibleClosed() -> Element {
    rsx! {
        Collapsible {
            CollapsibleTrigger { id: "sec", "展开" }
            CollapsibleContent { id: "sec-content", "内容" }
        }
    }
}

#[component]
fn CollapsibleOpen() -> Element {
    rsx! {
        Collapsible { default_open: true,
            CollapsibleTrigger { id: "sec", "展开" }
            CollapsibleContent { id: "sec-content", "内容" }
        }
    }
}

#[component]
fn CollapsibleNoId() -> Element {
    rsx! {
        Collapsible {
            CollapsibleTrigger { "展开" }
            CollapsibleContent { "内容" }
        }
    }
}

#[test]
fn collapsible_closed_keeps_subtree_mounted() {
    let out = html(CollapsibleClosed);

    assert!(out.contains(r#"data-slot="collapsible""#));
    assert!(out.contains(r#"data-slot="collapsible-trigger""#));
    assert!(out.contains(r#"data-slot="collapsible-content""#));
    assert!(out.contains(r#"data-state="closed""#));
    assert!(out.contains(r#"aria-expanded="false""#));
    assert!(out.contains(r#"aria-controls="sec-content""#));
    assert!(out.contains(r#"id="sec-content""#));

    // 与 Radix 的关键差异：Content 走 `hidden` 而不是 `Presence` 卸载，
    // 所以子树（含内部状态）在收起后仍然存在（见模块文档）。
    assert!(has_hidden(&out), "收起时应带 hidden");
    assert!(out.contains("内容"), "收起后子树应保持挂载");

    // 内层包裹 div 没有自己的 data-state，靠 group-data-[state=..] 读父层
    assert!(out.contains("group-data-[state=closed]/collapsible-content:animate-out"));
}

#[test]
fn collapsible_open_drops_hidden() {
    let out = html(CollapsibleOpen);

    assert!(out.contains(r#"data-state="open""#));
    assert!(out.contains(r#"aria-expanded="true""#));
    assert!(!has_hidden(&out), "展开时不应有 hidden");
    assert!(out.contains("group-data-[state=open]/collapsible-content:animate-in"));
}

#[test]
fn collapsible_without_id_omits_the_attribute() {
    // 发 `id=""` 是无效 HTML，而且 `aria-controls` 会指向不存在的 `-content`
    let out = html(CollapsibleNoId);

    assert!(!out.contains(r#"id="""#), "不应发出空 id");
    assert!(
        !out.contains("aria-controls"),
        "没有 id 就不该有 aria-controls"
    );
    // 组件本身仍然可用
    assert!(out.contains(r#"data-slot="collapsible-trigger""#));
    assert!(out.contains(r#"aria-expanded="false""#));
}

// ---------------------------------------------------------------------------
// Accordion
// ---------------------------------------------------------------------------

#[component]
fn AccordionCase() -> Element {
    rsx! {
        Accordion { default_value: vec!["a".to_string()],
            AccordionItem { value: "a",
                AccordionTrigger { id: "t-a", "第一项" }
                AccordionContent { id: "t-a-content", "第一项内容" }
            }
            AccordionItem { value: "b",
                AccordionTrigger { id: "t-b", "第二项" }
                AccordionContent { id: "t-b-content", "第二项内容" }
            }
        }
    }
}

#[test]
fn accordion_wires_items_and_appends_both_chevrons() {
    let out = html(AccordionCase);

    assert!(out.contains(r#"data-slot="accordion""#));
    assert!(out.contains(r#"data-orientation="vertical""#));
    assert_eq!(out.matches(r#"data-slot="accordion-item""#).count(), 2);

    // `default_value: ["a"]` → 第一项展开、第二项收起
    assert!(out.contains(r#"aria-expanded="true""#));
    assert!(out.contains(r#"aria-expanded="false""#));
    assert!(out.contains(r#"aria-controls="t-a-content""#));

    // 触发器自动追加一上一下两个 lucide chevron，靠 group-aria-expanded 切换显隐
    assert_eq!(
        out.matches(r#"data-slot="accordion-trigger-icon""#).count(),
        4,
        "两个触发器各应带 2 个图标"
    );
    assert!(out.contains("group-aria-expanded/accordion-trigger:hidden"));
    assert!(out.contains("group-aria-expanded/accordion-trigger:inline"));

    // 收起项的内容保持挂载 + hidden（同 Collapsible 的取舍）
    assert!(has_hidden(&out));
    assert!(out.contains("第二项内容"));
    assert!(out.contains(r#"role="region""#));
}

#[component]
fn AccordionHorizontal() -> Element {
    rsx! {
        Accordion { kind: AccordionType::Multiple, orientation: AccordionOrientation::Horizontal,
            AccordionItem { value: "x", AccordionTrigger { "X" } }
        }
    }
}

#[test]
fn accordion_type_and_orientation_are_forwarded() {
    let out = html(AccordionHorizontal);

    // 方向下发给根、item、trigger、content 四处，`data-horizontal:` 才有输入
    assert_eq!(out.matches(r#"data-orientation="horizontal""#).count(), 4);
    assert!(!out.contains(r#"data-orientation="vertical""#));
}

// ---------------------------------------------------------------------------
// RadioGroup
// ---------------------------------------------------------------------------

#[component]
fn RadioCase() -> Element {
    rsx! {
        RadioGroup { default_value: "b",
            RadioGroupItem { value: "a" }
            RadioGroupItem { value: "b" }
        }
    }
}

#[test]
fn radio_group_mounts_dot_only_when_checked() {
    let out = html(RadioCase);

    assert!(out.contains(r#"role="radiogroup""#));
    assert_eq!(out.matches(r#"data-slot="radio-group-item""#).count(), 2);
    assert!(out.contains(r#"data-value="a""#));
    assert!(out.contains(r#"data-value="b""#));

    // Radix 的 Indicator 只在选中时挂载，这里保持一致
    assert_eq!(out.matches(r#"data-slot="radio-group-indicator""#).count(), 1);
    assert!(out.contains(r#"data-state="checked""#));
    assert!(out.contains(r#"data-state="unchecked""#));
    assert!(out.contains(r#"aria-checked="true""#));
    assert!(out.contains(r#"aria-checked="false""#));

    // 圆点是小号实心图标
    assert!(out.contains("h-2.5 w-2.5 fill-current text-current"));

    // roving tabindex：选中项可 Tab 进入，其余靠方向键
    assert!(out.contains(r#"tabindex="0""#));
    assert!(out.contains(r#"tabindex="-1""#));
}

// ---------------------------------------------------------------------------
// ButtonGroup
// ---------------------------------------------------------------------------

#[component]
fn ButtonGroupCase() -> Element {
    rsx! {
        ButtonGroup { orientation: ButtonGroupOrientation::Vertical,
            ButtonGroupText { "文字" }
            ButtonGroupSeparator {}
        }
    }
}

#[test]
fn button_group_forwards_orientation_and_reuses_separator() {
    let out = html(ButtonGroupCase);

    assert!(out.contains(r#"role="group""#));
    assert!(out.contains(r#"data-slot="button-group""#));
    assert!(out.contains(r#"data-orientation="vertical""#));
    assert!(out.contains("flex-col"), "垂直方向要加 flex-col");
    assert!(out.contains(r#"data-slot="button-group-text""#));

    // 分隔器复用 Separator：默认垂直、装饰性（aria-hidden + role=none）
    let sep = tag_with_slot(&out, "button-group-separator");
    assert!(sep.contains(r#"data-orientation="vertical""#));
    assert!(sep.contains(r#"role="none""#));
    assert!(sep.contains(r#"aria-hidden="true""#));
}

// ---------------------------------------------------------------------------
// InputGroup
// ---------------------------------------------------------------------------

#[component]
fn InputGroupCase() -> Element {
    rsx! {
        InputGroup {
            InputGroupAddon { align: InputGroupAddonAlign::BlockEnd,
                InputGroupButton { size: InputGroupButtonSize::IconXs, "x" }
            }
            InputGroupText { "前缀" }
            InputGroupInput { placeholder: "占位" }
        }
    }
}

#[test]
fn input_group_overrides_inner_slots() {
    let out = html(InputGroupCase);

    assert!(out.contains(r#"data-slot="input-group""#));
    assert!(out.contains(r#"data-slot="input-group-text""#));
    assert!(out.contains(r#"data-slot="input-group-addon""#));
    assert!(out.contains(r#"data-align="block-end""#));

    // 内层 input 的 slot 被覆盖成 input-group-control（对应 TS 里 {...props} 的覆盖顺序）
    assert!(out.contains(r#"data-slot="input-group-control""#));
    assert!(
        !out.contains(r#"data-slot="input""#),
        "内层 input 不应保留原始 slot"
    );

    // InputGroupButton 渲染的是裸 button（不委托给 Button），但类名走 button()
    let btn = tag_with_slot(&out, "button");
    assert!(btn.starts_with("<button"));
    assert!(btn.contains(r#"data-size="icon-xs""#));

    assert!(out.contains(r#"placeholder="占位""#));
}

// ---------------------------------------------------------------------------
// ScrollArea
// ---------------------------------------------------------------------------

#[component]
fn ScrollCase() -> Element {
    rsx! {
        ScrollArea { hide_scrollbar: true, class: "h-24", "滚动内容" }
    }
}

#[test]
fn scroll_area_swaps_radix_js_scrollbar_for_native_scrolling() {
    let out = html(ScrollCase);

    assert!(out.contains(r#"data-slot="scroll-area""#));
    let vp = tag_with_slot(&out, "scroll-area-viewport");
    assert!(vp.contains("overflow-auto"), "视口必须真的能滚");
    assert!(vp.contains(r#"tabindex="0""#), "视口要可聚焦以便键盘滚动");

    // 这是本移植最实质的功能性差异：Radix 的 JS 滚动条被换成原生滚动 + 隐藏滚动条
    assert!(vp.contains("[scrollbar-width:none]"));
    assert!(vp.contains("[&::-webkit-scrollbar]:hidden"));
    assert!(out.contains("滚动内容"));
}

// ---------------------------------------------------------------------------
// 浮层：Tooltip / HoverCard / Popover
// ---------------------------------------------------------------------------

#[component]
fn TooltipClosed() -> Element {
    rsx! {
        Tooltip {
            TooltipTrigger { Button { "触发" } }
            TooltipContent { "提示" }
        }
    }
}

#[component]
fn TooltipOpen() -> Element {
    let o = use_signal(|| true);
    rsx! {
        Tooltip { open: Some(o),
            TooltipTrigger { Button { "触发" } }
            TooltipContent { side: Side::Right, align: Align::Start, side_offset: 8.0, "提示" }
        }
    }
}

#[test]
fn tooltip_closed_renders_only_the_anchor() {
    let out = html(TooltipClosed);

    // 锚点包裹层是定位基准（替代 Radix 的 Portal + floating-ui）
    let anchor = tag_with_slot(&out, "tooltip");
    assert!(anchor.contains("relative inline-flex"));
    assert!(out.contains(r#"data-slot="tooltip-trigger""#));

    // 关闭时内容整体不渲染，不会留一个透明浮层挡住底下的点击
    assert!(!out.contains(r#"data-slot="tooltip-content""#));
    assert!(!out.contains("提示"));
}

#[test]
fn tooltip_open_carries_side_align_and_offset() {
    let out = html(TooltipOpen);

    assert!(out.contains(r#"data-slot="tooltip-content""#));
    assert!(out.contains(r#"role="tooltip""#));
    assert!(out.contains(r#"data-state="open""#));
    assert!(out.contains(r#"data-side="right""#));
    assert!(out.contains(r#"data-align="start""#));

    // 没有 floating-ui：位置完全由类名表达（absolute + inset + 偏移）
    let content = tag_with_slot(&out, "tooltip-content");
    assert!(content.contains("absolute"));
    assert!(content.contains("left-full"));
    assert!(content.contains("ml-[8px]"), "side_offset: 8.0 应变成 ml-[8px]");
}

#[component]
fn HoverCardOpen() -> Element {
    let o = use_signal(|| true);
    rsx! {
        HoverCard { open: Some(o),
            HoverCardTrigger { Button { "触发" } }
            HoverCardContent { "悬停卡" }
        }
    }
}

#[test]
fn hover_card_defaults_to_bottom_center() {
    let out = html(HoverCardOpen);

    assert!(out.contains(r#"data-slot="hover-card-content""#));
    // 与 TS 一致：未显式传值时 side=bottom、align=center、sideOffset=4
    assert!(out.contains(r#"data-side="bottom""#));
    assert!(out.contains(r#"data-align="center""#));

    let content = tag_with_slot(&out, "hover-card-content");
    assert!(content.contains("absolute"));
    assert!(content.contains("top-full"));
    assert!(content.contains("left-1/2"));
    assert!(content.contains("-translate-x-1/2"));
    assert!(content.contains("mt-[4px]"));
}

#[component]
fn PopoverOpen() -> Element {
    let o = use_signal(|| true);
    rsx! {
        Popover { open: Some(o),
            PopoverTrigger { Button { "触发" } }
            PopoverAnchor { "锚" }
            PopoverContent {
                PopoverHeader {
                    PopoverTitle { "标题" }
                    PopoverDescription { "描述" }
                }
            }
        }
    }
}

#[test]
fn popover_open_renders_all_parts() {
    let out = html(PopoverOpen);

    for slot in [
        "popover",
        "popover-trigger",
        "popover-anchor",
        "popover-content",
        "popover-header",
        "popover-title",
        "popover-description",
    ] {
        assert!(out.contains(&format!(r#"data-slot="{slot}""#)), "缺少 {slot}");
    }

    assert!(out.contains(r#"data-state="open""#));
    assert!(out.contains("标题"));
    assert!(out.contains("描述"));
    // Popover 比 Tooltip 高一档 z-index
    assert!(tag_with_slot(&out, "popover-content").contains("z-[60]"));
}

// ---------------------------------------------------------------------------
// 浮层：Dialog / AlertDialog
// ---------------------------------------------------------------------------

#[component]
fn DialogClosed() -> Element {
    rsx! {
        Dialog {
            DialogTrigger { "打开" }
            DialogContent { "对话框" }
        }
    }
}

#[component]
fn DialogOpen() -> Element {
    let o = use_signal(|| true);
    rsx! {
        Dialog { open: Some(o),
            DialogTrigger { "打开" }
            DialogContent {
                DialogHeader {
                    DialogTitle { "标题" }
                    DialogDescription { "描述" }
                }
                DialogFooter { DialogClose { "关闭" } }
            }
        }
    }
}

#[test]
fn dialog_closed_renders_only_the_trigger() {
    let out = html(DialogClosed);

    assert!(out.contains(r#"data-slot="dialog-trigger""#));
    assert!(out.contains(r#"data-state="closed""#));
    assert!(!out.contains(r#"data-slot="dialog-content""#));
    assert!(!out.contains(r#"data-slot="dialog-overlay""#));
}

#[test]
fn dialog_open_is_modal_and_fixed_positioned() {
    let out = html(DialogOpen);

    assert!(out.contains(r#"role="dialog""#));
    assert!(out.contains(r#"aria-modal="true""#));
    assert!(out.contains(r#"tabindex="-1""#));

    // 遮罩用 `fixed inset-0`：containing block 是视口本身，
    // 所以不需要 Portal 也能盖满整屏（见 PORTING-UI.md §4.3）
    let overlay = tag_with_slot(&out, "dialog-overlay");
    assert!(overlay.contains("fixed inset-0"));
    assert!(overlay.contains("z-50"));

    for slot in [
        "dialog-content",
        "dialog-header",
        "dialog-title",
        "dialog-description",
        "dialog-footer",
    ] {
        assert!(out.contains(&format!(r#"data-slot="{slot}""#)), "缺少 {slot}");
    }

    // 自动右上角关闭按钮 + Footer 里手写的 DialogClose
    assert_eq!(out.matches(r#"data-slot="dialog-close""#).count(), 2);
    assert!(out.contains(r#"<span class="sr-only">Close</span>"#));

    // 内容面板居中：fixed + top/left 50% + 负位移
    let content = tag_with_slot(&out, "dialog-content");
    assert!(content.contains("fixed"));
    assert!(content.contains("top-1/2") && content.contains("left-1/2"));
    assert!(content.contains("-translate-x-1/2") && content.contains("-translate-y-1/2"));
}

#[component]
fn AlertDialogOpen() -> Element {
    let o = use_signal(|| true);
    rsx! {
        AlertDialog { open: Some(o),
            AlertDialogTrigger { "删除" }
            AlertDialogContent {
                AlertDialogHeader {
                    AlertDialogTitle { "确认？" }
                    AlertDialogDescription { "不可撤销" }
                }
                AlertDialogFooter {
                    AlertDialogCancel { "取消" }
                    AlertDialogAction { "删除" }
                }
            }
        }
    }
}

#[test]
fn alert_dialog_uses_the_alertdialog_role() {
    let out = html(AlertDialogOpen);

    assert!(out.contains(r#"role="alertdialog""#));
    assert!(out.contains(r#"aria-modal="true""#));
    assert!(out.contains(r#"data-slot="alert-dialog-overlay""#));
    assert!(out.contains(r#"data-slot="alert-dialog-content""#));
    assert!(out.contains(r#"data-slot="alert-dialog-header""#));
    assert!(out.contains(r#"data-slot="alert-dialog-title""#));
    assert!(out.contains(r#"data-slot="alert-dialog-description""#));
    assert!(out.contains(r#"data-slot="alert-dialog-footer""#));

    // AlertDialog 没有右上角关闭按钮（强制用户二选一）
    assert!(!out.contains(r#"<span class="sr-only">Close</span>"#));

    // Cancel 默认 outline、Action 默认 primary
    let cancel = tag_with_slot(&out, "alert-dialog-cancel");
    assert!(cancel.contains("border-border"));
    assert!(cancel.contains("hover:border-border-hover"));

    let action = tag_with_slot(&out, "alert-dialog-action");
    assert!(action.contains("bg-primary text-primary-foreground"));
}

// ---------------------------------------------------------------------------
// DropdownMenu
// ---------------------------------------------------------------------------

#[component]
fn DropdownOpen() -> Element {
    let o = use_signal(|| true);
    let s = use_signal(|| true);
    rsx! {
        DropdownMenu { open: Some(o),
            DropdownMenuTrigger { "菜单" }
            DropdownMenuContent {
                DropdownMenuLabel { "标签" }
                DropdownMenuGroup {
                    DropdownMenuItem { variant: DropdownMenuItemVariant::Destructive, "删除"
                        DropdownMenuShortcut { "⌘D" }
                    }
                    DropdownMenuCheckboxItem { checked: true, "勾选" }
                    DropdownMenuRadioItem { checked: true, "单选" }
                }
                DropdownMenuSeparator {}
                DropdownMenuSub { open: Some(s),
                    DropdownMenuSubTrigger { "子菜单" }
                    DropdownMenuSubContent { DropdownMenuItem { "子项" } }
                }
            }
        }
    }
}

#[test]
fn dropdown_menu_open_lays_out_every_item_kind() {
    let out = html(DropdownOpen);

    for slot in [
        "dropdown-menu",
        "dropdown-menu-trigger",
        "dropdown-menu-content",
        "dropdown-menu-label",
        "dropdown-menu-group",
        "dropdown-menu-item",
        "dropdown-menu-checkbox-item",
        "dropdown-menu-checkbox-item-indicator",
        "dropdown-menu-radio-item",
        "dropdown-menu-separator",
        "dropdown-menu-shortcut",
        "dropdown-menu-sub",
        "dropdown-menu-sub-trigger",
        "dropdown-menu-sub-content",
    ] {
        assert!(out.contains(&format!(r#"data-slot="{slot}""#)), "缺少 {slot}");
    }

    // 三种菜单项各用正确的 ARIA role
    assert!(out.contains(r#"role="menu""#));
    assert!(out.contains(r#"role="menuitem""#));
    assert!(out.contains(r#"role="menuitemcheckbox""#));
    assert!(out.contains(r#"role="menuitemradio""#));
    assert!(out.contains(r#"role="separator""#));

    // 高亮态由逐项 Signal 写成 `data-highlighted`，
    // 样式表里的 `data-[highlighted]:bg-menu-hover` 才有输入
    assert!(out.contains("data-[highlighted]:bg-menu-hover"));

    assert!(out.contains(r#"data-variant="destructive""#));
    assert!(out.contains("⌘D"));

    // 子菜单默认从右侧展开
    assert!(out.contains(r#"data-side="right""#));
    assert!(tag_with_slot(&out, "dropdown-menu-sub-content").contains("left-full"));

    // 指示器只在选中时挂载
    assert_eq!(
        out.matches(r#"data-slot="dropdown-menu-checkbox-item-indicator""#)
            .count(),
        1
    );
    assert_eq!(
        out.matches(r#"data-slot="dropdown-menu-radio-item-indicator""#)
            .count(),
        1
    );
}

// ---------------------------------------------------------------------------
// Icons
// ---------------------------------------------------------------------------

#[component]
fn IconsCase() -> Element {
    rsx! {
        IconPlus {}
        IconCheck {}
        IconChevronDown {}
        IconSearch {}
    }
}

#[test]
fn icons_are_inline_lucide_svgs() {
    let out = html(IconsCase);

    assert_eq!(out.matches("<svg").count(), 4);
    assert_eq!(out.matches("</svg>").count(), 4);
    // 全部走 24×24 网格 + currentColor 描边，继承文字颜色
    assert_eq!(out.matches(r#"viewBox="0 0 24 24""#).count(), 4);
    assert_eq!(out.matches(r#"stroke="currentColor""#).count(), 4);
    assert_eq!(out.matches(r#"fill="none""#).count(), 4);

    // data-slot 用图标名，方便 `**:data-[slot=accordion-trigger-icon]` 这类选择器命中
    for name in ["plus", "check", "chevron-down", "search"] {
        assert!(
            out.contains(&format!(r#"data-slot="{name}""#)),
            "缺少图标 {name}"
        );
    }

    // 几何是照抄 lucide 的 path，不是占位方块
    assert!(out.contains(r#"<path d="M20 6 9 17l-5-5">"#));
    assert!(out.contains(r#"<circle cx="11" cy="11" r="8">"#));
    assert!(out.contains(r#"<path d="M5 12h14">"#));
}
