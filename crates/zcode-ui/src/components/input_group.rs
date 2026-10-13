//! `InputGroup` 家族 —— Rust 移植自 `packages/ui/src/components/ui/input-group.tsx`
//!
//! TS 把输入框、前置/后置图标、内嵌按钮拼成一个复合输入框。三个 `cva` 分别对应：
//! 根容器（无变体）、Addon 的 `align`（inline-start / inline-end / block-start /
//! block-end）、Button 的 `size`（xs / sm / icon-xs / icon-sm）。Rust 侧一律换成
//! 枚举 + `classes()`。
//!
//! ## 与 TS 的差异
//!
//! - `InputGroupAddon` 的点击行为：TS 在 `onClick` 里判断 `closest("button")`，
//!   否则 `querySelector("input")?.focus()`。Dioxus 的 `MouseEvent` 没有
//!   `closest()` / `querySelector()`（需要 DOM 查询能力）。这里退化为
//!   **不做 DOM 查询、直接聚焦**：点击 addon 时调用 `on_focus_control`（由调用方
//!   决定聚焦哪个输入框）。这是刻意的取舍 —— 无渲染器环境下 DOM 查询不可测。
//! - `InputGroupButton` 不委托给 [`crate::components::button::Button`]，而是**直接
//!   渲染 button**：TS 里 `<Button size>` 并没有传下去（只传了 `data-size`），
//!   类名完全由 `inputGroupButtonVariants` 通过 `className` 覆盖。若在 Rust 侧把
//!   `size` 也传给 `Button`，`ButtonSize` 与 cva 的 size 会各自输出一条
//!   「按 `svg` 子元素设置尺寸」的任意 variant —— 两者的选择器写法不同
//!   （一个用下划线、一个用 `>` 组合子），合并器会视作两个不同分组而**双双保留**，
//!   产生与 TS 不同的 DOM。直接渲染才能逐字对齐。

use crate::cn::cn;
use crate::components::button::{button_classes, ButtonSize, ButtonVariant};
use crate::components::input::{input_classes, InputSize};
use crate::components::textarea::textarea_classes;
use dioxus::prelude::*;

/// `InputGroup` 根容器基础类名。
pub const INPUT_GROUP_BASE: &str = "group/input-group relative flex h-7 w-full min-w-0 items-center rounded-md border border-input bg-input/20 transition-colors outline-none in-data-[slot=combobox-content]:focus-within:border-inherit in-data-[slot=combobox-content]:focus-within:ring-0 has-data-[align=block-end]:rounded-md has-data-[align=block-start]:rounded-md has-[[data-slot=input-group-control]:focus-visible]:border-ring has-[[data-slot=input-group-control]:focus-visible]:ring-2 has-[[data-slot=input-group-control]:focus-visible]:ring-ring/30 has-[[data-slot][aria-invalid=true]]:border-destructive has-[[data-slot][aria-invalid=true]]:ring-2 has-[[data-slot][aria-invalid=true]]:ring-destructive/20 has-[textarea]:rounded-md has-[>[data-align=block-end]]:h-auto has-[>[data-align=block-end]]:flex-col has-[>[data-align=block-start]]:h-auto has-[>[data-align=block-start]]:flex-col has-[>textarea]:h-auto dark:bg-input/30 dark:has-[[data-slot][aria-invalid=true]]:ring-destructive/40 has-[>[data-align=block-end]]:[&>input]:pt-3 has-[>[data-align=block-start]]:[&>input]:pb-3 has-[>[data-align=inline-end]]:[&>input]:pr-1.5 has-[>[data-align=inline-start]]:[&>input]:pl-1.5";

/// `InputGroupAddon` 的 `align` 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum InputGroupAddonAlign {
    #[default]
    InlineStart,
    InlineEnd,
    BlockStart,
    BlockEnd,
}

