//! Git 服务层。
//!
//! 模块划分刻意与上游 `packages/services/src/git/` 对齐，
//! 这样后续补 `gitCommitMessageGenerator` / `gitCheckpoint*` 时能直接对号入座：
//!
//! | 上游路径 | 本模块 |
//! | --- | --- |
//! | `config.ts` | [`config`] |
//! | `providers/gitEnvironmentProvider.ts` + `providers/gitCommandProvider.ts` | [`provider`] |
//! | `repo/gitCliTypes.ts` | [`types`] |
//! | `repo/gitCliHelpers.ts` | [`cli_helpers`] |
//! | `repo/gitCliRepo.ts` | [`repo`] |
//! | `repo/gitCliRepo.ts`（`git log` 解析那一组纯函数） | [`graph`] |
//! | `repo/gitCliRepo.ts`（diff 结果构造那一组纯函数） | [`diff`] |
//! | `gitService.ts`（`getChanges`） | [`changes`] |
//!
//! [`graph`] 与 [`diff`] 在上游并不是独立文件 —— 它们是 `gitCliRepo.ts` /
//! `gitCliHelpers.ts` 顶部的模块级纯函数。拆出来只是为了能脱离进程直接测，
//! 逻辑逐行对应。

pub mod changes;
pub mod cli_helpers;
pub mod config;
pub mod diff;
pub mod graph;
pub mod provider;
pub mod repo;
pub mod types;
