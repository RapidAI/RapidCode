//! Git 服务层的配置常量与路径工具。
//!
//! Rust 移植自 `packages/services/src/git/config.ts`。
//!
//! 这里的常量全部保留，即使当前只读链路用不到（例如 push 的放宽配额）——
//! 它们是「这个服务层的契约」的一部分，删掉会让后续补写操作时无据可依。

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// 探测 `git --version` 的单次超时。
pub const DEFAULT_GIT_DISCOVERY_TIMEOUT_MS: u64 = 3_000;
/// 普通 Git 命令超时。
pub const DEFAULT_GIT_COMMAND_TIMEOUT_MS: u64 = 15_000;
/// `git push` 可能被 pre-push hook（例如 `pnpm test`）阻塞很久，单独放宽。
pub const DEFAULT_GIT_PUSH_TIMEOUT_MS: u64 = 10 * 60_000;
/// diff 类命令的超时。
pub const DEFAULT_GIT_DIFF_TIMEOUT_MS: u64 = 20_000;
/// 普通命令的输出上限。
pub const DEFAULT_GIT_OUTPUT_BYTES: usize = 512 * 1024;
/// push 的 hook 可能吐完整测试日志，单独放宽输出配额。
pub const DEFAULT_GIT_PUSH_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
/// diff 的输出上限（比普通命令大，patch 本来就长）。
pub const DEFAULT_GIT_DIFF_BYTES: usize = 1024 * 1024;
/// 未跟踪文件统计的行数预算；超过就只保留条目、跳过行数。
pub const GIT_UNTRACKED_STAT_MAX_BYTES: u64 = 1024 * 1024;
/// 未跟踪文件统计的读取块大小。
pub const GIT_UNTRACKED_STAT_CHUNK_BYTES: usize = 64 * 1024;
/// 未跟踪文件统计的并发度。
pub const GIT_UNTRACKED_STAT_CONCURRENCY: usize = 4;

/// 覆盖 Git 可执行文件路径的环境变量。
pub const GIT_BINARY_ENV_VAR: &str = "ZCODE_GIT_BINARY";

/// pre-push hook 会向子进程注入当前仓库的 `GIT_DIR` / `GIT_WORK_TREE` 等 local env。
/// 原样透传会让 Git 命令「串仓」到 hook 所在仓库，因此统一先清掉。
pub const GIT_LOCAL_ENV_VARS: [&str; 16] = [
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_CONFIG",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
    "GIT_DIR",
    "GIT_GRAFT_FILE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_INTERNAL_SUPER_PREFIX",
    "GIT_NAMESPACE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_PREFIX",
    "GIT_REPLACE_REF_BASE",
    "GIT_SHALLOW_FILE",
    "GIT_WORK_TREE",
];

/// 在清理 local env 之后再叠加的 ZCode 约束变量。
///
/// - `GIT_OPTIONAL_LOCKS=0`：只读命令不去抢 `index.lock`，避免和用户正在跑的 Git 打架。
/// - `GIT_PAGER` / `PAGER=cat`：否则 `git` 会试图起分页器并挂住管道。
/// - `TERM=dumb` + `LC_ALL` / `LANG=C`：输出格式稳定，不受本地语言与终端能力影响。
pub const GIT_COMMAND_ENV_OVERRIDES: [(&str, &str); 6] = [
    ("GIT_OPTIONAL_LOCKS", "0"),
    ("GIT_PAGER", "cat"),
    ("PAGER", "cat"),
    ("TERM", "dumb"),
    ("LC_ALL", "C"),
    ("LANG", "C"),
];

