//! `patch_diff_preview` —— Rust 移植自 `packages/ui/src/lib/patchDiffPreview.ts`
//!
//! 这里回答一个核心问题：**这份 patch 能不能安全地交给富 diff 渲染器？**
//!
//! 富渲染器（TS 侧的 `@pierre/diffs`）只能吃「恰好一个文件 diff」的 patch，
//! 而且会在主线程同步解析。遇到下面任何一类输入它都会抛错或长时间卡住：
//!
//! - 多文件拼在一起的 unified diff；
//! - 裸 hunk（没有 `---` / `+++` 文件头）；
//! - 超大 patch 或 hunk 行号极深的 patch；
//! - lockfile / `.gradle` / `zsh` 这类解析不稳定或没有高亮收益的文件；
//! - 只改了 rename / mode 的 patch（没有 `@@` hunk）。
//!
//! 上面每一条都对应一次线上事故，所以这里不是「顺手加个长度检查」，
//! 而是一套有明确降级顺序的判定。本移植逐条复刻，并把判据做成纯函数以便测试。
//!
//! ## 与 TS 的差异
//!
//! 1. **不引入 `@pierre/diffs`**：TS 在 `getPlainTextPatchFallbackLines` 里会直接调用
//!    `parsePatchFiles` / `getSingularPatch` / `getFiletypeFromFileName` 来确认
//!    「是不是恰好一个文件 diff」。Rust 侧没有这个库（也不需要为一个判定引入它），
//!    改为由调用方通过 [`PierrePatchProbe`] 注入这三个查询的结果。
//!    函数名与分支顺序与 TS 完全一致，只是把「去问库」换成了「读注入的结论」。
//! 2. **`MAX_PLAIN_TEXT_FALLBACK_RENDER_LINES` 的截断标记**：TS 里标记前缀是
//!    `"\\ __ZCODE_DIFF_TRUNCATED__:"` —— 它刻意**不**在 lib 层拼展示文案，
//!    避免把英文硬编码带进 UI。本移植保留同一套 marker token。
//! 3. **`getMaxHunkLineNumber` 里的 `Number.isNaN` 分支是死代码**：TS 写的是
//!    `Number.parseInt(match[2] ?? "1", 10)`，`?? "1"` 已经兜住了 `undefined`，
//!    所以 `Number.isNaN(oldCount)` 永远为假。本移植不复制这段死代码（见
//!    [`parse_hunk_header`] 的文档）。

/// 超过这么多行就降级（`MAX_PATCH_DIFF_SAFE_LINE_COUNT`）。
pub const MAX_PATCH_DIFF_SAFE_LINE_COUNT: usize = 1_200;

/// 超过这么多字符就降级（`MAX_PATCH_DIFF_SAFE_CHAR_COUNT`）。
pub const MAX_PATCH_DIFF_SAFE_CHAR_COUNT: usize = 180_000;

/// hunk 声明的最大行号超过它就降级（`MAX_PATCH_DIFF_SAFE_HUNK_LINE_NUMBER`）。
///
/// 某些大文件只改 1 行时，hunk 行号会落在很深的位置（如 1500+），
/// 富渲染器在这类输入上也可能长时间卡顿。
pub const MAX_PATCH_DIFF_SAFE_HUNK_LINE_NUMBER: usize = 1_200;

/// 轻量纯文本预览最多渲染多少行（`MAX_PLAIN_TEXT_FALLBACK_RENDER_LINES`）。
pub const MAX_PLAIN_TEXT_FALLBACK_RENDER_LINES: usize = 800;

/// 截断标记行的前缀。
///
/// 这里**不能**直接拼展示文案，否则 lib 层会把英文硬编码带进 UI、破坏国际化；
/// 改成内部 marker token，真正的文案在组件层渲染。
pub const FALLBACK_TRUNCATED_MARKER_PREFIX: &str = "\\ __ZCODE_DIFF_TRUNCATED__:";

/// 包管理器 lockfile：内容大、变更噪点多、高亮收益低，固定降级。
pub const PACKAGE_MANAGER_LOCKFILE_NAMES: [&str; 6] = [
    "bun.lock",
    "bun.lockb",
    "npm-shrinkwrap.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
];

/// 按**文件类型**强制降级的类型。
pub const PATCH_DIFF_FORCE_PLAIN_TEXT_FILE_TYPES: [&str; 1] = ["zsh"];

/// 按**路径后缀**强制降级的后缀。
///
/// Gradle 脚本会被 `@pierre/diffs` 识别成普通 text；这类 patch 在部分入口或
/// worker 不可用时继续走富渲染，仍可能同步解析卡住主线程。
pub const PATCH_DIFF_FORCE_PLAIN_TEXT_PATH_SUFFIXES: [&str; 2] = [".gradle", ".gradle.kts"];

/// 按 `/\r?\n/` 切行（与 TS 的 `patch.split(/\r?\n/)` 一致：CRLF 也只算一个换行）。
pub fn split_patch_lines(patch: &str) -> Vec<&str> {
    patch.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect()
}

fn build_truncated_marker_line(omitted_line_count: usize) -> String {
    format!("{FALLBACK_TRUNCATED_MARKER_PREFIX}{omitted_line_count}")
}

/// 把截断标记行解析回「省略了多少行」。
///
/// 解析不出来、或数字不是正数时返回 `None`（该行按普通内容渲染）。
pub fn parse_truncated_marker_omitted_line_count(line: &str) -> Option<usize> {
    let rest = line.strip_prefix(FALLBACK_TRUNCATED_MARKER_PREFIX)?;
    // TS 的正则是 `/^\\ __ZCODE_DIFF_TRUNCATED__:(\d+)$/` —— 整行必须只有数字
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let count: usize = rest.parse().ok()?;
    if count == 0 {
        return None;
    }
    Some(count)
}

