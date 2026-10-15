//! Git 仓库解析与工作区状态查询。
//!
//! Rust 移植自 `packages/services/src/git/repo/gitCliRepo.ts`：
//! `resolveRepository` / `getStatus` / `listLocalBranches` / `getIdentity` /
//! `getIgnoredPaths` / `getCommitGraph` / `getDiff`，以及
//! `stage` / `unstage` / `discard` / `commit` / `push` 这几个写操作。
//!
//! ## 没有移植的几样东西
//!
//! 1. **`reuseInFlightRequest`**：上游用「同一 key 的并发请求复用同一个 Promise」
//!    把一轮渲染里的多次 `git rev-parse` / `git status` 合并成一次。那是 JS 事件循环
//!    的产物；Rust 侧的调用方（应用层后台线程）本来就是一问一答，没有 in-flight
//!    窗口，因此这里不做缓存 —— 也就没有 `invalidate(workspacePath)` 的必要。
//! 2. **分支写操作**（`switchBranch` / `createBranchAndSwitch`）与
//!    `getBranchComparison`：各自依赖一套尚未移植的「分支变更结果」类型。
//! 3. **`commitMessageFileScope`** 与 `gitCommitMessageGenerator`：依赖 LLM provider。
//!
//! 唯一保留的「记忆」是未跟踪目录折叠：大仓库的逐文件未跟踪状态会撑爆输出上限，
//! 上游首次超限后按 repoRoot 记住折叠模式，避免每次刷新都重跑一遍必然失败的详细命令。
//! 这是纯粹的省时优化，与并发模型无关，因此照搬。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use zcode_shared::git::{
    GitBranchComparison, GitChangeSourceId, GitCommitGraphResult, GitDiffResult, GitHeadRefType,
    GitIdentity, GitLocalBranch, GitLocalBranchListResult, GitRepositoryAutoRefreshWatchPath,
    GitRepositorySummary,
};

