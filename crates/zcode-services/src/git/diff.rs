//! diff 结果的构造与降级。
//!
//! Rust 移植自两处上游代码：
//! - `packages/services/src/git/repo/gitCliHelpers.ts` 的
//!   `toDiffResult` / `splitUntrackedText` / `buildUntrackedTextDiffResult` / `isBinaryDiff`；
//! - `packages/services/src/git/repo/gitCliRepo.ts` 顶部的
//!   `toUnavailableDiff` / `toCompleteDiffContents` / `withDiffContents` / `isPreviewableText`。
//!
//! 全是纯函数（唯一带 IO 的 `buildUntrackedTextDiffResult` 在这里被拆成
//! 「读文件」由调用方做、「拼 patch」是纯的），所以能脱离进程直接测。
//!
//! ## 两条不能搞反的规则
//!
//! 1. **完整内容必须成对**（[`to_complete_diff_contents`]）。任一侧读失败若被
//!    补成空串，UI 会把「读不到」误判成「文件是空的」，进而把整个文件渲染成
//!    全新增或全删除。所以宁可整体降级，也不要半份内容。
//! 2. **patch 成功时不因全文预览失败而丢弃 patch**（[`with_diff_contents`]）。
//!    全文预览只服务于 MultiFileDiff；拿不到就退化成「只给 patch」，
//!    但已经正确生成的 patch 必须留下。

use zcode_shared::git::{GitDiffAvailability, GitDiffResult};

use super::config::normalize_git_path;
use super::provider::GitCommandExecutionResult;

/// [`to_diff_result`] 的可选参数。
///
/// 三个字段都对应上游 `options?` 里的同名项；`None` 表示用默认行为。
#[derive(Clone, Copy, Debug, Default)]
pub struct ToDiffResultOptions<'a> {
    /// 退出码白名单；`None` 等价于 `[0]`。
    pub allowed_exit_codes: Option<&'a [i32]>,
    /// stdout 为空时的说明；`None` 用默认文案。
    pub empty_summary: Option<&'a str>,
    /// 二进制时的说明；`None` 用默认文案。
    pub binary_summary: Option<&'a str>,
}

/// 构造一个「拿不到」的 diff 结果。
pub fn to_unavailable_diff(path: &str, summary: impl Into<String>) -> GitDiffResult {
    GitDiffResult {
        path: path.to_string(),
        availability: GitDiffAvailability::Unavailable,
        patch: None,
        before_content: None,
        after_content: None,
        summary: Some(summary.into()),
    }
}

/// 一对「改动前 / 改动后」的完整内容。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitDiffContents {
    pub before_content: String,
    pub after_content: String,
}

/// 两侧都拿到才算成功；否则返回 `None`（整体降级）。
///
/// 见模块文档规则 1。
pub fn to_complete_diff_contents(
    before_content: Option<String>,
    after_content: Option<String>,
) -> Option<GitDiffContents> {
    match (before_content, after_content) {
        (Some(before_content), Some(after_content)) => Some(GitDiffContents {
            before_content,
            after_content,
        }),
        _ => None,
    }
}

/// 内容里没有 NUL 才算可预览的文本。
///
/// 上游用 `content.includes("\0")`；这里判断字节，避免多字节字符干扰。
pub fn is_previewable_text(content: &str) -> bool {
    !content.contains('\0')
}

/// stdout 是不是一段二进制 patch。
///
/// 注意第二个模式带**尾随空格**：`git diff --binary` 的输出是
/// `Binary files a/x and b/x differ`，去掉空格就匹配不到。
pub fn is_binary_diff(stdout: &str) -> bool {
    stdout.contains("GIT binary patch") || stdout.contains("Binary files ")
}

