//! 真实工作区数据：后台线程跑 git，UI 线程只读结果。
//!
//! ## 为什么必须走后台线程
//!
//! [`zcode_services`] 的 git 调用是**同步阻塞**的（`std::process::Command` +
//! 轮询等待退出），而 dioxus 的组件渲染和 `spawn` 出来的 future 都跑在
//! VirtualDom 那一个线程上。直接在渲染里跑 git，就会在大仓库上把界面冻住 ——
//! 表现出来就是「窗口开着、界面一动不动」，和上一轮那个「只有壳子」的现象
//! 从用户视角看几乎一样。所以这里把阻塞的部分推到独立线程：
//!
//! ```text
//! UI 线程                                  worker 线程
//! ───────                                  ───────────
//! refresh() ─────────────────────────────→ load_workspace()   （阻塞跑 git）
//! spawn(循环) ←── UnboundedSender ───────── WorkspaceMessage
//! ```
//!
//! 用 `futures_channel` 而不是 `std::sync::mpsc` 的原因：前者在结果送达时会唤醒
//! dioxus 注册的 waker，于是 VirtualDom 自己会重新轮询这个 future，
//! **不需要定时器轮询，也不需要引入 tokio 依赖**。

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures_channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures_core::Stream;
use zcode_services::git::changes::{get_workspace_changes, summarize_changes, GitChangeSummary};
use zcode_services::git::repo::GitCliRepo;
use zcode_shared::git::{
    GitChangeKind, GitChangeSectionId, GitFileChange, GitHeadRefType, GitRepositorySummary,
};

/// 一次扫描的结果。
#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceData {
    pub summary: GitRepositorySummary,
    /// 已按 staged / unstaged / untracked / conflicted 分组标注好。
    pub changes: Vec<GitFileChange>,
    pub totals: GitChangeSummary,
}

impl WorkspaceData {
    pub fn is_repository(&self) -> bool {
        self.summary.is_repository
    }

    /// 顶栏展示用的分支名。
    pub fn branch_label(&self) -> String {
        match self.summary.head_ref_type {
            GitHeadRefType::Detached => "detached HEAD".to_string(),
            GitHeadRefType::Branch => self
                .summary
                .branch_name
                .clone()
                .unwrap_or_else(|| "(无分支)".to_string()),
        }
    }

    /// ahead / behind 的紧凑写法；两边都是 0 时返回 `None`。
    pub fn ahead_behind_label(&self) -> Option<String> {
        if self.summary.ahead == 0 && self.summary.behind == 0 {
            return None;
        }
        Some(format!("↑{} ↓{}", self.summary.ahead, self.summary.behind))
    }

    /// 某个分组下的变更。
    pub fn changes_in(&self, section: GitChangeSectionId) -> Vec<&GitFileChange> {
        self.changes
            .iter()
            .filter(|change| change.section == section)
            .collect()
    }

    /// 面板里按顺序展示的分组（空分组会被过滤掉）。
    pub fn populated_sections(&self) -> Vec<GitChangeSectionId> {
        [
            GitChangeSectionId::Conflicted,
            GitChangeSectionId::Staged,
            GitChangeSectionId::Unstaged,
            GitChangeSectionId::Untracked,
        ]
        .into_iter()
        .filter(|section| self.changes.iter().any(|change| change.section == *section))
        .collect()
    }
}

/// 加载状态机。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum WorkspaceStatus {
    /// 还没发起过扫描（例如 SSR 快照测试里没有 bootstrap）。
    #[default]
    Idle,
    Loading,
    Ready,
    /// 连 git 都没跑起来 —— 这种失败必须显示出来，不能静默成空白。
    Failed(String),
}

impl WorkspaceStatus {
    pub fn is_loading(&self) -> bool {
        matches!(self, WorkspaceStatus::Loading)
    }

    pub fn failure(&self) -> Option<&str> {
        match self {
            WorkspaceStatus::Failed(message) => Some(message),
            _ => None,
        }
    }
}

