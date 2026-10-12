//! 把 [`zcode_app::AppShell`] 渲染成一份**自包含**的 HTML 快照。
//!
//! 用途：在无头环境里也能看到应用长什么样。产出的文件内嵌了完整的
//! `assets/app.css`，用浏览器直接打开即可 —— 不需要 CDN、不需要本地服务、
//! 不需要 wasm。它证明的是「组件树 + 样式表能拼出一个成型的界面」，
//! 与 `cargo run -p zcode-app` 开原生窗口是两条独立的验证路径。
//!
//! ```bash
//! cargo run -p zcode-app --example app_preview    # → ./app-preview.html
//! ```
//!
//! 注意这里渲染的是 [`zcode_app::AppShell`] 而不是 [`zcode_app::App`]：
//! `App` 用 `document::Style` 注入样式，而那需要真实渲染器；SSR 下没有实现，
//! 所以样式表由本文件手工包进 `<style>`（内容仍是同一份 `APP_CSS` 常量）。

use dioxus::prelude::*;

fn main() {
    let mut dom = VirtualDom::new(zcode_app::AppShell);
    dom.rebuild_in_place();
    let body = dioxus_ssr::render(&dom);

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} · 静态快照</title>
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

    let path = "app-preview.html";
    std::fs::write(path, &html).expect("写出失败");
    println!(
        "已生成 {path}（{} 字节 / {} 行）",
        html.len(),
        html.lines().count()
    );
}
