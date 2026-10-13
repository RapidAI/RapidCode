//! Git 仓库解析与工作区状态查询。
//!
//! Rust 移植自 `packages/services/src/git/repo/gitCliRepo.ts` 的只读部分：
//! `resolveRepository` / `getStatus` / `listLocalBranches`。
//!
//! ## 没有移植的两样东西
//!
//! 1. **`reuseInFlightRequest`**：上游用「同一 key 的并发请求复用同一个 Promise」
//!    把一轮渲染里的多次 `git rev-parse` / `git status` 合并成一次。那是 JS 事件循环
//!    的产物；Rust 侧的调用方（应用层后台线程）本来就是一问一答，没有 in-flight
//!    窗口，因此这里不做缓存 —— 也就没有 `invalidate(workspacePath)` 的必要。
//! 2. **写操作**（stage / unstage / discard / commit / push / switch / createBranch）。
//!    它们会改用户的仓库，超出当前只读链路的目标。
//!
//! 唯一保留的「记忆」是未跟踪目录折叠：大仓库的逐文件未跟踪状态会撑爆输出上限，
//! 上游首次超限后按 repoRoot 记住折叠模式，避免每次刷新都重跑一遍必然失败的详细命令。
//! 这是纯粹的省时优化，与并发模型无关，因此照搬。

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use zcode_shared::git::{
    GitHeadRefType, GitLocalBranch, GitLocalBranchListResult, GitRepositoryAutoRefreshWatchPath,
    GitRepositorySummary,
};

use super::cli_helpers::{
    build_untracked_stats, ensure_git_command_succeeded, is_missing_working_directory_result,
    is_not_repository_result, parse_numstat, parse_status_porcelain, GitCommandFailure,
    GitCommandFailureKind,
};
use super::config::{
    normalize_workspace_in_repo_path, DEFAULT_GIT_COMMAND_TIMEOUT_MS, DEFAULT_GIT_OUTPUT_BYTES,
};
use super::provider::{GitCommandProvider, GitCommandRequest};
use super::types::{GitResolvedRepository, GitStatusSnapshot};

/// `git status` 的未跟踪模式。
///
/// `All` 逐文件列出；`Normal` 只列目录 —— 后者是超限后的降级形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UntrackedMode {
    All,
    Normal,
}

impl UntrackedMode {
    fn as_arg(&self) -> &'static str {
        match self {
            UntrackedMode::All => "--untracked-files=all",
            UntrackedMode::Normal => "--untracked-files=normal",
        }
    }
}

/// Git 只读仓库访问。
///
/// `Send + Sync`，可以直接放进 `Arc` 交给后台线程。
#[derive(Debug)]
pub struct GitCliRepo {
    provider: GitCommandProvider,
    /// 已经确认「逐文件未跟踪状态超限」的 repoRoot。
    collapsed_untracked_repo_roots: Mutex<HashSet<String>>,
}

impl Default for GitCliRepo {
    fn default() -> Self {
        Self::new()
    }
}

impl GitCliRepo {
    pub fn new() -> Self {
        Self {
            provider: GitCommandProvider::new(),
            collapsed_untracked_repo_roots: Mutex::new(HashSet::new()),
        }
    }

    pub fn with_provider(provider: GitCommandProvider) -> Self {
        Self {
            provider,
            collapsed_untracked_repo_roots: Mutex::new(HashSet::new()),
        }
    }

    /// 共享底层的命令执行器（二进制路径缓存挂在它身上）。
    pub fn provider(&self) -> &GitCommandProvider {
        &self.provider
    }

