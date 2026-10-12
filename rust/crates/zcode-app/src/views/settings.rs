//! 设置对话框与删除确认框。
//!
//! 用到的原语：`Dialog`（含 `DialogHeader/Title/Description/Footer`）/ `Tabs` /
//! `Switch` / `RadioGroup` / `Checkbox` / `Separator` / `Kbd` / `ButtonGroup` /
//! `AlertDialog`（含 `Action` / `Cancel`）/ `Alert`。

use crate::state::{use_app, ApprovalMode, Model};
use dioxus::prelude::*;
use zcode_ui::*;

/// 设置对话框。
#[component]
pub fn SettingsDialog() -> Element {
    let ctx = use_app();

    rsx! {
        Dialog { open: Some(ctx.settings_open),
            DialogContent { class: "max-w-2xl",
                DialogHeader {
                    DialogTitle { "设置" }
                    DialogDescription {
                        "这些开关只影响本机界面。所有值都保存在内存里 —— 本移植没有做持久化。"
                    }
                }

                Tabs { value: ctx.settings_tab,
                    TabsList {
                        TabsTrigger { value: "general", "常规" }
                        TabsTrigger { value: "appearance", "外观" }
                        TabsTrigger { value: "about", "关于" }
                    }

                    TabsContent { value: "general", class: "pt-3",
                        GeneralTab {}
                    }
                    TabsContent { value: "appearance", class: "pt-3",
                        AppearanceTab {}
                    }
                    TabsContent { value: "about", class: "pt-3",
                        AboutTab {}
                    }
                }

                DialogFooter {
                    Button { variant: ButtonVariant::Ghost, size: ButtonSize::Sm,
                        "恢复默认"
                    }
                    Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm,
                        onclick: move |_| {
                            let mut o = ctx.settings_open;
                            o.set(false);
                        },
                        "取消"
                    }
                    Button { variant: ButtonVariant::Default, size: ButtonSize::Sm,
                        onclick: move |_| {
                            let mut o = ctx.settings_open;
                            o.set(false);
                        },
                        "保存"
                        KbdGroup { class: "ml-1",
                            Kbd { "⌘" }
                            Kbd { "S" }
                        }
                    }
                }
            }
        }
    }
}

/// 常规页。
#[component]
fn GeneralTab() -> Element {
    let ctx = use_app();
    let state = ctx.read();

    rsx! {
        div { class: "flex flex-col gap-3",
            SettingRow {
                title: "显示推理过程",
                desc: "把模型的思考片段折叠显示在回答上方",
                Switch {
                    checked: state.show_reasoning,
                    on_checked_change: move |v| {
                        let mut s = ctx.state;
                        s.write().show_reasoning = v;
                    },
                }
            }
            Separator {}
            SettingRow {
                title: "自动携带上下文",
                desc: "自动把当前打开的文件加入上下文",
                Switch {
                    checked: state.auto_context,
                    on_checked_change: move |v| {
                        let mut s = ctx.state;
                        s.write().auto_context = v;
                    },
                }
            }
            Separator {}
            SettingRow {
                title: "匿名使用统计",
                desc: "只上报计数，不包含代码内容",
                Switch {
                    checked: state.telemetry,
                    on_checked_change: move |v| {
                        let mut s = ctx.state;
                        s.write().telemetry = v;
                    },
                }
            }
            Separator {}
            div { class: "flex flex-col gap-2",
                Label { "默认模型" }
                ButtonGroup { class: "self-start",
                    for m in Model::ALL {
                        Button {
                            key: "{m.as_str()}",
                            variant: if m == state.model {
                                ButtonVariant::Default
                            } else {
                                ButtonVariant::Outline
                            },
                            size: ButtonSize::Sm,
                            onclick: move |_| {
                                let mut s = ctx.state;
                                s.write().model = m;
                            },
                            {m.label()}
                        }
                    }
                }
            }
            Separator {}
            div { class: "flex flex-col gap-2",
                Label { "权限模式" }
                RadioGroup {
                    value: Some(ctx.approval_value),
                    class: "grid-cols-3",
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
                            class: "flex items-center gap-2",
                            onclick: move |_| {
                                let mut v = ctx.approval_value;
                                v.set(m.as_str().to_string());
                                let mut s = ctx.state;
                                s.write().approval = m;
                            },
                            RadioGroupItem { value: m.as_str() }
                            span { class: "text-ui-base/relaxed", {m.label()} }
                        }
                    }
                }
            }
        }
    }
}

