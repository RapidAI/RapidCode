//! `components` —— shadcn 风格 UI 原语组件（Rust + Dioxus 移植自 `packages/ui/src/components/ui`）。
//!
//! 每个模块对应一个 TSX 文件，并遵循同一套移植约定：
//!
//! 1. **`cva` → 枚举 + `classes()`**：TS 用 `class-variance-authority` 描述
//!    variant / size 到类名的映射；Rust 侧用枚举表达同样的映射表，类型安全且
//!    零运行时开销。
//! 2. **`cn()` → `crate::cn::cn()`**：`clsx` + `tailwind-merge` 的等价实现。
//! 3. **每个模块导出纯函数 `*_classes()`**：类名拼装与组件渲染解耦，
//!    因此可以在**没有渲染器**（无需 wasm / 窗口）的情况下做单元测试。
//! 4. **`data-slot` / `data-state` / `data-orientation` 原样保留**：这些属性是
//!    shadcn 的 `@custom-variant`（见 `packages/ui/src/styles.css` 注释）赖以
//!    生效的锚点，缺失就会「类名在、效果不在」。

pub mod accordion;
pub mod alert;
pub mod alert_dialog;
pub mod avatar;
pub mod badge;
pub mod brand;
pub mod button;
pub mod button_group;
pub mod card;
pub mod carousel;
pub mod chart;
pub mod checkbox;
pub mod code_viewer;
pub mod collapsible;
pub mod command;
pub mod context_menu;
pub mod dialog;
pub mod diff_viewer;
pub mod flip_metric_value;
pub mod floating_particles;
pub mod dropdown_menu;
pub mod glm_icon;
pub mod highlighted_lightweight_diff_preview;
pub mod hover_card;
pub mod input;
pub mod input_group;
pub mod kbd;
pub mod label;
pub mod lightweight_diff_preview;
pub mod overlay;
pub mod pdf_viewer;
pub mod popover;
pub mod pptx_preview_viewer;
pub mod pptx_selection_action_bar;
pub mod progress;
pub mod radio_group;
pub mod resizable;
pub mod scroll_area;
pub mod scroll_fade_viewport;
pub mod select;
pub mod separator;
pub mod smooth_metric_value;
pub mod spinner;
pub mod switch;
pub mod tabs;
pub mod textarea;
pub mod toast;
pub mod tooltip;
