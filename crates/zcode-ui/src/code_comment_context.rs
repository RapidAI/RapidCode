//! 代码批注的上下文构造 —— Rust 移植自
//! `packages/ui/src/lib/codeCommentContext.ts`
//!
//! 这个模块回答一个问题：**用户在 diff / 代码上划的那一条批注，最后怎么进到
//! 提示词里**。它不碰 DOM、不碰 store，全是纯函数，因此可以逐字符验证。
//!
//! ## 关键移植点 1：批注永远是「右侧」（`Side: R`）
//!
//! [`build_code_comment_markdown`] 里 `Side` 是**硬编码的 `R`**，不是从数据里取的。
//! 理由：批注只允许落在「改动后」的那一侧 —— 用户看到的就是右侧内容，
//! 划的是右侧的行号。左侧（改动前）的行号在 diff 里根本不可交互。
//! 所以这不是「还没实现」，而是**刻意不给选择**。
//!
//! ## 关键移植点 2：`Side` 只写不读
//!
//! [`parse_prompt_code_comments`] 解析时**完全不看 `Side` 字段** ——
//! 行号已经是右侧的行号，再读一遍 `Side` 也不会改变什么。
//! 换句话说 `Side: R` 是给**人或模型**看的冗余信息，不是解析依据。
//!
//! ## 关键移植点 3：块级正则绝不能用多行 `$`
//!
//! 见上游那段注释。这里 `m` 标志**开了但无害**：`([\s\S]*?)\s*$` 里
//! `[\s\S]` 能吃换行，`$` 在贪婪回溯下最终匹配到**整个字符串末尾**，
//! 所以批注正文里的空行、`## Comment` 字样都不会被截断。
//! 实测确认：`Comment: a\n\nb` 解析出的是 `a\n\nb` 而不是 `a`。
//!
//! ## 关键移植点 4：条目内部的正则要**宽松捕获到末尾**
//!
//! [`parse_code_comment_item`] 的 `Comment:` 之后用 `([\s\S]*)$` 一路吃到尾，
//! **不能**用非贪婪 + 行尾锚点 —— 那样多行批注会在第一个换行处被切断，
//! 用户写的第三行、第四行会静默丢失。
//!
//! ## 与 TS 的差异
//!
//! 1. **没有 `window.dispatchEvent` / `CustomEvent`**：那三个 `dispatch*` 与
//!    `is*Event` 守卫是浏览器事件总线，属于宿主职责。本移植保留
//!    *数据形状*与*校验函数*（[`is_code_comment_payload`] /
//!    [`is_code_comment_remove_payload`]），由调用方自行决定怎么送事件。
//! 2. **「已移除」集合改为显式参数**：TS 用模块级 `Set` 记住被移除的批注 id，
//!    本移植把它做成 [`CodeCommentRemovalRegistry`]，否则多次测试之间会串味。
//! 3. **正则手写**：本 crate 是零外部依赖，因此上面那几个 JS 正则全部改写成
//!    手写扫描 + `splitn`。**语义逐条对齐**（见各函数注释里的实测表）。

use std::collections::HashSet;

/// 「批注改动前/后哪一侧」的取值。目前只可能产出 `Right`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeCommentSide {
    /// `Side: L`
    Left,
    /// `Side: R`
    Right,
}

impl CodeCommentSide {
    /// 写进 markdown 的那个字母。
    pub fn as_str(&self) -> &'static str {
        match self {
            CodeCommentSide::Left => "L",
            CodeCommentSide::Right => "R",
        }
    }

    /// 解析 —— 但注意**没有任何调用方在用**（见模块移植点 2）。
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "L" | "l" => Some(CodeCommentSide::Left),
            "R" | "r" => Some(CodeCommentSide::Right),
            _ => None,
        }
    }
}

/// 批注锚定的行区间 —— **直接复用** [`crate::components::code_viewer::CodeCommentRange`]。
///
/// TS 里 `codeCommentContext.ts` 与 `code-viewer.tsx` 各自声明了一个同形接口，
/// 但 TS 是**结构类型**，两个声明在类型上完全互通；Rust 的具名类型不互通，
/// 所以这里 `pub use` 一份而不是再定义一遍 —— 否则 `CodeViewer` 产出的区间
/// 还要手工搬进本模块的类型里。
pub use crate::components::code_viewer::CodeCommentRange;

/// 一条批注的完整载荷（TS `CodeCommentPayload`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeCommentPayload {
    pub id: Option<String>,
    pub workspace_path: String,
    pub workspace_identity: Option<String>,
    pub source_path: Option<String>,
    pub source_title: String,
    pub selected_text: String,
    pub comment: String,
    pub context_label: Option<String>,
    pub comment_label: Option<String>,
    pub start_line: i64,
    pub end_line: i64,
}

/// 已经挂进输入框的那条批注（TS `CodeCommentComposerAttachment`）。
///
/// 与 [`CodeCommentPayload`] 的区别只有一点：`id` **一定有**。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeCommentComposerAttachment {
    pub id: String,
    pub workspace_path: String,
    pub workspace_identity: Option<String>,
    pub source_path: Option<String>,
    pub source_title: String,
    pub selected_text: String,
    pub comment: String,
    pub start_line: i64,
    pub end_line: i64,
}

