//! 左侧栏：品牌区 + 新建/搜索 + 会话列表 + 底部工具。
//!
//! 用到的原语：`Button` / `Badge` / `InputGroup`（Addon + Input）/ `Separator` /
//! `ScrollArea` / `Tooltip` / `DropdownMenu` / `Avatar` / `Kbd`。

use crate::state::{use_app, Conversation, GROUPS};
use dioxus::prelude::*;
use zcode_ui::*;

/// 左侧栏。
#[component]
pub fn Sidebar() -> Element {
    let ctx = use_app();
    let state = ctx.read();

    rsx! {
        aside { class: "flex w-[268px] shrink-0 flex-col border-r border-border bg-background-alt",
            BrandRow {}

            div { class: "flex flex-col gap-2 px-3 pb-2",
                Button {
                    class: "w-full justify-start",
                    onclick: move |_| {
                        // 新建会话：真实应用里会走 RPC；这里只演示状态流转
                        let mut s = ctx.state;
                        s.write().draft.clear();
                    },
                    IconPlus { class: "size-3.5" }
                    "新建会话"
                    KbdGroup { class: "ml-auto",
                        Kbd { "⌘" }
                        Kbd { "N" }
                    }
                }

                InputGroup {
                    InputGroupAddon { align: InputGroupAddonAlign::InlineStart,
                        IconSearch { class: "size-3.5" }
                    }
                    InputGroupInput {
                        placeholder: "搜索会话",
                        value: state.search.clone(),
                        oninput: move |e: FormEvent| {
                            let mut s = ctx.state;
                            s.write().search = e.value();
                        },
                    }
                }
            }

            Separator {}

            ScrollArea { class: "min-h-0 flex-1", hide_scrollbar: true,
                div { class: "flex flex-col gap-3 px-2 py-2",
                    for group in GROUPS {
                        ConversationGroup { group }
                    }
                    if state.filtered().is_empty() {
                        div { class: "px-2 py-6 text-center text-ui-sm text-foreground-subtlest",
                            "没有匹配的会话"
                        }
                    }
                }
            }

            Separator {}
            SidebarFooter {}
        }
    }
}

/// 品牌行。
#[component]
fn BrandRow() -> Element {
    let ctx = use_app();
    let dark = ctx.read().dark;

    rsx! {
        div { class: "flex h-12 shrink-0 items-center gap-2 px-3",
            div { class: "flex size-6 items-center justify-center rounded-md bg-primary text-primary-foreground",
                IconSparkles { class: "size-3.5" }
            }
            span { class: "text-ui-base font-semibold", "ZCode" }
            Badge { variant: BadgeVariant::Secondary, "Rust" }

            Tooltip {
                class: "ml-auto",
                TooltipTrigger {
                    Button {
                        variant: ButtonVariant::Ghost,
                        size: ButtonSize::IconSm,
                        onclick: move |_| {
                            let mut s = ctx.state;
                            let next = !s.read().dark;
                            s.write().dark = next;
                        },
                        if dark {
                            IconSun {}
                        } else {
                            IconMoon {}
                        }
                    }
                }
                TooltipContent { side: Side::Bottom, align: Align::End, side_offset: 6.0,
                    "切换主题"
                }
            }
        }
    }
}

/// 一个日期分组。
#[component]
fn ConversationGroup(group: &'static str) -> Element {
    let ctx = use_app();
    let list = ctx.read().in_group(group);

    if list.is_empty() {
        return rsx! {};
    }

    rsx! {
        div { key: "{group}",
            div { class: "flex items-center gap-1 px-2 py-1",
                span { class: "text-ui-xs font-medium text-foreground-subtlest", {group} }
                span { class: "text-ui-xs text-foreground-subtlest", "·" }
                span { class: "text-ui-xs text-foreground-subtlest", "{list.len()}" }
            }
            div { class: "flex flex-col gap-0.5",
                for c in list {
                    ConversationRow { key: "{c.id}", conversation: c }
                }
            }
        }
    }
}

/// 单条会话。
#[component]
fn ConversationRow(conversation: Conversation) -> Element {
    let ctx = use_app();
    let active = ctx.read().active_id == conversation.id;
    let id = conversation.id;

    let row_class = cn(&[
        "group/row flex cursor-default flex-col gap-0.5 rounded-md px-2 py-1.5 text-left transition-colors",
        if active { "bg-muted" } else { "hover:bg-hover" },
    ]);

    rsx! {
        div {
            class: row_class,
            "data-active": active.then_some(""),
            onclick: move |_| {
                let mut s = ctx.state;
                s.write().active_id = id;
            },

            div { class: "flex items-center gap-1.5",
                if conversation.pinned {
                    IconSparkles { class: "size-3 shrink-0 text-foreground-subtlest" }
                }
                span { class: "min-w-0 flex-1 truncate text-ui-base/relaxed text-foreground",
                    {conversation.title}
                }
                if conversation.unread > 0 {
                    Badge { class: "h-4 px-1.5 text-ui-xs", "{conversation.unread}" }
                }
                span { class: "shrink-0 text-ui-xs text-foreground-subtlest",
                    {conversation.updated}
                }
                // 行操作菜单：默认隐藏，hover 或激活时出现
                div {
                    class: cn(&[
                        "shrink-0",
                        if active { "opacity-100" } else { "opacity-0 group-hover/row:opacity-100" },
                    ]),
                    ConversationMenu { id, title: conversation.title }
                }
            }

            span { class: "truncate text-ui-xs text-foreground-subtlest",
                {conversation.preview}
            }

            div { class: "flex items-center gap-1 pt-0.5",
                Badge { variant: BadgeVariant::Outline, class: "h-4 gap-0.5 px-1 text-ui-xs",
                    IconFileText { class: "size-2.5" }
                    "+{conversation.added}"
                }
                Badge { variant: BadgeVariant::Outline, class: "h-4 gap-0.5 px-1 text-ui-xs",
                    "-{conversation.removed}"
                }
            }
        }
    }
}

