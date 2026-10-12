//! `Command` 家族 —— Rust 移植自 `packages/ui/src/components/ui/command.tsx`
//!
//! TS 用 `cmdk`：一个自带「过滤 + 分组 + 键盘漫游 + 空态」的命令面板原语。
//! Rust 侧没有 cmdk，这里把它的**可判定部分抽成纯函数**（[`command_filter_matches`]、
//! [`command_visible_indices`]、[`command_navigate`]），反应式接线只做「把结果
//! 写进 `Signal`」这一件事。
//!
//! ## 关键移植点 1：过滤算法只复刻「匹配与否」，不复刻打分
//!
//! cmdk 默认用 `command-score` 包：对候选串做**子序列**匹配并给出 0–1 分，
//! 再按分排序。Rust 侧 [`command_filter_matches`] 只回答「是否匹配」
//! （大小写不敏感的子序列匹配），**不重排**：顺序仍由调用方的书写顺序决定。
//! 这是刻意的取舍 —— 打分/排序会影响视觉顺序，宁可保持可预测。
//!
//! 与 cmdk 一致的边界：空查询匹配一切（初始状态全量可见）。
//!
//! ## 关键移植点 2：`data-selected` 与「hover 态共用一套背景」
//!
//! TS 注释里点名了一个 bug：cmdk 键盘上下键只切 `data-selected`，没同步背景色，
//! 于是「键盘选中态」和「鼠标 hover 态」看起来不是同一个状态。修法是让两者
//! **共用 `menu-hover`**。Rust 侧原样保留这个修法：
//! `hover:bg-menu-hover` 与 `data-selected:bg-menu-hover` 并存。
//!
//! `data-selected` 是**存在即真**的裸属性（对应 Tailwind 的 `data-selected:` →
//! `[data-selected]`），而 `data-disabled` 是**值**属性（`data-[disabled=true]:`）。
//! 两者不能混。
//!
//! ## 关键移植点 3：项注册表与 `use_drop`
//!
//! [`CommandEmpty`] 要判断「有没有任何项可见」，键盘漫游要跳过被过滤掉的项，
//! 因此需要一张注册表（[`CommandContext::matches`]）。做法：
//!
//! - 每个项用 `use_hook` 领一个自增索引，把匹配结果写进 `matches[index]`；
//! - 写之前用一个 `prev` 信号比对，**只在变化时写**，避免渲染期无限回写；
//! - 用 `use_drop` 在卸载时把自己的槽位置回 `false`。
//!
//! 最后一条是让「`CommandList` 卸载重挂载」也能正确的原因：索引会继续累加，
//! 但旧槽位已被置 `false`，导航会自动跳过它们，`CommandEmpty` 的计数也不会多算。
//!
//! ## 与 TS 的差异
//!
//! 1. 没有 `CommandDialog` 的 `Dialog` 内部状态联动差异：这里直接复用
//!    [`crate::components::dialog`]，`title` / `description` 用 `sr-only` 头部承载（同 TS）。
//! 2. 分组标题依赖 `cmdk-group-heading` 属性 + 组上的 `**:[[cmdk-group-heading]]`
//!    任意变体来上样式（与 TS 完全一致）；属性值是空串，`[attr]` 只判存在。
//! 3. 没有 `CommandPrimitive` 的 `filter` / `shouldFilter` 开关与 `forceMount`。
//! 4. 键盘只支持 ↑ / ↓ / Home / End；`Enter` 交给浏览器转成的 `click`。
//! 5. `scrollbar-hide` 由上游的 `tailwind-scrollbar-hide` 插件提供（本仓库没有这个
//!    包），改由 `rust/.tailwind-build/build.sh` 里的 `@utility` 补上。

use crate::cn::cn;
use crate::components::dialog::{
    Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle,
};
use crate::components::input_group::{InputGroup, InputGroupAddon};
use crate::icons::{IconCheck, IconSearch};
use dioxus::prelude::*;

/// `Command` 根容器基础类名（TS 原文，逐字保留）。
pub const COMMAND_BASE: &str =
    "flex size-full flex-col overflow-hidden rounded-xl bg-popover p-1 text-popover-foreground";

