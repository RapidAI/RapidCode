//! `icons` —— lucide 图标的内联 SVG 移植。
//!
//! TS 侧的图标全部来自 `lucide-react`：每个图标就是一份固定的
//! `viewBox="0 0 24 24"` 路径数据 + 一组默认外壳属性
//! （`fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"
//! stroke-linejoin="round"`）。
//!
//! Rust 侧**不引入任何图标库依赖**，直接把同一份路径数据内联成组件：
//! 视觉与 TS 完全一致，零额外依赖，也不需要 wasm。
//!
//! 差异：lucide-react 的组件没有 `data-slot`；shadcn 是在**调用处**手动加
//! `data-slot="..."`。这里把 `slot` 做成 prop（默认值与 lucide 组件名对应），
//! 调用处传字面量即可，保持与 TS 调用点逐字一致。

use dioxus::prelude::*;

/// 声明一个 lucide 图标组件。
///
/// lucide 所有图标共享同一组外壳属性，只有路径数据不同，因此用一个宏把
/// 重复部分收掉；`$body` 直接透传 rsx 子节点 token。
macro_rules! lucide {
    ($name:ident, $default_slot:literal, $($body:tt)*) => {
        /// lucide 图标（内联 SVG）。
        #[component]
        pub fn $name(
            #[props(default, into)] class: String,
            /// 覆盖 `data-slot`（默认与 lucide 组件名一致）。
            #[props(default = $default_slot)] slot: &'static str,
        ) -> Element {
            rsx! {
                svg {
                    class: class,
                    "data-slot": slot,
                    view_box: "0 0 24 24",
                    fill: "none",
                    stroke: "currentColor",
                    stroke_width: "2",
                    stroke_linecap: "round",
                    stroke_linejoin: "round",
                    $($body)*
                }
            }
        }
    };
}

/// lucide `chevron-down`。
#[component]
pub fn IconChevronDown(
    #[props(default, into)] class: String,
    #[props(default = "chevron-down")] slot: &'static str,
) -> Element {
    rsx! {
        svg {
            class: class,
            "data-slot": slot,
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            path { d: "m6 9 6 6 6-6" }
        }
    }
}

/// lucide `chevron-up`。
#[component]
pub fn IconChevronUp(
    #[props(default, into)] class: String,
    #[props(default = "chevron-up")] slot: &'static str,
) -> Element {
    rsx! {
        svg {
            class: class,
            "data-slot": slot,
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            path { d: "m18 15-6-6-6 6" }
        }
    }
}

/// lucide `chevron-right`。
#[component]
pub fn IconChevronRight(
    #[props(default, into)] class: String,
    #[props(default = "chevron-right")] slot: &'static str,
) -> Element {
    rsx! {
        svg {
            class: class,
            "data-slot": slot,
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            path { d: "m9 18 6-6-6-6" }
        }
    }
}

/// lucide `circle`（空心圆，radio 指示器用；`fill-current` 由调用处类名控制）。
#[component]
pub fn IconCircle(
    #[props(default, into)] class: String,
    #[props(default = "circle")] slot: &'static str,
) -> Element {
    rsx! {
        svg {
            class: class,
            "data-slot": slot,
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            circle { cx: "12", cy: "12", r: "10" }
        }
    }
}

/// lucide `check`。
#[component]
pub fn IconCheck(
    #[props(default, into)] class: String,
    #[props(default = "check")] slot: &'static str,
) -> Element {
    rsx! {
        svg {
            class: class,
            "data-slot": slot,
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            path { d: "M20 6 9 17l-5-5" }
        }
    }
}

/// lucide `x`。
#[component]
pub fn IconX(
    #[props(default, into)] class: String,
    #[props(default = "x")] slot: &'static str,
) -> Element {
    rsx! {
        svg {
            class: class,
            "data-slot": slot,
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            path { d: "M18 6 6 18" }
            path { d: "m6 6 12 12" }
        }
    }
}

/// lucide `search`。
#[component]
pub fn IconSearch(
    #[props(default, into)] class: String,
    #[props(default = "search")] slot: &'static str,
) -> Element {
    rsx! {
        svg {
            class: class,
            "data-slot": slot,
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            circle { cx: "11", cy: "11", r: "8" }
            path { d: "m21 21-4.3-4.3" }
        }
    }
}

lucide!(IconPlus, "plus",
    path { d: "M5 12h14" }
    path { d: "M12 5v14" }
);

lucide!(IconMinus, "minus", path { d: "M5 12h14" });

lucide!(IconSettings, "settings",
    path { d: "M20 7h-9" }
    path { d: "M14 17H5" }
    circle { cx: "17", cy: "17", r: "3" }
    circle { cx: "7", cy: "7", r: "3" }
);

lucide!(IconTrash, "trash",
    path { d: "M3 6h18" }
    path { d: "M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" }
    path { d: "M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" }
    line { x1: "10", x2: "10", y1: "11", y2: "17" }
    line { x1: "14", x2: "14", y1: "11", y2: "17" }
);

lucide!(IconCopy, "copy",
    rect { width: "14", height: "14", x: "8", y: "8", rx: "2", ry: "2" }
    path { d: "M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" }
);

lucide!(IconPencil, "pencil",
    path { d: "M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z" }
    path { d: "m15 5 4 4" }
);

