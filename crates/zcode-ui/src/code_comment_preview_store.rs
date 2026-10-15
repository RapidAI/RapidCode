//! 批注预览的**分桶**状态 —— Rust 移植自
//! `packages/ui/src/store/codeCommentPreviewStore.ts`
//!
//! 上游是 Zustand store。本移植把它做成一个**普通结构体**：store 的全部逻辑
//! 都是确定性的（查/增/删/按来源删/恢复/清空），没有任何 React 概念，
//! 所以不需要信号系统参与 —— 调用方自己决定什么时候把它包进 signal。
//!
//! ## 关键移植点 1：桶 key 是 `workspaceKey\0sourcePath`
//!
//! 用 `\0` 而不是 `/` 或 `:` —— workspace 路径与文件路径里都可能含这两个字符，
//! 只有 NUL 不可能出现在合法路径里。这样 `a\0b` 永远只会被解析成
//! 「workspace = a、文件 = b」，不存在歧义。
//!
//! ## 关键移植点 2：`add` 是「先删同 id 再加到末尾」，不是覆盖
//!
//! 上游用的是 `[...current.filter(c => c.id !== id), comment]`。
//! 这个顺序有语义：**同一条批注被重新添加时会移到列表末尾**。
//! 如果写成 `insert` 或直接替换，批注在界面上的位置就不会跟着「最近提交」
//! 变化，用户的观感是「点了没反应」。
//!
//! ## 关键移植点 3：`remove` 在**没删掉东西**时返回原状态
//!
//! 上游 `if (next.length === current.length) return state`。这不是性能优化，
//! 而是**避免无意义的状态更新** —— 在 React 里它会触发一次多余的订阅通知。
//! 本移植保留这个语义：返回值告诉你「有没有真的变」。
//!
//! ## 关键移植点 4：`removeCommentBySource` 扫**所有桶**
//!
//! 从输入框移除一个 chip 时，我们手里只有「workspace + id」，
//! **不知道它属于哪个文件**（chip 上确实显示文件名，但那不是可靠的索引）。
//! 所以要按 `<workspaceKey>\0` 前缀扫过所有桶，把匹配 id 的条目删掉。
//! 只扫「当前文件」那个桶是**错的** —— 同一个文件被打开成两个 tab、
//! 或从别的项目加进来的批注会漏删。
//!
//! ## 关键移植点 5：`restore` 必须先 `unmark`，且无 `sourcePath` 直接返回
//!
//! 恢复一条被移除的批注时，如果不先把它从「已移除」登记表里摘掉，
//! 下一次任何「按来源清理」都会立刻把它又删一遍。
//! 另外没有 `sourcePath` 的附件**根本放不进任何桶**（桶 key 需要文件路径），
//! 所以直接返回、不做任何事 —— 而不是造一个空路径的桶出来。

use std::collections::BTreeMap;

use crate::code_comment_context::{
    get_code_comment_workspace_key, CodeCommentComposerAttachment, CodeCommentPreviewEntry,
    CodeCommentRemovalRegistry, CodeCommentRemoveTarget,
};

/// 一个桶的定位参数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeCommentPreviewBucket {
    pub workspace_path: String,
    pub workspace_identity: Option<String>,
    pub source_path: String,
}

/// `buildCodeCommentPreviewBucketKey`：`<workspaceKey>\0<sourcePath>`。
pub fn build_code_comment_preview_bucket_key(bucket: &CodeCommentPreviewBucket) -> String {
    format!(
        "{}\0{}",
        get_code_comment_workspace_key(
            &bucket.workspace_path,
            bucket.workspace_identity.as_deref()
        ),
        bucket.source_path
    )
}

/// 批注预览的分桶表。
///
/// 用 `BTreeMap` 而不是 `HashMap`：**顺序稳定**，测试与快照不需要额外排序。
/// 上游用普通对象，迭代顺序依赖插入序 —— 但那里没有任何地方依赖顺序，
/// 所以选一个确定的实现更安全。
#[derive(Clone, Debug, Default)]
pub struct CodeCommentPreviewStore {
    comments_by_bucket_key: BTreeMap<String, Vec<CodeCommentPreviewEntry>>,
}

