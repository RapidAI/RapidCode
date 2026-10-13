//! `Tabs` 家族 —— Rust 移植自 `packages/ui/src/components/ui/tabs.tsx`
//!
//! TS 用 Radix `Tabs.Root/List/Trigger/Content`（内部通过 React Context 共享当前
//! 选中值）。Rust 侧用 Dioxus 的 `use_context_provider` / `use_context` 复刻同一套
//! 结构。
//!
//! ## 与 TS 的唯一设计差异
//!
//! Radix 的 `Tabs.Root` 同时支持受控（`value`）与非受控（`defaultValue`）两种模式，
//! 靠内部 state 兜底。Dioxus 的 context 只在**首次渲染**建立，把内部 `Signal` 塞进
//! context 会在受控模式下读到陈旧值。因此这里统一采用 **调用方持有状态** 的写法：
//!
//! ```ignore
//! let tab = use_signal(|| "general".to_string());
//! rsx! {
//!     Tabs { value: tab,
//!         TabsList {
//!             TabsTrigger { value: "general", "常规" }
//!             TabsTrigger { value: "advanced", "高级" }
//!         }
//!         TabsContent { value: "general", "…" }
//!     }
//! }
//! ```
//!
//! `Signal<T>` 是**句柄**（generational box），其标识跨渲染稳定，所以 context 里
//! 存的句柄永远指向最新值 —— 既没有陈旧问题，也天然同时覆盖受控/非受控两种用法。

use crate::cn::cn;
use dioxus::prelude::*;

/// Tabs 方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TabsOrientation {
    #[default]
    Horizontal,
    Vertical,
}

impl TabsOrientation {
    pub fn as_str(&self) -> &'static str {
        match self {
            TabsOrientation::Horizontal => "horizontal",
            TabsOrientation::Vertical => "vertical",
        }
    }
}

/// `TabsList` 视觉变体（对应 TS 的 `tabsListVariants.variants.variant`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TabsListVariant {
    #[default]
    Default,
    Line,
}

impl TabsListVariant {
    pub fn as_str(&self) -> &'static str {
        match self {
            TabsListVariant::Default => "default",
            TabsListVariant::Line => "line",
        }
    }

    pub fn classes(&self) -> &'static str {
        match self {
            TabsListVariant::Default => "bg-muted",
            TabsListVariant::Line => "gap-1 bg-transparent",
        }
    }
}

/// `Tabs` 根容器基础类名。
pub const TABS_BASE: &str = "group/tabs flex data-horizontal:flex-col";

/// `TabsList` 基础类名（对应 `cva` 的第一个参数）。
pub const TABS_LIST_BASE: &str = "group/tabs-list inline-flex w-fit items-center justify-center rounded-lg p-[3px] text-muted-foreground group-data-horizontal/tabs:h-8 group-data-vertical/tabs:h-fit group-data-vertical/tabs:flex-col data-[variant=line]:rounded-none";

/// `TabsTrigger` 基础类名。
pub const TABS_TRIGGER_BASE: &str = "relative inline-flex h-[calc(100%-1px)] flex-1 items-center justify-center gap-1.5 rounded-md border border-transparent px-1.5 py-0.5 text-ui-base font-medium whitespace-nowrap text-foreground/60 transition-all group-data-vertical/tabs:w-full group-data-vertical/tabs:justify-start group-data-vertical/tabs:py-[calc(--spacing(1.25))] hover:text-foreground focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-1 focus-visible:outline-ring disabled:pointer-events-none disabled:opacity-50 dark:text-muted-foreground dark:hover:text-foreground [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-3.5 group-data-[variant=line]/tabs-list:bg-transparent group-data-[variant=line]/tabs-list:data-active:bg-transparent dark:group-data-[variant=line]/tabs-list:data-active:border-transparent dark:group-data-[variant=line]/tabs-list:data-active:bg-transparent data-active:bg-background data-active:text-foreground dark:data-active:border-input dark:data-active:bg-input/30 dark:data-active:text-foreground after:absolute after:bg-foreground after:opacity-0 after:transition-opacity group-data-horizontal/tabs:after:inset-x-0 group-data-horizontal/tabs:after:bottom-[-5px] group-data-horizontal/tabs:after:h-0.5 group-data-vertical/tabs:after:inset-y-0 group-data-vertical/tabs:after:-right-1 group-data-vertical/tabs:after:w-0.5 group-data-[variant=line]/tabs-list:data-active:after:opacity-100";

/// `TabsContent` 基础类名。
pub const TABS_CONTENT_BASE: &str = "flex-1 text-ui-base outline-none";

pub fn tabs_classes(class: &str) -> String {
    cn(&[TABS_BASE, class])
}

pub fn tabs_list_classes(variant: TabsListVariant, class: &str) -> String {
    cn(&[TABS_LIST_BASE, variant.classes(), class])
}