/// 剥掉 unified diff 的行首协议标记（`+` / `-` / 空格），拿到真正的代码内容。
pub fn get_patch_preview_line_content(line: &str) -> String {
    if line.starts_with('+') || line.starts_with('-') || line.starts_with(' ') {
        return line.get(1..).unwrap_or("").to_string();
    }
    line.to_string()
}

/// 是否是 hunk 头（`@@` 或 `@@ ...`）。
pub fn is_patch_hunk_header_line(line: &str) -> bool {
    line == "@@" || line.starts_with("@@ ")
}

/// 吃掉一段前导 ASCII 数字，返回 `(数值, 剩余串)`；一个数字都没有时返回 `None`。
fn take_digits(input: &str) -> Option<(usize, &str)> {
    let end = input
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(input.len());
    if end == 0 {
        return None;
    }
    Some((input.get(..end)?.parse().ok()?, input.get(end..)?))
}

/// 吃掉 `,<数字>`；没有逗号时按 TS 的 `?? "1"` 语义返回 `1`。
fn take_optional_count(input: &str) -> Option<(usize, &str)> {
    match input.strip_prefix(',') {
        Some(after) => take_digits(after),
        None => Some((1, input)),
    }
}

/// 解析 hunk 头 `@@ -a[,b] +c[,d] @@`。
///
/// 返回 `(旧起始, 旧行数, 新起始, 新行数)`。数量缺省为 `1`
/// （TS 的 `match[2] ?? "1"`）—— 因为有了这个兜底，TS 里紧随其后的
/// `Number.isNaN(oldCount) ? 1 : oldCount` 永远走不到，本移植不复制它。
pub fn parse_hunk_header(line: &str) -> Option<(usize, usize, usize, usize)> {
    let rest = line.strip_prefix("@@")?;
    // TS 正则里是 `\s`（只吃一个空白），不是 `\s+`
    let rest = rest.strip_prefix(|c: char| c.is_whitespace())?;
    let rest = rest.strip_prefix('-')?;
    let (old_start, rest) = take_digits(rest)?;
    let (old_count, rest) = take_optional_count(rest)?;
    let rest = rest.strip_prefix(|c: char| c.is_whitespace())?;
    let rest = rest.strip_prefix('+')?;
    let (new_start, rest) = take_digits(rest)?;
    let (new_count, rest) = take_optional_count(rest)?;
    let rest = rest.strip_prefix(|c: char| c.is_whitespace())?;
    rest.strip_prefix("@@")?;
    Some((old_start, old_count, new_start, new_count))
}

/// 只取 `(旧行数, 新行数)` —— 对应 TS 的 `parseHunkRange`。
fn parse_hunk_range(line: &str) -> Option<(usize, usize)> {
    parse_hunk_header(line).map(|(_, old, _, new)| (old, new))
}

/// `lines[i]` 与 `lines[i + 1]` 是否构成一对文件头（`--- a/x` 紧跟 `+++ b/x`）。
fn is_file_header_pair(lines: &[&str], index: usize) -> bool {
    lines.get(index).is_some_and(|l| l.starts_with("--- "))
        && lines.get(index + 1).is_some_and(|l| l.starts_with("+++ "))
}

/// 按 hunk 头声明的行数消费正文，返回下一个位置；输入不完整时返回 `None`。
///
/// **关键点**：消费过程中先探测完整的 `---` / `+++` 文件头对，一旦遇到就提前结束
/// 当前 hunk。否则当模型把 hunk 头的行数写得比正文多（LLM 常见的 off-by-one）时，
/// 多出的配额正好会「吃掉」下一个文件的头两行，后续 `@@` 又被当成同一文件的下一个
/// hunk，于是**多文件 patch 被误计成单文件** —— 这正是要避免的降级失效场景。
fn consume_hunk_body(
    lines: &[&str],
    start: usize,
    old_lines: usize,
    new_lines: usize,
) -> Option<usize> {
    let mut cursor = start;
    // 用有符号数：TS 里 `remainingOld < 0` 会返回 null，所以要能表示负数
    let mut remaining_old = old_lines as isize;
    let mut remaining_new = new_lines as isize;

    while remaining_old > 0 || remaining_new > 0 {
        let line = *lines.get(cursor)?;

        if is_file_header_pair(lines, cursor) {
            return Some(cursor);
        }

        // `\ No newline at end of file` 这类反斜杠行不占 hunk 配额
        if line.starts_with("\\ ") {
            cursor += 1;
            continue;
        }

        if line.starts_with(' ') {
            remaining_old -= 1;
            remaining_new -= 1;
        } else if line.starts_with('-') {
            remaining_old -= 1;
        } else if line.starts_with('+') {
            remaining_new -= 1;
        } else {
            return None;
        }

        if remaining_old < 0 || remaining_new < 0 {
            return None;
        }
        cursor += 1;
    }

    // 正文结束后可能还跟着反斜杠行
    while lines.get(cursor).is_some_and(|l| l.starts_with("\\ ")) {
        cursor += 1;
    }
    Some(cursor)
}

/// 数出 patch 里有几个文件 diff。
///
/// 优先信 `diff --git ` 头；没有时才退回到「按 hunk 行数消费正文」的手写解析。
pub fn count_patch_file_diffs(patch: &str) -> usize {
    let lines = split_patch_lines(patch);

    let git_diff_header_count = lines
        .iter()
        .filter(|line| line.starts_with("diff --git "))
        .count();
    if git_diff_header_count > 0 {
        return git_diff_header_count;
    }

    let mut diff_count = 0usize;
    let mut cursor = 0usize;

    while cursor < lines.len() {
        if !is_file_header_pair(&lines, cursor) {
            cursor += 1;
            continue;
        }

        let mut hunk_cursor = cursor + 2;
        let mut has_hunk = false;
        let mut saw_hunk_header = false;

        while hunk_cursor < lines.len() {
            let Some(range) = lines.get(hunk_cursor).and_then(|l| parse_hunk_range(l)) else {
                break;
            };
            saw_hunk_header = true;

            let Some(next_cursor) = consume_hunk_body(&lines, hunk_cursor + 1, range.0, range.1)
            else {
                break;
            };

            has_hunk = true;
            hunk_cursor = next_cursor;
        }

        // 既没有 hunk 头、也没有成功消费的 hunk → 这不是文件头，继续往下找
        if !has_hunk && !saw_hunk_header {
            cursor += 1;
            continue;
        }

        // hunk 正文里允许出现以 `---` / `+++` 开头的真实文本行，所以不能只按前缀生数。
        // 但一旦见过 hunk 头，即使正文被截断（`has_hunk == false`）也要保守计数，
        // 避免把多文件 patch 误判成单文件。
        diff_count += 1;
        cursor = if has_hunk { hunk_cursor } else { cursor + 2 };
    }

    diff_count
}

