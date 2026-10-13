//! Git 命令结果的判定与输出解析。
//!
//! Rust 移植自 `packages/services/src/git/repo/gitCliHelpers.ts`。
//!
//! 这个文件是整条链路里**最值得移植**的部分：全部是纯函数，没有 IO，
//! 却承载了「Git 的机器可读输出 → 应用内部结构」的全部规则。
//! 上游在这里踩过的坑也都保留了注释。
//!
//! ## 一个刻意的实现选择
//!
//! 上游用正则解析 `git status --porcelain=v2` 的固定字段。这里**不用正则**
//! （本 crate 保持零第三方依赖），改成 `splitn` + 字段数校验。两者等价，
//! 因为 porcelain v2 的固定字段保证不含空格，而路径是行尾的剩余部分：
//!
//! ```text
//! 1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>          → 9 段
//! 2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path> → 10 段（下一段是原路径）
//! u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>  → 11 段
//! ```
//!
//! `splitn` 的好处是「路径里的空格」天然安全：切满固定段数之后，
//! 剩下的整段（含空格、含前导空格）就是路径。

use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use zcode_shared::git::{GitChangeKind, GitHeadRefType};

use super::config::{
    canonicalize_best_effort, normalize_git_path, GIT_UNTRACKED_STAT_CHUNK_BYTES,
    GIT_UNTRACKED_STAT_CONCURRENCY, GIT_UNTRACKED_STAT_MAX_BYTES,
};
use super::provider::{GitCommandExecutionResult, GitSpawnErrorKind};
use super::types::{GitLineStat, GitResolvedRepository, GitStatusEntry};

/// [`ensure_git_command_succeeded`] 的默认退出码白名单。
pub const DEFAULT_ALLOWED_EXIT_CODES: [i32; 1] = [0];

/// Git 命令失败的原因分类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitCommandFailureKind {
    TimedOut,
    OutputTruncated,
    /// 退出码不在白名单里（含 spawn 失败导致的 `-2`）。
    NonZeroExit,
    /// 前置条件不满足：没有可用的 git，或 workspace 不在仓库里。
    ///
    /// 上游在写操作入口用 `ensureRepositoryAvailable` 抛 `Error` 表达这一种；
    /// Rust 侧归到同一个错误类型里，调用方只需处理 [`GitCommandFailure`]。
    RepositoryUnavailable,
    /// 传入的路径落在仓库范围之外。
    PathOutsideRepository,
    /// 参数本身非法（目前只有「提交信息为空」）。
    InvalidArgument,
}

/// 一条 Git 命令失败。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitCommandFailure {
    /// 命令的可读标签，例如 `git status`。
    pub label: String,
    pub kind: GitCommandFailureKind,
    /// 可直接进日志/展示的完整描述。
    pub message: String,
}

impl fmt::Display for GitCommandFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for GitCommandFailure {}

/// 让 [`GitCommandError`] 能直接 `?` 进 [`GitCommandFailure`]。
///
/// 实践中这条路径几乎不可达 —— [`super::repo::GitCliRepo`] 在跑命令前会先看
/// `resolve_git_binary()`。留着它是为了「探测通过、执行时却找不到 git」这种竞态，
/// 也让 repo 层不必为每个 `run` 手写一遍映射。
impl From<super::provider::GitCommandError> for GitCommandFailure {
    fn from(error: super::provider::GitCommandError) -> Self {
        Self {
            label: "git".to_string(),
            kind: GitCommandFailureKind::NonZeroExit,
            message: error.to_string(),
        }
    }
}

/// 退出码不在白名单时返回错误；超时与输出超限优先判定。
///
/// 对应上游 `ensureGitCommandSucceeded(label, result, allowedExitCodes = [0])`。
pub fn ensure_git_command_succeeded(
    label: &str,
    result: &GitCommandExecutionResult,
) -> Result<(), GitCommandFailure> {
    ensure_git_command_succeeded_with(label, result, &DEFAULT_ALLOWED_EXIT_CODES)
}

/// 带自定义退出码白名单的版本。
///
/// `git check-ignore` 之类的命令会拿「退出码 1 = 没有命中」当正常结果，
/// 那时白名单要显式放宽。
pub fn ensure_git_command_succeeded_with(
    label: &str,
    result: &GitCommandExecutionResult,
    allowed_exit_codes: &[i32],
) -> Result<(), GitCommandFailure> {
    if result.timed_out {
        // 超时阈值和进程清理耗时不是一回事；日志里同时保留两者，
        // 避免把「15s 触发超时、随后等待清理」误读成配置了更长超时。
        let timeout_ms = result.timeout_elapsed_ms.unwrap_or(result.duration_ms);
        let mut details = vec![format!("elapsed={}ms", result.duration_ms)];
        if let Some(elapsed) = result.timeout_elapsed_ms {
            details.push(format!("killAt={elapsed}ms"));
        }
        if let Some(delay) = result.timeout_close_delay_ms {
            details.push(format!("cleanup={delay}ms"));
        }
        if result.force_kill_attempted {
            details.push("forceKill=true".to_string());
        }
        if result.orphaned {
            details.push("orphaned=true".to_string());
        }
        return Err(GitCommandFailure {
            label: label.to_string(),
            kind: GitCommandFailureKind::TimedOut,
            message: format!(
                "{label} timed out after {timeout_ms}ms ({})",
                details.join(", ")
            ),
        });
    }

    if result.output_truncated {
        return Err(GitCommandFailure {
            label: label.to_string(),
            kind: GitCommandFailureKind::OutputTruncated,
            message: format!("{label} output exceeded limit"),
        });
    }

    match result.exit_code {
        Some(code) if allowed_exit_codes.contains(&code) => Ok(()),
        _ => Err(GitCommandFailure {
            label: label.to_string(),
            kind: GitCommandFailureKind::NonZeroExit,
            message: format!("{label} failed: {}", result.message()),
        }),
    }
}

