//! 应用各个区域。
//!
//! 每个模块负责一块界面，全部由 [`zcode_ui`] 里移植过来的原语拼成 ——
//! 这里刻意**不写自定义 CSS**，只用类名组合，这样任何视觉问题都能定位到
//! 「是原语移植错了」还是「是我组合错了」。

pub mod composer;
pub mod conversation;
pub mod detail;
pub mod gallery;
pub mod settings;
pub mod sidebar;
pub mod topbar;

pub use composer::Composer;
pub use conversation::ConversationView;
pub use detail::DetailPanel;
pub use gallery::GalleryDialog;
pub use settings::{DeleteConfirmDialog, SettingsDialog};
pub use sidebar::Sidebar;
pub use topbar::TopBar;
