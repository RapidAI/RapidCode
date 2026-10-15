//! 生成提交信息时，**哪些文件该进 prompt** 的范围计算。
//!
//! Rust 移植自 `packages/services/src/git/commitMessageFileScope.ts`。
//!
//! 为什么需要这一层：一次会话可能改了 20 个文件，但用户这次只动了 2 个。
//! 把 20 个都塞进「写提交信息」的 prompt，模型会写出一段泛泛而谈、覆盖全部
//! 改动的废话。这里的职责就是拿**本次会话碰过的文件**把变更列表裁到相关子集。
//!
//! ## 为什么整块都能纯函数化
//!
//! 上游这段代码只用了 `node:path` 的 `isAbsolute` / `relative` 两个纯函数，
//! 没有任何 I/O。所以这里也是**零外部依赖**（只有 `std` 与 `zcode-shared`），
//! 因此可以脱离进程直接测 —— 包括 Windows 上「跨盘符」这种真实但难构造的情况。
//!
//! ## 一条重要的默认行为
//!
//! `current_session_file_paths` 为 `None` 或空时返回**全部文件**，不是空集。
//! 「不知道该裁什么」不等于「什么都不该发」—— 退化成「全发」才是安全的，
//! 因为提交信息生成拿到多余的上下文只是啰嗦，拿到空上下文则完全写不出来。

use crate::git::config::{
    is_path_in_workspace_scope, normalize_git_path, normalize_workspace_in_repo_path,
    relative_path,
};
use zcode_shared::git::GitFileChange;

/// `normalizeCommitMessageScopePath`：去掉前导 `./` / `/` 与全部尾随 `/`。
///
/// 只剥**一个**前导斜杠（上游正则 `^\.?\/` 没有 `+`）—— 但配合尾随裁剪，
/// 结果是一样的：`//a//` → `/a`（再往后没有第二轮剥）。
fn normalize_commit_message_scope_path(path: &str) -> String {
    let normalized = normalize_git_path(path.trim());
    // `^\.?\/` 只吃开头一个 `.` 加一个 `/`（或一个裸 `/`）
    let strip_one = normalized
        .strip_prefix("./")
        .or_else(|| normalized.strip_prefix('/'))
        .unwrap_or(&normalized);
    strip_one.trim_end_matches('/').to_string()
}

/// `isRelativePathInsideScope`：只接受「真的在当前目录里面」的相对路径。
///
/// 三个必须排除的形状：空串、`.` 归约后落到上一级的 `..`、以 `../` 开头的，
/// 以及**绝对路径**。最后一条正是跨盘符时 `relative_path` 的返回值
/// （见 [`crate::git::config::relative_path`]），不排除就会把一个 `D:\...`
/// 当成相对路径收进范围里。
fn is_relative_path_inside_scope(path: &str) -> bool {
    let normalized = normalize_git_path(path);
    !normalized.is_empty()
        && normalized != ".."
        && !normalized.starts_with("../")
        && !is_absolute_path(&normalized)
}

/// `node:path.isAbsolute` 的等价物（认 posix 与 Windows 两种写法）。
fn is_absolute_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    // posix：以 `/` 开头
    if bytes.first() == Some(&b'/') {
        return true;
    }
    // Windows 驱动器：`C:\` 或 `C:/`
    if bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'/' || bytes[2] == b'\\') {
        return (bytes[0] as char).is_ascii_alphabetic();
    }
    // UNC：`\\server\share`
    if path.starts_with("\\\\") {
        return true;
    }
    false
}

/// `addCommitMessageScopeCandidate`：空路径不入集合。
fn add_commit_message_scope_candidate(scope: &mut Vec<String>, path: &str) {
    let normalized = normalize_commit_message_scope_path(path);
    if normalized.is_empty() {
        return;
    }
    if !scope.contains(&normalized) {
        scope.push(normalized);
    }
}

