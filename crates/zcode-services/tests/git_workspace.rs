//! 端到端测试：在真实临时仓库上跑一遍完整链路。
//!
//! 单元测试能证明「解析规则对」，但证明不了「拼出来的命令行真的能被 git 接受、
//! 真的能产出这些形状的输出」。这一层就是补这个缺口 —— 它跑的是真的 `git`。
//!
//! 找不到 git 时整组测试会打印一行说明并跳过（而不是红掉），
//! 因为「本机没装 git」不是这个 crate 的缺陷。

use std::path::{Path, PathBuf};
use std::process::Command;

use zcode_services::git::changes::{get_workspace_changes, summarize_changes};
use zcode_services::git::repo::GitCliRepo;
use zcode_services::git::types::{GitChangeSection, GitDiffQuery};
use zcode_shared::git::{
    GitChangeKind, GitChangeSectionId, GitDiffAvailability, GitHeadRefType,
};

/// 本机能不能跑 git。
fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// 跑一条 git 命令，失败就 panic 并把 stderr 带出来。
fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        // 固定身份，免得依赖开发机的 user.name / user.email
        .env("GIT_AUTHOR_NAME", "ZCode Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "ZCode Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .unwrap_or_else(|error| panic!("git {args:?} 起不来: {error}"));

    assert!(
        output.status.success(),
        "git {args:?} 失败: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// 一个用完就删的临时目录。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("创建临时目录");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// 读一个文件并把换行统一成 `\n`。
///
/// Windows 上 `core.autocrlf` 默认开启，`git restore` / `checkout` 会把仓库里的
/// LF 写成工作区的 CRLF。断言文件内容时必须先归一，否则测的是「本机的 git 配置」
/// 而不是「我们的代码有没有把文件恢复对」。
fn read_text(root: &Path, relative: &str) -> String {
    std::fs::read_to_string(root.join(relative))
        .unwrap_or_else(|error| panic!("读不到 {relative}: {error}"))
        .replace("\r\n", "\n")
}

/// 这个目录是不是落在某个 Git 仓库里（含被 .gitignore 忽略的情况）。
fn is_inside_a_repository(path: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(path)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// 造一个**确定不在任何仓库里**的临时目录；造不出来就返回 `None`（调用方跳过）。
///
/// 为什么不能直接用 [`TempDir`]：它建在 `CARGO_TARGET_TMPDIR` 下，也就是本仓库的
/// `target/` 里 —— `target/` 只是被 `.gitignore` 忽略，**仍然在 work tree 内**，
/// `git rev-parse --show-toplevel` 会一路找到 `RapidCode` 仓库根。
///
/// 这个坑的代价很实在：`push` 测试曾经因此对着**真实 origin** 跑了一次 `git push`。
/// 所以这里换到系统临时目录，并且仍然做一次探测兜底。
fn dir_outside_any_repository(name: &str) -> Option<TempDir> {
    let candidate = std::env::temp_dir().join(format!("zcode-services-{name}"));
    let _ = std::fs::remove_dir_all(&candidate);
    std::fs::create_dir_all(&candidate).expect("创建系统临时目录");
    let dir = TempDir { path: candidate };
    if is_inside_a_repository(dir.path()) {
        eprintln!("跳过：{} 恰好位于某个 Git 仓库内", dir.path().display());
        return None;
    }
    Some(dir)
}

/// 建一个仓库，里面覆盖四类变更：已暂存、未暂存、未跟踪、以及一个已跟踪的子目录。
fn build_repository(name: &str) -> TempDir {
    let dir = TempDir::new(name);
    git(dir.path(), &["init", "--quiet", "-b", "main"]);

    std::fs::write(dir.path().join("tracked.txt"), "line one\nline two\n").unwrap();
    std::fs::create_dir_all(dir.path().join("nested")).unwrap();
    std::fs::write(dir.path().join("nested/inner.txt"), "inner\n").unwrap();
    git(dir.path(), &["add", "--", "tracked.txt", "nested/inner.txt"]);
    git(dir.path(), &["commit", "--quiet", "-m", "initial"]);

    // 未暂存改动：改一行
    std::fs::write(dir.path().join("tracked.txt"), "line one\nline two changed\n").unwrap();

    // 已暂存改动：新文件 + add
    std::fs::write(dir.path().join("staged.txt"), "a\nb\nc\n").unwrap();
    git(dir.path(), &["add", "--", "staged.txt"]);

    // 未跟踪：3 行
    std::fs::write(dir.path().join("untracked.txt"), "x\ny\nz\n").unwrap();

    dir
}

#[test]
fn resolves_a_real_repository() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-resolve");
    let repo = GitCliRepo::new();
    let resolution = repo
        .resolve_repository(dir.path())
        .expect("解析真实仓库不应失败");

    assert!(resolution.is_git_available);
    assert!(resolution.is_repository);
    assert!(resolution.is_usable());
    assert_eq!(resolution.workspace_in_repo_path, ".");

    // repo_root 应当指向临时目录本身（Windows 上分隔符与大小写都可能不同，比较规范化后的路径）
    let expected = std::fs::canonicalize(dir.path()).unwrap();
    let actual = std::fs::canonicalize(&resolution.repo_root).unwrap();
    assert_eq!(actual, expected);

    // Git 元数据刷新边界应当包含 .git
    assert!(
        resolution
            .auto_refresh_watch_paths
            .iter()
            .any(|entry| entry.path.replace('\\', "/").ends_with("/.git")),
        "刷新边界里应当有 .git：{:?}",
        resolution.auto_refresh_watch_paths
    );
}

#[test]
fn reads_a_real_workspace_status() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-status");
    let repo = GitCliRepo::new();
    let snapshot = repo.get_status(dir.path()).expect("读取真实状态不应失败");

    assert!(snapshot.summary.is_repository);
    assert_eq!(snapshot.summary.branch_name.as_deref(), Some("main"));
    assert_eq!(snapshot.summary.head_ref_type, GitHeadRefType::Branch);
    assert!(snapshot.summary.is_dirty);
    // 全新仓库没有上游
    assert!(snapshot.summary.tracking_branch_name.is_none());
    assert_eq!(snapshot.summary.ahead, 0);
    assert_eq!(snapshot.summary.behind, 0);

    let changes = get_workspace_changes(&snapshot);
    let by_path = |path: &str| {
        changes
            .iter()
            .find(|change| change.repo_relative_path == path)
            .unwrap_or_else(|| {
                panic!(
                    "没找到 {path}；实际有 {:?}",
                    changes
                        .iter()
                        .map(|change| change.repo_relative_path.as_str())
                        .collect::<Vec<_>>()
                )
            })
    };

    // 未暂存的修改
    let tracked = by_path("tracked.txt");
    assert_eq!(tracked.section, GitChangeSectionId::Unstaged);
    assert_eq!(tracked.kind, GitChangeKind::Modified);
    assert_eq!(tracked.y.as_deref(), Some("M"));
    assert!(!tracked.is_staged);
    assert_eq!(tracked.added, 1);
    assert_eq!(tracked.removed, 1);

    // 已暂存的新文件
    let staged = by_path("staged.txt");
    assert_eq!(staged.section, GitChangeSectionId::Staged);
    assert!(staged.is_staged);
    assert_eq!(staged.added, 3);
    assert_eq!(staged.removed, 0);

    // 未跟踪文件 —— 行数来自我们自己数的那一遍，不是 git
    let untracked = by_path("untracked.txt");
    assert_eq!(untracked.section, GitChangeSectionId::Untracked);
    assert!(untracked.is_untracked);
    assert_eq!(untracked.added, 3);
    assert_eq!(untracked.removed, 0);

    // 未跟踪统计表里也应当有它
    assert_eq!(
        snapshot
            .stat_for(GitChangeSection::Untracked, "untracked.txt")
            .added,
        3
    );

    // 汇总
    let summary = summarize_changes(&changes);
    assert_eq!(summary.file_count, 3);
    assert_eq!(summary.added, 1 + 3 + 3);
    assert_eq!(summary.removed, 1);
}

#[test]
fn subdirectory_workspace_is_scoped_to_that_directory() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-scope");
    // 在仓库子目录里再制造一处改动，以及仓库根的一处改动
    std::fs::write(dir.path().join("nested/inner.txt"), "inner changed\n").unwrap();
    std::fs::write(dir.path().join("root-level.txt"), "root\n").unwrap();

    let repo = GitCliRepo::new();
    let snapshot = repo
        .get_status(&dir.path().join("nested"))
        .expect("子目录 workspace 也应当能读");

    assert!(snapshot.summary.is_repository);
    assert_eq!(snapshot.summary.workspace_in_repo_path, "nested");

    let changes = get_workspace_changes(&snapshot);
    let paths: Vec<&str> = changes
        .iter()
        .map(|change| change.repo_relative_path.as_str())
        .collect();

    // 只有 nested 里的改动应当出现
    assert!(paths.contains(&"nested/inner.txt"), "实际：{paths:?}");
    assert!(
        !paths.contains(&"tracked.txt"),
        "仓库根的改动不该出现在子目录 workspace 里：{paths:?}"
    );
    assert!(
        !paths.contains(&"root-level.txt"),
        "仓库根的改动不该出现在子目录 workspace 里：{paths:?}"
    );

    // workspace 相对路径要削掉 nested/ 前缀
    let inner = changes
        .iter()
        .find(|change| change.repo_relative_path == "nested/inner.txt")
        .unwrap();
    assert_eq!(inner.workspace_relative_path, "inner.txt");
}

