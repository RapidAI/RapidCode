//! `toast` —— Rust 移植自 `packages/ui/src/components/ui/toast.tsx`
//!
//! TS 是一个「不引第三方库」的轻量提示：队列放在**模块级单例**里，
//! 用 `react-dom/client` 的 `createRoot` 把 `<ToastContainer />` 挂到 `body`，
//! 3 秒后自动消失。
//!
//! ## 关键移植点 1：模块级单例 → 纯状态机 + context
//!
//! TS 的队列之所以需要 `pendingToastIds` / `pendingToastUpdates` /
//! `dismissedBeforeMountToastIds` 三张簿记表，是因为 `toast()` 可能在
//! 「容器还没挂载」的窗口期被调用（`ensureHost` 同步建 DOM，但 `addToast`
//! 要等下一帧 React 渲染后才赋值），于是要用 `requestAnimationFrame` 重试，
//! 还要记录「挂载前就被取消」的 id。
//!
//! Rust 侧把这些整体去掉：
//!
//! - [`ToastStore`] 是**纯数据 + 纯方法**的队列，可以完全离线单测；
//! - [`ToastProvider`] 把 `Signal<ToastStore>` 放进 context，[`ToastHost`] 渲染它；
//! - 命令式 API（[`toast`] / [`update_toast`] / [`dismiss_toast`]）**显式接收队列句柄**。
//!
//! 队列的生存期由持有它的组件决定，因此不存在「宿主未挂载」的窗口期。
//!
//! ## 关键移植点 2：回调 → 事件回传 id
//!
//! TS 的 `ToastItem.onAction` 是一个闭包。Rust 侧 [`ToastItem`] 保持**纯数据**
//! （便于 `PartialEq` / 单测 / 跨组件传递），动作改成把 `id` 回传给
//! [`ToastHost::on_action`]。调用方按 id 分派。
//!
//! ## 关键移植点 3：计时器靠 `key` 重启
//!
//! TS 的 effect 依赖 `[item.durationMs]`，所以「进行中的 Toast 被更新成终态」
//! 会**重启**自动消失计时器（源码注释明确点了这条：持续中的 Toast 用
//! `duration=0`，切到终态时改 duration 以重新计时）。
//!
//! Rust 的 `use_future` 只跑一次，所以这里让 [`ToastHost`] 用
//! `key = "{id}-{duration_ms}"` 渲染 [`ToastMessage`] —— duration 一变，
//! Dioxus 就重建该组件，计时器随之重启。行为与 TS 等价（TS 也会再触发一次
//! 进入动画，因为 effect 重新执行了 `setVisible(true)`）。
//!
//! ## 与 TS 的差异
//!
//! 1. **没有 Portal**：TS 用 `createRoot` 把容器挂到 `document.body`；
//!    Rust 侧由调用方把 [`ToastHost`] 放在树里（通常在应用根部）。
//! 2. **锚点定位不做测量**：TS 用 `ResizeObserver` + `MutationObserver` +
//!    scroll/resize 监听实时算锚点中心。Rust 侧改为由调用方传入
//!    `anchor_left`（px）；传 `None` 时退回 `left-1/2`（即居中）。
//!    纯计算部分 [`resolve_toast_anchor_left`] 仍按 TS 逐字移植。
//! 3. **`duration_ms` 用 `f64`**：为了与 TS 的 `Number.isFinite` 判据一致
//!    （`0` / 负数 / `NaN` / `Infinity` 都不自动消失）。
//! 4. 文案拆分（首行当标题、其余当正文）与 TS 一致，见 [`toast_message_parts`]。

use crate::cn::cn;
use crate::icons::{IconInfo, IconTriangleAlert, IconX};
use dioxus::prelude::*;
use futures_timer::Delay;
use std::time::Duration;

/// 默认自动消失时长（TS `DEFAULT_TOAST_DURATION_MS`）。
pub const DEFAULT_TOAST_DURATION_MS: f64 = 3000.0;

/// 退出动画时长（TS `TOAST_TRANSITION_DURATION_MS`）。
pub const TOAST_TRANSITION_DURATION_MS: f64 = 200.0;

/// 提示出现的位置。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ToastPosition {
    #[default]
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
}

impl ToastPosition {
    pub fn as_str(&self) -> &'static str {
        match self {
            ToastPosition::TopCenter => "top-center",
            ToastPosition::TopRight => "top-right",
            ToastPosition::BottomLeft => "bottom-left",
            ToastPosition::BottomCenter => "bottom-center",
        }
    }

    /// 是否贴底（决定进入动画方向，见 [`toast_message_classes`]）。
    pub fn is_bottom(&self) -> bool {
        matches!(self, ToastPosition::BottomLeft | ToastPosition::BottomCenter)
    }

    /// 全部位置（渲染时按此顺序输出四个栈，与 TS 的 JSX 顺序一致）。
    pub const ALL: [ToastPosition; 4] = [
        ToastPosition::TopCenter,
        ToastPosition::TopRight,
        ToastPosition::BottomLeft,
        ToastPosition::BottomCenter,
    ];
}

/// 提示的视觉变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ToastVariant {
    #[default]
    Default,
    /// 把同一长耗时操作的阶段变化收敛到一个提示里（左下/右上常见）。
    Update,
    Info,
    Warning,
}

impl ToastVariant {
    pub fn as_str(&self) -> &'static str {
        match self {
            ToastVariant::Default => "default",
            ToastVariant::Update => "update",
            ToastVariant::Info => "info",
            ToastVariant::Warning => "warning",
        }
    }

    /// TS 的 `isNotice`：`info` / `warning` 共用一套版式。
    pub fn is_notice(&self) -> bool {
        matches!(self, ToastVariant::Info | ToastVariant::Warning)
    }
}

