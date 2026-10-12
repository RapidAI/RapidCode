//! 输入区：附件、草稿、发送 / 停止。
//!
//! 用到的原语：`InputGroup`（`InputGroupAddon` / `InputGroupTextarea` /
//! `InputGroupButton`）/ `Button` / `Kbd` / `Badge` / `Tooltip` / `Alert` /
//! `DropdownMenu`。

use crate::state::{use_app, ApprovalMode};
use dioxus::prelude::*;
use zcode_ui::*;

/// 底部输入区。
#[component]
pub fn Composer() -> Element {
    let ctx = use_app();
    let state = ctx.read();
    let streaming = state.streaming;
    let can_send = !state.draft.trim().is_empty();

    rsx! {
        div { class: "shrink-0 border-t border-border px-6 py-3",
            div { class: "mx-auto flex w-full max-w-3xl flex-col gap-2",

                InputGroup {
                    // 附件
                    InputGroupAddon { align: InputGroupAddonAlign::InlineStart,
                        Tooltip {
                            TooltipTrigger {
                                InputGroupButton { size: InputGroupButtonSize::IconXs,
                                    IconPaperclip {}
                                }
                            }
                            TooltipContent { side: Side::Top, side_offset: 6.0, "添加附件" }
                        }
                    }

                    InputGroupTextarea {
                        placeholder: "描述你想让 ZCode 做的事…",
                        value: state.draft.clone(),
                        rows: 3,
                        oninput: move |e: FormEvent| {
                            let mut s = ctx.state;
                            s.write().draft = e.value();
                        },
                    }

                    // 发送 / 停止
                    InputGroupAddon { align: InputGroupAddonAlign::InlineEnd,
                        KbdGroup { class: "mr-1",
                            Kbd { "⏎" }
                        }
                        if streaming {
                            InputGroupButton {
                                variant: ButtonVariant::Destructive,
                                size: InputGroupButtonSize::IconSm,
                                onclick: move |_| {
                                    let mut s = ctx.state;
                                    s.write().streaming = false;
                                },
                                IconStop {}
                            }
                        } else {
                            InputGroupButton {
                                variant: ButtonVariant::Default,
                                size: InputGroupButtonSize::IconSm,
                                disabled: !can_send,
                                onclick: move |_| {
                                    let mut s = ctx.state;
                                    if s.read().draft.trim().is_empty() {
                                        return;
                                    }
                                    s.write().draft.clear();
                                    s.write().streaming = true;
                                },
                                IconSend {}
                            }
                        }
                    }
                }

                // 状态行
                div { class: "flex items-center gap-2",
                    ApprovalPicker {}
                    Badge { variant: BadgeVariant::Ghost, class: "text-ui-xs",
                        "rust/ · 3 个文件已变更"
                    }
                    if streaming {
                        Badge { variant: BadgeVariant::Secondary, class: "gap-1 text-ui-xs",
                            Spinner { class: "size-2.5" }
                            "生成中"
                        }
                    }
                    span { class: "ml-auto text-ui-xs text-foreground-subtlest",
                        "Shift + ⏎ 换行"
                    }
                }

                if streaming {
                    Alert { variant: AlertVariant::Warning, class: "mt-1",
                        AlertTitle { "自动执行已开启" }
                        AlertDescription {
                            "接下来的工具调用不会再逐条询问，可在下方切换权限模式。"
                        }
                    }
                }
            }
        }
    }
}

/// 权限模式选择（演示 `DropdownMenuRadioGroup`）。
#[component]
fn ApprovalPicker() -> Element {
    let ctx = use_app();
    let current = ctx.read().approval;

    rsx! {
        DropdownMenu {
            DropdownMenuTrigger {
                variant: ButtonVariant::Ghost,
                size: ButtonSize::Xs,
                class: "gap-1 text-foreground-subtle",
                IconTerminal { class: "size-3" }
                {current.label()}
                IconChevronDown { class: "size-2.5" }
            }
            DropdownMenuContent { side: Side::Top, align: Align::Start, class: "w-56",
                DropdownMenuLabel { "权限模式" }
                DropdownMenuRadioGroup {
                    for m in ApprovalMode::ALL {
                        DropdownMenuRadioItem {
                            key: "{m.as_str()}",
                            checked: m == current,
                            onclick: move |_| {
                                // 同时更新受控 signal（详情面板的单选组读它）与类型化状态
                                let mut v = ctx.approval_value;
                                v.set(m.as_str().to_string());
                                let mut s = ctx.state;
                                s.write().approval = m;
                            },
                            div { class: "flex flex-col gap-0.5",
                                span { class: "text-ui-base/relaxed", {m.label()} }
                                span { class: "text-ui-xs text-foreground-subtlest", {m.hint()} }
                            }
                        }
                    }
                }
            }
        }
    }
}
