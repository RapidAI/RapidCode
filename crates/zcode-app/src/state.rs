//! 应用状态与演示数据。
//!
//! 状态集中放在一个 [`Signal<AppState>`] 里（等价 React 的 `useReducer` + 单一
//! store），跨组件的浮层开合这类「UI 瞬态」则各自用独立的 `Signal`，避免
//! 每次开合都让整棵组件树重算。
//!
//! 演示数据刻意贴近 ZCode 的真实形态（会话列表 + 消息流 + 工具调用 + 变更统计），
//! 这样每一个被移植的原语都能在真实语境里被看到，而不是孤立地摆一排。

use dioxus::prelude::*;

use crate::workspace::{DiffPreviewState, WorkspaceData, WorkspaceNotice, WorkspaceStatus};

/// 会话列表里的一条会话。
#[derive(Clone, PartialEq, Debug)]
pub struct Conversation {
    pub id: &'static str,
    pub title: &'static str,
    pub preview: &'static str,
    pub updated: &'static str,
    /// 分组标题（今天 / 昨天 / 更早）。
    pub group: &'static str,
    pub pinned: bool,
    pub unread: u32,
    /// 该会话涉及的文件变更（`+新增` / `-删除`）。
    pub added: u32,
    pub removed: u32,
}

/// 消息角色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    User,
    Assistant,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Role::User => "你",
            Role::Assistant => "ZCode",
        }
    }
}

/// 一条消息。
#[derive(Clone, PartialEq, Debug)]
pub struct Message {
    pub role: Role,
    pub text: &'static str,
    /// 助手消息附带的工具调用说明。
    pub tool: Option<ToolCall>,
}

/// 工具调用卡片。
#[derive(Clone, PartialEq, Debug)]
pub struct ToolCall {
    pub name: &'static str,
    pub target: &'static str,
    pub state: &'static str,
    pub added: u32,
    pub removed: u32,
}

/// 可切换的模型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Model {
    Pro,
    Air,
    Local,
}

impl Model {
    pub fn as_str(&self) -> &'static str {
        match self {
            Model::Pro => "zcode-pro",
            Model::Air => "zcode-air",
            Model::Local => "local/qwen3-coder",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Model::Pro => "ZCode Pro",
            Model::Air => "ZCode Air",
            Model::Local => "本地模型",
        }
    }

    pub fn hint(&self) -> &'static str {
        match self {
            Model::Pro => "深度推理 · 200K 上下文",
            Model::Air => "响应更快 · 适合轻量改动",
            Model::Local => "离线运行 · 不联网",
        }
    }

    pub const ALL: [Model; 3] = [Model::Pro, Model::Air, Model::Local];
}

/// 权限模式（设置页里的单选组）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApprovalMode {
    Ask,
    Auto,
    ReadOnly,
}

impl ApprovalMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApprovalMode::Ask => "ask",
            ApprovalMode::Auto => "auto",
            ApprovalMode::ReadOnly => "readonly",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            ApprovalMode::Ask => "每次询问",
            ApprovalMode::Auto => "自动执行",
            ApprovalMode::ReadOnly => "只读",
        }
    }

    pub fn hint(&self) -> &'static str {
        match self {
            ApprovalMode::Ask => "每个工具调用都需要确认",
            ApprovalMode::Auto => "在白名单目录内直接执行",
            ApprovalMode::ReadOnly => "禁止写入与执行命令",
        }
    }

    pub const ALL: [ApprovalMode; 3] = [
        ApprovalMode::Ask,
        ApprovalMode::Auto,
        ApprovalMode::ReadOnly,
    ];
}

/// 应用主状态。
#[derive(Clone, PartialEq, Debug)]
pub struct AppState {
    pub conversations: Vec<Conversation>,
    pub active_id: &'static str,
    pub messages: Vec<Message>,
    pub draft: String,
    /// 最近一次「发给助手」的**完整正文**（含 `# Code comments:` 批注块）。
    ///
    /// 单独留一份是因为输入框里的 `draft` 发完就清空了，而批注是**附加**上去的
    /// —— 用户看不到这一份就等于「静默改写了我要发的东西」。留档 + 界面上
    /// 展示，才能让「发出去的内容」和「我写的内容」的差异是可审计的。
    pub last_prompt: String,
    pub model: Model,
    pub approval: ApprovalMode,
    pub search: String,
    /// 全局开关。
    pub dark: bool,
    pub streaming: bool,
    /// 详情面板里的开关项。
    pub show_reasoning: bool,
    pub auto_context: bool,
    pub telemetry: bool,
    /// 变更统计里的进度（用于 Progress 组件演示）。
    pub token_used: f64,
    pub token_limit: f64,
}