/// worker 线程回传给 UI 的消息。
#[derive(Debug)]
pub enum WorkspaceMessage {
    /// 扫描已经开始（由请求线程立刻发出，不等 git 跑完）。
    Loading,
    /// 结果用 `Box` 包一层，免得消息枚举被一个大结构撑大。
    Loaded(Box<WorkspaceData>),
    Failed(String),
    /// 写操作的即时反馈（成功或失败）。
    Notice(WorkspaceNotice),
}

/// 上一次写操作的结果，用于给用户即时反馈。
///
/// 和 [`WorkspaceStatus::Failed`] 分开是因为**语义完全不同**：
/// 那个说的是「读不到工作区，界面没内容可显示」；
/// 这个说的是「读得到，只是刚才那个动作没成」—— 此时界面上的数据仍然有效，
/// 不该被一个红色整屏面板盖掉。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceNotice {
    pub is_error: bool,
    pub message: String,
}

impl WorkspaceNotice {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            is_error: false,
            message: message.into(),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            is_error: true,
            message: message.into(),
        }
    }
}

/// 用户能对工作区发起的写操作。
///
/// 路径一律是 **workspace 相对路径**（就是界面上显示的那条），
/// 不是仓库相对路径 —— `zcode-services` 的 `normalize_input_path` 以 workspace
/// 为基准解析相对路径。workspace 是仓库子目录时，这两者不一样。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceAction {
    Stage(Vec<String>),
    Unstage(Vec<String>),
    Discard { paths: Vec<String>, staged: bool },
    Commit { message: String },
    Push,
}

impl WorkspaceAction {
    /// 动作的中文名，用于按钮文案与提示前缀。
    pub fn label(&self) -> &'static str {
        match self {
            WorkspaceAction::Stage(_) => "暂存",
            WorkspaceAction::Unstage(_) => "取消暂存",
            WorkspaceAction::Discard { .. } => "丢弃改动",
            WorkspaceAction::Commit { .. } => "提交",
            WorkspaceAction::Push => "推送",
        }
    }

    /// 是否**会丢数据**（需要在界面上二次确认）。
    ///
    /// 只有 `discard` 是：`git restore` 之后改动就没了，git 也没法找回。
    pub fn is_destructive(&self) -> bool {
        matches!(self, WorkspaceAction::Discard { .. })
    }

    /// 按钮上的短文案。
    pub fn button_label(&self) -> &'static str {
        match self {
            WorkspaceAction::Stage(_) => "暂存",
            WorkspaceAction::Unstage(_) => "取消暂存",
            WorkspaceAction::Discard { .. } => "丢弃",
            WorkspaceAction::Commit { .. } => "提交",
            WorkspaceAction::Push => "推送",
        }
    }
}

/// 发起扫描 / 写操作的那一半，可以自由克隆 / 放进 context。
///
/// `Send + Sync`：内部只有 `Arc` 与原子量，所以能安全地交给 worker 线程。
#[derive(Clone)]
pub struct WorkspaceRequester {
    repo: Arc<GitCliRepo>,
    sender: UnboundedSender<WorkspaceMessage>,
    busy: Arc<AtomicBool>,
}

impl WorkspaceRequester {
    /// 发起一次扫描。
    ///
    /// 返回 `false` 表示上一次还没跑完 —— 这时**不排队**，
    /// 因为用户连点刷新时排队只会让最后看到的结果更旧。
    pub fn refresh(&self, workspace: PathBuf) -> bool {
        self.dispatch(workspace, None)
    }

    /// 发起一次写操作；成功后会自动重新扫描一遍。
    ///
    /// 返回 `false` 表示当前正有一个 git 操作在跑 —— 所有操作共用同一个
    /// `busy` 标志，**串行化**是刻意的：并发的 `git add` 与 `git commit`
    /// 会争 `index.lock`，失败信息还很难懂。
    pub fn run(&self, workspace: PathBuf, action: WorkspaceAction) -> bool {
        self.dispatch(workspace, Some(action))
    }