/// stderr 是否表明「这里不是 Git 仓库」。
///
/// 这不是错误 —— 用户完全可以把 workspace 指到普通目录上，
/// 此时应当降级成「非仓库」而不是报错。
pub fn is_not_repository_result(result: &GitCommandExecutionResult) -> bool {
    let stderr = result.stderr.to_lowercase();
    stderr.contains("not a git repository") || stderr.contains("outside repository")
}

/// 失败是否由「cwd 不存在」引起。
///
/// 上游靠 `exitCode === -2 && stderr.includes("enoent")`；Rust 侧 spawn 失败会
/// 带明确的 [`GitSpawnErrorKind`]，因此先看它，再退回文案匹配（覆盖 git 自己
/// 报「cwd 被删」的情况）。
pub fn is_missing_working_directory_result(result: &GitCommandExecutionResult) -> bool {
    if matches!(
        result.spawn_error.as_ref().map(|error| error.kind),
        Some(GitSpawnErrorKind::MissingWorkingDirectory)
    ) {
        return true;
    }

    let stderr = result.stderr.to_lowercase();
    stderr.contains("unable to read current working directory")
        || stderr.contains("no such file or directory")
}

/// 写操作的前置校验：git 可用 **且** workspace 确实在仓库里。
///
/// 对应上游 `ensureRepositoryAvailable(resolution, label)`。
/// 上游在这里抛 `Error`；Rust 侧归到 [`GitCommandFailureKind::RepositoryUnavailable`]，
/// 让调用方只需要处理一种错误类型。
pub fn ensure_repository_available<'a>(
    resolution: &'a GitResolvedRepository,
    label: &str,
) -> Result<&'a GitResolvedRepository, GitCommandFailure> {
    if !resolution.is_git_available {
        return Err(GitCommandFailure {
            label: label.to_string(),
            kind: GitCommandFailureKind::RepositoryUnavailable,
            message: format!("Cannot {label}: Git binary is not available"),
        });
    }

    if !resolution.is_repository {
        return Err(GitCommandFailure {
            label: label.to_string(),
            kind: GitCommandFailureKind::RepositoryUnavailable,
            message: format!("Cannot {label}: workspace is not inside a Git repository"),
        });
    }

    Ok(resolution)
}

/// 把用户给的路径归一成「仓库相对路径」。
///
/// 对应上游 `normalizeInputPath(resolution, path)`。三条规则：
///
/// 1. 绝对路径直接用；**相对路径以 workspace 为基准**（不是仓库根）——
///    界面上给的就是 workspace 相对路径；
/// 2. 先试 `realpath`（消解符号链接），失败退回词法归一。
///    `discard` 处理的正是「已被删除的文件」，它当然不存在；
/// 3. 落在仓库外一律报错。`git add -- <repo 外的路径>` 会被 git 自己拒掉，
///    但错误信息很不直观，这里提前拦掉。
pub fn normalize_input_path(
    resolution: &GitResolvedRepository,
    path: &str,
) -> Result<String, GitCommandFailure> {
    let raw_absolute = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        Path::new(&resolution.workspace_path).join(path.replace('/', std::path::MAIN_SEPARATOR_STR))
    };
    let absolute = canonicalize_best_effort(&raw_absolute);
    let repo_root = canonicalize_best_effort(Path::new(&resolution.repo_root));

    let repo_relative = absolute
        .strip_prefix(&repo_root)
        .map(|rest| normalize_git_path(&rest.to_string_lossy()))
        .unwrap_or_default();

    if repo_relative.is_empty()
        || repo_relative == "."
        || repo_relative == ".."
        || repo_relative.starts_with("../")
    {
        return Err(GitCommandFailure {
            label: "normalize path".to_string(),
            kind: GitCommandFailureKind::PathOutsideRepository,
            message: format!("Path is outside repository scope: {path}"),
        });
    }

    Ok(repo_relative)
}

/// 批量归一化并去重（保持首次出现的顺序）。
///
/// 对应上游的 `Array.from(new Set(await Promise.all(paths.map(normalizeInputPath))))`。
/// 去重很重要：界面上「已暂存」和「未暂存」两节可能选中同一个文件，
/// 重复的 pathspec 会让 `git add` 报歧义。
pub fn normalize_input_paths(
    resolution: &GitResolvedRepository,
    paths: &[String],
) -> Result<Vec<String>, GitCommandFailure> {
    let mut normalized = Vec::with_capacity(paths.len());
    for path in paths {
        let repo_relative = normalize_input_path(resolution, path)?;
        if !normalized.contains(&repo_relative) {
            normalized.push(repo_relative);
        }
    }
    Ok(normalized)
}

/// 单个状态码归一成变更性质。
pub fn infer_kind_from_status_code(status_code: char) -> GitChangeKind {
    match status_code {
        'A' | '?' => GitChangeKind::Added,
        'D' => GitChangeKind::Deleted,
        'R' | 'C' => GitChangeKind::Renamed,
        _ => GitChangeKind::Modified,
    }
}

/// 由行数统计反推变更性质（numstat 不带状态码时用）。
pub fn infer_kind_from_numstat(stat: &GitLineStat) -> GitChangeKind {
    if let Some(kind) = stat.kind {
        return kind;
    }

    if stat.added > 0 && stat.removed == 0 {
        return GitChangeKind::Added;
    }

    if stat.removed > 0 && stat.added == 0 {
        return GitChangeKind::Deleted;
    }

    GitChangeKind::Modified
}

