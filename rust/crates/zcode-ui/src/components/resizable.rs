//! `Resizable` 家族 —— Rust 移植自
//! `packages/ui/src/components/ui/resizable.tsx`。
//!
//! TS 版本只是 `react-resizable-panels` 的一层薄封装：类名表 + localStorage
//! 持久化。真正有价值的信息全在那张类名表里，尤其是那条注释：
//!
//! > `react-resizable-panels` 的 `Separator` 不会暴露 `data-orientation`，
//! > 实际输出的是 `aria-orientation`，而且方向和 `PanelGroup` 相反。
//! > 之前写的 `data-[orientation=...]` 一直没有命中，所以 `w-px` / `h-px`
//! > 看起来「没生效」。
//!
//! 这条坑必须在移植里保住 —— 所以类名表用的是 `aria-[orientation=vertical]:`
//! 这种**属性变体**，而不是 `data-*`。
//!
//! ## 与 TS 的差异
//!
//! 1. **布局数学做成纯函数**。库里的 `flex: <size> 1 0px` 与 min/max 夹取、
//!    「拖一根分隔条要同时改左右两个面板且总和恒为 100」这套算术被抽成
//!    [`resize_pair`] / [`normalize_sizes`] / [`step_size`]，可单测、与渲染器无关。
//! 2. **持久化改成 trait**。TS 的 `useDefaultLayout` 直接读写 `localStorage`；
//!    Rust 侧定义 [`LayoutStore`]，默认实现是内存版，桌面/Web 调用方可以换成
//!    `localStorage` 后端。组件库本身不碰任何平台 API。
//! 3. **像素 → 百分比的换算由调用方给**。`clientX` 之类要配合容器实测尺寸才能
//!    换算成百分比，而 Dioxus 0.6 的指针事件拿不到元素尺寸（同
//!    [`crate::components::scroll_fade_viewport`] 的处境）。所以
//!    [`ResizablePanelGroup`] 收一个 `extent_px` prop（主轴方向的像素长度），
//!    键盘方向键那条路径不依赖它。

use crate::cn::cn;
use dioxus::prelude::*;

/// `PanelGroup` 基础类名（TS 原文）。
pub const RESIZABLE_PANEL_GROUP_BASE: &str = "flex h-full w-full";

/// `ResizableHandle` 基础类名（TS 原文，逐字保留）。
///
/// 注意 `aria-[orientation=...]` 这两组：它们是整个组件的关键，
/// 换成 `data-[orientation=...]` 就完全失效（见模块文档）。
pub const RESIZABLE_HANDLE_BASE: &str = "group/handle relative flex shrink-0 items-center justify-center bg-transparent outline-none transition-colors focus:outline-none focus-visible:outline-none focus-visible:bg-border-hover/60 focus-visible:ring-0 hover:bg-border-hover/60 data-[separator=hover]:bg-border-hover/60 data-[separator=active]:bg-border-hover z-10 aria-[orientation=vertical]:translate-x-px aria-[orientation=vertical]:my-6 aria-[orientation=vertical]:w-px aria-[orientation=vertical]:h-[calc(100%-48px)] aria-[orientation=vertical]:[mask-image:linear-gradient(to_bottom,transparent_0%,black_18%,black_82%,transparent_100%)] aria-[orientation=vertical]:[-webkit-mask-image:linear-gradient(to_bottom,transparent_0%,black_18%,black_82%,transparent_100%)] aria-[orientation=horizontal]:translate-y-px aria-[orientation=horizontal]:mx-6 aria-[orientation=horizontal]:h-px aria-[orientation=horizontal]:w-[calc(100%-48px)] aria-[orientation=horizontal]:[mask-image:linear-gradient(to_right,transparent_0%,black_18%,black_82%,transparent_100%)] aria-[orientation=horizontal]:[-webkit-mask-image:linear-gradient(to_right,transparent_0%,black_18%,black_82%,transparent_100%)]";

/// 面板的最小百分比尺寸（库默认 `minSize` 量级）。
pub const DEFAULT_MIN_SIZE: f64 = 10.0;

