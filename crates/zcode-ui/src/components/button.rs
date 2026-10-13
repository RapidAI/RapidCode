//! `Button` —— Rust 移植自 `packages/ui/src/components/ui/button.tsx`
//!
//! TS 用 `cva`（class-variance-authority）定义 variant / size 的类名映射；
//! Rust 侧用「枚举 + `classes()`」表达同样的映射表，语义一致且类型安全。
//!
//! 注：TS 的 `asChild`（Radix `Slot`）用于把样式委托给子元素，Dioxus 0.6 无
//! 对应原语，故未移植；其余 props（variant / size / className / disabled）一一对应。

use crate::cn::cn;
use dioxus::prelude::*;

/// 按钮视觉变体（对应 `buttonVariants.variants.variant`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ButtonVariant {
    #[default]
    Default,
    Outline,
    Secondary,
    Ghost,
    Destructive,
    Warning,
    Link,
}

impl ButtonVariant {
    pub fn as_str(&self) -> &'static str {
        match self {
            ButtonVariant::Default => "default",
            ButtonVariant::Outline => "outline",
            ButtonVariant::Secondary => "secondary",
            ButtonVariant::Ghost => "ghost",
            ButtonVariant::Destructive => "destructive",
            ButtonVariant::Warning => "warning",
            ButtonVariant::Link => "link",
        }
    }

    pub fn classes(&self) -> &'static str {
        match self {
            ButtonVariant::Default => "bg-primary text-primary-foreground hover:bg-primary/80",
            ButtonVariant::Outline => {
                "border-border text-foreground hover:border-border-hover hover:bg-input/50 hover:text-foreground aria-expanded:bg-input/50 aria-expanded:text-foreground"
            }
            ButtonVariant::Secondary => {
                "bg-secondary text-foreground hover:bg-secondary/80 aria-expanded:bg-secondary aria-expanded:text-foreground"
            }
            ButtonVariant::Ghost => {
                "text-foreground hover:bg-hover hover:text-foreground aria-expanded:bg-hover aria-expanded:text-foreground"
            }
            ButtonVariant::Destructive => {
                "bg-destructive text-destructive-foreground hover:bg-destructive/90 focus-visible:border-destructive/40 focus-visible:ring-destructive/20 aria-expanded:bg-destructive aria-expanded:text-destructive-foreground"
            }
            ButtonVariant::Warning => {
                "bg-warning text-warning-foreground hover:bg-warning/90 focus-visible:border-warning/40 focus-visible:ring-warning/20 aria-expanded:bg-warning aria-expanded:text-warning-foreground"
            }
            ButtonVariant::Link => "text-primary underline-offset-4 hover:underline",
        }
    }
}

/// 按钮尺寸（对应 `buttonVariants.variants.size`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ButtonSize {
    #[default]
    Default,
    Xs,
    Sm,
    Lg,
    Icon,
    IconXs,
    IconSm,
    IconMd,
    IconLg,
}

impl ButtonSize {
    pub fn as_str(&self) -> &'static str {
        match self {
            ButtonSize::Default => "default",
            ButtonSize::Xs => "xs",
            ButtonSize::Sm => "sm",
            ButtonSize::Lg => "lg",
            ButtonSize::Icon => "icon",
            ButtonSize::IconXs => "icon-xs",
            ButtonSize::IconSm => "icon-sm",
            ButtonSize::IconMd => "icon-md",
            ButtonSize::IconLg => "icon-lg",
        }
    }

    pub fn classes(&self) -> &'static str {
        match self {
            ButtonSize::Default => {
                "h-7 gap-1 px-2 text-ui-base has-data-[icon=inline-end]:pr-1.5 has-data-[icon=inline-start]:pl-1.5 [&_svg:not([class*='size-'])]:size-3.5"
            }
            ButtonSize::Xs => {
                "h-5 gap-1 rounded-sm px-2 text-ui-base has-data-[icon=inline-end]:pr-1.5 has-data-[icon=inline-start]:pl-1.5 [&_svg:not([class*='size-'])]:size-2.5"
            }
            ButtonSize::Sm => {
                "h-6 gap-1 px-2 text-ui-base/relaxed has-data-[icon=inline-end]:pr-1.5 has-data-[icon=inline-start]:pl-1.5 [&_svg:not([class*='size-'])]:size-3"
            }
            ButtonSize::Lg => {
                "h-8 gap-1 rounded-lg px-2.5 text-ui-base has-data-[icon=inline-end]:pr-2 has-data-[icon=inline-start]:pl-2 [&_svg:not([class*='size-'])]:size-4"
            }
            ButtonSize::Icon => "size-7 [&_svg:not([class*='size-'])]:size-4",
            ButtonSize::IconXs => "size-5 rounded-sm [&_svg:not([class*='size-'])]:size-2.5",
            ButtonSize::IconSm => "size-6 [&_svg:not([class*='size-'])]:size-3",
            ButtonSize::IconMd => "size-7 rounded-lg [&_svg:not([class*='size-'])]:size-4",
            ButtonSize::IconLg => "size-8 rounded-lg [&_svg:not([class*='size-'])]:size-4",
        }
    }
}

