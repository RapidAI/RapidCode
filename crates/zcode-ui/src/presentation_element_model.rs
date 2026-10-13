//! `presentation_element_model` —— Rust 移植自
//! `packages/ui/src/presentation/presentationElementModel.ts`
//!
//! PPTX 预览的**元素模型**：把 OOXML 的节点树摊平成一张「可命中的矩形表」，
//! 元素选择浮层与命中测试都建立在这张表上。
//!
//! ## 关键移植点 1：表格单元格的坐标要按「比例 × 节点尺寸」换算
//!
//! OOXML 的表格不给绝对坐标，只给列宽数组与行高数组。所以单元格矩形是
//! `节点位置 + (列偏移 / 列宽总和) × 节点宽`。列宽总高都为 0 时整张表
//! **直接放弃**（返回空），否则会除出 `NaN` 污染后续命中测试。
//!
//! ## 关键移植点 2：合并单元格的推进规则不对称
//!
//! 见 TS 里的原注释：`pptx-renderer` 对**纵向** merge continuation 仍然推进
//! `gridSpan`，**横向** continuation 则不推进。直接用物理 `cellIndex` 当列号
//! 会把后续浮层整体左移。所以：
//!
//! | 情况 | 是否推进列号 | 是否产出元素 |
//! |---|---|---|
//! | `vMerge && !hMerge` | 推进 | 不产出 |
//! | `hMerge`（无论有无 `vMerge`） | 不推进 | 不产出 |
//! | 正常单元格 | 推进 `gridSpan` | 产出 |
//!
//! ## 关键移植点 3：组内文本的 `zIndex` 来自「组的序号」
//!
//! `buildTextIndex` 的真实路径是 `slides/{slide}/nodes/{groupId}/children/...`，
//! 但旧调用方给的是首段就是 `groupId`。所以先找 `"nodes"` 段取下一段，
//! 取不到再退回首段。`zIndex` 是该组在 `nodes` 里的下标，找不到时
//! `Math.max(0, -1)` = `0`。
//!
//! ## 与 TS 的差异
//!
//! 1. **`bounds` 用 `width` / `height`**（TS 是 `width` / `height`），
//!    但组内文本入参用的是 `w` / `h`（TS 也是），转换在这里做。
//! 2. **可选字段用 `Option` 而不是 `?`**：`text` / `nodePath` / `columns` /
//!    `rows` 在 TS 里是 `undefined`，Rust 侧用 `Option`。注意 TS 的
//!    `...(node.text ? { text: node.text } : {})` 是**真值判断**，
//!    空串等同于不存在 —— 用 [`is_truthy_text`] 复刻。
//! 3. **`Number(...)` 换成了显式的布尔量**：TS 的
//!    `Number(right.nodeType === "table-cell")` 只为排序做 `0/1` 转换。

/// 元素类型（TS `PresentationElementNodeType`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PresentationElementNodeType {
    Shape,
    Picture,
    Table,
    TableCell,
    Group,
    Chart,
    Unknown,
}

impl PresentationElementNodeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            PresentationElementNodeType::Shape => "shape",
            PresentationElementNodeType::Picture => "picture",
            PresentationElementNodeType::Table => "table",
            PresentationElementNodeType::TableCell => "table-cell",
            PresentationElementNodeType::Group => "group",
            PresentationElementNodeType::Chart => "chart",
            PresentationElementNodeType::Unknown => "unknown",
        }
    }

    /// 是否是「可产出元素」的节点类型（`group` / `unknown` 会被跳过）。
    pub fn is_direct_element(&self) -> bool {
        !matches!(
            self,
            PresentationElementNodeType::Group | PresentationElementNodeType::Unknown
        )
    }
}

/// 一个矩形。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PresentationElementBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// 一个点。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PresentationPoint {
    pub x: f64,
    pub y: f64,
}

/// 尺寸。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PresentationSize {
    pub w: f64,
    pub h: f64,
}

/// 表格单元格入参。
#[derive(Clone, Debug, PartialEq)]
pub struct PresentationElementCellInput {
    pub grid_span: f64,
    pub row_span: f64,
    pub h_merge: bool,
    pub v_merge: bool,
    pub text: Option<String>,
}

/// 表格行入参。
#[derive(Clone, Debug, PartialEq)]
pub struct PresentationElementRowInput {
    pub height: f64,
    pub cells: Vec<PresentationElementCellInput>,
}

