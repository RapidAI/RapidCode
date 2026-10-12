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

pub mod alert;
pub mod avatar;
pub mod badge;
pub mod button;
pub mod card;
pub mod checkbox;
pub mod input;
pub mod kbd;
pub mod label;
pub mod progress;
pub mod separator;
pub mod spinner;
pub mod switch;
pub mod tabs;
pub mod textarea;