/// 代码查看面板里的本地预览 —— **直接复用**
/// [`crate::components::code_viewer::CodeCommentPreview`]（同一个上游接口）。
///
/// 注意它**没有** `workspace_path`：预览挂在「workspace + 文件」那个桶里，
/// 桶 key 已经承载了 workspace 信息。
pub use crate::components::code_viewer::CodeCommentPreview as CodeCommentPreviewEntry;

/// 要移除的批注（TS `CodeCommentRemovePayload`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeCommentRemoveTarget {
    pub id: String,
    pub workspace_path: String,
    pub workspace_identity: Option<String>,
}

// ---- workspace key ----

/// `getCodeCommentWorkspaceKey`：`workspaceIdentity` **优先于** `workspacePath`。
///
/// 同一份仓库被不同路径打开（软链、大小写、挂载点不同）时，
/// 只有 identity 能证明「这是同一个工作区」。identity 是空白串时退回路径。
pub fn get_code_comment_workspace_key(
    workspace_path: &str,
    workspace_identity: Option<&str>,
) -> String {
    match workspace_identity {
        Some(identity) if !identity.trim().is_empty() => identity.trim().to_string(),
        _ => workspace_path.to_string(),
    }
}

/// `getCodeCommentRemovalKey`：`<workspaceKey>\0<id>`。
pub fn get_code_comment_removal_key(target: &CodeCommentRemoveTarget) -> String {
    format!(
        "{}\0{}",
        get_code_comment_workspace_key(&target.workspace_path, target.workspace_identity.as_deref()),
        target.id
    )
}

/// TS 里那个模块级 `Set<string>`（见模块差异 2）。
#[derive(Clone, Debug, Default)]
pub struct CodeCommentRemovalRegistry {
    keys: HashSet<String>,
}

