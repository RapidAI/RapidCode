//! # zcode-app
//!
//! ZCode 的 **Rust + Dioxus 桌面外壳**：把 [`zcode_ui`] 里移植过来的 shadcn 原语
//! 组装成一个真的能跑起来的应用，用来验证移植结果不是「只通过测试」。
//!
//! ## 为什么是桌面而不是浏览器
//!
//! `dioxus/web` 需要 `wasm32-unknown-unknown` target + `wasm-bindgen-cli`，
//! 当前环境两者都没有。`dioxus/desktop`（wry + tao）走的是原生 webview，
//! 只需要宿主 target，因此在 `x86_64-pc-windows-msvc` 上直接可用。
//! 两者共用同一份组件代码，切换只是换 feature：
//!
//! ```bash
//! cargo run -p zcode-app                     # 原生窗口（默认）
//! cargo build -p zcode-app --no-default-features --features web   # 浏览器（需 wasm target）
//! ```
//!
//! ## 样式从哪来
//!
//! 组件里的 Tailwind 类名需要一份真的样式表。这里**不用 CDN**（桌面端无网络），
//! 而是在构建期用 Tailwind v4 CLI 扫描 `crates/**/*.rs` 生成
//! `assets/app.css`（114 KB），再用 `include_str!` 内嵌进二进制。
//! 设计令牌是从上游 `packages/ui/src/styles.css` 的 `@theme` 块抽取后
//! vendored 到 `.tailwind-build/tokens.css`（参照源在 `rust` 分支），
//! 因此配色、圆角、字号与 TS 版**同源**。重新生成见
//! `.tailwind-build/build.sh`。
//!
//! ## 为什么拆出 lib.rs
//!
//! 组件树放在 lib 里，`tests/smoke.rs` 才能在不打开窗口的前提下把整棵树
//! 构建一遍（记录型渲染器 + SSR 快照）。窗口能不能开起来依赖环境，
//! 「树能不能挂载、锚点对不对」不依赖环境，后者更适合放进 CI。
//!
//! ## 目录
//!
//! - [`state`]：单一 store（`Signal<AppState>`）+ 演示数据
//! - [`workspace`]：**真实**工作区数据（后台线程跑 git，UI 线程只读结果）
//! - [`views`]：侧栏 / 顶栏 / 会话流 / 变更页 / 输入区 / 详情面板 / 设置 / 组件画廊
//!
//! ## 哪些是真的，哪些是演示
//!
//! 这一层必须说清楚，否则很容易把「原语都长出来了」误读成「功能都搬过来了」：
//!
//! | 区域 | 数据来源 |
//! | --- | --- |
//! | 「变更」页、详情面板的「变更文件」、顶栏分支徽标、侧栏底部仓库信息 | **真实** —— `zcode-services` 跑 `git status` / `git diff --numstat` |
//! | 会话列表、消息流、工具调用卡片、上下文用量 | 演示数据（`state::demo_state`）—— 依赖会话存储与 LLM provider，尚未移植 |
//! | 「任务」页 | 明确标注为未移植的占位页 |

pub mod state;
pub mod views;
pub mod workspace;

use dioxus::prelude::*;
use state::{AppCtx, ApprovalMode};
use workspace::{
    DiffPreviewState, WorkspaceData, WorkspaceMessage, WorkspaceNotice, WorkspaceRequester,
    WorkspaceStatus,
};
use zcode_ui::*;

/// 构建期生成的样式表（见 crate 文档）。
pub const APP_CSS: &str = include_str!("../assets/app.css");

/// 窗口标题。
pub const TITLE: &str = "ZCode — Rust + Dioxus 移植";

/// 启动时告诉 [`AppShell`] 该扫描哪个工作区。
///
/// 只有真实入口 [`App`] 会提供它。测试里直接渲染 `AppShell` 时没有这个 context，
/// 于是**不会**去碰真实文件系统与 git 进程 —— 无头快照测试因此保持确定性。
#[derive(Clone, Debug)]
pub struct WorkspaceBootstrap {
    pub path: std::path::PathBuf,
}

