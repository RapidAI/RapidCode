//! `Textarea` —— Rust 移植自 `packages/ui/src/components/ui/textarea.tsx`
//!
//! TS 用 `forwardRef` 透传原生 `<textarea>`；Dioxus 无 ref 概念（组件自身即渲染
//! 该元素），故省去 `ref`，其余 props 一一对应。

use crate::cn::cn;
use dioxus::prelude::*;

/// 基础类名。
pub const TEXTAREA_BASE: &str = "flex field-sizing-content min-h-16 w-full resize-none rounded-md border border-input bg-input/20 px-2 py-2 text-ui-base transition-colors outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/30 disabled:cursor-not-allowed disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-2 aria-invalid:ring-destructive/20 md:text-ui-base/relaxed";

pub fn textarea_classes(class: &str) -> String {
    cn(&[TEXTAREA_BASE, class])
}

/// 多行输入框。
#[component]
pub fn Textarea(
    #[props(default, into)] class: String,
    #[props(default)] value: Option<String>,
    #[props(default)] placeholder: Option<String>,
    #[props(default)] disabled: bool,
    #[props(default)] rows: Option<i64>,
    /// 覆盖 `data-slot`（`InputGroupTextarea` 需要 `input-group-control`）。
    #[props(default = "textarea")] slot: &'static str,
    /// 透传 `aria-label`（占位文案与无障碍标签分离时使用）。
    #[props(default)] aria_label: Option<String>,
    /// 挂载后自动聚焦（对应原生 `autoFocus`）。
    #[props(default)] autofocus: bool,
    #[props(default)] oninput: EventHandler<FormEvent>,
    #[props(default)] onchange: EventHandler<FormEvent>,
    /// 输入法合成开始 / 结束。
    ///
    /// TS 侧用它们维护一个 `compositionActiveRef`，因为部分平台在 `keydown`
    /// 上把 `isComposing` 提前置回 `false`（见 `PptxSelectionActionBar` 的
    /// Esc 处理）。Dioxus 的 `textarea` 支持这两个原生事件，这里直接透传。
    #[props(default)] oncompositionstart: EventHandler<Event<CompositionData>>,
    #[props(default)] oncompositionend: EventHandler<Event<CompositionData>>,
    /// `keydown`。`CodeViewer` 的评论草稿需要在这里处理「提交 / 取消」快捷键。
    #[props(default)] onkeydown: EventHandler<KeyboardEvent>,
) -> Element {
    rsx! {
        textarea {
            class: textarea_classes(&class),
            "data-slot": slot,
            value: value,
            placeholder: placeholder,
            disabled: disabled,
            rows: rows,
            "aria-label": aria_label,
            autofocus,
            oninput: move |e| oninput.call(e),
            onchange: move |e| onchange.call(e),
            oncompositionstart: move |e| oncompositionstart.call(e),
            oncompositionend: move |e| oncompositionend.call(e),
            onkeydown: move |e| onkeydown.call(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_classes() {
        let c = textarea_classes("");
        assert!(c.contains("min-h-16"));
        assert!(c.contains("resize-none"));
        assert!(c.contains("field-sizing-content"));
    }

    #[test]
    fn user_class_wins() {
        let c = textarea_classes("min-h-32 resize-y");
        assert!(c.contains("min-h-32"));
        assert!(!c.contains("min-h-16"));
        assert!(c.contains("resize-y"));
        assert!(!c.contains("resize-none"));
    }
}
