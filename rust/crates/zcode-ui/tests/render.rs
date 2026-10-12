//! 端到端「无渲染器」冒烟测试。
//!
//! 这个 crate 关掉了 Dioxus 的渲染器 feature（web / desktop / native 全关），
//! 所以 `cargo test` 不需要 wasm、不需要窗口、也不需要浏览器。
//!
//! 做法：实现一个**记录型渲染器**（`WriteMutations`），把 Dioxus 下发的
//! 「加载模板」（含编译期静态属性）与「写入属性」（运行期动态属性）全部收集起来，
//! 于是可以在纯 host 环境下断言「DOM 长什么样」：
//!
//! - 组件真的能挂载（`rebuild_in_place` 不 panic）；
//! - `data-slot` / `data-state` / `data-orientation` / `role` 等语义锚点确实输出；
//! - variant / size 真的拼进了 `class`；
//! - SVG（Spinner / Checkbox 图标）走了 SVG 命名空间且 `viewBox` 正确；
//! - 文本节点（含 `children` 透传）真的到达了渲染层。
//!
//! 注意：`TemplateNode::Dynamic*` 的位置在模板里只是占位符，真正的值由
//! `set_attribute` / `create_text_node` 送达 —— 因此断言要同时查这两处。

use dioxus::prelude::dioxus_core::{
    AttributeValue, ElementId, Template, TemplateAttribute, TemplateNode, VirtualDom,
    WriteMutations,
};
use dioxus::prelude::*;
use zcode_ui::*;

// ---------------------------------------------------------------------------
// 记录型渲染器
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Recorder {
    /// 模板里出现的所有元素标签（含静态属性）。
    static_elements: Vec<(String, Vec<(String, String)>)>,
    /// 运行期写入的属性。
    dynamic_attrs: Vec<(String, String)>,
    /// 文本节点。
    texts: Vec<String>,
}

impl Recorder {
    /// 全部属性（静态 + 动态），按 `(名字, 值)`。
    fn attrs(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = self
            .static_elements
            .iter()
            .flat_map(|(_, attrs)| attrs.iter().cloned())
            .collect();
        out.extend(self.dynamic_attrs.iter().cloned());
        out
    }

    /// 所有元素标签。
    fn tags(&self) -> Vec<String> {
        self.static_elements.iter().map(|(t, _)| t.clone()).collect()
    }

    /// 是否存在某个标签的元素。
    fn has_tag(&self, tag: &str) -> bool {
        self.tags().iter().any(|t| t == tag)
    }

    /// 是否存在带指定 `data-slot` 的元素。
    fn has_slot(&self, slot: &str) -> bool {
        self.attrs()
            .iter()
            .any(|(n, v)| n == "data-slot" && v == slot)
    }

    /// 某个属性名对应的所有取值。
    fn values_of(&self, name: &str) -> Vec<String> {
        self.attrs()
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
            .collect()
    }

    /// 是否存在「属性名 = 属性值」。
    fn has_attr(&self, name: &str, value: &str) -> bool {
        self.attrs().iter().any(|(n, v)| n == name && v == value)
    }

    /// 是否存在某个 `class` 值包含指定 token。
    fn class_contains(&self, needle: &str) -> bool {
        self.values_of("class").iter().any(|c| c.contains(needle))
    }

    /// 是否存在某个 `style` 值包含指定片段。
    fn style_contains(&self, needle: &str) -> bool {
        self.values_of("style").iter().any(|c| c.contains(needle))
    }

    fn has_text(&self, text: &str) -> bool {
        self.texts.iter().any(|t| t == text)
    }
}

/// 递归收集模板里的静态元素、静态属性与**静态文本**。
///
/// 关键点：`rsx!` 里的字面量文本（含 `Button { "删除" }` 这种作为 children 的
/// 字面量）会被编译进模板，走 `TemplateNode::Text` 而不是 `create_text_node`。
/// 只有运行期才知道值的文本才会走 mutation。
fn walk_template(
    node: &TemplateNode,
    elements: &mut Vec<(String, Vec<(String, String)>)>,
    texts: &mut Vec<String>,
) {
    match node {
        TemplateNode::Element {
            tag,
            attrs,
            children,
            ..
        } => {
            let mut collected = Vec::new();
            for attr in attrs.iter() {
                if let TemplateAttribute::Static { name, value, .. } = attr {
                    collected.push(((*name).to_string(), (*value).to_string()));
                }
            }
            elements.push(((*tag).to_string(), collected));
            for child in children.iter() {
                walk_template(child, elements, texts);
            }
        }
        TemplateNode::Text { text } => texts.push((*text).to_string()),
        TemplateNode::Dynamic { .. } => {}
    }
}