    /// 解析 workspace 与仓库的关系。
    ///
    /// 与上游一致，**不是仓库不算错误**：返回 `is_repository = false` 的降级结果，
    /// 让 UI 显示「当前目录不是 Git 仓库」而不是弹一个错误。
    pub fn resolve_repository(
        &self,
        workspace_path: &Path,
    ) -> Result<GitResolvedRepository, GitCommandFailure> {
        let workspace = workspace_path.to_string_lossy().into_owned();

        if self.provider.resolve_git_binary().is_none() {
            return Ok(GitResolvedRepository::unavailable(&workspace, false));
        }

        let result = self.provider.run(
            &GitCommandRequest::new(
                workspace_path,
                [
                    "rev-parse",
                    "--show-toplevel",
                    "--show-prefix",
                    "--absolute-git-dir",
                    "--git-common-dir",
                ],
            )
            .with_timeout_ms(DEFAULT_GIT_COMMAND_TIMEOUT_MS)
            .with_max_output_bytes(DEFAULT_GIT_OUTPUT_BYTES),
        )?;

        if result.exit_code != Some(0) {
            // 测试/切换工作区时目录可能被并发删除。目录缺失不属于「Git 协议失败」，
            // 应按「当前非可用仓库」降级返回，而不是把异常抛给上层。
            if is_missing_working_directory_result(&result) || is_not_repository_result(&result) {
                return Ok(GitResolvedRepository::unavailable(&workspace, true));
            }

            ensure_git_command_succeeded("git rev-parse", &result)?;
        }

        let stdout = result.stdout.replace("\r\n", "\n");
        let mut lines = stdout.split('\n');
        let repo_root = lines.next().unwrap_or("").trim();
        if repo_root.is_empty() {
            return Err(GitCommandFailure {
                label: "git rev-parse".to_string(),
                kind: GitCommandFailureKind::NonZeroExit,
                message: "Failed to resolve Git repository root".to_string(),
            });
        }

        // 注意 `--show-prefix` 这一行**不 trim**：仓库根时它就是空行，
        // 归一化函数会把空串变成 "."。多 trim 一层反而会丢掉「这是个空行」的信息。
        let workspace_in_repo_path = normalize_workspace_in_repo_path(lines.next().unwrap_or(""));
        let absolute_git_dir = lines.next().unwrap_or("").trim().to_string();
        let git_common_dir = lines.next().unwrap_or("").trim().to_string();

        Ok(GitResolvedRepository {
            workspace_path: workspace,
            repo_root: repo_root.to_string(),
            workspace_in_repo_path,
            auto_refresh_watch_paths: build_auto_refresh_watch_paths(
                workspace_path,
                &absolute_git_dir,
                &git_common_dir,
            ),
            is_git_available: true,
            is_repository: true,
        })
    }

    /// 取一份完整的工作区状态快照。
    ///
    /// 一次查询里跑三条命令：`status --porcelain=v2 --branch -z`、
    /// `diff --cached --numstat`、`diff --numstat`，再把未跟踪文件的行数补上。
    pub fn get_status(
        &self,
        workspace_path: &Path,
    ) -> Result<GitStatusSnapshot, GitCommandFailure> {
        let resolution = self.resolve_repository(workspace_path)?;
        if !resolution.is_usable() {
            return Ok(GitStatusSnapshot::empty(resolution));
        }

        let repo_root = PathBuf::from(&resolution.repo_root);

        let status_result = self.run_git_status(&resolution)?;
        let staged_stats_result = self.provider.run(
            &GitCommandRequest::new(
                &repo_root,
                [
                    "diff",
                    "--cached",
                    "--numstat",
                    "-z",
                    "--find-renames",
                    "--",
                ],
            )
            .with_timeout_ms(DEFAULT_GIT_COMMAND_TIMEOUT_MS)
            .with_max_output_bytes(DEFAULT_GIT_OUTPUT_BYTES),
        )?;
        let unstaged_stats_result = self.provider.run(
            &GitCommandRequest::new(
                &repo_root,
                ["diff", "--numstat", "-z", "--find-renames", "--"],
            )
            .with_timeout_ms(DEFAULT_GIT_COMMAND_TIMEOUT_MS)
            .with_max_output_bytes(DEFAULT_GIT_OUTPUT_BYTES),
        )?;

        ensure_git_command_succeeded("git status", &status_result)?;
        ensure_git_command_succeeded("git diff --cached --numstat", &staged_stats_result)?;
        ensure_git_command_succeeded("git diff --numstat", &unstaged_stats_result)?;

        let parsed_status = parse_status_porcelain(&status_result.stdout);
        let untracked_stats = build_untracked_stats(&repo_root, &parsed_status.entries);

        Ok(GitStatusSnapshot {
            summary: GitRepositorySummary {
                workspace_path: resolution.workspace_path.clone(),
                repo_root: resolution.repo_root.clone(),
                workspace_in_repo_path: resolution.workspace_in_repo_path.clone(),
                auto_refresh_watch_paths: resolution.auto_refresh_watch_paths.clone(),
                branch_name: parsed_status.branch_name,
                tracking_branch_name: parsed_status.tracking_branch_name,
                head_ref_type: parsed_status.head_ref_type,
                ahead: parsed_status.ahead,
                behind: parsed_status.behind,
                is_dirty: !parsed_status.entries.is_empty(),
                is_git_available: true,
                is_repository: true,
            },
            entries: parsed_status.entries,
            staged_stats: parse_numstat(&staged_stats_result.stdout),
            unstaged_stats: parse_numstat(&unstaged_stats_result.stdout),
            untracked_stats,
            resolution,
        })
    }