/// 解析 `# branch.ab +N -M` 里的 ahead / behind。
pub fn parse_branch_ahead_behind(value: &str) -> (u32, u32) {
    let mut ahead = 0u32;
    let mut behind = 0u32;
    for token in value.split_whitespace() {
        if let Some(rest) = token.strip_prefix('+') {
            ahead = leading_digits(rest);
        } else if let Some(rest) = token.strip_prefix('-') {
            behind = leading_digits(rest);
        }
    }
    (ahead, behind)
}

/// [`parse_status_porcelain`] 的产物。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitStatusParseResult {
    pub branch_name: Option<String>,
    pub tracking_branch_name: Option<String>,
    pub head_ref_type: GitHeadRefType,
    pub ahead: u32,
    pub behind: u32,
    pub entries: Vec<GitStatusEntry>,
}

/// 解析 `git status --porcelain=v2 --branch -z` 的输出。
///
/// 用 `-z` 而不是默认输出的理由：`-z` 不做路径引用（不转义、不加引号），
/// 且不受 `core.quotepath` 与本地语言影响，格式稳定。
pub fn parse_status_porcelain(stdout: &str) -> GitStatusParseResult {
    let records: Vec<&str> = stdout
        .split('\0')
        .filter(|record| !record.is_empty())
        .collect();
    let mut result = GitStatusParseResult::default();

    let mut index = 0usize;
    while index < records.len() {
        let record = records[index];
        index += 1;

        if let Some(rest) = record.strip_prefix("# ") {
            if let Some(head) = rest.strip_prefix("branch.head ") {
                if head == "(detached)" {
                    result.branch_name = None;
                    result.head_ref_type = GitHeadRefType::Detached;
                } else {
                    result.branch_name = Some(head.to_string());
                    result.head_ref_type = GitHeadRefType::Branch;
                }
            } else if let Some(upstream) = rest.strip_prefix("branch.upstream ") {
                result.tracking_branch_name = Some(upstream.to_string());
            } else if let Some(ahead_behind) = rest.strip_prefix("branch.ab ") {
                let (ahead, behind) = parse_branch_ahead_behind(ahead_behind);
                result.ahead = ahead;
                result.behind = behind;
            }
            continue;
        }

        if let Some(path) = record.strip_prefix("? ") {
            result.entries.push(GitStatusEntry {
                path: normalize_git_path(path),
                original_path: None,
                kind: GitChangeKind::Added,
                x: None,
                y: Some("?".to_string()),
                is_untracked: true,
                is_conflicted: false,
            });
            continue;
        }

        if record.starts_with("1 ") {
            if let Some(parts) = split_fixed_fields(record, 9) {
                if let Some((x, y)) = split_xy(parts[1]) {
                    result.entries.push(GitStatusEntry {
                        path: normalize_git_path(parts[8]),
                        original_path: None,
                        // X 非 `.` 时以 X 为准（索引里的改动更「原始」），否则看 Y。
                        kind: infer_kind_from_status_code(if x != '.' { x } else { y }),
                        x: Some(x.to_string()),
                        y: Some(y.to_string()),
                        is_untracked: false,
                        is_conflicted: false,
                    });
                }
            }
            continue;
        }

        if record.starts_with("2 ") {
            if let Some(parts) = split_fixed_fields(record, 10) {
                // 重命名/复制的原路径是**紧随其后的一条独立记录**，消费掉它。
                let original_path = records.get(index).copied();
                index += 1;
                if let Some((x, y)) = split_xy(parts[1]) {
                    result.entries.push(GitStatusEntry {
                        path: normalize_git_path(parts[9]),
                        original_path: original_path.map(normalize_git_path),
                        kind: GitChangeKind::Renamed,
                        x: Some(x.to_string()),
                        y: Some(y.to_string()),
                        is_untracked: false,
                        is_conflicted: false,
                    });
                }
            }
            continue;
        }

        if record.starts_with("u ") {
            if let Some(parts) = split_fixed_fields(record, 11) {
                if let Some((x, y)) = split_xy(parts[1]) {
                    result.entries.push(GitStatusEntry {
                        path: normalize_git_path(parts[10]),
                        original_path: None,
                        kind: GitChangeKind::Modified,
                        x: Some(x.to_string()),
                        y: Some(y.to_string()),
                        is_untracked: false,
                        is_conflicted: true,
                    });
                }
            }
        }
        // 其余记录（`!` 被忽略文件等）直接丢弃。
    }

    result
}

/// 解析 `git diff --numstat -z` 的输出。
///
/// 注意这里**不能**过滤空记录：重命名条目本身就是
/// `<added>\t<removed>\t\0<原路径>\0<新路径>\0` —— 第一条记录的路径字段是空的，
/// 空记录只是分隔符。这一点和 [`parse_status_porcelain`] 的处理正好相反。
pub fn parse_numstat(stdout: &str) -> BTreeMap<String, GitLineStat> {
    let records: Vec<&str> = stdout.split('\0').collect();
    let mut stats = BTreeMap::new();

    let mut index = 0usize;
    while index < records.len() {
        let record = records[index];
        index += 1;

        if record.is_empty() {
            continue;
        }

        let fields: Vec<&str> = record.split('\t').collect();
        if fields.len() < 3 {
            continue;
        }

        let added = parse_numstat_value(fields[0]);
        let removed = parse_numstat_value(fields[1]);
        let path_field = fields[2..].join("\t");
        if !path_field.is_empty() {
            stats.insert(
                normalize_git_path(&path_field),
                GitLineStat {
                    added,
                    removed,
                    kind: None,
                    original_path: None,
                },
            );
            continue;
        }

        let original_path = records.get(index).copied().unwrap_or("");
        let renamed_path = records.get(index + 1).copied().unwrap_or("");
        index += 2;
        if renamed_path.is_empty() {
            continue;
        }

        stats.insert(
            normalize_git_path(renamed_path),
            GitLineStat {
                added,
                removed,
                kind: Some(GitChangeKind::Renamed),
                original_path: Some(normalize_git_path(original_path)),
            },
        );
    }

    stats
}

