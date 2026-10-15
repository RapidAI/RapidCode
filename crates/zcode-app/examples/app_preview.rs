//! 把 [`zcode_app::AppShell`] 渲染成**自包含**的 HTML 快照。
//!
//! 用途：在无头环境里也能看到应用长什么样。产出的文件内嵌了完整的
//! `assets/app.css`，用浏览器直接打开即可 —— 不需要 CDN、不需要本地服务、
//! 不需要 wasm。它证明的是「组件树 + 样式表能拼出一个成型的界面」，
//! 与 `cargo run -p zcode-app` 开原生窗口是两条独立的验证路径。
//!
//! ```bash
//! cargo run -p zcode-app --example app_preview    # → ./app-preview.html / ./app-preview-changes.html
//! ```
//!
//! ## 快照里的「变更」页是**真实数据**
//!
//! 这个 example 会**同步跑一次真正的 git 查询**（走 `zcode-services` 那套移植过来的
//! 代码），把结果通过 `WorkspaceSeed` 注入界面。所以快照里列出的文件名、`+N/-M`
//! 都来自你当前这份仓库 —— 换一个目录跑（`ZCODE_WORKSPACE=...`）就会换一批内容。
//!
//! 之所以同步扫而不是等后台线程：SSR 只渲染一帧，没有事件循环去驱动
//! `spawn` 出来的 future，等不到回传。真实窗口里走的是后台线程那条路
//! （见 `crates/zcode-app/tests/workspace_loader.rs`）。
//!
//! 注意这里渲染的是 [`zcode_app::AppShell`] 而不是 [`zcode_app::App`]：
//! `App` 用 `document::Style` 注入样式，而那需要真实渲染器；SSR 下没有实现，
//! 所以样式表由本文件手工包进 `<style>`（内容仍是同一份 `APP_CSS` 常量）。

use dioxus::prelude::*;
use zcode_app::workspace::{
    default_workspace_path, diff_query_for, display_path, DiffPreviewState, WorkspaceData,
    WorkspaceStatus,
};
use zcode_app::{AppShell, WorkspaceSeed};
use zcode_services::git::changes::{get_workspace_changes, summarize_changes};
use zcode_services::git::repo::GitCliRepo;
use zcode_ui::{CodeCommentPreviewBucket, CodeCommentPreviewEntry};

/// 同步扫一次工作区，产出可以注入界面的数据。
fn scan_workspace() -> (Option<WorkspaceData>, WorkspaceStatus) {
    let workspace = default_workspace_path();
    let repo = GitCliRepo::new();

    match repo.get_status(&workspace) {
        Ok(snapshot) => {
            let changes = get_workspace_changes(&snapshot);
            let totals = summarize_changes(&changes);
            // 分支比较也走真实查询 —— 本仓库的 `rust-only` 通常落后/领先
            // `origin/rust-only` 若干提交，快照里就能看到「与上游比较」分组。
            // 没有上游（或目录不是仓库）时降级成默认值，分组自动不渲染。
            let branch_comparison = repo.get_branch_comparison(&workspace).unwrap_or_default();
            (
                Some(WorkspaceData {
                    summary: snapshot.summary,
                    changes,
                    totals,
                    branch_comparison,
                }),
                WorkspaceStatus::Ready,
            )
        }
        Err(failure) => (None, WorkspaceStatus::Failed(failure.message)),
    }
}

/// 同步取一个变更的 diff，用来在快照里展开一个真的 diff 面板。
///
/// 挑选规则：**优先挑一个 patch 行数适中、单行不过长**的变更。
/// 变更列表里的第一个往往正好是压缩过的 `app.css`（整份文件一行），
/// 展开它只会得到一条横到屏幕外的巨长行，快照完全没法看。
/// 挑不到合适的就退回第一个变更。
///
/// 失败（例如变更是未跟踪的二进制文件）时返回 `None`，
/// 快照里就只是没有展开项 —— 不影响其它内容。
fn scan_first_diff(data: &WorkspaceData) -> Option<DiffPreviewState> {
    /// 太长/太多行的 patch 不适合放进静态快照。
    fn is_readable(diff: &zcode_shared::git::GitDiffResult) -> bool {
        use zcode_shared::git::GitDiffAvailability;
        if diff.availability != GitDiffAvailability::Patch {
            return false;
        }
        let Some(patch) = diff.patch.as_deref() else {
            return false;
        };
        patch.lines().count() <= 80 && patch.lines().all(|line| line.chars().count() <= 200)
    }

    let workspace = default_workspace_path();
    let repo = GitCliRepo::new();

    let mut fallback: Option<(String, DiffPreviewState)> = None;
    for change in &data.changes {
        let query = diff_query_for(change);
        let path = display_path(change).to_string();
        let Ok(diff) = repo.get_diff(&workspace, &query) else {
            continue;
        };
        let state = DiffPreviewState::Ready {
            id: 1,
            path: path.clone(),
            diff: Box::new(diff),
        };
        let readable = matches!(
            &state,
            DiffPreviewState::Ready { diff, .. } if is_readable(diff)
        );
        if readable {
            return Some(state);
        }
        if fallback.is_none() {
            fallback = Some((path, state));
        }
    }

    fallback.map(|(_, state)| state)
}

/// 把种子塞进 context，然后渲染真实的 `AppShell`。
#[component]
fn PreviewRoot(
    data: Option<WorkspaceData>,
    status: WorkspaceStatus,
    tab: String,
    diff: Option<DiffPreviewState>,
    comments: Vec<(CodeCommentPreviewBucket, CodeCommentPreviewEntry)>,
) -> Element {
    use_context_provider(move || WorkspaceSeed {
        data,
        status,
        tab: Some(tab),
        diff,
        comments,
    });

    rsx! { AppShell {} }
}

