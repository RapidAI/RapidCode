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
use zcode_app::workspace::{
    create_workspace_channel, next_workspace_message, WorkspaceAction, WorkspaceMessage,
};
use zcode_services::git::types::GitDiffQuery;
use zcode_shared::git::{GitChangeSectionId, GitDiffAvailability};

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

/// 读文件并把行尾统一成 `\n`。
///
/// Windows 上 `core.autocrlf` 默认为 `true`，`git restore` 写回工作区的内容是
/// CRLF。直接比字符串会得到「内容其实对、断言却失败」的假故障。
fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("读 {} 失败: {error}", path.display()))
        .replace("\r\n", "\n")
}

/// 变更条目上展示用的相对路径（和界面一致：优先 workspace 相对）。
fn paths_in(section: GitChangeSectionId, data: &zcode_app::workspace::WorkspaceData) -> Vec<String> {
    data.changes_in(section)
        .iter()
        .map(|change| {
            if change.workspace_relative_path.is_empty() {
                change.repo_relative_path.clone()
            } else {
                change.workspace_relative_path.clone()
            }
        })
        .collect()
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

// ---------------------------------------------------------------------------
// 写操作
// ---------------------------------------------------------------------------
//
// 这里测的是 `refresh` 之外的那条路：`run` 除了重新扫描，还要先把写操作的
// **结果**报回界面。顺序是刻意的 —— Loading → Notice → Loaded，
// 用户先看到「暂存成功」再看到列表刷新，而不是先刷新再猜刚才那下成没成。

/// 把写操作的三条消息拆开，顺带断言顺序。
fn split_write_messages(messages: Vec<WorkspaceMessage>) -> (WorkspaceMessage, WorkspaceMessage) {
    assert_eq!(
        messages.len(),
        3,
        "应当收到 Loading + Notice + Loaded，实际 {messages:?}"
    );

    assert!(
        matches!(messages[0], WorkspaceMessage::Loading),
        "第一条应当是 Loading，实际 {:?}",
        messages[0]
    );

    // 后两条要按「先结果、后刷新」的顺序回传。
    let mut iter = messages.into_iter();
    let _ = iter.next();
    let notice = iter.next().expect("第二条");
    let loaded = iter.next().expect("第三条");
    (notice, loaded)
}

#[test]
fn run_stages_a_file_and_reloads_through_the_background_channel() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-app-loader-stage");
    let channel = create_workspace_channel();
    let receiver = channel.take_receiver().expect("第一次应当能取到结果端");

    assert!(
        channel.requester().run(
            dir.path().to_path_buf(),
            WorkspaceAction::Stage(vec!["untracked.txt".to_string()]),
        ),
        "写操作应当被接受"
    );

    let (notice, loaded) = split_write_messages(collect_messages(
        receiver,
        3,
        Duration::from_secs(30),
    ));

    match notice {
        WorkspaceMessage::Notice(notice) => {
            assert!(!notice.is_error, "暂存应当成功：{}", notice.message);
            assert!(
                notice.message.starts_with("暂存成功"),
                "提示应当带动作名，实际：{}",
                notice.message
            );
        }
        other => panic!("第二条应当是 Notice，实际 {other:?}"),
    }

    match loaded {
        WorkspaceMessage::Loaded(data) => {
            // 关键：写操作之后界面拿到的必须是**重新扫出来的**状态，
            // 否则用户点完暂存，文件还挂在「未跟踪」里，会以为没生效。
            assert!(
                paths_in(GitChangeSectionId::Staged, &data).contains(&"untracked.txt".to_string()),
                "untracked.txt 应当已进入「已暂存」，实际：{:?}",
                paths_in(GitChangeSectionId::Staged, &data)
            );
            assert!(
                !paths_in(GitChangeSectionId::Untracked, &data).contains(&"untracked.txt".to_string()),
                "同一个文件不该同时留在「未跟踪」里"
            );
        }
        other => panic!("第三条应当是 Loaded，实际 {other:?}"),
    }
}

