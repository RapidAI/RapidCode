//! `Carousel` —— Rust 移植自 `packages/ui/src/components/ui/carousel.tsx`
//!
//! TS 侧建立在 `embla-carousel-react` 之上：embla 负责测量、滚动吸附与
//! `canScrollPrev` / `canScrollNext` 的计算，这个文件只做「套壳 + 样式 + 键盘」。
//!
//! ## 关键移植点 1：用索引状态机替代滚动引擎
//!
//! Rust 侧没有 embla 等价物（也不需要为了一个组件引入滚动引擎），因此
//! [`CarouselState`] 直接按**索引**建模。这不是近似 —— `CarouselItem` 用的是
//! `basis-full`（每屏恰好一张），所以 embla 的吸附点数就等于张数，
//! 「按吸附点算」与「按索引算」在这里是同一件事。
//!
//! embla 的 `loop` 选项在 `canScrollPrev` / `canScrollNext` 上的表现也一并复刻：
//! 开启循环后只要张数 > 1，两个方向都恒为可滚动。
//!
//! ## 关键移植点 2：方向键不看朝向
//!
//! `handleKeyDown` 里**只判断 `ArrowLeft` / `ArrowRight`，没有判断 `orientation`**。
//! 也就是说竖向轮播同样只响应左右方向键（而不是上下）。这是上游的行为，
//! 移植时原样保留并由 `arrow_keys_ignore_the_orientation` 钉住。
//!
//! ## 关键移植点 3：`orientation` 的兜底是死代码
//!
//! `contextValue` 里写了 `orientation || (opts?.axis === "y" ? "vertical" : "horizontal")`，
//! 但函数签名是 `orientation = "horizontal"`，参数永远不为 `undefined`，
//! 因此 `opts.axis` 那条兜底永远不会触发。本移植不复制这段死代码，
//! 只在 [`Carousel`] 里保留 `orientation` 本身。
//!
//! ## 与 TS 的差异
//!
//! 1. **没有 `ref`**：Dioxus 组件自身即渲染 DOM，`carouselRef` 无对应物。
//! 2. **`onKeyDownCapture` → `onkeydown`**：Dioxus 0.6 没有捕获阶段的键盘事件
//!    （`onKeyDownCapture` 是 React 的合成事件，DOM 里并不存在对应事件），
//!    改用冒泡阶段的 `onkeydown`。轮播的方向键很少与子元素冲突，实践中等价。
//! 3. **`setApi` → `state`**：embla 的 `api` 对象无法移植，改为暴露
//!    `Option<Signal<CarouselState>>`（与 `Popover::open` 同款做法），
//!    调用方既能读也能主动 `scroll_to`。
//! 4. **`opts` / `plugins` 只保留 `loop`**：embla 的选项有几十个，其中影响
//!    `canScroll*` 的只有 `loop`（其余如 `align` / `containScroll` 影响的是像素
//!    偏移量，本移植不做滚动）。`plugins`（自动播放等）无对应物。
//! 5. **文案改为 prop**：TS 走 `intl.formatMessage`，本移植用
//!    `previous_label` / `next_label`，默认值与 `en-US` 语言包一致。

use crate::cn::cn;
use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::icons::{IconChevronLeft, IconChevronRight};
use dioxus::prelude::*;

/// 轮播朝向（对应 TS 的 `orientation`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CarouselOrientation {
    #[default]
    Horizontal,
    Vertical,
}

impl CarouselOrientation {
    pub fn as_str(&self) -> &'static str {
        match self {
            CarouselOrientation::Horizontal => "horizontal",
            CarouselOrientation::Vertical => "vertical",
        }
    }

    pub fn is_horizontal(&self) -> bool {
        matches!(self, CarouselOrientation::Horizontal)
    }

    /// embla 的 `axis` 取值（`"x"` / `"y"`）。
    pub fn axis(&self) -> &'static str {
        match self {
            CarouselOrientation::Horizontal => "x",
            CarouselOrientation::Vertical => "y",
        }
    }
}

// ---------------------------------------------------------------------------
// 类名
// ---------------------------------------------------------------------------

/// 根节点基础类名。
pub const CAROUSEL_BASE: &str = "relative";

/// 视口（`overflow-hidden` 的那层）。
pub const CAROUSEL_VIEWPORT_BASE: &str = "overflow-hidden";