/// 一条提示（纯数据，无闭包）。
#[derive(Clone, PartialEq, Debug)]
pub struct ToastItem {
    pub id: u64,
    pub message: String,
    /// 自动消失时长（毫秒）；`<= 0` / 非有限值表示不自动消失。
    pub duration_ms: f64,
    pub position: ToastPosition,
    pub variant: ToastVariant,
    /// 动作按钮文案；`None` 表示没有动作。
    pub action_label: Option<String>,
    /// 是否显示关闭按钮（只对 `info` / `warning` 版式生效，同 TS）。
    pub dismissible: bool,
    pub dismiss_label: Option<String>,
    /// 锚定到某个 DOM id 的中心（只对 `top-center` 生效，同 TS）。
    pub anchor_id: Option<String>,
    /// 同一业务目标与动作的稳定键：新结果替换旧结果并移到栈底。
    pub dedupe_key: Option<String>,
}

/// 入队选项（对应 TS `ToastOptions`）。
#[derive(Clone, PartialEq, Debug)]
pub struct ToastOptions {
    pub duration_ms: f64,
    pub position: ToastPosition,
    pub variant: ToastVariant,
    pub action_label: Option<String>,
    pub dismissible: bool,
    pub dismiss_label: Option<String>,
    pub anchor_id: Option<String>,
    pub dedupe_key: Option<String>,
}

impl Default for ToastOptions {
    fn default() -> Self {
        Self {
            duration_ms: DEFAULT_TOAST_DURATION_MS,
            position: ToastPosition::TopCenter,
            variant: ToastVariant::Default,
            action_label: None,
            dismissible: false,
            dismiss_label: None,
            anchor_id: None,
            dedupe_key: None,
        }
    }
}

impl ToastOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn duration_ms(mut self, ms: f64) -> Self {
        self.duration_ms = ms;
        self
    }

    pub fn position(mut self, p: ToastPosition) -> Self {
        self.position = p;
        self
    }

    pub fn variant(mut self, v: ToastVariant) -> Self {
        self.variant = v;
        self
    }

    pub fn action_label(mut self, label: impl Into<String>) -> Self {
        self.action_label = Some(label.into());
        self
    }

    pub fn dismissible(mut self, yes: bool) -> Self {
        self.dismissible = yes;
        self
    }

    pub fn dismiss_label(mut self, label: impl Into<String>) -> Self {
        self.dismiss_label = Some(label.into());
        self
    }

    pub fn anchor_id(mut self, id: impl Into<String>) -> Self {
        self.anchor_id = Some(id.into());
        self
    }

    pub fn dedupe_key(mut self, key: impl Into<String>) -> Self {
        self.dedupe_key = Some(key.into());
        self
    }
}

/// 部分更新（对应 TS `ToastUpdate` = `Partial<Omit<ToastItem, "id">>`）。
///
/// `id` 与 `message` 之外都能改；`None` 表示「本次不动这个字段」。
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ToastUpdate {
    pub message: Option<String>,
    pub duration_ms: Option<f64>,
    pub position: Option<ToastPosition>,
    pub variant: Option<ToastVariant>,
    /// 设成 `Some(None)` 才能清掉动作按钮 —— 普通 `Option` 表达不了「清空」，
    /// 所以这里用双层 `Option` 的语义：外层 `None` = 不改。
    pub action_label: Option<Option<String>>,
    pub dismissible: Option<bool>,
    pub dismiss_label: Option<Option<String>>,
    pub anchor_id: Option<Option<String>>,
    pub dedupe_key: Option<Option<String>>,
}

impl ToastUpdate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn message(mut self, m: impl Into<String>) -> Self {
        self.message = Some(m.into());
        self
    }

    pub fn duration_ms(mut self, ms: f64) -> Self {
        self.duration_ms = Some(ms);
        self
    }

    pub fn position(mut self, p: ToastPosition) -> Self {
        self.position = Some(p);
        self
    }

    pub fn variant(mut self, v: ToastVariant) -> Self {
        self.variant = Some(v);
        self
    }

    pub fn action_label(mut self, label: impl Into<String>) -> Self {
        self.action_label = Some(Some(label.into()));
        self
    }

    /// 清掉动作按钮。
    pub fn clear_action_label(mut self) -> Self {
        self.action_label = Some(None);
        self
    }

    /// 把补丁写进一条提示。
    pub fn apply(&self, item: &mut ToastItem) {
        if let Some(v) = &self.message {
            item.message = v.clone();
        }
        if let Some(v) = self.duration_ms {
            item.duration_ms = v;
        }
        if let Some(v) = self.position {
            item.position = v;
        }
        if let Some(v) = self.variant {
            item.variant = v;
        }
        if let Some(v) = &self.action_label {
            item.action_label = v.clone();
        }
        if let Some(v) = self.dismissible {
            item.dismissible = v;
        }
        if let Some(v) = &self.dismiss_label {
            item.dismiss_label = v.clone();
        }
        if let Some(v) = &self.anchor_id {
            item.anchor_id = v.clone();
        }
        if let Some(v) = &self.dedupe_key {
            item.dedupe_key = v.clone();
        }
    }
}

/// 入队：有 `dedupe_key` 时先摘掉同键的旧项再追加（旧项被替换并移到栈底）。
///
/// 逐字移植 TS `upsertToastItem`。
pub fn upsert_toast_item(mut items: Vec<ToastItem>, item: ToastItem) -> Vec<ToastItem> {
    match &item.dedupe_key {
        None => items.push(item),
        Some(key) => {
            items.retain(|current| current.dedupe_key.as_deref() != Some(key.as_str()));
            items.push(item);
        }
    }
    items
}

/// 贴底时的内边距（TS 里的 `bottomInset`）。
pub const TOAST_BOTTOM_INSET: &str = "bottom-[calc(1rem+env(safe-area-inset-bottom))]";

