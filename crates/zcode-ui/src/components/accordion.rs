//! `Accordion` 家族 —— Rust 移植自 `packages/ui/src/components/ui/accordion.tsx`
//!
//! TS 用 Radix `Accordion.Root/Item/Header/Trigger/Content`，展开状态由 Root 持有并
//! 通过 Context 下发给每个 Item。Rust 侧用 `use_context_provider` 复刻同一套结构。
//!
//! ## 与 TS 的行为差异
//!
//! 1. **`type` / `collapsible` 用枚举表达**。Radix 靠 `type="single" | "multiple"`
//!    联合类型 + `collapsible` 布尔组合出 4 种语义；Rust 侧用
//!    [`AccordionType`]（`Single` / `Multiple`）表达，`Single` 语义等价于
//!    Radix 的 `type="single" collapsible`（允许再次点击收起），因为「不可收起」
//!    在桌面端几乎不可能是想要的行为。
//! 2. **Content 保持挂载 + `hidden` 切换**（同 [`crate::components::collapsible`]）。
//!    代价是 `data-closed:animate-accordion-up` 的退场动画不播放。
//! 3. **`h-(--radix-accordion-content-height)` 保留但无效果**。该 CSS 变量由 Radix
//!    在动画期间写入 DOM；本移植没有 Radix，变量未定义 → `height: var(...)` 在
//!    计算值阶段失效并回退为 `auto`，内容正常显示。保留类名是为了与 TS 逐字对齐，
//!    将来若接入自研的高度测量再补上变量即可。
//! 4. **`id` 由调用方提供**（同 [`crate::components::collapsible`]）。Radix 用
//!    `useId()` 自动生成并接上 `aria-controls`；这里做成显式 prop，给了才发属性，
//!    不传时不会留下 `id=""`。

use crate::cn::cn;
use crate::icons::{IconChevronDown, IconChevronUp};
use dioxus::prelude::*;

/// 手风琴展开模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AccordionType {
    /// 同时只能展开一项（点击已展开项会收起）。
    #[default]
    Single,
    /// 可同时展开多项。
    Multiple,
}

impl AccordionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AccordionType::Single => "single",
            AccordionType::Multiple => "multiple",
        }
    }
}

/// 手风琴方向（Radix 支持，实际几乎只用 `vertical`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AccordionOrientation {
    #[default]
    Vertical,
    Horizontal,
}

impl AccordionOrientation {
    pub fn as_str(&self) -> &'static str {
        match self {
            AccordionOrientation::Vertical => "vertical",
            AccordionOrientation::Horizontal => "horizontal",
        }
    }
}

/// `Accordion` 根容器基础类名。
pub const ACCORDION_BASE: &str = "flex w-full flex-col overflow-hidden rounded-md border";

/// `AccordionItem` 基础类名。
pub const ACCORDION_ITEM_BASE: &str = "not-last:border-b data-open:bg-muted/50";

/// `AccordionTrigger` 基础类名。
pub const ACCORDION_TRIGGER_BASE: &str = "group/accordion-trigger relative flex flex-1 items-start justify-between gap-6 border border-transparent p-2 text-left text-ui-base/relaxed font-medium transition-all outline-none hover:underline disabled:pointer-events-none disabled:opacity-50 **:data-[slot=accordion-trigger-icon]:ml-auto **:data-[slot=accordion-trigger-icon]:size-4 **:data-[slot=accordion-trigger-icon]:text-muted-foreground";

/// `AccordionTrigger` 折叠态图标类名（`aria-expanded=true` 时隐藏）。
pub const ACCORDION_TRIGGER_ICON_DOWN: &str =
    "pointer-events-none shrink-0 group-aria-expanded/accordion-trigger:hidden";

/// `AccordionTrigger` 展开态图标类名（`aria-expanded=true` 时显示）。
pub const ACCORDION_TRIGGER_ICON_UP: &str =
    "pointer-events-none hidden shrink-0 group-aria-expanded/accordion-trigger:inline";

/// `AccordionContent` 基础类名。
pub const ACCORDION_CONTENT_BASE: &str =
    "overflow-hidden px-2 text-ui-base/relaxed data-open:animate-accordion-down data-closed:animate-accordion-up";

/// `AccordionContent` 内层包裹 div 基础类名。
pub const ACCORDION_CONTENT_INNER: &str = "h-(--radix-accordion-content-height) pt-0 pb-4 [&_a]:underline [&_a]:underline-offset-3 [&_a]:hover:text-foreground [&_p:not(:last-child)]:mb-4";

