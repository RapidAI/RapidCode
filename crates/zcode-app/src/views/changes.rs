//! 「变更」视图：把 `zcode-services` 跑出来的**真实**工作区状态摊开。
//!
//! 这一页是「移植过来的逻辑真的在跑」最直接的证据 —— 上面每一个文件名、
//! 每一个 `+N/-M` 都来自真的 `git status` / `git diff --numstat`，
//! 不是硬编码的演示数据。
//!
//! 分组（已暂存 / 未暂存 / 未跟踪 / 冲突）直接来自
//! `zcode_services::git::changes` 的分组结果，视图层只负责排序与展示。
//!
//! ## 写操作
//!
//! 每一行的「暂存 / 取消暂存 / 丢弃」和顶部的「提交 / 推送」都是真的在跑 git
//! （`git add` / `git restore` / `git commit` / `git push`）。三条设计约束：
//!
//! 1. **不阻塞渲染线程**：全部经 [`crate::workspace::WorkspaceRequester`] 丢到
//!    worker 线程，结果回来后自动重新扫描；
//! 2. **失败必须看得见**：写操作的结果走 `workspace_notice`，渲染成顶部横幅 ——
//!    上一轮「只有个壳子」的根因就是一个被吞掉的错误；
//! 3. **会丢数据的操作要二次确认**：只有「丢弃」属于这一类，走 `AlertDialog`。
//!
//! ## diff 预览
//!
//! 点一行就在它下面展开这个文件的 diff，用的是已经移植好的
//! [`zcode_ui::LightweightDiffPreview`] 原语。四种结果都**必须看得见**，
//! 不能出现「点了没反应」：
//!
//! | 状态 | 界面 |
//! | --- | --- |
//! | 加载中 | 转圈 + 「正在读取 diff」 |
//! | `Patch` | 真的 patch 逐行渲染 |
//! | `Binary` / `Truncated` / `Unavailable` | 各自的说明文案（不是空白） |
//! | 命令本身失败 | 红色错误行 |
//!
//! 展开状态只由 [`crate::workspace::DiffPreviewState`] 一个信号承载
//! （行的 key 就是它的 `path`），因此不会出现「展开了 A 却显示 B 的内容」。

use std::path::PathBuf;

use dioxus::prelude::*;
use zcode_shared::git::{
    GitBranchComparisonChange, GitChangeKind, GitChangeSectionId, GitDiffAvailability,
    GitDiffResult, GitFileChange,
};
use zcode_ui::*;

use crate::state::use_app;
use crate::workspace::{
    action_paths, default_workspace_path, diff_query_for, display_path, kind_label, section_label,
    split_patch_lines, use_workspace_requester, DiffPreviewState, WorkspaceAction, WorkspaceData,
    WorkspaceNotice, WorkspaceStatus,
};

use zcode_services::git::types::GitDiffQuery;

/// 待确认的「丢弃」请求。
#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingDiscard {
    paths: Vec<String>,
    staged: bool,
    /// 展示用：确认框里列出文件名。
    labels: Vec<String>,
}

/// 主区域里的「变更」页。
#[component]
pub fn ChangesView() -> Element {
    let ctx = use_app();
    let requester = use_workspace_requester();
    let data = ctx.workspace.read().clone();
    let status = ctx.workspace_status.read().clone();
    let notice = ctx.workspace_notice.read().clone();

    // 写操作需要的上下文：路径 + 当前是否已经有 git 在跑
    let workspace_path = data
        .as_ref()
        .map(|data| PathBuf::from(&data.summary.workspace_path))
        .unwrap_or_else(default_workspace_path);
    let busy = status.is_loading();

    // 「丢弃」的二次确认
    let mut pending_discard = use_signal(|| None::<PendingDiscard>);
    let mut discard_open = use_signal(|| false);

    let body = if let Some(message) = status.failure() {
        rsx! { FailurePane { message: message.to_string() } }
    } else {
        match data {
            None => rsx! { LoadingPane {} },
            Some(data) if !data.is_repository() => rsx! { NotARepositoryPane {} },
            Some(data) if data.changes.is_empty() => rsx! { CleanPane { data } },
            Some(data) => rsx! {
                ChangeSections {
                    data,
                    workspace_path: workspace_path.clone(),
                    busy,
                    on_discard: move |request: PendingDiscard| {
                        pending_discard.set(Some(request));
                        discard_open.set(true);
                    },
                }
            },
        }
    };

    // 确认框里要显示的路径（取前几个，其余折叠）
    let discard_detail = pending_discard
        .read()
        .as_ref()
        .map(|pending| summarize_paths(&pending.labels))
        .unwrap_or_default();
    let discard_staged = pending_discard
        .read()
        .as_ref()
        .map(|pending| pending.staged)
        .unwrap_or(false);

    rsx! {
        section { class: "flex min-h-0 flex-1 flex-col",
            WorkspaceBar {
                data: ctx.workspace.read().clone(),
                status: ctx.workspace_status.read().clone(),
            }
            if let Some(notice) = notice {
                NoticeBanner { notice }
            }
            ScrollArea { class: "min-h-0 flex-1", hide_scrollbar: true,
                div { class: "flex flex-col gap-4 p-4", {body} }
            }

            AlertDialog { open: Some(discard_open),
                AlertDialogContent {
                    AlertDialogHeader {
                        AlertDialogTitle { "丢弃这些改动？" }
                        AlertDialogDescription {
                            if discard_staged {
                                "会同时回退暂存区与工作区，改动将无法找回：{discard_detail}"
                            } else {
                                "会把工作区文件恢复到 HEAD 的内容，改动将无法找回：{discard_detail}"
                            }
                        }
                    }
                    AlertDialogFooter {
                        AlertDialogCancel { "取消" }
                        AlertDialogAction {
                            variant: ButtonVariant::Destructive,
                            onclick: move |_| {
                                let pending = pending_discard.write().take();
                                discard_open.set(false);
                                if let Some(pending) = pending {
                                    requester.run(
                                        workspace_path.clone(),
                                        WorkspaceAction::Discard {
                                            paths: pending.paths,
                                            staged: pending.staged,
                                        },
                                    );
                                }
                            },
                            IconTrash {}
                            "丢弃"
                        }
                    }
                }
            }
        }
    }
}