impl InputGroupAddonAlign {
    pub fn as_str(&self) -> &'static str {
        match self {
            InputGroupAddonAlign::InlineStart => "inline-start",
            InputGroupAddonAlign::InlineEnd => "inline-end",
            InputGroupAddonAlign::BlockStart => "block-start",
            InputGroupAddonAlign::BlockEnd => "block-end",
        }
    }

    /// 对应 `inputGroupAddonVariants.variants.align[align]`。
    pub fn classes(&self) -> &'static str {
        match self {
            InputGroupAddonAlign::InlineStart => {
                "order-first pl-2 has-[>button]:ml-[-0.275rem] has-[>kbd]:ml-[-0.275rem]"
            }
            InputGroupAddonAlign::InlineEnd => {
                "order-last pr-2 has-[>button]:mr-[-0.275rem] has-[>kbd]:mr-[-0.275rem]"
            }
            InputGroupAddonAlign::BlockStart => {
                "order-first w-full justify-start px-2 pt-2 group-has-[>input]/input-group:pt-2 [.border-b]:pb-2"
            }
            InputGroupAddonAlign::BlockEnd => {
                "order-last w-full justify-start px-2 pb-2 group-has-[>input]/input-group:pb-2 [.border-t]:pt-2"
            }
        }
    }

    /// 是否属于块级（纵向堆叠）对齐。
    pub fn is_block(&self) -> bool {
        matches!(
            self,
            InputGroupAddonAlign::BlockStart | InputGroupAddonAlign::BlockEnd
        )
    }
}

/// `inputGroupAddonVariants` 的第一个参数。
pub const INPUT_GROUP_ADDON_BASE: &str = "flex h-auto cursor-text items-center justify-center gap-1 py-2 text-ui-base/relaxed font-medium text-muted-foreground select-none group-data-[disabled=true]/input-group:opacity-50 **:data-[slot=kbd]:rounded-[calc(var(--radius-sm)-2px)] **:data-[slot=kbd]:bg-muted-foreground/10 **:data-[slot=kbd]:px-1 **:data-[slot=kbd]:text-ui-xs [&>svg:not([class*='size-'])]:size-3.5";

/// `InputGroupButton` 的 `size` 变体（对应 `inputGroupButtonVariants.variants.size`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum InputGroupButtonSize {
    #[default]
    Xs,
    Sm,
    IconXs,
    IconSm,
}

impl InputGroupButtonSize {
    pub fn as_str(&self) -> &'static str {
        match self {
            InputGroupButtonSize::Xs => "xs",
            InputGroupButtonSize::Sm => "sm",
            InputGroupButtonSize::IconXs => "icon-xs",
            InputGroupButtonSize::IconSm => "icon-sm",
        }
    }

    pub fn classes(&self) -> &'static str {
        match self {
            InputGroupButtonSize::Xs => {
                "h-5 gap-1 rounded-[calc(var(--radius-sm)-2px)] px-1 [&>svg:not([class*='size-'])]:size-3"
            }
            InputGroupButtonSize::Sm => "gap-1",
            InputGroupButtonSize::IconXs => "size-6 p-0 has-[>svg]:p-0",
            InputGroupButtonSize::IconSm => "size-7 p-0 has-[>svg]:p-0",
        }
    }
}

/// `inputGroupButtonVariants` 的第一个参数。
pub const INPUT_GROUP_BUTTON_BASE: &str =
    "flex items-center gap-2 rounded-md text-ui-base/relaxed shadow-none";

/// `InputGroupText` 基础类名。
pub const INPUT_GROUP_TEXT_BASE: &str = "flex items-center gap-2 text-ui-base/relaxed text-muted-foreground [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4";

/// `InputGroupInput` 追加类名。
pub const INPUT_GROUP_INPUT_BASE: &str = "flex-1 rounded-none border-0 bg-transparent shadow-none ring-0 focus-visible:ring-0 aria-invalid:ring-0 dark:bg-transparent";

/// `InputGroupTextarea` 追加类名。
pub const INPUT_GROUP_TEXTAREA_BASE: &str = "flex-1 resize-none rounded-none border-0 bg-transparent py-2 shadow-none ring-0 focus-visible:ring-0 aria-invalid:ring-0 dark:bg-transparent";

pub fn input_group_classes(class: &str) -> String {
    cn(&[INPUT_GROUP_BASE, class])
}

pub fn input_group_addon_classes(align: InputGroupAddonAlign, class: &str) -> String {
    cn(&[INPUT_GROUP_ADDON_BASE, align.classes(), class])
}

pub fn input_group_button_classes(
    size: InputGroupButtonSize,
    variant: ButtonVariant,
    class: &str,
) -> String {
    // 等价 TS：`cn(buttonVariants({ variant, size: "default" }), inputGroupButtonVariants({ size }), className)`
    button_classes(
        variant,
        ButtonSize::Default,
        &cn(&[INPUT_GROUP_BUTTON_BASE, size.classes(), class]),
    )
}

pub fn input_group_text_classes(class: &str) -> String {
    cn(&[INPUT_GROUP_TEXT_BASE, class])
}