/// 把 ZCode 的 Git 环境约束应用到一条待执行的命令上。
///
/// 上游是「先构造完整 env 对象，再整体替换子进程环境」；这里用
/// [`Command::env_remove`] + [`Command::env`] 达到同样效果，好处是不必
/// `env_clear()` —— 清空继承环境在 Windows 上会连 `SystemRoot` 一起丢掉。
///
/// 顺序有讲究：`env_remove` 必须在 `env` 之前，后写入的覆盖先写入的。
pub fn apply_git_command_env(command: &mut Command) {
    for name in GIT_LOCAL_ENV_VARS {
        command.env_remove(name);
    }
    for (name, value) in GIT_COMMAND_ENV_OVERRIDES {
        command.env(name, value);
    }
}

/// 当前平台的空设备路径。
pub fn get_git_null_device_path() -> &'static str {
    if cfg!(windows) {
        "NUL"
    } else {
        "/dev/null"
    }
}

/// Git 可执行文件的候选列表，按优先级排列。
///
/// 与上游一致：`ZCODE_GIT_BINARY` → `git`（PATH）→ Windows 上的四个常见安装位置。
pub fn get_git_binary_candidates() -> Vec<OsString> {
    let mut candidates: Vec<OsString> = Vec::new();
    if let Some(override_path) = non_empty_env(GIT_BINARY_ENV_VAR) {
        candidates.push(override_path);
    }
    candidates.push(OsString::from("git"));

    #[cfg(windows)]
    {
        // 顺序与上游一致：先 ProgramW6432（64 位进程看到的是真正的 Program Files），
        // 再 ProgramFiles(x86)；每个根下面先 cmd 再 bin。
        for base in [program_files(), program_files_x86()] {
            for sub in ["cmd", "bin"] {
                candidates.push(
                    base.join("Git")
                        .join(sub)
                        .join("git.exe")
                        .into_os_string(),
                );
            }
        }
    }

    candidates
}

/// 把路径里的反斜杠统一成 `/`。
///
/// Git 自己的输出恒用 `/`；只有 Windows 上从 `rev-parse` 拿到的绝对路径会是 `\`，
/// 所以入库前统一一次。
pub fn normalize_git_path(path: &str) -> String {
    path.replace('\\', "/")
}

/// 把「workspace 相对仓库根」的路径归一成 `.` 或 `a/b` 形式。
pub fn normalize_workspace_in_repo_path(path: &str) -> String {
    let normalized = normalize_git_path(path);
    let stripped = strip_leading_dot_slash(&normalized);
    let trimmed = stripped.trim_end_matches('/');
    if trimmed.is_empty() {
        ".".to_string()
    } else {
        trimmed.to_string()
    }
}

/// 仓库相对路径是否落在 workspace 范围内。
pub fn is_path_in_workspace_scope(repo_relative_path: &str, workspace_in_repo_path: &str) -> bool {
    let normalized = normalize_git_path(repo_relative_path);
    let normalized_path = strip_leading_dot_slash(&normalized);
    let normalized_workspace = normalize_workspace_in_repo_path(workspace_in_repo_path);
    if normalized_workspace == "." {
        return true;
    }

    normalized_path == normalized_workspace
        || normalized_path.starts_with(&format!("{normalized_workspace}/"))
}

/// 把仓库相对路径转成 workspace 相对路径。
pub fn to_workspace_relative_git_path(
    repo_relative_path: &str,
    workspace_in_repo_path: &str,
) -> String {
    let normalized = normalize_git_path(repo_relative_path);
    let normalized_path = strip_leading_dot_slash(&normalized);
    let normalized_workspace = normalize_workspace_in_repo_path(workspace_in_repo_path);
    if normalized_workspace == "." {
        return normalized_path.to_string();
    }

    if normalized_path == normalized_workspace {
        return ".".to_string();
    }

    match normalized_path.strip_prefix(&format!("{normalized_workspace}/")) {
        Some(rest) => rest.to_string(),
        None => normalized_path.to_string(),
    }
}

