//! 顶栏：视图 Tab + 模型选择 + 分支状态 + 面板开关。
//!
//! 用到的原语：`Tabs` / `DropdownMenu` / `Badge` / `Button` / `Tooltip` / `Separator`。

use crate::state::{use_app, Model};
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

    rsx! {
        header { class: "flex h-12 shrink-0 items-center gap-2 border-b border-border px-3",
            // 会话标题
            div { class: "flex min-w-0 items-center gap-2",
                span { class: "truncate text-ui-base font-medium",
                    {state.active().map(|c| c.title).unwrap_or("未选择会话")}
                }
                Badge { variant: BadgeVariant::Outline, class: "gap-0.5",
                    IconGitBranch { class: "size-2.5" }
                    "rust"
                }
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
                    label: "重新加载会话",
                    on_click: move |_| {
                        let mut s = ctx.state;
                        s.write().streaming = false;
                    },
                    IconRefresh {}
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