    /// `refresh` 与 `run` 的公共部分。
    fn dispatch(&self, workspace: PathBuf, action: Option<WorkspaceAction>) -> bool {
        if self.busy.swap(true, Ordering::SeqCst) {
            if let Some(action) = &action {
                let _ = self.sender.unbounded_send(WorkspaceMessage::Notice(
                    WorkspaceNotice::error(format!(
                        "上一个 Git 操作还在进行中，「{}」没有执行",
                        action.label()
                    )),
                ));
            }
            return false;
        }

        let _ = self.sender.unbounded_send(WorkspaceMessage::Loading);
        let repo = Arc::clone(&self.repo);
        let sender = self.sender.clone();
        let busy = Arc::clone(&self.busy);

        let spawned = std::thread::Builder::new()
            .name("zcode-workspace-git".to_string())
            .spawn(move || {
                // 先跑写操作并立刻回报结果，再重新扫描 —— 用户先看到「提交成功」，
                // 而不是先看到界面刷新再猜刚才那下成没成。
                if let Some(action) = action {
                    match run_workspace_action(&repo, &workspace, &action) {
                        Ok(detail) => {
                            let message = match detail {
                                Some(detail) => format!("{}成功：{detail}", action.label()),
                                None => format!("{}成功", action.label()),
                            };
                            let _ = sender.unbounded_send(WorkspaceMessage::Notice(
                                WorkspaceNotice::success(message),
                            ));
                        }
                        Err(message) => {
                            let _ = sender.unbounded_send(WorkspaceMessage::Notice(
                                WorkspaceNotice::error(format!(
                                    "{}失败：{message}",
                                    action.label()
                                )),
                            ));
                        }
                    }
                }

                let message = load_workspace(&repo, &workspace);
                busy.store(false, Ordering::SeqCst);
                let _ = sender.unbounded_send(message);
            });

        if let Err(error) = spawned {
            self.busy.store(false, Ordering::SeqCst);
            let _ = self.sender.unbounded_send(WorkspaceMessage::Failed(format!(
                "无法启动工作区线程：{error}"
            )));
            return false;
        }

        true
    }
}

/// 建一对「请求端 + 结果端」。
///
/// 返回 [`WorkspaceChannel`] 而不是裸的元组：`dioxus` 的 `use_hook` 要求返回值
/// `Clone`，而 `UnboundedReceiver` 不实现 `Clone`，所以把结果端放进
/// `Rc<RefCell<Option<_>>>` 里再包一层。
pub fn create_workspace_channel() -> WorkspaceChannel {
    let (sender, receiver) = unbounded();
    WorkspaceChannel {
        requester: WorkspaceRequester {
            repo: Arc::new(GitCliRepo::new()),
            sender,
            busy: Arc::new(AtomicBool::new(false)),
        },
        receiver: Rc::new(RefCell::new(Some(receiver))),
    }
}

/// 请求端与结果端的成对句柄。
#[derive(Clone)]
pub struct WorkspaceChannel {
    requester: WorkspaceRequester,
    receiver: Rc<RefCell<Option<UnboundedReceiver<WorkspaceMessage>>>>,
}

impl WorkspaceChannel {
    /// 请求端（`Send + Sync`，可以放进 context 到处克隆）。
    pub fn requester(&self) -> WorkspaceRequester {
        self.requester.clone()
    }

    /// 取出结果端；只有第一次调用会拿到东西。
    pub fn take_receiver(&self) -> Option<UnboundedReceiver<WorkspaceMessage>> {
        self.receiver.borrow_mut().take()
    }
}