/// numstat 里的一个数值：`-` 表示二进制，按 0 处理。
pub fn parse_numstat_value(value: &str) -> u32 {
    if value == "-" {
        return 0;
    }

    leading_digits(value)
}

/// 统计未跟踪文件的行数。
///
/// 为什么需要它：`git diff --numstat` 对未跟踪文件一无所知，
/// 但 UI 里每个未跟踪文件也要显示 `+N`。规则与上游一致：
///
/// - 不是普通文件、或超过 [`GIT_UNTRACKED_STAT_MAX_BYTES`] → 0（跳过统计，条目仍保留）
/// - 含 NUL 字节（二进制）→ 0
/// - 否则「换行符个数 +（文件末尾没有换行则 +1）」
pub fn build_untracked_stats(
    repo_root: &Path,
    entries: &[GitStatusEntry],
) -> BTreeMap<String, GitLineStat> {
    let untracked: Vec<&GitStatusEntry> = entries
        .iter()
        .filter(|entry| entry.is_untracked)
        .collect();
    if untracked.is_empty() {
        return BTreeMap::new();
    }

    // 上游注释里的原话：并发读整个未跟踪目录、读完之后再识别二进制，
    // 会让 Host 瞬间分配数 GiB。所以固定 worker 数、每个 worker 复用一个
    // 小缓冲，并且逐文件限量读取。
    let next_index = AtomicUsize::new(0);
    let collected: Mutex<Vec<(String, GitLineStat)>> = Mutex::new(Vec::new());
    let worker_count = GIT_UNTRACKED_STAT_CONCURRENCY.min(untracked.len());

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let next_index = &next_index;
            let collected = &collected;
            let untracked = &untracked;
            scope.spawn(move || {
                let mut buffer = vec![0u8; GIT_UNTRACKED_STAT_CHUNK_BYTES];
                loop {
                    let index = next_index.fetch_add(1, Ordering::SeqCst);
                    let Some(entry) = untracked.get(index) else {
                        break;
                    };

                    let absolute_path = join_repo_relative(repo_root, &entry.path);
                    let added = count_untracked_file_lines(&absolute_path, &mut buffer);
                    let mut guard = collected
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    guard.push((
                        entry.path.clone(),
                        GitLineStat {
                            added,
                            removed: 0,
                            kind: None,
                            original_path: None,
                        },
                    ));
                }
            });
        }
    });

    let mut stats = BTreeMap::new();
    for (path, stat) in collected
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
    {
        stats.insert(path, stat);
    }
    stats
}

/// 数一个文件的行数，失败一律返回 0（上游同样把异常吞成 `{added: 0}`）。
fn count_untracked_file_lines(absolute_path: &Path, buffer: &mut [u8]) -> u32 {
    let Ok(metadata) = std::fs::metadata(absolute_path) else {
        return 0;
    };
    if !metadata.is_file() || metadata.len() > GIT_UNTRACKED_STAT_MAX_BYTES {
        return 0;
    }

    let Ok(mut file) = File::open(absolute_path) else {
        return 0;
    };

    let max_bytes = GIT_UNTRACKED_STAT_MAX_BYTES;
    let mut total_bytes: u64 = 0;
    let mut newlines: u32 = 0;
    let mut last_byte: u8 = b'\n';

    while total_bytes <= max_bytes {
        // 文件可能在 stat 之后继续增长；读取预算也要受约束，
        // 多出的那一个字节只用来识别「越界」。
        let remaining = max_bytes + 1 - total_bytes;
        let length = (buffer.len() as u64).min(remaining) as usize;
        if length == 0 {
            return 0;
        }

        let Ok(bytes_read) = file.read(&mut buffer[..length]) else {
            return 0;
        };
        if bytes_read == 0 {
            return if last_byte == b'\n' { newlines } else { newlines + 1 };
        }

        total_bytes += bytes_read as u64;
        if total_bytes > max_bytes {
            return 0;
        }

        for byte in &buffer[..bytes_read] {
            if *byte == 0 {
                return 0;
            }
            if *byte == b'\n' {
                newlines += 1;
            }
        }

        last_byte = buffer[bytes_read - 1];
    }

    0
}

/// 把仓库相对路径拼成绝对路径。
pub(super) fn join_repo_relative(repo_root: &Path, repo_relative_path: &str) -> PathBuf {
    let mut path = repo_root.to_path_buf();
    for segment in normalize_git_path(repo_relative_path).split('/') {
        if !segment.is_empty() && segment != "." {
            path.push(segment);
        }
    }
    path
}

/// 按固定段数切分一条 porcelain 记录。
///
/// 返回 `None` 表示这条记录不符合该格式（上游对应正则不匹配），调用方应跳过。
fn split_fixed_fields(record: &str, count: usize) -> Option<Vec<&str>> {
    let parts: Vec<&str> = record.splitn(count, ' ').collect();
    if parts.len() != count {
        return None;
    }
    // 上游正则里的 `[^ ]+` / `.+` 都不接受空字段，这里保持一致。
    if parts.iter().any(|part| part.is_empty()) {
        return None;
    }
    Some(parts)
}

/// 把一个 XY 码拆成两个字符；必须正好两个。
fn split_xy(xy: &str) -> Option<(char, char)> {
    let mut chars = xy.chars();
    let x = chars.next()?;
    let y = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some((x, y))
}

