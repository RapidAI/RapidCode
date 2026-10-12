//! `PptxSelectionActionBar` —— Rust 移植自
//! `packages/ui/src/components/ui/pptx-selection-action-bar.tsx`
//!
//! PPTX 元素选择后浮在选区上方的应用层操作条：非编辑态是一个「AI 编辑」按钮，
//! 编辑态换成一个评论输入框 + 取消 / 加入对话两个按钮。
//!
//! ## 关键移植点 1：Esc 的三义性
//!
//! 同一个 Esc 在不同状态下做三件不同的事，而且**输入法优先**：
//!
//! | `editing` | 输入法正在合成 | 结果 |
//! |---|---|---|
//! | `true`  | 是 | 什么都不做（Esc 是取消候选词） |
//! | `true`  | 否 | 丢弃本次评论草稿，退回 AI 编辑条（**不**退出元素选择） |
//! | `false` | —  | 退出元素选择 |
//!
//! 这段判断被提成纯函数 [`resolve_pptx_selection_escape`]，可以脱离渲染器测试。
//!
//! ## 关键移植点 2：为什么需要本地合成标记
//!
//! `isImeComposingKeyEvent` 把三个来源「或」起来：本地 ref、事件的 `isComposing`、
//! 以及 `nativeEvent.isComposing`。TS 注释点明了原因：**部分平台的事件时序会让
//! `isComposing` 提前变回 `false`**，所以只信事件对象会漏判，必须叠加一个由
//! `compositionstart` / `compositionend` 维护的本地标记。
//!
//! ## 关键移植点 3：浮层位置不能随幻灯片缩放
//!
//! 锚点留在幻灯片的 transform 容器内（这样能读到缩放后的真实 `DOMRect`），
//! 内容则走 Popover 的 portal。本移植的 `PopoverContent` 同样渲染在锚点容器之外
//! 的兄弟位置，因此按钮尺寸不会被幻灯片的 `scale` 放大，也不会被页面 `overflow`
//! 裁掉 —— 与 TS 的意图一致。
//!
//! ## 与 TS 的差异
//!
//! 1. **`collisionBoundary` / `collisionPadding` / `sticky` / `hideWhenDetached` /
//!    `updatePositionStrategy` 未移植**：这些是 Radix Floating UI 的碰撞规避参数，
//!    本移植的浮层定位只做 `side` / `align` / `side_offset`（见 `overlay` 模块文档）。
//!    需要「贴边不越界」时由调用方给 `Popover` 传 `class`。
//! 2. **`onCloseAutoFocus` 的 `preventDefault` 未移植**：本移植不接管焦点，
//!    因此没有「关闭后把焦点还给触发点」这一步。
//! 3. **`modal={false}` 未移植**：本移植的 Popover 没有模态层，行为上等价于
//!    非模态，因此不需要这个开关。
//! 4. **`nativeEvent.isComposing` 与 `isComposing` 同源**：浏览器里 `nativeEvent`
//!    就是原生事件对象本身，Dioxus 也只暴露一个事件对象，因此
//!    [`ImeKeyEvent`] 里两者取同一个值（保留字段是为了逐条对齐 TS 的三路或）。

use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::components::overlay::{Align, Side};
use crate::components::popover::{Popover, PopoverContent};
use crate::components::textarea::Textarea;
use crate::icons::IconSparkles;
use dioxus::prelude::*;

/// 锚点与浮层之间的间距（TS `sideOffset={8}`）。
pub const PPTX_SELECTION_SIDE_OFFSET: f64 = 8.0;

/// 浮层类名 —— 编辑态（固定 320px 宽，给评论框留位置）。
pub const PPTX_SELECTION_CONTENT_EDITING_CLASS: &str =
    "w-80 max-w-[calc(100vw-1rem)] gap-0 border-popover-border p-1";