/// 所有 `@@` hunk 头里出现过的最大行号（旧/新两侧取大）。
pub fn get_max_hunk_line_number(lines: &[String]) -> usize {
    let mut max_line_number = 0usize;

    for line in lines {
        if !line.starts_with("@@ ") {
            continue;
        }
        let Some((old_start, old_count, new_start, new_count)) = parse_hunk_header(line) else {
            continue;
        };
        let old_end = old_start + old_count.saturating_sub(1);
        let new_end = new_start + new_count.saturating_sub(1);
        max_line_number = max_line_number.max(old_end).max(new_end);
    }

    max_line_number
}

/// 行数过多时「掐头去尾 + 中间插一条截断标记」。
///
/// 保留 `MAX_PLAIN_TEXT_FALLBACK_RENDER_LINES - 1` 行，其中前半取
/// `ceil(keep / 2)`（TS 用 `Math.ceil`），后半取剩下的。
pub fn limit_plain_text_preview_lines(lines: &[String]) -> Vec<String> {
    if lines.len() <= MAX_PLAIN_TEXT_FALLBACK_RENDER_LINES {
        return lines.to_vec();
    }

    let keep_count = MAX_PLAIN_TEXT_FALLBACK_RENDER_LINES - 1;
    let head_count = keep_count.div_ceil(2);
    let tail_count = keep_count - head_count;
    let omitted_count = lines.len() - keep_count;

    let mut out = Vec::with_capacity(keep_count + 1);
    out.extend_from_slice(&lines[..head_count]);
    out.push(build_truncated_marker_line(omitted_count));
    out.extend_from_slice(&lines[lines.len() - tail_count..]);
    out
}

/// 从 `--- ` / `+++ ` 行里取出文件名（跳过 `/dev/null`）。
///
/// 标准 unified diff 头部允许在路径后追加 tab 分隔的时间戳。**不先剥离时间戳**，
/// `.gradle` 这类按后缀降级的规则会被 `test.gradle\t2024-01-01` 绕过。
pub fn get_patch_content_file_name(lines: &[String]) -> Option<String> {
    for line in lines {
        if !line.starts_with("--- ") && !line.starts_with("+++ ") {
            continue;
        }

        let after_marker = line.get(4..).unwrap_or("").trim();
        let file_name = after_marker.split('\t').next().unwrap_or("").trim();

        if file_name.is_empty() || file_name == "/dev/null" {
            continue;
        }
        return Some(file_name.to_string());
    }

    None
}

/// 去掉 `a/` / `b/` 前缀（`git diff` 的默认前缀）。
pub fn normalize_patch_file_name(file_name: &str) -> String {
    if let Some(rest) = file_name.strip_prefix("a/") {
        return rest.to_string();
    }
    if let Some(rest) = file_name.strip_prefix("b/") {
        return rest.to_string();
    }
    file_name.to_string()
}

/// 是否是包管理器 lockfile（只看路径最后一段，大小写不敏感）。
pub fn is_package_manager_lockfile(file_name: Option<&str>) -> bool {
    let Some(file_name) = file_name else {
        return false;
    };
    let normalized = normalize_patch_file_name(file_name);
    let last_segment = normalized.split('/').next_back().unwrap_or("").to_lowercase();
    PACKAGE_MANAGER_LOCKFILE_NAMES.contains(&last_segment.as_str())
}

/// 是否命中按后缀强制降级的名单。
pub fn should_force_plain_text_patch_preview_by_file_name(file_name: Option<&str>) -> bool {
    let Some(file_name) = file_name else {
        return false;
    };
    let normalized = normalize_patch_file_name(file_name).to_lowercase();
    PATCH_DIFF_FORCE_PLAIN_TEXT_PATH_SUFFIXES
        .iter()
        .any(|suffix| normalized.ends_with(suffix))
}

/// 是否要强制走轻量纯文本预览。
///
/// 三条并列判据：lockfile / 路径后缀 / 文件类型。
pub fn should_force_plain_text_patch_preview(
    file_name: Option<&str>,
    file_type: &str,
) -> bool {
    if is_package_manager_lockfile(file_name) {
        return true;
    }
    if should_force_plain_text_patch_preview_by_file_name(file_name) {
        return true;
    }
    if file_name.is_none() {
        return false;
    }
    PATCH_DIFF_FORCE_PLAIN_TEXT_FILE_TYPES.contains(&file_type)
}

/// 只保留「变更元信息」行（rename / mode 变更这类没有 hunk 的 patch 用）。
pub fn collect_patch_metadata_preview_lines(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|line| {
            if line.trim().is_empty() {
                return false;
            }
            !line.starts_with("diff --git ")
                && !line.starts_with("index ")
                && !line.starts_with("--- ")
                && !line.starts_with("+++ ")
        })
        .cloned()
        .collect()
}