    /// 列出本地分支，当前分支排最前，其余按提交时间倒序。
    pub fn list_local_branches(
        &self,
        workspace_path: &Path,
    ) -> Result<GitLocalBranchListResult, GitCommandFailure> {
        let status = self.get_status(workspace_path)?;
        if !status.resolution.is_usable() {
            return Ok(GitLocalBranchListResult {
                head_ref_type: status.summary.head_ref_type,
                current_branch_name: status.summary.branch_name,
                branches: Vec::new(),
            });
        }

        let result = self.provider.run(
            &GitCommandRequest::new(
                &status.resolution.repo_root,
                [
                    "for-each-ref",
                    "refs/heads",
                    "--format=%(refname:short)%00%(upstream:short)%00%(objectname)%00%(committerdate:unix)",
                ],
            )
            .with_timeout_ms(DEFAULT_GIT_COMMAND_TIMEOUT_MS)
            .with_max_output_bytes(DEFAULT_GIT_OUTPUT_BYTES),
        )?;
        ensure_git_command_succeeded("git for-each-ref refs/heads", &result)?;

        let current_branch_name = match status.summary.head_ref_type {
            GitHeadRefType::Branch => status.summary.branch_name.clone(),
            GitHeadRefType::Detached => None,
        };
        let branches = parse_branch_ref_records(&result.stdout, current_branch_name.as_deref());

        Ok(GitLocalBranchListResult {
            head_ref_type: status.summary.head_ref_type,
            current_branch_name,
            branches,
        })
    }

    /// 跑 `git status`，必要时降级到目录折叠模式。
    fn run_git_status(
        &self,
        resolution: &GitResolvedRepository,
    ) -> Result<super::provider::GitCommandExecutionResult, GitCommandFailure> {
        let repo_root = PathBuf::from(&resolution.repo_root);
        let use_collapsed = {
            let collapsed = lock_unpoisoned(&self.collapsed_untracked_repo_roots);
            collapsed.contains(&resolution.repo_root)
        };

        let mode = if use_collapsed {
            UntrackedMode::Normal
        } else {
            UntrackedMode::All
        };
        let result = self.execute_git_status(&repo_root, mode)?;
        if use_collapsed || !result.output_truncated {
            return Ok(result);
        }

        // 大仓库的逐文件未跟踪状态可能超过输出上限；直接放大上限会让后续行数统计
        // 并发读取上万个文件。首次超限后按 repoRoot 记住目录折叠模式，
        // 既保留可用的摘要与变更入口，也避免每次刷新都重跑一遍必然失败的命令。
        {
            let mut collapsed = lock_unpoisoned(&self.collapsed_untracked_repo_roots);
            collapsed.insert(resolution.repo_root.clone());
        }
        self.execute_git_status(&repo_root, UntrackedMode::Normal)
    }

    fn execute_git_status(
        &self,
        repo_root: &Path,
        mode: UntrackedMode,
    ) -> Result<super::provider::GitCommandExecutionResult, GitCommandFailure> {
        Ok(self.provider.run(
            &GitCommandRequest::new(
                repo_root,
                [
                    "status",
                    "--porcelain=v2",
                    "--branch",
                    mode.as_arg(),
                    "-z",
                ],
            )
            .with_timeout_ms(DEFAULT_GIT_COMMAND_TIMEOUT_MS)
            .with_max_output_bytes(DEFAULT_GIT_OUTPUT_BYTES),
        )?)
    }
}