/// 轨道 —— 横向。
pub const CAROUSEL_TRACK_HORIZONTAL: &str = "flex -ml-4";

/// 轨道 —— 纵向。
pub const CAROUSEL_TRACK_VERTICAL: &str = "flex -mt-4 flex-col";

/// 单个 slide 的基础类名（`basis-full` 是「每屏一张」的来源）。
pub const CAROUSEL_ITEM_BASE: &str = "min-w-0 shrink-0 grow-0 basis-full";

/// slide 的横向间距。
pub const CAROUSEL_ITEM_HORIZONTAL: &str = "pl-4";

/// slide 的纵向间距。
pub const CAROUSEL_ITEM_VERTICAL: &str = "pt-4";

/// 前后按钮共享的基础类名。
pub const CAROUSEL_BUTTON_BASE: &str = "absolute touch-manipulation rounded-full";

/// 「上一张」按钮 —— 横向。
pub const CAROUSEL_PREVIOUS_HORIZONTAL: &str = "top-1/2 -left-12 -translate-y-1/2";

/// 「上一张」按钮 —— 纵向。
pub const CAROUSEL_PREVIOUS_VERTICAL: &str = "-top-12 left-1/2 -translate-x-1/2 rotate-90";

/// 「下一张」按钮 —— 横向。
pub const CAROUSEL_NEXT_HORIZONTAL: &str = "top-1/2 -right-12 -translate-y-1/2";

/// 「下一张」按钮 —— 纵向。
pub const CAROUSEL_NEXT_VERTICAL: &str = "-bottom-12 left-1/2 -translate-x-1/2 rotate-90";

/// 「上一张」按钮的默认无障碍文案（同 `en-US` 语言包）。
pub const CAROUSEL_PREVIOUS_LABEL: &str = "Previous slide";

/// 「下一张」按钮的默认无障碍文案（同 `en-US` 语言包）。
pub const CAROUSEL_NEXT_LABEL: &str = "Next slide";

pub fn carousel_classes(class: &str) -> String {
    cn(&[CAROUSEL_BASE, class])
}

/// 轨道的类名（TS 里 `className` 落在**内层轨道**上，不是视口那层）。
pub fn carousel_track_classes(orientation: CarouselOrientation, class: &str) -> String {
    let direction = if orientation.is_horizontal() {
        CAROUSEL_TRACK_HORIZONTAL
    } else {
        CAROUSEL_TRACK_VERTICAL
    };
    cn(&[direction, class])
}

pub fn carousel_item_classes(orientation: CarouselOrientation, class: &str) -> String {
    let direction = if orientation.is_horizontal() {
        CAROUSEL_ITEM_HORIZONTAL
    } else {
        CAROUSEL_ITEM_VERTICAL
    };
    cn(&[CAROUSEL_ITEM_BASE, direction, class])
}

pub fn carousel_previous_classes(orientation: CarouselOrientation, class: &str) -> String {
    let direction = if orientation.is_horizontal() {
        CAROUSEL_PREVIOUS_HORIZONTAL
    } else {
        CAROUSEL_PREVIOUS_VERTICAL
    };
    cn(&[CAROUSEL_BUTTON_BASE, direction, class])
}

pub fn carousel_next_classes(orientation: CarouselOrientation, class: &str) -> String {
    let direction = if orientation.is_horizontal() {
        CAROUSEL_NEXT_HORIZONTAL
    } else {
        CAROUSEL_NEXT_VERTICAL
    };
    cn(&[CAROUSEL_BUTTON_BASE, direction, class])
}

// ---------------------------------------------------------------------------
// 状态机
// ---------------------------------------------------------------------------

/// 轮播的滑动状态。
///
/// 见模块文档关键移植点 1：因为每个 slide 都是 `basis-full`，按索引建模与
/// embla 按吸附点建模等价。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CarouselState {
    /// 张数。
    pub count: usize,
    /// 当前吸附点索引（永远落在 `0..count`；`count == 0` 时为 `0`）。
    pub index: usize,
    /// 是否循环（embla `loop`）。
    pub loop_enabled: bool,
}

impl CarouselState {
    pub fn new(count: usize) -> Self {
        Self {
            count,
            index: 0,
            loop_enabled: false,
        }
    }

    pub fn with_loop(mut self, loop_enabled: bool) -> Self {
        self.loop_enabled = loop_enabled;
        self
    }

