//! `Badge` —— Rust 移植自 `packages/ui/src/components/ui/badge.tsx`

use crate::cn::cn;
use dioxus::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BadgeVariant {
    #[default]
    Default,
    Secondary,
    Destructive,
    Outline,
    Ghost,
    Link,
}

impl BadgeVariant {
    pub fn as_str(&self) -> &'static str {
        match self {
            BadgeVariant::Default => "default",
            BadgeVariant::Secondary => "secondary",
            BadgeVariant::Destructive => "destructive",
            BadgeVariant::Outline => "outline",
            BadgeVariant::Ghost => "ghost",
            BadgeVariant::Link => "link",
        }
    }

    pub fn classes(&self) -> &'static str {
        match self {
            BadgeVariant::Default => "bg-primary text-primary-foreground [a]:hover:bg-primary/80",
            BadgeVariant::Secondary => "bg-secondary text-secondary-foreground [a]:hover:bg-secondary/80",
            BadgeVariant::Destructive => {
                "bg-destructive/10 text-destructive focus-visible:ring-destructive/20 dark:bg-destructive/20 dark:focus-visible:ring-destructive/40 [a]:hover:bg-destructive/20"
            }
            BadgeVariant::Outline => {
                "border-border bg-input/20 text-foreground dark:bg-input/30 [a]:hover:bg-muted [a]:hover:text-muted-foreground"
            }
            BadgeVariant::Ghost => "hover:bg-muted hover:text-muted-foreground dark:hover:bg-muted/50",
            BadgeVariant::Link => "text-primary underline-offset-4 hover:underline",
        }
    }
}

pub const BADGE_BASE: &str = "group/badge inline-flex h-5 w-fit shrink-0 items-center justify-center gap-1 overflow-hidden rounded-full border border-transparent px-2 py-0.5 text-ui-xs font-medium whitespace-nowrap transition-all focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 has-data-[icon=inline-end]:pr-1.5 has-data-[icon=inline-start]:pl-1.5 aria-invalid:border-destructive aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 [&>svg]:pointer-events-none [&>svg]:size-2.5!";

pub fn badge_classes(variant: BadgeVariant, class: &str) -> String {
    cn(&[BADGE_BASE, variant.classes(), class])
}

#[component]
pub fn Badge(
    #[props(default)] variant: BadgeVariant,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let cls = badge_classes(variant, &class);
    rsx! {
        span {
            class: cls,
            "data-slot": "badge",
            "data-variant": variant.as_str(),
            {children}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_badge() {
        let c = badge_classes(BadgeVariant::Default, "");
        assert!(c.contains("rounded-full"));
        assert!(c.contains("bg-primary"));
        assert!(c.contains("text-ui-xs"));
    }

    #[test]
    fn user_class_overrides_bg() {
        let c = badge_classes(BadgeVariant::Default, "bg-accent");
        assert!(c.contains("bg-accent"));
        // 只覆盖「裸」的 bg-primary；[a]:hover:bg-primary/80 作用域不同，应保留
        assert!(!c.split_whitespace().any(|t| t == "bg-primary"));
        assert!(c.contains("[a]:hover:bg-primary/80"));
    }
}