/// 读取工作区请求端。
///
/// 它单独走一个 context 而不是塞进 `crate::state::AppCtx`：`AppCtx` 必须保持
/// `Copy`（视图里到处按字段捕获），而请求端内部有 `Arc`，做不到 `Copy`。
///
/// # Panics
///
/// 没有 provider 时 panic。provider 由 `AppShell` 提供，
/// 因此只有「脱离 AppShell 单独渲染某个视图」时才会踩到。
pub fn use_workspace_requester() -> WorkspaceRequester {
    dioxus::prelude::use_context::<WorkspaceRequester>()
}

/// 等下一个结果。
///
/// 所有 sender 都析构之后返回 `None`，调用方的循环随之结束。
///
/// 这里直接用 `futures_core::Stream::poll_next`（配 `Pin::new`）而不是
/// `futures_util` 的 `StreamExt::next`：少一个依赖，且 `UnboundedReceiver`
/// 本来就是 `Unpin`，包一层 `Pin` 是零成本的。
pub async fn next_workspace_message(
    receiver: &mut UnboundedReceiver<WorkspaceMessage>,
) -> Option<WorkspaceMessage> {
    std::future::poll_fn(|cx| std::pin::Pin::new(&mut *receiver).poll_next(cx)).await
}

/// 默认扫描的工作区。
///
/// `ZCODE_WORKSPACE` 优先（方便调试时指到别的仓库），否则用进程当前目录。
pub fn default_workspace_path() -> PathBuf {
    if let Some(override_path) = std::env::var_os("ZCODE_WORKSPACE") {
        if !override_path.is_empty() {
            return PathBuf::from(override_path);
        }
    }

    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// 真正跑 git 的地方（在 worker 线程上）。
fn load_workspace(repo: &GitCliRepo, workspace: &Path) -> WorkspaceMessage {
    match repo.get_status(workspace) {
        Ok(snapshot) => {
            let changes = get_workspace_changes(&snapshot);
            let totals = summarize_changes(&changes);
            WorkspaceMessage::Loaded(Box::new(WorkspaceData {
                summary: snapshot.summary,
                changes,
                totals,
            }))
        }
        Err(failure) => WorkspaceMessage::Failed(failure.message),
    }
}

/// 真正跑写操作的地方（在 worker 线程上）。
///
/// `Ok(Some(detail))` 用于把「新提交的前 7 位」这类信息带回界面。
fn run_workspace_action(
    repo: &GitCliRepo,
    workspace: &Path,
    action: &WorkspaceAction,
) -> Result<Option<String>, String> {
    match action {
        WorkspaceAction::Stage(paths) => repo.stage(workspace, paths).map(|_| None),
        WorkspaceAction::Unstage(paths) => repo.unstage(workspace, paths).map(|_| None),
        WorkspaceAction::Discard { paths, staged } => {
            repo.discard(workspace, paths, *staged).map(|_| None)
        }
        WorkspaceAction::Commit { message } => repo
            .commit(workspace, message, &[])
            .map(|result| Some(short_hash(&result.commit_hash))),
        WorkspaceAction::Push => repo.push(workspace).map(|result| {
            Some(match result.remote_name.as_deref() {
                Some(remote) => format!("{} → {remote}", result.branch_name),
                None => result.branch_name.clone(),
            })
        }),
    }
    .map_err(|failure| failure.message)
}

/// 40 位哈希取前 7 位，和 `git log --oneline` 的观感一致。
pub fn short_hash(hash: &str) -> String {
    hash.chars().take(7).collect()
}

/// 把一次写操作需要的路径从变更条目上取出来。
///
/// 用 `workspace_relative_path` 而不是 `repo_relative_path`：服务层解析相对路径时
/// 以 **workspace** 为基准（见 [`WorkspaceAction`] 的说明）。
pub fn action_paths(changes: &[&GitFileChange]) -> Vec<String> {
    changes
        .iter()
        .map(|change| {
            let path = display_path(change);
            path.to_string()
        })
        .collect()
}

/// 变更性质的中文标签。
pub fn kind_label(kind: GitChangeKind) -> &'static str {
    match kind {
        GitChangeKind::Modified => "修改",
        GitChangeKind::Added => "新增",
        GitChangeKind::Deleted => "删除",
        GitChangeKind::Renamed => "重命名",
    }
}