    /// 张数变化时保留当前索引（越界则夹回最后一张）。
    pub fn with_count(mut self, count: usize) -> Self {
        self.count = count;
        self.index = self.index.min(count.saturating_sub(1));
        self
    }

    /// 对应 embla `api.canScrollPrev()`。
    ///
    /// 循环模式下只要不止一张，两个方向都恒可滚动。
    pub fn can_scroll_prev(&self) -> bool {
        if self.count <= 1 {
            return false;
        }
        self.loop_enabled || self.index > 0
    }

    /// 对应 embla `api.canScrollNext()`。
    pub fn can_scroll_next(&self) -> bool {
        if self.count <= 1 {
            return false;
        }
        self.loop_enabled || self.index + 1 < self.count
    }

    /// 对应 embla `api.scrollPrev()`。返回是否真的动了。
    pub fn scroll_prev(&mut self) -> bool {
        if !self.can_scroll_prev() {
            return false;
        }
        self.index = if self.loop_enabled {
            // `+ count` 避免 usize 下溢
            (self.index + self.count - 1) % self.count
        } else {
            self.index - 1
        };
        true
    }

    /// 对应 embla `api.scrollNext()`。返回是否真的动了。
    pub fn scroll_next(&mut self) -> bool {
        if !self.can_scroll_next() {
            return false;
        }
        self.index = if self.loop_enabled {
            (self.index + 1) % self.count
        } else {
            self.index + 1
        };
        true
    }

    /// 跳到指定吸附点。循环模式取模，否则夹到边界。返回是否真的动了。
    pub fn scroll_to(&mut self, index: usize) -> bool {
        if self.count == 0 {
            return false;
        }
        let next = if self.loop_enabled {
            index % self.count
        } else {
            index.min(self.count - 1)
        };
        if next == self.index {
            return false;
        }
        self.index = next;
        true
    }
}

/// 方向键对应的动作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CarouselKeyAction {
    /// 不关轮播的事。
    None,
    Prev,
    Next,
}

/// 解析方向键（TS `handleKeyDown`）。
///
/// 注意**不看 `orientation`** —— 见模块文档关键移植点 2。
pub fn resolve_carousel_key_action(key: Key) -> CarouselKeyAction {
    match key {
        Key::ArrowLeft => CarouselKeyAction::Prev,
        Key::ArrowRight => CarouselKeyAction::Next,
        _ => CarouselKeyAction::None,
    }
}

// ---------------------------------------------------------------------------
// 组件
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct CarouselContext {
    state: Signal<CarouselState>,
    orientation: CarouselOrientation,
}

/// 轮播根节点。`role="region"` + `aria-roledescription="carousel"`。
#[component]
pub fn Carousel(
    #[props(default)] orientation: CarouselOrientation,
    /// 张数（embla 从 DOM 里数出来，这里显式传入）。
    #[props(default = 1)] count: usize,
    /// 是否循环（embla `opts.loop`）。
    #[props(default)] loop_enabled: bool,
    /// 外部持有的状态（对应 TS `setApi` 暴露的 embla api）。
    #[props(default)] state: Option<Signal<CarouselState>>,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let internal = use_signal(|| CarouselState::new(count).with_loop(loop_enabled));
    let state = state.unwrap_or(internal);

    // 渲染期收敛：把 prop 镜像进状态。`with_count` 会把越界索引夹回去。
    {
        let current = state();
        if current.count != count || current.loop_enabled != loop_enabled {
            let mut s = state;
            s.set(current.with_count(count).with_loop(loop_enabled));
        }
    }

    use_context_provider(|| CarouselContext { state, orientation });

    rsx! {
        div {
            class: carousel_classes(&class),
            role: "region",
            "aria-roledescription": "carousel",
            "data-slot": "carousel",
            "data-orientation": orientation.as_str(),
            // 见模块文档差异 2：冒泡阶段代替 React 的捕获阶段。
            onkeydown: move |e: KeyboardEvent| {
                let action = resolve_carousel_key_action(e.key());
                if action == CarouselKeyAction::None {
                    return;
                }
                e.prevent_default();
                let mut s = state;
                match action {
                    CarouselKeyAction::Prev => {
                        s.write().scroll_prev();
                    }
                    CarouselKeyAction::Next => {
                        s.write().scroll_next();
                    }
                    CarouselKeyAction::None => {}
                }
            },
            {children}
        }
    }
}

