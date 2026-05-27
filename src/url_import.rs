//! HTML `<table>` extraction from a URL.
//!
//! Fetches the page over HTTP/HTTPS (ureq + rustls), figures out the right
//! charset from `Content-Type` and/or `<meta charset>`, parses with
//! `scraper`, and returns each `<table>` as a `Vec<Vec<String>>` plus
//! optional caption.
//!
//! Caveats kept intentional for v1:
//! - `colspan` / `rowspan` are NOT expanded — the source cell text is
//!   emitted once at its declared column; subsequent cells in the same row
//!   may end up shifted. This is rare in data-style tables and avoids the
//!   complexity of a virtual grid.
//! - Nested `<table>` are extracted as separate top-level tables in the
//!   resulting list (scraper's `select` doesn't recurse-into-children
//!   bounds, so an inner table's rows would also be seen by the outer's
//!   `<tr>` selector — we restrict to direct row descendants of `tbody` /
//!   `thead` / `tfoot` / `table` to mitigate this).

use std::io::Read;

use scraper::{ElementRef, Html, Selector};

/// One extracted HTML table.
#[derive(Debug, Clone)]
pub struct ExtractedTable {
    /// `<caption>` text if present.
    pub caption: Option<String>,
    /// Rows × cells (already trimmed). `rows[0]` is whatever the source page
    /// put first — could be a header row or a data row; we don't try to
    /// detect headers automatically (the user can decide later).
    pub rows: Vec<Vec<String>>,
}

impl ExtractedTable {
    pub fn row_count(&self) -> usize { self.rows.len() }
    pub fn col_count(&self) -> usize { self.rows.iter().map(|r| r.len()).max().unwrap_or(0) }

    /// First-row text concatenated for the picker preview, truncated to ~60 chars.
    pub fn preview(&self) -> String {
        let first = self.rows.first().cloned().unwrap_or_default();
        let joined = first.join(" | ");
        let mut out = String::new();
        for c in joined.chars() {
            if out.chars().count() >= 60 { out.push('…'); break; }
            out.push(c);
        }
        out
    }
}

/// Fetch a URL and return the HTML body as a UTF-8 `String`, honoring the
/// HTTP `Content-Type` charset and `<meta charset>` if needed.
pub fn fetch_url(url: &str) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(concat!("tbla/", env!("CARGO_PKG_VERSION"), " (table import)"))
        .build();

    let resp = agent.get(url).call()
        .map_err(|e| format!("HTTP 取得失敗: {}", e))?;

    // Snapshot the Content-Type before consuming the body.
    let http_charset = resp
        .header("content-type")
        .and_then(parse_charset_from_content_type)
        .map(|s| s.to_ascii_lowercase());

    let mut bytes: Vec<u8> = Vec::new();
    resp.into_reader().take(20 * 1024 * 1024) // 20 MB cap
        .read_to_end(&mut bytes)
        .map_err(|e| format!("ボディ読み込み失敗: {}", e))?;

    Ok(decode_html_bytes(&bytes, http_charset.as_deref()))
}