/// 外观页。
#[component]
fn AppearanceTab() -> Element {
    let ctx = use_app();
    let state = ctx.read();
    let mut checkbox_compact = use_signal(|| true);

    rsx! {
        div { class: "flex flex-col gap-3",
            SettingRow {
                title: "深色主题",
                desc: "切换 .dark 类；所有颜色令牌在 styles.css 的 .dark 块里覆盖",
                Switch {
                    checked: state.dark,
                    on_checked_change: move |v| {
                        let mut s = ctx.state;
                        s.write().dark = v;
                    },
                }
            }
            Separator {}
            div { class: "flex flex-col gap-2",
                Label { "字号" }
                div { class: "flex flex-col gap-1.5",
                    div { class: "flex items-center gap-2",
                        Checkbox {
                            checked: if checkbox_compact() {
                                CheckboxState::Checked
                            } else {
                                CheckboxState::Unchecked
                            },
                            on_checked_change: move |c: CheckboxState| checkbox_compact.set(c.is_checked()),
                        }
                        span { class: "text-ui-base/relaxed", "紧凑行高" }
                        span { class: "ml-auto text-ui-xs text-foreground-subtlest",
                            "text-ui-base/relaxed"
                        }
                    }
                    div { class: "flex items-center gap-2",
                        Checkbox { checked: CheckboxState::Indeterminate, disabled: true }
                        span { class: "text-ui-base/relaxed text-foreground-subtle",
                            "跟随系统（不可用）"
                        }
                    }
                }
            }
            Separator {}
            Alert { variant: AlertVariant::Default,
                IconSparkles { class: "size-3.5" }
                AlertTitle { "主题令牌与 TS 版同源" }
                AlertDescription {
                    "app.css 里的 --color-* / --text-ui-* 是直接从 \
                     packages/ui/src/styles.css 的 @theme 块抽取的，不是手抄的近似值。"
                }
            }
        }
    }
}

/// 关于页。
#[component]
fn AboutTab() -> Element {
    rsx! {
        div { class: "flex flex-col gap-3",
            div { class: "flex items-center gap-3",
                Avatar { size: AvatarSize::Lg,
                    AvatarFallback { "ZC" }
                }
                div { class: "flex flex-col",
                    span { class: "text-ui-lg font-semibold", "ZCode（Rust 移植）" }
                    span { class: "text-ui-sm text-foreground-subtlest",
                        "版本 3.15.1 · Apache-2.0"
                    }
                }
                Badge { variant: BadgeVariant::Secondary, class: "ml-auto", "rust 分支" }
            }
            Separator {}
            Card {
                CardHeader {
                    CardTitle { "分层" }
                    CardDescription { "每一层都能独立编译与测试" }
                }
                CardContent { class: "flex flex-col gap-2",
                    LayerRow { name: "zcode-rpc", desc: "RPC 协议与传输", tests: "65" }
                    LayerRow { name: "zcode-shared", desc: "共享工具与判定", tests: "34" }
                    LayerRow { name: "zcode-ui", desc: "shadcn 原语（Rust + Dioxus）", tests: "182" }
                    LayerRow { name: "zcode-app", desc: "本窗口", tests: "2" }
                }
            }
            div { class: "flex items-center gap-2 text-ui-xs text-foreground-subtlest",
                span { "源码：" }
                span { class: "font-mono", "github.com/RapidAI/RapidCode" }
                KbdGroup { class: "ml-auto",
                    Kbd { "rust" }
                }
            }
        }
    }
}

/// 设置里的一行。
#[component]
fn SettingRow(title: &'static str, desc: &'static str, children: Element) -> Element {
    rsx! {
        div { class: "flex items-center gap-3",
            div { class: "flex min-w-0 flex-1 flex-col",
                span { class: "text-ui-base/relaxed", {title} }
                span { class: "text-ui-xs text-foreground-subtlest", {desc} }
            }
            {children}
        }
    }
}

/// 关于页里的一层。
#[component]
fn LayerRow(name: &'static str, desc: &'static str, tests: &'static str) -> Element {
    rsx! {
        div { class: "flex items-center gap-2",
            IconLayers { class: "size-3.5 shrink-0 text-foreground-subtlest" }
            span { class: "w-28 shrink-0 font-mono text-ui-sm", {name} }
            span { class: "min-w-0 flex-1 truncate text-ui-sm text-foreground-subtle", {desc} }
            Badge { variant: BadgeVariant::Outline, class: "shrink-0 text-ui-xs",
                "{tests} tests"
            }
        }
    }
}

/// 删除会话确认框。
#[component]
pub fn DeleteConfirmDialog() -> Element {
    let ctx = use_app();
    let title = ctx
        .pending_delete
        .read()
        .and_then(|id| {
            ctx.state
                .read()
                .conversations
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.title)
        })
        .unwrap_or("该会话");

    rsx! {
        AlertDialog { open: Some(ctx.delete_open),
            AlertDialogContent {
                AlertDialogHeader {
                    AlertDialogTitle { "删除会话？" }
                    AlertDialogDescription {
                        "「{title}」及其全部消息将从本机移除，此操作不可撤销。"
                    }
                }
                AlertDialogFooter {
                    AlertDialogCancel { "取消" }
                    AlertDialogAction {
                        variant: ButtonVariant::Destructive,
                        onclick: move |_| {
                            let mut pending = ctx.pending_delete;
                            pending.set(None);
                        },
                        IconTrash {}
                        "删除"
                    }
                }
            }
        }
    }
}