/// 键盘方向键的单步步长（百分比）。
pub const KEYBOARD_STEP: f64 = 5.0;

/// `PanelGroup` 的排布方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ResizableOrientation {
    /// 面板左右并排。
    #[default]
    Horizontal,
    /// 面板上下堆叠。
    Vertical,
}

impl ResizableOrientation {
    /// 写进 `data-panel-group-direction` 的取值。
    pub fn as_str(&self) -> &'static str {
        match self {
            ResizableOrientation::Horizontal => "horizontal",
            ResizableOrientation::Vertical => "vertical",
        }
    }

    /// 分隔条自身的朝向 —— **与组方向相反**（TS 注释里那条坑）。
    pub fn handle_orientation(&self) -> &'static str {
        match self {
            ResizableOrientation::Horizontal => "vertical",
            ResizableOrientation::Vertical => "horizontal",
        }
    }

    /// 排布方向对应的 `flex-direction`。
    ///
    /// TS 靠库写内联样式，这里用类名表达（等价，且能被 Tailwind 扫到）。
    pub fn flex_direction_class(&self) -> &'static str {
        match self {
            ResizableOrientation::Horizontal => "flex-row",
            ResizableOrientation::Vertical => "flex-col",
        }
    }
}

/// 拼装 `PanelGroup` 类名。
pub fn resizable_panel_group_classes(orientation: ResizableOrientation, class: &str) -> String {
    cn(&[
        RESIZABLE_PANEL_GROUP_BASE,
        orientation.flex_direction_class(),
        class,
    ])
}

/// 拼装 `ResizableHandle` 类名。
pub fn resizable_handle_classes(class: &str) -> String {
    cn(&[RESIZABLE_HANDLE_BASE, class])
}

// ---------------------------------------------------------------------------
// 布局数学（纯函数）
// ---------------------------------------------------------------------------

/// 把一个面板尺寸夹进 `[min, max]`。
pub fn clamp_size(size: f64, min: f64, max: f64) -> f64 {
    size.clamp(min, max)
}

/// 让一组尺寸归一化到总和 100，并逐个满足下限。
///
/// 直接「按比例缩放」在极端输入下会跌破下限，所以这里做迭代补偿：
/// 先把跌破下限的钉到下限，把多出来的量按比例从**还有余量**的面板上扣。
/// 最多迭代 `sizes.len()` 轮（每轮至少钉死一个），必然收敛。
pub fn normalize_sizes(sizes: &[f64], min: f64) -> Vec<f64> {
    let n = sizes.len();
    if n == 0 {
        return Vec::new();
    }

    // 先做非负化，避免负尺寸把后续算术带偏
    let mut out: Vec<f64> = sizes.iter().map(|s| s.max(0.0)).collect();
    let total: f64 = out.iter().sum();
    if total <= f64::EPSILON {
        // 全零 → 均分
        return vec![100.0 / n as f64; n];
    }
    for v in out.iter_mut() {
        *v = *v / total * 100.0;
    }

    // 下限不可达时（n * min > 100）只能放弃下限，否则总和会爆掉
    if min * n as f64 >= 100.0 {
        return out;
    }

    for _ in 0..n {
        let deficit: f64 = out
            .iter()
            .filter(|v| **v < min)
            .map(|v| min - v)
            .sum();
        if deficit <= 1e-9 {
            break;
        }
        // 可扣的余量只在「高于下限」的面板上
        let surplus: f64 = out.iter().filter(|v| **v > min).map(|v| v - min).sum();
        if surplus <= 1e-9 {
            break;
        }
        let ratio = (deficit / surplus).min(1.0);
        for v in out.iter_mut() {
            if *v < min {
                *v = min;
            } else {
                *v -= (*v - min) * ratio;
            }
        }
    }

    out
}

