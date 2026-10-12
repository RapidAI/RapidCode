//! `Input` —— Rust 移植自 `packages/ui/src/components/ui/input.tsx`

use crate::cn::cn;
use dioxus::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum InputSize {
    Xs,
    Sm,
    #[default]
    Default,
    Lg,
}

impl InputSize {
    pub fn as_str(&self) -> &'static str {
        match self {
            InputSize::Xs => "xs",
            InputSize::Sm => "sm",
            InputSize::Default => "default",
            InputSize::Lg => "lg",
        }
    }

    pub fn classes(&self) -> &'static str {
        match self {
            InputSize::Xs => "h-5 rounded-sm px-2 py-0.5 text-ui-base file:h-4 file:text-ui-base",
            InputSize::Sm => {
                "h-6 rounded-md px-2 py-0.5 text-ui-base/relaxed file:h-5 file:text-ui-base/relaxed"
            }
            InputSize::Default => {
                "h-7 rounded-md px-2 py-0.5 text-ui-base md:text-ui-base/relaxed file:h-6 file:text-ui-base/relaxed"
            }
            InputSize::Lg => "h-8 rounded-lg px-3 py-1.5 text-ui-base file:h-6 file:text-ui-base",
        }
    }
}

pub const INPUT_BASE: &str = "w-full min-w-0 border border-input-border bg-input text-foreground transition-colors outline-none file:inline-flex file:border-0 file:bg-transparent file:font-medium file:text-foreground placeholder:text-foreground-subtlest hover:border-input-border-hover focus-visible:border-input-border-focused focus-visible:bg-input-focused focus-visible:ring-0 disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-2 aria-invalid:ring-destructive/20 dark:aria-invalid:border-destructive/50 dark:aria-invalid:ring-destructive/40";

pub fn input_classes(size: InputSize, class: &str) -> String {
    cn(&[INPUT_BASE, size.classes(), class])
}

/// 输入框。
#[component]
pub fn Input(
    #[props(default)] size: InputSize,
    #[props(default, into)] class: String,
    #[props(default = "text".to_string())] input_type: String,
    #[props(default)] placeholder: Option<String>,
    #[props(default)] value: Option<String>,
    #[props(default)] disabled: bool,
    /// 对应 TS 的 `htmlSize`（透传给原生 `size` 属性）。
    #[props(default)] html_size: Option<i64>,
    #[props(default)] oninput: EventHandler<FormEvent>,
    #[props(default)] onchange: EventHandler<FormEvent>,
) -> Element {
    let cls = input_classes(size, &class);
    rsx! {
        input {
            class: cls,
            r#type: input_type,
            "data-slot": "input",
            placeholder: placeholder,
            value: value,
            disabled: disabled,
            size: html_size,
            oninput: move |e| oninput.call(e),
            onchange: move |e| onchange.call(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_input() {
        let c = input_classes(InputSize::Default, "");
        assert!(c.contains("border-input-border"));
        assert!(c.contains("h-7"));
    }

    #[test]
    fn size_override() {
        assert!(input_classes(InputSize::Lg, "").contains("h-8"));
        assert!(input_classes(InputSize::Xs, "").contains("h-5"));
    }
}