/// 解析 `git for-each-ref refs/heads` 的输出。
///
/// 每行四段，NUL 分隔：短名 / 上游 / 提交号 / 提交时间（Unix 秒）。
pub fn parse_branch_ref_records(stdout: &str, current_branch_name: Option<&str>) -> Vec<GitLocalBranch> {
    let normalized = stdout.replace("\r\n", "\n");
    let mut branches: Vec<GitLocalBranch> = Vec::new();

    for line in normalized.split('\n').filter(|line| !line.is_empty()) {
        let fields: Vec<&str> = line.split('\0').collect();
        let Some(name) = fields.first().copied().filter(|name| !name.is_empty()) else {
            continue;
        };

        let upstream_name = fields.get(1).copied().filter(|value| !value.is_empty());
        let commit_hash = fields.get(2).copied().filter(|value| !value.is_empty());
        let commit_timestamp_ms = fields
            .get(3)
            .and_then(|value| value.parse::<i64>().ok())
            .map(|seconds| seconds * 1000);

        branches.push(GitLocalBranch {
            name: name.to_string(),
            is_current: Some(name) == current_branch_name,
            upstream_name: upstream_name.map(str::to_string),
            commit_hash: commit_hash.map(str::to_string),
            commit_timestamp_ms,
        });
    }

    branches.sort_by(|left, right| {
        if left.is_current != right.is_current {
            return if left.is_current {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }

        // 时间未知的排到最后（对应上游的 -Infinity）。
        let left_time = left.commit_timestamp_ms.unwrap_or(i64::MIN);
        let right_time = right.commit_timestamp_ms.unwrap_or(i64::MIN);
        if left_time != right_time {
            return right_time.cmp(&left_time);
        }

        // 上游用 localeCompare；这里用字节序，保证跨平台稳定（同样的分支集合，
        // 在 zh-CN 与 C locale 下排序一致）。
        left.name.cmp(&right.name)
    });

    branches
}

/// 构造 Git 元数据的刷新边界。
///
/// 只输出 Git 元数据目录，不输出 workspacePath：Linux 上对整棵 workspace 做
/// recursive watch 会为每个子目录分配 watcher，慢挂载或大型生成目录会把
/// workspace Host 拖死。workspace 内容的 watcher 由 UI 按平台自己决定。
fn build_auto_refresh_watch_paths(
    workspace_path: &Path,
    absolute_git_dir: &str,
    git_common_dir: &str,
) -> Vec<GitRepositoryAutoRefreshWatchPath> {
    let mut paths = Vec::new();

    // Git 元数据可能在 linked worktree 或 separate git-dir 里位于 repoRoot 之外，
    // 所以用 Git 自己解析出的目录作为刷新边界，而不是猜 `.git` 布局。
    add_auto_refresh_watch_path(&mut paths, absolute_git_dir);

    let resolved_common_dir = if git_common_dir.is_empty() {
        absolute_git_dir.to_string()
    } else if Path::new(git_common_dir).is_absolute() {
        git_common_dir.to_string()
    } else {
        // `--git-common-dir` 的相对结果以**命令 cwd** 为基准，也就是 workspacePath。
        // 子目录 workspace 若误用 repoRoot，会把 `<root>` + `../.git` 解析成 `<parent>/.git`。
        normalize_lexically(&workspace_path.join(git_common_dir))
            .to_string_lossy()
            .into_owned()
    };
    add_auto_refresh_watch_path(&mut paths, &resolved_common_dir);

    paths
}

fn add_auto_refresh_watch_path(paths: &mut Vec<GitRepositoryAutoRefreshWatchPath>, path: &str) {
    let normalized = normalize_watch_path(path);
    if normalized.is_empty() || paths.iter().any(|entry| entry.path == normalized) {
        return;
    }

    paths.push(GitRepositoryAutoRefreshWatchPath {
        path: normalized,
        recursive: true,
    });
}

/// 去掉路径末尾的分隔符，但保留文件系统根。
fn normalize_watch_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed == "/" || is_windows_drive_root(trimmed) {
        return trimmed.to_string();
    }

    trimmed.trim_end_matches(['\\', '/']).to_string()
}

/// `C:`、`C:\`、`C:/` 这类「盘符根」不能被削成 `C`。
fn is_windows_drive_root(value: &str) -> bool {
    let bytes = value.as_bytes();
    let has_root = bytes.len() == 2
        || (bytes.len() == 3 && (bytes[2] == b'\\' || bytes[2] == b'/'));
    has_root
        && bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
}

/// 纯词法地消解 `.` / `..`（对应 Node 的 `path.resolve`）。
///
/// 刻意不用 [`std::fs::canonicalize`]：它要求路径存在，且在 Windows 上会加上
/// `\\?\` 前缀，两者都不是我们想要的。
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push("..");
                }
            }
            Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// 加锁时忽略中毒状态：集合里只有字符串，中毒也不会让它变成坏数据。
fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_branch_ref_records_with_all_fields() {
        let stdout = "main\0origin/main\0abc123\01700000000\n\
                      rust-only\0\0def456\01700001000\n\
                      orphan\0\0\0\n";
        let branches = parse_branch_ref_records(stdout, Some("rust-only"));

        assert_eq!(branches.len(), 3);
        // 当前分支排最前
        assert_eq!(branches[0].name, "rust-only");
        assert!(branches[0].is_current);
        assert!(branches[0].upstream_name.is_none());
        assert_eq!(branches[0].commit_hash.as_deref(), Some("def456"));
        assert_eq!(branches[0].commit_timestamp_ms, Some(1_700_001_000_000));

        // 其余按时间倒序
        assert_eq!(branches[1].name, "main");
        assert!(!branches[1].is_current);
        assert_eq!(branches[1].upstream_name.as_deref(), Some("origin/main"));

        // 无时间/无提交号也能解析
        assert_eq!(branches[2].name, "orphan");
        assert!(branches[2].commit_hash.is_none());
        assert!(branches[2].commit_timestamp_ms.is_none());
    }

    #[test]
    fn branch_sort_falls_back_to_name_for_equal_timestamps() {
        let stdout = "b\0\0h\0100\n\
                      a\0\0h\0100\n";
        let branches = parse_branch_ref_records(stdout, None);
        assert_eq!(
            branches.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn branch_records_skip_empty_lines_and_nameless_records() {
        let branches = parse_branch_ref_records("\n\0\0\0\0\nmain\0\0h\010\n", None);
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].name, "main");
    }

    #[test]
    fn detached_head_has_no_current_branch() {
        let stdout = "main\0\0h\010\n";
        let branches = parse_branch_ref_records(stdout, None);
        assert!(!branches[0].is_current);
    }

    #[test]
    fn untracked_mode_arguments_match_upstream() {
        assert_eq!(UntrackedMode::All.as_arg(), "--untracked-files=all");
        assert_eq!(UntrackedMode::Normal.as_arg(), "--untracked-files=normal");
    }

    #[test]
    fn normalize_watch_path_keeps_filesystem_roots() {
        assert_eq!(normalize_watch_path("/"), "/");
        assert_eq!(normalize_watch_path("C:/"), "C:/");
        assert_eq!(normalize_watch_path("C:"), "C:");
        assert_eq!(normalize_watch_path("/repo/.git/"), "/repo/.git");
        assert_eq!(normalize_watch_path(r"C:\repo\.git\\"), r"C:\repo\.git");
        assert_eq!(normalize_watch_path("  /repo/.git  "), "/repo/.git");
    }

    #[test]
    fn normalize_lexically_resolves_dot_segments() {
        assert_eq!(
            normalize_lexically(Path::new("/repo/sub/../.git")),
            PathBuf::from("/repo/.git")
        );
        assert_eq!(
            normalize_lexically(Path::new("/repo/./a/b/")),
            PathBuf::from("/repo/a/b")
        );
        // 起点就是 .. 时保留，不越过根
        assert_eq!(normalize_lexically(Path::new("../x")), PathBuf::from("../x"));
    }

    #[test]
    fn watch_paths_use_workspace_as_base_for_relative_common_dir() {
        let paths = build_auto_refresh_watch_paths(
            Path::new("/repo/sub"),
            "/repo/sub/.git",
            "../.git",
        );
        // 相对 common-dir 以 workspacePath 为基准 → <repo>/.git
        // 期望值走同一个归一化函数（Windows 上根分隔符会被规范成 `\`）。
        let expected_common_dir = normalize_lexically(Path::new("/repo/.git"));
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0].path, "/repo/sub/.git");
        assert_eq!(paths[1].path, expected_common_dir.to_string_lossy());
        assert!(paths.iter().all(|entry| entry.recursive));
    }

    #[test]
    fn watch_paths_dedupe_and_fall_back_to_absolute_git_dir() {
        let paths = build_auto_refresh_watch_paths(Path::new("/repo"), "/repo/.git", "");
        assert_eq!(paths.len(), 1, "common-dir 为空时退回 absolute-git-dir，应当去重");
        assert_eq!(paths[0].path, "/repo/.git");
    }

    #[test]
    fn watch_paths_skip_empty_absolute_git_dir() {
        let paths = build_auto_refresh_watch_paths(Path::new("/repo"), "", "");
        assert!(paths.is_empty());
    }
}