impl AppState {
    /// 当前会话。
    pub fn active(&self) -> Option<&Conversation> {
        self.conversations.iter().find(|c| c.id == self.active_id)
    }

    /// 按搜索词过滤后的会话。
    pub fn filtered(&self) -> Vec<Conversation> {
        let q = self.search.trim().to_lowercase();
        if q.is_empty() {
            return self.conversations.clone();
        }
        self.conversations
            .iter()
            .filter(|c| {
                c.title.to_lowercase().contains(&q) || c.preview.to_lowercase().contains(&q)
            })
            .cloned()
            .collect()
    }

    /// 某个分组下的会话。
    pub fn in_group(&self, group: &str) -> Vec<Conversation> {
        self.filtered()
            .into_iter()
            .filter(|c| c.group == group)
            .collect()
    }

    pub fn total_added(&self) -> u32 {
        self.conversations.iter().map(|c| c.added).sum()
    }

    pub fn total_removed(&self) -> u32 {
        self.conversations.iter().map(|c| c.removed).sum()
    }
}

/// 会话列表分组顺序。
pub const GROUPS: [&str; 3] = ["今天", "昨天", "更早"];

/// 跨组件共享的状态句柄集合。
///
/// `Signal<T>` 是句柄（generational box），标识跨渲染稳定，所以可以安全地塞进
/// context —— 这一点在 [`zcode_ui`] 的 `Tabs` / `Collapsible` 里已经反复验证过。
///
/// **必须保持 `Copy`**：视图里到处是 `move |_| { ctx.state ... }` 这类闭包，
/// `Copy` 让它们按字段复制而不是把整个 ctx move 走。因此这里只放 `Signal`；
/// 需要携带 `Arc` 的东西（例如工作区的请求端）走**另一个** context，见
/// [`crate::workspace::WorkspaceRequester`]。
#[derive(Clone, Copy)]
pub struct AppCtx {
    pub state: Signal<AppState>,
    /// 设置对话框开合。
    pub settings_open: Signal<bool>,
    /// 待删除的会话 id（`None` 表示没有待确认的删除）。
    pub pending_delete: Signal<Option<&'static str>>,
    /// 删除确认框的开合。与 [`AppCtx::pending_delete`] 分开持有，是因为
    /// `AlertDialog` 需要的是 `Signal<bool>`，而 `Signal<Option<T>>` 无法直接当
    /// `Signal<bool>` 用（`Signal` 不是协变的，也没有内置的映射组合子）。
    pub delete_open: Signal<bool>,
    /// 设置对话框当前 Tab。
    pub settings_tab: Signal<String>,
    /// 右侧详情面板开合。
    pub detail_open: Signal<bool>,
    /// 组件画廊开合。
    pub gallery_open: Signal<bool>,
    /// 顶栏的视图 Tab（`Tabs` 组件要求 `Signal<String>`，见 `zcode_ui::Tabs`）。
    pub workspace_tab: Signal<String>,
    /// 详情面板里「执行计划」折叠区的展开状态（受控用法示例）。
    pub plan_open: Signal<bool>,
    /// 权限模式的受控值。
    ///
    /// `RadioGroup` 是受控组件（见 `zcode_ui::radio_group`），要求外部持有唯一真相；
    /// 而 `AppState.approval` 是它的类型化投影。两处（详情面板、输入区）都读写
    /// 这个 signal，避免出现「两个控件各存一份、互相看不见」的分叉。
    pub approval_value: Signal<String>,
    /// **真实**工作区数据（来自 `zcode-services` 的 git 查询）。
    ///
    /// `None` 表示还没跑出结果 —— 与「跑出来是空的」是两件事，所以状态另放
    /// [`AppCtx::workspace_status`]。
    pub workspace: Signal<Option<WorkspaceData>>,
    /// 工作区数据的加载状态。
    pub workspace_status: Signal<WorkspaceStatus>,
    /// 最近一次写操作的反馈（暂存 / 提交 / 推送的成败）。
    ///
    /// 与 [`AppCtx::workspace_status`] 分开：那个说的是「工作区读不读得到」，
    /// 这个说的是「刚才那个动作成没成」。写操作失败时界面上的数据依然有效，
    /// 不该被一个整屏错误面板盖掉。
    pub workspace_notice: Signal<Option<WorkspaceNotice>>,
    /// 当前展开的那一条变更的 diff 预览。
    ///
    /// 一个信号同时表示「哪一行开着」和「开着的那行显示什么」（见
    /// [`crate::workspace::DiffPreviewState`]），因此不可能出现「展开了 A
    /// 却显示着 B 的内容」这种不同步。
    pub diff_preview: Signal<Option<DiffPreviewState>>,
}