pub fn accordion_classes(class: &str) -> String {
    cn(&[ACCORDION_BASE, class])
}

pub fn accordion_item_classes(class: &str) -> String {
    cn(&[ACCORDION_ITEM_BASE, class])
}

pub fn accordion_trigger_classes(class: &str) -> String {
    cn(&[ACCORDION_TRIGGER_BASE, class])
}

pub fn accordion_content_classes(class: &str) -> String {
    cn(&[ACCORDION_CONTENT_BASE, class])
}

pub fn accordion_content_inner_classes(class: &str) -> String {
    cn(&[ACCORDION_CONTENT_INNER, class])
}

/// 由 `Accordion` 提供、`AccordionItem` 及其子组件消费的共享状态。
#[derive(Clone, Copy)]
struct AccordionContext {
    open: Signal<Vec<String>>,
    kind: AccordionType,
    orientation: AccordionOrientation,
    disabled: bool,
}

impl AccordionContext {
    fn is_open(&self, value: &str) -> bool {
        self.open.read().iter().any(|v| v == value)
    }

    fn toggle(&mut self, value: &str) {
        if self.disabled {
            return;
        }
        let mut current = self.open.read().clone();
        let idx = current.iter().position(|v| v == value);
        match (self.kind, idx) {
            // 单开模式：点击已展开项 → 收起
            (AccordionType::Single, Some(_)) => current.clear(),
            (AccordionType::Single, None) => {
                current.clear();
                current.push(value.to_string());
            }
            (AccordionType::Multiple, Some(i)) => {
                current.remove(i);
            }
            (AccordionType::Multiple, None) => current.push(value.to_string()),
        }
        self.open.set(current);
    }
}

/// 读取当前展开的项（供自定义触发器/内容复用）。
pub fn use_accordion_open() -> Signal<Vec<String>> {
    use_context::<AccordionContext>().open
}

/// 手风琴根容器。
#[component]
pub fn Accordion(
    /// 受控模式：由调用方持有展开项集合。不传则使用内部状态。
    #[props(default)] value: Option<Signal<Vec<String>>>,
    /// 非受控模式下的初始展开项（对应 Radix 的 `defaultValue`）。
    #[props(default, into)] default_value: Vec<String>,
    #[props(default)] kind: AccordionType,
    #[props(default)] orientation: AccordionOrientation,
    #[props(default)] disabled: bool,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let internal = use_signal(|| default_value);
    let open = value.unwrap_or(internal);
    use_context_provider(|| AccordionContext {
        open,
        kind,
        orientation,
        disabled,
    });

    rsx! {
        div {
            class: accordion_classes(&class),
            "data-slot": "accordion",
            "data-orientation": orientation.as_str(),
            {children}
        }
    }
}

/// 手风琴单项。
#[component]
pub fn AccordionItem(
    /// 该项的唯一标识（对应 Radix 的 `value`）。
    #[props(into)] value: String,
    #[props(default)] disabled: bool,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let ctx = use_context::<AccordionContext>();
    let open = ctx.is_open(&value);
    let disabled = disabled || ctx.disabled;
    let state = if open { "open" } else { "closed" };
    // 把本项的 value 下发给 AccordionTrigger / AccordionContent
    use_context_provider(|| AccordionItemContext {
        value: value.clone(),
        disabled,
    });

    rsx! {
        div {
            class: accordion_item_classes(&class),
            "data-slot": "accordion-item",
            "data-state": state,
            "data-disabled": disabled.then_some(""),
            "data-orientation": ctx.orientation.as_str(),
            {children}
        }
    }
}