/// 渲染一份快照并落盘。
fn write_snapshot(
    path: &str,
    data: Option<WorkspaceData>,
    status: WorkspaceStatus,
    tab: &str,
    diff: Option<DiffPreviewState>,
    comments: Vec<(CodeCommentPreviewBucket, CodeCommentPreviewEntry)>,
) {
    let mut dom = VirtualDom::new_with_props(
        PreviewRoot,
        PreviewRootProps {
            data,
            status,
            tab: tab.to_string(),
            diff,
            comments,
        },
    );
    dom.rebuild_in_place();
    let body = dioxus_ssr::render(&dom);

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} · 静态快照（{tab}）</title>
<style>{css}</style>
<style>
  /* 把「应用窗口」摆到页面中央，模拟 1360x860 的桌面窗口 */
  html, body {{ height: 100%; margin: 0; }}
  body {{
    display: flex; align-items: center; justify-content: center;
    background: #1c1c1e; padding: 24px; box-sizing: border-box;
    font-family: system-ui, -apple-system, "Segoe UI", "PingFang SC",
                 "Microsoft YaHei", sans-serif;
  }}
  #frame {{
    width: 1360px; height: 860px; max-width: 100%; max-height: 100%;
    border-radius: 10px; overflow: hidden;
    box-shadow: 0 24px 64px rgba(0,0,0,.55), 0 0 0 1px rgba(255,255,255,.08);
    resize: both;
  }}
  #frame > div {{ height: 100%; }}
</style>
</head>
<body>
<div id="frame">{body}</div>
</body>
</html>
"#,
        title = zcode_app::TITLE,
        css = zcode_app::APP_CSS,
    );

    std::fs::write(path, &html).expect("写出失败");
    println!(
        "已生成 {path}（{} 字节 / {} 行）",
        html.len(),
        html.lines().count()
    );
}

fn main() {
    let (data, status) = scan_workspace();

    match data.as_ref() {
        Some(data) if data.is_repository() => println!(
            "工作区扫描成功：{} @ {}（{} 个变更，+{} -{}）",
            data.branch_label(),
            data.summary.repo_root,
            data.totals.file_count,
            data.totals.added,
            data.totals.removed
        ),
        Some(data) => println!("工作区不是 Git 仓库：{}", data.summary.workspace_path),
        None => println!("工作区扫描失败：{}", status.failure().unwrap_or("未知原因")),
    }

    // 「变更」页那份快照额外展开第一个变更的 diff，让静态快照也能证明
    // diff 预览这块真的在工作（否则它在快照里永远是收起的）。
    let diff = data.as_ref().and_then(scan_first_diff);
    match diff.as_ref() {
        Some(DiffPreviewState::Ready { path, diff, .. }) => println!(
            "已展开 diff：{path}（{}）",
            diff.availability.as_str()
        ),
        Some(DiffPreviewState::Failed { path, message, .. }) => {
            println!("展开 diff 失败：{path} —— {message}")
        }
        _ => println!("没有可展开的变更"),
    }

    // 给「变更」页那份快照塞一条**真实的**批注：锚在 patch 里第一条新增行上。
    // 这样截图里能看到批注卡片，而不是「有入口但没内容」。
    let comments = build_demo_comments(data.as_ref(), diff.as_ref());
    if !comments.is_empty() {
        println!("已注入 {} 条代码批注", comments.len());
    }

    write_snapshot(
        "app-preview.html",
        data.clone(),
        status.clone(),
        "conversation",
        None,
        Vec::new(),
    );
    write_snapshot("app-preview-changes.html", data, status, "changes", diff, comments);
}

/// 从真实的 diff 里挑一条新增行，造一条批注。
///
/// 用真实行号（而不是硬编码 1）是有意的：批注锚点错位是这一块最容易出的 bug，
/// 拿真实 patch 算出来的行号能让截图同时验证「映射对不对」。
fn build_demo_comments(
    data: Option<&WorkspaceData>,
    diff: Option<&DiffPreviewState>,
) -> Vec<(CodeCommentPreviewBucket, CodeCommentPreviewEntry)> {
    let Some(WorkspaceData { summary, .. }) = data else {
        return Vec::new();
    };
    let Some(DiffPreviewState::Ready { path, diff, .. }) = diff else {
        return Vec::new();
    };
    let Some(patch) = diff.patch.as_deref() else {
        return Vec::new();
    };

    let lines: Vec<String> = patch
        .split('\n')
        .map(|line| line.to_string())
        .collect();
    let numbers = zcode_ui::map_patch_lines_to_file_numbers(&lines);
    // 第一条**新增**行（`is_added`）才是我们要的锚点
    let Some((index, anchor)) = numbers
        .iter()
        .enumerate()
        .find_map(|(index, mapped)| mapped.filter(|m| m.is_added).map(|m| (index, m)))
    else {
        return Vec::new();
    };

    let selected_text = zcode_ui::get_lightweight_diff_line_parts(&lines[index])
        .content
        .trim()
        .to_string();
    let source_path = path.clone();

    vec![(
        CodeCommentPreviewBucket {
            workspace_path: summary.workspace_path.clone(),
            workspace_identity: Some(summary.branch_name.clone().unwrap_or_default()),
            source_path: source_path.clone(),
        },
        CodeCommentPreviewEntry {
            id: format!("code-comment:{}:{source_path}", anchor.new_line),
            source_path: Some(source_path.clone()),
            source_title: zcode_ui::get_source_title(&source_path),
            selected_text,
            comment: "这一行是这次改动新加的，确认一下是否需要同步更新文档。".to_string(),
            start_line: anchor.new_line as i64,
            end_line: anchor.new_line as i64,
        },
    )]
}