impl AppCtx {
    /// 读取共享状态。
    pub fn read(&self) -> AppState {
        self.state.read().clone()
    }
}

/// 读取共享状态。
pub fn use_app() -> AppCtx {
    use_context::<AppCtx>()
}

/// 构造演示数据。
pub fn demo_state() -> AppState {
    AppState {
        conversations: vec![
            Conversation {
                id: "c1",
                title: "重构 RPC 传输层",
                preview: "把 createQueuePair 的投递方向修正过来，避免消息回环",
                updated: "10:24",
                group: "今天",
                pinned: true,
                unread: 0,
                added: 128,
                removed: 96,
            },
            Conversation {
                id: "c2",
                title: "Rust 移植进度",
                preview: "zcode-ui 第二批原语已通过 182 个测试",
                updated: "09:51",
                group: "今天",
                pinned: true,
                unread: 2,
                added: 412,
                removed: 7,
            },
            Conversation {
                id: "c3",
                title: "UI 主题令牌对齐",
                preview: "补齐 shadcn/tailwind.css 里的 @custom-variant 映射",
                updated: "昨天 21:08",
                group: "昨天",
                pinned: false,
                unread: 0,
                added: 63,
                removed: 12,
            },
            Conversation {
                id: "c4",
                title: "桌面窗口拖拽区",
                preview: "[app-region:no-drag] 需要覆盖右上角关闭按钮",
                updated: "昨天 16:33",
                group: "昨天",
                pinned: false,
                unread: 0,
                added: 24,
                removed: 9,
            },
            Conversation {
                id: "c5",
                title: "长会话内存排查",
                preview: "hidden 期间禁用动画，避免 DocumentTimeline 持有整棵子树",
                updated: "周三",
                group: "更早",
                pinned: false,
                unread: 0,
                added: 57,
                removed: 31,
            },
            Conversation {
                id: "c6",
                title: "Tailwind v4 扫描范围",
                preview: "@source 必须显式指向 UI 包源码，否则 Select 会半残",
                updated: "周二",
                group: "更早",
                pinned: false,
                unread: 0,
                added: 15,
                removed: 4,
            },
        ],
        active_id: "c2",
        messages: vec![
            Message {
                role: Role::User,
                text: "继续完成剩余移植工作，直到能正常运行。",
                tool: None,
            },
            Message {
                role: Role::Assistant,
                text: "明白。我先把「能正常运行」这条约束落到可执行的路径上：\
                       当前环境只有 x86_64-pc-windows-msvc 这一个 target，也没有 wasm-bindgen-cli，\
                       所以走 wasm/浏览器这条路是死路。原生桌面渲染器（wry + tao）在缓存里齐全，\
                       于是我新建了一个 zcode-app 二进制 crate，用 dioxus/desktop 起窗口。",
                tool: None,
            },
            Message {
                role: Role::Assistant,
                text: "接着把第二批原语补齐。无浮层的一批（Collapsible / Accordion / RadioGroup / \
                       ButtonGroup / InputGroup）直接照着 cva 的映射表写成枚举；\
                       有浮层的一批（Tooltip / HoverCard / Popover / DropdownMenu / Dialog / AlertDialog）\
                       没有 Portal 也没有 floating-ui，所以统一改成「锚点包裹元素 + absolute 定位」的\
                       纯 CSS 方案。",
                tool: Some(ToolCall {
                    name: "cargo test",
                    target: "crates/zcode-ui",
                    state: "871 passed",
                    added: 412,
                    removed: 7,
                }),
            },
            Message {
                role: Role::Assistant,
                text: "样式这块没有走 CDN：设计令牌是从上游 styles.css 的 @theme 块抽取后 \
                       vendored 到 .tailwind-build/tokens.css，再用 Tailwind v4 CLI 扫描 Rust \
                       源码生成一份 114 KB 的样式表内嵌进二进制。\
                       关键是补上 shadcn/tailwind.css 那几行 @custom-variant —— 少了它们，\
                       data-active / data-checked / data-open 这些类名就只是字符串。",
                tool: None,
            },
            Message {
                role: Role::User,
                text: "那现在跑起来是什么样？",
                tool: None,
            },
        ],
        draft: String::new(),
        last_prompt: String::new(),
        model: Model::Pro,
        approval: ApprovalMode::Ask,
        search: String::new(),
        dark: false,
        streaming: false,
        show_reasoning: true,
        auto_context: true,
        telemetry: false,
        token_used: 128_400.0,
        token_limit: 200_000.0,
    }
}
