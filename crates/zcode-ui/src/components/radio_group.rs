//! `RadioGroup` 家族 —— Rust 移植自 `packages/ui/src/components/ui/radio-group.tsx`
//!
//! TS 用 Radix `RadioGroup.Root/Item/Indicator`，指示器图标来自 lucide-react 的
//! `Circle`。Rust 侧渲染同样的 `div[role=radiogroup]` + `button[role=radio]`，
//! 并显式输出 `data-state="checked|unchecked"` / `aria-checked`。
//!
//! ## 与 TS 的差异
//!
//! - Radix 的 `Indicator` 只在选中时挂载；这里同样**只在选中时渲染**圆点
//!   （与 [`crate::components::checkbox`] 的「始终挂载」策略相反 —— 因为 radio 的
//!   圆点没有像 checkbox 那样被 `[&[data-state=...]_[data-slot=...]]` 类名控制的显隐，
//!   少一个 `if` 反而更贴近 Radix）。
//! - 方向键导航（Radix 的 roving tabindex）未实现：桌面端由鼠标/触控板驱动，
//!   且 `tabindex` 的 roving 逻辑需要真实焦点管理，留待接入渲染器后补齐。

use crate::cn::cn;
use crate::icons::IconCircle;
use dioxus::prelude::*;

/// `RadioGroup` 根容器基础类名。
pub const RADIO_GROUP_BASE: &str = "grid gap-2";

/// `RadioGroupItem` 基础类名。
pub const RADIO_GROUP_ITEM_BASE: &str = "aspect-square h-4 w-4 rounded-full border border-border text-primary ring-offset-background focus:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50";

/// `RadioGroupItem` 指示器包裹层类名。
pub const RADIO_GROUP_INDICATOR_BASE: &str = "flex items-center justify-center";

/// 指示器圆点类名。
pub const RADIO_GROUP_DOT_BASE: &str = "h-2.5 w-2.5 fill-current text-current";

pub fn radio_group_classes(class: &str) -> String {
    cn(&[RADIO_GROUP_BASE, class])
}

pub fn radio_group_item_classes(class: &str) -> String {
    cn(&[RADIO_GROUP_ITEM_BASE, class])
}

/// 由 `RadioGroup` 提供、`RadioGroupItem` 消费的共享状态。
#[derive(Clone, Copy)]
struct RadioGroupContext {
    value: Signal<String>,
    disabled: bool,
}

/// 读取当前选中值（供自定义选项复用）。
pub fn use_radio_group_value() -> Signal<String> {
    use_context::<RadioGroupContext>().value
}

/// 单选组根容器。
#[component]
pub fn RadioGroup(
    /// 受控模式：由调用方持有选中值。不传则使用内部状态。
    #[props(default)] value: Option<Signal<String>>,
    /// 非受控模式下的初始选中值（对应 Radix 的 `defaultValue`）。
    #[props(default, into)] default_value: String,
    #[props(default)] disabled: bool,
    #[props(default, into)] class: String,
    /// 选中值变化回调（受控模式下由调用方更新自己的 signal）。
    #[props(default)] on_value_change: EventHandler<String>,
    children: Element,
) -> Element {
    let internal = use_signal(|| default_value);
    let value = value.unwrap_or(internal);
    use_context_provider(|| RadioGroupContext { value, disabled });

    rsx! {
        div {
            class: radio_group_classes(&class),
            "data-slot": "radio-group",
            role: "radiogroup",
            "data-disabled": disabled.then_some(""),
            {children}
        }
    }
}

/// 单个单选项。
#[component]
pub fn RadioGroupItem(
    /// 该选项的取值。
    #[props(into)] value: String,
    #[props(default)] disabled: bool,
    #[props(default, into)] class: String,
    /// 覆盖圆点类名。
    #[props(default, into)] indicator_class: String,
) -> Element {
    let mut ctx = use_context::<RadioGroupContext>();
    let checked = ctx.value.read().as_str() == value.as_str();
    let disabled = disabled || ctx.disabled;
    let state = if checked { "checked" } else { "unchecked" };
    let own_value = value.clone();

    rsx! {
        button {
            class: radio_group_item_classes(&class),
            r#type: "button",
            role: "radio",
            "data-slot": "radio-group-item",
            "data-state": state,
            "data-value": value,
            "aria-checked": if checked { "true" } else { "false" },
            "data-disabled": disabled.then_some(""),
            disabled: disabled,
            tabindex: if checked { "0" } else { "-1" },
            onclick: move |_| {
                if !disabled && !checked {
                    ctx.value.set(own_value.clone());
                }
            },
            if checked {
                span {
                    class: cn(&[RADIO_GROUP_INDICATOR_BASE, &indicator_class]),
                    "data-slot": "radio-group-indicator",
                    IconCircle { class: RADIO_GROUP_DOT_BASE }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_base() {
        assert_eq!(radio_group_classes(""), "grid gap-2");
        assert!(radio_group_classes("gap-4").contains("gap-4"));
        assert!(!radio_group_classes("gap-4").contains("gap-2"));
    }

    #[test]
    fn item_base() {
        let c = radio_group_item_classes("");
        assert!(c.contains("aspect-square"));
        assert!(c.contains("h-4"));
        assert!(c.contains("w-4"));
        assert!(c.contains("rounded-full"));
        assert!(c.contains("border-border"));
        assert!(c.contains("focus-visible:ring-2"));
    }

    #[test]
    fn dot_is_small_and_inherits_color() {
        assert!(RADIO_GROUP_DOT_BASE.contains("h-2.5"));
        assert!(RADIO_GROUP_DOT_BASE.contains("w-2.5"));
        assert!(RADIO_GROUP_DOT_BASE.contains("fill-current"));
        assert!(RADIO_GROUP_DOT_BASE.contains("text-current"));
    }
}
