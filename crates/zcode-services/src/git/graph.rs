//! 提交图（`git log`）的解析。
//!
//! Rust 移植自 `packages/services/src/git/repo/gitCliRepo.ts` 里
//! `normalizeGitGraph*` / `addGitGraphRef` / `parseGitGraphDecorationRef` /
//! `parseGitGraphRefs` / `parseGitGraphRecords` 这一组**纯函数**。
//!
//! ## 为什么单独拆一个模块
//!
//! 和 [`super::cli_helpers`] 一样，这一组没有任何 IO，只负责把 `git log` 的
//! 机器可读输出翻成 [`GitCommitGraphCommit`]。拆出来是为了能脱离进程直接测：
//! 上游对 decorations 的处理有一堆容易写错的细节（见下）。
//!
//! ## 格式与两个分隔符
//!
//! 命令固定用
//! `--format=%H%x00%P%x00%an%x00%at%x00%s%x00%D%x1e`：
//!
//! ```text
//! <hash> \0 <parents> \0 <author> \0 <authoredAt 秒> \0 <subject> \0 <decorations> \x1e
//! ```
//!
//! - 字段分隔符是 `\0`（NUL），所以 subject 里带空格/逗号都安全；
//! - 记录分隔符是 `\x1e`（RS）。`git log` 在每条之后还会补一个换行，
//!   于是按 `\x1e` 切开后，除第一条外每条都以 `\n` 开头 —— 上游用
//!   `record.trim()` 把它吃掉，这里照做。
//!
//! ## decorations 的三个坑（都来自上游，都保留）
//!
//! 1. `HEAD -> refs/heads/main` 要**同时**产出 `HEAD`(head) 与 `main`(branch)；
//! 2. `tag: v1.0` 与 `tag: refs/tags/v1.0` 都表示同一个标签，要去前缀；
//! 3. 兜底分支：没有已知前缀的 ref，含 `/` 判为 remote，否则判为 branch
//!    （`git log --decorate` 对远端分支有时给短名 `origin/main`）。

use zcode_shared::git::{GitCommitGraphCommit, GitCommitGraphRef, GitCommitGraphRefKind};

/// 不传 `maxCount` 时取多少条。
pub const DEFAULT_GIT_GRAPH_MAX_COUNT: usize = 100;

/// `maxCount` 的上限；再多也不给，避免一次拉爆输出。
pub const MAX_GIT_GRAPH_MAX_COUNT: usize = 200;

/// 记录分隔符（RS）。
const GIT_GRAPH_RECORD_SEPARATOR: char = '\x1e';

/// 字段分隔符（NUL）。
const GIT_GRAPH_FIELD_SEPARATOR: char = '\x00';

/// 把 `maxCount` 归一化到 `[1, MAX]`；`None` 取默认值。
///
/// 对应上游 `normalizeGitGraphMaxCount`。注意 `0` / 负数都会抬到 `1`，
/// 而不是变成「不限制」—— 调用方要「全都要」只能显式传上限值。
pub fn normalize_git_graph_max_count(max_count: Option<i64>) -> usize {
    match max_count {
        None => DEFAULT_GIT_GRAPH_MAX_COUNT,
        Some(value) => value.clamp(1, MAX_GIT_GRAPH_MAX_COUNT as i64) as usize,
    }
}

/// 把 `skip` 归一化到 `>= 0`；`None` 取 `0`。
pub fn normalize_git_graph_skip(skip: Option<i64>) -> usize {
    match skip {
        None => 0,
        Some(value) => value.max(0) as usize,
    }
}

/// 加一个 ref，按 `(kind, name)` 去重。
///
/// 去重是必要的：`HEAD -> main` 之后若 decorations 里又出现一次 `main`，
/// 界面不该出现两个同样的徽标。
pub fn add_git_graph_ref(refs: &mut Vec<GitCommitGraphRef>, candidate: GitCommitGraphRef) {
    if refs
        .iter()
        .any(|existing| existing.kind == candidate.kind && existing.name == candidate.name)
    {
        return;
    }
    refs.push(candidate);
}