/// 浮层类名 —— 非编辑态（宽度交给内容）。
pub const PPTX_SELECTION_CONTENT_IDLE_CLASS: &str =
    "w-auto max-w-[calc(100vw-1rem)] gap-0 border-popover-border p-1";

/// 评论区外框。
pub const PPTX_SELECTION_EDIT_FORM_CLASS: &str = "flex flex-col gap-2 p-1";

/// 评论区底部按钮行。
pub const PPTX_SELECTION_ACTIONS_CLASS: &str = "flex items-center justify-end gap-1";

/// 评论输入框类名（TS 原文，逐字保留）。
pub const PPTX_SELECTION_TEXTAREA_CLASS: &str = "min-h-16 resize-none border-input-border bg-input text-mobile-input-safe placeholder:text-foreground-subtlest hover:border-input-border-hover focus-visible:border-input-border-focused focus-visible:bg-input-focused focus-visible:ring-0 md:text-ui-base";

/// 评论框行数（TS `rows={3}`）。
pub const PPTX_SELECTION_TEXTAREA_ROWS: i64 = 3;

/// 非编辑态那个「AI 编辑」按钮的类名。
pub const PPTX_SELECTION_AI_BUTTON_CLASS: &str = "w-full justify-start rounded-lg px-2";

/// 按钮里 sparkles 图标的类名。
pub const PPTX_SELECTION_AI_ICON_CLASS: &str = "size-4";

/// 按编辑态挑浮层类名。
pub fn pptx_selection_content_class(editing: bool) -> &'static str {
    if editing {
        PPTX_SELECTION_CONTENT_EDITING_CLASS
    } else {
        PPTX_SELECTION_CONTENT_IDLE_CLASS
    }
}

/// 一次按键事件里与输入法合成相关的三个来源。
///
/// 对应 TS 的匿名对象参数 `{ compositionActive, isComposing, nativeEvent }`。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImeKeyEvent {
    /// 本地维护的合成标记：`compositionstart` → `true`，`compositionend` → `false`。
    pub composition_active: bool,
    /// 事件对象自带的 `isComposing`。
    pub is_composing: bool,
    /// `nativeEvent.isComposing`。见模块文档差异 4：与 [`Self::is_composing`] 同源。
    pub native_is_composing: bool,
}

/// 判断按键是否发生在输入法合成过程中（TS `isImeComposingKeyEvent`）。
///
/// 三个来源任一为真即算合成中 —— 只信事件对象会漏判（见模块文档关键移植点 2）。
pub fn is_ime_composing_key_event(event: &ImeKeyEvent) -> bool {
    event.composition_active || event.is_composing || event.native_is_composing
}

/// Esc 在操作条上的三种结局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PptxSelectionEscape {
    /// 输入法正在合成候选词 —— 什么都不做，把 Esc 让给输入法。
    IgnoreComposition,
    /// 丢弃本次评论草稿并退回 AI 编辑条（**不**退出元素选择）。
    CancelAiEdit,
    /// 退出元素选择。
    ExitSelection,
}

/// 解析 Esc 的语义（TS `onEscapeKeyDown` 里的分支）。
///
/// 输入法优先：只有**不在**合成中时，Esc 才会产生副作用。
pub fn resolve_pptx_selection_escape(editing: bool, ime_composing: bool) -> PptxSelectionEscape {
    if !editing {
        return PptxSelectionEscape::ExitSelection;
    }
    if ime_composing {
        PptxSelectionEscape::IgnoreComposition
    } else {
        PptxSelectionEscape::CancelAiEdit
    }
}

/// 编辑态切换时是否要清空评论草稿与合成标记。
///
/// 对应 TS 的 `useEffect(() => { if (!editing) { … } }, [editing])`。
/// 首次挂载时 `comment` 本来就是空串、ref 本来就是 `false`，所以那个分支是空操作，
/// 只有「由真变假」这一次转换有可观测效果 —— 这里就只表达这一次转换。
pub fn pptx_selection_resets_draft(previous_editing: bool, editing: bool) -> bool {
    previous_editing && !editing
}