/// 拖动第 `handle_index` 根分隔条 `delta`（百分比）后，重新分配两侧面板。
///
/// 分隔条 `i` 夹在面板 `i` 与 `i+1` 之间。只动这两个，其余面板不变 ——
/// 这正是 `react-resizable-panels` 的行为。
///
/// 返回 `None` 表示这次拖动会被下限挡住（`delta` 方向已无空间）。
pub fn resize_pair(
    sizes: &[f64],
    handle_index: usize,
    delta: f64,
    min: f64,
) -> Option<Vec<f64>> {
    let n = sizes.len();
    if n < 2 || handle_index + 1 >= n {
        return None;
    }

    let left = sizes[handle_index];
    let right = sizes[handle_index + 1];

    // 左增右减（或反向），两侧都不能跌破下限
    let max_delta = right - min; // 右面板最多让出这么多
    let min_delta = min - left; // 左面板最多让出这么多（负值）
    let applied = delta.clamp(min_delta, max_delta);

    // 被完全挡住：没有任何可移动空间
    if applied.abs() <= f64::EPSILON {
        return None;
    }

    let mut out = sizes.to_vec();
    out[handle_index] = left + applied;
    out[handle_index + 1] = right - applied;
    Some(out)
}

/// 方向键的四个语义方向。
///
/// 刻意不用 Dioxus 的 `Key` 直接做入参：`keyboard-types` 的枚举有几十个变体，
/// 拿它当纯函数的入参会把测试写得很啰嗦；拆出一个只有四个变体的窄类型后，
/// 判定逻辑可以被穷举测试，`Key` → [`ArrowKey`] 的映射单独测。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArrowKey {
    Left,
    Right,
    Up,
    Down,
}

/// Dioxus 的 `Key` → [`ArrowKey`]。非方向键返回 `None`。
pub fn key_to_arrow(key: &Key) -> Option<ArrowKey> {
    match key {
        Key::ArrowLeft => Some(ArrowKey::Left),
        Key::ArrowRight => Some(ArrowKey::Right),
        Key::ArrowUp => Some(ArrowKey::Up),
        Key::ArrowDown => Some(ArrowKey::Down),
        _ => None,
    }
}

/// 方向键 → 尺寸增量。返回 `None` 表示这个方向不作用于当前排布。
///
/// 「沿主轴正方向」= 横向组里向右、纵向组里向下。
pub fn arrow_to_delta(arrow: ArrowKey, orientation: ResizableOrientation) -> Option<f64> {
    use ArrowKey::*;
    use ResizableOrientation::*;
    match (arrow, orientation) {
        (Right, Horizontal) | (Down, Vertical) => Some(KEYBOARD_STEP),
        (Left, Horizontal) | (Up, Vertical) => Some(-KEYBOARD_STEP),
        // 垂直主轴上的按键对当前排布无意义（横向组里按上下键不改变宽度）
        _ => None,
    }
}

/// 像素位移 → 百分比位移。`extent_px` 是组在主轴上占的像素长度。
///
/// `extent_px <= 0` 时返回 `None`（调用方还没测到尺寸，忽略这次拖动）。
pub fn px_to_percent(delta_px: f64, extent_px: f64) -> Option<f64> {
    if extent_px <= 0.0 || !extent_px.is_finite() {
        return None;
    }
    Some(delta_px / extent_px * 100.0)
}

// ---------------------------------------------------------------------------
// 持久化
// ---------------------------------------------------------------------------

/// 布局持久化后端。
///
/// TS 的 `useDefaultLayout` 直接写 `localStorage`；这里抽成 trait，
/// 让组件库不依赖任何平台 API，默认实现是进程内内存版。
pub trait LayoutStore {
    /// 读取某个布局 id 下保存的尺寸。
    fn load(&self, id: &str) -> Option<Vec<f64>>;
    /// 保存尺寸。
    fn save(&self, id: &str, sizes: &[f64]);
}

/// 内存版 [`LayoutStore`]（默认）。
#[derive(Clone, Copy, Default)]
pub struct MemoryLayoutStore;

impl LayoutStore for MemoryLayoutStore {
    fn load(&self, _id: &str) -> Option<Vec<f64>> {
        None
    }
    fn save(&self, _id: &str, _sizes: &[f64]) {}
}

// ---------------------------------------------------------------------------
// 组件
// ---------------------------------------------------------------------------

/// 面板组的共享状态。
#[derive(Clone, Copy)]
struct ResizableContext {
    sizes: Signal<Vec<f64>>,
    orientation: ResizableOrientation,
    min_size: f64,
    next_index: Signal<usize>,
}