/// `node:path.relative(from, to)` 的等价物。
///
/// 逐条对齐 Node 的实测语义（用 `path.win32.relative` 验过）：
///
/// | `from` | `to` | 结果 |
/// | --- | --- | --- |
/// | `/a/b/c` | `/a/b/c/d.ts` | `d.ts` |
/// | `/a/b/c` | `/a/b/d.ts` | `../d.ts` |
/// | `/a/b/c` | `/a/b/c` | `""` |
/// | `/a/b/c` | `/a/b` | `..` |
/// | `C:\repo` | `D:\other\b.ts` | **`D:\other\b.ts`（原样返回）** |
///
/// 最后一行是关键：**跨盘符时 Node 不硬凑 `..`，而是把目标原样吐回来**。
/// 调用方（[`crate::git::commit_message_scope`]）正是靠
/// 「返回值是不是绝对路径」来判断要不要收下这个候选，所以这里必须照抄 ——
/// 换成「凑一串 `..`」会把一个完全无关的盘符路径当成仓库内的相对路径。
///
/// 输出一律用 `/` 分隔：本移植全程用 `/` 表示 git 路径，
/// 混进 `\` 会让下游的 `starts_with(".../")` 判断失效。
pub fn relative_path(from: &str, to: &str) -> String {
    let from = normalize_git_path(from);
    let to = normalize_git_path(to);

    // 跨盘符 / 一侧有盘符另一侧没有 —— Node 直接返回 to
    if drive_of(&from) != drive_of(&to) {
        return to;
    }

    // 词法归约后再比，`a/b/../c` 与 `a/c` 要算同一条路径
    let from_parts = absolute_parts(&from);
    let to_parts = absolute_parts(&to);

    let common = from_parts
        .iter()
        .zip(to_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let mut result: Vec<String> = Vec::new();
    // 先退到公共祖先
    for _ in common..from_parts.len() {
        result.push("..".to_string());
    }
    // 再往下走到目标
    result.extend(to_parts[common..].iter().cloned());

    result.join("/")
}

/// 取盘符（`C:` / `c:`）。没有盘符时返回空串 —— posix 路径与 UNC 都走这条。
///
/// 只归一大小写，不做别的：Node 在 Windows 上认为 `C:` 与 `c:` 是同一个盘。
fn drive_of(path: &str) -> String {
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && (bytes[0] as char).is_ascii_alphabetic() {
        return (bytes[0] as char)
            .to_ascii_lowercase()
            .to_string()
            .to_uppercase();
    }
    String::new()
}

/// 拆成「去掉盘符与根、消解了 `.` / `..`」的段列表。
///
/// 起点的 `..` 会保留（不越过根），与 Node 一致。
fn absolute_parts(path: &str) -> Vec<String> {
    let without_drive = {
        let bytes = path.as_bytes();
        if bytes.len() >= 2 && bytes[1] == b':' && (bytes[0] as char).is_ascii_alphabetic() {
            &path[2..]
        } else {
            path
        }
    };

    let mut parts: Vec<String> = Vec::new();
    for segment in without_drive.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                // 只有前面有普通段时才回退，否则保留 `..`
                if parts.last().is_some_and(|last| last != "..") {
                    parts.pop();
                } else {
                    parts.push("..".to_string());
                }
            }
            other => parts.push(other.to_string()),
        }
    }
    parts
}

/// 去掉开头至多一处 `/` 或 `./`（对应上游 `path.replace(/^\.?\//, "")`）。
fn strip_leading_dot_slash(value: &str) -> &str {    if let Some(rest) = value.strip_prefix("./") {
        rest
    } else if let Some(rest) = value.strip_prefix('/') {
        rest
    } else {
        value
    }
}