/// Strip the `charset=` parameter out of a `Content-Type` header value.
fn parse_charset_from_content_type(v: &str) -> Option<String> {
    for part in v.split(';') {
        let p = part.trim();
        if let Some(rest) = p.strip_prefix("charset=").or_else(|| p.strip_prefix("CHARSET=")) {
            let s = rest.trim().trim_matches('"').trim_matches('\'');
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

/// Decode an HTML byte stream to UTF-8 `String`. Priority:
/// 1. HTTP `Content-Type: charset=…`
/// 2. UTF-8 BOM
/// 3. `<meta charset="…">` / `<meta http-equiv="Content-Type" content="…charset=…">`
///    scanned in the first 2 KiB
/// 4. Strict UTF-8
/// 5. Shift-JIS / CP932 fallback (Japanese pages without declared charset)
fn decode_html_bytes(bytes: &[u8], http_charset: Option<&str>) -> String {
    if let Some(cs) = http_charset {
        if let Some(enc) = encoding_rs::Encoding::for_label(cs.as_bytes()) {
            let (cow, _, _) = enc.decode(bytes);
            return cow.into_owned();
        }
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    if let Some(meta_cs) = sniff_meta_charset(&bytes[..bytes.len().min(2048)]) {
        if let Some(enc) = encoding_rs::Encoding::for_label(meta_cs.as_bytes()) {
            let (cow, _, _) = enc.decode(bytes);
            return cow.into_owned();
        }
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let (cow, _, _) = encoding_rs::SHIFT_JIS.decode(bytes);
    cow.into_owned()
}

/// Very small `<meta charset=…>` sniffer. Operates on a byte slice (we
/// haven't decoded yet) — works because the meta tag itself is ASCII.
fn sniff_meta_charset(head: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(head).to_ascii_lowercase();
    // <meta charset="..."> or <meta http-equiv="Content-Type" content="...charset=...">
    let idx = text.find("charset")?;
    let after = &text[idx + "charset".len()..];
    let rest = after.trim_start().trim_start_matches('=').trim_start();
    // If quoted, value runs until the matching quote. Otherwise it runs until
    // whitespace / `>` / `;`.
    let (val_end, val_start) = if let Some(q) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') {
        let after_quote = &rest[q.len_utf8()..];
        let end = after_quote.find(q).unwrap_or(after_quote.len());
        (q.len_utf8() + end, q.len_utf8())
    } else {
        let end = rest.find(|c: char| c == '>' || c == ';' || c.is_whitespace())
            .unwrap_or(rest.len());
        (end, 0)
    };
    let val = rest[val_start..val_end].trim();
    if val.is_empty() { None } else { Some(val.to_string()) }
}

/// Parse all `<table>` elements out of an HTML document.
pub fn extract_tables(html: &str) -> Vec<ExtractedTable> {
    let doc = Html::parse_document(html);
    let table_sel = Selector::parse("table").unwrap();
    let tr_sel = Selector::parse(":scope > tr, :scope > thead > tr, :scope > tbody > tr, :scope > tfoot > tr").unwrap();
    let cell_sel = Selector::parse(":scope > th, :scope > td").unwrap();
    let caption_sel = Selector::parse(":scope > caption").unwrap();

    let mut out = Vec::new();
    for table in doc.select(&table_sel) {
        let caption = table.select(&caption_sel).next().map(|c| clean_text(&c));
        let mut rows: Vec<Vec<String>> = Vec::new();
        for tr in table.select(&tr_sel) {
            let mut cells: Vec<String> = Vec::new();
            for c in tr.select(&cell_sel) {
                cells.push(clean_text(&c));
            }
            if !cells.is_empty() {
                rows.push(cells);
            }
        }
        if !rows.is_empty() {
            out.push(ExtractedTable { caption, rows });
        }
    }
    out
}

/// Collapse all descendant text into a single trimmed string with whitespace
/// runs reduced to one space — matches what a spreadsheet cell wants.
fn clean_text(el: &ElementRef) -> String {
    let mut buf = String::new();
    for chunk in el.text() {
        if !buf.is_empty() && !buf.ends_with(' ') {
            buf.push(' ');
        }
        for c in chunk.chars() {
            if c.is_whitespace() {
                if !buf.ends_with(' ') {
                    buf.push(' ');
                }
            } else {
                buf.push(c);
            }
        }
    }
    buf.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_simple_table() {
        let html = r#"
            <html><body>
            <table>
              <caption>Sample</caption>
              <tr><th>Name</th><th>Age</th></tr>
              <tr><td>Alice</td><td>30</td></tr>
              <tr><td>Bob</td><td>25</td></tr>
            </table>
            </body></html>
        "#;
        let tables = extract_tables(html);
        assert_eq!(tables.len(), 1);
        let t = &tables[0];
        assert_eq!(t.caption.as_deref(), Some("Sample"));
        assert_eq!(t.row_count(), 3);
        assert_eq!(t.col_count(), 2);
        assert_eq!(t.rows[1], vec!["Alice", "30"]);
    }

    #[test]
    fn extracts_multiple_tables() {
        let html = r#"
            <table><tr><td>A</td></tr></table>
            <table><tr><td>B</td><td>C</td></tr><tr><td>D</td><td>E</td></tr></table>
        "#;
        let tables = extract_tables(html);
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[0].col_count(), 1);
        assert_eq!(tables[1].col_count(), 2);
    }

    #[test]
    fn sniffs_meta_charset() {
        let head = br#"<html><head><meta charset="shift_jis"></head>"#;
        assert_eq!(sniff_meta_charset(head).as_deref(), Some("shift_jis"));
    }

    #[test]
    fn parses_charset_from_content_type() {
        assert_eq!(
            parse_charset_from_content_type("text/html; charset=UTF-8").as_deref(),
            Some("UTF-8")
        );
        assert_eq!(
            parse_charset_from_content_type("text/html; charset=\"Shift_JIS\"").as_deref(),
            Some("Shift_JIS")
        );
        assert_eq!(parse_charset_from_content_type("text/html").as_deref(), None);
    }

    #[test]
    fn decodes_utf8_with_meta() {
        // No HTTP charset, has <meta charset="utf-8">; body is plain ASCII.
        let html = b"<html><head><meta charset=\"utf-8\"></head><body>hi</body></html>";
        let s = decode_html_bytes(html, None);
        assert!(s.contains("hi"));
    }
}