/// 把路径列表压成一行提示文案（最多列 3 个）。
fn summarize_paths(paths: &[String]) -> String {
    const MAX: usize = 3;
    if paths.len() <= MAX {
        return paths.join("、");
    }
    let shown = paths
        .iter()
        .take(MAX)
        .cloned()
        .collect::<Vec<_>>()
        .join("、");
    format!("{shown} 等 {} 个文件", paths.len())
}

/// 写操作结果的横幅。
///
/// 成功用主色、失败用 destructive。**失败不能被静默掉** —— 这正是上一轮
/// 「窗口里只有个壳子」的成因。
#[component]
fn NoticeBanner(notice: WorkspaceNotice) -> Element {
    let ctx = use_app();
    let mut current = ctx.workspace_notice;

    let (class, icon) = if notice.is_error {
        (
            "flex items-start gap-2 border-b border-destructive/40 bg-destructive/10 px-4 py-2",
            rsx! { IconTriangleAlert { class: "mt-0.5 size-3.5 shrink-0 text-destructive" } },
        )
    } else {
        (
            "flex items-start gap-2 border-b border-border bg-primary/10 px-4 py-2",
            rsx! { IconCheck { class: "mt-0.5 size-3.5 shrink-0 text-primary" } },
        )
    };

    rsx! {
        div { class: "{class}", "data-slot": "workspace-notice",
            {icon}
            span { class: "min-w-0 flex-1 break-all text-ui-xs text-foreground",
                {notice.message}
            }
            Button {
                variant: ButtonVariant::Ghost,
                size: ButtonSize::IconSm,
                onclick: move |_| current.set(None),
                IconX { class: "size-3" }
            }
        }
    }
}

