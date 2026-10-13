//! 「变更」视图：把 `zcode-services` 跑出来的**真实**工作区状态摊开。
//!
//! 这一页是「移植过来的逻辑真的在跑」最直接的证据 —— 上面每一个文件名、
//! 每一个 `+N/-M` 都来自真的 `git status` / `git diff --numstat`，
//! 不是硬编码的演示数据。
//!
//! 分组（已暂存 / 未暂存 / 未跟踪 / 冲突）直接来自
//! `zcode_services::git::changes` 的分组结果，视图层只负责排序与展示。

use std::path::PathBuf;

use dioxus::prelude::*;
use zcode_shared::git::{GitChangeKind, GitChangeSectionId, GitFileChange};
use zcode_ui::*;

use crate::state::use_app;
use crate::workspace::{
    default_workspace_path, display_path, kind_label, section_label, use_workspace_requester,
    WorkspaceData, WorkspaceStatus,
};

/// 主区域里的「变更」页。
#[component]
pub fn ChangesView() -> Element {
    let ctx = use_app();
    let data = ctx.workspace.read().clone();
    let status = ctx.workspace_status.read().clone();

    let body = if let Some(message) = status.failure() {
        rsx! { FailurePane { message: message.to_string() } }
    } else {
        match data {
            None => rsx! { LoadingPane {} },
            Some(data) if !data.is_repository() => rsx! { NotARepositoryPane {} },
            Some(data) if data.changes.is_empty() => rsx! { CleanPane { data } },
            Some(data) => rsx! { ChangeSections { data } },
        }
    };

    rsx! {
        section { class: "flex min-h-0 flex-1 flex-col",
            WorkspaceBar { data: ctx.workspace.read().clone(), status: ctx.workspace_status.read().clone() }
            ScrollArea { class: "min-h-0 flex-1", hide_scrollbar: true,
                div { class: "flex flex-col gap-4 p-4", {body} }
            }
        }
    }
}

/// 顶部信息条：分支、ahead/behind、仓库根、汇总、刷新。
#[component]
pub fn WorkspaceBar(data: Option<WorkspaceData>, status: WorkspaceStatus) -> Element {
    let requester = use_workspace_requester();
    let loading = status.is_loading();

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

    rsx! {
        div { class: "flex h-11 shrink-0 items-center gap-2 border-b border-border px-4",
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
                variant: ButtonVariant::Ghost,
                size: ButtonSize::IconSm,
                disabled: loading,
                onclick: move |_| {
                    requester.refresh(workspace_path.clone());
                },
                if loading {
                    Spinner { class: "size-3.5" }
                } else {
                    IconRefresh { class: "size-3.5" }
                }
            }
        }
    }
}

/// 按分组展示变更。
#[component]
fn ChangeSections(data: WorkspaceData) -> Element {
    rsx! {
        for section in data.populated_sections() {
            ChangeSection { key: "{section.as_str()}", section, data: data.clone() }
        }
    }
}

/// 一个分组。
#[component]
fn ChangeSection(section: GitChangeSectionId, data: WorkspaceData) -> Element {
    let changes = data.changes_in(section);
    let added: u32 = changes.iter().map(|change| change.added).sum();
    let removed: u32 = changes.iter().map(|change| change.removed).sum();

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
            }
            div { class: "flex flex-col overflow-hidden rounded-md border border-card-border bg-card",
                for change in changes {
                    ChangeRow { key: "{change.repo_relative_path}", change: change.clone() }
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
#[component]
fn CleanPane(data: WorkspaceData) -> Element {
    rsx! {
        EmptyPane {
            icon: rsx! { IconCheck { class: "size-4 text-primary" } },
            title: "工作区干净".to_string(),
            hint: format!("{}（{}）没有任何未提交改动。", data.branch_label(), data.summary.repo_root),
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