/// 视口 + 轨道两层结构。
///
/// TS 把 `className` 传给**内层轨道**，外层固定 `overflow-hidden`。
#[component]
pub fn CarouselContent(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let ctx = use_context::<CarouselContext>();
    rsx! {
        div { class: CAROUSEL_VIEWPORT_BASE, "data-slot": "carousel-content",
            div { class: carousel_track_classes(ctx.orientation, &class), {children} }
        }
    }
}

/// 单张幻灯片。`basis-full` 保证每屏一张。
#[component]
pub fn CarouselItem(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let ctx = use_context::<CarouselContext>();
    rsx! {
        div {
            class: carousel_item_classes(ctx.orientation, &class),
            role: "group",
            "aria-roledescription": "slide",
            "data-slot": "carousel-item",
            {children}
        }
    }
}

/// 「上一张」按钮。到边界（且未开循环）时自动 `disabled`。
#[component]
pub fn CarouselPrevious(
    #[props(default = ButtonVariant::Outline)] variant: ButtonVariant,
    #[props(default = ButtonSize::IconSm)] size: ButtonSize,
    /// 无障碍文案（TS 走 intl，默认与 `en-US` 一致）。
    #[props(default = CAROUSEL_PREVIOUS_LABEL.to_string(), into)] label: String,
    #[props(default, into)] class: String,
) -> Element {
    let ctx = use_context::<CarouselContext>();
    let can_scroll = (ctx.state)().can_scroll_prev();

    rsx! {
        Button {
            slot: "carousel-previous",
            variant,
            size,
            class: carousel_previous_classes(ctx.orientation, &class),
            disabled: !can_scroll,
            onclick: move |_| {
                let mut s = ctx.state;
                s.write().scroll_prev();
            },
            IconChevronLeft {}
            span { class: "sr-only", {label} }
        }
    }
}