/// 节点入参。
#[derive(Clone, Debug, PartialEq)]
pub struct PresentationElementNodeInput {
    pub id: String,
    pub name: String,
    pub node_type: PresentationElementNodeType,
    pub position: PresentationPoint,
    pub size: PresentationSize,
    pub text: Option<String>,
    /// 列宽数组（仅表格有意义）。
    pub columns: Option<Vec<f64>>,
    /// 行数组（仅表格有意义）。
    pub rows: Option<Vec<PresentationElementRowInput>>,
}

/// 组内文本入参（`buildTextIndex` 的产物）。
#[derive(Clone, Debug, PartialEq)]
pub struct PresentationGroupTextEntryInput {
    pub node_id: String,
    pub node_path: String,
    pub text: String,
    /// 注意用的是 `w` / `h`（与 TS 一致）。
    pub bounds: PresentationSize,
    /// 矩形原点。
    pub position: PresentationPoint,
}

/// 摊平后的页面元素。
#[derive(Clone, Debug, PartialEq)]
pub struct PresentationPageElement {
    pub slide_index: usize,
    pub slide_part: String,
    pub node_id: String,
    /// 组内文本才有（直连节点没有）。
    pub node_path: Option<String>,
    pub node_name: String,
    pub node_type: PresentationElementNodeType,
    /// 文本；空串等同于没有。
    pub text: Option<String>,
    pub bounds: PresentationElementBounds,
    /// 表格单元格才有。
    pub row_index: Option<usize>,
    /// 表格单元格才有。
    pub cell_index: Option<usize>,
    pub z_index: i64,
}

/// `Math.max(0, value)` —— **NaN 会传染**（Rust 的 `f64::max` 会忽略 NaN）。
fn non_negative(value: f64) -> f64 {
    if value.is_nan() {
        f64::NAN
    } else {
        0.0_f64.max(value)
    }
}

/// `values.reduce((t, v) => t + Math.max(0, v), 0)`。
fn sum(values: &[f64]) -> f64 {
    values.iter().map(|v| non_negative(*v)).sum()
}

/// `node.text ? { text: node.text } : {}` 的真值判断。
fn is_truthy_text(text: &Option<String>) -> Option<String> {
    text.as_ref()
        .filter(|value| !value.is_empty())
        .map(|value| value.clone())
}

/// `Math.max(1, cell.gridSpan || 1)` —— JS 里 `0` / `NaN` 都是 falsy。
fn positive_span(span: f64) -> f64 {
    let fallback = if span == 0.0 || span.is_nan() { 1.0 } else { span };
    1.0_f64.max(fallback)
}

fn to_bounds(node: &PresentationElementNodeInput) -> PresentationElementBounds {
    PresentationElementBounds {
        x: node.position.x,
        y: node.position.y,
        width: node.size.w,
        height: node.size.h,
    }
}

/// 表格节点 → 单元格元素。
///
/// 列宽 / 行高总和有一边不为正就返回空（见模块移植点 1）。
fn build_table_cell_elements(
    node: &PresentationElementNodeInput,
    base: &PresentationPageElement,
) -> Vec<PresentationPageElement> {
    let (Some(columns), Some(rows)) = (node.columns.as_ref(), node.rows.as_ref()) else {
        return Vec::new();
    };
    if columns.is_empty() || rows.is_empty() {
        return Vec::new();
    }
    let column_total = sum(columns);
    let row_total = sum(&rows.iter().map(|row| row.height).collect::<Vec<_>>());
    if column_total <= 0.0 || row_total <= 0.0 {
        return Vec::new();
    }

    let column_offsets: Vec<f64> = (0..columns.len())
        .map(|index| sum(&columns[..index]))
        .collect();
    let row_heights: Vec<f64> = rows.iter().map(|row| row.height).collect();
    let row_offsets: Vec<f64> = (0..rows.len())
        .map(|index| sum(&row_heights[..index]))
        .collect();

    let mut elements = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        // 列号在行内连续推进（不按物理 cellIndex 取）
        let mut column_index = 0.0_f64;
        for (cell_index, cell) in row.cells.iter().enumerate() {
            let grid_span = positive_span(cell.grid_span);
            if cell.h_merge || cell.v_merge {
                // 见模块移植点 2：只有「纵向合并的续格」才推进列号
                if cell.v_merge && !cell.h_merge {
                    column_index += grid_span;
                }
                continue;
            }
            let start_column_index = column_index;
            column_index += grid_span;
            let row_span = positive_span(cell.row_span);

            let start = start_column_index.max(0.0) as usize;
            let end = ((start_column_index + grid_span) as usize).min(columns.len());
            let cell_width = if start >= columns.len() {
                0.0
            } else {
                sum(&columns[start..end.max(start)])
            };
            let row_end = (row_index + row_span.max(1.0) as usize).min(rows.len());
            let cell_height = sum(&row_heights[row_index.min(rows.len())..row_end.max(row_index)]);

            elements.push(PresentationPageElement {
                node_type: PresentationElementNodeType::TableCell,
                row_index: Some(row_index),
                cell_index: Some(cell_index),
                text: cell.text.clone(),
                bounds: PresentationElementBounds {
                    x: node.position.x
                        + (column_offsets.get(start).copied().unwrap_or(0.0) / column_total)
                            * node.size.w,
                    y: node.position.y
                        + (row_offsets.get(row_index).copied().unwrap_or(0.0) / row_total)
                            * node.size.h,
                    width: (cell_width / column_total) * node.size.w,
                    height: (cell_height / row_total) * node.size.h,
                },
                ..base.clone()
            });
        }
    }
    elements
}