/// `CommandDialog` 里 `DialogContent` 的附加类名（TS 原文，逐字保留）。
pub const COMMAND_DIALOG_CONTENT: &str = "top-1/3 translate-y-0 overflow-hidden p-0";

/// `CommandInput` 外层包裹类名（TS 原文，逐字保留）。
pub const COMMAND_INPUT_WRAPPER: &str = "p-1 border-b border-border";

/// `CommandInput` 里 `InputGroup` 的附加类名（TS 原文，逐字保留）。
pub const COMMAND_INPUT_GROUP: &str =
    "h-8 border-0 !bg-transparent hover:border-input-border-hover";

/// `CommandInput` 的输入框类名（TS 原文，逐字保留）。
pub const COMMAND_INPUT: &str = "w-full text-ui-base/relaxed text-foreground outline-hidden placeholder:text-foreground-subtlest disabled:cursor-not-allowed disabled:opacity-50";

/// `CommandInput` 里的放大镜图标类名（TS 原文，逐字保留）。
pub const COMMAND_INPUT_ICON: &str = "size-4 shrink-0 text-foreground-subtlest";

/// `CommandList` 基础类名（TS 原文，逐字保留）。
pub const COMMAND_LIST: &str =
    "scrollbar-hide max-h-72 scroll-py-1 overflow-x-hidden overflow-y-auto outline-none";

/// `CommandEmpty` 基础类名（TS 原文，逐字保留）。
pub const COMMAND_EMPTY: &str = "py-6 text-center text-ui-base/relaxed";

/// `CommandGroup` 基础类名（TS 原文，逐字保留）。
///
/// `**:[[cmdk-group-heading]]:*` 是给组内任意带 `cmdk-group-heading` 属性的
/// 后代上样式 —— 所以标题元素必须真的带这个属性（见 [`COMMAND_GROUP_HEADING_ATTR`]）。
pub const COMMAND_GROUP: &str = "overflow-hidden p-1 text-foreground **:[[cmdk-group-heading]]:px-2.5 **:[[cmdk-group-heading]]:py-1.5 **:[[cmdk-group-heading]]:text-ui-base **:[[cmdk-group-heading]]:font-medium **:[[cmdk-group-heading]]:text-foreground-subtle";

/// 分组标题元素上的标记属性名。值是空串 —— `[attr]` 选择器只判存在。
pub const COMMAND_GROUP_HEADING_ATTR: &str = "cmdk-group-heading";

/// `CommandSeparator` 基础类名（TS 原文，逐字保留）。
///
/// 注意是 `bg-border/50`（半透明），与 `DropdownMenu` 的 `bg-border` 不同。
pub const COMMAND_SEPARATOR: &str = "-mx-1 my-1 h-px bg-border/50";

/// `CommandItem` 基础类名（TS 原文，逐字保留）。
pub const COMMAND_ITEM: &str = "group/command-item relative flex min-h-7 cursor-default items-center gap-2 rounded-lg px-2.5 py-1.5 text-ui-base/relaxed outline-hidden select-none data-[disabled=true]:pointer-events-none data-[disabled=true]:opacity-50 hover:bg-menu-hover data-selected:bg-menu-hover data-selected:text-foreground [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-3.5 data-selected:*:[svg]:text-foreground";

/// `CommandItem` 尾部对勾的类名（TS 原文，逐字保留）。
///
/// 三层条件：默认 `hidden`；有 `command-shortcut` 子节点时也隐藏；
/// 只有项自己 `data-checked=true` 时才 `inline-block`。
pub const COMMAND_ITEM_CHECK: &str = "size-4 ml-auto !text-foreground-subtle hidden group-has-data-[slot=command-shortcut]/command-item:hidden group-data-[checked=true]/command-item:inline-block";

/// `CommandShortcut` 基础类名（TS 原文，逐字保留）。
pub const COMMAND_SHORTCUT: &str = "ml-auto text-ui-xs tracking-widest text-foreground-subtlest group-data-selected/command-item:text-foreground";

/// 键盘漫游方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CommandNav {
    Next,
    Prev,
    First,
    Last,
}