use super::changes::to_branch_comparison_change;
use super::cli_helpers::{
    build_untracked_stats, ensure_git_command_succeeded, ensure_git_command_succeeded_with,
    ensure_repository_available, is_missing_working_directory_result, is_not_repository_result,
    normalize_input_path, normalize_input_paths, parse_git_config_value, parse_numstat,
    parse_status_porcelain, GitCommandFailure, GitCommandFailureKind,
};
use super::config::{
    get_git_null_device_path, normalize_lexically, normalize_workspace_in_repo_path,
    DEFAULT_GIT_COMMAND_TIMEOUT_MS, DEFAULT_GIT_DIFF_BYTES, DEFAULT_GIT_DIFF_TIMEOUT_MS,
    DEFAULT_GIT_OUTPUT_BYTES, DEFAULT_GIT_PUSH_OUTPUT_BYTES, DEFAULT_GIT_PUSH_TIMEOUT_MS,
};
use super::diff::{
    build_untracked_text_diff_result, is_previewable_text, to_complete_diff_contents,
    to_diff_result, to_unavailable_diff, with_diff_contents, GitDiffContents, ToDiffResultOptions,
};
use super::graph::{
    normalize_git_graph_max_count, normalize_git_graph_skip, parse_git_graph_records,
};
use super::provider::{GitCommandProvider, GitCommandRequest};
use super::types::{
    GitCommitResult, GitDiffQuery, GitPushResult, GitResolvedRepository, GitStatusSnapshot,
};

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

    // -----------------------------------------------------------------
    // 其余读操作
    //
    // `getIdentity` / `getIgnoredPaths` / `getCommitGraph` / `getDiff`。
    // 四条共同约定：**「不是仓库」是正常状态而不是错误** —— 一律返回一个
    // 空/降级结果（空身份、空列表、空提交图、`unavailable` diff），
    // 让界面能把「没有仓库」与「命令失败」分开渲染。
    // -----------------------------------------------------------------

    /// 读 `user.name` / `user.email` 及其作用域与来源。
    ///
    /// 用 `--show-scope --show-origin` 一次问出「值 + 它写在哪」，
    /// 这样设置页能显示「来自 global 的 ~/.gitconfig」而不只是一个值。
    pub fn get_identity(&self, workspace_path: &Path) -> Result<GitIdentity, GitCommandFailure> {
        let resolution = self.resolve_repository(workspace_path)?;
        if !resolution.is_usable() {
            return Ok(GitIdentity::default());
        }

        let name_result = self.provider.run(
            &GitCommandRequest::new(
                &resolution.repo_root,
                [
                    "config",
                    "--show-scope",
                    "--show-origin",
                    "--get",
                    "user.name",
                ],
            )
            .with_timeout_ms(DEFAULT_GIT_COMMAND_TIMEOUT_MS),
        )?;
        let email_result = self.provider.run(
            &GitCommandRequest::new(
                &resolution.repo_root,
                [
                    "config",
                    "--show-scope",
                    "--show-origin",
                    "--get",
                    "user.email",
                ],
            )
            .with_timeout_ms(DEFAULT_GIT_COMMAND_TIMEOUT_MS),
        )?;

        let name = parse_git_config_value(&name_result)?;
        let email = parse_git_config_value(&email_result)?;

        Ok(GitIdentity {
            user_name: name.value,
            user_email: email.value,
            name_source: name.source,
            email_source: email.source,
            // 两个键的作用域通常相同，优先用 name 的；name 没配就退到 email 的。
            scope_label: name.scope.or(email.scope),
        })
    }

    /// 在给定的一组路径里挑出**被 ignore 的**那些，返回它们的绝对路径。
    ///
    /// 输入里落在仓库之外的项会被**静默跳过**（上游 `try/catch → null` 的等价物），
    /// 而不是让整次查询失败 —— 文件树里混进一个越界路径不该让所有忽略标记消失。
    pub fn get_ignored_paths(
        &self,
        workspace_path: &Path,
        paths: &[String],
    ) -> Result<Vec<String>, GitCommandFailure> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }

        let resolution = self.resolve_repository(workspace_path)?;
        if !resolution.is_usable() {
            return Ok(Vec::new());
        }

        let mut pairs: Vec<(String, String)> = Vec::with_capacity(paths.len());
        for path in paths {
            let Ok(repo_relative) = normalize_input_path(&resolution, path) else {
                continue;
            };
            pairs.push((absolute_input_path(&resolution, path), repo_relative));
        }
        if pairs.is_empty() {
            return Ok(Vec::new());
        }

        let mut args: Vec<String> = vec!["check-ignore".to_string(), "--".to_string()];
        args.extend(pairs.iter().map(|(_, repo_relative)| repo_relative.clone()));

        let result = self.provider.run(
            &GitCommandRequest::new(&resolution.repo_root, args)
                .with_timeout_ms(DEFAULT_GIT_COMMAND_TIMEOUT_MS)
                .with_max_output_bytes(DEFAULT_GIT_OUTPUT_BYTES),
        )?;

        // 退出码 1 = 一个都没被忽略，是正常结果而不是失败。
        if result.exit_code == Some(1) {
            return Ok(Vec::new());
        }
        ensure_git_command_succeeded("git check-ignore", &result)?;

        // 注意**不能**用 `-z`：`git check-ignore -z` 只能配 `--stdin`，
        // 这里通过 argv 传路径，所以必须解析普通换行输出，否则命令直接失败。
        let ignored: HashSet<String> = result
            .stdout
            .split('\n')
            .map(|line| line.trim_end_matches('\r'))
            .filter(|line| !line.is_empty())
            .map(|line| line.replace('\\', "/"))
            .collect();

        Ok(pairs
            .into_iter()
            .filter(|(_, repo_relative)| ignored.contains(repo_relative))
            .map(|(absolute, _)| absolute)
            .collect())
    }

    /// 取一页提交图。
    ///
    /// 只展示用户可见历史：显式列 `HEAD --branches --tags --remotes`，
    /// **不用 `--all`** —— 否则 `refs/zcode/checkpoints` 之类的内部 hidden ref
    /// 会把检查点提交也画进图里。
    ///
    /// 多取一条（`max-count = max + 1`）来判断 `has_more`，返回前再切掉。
    pub fn get_commit_graph(
        &self,
        workspace_path: &Path,
        max_count: Option<i64>,
        skip: Option<i64>,
    ) -> Result<GitCommitGraphResult, GitCommandFailure> {
        let resolution = self.resolve_repository(workspace_path)?;
        if !resolution.is_usable() {
            return Ok(GitCommitGraphResult {
                commits: Vec::new(),
                has_more: false,
            });
        }

        let normalized_max = normalize_git_graph_max_count(max_count);
        let normalized_skip = normalize_git_graph_skip(skip);
        let skip_arg = format!("--skip={normalized_skip}");
        let max_arg = format!("--max-count={}", normalized_max + 1);

        let result = self.provider.run(
            &GitCommandRequest::new(
                &resolution.repo_root,
                [
                    "log",
                    "HEAD",
                    "--branches",
                    "--tags",
                    "--remotes",
                    "--date-order",
                    "--topo-order",
                    skip_arg.as_str(),
                    max_arg.as_str(),
                    "--format=%H%x00%P%x00%an%x00%at%x00%s%x00%D%x1e",
                ],
            )
            .with_timeout_ms(DEFAULT_GIT_COMMAND_TIMEOUT_MS)
            .with_max_output_bytes(DEFAULT_GIT_OUTPUT_BYTES),
        )?;

        if result.exit_code != Some(0) {
            // 空仓库（还没有任何提交）不是错误，而是一个正常的空图。
            let stderr = result.stderr.to_lowercase();
            if stderr.contains("does not have any commits yet")
                || stderr.contains("your current branch")
                || stderr.contains("bad default revision")
                || stderr.contains("ambiguous argument 'head'")
            {
                return Ok(GitCommitGraphResult {
                    commits: Vec::new(),
                    has_more: false,
                });
            }
            ensure_git_command_succeeded("git log visible refs", &result)?;
        }

        let parsed = parse_git_graph_records(&result.stdout);
        let has_more = parsed.len() > normalized_max;
        Ok(GitCommitGraphResult {
            commits: parsed.into_iter().take(normalized_max).collect(),
            has_more,
        })
    }

    /// 取「当前分支相对其上游」的差异文件列表。
    ///
    /// 对应上游 `getBranchComparison`。这是「这次改动相对远端多了什么」的视图，
    /// 界面把它渲染成**只读**分组（`readonly: true`）—— 因为 diff 的两侧都是
    /// 提交，不存在「暂存/丢弃」这种操作可言。
    ///
    /// ## 没有上游时**不是错误**
    ///
    /// 本地新建的分支、detached HEAD、没有 remote 的仓库，都没有 tracking branch。
    /// 这时返回一份**降级快照**（`changes` 为空、`comparison_label` 为 `None`），
    /// 而不是报错 —— 「还没推过」是仓库的常见正常状态，不该让用户看到一片红。
    /// 注意 `base_ref` 此时也是 `None`，调用方据此判断「这个分组压根不该出现」。
    ///
    /// ## 为什么复用 `get_status` 而不自己算分支名
    ///
    /// tracking branch 的名字、HEAD 是不是 detached、workspace 在不在仓库里
    /// —— 这些 `get_status` 都已经算好了，而且它的判定细节很多（见其文档）。
    /// 再实现一遍必然会与它漂移。代价是多跑一次 `git status`，
    /// 但相比「两个真源不一致」这个风险是划算的（上游也是这么做的）。
    pub fn get_branch_comparison(
        &self,
        workspace_path: &Path,
    ) -> Result<GitBranchComparison, GitCommandFailure> {
        let status = self.get_status(workspace_path)?;

        if !status.resolution.is_usable() {
            return Ok(GitBranchComparison::default());
        }

        // detached HEAD 时没有分支名，但比较仍然成立（`HEAD` 是个合法的 rev）。
        // 这个值**要在「没有上游」的提前返回之前**算好 —— `head_ref` 回答的是
        // 「我们现在在哪个分支上」，与「有没有比较基准」无关，不该被一起丢掉。
        let head_ref = status
            .summary
            .branch_name
            .clone()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "HEAD".to_string());

        let Some(tracking_branch_name) = status
            .summary
            .tracking_branch_name
            .clone()
            .filter(|name| !name.is_empty())
        else {
            return Ok(GitBranchComparison {
                base_ref: None,
                head_ref: Some(head_ref),
                comparison_label: None,
                changes: Vec::new(),
            });
        };

        let range = format!("{tracking_branch_name}...HEAD");
        let result = self.provider.run(
            &GitCommandRequest::new(
                &status.resolution.repo_root,
                ["diff", "--numstat", "-z", "--find-renames", &range, "--"],
            )
            .with_timeout_ms(DEFAULT_GIT_COMMAND_TIMEOUT_MS)
            .with_max_output_bytes(DEFAULT_GIT_OUTPUT_BYTES),
        )?;
        ensure_git_command_succeeded("git diff --numstat upstream...HEAD", &result)?;

        let stats = parse_numstat(&result.stdout);
        let changes = stats
            .iter()
            .filter_map(|(repo_relative_path, stat)| {
                to_branch_comparison_change(
                    &status.resolution,
                    &status.summary.workspace_in_repo_path,
                    repo_relative_path,
                    stat,
                )
            })
            .collect();

        Ok(GitBranchComparison {
            comparison_label: Some(format!(
                "{} -> {}",
                status.summary.branch_name.as_deref().unwrap_or("HEAD"),
                tracking_branch_name
            )),
            base_ref: Some(tracking_branch_name),
            head_ref: Some(head_ref),
            changes,
        })
    }

    /// 取单个文件的 diff（patch + 可选完整内容）。
    ///
    /// 三条来源分别走不同路径（对齐上游 `getDiff`）：
    ///
    /// - `source_id == Branch`：`git diff <tracking>...HEAD`，完整内容取
    ///   `merge-base` 与 `HEAD` 两个 blob；
    /// - `staged`：`git diff --cached`，完整内容取 `HEAD` 与索引（`":path"`）；
    /// - 其余（未跟踪）：`git diff` 拿不到东西，于是**手搓一份最小 unified diff**
    ///   （见 [`build_untracked_text_diff_result`]），只有文本不可预览时才退回
    ///   `git diff --no-index`。
    pub fn get_diff(
        &self,
        workspace_path: &Path,
        query: &GitDiffQuery,
    ) -> Result<GitDiffResult, GitCommandFailure> {
        let resolution = self.resolve_repository(workspace_path)?;
        let absolute_path = absolute_input_path(&resolution, &query.path);

        if !resolution.is_git_available {
            return Ok(to_unavailable_diff(
                &absolute_path,
                "Git binary is not available in the current environment.",
            ));
        }
        if !resolution.is_repository {
            return Ok(to_unavailable_diff(
                &absolute_path,
                "Workspace is not inside a Git repository.",
            ));
        }

        let repo_relative_path = normalize_input_path(&resolution, &query.path)?;
        let repo_root = PathBuf::from(&resolution.repo_root);

        if query.source_id == Some(GitChangeSourceId::Branch) {
            let status = self.get_status(workspace_path)?;
            let Some(tracking_branch_name) = status.summary.tracking_branch_name else {
                return Ok(to_unavailable_diff(
                    &absolute_path,
                    "Current branch does not have an upstream branch.",
                ));
            };

            let range = format!("{tracking_branch_name}...HEAD");
            let result = self.provider.run(
                &GitCommandRequest::new(
                    &repo_root,
                    [
                        "diff",
                        "--no-ext-diff",
                        "--no-color",
                        "--binary",
                        "--find-renames",
                        range.as_str(),
                        "--",
                        repo_relative_path.as_str(),
                    ],
                )
                .with_timeout_ms(DEFAULT_GIT_DIFF_TIMEOUT_MS)
                .with_max_output_bytes(DEFAULT_GIT_DIFF_BYTES),
            )?;

            let parsed = to_diff_result(
                &absolute_path,
                &result,
                ToDiffResultOptions {
                    empty_summary: Some("No branch comparison diff is available for this file."),
                    ..Default::default()
                },
            );
            let contents =
                self.read_branch_diff_contents(&resolution, &repo_relative_path, &tracking_branch_name);
            return Ok(with_diff_contents(parsed, contents));
        }

        let staged = query.staged.unwrap_or(query.source_id == Some(GitChangeSourceId::Staged));
        let mut args: Vec<&str> = vec!["diff"];
        if staged {
            args.push("--cached");
        }
        args.extend([
            "--no-ext-diff",
            "--no-color",
            "--binary",
            "--",
            repo_relative_path.as_str(),
        ]);

        let result = self.provider.run(
            &GitCommandRequest::new(&repo_root, args)
                .with_timeout_ms(DEFAULT_GIT_DIFF_TIMEOUT_MS)
                .with_max_output_bytes(DEFAULT_GIT_DIFF_BYTES),
        )?;

        let parsed = to_diff_result(
            &absolute_path,
            &result,
            ToDiffResultOptions {
                empty_summary: Some("No Git diff is available for this file."),
                ..Default::default()
            },
        );

        // 未跟踪文件根本不在 `git diff` 的视野里 —— 那会得到 `unavailable`。
        // 只有这种情况下才走下面的兜底；已暂存即使空输出也要取完整内容。
        if parsed.availability != zcode_shared::git::GitDiffAvailability::Unavailable || staged {
            let contents = if staged {
                self.read_staged_diff_contents(&resolution, &repo_relative_path)
            } else {
                self.read_unstaged_diff_contents(
                    Path::new(&absolute_path),
                    &resolution,
                    &repo_relative_path,
                )
            };
            return Ok(with_diff_contents(parsed, contents));
        }

        if !Path::new(&absolute_path).exists() {
            return Ok(parsed);
        }

        // 未跟踪文件：手搓最小 unified diff（不用 `--no-index`，理由见函数文档）。
        if let Some(untracked) = read_untracked_text_diff(
            &absolute_path,
            &repo_relative_path,
            DEFAULT_GIT_DIFF_BYTES,
        ) {
            return Ok(untracked);
        }

        let no_index_result = self.provider.run(
            &GitCommandRequest::new(
                &repo_root,
                [
                    "diff",
                    "--no-index",
                    "--no-ext-diff",
                    "--no-color",
                    "--binary",
                    get_git_null_device_path(),
                    absolute_path.as_str(),
                ],
            )
            .with_timeout_ms(DEFAULT_GIT_DIFF_TIMEOUT_MS)
            .with_max_output_bytes(DEFAULT_GIT_DIFF_BYTES),
        )?;

        Ok(to_diff_result(
            &absolute_path,
            &no_index_result,
            ToDiffResultOptions {
                // `git diff --no-index` 用退出码 1 表示「有差异」，那是正常结果。
                allowed_exit_codes: Some(&[0, 1]),
                empty_summary: Some("No previewable diff is available for this file."),
                ..Default::default()
            },
        ))
    }

    // -----------------------------------------------------------------
    // diff 的完整内容（before/after）
    // -----------------------------------------------------------------

    /// 读一个 Git 对象（blob）的文本内容；任何异常都返回 `None`。
    ///
    /// `ref_` 为空串时 `git show ":path"` 取的是**索引**里的版本 —— 暂存区
    /// diff 的「改动后」就是它。
    fn read_git_blob_preview_content(
        &self,
        resolution: &GitResolvedRepository,
        ref_: &str,
        repo_relative_path: &str,
    ) -> Option<String> {
        let spec = format!("{ref_}:{repo_relative_path}");
        let result = self
            .provider
            .run(
                &GitCommandRequest::new(&resolution.repo_root, ["show", spec.as_str()])
                    .with_timeout_ms(DEFAULT_GIT_DIFF_TIMEOUT_MS)
                    .with_max_output_bytes(DEFAULT_GIT_DIFF_BYTES),
            )
            .ok()?;

        if result.timed_out
            || result.output_truncated
            || result.exit_code != Some(0)
            || !is_previewable_text(&result.stdout)
        {
            return None;
        }
        Some(result.stdout)
    }

    /// 暂存区 diff 的完整内容：`HEAD` → 索引。
    fn read_staged_diff_contents(
        &self,
        resolution: &GitResolvedRepository,
        repo_relative_path: &str,
    ) -> Option<GitDiffContents> {
        let before = self.read_git_blob_preview_content(resolution, "HEAD", repo_relative_path);
        let after = self.read_git_blob_preview_content(resolution, "", repo_relative_path);
        to_complete_diff_contents(before, after)
    }

    /// 未暂存 diff 的完整内容：索引 → 工作区。
    fn read_unstaged_diff_contents(
        &self,
        absolute_path: &Path,
        resolution: &GitResolvedRepository,
        repo_relative_path: &str,
    ) -> Option<GitDiffContents> {
        let before = self.read_git_blob_preview_content(resolution, "", repo_relative_path);
        let after = read_working_tree_preview_content(absolute_path);
        to_complete_diff_contents(before, after)
    }

    /// 分支比较 diff 的完整内容：`merge-base` → `HEAD`。
    ///
    /// `merge-base` 拿不到时（比如两个分支没有共同祖先）退回用 tracking 分支本身，
    /// 与上游一致。
    fn read_branch_diff_contents(
        &self,
        resolution: &GitResolvedRepository,
        repo_relative_path: &str,
        tracking_branch_name: &str,
    ) -> Option<GitDiffContents> {
        let merge_base_result = self
            .provider
            .run(
                &GitCommandRequest::new(
                    &resolution.repo_root,
                    ["merge-base", tracking_branch_name, "HEAD"],
                )
                .with_timeout_ms(DEFAULT_GIT_COMMAND_TIMEOUT_MS)
                .with_max_output_bytes(DEFAULT_GIT_OUTPUT_BYTES),
            )
            .ok()?;
        let merge_base = if merge_base_result.exit_code == Some(0) {
            merge_base_result.stdout.trim().to_string()
        } else {
            String::new()
        };
        let before_ref = if merge_base.is_empty() {
            tracking_branch_name.to_string()
        } else {
            merge_base
        };

        let before = self.read_git_blob_preview_content(resolution, &before_ref, repo_relative_path);
        let after = self.read_git_blob_preview_content(resolution, "HEAD", repo_relative_path);
        to_complete_diff_contents(before, after)
    }

    // -----------------------------------------------------------------
    // 写操作
    //
    // 全部对应上游 `gitCliRepo.ts` 里的同名方法。四条共同约定：
    //
    // 1. 先 `ensure_repository_available`，再跑命令 —— 非仓库/无 git 时报的是
    //    「为什么不能做」而不是 git 的原始报错；
    // 2. 路径一律过 `normalize_input_paths`（归一 + 去重 + 越界拦截）；
    // 3. 空路径列表**直接返回成功**，不跑命令（上游同样如此）；
    // 4. 成功后调用方负责重新扫描 —— 上游用 `invalidate(workspacePath)` 清缓存，
    //    本移植没有缓存（见模块文档），所以这里只返回结果。
    // -----------------------------------------------------------------

    /// `git add -- <paths>`。
    pub fn stage(&self, workspace_path: &Path, paths: &[String]) -> Result<(), GitCommandFailure> {
        let resolution = self.resolve_repository(workspace_path)?;
        let resolution = ensure_repository_available(&resolution, "stage paths")?;
        let repo_paths = normalize_input_paths(resolution, paths)?;
        if repo_paths.is_empty() {
            return Ok(());
        }

        let mut args = vec!["add".to_string(), "--".to_string()];
        args.extend(repo_paths);
        let result = self.provider.run(
            &GitCommandRequest::new(&resolution.repo_root, args)
                .with_timeout_ms(DEFAULT_GIT_COMMAND_TIMEOUT_MS),
        )?;
        ensure_git_command_succeeded("git add", &result)
    }

    /// `git restore --staged -- <paths>`。
    pub fn unstage(&self, workspace_path: &Path, paths: &[String]) -> Result<(), GitCommandFailure> {
        let resolution = self.resolve_repository(workspace_path)?;
        let resolution = ensure_repository_available(&resolution, "unstage paths")?;
        let repo_paths = normalize_input_paths(resolution, paths)?;
        if repo_paths.is_empty() {
            return Ok(());
        }

        let mut args = vec![
            "restore".to_string(),
            "--staged".to_string(),
            "--".to_string(),
        ];
        args.extend(repo_paths);
        let result = self.provider.run(
            &GitCommandRequest::new(&resolution.repo_root, args)
                .with_timeout_ms(DEFAULT_GIT_COMMAND_TIMEOUT_MS),
        )?;
        ensure_git_command_succeeded("git restore --staged", &result)
    }

    /// 丢弃改动。
    ///
    /// `staged = true` 时连暂存区一起回退到 HEAD（`--source=HEAD --staged --worktree`），
    /// 否则只回退工作区（`--worktree`）。
    ///
    /// ⚠️ **这是本 crate 里唯一会丢数据的操作**，不可撤销。调用方必须先拿到用户确认。
    pub fn discard(
        &self,
        workspace_path: &Path,
        paths: &[String],
        staged: bool,
    ) -> Result<(), GitCommandFailure> {
        let resolution = self.resolve_repository(workspace_path)?;
        let resolution = ensure_repository_available(&resolution, "discard paths")?;
        let repo_paths = normalize_input_paths(resolution, paths)?;
        if repo_paths.is_empty() {
            return Ok(());
        }

        let mut args: Vec<String> = if staged {
            ["restore", "--source=HEAD", "--staged", "--worktree", "--"]
                .iter()
                .map(|value| (*value).to_string())
                .collect()
        } else {
            ["restore", "--worktree", "--"]
                .iter()
                .map(|value| (*value).to_string())
                .collect()
        };
        args.extend(repo_paths);
        let result = self.provider.run(
            &GitCommandRequest::new(&resolution.repo_root, args)
                .with_timeout_ms(DEFAULT_GIT_COMMAND_TIMEOUT_MS),
        )?;
        ensure_git_command_succeeded("git restore", &result)
    }

    /// `git commit -m <message>`，返回新提交的哈希。
    ///
    /// `paths` 非空时走 `git commit -m <msg> -- <paths>`（只提交这些路径）。
    ///
    /// 上游还有一个 `stagedOnly` 变体：用临时 `GIT_INDEX_FILE` 只把选中的
    /// 已暂存路径写进新提交。**本移植有意未实现**（见 `lib.rs` 的未移植清单），
    /// 因此调用方不要期待「部分提交已暂存内容」这个能力。
    pub fn commit(
        &self,
        workspace_path: &Path,
        message: &str,
        paths: &[String],
    ) -> Result<GitCommitResult, GitCommandFailure> {
        let resolution = self.resolve_repository(workspace_path)?;
        let resolution = ensure_repository_available(&resolution, "commit changes")?;

        let trimmed_message = message.trim();
        if trimmed_message.is_empty() {
            return Err(GitCommandFailure {
                label: "git commit".to_string(),
                kind: GitCommandFailureKind::InvalidArgument,
                message: "Commit message cannot be empty".to_string(),
            });
        }

        let repo_paths = normalize_input_paths(resolution, paths)?;

        let mut args = vec!["commit".to_string(), "-m".to_string(), trimmed_message.to_string()];
        if !repo_paths.is_empty() {
            args.push("--".to_string());
            args.extend(repo_paths);
        }

        let result = self.provider.run(
            &GitCommandRequest::new(&resolution.repo_root, args)
                .with_timeout_ms(DEFAULT_GIT_COMMAND_TIMEOUT_MS)
                .with_max_output_bytes(DEFAULT_GIT_OUTPUT_BYTES),
        )?;
        ensure_git_command_succeeded("git commit", &result)?;

        let hash_result = self.provider.run(
            &GitCommandRequest::new(&resolution.repo_root, ["rev-parse", "HEAD"])
                .with_timeout_ms(DEFAULT_GIT_COMMAND_TIMEOUT_MS),
        )?;
        ensure_git_command_succeeded("git rev-parse HEAD", &hash_result)?;

        Ok(GitCommitResult {
            commit_hash: hash_result.stdout.trim().to_string(),
        })
    }

    /// `git push`（无上游时补 `--set-upstream <remote> <branch>`）。
    ///
    /// 超时与输出上限都用 push 专用的放宽配额：pre-push hook 可能跑完整测试套件，
    /// 用 15s / 512KiB 会把「还在跑」误判成「失败」。
    pub fn push(&self, workspace_path: &Path) -> Result<GitPushResult, GitCommandFailure> {
        let status = self.get_status(workspace_path)?;
        let resolution = ensure_repository_available(&status.resolution, "push changes")?;

        let branch_name = status
            .summary
            .branch_name
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        if status.summary.head_ref_type != GitHeadRefType::Branch || branch_name.is_empty() {
            return Err(GitCommandFailure {
                label: "git push".to_string(),
                kind: GitCommandFailureKind::RepositoryUnavailable,
                message: "Cannot push while HEAD is detached.".to_string(),
            });
        }

        let tracking_branch_name = status.summary.tracking_branch_name.clone();
        let has_tracking_branch = tracking_branch_name
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        let remote_name = if has_tracking_branch {
            tracking_branch_name
                .as_deref()
                .and_then(parse_tracking_remote_name)
        } else {
            Some(self.resolve_push_remote(resolution, &branch_name)?)
        };

        let args: Vec<String> = if has_tracking_branch {
            vec!["push".to_string()]
        } else {
            vec![
                "push".to_string(),
                "--set-upstream".to_string(),
                remote_name.clone().unwrap_or_else(|| "origin".to_string()),
                branch_name.clone(),
            ]
        };

        let result = self.provider.run(
            &GitCommandRequest::new(&resolution.repo_root, args)
                .with_timeout_ms(DEFAULT_GIT_PUSH_TIMEOUT_MS)
                .with_max_output_bytes(DEFAULT_GIT_PUSH_OUTPUT_BYTES),
        )?;
        ensure_git_command_succeeded("git push", &result)?;

        // 重新读一次状态，拿到 push 之后的 tracking 关系（可能刚建立）。
        let next_status = self.get_status(workspace_path)?;
        Ok(GitPushResult {
            branch_name,
            tracking_branch_name: next_status.summary.tracking_branch_name.clone(),
            remote_name: remote_name
                .or_else(|| next_status.summary.tracking_branch_name.as_deref().and_then(parse_tracking_remote_name)),
            set_upstream: !has_tracking_branch,
        })
    }

    /// 决定「没有 upstream 时该推到哪个 remote」。
    ///
    /// 优先级与上游一致：`branch.<name>.remote` → `remote.pushDefault` →
    /// 叫 `origin` 的 remote → 唯一的一个 remote → 报错。
    fn resolve_push_remote(
        &self,
        resolution: &GitResolvedRepository,
        branch_name: &str,
    ) -> Result<String, GitCommandFailure> {
        if let Some(remote) = self.read_optional_git_config(
            resolution,
            &format!("branch.{branch_name}.remote"),
        )? {
            return Ok(remote);
        }

        if let Some(remote) = self.read_optional_git_config(resolution, "remote.pushDefault")? {
            return Ok(remote);
        }

        let remotes = self.list_remotes(resolution)?;
        if remotes.iter().any(|name| name == "origin") {
            return Ok("origin".to_string());
        }

        match remotes.len() {
            0 => Err(GitCommandFailure {
                label: "git push".to_string(),
                kind: GitCommandFailureKind::RepositoryUnavailable,
                message: "No Git remote is configured for the current repository.".to_string(),
            }),
            1 => Ok(remotes[0].clone()),
            _ => Err(GitCommandFailure {
                label: "git push".to_string(),
                kind: GitCommandFailureKind::RepositoryUnavailable,
                message:
                    "Multiple Git remotes are configured. Configure branch.<name>.remote or remote.pushDefault first."
                        .to_string(),
            }),
        }
    }

    /// 读一条 git config；键不存在（退出码 1）返回 `None` 而不是报错。
    fn read_optional_git_config(
        &self,
        resolution: &GitResolvedRepository,
        key: &str,
    ) -> Result<Option<String>, GitCommandFailure> {
        let result = self.provider.run(
            &GitCommandRequest::new(&resolution.repo_root, ["config", "--get", key])
                .with_timeout_ms(DEFAULT_GIT_COMMAND_TIMEOUT_MS),
        )?;

        // 退出码 1 = 键不存在，属于正常情况。
        ensure_git_command_succeeded_with(&format!("git config --get {key}"), &result, &[0, 1])?;
        let value = result.stdout.trim();
        Ok(if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        })
    }

    /// `git remote` —— 每行一个 remote 名。
    fn list_remotes(
        &self,
        resolution: &GitResolvedRepository,
    ) -> Result<Vec<String>, GitCommandFailure> {
        let result = self.provider.run(
            &GitCommandRequest::new(&resolution.repo_root, ["remote"])
                .with_timeout_ms(DEFAULT_GIT_COMMAND_TIMEOUT_MS),
        )?;
        ensure_git_command_succeeded("git remote", &result)?;
        Ok(result
            .stdout
            .split('\n')
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect())
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

/// 从 `origin/main` 这类上游名里取出 remote 名（`origin`）。
///
/// 对应上游 `parseTrackingRemoteName`。取不到（空串 / 没有 `/` 之前的段）时返回
/// `None`，与上游返回 `null` 一致。
pub fn parse_tracking_remote_name(tracking_branch_name: &str) -> Option<String> {
    let remote_name = tracking_branch_name
        .split('/')
        .next()
        .map(str::trim)
        .unwrap_or_default();
    if remote_name.is_empty() {
        None
    } else {
        Some(remote_name.to_string())
    }
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

/// 加锁时忽略中毒状态：集合里只有字符串，中毒也不会让它变成坏数据。
fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 把用户给的路径变成绝对路径（**纯词法**，不做 realpath）。
///
/// 对应上游 `isAbsolute(path) ? path : resolve(workspacePath, path.split("/").join(sep))`。
/// 刻意不做 `canonicalize`：这些绝对路径会被原样回传给调用方（`get_ignored_paths`
/// 用它标记哪些输入被忽略），换个形式反而对不上。
fn absolute_input_path(resolution: &GitResolvedRepository, path: &str) -> String {
    if Path::new(path).is_absolute() {
        return path.to_string();
    }
    Path::new(&resolution.workspace_path)
        .join(path.replace('/', std::path::MAIN_SEPARATOR_STR))
        .to_string_lossy()
        .into_owned()
}

/// 读工作区里一个文件的文本内容；任何异常都返回 `None`。
///
/// 对应上游 `readWorkingTreePreviewContent`。**不能把读失败当成空文件** ——
/// 文件被删除、编辑器原子保存的窗口期都会让 `metadata`/`read` 失败，
/// 猜成空文件会让 UI 把整个文件渲染成「全新增」。
fn read_working_tree_preview_content(absolute_path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(absolute_path).ok()?;
    if !metadata.is_file() || metadata.len() > DEFAULT_GIT_DIFF_BYTES as u64 {
        return None;
    }

    let bytes = std::fs::read(absolute_path).ok()?;
    // 与 Node 的 `Buffer.toString("utf-8")` 一致：非法字节替换成 U+FFFD，而不是报错。
    let content = String::from_utf8_lossy(&bytes).into_owned();
    is_previewable_text(&content).then_some(content)
}

/// 读文件内容并手搓未跟踪文件的 diff；读失败返回 `None`。
///
/// 上游 `buildUntrackedTextDiffResult` 把「读文件」和「拼 patch」放在一起；
/// 这里拆开，拼 patch 的纯逻辑在 [`build_untracked_text_diff_result`] 里。
/// 返回 `None` 表示**文件读不出来**，调用方应退回 `git diff --no-index`。
fn read_untracked_text_diff(
    absolute_path: &str,
    repo_relative_path: &str,
    max_preview_bytes: usize,
) -> Option<GitDiffResult> {
    let content = std::fs::read(absolute_path).ok()?;
    Some(build_untracked_text_diff_result(
        absolute_path,
        repo_relative_path,
        &content,
        max_preview_bytes,
    ))
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
    fn tracking_remote_name_takes_the_first_segment() {
        assert_eq!(
            parse_tracking_remote_name("origin/main"),
            Some("origin".to_string())
        );
        // 远端名里带 `/`（例如 gitlab 的 subgroup）时只取第一段，与上游一致
        assert_eq!(
            parse_tracking_remote_name("upstream/release/1.x"),
            Some("upstream".to_string())
        );
        // 没有 `/` → 整串就是 remote 名
        assert_eq!(
            parse_tracking_remote_name("origin"),
            Some("origin".to_string())
        );
        // 空串 / 只有空白 → None（上游返回 null）
        assert_eq!(parse_tracking_remote_name(""), None);
        assert_eq!(parse_tracking_remote_name("   "), None);
        // `/main` → 第一段是空串 → None
        assert_eq!(parse_tracking_remote_name("/main"), None);
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
