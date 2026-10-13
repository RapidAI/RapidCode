//! `Card` 家族 —— Rust 移植自 `packages/ui/src/components/ui/card.tsx`
//!
//! 包含 `Card` / `CardHeader` / `CardTitle` / `CardDescription` / `CardAction`
//! / `CardContent` / `CardFooter`。

use crate::cn::cn;
use dioxus::prelude::*;

/// 卡片尺寸。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CardSize {
    #[default]
    Default,
    Sm,
}

impl CardSize {
    pub fn as_str(&self) -> &'static str {
        match self {
            CardSize::Default => "default",
            CardSize::Sm => "sm",
        }
    }
}

pub const CARD_BASE: &str = "group/card flex flex-col gap-4 overflow-hidden rounded-xl border border-card-border bg-card py-4 text-ui-base/relaxed text-foreground has-[>img:first-child]:pt-0 data-[size=sm]:gap-3 data-[size=sm]:py-3 *:[img:first-child]:rounded-t-xl *:[img:last-child]:rounded-b-xl";
pub const CARD_HEADER_BASE: &str = "group/card-header @container/card-header grid auto-rows-min items-start gap-1 rounded-t-xl px-4 group-data-[size=sm]/card:px-3 has-data-[slot=card-action]:grid-cols-[1fr_auto] has-data-[slot=card-description]:grid-rows-[auto_auto] [.border-b]:pb-4 group-data-[size=sm]/card:[.border-b]:pb-3";
pub const CARD_TITLE_BASE: &str = "text-ui-base font-medium";
pub const CARD_DESCRIPTION_BASE: &str = "text-ui-base/relaxed text-foreground-subtle";
pub const CARD_ACTION_BASE: &str = "col-start-2 row-span-2 row-start-1 self-start justify-self-end";
pub const CARD_CONTENT_BASE: &str = "px-4 group-data-[size=sm]/card:px-3";
pub const CARD_FOOTER_BASE: &str = "flex items-center rounded-b-xl px-4 group-data-[size=sm]/card:px-3 [.border-t]:pt-4 group-data-[size=sm]/card:[.border-t]:pt-3";

/// 组合卡片最终类名。
///
/// 注意：与 TS 一致，`size` **不影响类名**，只作为 `data-size` 属性输出，
/// 由 `data-[size=sm]:*` / `group-data-[size=sm]/card:*` 变体在 CSS 侧生效。
pub fn card_classes(class: &str) -> String {
    cn(&[CARD_BASE, class])
}

/// 卡片根容器。
#[component]
pub fn Card(
    #[props(default)] size: CardSize,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let cls = card_classes(&class);
    rsx! {
        div {
            class: cls,
            "data-slot": "card",
            "data-size": size.as_str(),
            {children}
        }
    }
}

#[component]
pub fn CardHeader(#[props(default, into)] class: String, children: Element) -> Element {
    rsx! { div { class: cn(&[CARD_HEADER_BASE, &class]), "data-slot": "card-header", {children} } }
}

#[component]
pub fn CardTitle(#[props(default, into)] class: String, children: Element) -> Element {
    rsx! { div { class: cn(&[CARD_TITLE_BASE, &class]), "data-slot": "card-title", {children} } }
}

#[component]
pub fn CardDescription(#[props(default, into)] class: String, children: Element) -> Element {
    rsx! { div { class: cn(&[CARD_DESCRIPTION_BASE, &class]), "data-slot": "card-description", {children} } }
}

#[component]
pub fn CardAction(#[props(default, into)] class: String, children: Element) -> Element {
    rsx! { div { class: cn(&[CARD_ACTION_BASE, &class]), "data-slot": "card-action", {children} } }
}

#[component]
pub fn CardContent(#[props(default, into)] class: String, children: Element) -> Element {
    rsx! { div { class: cn(&[CARD_CONTENT_BASE, &class]), "data-slot": "card-content", {children} } }
}

#[component]
pub fn CardFooter(#[props(default, into)] class: String, children: Element) -> Element {
    rsx! { div { class: cn(&[CARD_FOOTER_BASE, &class]), "data-slot": "card-footer", {children} } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_base_contains_container_query() {
        let c = card_classes("");
        assert!(c.contains("rounded-xl"));
        assert!(c.contains("@container/card-header") == false); // header 类不在 card 上
    }

    #[test]
    fn size_is_attribute_only_not_class() {
        // TS 里 size 只写进 data-size，不改类名：类名恒等于 base + user class
        assert_eq!(card_classes(""), CARD_BASE);
        // base 里带的是 data-[size=sm]:* 变体，而不是「已应用的 sm 尺寸类」
        assert!(card_classes("").contains("data-[size=sm]:gap-3"));
        assert!(card_classes("").contains("data-[size=sm]:py-3"));
        // 不存在「裸」的 gap-3 / py-3 —— 它们只作为 data-[size=sm] 变体存在
        assert!(!card_classes("").split_whitespace().any(|t| t == "gap-3"));
        assert!(!card_classes("").split_whitespace().any(|t| t == "py-3"));
        assert!(card_classes("").split_whitespace().any(|t| t == "py-4"));
    }

    #[test]
    fn user_class_appended() {
        let c = card_classes("shadow-lg");
        assert!(c.contains("shadow-lg"));
        assert!(c.contains("bg-card"));
    }
}
