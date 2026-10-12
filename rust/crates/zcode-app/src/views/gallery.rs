//! 组件画廊：把移植过来的原语逐个摆出来。
//!
//! 这个对话框的用途不是「好看」，而是**可验证**：对照 TS 版逐项检查
//! variant / size / data-* 锚点是否都落地了。移植过程中最容易出的问题是
//! 「类名在、效果不在」（见 `rust/PORTING-UI.md` §3.4），只有真的渲染出来才看得见。

use dioxus::prelude::*;
use zcode_ui::*;

/// 组件画廊。
#[component]
pub fn GalleryDialog() -> Element {
    let ctx = crate::state::use_app();
    let tab = use_signal(|| "buttons".to_string());

    rsx! {
        Dialog { open: Some(ctx.gallery_open),
            DialogContent { class: "max-w-4xl",
                DialogHeader {
                    DialogTitle { "组件画廊" }
                    DialogDescription {
                        "zcode-ui 里已移植的 shadcn 原语。所有样式来自 packages/ui/src/styles.css 的令牌。"
                    }
                }

                Tabs { value: tab,
                    TabsList {
                        TabsTrigger { value: "buttons", "按钮 / 徽标" }
                        TabsTrigger { value: "forms", "表单" }
                        TabsTrigger { value: "feedback", "反馈" }
                        TabsTrigger { value: "display", "展示" }
                        TabsTrigger { value: "nav", "导航" }
                        TabsTrigger { value: "overlay", "浮层" }
                    }

                    div { class: "h-[420px]",
                        ScrollArea { class: "h-full",
                            TabsContent { value: "buttons", class: "pt-3",
                                ButtonsSection {}
                            }
                            TabsContent { value: "forms", class: "pt-3",
                                FormsSection {}
                            }
                            TabsContent { value: "feedback", class: "pt-3",
                                FeedbackSection {}
                            }
                            TabsContent { value: "display", class: "pt-3",
                                DisplaySection {}
                            }
                            TabsContent { value: "nav", class: "pt-3",
                                NavSection {}
                            }
                            TabsContent { value: "overlay", class: "pt-3",
                                OverlaySection {}
                            }
                        }
                    }
                }
            }
        }
    }
}

/// 分节外壳。
#[component]
fn Section(title: &'static str, hint: &'static str, children: Element) -> Element {
    rsx! {
        div { class: "flex flex-col gap-2",
            div { class: "flex items-baseline gap-2",
                span { class: "text-ui-base font-medium", {title} }
                span { class: "text-ui-xs text-foreground-subtlest", {hint} }
            }
            div { class: "flex flex-wrap items-center gap-2", {children} }
        }
    }
}

/// 按钮与徽标。
#[component]
fn ButtonsSection() -> Element {
    rsx! {
        div { class: "flex flex-col gap-4",
            Section { title: "Button", hint: "7 variant × 9 size",
                for v in [
                    ButtonVariant::Default,
                    ButtonVariant::Outline,
                    ButtonVariant::Secondary,
                    ButtonVariant::Ghost,
                    ButtonVariant::Destructive,
                    ButtonVariant::Warning,
                    ButtonVariant::Link,
                ] {
                    Button { key: "{v.as_str()}", variant: v, {v.as_str()} }
                }
            }
            Section { title: "Button size", hint: "h-5 → h-8",
                for s in [
                    ButtonSize::Xs,
                    ButtonSize::Sm,
                    ButtonSize::Default,
                    ButtonSize::Lg,
                ] {
                    Button { key: "{s.as_str()}", size: s, {s.as_str()} }
                }
                for s in [
                    ButtonSize::IconXs,
                    ButtonSize::IconSm,
                    ButtonSize::Icon,
                    ButtonSize::IconMd,
                    ButtonSize::IconLg,
                ] {
                    Button { key: "{s.as_str()}", size: s, variant: ButtonVariant::Outline,
                        IconSettings {}
                    }
                }
            }
            Section { title: "Badge", hint: "6 variant",
                for v in [
                    BadgeVariant::Default,
                    BadgeVariant::Secondary,
                    BadgeVariant::Destructive,
                    BadgeVariant::Outline,
                    BadgeVariant::Ghost,
                    BadgeVariant::Link,
                ] {
                    Badge { key: "{v.as_str()}", variant: v, {v.as_str()} }
                }
            }
            Section { title: "ButtonGroup", hint: "horizontal / vertical",
                ButtonGroup {
                    Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, "左" }
                    Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, "中" }
                    Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, "右" }
                }
                ButtonGroup { orientation: ButtonGroupOrientation::Vertical,
                    Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, "上" }
                    Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, "下" }
                }
                ButtonGroup {
                    ButtonGroupText { IconSearch {} }
                    Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, "搜索" }
                    ButtonGroupSeparator {}
                    Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, "全部" }
                }
            }
            Section { title: "Kbd", hint: "text-ui-xs / min-w-5",
                Kbd { "⌘" }
                Kbd { "K" }
                KbdGroup {
                    Kbd { "⌘" }
                    Kbd { "⇧" }
                    Kbd { "P" }
                }
            }
        }
    }
}