/// `buildPresentationPageElements`。
///
/// 顺序：先直连节点（按 `nodes` 顺序，表格展开成 `[表格, 单元格…]`），
/// 再组内文本（按 `groupTextEntries` 顺序）。
pub fn build_presentation_page_elements(
    slide_index: usize,
    slide_part: &str,
    nodes: &[PresentationElementNodeInput],
    group_text_entries: &[PresentationGroupTextEntryInput],
) -> Vec<PresentationPageElement> {
    let mut direct_elements: Vec<PresentationPageElement> = Vec::new();
    for (z_index, node) in nodes.iter().enumerate() {
        if !node.node_type.is_direct_element() {
            continue;
        }
        let base = PresentationPageElement {
            slide_index,
            slide_part: slide_part.to_string(),
            node_id: node.id.clone(),
            node_path: None,
            node_name: node.name.clone(),
            node_type: node.node_type,
            text: is_truthy_text(&node.text),
            bounds: to_bounds(node),
            row_index: None,
            cell_index: None,
            z_index: z_index as i64,
        };
        if node.node_type == PresentationElementNodeType::Table {
            let cells = build_table_cell_elements(node, &base);
            direct_elements.push(base);
            direct_elements.extend(cells);
        } else {
            direct_elements.push(base);
        }
    }

    // 直连 id 集合：**包含** unknown，但**不含** group
    let direct_ids: Vec<&str> = nodes
        .iter()
        .filter(|node| node.node_type != PresentationElementNodeType::Group)
        .map(|node| node.id.as_str())
        .collect();

    let mut group_text_elements: Vec<PresentationPageElement> = Vec::new();
    for entry in group_text_entries {
        if direct_ids.contains(&entry.node_id.as_str()) {
            continue;
        }
        let path_segments: Vec<&str> = entry.node_path.split('/').collect();
        let nodes_segment_index = path_segments.iter().position(|segment| *segment == "nodes");
        let group_id = match nodes_segment_index {
            // `pathSegments[i + 1] ?? pathSegments[0]`：越界才回退（空串不回退）
            Some(index) if index + 1 < path_segments.len() => path_segments[index + 1],
            _ => path_segments.first().copied().unwrap_or(""),
        };
        let found = nodes
            .iter()
            .position(|node| {
                node.node_type == PresentationElementNodeType::Group && node.id == group_id
            })
            .map(|index| index as i64)
            .unwrap_or(-1);
        group_text_elements.push(PresentationPageElement {
            slide_index,
            slide_part: slide_part.to_string(),
            node_id: entry.node_id.clone(),
            node_path: Some(entry.node_path.clone()),
            node_name: String::new(),
            node_type: PresentationElementNodeType::Shape,
            text: Some(entry.text.clone()),
            bounds: PresentationElementBounds {
                x: entry.position.x,
                y: entry.position.y,
                width: entry.bounds.w,
                height: entry.bounds.h,
            },
            row_index: None,
            cell_index: None,
            z_index: 0_i64.max(found),
        });
    }

    direct_elements.extend(group_text_elements);
    direct_elements
}