#[test]
fn a_failed_write_still_reloads_and_reports_the_reason() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-app-loader-failure");
    let channel = create_workspace_channel();
    let receiver = channel.take_receiver().unwrap();

    // 空提交信息在服务层就被拦下了（根本不跑 git），所以这里能稳定复现失败。
    assert!(channel.requester().run(
        dir.path().to_path_buf(),
        WorkspaceAction::Commit {
            message: "   ".to_string(),
        },
    ));

    let (notice, loaded) = split_write_messages(collect_messages(
        receiver,
        3,
        Duration::from_secs(30),
    ));

    match notice {
        WorkspaceMessage::Notice(notice) => {
            assert!(notice.is_error, "空提交信息应当失败");
            assert!(
                notice.message.starts_with("提交失败"),
                "实际：{}",
                notice.message
            );
            assert!(
                notice.message.contains("Commit message cannot be empty"),
                "提示里应当带上 git 侧的原因，实际：{}",
                notice.message
            );
        }
        other => panic!("第二条应当是 Notice，实际 {other:?}"),
    }

    // 失败之后**仍然要刷新**：否则界面上还留着上一次的状态，
    // 而用户刚刚明明动过工作区，两边的差异会一直藏在那儿。
    match loaded {
        WorkspaceMessage::Loaded(data) => {
            assert!(data.is_repository());
            assert!(
                paths_in(GitChangeSectionId::Unstaged, &data).contains(&"tracked.txt".to_string()),
                "工作区没被改动，tracked.txt 应当还在「未暂存」"
            );
        }
        other => panic!("第三条应当是 Loaded，实际 {other:?}"),
    }
}

