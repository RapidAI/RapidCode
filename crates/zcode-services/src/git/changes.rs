//! 把工作区状态快照展开成「可展示的变更列表」。
//!
//! Rust 移植自 `packages/services/src/git/gitService.ts` 里的
//! `toChangeForSource` / `getChangesForSource` / `buildFileChange` 一段。
//!
//! ## 这一层在做什么
//!
//! `git status` 的一条记录可以同时属于两个分组（例如 `MM`：索引里有改动，
//! 工作区里又有新改动）。所以「一条 status 记录 → 若干条变更」是一次**展开**，
//! 而不是过滤：
//!
//! - `staged` 分组：X 码有内容（且不是未跟踪/冲突）的条目
//! - `unstaged` 分组：Y 码有内容、或未跟踪、或冲突的条目
//!
//! 未跟踪文件还有一条额外的收敛规则（见 [`should_include_untracked_change`]）。

use std::path::Path;

use zcode_shared::git::{GitChangeSectionId, GitFileChange};

use super::cli_helpers::join_repo_relative;
use super::config::{is_path_in_workspace_scope, to_workspace_relative_git_path};
use super::types::{GitChangeSection, GitLineStat, GitStatusEntry, GitStatusSnapshot};

/// 变更的来源分组。上游是 `"unstaged" | "staged"` 的字面量联合。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitChangeSourceId {
    Unstaged,
    Staged,
}

impl GitChangeSourceId {
    pub fn as_str(&self) -> &'static str {
        match self {
            GitChangeSourceId::Unstaged => "unstaged",
            GitChangeSourceId::Staged => "staged",
        }
    }
}

/// 变更列表的汇总。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GitChangeSummary {
    pub file_count: usize,
    pub added: u32,
    pub removed: u32,
}

/// 把一个 status 条目展开成某个来源下的变更；不属于该来源时返回 `None`。
pub fn to_change_for_source(
    snapshot: &GitStatusSnapshot,
    entry: &GitStatusEntry,
    source_id: GitChangeSourceId,
) -> Option<GitFileChange> {
    // workspace 是仓库子目录时，仓库里其它目录的改动不该出现在面板上。
    if !matches_workspace_scope(&snapshot.summary.workspace_in_repo_path, entry) {
        return None;
    }

    match source_id {
        GitChangeSourceId::Staged => {
            // 未跟踪与冲突条目没有「索引态改动」可言。
            if entry.is_untracked || entry.is_conflicted || !has_change_marker(entry.x.as_deref()) {
                return None;
            }

            let stat = snapshot.stat_for(GitChangeSection::Staged, &entry.path);
            Some(build_file_change(
                snapshot,
                entry,
                GitChangeSectionId::Staged,
                stat.added,
                stat.removed,
            ))
        }
        GitChangeSourceId::Unstaged => {
            if entry.is_conflicted {
                // 冲突文件的行数统计没有意义（三方合并），统一给 0。
                return Some(build_file_change(
                    snapshot,
                    entry,
                    GitChangeSectionId::Conflicted,
                    0,
                    0,
                ));
            }

            if entry.is_untracked {
                let stat = snapshot.stat_for(GitChangeSection::Untracked, &entry.path);
                if !should_include_untracked_change(entry, &stat) {
                    return None;
                }
                return Some(build_file_change(
                    snapshot,
                    entry,
                    GitChangeSectionId::Untracked,
                    stat.added,
                    stat.removed,
                ));
            }

            if !has_change_marker(entry.y.as_deref()) {
                return None;
            }

            let stat = snapshot.stat_for(GitChangeSection::Unstaged, &entry.path);
            Some(build_file_change(
                snapshot,
                entry,
                GitChangeSectionId::Unstaged,
                stat.added,
                stat.removed,
            ))
        }
    }
}

/// 取某个来源下的全部变更。
pub fn get_changes(snapshot: &GitStatusSnapshot, source_id: GitChangeSourceId) -> Vec<GitFileChange> {
    snapshot
        .entries
        .iter()
        .filter_map(|entry| to_change_for_source(snapshot, entry, source_id))
        .collect()
}

/// 取工作区的全部变更（staged + unstaged），顺序与上游 `refresh` 一致：
/// 先未暂存（含未跟踪、冲突），再已暂存。
pub fn get_workspace_changes(snapshot: &GitStatusSnapshot) -> Vec<GitFileChange> {
    let mut changes = get_changes(snapshot, GitChangeSourceId::Unstaged);
    changes.extend(get_changes(snapshot, GitChangeSourceId::Staged));
    changes
}