/// 顶部信息条：分支、ahead/behind、仓库根、汇总、提交、推送、刷新。
#[component]
pub fn WorkspaceBar(data: Option<WorkspaceData>, status: WorkspaceStatus) -> Element {
    let requester = use_workspace_requester();
    let loading = status.is_loading();
    let commit_message = use_signal(String::new);
    let mut message_signal = commit_message;

    let workspace_path = data
        .as_ref()
        .map(|data| PathBuf::from(&data.summary.workspace_path))
        .unwrap_or_else(default_workspace_path);

    let (branch, ahead_behind, repo_root, totals_label) = match data.as_ref() {
        Some(data) if data.is_repository() => (
            data.branch_label(),
            data.ahead_behind_label(),
            data.summary.repo_root.clone(),
            format!(
                "{} 个文件 · +{} -{}",
                data.totals.file_count, data.totals.added, data.totals.removed
            ),
        ),
        Some(data) => (
            "非 Git 仓库".to_string(),
            None,
            data.summary.workspace_path.clone(),
            "无变更数据".to_string(),
        ),
        None => (
            "读取中".to_string(),
            None,
            workspace_path.to_string_lossy().into_owned(),
            "尚未扫描".to_string(),
        ),
    };
    let repo_root_hint = repo_root.clone();

    // 有已暂存内容才允许提交；消息为空也不允许（服务层还会再挡一次）
    let staged_count = data
        .as_ref()
        .map(|data| data.changes_in(GitChangeSectionId::Staged).len())
        .unwrap_or(0);
    let is_repository = data
        .as_ref()
        .map(WorkspaceData::is_repository)
        .unwrap_or(false);
    let can_commit = !loading && staged_count > 0 && !commit_message.read().trim().is_empty();
    let can_push = !loading && is_repository;

    let workspace_for_commit = workspace_path.clone();
    let workspace_for_push = workspace_path.clone();
    let workspace_for_refresh = workspace_path.clone();
    let requester_for_commit = requester.clone();
    let requester_for_push = requester.clone();
    let requester_for_refresh = requester.clone();

    rsx! {
        div { class: "flex flex-col border-b border-border",
            div { class: "flex h-11 shrink-0 items-center gap-2 px-4",
                IconGitBranch { class: "size-3.5 shrink-0 text-foreground-subtle" }
                span { class: "shrink-0 text-ui-base font-medium", {branch} }
                if let Some(ahead_behind) = ahead_behind {
                    Badge { variant: BadgeVariant::Secondary, class: "h-4 px-1.5 text-ui-xs",
                        {ahead_behind}
                    }
                }
                span { class: "shrink-0 text-ui-xs text-foreground-subtlest", "·" }
                span {
                    class: "min-w-0 truncate text-ui-xs text-foreground-subtlest",
                    title: "{repo_root_hint}",
                    {repo_root}
                }
                span { class: "ml-auto shrink-0 text-ui-xs text-foreground-subtle", {totals_label} }

                Button {
                    variant: ButtonVariant::Outline,
                    size: ButtonSize::Sm,
                    class: "h-7",
                    disabled: !can_push,
                    onclick: move |_| {
                        requester_for_push.run(workspace_for_push.clone(), WorkspaceAction::Push);
                    },
                    IconUpload { class: "size-3.5" }
                    "推送"
                }

                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::IconSm,
                    disabled: loading,
                    onclick: move |_| {
                        requester_for_refresh.refresh(workspace_for_refresh.clone());
                    },
                    if loading {
                        Spinner { class: "size-3.5" }
                    } else {
                        IconRefresh { class: "size-3.5" }
                    }
                }
            }

            // 提交行：只在确实是仓库时出现
            if is_repository {
                div { class: "flex shrink-0 items-center gap-2 px-4 pb-2",
                    Input {
                        class: "h-8 flex-1 text-ui-xs",
                        placeholder: if staged_count > 0 {
                            format!("提交信息（{staged_count} 个已暂存文件）")
                        } else {
                            "先暂存一些改动再提交".to_string()
                        },
                        value: commit_message.read().clone(),
                        disabled: loading,
                        oninput: move |event: FormEvent| message_signal.set(event.value()),
                    }
                    Button {
                        variant: ButtonVariant::Default,
                        size: ButtonSize::Sm,
                        class: "h-8",
                        disabled: !can_commit,
                        onclick: move |_| {
                            let message = message_signal.read().trim().to_string();
                            if message.is_empty() {
                                return;
                            }
                            if requester_for_commit.run(
                                workspace_for_commit.clone(),
                                WorkspaceAction::Commit { message },
                            ) {
                                message_signal.set(String::new());
                            }
                        },
                        IconCheck { class: "size-3.5" }
                        "提交"
                    }
                }
            }
        }
    }
}

/// 按分组展示变更。
///
/// 工作区那几组（已暂存 / 未暂存 / 未跟踪 / 冲突）是**可操作**的；
/// 最后那组「与上游比较」是**只读**的 —— 见 [`BranchComparisonSection`]。
#[component]
fn ChangeSections(
    data: WorkspaceData,
    workspace_path: PathBuf,
    busy: bool,
    on_discard: EventHandler<PendingDiscard>,
) -> Element {
    // 「相对上游」的差异不属于工作区状态，单列一组。
    // 没有上游时 `branch_comparison_label()` 是 `None`，整组不渲染。
    let branch_label = data.branch_comparison_label().map(str::to_string);

    rsx! {
        for section in data.populated_sections() {
            ChangeSection {
                key: "{section.as_str()}",
                section,
                data: data.clone(),
                workspace_path: workspace_path.clone(),
                busy,
                on_discard,
            }
        }
        if let Some(label) = branch_label {
            BranchComparisonSection {
                data: data.clone(),
                workspace_path: workspace_path.clone(),
                label,
            }
        }
    }
}