/// 栈容器的定位类名（逐字移植 TS `resolveToastStackClassName`）。
pub fn resolve_toast_stack_classes(position: ToastPosition) -> String {
    match position {
        ToastPosition::TopCenter => {
            "fixed top-16 left-1/2 z-[9999] flex -translate-x-1/2 flex-col items-center gap-2"
                .to_string()
        }
        ToastPosition::TopRight => {
            "fixed right-4 top-16 z-[9999] flex max-w-[calc(100vw-2rem)] flex-col items-end gap-2"
                .to_string()
        }
        ToastPosition::BottomCenter => format!(
            "fixed {TOAST_BOTTOM_INSET} left-1/2 z-[9999] flex -translate-x-1/2 flex-col items-center gap-2"
        ),
        ToastPosition::BottomLeft => {
            format!("fixed {TOAST_BOTTOM_INSET} left-4 z-[9999] flex flex-col items-start gap-2")
        }
    }
}

/// 锚点中心 = `left + width / 2`（逐字移植 TS `resolveToastAnchorLeft`）。
pub fn resolve_toast_anchor_left(left: f64, width: f64) -> f64 {
    left + width / 2.0
}

/// 把 `message` 拆成「首行标题」+「其余正文」。
///
/// 逐字移植 TS：`const [title, ...bodyLines] = message.split("\n")`，
/// 正文再 `join("\n").trim()`。首行**不做** trim（TS 也没做）。
pub fn toast_message_parts(message: &str) -> (String, String) {
    let mut lines = message.split('\n');
    let title = lines.next().unwrap_or_default().to_string();
    let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    (title, body)
}

/// 提示卡片的基础类名（TS 原文，逐字保留）。
pub const TOAST_MESSAGE_BASE: &str = "rounded-2xl border bg-toast/60 text-ui-base shadow-lg backdrop-blur-xl transition-[transform,opacity] duration-200 ease-[cubic-bezier(0.77,0,0.175,1)] motion-reduce:transform-none motion-reduce:transition-opacity";

/// `update` 变体的类名（TS 原文，逐字保留）。
pub const TOAST_MESSAGE_UPDATE: &str = "origin-bottom-left w-[min(300px,calc(100vw-1rem))] max-w-[min(300px,calc(100vw-1rem))] border-popover-border text-foreground shadow-lg";

/// `info` / `warning` 变体的类名（TS 原文，逐字保留）。
pub const TOAST_MESSAGE_NOTICE: &str =
    "w-[min(536px,calc(100vw-2rem))] border-border text-foreground";

/// `default` 变体的类名（TS 原文，逐字保留）。
pub const TOAST_MESSAGE_DEFAULT: &str =
    "border-border px-4 py-3 text-foreground whitespace-pre-line";

/// `update` 版式的一行（TS 原文，逐字保留）。
pub const TOAST_UPDATE_ROW: &str =
    "grid min-w-0 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-2 px-2.5 py-2";

/// `update` 版式左侧的小圆点（TS 原文，逐字保留）。
pub const TOAST_UPDATE_DOT: &str = "size-2 shrink-0 rounded-full bg-primary/80 ring-4 ring-accent";

/// `update` 版式的动作按钮（TS 原文，逐字保留）。
pub const TOAST_UPDATE_ACTION: &str = "h-7 max-w-14 shrink-0 truncate rounded-md bg-secondary px-2 text-ui-base font-medium text-foreground transition-colors hover:bg-hover";

/// `info` / `warning` 版式的一行（TS 原文，逐字保留）。
pub const TOAST_NOTICE_ROW: &str = "flex min-w-0 items-center gap-4 px-4";

/// `info` / `warning` 版式的动作按钮（TS 原文，逐字保留）。
pub const TOAST_NOTICE_ACTION: &str = "max-w-1/2 self-center shrink-0 whitespace-normal break-words text-left font-medium leading-snug text-foreground underline underline-offset-2 hover:text-foreground-subtle";

/// `info` / `warning` 版式的关闭按钮（TS 原文，逐字保留）。
pub const TOAST_NOTICE_DISMISS: &str = "flex size-6 shrink-0 items-center justify-center rounded-md text-foreground-subtle hover:bg-hover hover:text-foreground";

/// 提示卡片的完整类名。
///
/// 逐字移植 TS `ToastMessageView` 里那段嵌套三元：
///
/// 1. 版式：`update` → `update` 类名；否则 `info`/`warning` → notice 类名；
///    否则 default 类名。**`update` 优先于 notice**（TS 的嵌套顺序如此）。
/// 2. 进入/退出动画按「位置 + 可见性」选：
///    - `top-right` 走水平方向（TS 注释点名：曾复用纵向缩放，表达不出从右侧进入）；
///    - 贴底走 `translate-y-1`，其余走 `-translate-y-1`。
pub fn toast_message_classes(
    variant: ToastVariant,
    position: ToastPosition,
    visible: bool,
    is_bottom: bool,
) -> String {
    let layout = if variant == ToastVariant::Update {
        TOAST_MESSAGE_UPDATE
    } else if variant.is_notice() {
        TOAST_MESSAGE_NOTICE
    } else {
        TOAST_MESSAGE_DEFAULT
    };

    let motion = if position == ToastPosition::TopRight {
        if visible {
            "translate-x-0 opacity-100"
        } else {
            "translate-x-[calc(100%+1rem)] opacity-0"
        }
    } else if is_bottom {
        if visible {
            "translate-y-0 scale-100 opacity-100"
        } else {
            "translate-y-1 scale-[0.98] opacity-0"
        }
    } else if visible {
        "translate-y-0 scale-100 opacity-100"
    } else {
        "-translate-y-1 scale-[0.98] opacity-0"
    };

    cn(&[TOAST_MESSAGE_BASE, layout, motion])
}