/// 面板组的排布方向由 context 决定，面板用它取自己的索引。
#[component]
pub fn ResizablePanelGroup(
    #[props(default)] orientation: ResizableOrientation,
    /// 各面板的初始百分比（总和会自动归一化到 100）。
    #[props(default, into)] default_sizes: Vec<f64>,
    /// 面板数量。`default_sizes` 为空时按这个数量均分。
    #[props(default = 2)] panel_count: usize,
    #[props(default = DEFAULT_MIN_SIZE)] min_size: f64,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let initial = if default_sizes.is_empty() {
        vec![100.0 / panel_count.max(1) as f64; panel_count.max(1)]
    } else {
        normalize_sizes(&default_sizes, min_size)
    };

    let sizes = use_signal(|| initial);
    let next_index = use_signal(|| 0usize);
    use_context_provider(|| ResizableContext {
        sizes,
        orientation,
        min_size,
        next_index,
    });

    rsx! {
        div {
            class: resizable_panel_group_classes(orientation, &class),
            "data-slot": "resizable-panel-group",
            "data-panel-group-direction": orientation.as_str(),
            role: "group",
            {children}
        }
    }
}

/// 单个面板。尺寸由 [`ResizablePanelGroup`] 通过 context 下发。
#[component]
pub fn ResizablePanel(
    /// 固定百分比；不传则按 context 里的份额伸缩。
    #[props(default)] size: Option<f64>,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let ctx = use_context::<ResizableContext>();

    // 面板在组里的序号：按挂载顺序递增（`use_hook` 只在首次渲染跑一次）
    let index = use_hook(|| {
        let mut next = ctx.next_index;
        let i = next();
        next.set(i + 1);
        i
    });

    let pct = match size {
        Some(v) => v,
        None => {
            let sizes = (ctx.sizes)();
            sizes.get(index).copied().unwrap_or(0.0)
        }
    };

    rsx! {
        div {
            class: cn(&["min-w-0 min-h-0 overflow-hidden", &class]),
            "data-slot": "resizable-panel",
            "data-panel": "",
            style: "flex: {pct} 1 0px;",
            {children}
        }
    }
}