lucide!(IconSun, "sun",
    circle { cx: "12", cy: "12", r: "4" }
    path { d: "M12 2v2" }
    path { d: "M12 20v2" }
    path { d: "m4.93 4.93 1.41 1.41" }
    path { d: "m17.66 17.66 1.41 1.41" }
    path { d: "M2 12h2" }
    path { d: "M20 12h2" }
    path { d: "m6.34 17.66-1.41 1.41" }
    path { d: "m19.07 4.93-1.41 1.41" }
);

lucide!(IconMoon, "moon",
    path { d: "M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z" }
);

lucide!(IconSparkles, "sparkles",
    path { d: "M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z" }
    path { d: "M20 3v4" }
    path { d: "M22 5h-4" }
    path { d: "M4 17v2" }
    path { d: "M5 18H3" }
);

lucide!(IconTerminal, "terminal",
    path { d: "m4 17 6-6-6-6" }
    path { d: "M12 19h8" }
);

lucide!(IconFileText, "file-text",
    path { d: "M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z" }
    path { d: "M14 2v4a2 2 0 0 0 2 2h4" }
    path { d: "M10 9H8" }
    path { d: "M16 13H8" }
    path { d: "M16 17H8" }
);

lucide!(IconPanelRight, "panel-right",
    rect { width: "18", height: "18", x: "3", y: "3", rx: "2" }
    path { d: "M15 3v18" }
);

lucide!(IconMessageSquare, "message-square",
    path { d: "M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" }
);

lucide!(IconGitBranch, "git-branch",
    line { x1: "6", x2: "6", y1: "3", y2: "15" }
    circle { cx: "18", cy: "6", r: "3" }
    circle { cx: "6", cy: "18", r: "3" }
    path { d: "M18 9a9 9 0 0 1-9 9" }
);

lucide!(IconDownload, "download",
    path { d: "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" }
    polyline { points: "7 10 12 15 17 10" }
    line { x1: "12", x2: "12", y1: "15", y2: "3" }
);

lucide!(IconSend, "send",
    path { d: "M14.536 21.686a.5.5 0 0 0 .937-.024l6.5-19a.496.496 0 0 0-.635-.635l-19 6.5a.5.5 0 0 0-.024.937l7.93 3.18a2 2 0 0 1 1.112 1.11z" }
    path { d: "m21.854 2.147-10.94 10.939" }
);

lucide!(IconPaperclip, "paperclip",
    path { d: "m16 6-8.414 8.586a2 2 0 0 0 2.829 2.829l8.414-8.586a4 4 0 1 0-5.657-5.657l-8.379 8.551a6 6 0 1 0 8.485 8.485l8.379-8.551" }
);

lucide!(IconStop, "stop",
    rect { width: "14", height: "14", x: "5", y: "5", rx: "2" }
);

lucide!(IconRefresh, "refresh",
    path { d: "M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" }
    path { d: "M21 3v5h-5" }
);

lucide!(IconLayers, "layers",
    path { d: "M12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83z" }
    path { d: "m22 17.65-9.17 4.16a2 2 0 0 1-1.66 0L2 17.65" }
    path { d: "m22 12.65-9.17 4.16a2 2 0 0 1-1.66 0L2 12.65" }
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_are_constructible() {
        let _ = IconChevronDown as fn(IconChevronDownProps) -> Element;
        let _ = IconChevronUp as fn(IconChevronUpProps) -> Element;
        let _ = IconCircle as fn(IconCircleProps) -> Element;
        let _ = IconCheck as fn(IconCheckProps) -> Element;
        let _ = IconX as fn(IconXProps) -> Element;
        let _ = IconSearch as fn(IconSearchProps) -> Element;
        let _ = IconChevronRight as fn(IconChevronRightProps) -> Element;
        let _ = IconPlus as fn(IconPlusProps) -> Element;
        let _ = IconMinus as fn(IconMinusProps) -> Element;
        let _ = IconSettings as fn(IconSettingsProps) -> Element;
        let _ = IconTrash as fn(IconTrashProps) -> Element;
        let _ = IconCopy as fn(IconCopyProps) -> Element;
        let _ = IconPencil as fn(IconPencilProps) -> Element;
        let _ = IconSun as fn(IconSunProps) -> Element;
        let _ = IconMoon as fn(IconMoonProps) -> Element;
        let _ = IconSparkles as fn(IconSparklesProps) -> Element;
        let _ = IconTerminal as fn(IconTerminalProps) -> Element;
        let _ = IconFileText as fn(IconFileTextProps) -> Element;
        let _ = IconPanelRight as fn(IconPanelRightProps) -> Element;
        let _ = IconMessageSquare as fn(IconMessageSquareProps) -> Element;
        let _ = IconGitBranch as fn(IconGitBranchProps) -> Element;
        let _ = IconDownload as fn(IconDownloadProps) -> Element;
        let _ = IconSend as fn(IconSendProps) -> Element;
        let _ = IconPaperclip as fn(IconPaperclipProps) -> Element;
        let _ = IconStop as fn(IconStopProps) -> Element;
        let _ = IconRefresh as fn(IconRefreshProps) -> Element;
        let _ = IconLayers as fn(IconLayersProps) -> Element;
    }
}