/// 只保留 hunk 正文（丢掉 `diff --git` / `index` / `---` / `+++` / `@@` 这些协议头）。
pub fn collect_plain_text_preview_lines(lines: &[String]) -> Vec<String> {
    let mut preview_lines = Vec::new();
    let mut in_hunk = false;

    for line in lines {
        if !in_hunk {
            if is_patch_hunk_header_line(line) {
                in_hunk = true;
            }
            continue;
        }

        // 之前直接按 `+++` / `---` / `@@` 前缀过滤，会把正文里真实存在的
        // `++ ` / `-- ` 行误删。这里只跳过 hunk 头本身。
        if is_patch_hunk_header_line(line) {
            continue;
        }

        preview_lines.push(line.clone());
    }

    if in_hunk || !preview_lines.is_empty() {
        return preview_lines;
    }

    // rename-only / mode-only 这类 Git patch 没有 `@@` hunk，但仍然有用户需要看的
    // 变更信息；这里保留元信息走轻量 fallback。
    collect_patch_metadata_preview_lines(lines)
}

/// 去掉尾部空行，并且**永远不返回空列表**（至少一条空串）。
pub fn normalize_plain_text_preview_lines(lines: &[String]) -> Vec<String> {
    let mut normalized = limit_plain_text_preview_lines(lines);

    while normalized.last().is_some_and(|line| line.is_empty()) {
        normalized.pop();
    }

    if normalized.is_empty() {
        vec![String::new()]
    } else {
        normalized
    }
}

/// 轻量纯文本预览的正文（不含截断处理）。
pub fn get_plain_text_patch_content_lines(patch: &str) -> Vec<String> {
    let lines: Vec<String> = split_patch_lines(patch)
        .into_iter()
        .map(str::to_string)
        .collect();
    collect_plain_text_preview_lines(&lines)
}

/// 轻量纯文本预览（含截断处理）。
pub fn get_plain_text_patch_preview_lines(patch: &str) -> Vec<String> {
    normalize_plain_text_preview_lines(&get_plain_text_patch_content_lines(patch))
}

/// `@pierre/diffs` 侧的两个查询结果。
///
/// 本移植不引入该库，改由调用方注入（见模块文档差异 1）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PierrePatchProbe {
    /// `parsePatchFiles(patch, undefined, true)` 是否解析出「恰好一个文件 diff」
    /// （`parsedPatches.length === 1 && parsedPatches[0].files.length === 1`）。
    ///
    /// 解析抛错时传 `false`。
    pub single_file_diff: bool,
    /// `getFiletypeFromFileName(文件名)` 的结果。
    ///
    /// 未知后缀会回退成 `"text"` —— 调用方应当照此填充。
    pub file_type: String,
}

impl Default for PierrePatchProbe {
    fn default() -> Self {
        Self {
            single_file_diff: true,
            file_type: "text".to_string(),
        }
    }
}