/// 纯词法地消解 `.` / `..`，**不碰文件系统**。
///
/// 用途是「路径还不存在 / 已不存在」的场景：`discard` 要处理的正是被删掉的
/// 文件，`canonicalize` 对它必然失败。起点是 `..` 时保留，不越过根。
pub fn normalize_lexically(path: &Path) -> PathBuf {
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

/// `realpath` 的 Rust 等价物，带兜底。
///
/// 两个细节：
///
/// 1. `std::fs::canonicalize` 在 Windows 上返回 `\\?\C:\...` 这种 **verbatim 前缀**
///    形式，和普通路径做 `strip_prefix` 会永远比不中 —— 所以剥掉前缀。
///    `\\?\UNC\server\share` 要还原成 `\\server\share`。
/// 2. 失败时退回 [`normalize_lexically`]，与上游 `realpath(...).catch(() => raw)`
///    的兜底一致。**两边都必须走这个函数**，否则一边 verbatim 一边普通，
///    比较结果不可信。
pub fn canonicalize_best_effort(path: &Path) -> PathBuf {
    match std::fs::canonicalize(path) {
        Ok(canonical) => strip_verbatim_prefix(canonical),
        Err(_) => normalize_lexically(path),
    }
}

fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    let Some(rest) = text.strip_prefix(r"\\?\") else {
        return path;
    };
    match rest.strip_prefix("UNC\\") {
        Some(unc) => PathBuf::from(format!(r"\\{unc}")),
        None => PathBuf::from(rest.to_string()),
    }
}

