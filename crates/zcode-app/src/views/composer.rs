//! 输入区：附件、草稿、发送 / 停止。
//!
//! 用到的原语：`InputGroup`（`InputGroupAddon` / `InputGroupTextarea` /
//! `InputGroupButton`）/ `Button` / `Kbd` / `Badge` / `Tooltip` / `Alert` /
//! `DropdownMenu`。

use crate::state::{use_app, ApprovalMode};
use crate::workspace::use_code_comments;
use dioxus::prelude::*;
use zcode_ui::*;

/// 底部输入区。
#[component]
pub fn Composer() -> Element {
    let ctx = use_app();
    let comments = use_code_comments();
    let state = ctx.read();
    let streaming = state.streaming;
    let can_send = !state.draft.trim().is_empty();
    let last_prompt = state.last_prompt.clone();

    // 已经攒下的批注数（跨所有文件）。发送时会拼进「发给助手」的正文，
    // 所以这里必须让用户看得见 —— 静默附加会让用户以为发出去的只有自己写的那句话。
    let pending = comments.previews.read().total_comment_count();

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
                                    // 送出去的是**正文 + 批注块**：批注用
                                    // `build_prompt_with_code_comments`
                                    // 拼成 `# Code comments:` 段。拼装与解析
                                    // 共用同一套格式定义，所以助手那边
                                    // `parse_prompt_code_comments` 能原样读回来。
                                    let attachments =
                                        comments.previews.read().collect_composer_attachments();
                                    let prompt = build_prompt_with_code_comments(
                                        &s.read().draft,
                                        &attachments,
                                    );
                                    // 批注已经进了正文，就从「待发送」里清掉，
                                    // 否则下一条消息会把同一批批注再发一遍。
                                    let mut previews = comments.previews;
                                    previews.write().clear_all();

                                    s.write().last_prompt = prompt;
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
                        "crates/ · 3 个文件已变更"
                    }
                    // 待发送的批注数 —— 必须显式可见：批注是**附加**到正文上的，
                    // 不显示的话用户会以为「我写的那句话」就是全部要发的内容。
                    if pending > 0 {
                        // `Badge` 不接 `data-*` 透传，所以把锚点挂在外层
                        div {
                            "data-slot": "pending-code-comments",
                            "data-count": "{pending}",
                            Badge { variant: BadgeVariant::Secondary, class: "text-ui-xs",
                                "{pending} 条代码批注将一并发送"
                            }
                        }
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

                // 上一条消息真正发出去的正文（含批注块）。
                // 只在「有批注被拼进去」时显示 —— 没有批注时它就是用户自己写的话，
                // 再复述一遍是噪音。
                if !state.last_prompt.is_empty() && state.last_prompt.contains("# Code comments:") {
                    details {
                        class: "rounded-md border border-border bg-background-alt px-2 py-1",
                        "data-slot": "last-prompt",
                        summary { class: "cursor-pointer text-ui-xs text-foreground-subtle",
                            "上一条发送内容（含批注）"
                        }
                        pre {
                            class: "mt-1 max-h-40 overflow-auto whitespace-pre-wrap text-ui-xs text-foreground",
                            "{last_prompt}"
                        }
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
