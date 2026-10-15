//! Git 领域共享类型。
//!
//! Rust 移植自 `packages/shared/src/git.ts`。
//!
//! 这里只放**跨进程/跨层共享的形状**（协议类型），不放实现。上游同一个文件里
//! 还有 `GitCheckpoint*` 一族与 commit-message 生成相关的类型，它们依赖
//! `GitCheckpointStore` / LLM provider，属于另一条链路，尚未迁移（见 `PORTING.md`）。
//!
//! 命名映射约定：TS 的 camelCase 字段在 Rust 侧统一改成 snake_case，
//! 枚举值仍用 TS 里的字符串字面量（见各 `as_str`），这样跨层序列化时不用再翻一层。

/// 当前 HEAD 指向什么。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GitHeadRefType {
    /// 正常分支。
    #[default]
    Branch,
    /// 游离 HEAD（`git status` 报 `(detached)`）。
    Detached,
}

impl GitHeadRefType {
    pub fn as_str(&self) -> &'static str {
        match self {
            GitHeadRefType::Branch => "branch",
            GitHeadRefType::Detached => "detached",
        }
    }
}

/// 单条变更的性质。
///
/// 注意它**不是** `git status` 的 XY 码：`R`/`C` 都会归成 [`GitChangeKind::Renamed`]，
/// `A`/`?` 都会归成 [`GitChangeKind::Added`]。归一化规则见
/// `zcode_services::git::parse::infer_kind_from_status_code`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GitChangeKind {
    #[default]
    Modified,
    Added,
    Deleted,
    Renamed,
}

impl GitChangeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            GitChangeKind::Modified => "modified",
            GitChangeKind::Added => "added",
            GitChangeKind::Deleted => "deleted",
            GitChangeKind::Renamed => "renamed",
        }
    }
}

/// 一条变更属于哪个分组（UI 里的分区标题）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitChangeSectionId {
    Staged,
    Unstaged,
    Untracked,
    Conflicted,
    /// 与 base 分支的差异（不是工作区状态）。
    Branch,
    /// 本轮对话改动的文件。
    LastTurn,
}

impl GitChangeSectionId {
    pub fn as_str(&self) -> &'static str {
        match self {
            GitChangeSectionId::Staged => "staged",
            GitChangeSectionId::Unstaged => "unstaged",
            GitChangeSectionId::Untracked => "untracked",
            GitChangeSectionId::Conflicted => "conflicted",
            GitChangeSectionId::Branch => "branch",
            GitChangeSectionId::LastTurn => "last-turn",
        }
    }
}

/// 一次 diff 查询针对的是哪一类改动来源。
///
/// 对应上游 `GitChangeSourceId`（`"unstaged" | "staged" | "branch" | "last-turn"`）。
/// 注意它比 [`GitChangeSectionId`] 少两个值 —— `conflicted` / `untracked` 是
/// **分区**，不是 diff 来源。
///
/// 只有 `Branch` 与 `Staged` 会改变 `get_diff` 的行为：
/// `Branch` 走 `merge-base` 比较，`Staged` 走 `--cached`；`Unstaged` 与
/// `LastTurn` 都是「工作区 vs 索引」。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GitChangeSourceId {
    #[default]
    Unstaged,
    Staged,
    Branch,
    LastTurn,
}

impl GitChangeSourceId {
    pub fn as_str(&self) -> &'static str {
        match self {
            GitChangeSourceId::Unstaged => "unstaged",
            GitChangeSourceId::Staged => "staged",
            GitChangeSourceId::Branch => "branch",
            GitChangeSourceId::LastTurn => "last-turn",
        }
    }
}

/// Git 元数据 watcher 的边界。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitRepositoryAutoRefreshWatchPath {
    pub path: String,
    pub recursive: bool,
}

/// 仓库摘要：UI 顶栏/侧栏直接读它。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitRepositorySummary {
    pub workspace_path: String,
    pub repo_root: String,
    pub workspace_in_repo_path: String,
    /// Git 元数据 watcher 边界；workspace 内容 watcher 由 UI 按平台决定。
    pub auto_refresh_watch_paths: Vec<GitRepositoryAutoRefreshWatchPath>,
    pub branch_name: Option<String>,
    pub tracking_branch_name: Option<String>,
    pub head_ref_type: GitHeadRefType,
    pub ahead: u32,
    pub behind: u32,
    pub is_dirty: bool,
    pub is_git_available: bool,
    pub is_repository: bool,
}