/// 汇总一份变更列表的增删行数。
pub fn summarize_changes(changes: &[GitFileChange]) -> GitChangeSummary {
    let mut summary = GitChangeSummary {
        file_count: changes.len(),
        added: 0,
        removed: 0,
    };
    for change in changes {
        summary.added += change.added;
        summary.removed += change.removed;
    }
    summary
}

/// 未跟踪文件是否值得进面板。
///
/// 构建目录里常见大量未跟踪的空文件、失效链接或不可读产物。这类条目没有可审查的
/// 文本增删，之前会以 `+0/-0` 挤进 Review 面板，既污染列表又拖慢打开。
///
/// 但有一个例外：详细 status 超限后，Git 会用**末尾带 `/` 的目录记录**表达整棵
/// 未跟踪目录；它本身不可读、行数必然是 0，却仍必须保留 —— 那是用户
/// stage / discard 整棵目录的唯一入口。
fn should_include_untracked_change(entry: &GitStatusEntry, stat: &GitLineStat) -> bool {
    entry.path.ends_with('/') || stat.added > 0 || stat.removed > 0
}

/// 条目是否落在 workspace 范围内（原路径也算，重命名跨边界时按原路径放行）。
fn matches_workspace_scope(workspace_in_repo_path: &str, entry: &GitStatusEntry) -> bool {
    if is_path_in_workspace_scope(&entry.path, workspace_in_repo_path) {
        return true;
    }

    entry
        .original_path
        .as_deref()
        .is_some_and(|original| is_path_in_workspace_scope(original, workspace_in_repo_path))
}

fn build_file_change(
    snapshot: &GitStatusSnapshot,
    entry: &GitStatusEntry,
    section: GitChangeSectionId,
    added: u32,
    removed: u32,
) -> GitFileChange {
    GitFileChange {
        path: join_repo_relative(Path::new(&snapshot.resolution.repo_root), &entry.path)
            .to_string_lossy()
            .into_owned(),
        repo_relative_path: entry.path.clone(),
        workspace_relative_path: to_workspace_relative_git_path(
            &entry.path,
            &snapshot.summary.workspace_in_repo_path,
        ),
        x: entry.x.clone(),
        y: entry.y.clone(),
        kind: entry.kind,
        section,
        added,
        removed,
        is_staged: section == GitChangeSectionId::Staged,
        is_untracked: section == GitChangeSectionId::Untracked,
        is_conflicted: section == GitChangeSectionId::Conflicted,
    }
}

/// 状态码是否表示「这一侧真的有改动」（空 与 `.` 都表示没有）。
fn has_change_marker(code: Option<&str>) -> bool {
    matches!(code, Some(value) if !value.is_empty() && value != ".")
}

#[cfg(test)]
mod tests {
    use super::*;
    use zcode_shared::git::{GitChangeKind, GitRepositorySummary};
    use std::collections::BTreeMap;

    use super::super::types::GitResolvedRepository;

    /// 造一个仓库根在 `/repo` 的快照。
    fn snapshot(repo_root: &str, workspace_in_repo_path: &str) -> GitStatusSnapshot {
        GitStatusSnapshot {
            resolution: GitResolvedRepository {
                workspace_path: repo_root.to_string(),
                repo_root: repo_root.to_string(),
                workspace_in_repo_path: workspace_in_repo_path.to_string(),
                auto_refresh_watch_paths: Vec::new(),
                is_git_available: true,
                is_repository: true,
            },
            summary: GitRepositorySummary {
                workspace_path: repo_root.to_string(),
                repo_root: repo_root.to_string(),
                workspace_in_repo_path: workspace_in_repo_path.to_string(),
                auto_refresh_watch_paths: Vec::new(),
                branch_name: Some("main".into()),
                tracking_branch_name: None,
                head_ref_type: zcode_shared::git::GitHeadRefType::Branch,
                ahead: 0,
                behind: 0,
                is_dirty: true,
                is_git_available: true,
                is_repository: true,
            },
            entries: Vec::new(),
            staged_stats: BTreeMap::new(),
            unstaged_stats: BTreeMap::new(),
            untracked_stats: BTreeMap::new(),
        }
    }

    fn entry(path: &str, x: Option<&str>, y: Option<&str>) -> GitStatusEntry {
        GitStatusEntry {
            path: path.to_string(),
            original_path: None,
            kind: GitChangeKind::Modified,
            x: x.map(str::to_string),
            y: y.map(str::to_string),
            is_untracked: false,
            is_conflicted: false,
        }
    }