impl CodeCommentRemovalRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_marked_removed(&self, target: &CodeCommentRemoveTarget) -> bool {
        self.keys.contains(&get_code_comment_removal_key(target))
    }

    pub fn mark_removed(&mut self, target: &CodeCommentRemoveTarget) {
        self.keys.insert(get_code_comment_removal_key(target));
    }

    pub fn unmark_removed(&mut self, target: &CodeCommentRemoveTarget) {
        self.keys.remove(&get_code_comment_removal_key(target));
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

// ---- 校验 ----

/// `isCodeCommentRemovePayload`：`id` 与 `workspacePath` 都必须是**非空串**。
pub fn is_code_comment_remove_payload(target: &CodeCommentRemoveTarget) -> bool {
    !target.id.is_empty() && !target.workspace_path.is_empty()
}

/// `isCodeCommentPayload`。两个刻意的宽松点：
///
/// - `comment` 允许是**空串** —— 只划范围不写字是合法用法；
/// - `source_path` 却必须**非空** —— 没有文件路径的批注无法定位，是无意义的。
pub fn is_code_comment_payload(payload: &CodeCommentPayload) -> bool {
    !payload.workspace_path.is_empty()
        && payload
            .workspace_identity
            .as_ref()
            .is_none_or(|identity| !identity.is_empty())
        && payload
            .source_path
            .as_ref()
            .is_some_and(|path| !path.is_empty())
        && !payload.source_title.is_empty()
        && payload.start_line.is_finite()
        && payload.end_line.is_finite()
}

/// 一个 `f64` 算不算有限（对应 JS 的 `Number.isFinite`）。
trait FiniteExt {
    fn is_finite(&self) -> bool;
}

impl FiniteExt for i64 {
    /// `i64` 天然有限 —— 存在这个 trait 只是为了让上面的校验读起来跟 TS 一致。
    fn is_finite(&self) -> bool {
        true
    }
}

// ---- 构造 ----

fn build_line_label(payload: &CodeCommentPayload) -> String {
    if payload.start_line == payload.end_line {
        payload.start_line.to_string()
    } else {
        format!("{}-{}", payload.start_line, payload.end_line)
    }
}

/// 单条批注的 markdown 片段。
///
/// `Side` 恒为 `R`（见模块移植点 1）。
pub fn build_code_comment_markdown(payload: &CodeCommentPayload) -> String {
    let file = payload
        .source_path
        .as_deref()
        .unwrap_or(&payload.source_title);
    format!(
        "## Comment\nFile: {file}\nSide: R\nLines: {}\nSelected text:\n```\n{}\n```\nComment:\n{}\n",
        build_line_label(payload),
        payload.selected_text.trim(),
        payload.comment.trim()
    )
}

/// `# Code comments:` 整块。**空列表返回空串**（不是标题 + 空体）。
pub fn build_code_comments_block(attachments: &[CodeCommentComposerAttachment]) -> String {
    if attachments.is_empty() {
        return String::new();
    }
    let items: Vec<String> = attachments
        .iter()
        .enumerate()
        .map(|(index, attachment)| {
            let payload = CodeCommentPayload {
                id: Some(attachment.id.clone()),
                workspace_path: attachment.workspace_path.clone(),
                workspace_identity: attachment.workspace_identity.clone(),
                source_path: attachment.source_path.clone(),
                source_title: attachment.source_title.clone(),
                selected_text: attachment.selected_text.clone(),
                comment: attachment.comment.clone(),
                context_label: None,
                comment_label: None,
                start_line: attachment.start_line,
                end_line: attachment.end_line,
            };
            // 标题编号从 1 开始，且**只替换第一个** `## Comment`。
            build_code_comment_markdown(&payload).replacen("## Comment", &format!("## Comment {}", index + 1), 1)
        })
        .collect();
    format!("# Code comments:\n\n{}", items.join("\n"))
}

/// `buildPromptWithCodeComments`：正文 + 批注块。
///
/// 没有批注时返回**去掉尾部空白的正文**（`trim`），有批注时用**恰好两个换行**分隔。
pub fn build_prompt_with_code_comments(
    text: &str,
    attachments: &[CodeCommentComposerAttachment],
) -> String {
    let content = text.trim_end();
    let comments_block = build_code_comments_block(attachments);
    if comments_block.is_empty() {
        return content.trim().to_string();
    }
    let separator = if content.is_empty() { "" } else { "\n\n" };
    format!("{content}{separator}{comments_block}").trim().to_string()
}

// ---- 解析 ----

/// `getSourceTitle`：路径的最后一段。`a/b/c.ts` → `c.ts`。
pub fn get_source_title(path: &str) -> String {
    let last = path
        .split(['\\', '/'])
        .filter(|segment| !segment.is_empty())
        .next_back();
    last.unwrap_or(path).to_string()
}

/// `parseLineRange`。实测语义（逐条对齐上游正则）：
///
/// | 输入 | 结果 |
/// |---|---|
/// | `12` / `L12` / `l12` / ` 12 ` / `012` | `12-12` |
/// | `12-15` / `12 - L15` / `L12-L15` | `12-15` |
/// | `12-` / `-15` / `12abc` / `12-15-20` | `None` |
///
/// 要点：开头的一个 `L`（大小写不限）只在**整串最前面**被剥掉；
/// 两端都要锚死（所以 `12abc` 不算）；分隔符两侧允许任意空白。
pub fn parse_line_range(value: &str) -> Option<CodeCommentRange> {
    let trimmed = value.trim();
    let normalized = trimmed
        .strip_prefix('L')
        .or_else(|| trimmed.strip_prefix('l'))
        .unwrap_or(trimmed);
    let normalized = normalized.trim();

    let (start_text, end_text) = match normalized.find('-') {
        Some(index) => {
            let start = &normalized[..index];
            let mut end = &normalized[index + 1..];
            // 末尾可能还有一个 `L`（`12 - L15` / `L12-L15`）
            end = end.trim();
            if let Some(stripped) = end.strip_prefix('L').or_else(|| end.strip_prefix('l')) {
                end = stripped;
            }
            (start.trim(), end.trim())
        }
        None => (normalized, normalized),
    };

    let start_line = parse_all_digits(start_text)?;
    let end_line = if end_text == start_text {
        start_line
    } else {
        parse_all_digits(end_text)?
    };

    Some(CodeCommentRange {
        start_line,
        end_line,
    })
}

/// 整串必须全是十进制数字（不能有尾随垃圾），对应 `^(\d+)$`。
fn parse_all_digits(value: &str) -> Option<i64> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse::<i64>().ok()
}

/// 解析结果（TS `ParsedCodeCommentPrompt`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedCodeCommentPrompt {
    pub visible_content: String,
    pub code_comment_attachments: Vec<CodeCommentComposerAttachment>,
}

/// 在正文里找到 `# Code comments:` 块并切出来。
///
/// 返回 `(块起点, 块体)`。块必须出现在**行首**（前面是串首或一个空行），
/// 且标题行后必须紧跟一个空行 —— 所以正文里的 `hello # Code comments: nope`
/// 不会被误认。
///
/// 块起点取的是那个前导空行的**第一个** `\n`（实测 `idx=5`），
/// 于是 `visible_content` 正好不含多余的空白行。
fn split_code_comments_block(content: &str) -> Option<(usize, String)> {
    const HEADER: &str = "# Code comments:";
    let mut search_from = 0;
    while let Some(relative) = content[search_from..].find(HEADER) {
        let index = search_from + relative;
        // 必须在行首：要么是串首，要么前面紧跟着 `\n`（且该 `\n` 之前是
        // 串首或另一个 `\n`，即一个完整的空行）。
        let block_start = if index == 0 {
            Some(0)
        } else if content[..index].ends_with("\n\n") {
            Some(index - 1)
        } else {
            None
        };
        if let Some(block_start) = block_start {
            let after_header = &content[index + HEADER.len()..];
            // `\s*` 允许标题行尾有空白，然后必须跨过**两个**换行（即一个空行）。
            if let Some(body) = strip_blank_line_after_header(after_header) {
                return Some((block_start, body.trim_end().to_string()));
            }
        }
        search_from = index + HEADER.len();
    }
    None
}