/// 可拖拽的分隔条。
///
/// `on_drag` 收到的是**百分比**增量；像素换算由本组件用 `extent_px` 完成
/// （见模块文档差异 3）。`extent_px` 为 0 时忽略指针拖动，键盘仍可用。
#[component]
pub fn ResizableHandle(
    /// 第几根分隔条（0 表示夹在面板 0 与 1 之间）。
    #[props(default)] index: usize,
    /// 组在主轴上占的像素长度，用于把拖拽像素换算成百分比。
    #[props(default = 0.0)] extent_px: f64,
    #[props(default, into)] class: String,
    #[props(default)] on_drag: EventHandler<f64>,
) -> Element {
    let ctx = use_context::<ResizableContext>();
    let mut drag_origin = use_signal(|| None::<f64>);
    let mut last_px = use_signal(|| 0.0);

    let aria_orientation = ctx.orientation.handle_orientation();
    let min_size = ctx.min_size;

    // 键盘：方向键按固定步长推拉；与鼠标路径共用 resize_pair 的夹取
    let apply = move |delta: f64| {
        let sizes = (ctx.sizes)();
        if let Some(next) = resize_pair(&sizes, index, delta, min_size) {
            ctx.sizes.clone().set(next);
            on_drag.call(delta);
        }
    };

    rsx! {
        div {
            class: resizable_handle_classes(&class),
            "data-slot": "resizable-handle",
            role: "separator",
            "aria-orientation": aria_orientation,
            tabindex: "0",
            onpointerdown: move |e| {
                drag_origin.set(Some(e.client_coordinates().x));
                last_px.set(0.0);
            },
            onpointermove: move |e| {
                let Some(origin) = drag_origin() else { return };
                let x = e.client_coordinates().x;
                // 只在按住时移动：Dioxus 0.6 的指针事件不带 buttons 状态，
                // 所以用「已记录起点」代替「按键按下」判定。
                if (x - origin).abs() < f64::EPSILON {
                    return;
                }
                let delta_px = x - origin - last_px();
                last_px.set(x - origin);
                if let Some(delta) = px_to_percent(delta_px, extent_px) {
                    apply(delta);
                }
            },
            onpointerup: move |_| {
                drag_origin.set(None);
                last_px.set(0.0);
            },
            onpointerleave: move |_| {
                drag_origin.set(None);
                last_px.set(0.0);
            },
            onkeydown: move |e| {
                if let Some(arrow) = key_to_arrow(&e.key()) {
                    if let Some(delta) = arrow_to_delta(arrow, ctx.orientation) {
                        apply(delta);
                    }
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_base_matches_ts() {
        assert_eq!(RESIZABLE_PANEL_GROUP_BASE, "flex h-full w-full");
    }

    #[test]
    fn handle_base_keeps_aria_orientation_variants() {
        // 这条是 TS 注释里踩过的坑：换成 data-[orientation=...] 就完全失效
        assert!(RESIZABLE_HANDLE_BASE.contains("aria-[orientation=vertical]:w-px"));
        assert!(RESIZABLE_HANDLE_BASE.contains("aria-[orientation=horizontal]:h-px"));
        assert!(!RESIZABLE_HANDLE_BASE.contains("data-[orientation="));
        // 48px 的缩进（my-6 / mx-6 各 24px）
        assert!(RESIZABLE_HANDLE_BASE.contains("h-[calc(100%-48px)]"));
        assert!(RESIZABLE_HANDLE_BASE.contains("w-[calc(100%-48px)]"));
        // 两端的渐隐遮罩
        assert!(RESIZABLE_HANDLE_BASE.contains("transparent_0%,black_18%,black_82%,transparent_100%"));
    }

    #[test]
    fn handle_orientation_is_opposite_to_group() {
        assert_eq!(
            ResizableOrientation::Horizontal.handle_orientation(),
            "vertical"
        );
        assert_eq!(
            ResizableOrientation::Vertical.handle_orientation(),
            "horizontal"
        );
    }

    #[test]
    fn group_orientation_adds_flex_direction() {
        let h = resizable_panel_group_classes(ResizableOrientation::Horizontal, "");
        assert!(h.contains("flex-row"));
        let v = resizable_panel_group_classes(ResizableOrientation::Vertical, "");
        assert!(v.contains("flex-col"));
    }

    #[test]
    fn user_class_wins() {
        let c = resizable_panel_group_classes(ResizableOrientation::Horizontal, "h-96");
        assert!(c.contains("h-96"));
        assert!(!c.contains("h-full"));
    }

    #[test]
    fn clamp_respects_bounds() {
        assert_eq!(clamp_size(5.0, 10.0, 90.0), 10.0);
        assert_eq!(clamp_size(95.0, 10.0, 90.0), 90.0);
        assert_eq!(clamp_size(50.0, 10.0, 90.0), 50.0);
    }

    #[test]
    fn normalize_sums_to_100() {
        let out = normalize_sizes(&[1.0, 1.0, 2.0], 10.0);
        let sum: f64 = out.iter().sum();
        assert!((sum - 100.0).abs() < 1e-9, "总和 = {sum}");
        assert!((out[0] - 25.0).abs() < 1e-9);
        assert!((out[2] - 50.0).abs() < 1e-9);
    }

    #[test]
    fn normalize_lifts_panels_above_the_minimum() {
        // 1:99 会让第一个面板跌破下限，归一化后必须补到 10
        let out = normalize_sizes(&[1.0, 99.0], 10.0);
        assert!(out[0] >= 10.0 - 1e-9, "第一个面板 = {}", out[0]);
        let sum: f64 = out.iter().sum();
        assert!((sum - 100.0).abs() < 1e-9, "总和 = {sum}");
    }

    #[test]
    fn normalize_handles_degenerate_input() {
        assert!(normalize_sizes(&[], 10.0).is_empty());
        // 全零 → 均分
        assert_eq!(normalize_sizes(&[0.0, 0.0, 0.0, 0.0], 10.0), vec![25.0; 4]);
        // 负数被当作 0
        let out = normalize_sizes(&[-5.0, 10.0], 10.0);
        assert!(out.iter().all(|v| *v >= 0.0));
    }

    #[test]
    fn normalize_gives_up_on_min_when_infeasible() {
        // 4 个面板 × 下限 30 > 100：下限不可达，但不能让总和爆掉
        let out = normalize_sizes(&[10.0, 20.0, 30.0, 40.0], 30.0);
        let sum: f64 = out.iter().sum();
        assert!((sum - 100.0).abs() < 1e-9, "总和 = {sum}");
    }

    #[test]
    fn resize_pair_moves_both_neighbours() {
        let out = resize_pair(&[50.0, 50.0], 0, 10.0, 10.0).unwrap();
        assert_eq!(out, vec![60.0, 40.0]);
    }

    #[test]
    fn resize_pair_only_touches_the_two_neighbours() {
        // 分隔条 1 夹在面板 1 与 2 之间 → 面板 0 完全不动
        let out = resize_pair(&[30.0, 40.0, 30.0], 1, -5.0, 10.0).unwrap();
        assert_eq!(out, vec![30.0, 35.0, 35.0]);
    }

    #[test]
    fn resize_pair_clamps_at_the_minimum() {
        // 右面板只剩 15，最多让出 5
        let out = resize_pair(&[85.0, 15.0], 0, 40.0, 10.0).unwrap();
        assert_eq!(out, vec![90.0, 10.0]);
    }

    #[test]
    fn resize_pair_returns_none_when_fully_blocked() {
        // 两侧都贴在下限，任何方向都动不了
        assert!(resize_pair(&[10.0, 90.0], 0, -5.0, 10.0).is_none());
        assert!(resize_pair(&[90.0, 10.0], 0, 5.0, 10.0).is_none());
    }

    #[test]
    fn resize_pair_rejects_bad_handle_index() {
        assert!(resize_pair(&[50.0, 50.0], 5, 1.0, 10.0).is_none());
        assert!(resize_pair(&[100.0], 0, 1.0, 10.0).is_none());
    }

    #[test]
    fn key_mapping_follows_orientation() {
        use ArrowKey::*;
        use ResizableOrientation::*;
        assert_eq!(arrow_to_delta(Right, Horizontal), Some(KEYBOARD_STEP));
        assert_eq!(arrow_to_delta(Left, Horizontal), Some(-KEYBOARD_STEP));
        // 纵向组里左右键不生效
        assert_eq!(arrow_to_delta(Right, Vertical), None);
        assert_eq!(arrow_to_delta(Left, Vertical), None);
        assert_eq!(arrow_to_delta(Down, Vertical), Some(KEYBOARD_STEP));
        assert_eq!(arrow_to_delta(Up, Vertical), Some(-KEYBOARD_STEP));
        // 反向同理
        assert_eq!(arrow_to_delta(Up, Horizontal), None);
        assert_eq!(arrow_to_delta(Down, Horizontal), None);
    }

    #[test]
    fn dioxus_key_maps_to_arrow_key() {
        assert_eq!(key_to_arrow(&Key::ArrowLeft), Some(ArrowKey::Left));
        assert_eq!(key_to_arrow(&Key::ArrowRight), Some(ArrowKey::Right));
        assert_eq!(key_to_arrow(&Key::ArrowUp), Some(ArrowKey::Up));
        assert_eq!(key_to_arrow(&Key::ArrowDown), Some(ArrowKey::Down));
        // 非方向键不该被吞掉当方向用
        assert_eq!(key_to_arrow(&Key::Enter), None);
        assert_eq!(key_to_arrow(&Key::Character("a".into())), None);
    }

    #[test]
    fn px_to_percent_needs_a_measured_extent() {
        assert_eq!(px_to_percent(100.0, 1000.0), Some(10.0));
        assert_eq!(px_to_percent(100.0, 0.0), None);
        assert_eq!(px_to_percent(100.0, -1.0), None);
        assert_eq!(px_to_percent(100.0, f64::NAN), None);
    }
}