/// 一条可展示的变更（`GitStatusEntry` + 行数统计 + 分组的结果）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitFileChange {
    /// 绝对路径（平台分隔符）。
    pub path: String,
    /// 相对仓库根的路径，恒为 `/` 分隔。
    pub repo_relative_path: String,
    /// 相对 workspace 的路径（workspace 是仓库子目录时才有意义）。
    pub workspace_relative_path: String,
    /// `git status --porcelain=v2` 的 X 码（索引态）；未跟踪条目为 `None`。
    pub x: Option<String>,
    /// Y 码（工作区态）。
    pub y: Option<String>,
    pub kind: GitChangeKind,
    pub section: GitChangeSectionId,
    pub added: u32,
    pub removed: u32,
    pub is_staged: bool,
    pub is_untracked: bool,
    pub is_conflicted: bool,
}

/// 分支比较里的一条变更（上游 `GitBranchComparisonChange`）。
///
/// 与 [`GitFileChange`] 的区别：这里**没有** `x` / `y` 状态码，也没有
/// `is_staged` / `is_untracked` / `is_conflicted` —— 因为它描述的不是
/// 「工作区相对索引」的状态，而是「两个提交之间的差异」，
/// 那些状态位在语义上不存在。
///
/// 路径同样给三份（绝对 / 仓库相对 / workspace 相对），
/// 因为下游要按 workspace 范围过滤、又要拼绝对路径去读文件内容。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitBranchComparisonChange {
    /// 绝对路径（平台分隔符）。
    pub path: String,
    /// 相对仓库根的路径，恒为 `/` 分隔。
    pub repo_relative_path: String,
    /// 相对 workspace 的路径（workspace 是仓库子目录时才有意义）。
    pub workspace_relative_path: String,
    /// 重命名前的仓库相对路径。
    pub original_path: Option<String>,
    pub kind: GitChangeKind,
    pub added: u32,
    pub removed: u32,
}

/// 分支比较的快照（上游 `GitBranchComparison` 的 `changes` + 标签部分）。
///
/// 上游把 `resolution` 也塞在这个类型里，这里**刻意不塞** ——
/// 调用方本来就已经拿着 [`GitResolvedRepository`] 了（`get_status` 会返回），
/// 再复制一份只会有两个可能不一致的真源。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct GitBranchComparison {
    /// 比较基准（上游跟踪分支）。
    pub base_ref: Option<String>,
    /// 比较目标（当前分支；detached 时是 `"HEAD"`）。
    pub head_ref: Option<String>,
    /// 展示用标签，形如 `main -> origin/main`；无从比较时为 `None`。
    pub comparison_label: Option<String>,
    pub changes: Vec<GitBranchComparisonChange>,
}

/// workspace 与仓库的关系。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitWorkspaceRepositoryKind {
    NotRepository,
    MainTree,
    /// `.git` 是指向 `<main-tree>/.git/worktrees/<name>` 的文件。
    LinkedWorktree,
}

impl GitWorkspaceRepositoryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            GitWorkspaceRepositoryKind::NotRepository => "not-repository",
            GitWorkspaceRepositoryKind::MainTree => "main-tree",
            GitWorkspaceRepositoryKind::LinkedWorktree => "linked-worktree",
        }
    }
}

/// [`GitWorkspaceRepositoryKind`] 的判定结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitWorkspaceRepositoryInfo {
    pub workspace_path: String,
    pub kind: GitWorkspaceRepositoryKind,
    pub is_git_available: bool,
}

/// 一个本地分支。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitLocalBranch {
    pub name: String,
    pub is_current: bool,
    pub upstream_name: Option<String>,
    pub commit_hash: Option<String>,
    pub commit_timestamp_ms: Option<i64>,
}

/// [`GitLocalBranch`] 列表 + 当前 HEAD 信息。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitLocalBranchListResult {
    pub head_ref_type: GitHeadRefType,
    pub current_branch_name: Option<String>,
    pub branches: Vec<GitLocalBranch>,
}

/// 一份 diff 能拿到什么。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitDiffAvailability {
    /// 有可展开的 patch。
    Patch,
    /// 二进制文件，不可预览。
    Binary,
    /// 超过预览上限被截断。
    Truncated,
    /// 拿不到（超时/非 0 退出/空输出）。
    Unavailable,
}

impl GitDiffAvailability {
    pub fn as_str(&self) -> &'static str {
        match self {
            GitDiffAvailability::Patch => "patch",
            GitDiffAvailability::Binary => "binary",
            GitDiffAvailability::Truncated => "truncated",
            GitDiffAvailability::Unavailable => "unavailable",
        }
    }
}

/// 单文件 diff 结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitDiffResult {
    pub path: String,
    pub availability: GitDiffAvailability,
    pub patch: Option<String>,
    pub before_content: Option<String>,
    pub after_content: Option<String>,
    /// 拿不到 patch 时的原因（可直接展示给用户）。
    pub summary: Option<String>,
}

/// `user.name` / `user.email` 及其来源。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitIdentity {
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub name_source: Option<String>,
    pub email_source: Option<String>,
    pub scope_label: Option<String>,
}