/// 「与上游比较」分组 —— **只读**。
///
/// 上游对应的是 `readonly: true` 的那个数据集（`useGitRepository.ts` 的
/// `buildRepositoryDatasets`）。只读的原因很实在：这里 diff 的两侧都是**提交**，
/// 不存在「暂存 / 取消暂存 / 丢弃」这些动作的对象 —— 把它们画出来
/// 只会得到一排点了没反应的按钮。
///
/// 但「点开看补丁」仍然有意义，所以整行可点的行为保留，
/// 只是把右侧那组操作按钮整个去掉。
#[component]
fn BranchComparisonSection(data: WorkspaceData, workspace_path: PathBuf, label: String) -> Element {
    let changes = &data.branch_comparison.changes;
    let added: u32 = changes.iter().map(|change| change.added).sum();
    let removed: u32 = changes.iter().map(|change| change.removed).sum();

    rsx! {
        div { class: "flex flex-col gap-1",
            "data-slot": "branch-comparison-section",
            div { class: "flex items-center gap-2 px-1",
                span { class: "text-ui-xs font-medium text-foreground-subtle",
                    "与上游比较"
                }
                Badge { variant: BadgeVariant::Outline, class: "h-4 px-1 text-ui-xs",
                    "{changes.len()}"
                }
                if !label.is_empty() {
                    span {
                        class: "min-w-0 truncate text-ui-xs text-foreground-subtlest",
                        "data-slot": "branch-comparison-label",
                        "{label}"
                    }
                }
                span { class: "ml-auto text-ui-xs text-primary", "+{added}" }
                span { class: "text-ui-xs text-destructive", "-{removed}" }
            }
            if changes.is_empty() {
                div { class: "rounded-md border border-card-border bg-card px-3 py-2 text-ui-xs text-foreground-subtle",
                    "当前分支与上游没有任何差异。"
                }
            } else {
                div { class: "flex flex-col overflow-hidden rounded-md border border-card-border bg-card",
                    for change in changes {
                        ReadOnlyChangeRow {
                            key: "{change.repo_relative_path}",
                            change: change.clone(),
                            workspace_path: workspace_path.clone(),
                        }
                    }
                }
            }
        }
    }
}

/// 分支比较里一行该怎么显示。
///
/// 重命名要显示成 `旧路径 → 新路径` —— 只写新路径的话，用户看到一个
/// 从没见过的文件名，会以为是新加的文件。`→` 用普通字符即可，
/// 转义问题交给 dioxus 处理。
fn branch_comparison_display_path(change: &GitBranchComparisonChange) -> String {
    let new_path = if change.workspace_relative_path.is_empty() {
        change.repo_relative_path.as_str()
    } else {
        change.workspace_relative_path.as_str()
    };
    match change.original_path.as_deref() {
        Some(original) if !original.is_empty() => format!("{original} → {new_path}"),
        _ => new_path.to_string(),
    }
}

/// 只读的一行（分支比较用）。
///
/// 与 [`ActionableChangeRow`] 的唯一区别是**没有操作按钮**，
/// 但「点开看 diff」一样支持 —— 这是只读分组里用户真正想做的事。
#[component]
fn ReadOnlyChangeRow(
    change: GitBranchComparisonChange,
    workspace_path: PathBuf,
) -> Element {
    let ctx = use_app();
    let requester = use_workspace_requester();
    let mut diff_preview = ctx.diff_preview;

    let row_key = change.workspace_relative_path.clone();
    let is_open = diff_preview
        .read()
        .as_ref()
        .is_some_and(|state| state.is_open_for(&row_key));
    let preview_state = if is_open {
        diff_preview.read().clone()
    } else {
        None
    };

    // 分支比较的两侧都是提交 ⇒ 来源固定是 `Branch`（服务层会走 merge-base 比较）。
    let query = GitDiffQuery::branch(change.repo_relative_path.clone());
    let workspace_for_diff = workspace_path.clone();
    let requester_for_diff = requester.clone();
    let row_key_for_click = row_key.clone();

    let added = change.added;
    let removed = change.removed;

    rsx! {
        div {
            class: "border-b border-border last:border-b-0",
            "data-slot": "change-row",
            "data-expanded": if is_open { "true" } else { "false" },
            div {
                class: "flex cursor-pointer items-center gap-1 pr-1.5",
                onclick: move |_| {
                    if is_open {
                        diff_preview.set(None);
                        return;
                    }
                    let id = requester_for_diff.load_diff(
                        workspace_for_diff.clone(),
                        query.clone(),
                        row_key_for_click.clone(),
                    );
                    diff_preview.set(Some(DiffPreviewState::Loading {
                        id,
                        path: row_key_for_click.clone(),
                    }));
                },
                div { class: "min-w-0 flex-1",
                    ReadOnlyChangeLine {
                        label: branch_comparison_display_path(&change),
                        kind: change.kind,
                        added,
                        removed,
                    }
                }
            }
            if let Some(state) = preview_state {
                DiffPreviewPane { state }
            }
        }
    }
}

