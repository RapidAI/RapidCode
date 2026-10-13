//! `Chart` —— Rust 移植自 `packages/ui/src/components/ui/chart.tsx`
//!
//! shadcn 的图表包装层。它本身**不画图** —— 真正的绘图交给 recharts，
//! 这里负责三件事：
//!
//! 1. **把配置翻译成 CSS 变量**（[`build_chart_style`]）：每个数据系列一个
//!    `--color-<key>`，亮暗两套用 `.dark` 前缀区分；
//! 2. **容器 / 提示框 / 图例的类名与布局**；
//! 3. **从 recharts 的 payload 里反查出配置项**（[`get_payload_config_from_payload`]）。
//!
//! ## 关键移植点 1：`ChartStyle` 生成的 CSS 里有**空行**
//!
//! TS 的生成逻辑是 `colorConfig.map(...).join("\n")`，而 `map` 的回调在没有颜色时
//! 返回 `null` —— `Array.prototype.join` 会把 `null` 变成**空字符串**，于是输出里
//! 会留下一个空行。本移植逐字复刻（见 [`build_chart_style`] 的测试）。
//!
//! ## 关键移植点 2：`theme ?? color` 的短路语义
//!
//! 配置项是「`color` 或 `theme` 二选一」，但判定用的是 `??`（只看 `null`/`undefined`）
//! 与 `? :`（看真假值）混用：
//!
//! - 过滤时是 `config.theme ?? config.color` → **`theme` 存在就用 `theme`**；
//! - 取值时是 `itemConfig.theme?.[theme] ?? itemConfig.color`；
//! - 拼字符串时是 `color ? ... : null` → **空串也算没有颜色**。
//!
//! 所以一个 `{ color: "" }` 的配置项会被过滤掉。Rust 侧必须显式判空串，
//! 不能只判 `is_some()`。
//!
//! ## 与 TS 的差异
//!
//! 1. **不引入 recharts**：`ResponsiveContainer` / `Tooltip` / `Legend` 无对应物。
//!    [`ChartContainer`] 只渲染容器骨架 + `<style>`，图表本体由宿主挂进去。
//! 2. **`ChartTooltip` / `ChartLegend` 是 recharts 的原样转发**，本移植不提供；
//!    只提供它们的**内容组件**（`ChartTooltipContent` / `ChartLegendContent`）。
//! 3. **payload 用显式结构体而不是 `unknown`**：TS 的
//!    `getPayloadConfigFromPayload(config, payload: unknown, key)` 靠 `in` 运算符做
//!    动态探测，Rust 侧改成 [`ChartPayloadObject`]（见该类型的文档）。
//! 4. **`uniqueId` 改为显式 `fallback_id`**：TS 用 `React.useId()`（每实例稳定），
//!    Dioxus 0.6 没有等价原语，所以 `id` 缺省时由调用方传一个稳定字符串。
//! 5. **`toLocaleString()` 按「每三位加逗号」实现**：TS 跟随运行时默认区域，
//!    本移植固定这套分组（`1234.5` → `1,234.5`），与 `en-US` 一致。

use crate::cn::cn;
use dioxus::prelude::*;
use std::collections::BTreeMap;

/// 主题名 → CSS 选择器前缀（`THEMES`）。
///
/// 亮色没有前缀（直接命中 `[data-chart=...]`），暗色靠 `.dark` 祖先。
pub const CHART_THEMES: [(&str, &str); 2] = [("light", ""), ("dark", ".dark")];

/// recharts 挂载前的初始尺寸（`INITIAL_DIMENSION`）。
pub const CHART_INITIAL_DIMENSION: (f64, f64) = (320.0, 200.0);

/// 容器基础类名（TS 原文，逐字保留 —— 那一长串是给 recharts 内部 DOM 打的补丁）。
pub const CHART_CONTAINER_BASE: &str = "flex aspect-video justify-center text-ui-sm [&_.recharts-cartesian-axis-tick_text]:fill-muted-foreground [&_.recharts-cartesian-grid_line[stroke='#ccc']]:stroke-border/50 [&_.recharts-curve.recharts-tooltip-cursor]:stroke-border [&_.recharts-dot[stroke='#fff']]:stroke-transparent [&_.recharts-layer]:outline-hidden [&_.recharts-polar-grid_[stroke='#ccc']]:stroke-border [&_.recharts-radial-bar-background-sector]:fill-muted [&_.recharts-rectangle.recharts-tooltip-cursor]:fill-muted [&_.recharts-reference-line_[stroke='#ccc']]:stroke-border [&_.recharts-sector]:outline-hidden [&_.recharts-sector[stroke='#fff']]:stroke-transparent [&_.recharts-surface]:outline-hidden";

/// 提示框外壳。
pub const CHART_TOOLTIP_CONTENT_BASE: &str = "grid min-w-32 items-start gap-1.5 rounded-lg border border-border/50 bg-background px-2.5 py-1.5 text-ui-sm/relaxed shadow-xl";

/// 提示框里一行（图标 + 名称 + 值）。
pub const CHART_TOOLTIP_ROW_BASE: &str = "flex w-full flex-wrap items-stretch gap-2 [&>svg]:h-2.5 [&>svg]:w-2.5 [&>svg]:text-muted-foreground";

/// 提示框里「名称 + 值」那一层。
pub const CHART_TOOLTIP_VALUE_ROW_BASE: &str = "flex flex-1 justify-between leading-none";