/// 解析 decorations 里的**单个** ref（不含 `HEAD -> ` 前缀）。
///
/// 返回 `None` 表示这一项不是有效 ref（空串、或 `refs/heads/` 后面什么都没有）。
pub fn parse_git_graph_decoration_ref(raw_ref: &str) -> Option<GitCommitGraphRef> {
    let trimmed = raw_ref.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed == "HEAD" {
        return Some(GitCommitGraphRef {
            name: "HEAD".to_string(),
            kind: GitCommitGraphRefKind::Head,
        });
    }

    // `tag: v1.0` 与 `tag: refs/tags/v1.0` 是同一个东西，统一去掉 refs/tags/。
    if let Some(rest) = trimmed.strip_prefix("tag: ") {
        let tag_ref = rest.trim();
        let name = tag_ref.strip_prefix("refs/tags/").unwrap_or(tag_ref);
        return (!name.is_empty()).then(|| GitCommitGraphRef {
            name: name.to_string(),
            kind: GitCommitGraphRefKind::Tag,
        });
    }

    for (prefix, kind) in [
        ("refs/heads/", GitCommitGraphRefKind::Branch),
        ("refs/remotes/", GitCommitGraphRefKind::Remote),
        ("refs/tags/", GitCommitGraphRefKind::Tag),
    ] {
        if let Some(name) = trimmed.strip_prefix(prefix) {
            return (!name.is_empty()).then(|| GitCommitGraphRef {
                name: name.to_string(),
                kind,
            });
        }
    }

    // 兜底：没有已知前缀。含 `/` 的判为远端分支（短名形如 origin/main），
    // 否则判为本地分支。
    Some(GitCommitGraphRef {
        name: trimmed.to_string(),
        kind: if trimmed.contains('/') {
            GitCommitGraphRefKind::Remote
        } else {
            GitCommitGraphRefKind::Branch
        },
    })
}

/// 解析一条提交的整串 decorations（`%D` 的输出，逗号分隔）。
pub fn parse_git_graph_refs(raw_decorations: &str) -> Vec<GitCommitGraphRef> {
    let mut refs = Vec::new();
    for raw_decoration in raw_decorations.split(',') {
        let decoration = raw_decoration.trim();
        if decoration.is_empty() {
            continue;
        }

        if let Some(pointed) = decoration.strip_prefix("HEAD -> ") {
            add_git_graph_ref(
                &mut refs,
                GitCommitGraphRef {
                    name: "HEAD".to_string(),
                    kind: GitCommitGraphRefKind::Head,
                },
            );
            if let Some(parsed) = parse_git_graph_decoration_ref(pointed) {
                add_git_graph_ref(&mut refs, parsed);
            }
            continue;
        }

        if let Some(parsed) = parse_git_graph_decoration_ref(decoration) {
            add_git_graph_ref(&mut refs, parsed);
        }
    }
    refs
}

/// 解析整段 `git log` 输出。
///
/// 空记录（含被 `trim` 后变空的）直接跳过；`hash` 为空的记录也跳过。
pub fn parse_git_graph_records(stdout: &str) -> Vec<GitCommitGraphCommit> {
    stdout
        .split(GIT_GRAPH_RECORD_SEPARATOR)
        .map(str::trim)
        .filter(|record| !record.is_empty())
        .filter_map(parse_git_graph_record)
        .collect()
}