/// 取字符串开头连续的 ASCII 数字。
fn leading_digits(value: &str) -> u32 {
    let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::provider::{GitCommandExecutionResult, GitSpawnError, GitSpawnErrorKind};

    /// 把若干条 porcelain 记录拼成 `-z` 形式的输出（每条后面都跟一个 NUL）。
    fn porcelain(records: &[&str]) -> String {
        let mut out = String::new();
        for record in records {
            out.push_str(record);
            out.push('\0');
        }
        out
    }

    fn entry<'a>(entries: &'a [GitStatusEntry], path: &str) -> &'a GitStatusEntry {
        entries
            .iter()
            .find(|entry| entry.path == path)
            .unwrap_or_else(|| panic!("没有找到 {path}"))
    }

    #[test]
    fn parses_branch_headers() {
        let parsed = parse_status_porcelain(&porcelain(&[
            "# branch.oid 1d28429f",
            "# branch.head rust-only",
            "# branch.upstream origin/rust-only",
            "# branch.ab +2 -1",
        ]));

        assert_eq!(parsed.branch_name.as_deref(), Some("rust-only"));
        assert_eq!(parsed.tracking_branch_name.as_deref(), Some("origin/rust-only"));
        assert_eq!(parsed.head_ref_type, GitHeadRefType::Branch);
        assert_eq!(parsed.ahead, 2);
        assert_eq!(parsed.behind, 1);
        assert!(parsed.entries.is_empty());
    }

    #[test]
    fn detached_head_has_no_branch_name() {
        let parsed = parse_status_porcelain(&porcelain(&[
            "# branch.oid abc123",
            "# branch.head (detached)",
        ]));
        assert_eq!(parsed.head_ref_type, GitHeadRefType::Detached);
        assert!(parsed.branch_name.is_none());
    }

    #[test]
    fn no_headers_means_clean_branch_defaults() {
        let parsed = parse_status_porcelain(&porcelain(&["? only.txt"]));
        assert_eq!(parsed.head_ref_type, GitHeadRefType::Branch);
        assert_eq!(parsed.ahead, 0);
        assert_eq!(parsed.behind, 0);
        assert!(parsed.branch_name.is_none());
    }

    #[test]
    fn parses_ordinary_changed_entry() {
        let parsed = parse_status_porcelain(&porcelain(&[
            "# branch.head main",
            "1 M. N... 100644 100644 100644 aaa bbb crates/zcode-app/src/main.rs",
        ]));

        let changed = entry(&parsed.entries, "crates/zcode-app/src/main.rs");
        assert_eq!(changed.kind, GitChangeKind::Modified);
        assert_eq!(changed.x.as_deref(), Some("M"));
        assert_eq!(changed.y.as_deref(), Some("."));
        assert!(!changed.is_untracked);
        assert!(!changed.is_conflicted);
        assert!(changed.original_path.is_none());
    }

    #[test]
    fn x_code_wins_over_y_code_for_kind() {
        // X=D（索引里删了）Y=M（工作区又改回来）→ 以 X 为准，判 deleted。
        let parsed = parse_status_porcelain(&porcelain(&[
            "1 DM N... 100644 100644 100644 aaa bbb gone.rs",
        ]));
        assert_eq!(entry(&parsed.entries, "gone.rs").kind, GitChangeKind::Deleted);

        // X=. 时退回看 Y。
        let parsed = parse_status_porcelain(&porcelain(&[
            "1 .A N... 000000 100644 100644 aaa bbb added.rs",
        ]));
        assert_eq!(entry(&parsed.entries, "added.rs").kind, GitChangeKind::Added);

        // C（复制）也归到 renamed。
        let parsed = parse_status_porcelain(&porcelain(&[
            "1 C. N... 100644 100644 100644 aaa bbb copied.rs",
        ]));
        assert_eq!(entry(&parsed.entries, "copied.rs").kind, GitChangeKind::Renamed);
    }

    #[test]
    fn parses_untracked_entry() {
        let parsed = parse_status_porcelain(&porcelain(&["? notes/草稿.md"]));
        let untracked = entry(&parsed.entries, "notes/草稿.md");
        assert_eq!(untracked.kind, GitChangeKind::Added);
        assert!(untracked.is_untracked);
        assert!(untracked.x.is_none());
        assert_eq!(untracked.y.as_deref(), Some("?"));
    }

    #[test]
    fn parses_renamed_entry_and_consumes_original_path_record() {
        let parsed = parse_status_porcelain(&porcelain(&[
            "# branch.head main",
            "2 R. N... 100644 100644 100644 aaa bbb R100 crates/zcode-app/src/new.rs",
            "crates/zcode-app/src/old.rs",
            "? after.rs",
        ]));

        assert_eq!(parsed.entries.len(), 2, "原路径记录不能被当成独立条目");
        let renamed = entry(&parsed.entries, "crates/zcode-app/src/new.rs");
        assert_eq!(renamed.kind, GitChangeKind::Renamed);
        assert_eq!(
            renamed.original_path.as_deref(),
            Some("crates/zcode-app/src/old.rs")
        );
        // 后面的记录仍然被正确读到，说明索引推进没错位
        assert!(entry(&parsed.entries, "after.rs").is_untracked);
    }

    #[test]
    fn parses_unmerged_entry_as_conflicted() {
        let parsed = parse_status_porcelain(&porcelain(&[
            "u UU N... 100644 100644 100644 100644 aaa bbb ccc src/conflict.rs",
        ]));
        let conflicted = entry(&parsed.entries, "src/conflict.rs");
        assert!(conflicted.is_conflicted);
        assert_eq!(conflicted.kind, GitChangeKind::Modified);
        assert_eq!(conflicted.x.as_deref(), Some("U"));
        assert_eq!(conflicted.y.as_deref(), Some("U"));
    }

    #[test]
    fn windows_style_paths_are_normalized() {
        let parsed = parse_status_porcelain(&porcelain(&[
            r"1 M. N... 100644 100644 100644 aaa bbb crates\zcode-app\src\main.rs",
        ]));
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].path, "crates/zcode-app/src/main.rs");
    }

    #[test]
    fn paths_with_spaces_survive() {
        let parsed = parse_status_porcelain(&porcelain(&[
            "1 M. N... 100644 100644 100644 aaa bbb docs/my file name.md",
            "?  leading-space.txt",
        ]));
        assert_eq!(entry(&parsed.entries, "docs/my file name.md").kind, GitChangeKind::Modified);
        assert!(entry(&parsed.entries, " leading-space.txt").is_untracked);
    }

    #[test]
    fn malformed_records_are_skipped_not_fatal() {
        let parsed = parse_status_porcelain(&porcelain(&[
            "1 M. N... 100644 100644 100644 aaa",     // 字段不足
            "1 M. N... 100644 100644 100644 a b ",     // 路径为空
            "! ignored.log",                           // 被忽略的文件
            "x 完全不认识的记录",
            "? 有效.rs",
        ]));
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].path, "有效.rs");
    }

    #[test]
    fn parse_branch_ahead_behind_reads_both_sides() {
        assert_eq!(parse_branch_ahead_behind("+2 -1"), (2, 1));
        assert_eq!(parse_branch_ahead_behind("+0 -0"), (0, 0));
        assert_eq!(parse_branch_ahead_behind("+12"), (12, 0));
        assert_eq!(parse_branch_ahead_behind("-3"), (0, 3));
        assert_eq!(parse_branch_ahead_behind(""), (0, 0));
        assert_eq!(parse_branch_ahead_behind("garbage"), (0, 0));
    }

    #[test]
    fn parses_numstat_records() {
        let stats = parse_numstat("138\t0\tcrates/zcode-ui/src/components/accordion.rs\0\
                                   96\t4\tcrates/zcode-app/src/main.rs\0");
        assert_eq!(stats.len(), 2);
        assert_eq!(stats["crates/zcode-ui/src/components/accordion.rs"].added, 138);
        assert_eq!(stats["crates/zcode-ui/src/components/accordion.rs"].removed, 0);
        assert_eq!(stats["crates/zcode-app/src/main.rs"].removed, 4);
        assert!(stats["crates/zcode-app/src/main.rs"].kind.is_none());
    }

    #[test]
    fn binary_numstat_uses_zero() {
        let stats = parse_numstat("-\t-\tassets/logo.png\0");
        assert_eq!(stats["assets/logo.png"].added, 0);
        assert_eq!(stats["assets/logo.png"].removed, 0);
    }

    #[test]
    fn renamed_numstat_reads_the_following_two_records() {
        // 重命名条目：第一条记录的路径字段是空的，原路径与新路径各占一条记录。
        let stats = parse_numstat("0\t0\t\0crates/old.rs\0crates/new.rs\0");
        assert_eq!(stats.len(), 1);
        let renamed = &stats["crates/new.rs"];
        assert_eq!(renamed.kind, Some(GitChangeKind::Renamed));
        assert_eq!(renamed.original_path.as_deref(), Some("crates/old.rs"));
    }

    #[test]
    fn renamed_numstat_without_new_path_is_skipped() {
        let stats = parse_numstat("0\t0\t\0crates/old.rs\0");
        assert!(stats.is_empty());
    }

    #[test]
    fn numstat_skips_short_and_empty_records() {
        let stats = parse_numstat("\0only-one-field\0a\tb\t\0");
        // "a\tb\t" 的路径为空，会去读后两条记录，都拿不到 → 跳过
        assert!(stats.is_empty());
    }

    #[test]
    fn infer_kind_from_numstat_covers_all_branches() {
        let added = GitLineStat {
            added: 3,
            removed: 0,
            ..Default::default()
        };
        assert_eq!(infer_kind_from_numstat(&added), GitChangeKind::Added);

        let deleted = GitLineStat {
            added: 0,
            removed: 3,
            ..Default::default()
        };
        assert_eq!(infer_kind_from_numstat(&deleted), GitChangeKind::Deleted);

        let modified = GitLineStat {
            added: 3,
            removed: 3,
            ..Default::default()
        };
        assert_eq!(infer_kind_from_numstat(&modified), GitChangeKind::Modified);

        // 显式 kind 优先
        let renamed = GitLineStat {
            added: 3,
            removed: 0,
            kind: Some(GitChangeKind::Renamed),
            original_path: Some("a.rs".into()),
        };
        assert_eq!(infer_kind_from_numstat(&renamed), GitChangeKind::Renamed);
    }

    #[test]
    fn parse_numstat_value_handles_dash_and_garbage() {
        assert_eq!(parse_numstat_value("-"), 0);
        assert_eq!(parse_numstat_value("0"), 0);
        assert_eq!(parse_numstat_value("42"), 42);
        assert_eq!(parse_numstat_value("12abc"), 12);
        assert_eq!(parse_numstat_value("abc"), 0);
    }

    fn succeeded_result() -> GitCommandExecutionResult {
        GitCommandExecutionResult {
            binary_path: "git".into(),
            cwd: ".".into(),
            args: vec![],
            stdout: String::new(),
            stderr: String::new(),
            exit_code: Some(0),
            duration_ms: 12,
            timed_out: false,
            timeout_ms: 15_000,
            timeout_elapsed_ms: None,
            timeout_close_delay_ms: None,
            force_kill_attempted: false,
            orphaned: false,
            output_truncated: false,
            spawn_error: None,
        }
    }

    #[test]
    fn ensure_succeeds_on_zero_exit() {
        assert!(ensure_git_command_succeeded("git status", &succeeded_result()).is_ok());
    }

    #[test]
    fn ensure_allows_whitelisted_non_zero_exit() {
        let mut result = succeeded_result();
        result.exit_code = Some(1);
        assert!(ensure_git_command_succeeded("git check-ignore", &result).is_err());
        assert!(
            ensure_git_command_succeeded_with("git check-ignore", &result, &[0, 1]).is_ok()
        );
    }

    #[test]
    fn ensure_reports_timeout_with_diagnostics() {
        let mut result = succeeded_result();
        result.exit_code = None;
        result.timed_out = true;
        result.duration_ms = 15_400;
        result.timeout_elapsed_ms = Some(15_000);
        result.timeout_close_delay_ms = Some(400);
        result.force_kill_attempted = true;
        result.orphaned = true;

        let failure = ensure_git_command_succeeded("git status", &result).unwrap_err();
        assert_eq!(failure.kind, GitCommandFailureKind::TimedOut);
        assert_eq!(failure.label, "git status");
        for needle in ["timed out after 15000ms", "elapsed=15400ms", "killAt=15000ms", "cleanup=400ms", "forceKill=true", "orphaned=true"] {
            assert!(failure.message.contains(needle), "缺少 `{needle}`：{}", failure.message);
        }
    }

    #[test]
    fn ensure_reports_truncated_output() {
        let mut result = succeeded_result();
        result.output_truncated = true;
        let failure = ensure_git_command_succeeded("git diff", &result).unwrap_err();
        assert_eq!(failure.kind, GitCommandFailureKind::OutputTruncated);
        assert_eq!(failure.message, "git diff output exceeded limit");
    }

    #[test]
    fn ensure_reports_failure_message_from_stderr() {
        let mut result = succeeded_result();
        result.exit_code = Some(128);
        result.stderr = "fatal: not a git repository".into();
        let failure = ensure_git_command_succeeded("git status", &result).unwrap_err();
        assert_eq!(failure.kind, GitCommandFailureKind::NonZeroExit);
        assert_eq!(
            failure.message,
            "git status failed: fatal: not a git repository"
        );
    }

    #[test]
    fn ensure_treats_missing_exit_code_as_failure() {
        let mut result = succeeded_result();
        result.exit_code = None;
        assert!(ensure_git_command_succeeded("git status", &result).is_err());
    }

    #[test]
    fn detects_not_a_repository() {
        let mut result = succeeded_result();
        result.exit_code = Some(128);
        result.stderr = "fatal: not a git repository (or any of the parent directories): .git".into();
        assert!(is_not_repository_result(&result));

        result.stderr = "fatal: ../outside repository".into();
        assert!(is_not_repository_result(&result));

        result.stderr = "fatal: something else".into();
        assert!(!is_not_repository_result(&result));
    }

    #[test]
    fn detects_missing_working_directory_from_spawn_error() {
        let mut result = succeeded_result();
        result.exit_code = Some(crate::git::provider::SPAWN_FAILURE_EXIT_CODE);
        result.spawn_error = Some(GitSpawnError {
            kind: GitSpawnErrorKind::MissingWorkingDirectory,
            message: "The system cannot find the path specified.".into(),
        });
        assert!(is_missing_working_directory_result(&result));
    }

    #[test]
    fn detects_missing_working_directory_from_stderr() {
        let mut result = succeeded_result();
        result.exit_code = Some(128);
        result.stderr = "fatal: Unable to read current working directory: No such file or directory".into();
        assert!(is_missing_working_directory_result(&result));

        result.stderr = "fatal: bad revision".into();
        assert!(!is_missing_working_directory_result(&result));
    }

    #[test]
    fn build_untracked_stats_counts_lines() {
        let dir = std::env::temp_dir().join("zcode-services-untracked-stats");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // 3 行、以换行结尾
        std::fs::write(dir.join("with_newline.txt"), "a\nb\nc\n").unwrap();
        // 2 行、末尾没有换行
        std::fs::write(dir.join("no_newline.txt"), "a\nb").unwrap();
        // 二进制
        std::fs::write(dir.join("binary.bin"), [0u8, 1, 2, 3]).unwrap();

        let entries = vec![
            GitStatusEntry {
                path: "with_newline.txt".into(),
                original_path: None,
                kind: GitChangeKind::Added,
                x: None,
                y: Some("?".into()),
                is_untracked: true,
                is_conflicted: false,
            },
            GitStatusEntry {
                path: "no_newline.txt".into(),
                original_path: None,
                kind: GitChangeKind::Added,
                x: None,
                y: Some("?".into()),
                is_untracked: true,
                is_conflicted: false,
            },
            GitStatusEntry {
                path: "binary.bin".into(),
                original_path: None,
                kind: GitChangeKind::Added,
                x: None,
                y: Some("?".into()),
                is_untracked: true,
                is_conflicted: false,
            },
            GitStatusEntry {
                path: "does-not-exist.txt".into(),
                original_path: None,
                kind: GitChangeKind::Added,
                x: None,
                y: Some("?".into()),
                is_untracked: true,
                is_conflicted: false,
            },
            GitStatusEntry {
                path: "tracked.rs".into(),
                original_path: None,
                kind: GitChangeKind::Modified,
                x: Some("M".into()),
                y: Some(".".into()),
                is_untracked: false,
                is_conflicted: false,
            },
        ];

        let stats = build_untracked_stats(&dir, &entries);
        assert_eq!(stats.len(), 4, "已跟踪文件不该出现在统计里");
        assert_eq!(stats["with_newline.txt"].added, 3);
        assert_eq!(stats["with_newline.txt"].removed, 0);
        assert_eq!(stats["no_newline.txt"].added, 2);
        assert_eq!(stats["binary.bin"].added, 0, "二进制文件不给行数");
        assert_eq!(stats["does-not-exist.txt"].added, 0, "读不到就给 0，条目仍保留");
        assert!(!stats.contains_key("tracked.rs"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_untracked_stats_is_empty_without_untracked_entries() {
        let stats = build_untracked_stats(Path::new("."), &[]);
        assert!(stats.is_empty());
    }

    #[test]
    fn join_repo_relative_builds_absolute_paths() {
        let joined = join_repo_relative(Path::new("/repo"), "a/b/c.rs");
        assert_eq!(joined, Path::new("/repo/a/b/c.rs"));

        let joined = join_repo_relative(Path::new("/repo"), r"a\b\c.rs");
        assert_eq!(joined, Path::new("/repo/a/b/c.rs"));

        // 目录记录（超限降级时 Git 会输出末尾带 `/` 的目录）
        let joined = join_repo_relative(Path::new("/repo"), "build/");
        assert_eq!(joined, Path::new("/repo/build"));
    }

    // ---------------------------------------------------------------
    // 写操作的前置校验与路径归一
    // ---------------------------------------------------------------

    /// 造一个 resolution。路径刻意指向**不存在的目录**，
    /// 这样 `canonicalize` 必然失败、走词法兜底，测试就是纯的、不碰真实文件系统。
    fn resolution(workspace: &str, repo_root: &str, is_git_available: bool, is_repository: bool) -> GitResolvedRepository {
        GitResolvedRepository {
            workspace_path: workspace.to_string(),
            repo_root: repo_root.to_string(),
            workspace_in_repo_path: ".".to_string(),
            auto_refresh_watch_paths: Vec::new(),
            is_git_available,
            is_repository,
        }
    }

    #[test]
    fn ensure_repository_available_reports_missing_git_first() {
        let unavailable = resolution("/repo", "/repo", false, false);
        let failure = ensure_repository_available(&unavailable, "stage paths").unwrap_err();
        assert_eq!(failure.kind, GitCommandFailureKind::RepositoryUnavailable);
        // 「没有 git」要优先于「不是仓库」报出来 —— 否则用户会去查错方向
        assert_eq!(
            failure.message,
            "Cannot stage paths: Git binary is not available"
        );
    }

    #[test]
    fn ensure_repository_available_reports_non_repository() {
        let plain_dir = resolution("/plain", "/plain", true, false);
        let failure = ensure_repository_available(&plain_dir, "commit changes").unwrap_err();
        assert_eq!(failure.kind, GitCommandFailureKind::RepositoryUnavailable);
        assert_eq!(
            failure.message,
            "Cannot commit changes: workspace is not inside a Git repository"
        );
    }

    #[test]
    fn ensure_repository_available_passes_through_usable_resolution() {
        let usable = resolution("/repo", "/repo", true, true);
        let checked = ensure_repository_available(&usable, "stage paths").unwrap();
        assert_eq!(checked.repo_root, "/repo");
    }

    #[test]
    fn normalize_input_path_is_relative_to_workspace_not_repo_root() {
        // workspace 是仓库的子目录：界面上给的是 workspace 相对路径
        let res = resolution("/repo/sub", "/repo", true, true);
        let normalized = normalize_input_path(&res, "a/b.rs").unwrap();
        assert_eq!(normalized, "sub/a/b.rs");
    }

    #[test]
    fn normalize_input_path_converts_windows_separators_and_dot_segments() {
        let res = resolution("/repo", "/repo", true, true);
        // 反斜杠与 `..` 都要被消解；结果统一用 `/`
        let normalized = normalize_input_path(&res, r"src\..\src\a.rs").unwrap();
        assert_eq!(normalized, "src/a.rs");
    }

    #[test]
    fn normalize_input_path_rejects_paths_outside_the_repository() {
        let res = resolution("/repo", "/repo", true, true);
        let failure = normalize_input_path(&res, "../outside.txt").unwrap_err();
        assert_eq!(failure.kind, GitCommandFailureKind::PathOutsideRepository);
        assert!(
            failure.message.contains("outside repository scope"),
            "实际消息：{}",
            failure.message
        );
    }

    #[test]
    fn normalize_input_path_rejects_the_repository_root_itself() {
        let res = resolution("/repo", "/repo", true, true);
        // `.` 会归一到仓库根 → 空相对路径 → 必须拒绝（git 不接受空 pathspec）
        let failure = normalize_input_path(&res, ".").unwrap_err();
        assert_eq!(failure.kind, GitCommandFailureKind::PathOutsideRepository);
    }

    #[test]
    fn normalize_input_paths_dedupes_while_keeping_order() {
        let res = resolution("/repo", "/repo", true, true);
        let paths = vec![
            "b.rs".to_string(),
            "a.rs".to_string(),
            // 同一个文件的两种写法，归一后必须只剩一条
            "./b.rs".to_string(),
            r"a.rs".to_string(),
        ];
        let normalized = normalize_input_paths(&res, &paths).unwrap();
        assert_eq!(normalized, vec!["b.rs".to_string(), "a.rs".to_string()]);
    }

    #[test]
    fn normalize_input_paths_propagates_the_first_bad_path() {
        let res = resolution("/repo", "/repo", true, true);
        let paths = vec!["ok.rs".to_string(), "../bad.rs".to_string()];
        let failure = normalize_input_paths(&res, &paths).unwrap_err();
        assert_eq!(failure.kind, GitCommandFailureKind::PathOutsideRepository);
    }

    #[test]
    fn normalize_input_paths_is_empty_for_no_input() {
        let res = resolution("/repo", "/repo", true, true);
        assert!(normalize_input_paths(&res, &[]).unwrap().is_empty());
    }
}