/// 提示框里的名称。
pub const CHART_TOOLTIP_NAME: &str = "text-muted-foreground";

/// 提示框里的数值。
pub const CHART_TOOLTIP_VALUE: &str =
    "font-mono font-medium text-foreground tabular-nums";

/// 提示框标题（标签）。
pub const CHART_TOOLTIP_LABEL: &str = "font-medium";

/// 图例外壳。
pub const CHART_LEGEND_CONTENT_BASE: &str = "flex items-center justify-center gap-4";

/// 图例一项。
pub const CHART_LEGEND_ITEM: &str =
    "flex items-center gap-1.5 [&>svg]:h-3 [&>svg]:w-3 [&>svg]:text-muted-foreground";

/// 图例色块。
pub const CHART_LEGEND_SWATCH: &str = "h-2 w-2 shrink-0 rounded-[2px]";

/// 一个数据系列的配置。
///
/// TS 里是 `{ label?, icon? } & ({ color? } | { theme? })` 的交叉联合；
/// Rust 侧用两个 `Option` 表达并保留 `??` 的优先级（`theme` 优先）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChartConfigItem {
    pub label: Option<String>,
    /// 图标组件名。TS 里是 `React.ComponentType`，本移植只留一个名字给宿主查表。
    pub icon: Option<String>,
    /// 单色（亮暗共用）。
    pub color: Option<String>,
    /// 亮暗两套色（键是 [`CHART_THEMES`] 里的主题名）。
    pub theme: Option<BTreeMap<String, String>>,
}

impl ChartConfigItem {
    pub fn color_only(color: &str) -> Self {
        Self {
            color: Some(color.to_string()),
            ..Self::default()
        }
    }

    pub fn themed(light: &str, dark: &str) -> Self {
        let mut theme = BTreeMap::new();
        theme.insert("light".to_string(), light.to_string());
        theme.insert("dark".to_string(), dark.to_string());
        Self {
            theme: Some(theme),
            ..Self::default()
        }
    }

    /// `config.theme ?? config.color` 是否为真值。
    ///
    /// 注意空串是假值 —— 见模块文档关键移植点 2。
    fn has_any_color(&self) -> bool {
        if self.theme.is_some() {
            return true;
        }
        self.color.as_deref().is_some_and(|c| !c.is_empty())
    }

    /// 某个主题下的颜色（`itemConfig.theme?.[theme] ?? itemConfig.color`）。
    pub fn color_for_theme(&self, theme: &str) -> Option<&str> {
        if let Some(themed) = &self.theme {
            if let Some(color) = themed.get(theme) {
                return Some(color.as_str());
            }
        }
        self.color.as_deref()
    }
}

/// 图表配置（`ChartConfig`）。
pub type ChartConfig = BTreeMap<String, ChartConfigItem>;

/// 图表 id（`chart-${id ?? uniqueId.replace(/:/g, "")}`）。
///
/// `id` 缺省时用 `fallback_id`，其中 `:` 会被去掉（React 的 `useId` 形如 `:r0:`）。
pub fn chart_id(id: Option<&str>, fallback_id: &str) -> String {
    match id {
        Some(id) => format!("chart-{id}"),
        None => format!("chart-{}", fallback_id.replace(':', "")),
    }
}

/// 生成注入用的 `<style>` 内容（TS `ChartStyle`）。
///
/// 返回 `None` 表示没有任何系列配了颜色（TS 里直接 `return null`，连 `<style>` 都不渲染）。
///
/// 输出形如：
/// ```text
///
///  [data-chart=chart-x] {
///   --color-a: #fff;
/// }
///
/// .dark [data-chart=chart-x] {
///   --color-a: #111;
/// }
///
/// ```
/// 首尾都有换行、两个主题块之间也有 —— 这是逐字复刻 TS 的模板字面量结果，
/// 包括 `null` 项 join 出来的空行。
pub fn build_chart_style(id: &str, config: &ChartConfig) -> Option<String> {
    let color_config: Vec<(&String, &ChartConfigItem)> = config
        .iter()
        .filter(|(_, item)| item.has_any_color())
        .collect();

    if color_config.is_empty() {
        return None;
    }

    let blocks: Vec<String> = CHART_THEMES
        .iter()
        .map(|(theme, prefix)| {
            let lines = color_config
                .iter()
                .map(|(key, item)| {
                    // `color ? \`  --color-${key}: ${color};\` : null`
                    item.color_for_theme(theme)
                        .filter(|color| !color.is_empty())
                        .map(|color| format!("  --color-{key}: {color};"))
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!("\n{prefix} [data-chart={id}] {{\n{lines}\n}}\n")
        })
        .collect();

    Some(blocks.join("\n"))
}

/// 一个 payload 对象。
///
/// TS 的 `getPayloadConfigFromPayload` 收的是 `unknown`，靠
/// `"payload" in payload` + `typeof payload[key] === "string"` 动态探测。
/// Rust 侧改成显式结构体，只保留函数真正用到的信息：
///
/// - `fields`：外层键值对，**只关心值是字符串的键**（非字符串一律当作不存在）；
/// - `nested`：`payload.payload` 那一层（同样只关心字符串值）。
///
/// 这样 `key in obj && typeof obj[key] === "string"` 就退化成
/// `fields.get(key)`。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChartPayloadObject {
    pub fields: BTreeMap<String, String>,
    pub nested: Option<BTreeMap<String, String>>,
}

/// 解析出用于查配置的键（TS 里那段 `configLabelKey` 赋值）。
///
/// 顺序：外层字符串值 → 内层字符串值 → 原样返回 `key`。
pub fn resolve_payload_config_key(payload: &ChartPayloadObject, key: &str) -> String {
    if let Some(value) = payload.fields.get(key) {
        return value.clone();
    }
    if let Some(nested) = &payload.nested {
        if let Some(value) = nested.get(key) {
            return value.clone();
        }
    }
    key.to_string()
}

/// 从 payload 反查配置项（TS `getPayloadConfigFromPayload`）。
///
/// `payload` 为 `None` 对应 TS 的「不是对象或为 null」→ 直接 `undefined`。
///
/// 最后一步是 `configLabelKey in config ? config[configLabelKey] : config[key]`
/// —— 先按解析出来的键查，查不到再退回原键。
pub fn get_payload_config_from_payload<'a>(
    config: &'a ChartConfig,
    payload: Option<&ChartPayloadObject>,
    key: &str,
) -> Option<&'a ChartConfigItem> {
    let payload = payload?;
    let config_label_key = resolve_payload_config_key(payload, key);
    config
        .get(&config_label_key)
        .or_else(|| config.get(key))
}