pub fn input_group_input_classes(class: &str) -> String {
    // 等价 TS：`<Input className={cn("flex-1 rounded-none ...", className)} />`
    input_classes(InputSize::Default, &cn(&[INPUT_GROUP_INPUT_BASE, class]))
}

pub fn input_group_textarea_classes(class: &str) -> String {
    textarea_classes(&cn(&[INPUT_GROUP_TEXTAREA_BASE, class]))
}

/// 复合输入框根容器。
#[component]
pub fn InputGroup(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: input_group_classes(&class),
            "data-slot": "input-group",
            role: "group",
            {children}
        }
    }
}

/// 输入框的前/后置片段（图标、单位、`Kbd` 等）。
#[component]
pub fn InputGroupAddon(
    #[props(default)] align: InputGroupAddonAlign,
    #[props(default, into)] class: String,
    /// 点击 addon 时触发（TS 里是聚焦内部 input）。见模块文档的差异说明。
    #[props(default)] on_click: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: input_group_addon_classes(align, &class),
            role: "group",
            "data-slot": "input-group-addon",
            "data-align": align.as_str(),
            onclick: move |e| on_click.call(e),
            {children}
        }
    }
}

/// 内嵌在输入框里的按钮。
#[component]
pub fn InputGroupButton(
    #[props(default)] variant: ButtonVariant,
    #[props(default)] size: InputGroupButtonSize,
    #[props(default, into)] class: String,
    #[props(default)] disabled: bool,
    #[props(default = "button".to_string())] html_type: String,
    #[props(default)] onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    let cls = input_group_button_classes(size, variant, &class);
    rsx! {
        button {
            class: cls,
            r#type: html_type,
            disabled: disabled,
            "data-slot": "button",
            "data-variant": variant.as_str(),
            // TS：`<Button data-size={size} />` 覆盖了 buttonVariants 的默认 size
            "data-size": size.as_str(),
            onclick: move |e| onclick.call(e),
            {children}
        }
    }
}

/// 输入框旁的纯文本/图标片段。
#[component]
pub fn InputGroupText(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        span {
            class: input_group_text_classes(&class),
            "data-slot": "input-group-text",
            {children}
        }
    }
}

/// 复合输入框里的单行输入控件（`data-slot="input-group-control"`）。
#[component]
pub fn InputGroupInput(
    #[props(default, into)] class: String,
    #[props(default = "text".to_string())] input_type: String,
    #[props(default)] placeholder: Option<String>,
    #[props(default)] value: Option<String>,
    #[props(default)] disabled: bool,
    #[props(default)] oninput: EventHandler<FormEvent>,
    #[props(default)] onchange: EventHandler<FormEvent>,
) -> Element {
    rsx! {
        input {
            class: input_group_input_classes(&class),
            r#type: input_type,
            "data-slot": "input-group-control",
            placeholder: placeholder,
            value: value,
            disabled: disabled,
            oninput: move |e| oninput.call(e),
            onchange: move |e| onchange.call(e),
        }
    }
}

