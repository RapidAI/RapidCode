//! `Avatar` 家族 —— Rust 移植自 `packages/ui/src/components/ui/avatar.tsx`
//!
//! TS 用 Radix `Avatar.Root/Image/Fallback`。Radix 的 `Image` 带「加载失败自动
//! 回退到 Fallback」的运行时逻辑（依赖 `img.onload/onerror` + 延迟挂载）。
//! Dioxus 侧用 `onerror` 事件 + 信号复刻这一行为：图片加载失败即切到 fallback。

use crate::cn::cn;
use dioxus::prelude::*;

/// 头像尺寸。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AvatarSize {
    Sm,
    #[default]
    Default,
    Lg,
}

impl AvatarSize {
    pub fn as_str(&self) -> &'static str {
        match self {
            AvatarSize::Sm => "sm",
            AvatarSize::Default => "default",
            AvatarSize::Lg => "lg",
        }
    }
}

/// `Avatar` 根容器基础类名。
pub const AVATAR_BASE: &str = "group/avatar relative flex size-8 shrink-0 rounded-full select-none after:absolute after:inset-0 after:rounded-full after:border after:border-border after:mix-blend-darken data-[size=lg]:size-10 data-[size=sm]:size-6 dark:after:mix-blend-lighten";

/// `AvatarImage` 基础类名。
pub const AVATAR_IMAGE_BASE: &str = "aspect-square size-full rounded-full object-cover";

/// `AvatarFallback` 基础类名。
pub const AVATAR_FALLBACK_BASE: &str = "flex size-full items-center justify-center rounded-full bg-muted text-ui-base text-muted-foreground group-data-[size=sm]/avatar:text-ui-base";

/// `AvatarBadge` 基础类名。
pub const AVATAR_BADGE_BASE: &str = "absolute right-0 bottom-0 z-10 inline-flex items-center justify-center rounded-full bg-primary text-primary-foreground bg-blend-color ring-2 ring-background select-none group-data-[size=sm]/avatar:size-2 group-data-[size=sm]/avatar:[&>svg]:hidden group-data-[size=default]/avatar:size-2.5 group-data-[size=default]/avatar:[&>svg]:size-2 group-data-[size=lg]/avatar:size-3 group-data-[size=lg]/avatar:[&>svg]:size-2";

/// `AvatarGroup` 基础类名。
pub const AVATAR_GROUP_BASE: &str = "group/avatar-group flex -space-x-2 *:data-[slot=avatar]:ring-2 *:data-[slot=avatar]:ring-background";

/// `AvatarGroupCount` 基础类名。
pub const AVATAR_GROUP_COUNT_BASE: &str = "relative flex size-8 shrink-0 items-center justify-center rounded-full bg-muted text-ui-base/relaxed text-muted-foreground ring-2 ring-background group-has-data-[size=lg]/avatar-group:size-10 group-has-data-[size=sm]/avatar-group:size-6 [&>svg]:size-4 group-has-data-[size=lg]/avatar-group:[&>svg]:size-5 group-has-data-[size=sm]/avatar-group:[&>svg]:size-3";

pub fn avatar_classes(class: &str) -> String {
    cn(&[AVATAR_BASE, class])
}

pub fn avatar_image_classes(class: &str) -> String {
    cn(&[AVATAR_IMAGE_BASE, class])
}

pub fn avatar_fallback_classes(class: &str) -> String {
    cn(&[AVATAR_FALLBACK_BASE, class])
}

pub fn avatar_badge_classes(class: &str) -> String {
    cn(&[AVATAR_BADGE_BASE, class])
}

pub fn avatar_group_classes(class: &str) -> String {
    cn(&[AVATAR_GROUP_BASE, class])
}

pub fn avatar_group_count_classes(class: &str) -> String {
    cn(&[AVATAR_GROUP_COUNT_BASE, class])
}

/// 头像根容器。
#[component]
pub fn Avatar(
    #[props(default)] size: AvatarSize,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        span {
            class: avatar_classes(&class),
            "data-slot": "avatar",
            "data-size": size.as_str(),
            {children}
        }
    }
}

/// 头像图片。加载失败时自动隐藏自身（由外层 `Avatar` + `AvatarFallback` 承接）。
#[component]
pub fn AvatarImage(
    #[props(default, into)] class: String,
    #[props(default, into)] src: String,
    #[props(default, into)] alt: String,
) -> Element {
    let mut failed = use_signal(|| false);
    if failed() {
        return rsx! {};
    }
    rsx! {
        img {
            class: avatar_image_classes(&class),
            "data-slot": "avatar-image",
            src: src,
            alt: alt,
            onerror: move |_| failed.set(true),
        }
    }
}

/// 头像占位（图片缺失或加载失败时展示）。
#[component]
pub fn AvatarFallback(#[props(default, into)] class: String, children: Element) -> Element {
    rsx! {
        span {
            class: avatar_fallback_classes(&class),
            "data-slot": "avatar-fallback",
            {children}
        }
    }
}

/// 头像角标（如在线状态圆点）。
#[component]
pub fn AvatarBadge(#[props(default, into)] class: String, children: Element) -> Element {
    rsx! {
        span {
            class: avatar_badge_classes(&class),
            "data-slot": "avatar-badge",
            {children}
        }
    }
}

/// 头像组容器（负外边距叠放）。
#[component]
pub fn AvatarGroup(#[props(default, into)] class: String, children: Element) -> Element {
    rsx! {
        div {
            class: avatar_group_classes(&class),
            "data-slot": "avatar-group",
            {children}
        }
    }
}

/// 头像组末尾的「+N」计数块。
#[component]
pub fn AvatarGroupCount(#[props(default, into)] class: String, children: Element) -> Element {
    rsx! {
        div {
            class: avatar_group_count_classes(&class),
            "data-slot": "avatar-group-count",
            {children}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_size_strings() {
        assert_eq!(AvatarSize::Sm.as_str(), "sm");
        assert_eq!(AvatarSize::Default.as_str(), "default");
        assert_eq!(AvatarSize::Lg.as_str(), "lg");
        assert_eq!(AvatarSize::default(), AvatarSize::Default);
    }

    #[test]
    fn avatar_base_has_size_variants() {
        let c = avatar_classes("");
        assert!(c.contains("size-8"));
        assert!(c.contains("data-[size=lg]:size-10"));
        assert!(c.contains("data-[size=sm]:size-6"));
    }

    #[test]
    fn image_and_fallback_bases() {
        assert!(avatar_image_classes("").contains("object-cover"));
        assert!(avatar_fallback_classes("").contains("bg-muted"));
    }

    #[test]
    fn group_bases() {
        assert!(avatar_group_classes("").contains("-space-x-2"));
        assert!(avatar_group_count_classes("").contains("ring-2"));
    }

    #[test]
    fn user_class_overrides_size() {
        let c = avatar_classes("size-12");
        assert!(c.contains("size-12"));
        assert!(!c.contains("size-8"));
    }
}