#[test]
fn lists_local_branches_with_current_first() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-branches");
    git(dir.path(), &["branch", "feature/a"]);
    git(dir.path(), &["branch", "feature/b"]);

    let repo = GitCliRepo::new();
    let result = repo
        .list_local_branches(dir.path())
        .expect("列本地分支不应失败");

    assert_eq!(result.head_ref_type, GitHeadRefType::Branch);
    assert_eq!(result.current_branch_name.as_deref(), Some("main"));
    assert_eq!(result.branches.len(), 3);
    assert_eq!(result.branches[0].name, "main");
    assert!(result.branches[0].is_current);
    assert!(result.branches[0].commit_hash.is_some());
    assert!(result.branches[0].commit_timestamp_ms.is_some());
    assert!(result.branches.iter().skip(1).all(|branch| !branch.is_current));
}

#[test]
fn a_plain_directory_is_not_an_error() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = TempDir::new("zcode-services-e2e-not-a-repo");

    // 临时目录如果恰好在某个仓库里（例如整个 Temp 被纳入版本控制），
    // 这个测试的前提就不成立了 —— 直接跳过，而不是给出假红。
    let probe = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(dir.path())
        .output();
    if probe.map(|output| output.status.success()).unwrap_or(false) {
        eprintln!("跳过：临时目录恰好位于某个 Git 仓库内");
        return;
    }

    let repo = GitCliRepo::new();
    let snapshot = repo.get_status(dir.path()).expect("非仓库目录不该报错");

    assert!(snapshot.summary.is_git_available, "git 本身是有的");
    assert!(!snapshot.summary.is_repository);
    assert!(!snapshot.summary.is_dirty);
    assert!(snapshot.entries.is_empty());
    assert!(get_workspace_changes(&snapshot).is_empty());
    assert!(snapshot.summary.branch_name.is_none());

    // 分支列表也应当优雅降级
    let branches = repo.list_local_branches(dir.path()).unwrap();
    assert!(branches.branches.is_empty());
    assert!(branches.current_branch_name.is_none());
}