/// 表单。
#[component]
fn FormsSection() -> Element {
    let mut text = use_signal(String::new);
    let mut area = use_signal(String::new);
    let mut checked = use_signal(|| CheckboxState::Checked);
    let mut on = use_signal(|| true);
    let mut radio = use_signal(|| "ask".to_string());

    rsx! {
        div { class: "flex flex-col gap-4",
            Section { title: "Input", hint: "xs / sm / default / lg",
                for s in [InputSize::Xs, InputSize::Sm, InputSize::Default, InputSize::Lg] {
                    Input {
                        key: "{s.as_str()}",
                        size: s,
                        placeholder: s.as_str(),
                        class: "w-36",
                    }
                }
                Input { placeholder: "disabled", disabled: true, class: "w-36" }
            }
            Section { title: "Textarea", hint: "field-sizing-content",
                Textarea { placeholder: "多行输入", rows: 3, class: "w-full" }
            }
            Section { title: "Label", hint: "peer-disabled:*",
                Label { html_for: "demo-input", "表单标签" }
            }
            Section { title: "Checkbox", hint: "checked / indeterminate / disabled",
                div { class: "flex items-center gap-2",
                    Checkbox {
                        checked: checked(),
                        on_checked_change: move |c| checked.set(c),
                    }
                    span { class: "text-ui-sm text-foreground-subtle",
                        "state = {checked().as_str()}"
                    }
                }
                Checkbox { checked: CheckboxState::Indeterminate }
                Checkbox { checked: CheckboxState::Checked, disabled: true }
            }
            Section { title: "Switch", hint: "default / sm",
                Switch { checked: on(), on_checked_change: move |v| on.set(v) }
                Switch { checked: on(), size: SwitchSize::Sm, on_checked_change: move |v| on.set(v) }
                Switch { checked: false, disabled: true }
            }
            Section { title: "RadioGroup", hint: "grid gap-2",
                RadioGroup { value: Some(radio),
                    for v in ["ask", "auto", "readonly"] {
                        div { key: "{v}", class: "flex items-center gap-2",
                            onclick: move |_| radio.set(v.to_string()),
                            RadioGroupItem { value: v }
                            span { class: "text-ui-base/relaxed", {v} }
                        }
                    }
                }
            }
            Section { title: "InputGroup", hint: "addon 四种 align",
                div { class: "flex w-full flex-col gap-2",
                    InputGroup {
                        InputGroupAddon { align: InputGroupAddonAlign::InlineStart,
                            IconSearch { class: "size-3.5" }
                        }
                        InputGroupInput {
                            placeholder: "inline-start",
                            value: text(),
                            oninput: move |e: FormEvent| text.set(e.value()),
                        }
                        InputGroupAddon { align: InputGroupAddonAlign::InlineEnd,
                            InputGroupText { "⌘K" }
                        }
                    }
                    InputGroup {
                        InputGroupAddon { align: InputGroupAddonAlign::BlockStart,
                            InputGroupText { "标题" }
                        }
                        InputGroupTextarea {
                            placeholder: "block-start：addon 独占一行",
                            value: area(),
                            oninput: move |e: FormEvent| area.set(e.value()),
                        }
                    }
                    InputGroup {
                        InputGroupTextarea { placeholder: "block-end：addon 在下方" }
                        InputGroupAddon { align: InputGroupAddonAlign::BlockEnd,
                            InputGroupText { "字数 0 / 200" }
                        }
                    }
                }
            }
        }
    }
}

/// 反馈。
#[component]
fn FeedbackSection() -> Element {
    rsx! {
        div { class: "flex flex-col gap-4",
            Section { title: "Alert", hint: "default / warning / destructive",
                div { class: "flex w-full flex-col gap-2",
                    Alert { variant: AlertVariant::Default,
                        IconSparkles { class: "size-3.5" }
                        AlertTitle { "默认提示" }
                        AlertDescription { "border-card-border bg-card" }
                    }
                    Alert { variant: AlertVariant::Warning,
                        IconSparkles { class: "size-3.5" }
                        AlertTitle { "警告" }
                        AlertDescription { "border-warning/40 bg-warning/10" }
                    }
                    Alert { variant: AlertVariant::Destructive,
                        IconX { class: "size-3.5" }
                        AlertTitle { "错误" }
                        AlertDescription { "bg-destructive text-destructive-foreground" }
                        AlertAction {
                            Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconXs,
                                IconX {}
                            }
                        }
                    }
                }
            }
            Section { title: "Progress", hint: "solid / segmented",
                div { class: "flex w-full flex-col gap-2",
                    Progress { value: 62.0 }
                    Progress { value: 100.0 }
                    Progress {
                        value: 100.0,
                        segments: Some(vec![
                            ProgressSegment::new("a", 0.4).with_class("bg-primary"),
                            ProgressSegment::new("b", 0.25).with_class("bg-destructive"),
                            ProgressSegment::new("c", 0.15).with_class("bg-warning"),
                        ]),
                    }
                }
            }
            Section { title: "Spinner", hint: "lucide loader",
                Spinner {}
                Spinner { class: "size-6 text-primary" }
                Spinner { class: "size-3 text-foreground-subtlest" }
            }
        }
    }
}