/// 把 [`GitCommandExecutionResult`] 判成 [`GitDiffResult`]。
///
/// 判定顺序与上游完全一致，**不能调换**：超时 → 截断 → 退出码 → 空输出 →
/// 二进制 → 有 patch。前两种是「命令没正常跑完」，必须在看退出码之前判掉。
pub fn to_diff_result(
    path: &str,
    result: &GitCommandExecutionResult,
    options: ToDiffResultOptions<'_>,
) -> GitDiffResult {
    if result.timed_out {
        return to_unavailable_diff(path, "Git diff command timed out.");
    }

    if result.output_truncated {
        return GitDiffResult {
            path: path.to_string(),
            availability: GitDiffAvailability::Truncated,
            patch: None,
            before_content: None,
            after_content: None,
            summary: Some("Git diff output exceeded the preview limit.".to_string()),
        };
    }

    let allowed = options.allowed_exit_codes.unwrap_or(&[0]);
    match result.exit_code {
        Some(code) if allowed.contains(&code) => {}
        _ => return to_unavailable_diff(path, result.message()),
    }

    if result.stdout.trim().is_empty() {
        return to_unavailable_diff(
            path,
            options
                .empty_summary
                .unwrap_or("No diff output available."),
        );
    }

    if is_binary_diff(&result.stdout) {
        return GitDiffResult {
            path: path.to_string(),
            availability: GitDiffAvailability::Binary,
            patch: None,
            before_content: None,
            after_content: None,
            summary: Some(
                options
                    .binary_summary
                    .unwrap_or("Binary diff is not previewable.")
                    .to_string(),
            ),
        };
    }

    GitDiffResult {
        path: path.to_string(),
        availability: GitDiffAvailability::Patch,
        patch: Some(result.stdout.clone()),
        before_content: None,
        after_content: None,
        summary: None,
    }
}

/// 把完整内容并进一个已经成功生成 patch 的 diff。
///
/// - 非 `patch` 状态：原样返回（`before/after` 只对 patch 有意义）；
/// - 内容为 `None`：原样返回（见模块文档规则 2）；
/// - 否则补上 `before_content` / `after_content`。
pub fn with_diff_contents(
    diff: GitDiffResult,
    contents: Option<GitDiffContents>,
) -> GitDiffResult {
    if diff.availability != GitDiffAvailability::Patch {
        return diff;
    }

    let Some(contents) = contents else {
        return diff;
    };

    GitDiffResult {
        before_content: Some(contents.before_content),
        after_content: Some(contents.after_content),
        ..diff
    }
}

/// 未跟踪文件内容的切行结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UntrackedText {
    pub lines: Vec<String>,
    pub has_trailing_newline: bool,
}

/// 按 `\n` 切行，并把 CRLF 归一成 LF。
///
/// 空内容返回空行表（而不是 `[""]`），否则空文件会被拼出一个多余的空行。
pub fn split_untracked_text(content: &str) -> UntrackedText {
    let normalized = content.replace("\r\n", "\n");
    if normalized.is_empty() {
        return UntrackedText {
            lines: Vec::new(),
            has_trailing_newline: false,
        };
    }

    let has_trailing_newline = normalized.ends_with('\n');
    let mut lines: Vec<String> = normalized.split('\n').map(str::to_string).collect();
    if has_trailing_newline {
        lines.pop();
    }

    UntrackedText {
        lines,
        has_trailing_newline,
    }
}