#[test]
fn missing_working_directory_degrades_instead_of_erroring() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let missing = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("zcode-services-e2e-missing-cwd");
    let _ = std::fs::remove_dir_all(&missing);

    let repo = GitCliRepo::new();
    let snapshot = repo
        .get_status(&missing)
        .expect("目录不存在应当降级，而不是返回 Err");

    assert!(snapshot.summary.is_git_available);
    assert!(!snapshot.summary.is_repository);
    assert!(snapshot.entries.is_empty());
}

// ---------------------------------------------------------------------------
// 写操作
//
// 这一组的意义和上面的只读测试一样：证明「拼出来的命令行真的能被 git 接受」。
// 每条测试都从「跑写操作」走到「重新读状态」，验证**副作用真的发生了** ——
// 只断言「函数返回 Ok」是不够的，那样 `git add` 拼错参数也测不出来。
// ---------------------------------------------------------------------------

/// 从快照里找某个路径的条目，返回 (x, y) 状态码。
fn status_of(snapshot: &zcode_services::git::types::GitStatusSnapshot, path: &str) -> Option<String> {
    snapshot
        .entries
        .iter()
        .find(|entry| entry.path == path)
        .map(|entry| {
            format!(
                "{}{}",
                entry.x.as_deref().unwrap_or("."),
                entry.y.as_deref().unwrap_or(".")
            )
        })
}

#[test]
fn stage_moves_an_untracked_file_into_the_index() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-stage");
    let repo = GitCliRepo::new();

    // 未跟踪 → 不在暂存区
    let before = repo.get_status(dir.path()).unwrap();
    let changes = get_workspace_changes(&before);
    assert!(
        changes
            .iter()
            .any(|change| change.repo_relative_path == "untracked.txt"
                && change.section == GitChangeSectionId::Untracked),
        "前置条件：untracked.txt 应当出现在未跟踪区"
    );

    repo.stage(dir.path(), &["untracked.txt".to_string()])
        .expect("git add 应当成功");

    let after = repo.get_status(dir.path()).unwrap();
    let changes = get_workspace_changes(&after);
    assert!(
        changes
            .iter()
            .any(|change| change.repo_relative_path == "untracked.txt"
                && change.section == GitChangeSectionId::Staged),
        "stage 之后 untracked.txt 应当出现在已暂存区，实际：{changes:#?}"
    );
    assert!(
        !changes
            .iter()
            .any(|change| change.repo_relative_path == "untracked.txt"
                && change.section == GitChangeSectionId::Untracked),
        "stage 之后不应再出现在未跟踪区"
    );
}

#[test]
fn unstage_puts_a_file_back_into_the_working_tree() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-unstage");
    let repo = GitCliRepo::new();

    // staged.txt 在 fixture 里已经是已暂存状态
    let before = repo.get_status(dir.path()).unwrap();
    assert!(
        get_workspace_changes(&before)
            .iter()
            .any(|change| change.repo_relative_path == "staged.txt"
                && change.section == GitChangeSectionId::Staged),
        "前置条件：staged.txt 应当已在暂存区"
    );

    repo.unstage(dir.path(), &["staged.txt".to_string()])
        .expect("git restore --staged 应当成功");

    let after = repo.get_status(dir.path()).unwrap();
    let changes = get_workspace_changes(&after);
    assert!(
        !changes
            .iter()
            .any(|change| change.repo_relative_path == "staged.txt"
                && change.section == GitChangeSectionId::Staged),
        "unstage 之后不应再出现在已暂存区"
    );
    // 文件变成未跟踪（之前从未提交过）
    assert!(
        changes
            .iter()
            .any(|change| change.repo_relative_path == "staged.txt"
                && change.section == GitChangeSectionId::Untracked),
        "unstage 之后应当回到未跟踪区"
    );
}