/// 会话行右上角的操作菜单。
#[component]
fn ConversationMenu(id: &'static str, title: &'static str) -> Element {
    let ctx = use_app();

    rsx! {
        DropdownMenu {
            DropdownMenuTrigger {
                variant: ButtonVariant::Ghost,
                size: ButtonSize::IconXs,
                class: "text-foreground-subtlest",
                IconChevronDown {}
            }
            DropdownMenuContent { align: Align::End, class: "w-40",
                DropdownMenuLabel { {title} }
                DropdownMenuSeparator {}
                DropdownMenuItem {
                    onclick: move |_| {
                        let mut s = ctx.state;
                        s.write().active_id = id;
                    },
                    IconPencil { class: "size-3.5" }
                    "重命名"
                    DropdownMenuShortcut { "F2" }
                }
                DropdownMenuItem {
                    onclick: move |_| {
                        let mut s = ctx.state;
                        s.write().active_id = id;
                    },
                    IconCopy { class: "size-3.5" }
                    "复制会话"
                }
                DropdownMenuItem {
                    onclick: move |_| {
                        let mut s = ctx.state;
                        s.write().active_id = id;
                    },
                    IconDownload { class: "size-3.5" }
                    "导出 Markdown"
                }
                DropdownMenuSeparator {}
                DropdownMenuItem {
                    variant: DropdownMenuItemVariant::Destructive,
                    onclick: move |_| {
                        // 交给 AlertDialog 确认：先记下目标，再打开确认框
                        let mut pending = ctx.pending_delete;
                        pending.set(Some(id));
                        let mut open = ctx.delete_open;
                        open.set(true);
                    },
                    IconTrash { class: "size-3.5" }
                    "删除会话"
                }
            }
        }
    }
}

/// 侧栏底部：组件画廊入口 + 设置 + **真实**工作区信息。
///
/// 「上下文用量」与用户名仍是演示数据（依赖尚未移植的会话存储与账号体系），
/// 但工作区那一行读的是 `zcode-services` 跑出来的真实仓库状态。
#[component]
fn SidebarFooter() -> Element {
    let ctx = use_app();
    let state = ctx.read();
    let pct = if state.token_limit > 0.0 {
        state.token_used / state.token_limit * 100.0
    } else {
        0.0
    };

    let workspace = ctx.workspace.read().clone();
    let (workspace_name, workspace_detail) = match workspace.as_ref() {
        Some(data) if data.is_repository() => (
            path_leaf(&data.summary.workspace_path),
            format!("{} · {} 个变更", data.branch_label(), data.totals.file_count),
        ),
        Some(data) => (path_leaf(&data.summary.workspace_path), "非 Git 仓库".to_string()),
        None => ("本地工作区".to_string(), "读取中…".to_string()),
    };

    rsx! {
        div { class: "flex flex-col gap-2 p-2",
            // 上下文用量
            div { class: "flex flex-col gap-1 rounded-md border border-card-border bg-card px-2 py-1.5",
                div { class: "flex items-center justify-between",
                    span { class: "text-ui-xs text-foreground-subtle", "上下文用量" }
                    span { class: "text-ui-xs text-foreground-subtlest",
                        "{state.token_used as i64} / {state.token_limit as i64}"
                    }
                }
                Progress { value: pct }
            }

            div { class: "flex items-center gap-1",
                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::Sm,
                    class: "flex-1 justify-start",
                    onclick: move |_| {
                        let mut g = ctx.gallery_open;
                        g.set(true);
                    },
                    IconLayers {}
                    "组件画廊"
                }
                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::Sm,
                    class: "flex-1 justify-start",
                    onclick: move |_| {
                        let mut s = ctx.settings_open;
                        s.set(true);
                    },
                    IconSettings {}
                    "设置"
                }
            }

            div { class: "flex items-center gap-2 px-1",
                Avatar { size: AvatarSize::Sm,
                    AvatarFallback { "M" }
                }
                div { class: "flex min-w-0 flex-1 flex-col",
                    span { class: "truncate text-ui-xs text-foreground", {workspace_name} }
                    span { class: "truncate text-ui-xs text-foreground-subtlest",
                        {workspace_detail}
                    }
                }
                Badge { variant: BadgeVariant::Outline, class: "h-4 px-1 text-ui-xs", "Pro" }
            }
        }
    }
}

/// 取路径的最后一段（`F:\RapidCode` → `RapidCode`）。
fn path_leaf(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.to_string())
}