/// 基础类名（对应 `cva` 的第一个参数）。
pub const BUTTON_BASE: &str = "group/button inline-flex shrink-0 items-center justify-center rounded-md border border-transparent bg-clip-padding text-ui-base/relaxed whitespace-nowrap transition-colors outline-none select-none disabled:pointer-events-none disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-2 aria-invalid:ring-destructive/20 dark:aria-invalid:border-destructive/50 dark:aria-invalid:ring-destructive/40 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4";

/// 组合按钮最终类名（等价 `cn(buttonVariants({ variant, size, className }))`）。
pub fn button_classes(variant: ButtonVariant, size: ButtonSize, class: &str) -> String {
    cn(&[BUTTON_BASE, variant.classes(), size.classes(), class])
}

/// 按钮组件。
#[component]
pub fn Button(
    #[props(default)] variant: ButtonVariant,
    #[props(default)] size: ButtonSize,
    #[props(default, into)] class: String,
    #[props(default)] disabled: bool,
    /// 对应 TS 里透传的 `type`（默认 `button`）。
    #[props(default = "button".to_string())] html_type: String,
    /// 覆盖 `data-slot`。
    #[props(default = "button")] slot: &'static str,
    /// 覆盖 `data-size`。TS 里 `InputGroupButton` 用 cva 的 size 覆盖了
    /// `buttonVariants` 的 size 属性，但类名仍走 `className`，两者不一致。
    #[props(default)] data_size: Option<&'static str>,
    /// 透传 `aria-label`（图标按钮或文案与标签分离时使用）。
    #[props(default)] aria_label: Option<String>,
    /// `mousedown`。少数调用点需要 `preventDefault` 来保住页面上的文本选区
    /// （见 `PptxSelectionActionBar`：按钮拿到鼠标焦点前浏览器可能清空 PPTX
    /// 的 `Selection`，阻止默认 mousedown 仍保留 click / 键盘激活）。
    #[props(default)] onmousedown: EventHandler<MouseEvent>,
    #[props(default)] onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    let cls = button_classes(variant, size, &class);
    rsx! {
        button {
            class: cls,
            r#type: html_type,
            disabled: disabled,
            "data-slot": slot,
            "data-variant": variant.as_str(),
            "data-size": data_size.unwrap_or(size.as_str()),
            "aria-label": aria_label,
            onmousedown: move |e| onmousedown.call(e),
            onclick: move |e| onclick.call(e),
            {children}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_variant_and_size_present() {
        let c = button_classes(ButtonVariant::Default, ButtonSize::Default, "");
        assert!(c.contains("bg-primary"));
        assert!(c.contains("h-7"));
        assert!(c.contains("data-slot" ) == false); // 类名里没有 data-slot
    }

    #[test]
    fn user_class_overrides_defaults() {
        let c = button_classes(ButtonVariant::Default, ButtonSize::Default, "bg-accent h-9");
        assert!(c.contains("bg-accent"));
        // 只断言「裸」的 bg-primary 被覆盖；hover:bg-primary/80 属于 hover 作用域，应保留
        assert!(!c.split_whitespace().any(|t| t == "bg-primary"));
        assert!(c.contains("hover:bg-primary/80"));
        assert!(c.contains("h-9"));
        assert!(!c.contains("h-7"));
    }

    #[test]
    fn ghost_link_variants() {
        assert!(button_classes(ButtonVariant::Ghost, ButtonSize::Default, "").contains("hover:bg-hover"));
        assert!(button_classes(ButtonVariant::Link, ButtonSize::Default, "").contains("underline-offset-4"));
    }
}