#[test]
fn stage_then_unstage_round_trips() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-round-trip");
    let repo = GitCliRepo::new();
    let paths = vec!["tracked.txt".to_string()];

    repo.stage(dir.path(), &paths).unwrap();
    let staged = repo.get_status(dir.path()).unwrap();
    assert_eq!(
        status_of(&staged, "tracked.txt").as_deref(),
        Some("M."),
        "stage 后 index 与 worktree 都干净 → X=M, Y=."
    );

    repo.unstage(dir.path(), &paths).unwrap();
    let unstaged = repo.get_status(dir.path()).unwrap();
    assert_eq!(
        status_of(&unstaged, "tracked.txt").as_deref(),
        Some(".M"),
        "unstage 后改动回到 worktree → X=., Y=M"
    );
}

#[test]
fn discard_restores_an_unstaged_modification() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-discard-unstaged");
    let repo = GitCliRepo::new();

    // 前置条件：tracked.txt 确实被改过
    assert_eq!(
        read_text(dir.path(), "tracked.txt"),
        "line one\nline two changed\n"
    );

    repo.discard(dir.path(), &["tracked.txt".to_string()], false)
        .expect("git restore --worktree 应当成功");

    assert_eq!(
        read_text(dir.path(), "tracked.txt"),
        "line one\nline two\n",
        "丢弃之后工作区文件应当回到 HEAD 的内容"
    );
    let after = repo.get_status(dir.path()).unwrap();
    assert!(
        status_of(&after, "tracked.txt").is_none(),
        "丢弃之后 tracked.txt 不应再出现在变更列表里"
    );
}

#[test]
fn discard_with_staged_also_clears_the_index() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-discard-staged");
    let repo = GitCliRepo::new();

    // 先让 tracked.txt 同时有「已暂存 + 又改了一次」两个层面的改动
    std::fs::write(dir.path().join("tracked.txt"), "line one\nstaged version\n").unwrap();
    git(dir.path(), &["add", "--", "tracked.txt"]);
    std::fs::write(dir.path().join("tracked.txt"), "line one\nworktree version\n").unwrap();

    let before = repo.get_status(dir.path()).unwrap();
    assert_eq!(status_of(&before, "tracked.txt").as_deref(), Some("MM"));

    repo.discard(dir.path(), &["tracked.txt".to_string()], true)
        .expect("git restore --source=HEAD --staged --worktree 应当成功");

    assert_eq!(
        read_text(dir.path(), "tracked.txt"),
        "line one\nline two\n",
        "staged=true 应当把工作区也一并回退到 HEAD"
    );
    let after = repo.get_status(dir.path()).unwrap();
    assert!(
        status_of(&after, "tracked.txt").is_none(),
        "丢弃之后不应再有任何层面的改动"
    );
}

#[test]
fn discard_handles_a_file_that_was_deleted() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-discard-deleted");
    let repo = GitCliRepo::new();

    // 删掉一个已跟踪文件 —— 这正是「路径不存在、canonicalize 会失败」的场景，
    // 也是 normalize_input_path 必须有词法兜底的原因。
    std::fs::remove_file(dir.path().join("nested/inner.txt")).unwrap();

    let before = repo.get_status(dir.path()).unwrap();
    assert_eq!(status_of(&before, "nested/inner.txt").as_deref(), Some(".D"));

    repo.discard(dir.path(), &["nested/inner.txt".to_string()], false)
        .expect("丢弃删除也应当成功（路径不存在不能拦住它）");

    assert!(
        dir.path().join("nested/inner.txt").is_file(),
        "丢弃删除应当把文件恢复出来"
    );
}

#[test]
fn commit_creates_a_real_commit_with_the_given_message() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-commit");
    let repo = GitCliRepo::new();

    // 只提交已暂存的 staged.txt（fixture 里已 add）
    let result = repo
        .commit(dir.path(), "  feat: add staged file  ", &[])
        .expect("git commit 应当成功");

    assert_eq!(result.commit_hash.len(), 40, "应当是完整哈希");
    assert!(result.commit_hash.chars().all(|c| c.is_ascii_hexdigit()));

    // 提交信息应当被 trim 过
    let output = Command::new("git")
        .args(["log", "-1", "--pretty=%s"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "feat: add staged file"
    );

    // HEAD 应当就是刚返回的那个哈希
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&head.stdout).trim(), result.commit_hash);

    // 提交之后 staged.txt 不再出现在变更里
    let after = repo.get_status(dir.path()).unwrap();
    assert!(
        status_of(&after, "staged.txt").is_none(),
        "已提交的文件不应还在变更列表里"
    );
}