/// 为未跟踪文件手搓一份最小 unified diff。
///
/// 上游刻意**不用** `git diff --no-index`：那样在 Windows 上会带上平台相关头部，
/// 而 `@pierre/diffs` 对某些 `diff --git` 头部格式不兼容，UI 展开时会直接抛异常。
/// 这里只保留单文件预览真正需要的 `---/+++ / @@`。
///
/// 判定顺序与上游一致：**先判二进制，再判超限** —— 所以一个超大的二进制文件
/// 会报 `binary` 而不是 `truncated`。
pub fn build_untracked_text_diff_result(
    path: &str,
    repo_relative_path: &str,
    content: &[u8],
    max_preview_bytes: usize,
) -> GitDiffResult {
    if content.contains(&0u8) {
        return GitDiffResult {
            path: path.to_string(),
            availability: GitDiffAvailability::Binary,
            patch: None,
            before_content: None,
            after_content: None,
            summary: Some("Binary diff is not previewable.".to_string()),
        };
    }

    if content.len() > max_preview_bytes {
        return GitDiffResult {
            path: path.to_string(),
            availability: GitDiffAvailability::Truncated,
            patch: None,
            before_content: None,
            after_content: None,
            summary: Some("Git diff output exceeded the preview limit.".to_string()),
        };
    }

    let text = String::from_utf8_lossy(content).into_owned();
    let normalized_path = normalize_git_path(repo_relative_path);
    let UntrackedText {
        lines,
        has_trailing_newline,
    } = split_untracked_text(&text);

    let mut patch_lines = vec![
        "--- /dev/null".to_string(),
        format!("+++ b/{normalized_path}"),
    ];
    if !lines.is_empty() {
        patch_lines.push(format!("@@ -0,0 +1,{} @@", lines.len()));
        patch_lines.extend(lines.iter().map(|line| format!("+{line}")));
        if !has_trailing_newline {
            patch_lines.push("\\ No newline at end of file".to_string());
        }
    }

    GitDiffResult {
        path: path.to_string(),
        availability: GitDiffAvailability::Patch,
        patch: Some(format!("{}\n", patch_lines.join("\n"))),
        before_content: Some(String::new()),
        after_content: Some(text),
        summary: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::provider::GitCommandExecutionResult;

    fn succeeded(stdout: &str) -> GitCommandExecutionResult {
        GitCommandExecutionResult {
            binary_path: "git".to_string(),
            cwd: ".".to_string(),
            args: Vec::new(),
            stdout: stdout.to_string(),
            stderr: String::new(),
            exit_code: Some(0),
            duration_ms: 1,
            timed_out: false,
            timeout_ms: 1000,
            timeout_elapsed_ms: None,
            timeout_close_delay_ms: None,
            force_kill_attempted: false,
            orphaned: false,
            output_truncated: false,
            spawn_error: None,
        }
    }

    #[test]
    fn timed_out_is_unavailable_even_with_stdout() {
        let mut result = succeeded("some output");
        result.timed_out = true;
        let diff = to_diff_result("a.txt", &result, ToDiffResultOptions::default());
        assert_eq!(diff.availability, GitDiffAvailability::Unavailable);
        assert_eq!(diff.summary.as_deref(), Some("Git diff command timed out."));
        assert!(diff.patch.is_none());
    }

    #[test]
    fn truncated_wins_over_exit_code() {
        // 截断优先于退出码判定：否则「输出超限」会被误报成「命令失败」。
        let mut result = succeeded("partial");
        result.output_truncated = true;
        result.exit_code = Some(1);
        let diff = to_diff_result("a.txt", &result, ToDiffResultOptions::default());
        assert_eq!(diff.availability, GitDiffAvailability::Truncated);
        assert_eq!(
            diff.summary.as_deref(),
            Some("Git diff output exceeded the preview limit.")
        );
    }

    #[test]
    fn non_zero_exit_uses_the_command_message() {
        let mut result = succeeded("");
        result.exit_code = Some(128);
        result.stderr = "fatal: bad revision\n".to_string();
        let diff = to_diff_result("a.txt", &result, ToDiffResultOptions::default());
        assert_eq!(diff.availability, GitDiffAvailability::Unavailable);
        assert_eq!(diff.summary.as_deref(), Some("fatal: bad revision"));
    }

    #[test]
    fn missing_exit_code_counts_as_failure() {
        // 上游 `result.exitCode ?? Number.NaN` —— null 一定不在白名单里。
        let mut result = succeeded("x");
        result.exit_code = None;
        let diff = to_diff_result("a.txt", &result, ToDiffResultOptions::default());
        assert_eq!(diff.availability, GitDiffAvailability::Unavailable);
    }

    #[test]
    fn allowed_exit_codes_can_widen_the_whitelist() {
        // `git diff --no-index` 用 1 表示「有差异」，那是正常结果。
        let mut result = succeeded("--- a\n+++ b\n");
        result.exit_code = Some(1);
        let diff = to_diff_result(
            "a.txt",
            &result,
            ToDiffResultOptions {
                allowed_exit_codes: Some(&[0, 1]),
                ..Default::default()
            },
        );
        assert_eq!(diff.availability, GitDiffAvailability::Patch);
    }

    #[test]
    fn empty_stdout_is_unavailable_with_custom_summary() {
        let result = succeeded("   \n  ");
        let diff = to_diff_result(
            "a.txt",
            &result,
            ToDiffResultOptions {
                empty_summary: Some("No Git diff is available for this file."),
                ..Default::default()
            },
        );
        assert_eq!(diff.availability, GitDiffAvailability::Unavailable);
        assert_eq!(
            diff.summary.as_deref(),
            Some("No Git diff is available for this file.")
        );
    }

    #[test]
    fn empty_stdout_default_summary() {
        let result = succeeded("");
        let diff = to_diff_result("a.txt", &result, ToDiffResultOptions::default());
        assert_eq!(diff.summary.as_deref(), Some("No diff output available."));
    }

    #[test]
    fn binary_markers_are_detected() {
        assert!(is_binary_diff("GIT binary patch\nliteral 0\n"));
        assert!(is_binary_diff("Binary files a/x and b/x differ\n"));
        assert!(!is_binary_diff("--- a/x\n+++ b/x\n@@ -1 +1 @@\n"));
        // 尾随空格是模式的一部分：`Binary files` 后面必须跟空格才算是那条消息。
        assert!(!is_binary_diff("Binary files"));
        assert!(!is_binary_diff("Binary files_are_not_here"));
    }

    #[test]
    fn binary_stdout_becomes_binary_availability() {
        let result = succeeded("Binary files a/x and b/x differ\n");
        let diff = to_diff_result("x", &result, ToDiffResultOptions::default());
        assert_eq!(diff.availability, GitDiffAvailability::Binary);
        assert_eq!(diff.summary.as_deref(), Some("Binary diff is not previewable."));
        assert!(diff.patch.is_none());
    }

    #[test]
    fn text_stdout_becomes_patch_with_raw_stdout() {
        let stdout = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new\n";
        let result = succeeded(stdout);
        let diff = to_diff_result("x", &result, ToDiffResultOptions::default());
        assert_eq!(diff.availability, GitDiffAvailability::Patch);
        assert_eq!(diff.patch.as_deref(), Some(stdout));
        assert!(diff.summary.is_none());
        // patch 阶段不带完整内容，由 with_diff_contents 之后补。
        assert!(diff.before_content.is_none());
        assert!(diff.after_content.is_none());
    }

    #[test]
    fn complete_contents_require_both_sides() {
        let both = to_complete_diff_contents(Some("a".to_string()), Some("b".to_string()));
        assert_eq!(
            both,
            Some(GitDiffContents {
                before_content: "a".to_string(),
                after_content: "b".to_string(),
            })
        );
        // 任一侧缺失 → 整体 None（不能把「读不到」当成「空文件」）。
        assert!(to_complete_diff_contents(None, Some("b".to_string())).is_none());
        assert!(to_complete_diff_contents(Some("a".to_string()), None).is_none());
        assert!(to_complete_diff_contents(None, None).is_none());
    }

    #[test]
    fn with_diff_contents_fills_a_patch() {
        let diff = GitDiffResult {
            path: "x".to_string(),
            availability: GitDiffAvailability::Patch,
            patch: Some("patch".to_string()),
            before_content: None,
            after_content: None,
            summary: None,
        };
        let filled = with_diff_contents(
            diff,
            Some(GitDiffContents {
                before_content: "old".to_string(),
                after_content: "new".to_string(),
            }),
        );
        assert_eq!(filled.before_content.as_deref(), Some("old"));
        assert_eq!(filled.after_content.as_deref(), Some("new"));
        assert_eq!(filled.patch.as_deref(), Some("patch"));
    }

    #[test]
    fn with_diff_contents_keeps_the_patch_when_contents_fail() {
        // 关键回归：全文预览失败**不能**把已经正确的 patch 一起丢掉。
        let diff = GitDiffResult {
            path: "x".to_string(),
            availability: GitDiffAvailability::Patch,
            patch: Some("patch".to_string()),
            before_content: None,
            after_content: None,
            summary: None,
        };
        let kept = with_diff_contents(diff, None);
        assert_eq!(kept.availability, GitDiffAvailability::Patch);
        assert_eq!(kept.patch.as_deref(), Some("patch"));
        assert!(kept.before_content.is_none());
    }

    #[test]
    fn with_diff_contents_ignores_non_patch_states() {
        let diff = to_unavailable_diff("x", "nope");
        let untouched = with_diff_contents(
            diff.clone(),
            Some(GitDiffContents {
                before_content: "old".to_string(),
                after_content: "new".to_string(),
            }),
        );
        assert_eq!(untouched, diff);
    }

    #[test]
    fn previewable_text_rejects_nul() {
        assert!(is_previewable_text("hello\nworld\n"));
        assert!(!is_previewable_text("hello\0world"));
    }

    #[test]
    fn split_untracked_text_handles_trailing_newline() {
        let with = split_untracked_text("a\nb\n");
        assert_eq!(with.lines, vec!["a", "b"]);
        assert!(with.has_trailing_newline);

        let without = split_untracked_text("a\nb");
        assert_eq!(without.lines, vec!["a", "b"]);
        assert!(!without.has_trailing_newline);
    }

    #[test]
    fn split_untracked_text_normalises_crlf() {
        let split = split_untracked_text("a\r\nb\r\n");
        assert_eq!(split.lines, vec!["a", "b"]);
        assert!(split.has_trailing_newline);
    }

    #[test]
    fn split_untracked_text_treats_empty_as_no_lines() {
        // 空文件不该被切成一个空行，否则 patch 里会多一个 `+`。
        let empty = split_untracked_text("");
        assert!(empty.lines.is_empty());
        assert!(!empty.has_trailing_newline);
    }

    #[test]
    fn untracked_patch_has_the_minimal_header() {
        let diff = build_untracked_text_diff_result(
            "/abs/new.txt",
            "src\\new.txt",
            b"a\nb\n",
            1024,
        );
        assert_eq!(diff.availability, GitDiffAvailability::Patch);
        // 路径统一成 `/` 分隔，且只保留 ---/+++/@@ 三段。
        assert_eq!(
            diff.patch.as_deref(),
            Some("--- /dev/null\n+++ b/src/new.txt\n@@ -0,0 +1,2 @@\n+a\n+b\n")
        );
        assert_eq!(diff.before_content.as_deref(), Some(""));
        assert_eq!(diff.after_content.as_deref(), Some("a\nb\n"));
        assert!(diff.summary.is_none());
    }

    #[test]
    fn untracked_patch_marks_missing_trailing_newline() {
        let diff = build_untracked_text_diff_result("/abs/x.txt", "x.txt", b"only", 1024);
        assert_eq!(
            diff.patch.as_deref(),
            Some("--- /dev/null\n+++ b/x.txt\n@@ -0,0 +1,1 @@\n+only\n\\ No newline at end of file\n")
        );
    }

    #[test]
    fn untracked_empty_file_has_only_the_header() {
        let diff = build_untracked_text_diff_result("/abs/empty.txt", "empty.txt", b"", 1024);
        assert_eq!(diff.availability, GitDiffAvailability::Patch);
        assert_eq!(diff.patch.as_deref(), Some("--- /dev/null\n+++ b/empty.txt\n"));
        assert_eq!(diff.after_content.as_deref(), Some(""));
    }

    #[test]
    fn untracked_binary_is_checked_before_the_size_limit() {
        // 一个超大的二进制文件报 binary 而不是 truncated（顺序与上游一致）。
        let mut content = vec![0u8; 32];
        content.extend(std::iter::repeat_n(b'x', 4096));
        let diff = build_untracked_text_diff_result("/abs/blob.bin", "blob.bin", &content, 64);
        assert_eq!(diff.availability, GitDiffAvailability::Binary);
        assert_eq!(diff.summary.as_deref(), Some("Binary diff is not previewable."));
    }

    #[test]
    fn untracked_oversized_text_is_truncated() {
        let content = vec![b'x'; 128];
        let diff = build_untracked_text_diff_result("/abs/big.txt", "big.txt", &content, 64);
        assert_eq!(diff.availability, GitDiffAvailability::Truncated);
        assert_eq!(
            diff.summary.as_deref(),
            Some("Git diff output exceeded the preview limit.")
        );
    }

    #[test]
    fn untracked_patch_keeps_non_ascii_content() {
        let diff = build_untracked_text_diff_result("/abs/x.txt", "x.txt", "中文\n".as_bytes(), 1024);
        assert_eq!(diff.after_content.as_deref(), Some("中文\n"));
        assert!(diff.patch.as_deref().unwrap().contains("+中文"));
    }

    #[test]
    fn unavailable_diff_has_no_payload() {
        let diff = to_unavailable_diff("/abs/x", "boom");
        assert_eq!(diff.path, "/abs/x");
        assert_eq!(diff.availability, GitDiffAvailability::Unavailable);
        assert_eq!(diff.summary.as_deref(), Some("boom"));
        assert!(diff.patch.is_none());
        assert!(diff.before_content.is_none());
        assert!(diff.after_content.is_none());
    }
}
