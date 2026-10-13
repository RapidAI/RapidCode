//! # zcode-services
//!
//! ZCode 服务层的 Rust 移植（对应 `packages/services`）。
//!
//! `packages/services` 有 681 个文件，这里**只移植了 git 工作区状态这一条链路**，
//! 而且是它自包含的那一段：环境发现 → 命令执行 → 输出解析 → 仓库解析 → 变更分组。
//! 选它的理由：不依赖 LLM provider、不依赖文件 watcher、不依赖 RPC 传输，
//! 但能产出**真实数据**（真实的变更文件、分支、ahead/behind），
//! 因此可以端到端验证「移植过来的逻辑真的在跑」，而不是只跑单元测试。
//!
//! ## 已移植
//!
//! | 上游 | 本 crate |
//! | --- | --- |
//! | `git/config.ts` | [`git::config`] |
//! | `git/providers/gitEnvironmentProvider.ts` + `git/providers/gitCommandProvider.ts` | [`git::provider`] |
//! | `git/repo/gitCliTypes.ts`（类型子集） | [`git::types`] |
//! | `git/repo/gitCliHelpers.ts`（纯解析 + 未跟踪文件行数统计） | [`git::cli_helpers`] |
//! | `git/repo/gitCliRepo.ts`（`resolveRepository` / `getStatus` / `listLocalBranches`） | [`git::repo`] |
//! | `git/gitService.ts`（`getChanges` 的分组与统计合并） | [`git::changes`] |
//!
//! ## 未移植（有意为之）
//!
//! `gitCommitMessageGenerator`（依赖 LLM provider）、`gitCheckpoint*`（依赖
//! checkpoint store 与文件系统快照）、`commitMessageFileScope`、
//! `getIgnoredPaths` / `getDiff` / `getCommitGraph` / `getIdentity`、
//! 以及 `stage/unstage/discard/commit/push` 这些**写操作**。
//! 写操作没有移植的原因很直接：它们会改用户的仓库，而当前链路只读。
//! 具体缺口见 `PORTING.md`。
//!
//! ## 与上游的两处刻意差异
//!
//! 1. **超时实现**：Node 侧用 `setTimeout` + `child.kill()`（SIGTERM）+ 宽限期 +
//!    `taskkill /T /F`。Rust 的 `Child::kill` 在 Windows 上等价于
//!    `TerminateProcess`（即 SIGKILL），没有「先礼后兵」的余地，因此
//!    [`git::provider::GitCommandProvider`] 只保留「杀 + 宽限期 + 判定 orphaned」
//!    这三步，去掉了中间的 force-kill 阶段。
//! 2. **spawn 失败的表达**：Node 把 spawn 错误降级成 `exitCode = -2` +
//!    `stderr = error.message`。Rust 里 spawn 失败有明确的 `io::ErrorKind`，
//!    因此额外带一个 [`git::provider::GitSpawnErrorKind`]，判定「cwd 不存在」
//!    时不再依赖 stderr 文案（见 `is_missing_working_directory_result`）。

pub mod git;
