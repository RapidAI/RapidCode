//! # zcode-shared
//!
//! ZCode 共享核心逻辑的 Rust 移植（对应 `packages/shared`）。
//!
//! 完整的范围与映射见仓库根 `PORTING.md`。当前已移植的模块：
//!
//! - [`git`] —— Git 领域共享类型（`git.ts`）
//! - [`line_change_stat`] —— 行级 LCS 变更统计（`lineChangeStat.ts`）
//! - [`model_provider_family`] —— 模型供应商族归一与可见性（`model-provider-family.ts`）
//! - [`plan_identity`] —— 套餐身份解析（`plan-identity.ts`）
//!
//! `packages/shared` 其余模块（zod schema、协议类型等）尚未迁移，
//! 详见 `PORTING.md` 的「待办」。

pub mod git;
pub mod line_change_stat;
pub mod model_provider_family;
pub mod plan_identity;

/// crate 版本（对应 workspace 版本）。
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