    fn untracked(path: &str) -> GitStatusEntry {
        GitStatusEntry {
            path: path.to_string(),
            original_path: None,
            kind: GitChangeKind::Added,
            x: None,
            y: Some("?".to_string()),
            is_untracked: true,
            is_conflicted: false,
        }
    }

    fn conflicted(path: &str) -> GitStatusEntry {
        GitStatusEntry {
            path: path.to_string(),
            original_path: None,
            kind: GitChangeKind::Modified,
            x: Some("U".into()),
            y: Some("U".into()),
            is_untracked: false,
            is_conflicted: true,
        }
    }

    #[test]
    fn staged_only_entry_lands_in_staged_section() {
        let mut snap = snapshot("/repo", ".");
        snap.entries.push(entry("a.rs", Some("M"), Some(".")));
        snap.staged_stats.insert(
            "a.rs".into(),
            GitLineStat {
                added: 5,
                removed: 2,
                ..Default::default()
            },
        );

        let changes = get_workspace_changes(&snap);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].section, GitChangeSectionId::Staged);
        assert!(changes[0].is_staged);
        assert_eq!(changes[0].added, 5);
        assert_eq!(changes[0].removed, 2);
        // `path` 是平台绝对路径（Windows 上是 `\`），所以按 PathBuf 拼期望值，
        // 别硬写 `/`。
        assert_eq!(changes[0].path, Path::new("/repo").join("a.rs").to_string_lossy());
        assert_eq!(changes[0].repo_relative_path, "a.rs");
    }

    #[test]
    fn unstaged_only_entry_lands_in_unstaged_section() {
        let mut snap = snapshot("/repo", ".");
        snap.entries.push(entry("a.rs", Some("."), Some("M")));
        snap.unstaged_stats.insert(
            "a.rs".into(),
            GitLineStat {
                added: 7,
                removed: 0,
                ..Default::default()
            },
        );

        let changes = get_workspace_changes(&snap);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].section, GitChangeSectionId::Unstaged);
        assert_eq!(changes[0].added, 7);
    }

    #[test]
    fn mm_entry_expands_into_both_sections() {
        // `MM` 是最容易漏的一种：索引里有改动，工作区里又有新改动。
        let mut snap = snapshot("/repo", ".");
        snap.entries.push(entry("a.rs", Some("M"), Some("M")));
        snap.staged_stats.insert(
            "a.rs".into(),
            GitLineStat {
                added: 1,
                removed: 0,
                ..Default::default()
            },
        );
        snap.unstaged_stats.insert(
            "a.rs".into(),
            GitLineStat {
                added: 9,
                removed: 3,
                ..Default::default()
            },
        );

        let changes = get_workspace_changes(&snap);
        assert_eq!(changes.len(), 2, "同一条 status 记录应当展开成两条变更");

        let staged = changes
            .iter()
            .find(|c| c.section == GitChangeSectionId::Staged)
            .expect("缺 staged");
        let unstaged = changes
            .iter()
            .find(|c| c.section == GitChangeSectionId::Unstaged)
            .expect("缺 unstaged");
        assert_eq!(staged.added, 1);
        assert_eq!(unstaged.added, 9);
        // 未暂存的排在前面（与上游 refresh 的顺序一致）
        assert_eq!(changes[0].section, GitChangeSectionId::Unstaged);
    }

    #[test]
    fn untracked_with_lines_is_included() {
        let mut snap = snapshot("/repo", ".");
        snap.entries.push(untracked("new.rs"));
        snap.untracked_stats.insert(
            "new.rs".into(),
            GitLineStat {
                added: 12,
                removed: 0,
                ..Default::default()
            },
        );

        let changes = get_workspace_changes(&snap);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].section, GitChangeSectionId::Untracked);
        assert!(changes[0].is_untracked);
        assert_eq!(changes[0].added, 12);
        assert_eq!(changes[0].y.as_deref(), Some("?"));
    }

    #[test]
    fn empty_untracked_file_is_dropped() {
        let mut snap = snapshot("/repo", ".");
        snap.entries.push(untracked("build/.keep"));
        // 未跟踪统计表里没有它 → stat 默认 0/0
        assert!(get_workspace_changes(&snap).is_empty());
    }

    #[test]
    fn empty_untracked_directory_record_is_kept() {
        // 超限降级后 Git 用末尾带 `/` 的目录记录表达整棵未跟踪目录，
        // 它行数必然为 0，但必须保留 —— 那是唯一的 stage/discard 入口。
        let mut snap = snapshot("/repo", ".");
        snap.entries.push(untracked("build/"));

        let changes = get_workspace_changes(&snap);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].repo_relative_path, "build/");
        assert_eq!(changes[0].added, 0);
    }

    #[test]
    fn conflicted_entry_goes_to_conflicted_with_zero_stats() {
        let mut snap = snapshot("/repo", ".");
        snap.entries.push(conflicted("src/conflict.rs"));
        // 故意放一份统计进去，验证冲突条目不会去读它
        snap.unstaged_stats.insert(
            "src/conflict.rs".into(),
            GitLineStat {
                added: 99,
                removed: 99,
                ..Default::default()
            },
        );

        let changes = get_workspace_changes(&snap);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].section, GitChangeSectionId::Conflicted);
        assert!(changes[0].is_conflicted);
        assert_eq!(changes[0].added, 0);
        assert_eq!(changes[0].removed, 0);
    }

    #[test]
    fn clean_entry_produces_nothing() {
        let mut snap = snapshot("/repo", ".");
        // 理论上不会出现 `..`，但解析层可能给出这种值，这里保证它被丢掉
        snap.entries.push(entry("a.rs", Some("."), Some(".")));
        assert!(get_workspace_changes(&snap).is_empty());
    }

    #[test]
    fn entries_outside_workspace_scope_are_filtered() {
        let mut snap = snapshot("/repo", "crates/zcode-app");
        snap.entries.push(entry(
            "crates/zcode-ui/src/lib.rs",
            Some("."),
            Some("M"),
        ));
        snap.entries.push(entry(
            "crates/zcode-app/src/main.rs",
            Some("."),
            Some("M"),
        ));

        let changes = get_workspace_changes(&snap);
        assert_eq!(changes.len(), 1, "只有 workspace 范围内的文件应当出现");
        assert_eq!(changes[0].repo_relative_path, "crates/zcode-app/src/main.rs");
        // workspace 相对路径要削掉前缀
        assert_eq!(changes[0].workspace_relative_path, "src/main.rs");
    }

    #[test]
    fn rename_out_of_workspace_is_kept_when_original_was_inside() {
        // 文件被移出 workspace：新路径在外面，但原路径在里面，
        // 用户需要看到这次移动，所以按原路径放行。
        let mut snap = snapshot("/repo", "crates/zcode-app");
        snap.entries.push(GitStatusEntry {
            path: "crates/zcode-ui/src/moved.rs".into(),
            original_path: Some("crates/zcode-app/src/moved.rs".into()),
            kind: GitChangeKind::Renamed,
            x: Some("R".into()),
            y: Some(".".into()),
            is_untracked: false,
            is_conflicted: false,
        });

        let changes = get_workspace_changes(&snap);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, GitChangeKind::Renamed);
    }

    #[test]
    fn get_changes_filters_by_source() {
        let mut snap = snapshot("/repo", ".");
        snap.entries.push(entry("staged.rs", Some("M"), Some(".")));
        snap.entries.push(entry("unstaged.rs", Some("."), Some("M")));

        let staged = get_changes(&snap, GitChangeSourceId::Staged);
        assert_eq!(staged.len(), 1);
        assert_eq!(staged[0].repo_relative_path, "staged.rs");

        let unstaged = get_changes(&snap, GitChangeSourceId::Unstaged);
        assert_eq!(unstaged.len(), 1);
        assert_eq!(unstaged[0].repo_relative_path, "unstaged.rs");

        assert_eq!(GitChangeSourceId::Staged.as_str(), "staged");
        assert_eq!(GitChangeSourceId::Unstaged.as_str(), "unstaged");
    }

    #[test]
    fn summarize_counts_files_and_lines() {
        let mut snap = snapshot("/repo", ".");
        snap.entries.push(entry("a.rs", Some("M"), Some(".")));
        snap.entries.push(entry("b.rs", Some("."), Some("M")));
        snap.staged_stats.insert(
            "a.rs".into(),
            GitLineStat {
                added: 10,
                removed: 4,
                ..Default::default()
            },
        );
        snap.unstaged_stats.insert(
            "b.rs".into(),
            GitLineStat {
                added: 1,
                removed: 2,
                ..Default::default()
            },
        );

        let summary = summarize_changes(&get_workspace_changes(&snap));
        assert_eq!(summary.file_count, 2);
        assert_eq!(summary.added, 11);
        assert_eq!(summary.removed, 6);
    }

    #[test]
    fn empty_snapshot_summarizes_to_zero() {
        let snap = snapshot("/repo", ".");
        let summary = summarize_changes(&get_workspace_changes(&snap));
        assert_eq!(summary, GitChangeSummary::default());
    }
}
