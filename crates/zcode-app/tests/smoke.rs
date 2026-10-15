//! 整棵应用树的冒烟测试。
//!
//! 为什么要有这一层：`cargo build` 只能证明「能编译」，开窗口才能证明「能跑」，
//! 但开窗口依赖图形环境（CI、无头容器里没有）。这里用 `dioxus-ssr` 把
//! [`zcode_app::AppShell`] 以及三个浮层真渲染一遍，断言：
//!
//! 1. 整棵树能挂载（任何 `use_context` 缺失、hook 顺序错误都会在这里 panic）；
//! 2. 各个区域的文案真的出现了（说明分支没走空）；
//! 3. 关键原语的 `data-slot` 锚点与 `data-*` 语义属性真的在 DOM 里 —— 这是
//!    shadcn `@custom-variant` 生效的前提，也是移植中最容易漏掉的东西。
//!
//! 这是「能正常运行」在无头环境下的最强代理。

use dioxus::prelude::*;
use zcode_app::state::{demo_state, AppCtx};
use zcode_app::workspace::{
    DiffPreviewState, WorkspaceData, WorkspaceNotice, WorkspaceStatus,
};
use zcode_shared::git::{GitBranchComparison, GitBranchComparisonChange, GitDiffAvailability, GitDiffResult};
use zcode_ui::*;

/// 把组件渲染成 HTML 字符串。
///
/// 设 `DUMP_HTML=1` 会把每棵树的输出落到 `target/tmp/dom-<测试名>.html`
/// （`CARGO_TARGET_TMPDIR`），断言对不上时用它看真实 DOM，比猜快得多。
fn html(app: fn() -> Element) -> String {
    let mut dom = VirtualDom::new(app);
    dom.rebuild_in_place();
    let out = dioxus_ssr::render(&dom);
    if std::env::var("DUMP_HTML").is_ok() {
        let name = std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .replace("::", "-");
        let _ = std::fs::write(dump_path(&format!("dom-{name}.html")), &out);
    }
    out
}

/// 调试产物的落盘路径。用 `CARGO_TARGET_TMPDIR` 而不是相对路径，
/// 免得 `cargo test` 的工作目录（包根）下多出一个 `target/`。
fn dump_path(name: &str) -> String {
    let dir = env!("CARGO_TARGET_TMPDIR");
    let _ = std::fs::create_dir_all(dir);
    format!("{dir}/{name}")
}