/// 用一份**给定**的工作区数据启动 [`AppShell`]。
///
/// 与 [`WorkspaceBootstrap`] 的区别：那个是「自己去扫」，这个是「数据已经在这儿了」。
/// 两条用途：
///
/// 1. `examples/app_preview.rs` 在进程里同步扫一次真实仓库，再把结果塞进来 ——
///    这样静态快照里显示的是**真实**的文件名与行数，而不是编的。
/// 2. 快照测试注入固定数据，断言各种状态（干净 / 非仓库 / 失败 / 加载中）的渲染。
///
/// 提供它时不会触发后台扫描。
#[derive(Clone, Debug)]
pub struct WorkspaceSeed {
    pub data: Option<WorkspaceData>,
    pub status: WorkspaceStatus,
    /// 初始视图 Tab；`None` 用默认值（对话页）。
    pub tab: Option<String>,
    /// 初始就展开的 diff 面板；`None` 表示全部收起。
    ///
    /// 单独开一个字段是为了让静态快照也能把 diff 预览拍进去 ——
    /// 否则那一块在快照里永远是关着的，等于没验证。
    pub diff: Option<DiffPreviewState>,
}

impl WorkspaceSeed {
    /// 用一份扫描结果构造种子。
    pub fn ready(data: WorkspaceData) -> Self {
        Self {
            data: Some(data),
            status: WorkspaceStatus::Ready,
            tab: None,
            diff: None,
        }
    }

    /// 指定初始 Tab。
    pub fn with_tab(mut self, tab: impl Into<String>) -> Self {
        self.tab = Some(tab.into());
        self
    }

    /// 指定初始展开的 diff 面板。
    pub fn with_diff(mut self, diff: DiffPreviewState) -> Self {
        self.diff = Some(diff);
        self
    }
}

/// 应用根组件：往 `<head>` 注入标题与样式表，然后挂上界面。
///
/// 注意这里**没有把 `<head>` 注入和界面写在同一个组件里** ——
/// `document::Style` / `document::Title` 只在真实渲染器（desktop/web）下有实现，
/// 无渲染器的 SSR 快照（`tests/smoke.rs`）会跳过它们，因此界面被拆到
/// [`AppShell`]，两边各自可测。
#[component]
pub fn App() -> Element {
    // 在根上把「从哪个目录开始扫描」定下来，再交给 AppShell。
    // 放在根上而不是 AppShell 里，是为了让 AppShell 在测试中保持零副作用。
    use_context_provider(|| WorkspaceBootstrap {
        path: workspace::default_workspace_path(),
    });

    rsx! {
        document::Title { {TITLE} }
        document::Style { {APP_CSS} }
        AppShell {}
    }
}

