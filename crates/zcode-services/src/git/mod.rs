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
//! | `gitService.ts`（`getChanges`） | [`changes`] |

pub mod changes;
pub mod cli_helpers;
pub mod config;
pub mod provider;
pub mod repo;
pub mod types;