pub fn tabs_trigger_classes(class: &str) -> String {
    cn(&[TABS_TRIGGER_BASE, class])
}

pub fn tabs_content_classes(class: &str) -> String {
    cn(&[TABS_CONTENT_BASE, class])
}

/// 由 `Tabs` 提供、`TabsTrigger` / `TabsContent` 消费的共享状态。
#[derive(Clone, Copy)]
struct TabsContext {
    value: Signal<String>,
}

/// 读取当前选中的 tab 值（供自定义触发器复用）。
pub fn use_tabs_value() -> Signal<String> {
    use_context::<TabsContext>().value
}

/// Tabs 根容器。`value` 由调用方持有（见模块文档）。
#[component]
pub fn Tabs(
    value: Signal<String>,
    #[props(default)] orientation: TabsOrientation,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    use_context_provider(|| TabsContext { value });

    rsx! {
        div {
            class: tabs_classes(&class),
            "data-slot": "tabs",
            "data-orientation": orientation.as_str(),
            {children}
        }
    }
}

/// Tab 按钮条。
#[component]
pub fn TabsList(
    #[props(default)] variant: TabsListVariant,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: tabs_list_classes(variant, &class),
            "data-slot": "tabs-list",
            "data-variant": variant.as_str(),
            role: "tablist",
            {children}
        }
    }
}

/// 单个 Tab 按钮。
#[component]
pub fn TabsTrigger(
    /// 该触发器对应的取值。
    #[props(into)] value: String,
    #[props(default, into)] class: String,
    #[props(default)] disabled: bool,
    children: Element,
) -> Element {
    let mut ctx = use_context::<TabsContext>();
    let active = ctx.value.read().as_str() == value.as_str();
    let state = if active { "active" } else { "inactive" };
    let own_value = value.clone();

    rsx! {
        button {
            class: tabs_trigger_classes(&class),
            r#type: "button",
            role: "tab",
            "data-slot": "tabs-trigger",
            "data-state": state,
            "data-value": value,
            "aria-selected": if active { "true" } else { "false" },
            disabled: disabled,
            onclick: move |_| {
                if !disabled {
                    ctx.value.set(own_value.clone());
                }
            },
            {children}
        }
    }
}

/// Tab 内容面板。
///
/// 差异：Radix 默认**卸载**非激活面板；这里保持挂载并用 `hidden` 隐藏，
/// 以免切换 tab 时丢失子组件内部状态（更贴近桌面端长驻面板的使用习惯）。
#[component]
pub fn TabsContent(
    /// 该面板对应的取值。
    #[props(into)] value: String,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let ctx = use_context::<TabsContext>();
    let active = ctx.value.read().as_str() == value.as_str();
    let state = if active { "active" } else { "inactive" };

    rsx! {
        div {
            class: tabs_content_classes(&class),
            "data-slot": "tabs-content",
            "data-state": state,
            role: "tabpanel",
            hidden: !active,
            {children}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientation_strings() {
        assert_eq!(TabsOrientation::Horizontal.as_str(), "horizontal");
        assert_eq!(TabsOrientation::Vertical.as_str(), "vertical");
        assert_eq!(TabsOrientation::default(), TabsOrientation::Horizontal);
    }

    #[test]
    fn list_variant_classes() {
        assert_eq!(TabsListVariant::Default.classes(), "bg-muted");
        assert_eq!(TabsListVariant::Line.classes(), "gap-1 bg-transparent");
        assert_eq!(TabsListVariant::default(), TabsListVariant::Default);
    }

    #[test]
    fn list_base_has_orientation_variants() {
        let c = tabs_list_classes(TabsListVariant::Default, "");
        assert!(c.contains("group-data-horizontal/tabs:h-8"));
        assert!(c.contains("group-data-vertical/tabs:flex-col"));
        assert!(c.contains("data-[variant=line]:rounded-none"));
        assert!(c.contains("bg-muted"));
    }

    #[test]
    fn list_line_variant_merges_gap() {
        let c = tabs_list_classes(TabsListVariant::Line, "");
        assert!(c.contains("gap-1"));
        assert!(c.contains("bg-transparent"));
        assert!(!c.contains("bg-muted"));
    }

    #[test]
    fn trigger_base_has_active_and_line_variants() {
        let c = tabs_trigger_classes("");
        assert!(c.contains("data-active:bg-background"));
        assert!(c.contains("group-data-[variant=line]/tabs-list:data-active:after:opacity-100"));
        assert!(c.contains("text-ui-base"));
    }

    #[test]
    fn trigger_user_class_overrides() {
        let c = tabs_trigger_classes("text-ui-lg");
        assert!(c.contains("text-ui-lg"));
        assert!(!c.contains("text-ui-base "));
    }

    #[test]
    fn content_base() {
        assert!(tabs_content_classes("").contains("flex-1"));
        assert!(tabs_content_classes("p-2").contains("p-2"));
    }
}
