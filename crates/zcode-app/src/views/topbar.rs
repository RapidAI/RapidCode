//! 顶栏：视图 Tab + 模型选择 + **真实**工作区状态 + 面板开关。
//!
//! 用到的原语：`Tabs` / `DropdownMenu` / `Badge` / `Button` / `Tooltip` / `Separator`。
//!
//! 分支徽标与刷新按钮接的是 `zcode-services` 跑出来的真实仓库状态
//! （见 [`crate::workspace`]），不再是硬编码的字符串。

use std::path::PathBuf;

use crate::state::{use_app, Model};
use crate::workspace::{default_workspace_path, use_workspace_requester};
use dioxus::prelude::*;
use zcode_ui::*;

/// 视图 Tab 的取值。
pub const WORKSPACE_TABS: [(&str, &str); 3] = [
    ("conversation", "对话"),
    ("changes", "变更"),
    ("tasks", "任务"),
];

/// 顶栏。
#[component]
pub fn TopBar() -> Element {
    let ctx = use_app();
    let state = ctx.read();
    let tab = ctx.workspace_tab;

    let requester = use_workspace_requester();
    let loading = ctx.workspace_status.read().is_loading();
    let workspace_path = ctx
        .workspace
        .read()
        .as_ref()
        .map(|data| PathBuf::from(&data.summary.workspace_path))
        .unwrap_or_else(default_workspace_path);

    let refresh_icon = if loading {
        rsx! { Spinner { class: "size-3.5" } }
    } else {
        rsx! { IconRefresh {} }
    };

    rsx! {
        header { class: "flex h-12 shrink-0 items-center gap-2 border-b border-border px-3",
            // 会话标题
            div { class: "flex min-w-0 items-center gap-2",
                span { class: "truncate text-ui-base font-medium",
                    {state.active().map(|c| c.title).unwrap_or("未选择会话")}
                }
                WorkspaceBadge {}
            }

            Separator { orientation: SeparatorOrientation::Vertical, class: "mx-1 h-4 self-center" }

            // 视图 Tab
            Tabs { value: tab,
                TabsList { variant: TabsListVariant::Default,
                    for (value , label) in WORKSPACE_TABS {
                        TabsTrigger { value, {label} }
                    }
                }
            }

            div { class: "ml-auto flex items-center gap-1",
                ModelPicker {}
                Separator { orientation: SeparatorOrientation::Vertical, class: "mx-1 h-4 self-center" }
                IconAction {
                    label: "刷新工作区状态",
                    on_click: move |_| {
                        requester.refresh(workspace_path.clone());
                    },
                    {refresh_icon}
                }
                IconAction {
                    label: "导出",
                    on_click: move |_| {},
                    IconDownload {}
                }
                IconAction {
                    label: "切换详情面板",
                    on_click: move |_| {
                        let mut d = ctx.detail_open;
                        let next = !d();
                        d.set(next);
                    },
                    IconPanelRight {}
                }
            }
        }
    }
}

/// 顶栏上的工作区徽标。
///
/// 四种状态都要有明确表现 —— 尤其是**失败**：读取失败必须看得见，
/// 静默成空白正是上一轮那个「只有壳子」的成因之一。
#[component]
fn WorkspaceBadge() -> Element {
    let ctx = use_app();
    let data = ctx.workspace.read().clone();
    let status = ctx.workspace_status.read().clone();

    if let Some(message) = status.failure() {
        return rsx! {
            Tooltip {
                TooltipTrigger {
                    Badge { variant: BadgeVariant::Destructive, class: "gap-0.5",
                        IconTriangleAlert { class: "size-2.5" }
                        "工作区读取失败"
                    }
                }
                TooltipContent { side: Side::Bottom, side_offset: 6.0,
                    {message.to_string()}
                }
            }
        };
    }

    if status.is_loading() && data.is_none() {
        return rsx! {
            Badge { variant: BadgeVariant::Outline, class: "gap-0.5",
                Spinner { class: "size-2.5" }
                "读取中"
            }
        };
    }

    let Some(data) = data else {
        return rsx! {
            Badge { variant: BadgeVariant::Outline, class: "gap-0.5", "未扫描" }
        };
    };

    if !data.is_repository() {
        return rsx! {
            Tooltip {
                TooltipTrigger {
                    Badge { variant: BadgeVariant::Outline, class: "gap-0.5",
                        IconInfo { class: "size-2.5" }
                        "非 Git 仓库"
                    }
                }
                TooltipContent { side: Side::Bottom, side_offset: 6.0,
                    {data.summary.workspace_path.clone()}
                }
            }
        };
    }

    let branch_label = data.branch_label();
    let repo_root = data.summary.repo_root.clone();
    let ahead_behind = data.ahead_behind_label();
    let dirty = data.summary.is_dirty;
    let change_count = data.totals.file_count;

    rsx! {
        div { class: "flex items-center gap-1",
            Tooltip {
                TooltipTrigger {
                    Badge { variant: BadgeVariant::Outline, class: "gap-0.5",
                        IconGitBranch { class: "size-2.5" }
                        {branch_label}
                    }
                }
                TooltipContent { side: Side::Bottom, side_offset: 6.0, {repo_root} }
            }
            if let Some(ahead_behind) = ahead_behind {
                Badge { variant: BadgeVariant::Secondary, class: "h-4 px-1 text-ui-xs",
                    {ahead_behind}
                }
            }
            if dirty {
                Badge { variant: BadgeVariant::Secondary, class: "h-4 px-1 text-ui-xs",
                    "{change_count} 个变更"
                }
            }
        }
    }
}

/// 带 tooltip 的图标按钮。
#[component]
fn IconAction(label: &'static str, on_click: EventHandler<MouseEvent>, children: Element) -> Element {
    rsx! {
        Tooltip {
            TooltipTrigger {
                Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconSm,
                    onclick: move |e| on_click.call(e),
                    {children}
                }
            }
            TooltipContent { side: Side::Bottom, align: Align::End, side_offset: 6.0,
                {label}
            }
        }
    }
}

/// 模型选择器。
#[component]
fn ModelPicker() -> Element {
    let ctx = use_app();
    let state = ctx.read();
    let current = state.model;

    rsx! {
        DropdownMenu {
            DropdownMenuTrigger {
                variant: ButtonVariant::Outline,
                size: ButtonSize::Sm,
                class: "gap-1.5",
                IconSparkles { class: "size-3" }
                {current.label()}
                IconChevronDown { class: "size-3 text-foreground-subtle" }
            }
            DropdownMenuContent { align: Align::End, class: "w-64",
                DropdownMenuLabel { "模型" }
                for m in Model::ALL {
                    DropdownMenuRadioItem {
                        key: "{m.as_str()}",
                        checked: m == current,
                        onclick: move |_| {
                            let mut s = ctx.state;
                            s.write().model = m;
                        },
                        div { class: "flex flex-col gap-0.5",
                            span { class: "text-ui-base/relaxed", {m.label()} }
                            span { class: "text-ui-xs text-foreground-subtlest", {m.hint()} }
                        }
                    }
                }
                DropdownMenuSeparator {}
                DropdownMenuItem {
                    IconTerminal { class: "size-3.5" }
                    "配置自定义模型"
                }
            }
        }
    }
}