/// 只读行的内容部分（路径 + 变更性质 + `+N/-M`）。
#[component]
fn ReadOnlyChangeLine(
    label: String,
    kind: GitChangeKind,
    added: u32,
    removed: u32,
) -> Element {
    rsx! {
        div { class: "flex min-w-0 flex-1 items-center gap-2 py-1 pl-2",
            span {
                class: "min-w-0 flex-1 truncate text-ui-xs text-foreground",
                title: "{label}",
                "{label}"
            }
            span { class: "shrink-0 text-ui-xs text-foreground-subtlest",
                {kind_label(kind)}
            }
            span { class: "shrink-0 text-ui-xs text-primary", "+{added}" }
            span { class: "shrink-0 text-ui-xs text-destructive", "-{removed}" }
        }
    }
}

/// 一个分组。
#[component]
fn ChangeSection(
    section: GitChangeSectionId,
    data: WorkspaceData,
    workspace_path: PathBuf,
    busy: bool,
    on_discard: EventHandler<PendingDiscard>,
) -> Element {
    let requester = use_workspace_requester();
    let changes = data.changes_in(section);
    let added: u32 = changes.iter().map(|change| change.added).sum();
    let removed: u32 = changes.iter().map(|change| change.removed).sum();

    // 整组一起操作
    let all_paths = action_paths(&changes);
    let group_paths = all_paths.clone();
    let group_paths_for_discard = all_paths.clone();
    let workspace_for_group = workspace_path.clone();
    let requester_for_group = requester.clone();
    let is_staged_section = section == GitChangeSectionId::Staged;

    rsx! {
        div { class: "flex flex-col gap-1",
            div { class: "flex items-center gap-2 px-1",
                span { class: "text-ui-xs font-medium text-foreground-subtle",
                    {section_label(section)}
                }
                Badge { variant: BadgeVariant::Outline, class: "h-4 px-1 text-ui-xs",
                    "{changes.len()}"
                }
                span { class: "ml-auto text-ui-xs text-primary", "+{added}" }
                span { class: "text-ui-xs text-destructive", "-{removed}" }

                if !all_paths.is_empty() {
                    Button {
                        variant: ButtonVariant::Ghost,
                        size: ButtonSize::Sm,
                        class: "h-5 px-1.5 text-ui-xs",
                        disabled: busy,
                        onclick: move |_| {
                            let action = if is_staged_section {
                                WorkspaceAction::Unstage(group_paths.clone())
                            } else {
                                WorkspaceAction::Stage(group_paths.clone())
                            };
                            requester_for_group.run(workspace_for_group.clone(), action);
                        },
                        if is_staged_section { "全部取消暂存" } else { "全部暂存" }
                    }
                    Button {
                        variant: ButtonVariant::Ghost,
                        size: ButtonSize::Sm,
                        class: "h-5 px-1.5 text-ui-xs text-destructive",
                        disabled: busy,
                        onclick: move |_| {
                            on_discard.call(PendingDiscard {
                                paths: group_paths_for_discard.clone(),
                                staged: is_staged_section,
                                labels: group_paths_for_discard.clone(),
                            });
                        },
                        "全部丢弃"
                    }
                }
            }
            div { class: "flex flex-col overflow-hidden rounded-md border border-card-border bg-card",
                for change in changes {
                    ActionableChangeRow {
                        key: "{change.repo_relative_path}",
                        change: change.clone(),
                        workspace_path: workspace_path.clone(),
                        busy,
                        on_discard,
                    }
                }
            }
        }
    }
}