/// 队列（纯状态机）。所有方法都是纯函数式的「读 + 改」，无需渲染器。
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ToastStore {
    items: Vec<ToastItem>,
    next_id: u64,
}

impl ToastStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn items(&self) -> &[ToastItem] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, id: u64) -> Option<&ToastItem> {
        self.items.iter().find(|i| i.id == id)
    }

    /// 分配 id 并入队（含 `dedupe_key` 替换）。返回新 id。
    pub fn push(&mut self, message: impl Into<String>, options: ToastOptions) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let item = ToastItem {
            id,
            message: message.into(),
            duration_ms: options.duration_ms,
            position: options.position,
            variant: options.variant,
            action_label: options.action_label,
            dismissible: options.dismissible,
            dismiss_label: options.dismiss_label,
            anchor_id: options.anchor_id,
            dedupe_key: options.dedupe_key,
        };
        self.items = upsert_toast_item(std::mem::take(&mut self.items), item);
        id
    }

    /// 打补丁。返回是否命中（未命中说明该 id 已消失）。
    pub fn update(&mut self, id: u64, patch: &ToastUpdate) -> bool {
        match self.items.iter_mut().find(|i| i.id == id) {
            Some(item) => {
                patch.apply(item);
                true
            }
            None => false,
        }
    }

    /// 移除。返回是否命中。
    pub fn dismiss(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|i| i.id != id);
        self.items.len() != before
    }

    /// 取出某个位置的提示。
    ///
    /// 注意 `TopCenter` **排除**带 `anchor_id` 的项 —— 它们由
    /// [`ToastStore::anchored_groups`] 单独渲染（与 TS 的 filter 一致）。
    pub fn items_for(&self, position: ToastPosition) -> Vec<&ToastItem> {
        self.items
            .iter()
            .filter(|i| {
                i.position == position
                    && !(position == ToastPosition::TopCenter && i.anchor_id.is_some())
            })
            .collect()
    }

    /// 带锚点的 `top-center` 提示，按 `anchor_id` 首次出现顺序分组。
    pub fn anchored_groups(&self) -> Vec<(String, Vec<&ToastItem>)> {
        let mut groups: Vec<(String, Vec<&ToastItem>)> = Vec::new();
        for item in &self.items {
            if item.position != ToastPosition::TopCenter {
                continue;
            }
            let Some(anchor) = &item.anchor_id else {
                continue;
            };
            match groups.iter_mut().find(|(k, _)| k == anchor) {
                Some((_, bucket)) => bucket.push(item),
                None => groups.push((anchor.clone(), vec![item])),
            }
        }
        groups
    }
}

/// 命令式 API 需要的队列句柄。
#[derive(Clone, Copy)]
struct ToastContext {
    store: Signal<ToastStore>,
}

/// 读取当前上下文的队列。
///
/// 需要在 [`ToastProvider`] 之内调用。
pub fn use_toast_store() -> Signal<ToastStore> {
    use_context::<ToastContext>().store
}

/// 在应用根部提供队列（等价 TS 里 `ensureHost` 建的模块级单例）。
///
/// 传 `store` 可用外部持有的队列（便于在组件树之外入队、或在测试里预置数据）。
#[component]
pub fn ToastProvider(
    /// 受控队列；不传则内部新建。
    #[props(default)] store: Option<Signal<ToastStore>>,
    children: Element,
) -> Element {
    let internal = use_signal(ToastStore::new);
    let store = store.unwrap_or(internal);
    use_context_provider(|| ToastContext { store });
    rsx! { {children} }
}

/// 入队一条提示，返回 id（对应 TS `toast(message)`）。
pub fn toast(store: Signal<ToastStore>, message: impl Into<String>) -> u64 {
    toast_with(store, message, ToastOptions::default())
}

/// 带选项入队（对应 TS `toast(message, options)`）。
pub fn toast_with(
    mut store: Signal<ToastStore>,
    message: impl Into<String>,
    options: ToastOptions,
) -> u64 {
    let message = message.into();
    let mut id = 0;
    store.with_mut(|s| {
        id = s.push(message, options);
    });
    id
}

/// 更新已有提示（对应 TS `updateToast`）。
///
/// 把同一长耗时操作的阶段变化收敛到一个提示里时用这个。
pub fn update_toast(mut store: Signal<ToastStore>, id: u64, patch: ToastUpdate) {
    store.with_mut(|s| {
        s.update(id, &patch);
    });
}

/// 移除提示（对应 TS `dismissToast`）。
pub fn dismiss_toast(mut store: Signal<ToastStore>, id: u64) {
    store.with_mut(|s| {
        s.dismiss(id);
    });
}

/// 提示宿主：把四个位置的栈 + 所有锚点栈渲染出来。
///
/// 放在 [`ToastProvider`] 之内、应用内容旁边。
#[component]
pub fn ToastHost(
    /// 点动作按钮时回传该提示的 `id`（TS 是 per-item 闭包，见模块文档差异）。
    #[props(default)] on_action: EventHandler<u64>,
    /// 点关闭按钮时回传该提示的 `id`。
    #[props(default)] on_dismiss: EventHandler<u64>,
    /// 锚点中心 x（px）。`None` 时锚点栈退回居中。
    /// TS 用 ResizeObserver 实时测量，这里改为调用方提供。
    #[props(default)] anchor_left: Option<f64>,
) -> Element {
    let store = use_toast_store();
    let snapshot = (store)();
    let anchored = snapshot.anchored_groups();

    rsx! {
        for position in ToastPosition::ALL {
            div {
                key: "{position.as_str()}-stack",
                class: resolve_toast_stack_classes(position),
                "data-slot": "toast-stack",
                "data-position": position.as_str(),
                for item in snapshot.items_for(position) {
                    ToastMessage {
                        // key 里带 duration：duration 变化时重建组件 → 计时器重启
                        // （对应 TS effect 依赖 `[item.durationMs]`）
                        key: "{item.id}-{item.duration_ms}",
                        item: item.clone(),
                        is_bottom: position.is_bottom(),
                        on_action,
                        on_dismiss,
                        on_done: move |id| dismiss_toast(store, id),
                    }
                }
            }
        }
        for (anchor_id, items) in anchored {
            AnchoredToastStack {
                key: "anchor-{anchor_id}",
                anchor_id: anchor_id.clone(),
                items: items.into_iter().cloned().collect::<Vec<_>>(),
                anchor_left,
                on_action,
                on_dismiss,
                on_done: move |id| dismiss_toast(store, id),
            }
        }
    }
}