/// 判断候选串是否命中查询串。
///
/// 规则（与 cmdk 的「是否匹配」一致，不复刻打分）：
///
/// - 查询串 `trim` 后为空 → 恒真（初始状态全量可见）。
/// - 否则做**大小写不敏感的子序列**匹配：查询串的字符必须按序出现在候选串里
///   （允许中间隔着别的字符）。例如 `"gs"` 命中 `"Git Status"`。
///
/// 不剥离音调符号（cmdk 会），见模块文档。
pub fn command_filter_matches(text: &str, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }

    let mut haystack = text.chars().flat_map(char::to_lowercase);
    // 逐字符消费候选串：能在其中按序找到查询串的每个字符即命中。
    query
        .chars()
        .flat_map(char::to_lowercase)
        .all(|needle| haystack.any(|c| c == needle))
}

/// 从匹配位图里取出「可见项」的索引序列（升序）。
pub fn command_visible_indices(matches: &[bool]) -> Vec<usize> {
    matches
        .iter()
        .enumerate()
        .filter_map(|(i, m)| m.then_some(i))
        .collect()
}

/// 键盘漫游：在**可见项**之间移动高亮。
///
/// `loop_enabled` 对应 cmdk 的 `loop`（本仓库的包装默认 `false`）。
///
/// - 可见项为空 → `None`。
/// - 非环绕模式下撞到两端**停在原地**（返回 `current`），与 cmdk 一致。
/// - `current` 不在可见集合里（例如刚被过滤掉）时，`Next` / `Prev` 分别落到
///   第一项 / 最后一项。
pub fn command_navigate(
    current: Option<usize>,
    matches: &[bool],
    nav: CommandNav,
    loop_enabled: bool,
) -> Option<usize> {
    let visible = command_visible_indices(matches);
    if visible.is_empty() {
        return None;
    }

    match nav {
        CommandNav::First => Some(visible[0]),
        CommandNav::Last => Some(*visible.last().unwrap()),
        CommandNav::Next => match current {
            None => Some(visible[0]),
            Some(cur) => match visible.iter().find(|i| **i > cur) {
                Some(next) => Some(*next),
                // 已在最后一个可见项
                None if loop_enabled => Some(visible[0]),
                None => Some(cur),
            },
        },
        CommandNav::Prev => match current {
            None => Some(*visible.last().unwrap()),
            Some(cur) => match visible.iter().rev().find(|i| **i < cur) {
                Some(prev) => Some(*prev),
                None if loop_enabled => Some(*visible.last().unwrap()),
                None => Some(cur),
            },
        },
    }
}

/// `Command` 的共享状态。
#[derive(Clone, Copy)]
struct CommandContext {
    /// 搜索框内容。
    query: Signal<String>,
    /// 键盘高亮的**项索引**（不是可见序号）。
    selected: Signal<Option<usize>>,
    /// 匹配位图：下标 = 项索引，值 = 该项当前是否命中查询。
    matches: Signal<Vec<bool>>,
    /// 下一个待分配的项索引。
    next_index: Signal<usize>,
}

/// 读取当前 Command 的搜索串。
pub fn use_command_query() -> Signal<String> {
    use_context::<CommandContext>().query
}

/// 读取当前 Command 的键盘高亮索引。
pub fn use_command_selected() -> Signal<Option<usize>> {
    use_context::<CommandContext>().selected
}

/// 命令面板根容器（状态持有者 + 键盘漫游）。
///
/// 键盘处理挂在根上：输入框与列表是兄弟节点，只有根是它们共同的祖先，
/// 事件从输入框冒泡到根才能统一处理。
#[component]
pub fn Command(
    /// 对应 cmdk 的 `loop`（本仓库 TS 包装默认 `false`）。
    ///
    /// Rust 里 `loop` 是关键字，故改名并在此说明。
    #[props(default)] loop_enabled: bool,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let query = use_signal(String::new);
    let selected = use_signal(|| None::<usize>);
    let matches = use_signal(Vec::<bool>::new);
    let next_index = use_signal(|| 0usize);

    use_context_provider(|| CommandContext {
        query,
        selected,
        matches,
        next_index,
    });

    rsx! {
        div {
            class: cn(&[COMMAND_BASE, &class]),
            "data-slot": "command",
            onkeydown: move |e| {
                let nav = match e.key() {
                    Key::ArrowDown => CommandNav::Next,
                    Key::ArrowUp => CommandNav::Prev,
                    Key::Home => CommandNav::First,
                    Key::End => CommandNav::Last,
                    _ => return,
                };
                e.prevent_default();
                let next = command_navigate(
                    (selected)(),
                    &(matches)(),
                    nav,
                    loop_enabled,
                );
                let mut selected = selected;
                selected.set(next);
            },
            {children}
        }
    }
}