/// 界面主体（不含 `<head>` 注入，便于无渲染器测试）。
#[component]
pub fn AppShell() -> Element {
    // 外部可以塞一份「已经拿到的」工作区数据进来（预览与快照测试用）。
    // 读它用 `try_consume_context`：真实运行时没有这个 context，走后台扫描。
    let seed = try_consume_context::<WorkspaceSeed>();
    let seeded_tab = seed.as_ref().and_then(|seed| seed.tab.clone());
    let seeded_data = seed.as_ref().and_then(|seed| seed.data.clone());
    let seeded_status = seed.as_ref().map(|seed| seed.status.clone());
    let seeded_diff = seed.as_ref().and_then(|seed| seed.diff.clone());

    // 全局 store
    let state = use_signal(state::demo_state);
    // UI 瞬态：各自的 Signal，避免开合浮层时让整棵树重算
    let settings_open = use_signal(|| false);
    let pending_delete = use_signal(|| None::<&'static str>);
    let delete_open = use_signal(|| false);
    let settings_tab = use_signal(|| "general".to_string());
    let detail_open = use_signal(|| true);
    let gallery_open = use_signal(|| false);
    let workspace_tab = use_signal(move || {
        seeded_tab.unwrap_or_else(|| "conversation".to_string())
    });
    let plan_open = use_signal(|| true);
    let approval_value = use_signal(|| ApprovalMode::Ask.as_str().to_string());
    // 真实工作区数据
    let workspace_data = use_signal(move || seeded_data);
    let workspace_status = use_signal(move || seeded_status.unwrap_or_default());
    let workspace_notice = use_signal(|| None::<WorkspaceNotice>);
    let diff_preview = use_signal(move || seeded_diff);

    // 请求端 / 结果端只建一次；请求端要通过 context 给顶栏与变更页用。
    let workspace_channel = use_hook(workspace::create_workspace_channel);
    let requester_for_context = workspace_channel.requester();
    use_context_provider(move || requester_for_context);

    // 结果回传循环。
    //
    // 阻塞的 git 调用在 worker 线程里（见 [`workspace`]），这里只 await 结果，
    // 因此渲染线程不会被 `git status` 卡住。`futures-channel` 会在消息到达时
    // 唤醒 dioxus 的 waker，所以不需要定时轮询。
    let channel_for_poller = workspace_channel.clone();
    let _workspace_poller = use_hook(move || {
        let mut receiver = channel_for_poller.take_receiver();
        let mut workspace_data = workspace_data;
        let mut workspace_status = workspace_status;
        let mut workspace_notice = workspace_notice;
        let mut diff_preview = diff_preview;
        spawn(async move {
            let Some(receiver) = receiver.as_mut() else {
                // 理论上不可达（`use_hook` 保证初始化只跑一次）。
                // 这里挂起而不是 panic：内部不变量出问题时不该把整个界面带走。
                std::future::pending::<()>().await;
                return;
            };

            while let Some(message) = workspace::next_workspace_message(receiver).await {
                match message {
                    WorkspaceMessage::Loading => workspace_status.set(WorkspaceStatus::Loading),
                    WorkspaceMessage::Loaded(data) => {
                        workspace_data.set(Some(*data));
                        workspace_status.set(WorkspaceStatus::Ready);
                    }
                    WorkspaceMessage::Failed(message) => {
                        workspace_status.set(WorkspaceStatus::Failed(message));
                    }
                    WorkspaceMessage::Notice(notice) => workspace_notice.set(Some(notice)),
                    WorkspaceMessage::Diff(preview) => {
                        // 只采纳**当前那次请求**的结果：用户快速点了 A 又点 B 时，
                        // A 的 diff 可能后到，直接丢弃，否则会把 B 的内容盖成 A 的。
                        // 用户中途把面板收起来（状态变 None）时也一并丢弃。
                        let is_current = diff_preview
                            .read()
                            .as_ref()
                            .map(DiffPreviewState::id)
                            == Some(preview.id);
                        if !is_current {
                            continue;
                        }
                        diff_preview.set(Some(match preview.outcome {
                            Ok(diff) => DiffPreviewState::Ready {
                                id: preview.id,
                                path: preview.path,
                                diff,
                            },
                            Err(message) => DiffPreviewState::Failed {
                                id: preview.id,
                                path: preview.path,
                                message,
                            },
                        }));
                    }
                }
            }
        })
    });

    // 首帧发起一次扫描；只有真实入口提供了 bootstrap、且外部没有塞数据时才做。
    let bootstrap_path = if seed.is_none() {
        try_consume_context::<WorkspaceBootstrap>().map(|bootstrap| bootstrap.path)
    } else {
        None
    };
    let requester_for_initial_load: WorkspaceRequester = workspace_channel.requester();
    use_hook(move || {
        if let Some(path) = bootstrap_path {
            requester_for_initial_load.refresh(path);
        }
    });

    use_context_provider(|| AppCtx {
        state,
        settings_open,
        pending_delete,
        delete_open,
        settings_tab,
        detail_open,
        gallery_open,
        workspace_tab,
        plan_open,
        approval_value,
        workspace: workspace_data,
        workspace_status,
        workspace_notice,
        diff_preview,
    });

    let dark = state.read().dark;
    let root_class = if dark { "dark h-full w-full" } else { "h-full w-full" };

    rsx! {
        div {
            class: "{root_class}",
            div { class: "flex h-full w-full overflow-hidden bg-background text-foreground",
                views::Sidebar {}
                div { class: "flex min-w-0 flex-1 flex-col",
                    views::TopBar {}
                    div { class: "flex min-h-0 flex-1",
                        // 顶栏的 Tab 必须真的切换内容，见 [`views::WorkspacePane`]。
                        views::WorkspacePane {}
                        if (detail_open)() {
                            views::DetailPanel {}
                        }
                    }
                    views::Composer {}
                }
            }
            views::SettingsDialog {}
            views::DeleteConfirmDialog {}
            views::GalleryDialog {}
        }
    }
}

/// 面板通用外壳：`overflow-hidden` + 纵向布局，供各面板复用。
pub fn pane_class(extra: &str) -> String {
    cn(&["flex min-h-0 flex-col border-border bg-background", extra])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stylesheet_is_embedded_and_non_trivial() {
        assert!(
            APP_CSS.len() > 50_000,
            "样式表看起来没生成：{} 字节",
            APP_CSS.len()
        );
        // 关键令牌与工具类必须真的在表里，否则窗口会是一片白
        for needle in [
            "--color-background:",
            "--color-menu:",
            "--color-tooltip:",
            "text-ui-base",
            ".dark",
            "data-highlighted",
        ] {
            assert!(APP_CSS.contains(needle), "样式表缺少 `{needle}`");
        }
    }

    #[test]
    fn pane_class_merges() {
        assert!(pane_class("w-64").contains("w-64"));
        assert!(pane_class("").contains("flex"));
    }
}