#[test]
fn commit_rejects_an_empty_message() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-commit-empty");
    let repo = GitCliRepo::new();

    for message in ["", "   ", "\t\n"] {
        let failure = repo.commit(dir.path(), message, &[]).unwrap_err();
        assert_eq!(
            failure.kind,
            zcode_services::git::cli_helpers::GitCommandFailureKind::InvalidArgument,
            "空白提交信息应当在跑 git 之前就被拦下"
        );
        assert_eq!(failure.message, "Commit message cannot be empty");
    }

    // 确认真的没有产生新提交：仍然只有 initial 那一个
    let count = Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&count.stdout).trim(), "1");
}

#[test]
fn commit_with_paths_only_commits_those_paths() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-commit-paths");
    let repo = GitCliRepo::new();

    // tracked.txt 有未暂存改动；用 pathspec 形式直接提交它
    repo.commit(dir.path(), "fix: tracked only", &["tracked.txt".to_string()])
        .expect("带 pathspec 的 commit 应当成功");

    let after = repo.get_status(dir.path()).unwrap();
    assert!(
        status_of(&after, "tracked.txt").is_none(),
        "被提交的路径应当从变更列表消失"
    );
    // staged.txt 仍然留在暂存区，没被顺手提交
    assert_eq!(
        status_of(&after, "staged.txt").as_deref(),
        Some("A."),
        "其它已暂存文件应当原样保留"
    );
}

#[test]
fn write_operations_refuse_a_non_repository() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    // ⚠️ 必须用一个**真的不在仓库里**的目录。用 TempDir（target/ 下）会命中本仓库，
    // 于是这些写操作会作用在真实仓库上 —— `push` 就真的推了一次。
    let Some(plain) = dir_outside_any_repository("write-non-repo") else {
        return;
    };
    let repo = GitCliRepo::new();
    let paths = vec!["a.txt".to_string()];

    // 先确认前提成立，否则后面断言的是错的东西
    assert!(!is_inside_a_repository(plain.path()));

    for failure in [
        repo.stage(plain.path(), &paths).unwrap_err(),
        repo.unstage(plain.path(), &paths).unwrap_err(),
        repo.discard(plain.path(), &paths, false).unwrap_err(),
        repo.commit(plain.path(), "msg", &[]).unwrap_err(),
    ] {
        assert_eq!(
            failure.kind,
            zcode_services::git::cli_helpers::GitCommandFailureKind::RepositoryUnavailable,
            "非仓库应当被前置校验拦下，而不是把 git 的原始报错抛出去：{}",
            failure.message
        );
    }

    // push 单独放最后：它是唯一会碰网络的，确认目录真的不在仓库里再跑。
    let failure = repo.push(plain.path()).unwrap_err();
    assert_eq!(
        failure.kind,
        zcode_services::git::cli_helpers::GitCommandFailureKind::RepositoryUnavailable
    );
}

#[test]
fn push_without_a_remote_reports_a_clear_error() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    // 临时仓库没有配 remote，也没有 upstream
    let dir = build_repository("zcode-services-e2e-push-no-remote");
    let repo = GitCliRepo::new();

    let failure = repo.push(dir.path()).unwrap_err();
    assert_eq!(
        failure.kind,
        zcode_services::git::cli_helpers::GitCommandFailureKind::RepositoryUnavailable
    );
    assert_eq!(
        failure.message,
        "No Git remote is configured for the current repository."
    );
}

#[test]
fn push_refuses_a_detached_head() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-push-detached");
    // 先配一个 remote，把「没有 remote」那条分支排除掉
    git(dir.path(), &["remote", "add", "origin", "https://example.invalid/repo.git"]);
    git(dir.path(), &["checkout", "--quiet", "--detach", "HEAD"]);

    let repo = GitCliRepo::new();
    let failure = repo.push(dir.path()).unwrap_err();
    assert_eq!(failure.message, "Cannot push while HEAD is detached.");
}

#[test]
fn write_operations_reject_paths_outside_the_repository() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-write-outside");
    let repo = GitCliRepo::new();
    let outside = vec!["../outside.txt".to_string()];

    for failure in [
        repo.stage(dir.path(), &outside).unwrap_err(),
        repo.unstage(dir.path(), &outside).unwrap_err(),
        repo.discard(dir.path(), &outside, false).unwrap_err(),
    ] {
        assert_eq!(
            failure.kind,
            zcode_services::git::cli_helpers::GitCommandFailureKind::PathOutsideRepository,
            "越界路径应当在跑 git 之前被拦下"
        );
    }
}