/// 读取一个「存在且非空」的环境变量。
fn non_empty_env(name: &str) -> Option<OsString> {
    let value = std::env::var_os(name)?;
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// 64 位视角下的 `Program Files`。
#[cfg(windows)]
fn program_files() -> PathBuf {
    non_empty_env("ProgramW6432")
        .or_else(|| non_empty_env("ProgramFiles"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
}

/// `Program Files (x86)`。
#[cfg(windows)]
fn program_files_x86() -> PathBuf {
    non_empty_env("ProgramFiles(x86)")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_git_path_converts_backslashes() {
        assert_eq!(normalize_git_path(r"a\b\c.rs"), "a/b/c.rs");
        assert_eq!(normalize_git_path("a/b/c.rs"), "a/b/c.rs");
    }

    #[test]
    fn canonicalize_best_effort_never_returns_a_verbatim_prefix() {
        // 真实存在的路径走 canonicalize；不存在的走词法兜底。两者都不能带
        // Windows 的 `\\?\` 前缀，否则和普通路径 strip_prefix 会比不中。
        let existing = std::env::temp_dir();
        let canonical = canonicalize_best_effort(&existing);
        assert!(
            !canonical.to_string_lossy().starts_with(r"\\?\"),
            "verbatim 前缀没被剥掉：{}",
            canonical.display()
        );

        let missing = existing.join("zcode-does-not-exist-12345").join("a.rs");
        let fallback = canonicalize_best_effort(&missing);
        assert_eq!(fallback, normalize_lexically(&missing));
    }

    #[test]
    fn canonicalize_best_effort_agrees_with_itself_on_both_sides() {
        // 归一化必须幂等：调用方靠「两边都过同一个函数」来做 strip_prefix。
        let path = std::env::temp_dir().join("zcode-idempotent-probe");
        let once = canonicalize_best_effort(&path);
        let twice = canonicalize_best_effort(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn normalize_workspace_in_repo_path_handles_empty_and_root() {
        assert_eq!(normalize_workspace_in_repo_path(""), ".");
        assert_eq!(normalize_workspace_in_repo_path("/"), ".");
        assert_eq!(normalize_workspace_in_repo_path("./"), ".");
        assert_eq!(normalize_workspace_in_repo_path("crates/zcode-app/"), "crates/zcode-app");
        assert_eq!(normalize_workspace_in_repo_path("/crates/zcode-app"), "crates/zcode-app");
        assert_eq!(normalize_workspace_in_repo_path("./crates/zcode-app"), "crates/zcode-app");
        assert_eq!(normalize_workspace_in_repo_path(r"crates\zcode-app"), "crates/zcode-app");
    }

    #[test]
    fn workspace_scope_is_true_for_repo_root() {
        assert!(is_path_in_workspace_scope("anything/at/all.rs", "."));
        assert!(is_path_in_workspace_scope("anything/at/all.rs", ""));
    }

    #[test]
    fn workspace_scope_accepts_self_and_children_only() {
        let ws = "crates/zcode-app";
        assert!(is_path_in_workspace_scope("crates/zcode-app", ws));
        assert!(is_path_in_workspace_scope("crates/zcode-app/src/main.rs", ws));
        assert!(is_path_in_workspace_scope("./crates/zcode-app/src/main.rs", ws));
        // 前缀相同但不是子路径 —— 这正是用 starts_with(ws + "/") 而不是 starts_with(ws) 的原因
        assert!(!is_path_in_workspace_scope("crates/zcode-appendix/x.rs", ws));
        assert!(!is_path_in_workspace_scope("crates/zcode-ui/src/lib.rs", ws));
    }

    #[test]
    fn to_workspace_relative_strips_the_workspace_prefix() {
        let ws = "crates/zcode-app";
        assert_eq!(to_workspace_relative_git_path("crates/zcode-app", ws), ".");
        assert_eq!(
            to_workspace_relative_git_path("crates/zcode-app/src/main.rs", ws),
            "src/main.rs"
        );
        assert_eq!(
            to_workspace_relative_git_path("crates/zcode-ui/src/lib.rs", ws),
            "crates/zcode-ui/src/lib.rs"
        );
    }

    #[test]
    fn to_workspace_relative_is_identity_at_repo_root() {
        assert_eq!(to_workspace_relative_git_path("a/b.rs", "."), "a/b.rs");
    }

    #[test]
    fn candidates_start_with_git_and_include_env_override() {
        // 不设 ZCODE_GIT_BINARY 时，第一个候选必须是裸 `git`（走 PATH）。
        let candidates = get_git_binary_candidates();
        assert_eq!(candidates[0], OsString::from("git"));

        // 环境变量优先级最高 —— 这里不真的改进程环境，只验证常量名对得上。
        assert_eq!(GIT_BINARY_ENV_VAR, "ZCODE_GIT_BINARY");
    }

    #[cfg(windows)]
    #[test]
    fn windows_candidates_are_absolute_git_exe_paths() {
        let candidates = get_git_binary_candidates();
        let windows_paths: Vec<String> = candidates
            .iter()
            .skip(1)
            .map(|c| c.to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(windows_paths.len(), 4, "应当有 cmd/bin × 两个 Program Files 根");
        for path in &windows_paths {
            assert!(path.ends_with("/git.exe"), "{path} 不是 git.exe");
            assert!(path.contains("/Git/"), "{path} 不在 Git 目录下");
        }
        assert!(windows_paths[0].contains("/Git/cmd/git.exe"));
        assert!(windows_paths[1].contains("/Git/bin/git.exe"));
    }

    #[test]
    fn null_device_matches_platform() {
        if cfg!(windows) {
            assert_eq!(get_git_null_device_path(), "NUL");
        } else {
            assert_eq!(get_git_null_device_path(), "/dev/null");
        }
    }

    #[test]
    fn command_env_removes_local_vars_and_sets_overrides() {
        let mut command = Command::new("git");
        apply_git_command_env(&mut command);
        let envs: Vec<(String, Option<String>)> = command
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().to_string(),
                    v.map(|v| v.to_string_lossy().to_string()),
                )
            })
            .collect();

        for local in GIT_LOCAL_ENV_VARS {
            let entry = envs
                .iter()
                .find(|(k, _)| k == local)
                .unwrap_or_else(|| panic!("{local} 应当被显式移除"));
            assert!(entry.1.is_none(), "{local} 应当是移除而不是赋值");
        }

        for (name, value) in GIT_COMMAND_ENV_OVERRIDES {
            let entry = envs
                .iter()
                .find(|(k, _)| k == name)
                .unwrap_or_else(|| panic!("{name} 应当被设置"));
            assert_eq!(entry.1.as_deref(), Some(value));
        }
    }
}
