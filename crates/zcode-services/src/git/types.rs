//! Git repo 层的数据形状。
//!
//! Rust 移植自 `packages/services/src/git/repo/gitCliTypes.ts`。
//!
//! 上游这个文件还声明了 `GitBranchComparisonSnapshot` / `GitCommitGraphSnapshot`
//! 与 `GitCliRepo` trait；它们分别依赖分支比较与提交图两条尚未移植的链路，
//! 因此这里先不放，等对应实现落地时一起补 —— 空 trait 只会变成噪音。

use std::collections::BTreeMap;

use zcode_shared::git::{
    GitChangeKind, GitHeadRefType, GitRepositoryAutoRefreshWatchPath, GitRepositorySummary,
};

/// 单个文件的行级增删。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitLineStat {
    pub added: u32,
    pub removed: u32,
    /// 仅在重命名/复制条目上出现。
    pub kind: Option<GitChangeKind>,
    /// 重命名/复制前的路径。
    pub original_path: Option<String>,
}

/// `git status --porcelain=v2` 里的一条记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitStatusEntry {
    /// 仓库相对路径，恒为 `/` 分隔。
    pub path: String,
    /// 重命名/复制前的路径。
    pub original_path: Option<String>,
    /// 归一化后的性质（不是原始 XY 码，见 [`GitChangeKind`]）。
    pub kind: GitChangeKind,
    /// 索引态码；未跟踪条目为 `None`。
    pub x: Option<String>,
    /// 工作区态码；未跟踪条目恒为 `"?"`。
    pub y: Option<String>,
    pub is_untracked: bool,
    pub is_conflicted: bool,
}

/// 一次 workspace → 仓库的解析结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitResolvedRepository {
    pub workspace_path: String,
    pub repo_root: String,
    /// workspace 相对仓库根的路径；workspace 就是仓库根时是 `"."`。
    pub workspace_in_repo_path: String,
    pub auto_refresh_watch_paths: Vec<GitRepositoryAutoRefreshWatchPath>,
    pub is_git_available: bool,
    pub is_repository: bool,
}

impl GitResolvedRepository {
    /// 构造一个「Git 不可用」的降级结果。
    pub fn unavailable(workspace_path: &str, is_git_available: bool) -> Self {
        Self {
            workspace_path: workspace_path.to_string(),
            repo_root: workspace_path.to_string(),
            workspace_in_repo_path: ".".to_string(),
            auto_refresh_watch_paths: Vec::new(),
            is_git_available,
            is_repository: false,
        }
    }

    /// 是否可以直接拿去跑 Git 命令。
    pub fn is_usable(&self) -> bool {
        self.is_git_available && self.is_repository
    }
}

/// 一次完整的工作区状态快照。
///
/// 上游把 `status` 与两条 `diff --numstat` 合成一次查询（并发复用同一份结果），
/// 这里保持同样的形状：解析一次，三份统计各就各位。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitStatusSnapshot {
    pub resolution: GitResolvedRepository,
    pub summary: GitRepositorySummary,
    pub entries: Vec<GitStatusEntry>,
    pub staged_stats: BTreeMap<String, GitLineStat>,
    pub unstaged_stats: BTreeMap<String, GitLineStat>,
    pub untracked_stats: BTreeMap<String, GitLineStat>,
}

impl GitStatusSnapshot {
    /// 构造一个「不是仓库」的空快照。
    pub fn empty(resolution: GitResolvedRepository) -> Self {
        let summary = zcode_shared::git::create_empty_summary(
            &resolution.workspace_path,
            resolution.is_git_available,
        );
        Self {
            resolution,
            summary,
            entries: Vec::new(),
            staged_stats: BTreeMap::new(),
            unstaged_stats: BTreeMap::new(),
            untracked_stats: BTreeMap::new(),
        }
    }

    /// 按路径取行数统计，缺省为 `+0/-0`。
    pub fn stat_for(&self, section: GitChangeSection, path: &str) -> GitLineStat {
        let table = match section {
            GitChangeSection::Staged => &self.staged_stats,
            GitChangeSection::Unstaged => &self.unstaged_stats,
            GitChangeSection::Untracked => &self.untracked_stats,
        };
        table.get(path).cloned().unwrap_or_default()
    }
}

/// [`GitStatusSnapshot`] 里三张统计表的归属。
///
/// 单独抽一个枚举，是为了让 [`GitStatusSnapshot::stat_for`] 无法被传错表 ——
/// 上游用字符串字面量做这件事，写错了编译期不会报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitChangeSection {
    Staged,
    Unstaged,
    Untracked,
}

/// HEAD 的简短描述（`git rev-parse --short` 之类）。
///
/// 保留它是为了让 [`GitResolvedRepository`] 之外的调用方有一个
/// 「当前分支」的轻量载体，不必拖整个 summary。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitHeadInfo {
    pub head_ref_type: GitHeadRefType,
    pub branch_name: Option<String>,
    pub tracking_branch_name: Option<String>,
    pub ahead: u32,
    pub behind: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_resolution_is_not_usable() {
        let resolution = GitResolvedRepository::unavailable("/tmp/x", false);
        assert!(!resolution.is_usable());
        assert!(!resolution.is_repository);
        assert_eq!(resolution.workspace_in_repo_path, ".");
    }

    #[test]
    fn available_resolution_is_usable() {
        let mut resolution = GitResolvedRepository::unavailable("/tmp/x", true);
        resolution.is_repository = true;
        assert!(resolution.is_usable());
    }

    #[test]
    fn empty_snapshot_has_no_entries_and_inherits_git_flag() {
        let snapshot = GitStatusSnapshot::empty(GitResolvedRepository::unavailable("/tmp/x", true));
        assert!(snapshot.entries.is_empty());
        assert!(snapshot.summary.is_git_available);
        assert!(!snapshot.summary.is_repository);
        assert!(snapshot.staged_stats.is_empty());
    }

    #[test]
    fn stat_for_picks_the_right_table() {
        let mut snapshot = GitStatusSnapshot::empty(GitResolvedRepository::unavailable("/tmp/x", true));
        snapshot.staged_stats.insert(
            "a.rs".to_string(),
            GitLineStat {
                added: 3,
                removed: 1,
                ..Default::default()
            },
        );
        snapshot.unstaged_stats.insert(
            "a.rs".to_string(),
            GitLineStat {
                added: 7,
                removed: 2,
                ..Default::default()
            },
        );

        assert_eq!(snapshot.stat_for(GitChangeSection::Staged, "a.rs").added, 3);
        assert_eq!(snapshot.stat_for(GitChangeSection::Unstaged, "a.rs").added, 7);
        // 未跟踪表里没有 a.rs，应当是默认值而不是串到别的表
        assert_eq!(snapshot.stat_for(GitChangeSection::Untracked, "a.rs").added, 0);
        assert_eq!(snapshot.stat_for(GitChangeSection::Staged, "missing.rs").added, 0);
    }
}