/// 命令面板外壳：把内容塞进一个居中靠上的 `Dialog`。
#[component]
pub fn CommandDialog(
    #[props(default)] open: Option<Signal<bool>>,
    /// 无障碍标题（`sr-only`）。
    #[props(default = "Command Palette".to_string(), into)] title: String,
    /// 无障碍说明（`sr-only`）。
    #[props(default = "Search for a command to run...".to_string(), into)]
    description: String,
    /// 是否渲染右上角关闭按钮（TS 默认 `false`）。
    #[props(default = false)] show_close_button: bool,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        Dialog { open,
            DialogHeader { class: "sr-only",
                DialogTitle { {title} }
                DialogDescription { {description} }
            }
            DialogContent {
                class: cn(&[COMMAND_DIALOG_CONTENT, &class]),
                show_close_button,
                {children}
            }
        }
    }
}

/// 搜索框（`InputGroup` + 输入框 + 放大镜）。
#[component]
pub fn CommandInput(
    #[props(default, into)] class: String,
    #[props(default, into)] input_group_class: String,
    #[props(default, into)] placeholder: String,
    #[props(default)] disabled: bool,
    #[props(default)] oninput: EventHandler<FormEvent>,
    children: Element,
) -> Element {
    let mut ctx = use_context::<CommandContext>();

    rsx! {
        div {
            class: COMMAND_INPUT_WRAPPER,
            "data-slot": "command-input-wrapper",
            InputGroup {
                class: cn(&[COMMAND_INPUT_GROUP, &input_group_class]),
                input {
                    class: cn(&[COMMAND_INPUT, &class]),
                    "data-slot": "command-input",
                    r#type: "text",
                    // cmdk 的 Input 是「输入即筛选」，所以受控绑定 query
                    value: (ctx.query)(),
                    placeholder,
                    disabled,
                    // 输入框默认不进 tab 序（cmdk 用 autofocus + 自己的焦点管理）
                    autocomplete: "off",
                    // dioxus-html 没有 `autocorrect` 常量，只能用字符串键
                    "autocorrect": "off",
                    spellcheck: "false",
                    oninput: move |e| {
                        ctx.query.set(e.value());
                        // 查询变化后旧的高亮可能落在被过滤掉的项上，先清掉
                        ctx.selected.set(None);
                        oninput.call(e);
                    },
                    {children}
                }
                InputGroupAddon {
                    IconSearch { class: COMMAND_INPUT_ICON }
                }
            }
        }
    }
}

/// 结果列表（滚动容器）。
#[component]
pub fn CommandList(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: cn(&[COMMAND_LIST, &class]),
            "data-slot": "command-list",
            role: "listbox",
            {children}
        }
    }
}

/// 空态：**有任何一项可见时不渲染**。
///
/// 「注册表还是空的」也算不渲染 —— 项在渲染期注册，首帧可能尚未登记完，
/// 若此时按「零可见」显示空态会闪一下。
#[component]
pub fn CommandEmpty(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let ctx = use_context::<CommandContext>();
    let matches = (ctx.matches)();
    if matches.is_empty() || !command_visible_indices(&matches).is_empty() {
        return rsx! {};
    }

    rsx! {
        div {
            class: cn(&[COMMAND_EMPTY, &class]),
            "data-slot": "command-empty",
            {children}
        }
    }
}