/// 判断这份 patch 是否必须降级成轻量纯文本预览。
///
/// 返回 `None` 表示「可以交给富渲染器」；返回 `Some(lines)` 表示降级后的预览行。
///
/// 判据顺序与 TS 完全一致，逐个解释见各分支注释。
pub fn get_plain_text_patch_fallback_lines(
    patch: &str,
    probe: &PierrePatchProbe,
) -> Option<Vec<String>> {
    let lines: Vec<String> = split_patch_lines(patch)
        .into_iter()
        .map(str::to_string)
        .collect();

    let patch_file_diff_count = count_patch_file_diffs(patch);
    let has_multiple_file_diffs = patch_file_diff_count > 1;
    let has_no_file_diff = patch_file_diff_count == 0;
    let is_created_file = lines.iter().any(|line| line == "--- /dev/null");
    let is_deleted_file = lines.iter().any(|line| line == "+++ /dev/null");
    let max_hunk_line_number = get_max_hunk_line_number(&lines);
    let is_oversized_patch = lines.len() > MAX_PATCH_DIFF_SAFE_LINE_COUNT
        || patch.len() > MAX_PATCH_DIFF_SAFE_CHAR_COUNT;
    let is_deep_hunk_line_patch = max_hunk_line_number > MAX_PATCH_DIFF_SAFE_HUNK_LINE_NUMBER;
    let patch_file_name = get_patch_content_file_name(&lines);
    let should_force_plain_text_preview =
        should_force_plain_text_patch_preview(patch_file_name.as_deref(), &probe.file_type);

    let metadata_only_preview_lines = collect_patch_metadata_preview_lines(&lines);
    let is_metadata_only_patch = !metadata_only_preview_lines.is_empty()
        && !lines.iter().any(|line| is_patch_hunk_header_line(line));

    let clearly_safe = !has_no_file_diff
        && !has_multiple_file_diffs
        && !is_created_file
        && !is_deleted_file
        && !is_oversized_patch
        && !is_deep_hunk_line_patch
        && !is_metadata_only_patch
        && !should_force_plain_text_preview;

    if clearly_safe {
        // 自有计数器按 hunk 行数能判断这是逻辑上的单文件，但富渲染器的实际解析器
        // 会把正文里的 `--- ...` 误切成第二个文件。最终是否安全必须以**真正负责渲染
        // 的那套解析结果**为准，不能让两套解析语义再次产生漏网输入。
        if probe.single_file_diff {
            return None;
        }
        return Some(normalize_plain_text_preview_lines(
            &collect_plain_text_preview_lines(&lines),
        ));
    }

    if is_metadata_only_patch {
        return Some(normalize_plain_text_preview_lines(
            &metadata_only_preview_lines,
        ));
    }

    if has_no_file_diff {
        // summary / 工具回放里可能拿到 apply_patch 片段或裸 hunk。这些输入没有标准
        // 文件头，富渲染器会同步抛「must contain exactly 1 file diff」，必须直接降级。
        return Some(normalize_plain_text_preview_lines(
            &collect_plain_text_preview_lines(&lines),
        ));
    }

    if should_force_plain_text_preview {
        // lockfile / shell script 这类文件走富渲染的高亮 worker 路径不稳定，
        // 已观察到 diff 已加载但展开内容失败。固定走轻量文本预览，优先保证内容可见。
        return Some(normalize_plain_text_preview_lines(
            &collect_plain_text_preview_lines(&lines),
        ));
    }

    if (is_created_file || is_deleted_file) && !is_oversized_patch && !is_deep_hunk_line_patch {
        // 展开「非纯文本」新增/删除文件时富渲染器可能只渲染空壳。新增/删除本来就是
        // 整文件快照，这里不再按扩展名分流，统一走轻量 fallback，保证展开态始终有内容。
        return Some(normalize_plain_text_preview_lines(
            &collect_plain_text_preview_lines(&lines),
        ));
    }

    if !has_multiple_file_diffs && !is_oversized_patch && !is_deep_hunk_line_patch {
        // 新建/删除的纯文本文件即使 patch 很小，富渲染器也可能在同步渲染阶段卡住
        // 主线程。这里按**解析后的文件类型**拦截（而不是只匹配固定扩展名），
        // 未知后缀会回退成 text，所以非 text 就说明它有专门的高亮路径。
        if probe.file_type != "text" {
            return None;
        }
        // 类型是 text：继续往下走，最终仍然降级
    }

    // 富渲染器只能渲染单文件 patch；多文件拼在一个字段里会在渲染阶段抛错并触发
    // 整页错误边界。超大 patch 与深行号 patch 同样降级，优先保证界面可用。
    Some(normalize_plain_text_preview_lines(
        &collect_plain_text_preview_lines(&lines),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|l| l.to_string()).collect()
    }

    // ---- 切行 ----

    #[test]
    fn split_handles_lf_and_crlf() {
        assert_eq!(split_patch_lines("a\nb"), vec!["a", "b"]);
        // CRLF 只算一个换行，`\r` 被剥掉
        assert_eq!(split_patch_lines("a\r\nb"), vec!["a", "b"]);
        assert_eq!(split_patch_lines(""), vec![""]);
    }

    // ---- 截断标记 ----

    #[test]
    fn truncated_marker_round_trips() {
        let line = build_truncated_marker_line(42);
        assert_eq!(line, "\\ __ZCODE_DIFF_TRUNCATED__:42");
        assert_eq!(parse_truncated_marker_omitted_line_count(&line), Some(42));
    }

    #[test]
    fn truncated_marker_rejects_non_markers() {
        for line in [
            "",
            "\\ __ZCODE_DIFF_TRUNCATED__:",
            "\\ __ZCODE_DIFF_TRUNCATED__:abc",
            "\\ __ZCODE_DIFF_TRUNCATED__:0",
            "\\ __ZCODE_DIFF_TRUNCATED__:12 ",
            " __ZCODE_DIFF_TRUNCATED__:12",
            "\\ __ZCODE_DIFF_TRUNCATED__:12x",
            "+ normal diff line",
        ] {
            assert_eq!(
                parse_truncated_marker_omitted_line_count(line),
                None,
                "{line:?}"
            );
        }
    }

    // ---- 行内容 ----

    #[test]
    fn line_content_strips_exactly_one_marker() {
        assert_eq!(get_patch_preview_line_content("+added"), "added");
        assert_eq!(get_patch_preview_line_content("-removed"), "removed");
        assert_eq!(get_patch_preview_line_content(" context"), "context");
        // 只剥一个：`++x` 是「新增了一行内容为 `+x`」
        assert_eq!(get_patch_preview_line_content("++x"), "+x");
        assert_eq!(get_patch_preview_line_content("@@ -1 +1 @@"), "@@ -1 +1 @@");
        assert_eq!(get_patch_preview_line_content(""), "");
    }

    // ---- hunk 头解析 ----

    #[test]
    fn hunk_header_with_explicit_counts() {
        assert_eq!(parse_hunk_header("@@ -1,3 +1,4 @@"), Some((1, 3, 1, 4)));
        assert_eq!(parse_hunk_header("@@ -10,0 +20,7 @@"), Some((10, 0, 20, 7)));
    }

    #[test]
    fn hunk_header_defaults_counts_to_one() {
        // TS 的 `?? "1"`：省略数量就是 1
        assert_eq!(parse_hunk_header("@@ -5 +5 @@"), Some((5, 1, 5, 1)));
        // 旧侧省略、新侧显式
        assert_eq!(parse_hunk_header("@@ -5 +5,2 @@"), Some((5, 1, 5, 2)));
    }

    #[test]
    fn hunk_header_rejects_malformed_input() {
        for line in [
            "",
            "@@",
            "@@ -1 +1",
            "@@ -1 +1 @@ trailing", // 尾部允许（TS 正则不锚定结尾）
            "@@ -x +1 @@",
            "@@ -1 +y @@",
            "@@ 1,3 +1,4 @@", // 缺 `-`
            " @@ -1 +1 @@",   // 前导空格
        ] {
            let parsed = parse_hunk_header(line);
            if line == "@@ -1 +1 @@ trailing" {
                assert!(parsed.is_some(), "TS 正则在 `@@` 处即成功，尾部内容不参与");
            } else {
                assert!(parsed.is_none(), "{line:?} 不该被解析");
            }
        }
    }

    #[test]
    fn max_hunk_line_number_takes_the_farther_side() {
        let lines = owned(&[
            "@@ -1,3 +1,4 @@",
            "@@ -100,5 +150,2 @@", // 旧侧到 104，新侧到 151
        ]);
        assert_eq!(get_max_hunk_line_number(&lines), 151);
    }

    #[test]
    fn max_hunk_line_number_ignores_bare_hunk_headers() {
        // `isPatchHunkHeaderLine` 认 `@@`，但 `getMaxHunkLineNumber` 只认 `@@ `
        let lines = owned(&["@@", "@@ -1,2 +1,2 @@"]);
        assert_eq!(get_max_hunk_line_number(&lines), 2);
        assert_eq!(get_max_hunk_line_number(&owned(&[])), 0);
    }

    // ---- 文件数统计 ----

    #[test]
    fn git_diff_headers_short_circuit_the_parser() {
        let patch = "\
diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1 +1 @@
-x
+y
diff --git a/b.txt b/b.txt
--- a/b.txt
+++ b/b.txt
@@ -1 +1 @@
-p
+q";
        assert_eq!(count_patch_file_diffs(patch), 2);
    }

    #[test]
    fn headerless_single_file_patch_counts_as_one() {
        let patch = "\
--- a/a.txt
+++ b/a.txt
@@ -1,2 +1,2 @@
 keep
-old
+new";
        assert_eq!(count_patch_file_diffs(patch), 1);
    }

    #[test]
    fn headerless_multi_file_patch_counts_both() {
        let patch = "\
--- a/a.txt
+++ b/a.txt
@@ -1 +1 @@
-x
+y
--- a/b.txt
+++ b/b.txt
@@ -1 +1 @@
-p
+q";
        assert_eq!(count_patch_file_diffs(patch), 2);
    }

    /// 模型把 hunk 行数写多（off-by-one）时，多出的配额**不能**吃掉下一个文件头。
    #[test]
    fn over_declared_hunk_counts_do_not_swallow_the_next_file_header() {
        let patch = "\
--- a/a.txt
+++ b/a.txt
@@ -1,9 +1,9 @@
-x
+y
--- a/b.txt
+++ b/b.txt
@@ -1 +1 @@
-p
+q";
        // 第 1 个 hunk 声明 9 行，正文只有 2 行；若不探测文件头对就会一路吃到 b.txt
        assert_eq!(count_patch_file_diffs(patch), 2);
    }

    #[test]
    fn bare_hunks_have_no_file_diff() {
        assert_eq!(count_patch_file_diffs("@@ -1 +1 @@\n-x\n+y"), 0);
        assert_eq!(count_patch_file_diffs(""), 0);
        assert_eq!(count_patch_file_diffs("随便一段没有 patch 结构的文本"), 0);
    }

    /// `--- /dev/null` 这种「新建文件」的头也符合文件头对，会被计成一个 diff。
    #[test]
    fn created_file_headers_count_as_a_diff() {
        let patch = "\
--- /dev/null
+++ b/new.txt
@@ -0,0 +1,2 @@
+hello
+world";
        assert_eq!(count_patch_file_diffs(patch), 1);
    }

    #[test]
    fn no_newline_marker_lines_do_not_consume_quota() {
        let patch = "\
--- a/a.txt
+++ b/a.txt
@@ -1 +1 @@
-x
\\ No newline at end of file
+y
\\ No newline at end of file";
        assert_eq!(count_patch_file_diffs(patch), 1);
    }

    // ---- 截断 ----

    #[test]
    fn short_previews_are_returned_untouched() {
        let lines = owned(&["a", "b", "c"]);
        assert_eq!(limit_plain_text_preview_lines(&lines), lines);
    }

    #[test]
    fn long_previews_keep_the_head_and_tail() {
        let lines: Vec<String> = (0..MAX_PLAIN_TEXT_FALLBACK_RENDER_LINES + 200)
            .map(|i| i.to_string())
            .collect();
        let limited = limit_plain_text_preview_lines(&lines);

        let keep_count = MAX_PLAIN_TEXT_FALLBACK_RENDER_LINES - 1;
        assert_eq!(limited.len(), keep_count + 1);

        let head_count = keep_count.div_ceil(2);
        assert_eq!(head_count, 400);
        // 头部保留
        assert_eq!(limited[0], "0");
        assert_eq!(limited[head_count - 1], (head_count - 1).to_string());
        // 中间是截断标记
        let omitted = lines.len() - keep_count;
        assert_eq!(limited[head_count], build_truncated_marker_line(omitted));
        assert_eq!(
            parse_truncated_marker_omitted_line_count(&limited[head_count]),
            Some(omitted)
        );
        // 尾部保留到最后一行的原文
        assert_eq!(limited.last().unwrap(), lines.last().unwrap());
    }

    // ---- 文件名 ----

    #[test]
    fn file_name_comes_from_the_first_header() {
        let lines = owned(&["diff --git a/x b/x", "index 1..2", "--- a/src/x.ts", "+++ b/src/x.ts"]);
        assert_eq!(get_patch_content_file_name(&lines).as_deref(), Some("a/src/x.ts"));
    }

    #[test]
    fn file_name_strips_the_tab_separated_timestamp() {
        let lines = owned(&["--- a/build.gradle\t2024-01-01 10:00:00 +0000"]);
        assert_eq!(
            get_patch_content_file_name(&lines).as_deref(),
            Some("a/build.gradle")
        );
    }

    #[test]
    fn file_name_skips_dev_null() {
        let lines = owned(&["--- /dev/null", "+++ b/new.txt"]);
        assert_eq!(get_patch_content_file_name(&lines).as_deref(), Some("b/new.txt"));
        assert_eq!(get_patch_content_file_name(&owned(&["--- /dev/null"])), None);
        assert_eq!(get_patch_content_file_name(&owned(&["普通文本"])), None);
    }

    #[test]
    fn normalize_strips_only_a_leading_git_prefix() {
        assert_eq!(normalize_patch_file_name("a/x.ts"), "x.ts");
        assert_eq!(normalize_patch_file_name("b/x.ts"), "x.ts");
        // 只剥一层，且必须带斜杠
        assert_eq!(normalize_patch_file_name("ab/x.ts"), "ab/x.ts");
        assert_eq!(normalize_patch_file_name("a"), "a");
        assert_eq!(normalize_patch_file_name("src/a/x.ts"), "src/a/x.ts");
    }

    #[test]
    fn lockfiles_are_detected_by_last_segment() {
        for name in [
            "bun.lock",
            "bun.lockb",
            "npm-shrinkwrap.json",
            "package-lock.json",
            "pnpm-lock.yaml",
            "yarn.lock",
        ] {
            assert!(is_package_manager_lockfile(Some(name)), "{name}");
            // 带目录前缀也认
            assert!(is_package_manager_lockfile(Some(&format!("a/nested/{name}"))));
            // 大小写不敏感
            assert!(is_package_manager_lockfile(Some(&name.to_uppercase())));
            // 带 b/ 前缀也认
            assert!(is_package_manager_lockfile(Some(&format!("b/{name}"))));
        }
        assert!(!is_package_manager_lockfile(None));
        assert!(!is_package_manager_lockfile(Some("src/index.ts")));
        // 只在最后一段匹配：`yarn.lock.ts` 不是 lockfile
        assert!(!is_package_manager_lockfile(Some("yarn.lock.ts")));
    }

    #[test]
    fn gradle_suffixes_force_plain_text() {
        assert!(should_force_plain_text_patch_preview_by_file_name(Some(
            "a/build.gradle"
        )));
        assert!(should_force_plain_text_patch_preview_by_file_name(Some(
            "b/settings.gradle.kts"
        )));
        // 大小写不敏感
        assert!(should_force_plain_text_patch_preview_by_file_name(Some(
            "a/BUILD.GRADLE"
        )));
        assert!(!should_force_plain_text_patch_preview_by_file_name(Some(
            "a/build.gradle.bak"
        )));
        assert!(!should_force_plain_text_patch_preview_by_file_name(None));
    }

    #[test]
    fn forced_plain_text_needs_a_file_name_for_the_type_rule() {
        // 类型命中但没有文件名 → 类型规则不生效
        assert!(!should_force_plain_text_patch_preview(None, "zsh"));
        // 有文件名 + 类型命中 → 生效
        assert!(should_force_plain_text_patch_preview(Some("a/run.zsh"), "zsh"));
        assert!(!should_force_plain_text_patch_preview(Some("a/run.sh"), "shell"));
        // lockfile 规则不需要类型
        assert!(should_force_plain_text_patch_preview(
            Some("package-lock.json"),
            "json"
        ));
        // 后缀规则也不需要类型
        assert!(should_force_plain_text_patch_preview(Some("a/build.gradle"), "text"));
    }

    // ---- 元信息 / 正文收集 ----

    #[test]
    fn metadata_lines_drop_the_protocol_headers() {
        let lines = owned(&[
            "diff --git a/x b/x",
            "index 1..2 100644",
            "--- a/x",
            "+++ b/x",
            "rename from x",
            "rename to y",
            "",
        ]);
        assert_eq!(
            collect_patch_metadata_preview_lines(&lines),
            owned(&["rename from x", "rename to y"])
        );
    }

    #[test]
    fn plain_text_lines_start_at_the_first_hunk() {
        let lines = owned(&[
            "diff --git a/x b/x",
            "index 1..2 100644",
            "--- a/x",
            "+++ b/x",
            "@@ -1,3 +1,3 @@",
            " keep",
            "-old",
            "+new",
        ]);
        assert_eq!(
            collect_plain_text_preview_lines(&lines),
            owned(&[" keep", "-old", "+new"])
        );
    }

    /// 进入 hunk 后，正文里真实的 `-- ` / `++ ` 行必须保留。
    #[test]
    fn plain_text_lines_keep_body_lines_that_look_like_headers() {
        let lines = owned(&[
            "--- a/x",
            "+++ b/x",
            "@@ -1,2 +1,3 @@",
            " -- a real comment",
            " ++ another",
            "@@ -10,2 +11,2 @@",
            " tail",
        ]);
        assert_eq!(
            collect_plain_text_preview_lines(&lines),
            owned(&[" -- a real comment", " ++ another", " tail"])
        );
    }

    #[test]
    fn metadata_only_patches_fall_back_to_metadata() {
        let lines = owned(&[
            "diff --git a/x b/x",
            "old mode 100644",
            "new mode 100755",
        ]);
        assert_eq!(
            collect_plain_text_preview_lines(&lines),
            owned(&["old mode 100644", "new mode 100755"])
        );
    }

    #[test]
    fn normalize_never_returns_an_empty_list() {
        assert_eq!(normalize_plain_text_preview_lines(&[]), owned(&[""]));
        // 全是空行 → 全被弹掉 → 回落到一条空串
        assert_eq!(
            normalize_plain_text_preview_lines(&owned(&["", "", ""])),
            owned(&[""])
        );
        // 只弹尾部的空行
        assert_eq!(
            normalize_plain_text_preview_lines(&owned(&["a", "", "b", ""])),
            owned(&["a", "", "b"])
        );
    }

    // ---- 端到端降级判定 ----

    /// 干净的单文件纯文本 patch：可以交给富渲染器。
    #[test]
    fn a_clean_single_text_patch_is_left_to_the_rich_viewer() {
        let patch = "\
--- a/a.txt
+++ b/a.txt
@@ -1,2 +1,2 @@
 keep
-old
+new";
        assert_eq!(get_plain_text_patch_fallback_lines(patch, &PierrePatchProbe::default()), None);
    }

    /// 多文件 patch 必须降级（富渲染器只吃单文件）。
    ///
    /// 注意输出里**混进了第二个文件的 `---` / `+++` 头**：TS 的收集逻辑是
    /// 「进入 hunk 后除 `@@` 外一律原样保留」，第二个文件的头两行此时已经处在
    /// hunk 状态里，于是被当成正文。这是上游行为，原样保留（它只影响预览里多出
    /// 两行，不影响判定结果）。
    #[test]
    fn a_multi_file_patch_is_forced_to_plain_text() {
        let patch = "\
--- a/a.txt
+++ b/a.txt
@@ -1 +1 @@
-x
+y
--- a/b.txt
+++ b/b.txt
@@ -1 +1 @@
-p
+q";
        let lines = get_plain_text_patch_fallback_lines(patch, &PierrePatchProbe::default())
            .expect("多文件 patch 必须降级");
        assert_eq!(
            lines,
            owned(&["-x", "+y", "--- a/b.txt", "+++ b/b.txt", "-p", "+q"])
        );
        // 只有 `@@` 会被继续过滤掉
        assert!(!lines.iter().any(|l| l.starts_with("@@")));
    }

    /// 裸 hunk（没有文件头）也要降级 —— 富渲染器会同步抛错。
    #[test]
    fn a_bare_hunk_is_forced_to_plain_text() {
        let lines = get_plain_text_patch_fallback_lines(
            "@@ -1 +1 @@\n-x\n+y",
            &PierrePatchProbe::default(),
        )
        .expect("裸 hunk 必须降级");
        assert_eq!(lines, owned(&["-x", "+y"]));
    }

    /// 自有计数器说是单文件，但富渲染器的解析器说不是 —— 以解析器为准。
    #[test]
    fn the_rich_viewer_parser_has_the_final_say() {
        let patch = "\
--- a/a.txt
+++ b/a.txt
@@ -1 +1 @@
-x
+y";
        let probe = PierrePatchProbe {
            single_file_diff: false,
            file_type: "text".to_string(),
        };
        let lines = get_plain_text_patch_fallback_lines(patch, &probe)
            .expect("解析器说不是单文件就必须降级");
        assert_eq!(lines, owned(&["-x", "+y"]));
    }

    #[test]
    fn oversized_patches_are_forced_to_plain_text() {
        // 靠行数超限
        let mut patch = String::from("--- a/a.txt\n+++ b/a.txt\n@@ -1,1 +1,2000 @@\n");
        for i in 0..(MAX_PATCH_DIFF_SAFE_LINE_COUNT + 10) {
            patch.push_str(&format!("+line {i}\n"));
        }
        assert!(
            get_plain_text_patch_fallback_lines(&patch, &PierrePatchProbe::default()).is_some()
        );
    }

    /// hunk 声明了很深的行号（大文件只改一行）→ 降级。
    #[test]
    fn deep_hunk_line_numbers_are_forced_to_plain_text() {
        let patch = format!(
            "--- a/a.txt\n+++ b/a.txt\n@@ -{},1 +{},1 @@\n-x\n+y",
            MAX_PATCH_DIFF_SAFE_HUNK_LINE_NUMBER + 100,
            MAX_PATCH_DIFF_SAFE_HUNK_LINE_NUMBER + 100
        );
        assert!(
            get_plain_text_patch_fallback_lines(&patch, &PierrePatchProbe::default()).is_some()
        );
    }

    /// 新建 / 删除文件的 patch 一律降级（富渲染器可能只渲染空壳）。
    #[test]
    fn created_and_deleted_files_are_forced_to_plain_text() {
        let created = "\
--- /dev/null
+++ b/new.txt
@@ -0,0 +1,2 @@
+hello
+world";
        let deleted = "\
--- a/gone.txt
+++ /dev/null
@@ -1,2 +0,0 @@
-bye
-world";
        for patch in [created, deleted] {
            let lines = get_plain_text_patch_fallback_lines(patch, &PierrePatchProbe::default())
                .unwrap_or_else(|| panic!("{patch} 必须降级"));
            assert!(!lines.is_empty());
        }
    }

    /// lockfile → 降级，且预览里不该出现任何协议头。
    #[test]
    fn lockfiles_are_forced_to_plain_text() {
        let patch = "\
--- a/package-lock.json
+++ b/package-lock.json
@@ -1,2 +1,2 @@
 {
-  \"version\": 1
+  \"version\": 2
 }";
        let lines = get_plain_text_patch_fallback_lines(patch, &PierrePatchProbe::default())
            .expect("lockfile 必须降级");
        // 收集阶段保留行首协议标记（`+` / `-` / 空格），剥标记是组件层
        // `getLightweightDiffLineParts` 的事。
        assert_eq!(
            lines,
            owned(&[" {", "-  \"version\": 1", "+  \"version\": 2", " }"])
        );
        // 协议头不会出现在预览里
        for line in &lines {
            assert!(!line.starts_with("--- "), "{line}");
            assert!(!line.starts_with("+++ "), "{line}");
            assert!(!line.starts_with("@@"), "{line}");
        }
    }

    /// 只改 rename 的 patch 没有 hunk → 走元信息预览。
    #[test]
    fn rename_only_patches_preview_their_metadata() {
        let patch = "\
diff --git a/old.txt b/new.txt
similarity index 100%
rename from old.txt
rename to new.txt";
        let lines = get_plain_text_patch_fallback_lines(patch, &PierrePatchProbe::default())
            .expect("rename-only 必须降级");
        assert_eq!(
            lines,
            owned(&["similarity index 100%", "rename from old.txt", "rename to new.txt"])
        );
    }

    /// 文件类型不是 text 的单文件小 patch 可以交给富渲染器。
    #[test]
    fn non_text_single_file_patches_are_left_to_the_rich_viewer() {
        let patch = "\
--- a/main.rs
+++ b/main.rs
@@ -1,2 +1,2 @@
 fn main() {}
-// old
+// new";
        let probe = PierrePatchProbe {
            single_file_diff: true,
            file_type: "rust".to_string(),
        };
        assert_eq!(get_plain_text_patch_fallback_lines(patch, &probe), None);
    }

    /// 完全不符合 patch 结构的文本 → 降级成「原样展示」。
    ///
    /// 走的是「只有元信息、没有 hunk 头」这条分支：所有非空且不带协议前缀的行
    /// 都算元信息，于是原文被原样搬进预览（比丢掉内容更有用）。
    #[test]
    fn unrecognizable_input_degrades_to_the_raw_text() {
        let lines =
            get_plain_text_patch_fallback_lines("完全看不懂的输入", &PierrePatchProbe::default())
                .expect("必须降级");
        assert_eq!(lines, owned(&["完全看不懂的输入"]));
    }

    /// 空输入降级成一条空串（`normalize` 保证永不为空列表）。
    #[test]
    fn empty_input_degrades_to_a_single_blank_line() {
        let lines = get_plain_text_patch_fallback_lines("", &PierrePatchProbe::default())
            .expect("必须降级");
        assert_eq!(lines, owned(&[""]));
    }
}
