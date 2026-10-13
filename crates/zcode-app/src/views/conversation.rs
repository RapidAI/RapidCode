//! 会话流：消息列表 + 工具调用卡片。
//!
//! 用到的原语：`ScrollArea` / `Avatar` / `Badge` / `Alert` / `Card` / `Progress` /
//! `Separator` / `Spinner` / `Collapsible` / `Tooltip` / `Kbd`。

use crate::state::{use_app, Message, Role, ToolCall};
use dioxus::prelude::*;
use zcode_ui::*;

/// 会话流主体。
#[component]
pub fn ConversationView() -> Element {
    let ctx = use_app();
    let state = ctx.read();

    rsx! {
        section { class: "flex min-h-0 min-w-0 flex-1 flex-col",
            ScrollArea { class: "min-h-0 flex-1",
                div { class: "mx-auto flex w-full max-w-3xl flex-col gap-5 px-6 py-6",
                    // 会话概览
                    div { class: "flex items-center gap-3",
                        Avatar { size: AvatarSize::Lg,
                            AvatarFallback { "ZC" }
                        }
                        div { class: "flex min-w-0 flex-col gap-1",
                            div { class: "flex items-center gap-2",
                                span { class: "text-ui-lg font-semibold",
                                    {state.active().map(|c| c.title).unwrap_or("未选择会话")}
                                }
                                Badge { variant: BadgeVariant::Secondary,
                                    {state.model.label()}
                                }
                                Badge { variant: BadgeVariant::Outline,
                                    {state.approval.label()}
                                }
                            }
                            span { class: "text-ui-sm text-foreground-subtlest",
                                "{state.messages.len()} 条消息 · 工作区 zcode-rust · 分支 rust-only"
                            }
                        }
                        div { class: "ml-auto flex items-center gap-2",
                            Badge { variant: BadgeVariant::Outline, class: "gap-0.5",
                                "+{state.total_added()}"
                            }
                            Badge { variant: BadgeVariant::Outline, class: "gap-0.5",
                                "-{state.total_removed()}"
                            }
                        }
                    }

                    Separator {}

                    for (i , m) in state.messages.iter().enumerate() {
                        MessageRow { key: "{i}", message: m.clone() }
                    }

                    if state.streaming {
                        div { class: "flex items-center gap-2 text-ui-sm text-foreground-subtle",
                            Spinner {}
                            "正在生成…"
                            Kbd { "Esc" }
                            span { class: "text-ui-xs text-foreground-subtlest", "中断" }
                        }
                    } else {
                        StreamingHint {}
                    }
                }
            }
        }
    }
}

/// 消息行。
#[component]
fn MessageRow(message: Message) -> Element {
    let is_user = message.role == Role::User;

    rsx! {
        div {
            class: cn(&[
                "flex gap-3",
                if is_user { "flex-row-reverse" } else { "" },
            ]),
            "data-role": message.role.as_str(),

            // 头像（助手侧挂一个 HoverCard，演示浮层）
            if is_user {
                Avatar { size: AvatarSize::Sm,
                    AvatarFallback { "我" }
                }
            } else {
                HoverCard {
                    HoverCardTrigger {
                        Avatar { size: AvatarSize::Sm,
                            AvatarFallback { "ZC" }
                        }
                    }
                    HoverCardContent {
                        div { class: "flex flex-col gap-2",
                            div { class: "flex items-center gap-2",
                                Avatar { size: AvatarSize::Sm,
                                    AvatarFallback { "ZC" }
                                }
                                div { class: "flex flex-col",
                                    span { class: "text-ui-base font-medium", "ZCode" }
                                    span { class: "text-ui-xs text-foreground-subtlest",
                                        "Rust + Dioxus 移植"
                                    }
                                }
                            }
                            Separator {}
                            div { class: "flex flex-col gap-1 text-ui-sm text-foreground-subtle",
                                span { "模型：zcode-pro" }
                                span { "工具：cargo / git / 文件读写" }
                                span { "工作区：F:\\RapidCode" }
                            }
                        }
                    }
                }
            }

            div {
                class: cn(&[
                    "flex min-w-0 flex-1 flex-col gap-2",
                    if is_user { "items-end" } else { "items-start" },
                ]),

                div { class: "flex items-center gap-2",
                    span { class: "text-ui-sm font-medium text-foreground-subtle",
                        {message.role.label()}
                    }
                }

                div {
                    class: cn(&[
                        "max-w-full rounded-lg px-3 py-2 text-ui-base/relaxed whitespace-pre-wrap",
                        if is_user {
                            "bg-primary text-primary-foreground"
                        } else {
                            "border border-card-border bg-card text-foreground"
                        },
                    ]),
                    {message.text}
                }

                if let Some(tool) = message.tool.clone() {
                    ToolCallCard { tool }
                }
            }
        }
    }
}