/// 结果分组。`heading` 渲染成带 [`COMMAND_GROUP_HEADING_ATTR`] 的标题元素，
/// 样式由组上的任意变体提供（与 TS 一致）。
#[component]
pub fn CommandGroup(
    #[props(default, into)] heading: String,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: cn(&[COMMAND_GROUP, &class]),
            "data-slot": "command-group",
            role: "group",
            if !heading.is_empty() {
                div {
                    // 属性名必须写成字面量：rsx 不接受 `{CONST}: value` 形式。
                    // 名字与 COMMAND_GROUP_HEADING_ATTR 一致（测试会钉住这一点）。
                    "cmdk-group-heading": "",
                    "data-slot": "command-group-heading",
                    {heading}
                }
            }
            {children}
        }
    }
}

/// 分组之间的分隔线。
#[component]
pub fn CommandSeparator(
    #[props(default, into)] class: String,
) -> Element {
    rsx! {
        div {
            class: cn(&[COMMAND_SEPARATOR, &class]),
            "data-slot": "command-separator",
            role: "separator",
            "aria-orientation": "horizontal",
        }
    }
}

/// 命令项。
///
/// `value` 是参与过滤的候选串（cmdk 的 `value`，通常由内容派生）；
/// 留空时退回 `children` 的文本无从取得，因此**建议显式传**。
#[component]
pub fn CommandItem(
    /// 参与过滤的候选串（cmdk `value`）。
    #[props(default, into)] value: String,
    /// 额外关键词（cmdk `keywords`），与 `value` 拼接后一起参与匹配。
    #[props(default)] keywords: Vec<String>,
    #[props(default)] disabled: bool,
    /// 是否显示尾部对勾（cmdk `data-checked`）。
    #[props(default)] checked: bool,
    #[props(default, into)] class: String,
    #[props(default)] onselect: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    let mut ctx = use_context::<CommandContext>();

    // 项索引：按挂载顺序递增（`use_hook` 只在首次渲染跑一次）。
    let index = use_hook(|| {
        let mut next = ctx.next_index;
        let i = next();
        next.set(i + 1);
        i
    });

    let mut haystack = value.clone();
    for kw in &keywords {
        haystack.push(' ');
        haystack.push_str(kw);
    }
    let matched = command_filter_matches(&haystack, &(ctx.query)());

    // 只在匹配结果**变化**时写注册表：渲染期写信号若每次都写会触发无谓重渲染，
    // 比对 `prev` 后写入是收敛的（第二次渲染值相同，不再写）。
    let mut prev = use_signal(|| None::<bool>);
    if prev() != Some(matched) {
        prev.set(Some(matched));
        let mut slots = ctx.matches;
        let mut v = slots.write();
        if v.len() <= index {
            v.resize(index + 1, false);
        }
        v[index] = matched;
    }

    // 卸载时归还槽位。这是「列表重挂载后索引继续累加」仍然正确的原因：
    // 旧索引的槽位被置 false，导航会跳过、空态计数也不会多算。
    use_drop(move || {
        let mut slots = ctx.matches;
        let mut v = slots.write();
        if index < v.len() {
            v[index] = false;
        }
    });

    // 被过滤掉就不渲染（cmdk 也是把不匹配的项摘掉，而不是仅隐藏）
    if !matched {
        return rsx! {};
    }

    let is_selected = (ctx.selected)() == Some(index);

    rsx! {
        div {
            class: cn(&[COMMAND_ITEM, &class]),
            "data-slot": "command-item",
            "data-index": "{index}",
            "data-value": value,
            "data-disabled": if disabled { "true" } else { "false" },
            // 存在即真：Tailwind 的 `data-selected:` 展开成 `[data-selected]`
            "data-selected": is_selected.then_some(""),
            "data-checked": if checked { "true" } else { "false" },
            "aria-selected": if is_selected { "true" } else { "false" },
            "aria-disabled": if disabled { "true" } else { "false" },
            role: "option",
            onmouseenter: move |_| ctx.selected.set(Some(index)),
            onclick: move |e| {
                if !disabled {
                    onselect.call(e);
                }
            },
            {children}
            IconCheck { class: COMMAND_ITEM_CHECK }
        }
    }
}