fn value_to_string(value: &AttributeValue) -> String {
    match value {
        AttributeValue::Text(t) => t.clone(),
        AttributeValue::Float(f) => f.to_string(),
        AttributeValue::Int(i) => i.to_string(),
        AttributeValue::Bool(b) => b.to_string(),
        AttributeValue::None => String::new(),
        AttributeValue::Listener(_) => "<listener>".to_string(),
        AttributeValue::Any(_) => "<any>".to_string(),
    }
}

impl WriteMutations for Recorder {
    fn append_children(&mut self, _: ElementId, _: usize) {}
    fn assign_node_id(&mut self, _: &'static [u8], _: ElementId) {}
    fn create_placeholder(&mut self, _: ElementId) {}

    fn create_text_node(&mut self, value: &str, _: ElementId) {
        self.texts.push(value.to_string());
    }

    fn load_template(&mut self, template: Template, _index: usize, _id: ElementId) {
        for root in template.roots.iter() {
            walk_template(root, &mut self.static_elements, &mut self.texts);
        }
    }

    fn replace_node_with(&mut self, _: ElementId, _: usize) {}
    fn replace_placeholder_with_nodes(&mut self, _: &'static [u8], _: usize) {}
    fn insert_nodes_after(&mut self, _: ElementId, _: usize) {}
    fn insert_nodes_before(&mut self, _: ElementId, _: usize) {}

    fn set_attribute(
        &mut self,
        name: &'static str,
        _ns: Option<&'static str>,
        value: &AttributeValue,
        _id: ElementId,
    ) {
        self.dynamic_attrs
            .push((name.to_string(), value_to_string(value)));
    }

    fn set_node_text(&mut self, value: &str, _: ElementId) {
        self.texts.push(value.to_string());
    }

    fn create_event_listener(&mut self, _: &'static str, _: ElementId) {}
    fn remove_event_listener(&mut self, _: &'static str, _: ElementId) {}
    fn remove_node(&mut self, _: ElementId) {}
    fn push_root(&mut self, _: ElementId) {}
}

/// 渲染一个组件树并返回记录结果。
///
/// 注意：不能用 `rebuild_in_place()` —— 它等价于 `rebuild(&mut NoOpMutations)`，
/// 初始挂载的全部 mutation 都会被丢进空实现里，什么也记录不到。
fn render(app: fn() -> Element) -> Recorder {
    let mut vdom = VirtualDom::new(app);
    let mut rec = Recorder::default();
    vdom.rebuild(&mut rec);
    rec
}

// ---------------------------------------------------------------------------
// 被测组件树：把所有已移植的原语串起来
// ---------------------------------------------------------------------------

#[component]
fn Showcase() -> Element {
    let tab = use_signal(|| "general".to_string());
    let mut on = use_signal(|| true);
    let mut count = use_signal(|| 0);

    rsx! {
        div { class: "app",
            Button {
                variant: ButtonVariant::Destructive,
                size: ButtonSize::Lg,
                class: "w-full",
                onclick: move |_| count += 1,
                "删除"
            }
            Button { variant: ButtonVariant::Outline, disabled: true, "取消" }

            Badge { variant: BadgeVariant::Secondary, "v3.15.1" }

            Card { size: CardSize::Sm,
                CardHeader {
                    CardTitle { "标题" }
                    CardDescription { "描述" }
                    CardAction { Button { size: ButtonSize::IconSm, "×" } }
                }
                CardContent { "内容" }
                CardFooter { "页脚" }
            }

            Alert { variant: AlertVariant::Warning,
                AlertTitle { "注意" }
                AlertDescription { "这是一条警告" }
                AlertAction { Button { variant: ButtonVariant::Ghost, "关闭" } }
            }

            Input { size: InputSize::Sm, placeholder: "搜索", disabled: false }
            Textarea { placeholder: "描述", rows: 4 }
            Label { html_for: "search", "标签" }

            Separator {}
            Progress { value: 42.0 }
            Progress {
                value: 60.0,
                segments: vec![
                    ProgressSegment::new("a", 0.3).with_class("bg-primary"),
                    ProgressSegment::new("b", 0.7),
                ],
            }

            KbdGroup {
                Kbd { "⌘" }
                Kbd { "K" }
            }

            Spinner { class: "text-muted-foreground" }

            Avatar { size: AvatarSize::Lg,
                AvatarImage { src: "https://example.com/a.png", alt: "用户" }
                AvatarFallback { "A" }
                AvatarBadge {}
            }

            AvatarGroup {
                Avatar { AvatarFallback { "B" } }
                AvatarGroupCount { "+3" }
            }

            Switch {
                checked: *on.read(),
                on_checked_change: move |v| on.set(v),
            }

            Checkbox {
                checked: CheckboxState::Indeterminate,
                check_icon_stroke_width: Some(3.0),
                on_checked_change: move |_| {},
            }

            Tabs { value: tab,
                TabsList { variant: TabsListVariant::Line,
                    TabsTrigger { value: "general", "常规" }
                    TabsTrigger { value: "advanced", disabled: true, "高级" }
                }
                TabsContent { value: "general", "常规内容" }
                TabsContent { value: "advanced", "高级内容" }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 断言
// ---------------------------------------------------------------------------

#[test]
fn component_tree_mounts_without_renderer() {
    // 只要能跑完不 panic，就说明所有组件的 props 绑定、信号读取、
    // context 提供/消费（Tabs）都是合法的。
    let rec = render(Showcase);
    assert!(!rec.static_elements.is_empty(), "没有加载到任何模板");
    assert!(rec.texts.len() > 3, "文本节点太少：{:?}", rec.texts);
}

#[test]
fn html_tags_are_used() {
    let rec = render(Showcase);
    for tag in [
        "div", "button", "span", "input", "textarea", "label", "kbd", "img", "svg",
    ] {
        assert!(rec.has_tag(tag), "缺少 <{tag}> 元素");
    }
    // Spinner / Checkbox 图标需要 SVG 子元素
    assert!(rec.has_tag("line"), "Spinner 缺少 <line>");
    assert!(rec.has_tag("path"), "Checkbox 缺少 <path>");
}

#[test]
fn data_slot_anchors_are_emitted() {
    let rec = render(Showcase);
    for slot in [
        "button",
        "badge",
        "card",
        "card-header",
        "card-title",
        "card-description",
        "card-action",
        "card-content",
        "card-footer",
        "alert",
        "alert-title",
        "alert-description",
        "alert-action",
        "input",
        "textarea",
        "label",
        "separator",
        "progress",
        "progress-indicator",
        "kbd",
        "kbd-group",
        "spinner",
        "avatar",
        "avatar-image",
        "avatar-fallback",
        "avatar-badge",
        "avatar-group",
        "avatar-group-count",
        "switch",
        "switch-thumb",
        "checkbox",
        "checkbox-indicator",
        "checkbox-checked-icon",
        "checkbox-indeterminate-icon",
        "tabs",
        "tabs-list",
        "tabs-trigger",
        "tabs-content",
    ] {
        assert!(rec.has_slot(slot), "缺少 data-slot=\"{slot}\"");
    }
}

#[test]
fn variants_reach_the_class_attribute() {
    let rec = render(Showcase);
    // Button variant / size
    assert!(rec.class_contains("bg-destructive"), "Destructive 变体没进 class");
    assert!(rec.class_contains("h-8"), "Lg 尺寸没进 class");
    assert!(rec.class_contains("w-full"), "用户 class 被吞了");
    // Badge variant
    assert!(rec.class_contains("bg-secondary"));
    // Alert variant
    assert!(rec.class_contains("bg-warning"));
    // Input size
    assert!(rec.class_contains("file:h-5"), "Input Sm 尺寸没进 class");
    // TabsList line variant
    assert!(rec.class_contains("gap-1 bg-transparent"));
    // Card base
    assert!(rec.class_contains("rounded-xl"));
    // 用户 class 覆盖组件默认（tailwind-merge 语义）
    let rec2 = render(|| {
        rsx! { Button { size: ButtonSize::Default, class: "h-9", "x" } }
    });
    assert!(rec2.class_contains("h-9"));
    assert!(!rec2.values_of("class").iter().any(|c| c
        .split_whitespace()
        .any(|t| t == "h-7")));
}

#[test]
fn state_and_orientation_attributes_are_emitted() {
    let rec = render(Showcase);
    // Checkbox：半选
    assert!(rec.has_attr("data-state", "indeterminate"));
    assert!(rec.has_attr("aria-checked", "mixed"));
    // Switch：选中
    assert!(rec.has_attr("data-state", "checked"));
    assert!(rec.has_attr("role", "switch"));
    // Tabs
    assert!(rec.has_attr("data-orientation", "horizontal"));
    assert!(rec.has_attr("data-variant", "line"));
    // TabsTrigger 当前激活项
    assert!(rec.has_attr("data-state", "active"));
    assert!(rec.has_attr("aria-selected", "true"));
    assert!(rec.has_attr("aria-selected", "false"));
    // Separator
    assert!(rec.has_attr("data-orientation", "horizontal"));
    assert!(rec.has_attr("role", "none"));
    // Progress 无障碍
    assert!(rec.has_attr("role", "progressbar"));
    assert!(rec.has_attr("aria-valuenow", "42"));
    // Spinner
    assert!(rec.has_attr("role", "status"));
}

#[test]
fn svg_attributes_are_emitted() {
    let rec = render(Showcase);
    // Spinner 走 lucide `loader` 的 viewBox
    assert!(rec.has_attr("viewBox", "0 0 24 24"), "缺少 viewBox");
    assert!(rec.has_attr("stroke", "currentColor"));
    assert!(rec.has_attr("stroke-linecap", "round"));
    // Checkbox 指定了 checkIconStrokeWidth
    assert!(rec.has_attr("stroke-width", "3"));
}

#[test]
fn progress_widths_and_segments_are_emitted() {
    let rec = render(Showcase);
    assert!(rec.style_contains("width: 42%"), "value=42 未转成宽度");
    assert!(rec.style_contains("width: 60%"), "value=60 未转成宽度");
    assert!(rec.style_contains("flex-basis: 30%"), "分段 a 未转成 flex-basis");
    assert!(rec.style_contains("flex-basis: 70%"), "分段 b 未转成 flex-basis");
}

#[test]
fn children_text_is_forwarded() {
    let rec = render(Showcase);
    for text in [
        "删除", "取消", "v3.15.1", "标题", "描述", "内容", "页脚", "注意", "这是一条警告",
        "关闭", "标签", "⌘", "K", "A", "B", "+3", "常规", "高级", "常规内容", "高级内容",
    ] {
        assert!(rec.has_text(text), "文本 {text:?} 没有到达渲染层：{:?}", rec.texts);
    }
}

#[test]
fn controlled_props_are_reflected_in_the_dom() {
    // 受控组件的静态取值应当直接出现在 DOM 上
    let rec = render(|| {
        rsx! {
            Switch { checked: false }
            Checkbox { checked: CheckboxState::Checked }
        }
    });
    assert!(rec.has_attr("data-state", "unchecked"));
    assert!(rec.has_attr("aria-checked", "false"));
    assert!(rec.has_attr("data-state", "checked"));
    assert!(rec.has_attr("aria-checked", "true"));
}

#[test]
fn avatar_image_disappears_when_no_src_needed() {
    // 只渲染 fallback：不应出现 img
    let rec = render(|| {
        rsx! { Avatar { AvatarFallback { "Z" } } }
    });
    assert!(!rec.has_tag("img"));
    assert!(rec.has_slot("avatar-fallback"));
}

#[test]
fn inactive_tab_panel_is_hidden_not_unmounted() {
    // 非激活面板保持挂载（hidden）—— 这是与 Radix 默认卸载的有意差异
    let rec = render(Showcase);
    assert!(rec.has_attr("hidden", "true"), "非激活面板没有被 hidden");
    assert!(rec.has_text("常规内容"));
    assert!(rec.has_text("高级内容"));
    // 激活面板不应带 hidden
    let hidden_count = rec
        .values_of("hidden")
        .iter()
        .filter(|v| v.as_str() == "true")
        .count();
    assert_eq!(hidden_count, 1, "应当恰好有一个面板被 hidden");
}