/// 点是否落在矩形内（**闭区间**：边界上的点算命中）。
pub fn contains_point(bounds: &PresentationElementBounds, point: PresentationPoint) -> bool {
    point.x >= bounds.x
        && point.y >= bounds.y
        && point.x <= bounds.x + bounds.width
        && point.y <= bounds.y + bounds.height
}

/// 命中测试：取最上面（`zIndex` 最大）的那个元素，单元格优先。
///
/// 排序键 `(是否单元格, zIndex)` 降序；`sort` 是稳定排序，同键保持原顺序。
pub fn hit_test_presentation_element(
    elements: &[PresentationPageElement],
    point: PresentationPoint,
) -> Option<PresentationPageElement> {
    let mut matches: Vec<&PresentationPageElement> = elements
        .iter()
        .filter(|element| contains_point(&element.bounds, point))
        .collect();
    matches.sort_by(|left, right| {
        let left_cell = left.node_type == PresentationElementNodeType::TableCell;
        let right_cell = right.node_type == PresentationElementNodeType::TableCell;
        // `cellPriority || right.zIndex - left.zIndex`
        match (right_cell as i64).cmp(&(left_cell as i64)) {
            std::cmp::Ordering::Equal => right.z_index.cmp(&left.z_index),
            other => other,
        }
    });
    matches.first().map(|element| (*element).clone())
}