/// 项尾部的快捷键提示。
///
/// 它的存在会改变对勾的显隐：`CommandItem` 的对勾类名里有
/// `group-has-data-[slot=command-shortcut]/command-item:hidden`，
/// 所以本组件必须带 `data-slot="command-shortcut"`。
#[component]
pub fn CommandShortcut(
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    rsx! {
        span {
            class: cn(&[COMMAND_SHORTCUT, &class]),
            "data-slot": "command-shortcut",
            {children}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_matches_everything() {
        assert!(command_filter_matches("Git Status", ""));
        assert!(command_filter_matches("", ""));
        // 只有空白的查询也按「空」处理
        assert!(command_filter_matches("anything", "   "));
        assert!(command_filter_matches("anything", "\t\n"));
    }

    #[test]
    fn match_is_case_insensitive_substring() {
        assert!(command_filter_matches("Git Status", "git"));
        assert!(command_filter_matches("Git Status", "GIT"));
        assert!(command_filter_matches("Git Status", "status"));
        assert!(command_filter_matches("Git Status", "t st"));
        assert!(!command_filter_matches("Git Status", "gti"));
        assert!(!command_filter_matches("Git Status", "zzz"));
    }

    /// 子序列匹配：允许中间隔字符，但不允许乱序。
    #[test]
    fn match_is_subsequence_not_permutation() {
        assert!(command_filter_matches("Git Status", "gs"));
        assert!(command_filter_matches("Git Status", "gst"));
        assert!(command_filter_matches("Open Settings", "os"));
        assert!(!command_filter_matches("Open Settings", "so"));
        assert!(!command_filter_matches("abc", "acb"));
    }

    #[test]
    fn match_handles_unicode_and_multibyte() {
        assert!(command_filter_matches("切换主题", "切换"));
        assert!(command_filter_matches("切换主题", "主题"));
        assert!(command_filter_matches("切换主题", "切题"));
        assert!(!command_filter_matches("切换主题", "题切"));
        // 大小写折叠对 ASCII 与拉丁扩展都生效
        assert!(command_filter_matches("Ünïcode", "ünïcode"));
        assert!(command_filter_matches("Ünïcode", "ÜNÏCODE"));
    }

    #[test]
    fn visible_indices_are_ascending_and_skip_false() {
        assert_eq!(command_visible_indices(&[]), Vec::<usize>::new());
        assert_eq!(command_visible_indices(&[false, false]), Vec::<usize>::new());
        assert_eq!(command_visible_indices(&[true, false, true, true]), vec![0, 2, 3]);
        assert_eq!(command_visible_indices(&[false, true]), vec![1]);
    }

    #[test]
    fn navigate_skips_filtered_items() {
        // 只有 1 和 3 可见
        let m = [false, true, false, true];
        assert_eq!(command_navigate(None, &m, CommandNav::Next, false), Some(1));
        assert_eq!(command_navigate(Some(1), &m, CommandNav::Next, false), Some(3));
        assert_eq!(command_navigate(Some(3), &m, CommandNav::Prev, false), Some(1));
        assert_eq!(command_navigate(Some(1), &m, CommandNav::Prev, false), Some(1));
    }

    #[test]
    fn navigate_without_loop_stops_at_ends() {
        let m = [true, true, true];
        assert_eq!(command_navigate(Some(2), &m, CommandNav::Next, false), Some(2));
        assert_eq!(command_navigate(Some(0), &m, CommandNav::Prev, false), Some(0));
    }

    #[test]
    fn navigate_with_loop_wraps() {
        let m = [true, true, true];
        assert_eq!(command_navigate(Some(2), &m, CommandNav::Next, true), Some(0));
        assert_eq!(command_navigate(Some(0), &m, CommandNav::Prev, true), Some(2));
        // 单元素环绕仍是自身
        assert_eq!(command_navigate(Some(0), &[true], CommandNav::Next, true), Some(0));
    }

    #[test]
    fn navigate_first_and_last_ignore_current() {
        let m = [false, true, true, false];
        assert_eq!(command_navigate(Some(2), &m, CommandNav::First, false), Some(1));
        assert_eq!(command_navigate(Some(1), &m, CommandNav::Last, false), Some(2));
        assert_eq!(command_navigate(None, &m, CommandNav::First, false), Some(1));
        assert_eq!(command_navigate(None, &m, CommandNav::Last, false), Some(2));
    }

    /// 高亮落在被过滤掉的项上时（刚改过查询），方向决定落到哪端。
    #[test]
    fn navigate_recovers_when_current_is_filtered_out() {
        let m = [false, true, true, false];
        assert_eq!(command_navigate(Some(0), &m, CommandNav::Next, false), Some(1));
        assert_eq!(command_navigate(Some(3), &m, CommandNav::Prev, false), Some(2));
    }

    #[test]
    fn navigate_on_empty_visibility_is_none() {
        let m = [false, false];
        for nav in [
            CommandNav::Next,
            CommandNav::Prev,
            CommandNav::First,
            CommandNav::Last,
        ] {
            assert_eq!(command_navigate(Some(0), &m, nav, true), None);
            assert_eq!(command_navigate(None, &m, nav, false), None);
            assert_eq!(command_navigate(None, &[], nav, false), None);
        }
    }

    #[test]
    fn root_and_item_classes() {
        assert!(COMMAND_BASE.contains("rounded-xl"));
        assert!(COMMAND_BASE.contains("bg-popover"));
        assert!(COMMAND_BASE.contains("text-popover-foreground"));

        // TS 注释点名的修法：hover 与 data-selected 共用同一背景
        assert!(COMMAND_ITEM.contains("hover:bg-menu-hover"));
        assert!(COMMAND_ITEM.contains("data-selected:bg-menu-hover"));
        assert!(COMMAND_ITEM.contains("data-selected:text-foreground"));
        // 两个 data 变体的语义不同：selected 判存在，disabled 判值
        assert!(COMMAND_ITEM.contains("data-[disabled=true]:opacity-50"));
        assert!(COMMAND_ITEM.contains("group/command-item"));
    }

    #[test]
    fn separator_uses_half_transparent_border() {
        assert!(COMMAND_SEPARATOR.contains("bg-border/50"));
        assert!(!COMMAND_SEPARATOR.contains("bg-border "));
    }

    #[test]
    fn group_targets_the_heading_attribute() {
        // 组上的任意变体必须指向 cmdk-group-heading 属性
        assert!(COMMAND_GROUP.contains("**:[[cmdk-group-heading]]:px-2.5"));
        assert!(COMMAND_GROUP.contains("**:[[cmdk-group-heading]]:font-medium"));
        assert!(COMMAND_GROUP.contains("**:[[cmdk-group-heading]]:text-foreground-subtle"));
        // 标题元素必须真的带这个属性，否则上面那串类名没有落点
        assert_eq!(COMMAND_GROUP_HEADING_ATTR, "cmdk-group-heading");
    }

    #[test]
    fn check_icon_visibility_rules() {
        // 默认隐藏 → 有 shortcut 时仍隐藏 → checked 时才显示
        assert!(COMMAND_ITEM_CHECK.contains("hidden"));
        assert!(COMMAND_ITEM_CHECK.contains(
            "group-has-data-[slot=command-shortcut]/command-item:hidden"
        ));
        assert!(COMMAND_ITEM_CHECK.contains(
            "group-data-[checked=true]/command-item:inline-block"
        ));
    }

    #[test]
    fn shortcut_reads_the_item_selected_state() {
        assert!(COMMAND_SHORTCUT.contains("ml-auto"));
        assert!(COMMAND_SHORTCUT.contains("tracking-widest"));
        assert!(COMMAND_SHORTCUT
            .contains("group-data-selected/command-item:text-foreground"));
    }

    #[test]
    fn input_and_list_classes() {
        assert!(COMMAND_INPUT_WRAPPER.contains("border-b"));
        assert!(COMMAND_INPUT_GROUP.contains("h-8"));
        assert!(COMMAND_INPUT_GROUP.contains("border-0"));
        assert!(COMMAND_INPUT.contains("placeholder:text-foreground-subtlest"));
        assert!(COMMAND_LIST.contains("max-h-72"));
        assert!(COMMAND_LIST.contains("scrollbar-hide"));
        assert!(COMMAND_EMPTY.contains("text-center"));
    }

    #[test]
    fn user_class_wins_on_root() {
        let c = cn(&[COMMAND_BASE, "rounded-none"]);
        assert!(c.contains("rounded-none"));
        assert!(!c.contains("rounded-xl"));
    }
}