/// 展示。
#[component]
fn DisplaySection() -> Element {
    rsx! {
        div { class: "flex flex-col gap-4",
            Section { title: "Avatar", hint: "sm / default / lg + badge",
                Avatar { size: AvatarSize::Sm, AvatarFallback { "S" } }
                Avatar { AvatarFallback { "D" } }
                Avatar { size: AvatarSize::Lg,
                    AvatarFallback { "L" }
                    AvatarBadge {}
                }
                AvatarGroup {
                    Avatar { AvatarFallback { "A" } }
                    Avatar { AvatarFallback { "B" } }
                    Avatar { AvatarFallback { "C" } }
                    AvatarGroupCount { "+9" }
                }
            }
            Section { title: "Card", hint: "header / content / footer / action",
                div { class: "w-full",
                    Card {
                        CardHeader {
                            CardTitle { "Card 标题" }
                            CardDescription { "group/card + @container/card-header" }
                            CardAction {
                                Badge { variant: BadgeVariant::Outline, "action" }
                            }
                        }
                        CardContent { "CardContent —— px-4" }
                        CardFooter { class: "border-t",
                            Button { variant: ButtonVariant::Outline, size: ButtonSize::Xs,
                                "取消"
                            }
                            Button { size: ButtonSize::Xs, "确认" }
                        }
                    }
                }
            }
            Section { title: "Separator", hint: "horizontal / vertical",
                div { class: "flex h-8 items-center gap-3",
                    span { class: "text-ui-sm", "上" }
                    Separator { orientation: SeparatorOrientation::Vertical }
                    span { class: "text-ui-sm", "下" }
                }
                Separator { class: "w-full" }
            }
            Section { title: "ScrollArea", hint: "原生滚动 + 可选隐藏滚动条",
                ScrollArea { class: "h-24 w-full rounded-md border border-border",
                    div { class: "flex flex-col gap-1 p-2",
                        for i in 0..20 {
                            span { key: "{i}", class: "text-ui-xs text-foreground-subtle",
                                "滚动内容第 {i + 1} 行"
                            }
                        }
                    }
                }
            }
        }
    }
}

/// 导航。
#[component]
fn NavSection() -> Element {
    let tab = use_signal(|| "a".to_string());
    let open = use_signal(|| true);

    rsx! {
        div { class: "flex flex-col gap-4",
            Section { title: "Tabs", hint: "default / line",
                div { class: "w-full",
                    Tabs { value: tab,
                        TabsList {
                            TabsTrigger { value: "a", "选项 A" }
                            TabsTrigger { value: "b", "选项 B" }
                            TabsTrigger { value: "c", "选项 C" }
                        }
                        TabsContent { value: "a", class: "pt-2 text-ui-sm", "面板 A" }
                        TabsContent { value: "b", class: "pt-2 text-ui-sm", "面板 B" }
                        TabsContent { value: "c", class: "pt-2 text-ui-sm", "面板 C" }
                    }
                }
                div { class: "w-full",
                    Tabs { value: tab,
                        TabsList { variant: TabsListVariant::Line,
                            TabsTrigger { value: "a", "Line A" }
                            TabsTrigger { value: "b", "Line B" }
                        }
                    }
                }
            }
            Section { title: "Accordion", hint: "single / multiple",
                div { class: "w-full",
                    Accordion { kind: AccordionType::Multiple,
                        AccordionItem { value: "1",
                            AccordionTrigger { "第一项" }
                            AccordionContent { "data-open:bg-muted/50 + 两个 chevron 图标切换" }
                        }
                        AccordionItem { value: "2",
                            AccordionTrigger { "第二项" }
                            AccordionContent { "not-last:border-b" }
                        }
                        AccordionItem { value: "3",
                            AccordionTrigger { "禁用项" }
                            AccordionContent { "不该看到这段" }
                        }
                    }
                }
            }
            Section { title: "Collapsible", hint: "受控 open: Some(signal)",
                div { class: "w-full",
                    Collapsible { open: Some(open),
                        CollapsibleTrigger {
                            class: "flex items-center gap-1 text-ui-sm",
                            if open() {
                                IconChevronDown { class: "size-3.5" }
                            } else {
                                IconChevronRight { class: "size-3.5" }
                            }
                            "展开 / 收起"
                        }
                        CollapsibleContent {
                            div { class: "pt-2 text-ui-sm text-foreground-subtle",
                                "关闭时保持挂载并加 hidden —— 子树状态不丢（见模块文档）。"
                            }
                        }
                    }
                }
            }
        }
    }
}