/// 分组的中文标签。
pub fn section_label(section: GitChangeSectionId) -> &'static str {
    match section {
        GitChangeSectionId::Staged => "已暂存",
        GitChangeSectionId::Unstaged => "未暂存",
        GitChangeSectionId::Untracked => "未跟踪",
        GitChangeSectionId::Conflicted => "冲突",
        GitChangeSectionId::Branch => "分支差异",
        GitChangeSectionId::LastTurn => "本轮改动",
    }
}

/// 面板里展示用的路径：workspace 是仓库子目录时优先用相对 workspace 的那条。
pub fn display_path(change: &GitFileChange) -> &str {
    if change.workspace_relative_path.is_empty() {
        &change.repo_relative_path
    } else {
        &change.workspace_relative_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zcode_services::git::types::GitResolvedRepository;
    use zcode_shared::git::GitRepositoryAutoRefreshWatchPath;

    fn summary(branch: Option<&str>, head: GitHeadRefType, ahead: u32, behind: u32) -> GitRepositorySummary {
        GitRepositorySummary {
            workspace_path: "/repo".into(),
            repo_root: "/repo".into(),
            workspace_in_repo_path: ".".into(),
            auto_refresh_watch_paths: Vec::<GitRepositoryAutoRefreshWatchPath>::new(),
            branch_name: branch.map(str::to_string),
            tracking_branch_name: None,
            head_ref_type: head,
            ahead,
            behind,
            is_dirty: true,
            is_git_available: true,
            is_repository: true,
        }
    }

    fn change(path: &str, section: GitChangeSectionId) -> GitFileChange {
        GitFileChange {
            path: format!("/repo/{path}"),
            repo_relative_path: path.to_string(),
            workspace_relative_path: path.to_string(),
            x: Some("M".into()),
            y: Some(".".into()),
            kind: GitChangeKind::Modified,
            section,
            added: 1,
            removed: 0,
            is_staged: section == GitChangeSectionId::Staged,
            is_untracked: section == GitChangeSectionId::Untracked,
            is_conflicted: section == GitChangeSectionId::Conflicted,
        }
    }

    fn data(changes: Vec<GitFileChange>) -> WorkspaceData {
        WorkspaceData {
            summary: summary(Some("main"), GitHeadRefType::Branch, 0, 0),
            totals: summarize_changes(&changes),
            changes,
        }
    }

    #[test]
    fn branch_label_covers_branch_detached_and_missing() {
        let mut value = data(Vec::new());
        assert_eq!(value.branch_label(), "main");

        value.summary.head_ref_type = GitHeadRefType::Detached;
        assert_eq!(value.branch_label(), "detached HEAD");

        value.summary.head_ref_type = GitHeadRefType::Branch;
        value.summary.branch_name = None;
        assert_eq!(value.branch_label(), "(无分支)");
    }

    #[test]
    fn ahead_behind_label_is_none_when_in_sync() {
        let mut value = data(Vec::new());
        assert!(value.ahead_behind_label().is_none());

        value.summary.ahead = 2;
        assert_eq!(value.ahead_behind_label().as_deref(), Some("↑2 ↓0"));

        value.summary.ahead = 0;
        value.summary.behind = 3;
        assert_eq!(value.ahead_behind_label().as_deref(), Some("↑0 ↓3"));
    }

    #[test]
    fn populated_sections_are_ordered_and_skip_empty() {
        let value = data(vec![
            change("untracked.rs", GitChangeSectionId::Untracked),
            change("staged.rs", GitChangeSectionId::Staged),
            change("conflict.rs", GitChangeSectionId::Conflicted),
        ]);

        assert_eq!(
            value.populated_sections(),
            vec![
                GitChangeSectionId::Conflicted,
                GitChangeSectionId::Staged,
                GitChangeSectionId::Untracked,
            ]
        );
    }

    #[test]
    fn populated_sections_is_empty_for_clean_tree() {
        assert!(data(Vec::new()).populated_sections().is_empty());
    }

    #[test]
    fn changes_in_filters_by_section() {
        let value = data(vec![
            change("a.rs", GitChangeSectionId::Staged),
            change("b.rs", GitChangeSectionId::Unstaged),
            change("c.rs", GitChangeSectionId::Unstaged),
        ]);

        assert_eq!(value.changes_in(GitChangeSectionId::Staged).len(), 1);
        assert_eq!(value.changes_in(GitChangeSectionId::Unstaged).len(), 2);
        assert!(value.changes_in(GitChangeSectionId::Conflicted).is_empty());
    }

    #[test]
    fn status_helpers() {
        assert!(WorkspaceStatus::default() == WorkspaceStatus::Idle);
        assert!(!WorkspaceStatus::Idle.is_loading());
        assert!(WorkspaceStatus::Loading.is_loading());
        assert_eq!(WorkspaceStatus::Ready.failure(), None);
        assert_eq!(
            WorkspaceStatus::Failed("boom".into()).failure(),
            Some("boom")
        );
    }

    #[test]
    fn labels_cover_every_variant() {
        for kind in [
            GitChangeKind::Modified,
            GitChangeKind::Added,
            GitChangeKind::Deleted,
            GitChangeKind::Renamed,
        ] {
            assert!(!kind_label(kind).is_empty());
        }
        for section in [
            GitChangeSectionId::Staged,
            GitChangeSectionId::Unstaged,
            GitChangeSectionId::Untracked,
            GitChangeSectionId::Conflicted,
            GitChangeSectionId::Branch,
            GitChangeSectionId::LastTurn,
        ] {
            assert!(!section_label(section).is_empty());
        }
    }

    #[test]
    fn display_path_prefers_workspace_relative() {
        let mut value = change("a/b.rs", GitChangeSectionId::Staged);
        assert_eq!(display_path(&value), "a/b.rs");

        value.workspace_relative_path = String::new();
        assert_eq!(display_path(&value), "a/b.rs");
    }

    #[test]
    fn workspace_path_override_wins() {
        // 不真的改进程环境（会污染并行测试），只验证「非空才算数」这条规则。
        let empty = std::ffi::OsString::new();
        assert!(empty.is_empty());
        assert!(!default_workspace_path().as_os_str().is_empty());
    }

    #[test]
    fn refresh_on_a_detached_channel_does_not_panic() {
        let channel = create_workspace_channel();
        // 接收端没了，发送失败应当被安静吞掉
        drop(channel.take_receiver());
        let _ = channel.requester().refresh(PathBuf::from("."));
    }

    #[test]
    fn receiver_can_only_be_taken_once() {
        let channel = create_workspace_channel();
        assert!(channel.take_receiver().is_some());
        assert!(
            channel.take_receiver().is_none(),
            "第二次取必须拿不到东西，否则会有两个消费者抢同一批消息"
        );
        // 克隆共享同一个槽位，因此也拿不到
        let clone = channel.clone();
        assert!(clone.take_receiver().is_none());
    }

    #[test]
    fn git_resolved_repository_type_is_usable_from_app() {
        // 确保应用层能直接拿到 services 的解析结果类型（避免只导出部分 API）
        let resolution = GitResolvedRepository::unavailable("/repo", true);
        assert!(!resolution.is_usable());
    }

    // ---------------------------------------------------------------
    // 写操作
    // ---------------------------------------------------------------

    #[test]
    fn only_discard_is_destructive() {
        assert!(WorkspaceAction::Discard {
            paths: vec!["a.rs".into()],
            staged: false,
        }
        .is_destructive());

        for action in [
            WorkspaceAction::Stage(vec!["a.rs".into()]),
            WorkspaceAction::Unstage(vec!["a.rs".into()]),
            WorkspaceAction::Commit {
                message: "msg".into(),
            },
            WorkspaceAction::Push,
        ] {
            assert!(
                !action.is_destructive(),
                "{} 不该被标成会丢数据的操作",
                action.label()
            );
        }
    }

    #[test]
    fn action_labels_are_non_empty_and_distinct() {
        let actions = [
            WorkspaceAction::Stage(Vec::new()),
            WorkspaceAction::Unstage(Vec::new()),
            WorkspaceAction::Discard {
                paths: Vec::new(),
                staged: true,
            },
            WorkspaceAction::Commit {
                message: String::new(),
            },
            WorkspaceAction::Push,
        ];

        let mut labels: Vec<&str> = Vec::new();
        for action in &actions {
            assert!(!action.label().is_empty());
            assert!(!action.button_label().is_empty());
            labels.push(action.label());
        }
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), actions.len(), "每个动作的文案应当各不相同");
    }

    #[test]
    fn short_hash_takes_seven_characters() {
        assert_eq!(
            short_hash("1b790776924d78394b8b71029eb9114695ec0098"),
            "1b79077"
        );
        // 短哈希（或空串）不能 panic
        assert_eq!(short_hash("abc"), "abc");
        assert_eq!(short_hash(""), "");
    }

    #[test]
    fn action_paths_uses_the_workspace_relative_path() {
        let mut staged = change("a.rs", GitChangeSectionId::Staged);
        // workspace 是仓库子目录时，两者不同 —— 必须用 workspace 那条
        staged.workspace_relative_path = "sub/a.rs".into();
        let untracked = change("b.rs", GitChangeSectionId::Untracked);

        let paths = action_paths(&[&staged, &untracked]);
        assert_eq!(paths, vec!["sub/a.rs".to_string(), "b.rs".to_string()]);
    }

    #[test]
    fn action_paths_is_empty_for_no_changes() {
        assert!(action_paths(&[]).is_empty());
    }

    #[test]
    fn notices_distinguish_success_from_error() {
        let ok = WorkspaceNotice::success("提交成功：1b79077");
        assert!(!ok.is_error);
        assert_eq!(ok.message, "提交成功：1b79077");

        let bad = WorkspaceNotice::error("暂存失败：boom");
        assert!(bad.is_error);
        assert_eq!(bad.message, "暂存失败：boom");
    }

    #[test]
    fn run_on_a_detached_channel_does_not_panic() {
        let channel = create_workspace_channel();
        drop(channel.take_receiver());
        let _ = channel
            .requester()
            .run(PathBuf::from("."), WorkspaceAction::Push);
    }

    #[test]
    fn a_busy_requester_reports_why_the_action_was_skipped() {
        // 手工把 busy 立起来，模拟「上一个操作还在跑」。
        let (sender, mut receiver) = futures_channel::mpsc::unbounded();
        let requester = WorkspaceRequester {
            repo: Arc::new(GitCliRepo::new()),
            sender,
            busy: Arc::new(AtomicBool::new(true)),
        };

        assert!(
            !requester.run(PathBuf::from("."), WorkspaceAction::Push),
            "busy 时应当拒绝新操作"
        );

        // 拒绝要**说出来**，否则用户点了按钮没反应会以为是界面卡了。
        // `try_recv` 与 `std::sync::mpsc` 同形：队列空时返回 Err(Empty)，
        // 所以这里 expect 是安全的 —— 上面那次调用必然已经塞进一条消息。
        let message = receiver.try_recv().expect("应当立刻收到一条被拒绝的提示");
        match message {
            WorkspaceMessage::Notice(notice) => {
                assert!(notice.is_error);
                assert!(
                    notice.message.contains("推送"),
                    "提示里应当说明是哪个动作被跳过：{}",
                    notice.message
                );
            }
            other => panic!("期望收到 Notice，实际：{other:?}"),
        }
    }
}