/// 提示框里的指示器形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ChartIndicator {
    #[default]
    Dot,
    Line,
    Dashed,
}

impl ChartIndicator {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChartIndicator::Dot => "dot",
            ChartIndicator::Line => "line",
            ChartIndicator::Dashed => "dashed",
        }
    }
}

/// 一个提示框条目（recharts `payload[i]` 的最小子集）。
///
/// 没有 `Eq`：`ChartValue::Number` 装的是 `f64`。
#[derive(Clone, Debug, PartialEq)]
pub struct ChartTooltipItem {
    pub name: Option<String>,
    pub data_key: Option<String>,
    pub value: Option<ChartValue>,
    /// `item.type`；等于 `"none"` 的条目会被过滤掉。
    pub item_type: Option<String>,
    pub color: Option<String>,
    /// `item.payload`，只用到 `fill`（指示器兜底颜色）。
    pub payload_fill: Option<String>,
}

/// 提示框 / 图例里的值。
#[derive(Clone, Debug, PartialEq)]
pub enum ChartValue {
    Number(f64),
    Text(String),
}

/// 按 JS 的 `Number.prototype.toLocaleString()`（每三位加逗号）格式化。
///
/// 见模块文档差异 5：TS 跟随运行时默认区域，这里固定这套分组。
pub fn format_chart_value(value: &ChartValue) -> String {
    match value {
        ChartValue::Text(text) => text.clone(),
        ChartValue::Number(number) => format_number_with_grouping(*number),
    }
}

fn format_number_with_grouping(number: f64) -> String {
    if !number.is_finite() {
        // JS 的 `NaN.toLocaleString()` → "NaN"，`Infinity` → "∞"
        return if number.is_nan() {
            "NaN".to_string()
        } else if number > 0.0 {
            "∞".to_string()
        } else {
            "-∞".to_string()
        };
    }

    // 先拿 Rust 的默认浮点表示，再给整数部分插逗号
    let raw = format!("{number}");
    let (sign, rest) = match raw.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", raw.as_str()),
    };
    let (integer, fraction) = match rest.split_once('.') {
        Some((integer, fraction)) => (integer, Some(fraction)),
        None => (rest, None),
    };
    // 指数记法（1e21 以上）不做分组，原样返回
    if integer.contains('e') || integer.contains('E') {
        return raw;
    }

    let mut grouped = String::with_capacity(raw.len() + raw.len() / 3);
    grouped.push_str(sign);
    for (index, character) in integer.chars().enumerate() {
        if index > 0 && (integer.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(character);
    }
    if let Some(fraction) = fraction {
        grouped.push('.');
        grouped.push_str(fraction);
    }
    grouped
}

/// `nestLabel`：只有一条数据且指示器不是点时，标签嵌进条目里而不是单独一行。
pub fn nest_label(payload_len: usize, indicator: ChartIndicator) -> bool {
    payload_len == 1 && indicator != ChartIndicator::Dot
}

/// 提示框行的类名。
pub fn chart_tooltip_row_classes(indicator: ChartIndicator) -> String {
    if indicator == ChartIndicator::Dot {
        cn(&[CHART_TOOLTIP_ROW_BASE, "items-center"])
    } else {
        CHART_TOOLTIP_ROW_BASE.to_string()
    }
}

/// 指示器色块的类名（对应 TS 里那组 cva 风格的键值对）。
pub fn chart_tooltip_indicator_classes(
    indicator: ChartIndicator,
    nest_label: bool,
) -> String {
    let mut classes: Vec<&str> = vec!["shrink-0 rounded-[2px] border-(--color-border) bg-(--color-bg)"];
    match indicator {
        ChartIndicator::Dot => classes.push("h-2.5 w-2.5"),
        ChartIndicator::Line => classes.push("w-1"),
        ChartIndicator::Dashed => {
            classes.push("w-0 border-[1.5px] border-dashed bg-transparent");
            // 只有嵌套标签时虚线指示器才需要一点上下留白
            if nest_label {
                classes.push("my-0.5");
            }
        }
    }
    cn(&classes)
}

/// 指示器色块的内联样式（两个 CSS 变量都设成同一个颜色）。
pub fn chart_tooltip_indicator_style(color: Option<&str>) -> Option<String> {
    let color = color?;
    Some(format!("--color-bg: {color}; --color-border: {color};"))
}