/// 标题行之后要求：`\s*` + `\n` + `\n`，返回其后的正文。
fn strip_blank_line_after_header(rest: &str) -> Option<&str> {
    let trimmed_start = rest.trim_start_matches([' ', '\t', '\r']);
    let after_newline = trimmed_start.strip_prefix('\n')?;
    let mut chars = after_newline.char_indices();
    let (index, ch) = chars.next()?;
    if ch != '\n' {
        // 允许标题行与空行之间夹空白
        let remainder = after_newline.trim_start_matches([' ', '\t', '\r']);
        return remainder.strip_prefix('\n');
    }
    Some(&after_newline[index + ch.len_utf8()..])
}

/// 按 `\n## Comment` / `\n## Comment N` 切分条目。
fn split_comment_items(raw_block: &str) -> Vec<String> {
    let mut items: Vec<String> = Vec::new();
    let mut current_start: Option<usize> = None;
    let mut cursor = 0;

    while let Some(relative) = raw_block[cursor..].find("## Comment") {
        let index = cursor + relative;
        // 必须是行首（串首或前面是 `\n`）。
        let at_line_start = index == 0 || raw_block.as_bytes()[index - 1] == b'\n';
        if at_line_start && is_comment_heading(&raw_block[index..]) {
            if let Some(start) = current_start {
                items.push(raw_block[start..index.saturating_sub(1).max(start)].trim().to_string());
            }
            current_start = Some(index);
        }
        cursor = index + "## Comment".len();
    }
    if let Some(start) = current_start {
        items.push(raw_block[start..].trim().to_string());
    }
    items.into_iter().filter(|item| !item.is_empty()).collect()
}

/// `## Comment` / `## Comment 1` 后面必须是换行（对应 `## Comment(?:\s+\d+)?\n`）。
fn is_comment_heading(text: &str) -> bool {
    let rest = match text.strip_prefix("## Comment") {
        Some(rest) => rest,
        None => return false,
    };
    let rest = rest.trim_start_matches([' ', '\t']);
    let rest = if rest == "\n" || rest.starts_with('\n') {
        return true;
    } else {
        // 可能是编号：吃掉所有数字
        let digits_end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        if digits_end == 0 {
            return false;
        }
        &rest[digits_end..]
    };
    rest.starts_with('\n')
}

/// 单条目解析。三条字段锚（`File:` / `Lines:` / `Selected text:`+`Comment:`）
/// 都必须存在，否则整条丢弃（返回 `None`）。
fn parse_code_comment_item(
    raw_item: &str,
    index: usize,
    workspace_path: &str,
    workspace_identity: Option<&str>,
) -> Option<CodeCommentComposerAttachment> {
    let file = extract_anchored_value(raw_item, "File:")?;
    let lines = extract_anchored_value(raw_item, "Lines:")?;
    let (selected_text, raw_comment) = extract_selected_and_comment(raw_item)?;

    if file.is_empty() || lines.is_empty() {
        return None;
    }
    let range = parse_line_range(lines)?;
    let source_path = file.trim().to_string();

    Some(CodeCommentComposerAttachment {
        id: format!(
            "parsed-code-comment-{}-{}-{}",
            index + 1,
            range.start_line,
            range.end_line
        ),
        workspace_path: workspace_path.to_string(),
        workspace_identity: workspace_identity.map(str::to_string),
        source_title: get_source_title(&source_path),
        source_path: Some(source_path),
        start_line: range.start_line,
        end_line: range.end_line,
        selected_text: selected_text.trim().to_string(),
        comment: raw_comment.trim().to_string(),
    })
}