/// 浮层。
#[component]
fn OverlaySection() -> Element {
    // 注意：hook 只能在渲染期调用，不能写进事件回调里
    let ctx = crate::state::use_app();

    rsx! {
        div { class: "flex flex-col gap-4",
            Section { title: "Tooltip", hint: "hover 立即显示（无 delayDuration）",
                Tooltip {
                    TooltipTrigger {
                        Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, "悬停我" }
                    }
                    TooltipContent { "这是一个 tooltip" }
                }
                Tooltip {
                    TooltipTrigger {
                        Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, "下方" }
                    }
                    TooltipContent { side: Side::Bottom, side_offset: 6.0, "side = bottom" }
                }
                Tooltip {
                    TooltipTrigger {
                        Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, "右侧" }
                    }
                    TooltipContent { side: Side::Right, align: Align::Start, side_offset: 6.0,
                        "side = right"
                    }
                }
            }
            Section { title: "HoverCard", hint: "w-72 / 无 openDelay",
                HoverCard {
                    HoverCardTrigger {
                        Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, "悬停卡片" }
                    }
                    HoverCardContent {
                        div { class: "flex flex-col gap-2",
                            span { class: "text-ui-base font-medium", "HoverCardContent" }
                            Separator {}
                            span { class: "text-ui-sm text-foreground-subtle",
                                "bg-popover + shadow-md + ring-1 ring-foreground/10"
                            }
                        }
                    }
                }
            }
            Section { title: "Popover", hint: "点击切换 / 无点击外部关闭",
                Popover {
                    PopoverTrigger {
                        Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, "打开 Popover" }
                    }
                    PopoverContent {
                        PopoverHeader {
                            PopoverTitle { "Popover" }
                            PopoverDescription { "z-[60]，刻意高于 tooltip 的 z-50。" }
                        }
                        Separator {}
                        div { class: "flex items-center gap-2",
                            Checkbox { checked: CheckboxState::Checked }
                            span { class: "text-ui-sm", "带一个复选框" }
                        }
                    }
                }
            }
            Section { title: "DropdownMenu", hint: "highlight 由 hover 驱动",
                DropdownMenu {
                    DropdownMenuTrigger {
                        variant: ButtonVariant::Outline,
                        size: ButtonSize::Sm,
                        "打开菜单"
                        IconChevronDown { class: "size-3" }
                    }
                    DropdownMenuContent { class: "w-56",
                        DropdownMenuLabel { "操作" }
                        DropdownMenuGroup {
                            DropdownMenuItem {
                                IconCopy { class: "size-3.5" }
                                "复制"
                                DropdownMenuShortcut { "⌘C" }
                            }
                            DropdownMenuItem {
                                IconPencil { class: "size-3.5" }
                                "重命名"
                                DropdownMenuShortcut { "F2" }
                            }
                            DropdownMenuCheckboxItem { checked: true, inset: true,
                                "显示隐藏文件"
                            }
                            DropdownMenuSeparator {}
                            DropdownMenuSub {
                                DropdownMenuSubTrigger { inset: true,
                                    "更多"
                                }
                                DropdownMenuSubContent {
                                    DropdownMenuItem { "子项一" }
                                    DropdownMenuItem { "子项二" }
                                }
                            }
                        }
                        DropdownMenuSeparator {}
                        DropdownMenuItem {
                            variant: DropdownMenuItemVariant::Destructive,
                            IconTrash { class: "size-3.5" }
                            "删除"
                        }
                    }
                }
            }
            Section { title: "Dialog / AlertDialog", hint: "fixed 居中，无需 Portal",
                Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm,
                    onclick: move |_| {
                        let mut o = ctx.settings_open;
                        o.set(true);
                    },
                    "打开设置（Dialog + Tabs）"
                }
                AlertDialog {
                    AlertDialogTrigger {
                        variant: ButtonVariant::Destructive,
                        size: ButtonSize::Sm,
                        "打开确认框"
                    }
                    AlertDialogContent {
                        AlertDialogHeader {
                            AlertDialogTitle { "AlertDialog" }
                            AlertDialogDescription {
                                "gap-5 / rounded-2xl / ring-border / shadow-2xl / sm:max-w-md"
                            }
                        }
                        AlertDialogFooter {
                            AlertDialogCancel { "取消" }
                            AlertDialogAction { "确认" }
                        }
                    }
                }
            }
        }
    }
}