/// 提交图上一个 ref 属于哪一类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitCommitGraphRefKind {
    Branch,
    Remote,
    Tag,
    /// 当前 HEAD 本身（`git log` 的 `HEAD ->` 会额外产出一条）。
    Head,
}

impl GitCommitGraphRefKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            GitCommitGraphRefKind::Branch => "branch",
            GitCommitGraphRefKind::Remote => "remote",
            GitCommitGraphRefKind::Tag => "tag",
            GitCommitGraphRefKind::Head => "head",
        }
    }
}

/// 挂在某个提交上的 ref。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitCommitGraphRef {
    pub name: String,
    pub kind: GitCommitGraphRefKind,
}

/// 提交图上的一条提交。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitCommitGraphCommit {
    pub hash: String,
    pub parents: Vec<String>,
    pub refs: Vec<GitCommitGraphRef>,
    pub subject: String,
    pub author_name: Option<String>,
    /// 作者时间的毫秒时间戳；解析不出来时是 `None`（不是 0）。
    pub authored_at_ms: Option<i64>,
}

/// 一页提交图。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitCommitGraphResult {
    pub commits: Vec<GitCommitGraphCommit>,
    /// 是否还有下一页（靠多取一条判断）。
    pub has_more: bool,
}

/// 构造一个「仓库不可用」的空摘要。
///
/// 对应上游 `createEmptySummary()`：`isGitAvailable` 由调用方传入，
/// 其余字段按「不是仓库」填。
pub fn create_empty_summary(
    workspace_path: &str,
    is_git_available: bool,
) -> GitRepositorySummary {
    GitRepositorySummary {
        workspace_path: workspace_path.to_string(),
        repo_root: workspace_path.to_string(),
        workspace_in_repo_path: ".".to_string(),
        auto_refresh_watch_paths: Vec::new(),
        branch_name: None,
        tracking_branch_name: None,
        head_ref_type: GitHeadRefType::Branch,
        ahead: 0,
        behind: 0,
        is_dirty: false,
        is_git_available,
        is_repository: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_strings_match_upstream() {
        // 这些字符串会直接进 DOM 属性 / 协议字段，改了就等于换协议。
        assert_eq!(GitHeadRefType::Branch.as_str(), "branch");
        assert_eq!(GitHeadRefType::Detached.as_str(), "detached");
        assert_eq!(GitChangeKind::Modified.as_str(), "modified");
        assert_eq!(GitChangeKind::Added.as_str(), "added");
        assert_eq!(GitChangeKind::Deleted.as_str(), "deleted");
        assert_eq!(GitChangeKind::Renamed.as_str(), "renamed");
        assert_eq!(GitChangeSectionId::LastTurn.as_str(), "last-turn");
        assert_eq!(GitChangeSectionId::Conflicted.as_str(), "conflicted");
        assert_eq!(
            GitWorkspaceRepositoryKind::LinkedWorktree.as_str(),
            "linked-worktree"
        );
        assert_eq!(GitDiffAvailability::Unavailable.as_str(), "unavailable");
        assert_eq!(GitCommitGraphRefKind::Branch.as_str(), "branch");
        assert_eq!(GitCommitGraphRefKind::Remote.as_str(), "remote");
        assert_eq!(GitCommitGraphRefKind::Tag.as_str(), "tag");
        assert_eq!(GitCommitGraphRefKind::Head.as_str(), "head");
        assert_eq!(GitChangeSourceId::Unstaged.as_str(), "unstaged");
        assert_eq!(GitChangeSourceId::Staged.as_str(), "staged");
        assert_eq!(GitChangeSourceId::Branch.as_str(), "branch");
        assert_eq!(GitChangeSourceId::LastTurn.as_str(), "last-turn");
    }

    #[test]
    fn change_source_defaults_to_unstaged() {
        assert_eq!(GitChangeSourceId::default(), GitChangeSourceId::Unstaged);
    }

    #[test]
    fn empty_summary_is_not_a_repository() {
        let summary = create_empty_summary("/tmp/x", true);
        assert!(summary.is_git_available);
        assert!(!summary.is_repository);
        assert!(!summary.is_dirty);
        assert_eq!(summary.repo_root, "/tmp/x");
        assert_eq!(summary.workspace_in_repo_path, ".");
        assert_eq!(summary.ahead, 0);
        assert!(summary.branch_name.is_none());
        assert!(summary.auto_refresh_watch_paths.is_empty());
    }

    #[test]
    fn empty_summary_can_report_missing_git() {
        let summary = create_empty_summary("/tmp/x", false);
        assert!(!summary.is_git_available);
        assert!(!summary.is_repository);
    }

    #[test]
    fn defaults_are_branch_and_modified() {
        assert_eq!(GitHeadRefType::default(), GitHeadRefType::Branch);
        assert_eq!(GitChangeKind::default(), GitChangeKind::Modified);
    }
}