/// 手风琴触发按钮（自动追加 lucide `chevron-down` / `chevron-up` 两个图标）。
///
/// 图标不是由调用方传入，而是像 TS 一样由本组件内部追加在 `{children}` 之后 ——
/// 两个图标都带 `data-slot="accordion-trigger-icon"`，靠 `group-aria-expanded`
/// 与 trigger 基类里的 `**:data-[slot=accordion-trigger-icon]:size-4` 联动。
#[component]
pub fn AccordionTrigger(
    #[props(default)] disabled: bool,
    #[props(default, into)] class: String,
    #[props(default, into)] id: String,
    children: Element,
) -> Element {
    let ctx = use_context::<AccordionContext>();
    // 触发器需要知道「自己属于哪一项」才能读写展开状态：由最近的 AccordionItem 通过
    // 一个嵌套 context 传下来。
    let item = use_context::<AccordionItemContext>();
    let open = ctx.is_open(&item.value);
    let disabled = disabled || ctx.disabled || item.disabled;
    let state = if open { "open" } else { "closed" };
    let mut ctx_mut = ctx;
    let value = item.value.clone();
    let controls = (!id.is_empty()).then(|| format!("{id}-content"));
    // Radix 用 `useId()` 自动生成 id；本移植没有等价物，改由调用方显式给。
    // 没给就整个属性不发 —— 发 `id=""` 是无效 HTML。
    let id_attr = (!id.is_empty()).then_some(id);

    rsx! {
        h3 {
            class: "flex",
            "data-orientation": ctx.orientation.as_str(),
            button {
                class: accordion_trigger_classes(&class),
                r#type: "button",
                "data-slot": "accordion-trigger",
                "data-state": state,
                "data-disabled": disabled.then_some(""),
                "data-orientation": ctx.orientation.as_str(),
                disabled: disabled,
                id: id_attr,
                "aria-expanded": if open { "true" } else { "false" },
                "aria-controls": controls,
                onclick: move |_| ctx_mut.toggle(&value),
                {children}
                IconChevronDown {
                    class: ACCORDION_TRIGGER_ICON_DOWN,
                    slot: "accordion-trigger-icon",
                }
                IconChevronUp {
                    class: ACCORDION_TRIGGER_ICON_UP,
                    slot: "accordion-trigger-icon",
                }
            }
        }
    }
}

/// 手风琴内容区。
#[component]
pub fn AccordionContent(
    #[props(default, into)] class: String,
    #[props(default, into)] id: String,
    #[props(default, into)] inner_class: String,
    children: Element,
) -> Element {
    let ctx = use_context::<AccordionContext>();
    let item = use_context::<AccordionItemContext>();
    let open = ctx.is_open(&item.value);
    let state = if open { "open" } else { "closed" };
    let id_attr = (!id.is_empty()).then_some(id);

    rsx! {
        div {
            class: accordion_content_classes(&class),
            "data-slot": "accordion-content",
            "data-state": state,
            "data-disabled": (ctx.disabled || item.disabled).then_some(""),
            "data-orientation": ctx.orientation.as_str(),
            id: id_attr,
            role: "region",
            hidden: !open,
            div {
                class: accordion_content_inner_classes(&inner_class),
                {children}
            }
        }
    }
}

/// `AccordionItem` → 其子组件之间的私有 context（携带本项的 `value`）。
#[derive(Clone, PartialEq)]
struct AccordionItemContext {
    value: String,
    disabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_strings() {
        assert_eq!(AccordionType::Single.as_str(), "single");
        assert_eq!(AccordionType::Multiple.as_str(), "multiple");
        assert_eq!(AccordionType::default(), AccordionType::Single);
    }

    #[test]
    fn root_base() {
        let c = accordion_classes("");
        assert!(c.contains("flex w-full flex-col"));
        assert!(c.contains("rounded-md"));
        assert!(c.contains("border"));
    }

    #[test]
    fn item_uses_data_open_variant() {
        let c = accordion_item_classes("");
        assert!(c.contains("not-last:border-b"));
        assert!(c.contains("data-open:bg-muted/50"));
    }

    #[test]
    fn trigger_has_icon_rules() {
        let c = accordion_trigger_classes("");
        assert!(c.contains("group/accordion-trigger"));
        assert!(c.contains("**:data-[slot=accordion-trigger-icon]:ml-auto"));
        assert!(c.contains("**:data-[slot=accordion-trigger-icon]:size-4"));
        assert!(c.contains("text-ui-base/relaxed"));
    }

    #[test]
    fn trigger_icons_toggle_on_aria_expanded() {
        assert!(ACCORDION_TRIGGER_ICON_DOWN.contains("group-aria-expanded/accordion-trigger:hidden"));
        assert!(ACCORDION_TRIGGER_ICON_UP.contains("hidden"));
        assert!(ACCORDION_TRIGGER_ICON_UP.contains("group-aria-expanded/accordion-trigger:inline"));
    }

    #[test]
    fn content_base() {
        let c = accordion_content_classes("");
        assert!(c.contains("data-open:animate-accordion-down"));
        assert!(c.contains("data-closed:animate-accordion-up"));
    }

    #[test]
    fn content_inner_keeps_radix_height_var() {
        // 保留类名以与 TS 逐字对齐；无 Radix 时该变量未定义，height 回退 auto（见模块文档）
        let c = accordion_content_inner_classes("");
        assert!(c.contains("h-(--radix-accordion-content-height)"));
        assert!(c.contains("[&_p:not(:last-child)]:mb-4"));
    }
}
