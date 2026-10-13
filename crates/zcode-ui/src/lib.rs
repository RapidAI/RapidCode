//! # zcode-ui
//!
//! ZCode UI 原语组件库的 **Rust + Dioxus** 移植，来源为
//! `packages/ui/src/components/ui/`（shadcn/ui 风格，49 个 TSX 原语）。
//!
//! ## 为什么是 Dioxus
//!
//! `packages/ui` 的组件是 React 组件（663,605 行 TS/TSX）。Rust 生态中与
//! React 语义最贴近的等价物是 [Dioxus](https://dioxuslabs.com/)：
//!
//! | React | Dioxus |
//! | --- | --- |
//! | JSX | RSX（`rsx! { ... }`） |
//! | 函数组件 | `#[component] fn` |
//! | `useState` | `use_signal` |
//! | `useContext` / `Provider` | `use_context` / `use_context_provider` |
//! | `props.children` | `children: Element` |
//! | `className` | `class` |
//!
//! ## 编译不依赖 wasm / 窗口
//!
//! `Cargo.toml` 里显式关掉了 Dioxus 的默认 feature，只保留
//! `macro` / `hooks` / `html` / `signals`。这意味着：
//!
//! - **没有渲染器**（web / desktop / native 全关），因此 `cargo build` /
//!   `cargo test` 在任意环境（含 CI、无图形界面的容器）都能跑通；
//! - 组件返回 `Element`，在真正的应用里由调用方选择渲染器（`dioxus-web`
//!   或 `dioxus-desktop`）挂载；
//! - 类名拼装逻辑被抽成纯函数，单元测试不经过渲染器，秒级完成。
//!
//! ## 移植范围与取舍
//!
//! 已移植 **31 / 47** 个原语：
//!
//! - 基础：Button / Badge / Card / Alert / Input / Textarea / Label / Separator /
//!   Progress / Kbd / Spinner / Avatar / Switch / Checkbox / Tabs
//! - 折叠：Collapsible / Accordion
//! - 组合：RadioGroup / ButtonGroup / InputGroup / ScrollArea / Resizable
//! - 浮层：Tooltip / HoverCard / Popover / Dialog / AlertDialog / DropdownMenu
//! - 展示：ScrollFadeViewport / ZCodeAboutLogo / GlmMonochromeIcon
//!
//! 尚未移植的 16 个集中在两类：需要浏览器运行时能力的重型渲染器
//! （carousel / chart / code-viewer / diff-viewer / pdf-viewer /
//! pptx-preview-viewer / floating-particles / smooth-metric-value /
//! flip-metric-value），以及它们派生出的组合件
//! （command / context-menu / select / toast / lightweight-diff-preview /
//! highlighted-lightweight-diff-preview / pptx-selection-action-bar）。
//!
//! 细节见仓库根目录 `rust/PORTING-UI.md`。

/// 类名合并（`clsx` + `tailwind-merge` 的务实子集）。
pub mod cn;
pub mod code_preview_preferences;
pub mod diffs_highlighter_engine;
/// shadcn 风格原语组件。
pub mod components;
/// lucide 图标的内联 SVG 移植（零额外依赖）。
pub mod icons;
pub mod keyboard_shortcuts;
pub mod patch_diff_preview;
pub mod pdf_js_assets;
pub mod pdf_zoom_overlay;
pub mod presentation_element_model;
pub mod shiki_highlighter;

pub use cn::{cn, cn_opt};
pub use components::accordion::*;
pub use components::alert::*;
pub use components::alert_dialog::*;
pub use components::avatar::*;
pub use components::badge::*;
pub use components::brand::*;
pub use components::button::*;
pub use components::button_group::*;
pub use components::card::*;
pub use components::carousel::*;
pub use components::chart::*;
pub use components::checkbox::*;
pub use components::code_viewer::*;
pub use components::collapsible::*;
pub use components::command::*;
pub use components::context_menu::*;
pub use components::dialog::*;
pub use components::diff_viewer::*;
pub use components::flip_metric_value::*;
pub use components::floating_particles::*;
pub use components::dropdown_menu::*;
pub use components::glm_icon::*;
pub use components::highlighted_lightweight_diff_preview::*;
pub use components::hover_card::*;
pub use components::input::*;
pub use components::input_group::*;
pub use components::kbd::*;
pub use components::label::*;
pub use components::lightweight_diff_preview::*;
pub use components::overlay::{Align, Side};
pub use components::pdf_viewer::*;
pub use components::popover::*;
pub use components::pptx_preview_viewer::*;
pub use components::pptx_selection_action_bar::*;
pub use components::progress::*;
pub use components::radio_group::*;
pub use components::resizable::*;
pub use components::scroll_area::*;
pub use components::scroll_fade_viewport::*;
pub use components::select::*;
pub use components::separator::*;
pub use components::smooth_metric_value::*;
pub use components::spinner::*;
pub use components::switch::*;
pub use components::tabs::*;
pub use components::textarea::*;
pub use components::toast::*;
pub use components::tooltip::*;
pub use icons::*;
