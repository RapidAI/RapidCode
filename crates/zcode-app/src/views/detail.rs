//! 右侧详情面板：执行计划、**真实变更**、上下文、以及一批设置项。
//!
//! 这一块刻意把「表单类」原语都过一遍：`Collapsible` / `Accordion` / `Switch` /
//! `RadioGroup` / `Checkbox` / `Progress`（分段）/ `AvatarGroup` / `ButtonGroup` /
//! `Kbd` / `Separator` / `Tooltip`。
//!
//! 面板里只有「变更文件」那一块接的是**真实数据**（`zcode-services` 的 git 查询）；
//! 执行计划、工具调用、上下文来源仍是演示数据，且各自标注了这一点。

use crate::state::{use_app, ApprovalMode, Model};
use crate::workspace::section_label;
use dioxus::prelude::*;
use zcode_ui::*;

use super::ChangeRow;

/// 详情面板。
#[component]
pub fn DetailPanel() -> Element {
    let ctx = use_app();
    let state = ctx.read();

    rsx! {
        aside { class: "flex w-[304px] shrink-0 flex-col border-l border-border bg-background-alt",
            div { class: "flex h-12 shrink-0 items-center gap-2 border-b border-border px-3",
                IconLayers { class: "size-3.5 text-foreground-subtle" }
                span { class: "text-ui-base font-medium", "详情" }
                Badge { variant: BadgeVariant::Ghost, class: "text-ui-xs", "只读" }
                Tooltip {
                    class: "ml-auto",
                    TooltipTrigger {
                        Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconXs,
                            onclick: move |_| {
                                let mut d = ctx.detail_open;
                                d.set(false);
                            },
                            IconX {}
                        }
                    }
                    TooltipContent { side: Side::Bottom, align: Align::End, side_offset: 6.0,
                        "关闭面板"
                    }
                }
            }

            ScrollArea { class: "min-h-0 flex-1", hide_scrollbar: true,
                div { class: "flex flex-col gap-3 p-3",

                    PlanSection {}

                    Separator {}

                    Accordion { kind: AccordionType::Single,
                        AccordionItem { value: "changes",
                            AccordionTrigger { "变更文件" }
                            AccordionContent { ChangesSection {} }
                        }
                        AccordionItem { value: "tools",
                            AccordionTrigger { "工具调用" }
                            AccordionContent {
                                div { class: "flex flex-col gap-2",
                                    ToolRow { name: "cargo test", detail: "871 passed", ok: true }
                                    ToolRow { name: "cargo build", detail: "desktop · 16.6s", ok: true }
                                    ToolRow { name: "tailwindcss", detail: "app.css · 114 KB", ok: true }
                                    DemoHint { text: "演示数据：工具调用流依赖尚未移植的会话存储。" }
                                }
                            }
                        }
                        AccordionItem { value: "context",
                            AccordionTrigger { "上下文来源" }
                            AccordionContent {
                                div { class: "flex flex-col gap-2",
                                    for src in [
                                        "packages/ui/src/styles.css（rust 分支）",
                                        "packages/ui/src/components/ui/*.tsx（rust 分支）",
                                        "PORTING-UI.md",
                                    ] {
                                        div { key: "{src}", class: "flex items-center gap-2",
                                            IconFileText { class: "size-3 shrink-0 text-foreground-subtlest" }
                                            span { class: "min-w-0 flex-1 truncate text-ui-xs text-foreground-subtle",
                                                {src}
                                            }
                                            Checkbox { checked: CheckboxState::Checked }
                                        }
                                    }
                                    DemoHint { text: "演示数据：真实上下文索引依赖尚未移植的 RAG 链路。" }
                                }
                            }
                        }
                    }

                    Separator {}

                    // 开关组
                    div { class: "flex flex-col gap-2",
                        span { class: "text-ui-xs font-medium text-foreground-subtlest",
                            "显示"
                        }
                        SwitchRow {
                            label: "显示推理过程",
                            hint: "折叠思考片段",
                            checked: state.show_reasoning,
                            on_change: move |v: bool| {
                                let mut s = ctx.state;
                                s.write().show_reasoning = v;
                            },
                        }
                        SwitchRow {
                            label: "自动携带上下文",
                            hint: "引用当前打开的文件",
                            checked: state.auto_context,
                            on_change: move |v: bool| {
                                let mut s = ctx.state;
                                s.write().auto_context = v;
                            },
                        }
                        SwitchRow {
                            label: "匿名使用统计",
                            hint: "仅发送计数，不含代码",
                            checked: state.telemetry,
                            on_change: move |v: bool| {
                                let mut s = ctx.state;
                                s.write().telemetry = v;
                            },
                        }
                    }

                    Separator {}

                    // 权限模式：单选组（受控：value 由 AppCtx 持有）
                    div { class: "flex flex-col gap-2",
                        span { class: "text-ui-xs font-medium text-foreground-subtlest",
                            "权限模式"
                        }
                        RadioGroup {
                            value: Some(ctx.approval_value),
                            on_value_change: move |v: String| {
                                let mut s = ctx.state;
                                s.write().approval = ApprovalMode::ALL
                                    .into_iter()
                                    .find(|m| m.as_str() == v)
                                    .unwrap_or(ApprovalMode::Ask);
                            },
                            for m in ApprovalMode::ALL {
                                div {
                                    key: "{m.as_str()}",
                                    class: "flex cursor-default items-center gap-2",
                                    // 整行可点：点文字也切到这一项（RadioGroupItem 自己也会设同一个 signal）
                                    onclick: move |_| {
                                        let mut v = ctx.approval_value;
                                        v.set(m.as_str().to_string());
                                        let mut s = ctx.state;
                                        s.write().approval = m;
                                    },
                                    RadioGroupItem { value: m.as_str() }
                                    div { class: "flex flex-col",
                                        span { class: "text-ui-base/relaxed", {m.label()} }
                                        span { class: "text-ui-xs text-foreground-subtlest",
                                            {m.hint()}
                                        }
                                    }
                                }
                            }
                        }
                    }

                    Separator {}

                    // 分段进度
                    div { class: "flex flex-col gap-2",
                        div { class: "flex items-center justify-between",
                            span { class: "text-ui-xs font-medium text-foreground-subtlest",
                                "上下文占用"
                            }
                            span { class: "text-ui-xs text-foreground-subtle",
                                "{state.token_used as i64} / {state.token_limit as i64}"
                            }
                        }
                        Progress {
                            value: state.token_used / state.token_limit * 100.0,
                            segments: Some(vec![
                                ProgressSegment::new("system", 0.08).with_class("bg-muted-foreground/50"),
                                ProgressSegment::new("code", 0.44).with_class("bg-primary"),
                                ProgressSegment::new("history", 0.12).with_class("bg-secondary-foreground/40"),
                                ProgressSegment::new("free", 0.36).with_class("bg-transparent"),
                            ]),
                        }
                        div { class: "flex items-center gap-2",
                            LegendDot { class: "bg-muted-foreground/50", label: "系统提示" }
                            LegendDot { class: "bg-primary", label: "代码" }
                            LegendDot { class: "bg-secondary-foreground/40", label: "历史" }
                        }
                    }

                    Separator {}

                    // 头像组 + 按钮组
                    div { class: "flex flex-col gap-2",
                        span { class: "text-ui-xs font-medium text-foreground-subtlest",
                            "参与者"
                        }
                        div { class: "flex items-center gap-2",
                            AvatarGroup {
                                Avatar { AvatarFallback { "ZC" } }
                                Avatar { AvatarFallback { "M" } }
                                Avatar { AvatarFallback { "R" } }
                                AvatarGroupCount { "+3" }
                            }
                            ButtonGroup { class: "ml-auto",
                                Button { variant: ButtonVariant::Outline, size: ButtonSize::IconXs,
                                    IconMinus {}
                                }
                                Button { variant: ButtonVariant::Outline, size: ButtonSize::IconXs,
                                    IconPlus {}
                                }
                                ButtonGroupSeparator {}
                                Button { variant: ButtonVariant::Outline, size: ButtonSize::IconXs,
                                    IconSettings {}
                                }
                            }
                        }
                    }

                    Separator {}

                    // 模型信息
                    div { class: "flex flex-col gap-1.5",
                        span { class: "text-ui-xs font-medium text-foreground-subtlest",
                            "模型"
                        }
                        for m in Model::ALL {
                            div { key: "{m.as_str()}",
                                class: cn(&[
                                    "flex items-center gap-2 rounded-md px-2 py-1",
                                    if m == state.model { "bg-muted" } else { "" },
                                ]),
                                IconSparkles { class: "size-3 text-foreground-subtlest" }
                                span { class: "flex-1 text-ui-xs text-foreground-subtle",
                                    {m.label()}
                                }
                                if m == state.model {
                                    IconCheck { class: "size-3 text-primary" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// 详情面板里的「变更文件」。
///
/// 数据来自 [`crate::workspace`]（真实 git 查询），按 staged / unstaged /
/// untracked / conflicted 分组展示；每种异常状态都有明确文案。
#[component]
fn ChangesSection() -> Element {
    let ctx = use_app();
    let data = ctx.workspace.read().clone();
    let status = ctx.workspace_status.read().clone();

    if let Some(message) = status.failure() {
        return rsx! {
            div { class: "flex items-start gap-2",
                IconTriangleAlert { class: "mt-0.5 size-3 shrink-0 text-destructive" }
                span { class: "break-all text-ui-xs text-foreground-subtle",
                    {message.to_string()}
                }
            }
        };
    }

    let Some(data) = data else {
        return rsx! { PanelHint { text: "正在读取工作区…" } };
    };

    if !data.is_repository() {
        return rsx! { PanelHint { text: "当前目录不是 Git 仓库".to_string() } };
    }

    if data.changes.is_empty() {
        return rsx! {
            PanelHint { text: format!("{} 工作区干净", data.branch_label()) }
        };
    }

    let totals = format!(
        "{} 个文件 · +{} -{}",
        data.totals.file_count, data.totals.added, data.totals.removed
    );

    rsx! {
        div { class: "flex flex-col gap-2",
            span { class: "text-ui-xs text-foreground-subtlest", {totals} }
            for section in data.populated_sections() {
                div { key: "{section.as_str()}", class: "flex flex-col gap-1",
                    div { class: "flex items-center gap-1.5",
                        span { class: "text-ui-xs font-medium text-foreground-subtle",
                            {section_label(section)}
                        }
                        span { class: "text-ui-xs text-foreground-subtlest",
                            "{data.changes_in(section).len()}"
                        }
                    }
                    for change in data.changes_in(section) {
                        ChangeRow {
                            key: "{change.repo_relative_path}",
                            change: change.clone(),
                            compact: true,
                        }
                    }
                }
            }
        }
    }
}

/// 面板里的一句提示。
#[component]
fn PanelHint(text: String) -> Element {
    rsx! {
        span { class: "text-ui-xs text-foreground-subtlest", {text} }
    }
}

/// 「这块是演示数据」的显式标注。
///
/// 不标出来，读者会把整页都当成已移植的功能 —— 这一轮就是被这个误读坑过一次。
#[component]
fn DemoHint(text: &'static str) -> Element {
    rsx! {
        div { class: "flex items-start gap-1.5 rounded-md bg-muted/50 px-2 py-1",
            IconInfo { class: "mt-0.5 size-3 shrink-0 text-foreground-subtlest" }
            span { class: "text-ui-xs text-foreground-subtlest", {text} }
        }
    }
}

/// 「执行计划」折叠区（受控用法）。
#[component]
fn PlanSection() -> Element {
    let ctx = use_app();
    let open = ctx.plan_open;

    rsx! {
        Collapsible { open: Some(open), class: "flex flex-col gap-1",
            CollapsibleTrigger {
                class: "flex w-full items-center gap-2 rounded-md px-1 py-0.5 text-left",
                if open() {
                    IconChevronDown { class: "size-3 text-foreground-subtlest" }
                } else {
                    IconChevronRight { class: "size-3 text-foreground-subtlest" }
                }
                span { class: "text-ui-xs font-medium text-foreground-subtlest",
                    "执行计划"
                }
                Badge { variant: BadgeVariant::Secondary, class: "ml-auto text-ui-xs",
                    "4 / 5"
                }
            }
            CollapsibleContent {
                div { class: "flex flex-col gap-1.5 pt-1",
                    PlanStep { done: true, text: "补齐第二批原语（无浮层）" }
                    PlanStep { done: true, text: "浮层类改用纯 CSS 自管理定位" }
                    PlanStep { done: true, text: "生成并内嵌 Tailwind 样式表" }
                    PlanStep { done: true, text: "接入真实 git 工作区状态" }
                    PlanStep { done: false, text: "跑通桌面窗口（受 WebView2 限制）" }
                }
            }
        }
    }
}

/// 计划中的一步。
#[component]
fn PlanStep(done: bool, text: &'static str) -> Element {
    rsx! {
        div { class: "flex items-start gap-2",
            if done {
                IconCheck { class: "mt-0.5 size-3 shrink-0 text-primary" }
            } else {
                IconCircle { class: "mt-0.5 size-3 shrink-0 text-foreground-subtlest" }
            }
            span {
                class: cn(&[
                    "text-ui-xs",
                    if done { "text-foreground-subtle line-through" } else { "text-foreground" },
                ]),
                {text}
            }
        }
    }
}

/// 一行开关。
#[component]
fn SwitchRow(
    label: &'static str,
    hint: &'static str,
    checked: bool,
    on_change: EventHandler<bool>,
) -> Element {
    rsx! {
        div { class: "flex items-start gap-2",
            div { class: "flex flex-1 flex-col",
                span { class: "text-ui-base/relaxed", {label} }
                span { class: "text-ui-xs text-foreground-subtlest", {hint} }
            }
            Switch {
                checked,
                class: "mt-0.5",
                on_checked_change: move |v| on_change.call(v),
            }
        }
    }
}

/// 一行工具调用。
#[component]
fn ToolRow(name: &'static str, detail: &'static str, ok: bool) -> Element {
    rsx! {
        div { class: "flex items-center gap-2",
            IconTerminal { class: "size-3 shrink-0 text-foreground-subtlest" }
            span { class: "text-ui-xs text-foreground", {name} }
            span { class: "ml-auto text-ui-xs text-foreground-subtlest", {detail} }
            if ok {
                IconCheck { class: "size-3 text-primary" }
            }
        }
    }
}

/// 图例小圆点。
#[component]
fn LegendDot(class: &'static str, label: &'static str) -> Element {
    rsx! {
        span { class: "flex items-center gap-1",
            span { class: cn(&["size-2 rounded-full", class]) }
            span { class: "text-ui-xs text-foreground-subtlest", {label} }
        }
    }
}