#[test]
fn write_operations_with_no_paths_are_a_no_op() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-write-no-paths");
    let repo = GitCliRepo::new();
    let before = repo.get_status(dir.path()).unwrap();

    repo.stage(dir.path(), &[]).unwrap();
    repo.unstage(dir.path(), &[]).unwrap();
    repo.discard(dir.path(), &[], true).unwrap();

    let after = repo.get_status(dir.path()).unwrap();
    assert_eq!(
        before.entries.len(),
        after.entries.len(),
        "空路径列表不应当改变任何东西"
    );
    assert_eq!(
        status_of(&after, "tracked.txt").as_deref(),
        Some(".M"),
        "改动应当原样保留"
    );
}

// ---------------------------------------------------------------------------
// 其余读操作：identity / ignored paths / commit graph / diff
//
// 这一组测的是「拼出来的命令行真能被 git 接受，并且解析出的形状与界面预期一致」。
// 单文件 diff 尤其值得端到端跑 —— 它的三种来源（分支比较 / 暂存区 / 未跟踪）
// 走的是完全不同的命令，且未跟踪那条是**我们手搓 patch**，只有真跑一遍才知道
// 格式对不对。
// ---------------------------------------------------------------------------

/// 造一个「干净」的仓库：只有一条提交，工作区没有改动。
fn build_clean_repository(name: &str, file: &str, content: &str) -> TempDir {
    let dir = TempDir::new(name);
    git(dir.path(), &["init", "--quiet", "-b", "main"]);
    std::fs::write(dir.path().join(file), content).unwrap();
    git(dir.path(), &["add", "--", file]);
    git(dir.path(), &["commit", "--quiet", "-m", "initial"]);
    dir
}

#[test]
fn reads_the_configured_identity() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_clean_repository("zcode-services-e2e-identity", "a.txt", "a\n");
    git(dir.path(), &["config", "user.name", "ZCode Test"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);

    let repo = GitCliRepo::new();
    let identity = repo.get_identity(dir.path()).expect("读身份不应失败");

    assert_eq!(identity.user_name.as_deref(), Some("ZCode Test"));
    assert_eq!(identity.user_email.as_deref(), Some("test@example.com"));
    // 我们写的是 local 配置，作用域应当报 local
    assert_eq!(identity.scope_label.as_deref(), Some("local"));
    let source = identity.name_source.expect("应当能报出值来自哪个文件");
    assert!(source.contains("config"), "来源应当是配置文件：{source}");
}

#[test]
fn identity_is_empty_for_a_plain_directory() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let Some(plain) = dir_outside_any_repository("identity-non-repo") else {
        return;
    };
    let repo = GitCliRepo::new();
    let identity = repo.get_identity(plain.path()).expect("非仓库不该报错");

    assert!(identity.user_name.is_none());
    assert!(identity.user_email.is_none());
    assert!(identity.scope_label.is_none());
}

#[test]
fn reports_which_paths_are_ignored() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-ignored");
    std::fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
    std::fs::write(dir.path().join("ignored.txt"), "nope\n").unwrap();

    let repo = GitCliRepo::new();
    let inputs = vec!["ignored.txt".to_string(), "tracked.txt".to_string()];
    let ignored = repo
        .get_ignored_paths(dir.path(), &inputs)
        .expect("check-ignore 不应失败");

    assert_eq!(ignored.len(), 1, "只有 ignored.txt 被忽略：{ignored:?}");
    let only = ignored[0].replace('\\', "/");
    assert!(
        only.ends_with("/ignored.txt"),
        "返回的应当是绝对路径：{only}"
    );
}

#[test]
fn ignored_paths_is_empty_for_empty_input_and_plain_directory() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let repo = GitCliRepo::new();

    // 空输入直接返回空，连命令都不跑
    let dir = build_repository("zcode-services-e2e-ignored-empty");
    assert!(repo.get_ignored_paths(dir.path(), &[]).unwrap().is_empty());

    // 非仓库也是空列表，而不是报错
    let Some(plain) = dir_outside_any_repository("ignored-non-repo") else {
        return;
    };
    let inputs = vec!["a.txt".to_string()];
    assert!(repo
        .get_ignored_paths(plain.path(), &inputs)
        .unwrap()
        .is_empty());
}

#[test]
fn commit_graph_lists_real_commits_newest_first() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_clean_repository("zcode-services-e2e-graph", "a.txt", "one\n");
    std::fs::write(dir.path().join("a.txt"), "two\n").unwrap();
    git(dir.path(), &["add", "--", "a.txt"]);
    git(dir.path(), &["commit", "--quiet", "-m", "second"]);
    std::fs::write(dir.path().join("a.txt"), "three\n").unwrap();
    git(dir.path(), &["add", "--", "a.txt"]);
    git(dir.path(), &["commit", "--quiet", "-m", "third"]);

    let repo = GitCliRepo::new();
    let graph = repo
        .get_commit_graph(dir.path(), None, None)
        .expect("读提交图不应失败");

    assert_eq!(graph.commits.len(), 3);
    assert!(!graph.has_more);
    assert_eq!(graph.commits[0].subject, "third");
    assert_eq!(graph.commits[2].subject, "initial");
    assert_eq!(graph.commits[0].parents.len(), 1, "HEAD 应当有一个父提交");
    assert!(graph.commits[2].parents.is_empty(), "根提交没有父");
    assert!(graph.commits[0].authored_at_ms.is_some());

    // HEAD 那一条应当同时带 HEAD 与 main 两个 ref
    let names: Vec<&str> = graph.commits[0]
        .refs
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert!(names.contains(&"HEAD"), "实际 refs：{names:?}");
    assert!(names.contains(&"main"), "实际 refs：{names:?}");
}