/// PPTX 元素选择的操作条。
#[component]
pub fn PptxSelectionActionBar(
    /// 浮层是否展开。
    #[props(default)] open: bool,
    /// 无障碍标签；非编辑态下同时充当按钮文案（TS 复用同一个 `label`）。
    #[props(default, into)] label: String,
    /// 评论框的占位文案，同时作为它的 `aria-label`。
    #[props(default, into)] comment_placeholder: String,
    /// 「取消」按钮文案。
    #[props(default, into)] cancel_label: String,
    /// 「加入对话」按钮文案。
    #[props(default, into)] add_to_conversation_label: String,
    /// 是否处于「写评论」态。
    #[props(default)] editing: bool,
    #[props(default)] disabled: bool,
    /// 非编辑态点「AI 编辑」。
    #[props(default)] on_ai_edit: EventHandler<()>,
    /// 取消评论（Esc 或「取消」按钮）。
    #[props(default)] on_cancel_ai_edit: EventHandler<()>,
    /// 「加入对话」，带当前评论内容。
    #[props(default)] on_add_to_conversation: EventHandler<String>,
    /// 退出元素选择（非编辑态按 Esc）。
    #[props(default)] on_exit_selection: EventHandler<()>,
    /// 锚点内容（通常是被选中的元素本身或它的包裹层）。
    children: Element,
) -> Element {
    let mut comment = use_signal(String::new);
    // 本地合成标记，由评论框的 composition 事件维护。
    let mut composing = use_signal(|| false);

    // `Popover` 收的是 `Option<Signal<bool>>`，而这里 `open` 是普通 prop。
    // 用渲染期收敛把 prop 镜像进信号：TS 侧 `open={open}` 不带 `onOpenChange`，
    // 本身就是「完全受控」语义，外部 prop 才是唯一真源。
    let mut popover_open = use_signal(|| open);
    if popover_open() != open {
        popover_open.set(open);
    }

    // 渲染期收敛：`editing` 变化时按需清空草稿。
    //
    // 用「上一趟的 editing」而不是 `use_effect`：Dioxus 的 `use_effect` 订阅的是
    // 信号，而 `editing` 是 prop。写成收敛式（写完之后下一趟条件不再成立）即可，
    // 与 `SmoothMetricValue` 里的做法一致。
    let mut was_editing = use_signal(|| false);
    let previous = was_editing();
    if previous != editing {
        was_editing.set(editing);
        if pptx_selection_resets_draft(previous, editing) {
            comment.set(String::new());
            composing.set(false);
        }
    }

    rsx! {
        Popover { open: Some(popover_open),
            {children}
            PopoverContent {
                role: "toolbar".to_string(),
                aria_label: label.clone(),
                side: Side::Top,
                align: Align::Center,
                side_offset: PPTX_SELECTION_SIDE_OFFSET,
                class: pptx_selection_content_class(editing).to_string(),
                onkeydown: move |e: KeyboardEvent| {
                    if e.key() != Key::Escape {
                        return;
                    }
                    // TS 无条件 `preventDefault`：Esc 不该继续冒泡去关掉外层浮层。
                    e.prevent_default();
                    let ime = is_ime_composing_key_event(&ImeKeyEvent {
                        composition_active: composing(),
                        is_composing: e.is_composing(),
                        native_is_composing: e.is_composing(),
                    });
                    match resolve_pptx_selection_escape(editing, ime) {
                        PptxSelectionEscape::IgnoreComposition => {}
                        PptxSelectionEscape::CancelAiEdit => on_cancel_ai_edit.call(()),
                        PptxSelectionEscape::ExitSelection => on_exit_selection.call(()),
                    }
                },
                if editing {
                    div { class: PPTX_SELECTION_EDIT_FORM_CLASS,
                        Textarea {
                            autofocus: true,
                            aria_label: Some(comment_placeholder.clone()),
                            placeholder: comment_placeholder.clone(),
                            value: Some(comment()),
                            disabled,
                            rows: Some(PPTX_SELECTION_TEXTAREA_ROWS),
                            class: PPTX_SELECTION_TEXTAREA_CLASS.to_string(),
                            oninput: move |e: FormEvent| comment.set(e.value()),
                            oncompositionstart: move |_: Event<CompositionData>| composing.set(true),
                            oncompositionend: move |_: Event<CompositionData>| composing.set(false),
                        }
                        div { class: PPTX_SELECTION_ACTIONS_CLASS,
                            Button {
                                html_type: "button".to_string(),
                                size: ButtonSize::Sm,
                                variant: ButtonVariant::Ghost,
                                disabled,
                                onclick: move |_| on_cancel_ai_edit.call(()),
                                {cancel_label.clone()}
                            }
                            Button {
                                html_type: "button".to_string(),
                                size: ButtonSize::Sm,
                                disabled,
                                onclick: move |_| on_add_to_conversation.call(comment()),
                                {add_to_conversation_label.clone()}
                            }
                        }
                    }
                } else {
                    Button {
                        html_type: "button".to_string(),
                        size: ButtonSize::Lg,
                        variant: ButtonVariant::Ghost,
                        aria_label: Some(label.clone()),
                        disabled,
                        class: PPTX_SELECTION_AI_BUTTON_CLASS.to_string(),
                        // 保住 PPTX 文本选区：见 `Button::onmousedown` 的文档。
                        onmousedown: move |e: MouseEvent| e.prevent_default(),
                        onclick: move |_| on_ai_edit.call(()),
                        IconSparkles { class: PPTX_SELECTION_AI_ICON_CLASS.to_string() }
                        {label.clone()}
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- 浮层类名 ----

    #[test]
    fn content_class_follows_the_editing_state() {
        let editing = pptx_selection_content_class(true);
        assert!(editing.contains("w-80"));
        assert!(!editing.contains("w-auto"));

        let idle = pptx_selection_content_class(false);
        assert!(idle.contains("w-auto"));
        assert!(!idle.contains("w-80"));
    }

    #[test]
    fn both_content_states_share_the_shell() {
        for editing in [true, false] {
            let c = pptx_selection_content_class(editing);
            assert!(c.contains("max-w-[calc(100vw-1rem)]"), "{c}");
            assert!(c.contains("gap-0"), "{c}");
            assert!(c.contains("border-popover-border"), "{c}");
            assert!(c.contains("p-1"), "{c}");
        }
    }

    #[test]
    fn textarea_class_is_carried_over_verbatim() {
        // 这些 token 是 TS 里手写的，逐字保留才不会跟其它输入框跑偏
        for token in [
            "min-h-16",
            "resize-none",
            "border-input-border",
            "bg-input",
            "text-mobile-input-safe",
            "placeholder:text-foreground-subtlest",
            "hover:border-input-border-hover",
            "focus-visible:border-input-border-focused",
            "focus-visible:bg-input-focused",
            "focus-visible:ring-0",
            "md:text-ui-base",
        ] {
            assert!(
                PPTX_SELECTION_TEXTAREA_CLASS.contains(token),
                "缺少 {token}"
            );
        }
    }

    #[test]
    fn layout_constants_match_the_ts_literals() {
        assert_eq!(PPTX_SELECTION_EDIT_FORM_CLASS, "flex flex-col gap-2 p-1");
        assert_eq!(
            PPTX_SELECTION_ACTIONS_CLASS,
            "flex items-center justify-end gap-1"
        );
        assert_eq!(
            PPTX_SELECTION_AI_BUTTON_CLASS,
            "w-full justify-start rounded-lg px-2"
        );
        assert_eq!(PPTX_SELECTION_AI_ICON_CLASS, "size-4");
        assert_eq!(PPTX_SELECTION_SIDE_OFFSET, 8.0);
        assert_eq!(PPTX_SELECTION_TEXTAREA_ROWS, 3);
    }

    // ---- IME ----

    #[test]
    fn ime_flag_ors_all_three_sources() {
        let none = ImeKeyEvent::default();
        assert!(!is_ime_composing_key_event(&none));

        // 任一为真即算合成中
        assert!(is_ime_composing_key_event(&ImeKeyEvent {
            composition_active: true,
            ..none
        }));
        assert!(is_ime_composing_key_event(&ImeKeyEvent {
            is_composing: true,
            ..none
        }));
        assert!(is_ime_composing_key_event(&ImeKeyEvent {
            native_is_composing: true,
            ..none
        }));
        // 三者同时为真也只返回真
        assert!(is_ime_composing_key_event(&ImeKeyEvent {
            composition_active: true,
            is_composing: true,
            native_is_composing: true,
        }));
    }

    /// 本地标记是「事件对象提前复位」的兜底：事件说没在合成，但本地知道在合成。
    #[test]
    fn local_composition_flag_covers_a_premature_event_reset() {
        let event = ImeKeyEvent {
            composition_active: true,
            is_composing: false,
            native_is_composing: false,
        };
        assert!(is_ime_composing_key_event(&event));
        // 没有这个兜底就会误判成「用户想取消评论」
        assert_eq!(
            resolve_pptx_selection_escape(true, is_ime_composing_key_event(&event)),
            PptxSelectionEscape::IgnoreComposition
        );
    }

    // ---- Esc 三义性 ----

    #[test]
    fn escape_in_edit_mode_cancels_only_the_draft() {
        assert_eq!(
            resolve_pptx_selection_escape(true, false),
            PptxSelectionEscape::CancelAiEdit
        );
    }

    #[test]
    fn escape_while_composing_is_left_to_the_ime() {
        assert_eq!(
            resolve_pptx_selection_escape(true, true),
            PptxSelectionEscape::IgnoreComposition
        );
    }

    #[test]
    fn escape_outside_edit_mode_exits_the_selection() {
        // 非编辑态不看输入法状态：没有输入框就不可能有合成
        assert_eq!(
            resolve_pptx_selection_escape(false, false),
            PptxSelectionEscape::ExitSelection
        );
        assert_eq!(
            resolve_pptx_selection_escape(false, true),
            PptxSelectionEscape::ExitSelection
        );
    }

    /// 合成中按 Esc **不能**连带退出元素选择 —— 这是 TS 注释里点名的坑。
    ///
    /// 只在编辑态成立：非编辑态压根没有评论框，不可能处于合成中，
    /// 此时 Esc 仍然按原语义退出元素选择。
    #[test]
    fn composing_never_escalates_to_exiting_the_selection() {
        assert_eq!(
            resolve_pptx_selection_escape(true, true),
            PptxSelectionEscape::IgnoreComposition
        );
        // 非编辑态没有输入框，Esc 依旧退出选择
        assert_eq!(
            resolve_pptx_selection_escape(false, true),
            PptxSelectionEscape::ExitSelection
        );
    }

    // ---- 草稿重置 ----

    #[test]
    fn draft_resets_only_on_the_editing_true_to_false_edge() {
        assert!(pptx_selection_resets_draft(true, false));
        // 进入编辑态不清空
        assert!(!pptx_selection_resets_draft(false, true));
        // 挂载时就是非编辑态：TS 的 effect 会跑，但草稿本来就是空的，无观测差异
        assert!(!pptx_selection_resets_draft(false, false));
        // 保持编辑态不清空
        assert!(!pptx_selection_resets_draft(true, true));
    }

    #[test]
    fn component_renders_without_a_renderer() {
        let _ = PptxSelectionActionBar as fn(PptxSelectionActionBarProps) -> Element;
    }
}