/// `buildCommitMessageFileScope`。
///
/// 返回 `None` 表示「不做裁剪」（上游的 `null`）—— 与「裁到空集」是两回事。
#[allow(clippy::too_many_arguments)]
fn build_commit_message_file_scope(
    workspace_path: &str,
    repo_root: &str,
    workspace_in_repo_path: &str,
    current_session_file_paths: Option<&[String]>,
) -> Option<Vec<String>> {
    let source_paths: Vec<&str> = current_session_file_paths
        .unwrap_or(&[])
        .iter()
        .map(|path| path.trim())
        .filter(|path| !path.is_empty())
        .collect();
    if source_paths.is_empty() {
        return None;
    }

    let mut scope: Vec<String> = Vec::new();
    let normalized_workspace_in_repo_path = normalize_workspace_in_repo_path(workspace_in_repo_path);

    for path in source_paths {
        add_commit_message_scope_candidate(&mut scope, path);

        if is_absolute_path(path) {
            // 绝对路径同时按两种基准归一：仓库根、workspace 根。
            // 两个都要，因为文件既可能在 workspace 里，也可能在仓库里但在
            // workspace 之外（monorepo 里很常见）。
            let repo_relative = relative_path(repo_root, path);
            if is_relative_path_inside_scope(&repo_relative) {
                add_commit_message_scope_candidate(&mut scope, &repo_relative);
            }

            let workspace_relative = relative_path(workspace_path, path);
            if is_relative_path_inside_scope(&workspace_relative) {
                add_commit_message_scope_candidate(&mut scope, &workspace_relative);
            }
            continue;
        }

        let normalized_path = normalize_commit_message_scope_path(path);
        // workspace 不是仓库根时，用户给的相对路径是**相对 workspace** 的，
        // 而 `GitFileChange` 上还有一份仓库相对路径 —— 补一条前缀版本，
        // 两边才能对上。
        if normalized_workspace_in_repo_path != "."
            && !normalized_path.starts_with(&format!("{normalized_workspace_in_repo_path}/"))
        {
            add_commit_message_scope_candidate(
                &mut scope,
                &format!("{normalized_workspace_in_repo_path}/{normalized_path}"),
            );
        }
    }

    if scope.is_empty() {
        None
    } else {
        Some(scope)
    }
}

/// `isCommitMessageFileInScope`：三个路径**任一**命中即算在范围内。
///
/// `scope` 为 `None`（不裁剪）时一律返回 `true`。
fn is_commit_message_file_in_scope(file: &GitFileChange, scope: Option<&[String]>) -> bool {
    let Some(scope) = scope else {
        return true;
    };
    [&file.path, &file.repo_relative_path, &file.workspace_relative_path]
        .iter()
        .any(|path| scope.contains(&normalize_commit_message_scope_path(path)))
}

/// `filterCommitMessageFilesByCurrentSession`：按本次会话碰过的文件裁剪变更列表。
///
/// 没有会话文件信息时**原样返回**（见模块文档）。
pub fn filter_commit_message_files_by_current_session(
    files: &[GitFileChange],
    workspace_path: &str,
    repo_root: &str,
    workspace_in_repo_path: &str,
    current_session_file_paths: Option<&[String]>,
) -> Vec<GitFileChange> {
    let scope = build_commit_message_file_scope(
        workspace_path,
        repo_root,
        workspace_in_repo_path,
        current_session_file_paths,
    );
    files
        .iter()
        .filter(|file| is_commit_message_file_in_scope(file, scope.as_deref()))
        .cloned()
        .collect()
}