/// 锚定到某个 DOM id 的 `top-center` 栈。
#[component]
pub fn AnchoredToastStack(
    /// 目标元素的 DOM id。
    #[props(default, into)] anchor_id: String,
    items: Vec<ToastItem>,
    /// 锚点中心 x（px）；`None` 时退回居中。
    #[props(default)] anchor_left: Option<f64>,
    #[props(default)] on_action: EventHandler<u64>,
    #[props(default)] on_dismiss: EventHandler<u64>,
    #[props(default)] on_done: EventHandler<u64>,
) -> Element {
    // `left-1/2` + `-translate-x-1/2` 是基类；有实测值时用内联 `left` 覆盖，
    // 并把位移还原（TS 同样是覆写 `left` 并保留 translate）。
    let style = match anchor_left {
        Some(left) => format!("left: {left}px;"),
        None => String::new(),
    };

    rsx! {
        div {
            class: resolve_toast_stack_classes(ToastPosition::TopCenter),
            style,
            "data-slot": "toast-stack",
            "data-position": "top-center",
            "data-anchor-id": anchor_id,
            for item in items {
                ToastMessage {
                    key: "{item.id}-{item.duration_ms}",
                    item: item.clone(),
                    is_bottom: false,
                    on_action,
                    on_dismiss,
                    on_done,
                }
            }
        }
    }
}

/// 单条提示：负责「进入动画 → 自动消失 → 等退出动画 → 通知移除」。
#[component]
pub fn ToastMessage(
    item: ToastItem,
    #[props(default)] is_bottom: bool,
    #[props(default)] on_action: EventHandler<u64>,
    #[props(default)] on_dismiss: EventHandler<u64>,
    /// 退出动画结束后回调（宿主据此出队）。
    #[props(default)] on_done: EventHandler<u64>,
) -> Element {
    let mut visible = use_signal(|| false);
    let id = item.id;
    let duration_ms = item.duration_ms;

    use_future(move || async move {
        // 对应 TS 的 `requestAnimationFrame(() => setVisible(true))`：
        // 先以「未进入」状态挂一帧，进入动画才有起点。
        Delay::new(Duration::from_millis(0)).await;
        visible.set(true);

        // TS：`!Number.isFinite(durationMs) || durationMs <= 0` → 不自动消失
        if !duration_ms.is_finite() || duration_ms <= 0.0 {
            return;
        }

        Delay::new(Duration::from_millis(duration_ms as u64)).await;
        visible.set(false);
        // 等退出动画结束再出队
        Delay::new(Duration::from_millis(
            TOAST_TRANSITION_DURATION_MS as u64
        ))
        .await;
        on_done.call(id);
    });

    let (title, body) = toast_message_parts(&item.message);

    rsx! {
        ToastMessageView {
            item,
            visible: visible(),
            is_bottom,
            title: Some(title),
            body: Some(body),
            on_action: move |_| on_action.call(id),
            on_dismiss: move |_| on_dismiss.call(id),
        }
    }
}

