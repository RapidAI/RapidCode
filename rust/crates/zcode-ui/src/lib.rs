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
//! 已移植：Button / Badge / Card / Alert / Input / Label / Separator / Progress /
//! Kbd / Spinner / Textarea / Avatar / Switch / Checkbox / Tabs。
//!
//! 未移植（需要浏览器/Electron 运行时能力，或依赖 React 生态的浮层库）：
//! Dialog / Popover / Tooltip / DropdownMenu / ContextMenu / Select / Combobox /
//! Command / Toast / Carousel / Resizable / ScrollArea 等。这些组件的可移植部分
//! 是「类名表 + 状态机」，需要配合 Dioxus 的 `Portal` 与定位计算单独设计。
//!
//! 细节见仓库根目录 `rust/PORTING-UI.md`。

/// 类名合并（`clsx` + `tailwind-merge` 的务实子集）。
pub mod cn;
/// shadcn 风格原语组件。
pub mod components;
/// lucide 图标的内联 SVG 移植（零额外依赖）。
pub mod icons;

pub use cn::{cn, cn_opt};
pub use components::accordion::*;
pub use components::alert::*;
pub use components::alert_dialog::*;
pub use components::avatar::*;
pub use components::badge::*;
pub use components::button::*;
pub use components::button_group::*;
pub use components::card::*;
pub use components::checkbox::*;
pub use components::collapsible::*;
pub use components::dialog::*;
pub use components::dropdown_menu::*;
pub use components::hover_card::*;
pub use components::input::*;
pub use components::input_group::*;
pub use components::kbd::*;
pub use components::label::*;
pub use components::overlay::{Align, Side};
pub use components::popover::*;
pub use components::progress::*;
pub use components::radio_group::*;
pub use components::scroll_area::*;
pub use components::separator::*;
pub use components::spinner::*;
pub use components::switch::*;
pub use components::tabs::*;
pub use components::textarea::*;
pub use components::tooltip::*;
pub use icons::*;