/// 取 `Key: value` 形式的一整行值（对应 `/^Key:\s*(.+)$/m`）。
fn extract_anchored_value<'a>(raw_item: &'a str, key: &str) -> Option<&'a str> {
    for line in raw_item.split('\n') {
        if let Some(rest) = line.strip_prefix(key) {
            let value = rest.trim_start_matches([' ', '\t']).trim_end_matches('\r');
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

/// 同时取「选中代码」与「批注正文」。
///
/// 结构固定为（外面用四个反引号，因为内容自己含三个反引号）：
///
/// ````text
/// Selected text:
/// ```
/// <选中内容>
/// ```
/// Comment:
/// <批注正文，可空、可多行>
/// ````
///
/// 匹配不上返回 `None`（整条丢弃）。
///
/// **批注正文一路吃到字符串末尾**（见模块移植点 4）—— 用非贪婪会在第一个
/// 换行处切断，用户写的后续行会静默丢掉。
fn extract_selected_and_comment(raw_item: &str) -> Option<(&str, &str)> {
    let selected_start = raw_item.find("Selected text:")?;
    let after_selected = &raw_item[selected_start + "Selected text:".len()..];
    // `\s*` 允许冒号后跟空白，然后必须是 `\n` + 围栏
    let after_selected = after_selected.trim_start_matches([' ', '\t']);
    let after_selected = after_selected.strip_prefix('\n')?;
    // 围栏：``` 后可有任意（不含换行与反引号）的语言标记，再换行
    let after_fence = after_selected.strip_prefix("```")?;
    let after_fence = match after_fence.find('\n') {
        Some(index) => {
            let marker = &after_fence[..index];
            if marker.contains('`') {
                return None;
            }
            &after_fence[index + 1..]
        }
        None => return None,
    };
    // 非贪婪：找到第一个 `\n````
    let closing = after_fence.find("\n```")?;
    let selected_text = &after_fence[..closing];
    let after_closing = &after_fence[closing + "\n```".len()..];
    // `\s*` + `\n` + `Comment:` + `\s*` + 可选 `\n`
    let after_closing = after_closing.trim_start_matches([' ', '\t', '\r', '\n']);
    let after_comment_key = after_closing.strip_prefix("Comment:")?;
    let after_comment_key = after_comment_key.trim_start_matches([' ', '\t', '\r']);
    let comment = after_comment_key.strip_prefix('\n').unwrap_or(after_comment_key);
    Some((selected_text, comment))
}

/// `parsePromptCodeComments`：把正文拆成「可见正文 + 批注附件」。
///
/// 解析不出任何一条批注时**原样返回**（`visible_content` 是完整输入）——
/// 不因为看到一个像模像样的标题就把用户正文切掉。
pub fn parse_prompt_code_comments(
    content: &str,
    workspace_path: &str,
    workspace_identity: Option<&str>,
) -> ParsedCodeCommentPrompt {
    let unchanged = || ParsedCodeCommentPrompt {
        visible_content: content.to_string(),
        code_comment_attachments: Vec::new(),
    };

    let Some((block_start, raw_block)) = split_code_comments_block(content) else {
        return unchanged();
    };
    if raw_block.is_empty() {
        return unchanged();
    }

    let code_comment_attachments: Vec<CodeCommentComposerAttachment> =
        split_comment_items(&raw_block)
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                parse_code_comment_item(item, index, workspace_path, workspace_identity)
            })
            .collect();

    if code_comment_attachments.is_empty() {
        return unchanged();
    }

    ParsedCodeCommentPrompt {
        visible_content: content[..block_start].trim_end().to_string(),
        code_comment_attachments,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attachment(id: &str, path: &str, title: &str, start: i64, end: i64, text: &str, comment: &str) -> CodeCommentComposerAttachment {
        CodeCommentComposerAttachment {
            id: id.to_string(),
            workspace_path: "F:/repo".to_string(),
            workspace_identity: None,
            source_path: Some(path.to_string()),
            source_title: title.to_string(),
            selected_text: text.to_string(),
            comment: comment.to_string(),
            start_line: start,
            end_line: end,
        }
    }

    fn payload(path: &str, start: i64, end: i64) -> CodeCommentPayload {
        CodeCommentPayload {
            id: Some("c1".to_string()),
            workspace_path: "F:/repo".to_string(),
            workspace_identity: None,
            source_path: Some(path.to_string()),
            source_title: get_source_title(path),
            selected_text: "let a = 1;".to_string(),
            comment: "这里为什么要这样写".to_string(),
            context_label: None,
            comment_label: None,
            start_line: start,
            end_line: end,
        }
    }

    // ---- workspace key ----

    #[test]
    fn workspace_identity_wins_over_the_path() {
        assert_eq!(
            get_code_comment_workspace_key("F:/repo", Some("git:abc123")),
            "git:abc123"
        );
    }

    #[test]
    fn blank_identity_falls_back_to_the_path() {
        // 空白串要当成「没给」，否则同一个工作区会分裂成两个桶
        for identity in [None, Some(""), Some("   ")] {
            assert_eq!(get_code_comment_workspace_key("F:/repo", identity), "F:/repo");
        }
    }

    #[test]
    fn identity_is_trimmed_before_use() {
        assert_eq!(
            get_code_comment_workspace_key("F:/repo", Some("  git:abc  ")),
            "git:abc"
        );
    }

    #[test]
    fn removal_key_joins_with_a_nul_separator() {
        let target = CodeCommentRemoveTarget {
            id: "c1".to_string(),
            workspace_path: "F:/repo".to_string(),
            workspace_identity: None,
        };
        // 用 \0 而不是 `/`：id 或路径里都可能含 `/`，只有 \0 不会撞
        assert_eq!(get_code_comment_removal_key(&target), "F:/repo\0c1");
    }

    #[test]
    fn removal_registry_round_trips() {
        let target = CodeCommentRemoveTarget {
            id: "c1".to_string(),
            workspace_path: "F:/repo".to_string(),
            workspace_identity: Some("git:abc".to_string()),
        };
        let mut registry = CodeCommentRemovalRegistry::new();
        assert!(!registry.is_marked_removed(&target));
        registry.mark_removed(&target);
        assert!(registry.is_marked_removed(&target));
        registry.unmark_removed(&target);
        assert!(!registry.is_marked_removed(&target));
        assert!(registry.is_empty());
    }

    #[test]
    fn the_same_id_in_another_workspace_is_not_marked() {
        let mut registry = CodeCommentRemovalRegistry::new();
        registry.mark_removed(&CodeCommentRemoveTarget {
            id: "c1".to_string(),
            workspace_path: "F:/repo-a".to_string(),
            workspace_identity: None,
        });
        assert!(registry.is_marked_removed(&CodeCommentRemoveTarget {
            id: "c1".to_string(),
            workspace_path: "F:/repo-a".to_string(),
            workspace_identity: None,
        }));
        // 同一个 id 换个 workspace 就是**另一条**批注
        assert!(!registry.is_marked_removed(&CodeCommentRemoveTarget {
            id: "c1".to_string(),
            workspace_path: "F:/repo-b".to_string(),
            workspace_identity: None,
        }));
    }

    // ---- 校验 ----

    #[test]
    fn a_payload_needs_a_path_but_allows_an_empty_comment() {
        let mut value = payload("src/a.rs", 1, 1);
        assert!(is_code_comment_payload(&value));
        // 只划范围不写字是合法用法
        value.comment = String::new();
        assert!(is_code_comment_payload(&value));
        // 没有文件路径就无法定位
        value.source_path = None;
        assert!(!is_code_comment_payload(&value));
    }

    #[test]
    fn a_remove_target_needs_both_id_and_path() {
        let base = CodeCommentRemoveTarget {
            id: "c1".to_string(),
            workspace_path: "F:/repo".to_string(),
            workspace_identity: None,
        };
        assert!(is_code_comment_remove_payload(&base));
        assert!(!is_code_comment_remove_payload(&CodeCommentRemoveTarget {
            id: String::new(),
            ..base.clone()
        }));
        assert!(!is_code_comment_remove_payload(&CodeCommentRemoveTarget {
            workspace_path: String::new(),
            ..base
        }));
    }

    // ---- markdown 构造 ----

    #[test]
    fn markdown_always_declares_the_right_side() {
        // `Side: R` 是硬编码的（见模块移植点 1）—— 批注只落在「改动后」那侧
        let markdown = build_code_comment_markdown(&payload("src/a.rs", 3, 3));
        assert!(markdown.starts_with("## Comment\nFile: src/a.rs\nSide: R\nLines: 3\n"));
        assert!(markdown.contains("Selected text:\n```\nlet a = 1;\n```\n"));
        assert!(markdown.ends_with("Comment:\n这里为什么要这样写\n"));
    }

    #[test]
    fn markdown_writes_a_range_label_when_the_lines_differ() {
        let markdown = build_code_comment_markdown(&payload("src/a.rs", 3, 7));
        assert!(markdown.contains("\nLines: 3-7\n"), "实际：{markdown}");
    }

    #[test]
    fn markdown_falls_back_to_the_title_when_there_is_no_path() {
        // 没有 sourcePath 时用 sourceTitle 顶上，不能写出 `File: `（空值）
        let mut value = payload("src/a.rs", 1, 1);
        value.source_path = None;
        value.source_title = "a.rs".to_string();
        let markdown = build_code_comment_markdown(&value);
        assert!(markdown.contains("\nFile: a.rs\n"), "实际：{markdown}");
    }

    #[test]
    fn an_empty_attachment_list_produces_an_empty_block() {
        // 空列表要返回空串，而不是「只有标题没有内容」
        assert_eq!(build_code_comments_block(&[]), "");
    }

    #[test]
    fn the_comments_block_numbers_every_entry_from_one() {
        let block = build_code_comments_block(&[
            attachment("c1", "a.rs", "a.rs", 1, 1, "x", "第一条"),
            attachment("c2", "b.rs", "b.rs", 2, 3, "y", "第二条"),
        ]);
        assert!(block.starts_with("# Code comments:\n\n## Comment 1\n"));
        assert!(block.contains("## Comment 2\n"), "实际：{block}");
        // 编号只替换**第一个** `## Comment`：正文里再出现这个字样不能被改
        assert_eq!(block.matches("## Comment 1").count(), 1);
    }

    #[test]
    fn a_comment_body_containing_the_heading_is_not_renumbered() {
        let block = build_code_comments_block(&[attachment(
            "c1", "a.rs", "a.rs", 1, 1, "x", "## Comment\n这是正文里的一行",
        )]);
        // 标题被编成 1，正文里那行保持原样
        assert!(block.starts_with("# Code comments:\n\n## Comment 1\n"));
        assert!(block.contains("这是正文里的一行"));
    }

    // ---- prompt 拼装 ----

    #[test]
    fn a_prompt_without_comments_is_just_trimmed_text() {
        assert_eq!(build_prompt_with_code_comments("  帮我看看  \n\n", &[]), "帮我看看");
    }

    #[test]
    fn a_prompt_with_comments_separates_them_with_one_blank_line() {
        let prompt = build_prompt_with_code_comments("帮我看看", &[
            attachment("c1", "a.rs", "a.rs", 1, 1, "x", "为什么"),
        ]);
        assert!(prompt.starts_with("帮我看看\n\n# Code comments:\n\n"));
    }

    #[test]
    fn an_empty_prompt_does_not_leave_a_leading_blank_line() {
        // 只贴批注不打字时，不能变成「\n\n# Code comments:」
        let prompt = build_prompt_with_code_comments("   ", &[
            attachment("c1", "a.rs", "a.rs", 1, 1, "x", "为什么"),
        ]);
        assert!(prompt.starts_with("# Code comments:"), "实际：{prompt:?}");
    }

    // ---- 行号解析（逐条对齐上游正则，见函数文档里的实测表） ----

    #[test]
    fn a_bare_number_is_a_single_line_range() {
        for value in ["12", " 12 ", "012"] {
            assert_eq!(
                parse_line_range(value),
                Some(CodeCommentRange { start_line: 12, end_line: 12 }),
                "输入 {value:?}"
            );
        }
    }

    #[test]
    fn a_leading_l_is_stripped_once() {
        for value in ["L12", "l12"] {
            assert_eq!(
                parse_line_range(value),
                Some(CodeCommentRange { start_line: 12, end_line: 12 }),
                "输入 {value:?}"
            );
        }
    }

    #[test]
    fn ranges_accept_whitespace_and_an_l_on_either_end() {
        for value in ["12-15", "12 - L15", "L12-L15", "12 - 15"] {
            assert_eq!(
                parse_line_range(value),
                Some(CodeCommentRange { start_line: 12, end_line: 15 }),
                "输入 {value:?}"
            );
        }
    }

    #[test]
    fn malformed_ranges_are_rejected_whole() {
        // 两端都要锚死 —— 不能只解析出前面那段数字就算成功
        for value in ["12-", "-15", "12abc", "12-15-20", "", "abc", "L"] {
            assert_eq!(parse_line_range(value), None, "输入 {value:?} 本应被拒");
        }
    }

    // ---- 整段解析 ----

    #[test]
    fn a_generated_prompt_round_trips_back_into_attachments() {
        // 这一条是整条链路的核心：写出去的能被自己读回来
        let attachments = vec![
            attachment("code-comment:10:F:/repo/src/a.rs", "F:/repo/src/a.rs", "a.rs", 3, 3, "let a = 1;", "为什么用 i32"),
            attachment("code-comment:80:F:/repo/src/b.rs", "F:/repo/src/b.rs", "b.rs", 5, 6, "fn f() {\n}", "这里要不要加锁"),
        ];
        let prompt = build_prompt_with_code_comments("看一下这两处", &attachments);
        let parsed = parse_prompt_code_comments(&prompt, "F:/repo", None);

        assert_eq!(parsed.visible_content, "看一下这两处");
        assert_eq!(parsed.code_comment_attachments.len(), 2);

        let first = &parsed.code_comment_attachments[0];
        assert_eq!(first.source_path.as_deref(), Some("F:/repo/src/a.rs"));
        assert_eq!(first.source_title, "a.rs");
        assert_eq!(first.start_line, 3);
        assert_eq!(first.end_line, 3);
        assert_eq!(first.selected_text, "let a = 1;");
        assert_eq!(first.comment, "为什么用 i32");

        let second = &parsed.code_comment_attachments[1];
        assert_eq!(second.start_line, 5);
        assert_eq!(second.end_line, 6);
        assert_eq!(second.selected_text, "fn f() {\n}");
        assert_eq!(second.comment, "这里要不要加锁");
    }

    #[test]
    fn a_multiline_comment_survives_the_round_trip() {
        // 非贪婪解析会在第一个换行处切断，把后两行静默丢掉
        let attachments = vec![attachment(
            "c1", "a.rs", "a.rs", 1, 1, "x", "第一行\n\n第三行\n第四行",
        )];
        let parsed = parse_prompt_code_comments(
            &build_prompt_with_code_comments("正文", &attachments),
            "F:/repo",
            None,
        );
        assert_eq!(parsed.code_comment_attachments.len(), 1);
        assert_eq!(
            parsed.code_comment_attachments[0].comment,
            "第一行\n\n第三行\n第四行"
        );
    }

    #[test]
    fn a_missing_range_makes_the_whole_item_drop_out() {
        // 没有 Lines 的行号就无法定位，宁可整条丢弃也不要产出错行号的批注
        let malformed = "正文\n\n# Code comments:\n\n## Comment 1\nFile: a.rs\nSide: R\nLines: 12-\nSelected text:\n```\nx\n```\nComment:\nhi\n";
        let parsed = parse_prompt_code_comments(malformed, "F:/repo", None);
        assert!(parsed.code_comment_attachments.is_empty());
        // 一条都解析不出来时正文**原样保留**，不能被切掉
        assert_eq!(parsed.visible_content, malformed);
    }

    #[test]
    fn plain_text_that_merely_mentions_the_header_is_left_alone() {
        let content = "你可以用 `# Code comments:` 这个标记，但这里不是真的块";
        let parsed = parse_prompt_code_comments(content, "F:/repo", None);
        assert_eq!(parsed.visible_content, content);
        assert!(parsed.code_comment_attachments.is_empty());
    }

    #[test]
    fn the_header_must_be_at_a_line_start() {
        let content = "正文开头 # Code comments:\n\n## Comment 1\nFile: a.rs\nLines: 1\nSelected text:\n```\nx\n```\nComment:\nhi\n";
        let parsed = parse_prompt_code_comments(content, "F:/repo", None);
        assert!(parsed.code_comment_attachments.is_empty(), "行中间的标题不算块");
        assert_eq!(parsed.visible_content, content);
    }

    #[test]
    fn a_parsed_attachment_carries_the_workspace_it_was_parsed_for() {
        let attachments = vec![attachment("c1", "a.rs", "a.rs", 1, 1, "x", "hi")];
        let parsed = parse_prompt_code_comments(
            &build_prompt_with_code_comments("t", &attachments),
            "F:/repo",
            Some("git:abc"),
        );
        let item = &parsed.code_comment_attachments[0];
        assert_eq!(item.workspace_path, "F:/repo");
        assert_eq!(item.workspace_identity.as_deref(), Some("git:abc"));
    }

    #[test]
    fn parsed_ids_are_synthesised_from_the_position_and_range() {
        let attachments = vec![
            attachment("c1", "a.rs", "a.rs", 3, 3, "x", "one"),
            attachment("c2", "b.rs", "b.rs", 5, 6, "y", "two"),
        ];
        let parsed = parse_prompt_code_comments(
            &build_prompt_with_code_comments("t", &attachments),
            "F:/repo",
            None,
        );
        // 解析出来的 id 不复用原 id —— 它是「第几条 + 区间」，用于在重新渲染时保持稳定
        assert_eq!(parsed.code_comment_attachments[0].id, "parsed-code-comment-1-3-3");
        assert_eq!(parsed.code_comment_attachments[1].id, "parsed-code-comment-2-5-6");
    }

    #[test]
    fn an_empty_comment_body_still_parses() {
        // 「只划范围不写字」在构造侧是合法的，解析侧也要能读回来
        let attachments = vec![attachment("c1", "a.rs", "a.rs", 2, 2, "x", "")];
        let parsed = parse_prompt_code_comments(
            &build_prompt_with_code_comments("t", &attachments),
            "F:/repo",
            None,
        );
        assert_eq!(parsed.code_comment_attachments.len(), 1);
        assert_eq!(parsed.code_comment_attachments[0].comment, "");
    }

    // ---- 帮助函数 ----

    #[test]
    fn the_source_title_is_the_last_path_segment() {
        assert_eq!(get_source_title("src/a/b.ts"), "b.ts");
        assert_eq!(get_source_title("src\\a\\b.ts"), "b.ts");
        assert_eq!(get_source_title("b.ts"), "b.ts");
        // 尾随分隔符不能产出空标题
        assert_eq!(get_source_title("src/a/"), "a");
        assert_eq!(get_source_title(""), "");
    }

    #[test]
    fn the_right_side_is_the_only_thing_we_write() {
        assert_eq!(CodeCommentSide::Right.as_str(), "R");
        assert_eq!(CodeCommentSide::parse("R"), Some(CodeCommentSide::Right));
        assert_eq!(CodeCommentSide::parse("l"), Some(CodeCommentSide::Left));
        assert_eq!(CodeCommentSide::parse("x"), None);
    }

    /// 端到端往返：**拼出来**的 prompt 必须能被**解析回来**。
    ///
    /// 这是「批注真的传到了助手那边」唯一可信的证据 —— 两侧共用同一套格式
    /// 定义，一旦哪边改了格式而没有同步，这条测试会先红。
    #[test]
    fn a_composed_prompt_round_trips_through_the_parser() {
        let attachments = vec![
            attachment("c1", "src/a.rs", "a.rs", 3, 5, "let a = 1;\nlet b = 2;", "这段为什么要循环？"),
            attachment("c2", "src/a.rs", "a.rs", 9, 9, "return x;", "建议早返回"),
        ];

        let prompt = build_prompt_with_code_comments("帮我看看这些改动", &attachments);
        assert!(prompt.starts_with("帮我看看这些改动"));
        assert!(prompt.contains("# Code comments:"));

        let parsed = parse_prompt_code_comments(&prompt, "F:/repo", None);
        // 正文被剥掉了批注块，只剩用户自己写的那句话
        assert_eq!(parsed.visible_content, "帮我看看这些改动");
        assert_eq!(parsed.code_comment_attachments.len(), 2);

        // 逐字段核对：**行号与选中的代码**都不能漂
        let first = &parsed.code_comment_attachments[0];
        assert_eq!(first.start_line, 3);
        assert_eq!(first.end_line, 5);
        assert_eq!(first.selected_text, "let a = 1;\nlet b = 2;");
        assert_eq!(first.comment, "这段为什么要循环？");
        assert_eq!(first.source_path.as_deref(), Some("src/a.rs"));

        let second = &parsed.code_comment_attachments[1];
        assert_eq!(second.start_line, 9);
        assert_eq!(second.end_line, 9);
        assert_eq!(second.comment, "建议早返回");
    }

    /// 没有批注时，拼出来的就是**原样正文**（不留任何 `# Code comments:` 痕迹）。
    #[test]
    fn a_prompt_without_annotations_is_just_the_text() {
        let prompt = build_prompt_with_code_comments("只有正文", &[]);
        assert_eq!(prompt, "只有正文");
        assert!(!prompt.contains("# Code comments:"));

        // 而且解析它拿不到任何附件，正文也原封不动
        let parsed = parse_prompt_code_comments(&prompt, "F:/repo", None);
        assert_eq!(parsed.visible_content, "只有正文");
        assert!(parsed.code_comment_attachments.is_empty());
    }
}