#[test]
fn commit_graph_respects_max_count_and_reports_more() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_clean_repository("zcode-services-e2e-graph-page", "a.txt", "one\n");
    for index in 2..=3 {
        std::fs::write(dir.path().join("a.txt"), format!("v{index}\n")).unwrap();
        git(dir.path(), &["add", "--", "a.txt"]);
        git(dir.path(), &["commit", "--quiet", "-m", &format!("c{index}")]);
    }

    let repo = GitCliRepo::new();
    let page = repo.get_commit_graph(dir.path(), Some(2), None).unwrap();
    assert_eq!(page.commits.len(), 2);
    assert!(page.has_more, "还有第三条没取，应当报 has_more");
    assert_eq!(page.commits[0].subject, "c3");

    // skip 掉最新的一条
    let skipped = repo.get_commit_graph(dir.path(), Some(2), Some(1)).unwrap();
    assert_eq!(skipped.commits[0].subject, "c2");
    assert_eq!(skipped.commits.len(), 2);
    assert!(!skipped.has_more);
}

#[test]
fn commit_graph_is_empty_for_a_repository_without_commits() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    // 空仓库不是错误，是一个正常的空图
    let dir = TempDir::new("zcode-services-e2e-graph-empty");
    git(dir.path(), &["init", "--quiet", "-b", "main"]);

    let repo = GitCliRepo::new();
    let graph = repo
        .get_commit_graph(dir.path(), None, None)
        .expect("空仓库应当返回空图而不是报错");
    assert!(graph.commits.is_empty());
    assert!(!graph.has_more);
}

#[test]
fn diff_shows_an_unstaged_modification_with_both_sides() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-diff-unstaged");
    let repo = GitCliRepo::new();

    let diff = repo
        .get_diff(dir.path(), &GitDiffQuery::unstaged("tracked.txt"))
        .expect("取 diff 不应失败");

    assert_eq!(diff.availability, GitDiffAvailability::Patch);
    let patch = diff.patch.expect("应当有 patch");
    assert!(patch.contains("-line two"), "patch：{patch}");
    assert!(patch.contains("+line two changed"), "patch：{patch}");

    // 两侧完整内容都应当拿到：HEAD 版本 → 工作区版本
    assert_eq!(diff.before_content.as_deref(), Some("line one\nline two\n"));
    assert_eq!(
        diff.after_content.as_deref(),
        Some("line one\nline two changed\n")
    );
}

#[test]
fn diff_for_a_staged_modification_fills_both_sides() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_clean_repository("zcode-services-e2e-diff-staged", "a.txt", "v1\n");
    std::fs::write(dir.path().join("a.txt"), "v2\n").unwrap();
    git(dir.path(), &["add", "--", "a.txt"]);

    let repo = GitCliRepo::new();
    let diff = repo
        .get_diff(dir.path(), &GitDiffQuery::staged("a.txt"))
        .expect("取暂存区 diff 不应失败");

    assert_eq!(diff.availability, GitDiffAvailability::Patch);
    // HEAD 版本 → 索引版本
    assert_eq!(diff.before_content.as_deref(), Some("v1\n"));
    assert_eq!(diff.after_content.as_deref(), Some("v2\n"));
}

#[test]
fn diff_for_a_newly_staged_file_keeps_the_patch_without_contents() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-diff-staged-new");
    let repo = GitCliRepo::new();

    // staged.txt 是新增文件：`HEAD:staged.txt` 取不到 → 完整内容必须**整体降级**，
    // 但 patch 本身是好的，不能一起丢。这正是 to_complete_diff_contents 的规则。
    let diff = repo
        .get_diff(dir.path(), &GitDiffQuery::staged("staged.txt"))
        .expect("取 diff 不应失败");

    assert_eq!(diff.availability, GitDiffAvailability::Patch);
    assert!(diff.patch.is_some(), "patch 必须留下");
    assert!(
        diff.before_content.is_none() && diff.after_content.is_none(),
        "任一侧取不到就应当整体不带完整内容"
    );
}

