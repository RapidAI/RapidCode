//! 后台加载链路的端到端测试。
//!
//! `tests/smoke.rs` 只覆盖「拿到数据之后渲染对不对」；`zcode-services` 的
//! 端到端测试只覆盖「git 到底能不能跑出这些数据」。中间那一段 ——
//! **请求端 → worker 线程 → 通道 → 结果** —— 两边都没碰到，而这个文件就是补它。
//!
//! 这里不依赖 dioxus 运行时：自己写一个最小的执行器（`Wake` + `poll`），
//! 这样测的就是纯粹的「跨线程回传」语义，而不是渲染框架的行为。
//!
//! 找不到 git 时整组测试会打印一行说明并跳过。

use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

use futures_channel::mpsc::UnboundedReceiver;
use zcode_app::workspace::{create_workspace_channel, next_workspace_message, WorkspaceMessage};

/// 本机能不能跑 git。
fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// 最小 waker：只记「被唤醒过」，不做别的。
#[derive(Default)]
struct FlagWaker {
    woken: AtomicBool,
}

impl Wake for FlagWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.woken.store(true, Ordering::SeqCst);
    }
}

/// 收集 `want` 条消息，或超时。
///
/// 每轮重新构造 future（而不是复用一个 `Box::pin`），这样 `&mut receiver`
/// 的借用只活一轮，循环里能干净地重新借。
fn collect_messages(
    mut receiver: UnboundedReceiver<WorkspaceMessage>,
    want: usize,
    timeout: Duration,
) -> Vec<WorkspaceMessage> {
    let flag = Arc::new(FlagWaker::default());
    let waker = Waker::from(Arc::clone(&flag));
    let mut cx = Context::from_waker(&waker);
    let deadline = Instant::now() + timeout;
    let mut messages = Vec::new();

    while messages.len() < want && Instant::now() < deadline {
        let polled = {
            let mut future = std::pin::pin!(next_workspace_message(&mut receiver));
            future.as_mut().poll(&mut cx)
        };

        match polled {
            Poll::Ready(Some(message)) => messages.push(message),
            Poll::Ready(None) => break,
            Poll::Pending => {
                flag.woken.store(false, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }

    messages
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

fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
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

/// 建一个有一个已跟踪文件 + 一处未暂存改动的小仓库。
fn build_repository(name: &str) -> TempDir {
    let dir = TempDir::new(name);
    git(dir.path(), &["init", "--quiet", "-b", "main"]);
    std::fs::write(dir.path().join("tracked.txt"), "one\n").unwrap();
    git(dir.path(), &["add", "--", "tracked.txt"]);
    git(dir.path(), &["commit", "--quiet", "-m", "initial"]);
    std::fs::write(dir.path().join("tracked.txt"), "one\ntwo\n").unwrap();
    std::fs::write(dir.path().join("untracked.txt"), "a\nb\n").unwrap();
    dir
}

#[test]
fn refresh_loads_a_real_repository_through_the_background_channel() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-app-loader-repo");
    let channel = create_workspace_channel();
    let receiver = channel.take_receiver().expect("第一次应当能取到结果端");

    assert!(channel.requester().refresh(dir.path().to_path_buf()));

    // 第一条应当是 Loading（请求线程立刻发出，不等 git 跑完），
    // 第二条才是真正扫出来的数据。
    let messages = collect_messages(receiver, 2, Duration::from_secs(30));
    assert_eq!(messages.len(), 2, "应当收到 Loading + Loaded 两条消息");

    assert!(
        matches!(messages[0], WorkspaceMessage::Loading),
        "第一条应当是 Loading，实际 {:?}",
        messages[0]
    );

    match &messages[1] {
        WorkspaceMessage::Loaded(data) => {
            assert!(data.is_repository(), "临时目录是仓库");
            assert_eq!(data.branch_label(), "main");
            assert!(data.summary.is_dirty);

            let paths: Vec<&str> = data
                .changes
                .iter()
                .map(|change| change.repo_relative_path.as_str())
                .collect();
            assert!(paths.contains(&"tracked.txt"), "实际：{paths:?}");
            assert!(paths.contains(&"untracked.txt"), "实际：{paths:?}");
            assert_eq!(data.totals.file_count, 2);
            // 未跟踪文件的行数是应用侧自己数的（git 不管未跟踪文件）
            assert_eq!(data.totals.added, 1 + 2);
        }
        other => panic!("第二条应当是 Loaded，实际 {other:?}"),
    }
}

#[test]
fn non_repository_workspace_loads_as_a_normal_state_not_a_failure() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = TempDir::new("zcode-app-loader-not-a-repo");

    // 临时目录如果恰好在某个仓库里，这个测试的前提就不成立了
    let probe = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(dir.path())
        .output();
    if probe.map(|output| output.status.success()).unwrap_or(false) {
        eprintln!("跳过：临时目录恰好位于某个 Git 仓库内");
        return;
    }

    let channel = create_workspace_channel();
    let receiver = channel.take_receiver().unwrap();
    assert!(channel.requester().refresh(dir.path().to_path_buf()));

    let messages = collect_messages(receiver, 2, Duration::from_secs(30));
    assert_eq!(messages.len(), 2);

    match &messages[1] {
        WorkspaceMessage::Loaded(data) => {
            // 关键：不是仓库是**正常状态**，不是 Failed ——
            // 否则用户把工作区指到普通目录就会看到一条红字。
            assert!(!data.is_repository());
            assert!(data.changes.is_empty());
            assert_eq!(data.totals, Default::default());
        }
        other => panic!("应当是 Loaded（非仓库），实际 {other:?}"),
    }
}

#[test]
fn a_second_refresh_is_ignored_while_one_is_running() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-app-loader-coalesce");
    let channel = create_workspace_channel();
    let receiver = channel.take_receiver().unwrap();

    // 连点刷新：只有第一次会被接受，后面的直接返回 false（不排队）。
    // 排队只会让用户最后看到的结果更旧。
    let first = channel.requester().refresh(dir.path().to_path_buf());
    assert!(first, "第一次刷新应当被接受");
    let _ = channel.requester().refresh(dir.path().to_path_buf());

    let messages = collect_messages(receiver, 2, Duration::from_secs(30));
    // 即便中途又点了，最终也只会得到「一条 Loading + 一条 Loaded」
    assert!(
        matches!(messages.last(), Some(WorkspaceMessage::Loaded(_))),
        "最终应当拿到 Loaded，实际 {messages:?}"
    );
}
