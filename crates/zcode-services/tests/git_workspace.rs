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
use zcode_services::git::types::GitChangeSection;
use zcode_shared::git::{GitChangeKind, GitChangeSectionId, GitHeadRefType};

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