impl CodeCommentPreviewStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 某个文件下已提交的批注。查不到时返回空切片。
    pub fn get_comments(&self, bucket: &CodeCommentPreviewBucket) -> &[CodeCommentPreviewEntry] {
        self.comments_by_bucket_key
            .get(&build_code_comment_preview_bucket_key(bucket))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// 添加（或**移到末尾**，见模块移植点 2）。
    pub fn add_comment(
        &mut self,
        bucket: &CodeCommentPreviewBucket,
        comment: CodeCommentPreviewEntry,
    ) {
        let key = build_code_comment_preview_bucket_key(bucket);
        let entry = self.comments_by_bucket_key.entry(key).or_default();
        entry.retain(|existing| existing.id != comment.id);
        entry.push(comment);
    }

    /// 按 id 删除。返回**是否真的删掉了**（见模块移植点 3）。
    pub fn remove_comment(&mut self, bucket: &CodeCommentPreviewBucket, id: &str) -> bool {
        let key = build_code_comment_preview_bucket_key(bucket);
        let Some(entry) = self.comments_by_bucket_key.get_mut(&key) else {
            return false;
        };
        let before = entry.len();
        entry.retain(|comment| comment.id != id);
        entry.len() != before
    }

    /// 跨**所有桶**删除指定 id（见模块移植点 4）。
    ///
    /// 只处理前缀匹配 `<workspaceKey>\0` 的桶 —— 别的 workspace 的同名 id
    /// 是另一条批注，绝不能一起删掉。
    pub fn remove_comment_by_source(&mut self, target: &CodeCommentRemoveTarget) -> bool {
        let workspace_key = get_code_comment_workspace_key(
            &target.workspace_path,
            target.workspace_identity.as_deref(),
        );
        let prefix = format!("{workspace_key}\0");
        let mut changed = false;
        for (bucket_key, comments) in self.comments_by_bucket_key.iter_mut() {
            if !bucket_key.starts_with(&prefix) {
                continue;
            }
            let before = comments.len();
            comments.retain(|comment| comment.id != target.id);
            changed |= comments.len() != before;
        }
        changed
    }

    /// 从输入框的附件恢复一条批注预览（见模块移植点 5）。
    pub fn restore_comment_from_attachment(
        &mut self,
        registry: &mut CodeCommentRemovalRegistry,
        attachment: &CodeCommentComposerAttachment,
    ) {
        let Some(source_path) = attachment.source_path.clone() else {
            return;
        };
        if source_path.is_empty() {
            return;
        }
        registry.unmark_removed(&CodeCommentRemoveTarget {
            id: attachment.id.clone(),
            workspace_path: attachment.workspace_path.clone(),
            workspace_identity: attachment.workspace_identity.clone(),
        });
        self.add_comment(
            &CodeCommentPreviewBucket {
                workspace_path: attachment.workspace_path.clone(),
                workspace_identity: attachment.workspace_identity.clone(),
                source_path: source_path.clone(),
            },
            CodeCommentPreviewEntry {
                id: attachment.id.clone(),
                source_path: Some(source_path),
                source_title: attachment.source_title.clone(),
                start_line: attachment.start_line,
                end_line: attachment.end_line,
                selected_text: attachment.selected_text.clone(),
                comment: attachment.comment.clone(),
            },
        );
    }

    /// 整个桶清空（关掉文件 tab 时用）。桶不存在时什么都不做。
    pub fn clear_source(&mut self, bucket: &CodeCommentPreviewBucket) {
        self.comments_by_bucket_key
            .remove(&build_code_comment_preview_bucket_key(bucket));
    }

    /// 所有桶一起清空。
    ///
    /// 用在「批注已经拼进正文发出去了」这个时机 —— 不清就会在下一条消息里
    /// 把同一批批注重复发一遍。
    pub fn clear_all(&mut self) {
        self.comments_by_bucket_key.clear();
    }

    /// 一共有几个桶（诊断 / 测试用）。
    pub fn bucket_count(&self) -> usize {
        self.comments_by_bucket_key.len()
    }

    /// 所有桶里的批注总数。
    ///
    /// 输入区要显示「有几条批注会被一起发出去」—— 这是跨文件的合计，
    /// 所以不能只数当前桶。
    pub fn total_comment_count(&self) -> usize {
        self.comments_by_bucket_key.values().map(Vec::len).sum()
    }

    /// 所有批注，按**桶 key 有序**展开成可直接拼进 prompt 的附件。
    ///
    /// 顺序必须稳定：拼进 prompt 时用的是这个顺序，随 `BTreeMap` 的迭代顺序
    /// 是稳定的，同样的输入永远产出同样的 prompt。
    ///
    /// 桶 key 是 `<workspaceKey>\0<sourcePath>`，所以这里要**拆回**桶的两个
    /// 组成字段 —— 拼 prompt 时两者都要（`workspacePath` 与 `sourcePath`）。
    pub fn collect_composer_attachments(&self) -> Vec<CodeCommentComposerAttachment> {
        let mut attachments = Vec::new();
        for (key, entries) in &self.comments_by_bucket_key {
            let Some((workspace_key, source_path)) = key.split_once('\0') else {
                continue;
            };
            for entry in entries {
                attachments.push(CodeCommentComposerAttachment {
                    id: entry.id.clone(),
                    // bucket key 的第一段就是 `getCodeCommentWorkspaceKey` 的结果
                    workspace_path: workspace_key.to_string(),
                    workspace_identity: None,
                    source_path: entry.source_path.clone().or_else(|| Some(source_path.to_string())),
                    source_title: entry.source_title.clone(),
                    comment: entry.comment.clone(),
                    selected_text: entry.selected_text.clone(),
                    start_line: entry.start_line,
                    end_line: entry.end_line,
                });
            }
        }
        attachments
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bucket(source: &str) -> CodeCommentPreviewBucket {
        CodeCommentPreviewBucket {
            workspace_path: "F:/repo".to_string(),
            workspace_identity: None,
            source_path: source.to_string(),
        }
    }

    fn entry(id: &str, start: i64) -> CodeCommentPreviewEntry {
        CodeCommentPreviewEntry {
            id: id.to_string(),
            source_path: Some("src/a.rs".to_string()),
            source_title: "a.rs".to_string(),
            selected_text: "let a = 1;".to_string(),
            comment: format!("批注 {id}"),
            start_line: start,
            end_line: start,
        }
    }

    fn attachment(id: &str, source: Option<&str>) -> CodeCommentComposerAttachment {
        CodeCommentComposerAttachment {
            id: id.to_string(),
            workspace_path: "F:/repo".to_string(),
            workspace_identity: None,
            source_path: source.map(str::to_string),
            source_title: "a.rs".to_string(),
            selected_text: "let a = 1;".to_string(),
            comment: "为什么".to_string(),
            start_line: 3,
            end_line: 3,
        }
    }

    #[test]
    fn the_bucket_key_joins_with_a_nul_separator() {
        assert_eq!(
            build_code_comment_preview_bucket_key(&bucket("src/a.rs")),
            "F:/repo\0src/a.rs"
        );
    }

    #[test]
    fn the_identity_replaces_the_path_in_the_bucket_key() {
        let with_identity = CodeCommentPreviewBucket {
            workspace_path: "F:/repo".to_string(),
            workspace_identity: Some("git:abc".to_string()),
            source_path: "src/a.rs".to_string(),
        };
        assert_eq!(
            build_code_comment_preview_bucket_key(&with_identity),
            "git:abc\0src/a.rs"
        );
    }

    #[test]
    fn comments_are_isolated_per_file() {
        let mut store = CodeCommentPreviewStore::new();
        store.add_comment(&bucket("src/a.rs"), entry("c1", 3));
        store.add_comment(&bucket("src/b.rs"), entry("c2", 7));

        assert_eq!(store.get_comments(&bucket("src/a.rs")).len(), 1);
        assert_eq!(store.get_comments(&bucket("src/a.rs"))[0].id, "c1");
        assert_eq!(store.get_comments(&bucket("src/b.rs"))[0].id, "c2");
        // 没碰过的文件是空的，不是「拿到别人的」
        assert!(store.get_comments(&bucket("src/c.rs")).is_empty());
    }

    #[test]
    fn re_adding_the_same_id_moves_it_to_the_end() {
        // 见模块移植点 2：是「移到末尾」，不是原地覆盖
        let mut store = CodeCommentPreviewStore::new();
        store.add_comment(&bucket("src/a.rs"), entry("c1", 1));
        store.add_comment(&bucket("src/a.rs"), entry("c2", 2));
        store.add_comment(&bucket("src/a.rs"), entry("c1", 9));

        let ids: Vec<&str> = store
            .get_comments(&bucket("src/a.rs"))
            .iter()
            .map(|c| c.id.as_str())
            .collect();
        assert_eq!(ids, vec!["c2", "c1"], "同 id 应当被移到末尾且不重复");
        // 内容也要跟着更新
        assert_eq!(store.get_comments(&bucket("src/a.rs"))[1].start_line, 9);
    }

    #[test]
    fn removing_reports_whether_anything_changed() {
        let mut store = CodeCommentPreviewStore::new();
        store.add_comment(&bucket("src/a.rs"), entry("c1", 3));

        assert!(store.remove_comment(&bucket("src/a.rs"), "c1"));
        // 再删一次没东西可删，必须如实返回 false（见模块移植点 3）
        assert!(!store.remove_comment(&bucket("src/a.rs"), "c1"));
        // 文件根本不存在时也一样
        assert!(!store.remove_comment(&bucket("src/nope.rs"), "c1"));
        assert!(store.get_comments(&bucket("src/a.rs")).is_empty());
    }

    #[test]
    fn removing_by_source_sweeps_every_bucket_of_that_workspace() {
        // 见模块移植点 4：chip 上不知道文件，必须扫全部桶
        let mut store = CodeCommentPreviewStore::new();
        store.add_comment(&bucket("src/a.rs"), entry("c1", 1));
        store.add_comment(&bucket("src/b.rs"), entry("c1", 2));
        store.add_comment(&bucket("src/b.rs"), entry("c9", 3));

        let changed = store.remove_comment_by_source(&CodeCommentRemoveTarget {
            id: "c1".to_string(),
            workspace_path: "F:/repo".to_string(),
            workspace_identity: None,
        });
        assert!(changed);
        assert!(store.get_comments(&bucket("src/a.rs")).is_empty());
        let remaining: Vec<&str> = store
            .get_comments(&bucket("src/b.rs"))
            .iter()
            .map(|c| c.id.as_str())
            .collect();
        assert_eq!(remaining, vec!["c9"], "别的 id 不能被误删");
    }

    #[test]
    fn removing_by_source_does_not_touch_another_workspace() {
        let mut store = CodeCommentPreviewStore::new();
        store.add_comment(
            &CodeCommentPreviewBucket {
                workspace_path: "F:/other".to_string(),
                workspace_identity: None,
                source_path: "src/a.rs".to_string(),
            },
            entry("c1", 1),
        );

        let changed = store.remove_comment_by_source(&CodeCommentRemoveTarget {
            id: "c1".to_string(),
            workspace_path: "F:/repo".to_string(),
            workspace_identity: None,
        });
        assert!(!changed, "别的 workspace 的同名 id 是另一条批注");
        assert_eq!(store.bucket_count(), 1);
    }

    #[test]
    fn removing_by_source_matches_on_the_identity_when_present() {
        let mut store = CodeCommentPreviewStore::new();
        let identified = CodeCommentPreviewBucket {
            workspace_path: "F:/repo".to_string(),
            workspace_identity: Some("git:abc".to_string()),
            source_path: "src/a.rs".to_string(),
        };
        store.add_comment(&identified, entry("c1", 1));

        // 用 identity 查得到
        assert!(store.remove_comment_by_source(&CodeCommentRemoveTarget {
            id: "c1".to_string(),
            workspace_path: "F:/whatever".to_string(),
            workspace_identity: Some("git:abc".to_string()),
        }));
        // 只给路径（identity 是 None）时**不该**命中 —— 桶 key 是 identity
        store.add_comment(&identified, entry("c1", 1));
        assert!(!store.remove_comment_by_source(&CodeCommentRemoveTarget {
            id: "c1".to_string(),
            workspace_path: "F:/repo".to_string(),
            workspace_identity: None,
        }));
    }

    #[test]
    fn restoring_an_attachment_brings_the_comment_back() {
        let mut store = CodeCommentPreviewStore::new();
        let mut registry = CodeCommentRemovalRegistry::new();
        let item = attachment("c1", Some("src/a.rs"));

        registry.mark_removed(&CodeCommentRemoveTarget {
            id: "c1".to_string(),
            workspace_path: "F:/repo".to_string(),
            workspace_identity: None,
        });
        assert!(registry.is_marked_removed(&CodeCommentRemoveTarget {
            id: "c1".to_string(),
            workspace_path: "F:/repo".to_string(),
            workspace_identity: None,
        }));

        store.restore_comment_from_attachment(&mut registry, &item);

        assert_eq!(store.get_comments(&bucket("src/a.rs")).len(), 1);
        assert_eq!(store.get_comments(&bucket("src/a.rs"))[0].id, "c1");
        // 关键：恢复之后必须**不再**被登记为已移除，否则下一次清理会立刻删掉它
        assert!(!registry.is_marked_removed(&CodeCommentRemoveTarget {
            id: "c1".to_string(),
            workspace_path: "F:/repo".to_string(),
            workspace_identity: None,
        }));
    }

    #[test]
    fn restoring_without_a_source_path_does_nothing() {
        // 没有文件路径就放不进任何桶（见模块移植点 5），而不是造一个空路径桶
        let mut store = CodeCommentPreviewStore::new();
        let mut registry = CodeCommentRemovalRegistry::new();
        store.restore_comment_from_attachment(&mut registry, &attachment("c1", None));

        assert_eq!(store.bucket_count(), 0);
        assert!(registry.is_empty(), "什么都没做就不该动登记表");
    }

    #[test]
    fn restoring_an_empty_source_path_does_nothing() {
        let mut store = CodeCommentPreviewStore::new();
        let mut registry = CodeCommentRemovalRegistry::new();
        store.restore_comment_from_attachment(&mut registry, &attachment("c1", Some("")));
        assert_eq!(store.bucket_count(), 0);
    }

    #[test]
    fn clearing_a_source_removes_only_that_bucket() {
        let mut store = CodeCommentPreviewStore::new();
        store.add_comment(&bucket("src/a.rs"), entry("c1", 1));
        store.add_comment(&bucket("src/b.rs"), entry("c2", 2));

        store.clear_source(&bucket("src/a.rs"));

        assert!(store.get_comments(&bucket("src/a.rs")).is_empty());
        assert_eq!(store.get_comments(&bucket("src/b.rs")).len(), 1);
        // 清空之后桶本身也应当消失（否则会一直漂着一个空桶）
        assert_eq!(store.bucket_count(), 1);
    }

    #[test]
    fn clearing_an_unknown_source_is_a_no_op() {
        let mut store = CodeCommentPreviewStore::new();
        store.clear_source(&bucket("src/never.rs"));
        assert_eq!(store.bucket_count(), 0);
    }

    // ---- 汇总（发消息时用） ----

    /// 总数是**跨文件**的合计 —— 输入区要显示「一共几条会一起发出去」。
    #[test]
    fn total_comment_count_spans_every_bucket() {
        let mut store = CodeCommentPreviewStore::new();
        assert_eq!(store.total_comment_count(), 0);

        store.add_comment(&bucket("src/a.rs"), entry("c1", 1));
        store.add_comment(&bucket("src/a.rs"), entry("c2", 2));
        store.add_comment(&bucket("src/b.rs"), entry("c3", 3));

        assert_eq!(store.total_comment_count(), 3);
    }

    /// 同一个 id 重新添加**不会**让总数变大（是先删后加，不是纯追加）。
    #[test]
    fn re_adding_an_id_does_not_inflate_the_total() {
        let mut store = CodeCommentPreviewStore::new();
        store.add_comment(&bucket("src/a.rs"), entry("c1", 1));
        store.add_comment(&bucket("src/a.rs"), entry("c1", 1));
        assert_eq!(store.total_comment_count(), 1);
    }

    /// 汇总成附件时，`source_path` 与 `workspace_path` 都要带上 ——
    /// 少任何一个，助手那边就不知道批注是说哪个仓库的哪个文件。
    #[test]
    fn composer_attachments_carry_workspace_and_source() {
        let mut store = CodeCommentPreviewStore::new();
        store.add_comment(&bucket("src/a.rs"), entry("c1", 7));

        let attachments = store.collect_composer_attachments();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].id, "c1");
        assert_eq!(attachments[0].source_path.as_deref(), Some("src/a.rs"));
        assert_eq!(attachments[0].workspace_path, "F:/repo");
        assert_eq!(attachments[0].start_line, 7);
        assert_eq!(attachments[0].end_line, 7);
    }

    /// 顺序必须**确定**：同样的 store 状态每次都要产出同样的顺序，
    /// 否则同一个输入会产出不同的 prompt，问题无法复现。
    #[test]
    fn composer_attachment_order_is_deterministic() {
        let mut store = CodeCommentPreviewStore::new();
        // 故意用乱序插入
        store.add_comment(&bucket("src/c.rs"), entry("c3", 3));
        store.add_comment(&bucket("src/a.rs"), entry("c1", 1));
        store.add_comment(&bucket("src/b.rs"), entry("c2", 2));

        let first: Vec<String> = store
            .collect_composer_attachments()
            .into_iter()
            .map(|a| a.id)
            .collect();
        let second: Vec<String> = store
            .collect_composer_attachments()
            .into_iter()
            .map(|a| a.id)
            .collect();

        assert_eq!(first, second, "同一状态两次汇总必须一致");
        // `BTreeMap` 按桶 key 排序，而桶 key 以 sourcePath 结尾
        assert_eq!(first, vec!["c1", "c2", "c3"]);
    }

    /// `clear_all` 之后一条都不剩 —— 批注已经拼进正文发走了，不清就会重发。
    #[test]
    fn clear_all_empties_everything() {
        let mut store = CodeCommentPreviewStore::new();
        store.add_comment(&bucket("src/a.rs"), entry("c1", 1));
        store.add_comment(&bucket("src/b.rs"), entry("c2", 2));

        store.clear_all();

        assert_eq!(store.bucket_count(), 0);
        assert_eq!(store.total_comment_count(), 0);
        assert!(store.collect_composer_attachments().is_empty());
    }

    /// 客户端传入的 `source_path` 为空时，回退到桶 key 里的那段 ——
    /// 否则附件会带着一个空路径发出去，助手无从定位。
    #[test]
    fn composer_attachment_falls_back_to_the_bucket_source_path() {
        let mut store = CodeCommentPreviewStore::new();
        let mut bare = entry("c1", 1);
        bare.source_path = None;
        store.add_comment(&bucket("src/a.rs"), bare);

        let attachments = store.collect_composer_attachments();
        assert_eq!(attachments[0].source_path.as_deref(), Some("src/a.rs"));
    }
}
