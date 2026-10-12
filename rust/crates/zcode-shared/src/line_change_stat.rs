//! 行变更统计 (Line Change Stat)
//!
//! Rust 移植自 `packages/shared/src/lineChangeStat.ts`。
//!
//! UI 的 edit 卡片与任务摘要都需要「真实改动行数」，不能把 before/after
//! 总行数直接当成 +/-。这里统一做一次行级 LCS 统计，再由各端复用同一份结果。
//!
//! 超大文件如果强行算完整 LCS 会明显卡顿，所以超过阈值时退回到保守估算。

/// LCS 单元格数上限（超过则退化为保守估算）。
const MAX_LCS_CELLS: usize = 400_000;

/// 行变更统计结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineChangeStat {
    pub added: usize,
    pub removed: usize,
}

/// 按 `\n` 切分为逻辑行，并丢弃末尾的空行。
fn split_into_logical_lines(content: Option<&str>) -> Vec<&str> {
    let Some(content) = content else {
        return Vec::new();
    };
    if content.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<&str> = content.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
}

/// 计算 before/after 之间的真实新增/删除行数。
pub fn compute_line_change_stat(before_content: Option<&str>, after_content: &str) -> LineChangeStat {
    let before_lines = split_into_logical_lines(before_content);
    let after_lines = split_into_logical_lines(Some(after_content));

    // 公共前缀
    let mut prefix_index = 0usize;
    while prefix_index < before_lines.len()
        && prefix_index < after_lines.len()
        && before_lines[prefix_index] == after_lines[prefix_index]
    {
        prefix_index += 1;
    }

    // 公共后缀
    let mut before_tail_index = before_lines.len() as isize - 1;
    let mut after_tail_index = after_lines.len() as isize - 1;
    while before_tail_index >= prefix_index as isize
        && after_tail_index >= prefix_index as isize
        && before_lines[before_tail_index as usize] == after_lines[after_tail_index as usize]
    {
        before_tail_index -= 1;
        after_tail_index -= 1;
    }

    let trimmed_before = &before_lines[prefix_index..(before_tail_index + 1) as usize];
    let trimmed_after = &after_lines[prefix_index..(after_tail_index + 1) as usize];

    if trimmed_before.is_empty() {
        return LineChangeStat {
            added: trimmed_after.len(),
            removed: 0,
        };
    }
    if trimmed_after.is_empty() {
        return LineChangeStat {
            added: 0,
            removed: trimmed_before.len(),
        };
    }

    // 超大文件：保守估算，优先保证交互流畅。
    if trimmed_before.len() * trimmed_after.len() > MAX_LCS_CELLS {
        return LineChangeStat {
            added: trimmed_after.len(),
            removed: trimmed_before.len(),
        };
    }

    let unchanged_line_count = lcs_length(trimmed_before, trimmed_after);
    LineChangeStat {
        added: trimmed_after.len() - unchanged_line_count,
        removed: trimmed_before.len() - unchanged_line_count,
    }
}

/// 行级 LCS 长度（滚动数组，O(n*m) 时间、O(m) 空间）。
fn lcs_length(before: &[&str], after: &[&str]) -> usize {
    let mut lcs = vec![0usize; after.len() + 1];
    for before_index in 1..=before.len() {
        let mut previous_diagonal = 0usize;
        for after_index in 1..=after.len() {
            let previous_row = lcs[after_index];
            if before[before_index - 1] == after[after_index - 1] {
                lcs[after_index] = previous_diagonal + 1;
            } else {
                lcs[after_index] = lcs[after_index].max(lcs[after_index - 1]);
            }
            previous_diagonal = previous_row;
        }
    }
    lcs[after.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_addition() {
        let stat = compute_line_change_stat(Some("a\nb\n"), "a\nb\nc\n");
        assert_eq!(stat, LineChangeStat { added: 1, removed: 0 });
    }

    #[test]
    fn pure_removal() {
        let stat = compute_line_change_stat(Some("a\nb\nc\n"), "a\nb\n");
        assert_eq!(stat, LineChangeStat { added: 0, removed: 1 });
    }

    #[test]
    fn no_change() {
        let stat = compute_line_change_stat(Some("a\nb\n"), "a\nb\n");
        assert_eq!(stat, LineChangeStat { added: 0, removed: 0 });
    }

    #[test]
    fn new_file() {
        let stat = compute_line_change_stat(None, "x\ny\nz\n");
        assert_eq!(stat, LineChangeStat { added: 3, removed: 0 });
    }

    #[test]
    fn middle_edit_uses_lcs_not_total() {
        // before: a b c ; after: a x c —— 真实改动为 1 增 1 删
        let stat = compute_line_change_stat(Some("a\nb\nc\n"), "a\nx\nc\n");
        assert_eq!(stat, LineChangeStat { added: 1, removed: 1 });
    }

    #[test]
    fn trailing_newline_is_ignored() {
        let stat = compute_line_change_stat(Some("a\nb\n"), "a\nb");
        assert_eq!(stat, LineChangeStat { added: 0, removed: 0 });
    }
}