#[test]
fn diff_for_an_untracked_file_uses_the_minimal_patch() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-diff-untracked");
    let repo = GitCliRepo::new();

    let diff = repo
        .get_diff(dir.path(), &GitDiffQuery::unstaged("untracked.txt"))
        .expect("取未跟踪文件 diff 不应失败");

    assert_eq!(diff.availability, GitDiffAvailability::Patch);
    let patch = diff.patch.expect("应当有手搓的 patch");
    // 我们不用 `--no-index`，只保留 ---/+++/@@ 三段
    assert!(patch.starts_with("--- /dev/null\n+++ b/untracked.txt\n"), "patch：{patch}");
    assert!(patch.contains("@@ -0,0 +1,3 @@"), "patch：{patch}");
    assert!(patch.contains("+x\n+y\n+z\n"), "patch：{patch}");
    assert_eq!(diff.before_content.as_deref(), Some(""));
    assert_eq!(diff.after_content.as_deref(), Some("x\ny\nz\n"));
}

#[test]
fn diff_reports_binary_content_as_not_previewable() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-diff-binary");
    // 含 NUL 字节 → 二进制
    std::fs::write(dir.path().join("blob.bin"), [0u8, 1, 2, 3, 0, 255]).unwrap();

    let repo = GitCliRepo::new();
    let diff = repo
        .get_diff(dir.path(), &GitDiffQuery::unstaged("blob.bin"))
        .expect("取二进制 diff 不应失败");

    assert_eq!(diff.availability, GitDiffAvailability::Binary);
    assert_eq!(
        diff.summary.as_deref(),
        Some("Binary diff is not previewable.")
    );
    assert!(diff.patch.is_none());
}

#[test]
fn diff_for_a_missing_file_is_unavailable() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-diff-missing");
    let repo = GitCliRepo::new();

    let diff = repo
        .get_diff(dir.path(), &GitDiffQuery::unstaged("does-not-exist.txt"))
        .expect("不存在的路径不该报错，只应当 unavailable");

    assert_eq!(diff.availability, GitDiffAvailability::Unavailable);
    assert_eq!(
        diff.summary.as_deref(),
        Some("No Git diff is available for this file.")
    );
    assert!(diff.patch.is_none());
}

#[test]
fn diff_for_a_plain_directory_is_unavailable() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let Some(plain) = dir_outside_any_repository("diff-non-repo") else {
        return;
    };
    let repo = GitCliRepo::new();
    let diff = repo
        .get_diff(plain.path(), &GitDiffQuery::unstaged("a.txt"))
        .expect("非仓库不该报错");

    assert_eq!(diff.availability, GitDiffAvailability::Unavailable);
    assert_eq!(
        diff.summary.as_deref(),
        Some("Workspace is not inside a Git repository.")
    );
}

#[test]
fn diff_without_an_upstream_cannot_do_a_branch_comparison() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-diff-branch-no-upstream");
    let repo = GitCliRepo::new();

    let diff = repo
        .get_diff(dir.path(), &GitDiffQuery::branch("tracked.txt"))
        .expect("没有上游时不该报错");

    assert_eq!(diff.availability, GitDiffAvailability::Unavailable);
    assert_eq!(
        diff.summary.as_deref(),
        Some("Current branch does not have an upstream branch.")
    );
}

#[test]
fn diff_with_a_branch_source_compares_against_the_upstream() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    // 用一个本地目录当 remote，避免测试碰网络。
    let remote = build_clean_repository("zcode-services-e2e-branch-remote", "file.txt", "v1\n");

    let dir = build_clean_repository("zcode-services-e2e-branch-local", "file.txt", "v1\n");
    let remote_url = remote.path().to_str().unwrap().to_string();
    git(dir.path(), &["remote", "add", "origin", &remote_url]);
    git(dir.path(), &["fetch", "--quiet", "origin"]);
    git(
        dir.path(),
        &["branch", "--set-upstream-to=origin/main", "main"],
    );

    // 本地往前走一条提交，此时 origin/main...HEAD 恰好是 v1 → v2
    std::fs::write(dir.path().join("file.txt"), "v2\n").unwrap();
    git(dir.path(), &["add", "--", "file.txt"]);
    git(dir.path(), &["commit", "--quiet", "-m", "v2"]);

    let repo = GitCliRepo::new();
    let diff = repo
        .get_diff(dir.path(), &GitDiffQuery::branch("file.txt"))
        .expect("分支比较不应失败");

    assert_eq!(diff.availability, GitDiffAvailability::Patch);
    let patch = diff.patch.expect("应当有 patch");
    assert!(patch.contains("-v1"), "patch：{patch}");
    assert!(patch.contains("+v2"), "patch：{patch}");
    // 完整内容来自 merge-base（v1）与 HEAD（v2）
    assert_eq!(diff.before_content.as_deref(), Some("v1\n"));
    assert_eq!(diff.after_content.as_deref(), Some("v2\n"));
}

#[test]
fn diff_rejects_paths_outside_the_repository() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-services-e2e-diff-outside");
    let repo = GitCliRepo::new();

    let failure = repo
        .get_diff(dir.path(), &GitDiffQuery::unstaged("../outside.txt"))
        .unwrap_err();
    assert_eq!(
        failure.kind,
        zcode_services::git::cli_helpers::GitCommandFailureKind::PathOutsideRepository
    );
}