/// 断言一组 `data-slot` 锚点都在。
fn assert_slots(out: &str, slots: &[&str], where_: &str) {
    for slot in slots {
        let anchor = format!(r#"data-slot="{slot}""#);
        assert!(out.contains(&anchor), "{where_} 里缺少 `{anchor}`");
    }
}

// ---------------------------------------------------------------------------
// 夹具：注入 AppCtx，并按需把浮层打开
// ---------------------------------------------------------------------------

/// 注入 `AppCtx` 并挂上三个浮层。
///
/// 真实的 [`zcode_app::AppShell`] 里浮层默认是关的（关着就整体不渲染），
/// 所以浮层内部的锚点必须单独渲染才能断言。
#[component]
fn Harness(
    #[props(default)] gallery: bool,
    #[props(default)] settings: bool,
    #[props(default)] delete: bool,
) -> Element {
    let ctx = AppCtx {
        state: use_signal(demo_state),
        settings_open: use_signal(|| settings),
        pending_delete: use_signal(|| Some("c2")),
        delete_open: use_signal(|| delete),
        settings_tab: use_signal(|| "general".to_string()),
        detail_open: use_signal(|| false),
        gallery_open: use_signal(|| gallery),
        workspace_tab: use_signal(|| "conversation".to_string()),
        plan_open: use_signal(|| true),
        approval_value: use_signal(|| "ask".to_string()),
        // 工作区数据留空：夹具不提供 `WorkspaceBootstrap`，也就不会去跑真实 git。
        // 真实数据的渲染另见 `changes_view_*` 一组测试。
        workspace: use_signal(|| None::<WorkspaceData>),
        workspace_status: use_signal(WorkspaceStatus::default),
        workspace_notice: use_signal(|| None::<WorkspaceNotice>),
        diff_preview: use_signal(|| None::<DiffPreviewState>),
    };
    use_context_provider(|| ctx);

    rsx! {
        zcode_app::views::GalleryDialog {}
        zcode_app::views::SettingsDialog {}
        zcode_app::views::DeleteConfirmDialog {}
    }
}

#[component]
fn GalleryOpen() -> Element {
    rsx! { Harness { gallery: true } }
}

#[component]
fn SettingsOpen() -> Element {
    rsx! { Harness { settings: true } }
}

#[component]
fn DeleteOpen() -> Element {
    rsx! { Harness { delete: true } }
}

/// 把所有「需要展开才有 DOM」的浮层强制打开，用来断言它们的内容标记。
///
/// `Tooltip` / `HoverCard` / `Popover` / `DropdownMenu` / `DropdownMenuSub` 都支持
/// 受控 `open`，所以这里直接传 `Some(signal(true))`，不需要模拟 hover。
#[component]
fn OverlaysOpen() -> Element {
    let tip = use_signal(|| true);
    let hov = use_signal(|| true);
    let pop = use_signal(|| true);
    let menu = use_signal(|| true);
    let sub = use_signal(|| true);

    rsx! {
        Tooltip { open: Some(tip),
            TooltipTrigger { Button { size: ButtonSize::Sm, "tip" } }
            TooltipContent { side: Side::Bottom, align: Align::Center, side_offset: 6.0,
                "tooltip 内容"
                Kbd { "K" }
            }
        }

        HoverCard { open: Some(hov),
            HoverCardTrigger { Button { size: ButtonSize::Sm, "hov" } }
            HoverCardContent { side: Side::Bottom, side_offset: 4.0, "hover card 内容" }
        }

        Popover { open: Some(pop),
            PopoverTrigger { Button { size: ButtonSize::Sm, "pop" } }
            PopoverContent { side: Side::Bottom, side_offset: 4.0,
                PopoverHeader {
                    PopoverTitle { "popover 标题" }
                    PopoverDescription { "popover 描述" }
                }
                Separator {}
            }
        }

        DropdownMenu { open: Some(menu),
            DropdownMenuTrigger { variant: ButtonVariant::Outline, size: ButtonSize::Sm, "menu" }
            DropdownMenuContent { side: Side::Bottom, align: Align::Start, side_offset: 2.0,
                DropdownMenuLabel { "标签" }
                DropdownMenuGroup {
                    DropdownMenuItem { "普通项" DropdownMenuShortcut { "⌘C" } }
                    DropdownMenuCheckboxItem { checked: true, "复选项" }
                    DropdownMenuRadioItem { checked: true, "单选项" }
                }
                DropdownMenuSeparator {}
                DropdownMenuSub { open: Some(sub),
                    DropdownMenuSubTrigger { "子菜单" }
                    DropdownMenuSubContent { DropdownMenuItem { "子项" } }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 主界面
// ---------------------------------------------------------------------------

/// 调试用：一次把四棵树都 dump 到 `CARGO_TARGET_TMPDIR`。断言对不上时先跑它。
#[test]
#[ignore = "调试辅助，不参与常规测试"]
fn dump_every_tree() {
    for (name, f) in [
        ("shell", zcode_app::AppShell as fn() -> Element),
        ("gallery", GalleryOpen as fn() -> Element),
        ("overlays", OverlaysOpen as fn() -> Element),
        ("settings", SettingsOpen as fn() -> Element),
        ("delete", DeleteOpen as fn() -> Element),
    ] {
        let mut dom = VirtualDom::new(f);
        dom.rebuild_in_place();
        let out = dioxus_ssr::render(&dom);
        std::fs::write(dump_path(&format!("dom-{name}.html")), &out).unwrap();
        println!("写出 {}", dump_path(&format!("dom-{name}.html")));
    }
}

#[test]
fn app_shell_mounts_without_renderer() {
    // 任何 context 缺失 / hook 顺序错误都会在这里 panic
    let out = html(zcode_app::AppShell);
    assert!(out.len() > 20_000, "渲染结果过短：{} 字节", out.len());
}

#[test]
fn all_regions_are_present() {
    let out = html(zcode_app::AppShell);
    for needle in [
        // 侧栏
        "ZCode",
        "新建会话",
        "搜索会话",
        "今天",
        "昨天",
        "更早",
        "重构 RPC 传输层",
        "组件画廊",
        "上下文用量",
        // 顶栏
        "对话",
        "变更",
        "任务",
        "ZCode Pro",
        // 顶栏的工作区徽标：没有 bootstrap 时不扫描，显示「未扫描」而不是假的 `rust`。
        // 真实分支名的渲染见 `changes_view_*` 一组测试。
        "未扫描",
        // 会话流
        "继续完成剩余移植工作",
        "cargo test",
        "871 passed",
        // 详情面板
        "执行计划",
        "变更文件",
        "工具调用",
        "权限模式",
        "每次询问",
        "显示推理过程",
        // 输入区
        "描述你想让 ZCode 做的事",
        "Shift + ⏎ 换行",
    ] {
        assert!(out.contains(needle), "界面里缺少文案 `{needle}`");
    }
}

#[test]
fn shell_primitive_anchors_are_emitted() {
    let out = html(zcode_app::AppShell);
    // 只列「默认状态下一定在 DOM 里」的锚点。刻意排除的三类：
    //   * `spinner` —— 只在 `state.streaming` 为真时渲染（演示状态里是 false）；
    //   * `tabs-content` —— 顶栏的 Tabs 只当分段切换器用，面板内容由工作区渲染；
    //   * 浮层内容（`*-content`）—— 关闭时整体不渲染，见 `overlay_contents_render_when_open`。
    assert_slots(
        &out,
        &[
            "button",
            "badge",
            "card",
            "card-header",
            "card-title",
            "card-description",
            "card-content",
            "card-footer",
            "alert",
            "alert-title",
            "alert-description",
            "alert-action",
            // 侧栏的搜索框是 InputGroupInput → slot 被覆盖成 input-group-control
            "input-group",
            "input-group-addon",
            "input-group-control",
            "separator",
            "progress",
            "progress-indicator",
            "avatar",
            "avatar-fallback",
            "avatar-group",
            "avatar-group-count",
            "switch",
            "switch-thumb",
            "checkbox",
            "tabs",
            "tabs-list",
            "tabs-trigger",
            "accordion",
            "accordion-item",
            "accordion-trigger",
            "accordion-content",
            "collapsible",
            "collapsible-trigger",
            "collapsible-content",
            "radio-group",
            "radio-group-item",
            "radio-group-indicator",
            "button-group",
            "button-group-separator",
            "dropdown-menu",
            "dropdown-menu-trigger",
            "tooltip",
            "tooltip-trigger",
            "hover-card",
            "hover-card-trigger",
            "scroll-area",
            "scroll-area-viewport",
            "kbd",
        ],
        "主界面",
    );
}

#[test]
fn shell_semantic_data_attributes_are_emitted() {
    let out = html(zcode_app::AppShell);
    // 这些属性是 shadcn @custom-variant 的输入。缺了它们，样式表里的
    // data-open: / data-checked: / data-vertical: 就永远不会命中。
    //
    // `data-align` 只列 InputGroupAddon 的两种行内对齐：Tooltip / DropdownMenu
    // 的 `Align::End` 只在浮层展开时出现，归 `overlay_contents_render_when_open` 管。
    for attr in [
        r#"data-orientation="vertical""#,
        r#"data-orientation="horizontal""#,
        r#"data-state="unchecked""#,
        r#"data-state="checked""#,
        r#"data-state="open""#,
        r#"data-variant="outline""#,
        r#"data-variant="ghost""#,
        r#"data-variant="secondary""#,
        r#"data-size="sm""#,
        r#"data-size="lg""#,
        r#"data-align="inline-start""#,
        r#"data-align="inline-end""#,
        "data-disabled",
    ] {
        assert!(out.contains(attr), "主界面 DOM 里缺少 `{attr}`");
    }
}

#[test]
fn overlays_are_closed_by_default() {
    let out = html(zcode_app::AppShell);
    // 三个浮层默认关闭：DialogContent 关闭时整体不渲染，遮罩不会挡住底下的点击
    assert!(!out.contains(r#"data-slot="dialog-content""#));
    assert!(!out.contains(r#"data-slot="alert-dialog-content""#));
    assert!(!out.contains(r#"data-slot="dialog-overlay""#));
    // 但顶栏的 Tabs 选项始终可见
    assert!(out.contains(r#"data-slot="tabs-trigger""#));
}

#[test]
fn dark_class_is_absent_by_default() {
    let out = html(zcode_app::AppShell);
    assert!(!out.contains(r#"class="dark h-full w-full""#));
    assert!(out.contains("h-full w-full"));
}

// ---------------------------------------------------------------------------
// 组件画廊
// ---------------------------------------------------------------------------

#[test]
fn gallery_renders_every_always_on_primitive() {
    let out = html(GalleryOpen);
    // 「always-on」= 不需要展开就出现在 DOM 里。浮层内部（`*-content`、
    // `dropdown-menu-sub` 等）由 `overlay_contents_render_when_open` 覆盖。
    assert_slots(
        &out,
        &[
            // 表单
            "input",
            "input-group",
            "input-group-text",
            "input-group-addon",
            "input-group-control",
            "textarea",
            "label",
            "checkbox",
            "switch",
            "switch-thumb",
            "radio-group",
            "radio-group-item",
            "radio-group-indicator",
            // 反馈
            "alert",
            "alert-title",
            "alert-description",
            "alert-action",
            "progress",
            "progress-indicator",
            "spinner",
            // 展示
            "avatar",
            "avatar-fallback",
            "avatar-badge",
            "avatar-group",
            "avatar-group-count",
            "card",
            "card-header",
            "card-title",
            "card-description",
            "card-action",
            "card-content",
            "card-footer",
            "separator",
            "scroll-area",
            "scroll-area-viewport",
            // 导航
            "tabs",
            "tabs-list",
            "tabs-trigger",
            "tabs-content",
            "accordion",
            "accordion-item",
            "accordion-trigger",
            "accordion-content",
            "collapsible",
            "collapsible-trigger",
            "collapsible-content",
            // 浮层（外壳部分：锚点与触发器）
            "tooltip",
            "tooltip-trigger",
            "hover-card",
            "hover-card-trigger",
            "popover",
            "popover-trigger",
            "dropdown-menu",
            "dropdown-menu-trigger",
            "alert-dialog-trigger",
            "button",
            "badge",
            "kbd",
        ],
        "画廊",
    );
}

#[test]
fn gallery_covers_all_variants() {
    let out = html(GalleryOpen);
    for attr in [
        r#"data-variant="default""#,
        r#"data-variant="outline""#,
        r#"data-variant="secondary""#,
        r#"data-variant="ghost""#,
        r#"data-variant="destructive""#,
        r#"data-variant="warning""#,
        r#"data-variant="link""#,
        r#"data-size="xs""#,
        r#"data-size="sm""#,
        r#"data-size="default""#,
        r#"data-size="lg""#,
        r#"data-size="icon-xs""#,
        r#"data-size="icon-sm""#,
        r#"data-size="icon-md""#,
        r#"data-size="icon-lg""#,
        r#"data-align="inline-start""#,
        r#"data-align="inline-end""#,
        r#"data-align="block-start""#,
        r#"data-align="block-end""#,
        r#"data-state="indeterminate""#,
        r#"data-orientation="vertical""#,
        r#"data-orientation="horizontal""#,
    ] {
        assert!(out.contains(attr), "画廊里缺少 `{attr}`");
    }
}

#[test]
fn gallery_scrolls() {
    let out = html(GalleryOpen);
    assert!(out.contains(r#"data-slot="dialog-content""#));
    // 画廊用 ScrollArea 包住内容，视口必须真的能滚（我们加了 overflow-auto）
    assert!(out.contains("overflow-auto"));
    assert!(out.contains(r#"data-slot="scroll-area-viewport""#));
}

/// Radix 的 ScrollArea 自带 JS 滚动条；我们换成原生滚动 + 可选隐藏（见 PORTING-UI.md §4.3）。
/// 这条测试保证「换法」两头都成立：DOM 上写了类名，样式表里也真编译出了规则。
#[test]
fn scroll_area_hides_native_scrollbar_where_asked() {
    // 侧栏与会话流要「能滚但不显示滚动条」，靠 `hide_scrollbar: true`
    let out = html(zcode_app::AppShell);
    assert!(
        out.contains("[scrollbar-width:none]"),
        "侧栏/详情面板应当启用 hide_scrollbar"
    );
    // 类名在 DOM 上没用 —— 得确认 Tailwind 真把它编译成了规则
    assert!(
        zcode_app::APP_CSS.contains("scrollbar-width:none"),
        "样式表里没有编译出 scrollbar-width:none 规则"
    );
}

// ---------------------------------------------------------------------------
// 浮层内容（需要展开才有 DOM）
// ---------------------------------------------------------------------------

#[test]
fn overlay_contents_render_when_open() {
    let out = html(OverlaysOpen);
    assert_slots(
        &out,
        &[
            "tooltip-content",
            "hover-card-content",
            "popover-content",
            "popover-header",
            "popover-title",
            "popover-description",
            "dropdown-menu-content",
            "dropdown-menu-item",
            "dropdown-menu-checkbox-item",
            "dropdown-menu-checkbox-item-indicator",
            "dropdown-menu-radio-item",
            "dropdown-menu-separator",
            "dropdown-menu-shortcut",
            "dropdown-menu-sub",
            "dropdown-menu-sub-trigger",
            "dropdown-menu-sub-content",
        ],
        "浮层",
    );
    // 方向/对齐属性真的写出来了
    assert!(out.contains(r#"data-side="bottom""#));
    assert!(out.contains(r#"data-align="center""#));
    // Radix 的 ItemIndicator 只在选中时挂载，这里也应如此
    assert!(out.contains(r#"data-slot="dropdown-menu-checkbox-item-indicator""#));
    assert!(out.contains(r#"data-state="open""#));
}

#[test]
fn tooltip_and_popover_position_themselves_with_css() {
    let out = html(OverlaysOpen);
    // 没有 floating-ui，位置完全靠类名：absolute + 方向 inset + 偏移
    assert!(out.contains("absolute"));
    assert!(out.contains("bottom-full") || out.contains("top-full"));
    assert!(out.contains("-translate-x-1/2") || out.contains("left-0"));
    assert!(out.contains("mt-[6px]") || out.contains("mt-[4px]") || out.contains("mt-[2px]"));
}

// ---------------------------------------------------------------------------
// 设置与确认框
// ---------------------------------------------------------------------------

#[test]
fn settings_dialog_renders_tabs() {
    let out = html(SettingsOpen);
    assert_slots(
        &out,
        &[
            "dialog-content",
            "dialog-overlay",
            "dialog-header",
            "dialog-title",
            "dialog-description",
            "dialog-footer",
            "dialog-close",
            "tabs",
            "tabs-list",
            "tabs-trigger",
            "tabs-content",
            "switch",
            "switch-thumb",
            "radio-group",
            "radio-group-item",
            "radio-group-indicator",
            "checkbox",
            "button-group",
            "alert",
            "separator",
            "label",
            "kbd",
        ],
        "设置对话框",
    );
    for needle in ["常规", "外观", "关于", "默认模型", "字号", "zcode-rpc", "zcode-ui"] {
        assert!(out.contains(needle), "设置里缺少 `{needle}`");
    }
    // 遮罩用 fixed inset-0，不需要 Portal 也能盖满视口
    assert!(out.contains("fixed inset-0"));
    assert!(out.contains(r#"role="dialog""#));
    assert!(out.contains(r#"aria-modal="true""#));
}

#[test]
fn delete_confirm_dialog_shows_target_title() {
    let out = html(DeleteOpen);
    assert_slots(
        &out,
        &[
            "alert-dialog-content",
            "alert-dialog-overlay",
            "alert-dialog-header",
            "alert-dialog-title",
            "alert-dialog-description",
            "alert-dialog-footer",
            "alert-dialog-action",
            "alert-dialog-cancel",
        ],
        "确认框",
    );
    assert!(out.contains(r#"role="alertdialog""#));
    assert!(out.contains("删除会话？"));
    // 夹具里把待删除项设为 c2 → 标题应当被插进描述里
    assert!(out.contains("Rust 移植进度"), "确认框没有带出会话标题");
    // Action 是 destructive、Cancel 默认 outline
    assert!(out.contains(r#"data-slot="alert-dialog-action""#));
}

// ---------------------------------------------------------------------------
// 样式表
// ---------------------------------------------------------------------------

#[test]
fn stylesheet_is_embedded() {
    let css = zcode_app::APP_CSS;
    assert!(css.len() > 50_000);
    assert!(css.contains("--color-background:"));
    // 内嵌 CSS 不能带 `<style>` 标签（document::Style 会自己包）
    assert!(!css.contains("<style"));
}

#[test]
fn stylesheet_covers_the_custom_variants_we_rely_on() {
    let css = zcode_app::APP_CSS;
    // 这几条 @custom-variant 是「类名在、效果不在」的解药（见 PORTING-UI.md §3.4）。
    // 注意压缩后的属性选择器不带引号：[data-state=open] 而不是 [data-state="open"]。
    for needle in [
        "[data-state=open]",
        "[data-state=closed]",
        "[data-state=checked]",
        "[data-state=unchecked]",
        "[data-state=active]",
        "[data-orientation=vertical]",
        "[data-orientation=horizontal]",
        "[data-highlighted]",
        "[data-disabled]",
        "[data-inset]",
        ".dark",
    ] {
        assert!(css.contains(needle), "样式表缺少变体 `{needle}`");
    }
}

#[test]
fn stylesheet_contains_the_repo_design_tokens() {
    let css = zcode_app::APP_CSS;
    // 这些令牌只存在于上游 styles.css 的 @theme 块里（已 vendored 到
    // .tailwind-build/tokens.css），说明样式表确实是从上游令牌派生的，
    // 而不是手抄的近似值。
    //
    // 注意 Tailwind v4 会 **按需裁剪** `@theme`：只有被实际用到的令牌才会
    // 作为 CSS 变量输出。所以这里只能断言「移植后的代码用到的那几个」——
    // 例如 `--text-ui-2xs` 目前只有未移植的 WorkflowTimelineLedge 在用，
    // 它就不会出现，这不代表 @theme 没被导入。
    for token in [
        "--text-ui-xl:",
        "--text-ui-base:",
        "--color-menu:",
        "--color-tooltip:",
        "--color-foreground-subtlest:",
        "--color-input-border-focused:",
    ] {
        assert!(css.contains(token), "样式表缺少令牌 `{token}`");
    }

    // 上面是「点」的检查，这里补一条「面」的：整块 @theme 确实进来了。
    // 仓库的 @theme 定义了两百多个色板令牌，只导入手抄的几十个不可能到这个量级。
    let color_vars = css.matches("--color-").count();
    assert!(
        color_vars > 200,
        "色板令牌只有 {color_vars} 个，@theme 块可能没被完整导入"
    );
}

// ---------------------------------------------------------------------------
// 真实工作区数据（「变更」页与 Tab 切换）
// ---------------------------------------------------------------------------
//
// 这一组刻意**不跑 git**：这里测的是「拿到数据之后渲染对不对」。
// 「git 到底能不能跑出这些数据」由 `crates/zcode-services/tests/git_workspace.rs`
// 在真实临时仓库上覆盖 —— 两层分开，失败时能立刻定位到是哪一层的问题。

/// 造一份固定的工作区数据。
fn workspace_fixture(case: &str) -> (Option<WorkspaceData>, WorkspaceStatus) {
    use zcode_services::git::changes::GitChangeSummary;
    use zcode_shared::git::{
        GitChangeKind, GitChangeSectionId, GitFileChange, GitHeadRefType, GitRepositorySummary,
    };

    let summary = |branch: &str, is_repository: bool| GitRepositorySummary {
        workspace_path: "F:/demo/workspace".to_string(),
        repo_root: "F:/demo/workspace".to_string(),
        workspace_in_repo_path: ".".to_string(),
        auto_refresh_watch_paths: Vec::new(),
        branch_name: Some(branch.to_string()),
        tracking_branch_name: Some(format!("origin/{branch}")),
        head_ref_type: GitHeadRefType::Branch,
        ahead: 2,
        behind: 1,
        is_dirty: true,
        is_git_available: true,
        is_repository,
    };

    let change = |path: &str, section: GitChangeSectionId, added: u32, removed: u32| GitFileChange {
        path: format!("F:/demo/workspace/{path}"),
        repo_relative_path: path.to_string(),
        workspace_relative_path: path.to_string(),
        x: Some("M".to_string()),
        y: Some(".".to_string()),
        kind: GitChangeKind::Modified,
        section,
        added,
        removed,
        is_staged: section == GitChangeSectionId::Staged,
        is_untracked: section == GitChangeSectionId::Untracked,
        is_conflicted: section == GitChangeSectionId::Conflicted,
    };

    let totals = |changes: &[GitFileChange]| GitChangeSummary {
        file_count: changes.len(),
        added: changes.iter().map(|change| change.added).sum(),
        removed: changes.iter().map(|change| change.removed).sum(),
    };

    // 一个「本地领先上游两个文件」的分支比较。
    //
    // 刻意**不叫** `git_status` / `status` —— 它是 `WorkspaceData` 的一个字段，
    // 与 `summary()` 里的 `ahead` / `behind` 是两回事：后者数的是提交数，
    // 这里列的是文件级差异。
    let branch_comparison = || GitBranchComparison {
        base_ref: Some("origin/main".to_string()),
        head_ref: Some("rust-only".to_string()),
        comparison_label: Some("rust-only -> origin/main".to_string()),
        changes: vec![
            GitBranchComparisonChange {
                path: "F:/demo/workspace/src/ahead.rs".to_string(),
                repo_relative_path: "src/ahead.rs".to_string(),
                workspace_relative_path: "src/ahead.rs".to_string(),
                original_path: None,
                kind: GitChangeKind::Modified,
                added: 5,
                removed: 2,
            },
            GitBranchComparisonChange {
                path: "F:/demo/workspace/src/renamed.rs".to_string(),
                repo_relative_path: "src/renamed.rs".to_string(),
                workspace_relative_path: "src/renamed.rs".to_string(),
                original_path: Some("src/old.rs".to_string()),
                kind: GitChangeKind::Renamed,
                added: 0,
                removed: 0,
            },
        ],
    };

    match case {
        "changes" => {
            let changes = vec![
                change("src/staged.rs", GitChangeSectionId::Staged, 12, 3),
                change("src/dirty.rs", GitChangeSectionId::Unstaged, 1, 1),
                change("src/new.rs", GitChangeSectionId::Untracked, 7, 0),
                change("src/conflict.rs", GitChangeSectionId::Conflicted, 0, 0),
            ];
            let totals = totals(&changes);
            (
                Some(WorkspaceData {
                    summary: summary("rust-only", true),
                    changes,
                    totals,
                    // 有上游 —— 「与上游比较」分组会跟着一起渲染。
                    branch_comparison: branch_comparison(),
                }),
                WorkspaceStatus::Ready,
            )
        }
        "clean" => (
            Some(WorkspaceData {
                summary: summary("main", true),
                changes: Vec::new(),
                totals: GitChangeSummary::default(),
                // 「干净」只说明没有未提交改动，不代表和上游一致。
                // 这里故意给一个非空的分支比较：工作区干净但本地领先上游时，
                // 这个分组**必须**照样出现，否则用户会以为「没东西可推」。
                branch_comparison: branch_comparison(),
            }),
            WorkspaceStatus::Ready,
        ),
        "clean-no-upstream" => (
            Some(WorkspaceData {
                summary: summary("main", true),
                changes: Vec::new(),
                totals: GitChangeSummary::default(),
                // 没有上游 ⇒ `base_ref` 是 `None` ⇒ 整个分组不渲染。
                // 这是与上面 `"clean"` 配对的反例：两边工作区一样干净，
                // 差别只在「有没有可比较的基准」。
                branch_comparison: GitBranchComparison::default(),
            }),
            WorkspaceStatus::Ready,
        ),
        "not-repo" => (
            Some(WorkspaceData {
                summary: summary("main", false),
                changes: Vec::new(),
                totals: GitChangeSummary::default(),
                branch_comparison: GitBranchComparison::default(),
            }),
            WorkspaceStatus::Ready,
        ),
        "failed" => (
            None,
            WorkspaceStatus::Failed("fatal: not a git repository".to_string()),
        ),
        "loading" => (None, WorkspaceStatus::Loading),
        other => panic!("未知的夹具：{other}"),
    }
}

/// 注入 AppCtx + 一个不会真跑 git 的请求端，然后渲染主区域。
#[component]
fn WorkspaceHarness(
    case: &'static str,
    #[props(default = "changes")] tab: &'static str,
    /// 注入一条写操作反馈（`None` = 没有反馈）。
    #[props(default)] notice: Option<WorkspaceNotice>,
    /// 注入一个已展开的 diff 面板（`None` = 没有展开项）。
    #[props(default)] diff: Option<DiffPreviewState>,
    /// 预先塞进批注 store 的一条批注（`None` = 没有批注）。
    #[props(default)] comment: Option<CodeCommentPreviewEntry>,
    /// 上面那条批注挂在哪个文件上（决定它进哪个桶）。
    ///
    /// 默认 `src/dirty.rs`（未暂存）；测提交范围裁剪时要换成 `src/staged.rs`
    /// 或别的路径，才能构造出「裁剪前 = 裁剪后」与「裁剪后为空」两种情形。
    #[props(default = "src/dirty.rs")] comment_path: &'static str,
) -> Element {
    let channel = use_hook(zcode_app::workspace::create_workspace_channel);
    let requester = channel.requester();
    use_context_provider(move || requester);

    // 批注上下文和 `AppShell` 里提供的保持一致（`AppCtx` 必须保持 `Copy`，
    // 所以它单独走一个 context）。
    let mut comment_previews = use_signal(CodeCommentPreviewStore::new);
    let comment_draft = use_signal(|| None::<(CodeCommentRange, String)>);
    let comment_removals = use_signal(CodeCommentRemovalRegistry::new);
    use_hook(move || {
        if let Some(comment) = comment {
            comment_previews.write().add_comment(
                &CodeCommentPreviewBucket {
                    // 与 `workspace_fixture` 里的 path 保持一致 —— 否则批注
                    // 会被分到另一个桶里，面板上一条也看不到。
                    workspace_path: "F:/demo/workspace".to_string(),
                    // 视图层用**分支名**当 identity（见 `workspace_identity_of`），
                    // fixture 的分支是 `rust-only`，这里必须对齐。
                    workspace_identity: Some("rust-only".to_string()),
                    source_path: comment_path.to_string(),
                },
                comment,
            );
        }
    });
    use_context_provider(move || zcode_app::workspace::CodeCommentCtx {
        previews: comment_previews,
        draft: comment_draft,
        removals: comment_removals,
    });

    let (data, status) = workspace_fixture(case);
    let ctx = AppCtx {
        state: use_signal(demo_state),
        settings_open: use_signal(|| false),
        pending_delete: use_signal(|| None::<&'static str>),
        delete_open: use_signal(|| false),
        settings_tab: use_signal(|| "general".to_string()),
        detail_open: use_signal(|| false),
        gallery_open: use_signal(|| false),
        workspace_tab: use_signal(move || tab.to_string()),
        plan_open: use_signal(|| true),
        approval_value: use_signal(|| "ask".to_string()),
        workspace: use_signal(move || data),
        workspace_status: use_signal(move || status),
        workspace_notice: use_signal(move || notice),
        diff_preview: use_signal(move || diff),
    };
    use_context_provider(|| ctx);

    rsx! { zcode_app::views::WorkspacePane {} }
}

#[component]
fn ChangesFixture() -> Element {
    rsx! { WorkspaceHarness { case: "changes" } }
}

#[component]
fn CleanFixture() -> Element {
    rsx! { WorkspaceHarness { case: "clean" } }
}

#[component]
fn NotRepoFixture() -> Element {
    rsx! { WorkspaceHarness { case: "not-repo" } }
}

#[component]
fn FailedFixture() -> Element {
    rsx! { WorkspaceHarness { case: "failed" } }
}

#[component]
fn LoadingFixture() -> Element {
    rsx! { WorkspaceHarness { case: "loading" } }
}

#[component]
fn TasksTabFixture() -> Element {
    rsx! { WorkspaceHarness { case: "clean", tab: "tasks" } }
}

#[component]
fn ConversationTabFixture() -> Element {
    rsx! { WorkspaceHarness { case: "clean", tab: "conversation" } }
}

/// 工作区干净、**没有**上游 —— 「与上游比较」分组不该出现。
#[component]
fn CleanNoUpstreamFixture() -> Element {
    rsx! { WorkspaceHarness { case: "clean-no-upstream" } }
}

#[test]
fn changes_view_lists_real_change_sections() {
    let out = html(ChangesFixture);

    // 分组标题：顺序与 `populated_sections()` 一致（冲突最前）
    for needle in ["冲突", "已暂存", "未暂存", "未跟踪"] {
        assert!(out.contains(needle), "变更页缺少分组 `{needle}`");
    }
    // 真实文件名（来自夹具，不是硬编码在视图里的）
    for needle in ["src/staged.rs", "src/dirty.rs", "src/new.rs", "src/conflict.rs"] {
        assert!(out.contains(needle), "变更页缺少文件 `{needle}`");
    }
    // 行数来自统计表
    assert!(out.contains("+12"), "缺少暂存区新增行数");
    assert!(out.contains("-3"), "缺少暂存区删除行数");
    // 汇总：4 个文件，+20/-4
    assert!(out.contains("4 个文件"), "缺少文件数汇总");
    assert!(out.contains("+20"), "缺少总新增行数");
    assert!(out.contains("-4"), "缺少总删除行数");
    // 顶栏的真实分支与 ahead/behind
    assert!(out.contains("rust-only"), "缺少真实分支名");
    assert!(out.contains("↑2 ↓1"), "缺少 ahead/behind");
}

#[test]
fn changes_view_shows_clean_state() {
    let out = html(CleanFixture);
    assert!(out.contains("工作区干净"), "干净工作区应当有明确文案");
    // 干净不等于「什么都不显示」——分支信息仍要在
    assert!(out.contains("main"));
}

#[test]
fn branch_comparison_group_renders_as_readonly() {
    let out = html(ChangesFixture);

    // 分组标题与 `comparisonLabel`（上游 `branch` 数据集那个字段）
    assert!(out.contains("与上游比较"), "缺少「与上游比较」分组");
    assert!(
        out.contains("rust-only -&gt; origin/main") || out.contains("rust-only -> origin/main"),
        "缺少 comparisonLabel，实际没有把 base/head 带出来"
    );
    // 分组内的文件
    assert!(out.contains("src/ahead.rs"), "缺少分支比较里的文件");
    // 重命名行要显示成 `旧 → 新`，否则用户看到一个没见过的文件名，
    // 会以为它是新加的文件
    assert!(
        out.contains("src/old.rs → src/renamed.rs"),
        "重命名行没有显示成 `旧 → 新`"
    );

    // 只读的含义：这一分组里**不能**出现写操作按钮。
    // 两侧都是提交，`stage` / `discard` 没有意义可言。
    let group = branch_comparison_section(&out);
    for forbidden in ["暂存", "取消暂存", "丢弃"] {
        assert!(
            !group.contains(forbidden),
            "分支比较是只读的，不该出现 `{forbidden}` 按钮"
        );
    }
    assert!(
        group.contains("data-slot=\"change-row\""),
        "只读分组里的行仍应可点开看 diff"
    );
}

/// 从整页 HTML 里裁出「与上游比较」那一段，供「不该出现什么」类断言使用。
///
/// 用整页去断言 `!contains("丢弃")` 是无效的 —— 工作区那几组里本来就有
/// 「丢弃」按钮，无论只读分组对不对，断言都会通过。
fn branch_comparison_section(out: &str) -> String {
    const MARKER: &str = "data-slot=\"branch-comparison-section\"";
    let start = out.find(MARKER).expect("页面里应当有分支比较分组");
    let rest = &out[start + MARKER.len()..];
    // 分组是同级里的最后一个；截到「拿不到闭合标签」为止即可，
    // 只要把后面的内容排除掉就够做出「这一段里有没有 X」的判断。
    let end = rest
        .find("</section>")
        .or_else(|| rest.find("data-slot=\"not"))
        .unwrap_or(rest.len());
    rest[..end].to_string()
}

#[test]
fn branch_comparison_group_is_absent_without_an_upstream() {
    // `base_ref == None` 是「整个分组不该渲染」的信号，与
    // 「有基准但没差异」是两回事 —— 后者要渲染一个空分组说明「和上游一致」，
    // 前者渲染出来只会让人误以为「已经同步」。
    let out = html(CleanNoUpstreamFixture);
    assert!(
        !out.contains("与上游比较"),
        "没有上游时不该渲染分支比较分组"
    );
    // 但工作区的「干净」状态本身照常呈现
    assert!(out.contains("工作区干净"));
}

#[test]
fn a_clean_workspace_can_still_be_ahead_of_upstream() {
    // 「干净」只说明没有未提交改动。本地领先上游时，
    // 分支比较分组**必须**照常出现，否则用户会以为「没东西可推」。
    let out = html(CleanFixture);
    assert!(out.contains("工作区干净"));
    assert!(
        out.contains("与上游比较"),
        "工作区干净但领先上游时，分支比较分组必须照样渲染"
    );
    assert!(out.contains("src/ahead.rs"));
}

#[test]
fn changes_view_shows_not_a_repository_state() {
    let out = html(NotRepoFixture);
    assert!(out.contains("当前目录不是 Git 仓库"));
    // 非仓库是正常状态，不该被渲染成错误
    assert!(!out.contains("读取工作区失败"));
}

#[test]
fn changes_view_surfaces_failures_instead_of_going_blank() {
    // 这一条是这一轮的核心教训：失败必须看得见。
    // 之前的「只有壳子」正是「出错了但界面什么都不说」的后果。
    let out = html(FailedFixture);
    assert!(out.contains("读取工作区失败"), "失败必须显式呈现");
    assert!(
        out.contains("fatal: not a git repository"),
        "失败原因必须带出来"
    );
}

#[test]
fn changes_view_shows_loading_state() {
    let out = html(LoadingFixture);
    assert!(out.contains("正在读取工作区"));
    // 加载态也要给出「为什么不会卡」的说明
    assert!(out.contains("后台线程"));
}

#[test]
fn workspace_tab_switches_the_main_pane() {
    // 顶栏的 Tab 点了必须真的换内容。之前「任务」Tab 点了没反应，
    // 用户看到的仍然是一个不动的壳子。
    let tasks = html(TasksTabFixture);
    assert!(tasks.contains("任务页尚未移植"));
    assert!(!tasks.contains("继续完成剩余移植工作"));

    let conversation = html(ConversationTabFixture);
    assert!(conversation.contains("继续完成剩余移植工作"));
    assert!(!conversation.contains("任务页尚未移植"));
}

// ---------------------------------------------------------------------------
// 写操作（暂存 / 取消暂存 / 丢弃 / 提交 / 推送）
// ---------------------------------------------------------------------------
//
// 这一组只断言**界面有没有给出正确的入口与反馈**，不断言 git 真的被调用了
// ——后者由 `crates/zcode-services/tests/git_workspace.rs` 在真实仓库上覆盖。
// 分层的理由：如果「按钮没渲染出来」和「git 命令拼错了」混在一个测试里，
// 失败信息分不清是哪一边。

#[component]
fn SuccessNoticeFixture() -> Element {
    rsx! {
        WorkspaceHarness {
            case: "changes",
            notice: Some(WorkspaceNotice::success("提交成功：1b79077")),
        }
    }
}

#[component]
fn ErrorNoticeFixture() -> Element {
    rsx! {
        WorkspaceHarness {
            case: "changes",
            notice: Some(WorkspaceNotice::error("暂存失败：fatal: pathspec did not match")),
        }
    }
}

#[component]
fn NotRepoNoticeFixture() -> Element {
    rsx! {
        WorkspaceHarness {
            case: "not-repo",
            notice: Some(WorkspaceNotice::error("推送失败：没有配置 remote")),
        }
    }
}

#[test]
fn changes_view_offers_stage_and_discard_per_row() {
    let out = html(ChangesFixture);

    // 未暂存 / 未跟踪 / 冲突三节给「暂存」，已暂存那节给「取消暂存」
    assert!(out.contains("暂存"), "缺少暂存入口");
    assert!(out.contains("取消暂存"), "缺少取消暂存入口");
    assert!(out.contains("丢弃改动"), "缺少丢弃入口（aria-label）");
    // 整组操作
    assert!(out.contains("全部暂存"), "缺少整组暂存");
    assert!(out.contains("全部丢弃"), "缺少整组丢弃");
}

#[test]
fn changes_view_offers_a_commit_box_and_push() {
    let out = html(ChangesFixture);

    assert!(out.contains("提交"), "缺少提交按钮");
    assert!(out.contains("推送"), "缺少推送按钮");
    // 已暂存文件数会写进占位符，用户才知道这次提交会带上什么
    assert!(
        out.contains("1 个已暂存文件"),
        "提交输入框应当说明有几个已暂存文件"
    );
}

#[test]
fn commit_box_is_absent_for_a_non_repository() {
    // 非仓库时不该出现提交入口 —— 点了必然失败，摆在那里只会误导
    let out = html(NotRepoFixture);
    assert!(!out.contains("先暂存一些改动再提交"));
    assert!(!out.contains("1 个已暂存文件"));
}

#[test]
fn commit_scope_hint_is_absent_without_any_session_files() {
    // 没有任何批注 ⇒ session_paths 为空 ⇒ 范围计算退化成「不裁剪」。
    // 此时若还渲染提示，就等于告诉用户「你的提交信息被裁剪了」，而事实上没有 ——
    // 这是会让人误删文件、误改提交范围的错误信息。
    let out = html(ChangesFixture);
    assert!(
        !out.contains("data-slot=\"commit-scope-hint\""),
        "没有会话批注时不该出现提交范围提示"
    );
    assert!(
        !out.contains("生成提交信息时只会带上这些"),
        "没有会话批注时不该出现裁剪文案"
    );
}

#[test]
fn commit_scope_hint_is_absent_when_the_scope_changes_nothing() {
    // 会话碰过的文件与已暂存列表**完全重合**（`src/staged.rs` 是唯一已暂存文件，
    // 也正是唯一批注过的文件）⇒ 裁剪前后个数相同 ⇒ 提示没有信息量，不该出现。
    //
    // 反过来说：如果这条断言挂了，说明判断条件退化成了「只要有会话文件就提示」，
    // 用户会在没有裁剪的时候被告知「被裁剪了」。
    let out = html(ScopedAllStagedFixture);
    assert!(
        !out.contains("data-slot=\"commit-scope-hint\""),
        "裁剪前后一致时不该出现提交范围提示"
    );
}

#[test]
fn commit_scope_hint_reports_how_many_staged_files_are_in_scope() {
    // 夹具里已暂存 1 个文件（`src/staged.rs`），但会话批注落在 `src/dirty.rs` 上
    // —— 一个未暂存文件。于是裁剪结果是**空集**：写提交信息时一个已暂存文件都不带。
    //
    // 这正是这个提示存在的理由：用户明明暂存了东西，却被告知「本次会话没碰过它们」，
    // 才会意识到自己看的不是将要提交的那批文件。
    let out = html(ScopedNoneStagedFixture);
    let hint = scoped_hint_section(&out);
    assert!(
        !hint.is_empty(),
        "已暂存文件全被裁掉时，必须出现提交范围提示，否则用户以为提交信息覆盖了全部改动"
    );
    assert!(
        hint.contains("data-scoped-count=\"0\""),
        "`data-scoped-count` 应为 0（一个已暂存文件都不在范围内），实际：{hint}"
    );
    assert!(
        hint.contains("data-staged-count=\"1\""),
        "`data-staged-count` 应为 1（真实已暂存文件数），实际：{hint}"
    );
    assert!(
        hint.contains("本次会话只碰过其中 0 个文件"),
        "提示文案必须带上真实数字，否则用户看不懂发生了什么"
    );
}

/// 从整页 HTML 里裁出提交范围提示那一段。
///
/// 与 [`branch_comparison_section`] 同理：直接对整页断言 `contains("0")` 是无效的
/// —— 页面上到处都有数字，断言必然通过。
fn scoped_hint_section(out: &str) -> String {
    const MARKER: &str = "data-slot=\"commit-scope-hint\"";
    let Some(start) = out.find(MARKER) else {
        return String::new();
    };
    let rest = &out[start..];
    // 提示是个单层 div，取到它自己闭合为止即可。
    let end = rest.find("</div>").map(|i| i + 6).unwrap_or(rest.len());
    rest[..end].to_string()
}

#[test]
fn discard_asks_for_confirmation_before_doing_anything() {
    // 「丢弃」是会丢数据的操作，默认必须是关闭的确认框，而不是直接执行。
    let out = html(ChangesFixture);
    assert!(
        !out.contains("alert-dialog-content"),
        "确认框默认必须关闭"
    );
    assert!(!out.contains("丢弃这些改动？"));
}

#[test]
fn success_notice_is_rendered_and_readable() {
    let out = html(SuccessNoticeFixture);
    assert!(out.contains("workspace-notice"), "成功提示应当渲染出来");
    assert!(out.contains("提交成功：1b79077"));
}

#[test]
fn failure_notice_is_rendered_with_the_reason() {
    // 这一条延续「失败必须看得见」：写操作失败不能悄悄吞掉。
    let out = html(ErrorNoticeFixture);
    assert!(out.contains("workspace-notice"));
    assert!(out.contains("暂存失败"));
    assert!(
        out.contains("pathspec did not match"),
        "失败原因必须原样带出来，否则用户没法自查"
    );
}

#[test]
fn notices_still_render_on_top_of_a_non_repository_state() {
    // 提示条挂在信息条下方，不受主体内容影响 ——
    // 否则「在非仓库目录点了推送」这类反馈会消失得无影无踪。
    let out = html(NotRepoNoticeFixture);
    assert!(out.contains("workspace-notice"));
    assert!(out.contains("推送失败：没有配置 remote"));
    assert!(out.contains("当前目录不是 Git 仓库"));
}

// ---------------------------------------------------------------------------
// diff 预览
// ---------------------------------------------------------------------------
//
// 同样只测渲染层：把 `DiffPreviewState` 直接注入 AppCtx，断言四种结果
// （加载中 / 有 patch / 二进制 / 不可用 / 命令失败）**都看得见**。
// 「diff 内容对不对」由 `crates/zcode-services/tests/git_workspace.rs` 在
// 真实仓库上覆盖。

/// 造一份 diff 结果。
fn diff_result(availability: GitDiffAvailability, patch: Option<&str>, summary: Option<&str>) -> GitDiffResult {
    GitDiffResult {
        path: "F:/demo/workspace/src/dirty.rs".to_string(),
        availability,
        patch: patch.map(str::to_string),
        before_content: None,
        after_content: None,
        summary: summary.map(str::to_string),
    }
}

/// 一个展开在 `src/dirty.rs` 那一行之下的 diff 面板。
#[component]
fn DiffFixture(
    state: DiffPreviewState,
    #[props(default)] comment: Option<CodeCommentPreviewEntry>,
    #[props(default = "src/dirty.rs")] comment_path: &'static str,
) -> Element {
    rsx! { WorkspaceHarness { case: "changes", diff: Some(state), comment, comment_path } }
}

/// 会话批注**恰好落在唯一的已暂存文件**上 ⇒ 裁剪前后个数一致 ⇒ 不该有提示。
#[component]
fn ScopedAllStagedFixture() -> Element {
    rsx! {
        WorkspaceHarness {
            case: "changes",
            comment_path: "src/staged.rs",
            comment: Some(CodeCommentPreviewEntry {
                id: "code-comment:1:src/staged.rs".to_string(),
                source_path: Some("src/staged.rs".to_string()),
                source_title: "staged.rs".to_string(),
                start_line: 1,
                end_line: 1,
                selected_text: "let a = 1;".to_string(),
                comment: "这条改动我确认过了".to_string(),
            }),
        }
    }
}

/// 会话批注落在**未暂存**文件上 ⇒ 已暂存文件全被裁掉 ⇒ 必须有提示，且数字为 0。
#[component]
fn ScopedNoneStagedFixture() -> Element {
    rsx! {
        WorkspaceHarness {
            case: "changes",
            comment_path: "src/dirty.rs",
            comment: Some(CodeCommentPreviewEntry {
                id: "code-comment:1:src/dirty.rs".to_string(),
                source_path: Some("src/dirty.rs".to_string()),
                source_title: "dirty.rs".to_string(),
                start_line: 1,
                end_line: 1,
                selected_text: "let a = 1;".to_string(),
                comment: "看看这一行".to_string(),
            }),
        }
    }
}

#[component]
fn DiffPatchFixture() -> Element {
    rsx! {
        DiffFixture {
            state: DiffPreviewState::Ready {
                id: 1,
                path: "src/dirty.rs".to_string(),
                diff: Box::new(diff_result(
                    GitDiffAvailability::Patch,
                    Some("--- a/src/dirty.rs\n+++ b/src/dirty.rs\n@@ -1 +1 @@\n-let a = 1;\n+let a = 2;\n"),
                    None,
                )),
            },
        }
    }
}

/// 一个已展开、且**第 1 行上已经有一条批注**的 patch 面板。
#[component]
fn DiffAnnotatedFixture() -> Element {
    rsx! {
        DiffFixture {
            comment: Some(CodeCommentPreviewEntry {
                id: "code-comment:1:src/dirty.rs".to_string(),
                source_path: Some("src/dirty.rs".to_string()),
                source_title: "dirty.rs".to_string(),
                // 批注锚在第一行（= 新增行 `let a = 2;` 所在的那一行）
                start_line: 1,
                end_line: 1,
                selected_text: "let a = 2;".to_string(),
                comment: "这里的常量名建议改成大写".to_string(),
            }),
            state: DiffPreviewState::Ready {
                id: 1,
                path: "src/dirty.rs".to_string(),
                diff: Box::new(diff_result(
                    GitDiffAvailability::Patch,
                    Some("--- a/src/dirty.rs\n+++ b/src/dirty.rs\n@@ -1 +1 @@\n-let a = 1;\n+let a = 2;\n"),
                    None,
                )),
            },
        }
    }
}

#[component]
fn DiffLoadingFixture() -> Element {
    rsx! {
        DiffFixture {
            state: DiffPreviewState::Loading {
                id: 1,
                path: "src/dirty.rs".to_string(),
            },
        }
    }
}

#[component]
fn DiffBinaryFixture() -> Element {
    rsx! {
        DiffFixture {
            state: DiffPreviewState::Ready {
                id: 1,
                path: "src/dirty.rs".to_string(),
                diff: Box::new(diff_result(
                    GitDiffAvailability::Binary,
                    None,
                    Some("Binary diff is not previewable."),
                )),
            },
        }
    }
}

#[component]
fn DiffTruncatedFixture() -> Element {
    rsx! {
        DiffFixture {
            state: DiffPreviewState::Ready {
                id: 1,
                path: "src/dirty.rs".to_string(),
                diff: Box::new(diff_result(
                    GitDiffAvailability::Truncated,
                    None,
                    Some("Git diff output exceeded the preview limit."),
                )),
            },
        }
    }
}

#[component]
fn DiffUnavailableFixture() -> Element {
    rsx! {
        DiffFixture {
            state: DiffPreviewState::Ready {
                id: 1,
                path: "src/dirty.rs".to_string(),
                diff: Box::new(diff_result(
                    GitDiffAvailability::Unavailable,
                    None,
                    Some("No Git diff is available for this file."),
                )),
            },
        }
    }
}

#[component]
fn DiffFailedFixture() -> Element {
    rsx! {
        DiffFixture {
            state: DiffPreviewState::Failed {
                id: 1,
                path: "src/dirty.rs".to_string(),
                message: "git 起不来".to_string(),
            },
        }
    }
}

#[test]
fn diff_preview_is_closed_by_default() {
    // 默认不该有任何一个面板是展开的 —— 否则一进页面就挂着一堆 diff。
    let out = html(ChangesFixture);
    assert!(
        !out.contains(r#"data-slot="diff-preview""#),
        "默认不应展开任何 diff 面板"
    );
    assert!(!out.contains(r#"data-expanded="true""#));
    // 但每一行都要带着「可展开」的锚点
    assert!(out.contains(r#"data-slot="change-row""#));
}

#[test]
fn diff_preview_renders_the_real_patch() {
    let out = html(DiffPatchFixture);
    assert!(out.contains(r#"data-slot="diff-preview""#), "面板没渲染");
    assert!(out.contains(r#"data-diff-path="src/dirty.rs""#));
    assert!(out.contains(r#"data-slot="diff-patch""#), "缺少 patch 容器");
    // 用的是**可批注**的 patch 渲染（不是纯展示的 `LightweightDiffPreview`）
    assert!(
        out.contains(r#"data-slot="annotatable-patch""#),
        "缺少可批注 patch 容器"
    );
    // patch 原文逐行渲染出来了。行首的协议标记（`+` / `-` / 空格）被
    // **取出来做样式**，所以正文里看到的是去掉标记之后的内容，
    // 而标记体现在 diff-removed / diff-added 类名上。
    assert!(out.contains("let a = 1;"), "缺少删除行内容");
    assert!(out.contains("let a = 2;"), "缺少新增行内容");
    assert!(
        out.contains("text-diff-removed") && out.contains("text-diff-added"),
        "行首标记应当被识别成删除/新增"
    );
    // 展开的那一行要标出来
    assert!(out.contains(r#"data-expanded="true""#));
}

#[test]
fn patch_lines_expose_file_line_numbers_for_annotation() {
    // 批注锚定的是**文件行号**，不是 patch 内的序号 —— 这里钉住这个坐标。
    //
    // fixture 的 patch 是 `@@ -1 +1 @@` 加一条删除、一条新增：
    // 删除行在改动后的文件里不存在，所以**没有**行号；新增行是第 1 行。
    let out = html(DiffPatchFixture);

    assert!(
        out.contains(r#"data-slot="patch-row""#),
        "每一行都要是可寻址的一行"
    );
    // 新增行拿到文件行号 1
    assert!(
        out.contains(r#"data-file-line="1""#),
        "新增行应当带上文件行号 1"
    );
    // 可批注的行才有「加批注」按钮
    assert!(
        out.contains(r#"data-slot="patch-comment-button""#),
        "缺少加批注入口"
    );
}

#[test]
fn patch_rows_without_a_file_line_are_not_annotatable() {
    // `@@` hunk 头与 `---` / `+++` 协议头在改动后的文件里没有对应行，
    // 它们**不能**被批注 —— 给它们编个行号会让批注指向不存在的位置。
    let out = html(DiffPatchFixture);
    let rows = out.matches(r#"data-slot="patch-row""#).count();
    let buttons = out.matches(r#"data-slot="patch-comment-button""#).count();

    assert!(rows > 0, "应该有行");
    assert!(
        buttons < rows,
        "并非每一行都可批注：{buttons} 个入口 / {rows} 行"
    );
}

#[test]
fn annotation_hint_is_shown_when_there_is_nothing_to_annotate() {
    // 没有任何批注时给一条提示，否则用户不知道入口在哪。
    let out = html(DiffPatchFixture);
    assert!(
        out.contains(r#"data-slot="annotation-hint""#),
        "没有批注时应当给出提示"
    );
    assert!(!out.contains("data-code-comment-id"), "此时不该有批注卡片");
}

#[test]
fn an_existing_annotation_is_rendered_next_to_its_line() {
    let out = html(DiffAnnotatedFixture);

    // 批注卡片本身
    assert!(
        out.contains(r#"data-code-comment-id="code-comment:1:src/dirty.rs""#),
        "批注卡片没渲染出来"
    );
    // 批注正文
    assert!(
        out.contains("这里的常量名建议改成大写"),
        "批注正文缺失"
    );
    // 已经是「有批注」的状态，就不该再出现「点击任意一行」的提示
    assert!(
        !out.contains(r#"data-slot="annotation-hint""#),
        "有批注时不该再提示怎么加批注"
    );
}

#[test]
fn diff_preview_shows_a_loading_hint() {
    let out = html(DiffLoadingFixture);
    assert!(out.contains(r#"data-slot="diff-preview""#));
    assert!(out.contains("正在读取 diff"), "加载中必须有文案");
    // 加载中不该已经有 patch 容器
    assert!(!out.contains(r#"data-slot="diff-patch""#));
}

#[test]
fn diff_preview_explains_why_there_is_no_patch() {
    // 关键约束：拿不到 patch 的三种情况**不能是空白**，
    // 否则用户会以为界面坏了。
    let binary = html(DiffBinaryFixture);
    assert!(binary.contains(r#"data-slot="diff-unavailable""#));
    assert!(binary.contains(r#"data-diff-availability="binary""#));
    assert!(binary.contains("Binary diff is not previewable."));

    let truncated = html(DiffTruncatedFixture);
    assert!(truncated.contains(r#"data-diff-availability="truncated""#));
    assert!(truncated.contains("exceeded the preview limit"));

    let unavailable = html(DiffUnavailableFixture);
    assert!(unavailable.contains(r#"data-diff-availability="unavailable""#));
    assert!(unavailable.contains("No Git diff is available for this file."));
}

#[test]
fn diff_preview_surfaces_command_failures() {
    let out = html(DiffFailedFixture);
    assert!(out.contains("读取 diff 失败"), "失败必须显式呈现");
    assert!(out.contains("git 起不来"), "失败原因必须带出来");
}

#[test]
fn diff_preview_does_not_hide_the_rest_of_the_page() {
    // 展开一个面板不该让分组、汇总、写操作入口消失。
    let out = html(DiffPatchFixture);
    for needle in ["已暂存", "未暂存", "未跟踪", "冲突", "全部暂存", "推送"] {
        assert!(out.contains(needle), "展开 diff 后 `{needle}` 不见了");
    }
}