/// 复合输入框里的多行输入控件。
#[component]
pub fn InputGroupTextarea(
    #[props(default, into)] class: String,
    #[props(default)] placeholder: Option<String>,
    #[props(default)] value: Option<String>,
    #[props(default)] disabled: bool,
    #[props(default)] rows: Option<i64>,
    #[props(default)] oninput: EventHandler<FormEvent>,
    #[props(default)] onchange: EventHandler<FormEvent>,
) -> Element {
    rsx! {
        textarea {
            class: input_group_textarea_classes(&class),
            "data-slot": "input-group-control",
            placeholder: placeholder,
            value: value,
            disabled: disabled,
            rows: rows,
            oninput: move |e| oninput.call(e),
            onchange: move |e| onchange.call(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::button::BUTTON_BASE;

    #[test]
    fn align_strings_and_default() {
        assert_eq!(InputGroupAddonAlign::InlineStart.as_str(), "inline-start");
        assert_eq!(InputGroupAddonAlign::InlineEnd.as_str(), "inline-end");
        assert_eq!(InputGroupAddonAlign::BlockStart.as_str(), "block-start");
        assert_eq!(InputGroupAddonAlign::BlockEnd.as_str(), "block-end");
        assert_eq!(InputGroupAddonAlign::default(), InputGroupAddonAlign::InlineStart);
    }

    #[test]
    fn block_aligns_are_block() {
        assert!(!InputGroupAddonAlign::InlineStart.is_block());
        assert!(!InputGroupAddonAlign::InlineEnd.is_block());
        assert!(InputGroupAddonAlign::BlockStart.is_block());
        assert!(InputGroupAddonAlign::BlockEnd.is_block());
    }

    #[test]
    fn addon_align_classes() {
        assert!(input_group_addon_classes(InputGroupAddonAlign::InlineStart, "")
            .contains("order-first pl-2"));
        assert!(input_group_addon_classes(InputGroupAddonAlign::BlockEnd, "")
            .contains("order-last w-full justify-start px-2 pb-2"));
        assert!(input_group_addon_classes(InputGroupAddonAlign::BlockStart, "")
            .contains("group-has-[>input]/input-group:pt-2"));
    }

    #[test]
    fn addon_base_has_kbd_rules() {
        let c = input_group_addon_classes(InputGroupAddonAlign::InlineStart, "");
        assert!(c.contains("cursor-text"));
        assert!(c.contains("**:data-[slot=kbd]:bg-muted-foreground/10"));
        assert!(c.contains("[&>svg:not([class*='size-'])]:size-3.5"));
    }

    #[test]
    fn root_base_has_focus_within_chain() {
        let c = input_group_classes("");
        assert!(c.contains("group/input-group"));
        assert!(c.contains("has-[[data-slot=input-group-control]:focus-visible]:ring-2"));
        assert!(c.contains("has-[>[data-align=block-end]]:flex-col"));
    }

    #[test]
    fn button_size_strings() {
        assert_eq!(InputGroupButtonSize::Xs.as_str(), "xs");
        assert_eq!(InputGroupButtonSize::Sm.as_str(), "sm");
        assert_eq!(InputGroupButtonSize::IconXs.as_str(), "icon-xs");
        assert_eq!(InputGroupButtonSize::IconSm.as_str(), "icon-sm");
        assert_eq!(InputGroupButtonSize::default(), InputGroupButtonSize::Xs);
    }

    #[test]
    fn button_keeps_default_button_size_classes_and_appends_cva_size() {
        let c = input_group_button_classes(
            InputGroupButtonSize::Xs,
            ButtonVariant::Ghost,
            "",
        );
        // Button 基类与 ghost variant 仍在
        assert!(c.contains("group/button"));
        assert!(c.contains("hover:bg-hover"));
        // 默认 size 的 h-7 被 cva 的 h-5 覆盖
        assert!(c.contains("h-5"));
        assert!(!c.split_whitespace().any(|t| t == "h-7"));
        assert!(c.contains("shadow-none"));
        assert!(c.contains("rounded-[calc(var(--radius-sm)-2px)]"));
    }

    #[test]
    fn input_control_strips_border_and_radius() {
        let c = input_group_input_classes("");
        assert!(c.contains("flex-1"));
        assert!(c.contains("rounded-none"));
        assert!(c.contains("border-0"));
        assert!(c.contains("bg-transparent"));
        // `rounded-none` 与 Input 基类的 `rounded-md` 同组 → 后者被移除
        assert!(!c.contains("rounded-md"));
        // `bg-transparent` 与 `bg-input` 同组 → 后者被移除
        assert!(!c.split_whitespace().any(|t| t == "bg-input"));
        // 注意：`border-0`（border-width）与 `border-input-border`（border-color）
        // 在 tailwind-merge 语义下**属于不同组**，两者都保留；
        // 视觉上 border-width:0 已足够，颜色无影响。
        assert!(c.contains("border-0"));
        assert!(c.contains("border-input-border"));
    }

    #[test]
    fn textarea_control_strips_border_and_resize() {
        let c = input_group_textarea_classes("");
        assert!(c.contains("flex-1"));
        assert!(c.contains("resize-none"));
        assert!(c.contains("border-0"));
        // Textarea 基类的 min-h-16 保留（cva 没有覆盖它）
        assert!(c.contains("min-h-16"));
    }

    #[test]
    fn text_base() {
        let c = input_group_text_classes("");
        assert!(c.contains("text-muted-foreground"));
        assert!(c.contains("[&_svg]:pointer-events-none"));
    }

    #[test]
    fn button_base_const_is_used() {
        // 防止 INPUT_GROUP_BUTTON_BASE 与实际拼装脱节
        assert!(BUTTON_BASE.contains("group/button"));
        let c = input_group_button_classes(InputGroupButtonSize::IconXs, ButtonVariant::Outline, "");
        assert!(c.contains("size-6"));
        assert!(c.contains("border-border"));
    }
}