/// 提示框里「名称 + 值」那一层的类名。
pub fn chart_tooltip_value_row_classes(nest_label: bool) -> String {
    if nest_label {
        cn(&[CHART_TOOLTIP_VALUE_ROW_BASE, "items-end"])
    } else {
        cn(&[CHART_TOOLTIP_VALUE_ROW_BASE, "items-center"])
    }
}

/// 提示框条目用的键（`nameKey ?? item.name ?? item.dataKey ?? "value"`）。
pub fn chart_tooltip_item_key(name_key: Option<&str>, item: &ChartTooltipItem) -> String {
    name_key
        .map(str::to_string)
        .or_else(|| item.name.clone())
        .or_else(|| item.data_key.clone())
        .unwrap_or_else(|| "value".to_string())
}

/// 图例条目用的键（`nameKey ?? item.dataKey ?? "value"` —— 没有 `name` 这一级）。
pub fn chart_legend_item_key(name_key: Option<&str>, item: &ChartTooltipItem) -> String {
    name_key
        .map(str::to_string)
        .or_else(|| item.data_key.clone())
        .unwrap_or_else(|| "value".to_string())
}

/// 提示框指示器的颜色（`color ?? item.payload?.fill ?? item.color`）。
pub fn chart_indicator_color<'a>(
    color: Option<&'a str>,
    item: &'a ChartTooltipItem,
) -> Option<&'a str> {
    color
        .or(item.payload_fill.as_deref())
        .or(item.color.as_deref())
}

/// 图例外壳类名（`verticalAlign === "top" ? "pb-3" : "pt-3"`）。
pub fn chart_legend_content_classes(vertical_align_top: bool) -> String {
    if vertical_align_top {
        cn(&[CHART_LEGEND_CONTENT_BASE, "pb-3"])
    } else {
        cn(&[CHART_LEGEND_CONTENT_BASE, "pt-3"])
    }
}

pub fn chart_container_classes(class: &str) -> String {
    cn(&[CHART_CONTAINER_BASE, class])
}

/// 图表容器。
#[component]
pub fn ChartContainer(
    /// 图表配置。
    #[props(default)] config: ChartConfig,
    /// 显式 id；缺省时用 `fallback_id`。
    #[props(default)] id: Option<String>,
    /// `id` 缺省时的兜底（对应 `React.useId()`，本移植由调用方给稳定值）。
    #[props(default = "default".to_string(), into)] fallback_id: String,
    #[props(default, into)] class: String,
    children: Element,
) -> Element {
    let resolved_id = chart_id(id.as_deref(), &fallback_id);
    let style = build_chart_style(&resolved_id, &config);

    rsx! {
        div {
            class: chart_container_classes(&class),
            "data-slot": "chart",
            "data-chart": resolved_id.clone(),
            if let Some(style) = style {
                style { dangerous_inner_html: style }
            }
            // recharts 的 `ResponsiveContainer` 无对应物，见模块文档差异 1
            div {
                "data-slot": "chart-body",
                "data-initial-width": CHART_INITIAL_DIMENSION.0.to_string(),
                "data-initial-height": CHART_INITIAL_DIMENSION.1.to_string(),
                {children}
            }
        }
    }
}