/// `isSamePresentationElement`：四个字段全等才算同一个元素。
pub fn is_same_presentation_element(
    left: Option<&PresentationPageElement>,
    right: Option<&PresentationPageElement>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            left.node_id == right.node_id
                && left.node_type == right.node_type
                && left.row_index == right.row_index
                && left.cell_index == right.cell_index
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, node_type: PresentationElementNodeType) -> PresentationElementNodeInput {
        PresentationElementNodeInput {
            id: id.to_string(),
            name: format!("name-{id}"),
            node_type,
            position: PresentationPoint { x: 10.0, y: 20.0 },
            size: PresentationSize { w: 100.0, h: 50.0 },
            text: None,
            columns: None,
            rows: None,
        }
    }

    fn cell(grid_span: f64, row_span: f64, h_merge: bool, v_merge: bool) -> PresentationElementCellInput {
        PresentationElementCellInput {
            grid_span,
            row_span,
            h_merge,
            v_merge,
            text: None,
        }
    }

    // ---- 基础摊平 ----

    #[test]
    fn groups_and_unknowns_are_skipped_but_still_take_a_z_index() {
        let nodes = vec![
            node("g1", PresentationElementNodeType::Group),
            node("s1", PresentationElementNodeType::Shape),
            node("u1", PresentationElementNodeType::Unknown),
            node("p1", PresentationElementNodeType::Picture),
        ];
        let elements = build_presentation_page_elements(0, "slide1.xml", &nodes, &[]);
        assert_eq!(elements.len(), 2);
        assert_eq!(elements[0].node_id, "s1");
        // zIndex 用的是 nodes 里的下标（group 占了 0）
        assert_eq!(elements[0].z_index, 1);
        assert_eq!(elements[1].node_id, "p1");
        assert_eq!(elements[1].z_index, 3);
    }

    #[test]
    fn bounds_come_straight_from_position_and_size() {
        let elements =
            build_presentation_page_elements(2, "slide2.xml", &[node("s1", PresentationElementNodeType::Shape)], &[]);
        let element = &elements[0];
        assert_eq!(element.slide_index, 2);
        assert_eq!(element.slide_part, "slide2.xml");
        assert_eq!(element.node_name, "name-s1");
        assert_eq!(
            element.bounds,
            PresentationElementBounds {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 50.0
            }
        );
        assert_eq!(element.node_path, None);
        assert_eq!(element.text, None);
    }

    /// `...(node.text ? { text } : {})`：空串等同于不存在。
    #[test]
    fn empty_text_is_treated_as_absent() {
        let mut with_text = node("s1", PresentationElementNodeType::Shape);
        with_text.text = Some("hello".to_string());
        let mut with_empty = node("s2", PresentationElementNodeType::Shape);
        with_empty.text = Some(String::new());

        let elements = build_presentation_page_elements(0, "p", &[with_text, with_empty], &[]);
        assert_eq!(elements[0].text, Some("hello".to_string()));
        assert_eq!(elements[1].text, None);
    }

    // ---- 表格单元格 ----

    fn table_node() -> PresentationElementNodeInput {
        let mut table = node("t1", PresentationElementNodeType::Table);
        table.position = PresentationPoint { x: 0.0, y: 0.0 };
        table.size = PresentationSize { w: 200.0, h: 100.0 };
        // 两列 1:1、两行 1:1
        table.columns = Some(vec![50.0, 50.0]);
        table.rows = Some(vec![
            PresentationElementRowInput {
                height: 25.0,
                cells: vec![cell(1.0, 1.0, false, false), cell(1.0, 1.0, false, false)],
            },
            PresentationElementRowInput {
                height: 25.0,
                cells: vec![cell(1.0, 1.0, false, false), cell(1.0, 1.0, false, false)],
            },
        ]);
        table
    }

    #[test]
    fn a_table_expands_into_cells_after_itself() {
        let elements = build_presentation_page_elements(0, "p", &[table_node()], &[]);
        // 1 个表格 + 4 个单元格
        assert_eq!(elements.len(), 5);
        assert_eq!(elements[0].node_type, PresentationElementNodeType::Table);
        assert_eq!(elements[1].node_type, PresentationElementNodeType::TableCell);
        assert_eq!(elements[1].row_index, Some(0));
        assert_eq!(elements[1].cell_index, Some(0));
        assert_eq!(elements[4].row_index, Some(1));
        assert_eq!(elements[4].cell_index, Some(1));
        // 单元格继承表格的身份字段
        assert_eq!(elements[1].node_id, "t1");
        assert_eq!(elements[1].z_index, 0);
    }

    #[test]
    fn cell_rects_are_ratios_of_the_table_box() {
        let elements = build_presentation_page_elements(0, "p", &[table_node()], &[]);
        // 第一格：x=0、y=0、宽 = 50/100*200 = 100、高 = 25/50*100 = 50
        let first = &elements[1].bounds;
        assert_eq!((first.x, first.y), (0.0, 0.0));
        assert_eq!((first.width, first.height), (100.0, 50.0));
        // 第二格：x = 50/100*200 = 100
        let second = &elements[2].bounds;
        assert_eq!((second.x, second.y), (100.0, 0.0));
        assert_eq!((second.width, second.height), (100.0, 50.0));
        // 第二行：y = 25/50*100 = 50
        let third = &elements[3].bounds;
        assert_eq!((third.x, third.y), (0.0, 50.0));
    }

    #[test]
    fn a_table_without_columns_or_rows_yields_no_cells() {
        let mut no_columns = table_node();
        no_columns.columns = None;
        assert_eq!(
            build_presentation_page_elements(0, "p", &[no_columns], &[]).len(),
            1
        );

        let mut empty_rows = table_node();
        empty_rows.rows = Some(Vec::new());
        assert_eq!(
            build_presentation_page_elements(0, "p", &[empty_rows], &[]).len(),
            1
        );
    }

    /// 总和不为正 → 整张表放弃，避免除出 NaN。
    #[test]
    fn a_degenerate_table_is_dropped() {
        let mut zero_columns = table_node();
        zero_columns.columns = Some(vec![0.0, 0.0]);
        assert_eq!(
            build_presentation_page_elements(0, "p", &[zero_columns], &[]).len(),
            1
        );

        let mut zero_rows = table_node();
        zero_rows.rows = Some(vec![PresentationElementRowInput {
            height: 0.0,
            cells: vec![cell(1.0, 1.0, false, false)],
        }]);
        assert_eq!(
            build_presentation_page_elements(0, "p", &[zero_rows], &[]).len(),
            1
        );
    }

    /// 负的列宽在 `sum` 里被夹到 0。
    #[test]
    fn negative_column_widths_count_as_zero() {
        let mut table = table_node();
        table.columns = Some(vec![-50.0, 50.0]);
        let elements = build_presentation_page_elements(0, "p", &[table], &[]);
        // 总宽仍是 50（负数被夹到 0）
        assert_eq!(elements.len(), 5);
        // 第一格宽 = 0/50*200 = 0
        assert_eq!(elements[1].bounds.width, 0.0);
        // 第二格 x = 0/50*200 = 0、宽 = 50/50*200 = 200
        assert_eq!(elements[2].bounds.x, 0.0);
        assert_eq!(elements[2].bounds.width, 200.0);
    }

    /// 移植点 2：纵向续格推进列号，横向续格不推进。
    #[test]
    fn vertical_merge_continuations_advance_the_column_index() {
        let mut table = node("t1", PresentationElementNodeType::Table);
        table.position = PresentationPoint { x: 0.0, y: 0.0 };
        table.size = PresentationSize { w: 300.0, h: 100.0 };
        table.columns = Some(vec![100.0, 100.0, 100.0]);
        table.rows = Some(vec![PresentationElementRowInput {
            height: 100.0,
            cells: vec![
                cell(1.0, 1.0, false, false),
                // 纵向续格：占用第 2 列，必须推进
                cell(1.0, 1.0, false, true),
                cell(1.0, 1.0, false, false),
            ],
        }]);
        let elements = build_presentation_page_elements(0, "p", &[table], &[]);
        // 表格 + 2 个产出的单元格
        assert_eq!(elements.len(), 3);
        assert_eq!(elements[1].cell_index, Some(0));
        // 第 3 个物理单元格落到第 3 列（x = 200/300*300 = 200）
        assert_eq!(elements[2].cell_index, Some(2));
        assert_eq!(elements[2].bounds.x, 200.0);
    }

    #[test]
    fn horizontal_merge_continuations_do_not_advance_the_column_index() {
        let mut table = node("t1", PresentationElementNodeType::Table);
        table.position = PresentationPoint { x: 0.0, y: 0.0 };
        table.size = PresentationSize { w: 300.0, h: 100.0 };
        table.columns = Some(vec![100.0, 100.0, 100.0]);
        table.rows = Some(vec![PresentationElementRowInput {
            height: 100.0,
            cells: vec![
                cell(1.0, 1.0, false, false),
                // 横向续格：不推进
                cell(1.0, 1.0, true, false),
                cell(1.0, 1.0, false, false),
            ],
        }]);
        let elements = build_presentation_page_elements(0, "p", &[table], &[]);
        assert_eq!(elements.len(), 3);
        // 第 3 个物理单元格仍落在第 2 列（x = 100/300*300 = 100）
        assert_eq!(elements[2].bounds.x, 100.0);
    }

    #[test]
    fn a_horizontal_merge_wins_over_a_vertical_one() {
        let mut table = node("t1", PresentationElementNodeType::Table);
        table.position = PresentationPoint { x: 0.0, y: 0.0 };
        table.size = PresentationSize { w: 200.0, h: 100.0 };
        table.columns = Some(vec![100.0, 100.0]);
        table.rows = Some(vec![PresentationElementRowInput {
            height: 100.0,
            cells: vec![cell(1.0, 1.0, true, true), cell(1.0, 1.0, false, false)],
        }]);
        let elements = build_presentation_page_elements(0, "p", &[table], &[]);
        // hMerge 生效 → 不推进；第 2 格落到第 1 列
        assert_eq!(elements[1].bounds.x, 0.0);
    }

    #[test]
    fn grid_span_widens_the_cell() {
        let mut table = node("t1", PresentationElementNodeType::Table);
        table.position = PresentationPoint { x: 0.0, y: 0.0 };
        table.size = PresentationSize { w: 200.0, h: 100.0 };
        table.columns = Some(vec![50.0, 50.0]);
        table.rows = Some(vec![PresentationElementRowInput {
            height: 100.0,
            cells: vec![cell(2.0, 1.0, false, false)],
        }]);
        let elements = build_presentation_page_elements(0, "p", &[table], &[]);
        assert_eq!(elements.len(), 2);
        assert_eq!(elements[1].bounds.width, 200.0);
    }

    #[test]
    fn row_span_tallens_the_cell() {
        let mut table = node("t1", PresentationElementNodeType::Table);
        table.position = PresentationPoint { x: 0.0, y: 0.0 };
        table.size = PresentationSize { w: 100.0, h: 200.0 };
        table.columns = Some(vec![100.0]);
        table.rows = Some(vec![
            PresentationElementRowInput {
                height: 50.0,
                cells: vec![cell(1.0, 2.0, false, false)],
            },
            PresentationElementRowInput {
                height: 50.0,
                cells: vec![],
            },
        ]);
        let elements = build_presentation_page_elements(0, "p", &[table], &[]);
        assert_eq!(elements.len(), 2);
        // 高 = (50+50)/100 * 200 = 200
        assert_eq!(elements[1].bounds.height, 200.0);
    }

    /// `cell.gridSpan || 1`：0 和 NaN 都退回 1。
    #[test]
    fn zero_and_nan_spans_fall_back_to_one() {
        assert_eq!(positive_span(0.0), 1.0);
        assert_eq!(positive_span(f64::NAN), 1.0);
        assert_eq!(positive_span(3.0), 3.0);
        assert_eq!(positive_span(0.5), 1.0);
    }

    #[test]
    fn cell_text_is_carried_through() {
        let mut table = table_node();
        if let Some(rows) = table.rows.as_mut() {
            rows[0].cells[0].text = Some("A1".to_string());
        }
        let elements = build_presentation_page_elements(0, "p", &[table], &[]);
        assert_eq!(elements[1].text, Some("A1".to_string()));
    }

    // ---- 组内文本 ----

    fn entry(node_id: &str, node_path: &str, text: &str) -> PresentationGroupTextEntryInput {
        PresentationGroupTextEntryInput {
            node_id: node_id.to_string(),
            node_path: node_path.to_string(),
            text: text.to_string(),
            bounds: PresentationSize { w: 30.0, h: 40.0 },
            position: PresentationPoint { x: 5.0, y: 6.0 },
        }
    }

    #[test]
    fn group_text_is_appended_after_the_direct_elements() {
        let nodes = vec![
            node("g1", PresentationElementNodeType::Group),
            node("s1", PresentationElementNodeType::Shape),
        ];
        let elements = build_presentation_page_elements(
            0,
            "p",
            &nodes,
            &[entry("g1-text", "slides/1/nodes/g1/children/2", "标题")],
        );
        assert_eq!(elements.len(), 2);
        assert_eq!(elements[0].node_id, "s1");
        let group_text = &elements[1];
        assert_eq!(group_text.node_id, "g1-text");
        assert_eq!(group_text.node_type, PresentationElementNodeType::Shape);
        assert_eq!(group_text.text, Some("标题".to_string()));
        assert_eq!(group_text.node_name, "");
        // zIndex 来自组的序号（g1 在 nodes 里是 0）
        assert_eq!(group_text.z_index, 0);
        assert_eq!(
            group_text.bounds,
            PresentationElementBounds {
                x: 5.0,
                y: 6.0,
                width: 30.0,
                height: 40.0
            }
        );
    }

    /// 直连 id 集合包含 `unknown`，但不含 `group`。
    #[test]
    fn group_text_is_dropped_when_the_node_is_already_direct() {
        let nodes = vec![node("s1", PresentationElementNodeType::Shape)];
        let elements =
            build_presentation_page_elements(0, "p", &nodes, &[entry("s1", "slides/1/nodes/g1", "x")]);
        assert_eq!(elements.len(), 1);
    }

    #[test]
    fn group_text_z_index_uses_the_group_position() {
        let nodes = vec![
            node("a", PresentationElementNodeType::Shape),
            node("g1", PresentationElementNodeType::Group),
            node("g2", PresentationElementNodeType::Group),
        ];
        let elements = build_presentation_page_elements(
            0,
            "p",
            &nodes,
            &[
                entry("t1", "slides/1/nodes/g1/children/1", "一"),
                entry("t2", "slides/1/nodes/g2/children/1", "二"),
            ],
        );
        assert_eq!(elements[1].z_index, 1);
        assert_eq!(elements[2].z_index, 2);
    }

    /// 找不到组 → `Math.max(0, -1)` = 0。
    #[test]
    fn an_unknown_group_falls_back_to_z_index_zero() {
        let elements = build_presentation_page_elements(
            0,
            "p",
            &[node("s1", PresentationElementNodeType::Shape)],
            &[entry("t1", "slides/1/nodes/missing/children/1", "x")],
        );
        assert_eq!(elements[1].z_index, 0);
    }

    /// 兼容旧调用方：路径首段直接是 groupId。
    #[test]
    fn a_legacy_path_uses_its_first_segment_as_the_group_id() {
        let nodes = vec![
            node("s1", PresentationElementNodeType::Shape),
            node("g9", PresentationElementNodeType::Group),
        ];
        let elements =
            build_presentation_page_elements(0, "p", &nodes, &[entry("t1", "g9/children/1", "x")]);
        // 没有 "nodes" 段 → 用首段 "g9" → 组下标 1
        assert_eq!(elements[1].z_index, 1);
    }

    // ---- 命中测试 ----

    fn element(id: &str, z_index: i64, node_type: PresentationElementNodeType) -> PresentationPageElement {
        PresentationPageElement {
            slide_index: 0,
            slide_part: "p".to_string(),
            node_id: id.to_string(),
            node_path: None,
            node_name: String::new(),
            node_type,
            text: None,
            bounds: PresentationElementBounds {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            row_index: None,
            cell_index: None,
            z_index,
        }
    }

    #[test]
    fn contains_point_is_inclusive() {
        let bounds = PresentationElementBounds {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        assert!(contains_point(&bounds, PresentationPoint { x: 0.0, y: 0.0 }));
        assert!(contains_point(&bounds, PresentationPoint { x: 10.0, y: 10.0 }));
        assert!(contains_point(&bounds, PresentationPoint { x: 5.0, y: 5.0 }));
        assert!(!contains_point(&bounds, PresentationPoint { x: 10.1, y: 5.0 }));
        assert!(!contains_point(&bounds, PresentationPoint { x: -0.1, y: 5.0 }));
    }

    #[test]
    fn hit_test_picks_the_highest_z_index() {
        let elements = vec![
            element("low", 1, PresentationElementNodeType::Shape),
            element("high", 5, PresentationElementNodeType::Shape),
            element("mid", 3, PresentationElementNodeType::Shape),
        ];
        let hit = hit_test_presentation_element(&elements, PresentationPoint { x: 5.0, y: 5.0 }).unwrap();
        assert_eq!(hit.node_id, "high");
    }

    /// 单元格优先于同层（甚至更高层）的普通元素。
    #[test]
    fn table_cells_win_over_other_elements() {
        let elements = vec![
            element("shape", 9, PresentationElementNodeType::Shape),
            element("cell", 0, PresentationElementNodeType::TableCell),
        ];
        let hit = hit_test_presentation_element(&elements, PresentationPoint { x: 5.0, y: 5.0 }).unwrap();
        assert_eq!(hit.node_id, "cell");
    }

    #[test]
    fn hit_test_returns_none_when_nothing_matches() {
        let elements = vec![element("a", 0, PresentationElementNodeType::Shape)];
        assert_eq!(
            hit_test_presentation_element(&elements, PresentationPoint { x: 50.0, y: 50.0 }),
            None
        );
        assert_eq!(
            hit_test_presentation_element(&[], PresentationPoint { x: 0.0, y: 0.0 }),
            None
        );
    }

    /// 同键时保持原顺序（`Array.prototype.sort` 自 ES2019 起稳定）。
    #[test]
    fn hit_test_is_stable_for_equal_keys() {
        let elements = vec![
            element("first", 2, PresentationElementNodeType::Shape),
            element("second", 2, PresentationElementNodeType::Shape),
        ];
        let hit = hit_test_presentation_element(&elements, PresentationPoint { x: 5.0, y: 5.0 }).unwrap();
        assert_eq!(hit.node_id, "first");
    }

    // ---- isSamePresentationElement ----

    #[test]
    fn element_identity_needs_all_four_fields() {
        let mut a = element("n", 0, PresentationElementNodeType::TableCell);
        a.row_index = Some(1);
        a.cell_index = Some(2);
        let mut b = a.clone();
        assert!(is_same_presentation_element(Some(&a), Some(&b)));

        b.cell_index = Some(3);
        assert!(!is_same_presentation_element(Some(&a), Some(&b)));

        b = a.clone();
        b.row_index = Some(9);
        assert!(!is_same_presentation_element(Some(&a), Some(&b)));

        b = a.clone();
        b.node_type = PresentationElementNodeType::Shape;
        assert!(!is_same_presentation_element(Some(&a), Some(&b)));

        b = a.clone();
        b.node_id = "other".to_string();
        assert!(!is_same_presentation_element(Some(&a), Some(&b)));
    }

    #[test]
    fn element_identity_is_false_when_either_side_is_missing() {
        let a = element("n", 0, PresentationElementNodeType::Shape);
        assert!(!is_same_presentation_element(None, Some(&a)));
        assert!(!is_same_presentation_element(Some(&a), None));
        assert!(!is_same_presentation_element(None, None));
    }
}