/// 纯展示层：只按传入的 `visible` / 版式渲染，不碰计时器。
///
/// 拆出这一层的好处是它可以在**没有渲染器**的情况下被 SSR 快照测试覆盖。
#[component]
pub fn ToastMessageView(
    item: ToastItem,
    visible: bool,
    #[props(default)] is_bottom: bool,
    /// 覆盖标题；`None` 时从 `item.message` 拆。
    #[props(default)] title: Option<String>,
    /// 覆盖正文；`None` 时从 `item.message` 拆。
    #[props(default)] body: Option<String>,
    #[props(default)] on_action: EventHandler<MouseEvent>,
    #[props(default)] on_dismiss: EventHandler<MouseEvent>,
) -> Element {
    let is_update = item.variant == ToastVariant::Update;
    let is_notice = item.variant.is_notice();

    let (fallback_title, fallback_body) = toast_message_parts(&item.message);
    let display_title = title.unwrap_or(fallback_title);
    let display_body = body.unwrap_or(fallback_body);

    let action_label = item.action_label.clone();
    let dismiss_label = item
        .dismiss_label
        .clone()
        .unwrap_or_else(|| "Close".to_string());

    rsx! {
        div {
            class: toast_message_classes(item.variant, item.position, visible, is_bottom),
            "data-slot": "toast",
            "data-variant": item.variant.as_str(),
            "data-position": item.position.as_str(),
            "data-state": if visible { "open" } else { "closed" },
            role: "status",
            "aria-live": "polite",
            if is_update {
                div {
                    class: TOAST_UPDATE_ROW,
                    span {
                        class: TOAST_UPDATE_DOT,
                        "aria-hidden": "true",
                    }
                    div { class: "min-w-0 flex-1",
                        div {
                            class: "truncate text-ui-base font-medium text-foreground",
                            {display_title.clone()}
                        }
                        if !display_body.is_empty() {
                            div {
                                class: "mt-0.5 truncate text-ui-base leading-4 text-foreground-subtle",
                                {display_body.clone()}
                            }
                        }
                    }
                    if let Some(label) = action_label.clone() {
                        button {
                            r#type: "button",
                            class: TOAST_UPDATE_ACTION,
                            onclick: move |e| on_action.call(e),
                            {label}
                        }
                    }
                }
            } else if is_notice {
                // 注释点名：固定最小高度会在 12px 内容 padding 之外继续补高，
                // 产生额外上下留白 —— 所以这里不设 min-h。
                div {
                    class: TOAST_NOTICE_ROW,
                    if item.variant == ToastVariant::Warning {
                        IconTriangleAlert {
                            class: "size-4 shrink-0 text-warning",
                            aria_hidden: true,
                        }
                    } else {
                        IconInfo {
                            class: "size-4 shrink-0 text-foreground-subtle",
                            aria_hidden: true,
                        }
                    }
                    div {
                        class: "flex min-w-0 flex-[1_0_0] items-start gap-4 py-3 text-sm",
                        div { class: "min-w-0 flex-1 leading-5",
                            div { class: "text-foreground", {display_title.clone()} }
                            if !display_body.is_empty() {
                                div {
                                    class: "mt-0.5 whitespace-pre-line text-foreground-subtle",
                                    {display_body.clone()}
                                }
                            }
                        }
                        // 注释点名：固定 18px 行高会在大字号下压缩操作文案，
                        // 所以按钮不设固定行高，改用 leading-snug。
                        if let Some(label) = action_label.clone() {
                            button {
                                r#type: "button",
                                class: TOAST_NOTICE_ACTION,
                                onclick: move |e| on_action.call(e),
                                {label}
                            }
                        }
                    }
                    if item.dismissible {
                        button {
                            r#type: "button",
                            class: TOAST_NOTICE_DISMISS,
                            "aria-label": dismiss_label,
                            onclick: move |e| on_dismiss.call(e),
                            IconX { class: "size-3.5", aria_hidden: true }
                        }
                    }
                }
            } else {
                {item.message.clone()}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: u64, message: &str) -> ToastItem {
        ToastItem {
            id,
            message: message.to_string(),
            duration_ms: DEFAULT_TOAST_DURATION_MS,
            position: ToastPosition::TopCenter,
            variant: ToastVariant::Default,
            action_label: None,
            dismissible: false,
            dismiss_label: None,
            anchor_id: None,
            dedupe_key: None,
        }
    }

    #[test]
    fn position_and_variant_strings() {
        assert_eq!(ToastPosition::TopCenter.as_str(), "top-center");
        assert_eq!(ToastPosition::TopRight.as_str(), "top-right");
        assert_eq!(ToastPosition::BottomLeft.as_str(), "bottom-left");
        assert_eq!(ToastPosition::BottomCenter.as_str(), "bottom-center");

        assert_eq!(ToastVariant::Default.as_str(), "default");
        assert_eq!(ToastVariant::Update.as_str(), "update");
        assert_eq!(ToastVariant::Info.as_str(), "info");
        assert_eq!(ToastVariant::Warning.as_str(), "warning");

        assert_eq!(ToastPosition::default(), ToastPosition::TopCenter);
        assert_eq!(ToastVariant::default(), ToastVariant::Default);
    }

    #[test]
    fn only_notice_variants_are_notices() {
        assert!(ToastVariant::Info.is_notice());
        assert!(ToastVariant::Warning.is_notice());
        assert!(!ToastVariant::Default.is_notice());
        // update 不是 notice —— 它有自己的版式，且优先级更高
        assert!(!ToastVariant::Update.is_notice());
    }

    #[test]
    fn bottom_positions_are_flagged() {
        assert!(ToastPosition::BottomLeft.is_bottom());
        assert!(ToastPosition::BottomCenter.is_bottom());
        assert!(!ToastPosition::TopCenter.is_bottom());
        assert!(!ToastPosition::TopRight.is_bottom());
    }

    /// TS 的 JSX 顺序就是 top-center → top-right → bottom-left → bottom-center。
    #[test]
    fn stack_render_order_matches_ts() {
        assert_eq!(
            ToastPosition::ALL,
            [
                ToastPosition::TopCenter,
                ToastPosition::TopRight,
                ToastPosition::BottomLeft,
                ToastPosition::BottomCenter,
            ]
        );
    }

    #[test]
    fn upsert_without_key_appends() {
        let items = vec![item(0, "a"), item(1, "b")];
        let out = upsert_toast_item(items, item(2, "c"));
        assert_eq!(out.len(), 3);
        assert_eq!(out[2].id, 2);
    }

    /// 有 dedupeKey 时：旧项被摘掉，新项落到**栈底**。
    #[test]
    fn upsert_with_key_replaces_and_moves_to_bottom() {
        let mut a = item(0, "old");
        a.dedupe_key = Some("share".into());
        let mut b = item(1, "other");
        b.dedupe_key = Some("other".into());
        let mut c = item(2, "new");
        c.dedupe_key = Some("share".into());

        let out = upsert_toast_item(vec![a, b], c);
        assert_eq!(out.len(), 2);
        // 顺序：other, new（share 的旧项已消失，新项在末尾）
        assert_eq!(out[0].id, 1);
        assert_eq!(out[1].id, 2);
    }

    #[test]
    fn upsert_with_key_removes_all_duplicates() {
        let mut a = item(0, "x");
        a.dedupe_key = Some("k".into());
        let mut b = item(1, "y");
        b.dedupe_key = Some("k".into());
        let mut c = item(2, "z");
        c.dedupe_key = Some("k".into());

        let out = upsert_toast_item(vec![a, b], c);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, 2);
    }

    #[test]
    fn store_allocates_increasing_ids() {
        let mut s = ToastStore::new();
        assert!(s.is_empty());
        let a = s.push("one", ToastOptions::default());
        let b = s.push("two", ToastOptions::default());
        assert_eq!((a, b), (0, 1));
        assert_eq!(s.len(), 2);
        assert_eq!(s.get(a).unwrap().message, "one");
        assert!(s.get(99).is_none());
    }

    #[test]
    fn store_options_defaults_match_ts() {
        let mut s = ToastStore::new();
        let id = s.push("hi", ToastOptions::default());
        let it = s.get(id).unwrap();
        assert_eq!(it.duration_ms, 3000.0);
        assert_eq!(it.position, ToastPosition::TopCenter);
        assert_eq!(it.variant, ToastVariant::Default);
        assert!(!it.dismissible);
        assert!(it.action_label.is_none());
        assert!(it.anchor_id.is_none());
    }

    #[test]
    fn store_update_applies_patch_and_reports_miss() {
        let mut s = ToastStore::new();
        let id = s.push("start", ToastOptions::default());

        assert!(s.update(
            id,
            &ToastUpdate::new()
                .message("done")
                .duration_ms(0.0)
                .variant(ToastVariant::Update)
                .action_label("Undo")
        ));
        let it = s.get(id).unwrap();
        assert_eq!(it.message, "done");
        assert_eq!(it.duration_ms, 0.0);
        assert_eq!(it.variant, ToastVariant::Update);
        assert_eq!(it.action_label.as_deref(), Some("Undo"));

        // 未命中的 id
        assert!(!s.update(1234, &ToastUpdate::new().message("nope")));
    }

    /// 双层 `Option` 才能区分「不改」与「清空」。
    #[test]
    fn update_can_clear_optional_fields() {
        let mut s = ToastStore::new();
        let id = s.push("m", ToastOptions::new().action_label("Go").dedupe_key("k"));
        assert_eq!(s.get(id).unwrap().action_label.as_deref(), Some("Go"));

        // 空补丁不动任何字段
        s.update(id, &ToastUpdate::new());
        assert_eq!(s.get(id).unwrap().action_label.as_deref(), Some("Go"));
        assert_eq!(s.get(id).unwrap().dedupe_key.as_deref(), Some("k"));

        // 显式清空
        s.update(id, &ToastUpdate::new().clear_action_label());
        assert!(s.get(id).unwrap().action_label.is_none());
        assert_eq!(s.get(id).unwrap().dedupe_key.as_deref(), Some("k"));
    }

    #[test]
    fn store_dismiss_reports_hit() {
        let mut s = ToastStore::new();
        let id = s.push("m", ToastOptions::default());
        assert!(s.dismiss(id));
        assert!(s.is_empty());
        assert!(!s.dismiss(id));
    }

    /// `top-center` 会排除带锚点的项（TS 的 filter 条件）。
    #[test]
    fn items_for_excludes_anchored_top_center() {
        let mut s = ToastStore::new();
        s.push("plain", ToastOptions::default());
        s.push(
            "anchored",
            ToastOptions::new().anchor_id("composer").duration_ms(0.0),
        );

        let top = s.items_for(ToastPosition::TopCenter);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].message, "plain");

        let groups = s.anchored_groups();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, "composer");
        assert_eq!(groups[0].1.len(), 1);
    }

    /// 非 `top-center` 位置上的 `anchor_id` 会被忽略（TS 的 `if` 只认 top-center）。
    #[test]
    fn anchored_groups_ignore_non_top_center() {
        let mut s = ToastStore::new();
        s.push(
            "bottom",
            ToastOptions::new()
                .position(ToastPosition::BottomLeft)
                .anchor_id("x"),
        );
        assert!(s.anchored_groups().is_empty());
        assert_eq!(s.items_for(ToastPosition::BottomLeft).len(), 1);
    }

    #[test]
    fn anchored_groups_keep_first_appearance_order() {
        let mut s = ToastStore::new();
        s.push("a1", ToastOptions::new().anchor_id("A"));
        s.push("b1", ToastOptions::new().anchor_id("B"));
        s.push("a2", ToastOptions::new().anchor_id("A"));

        let groups = s.anchored_groups();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, "A");
        assert_eq!(groups[0].1.len(), 2);
        assert_eq!(groups[1].0, "B");
        assert_eq!(groups[1].1.len(), 1);
    }

    #[test]
    fn stack_classes_are_position_specific() {
        let tc = resolve_toast_stack_classes(ToastPosition::TopCenter);
        assert!(tc.contains("fixed"));
        assert!(tc.contains("top-16"));
        assert!(tc.contains("left-1/2"));
        assert!(tc.contains("-translate-x-1/2"));
        assert!(tc.contains("items-center"));
        assert!(tc.contains("z-[9999]"));

        let tr = resolve_toast_stack_classes(ToastPosition::TopRight);
        assert!(tr.contains("right-4"));
        assert!(tr.contains("items-end"));
        assert!(tr.contains("max-w-[calc(100vw-2rem)]"));
        assert!(!tr.contains("left-1/2"));

        let bl = resolve_toast_stack_classes(ToastPosition::BottomLeft);
        assert!(bl.contains(TOAST_BOTTOM_INSET));
        assert!(bl.contains("left-4"));
        assert!(bl.contains("items-start"));

        let bc = resolve_toast_stack_classes(ToastPosition::BottomCenter);
        assert!(bc.contains(TOAST_BOTTOM_INSET));
        assert!(bc.contains("left-1/2"));
        assert!(bc.contains("items-center"));
    }

    #[test]
    fn anchor_left_is_the_center_of_the_rect() {
        assert_eq!(resolve_toast_anchor_left(100.0, 40.0), 120.0);
        assert_eq!(resolve_toast_anchor_left(0.0, 0.0), 0.0);
        assert_eq!(resolve_toast_anchor_left(10.5, 3.0), 12.0);
    }

    #[test]
    fn message_splits_into_title_and_body() {
        assert_eq!(
            toast_message_parts("Saved"),
            ("Saved".to_string(), String::new())
        );
        assert_eq!(
            toast_message_parts("Saved\n3 files changed"),
            ("Saved".to_string(), "3 files changed".to_string())
        );
        // 多行正文用 \n 重连
        assert_eq!(
            toast_message_parts("Title\nline1\nline2"),
            ("Title".to_string(), "line1\nline2".to_string())
        );
        // 正文整体 trim
        assert_eq!(
            toast_message_parts("Title\n\n  spaced  \n"),
            ("Title".to_string(), "spaced".to_string())
        );
        // 首行不 trim（TS 也没做）
        assert_eq!(
            toast_message_parts("  Title  \nbody"),
            ("  Title  ".to_string(), "body".to_string())
        );
        // 空串
        assert_eq!(toast_message_parts(""), (String::new(), String::new()));
        // 只有换行：标题是空串，正文也是空串
        assert_eq!(toast_message_parts("\n"), (String::new(), String::new()));
    }

    #[test]
    fn message_classes_pick_layout_by_variant() {
        let upd = toast_message_classes(
            ToastVariant::Update,
            ToastPosition::TopCenter,
            true,
            false,
        );
        assert!(upd.contains("origin-bottom-left"));
        assert!(upd.contains("w-[min(300px,calc(100vw-1rem))]"));
        assert!(upd.contains("border-popover-border"));

        let warn = toast_message_classes(
            ToastVariant::Warning,
            ToastPosition::TopCenter,
            true,
            false,
        );
        assert!(warn.contains("w-[min(536px,calc(100vw-2rem))]"));
        assert!(warn.contains("border-border"));
        assert!(!warn.contains("origin-bottom-left"));

        let def = toast_message_classes(
            ToastVariant::Default,
            ToastPosition::TopCenter,
            true,
            false,
        );
        assert!(def.contains("px-4 py-3"));
        assert!(def.contains("whitespace-pre-line"));

        // 三者共享同一套基础类名
        for c in [&upd, &warn, &def] {
            assert!(c.contains("rounded-2xl"));
            assert!(c.contains("bg-toast/60"));
            assert!(c.contains("backdrop-blur-xl"));
            assert!(c.contains("motion-reduce:transform-none"));
        }
    }

    /// `update` 版式优先于 notice 版式（TS 的嵌套三元顺序）。
    #[test]
    fn update_layout_wins_over_notice() {
        // 一个变体不可能同时是两者，但顺序错了就会在 info/warning 上走 update 分支
        let info = toast_message_classes(
            ToastVariant::Info,
            ToastPosition::TopCenter,
            true,
            false,
        );
        assert!(!info.contains("origin-bottom-left"));
        assert!(info.contains("w-[min(536px,calc(100vw-2rem))]"));
    }

    #[test]
    fn top_right_animates_horizontally() {
        let shown = toast_message_classes(
            ToastVariant::Default,
            ToastPosition::TopRight,
            true,
            false,
        );
        assert!(shown.contains("translate-x-0 opacity-100"));
        assert!(!shown.contains("scale-100"));

        let hidden = toast_message_classes(
            ToastVariant::Default,
            ToastPosition::TopRight,
            false,
            false,
        );
        assert!(hidden.contains("translate-x-[calc(100%+1rem)] opacity-0"));
        // 不应落到纵向路径
        assert!(!hidden.contains("-translate-y-1"));
        assert!(!hidden.contains("translate-y-1"));
    }

    #[test]
    fn bottom_and_top_animate_in_opposite_y_directions() {
        let top_hidden = toast_message_classes(
            ToastVariant::Default,
            ToastPosition::TopCenter,
            false,
            false,
        );
        assert!(top_hidden.contains("-translate-y-1"));
        assert!(top_hidden.contains("scale-[0.98]"));
        assert!(top_hidden.contains("opacity-0"));

        let bottom_hidden = toast_message_classes(
            ToastVariant::Default,
            ToastPosition::BottomCenter,
            false,
            true,
        );
        assert!(bottom_hidden.contains("translate-y-1"));
        assert!(!bottom_hidden.contains("-translate-y-1"));

        // 可见时两者一致
        for pos in [ToastPosition::TopCenter, ToastPosition::BottomCenter] {
            let c = toast_message_classes(
                ToastVariant::Default,
                pos,
                true,
                pos.is_bottom(),
            );
            assert!(c.contains("translate-y-0 scale-100 opacity-100"));
        }
    }

    /// `is_bottom` 是调用方传的，不看 position —— 与 TS 的 `isBottom` prop 一致。
    #[test]
    fn is_bottom_is_driven_by_the_prop_not_the_position() {
        let c = toast_message_classes(
            ToastVariant::Default,
            ToastPosition::TopCenter,
            false,
            true,
        );
        assert!(c.contains("translate-y-1"));
        assert!(!c.contains("-translate-y-1"));
    }

    #[test]
    fn options_builder_matches_ts_field_names() {
        let o = ToastOptions::new()
            .duration_ms(0.0)
            .position(ToastPosition::BottomLeft)
            .variant(ToastVariant::Info)
            .action_label("Retry")
            .dismissible(true)
            .dismiss_label("Dismiss")
            .anchor_id("composer")
            .dedupe_key("save");

        assert_eq!(o.duration_ms, 0.0);
        assert_eq!(o.position, ToastPosition::BottomLeft);
        assert_eq!(o.variant, ToastVariant::Info);
        assert_eq!(o.action_label.as_deref(), Some("Retry"));
        assert!(o.dismissible);
        assert_eq!(o.dismiss_label.as_deref(), Some("Dismiss"));
        assert_eq!(o.anchor_id.as_deref(), Some("composer"));
        assert_eq!(o.dedupe_key.as_deref(), Some("save"));
    }
}