/// 解析单条记录（六个字段）。
fn parse_git_graph_record(record: &str) -> Option<GitCommitGraphCommit> {
    let fields: Vec<&str> = record.split(GIT_GRAPH_FIELD_SEPARATOR).collect();

    // 第 0 段是 hash：空串等同于缺失，整条丢弃。
    let hash = fields.first().copied().unwrap_or("");
    if hash.is_empty() {
        return None;
    }

    let parents = fields
        .get(1)
        .map(|raw| {
            raw.split(' ')
                .filter(|parent| !parent.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // 空作者名等同于「没有」（上游 `authorName || null`）。
    let author_name = fields
        .get(2)
        .map(|raw| raw.trim())
        .filter(|raw| !raw.is_empty())
        .map(str::to_string);

    // 时间戳解析失败就是 `None`，**不是** 0 —— 0 会被界面当成 1970 年。
    let authored_at_ms = fields
        .get(3)
        .and_then(|raw| parse_leading_i64(raw))
        .map(|seconds| seconds * 1000);

    let subject = fields.get(4).copied().unwrap_or("").to_string();
    let refs = parse_git_graph_refs(fields.get(5).copied().unwrap_or(""));

    Some(GitCommitGraphCommit {
        hash: hash.to_string(),
        parents,
        refs,
        subject,
        author_name,
        authored_at_ms,
    })
}

/// 取字符串开头的整数值，语义对齐 JS 的 `Number.parseInt(x, 10)`。
///
/// 和 `str::parse::<i64>()` 的区别是**宽松**：`parseInt` 允许前导空白、
/// 允许后面跟垃圾（`"12abc"` → `12`）、遇到小数点在点处截断（`"12.9"` → `12`）。
/// 上游正是这么解析 `%at` 的，照搬以免行为漂移。
fn parse_leading_i64(raw: &str) -> Option<i64> {
    let trimmed = raw.trim_start();
    let mut chars = trimmed.char_indices().peekable();

    let mut sign = 1i64;
    if let Some((_, first)) = chars.peek().copied() {
        match first {
            '-' => {
                sign = -1;
                chars.next();
            }
            '+' => {
                chars.next();
            }
            _ => {}
        }
    }

    let mut digits = String::new();
    for (_, ch) in chars {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else {
            break;
        }
    }

    if digits.is_empty() {
        return None;
    }
    digits.parse::<i64>().ok().map(|value| sign * value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ref_of(name: &str, kind: GitCommitGraphRefKind) -> GitCommitGraphRef {
        GitCommitGraphRef {
            name: name.to_string(),
            kind,
        }
    }

    #[test]
    fn max_count_defaults_and_clamps() {
        assert_eq!(normalize_git_graph_max_count(None), 100);
        assert_eq!(normalize_git_graph_max_count(Some(50)), 50);
        // 0 与负数都被抬到 1，而不是「不限制」。
        assert_eq!(normalize_git_graph_max_count(Some(0)), 1);
        assert_eq!(normalize_git_graph_max_count(Some(-5)), 1);
        assert_eq!(normalize_git_graph_max_count(Some(200)), 200);
        assert_eq!(normalize_git_graph_max_count(Some(5000)), 200);
    }

    #[test]
    fn skip_defaults_and_clamps_to_zero() {
        assert_eq!(normalize_git_graph_skip(None), 0);
        assert_eq!(normalize_git_graph_skip(Some(7)), 7);
        assert_eq!(normalize_git_graph_skip(Some(-3)), 0);
        assert_eq!(normalize_git_graph_skip(Some(0)), 0);
    }

    #[test]
    fn decoration_ref_recognises_each_prefix() {
        assert_eq!(
            parse_git_graph_decoration_ref("HEAD"),
            Some(ref_of("HEAD", GitCommitGraphRefKind::Head))
        );
        assert_eq!(
            parse_git_graph_decoration_ref("refs/heads/main"),
            Some(ref_of("main", GitCommitGraphRefKind::Branch))
        );
        assert_eq!(
            parse_git_graph_decoration_ref("refs/remotes/origin/main"),
            Some(ref_of("origin/main", GitCommitGraphRefKind::Remote))
        );
        assert_eq!(
            parse_git_graph_decoration_ref("refs/tags/v1.0"),
            Some(ref_of("v1.0", GitCommitGraphRefKind::Tag))
        );
    }

    #[test]
    fn decoration_ref_unwraps_the_tag_prefix() {
        // `tag: v1.0` 与 `tag: refs/tags/v1.0` 必须得到同一个结果。
        let short = parse_git_graph_decoration_ref("tag: v1.0");
        let long = parse_git_graph_decoration_ref("tag: refs/tags/v1.0");
        assert_eq!(short, Some(ref_of("v1.0", GitCommitGraphRefKind::Tag)));
        assert_eq!(short, long);
    }

    #[test]
    fn decoration_ref_falls_back_on_bare_names() {
        // 短名带 `/` → 远端；不带 → 本地分支。
        assert_eq!(
            parse_git_graph_decoration_ref("origin/main"),
            Some(ref_of("origin/main", GitCommitGraphRefKind::Remote))
        );
        assert_eq!(
            parse_git_graph_decoration_ref("feature/login"),
            Some(ref_of("feature/login", GitCommitGraphRefKind::Remote))
        );
        assert_eq!(
            parse_git_graph_decoration_ref("main"),
            Some(ref_of("main", GitCommitGraphRefKind::Branch))
        );
    }

    #[test]
    fn decoration_ref_rejects_empty_and_prefix_only() {
        assert_eq!(parse_git_graph_decoration_ref(""), None);
        assert_eq!(parse_git_graph_decoration_ref("   "), None);
        assert_eq!(parse_git_graph_decoration_ref("refs/heads/"), None);
        assert_eq!(parse_git_graph_decoration_ref("refs/remotes/"), None);
        assert_eq!(parse_git_graph_decoration_ref("refs/tags/"), None);
        // `tag:` 后面只有 `refs/tags/`，去掉前缀后是空 → 丢弃。
        assert_eq!(parse_git_graph_decoration_ref("tag: refs/tags/"), None);
    }

    #[test]
    fn bare_tag_prefix_falls_through_like_upstream() {
        // 注意：上游是**先 trim 再匹配** `"tag: "`，所以 `"tag: "` 被 trim 成
        // `"tag:"`，匹配不上，于是走兜底分支变成一个叫 `tag:` 的本地分支。
        // 这是上游的既有行为（`%D` 不会真的产出这种输入），照搬以免行为漂移。
        assert_eq!(
            parse_git_graph_decoration_ref("tag: "),
            Some(ref_of("tag:", GitCommitGraphRefKind::Branch))
        );
    }

    #[test]
    fn head_pointer_produces_both_refs() {
        let refs = parse_git_graph_refs("HEAD -> refs/heads/main");
        assert_eq!(
            refs,
            vec![
                ref_of("HEAD", GitCommitGraphRefKind::Head),
                ref_of("main", GitCommitGraphRefKind::Branch),
            ]
        );
    }

    #[test]
    fn refs_are_split_on_commas_and_trimmed() {
        let refs = parse_git_graph_refs("HEAD -> main, refs/remotes/origin/main, tag: v1.0");
        assert_eq!(
            refs,
            vec![
                ref_of("HEAD", GitCommitGraphRefKind::Head),
                ref_of("main", GitCommitGraphRefKind::Branch),
                ref_of("origin/main", GitCommitGraphRefKind::Remote),
                ref_of("v1.0", GitCommitGraphRefKind::Tag),
            ]
        );
    }

    #[test]
    fn refs_are_deduped_by_kind_and_name() {
        let mut refs = Vec::new();
        let branch = ref_of("main", GitCommitGraphRefKind::Branch);
        add_git_graph_ref(&mut refs, branch.clone());
        add_git_graph_ref(&mut refs, branch.clone());
        // 同名但不同 kind 不算重复。
        add_git_graph_ref(&mut refs, ref_of("main", GitCommitGraphRefKind::Remote));
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn refs_dedupe_head_pointer_repeats() {
        // 同一条 decoration 里重复出现时也只留一份。
        let refs = parse_git_graph_refs("HEAD -> main, refs/heads/main");
        assert_eq!(
            refs,
            vec![
                ref_of("HEAD", GitCommitGraphRefKind::Head),
                ref_of("main", GitCommitGraphRefKind::Branch),
            ]
        );
    }

    #[test]
    fn refs_ignore_empty_decorations() {
        assert!(parse_git_graph_refs("").is_empty());
        assert!(parse_git_graph_refs("  ,  , ").is_empty());
    }

    #[test]
    fn records_parse_every_field() {
        let stdout = "abc123\x0011112222 33334444\x00Ada\x001700000000\x00fix: thing\x00HEAD -> main, tag: v1\x1e";
        let commits = parse_git_graph_records(stdout);
        assert_eq!(commits.len(), 1);
        let commit = &commits[0];
        assert_eq!(commit.hash, "abc123");
        assert_eq!(commit.parents, vec!["11112222", "33334444"]);
        assert_eq!(commit.author_name.as_deref(), Some("Ada"));
        assert_eq!(commit.authored_at_ms, Some(1_700_000_000_000));
        assert_eq!(commit.subject, "fix: thing");
        assert_eq!(
            commit.refs,
            vec![
                ref_of("HEAD", GitCommitGraphRefKind::Head),
                ref_of("main", GitCommitGraphRefKind::Branch),
                ref_of("v1", GitCommitGraphRefKind::Tag),
            ]
        );
    }

    #[test]
    fn records_tolerate_the_newline_between_entries() {
        // `git log` 在每条之后补一个 `\n`，于是第二条记录以 `\n` 开头。
        let stdout = concat!(
            "aaa\x00p1\x00Ada\x001700000000\x00first\x00HEAD -> main\x1e",
            "\n",
            "bbb\x00aaa\x00Bob\x001700000001\x00second\x00\x1e",
        );
        let commits = parse_git_graph_records(stdout);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].hash, "aaa");
        assert_eq!(commits[1].hash, "bbb");
        assert_eq!(commits[1].subject, "second");
        assert!(commits[1].refs.is_empty());
    }

    #[test]
    fn records_without_parents_or_subject_are_still_valid() {
        // 根提交没有父；subject 缺失时退化成空串而不是报错。
        let stdout = "root\x00\x00\x00\x00\x1e";
        let commits = parse_git_graph_records(stdout);
        assert_eq!(commits.len(), 1);
        assert!(commits[0].parents.is_empty());
        assert_eq!(commits[0].subject, "");
        assert_eq!(commits[0].author_name, None);
        assert_eq!(commits[0].authored_at_ms, None);
        assert!(commits[0].refs.is_empty());
    }

    #[test]
    fn records_with_empty_hash_are_dropped() {
        let stdout = "\x00p1\x00Ada\x001700000000\x00x\x00\x1e";
        assert!(parse_git_graph_records(stdout).is_empty());
    }

    #[test]
    fn records_skip_blank_records() {
        // 结尾的 `\x1e` 之后什么都没有；中间的空白记录也要跳过。
        let stdout = "\x1e   \x1eaaa\x00\x00\x00\x00\x00\x1e\x1e";
        let commits = parse_git_graph_records(stdout);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].hash, "aaa");
    }

    #[test]
    fn bad_timestamp_becomes_none_not_zero() {
        // 0 会被界面渲染成 1970 年，所以解析失败必须是 None。
        let stdout = "h\x00\x00Ada\x00not-a-number\x00s\x00\x1e";
        let commits = parse_git_graph_records(stdout);
        assert_eq!(commits[0].authored_at_ms, None);
    }

    #[test]
    fn parse_int_semantics_are_preserved() {
        // 对齐 JS Number.parseInt 的宽松行为。
        assert_eq!(parse_leading_i64("1700000000"), Some(1_700_000_000));
        assert_eq!(parse_leading_i64("  42"), Some(42));
        assert_eq!(parse_leading_i64("42abc"), Some(42));
        assert_eq!(parse_leading_i64("42.9"), Some(42));
        assert_eq!(parse_leading_i64("-7"), Some(-7));
        assert_eq!(parse_leading_i64("+7"), Some(7));
        assert_eq!(parse_leading_i64(""), None);
        assert_eq!(parse_leading_i64("abc"), None);
        assert_eq!(parse_leading_i64("-"), None);
    }

    #[test]
    fn subject_keeps_commas_and_spaces() {
        // 字段分隔符是 NUL，所以 subject 里的逗号不该被当成 ref 分隔。
        let stdout = "h\x00\x00Ada\x001700000000\x00fix: a, b and c\x00main\x1e";
        let commits = parse_git_graph_records(stdout);
        assert_eq!(commits[0].subject, "fix: a, b and c");
        assert_eq!(commits[0].refs, vec![ref_of("main", GitCommitGraphRefKind::Branch)]);
    }
}