#[test]
fn discard_through_the_channel_restores_the_working_tree() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-app-loader-discard");
    let target = dir.path().join("tracked.txt");
    assert_eq!(read_text(&target), "one\ntwo\n", "前置条件：改动已在工作区");

    let channel = create_workspace_channel();
    let receiver = channel.take_receiver().unwrap();

    assert!(channel.requester().run(
        dir.path().to_path_buf(),
        WorkspaceAction::Discard {
            paths: vec!["tracked.txt".to_string()],
            staged: false,
        },
    ));

    let (notice, loaded) = split_write_messages(collect_messages(
        receiver,
        3,
        Duration::from_secs(30),
    ));

    match notice {
        WorkspaceMessage::Notice(notice) => {
            assert!(!notice.is_error, "丢弃应当成功：{}", notice.message);
        }
        other => panic!("第二条应当是 Notice，实际 {other:?}"),
    }

    // 这条是唯一会丢数据的操作，所以除了「提示说成功」之外，
    // 还要真的去磁盘上确认内容回到了 HEAD 版本。
    assert_eq!(read_text(&target), "one\n", "丢弃后应当回到 HEAD 内容");

    match loaded {
        WorkspaceMessage::Loaded(data) => {
            assert!(
                !paths_in(GitChangeSectionId::Unstaged, &data).contains(&"tracked.txt".to_string()),
                "tracked.txt 已经干净了，不该再出现在「未暂存」里"
            );
        }
        other => panic!("第三条应当是 Loaded，实际 {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// diff 加载
// ---------------------------------------------------------------------------
//
// 这条路刻意**不共用** `busy` 标志：读 diff 不改仓库、也不碰 `index.lock`，
// 没有理由和「刷新 / 提交」互相阻塞。所以它也不发 Loading —— 界面上的
// 「加载中」是由视图自己置的（它才是知道「哪一行被展开」的那一方）。

#[test]
fn load_diff_returns_a_patch_through_the_background_channel() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-app-loader-diff");
    let channel = create_workspace_channel();
    let receiver = channel.take_receiver().expect("第一次应当能取到结果端");

    let id = channel.requester().load_diff(
        dir.path().to_path_buf(),
        GitDiffQuery::unstaged("tracked.txt"),
        "tracked.txt".to_string(),
    );
    assert!(id > 0, "请求 id 必须从 1 开始");

    let messages = collect_messages(receiver, 1, Duration::from_secs(30));
    assert_eq!(
        messages.len(),
        1,
        "diff 不该顺带发 Loading/Loaded —— 它不走 busy 那条路：{messages:?}"
    );

    match &messages[0] {
        WorkspaceMessage::Diff(preview) => {
            assert_eq!(preview.id, id, "回传的 id 必须与请求一致");
            assert_eq!(preview.path, "tracked.txt");
            let diff = preview.outcome.as_ref().expect("取 diff 不该失败");
            assert_eq!(diff.availability, GitDiffAvailability::Patch);
            let patch = diff.patch.as_deref().expect("应当有 patch");
            assert!(patch.contains("+two"), "patch：{patch}");
            // 完整内容也要一并回传（before 来自索引，after 来自工作区）
            assert_eq!(diff.before_content.as_deref(), Some("one\n"));
            assert_eq!(diff.after_content.as_deref(), Some("one\ntwo\n"));
        }
        other => panic!("应当是 Diff，实际 {other:?}"),
    }
}

#[test]
fn concurrent_diff_requests_get_distinct_ids() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-app-loader-diff-ids");
    let channel = create_workspace_channel();
    let receiver = channel.take_receiver().unwrap();
    let requester = channel.requester();

    // 连开两个文件：两次都必须被接受（不排队、不丢弃），各自带自己的 id，
    // 由界面按 id 决定采纳哪一份。
    let first = requester.load_diff(
        dir.path().to_path_buf(),
        GitDiffQuery::unstaged("tracked.txt"),
        "tracked.txt".to_string(),
    );
    let second = requester.load_diff(
        dir.path().to_path_buf(),
        GitDiffQuery::unstaged("untracked.txt"),
        "untracked.txt".to_string(),
    );
    assert!(second > first, "id 必须单调递增：{first} → {second}");

    let messages = collect_messages(receiver, 2, Duration::from_secs(30));
    assert_eq!(messages.len(), 2, "两次请求都应当回来：{messages:?}");

    let mut ids: Vec<u64> = messages
        .iter()
        .map(|message| match message {
            WorkspaceMessage::Diff(preview) => preview.id,
            other => panic!("应当是 Diff，实际 {other:?}"),
        })
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, vec![first, second]);

    // 未跟踪文件走的是「手搓 patch」那条路
    let untracked = messages
        .iter()
        .find_map(|message| match message {
            WorkspaceMessage::Diff(preview) if preview.path == "untracked.txt" => Some(preview),
            _ => None,
        })
        .expect("应当有 untracked.txt 的结果");
    let diff = untracked.outcome.as_ref().expect("不该失败");
    assert_eq!(diff.availability, GitDiffAvailability::Patch);
    assert!(
        diff.patch.as_deref().unwrap().starts_with("--- /dev/null"),
        "未跟踪文件应当走最简 unified diff"
    );
}

#[test]
fn a_failing_diff_reports_the_reason_instead_of_going_silent() {
    if !git_available() {
        eprintln!("跳过：本机找不到 git");
        return;
    }

    let dir = build_repository("zcode-app-loader-diff-outside");
    let channel = create_workspace_channel();
    let receiver = channel.take_receiver().unwrap();

    // 越界路径在服务层就被拦下，于是这条会以 Err 回来。
    let id = channel.requester().load_diff(
        dir.path().to_path_buf(),
        GitDiffQuery::unstaged("../outside.txt"),
        "../outside.txt".to_string(),
    );

    let messages = collect_messages(receiver, 1, Duration::from_secs(30));
    match &messages[0] {
        WorkspaceMessage::Diff(preview) => {
            assert_eq!(preview.id, id);
            let message = preview
                .outcome
                .as_ref()
                .expect_err("越界路径应当以 Err 回来");
            assert!(
                message.contains("outside repository"),
                "原因必须带出来，否则界面只能显示一个空的「加载中」：{message}"
            );
        }
        other => panic!("应当是 Diff，实际 {other:?}"),
    }
}
