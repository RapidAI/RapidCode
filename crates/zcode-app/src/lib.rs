//! # zcode-app
//!
//! ZCode 的 **Rust + Dioxus 桌面外壳**：把 [`zcode_ui`] 里移植过来的 shadcn 原语
//! 组装成一个真的能跑起来的应用，用来验证移植结果不是「只通过测试」。
//!
//! ## 为什么是桌面而不是浏览器
//!
//! `dioxus/web` 需要 `wasm32-unknown-unknown` target + `wasm-bindgen-cli`，
//! 当前环境两者都没有。`dioxus/desktop`（wry + tao）走的是原生 webview，
//! 只需要宿主 target，因此在 `x86_64-pc-windows-msvc` 上直接可用。
//! 两者共用同一份组件代码，切换只是换 feature：
//!
//! ```bash
//! cargo run -p zcode-app                     # 原生窗口（默认）
//! cargo build -p zcode-app --no-default-features --features web   # 浏览器（需 wasm target）
//! ```
//!
//! ## 样式从哪来
//!
//! 组件里的 Tailwind 类名需要一份真的样式表。这里**不用 CDN**（桌面端无网络），
//! 而是在构建期用 Tailwind v4 CLI 扫描 `crates/**/*.rs` 生成
//! `assets/app.css`（114 KB），再用 `include_str!` 内嵌进二进制。
//! 设计令牌是从上游 `packages/ui/src/styles.css` 的 `@theme` 块抽取后
//! vendored 到 `.tailwind-build/tokens.css`（参照源在 `rust` 分支），
//! 因此配色、圆角、字号与 TS 版**同源**。重新生成见
//! `.tailwind-build/build.sh`。
//!
//! ## 为什么拆出 lib.rs
//!
//! 组件树放在 lib 里，`tests/smoke.rs` 才能在不打开窗口的前提下把整棵树
//! 构建一遍（记录型渲染器 + SSR 快照）。窗口能不能开起来依赖环境，
//! 「树能不能挂载、锚点对不对」不依赖环境，后者更适合放进 CI。
//!
//! ## 目录
//!
//! - [`state`]：单一 store（`Signal<AppState>`）+ 演示数据
//! - [`views`]：侧栏 / 顶栏 / 会话流 / 输入区 / 详情面板 / 设置 / 组件画廊

pub mod state;
pub mod views;

use dioxus::prelude::*;
use state::{AppCtx, ApprovalMode};
use zcode_ui::*;

/// 构建期生成的样式表（见 crate 文档）。
pub const APP_CSS: &str = include_str!("../assets/app.css");

/// 窗口标题。
pub const TITLE: &str = "ZCode — Rust + Dioxus 移植";

/// 应用根组件：往 `<head>` 注入标题与样式表，然后挂上界面。
///
/// 注意这里**没有把 `<head>` 注入和界面写在同一个组件里** ——
/// `document::Style` / `document::Title` 只在真实渲染器（desktop/web）下有实现，
/// 无渲染器的 SSR 快照（`tests/smoke.rs`）会跳过它们，因此界面被拆到
/// [`AppShell`]，两边各自可测。
#[component]
pub fn App() -> Element {
    rsx! {
        document::Title { {TITLE} }
        document::Style { {APP_CSS} }
        AppShell {}
    }
}

/// 界面主体（不含 `<head>` 注入，便于无渲染器测试）。
#[component]
pub fn AppShell() -> Element {
    // 全局 store
    let state = use_signal(state::demo_state);
    // UI 瞬态：各自的 Signal，避免开合浮层时让整棵树重算
    let settings_open = use_signal(|| false);
    let pending_delete = use_signal(|| None::<&'static str>);
    let delete_open = use_signal(|| false);
    let settings_tab = use_signal(|| "general".to_string());
    let detail_open = use_signal(|| true);
    let gallery_open = use_signal(|| false);
    let workspace_tab = use_signal(|| "conversation".to_string());
    let plan_open = use_signal(|| true);
    let approval_value = use_signal(|| ApprovalMode::Ask.as_str().to_string());

    use_context_provider(|| AppCtx {
        state,
        settings_open,
        pending_delete,
        delete_open,
        settings_tab,
        detail_open,
        gallery_open,
        workspace_tab,
        plan_open,
        approval_value,
    });

    let dark = state.read().dark;
    let root_class = if dark { "dark h-full w-full" } else { "h-full w-full" };

    rsx! {
        div {
            class: "{root_class}",
            div { class: "flex h-full w-full overflow-hidden bg-background text-foreground",
                views::Sidebar {}
                div { class: "flex min-w-0 flex-1 flex-col",
                    views::TopBar {}
                    div { class: "flex min-h-0 flex-1",
                        views::ConversationView {}
                        if (detail_open)() {
                            views::DetailPanel {}
                        }
                    }
                    views::Composer {}
                }
            }
            views::SettingsDialog {}
            views::DeleteConfirmDialog {}
            views::GalleryDialog {}
        }
    }
}

/// 面板通用外壳：`overflow-hidden` + 纵向布局，供各面板复用。
pub fn pane_class(extra: &str) -> String {
    cn(&["flex min-h-0 flex-col border-border bg-background", extra])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stylesheet_is_embedded_and_non_trivial() {
        assert!(
            APP_CSS.len() > 50_000,
            "样式表看起来没生成：{} 字节",
            APP_CSS.len()
        );
        // 关键令牌与工具类必须真的在表里，否则窗口会是一片白
        for needle in [
            "--color-background:",
            "--color-menu:",
            "--color-tooltip:",
            "text-ui-base",
            ".dark",
            "data-highlighted",
        ] {
            assert!(APP_CSS.contains(needle), "样式表缺少 `{needle}`");
        }
    }

    #[test]
    fn pane_class_merges() {
        assert!(pane_class("w-64").contains("w-64"));
        assert!(pane_class("").contains("flex"));
    }
}