/// 供上层复用：`workspace_in_repo_path` 是否真的落在仓库子目录里。
///
/// 这里只是把 [`is_path_in_workspace_scope`] 暴露成更贴近本模块语义的名字，
/// 避免调用方直接依赖 `config` 里的路径工具。
pub fn is_commit_scope_inside_workspace(repo_relative_path: &str, workspace_in_repo_path: &str) -> bool {
    is_path_in_workspace_scope(repo_relative_path, workspace_in_repo_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zcode_shared::git::{GitChangeKind, GitChangeSectionId};

    fn file(path: &str, repo_relative: &str, workspace_relative: &str) -> GitFileChange {
        GitFileChange {
            path: path.to_string(),
            repo_relative_path: repo_relative.to_string(),
            workspace_relative_path: workspace_relative.to_string(),
            x: Some("M".to_string()),
            y: Some(".".to_string()),
            kind: GitChangeKind::Modified,
            section: GitChangeSectionId::Unstaged,
            added: 1,
            removed: 0,
            is_staged: false,
            is_untracked: false,
            is_conflicted: false,
        }
    }

    fn owned(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| p.to_string()).collect()
    }

    // ---- 路径归一 ----

    #[test]
    fn scope_path_strips_one_leading_segment_and_all_trailing_slashes() {
        assert_eq!(normalize_commit_message_scope_path("a/b.ts"), "a/b.ts");
        assert_eq!(normalize_commit_message_scope_path("./a/b.ts"), "a/b.ts");
        assert_eq!(normalize_commit_message_scope_path("/a/b.ts"), "a/b.ts");
        assert_eq!(normalize_commit_message_scope_path("a/b.ts/"), "a/b.ts");
        assert_eq!(normalize_commit_message_scope_path("a/b.ts///"), "a/b.ts");
        // 前后空白先 trim
        assert_eq!(normalize_commit_message_scope_path("  a/b.ts  "), "a/b.ts");
        // 反斜杠归一成 `/`
        assert_eq!(normalize_commit_message_scope_path("a\\b.ts"), "a/b.ts");
    }

    /// 只有**一个**前导斜杠被剥 —— `//a` 剩下 `/a`，仍被判定为绝对路径。
    #[test]
    fn scope_path_only_strips_a_single_leading_slash() {
        assert_eq!(normalize_commit_message_scope_path("//a/b.ts"), "/a/b.ts");
        assert!(is_absolute_path(&normalize_commit_message_scope_path("//a/b.ts")));
    }

    #[test]
    fn absolute_path_detection_covers_posix_windows_and_unc() {
        assert!(is_absolute_path("/a/b.ts"));
        assert!(is_absolute_path("C:/a/b.ts"));
        assert!(is_absolute_path("C:\\a\\b.ts"));
        assert!(is_absolute_path("\\\\server\\share\\b.ts"));
        assert!(!is_absolute_path("a/b.ts"));
        assert!(!is_absolute_path("./a.ts"));
        assert!(!is_absolute_path(""));
        // `1:` 不是盘符
        assert!(!is_absolute_path("1:/a"));
    }

    #[test]
    fn relative_inside_scope_rejects_the_three_escape_shapes() {
        assert!(is_relative_path_inside_scope("a/b.ts"));
        assert!(!is_relative_path_inside_scope(""));
        assert!(!is_relative_path_inside_scope(".."));
        assert!(!is_relative_path_inside_scope("../a.ts"));
        assert!(!is_relative_path_inside_scope("/a.ts"));
    }

    // ---- relative_path（node:path.relative 的等价物） ----

    #[test]
    fn relative_path_walks_up_to_the_common_ancestor() {
        assert_eq!(relative_path("/a/b/c", "/a/b/c/d.ts"), "d.ts");
        assert_eq!(relative_path("/a/b/c", "/a/b/d.ts"), "../d.ts");
        assert_eq!(relative_path("/a/b/c", "/a/b/c"), "");
        assert_eq!(relative_path("/a/b/c", "/a/b"), "..");
        assert_eq!(relative_path("/a", "/a/b/c/d.ts"), "b/c/d.ts");
    }

    /// 跨盘符时 `node:path.relative` 返回**原样的绝对路径**，而不是硬凑一串 `..`。
    /// 这不是边角情况 —— 正是 `is_relative_path_inside_scope` 要挡掉的那个输入。
    #[test]
    fn relative_path_returns_the_target_unchanged_across_drives() {
        let result = relative_path("C:\\repo", "D:\\other\\b.ts");
        assert!(
            is_absolute_path(&result),
            "跨盘符应当返回绝对路径，实际：{result:?}"
        );
        assert!(
            !is_relative_path_inside_scope(&result),
            "跨盘符的结果必须被判定为「不在范围内」"
        );
    }

    #[test]
    fn relative_path_normalizes_dot_segments() {
        assert_eq!(relative_path("C:/r", "C:/r/../b.ts"), "../b.ts");
        assert_eq!(relative_path("/a/b", "/a/b/./c/d.ts"), "c/d.ts");
    }

    /// 逐条对齐 Node 的实际输出。
    ///
    /// 期望值是把这批输入喂给 `path.win32.relative(...)` 再统一成 `/` 拿到的，
    /// 不是推出来的 —— 这样「移植得准不准」有一个外部基准，而不是自说自话。
    #[test]
    fn relative_path_matches_the_node_baseline() {
        let cases: &[(&str, &str, &str)] = &[
            ("/a/b/c", "/a/b/c/d.ts", "d.ts"),
            ("/a/b/c", "/a/b/d.ts", "../d.ts"),
            ("/a/b/c", "/a/b/c", ""),
            ("/a/b/c", "/a/b", ".."),
            ("/a", "/a/b/c/d.ts", "b/c/d.ts"),
            ("C:/repo", "C:/repo/a/b.ts", "a/b.ts"),
            ("C:/r", "C:/r/../b.ts", "../b.ts"),
            ("/a/b", "/a/b/./c/d.ts", "c/d.ts"),
            ("/a/b/c", "/x/y.ts", "../../../x/y.ts"),
            ("", "a.ts", "a.ts"),
            ("/a", "/a", ""),
            ("C:/a/b", "C:/a", ".."),
            ("C:/a", "C:/a/b", "b"),
            ("/a/a", "/a/b", "../b"),
            ("/a/b/c/d", "/a/b/e/f", "../../e/f"),
        ];

        for (from, to, expected) in cases {
            assert_eq!(
                relative_path(from, to),
                *expected,
                "relative_path({from:?}, {to:?}) 与 Node 基准不一致"
            );
        }
    }

    /// 盘符大小写不敏感：`c:` 与 `C:` 是同一个盘。
    #[test]
    fn drive_letters_are_case_insensitive() {
        assert_eq!(relative_path("c:/repo", "C:/repo/a.ts"), "a.ts");
        assert_eq!(relative_path("C:/repo", "c:/repo/a.ts"), "a.ts");
    }

    /// posix 路径与 UNC 都**没有**盘符，不该被当成「跨盘符」而原样返回。
    #[test]
    fn paths_without_a_drive_letter_are_compared_normally() {
        assert_eq!(relative_path("/a/b", "/a/b/c.ts"), "c.ts");
        // UNC 前缀在词法归约里是普通段，两侧一致时能正常算出相对路径
        let result = relative_path("//srv/share/r", "//srv/share/r/a.ts");
        assert_eq!(result, "a.ts");
    }

    // ---- 范围构建 ----

    /// 没有会话文件信息 → 不裁剪（返回 `None`，最终「全发」）。
    #[test]
    fn no_session_paths_means_no_filtering() {
        let files = vec![
            file("F:/r/a.ts", "a.ts", "a.ts"),
            file("F:/r/b.ts", "b.ts", "b.ts"),
        ];
        let kept = filter_commit_message_files_by_current_session(&files, "F:/r", "F:/r", ".", None);
        assert_eq!(kept.len(), 2);

        // 空数组同理
        let kept = filter_commit_message_files_by_current_session(
            &files,
            "F:/r",
            "F:/r",
            ".",
            Some(&[]),
        );
        assert_eq!(kept.len(), 2);

        // 全空白字符串也当作「没有信息」
        let only_blanks = owned(&["   ", "\t"]);
        let kept = filter_commit_message_files_by_current_session(
            &files,
            "F:/r",
            "F:/r",
            ".",
            Some(&only_blanks),
        );
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn relative_session_path_filters_to_that_file() {
        let files = vec![
            file("F:/r/a.ts", "a.ts", "a.ts"),
            file("F:/r/b.ts", "b.ts", "b.ts"),
        ];
        let kept = filter_commit_message_files_by_current_session(
            &files,
            "F:/r",
            "F:/r",
            ".",
            Some(&owned(&["./a.ts"])),
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].repo_relative_path, "a.ts");
    }

    /// 绝对路径要同时按仓库根与 workspace 根两种基准归一 —— 只要有一个对得上就算命中。
    #[test]
    fn absolute_session_path_matches_via_repo_root_or_workspace() {
        let files = vec![
            file("F:/r/pkg/a.ts", "pkg/a.ts", "a.ts"),
            file("F:/r/b.ts", "b.ts", "../b.ts"),
        ];

        // workspace 是仓库里的 `pkg/` 子目录
        let kept = filter_commit_message_files_by_current_session(
            &files,
            "F:/r/pkg",
            "F:/r",
            "pkg",
            Some(&owned(&["F:/r/pkg/a.ts"])),
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].repo_relative_path, "pkg/a.ts");
    }

    /// workspace 不是仓库根时，用户给的相对路径（相对 workspace）要补一条
    /// 带前缀的候选，才能和 `repoRelativePath` 对上。
    #[test]
    fn workspace_relative_input_gets_prefixed_with_the_workspace_segment() {
        let files = vec![
            file("F:/r/pkg/a.ts", "pkg/a.ts", "a.ts"),
            file("F:/r/b.ts", "b.ts", "../b.ts"),
        ];
        let kept = filter_commit_message_files_by_current_session(
            &files,
            "F:/r/pkg",
            "F:/r",
            "pkg",
            Some(&owned(&["a.ts"])),
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].repo_relative_path, "pkg/a.ts");
    }

    /// 已经带前缀的路径不该被再补一次前缀（否则会得到 `pkg/pkg/a.ts`）。
    #[test]
    fn already_prefixed_path_is_not_prefixed_again() {
        let files = vec![file("F:/r/pkg/a.ts", "pkg/a.ts", "a.ts")];
        let kept = filter_commit_message_files_by_current_session(
            &files,
            "F:/r/pkg",
            "F:/r",
            "pkg",
            Some(&owned(&["pkg/a.ts"])),
        );
        assert_eq!(kept.len(), 1);
    }

    /// workspace 就是仓库根时**不**补前缀（上游的 `!== "."` 判断）。
    #[test]
    fn at_repo_root_no_prefix_is_added() {
        let files = vec![file("F:/r/a.ts", "a.ts", "a.ts")];
        let kept = filter_commit_message_files_by_current_session(
            &files,
            "F:/r",
            "F:/r",
            ".",
            Some(&owned(&["a.ts"])),
        );
        assert_eq!(kept.len(), 1);
    }

    /// 完全不相干的会话文件 → 裁到空集（**不是** `None`，「全发」）。
    ///
    /// 这一条与 `no_session_paths_means_no_filtering` 配对：有信息但没命中
    /// 就是真的空，模型拿不到任何文件。
    #[test]
    fn unrelated_session_paths_filter_everything_out() {
        let files = vec![file("F:/r/a.ts", "a.ts", "a.ts")];
        let kept = filter_commit_message_files_by_current_session(
            &files,
            "F:/r",
            "F:/r",
            ".",
            Some(&owned(&["totally/elsewhere.ts"])),
        );
        assert!(kept.is_empty());
    }

    /// 三个路径里**任一**命中即可 —— `workspaceRelativePath` 命中也要收进来。
    #[test]
    fn matching_any_of_the_three_paths_is_enough() {
        let files = vec![file("F:/r/pkg/deep/a.ts", "pkg/deep/a.ts", "deep/a.ts")];
        let kept = filter_commit_message_files_by_current_session(
            &files,
            "F:/r/pkg",
            "F:/r",
            "pkg",
            Some(&owned(&["deep/a.ts"])),
        );
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn filtering_preserves_the_original_order() {
        let files = vec![
            file("F:/r/c.ts", "c.ts", "c.ts"),
            file("F:/r/a.ts", "a.ts", "a.ts"),
            file("F:/r/b.ts", "b.ts", "b.ts"),
        ];
        let kept = filter_commit_message_files_by_current_session(
            &files,
            "F:/r",
            "F:/r",
            ".",
            Some(&owned(&["c.ts", "a.ts"])),
        );
        let order: Vec<&str> = kept.iter().map(|f| f.repo_relative_path.as_str()).collect();
        assert_eq!(order, vec!["c.ts", "a.ts"]);
    }
}
