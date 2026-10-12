//! `Alert` 家族 —— Rust 移植自 `packages/ui/src/components/ui/alert.tsx`

use crate::cn::cn;
use dioxus::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AlertVariant {
    #[default]
    Default,
    Warning,
    Destructive,
}

impl AlertVariant {
    pub fn as_str(&self) -> &'static str {
        match self {
            AlertVariant::Default => "default",
            AlertVariant::Warning => "warning",
            AlertVariant::Destructive => "destructive",
        }
    }

    pub fn classes(&self) -> &'static str {
        match self {
            AlertVariant::Default => "border-card-border bg-card text-foreground",
            AlertVariant::Warning => {
                "border-warning/40 bg-warning/10 text-warning *:data-[slot=alert-description]:text-warning *:[svg]:text-current"
            }
            AlertVariant::Destructive => {
                "border-destructive bg-destructive text-destructive-foreground *:data-[slot=alert-description]:text-destructive-foreground/90 *:[svg]:text-current"
            }
        }
    }
}

pub const ALERT_BASE: &str = "group/alert relative grid w-full gap-0.5 rounded-lg border px-2 py-1.5 text-left text-ui-base/relaxed has-data-[slot=alert-action]:relative has-data-[slot=alert-action]:pr-18 has-[>svg]:grid-cols-[auto_1fr] has-[>svg]:gap-x-1.5 *:[svg]:row-span-2 *:[svg]:translate-y-0.5 *:[svg]:text-current *:[svg:not([class*='size-'])]:size-3.5";
pub const ALERT_TITLE_BASE: &str = "font-medium group-has-[>svg]/alert:col-start-2 [&_a]:underline [&_a]:underline-offset-3 [&_a]:hover:text-foreground";
pub const ALERT_DESCRIPTION_BASE: &str = "text-ui-base/relaxed text-balance text-foreground-subtle md:text-pretty [&_a]:underline [&_a]:underline-offset-3 [&_a]:hover:text-foreground [&_p:not(:last-child)]:mb-4";
pub const ALERT_ACTION_BASE: &str = "absolute top-1.5 right-2";

pub fn alert_classes(variant: AlertVariant, class: &str) -> String {
    cn(&[ALERT_BASE, variant.classes(), class])
}

#[component]
pub fn Alert(
    #[props(default)] variant: AlertVariant,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let cls = alert_classes(variant, &class);
    rsx! {
        div {
            class: cls,
            role: "alert",
            "data-slot": "alert",
            "data-variant": variant.as_str(),
            {children}
        }
    }
}

#[component]
pub fn AlertTitle(#[props(default, into)] class: String, children: Element) -> Element {
    rsx! { div { class: cn(&[ALERT_TITLE_BASE, &class]), "data-slot": "alert-title", {children} } }
}

#[component]
pub fn AlertDescription(#[props(default, into)] class: String, children: Element) -> Element {
    rsx! { div { class: cn(&[ALERT_DESCRIPTION_BASE, &class]), "data-slot": "alert-description", {children} } }
}

#[component]
pub fn AlertAction(#[props(default, into)] class: String, children: Element) -> Element {
    rsx! { div { class: cn(&[ALERT_ACTION_BASE, &class]), "data-slot": "alert-action", {children} } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_alert() {
        let c = alert_classes(AlertVariant::Default, "");
        assert!(c.contains("border-card-border"));
        assert!(c.contains("rounded-lg"));
    }

    #[test]
    fn warning_variant() {
        assert!(alert_classes(AlertVariant::Warning, "").contains("bg-warning/10"));
    }
}