/// 工具调用卡片。
#[component]
fn ToolCallCard(tool: ToolCall) -> Element {
    rsx! {
        Card { class: "w-full",
            CardHeader {
                CardTitle { class: "flex items-center gap-2",
                    IconTerminal { class: "size-3.5 text-foreground-subtle" }
                    {tool.name}
                    Badge { variant: BadgeVariant::Outline, class: "gap-0.5",
                        IconCheck { class: "size-2.5" }
                        {tool.state}
                    }
                }
                CardDescription { {tool.target} }
            }
            CardContent { class: "flex flex-col gap-2",
                div { class: "flex items-center gap-2",
                    Badge { variant: BadgeVariant::Secondary, class: "gap-0.5",
                        "+{tool.added} 新增"
                    }
                    Badge { variant: BadgeVariant::Outline, class: "gap-0.5",
                        "-{tool.removed} 删除"
                    }
                    KbdGroup { class: "ml-auto",
                        Kbd { "⌘" }
                        Kbd { "L" }
                    }
                }
                Progress {
                    value: 100.0,
                    segments: Some(vec![
                        ProgressSegment::new("added", 0.72).with_class("bg-primary"),
                        ProgressSegment::new("removed", 0.18).with_class("bg-destructive"),
                        ProgressSegment::new("rest", 0.10).with_class("bg-muted-foreground/40"),
                    ]),
                }
            }
            CardFooter { class: "gap-2",
                Button { variant: ButtonVariant::Outline, size: ButtonSize::Xs,
                    "查看差异"
                }
                Button { variant: ButtonVariant::Ghost, size: ButtonSize::Xs,
                    "撤销"
                }
            }
        }
    }
}

/// 未生成时的提示条（演示 `Collapsible` 在真实语境里的用法）。
#[component]
fn StreamingHint() -> Element {
    let open = use_signal(|| false);

    rsx! {
        Collapsible { open: Some(open),
            div { class: "flex items-center gap-2",
                CollapsibleTrigger {
                    class: "flex items-center gap-1 text-ui-xs text-foreground-subtlest hover:text-foreground",
                    if open() {
                        IconChevronDown { class: "size-3" }
                    } else {
                        IconChevronRight { class: "size-3" }
                    }
                    "本会话由 zcode-app 渲染"
                }
                Badge { variant: BadgeVariant::Ghost, class: "text-ui-xs",
                    "182 tests passed"
                }
            }
            CollapsibleContent {
                div { class: "pt-2",
                    Alert { variant: AlertVariant::Default,
                        IconSparkles { class: "size-3.5" }
                        AlertTitle { "所有组件都来自 zcode-ui" }
                        AlertDescription {
                            "本页没有任何自定义 CSS：类名由 zcode-ui 的 *_classes() 生成，\
                             样式表由 Tailwind v4 CLI 扫描 Rust 源码产出并内嵌进二进制。"
                        }
                        AlertAction {
                            Tooltip {
                                TooltipTrigger {
                                    Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconXs,
                                        IconCopy {}
                                    }
                                }
                                TooltipContent { "复制说明" }
                            }
                        }
                    }
                }
            }
        }
    }
}