/// 提示框内容。
#[component]
pub fn ChartTooltipContent(
    #[props(default)] config: ChartConfig,
    /// 是否处于激活态（recharts 传入）。
    #[props(default)] active: bool,
    /// recharts 的数据条目。
    #[props(default)] payload: Vec<ChartTooltipItem>,
    #[props(default = ChartIndicator::Dot)] indicator: ChartIndicator,
    #[props(default)] hide_label: bool,
    #[props(default)] hide_indicator: bool,
    /// 已经格式化好的标签文本（对应 TS 的 `label` 分支）。
    #[props(default)] label: Option<String>,
    /// 查配置用的键（对应 `labelKey`）。
    #[props(default)] label_key: Option<String>,
    /// 查配置用的键（对应 `nameKey`）。
    #[props(default)] name_key: Option<String>,
    #[props(default, into)] label_class_name: String,
    #[props(default, into)] class: String,
) -> Element {
    if !active || payload.is_empty() {
        return rsx! {};
    }

    let nested = nest_label(payload.len(), indicator);

    // 标签：`!labelKey && typeof label === "string"` → `config[label]?.label ?? label`
    let tooltip_label = if hide_label {
        None
    } else {
        let first = &payload[0];
        let key = label_key
            .clone()
            .or_else(|| first.data_key.clone())
            .or_else(|| first.name.clone())
            .unwrap_or_else(|| "value".to_string());
        let item_config = get_payload_config_from_payload(
            &config,
            Some(&ChartPayloadObject {
                fields: first.data_key.clone().map(|k| (k, key.clone())).into_iter().collect(),
                nested: None,
            }),
            &key,
        );
        if label_key.is_none() {
            label
                .clone()
                .map(|label| config.get(&label).and_then(|c| c.label.clone()).unwrap_or(label))
                .or_else(|| item_config.and_then(|c| c.label.clone()))
        } else {
            item_config.and_then(|c| c.label.clone())
        }
    };

    rsx! {
        div { class: cn(&[CHART_TOOLTIP_CONTENT_BASE, &class]),
            if !nested {
                if let Some(label) = tooltip_label.clone() {
                    div { class: cn(&[CHART_TOOLTIP_LABEL, &label_class_name]), {label} }
                }
            }
            div { class: "grid gap-1.5",
                for (index, item) in payload.iter().enumerate() {
                    if item.item_type.as_deref() != Some("none") {
                        {
                            let key = chart_tooltip_item_key(name_key.as_deref(), item);
                            let item_config = get_payload_config_from_payload(
                                &config,
                                Some(&ChartPayloadObject {
                                    fields: item
                                        .data_key
                                        .clone()
                                        .map(|k| (k, key.clone()))
                                        .into_iter()
                                        .collect(),
                                    nested: None,
                                }),
                                &key,
                            );
                            let indicator_color = chart_indicator_color(None, item);
                            rsx! {
                                div {
                                    key: "{index}",
                                    class: chart_tooltip_row_classes(indicator),
                                    if !hide_indicator {
                                        div {
                                            class: chart_tooltip_indicator_classes(indicator, nested),
                                            style: chart_tooltip_indicator_style(indicator_color),
                                        }
                                    }
                                    div { class: chart_tooltip_value_row_classes(nested),
                                        div { class: "grid gap-1.5",
                                            if nested {
                                                if let Some(label) = tooltip_label.clone() {
                                                    div { class: cn(&[CHART_TOOLTIP_LABEL, &label_class_name]), {label} }
                                                }
                                            }
                                            span { class: CHART_TOOLTIP_NAME,
                                                {item_config
                                                    .and_then(|c| c.label.clone())
                                                    .or_else(|| item.name.clone())
                                                    .unwrap_or_default()}
                                            }
                                        }
                                        if let Some(value) = &item.value {
                                            span { class: CHART_TOOLTIP_VALUE,
                                                {format_chart_value(value)}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// 图例内容。
#[component]
pub fn ChartLegendContent(
    #[props(default)] config: ChartConfig,
    #[props(default)] payload: Vec<ChartTooltipItem>,
    #[props(default)] hide_icon: bool,
    /// 对应 `verticalAlign === "top"`。
    #[props(default)] vertical_align_top: bool,
    #[props(default)] name_key: Option<String>,
    #[props(default, into)] class: String,
) -> Element {
    if payload.is_empty() {
        return rsx! {};
    }

    rsx! {
        div { class: cn(&[chart_legend_content_classes(vertical_align_top).as_str(), &class]),
            for (index, item) in payload.iter().enumerate() {
                if item.item_type.as_deref() != Some("none") {
                    {
                        let key = chart_legend_item_key(name_key.as_deref(), item);
                        let item_config = get_payload_config_from_payload(
                            &config,
                            Some(&ChartPayloadObject {
                                fields: item
                                    .data_key
                                    .clone()
                                    .map(|k| (k, key.clone()))
                                    .into_iter()
                                    .collect(),
                                nested: None,
                            }),
                            &key,
                        );
                        rsx! {
                            div { key: "{index}", class: CHART_LEGEND_ITEM,
                                // TS：有 icon 且没被隐藏 → 渲染 icon，否则渲染色块
                                if !(item_config.and_then(|c| c.icon.as_ref()).is_some() && !hide_icon) {
                                    div {
                                        class: CHART_LEGEND_SWATCH,
                                        style: item.color.as_ref().map(|c| format!("background-color: {c};")),
                                    }
                                }
                                {item_config.and_then(|c| c.label.clone()).unwrap_or_default()}
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(entries: &[(&str, ChartConfigItem)]) -> ChartConfig {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    // ---- id ----

    #[test]
    fn explicit_id_wins() {
        assert_eq!(chart_id(Some("revenue"), "ignored"), "chart-revenue");
    }

    /// React 的 `useId()` 形如 `:r0:`，冒号必须去掉（否则 CSS 选择器非法）。
    #[test]
    fn fallback_id_drops_colons() {
        assert_eq!(chart_id(None, ":r0:"), "chart-r0");
        assert_eq!(chart_id(None, "a:b:c"), "chart-abc");
        assert_eq!(chart_id(None, "plain"), "chart-plain");
    }

    // ---- CSS 变量生成 ----

    #[test]
    fn no_colors_means_no_style_tag() {
        // 只有 label、没有颜色的配置不生成 `<style>`
        let config = config(&[(
            "a",
            ChartConfigItem {
                label: Some("A".to_string()),
                ..ChartConfigItem::default()
            },
        )]);
        assert_eq!(build_chart_style("chart-x", &config), None);
        assert_eq!(build_chart_style("chart-x", &ChartConfig::new()), None);
    }

    #[test]
    fn a_plain_color_goes_into_both_themes() {
        let config = config(&[("desktop", ChartConfigItem::color_only("#fff"))]);
        let style = build_chart_style("chart-x", &config).unwrap();

        // 亮色块（无前缀）
        assert!(style.contains("\n [data-chart=chart-x] {\n  --color-desktop: #fff;\n}\n"));
        // 暗色块（`.dark` 前缀），同一个颜色
        assert!(style.contains("\n.dark [data-chart=chart-x] {\n  --color-desktop: #fff;\n}\n"));
    }

    #[test]
    fn themed_colors_differ_per_theme() {
        let config = config(&[("revenue", ChartConfigItem::themed("#111", "#eee"))]);
        let style = build_chart_style("chart-y", &config).unwrap();
        assert!(style.contains("--color-revenue: #111;"), "{style}");
        assert!(style.contains("--color-revenue: #eee;"), "{style}");
        // 亮色在前、暗色在后
        let light_at = style.find("#111").unwrap();
        let dark_at = style.find("#eee").unwrap();
        assert!(light_at < dark_at);
    }

    /// `theme` 里缺某个主题时回退到 `color`。
    #[test]
    fn a_missing_theme_entry_falls_back_to_the_plain_color() {
        let mut theme = BTreeMap::new();
        theme.insert("dark".to_string(), "#222".to_string());
        let config = config(&[(
            "a",
            ChartConfigItem {
                color: Some("#fallback".to_string()),
                theme: Some(theme),
                ..ChartConfigItem::default()
            },
        )]);
        let style = build_chart_style("chart-x", &config).unwrap();
        // 亮色没有 theme 条目 → 用 color
        assert!(style.contains("--color-a: #fallback;"), "{style}");
        // 暗色有 → 用 theme
        assert!(style.contains("--color-a: #222;"), "{style}");
    }

    /// 空串颜色是假值 → 该配置项被整个过滤掉。
    #[test]
    fn an_empty_color_is_treated_as_absent() {
        let config = config(&[("a", ChartConfigItem::color_only(""))]);
        assert_eq!(build_chart_style("chart-x", &config), None);
    }

    /// 空串颜色**但**有 theme → 按 theme 处理（`??` 短路在 theme 上）。
    #[test]
    fn an_empty_color_still_yields_to_a_present_theme() {
        let mut theme = BTreeMap::new();
        theme.insert("light".to_string(), "#light".to_string());
        theme.insert("dark".to_string(), "#dark".to_string());
        let config = config(&[(
            "a",
            ChartConfigItem {
                color: Some(String::new()),
                theme: Some(theme),
                ..ChartConfigItem::default()
            },
        )]);
        let style = build_chart_style("chart-x", &config).unwrap();
        assert!(style.contains("--color-a: #light;"));
        assert!(style.contains("--color-a: #dark;"));
    }

    /// 某个主题下没颜色时，那一行是**空行**（`null` 被 join 成空串）—— 逐字复刻。
    #[test]
    fn a_theme_without_a_color_leaves_a_blank_line() {
        let mut theme = BTreeMap::new();
        theme.insert("dark".to_string(), "#only-dark".to_string());
        let config = config(&[(
            "a",
            ChartConfigItem {
                theme: Some(theme),
                ..ChartConfigItem::default()
            },
        )]);
        let style = build_chart_style("chart-x", &config).unwrap();

        // 亮色块里 `--color-a` 那一行是空的 → `{\n\n}`
        assert!(style.contains("[data-chart=chart-x] {\n\n}"), "{style:?}");
        assert!(style.contains(".dark [data-chart=chart-x] {\n  --color-a: #only-dark;\n}"));
    }

    /// 多个系列的顺序由 `BTreeMap` 决定（TS 是插入顺序），但每个系列都在。
    #[test]
    fn every_configured_series_gets_a_variable() {
        let config = config(&[
            ("a", ChartConfigItem::color_only("#a")),
            ("b", ChartConfigItem::color_only("#b")),
            ("c", ChartConfigItem::color_only("#c")),
        ]);
        let style = build_chart_style("chart-x", &config).unwrap();
        for key in ["a", "b", "c"] {
            assert!(style.contains(&format!("--color-{key}: #{key};")), "{style}");
        }
        // 两个主题块
        assert_eq!(style.matches("[data-chart=chart-x] {").count(), 2);
    }

    #[test]
    fn the_style_starts_and_ends_with_a_newline() {
        let config = config(&[("a", ChartConfigItem::color_only("#a"))]);
        let style = build_chart_style("chart-x", &config).unwrap();
        assert!(style.starts_with('\n'), "{style:?}");
        assert!(style.ends_with('\n'), "{style:?}");
    }

    // ---- payload → 配置 ----

    #[test]
    fn a_null_payload_yields_nothing() {
        let config = config(&[("value", ChartConfigItem::color_only("#a"))]);
        assert_eq!(
            get_payload_config_from_payload(&config, None, "value"),
            None
        );
    }

    #[test]
    fn the_key_is_used_directly_when_nothing_matches() {
        let config = config(&[("value", ChartConfigItem::color_only("#a"))]);
        let payload = ChartPayloadObject::default();
        let found = get_payload_config_from_payload(&config, Some(&payload), "value");
        assert_eq!(found.unwrap().color.as_deref(), Some("#a"));
    }

    /// 外层的字符串值可以「重定向」到另一个配置键。
    #[test]
    fn an_outer_string_field_redirects_the_lookup() {
        let config = config(&[("real", ChartConfigItem::color_only("#real"))]);
        let mut fields = BTreeMap::new();
        fields.insert("dataKey".to_string(), "real".to_string());
        let payload = ChartPayloadObject {
            fields,
            nested: None,
        };
        let found = get_payload_config_from_payload(&config, Some(&payload), "dataKey");
        assert_eq!(found.unwrap().color.as_deref(), Some("#real"));
    }

    /// 外层没有时看 `payload.payload` 那一层。
    #[test]
    fn the_nested_payload_is_consulted_second() {
        let config = config(&[("nested-key", ChartConfigItem::color_only("#nested"))]);
        let mut nested = BTreeMap::new();
        nested.insert("dataKey".to_string(), "nested-key".to_string());
        let payload = ChartPayloadObject {
            fields: BTreeMap::new(),
            nested: Some(nested),
        };
        let found = get_payload_config_from_payload(&config, Some(&payload), "dataKey");
        assert_eq!(found.unwrap().color.as_deref(), Some("#nested"));
    }

    /// 外层命中就不再看内层。
    #[test]
    fn the_outer_payload_wins_over_the_nested_one() {
        let config = config(&[
            ("outer", ChartConfigItem::color_only("#outer")),
            ("inner", ChartConfigItem::color_only("#inner")),
        ]);
        let mut fields = BTreeMap::new();
        fields.insert("k".to_string(), "outer".to_string());
        let mut nested = BTreeMap::new();
        nested.insert("k".to_string(), "inner".to_string());
        let payload = ChartPayloadObject {
            fields,
            nested: Some(nested),
        };
        let found = get_payload_config_from_payload(&config, Some(&payload), "k");
        assert_eq!(found.unwrap().color.as_deref(), Some("#outer"));
    }

    /// 重定向到的键不存在时，回退用原键查。
    #[test]
    fn a_redirect_to_a_missing_key_falls_back_to_the_original() {
        let config = config(&[("k", ChartConfigItem::color_only("#k"))]);
        let mut fields = BTreeMap::new();
        fields.insert("k".to_string(), "does-not-exist".to_string());
        let payload = ChartPayloadObject {
            fields,
            nested: None,
        };
        let found = get_payload_config_from_payload(&config, Some(&payload), "k");
        assert_eq!(found.unwrap().color.as_deref(), Some("#k"));
    }

    #[test]
    fn resolve_key_prefers_outer_then_nested_then_the_key_itself() {
        let mut nested = BTreeMap::new();
        nested.insert("k".to_string(), "from-nested".to_string());
        let payload = ChartPayloadObject {
            fields: BTreeMap::new(),
            nested: Some(nested.clone()),
        };
        assert_eq!(resolve_payload_config_key(&payload, "k"), "from-nested");
        assert_eq!(resolve_payload_config_key(&payload, "missing"), "missing");

        let mut fields = BTreeMap::new();
        fields.insert("k".to_string(), "from-outer".to_string());
        let payload = ChartPayloadObject {
            fields,
            nested: Some(nested),
        };
        assert_eq!(resolve_payload_config_key(&payload, "k"), "from-outer");
    }

    // ---- 提示框 ----

    #[test]
    fn nest_label_requires_a_single_non_dot_item() {
        assert!(nest_label(1, ChartIndicator::Line));
        assert!(nest_label(1, ChartIndicator::Dashed));
        // 单条 + 点 → 不嵌套
        assert!(!nest_label(1, ChartIndicator::Dot));
        // 多条 → 不嵌套
        assert!(!nest_label(2, ChartIndicator::Line));
        assert!(!nest_label(0, ChartIndicator::Line));
    }

    #[test]
    fn indicator_strings() {
        assert_eq!(ChartIndicator::Dot.as_str(), "dot");
        assert_eq!(ChartIndicator::Line.as_str(), "line");
        assert_eq!(ChartIndicator::Dashed.as_str(), "dashed");
        assert_eq!(ChartIndicator::default(), ChartIndicator::Dot);
    }

    #[test]
    fn indicator_classes_follow_the_shape() {
        let dot = chart_tooltip_indicator_classes(ChartIndicator::Dot, false);
        assert!(dot.contains("h-2.5"));
        assert!(dot.contains("w-2.5"));

        let line = chart_tooltip_indicator_classes(ChartIndicator::Line, false);
        assert!(line.contains("w-1"));
        assert!(!line.contains("h-2.5"));

        let dashed = chart_tooltip_indicator_classes(ChartIndicator::Dashed, false);
        assert!(dashed.contains("w-0"));
        assert!(dashed.contains("border-dashed"));
        assert!(dashed.contains("bg-transparent"));
        // 不嵌套时不加 `my-0.5`
        assert!(!dashed.contains("my-0.5"));

        let dashed_nested = chart_tooltip_indicator_classes(ChartIndicator::Dashed, true);
        assert!(dashed_nested.contains("my-0.5"));
    }

    /// 只有虚线指示器在嵌套时才要留白；点 / 线即使嵌套也不加。
    #[test]
    fn only_the_dashed_indicator_reacts_to_nesting() {
        for indicator in [ChartIndicator::Dot, ChartIndicator::Line] {
            assert_eq!(
                chart_tooltip_indicator_classes(indicator, false),
                chart_tooltip_indicator_classes(indicator, true)
            );
        }
    }

    #[test]
    fn indicator_style_sets_both_variables() {
        let style = chart_tooltip_indicator_style(Some("#f00")).unwrap();
        assert_eq!(style, "--color-bg: #f00; --color-border: #f00;");
        assert_eq!(chart_tooltip_indicator_style(None), None);
    }

    #[test]
    fn row_classes_center_only_for_the_dot() {
        assert!(chart_tooltip_row_classes(ChartIndicator::Dot).contains("items-center"));
        assert!(!chart_tooltip_row_classes(ChartIndicator::Line).contains("items-center"));
        assert!(!chart_tooltip_row_classes(ChartIndicator::Dashed).contains("items-center"));
    }

    #[test]
    fn value_row_alignment_follows_nesting() {
        assert!(chart_tooltip_value_row_classes(false).contains("items-center"));
        assert!(chart_tooltip_value_row_classes(true).contains("items-end"));
        assert!(!chart_tooltip_value_row_classes(true).contains("items-center"));
    }

    #[test]
    fn indicator_color_falls_back_through_the_chain() {
        let item = ChartTooltipItem {
            name: None,
            data_key: None,
            value: None,
            item_type: None,
            color: Some("#item".to_string()),
            payload_fill: Some("#fill".to_string()),
        };
        // 显式 color 优先
        assert_eq!(chart_indicator_color(Some("#explicit"), &item), Some("#explicit"));
        // 再看 item.payload.fill
        assert_eq!(chart_indicator_color(None, &item), Some("#fill"));
        // 最后看 item.color
        let without_fill = ChartTooltipItem {
            payload_fill: None,
            ..item.clone()
        };
        assert_eq!(chart_indicator_color(None, &without_fill), Some("#item"));
        // 都没有
        let bare = ChartTooltipItem {
            payload_fill: None,
            color: None,
            ..item
        };
        assert_eq!(chart_indicator_color(None, &bare), None);
    }

    #[test]
    fn item_keys_use_different_fallback_chains() {
        let item = ChartTooltipItem {
            name: Some("Name".to_string()),
            data_key: Some("dataKey".to_string()),
            value: None,
            item_type: None,
            color: None,
            payload_fill: None,
        };
        // 提示框：nameKey → name → dataKey → "value"
        assert_eq!(chart_tooltip_item_key(Some("k"), &item), "k");
        assert_eq!(chart_tooltip_item_key(None, &item), "Name");
        // 图例：nameKey → dataKey → "value"（没有 name 这一级）
        assert_eq!(chart_legend_item_key(Some("k"), &item), "k");
        assert_eq!(chart_legend_item_key(None, &item), "dataKey");

        let bare = ChartTooltipItem {
            name: None,
            data_key: None,
            ..item
        };
        assert_eq!(chart_tooltip_item_key(None, &bare), "value");
        assert_eq!(chart_legend_item_key(None, &bare), "value");
    }

    // ---- 数值格式化 ----

    #[test]
    fn numbers_get_thousands_separators() {
        assert_eq!(format_chart_value(&ChartValue::Number(1.0)), "1");
        assert_eq!(format_chart_value(&ChartValue::Number(999.0)), "999");
        assert_eq!(format_chart_value(&ChartValue::Number(1_000.0)), "1,000");
        assert_eq!(format_chart_value(&ChartValue::Number(12_345.0)), "12,345");
        assert_eq!(
            format_chart_value(&ChartValue::Number(1_234_567.0)),
            "1,234,567"
        );
    }

    #[test]
    fn decimals_keep_their_fraction_and_group_the_integer_part() {
        assert_eq!(format_chart_value(&ChartValue::Number(1234.5)), "1,234.5");
        assert_eq!(format_chart_value(&ChartValue::Number(-1234.5)), "-1,234.5");
        assert_eq!(format_chart_value(&ChartValue::Number(-1.0)), "-1");
        assert_eq!(format_chart_value(&ChartValue::Number(0.0)), "0");
    }

    #[test]
    fn text_values_pass_through_untouched() {
        assert_eq!(
            format_chart_value(&ChartValue::Text("N/A".to_string())),
            "N/A"
        );
        assert_eq!(
            format_chart_value(&ChartValue::Text("1,234".to_string())),
            "1,234"
        );
    }

    #[test]
    fn non_finite_numbers_mimic_js() {
        assert_eq!(format_chart_value(&ChartValue::Number(f64::NAN)), "NaN");
        assert_eq!(
            format_chart_value(&ChartValue::Number(f64::INFINITY)),
            "∞"
        );
        assert_eq!(
            format_chart_value(&ChartValue::Number(f64::NEG_INFINITY)),
            "-∞"
        );
    }

    // ---- 类名 ----

    #[test]
    fn container_classes_keep_the_recharts_patches() {
        let c = chart_container_classes("");
        assert!(c.contains("aspect-video"));
        assert!(c.contains("text-ui-sm"));
        // 这些选择器是给 recharts 内部 DOM 打的补丁，少一个都会掉样式
        for selector in [
            "[&_.recharts-cartesian-axis-tick_text]:fill-muted-foreground",
            "[&_.recharts-layer]:outline-hidden",
            "[&_.recharts-surface]:outline-hidden",
            "[&_.recharts-radial-bar-background-sector]:fill-muted",
        ] {
            assert!(c.contains(selector), "缺少 {selector}");
        }
        assert!(chart_container_classes("h-64").contains("h-64"));
    }

    #[test]
    fn legend_classes_follow_the_vertical_align() {
        assert!(chart_legend_content_classes(true).contains("pb-3"));
        assert!(!chart_legend_content_classes(true).contains("pt-3"));
        assert!(chart_legend_content_classes(false).contains("pt-3"));
        assert!(!chart_legend_content_classes(false).contains("pb-3"));
        // 共同部分
        for top in [true, false] {
            let c = chart_legend_content_classes(top);
            assert!(c.contains("flex"));
            assert!(c.contains("items-center"));
            assert!(c.contains("justify-center"));
            assert!(c.contains("gap-4"));
        }
    }

    #[test]
    fn components_are_constructible() {
        let _ = ChartContainer as fn(ChartContainerProps) -> Element;
        let _ = ChartTooltipContent as fn(ChartTooltipContentProps) -> Element;
        let _ = ChartLegendContent as fn(ChartLegendContentProps) -> Element;
    }
}
