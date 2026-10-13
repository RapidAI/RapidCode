//! `Kbd` / `KbdGroup` —— Rust 移植自 `packages/ui/src/components/ui/kbd.tsx`
//!
//! 键帽组件：每个按键独立成 chip，固定高度 + 最小方形宽度 + flex 居中，
//! 保证 ⌘/⇧ 等单符号键与字母键视觉尺寸一致。

use crate::cn::cn;
use dioxus::prelude::*;

/// `Kbd` 基础类名。
pub const KBD_BASE: &str = "pointer-events-none inline-flex h-5 w-fit min-w-5 select-none items-center justify-center gap-1 rounded-sm bg-muted px-1 font-sans text-ui-xs font-medium text-muted-foreground";

/// `KbdGroup` 基础类名。
pub const KBD_GROUP_BASE: &str = "inline-flex items-center gap-1";

pub fn kbd_classes(class: &str) -> String {
    cn(&[KBD_BASE, class])
}

pub fn kbd_group_classes(class: &str) -> String {
    cn(&[KBD_GROUP_BASE, class])
}

/// 单个键帽。
#[component]
pub fn Kbd(#[props(default, into)] class: String, children: Element) -> Element {
    rsx! {
        kbd {
            class: kbd_classes(&class),
            "data-slot": "kbd",
            {children}
        }
    }
}

/// 键帽组合容器：把一次组合的多个键横排（`gap-1`），如 `⌘ + K`。
#[component]
pub fn KbdGroup(#[props(default, into)] class: String, children: Element) -> Element {
    rsx! {
        div {
            class: kbd_group_classes(&class),
            "data-slot": "kbd-group",
            {children}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kbd_base() {
        let c = kbd_classes("");
        assert!(c.contains("h-5"));
        assert!(c.contains("min-w-5"));
        assert!(c.contains("font-sans"));
        assert!(c.contains("text-ui-xs"));
    }

    #[test]
    fn kbd_group_base() {
        assert!(kbd_group_classes("").contains("gap-1"));
        assert!(kbd_group_classes("ml-2").contains("ml-2"));
    }
}
