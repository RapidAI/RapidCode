//! `pdf_js_assets` —— Rust 移植自 `packages/ui/src/lib/pdfJsAssets.ts`
//!
//! 只有一件事：算出 PDF.js 的 **cMap 目录**。
//!
//! ## 为什么要显式给 cMapUrl
//!
//! ReportLab 生成的 `STSong-Light` 等 Type0 字体只声明了预定义 CMap，
//! **PDF 内并未嵌入字符映射**。浏览器原生 PDF 预览自带这份资源，而 PDF.js
//! 必须显式传入 `cMapUrl` 才能渲染这类中文文档 —— 不给就是一片空白/乱码。
//!
//! ## 与 TS 的差异
//!
//! **不引入 URL 库**：TS 直接用浏览器的 `new URL(relative, base)`，
//! 本移植手写一个最小子集（[`resolve_url`]）。支持：
//!
//! - `relative` 自带 scheme（`http:` / `https:` / `file:` …）→ 原样返回；
//! - `relative` 以 `/` 开头 → 挂到 `base` 的 origin 上；
//! - 其余 → 按 `base` 的目录拼接（`./` 会被消掉）。
//!
//! 不支持 `..` 归一化与查询串/片段处理 —— 这两个入口的实参都是固定的
//! 资源路径，用不到。

/// PDF.js 文档加载参数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdfJsDocumentOptions {
    pub c_map_packed: bool,
    pub c_map_url: String,
}

/// 是否是绝对 URL（自带 scheme）。
fn has_scheme(url: &str) -> bool {
    let Some(colon) = url.find(':') else {
        return false;
    };
    let scheme = &url[..colon];
    !scheme.is_empty()
        && scheme
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// 拆出 `scheme://authority` 前缀与剩余路径。
fn split_origin(url: &str) -> (String, String) {
    if let Some(index) = url.find("://") {
        let after = &url[index + 3..];
        let authority_end = after
            .find('/')
            .map(|offset| index + 3 + offset)
            .unwrap_or(url.len());
        return (
            url[..authority_end].to_string(),
            url[authority_end..].to_string(),
        );
    }
    (String::new(), url.to_string())
}

/// `new URL(relative, base)` 的最小子集，见模块文档差异。
pub fn resolve_url(relative: &str, base: &str) -> String {
    if has_scheme(relative) {
        return relative.to_string();
    }

    let (origin, base_path) = split_origin(base);

    if relative.starts_with('/') {
        return format!("{origin}{relative}");
    }

    let directory = match base_path.rfind('/') {
        Some(index) => &base_path[..index + 1],
        None => "/",
    };

    // `./` 就是「当前目录」，直接消掉
    let relative = relative.strip_prefix("./").unwrap_or(relative);
    if relative.is_empty() {
        return format!("{origin}{directory}");
    }
    format!("{origin}{directory}{relative}")
}

/// 构造 PDF.js 文档加载参数（TS `createPdfJsDocumentOptions`）。
///
/// - `vite_base_url`：应用基路径（Vite 的 `import.meta.env.BASE_URL`，
///   Desktop 的 `file://` 与 Web/手机远控的子路径部署各不相同）；
/// - `document_url`：当前文档 URL（解析基路径用）。
pub fn create_pdf_js_document_options(
    vite_base_url: &str,
    document_url: &str,
) -> PdfJsDocumentOptions {
    let application_base_url = resolve_url(vite_base_url, document_url);
    PdfJsDocumentOptions {
        c_map_packed: true,
        c_map_url: resolve_url("pdfjs/cmaps/", &application_base_url),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_detection() {
        assert!(has_scheme("http://x/"));
        assert!(has_scheme("https://x/"));
        assert!(has_scheme("file:///C:/x"));
        assert!(has_scheme("mailto:a@b"));
        // 相对路径与绝对路径都没有 scheme
        assert!(!has_scheme("./"));
        assert!(!has_scheme("pdfjs/cmaps/"));
        assert!(!has_scheme("/abs/path"));
        assert!(!has_scheme(""));
        // `:` 在路径里不算 scheme（首字符必须是字母）
        assert!(!has_scheme("1:x"));
    }

    #[test]
    fn split_origin_separates_authority_from_path() {
        assert_eq!(
            split_origin("http://localhost/"),
            ("http://localhost".to_string(), "/".to_string())
        );
        assert_eq!(
            split_origin("https://a.b/c/d?e"),
            ("https://a.b".to_string(), "/c/d?e".to_string())
        );
        // 没有路径
        assert_eq!(
            split_origin("http://localhost"),
            ("http://localhost".to_string(), String::new())
        );
        // `file://` 的 authority 是空的
        assert_eq!(
            split_origin("file:///C:/app/index.html"),
            ("file://".to_string(), "/C:/app/index.html".to_string())
        );
        assert_eq!(split_origin("relative/x"), (String::new(), "relative/x".to_string()));
    }

    #[test]
    fn an_absolute_relative_url_wins() {
        assert_eq!(
            resolve_url("https://cdn.example/x.js", "http://localhost/app/"),
            "https://cdn.example/x.js"
        );
    }

    #[test]
    fn a_root_relative_url_replaces_the_path() {
        assert_eq!(
            resolve_url("/assets/x.js", "http://localhost/app/index.html"),
            "http://localhost/assets/x.js"
        );
    }

    #[test]
    fn a_relative_url_joins_the_base_directory() {
        assert_eq!(
            resolve_url("pdfjs/cmaps/", "http://localhost/app/index.html"),
            "http://localhost/app/pdfjs/cmaps/"
        );
    }

    /// `./` 表示当前目录，解析结果就是 base 的目录本身。
    #[test]
    fn a_dot_slash_resolves_to_the_base_directory() {
        assert_eq!(resolve_url("./", "http://localhost/"), "http://localhost/");
        assert_eq!(
            resolve_url("./", "http://localhost/app/index.html"),
            "http://localhost/app/"
        );
        assert_eq!(
            resolve_url("./", "file:///C:/app/index.html"),
            "file:///C:/app/"
        );
    }

    // ---- 端到端 ----

    #[test]
    fn the_web_root_case() {
        let options = create_pdf_js_document_options("./", "http://localhost/");
        assert!(options.c_map_packed);
        assert_eq!(options.c_map_url, "http://localhost/pdfjs/cmaps/");
    }

    /// 子路径部署（Web / 手机远控）。
    #[test]
    fn the_sub_path_case() {
        let options = create_pdf_js_document_options("/preview/", "https://host/app/index.html");
        assert_eq!(options.c_map_url, "https://host/preview/pdfjs/cmaps/");
    }

    /// Desktop 走 `file://`，此时 `BASE_URL` 是相对路径。
    #[test]
    fn the_file_protocol_case() {
        let options = create_pdf_js_document_options("./", "file:///C:/app/index.html");
        assert_eq!(options.c_map_url, "file:///C:/app/pdfjs/cmaps/");
    }

    #[test]
    fn c_map_packed_is_always_true() {
        // 打包过的 cMap 体积小一个数量级，没有理由关掉
        for (base, doc) in [
            ("./", "http://localhost/"),
            ("/a/", "https://h/b/c.html"),
            ("", "file:///x/y.html"),
        ] {
            assert!(create_pdf_js_document_options(base, doc).c_map_packed);
        }
    }

    /// 空基路径等价于「当前目录」。
    #[test]
    fn an_empty_base_url_resolves_to_the_document_directory() {
        let options = create_pdf_js_document_options("", "http://localhost/app/index.html");
        assert_eq!(options.c_map_url, "http://localhost/app/pdfjs/cmaps/");
    }
}
