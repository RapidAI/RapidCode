//! 应用各个区域。
//!
//! 每个模块负责一块界面，全部由 [`zcode_ui`] 里移植过来的原语拼成 ——
//! 这里刻意**不写自定义 CSS**，只用类名组合，这样任何视觉问题都能定位到
//! 「是原语移植错了」还是「是我组合错了」。

use dioxus::prelude::*;

pub mod changes;
pub mod composer;
pub mod conversation;
pub mod detail;
pub mod gallery;
pub mod settings;
pub mod sidebar;
pub mod topbar;

pub use changes::{ChangeRow, ChangesView, NotPortedPane, WorkspaceBar};
pub use composer::Composer;
pub use conversation::ConversationView;
pub use detail::DetailPanel;
pub use gallery::GalleryDialog;
pub use settings::{DeleteConfirmDialog, SettingsDialog};
pub use sidebar::Sidebar;
pub use topbar::TopBar;

/// 主区域的当前页。
///
/// 顶栏的 Tab 点下去必须**真的换内容**：之前「变更」「任务」两个 Tab 点了没反应，
/// 用户看到的仍然是一个不动的壳子 —— 那正是我们要修掉的现象。
///
/// 单独抽成组件而不是写在 `AppShell` 里，是为了让快照测试能直接注入
/// `workspace_tab` 并断言切换结果（`AppShell` 自己持有那个 signal，外部改不了）。
#[component]
pub fn WorkspacePane() -> Element {
    let tab = crate::state::use_app().workspace_tab;

    match tab().as_str() {
        "changes" => rsx! { ChangesView {} },
        "tasks" => rsx! {
            NotPortedPane {
                title: "任务页尚未移植",
                detail: "上游的任务面板依赖会话存储与 agent 调度（packages/desktop），这一层还没搬过来。",
            }
        },
        _ => rsx! { ConversationView {} },
    }
}