/// 「下一张」按钮。到边界（且未开循环）时自动 `disabled`。
#[component]
pub fn CarouselNext(
    #[props(default = ButtonVariant::Outline)] variant: ButtonVariant,
    #[props(default = ButtonSize::IconSm)] size: ButtonSize,
    /// 无障碍文案（TS 走 intl，默认与 `en-US` 一致）。
    #[props(default = CAROUSEL_NEXT_LABEL.to_string(), into)] label: String,
    #[props(default, into)] class: String,
) -> Element {
    let ctx = use_context::<CarouselContext>();
    let can_scroll = (ctx.state)().can_scroll_next();

    rsx! {
        Button {
            slot: "carousel-next",
            variant,
            size,
            class: carousel_next_classes(ctx.orientation, &class),
            disabled: !can_scroll,
            onclick: move |_| {
                let mut s = ctx.state;
                s.write().scroll_next();
            },
            IconChevronRight {}
            span { class: "sr-only", {label} }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- 朝向 ----

    #[test]
    fn orientation_strings_and_axis() {
        assert_eq!(CarouselOrientation::Horizontal.as_str(), "horizontal");
        assert_eq!(CarouselOrientation::Vertical.as_str(), "vertical");
        // embla 的 axis 是 `x` / `y`，与 `orientation` 不是同一个词表
        assert_eq!(CarouselOrientation::Horizontal.axis(), "x");
        assert_eq!(CarouselOrientation::Vertical.axis(), "y");
        assert!(CarouselOrientation::default().is_horizontal());
        assert!(!CarouselOrientation::Vertical.is_horizontal());
    }

    // ---- 类名 ----

    #[test]
    fn root_class() {
        let c = carousel_classes("");
        assert_eq!(c, "relative");
        assert!(carousel_classes("h-64").contains("h-64"));
    }

    #[test]
    fn track_class_flips_the_gutter_axis() {
        let h = carousel_track_classes(CarouselOrientation::Horizontal, "");
        assert!(h.contains("flex"));
        assert!(h.contains("-ml-4"));
        assert!(!h.contains("flex-col"));

        let v = carousel_track_classes(CarouselOrientation::Vertical, "");
        assert!(v.contains("-mt-4"));
        assert!(v.contains("flex-col"));
        assert!(!v.contains("-ml-4"));
    }

    #[test]
    fn item_class_keeps_basis_full_in_both_orientations() {
        for orientation in [
            CarouselOrientation::Horizontal,
            CarouselOrientation::Vertical,
        ] {
            let c = carousel_item_classes(orientation, "");
            assert!(c.contains("min-w-0"));
            assert!(c.contains("shrink-0"));
            assert!(c.contains("grow-0"));
            assert!(c.contains("basis-full"), "{c}");
        }
        assert!(carousel_item_classes(CarouselOrientation::Horizontal, "").contains("pl-4"));
        assert!(carousel_item_classes(CarouselOrientation::Vertical, "").contains("pt-4"));
    }

    #[test]
    fn buttons_share_the_absolute_shell() {
        for c in [
            carousel_previous_classes(CarouselOrientation::Horizontal, ""),
            carousel_previous_classes(CarouselOrientation::Vertical, ""),
            carousel_next_classes(CarouselOrientation::Horizontal, ""),
            carousel_next_classes(CarouselOrientation::Vertical, ""),
        ] {
            assert!(c.contains("absolute"), "{c}");
            assert!(c.contains("touch-manipulation"), "{c}");
            assert!(c.contains("rounded-full"), "{c}");
        }
    }

    #[test]
    fn previous_and_next_are_mirrored() {
        let prev = carousel_previous_classes(CarouselOrientation::Horizontal, "");
        let next = carousel_next_classes(CarouselOrientation::Horizontal, "");
        assert!(prev.contains("-left-12"));
        assert!(next.contains("-right-12"));
        assert!(prev.contains("-translate-y-1/2"));
        assert!(next.contains("-translate-y-1/2"));
    }

    /// 竖向时按钮用 `rotate-90` 把箭头指向上下，并且各自贴到相反的一端。
    #[test]
    fn vertical_buttons_rotate_and_swap_ends() {
        let prev = carousel_previous_classes(CarouselOrientation::Vertical, "");
        let next = carousel_next_classes(CarouselOrientation::Vertical, "");
        assert!(prev.contains("-top-12"));
        assert!(next.contains("-bottom-12"));
        for c in [&prev, &next] {
            assert!(c.contains("left-1/2"), "{c}");
            assert!(c.contains("-translate-x-1/2"), "{c}");
            assert!(c.contains("rotate-90"), "{c}");
        }
        // 竖向不再用左右定位
        assert!(!prev.contains("-left-12"));
        assert!(!next.contains("-right-12"));
    }

    #[test]
    fn user_class_is_appended_last() {
        let c = carousel_item_classes(CarouselOrientation::Horizontal, "basis-1/2");
        // `basis-full` 与 `basis-1/2` 同属 flex-basis 组，tailwind-merge 语义下后者胜
        assert!(c.contains("basis-1/2"));
        assert!(!c.contains("basis-full"));
    }

    // ---- 状态机 ----

    #[test]
    fn empty_and_single_slide_carousels_never_scroll() {
        for count in [0, 1] {
            for loop_enabled in [false, true] {
                let s = CarouselState::new(count).with_loop(loop_enabled);
                assert!(!s.can_scroll_prev(), "count={count} loop={loop_enabled}");
                assert!(!s.can_scroll_next(), "count={count} loop={loop_enabled}");
            }
        }
        // 循环模式下只有一张也不能滚（否则会原地打转）
        let mut single = CarouselState::new(1).with_loop(true);
        assert!(!single.scroll_next());
        assert_eq!(single.index, 0);
    }

    #[test]
    fn non_looping_carousel_stops_at_both_ends() {
        let mut s = CarouselState::new(3);
        assert!(!s.can_scroll_prev());
        assert!(s.can_scroll_next());

        assert!(s.scroll_next());
        assert_eq!(s.index, 1);
        assert!(s.can_scroll_prev());
        assert!(s.can_scroll_next());

        assert!(s.scroll_next());
        assert_eq!(s.index, 2);
        assert!(s.can_scroll_prev());
        assert!(!s.can_scroll_next());

        // 到底了再点无效
        assert!(!s.scroll_next());
        assert_eq!(s.index, 2);

        assert!(s.scroll_prev());
        assert_eq!(s.index, 1);
    }

    #[test]
    fn looping_carousel_wraps_both_ways() {
        let mut s = CarouselState::new(3).with_loop(true);
        assert!(s.can_scroll_prev());

        // 从 0 往回走 → 2（不是下溢）
        assert!(s.scroll_prev());
        assert_eq!(s.index, 2);

        assert!(s.scroll_next());
        assert_eq!(s.index, 0);
    }

    #[test]
    fn scroll_to_clamps_or_wraps() {
        let mut clamped = CarouselState::new(3);
        assert!(clamped.scroll_to(99));
        assert_eq!(clamped.index, 2);
        assert!(!clamped.scroll_to(99));
        assert!(clamped.scroll_to(0));
        assert_eq!(clamped.index, 0);

        let mut wrapped = CarouselState::new(3).with_loop(true);
        assert!(wrapped.scroll_to(4));
        assert_eq!(wrapped.index, 1);
    }

    #[test]
    fn scroll_to_on_an_empty_carousel_is_a_no_op() {
        let mut s = CarouselState::new(0);
        assert!(!s.scroll_to(0));
        assert_eq!(s.index, 0);
    }

    /// 张数变少时索引要夹回最后一张，否则会指向不存在的 slide。
    #[test]
    fn shrinking_the_count_clamps_the_index() {
        let s = CarouselState::new(5);
        let mut moved = s;
        assert!(moved.scroll_to(4));
        assert_eq!(moved.index, 4);

        let shrunk = moved.with_count(2);
        assert_eq!(shrunk.index, 1);
        assert!(shrunk.can_scroll_prev());
        assert!(!shrunk.can_scroll_next());
    }

    #[test]
    fn growing_the_count_keeps_the_index() {
        let s = CarouselState::new(2);
        let mut moved = s;
        assert!(moved.scroll_to(1));
        assert_eq!(moved.with_count(5).index, 1);
    }

    // ---- 键盘 ----

    #[test]
    fn arrow_keys_map_to_prev_and_next() {
        assert_eq!(
            resolve_carousel_key_action(Key::ArrowLeft),
            CarouselKeyAction::Prev
        );
        assert_eq!(
            resolve_carousel_key_action(Key::ArrowRight),
            CarouselKeyAction::Next
        );
    }

    #[test]
    fn other_keys_are_ignored() {
        for key in [
            Key::ArrowUp,
            Key::ArrowDown,
            Key::Enter,
            Key::Escape,
            Key::Home,
            Key::End,
            Key::Tab,
        ] {
            assert_eq!(resolve_carousel_key_action(key), CarouselKeyAction::None);
        }
    }

    /// 上游的 `handleKeyDown` 不判断朝向：竖向轮播也只吃左右方向键。
    #[test]
    fn arrow_keys_ignore_the_orientation() {
        for orientation in [
            CarouselOrientation::Horizontal,
            CarouselOrientation::Vertical,
        ] {
            let ctx_orientation = orientation;
            // 解析函数根本不接收 orientation，这就是「不看朝向」的证据
            assert_eq!(
                resolve_carousel_key_action(Key::ArrowLeft),
                CarouselKeyAction::Prev,
                "{ctx_orientation:?}"
            );
            // 竖向轮播按上下键不会翻页
            assert_eq!(
                resolve_carousel_key_action(Key::ArrowDown),
                CarouselKeyAction::None
            );
        }
    }

    /// 键盘翻页要真的改状态（把解析结果接到状态机上跑一遍）。
    #[test]
    fn keyboard_drives_the_state_machine() {
        let mut s = CarouselState::new(3);
        for _ in 0..5 {
            match resolve_carousel_key_action(Key::ArrowRight) {
                CarouselKeyAction::Next => {
                    s.scroll_next();
                }
                _ => panic!("右方向键应映射到 Next"),
            }
        }
        // 非循环：停在最后一张
        assert_eq!(s.index, 2);

        for _ in 0..5 {
            match resolve_carousel_key_action(Key::ArrowLeft) {
                CarouselKeyAction::Prev => {
                    s.scroll_prev();
                }
                _ => panic!("左方向键应映射到 Prev"),
            }
        }
        assert_eq!(s.index, 0);
    }

    #[test]
    fn components_are_constructible() {
        let _ = Carousel as fn(CarouselProps) -> Element;
        let _ = CarouselContent as fn(CarouselContentProps) -> Element;
        let _ = CarouselItem as fn(CarouselItemProps) -> Element;
        let _ = CarouselPrevious as fn(CarouselPreviousProps) -> Element;
        let _ = CarouselNext as fn(CarouselNextProps) -> Element;
    }
}