/// 带操作按钮的一行变更。
///
/// 整行可点：点一下在下面展开这个文件的 diff，再点一下收起。
///
/// 按钮默认半透明，hover 时才完全显形 —— 否则每一行都挂着两个按钮，
/// 变更列表会变成一片按钮墙。
///
/// 行上的按钮必须 `stop_propagation()`：dioxus 的事件会向上冒泡，
/// 不拦住的话「暂存」会顺带把 diff 面板开开关关。
#[component]
fn ActionableChangeRow(
    change: GitFileChange,
    workspace_path: PathBuf,
    busy: bool,
    on_discard: EventHandler<PendingDiscard>,
) -> Element {
    let ctx = use_app();
    let requester = use_workspace_requester();
    let mut diff_preview = ctx.diff_preview;

    let path = action_paths(&[&change]);

    // 已暂存的条目给「取消暂存」，其余给「暂存」
    let is_staged = change.section == GitChangeSectionId::Staged;
    let (action, label) = if is_staged {
        (WorkspaceAction::Unstage(path.clone()), "取消暂存")
    } else {
        (WorkspaceAction::Stage(path.clone()), "暂存")
    };
    let action_for_click = action;
    let workspace_for_stage = workspace_path.clone();

    let discard_paths = path;
    let discard_label = display_path(&change).to_string();

    // 展开状态直接由 diff 预览信号决定：行 key 就是它的 path。
    let row_key = display_path(&change).to_string();
    let is_open = diff_preview
        .read()
        .as_ref()
        .is_some_and(|state| state.is_open_for(&row_key));
    let preview_state = if is_open {
        diff_preview.read().clone()
    } else {
        None
    };

    let query = diff_query_for(&change);
    let workspace_for_diff = workspace_path.clone();
    let requester_for_diff = requester.clone();
    let row_key_for_click = row_key.clone();

    rsx! {
        div {
            class: "border-b border-border last:border-b-0",
            "data-slot": "change-row",
            "data-expanded": if is_open { "true" } else { "false" },
            div {
                class: "group flex cursor-pointer items-center gap-1 pr-1.5",
                onclick: move |_| {
                    if is_open {
                        diff_preview.set(None);
                        return;
                    }
                    let id = requester_for_diff.load_diff(
                        workspace_for_diff.clone(),
                        query.clone(),
                        row_key_for_click.clone(),
                    );
                    diff_preview.set(Some(DiffPreviewState::Loading {
                        id,
                        path: row_key_for_click.clone(),
                    }));
                },
                div { class: "min-w-0 flex-1",
                    ChangeRow { change: change.clone() }
                }
                div { class: "flex shrink-0 items-center gap-0.5 opacity-40 transition-opacity group-hover:opacity-100",
                    Button {
                        variant: ButtonVariant::Ghost,
                        size: ButtonSize::Sm,
                        class: "h-5 px-1.5 text-ui-xs",
                        disabled: busy,
                        aria_label: "{label}",
                        onclick: move |event: MouseEvent| {
                            event.stop_propagation();
                            requester.run(workspace_for_stage.clone(), action_for_click.clone());
                        },
                        {label}
                    }
                    Button {
                        variant: ButtonVariant::Ghost,
                        size: ButtonSize::IconSm,
                        class: "size-5 text-destructive",
                        disabled: busy,
                        aria_label: "丢弃改动",
                        onclick: move |event: MouseEvent| {
                            event.stop_propagation();
                            on_discard.call(PendingDiscard {
                                paths: discard_paths.clone(),
                                staged: is_staged,
                                labels: vec![discard_label.clone()],
                            });
                        },
                        IconTrash { class: "size-3" }
                    }
                }
            }
            if let Some(state) = preview_state {
                DiffPreviewPane { state }
            }
        }
    }
}

/// 一行下面的 diff 面板。
#[component]
fn DiffPreviewPane(state: DiffPreviewState) -> Element {
    let path = state.path().to_string();
    let body = match state {
        DiffPreviewState::Loading { .. } => rsx! {
            div { class: "flex items-center gap-2 px-2 py-2 text-ui-xs text-foreground-subtle",
                Spinner { class: "size-3" }
                "正在读取 diff…"
            }
        },
        DiffPreviewState::Failed { message, .. } => rsx! {
            div { class: "flex items-start gap-2 px-2 py-2",
                IconTriangleAlert { class: "mt-0.5 size-3 shrink-0 text-destructive" }
                span { class: "min-w-0 break-all text-ui-xs text-destructive",
                    "读取 diff 失败：{message}"
                }
            }
        },
        DiffPreviewState::Ready { diff, .. } => rsx! { DiffReadyPane { diff: *diff } },
    };

    rsx! {
        div {
            // 注意用 `bg-background-alt` 而不是 `bg-muted/30`：本设计系统里
            // **没有 `--color-muted` 令牌**，写了也只会得到「类名在 DOM 上、
            // 样式表里没有规则」，背景静默不生效。
            class: "border-t border-border bg-background-alt",
            "data-slot": "diff-preview",
            "data-diff-path": "{path}",
            div { class: "flex items-center gap-2 px-2 py-1",
                IconFileText { class: "size-3 shrink-0 text-foreground-subtlest" }
                span { class: "min-w-0 flex-1 truncate text-ui-xs text-foreground-subtle",
                    "{path}"
                }
            }
            {body}
        }
    }
}

/// diff 已经拿到了 —— 按 `availability` 分四种渲染。
///
/// 关键点：**非 `Patch` 的三种也必须有文案**。空面板会让用户以为界面坏了，
/// 而这正是「失败必须看得见」这条约束要防的东西。
#[component]
fn DiffReadyPane(diff: GitDiffResult) -> Element {
    match diff.availability {
        GitDiffAvailability::Patch => {
            let lines = diff
                .patch
                .as_deref()
                .map(split_patch_lines)
                .unwrap_or_default();
            rsx! {
                div { class: "max-h-80 overflow-auto px-2 pb-2",
                    "data-slot": "diff-patch",
                    LightweightDiffPreview { lines, font_size_px: 11.0 }
                }
            }
        }
        availability => {
            // 图标要显式标成 `Element`：dioxus 0.6 的组件在 Rust 代码里是
            // **结构体字面量**，`IconInfo {}` 与 `IconTriangleAlert {}` 是不同类型，
            // 直接放进同一个 `match` 的分支里会编译不过。
            let icon: Element = match availability {
                GitDiffAvailability::Truncated => {
                    rsx! { IconTriangleAlert { class: "size-3" } }
                }
                _ => rsx! { IconInfo { class: "size-3" } },
            };
            let fallback = match availability {
                GitDiffAvailability::Binary => "这是二进制文件，无法预览内容。",
                GitDiffAvailability::Truncated => "diff 输出超过预览上限，已省略。",
                _ => "没有可预览的 diff。",
            };
            rsx! {
                div {
                    class: "flex items-start gap-2 px-2 pb-2 text-foreground-subtle",
                    "data-slot": "diff-unavailable",
                    "data-diff-availability": "{availability.as_str()}",
                    div { class: "mt-0.5 shrink-0", {icon} }
                    span { class: "min-w-0 break-all text-ui-xs",
                        {diff.summary.unwrap_or_else(|| fallback.to_string())}
                    }
                }
            }
        }
    }
}

/// 一行变更。详情面板与变更页共用。
#[component]
pub fn ChangeRow(change: GitFileChange, #[props(default)] compact: bool) -> Element {
    rsx! {
        div { class: "flex items-center gap-2 border-b border-border px-2 py-1.5 last:border-b-0",
            {kind_icon(change.kind)}
            span {
                class: "min-w-0 flex-1 truncate text-ui-xs text-foreground",
                title: "{change.repo_relative_path}",
                {display_path(&change)}
            }
            if !compact {
                span { class: "shrink-0 text-ui-xs text-foreground-subtlest",
                    {kind_label(change.kind)}
                }
            }
            if change.is_conflicted {
                Badge { variant: BadgeVariant::Destructive, class: "h-4 px-1 text-ui-xs", "冲突" }
            }
            if change.added > 0 {
                span { class: "shrink-0 text-ui-xs text-primary", "+{change.added}" }
            }
            if change.removed > 0 {
                span { class: "shrink-0 text-ui-xs text-destructive", "-{change.removed}" }
            }
            if change.added == 0 && change.removed == 0 {
                span { class: "shrink-0 text-ui-xs text-foreground-subtlest", "±0" }
            }
        }
    }
}

/// 变更性质对应的图标。
fn kind_icon(kind: GitChangeKind) -> Element {
    match kind {
        GitChangeKind::Added => rsx! { IconPlus { class: "size-3 shrink-0 text-primary" } },
        GitChangeKind::Deleted => {
            rsx! { IconMinus { class: "size-3 shrink-0 text-destructive" } }
        }
        GitChangeKind::Renamed => {
            rsx! { IconCopy { class: "size-3 shrink-0 text-foreground-subtlest" } }
        }
        GitChangeKind::Modified => {
            rsx! { IconFileText { class: "size-3 shrink-0 text-foreground-subtlest" } }
        }
    }
}

/// 首次扫描还没回来。
#[component]
fn LoadingPane() -> Element {
    rsx! {
        EmptyPane {
            icon: rsx! { Spinner { class: "size-4" } },
            title: "正在读取工作区".to_string(),
            hint: "git status / diff 在后台线程执行，界面不会被阻塞。".to_string(),
        }
    }
}

/// 工作区干净。
///
/// 「干净」只说明**没有未提交的改动**，不代表「和上游一致」——
/// 工作区干净但本地领先上游若干提交时，相对上游的差异必须照样摆出来，
/// 否则用户会以为「没东西可推」。
#[component]
fn CleanPane(data: WorkspaceData) -> Element {
    let branch_label = data.branch_comparison_label().map(str::to_string);
    let workspace_path = PathBuf::from(&data.summary.workspace_path);

    rsx! {
        div { class: "flex flex-col gap-3",
            EmptyPane {
                icon: rsx! { IconCheck { class: "size-4 text-primary" } },
                title: "工作区干净".to_string(),
                hint: format!("{}（{}）没有任何未提交改动。", data.branch_label(), data.summary.repo_root),
            }
            if let Some(label) = branch_label {
                BranchComparisonSection { data, workspace_path, label }
            }
        }
    }
}

/// 不是 Git 仓库 —— 这是正常状态，不是错误。
#[component]
fn NotARepositoryPane() -> Element {
    rsx! {
        EmptyPane {
            icon: rsx! { IconInfo { class: "size-4 text-foreground-subtle" } },
            title: "当前目录不是 Git 仓库".to_string(),
            hint: "把工作区指到一个 Git 仓库（或设置 ZCODE_WORKSPACE）后点刷新即可。"
                .to_string(),
        }
    }
}

/// 真的失败了 —— 必须显示出来，不能静默成空白。
#[component]
fn FailurePane(message: String) -> Element {
    rsx! {
        div { class: "flex items-start gap-3 rounded-md border border-destructive/40 bg-card p-3",
            IconTriangleAlert { class: "mt-0.5 size-4 shrink-0 text-destructive" }
            div { class: "flex min-w-0 flex-col gap-1",
                span { class: "text-ui-base font-medium text-foreground", "读取工作区失败" }
                span { class: "break-all text-ui-xs text-foreground-subtle", {message} }
            }
        }
    }
}

/// 空状态外壳。
#[component]
fn EmptyPane(icon: Element, title: String, hint: String) -> Element {
    rsx! {
        div { class: "flex flex-col items-center justify-center gap-2 rounded-md border border-dashed border-border px-4 py-12 text-center",
            {icon}
            span { class: "text-ui-base font-medium text-foreground", {title} }
            span { class: "max-w-[420px] text-ui-xs text-foreground-subtlest", {hint} }
        }
    }
}

/// 「尚未移植」占位页。
///
/// 存在的理由：顶栏的 Tab 点下去**必须有反应**。之前「变更」「任务」两个 Tab
/// 点了什么都不发生，用户看到的仍然是一个不动的壳子 —— 那正是我们要修掉的问题。
#[component]
pub fn NotPortedPane(title: &'static str, detail: &'static str) -> Element {
    rsx! {
        section { class: "flex min-h-0 flex-1 flex-col items-center justify-center gap-2 p-8 text-center",
            IconInfo { class: "size-4 text-foreground-subtle" }
            span { class: "text-ui-base font-medium text-foreground", {title} }
            span { class: "max-w-[420px] text-ui-xs text-foreground-subtlest", {detail} }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zcode_services::git::types::GitResolvedRepository;
    use zcode_shared::git::{GitHeadRefType, GitRepositorySummary};

    fn change(path: &str, kind: GitChangeKind, section: GitChangeSectionId) -> GitFileChange {
        GitFileChange {
            path: format!("/repo/{path}"),
            repo_relative_path: path.to_string(),
            workspace_relative_path: path.to_string(),
            x: Some("M".into()),
            y: Some(".".into()),
            kind,
            section,
            added: 3,
            removed: 1,
            is_staged: section == GitChangeSectionId::Staged,
            is_untracked: section == GitChangeSectionId::Untracked,
            is_conflicted: section == GitChangeSectionId::Conflicted,
        }
    }

    fn data(changes: Vec<GitFileChange>) -> WorkspaceData {
        WorkspaceData {
            summary: GitRepositorySummary {
                workspace_path: "/repo".into(),
                repo_root: "/repo".into(),
                workspace_in_repo_path: ".".into(),
                auto_refresh_watch_paths: Vec::new(),
                branch_name: Some("rust-only".into()),
                tracking_branch_name: Some("origin/rust-only".into()),
                head_ref_type: GitHeadRefType::Branch,
                ahead: 1,
                behind: 0,
                is_dirty: !changes.is_empty(),
                is_git_available: true,
                is_repository: true,
            },
            totals: zcode_services::git::changes::summarize_changes(&changes),
            changes,
            branch_comparison: zcode_shared::git::GitBranchComparison::default(),
        }
    }

    #[test]
    fn kind_icon_covers_every_variant() {
        // 只要不 panic、能产出节点即可；真正的视觉断言在 DOM 快照测试里。
        for kind in [
            GitChangeKind::Modified,
            GitChangeKind::Added,
            GitChangeKind::Deleted,
            GitChangeKind::Renamed,
        ] {
            let _ = kind_icon(kind);
        }
    }

    #[test]
    fn sections_render_the_real_change_set() {
        let data = data(vec![
            change("staged.rs", GitChangeKind::Modified, GitChangeSectionId::Staged),
            change("untracked.rs", GitChangeKind::Added, GitChangeSectionId::Untracked),
        ]);
        assert_eq!(data.populated_sections().len(), 2);
        assert_eq!(data.changes_in(GitChangeSectionId::Staged).len(), 1);
        assert_eq!(data.totals.added, 6);
        assert_eq!(data.totals.removed, 2);
    }

    #[test]
    fn resolved_repository_is_reachable_from_views() {
        let _ = GitResolvedRepository::unavailable("/repo", false);
    }
}
