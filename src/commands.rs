use crate::App;
use crate::cell::CellValue;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use serde::{Deserialize, Serialize};
use unicode_width::UnicodeWidthStr;

/// JSON file format for tbla
#[derive(Serialize, Deserialize)]
struct TblaFile {
    version: String,
    name: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    col_widths: HashMap<String, usize>,
    cells: HashMap<String, CellData>,
}

#[derive(Serialize, Deserialize)]
struct CellData {
    value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    formula: Option<String>,
}

/// Normalize a user-supplied file path string from a dialog or CLI arg.
/// Trims whitespace and strips one matching pair of surrounding `"` or `'`
/// characters — Windows Explorer's "Copy as path" wraps the path in `"…"`,
/// and quotes inside a Windows path are themselves illegal (ERROR_INVALID_NAME).
pub fn sanitize_path_input(raw: &str) -> String {
    let trimmed = raw.trim();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return trimmed[1..trimmed.len() - 1].trim().to_string();
        }
    }
    trimmed.to_string()
}

/// Save to file (auto-detect extension; defaults to .json).
pub fn save_to_file(app: &mut App, filename: &str) {
    match save_file(app, filename) {
        Ok(actual) => {
            app.current_file = Some(actual.clone());
            app.status_message = format!("{} に保存しました", actual);
        }
        Err(e) => {
            app.status_message = format!("保存エラー: {}", e);
        }
    }
}

/// Load from file (auto-detect extension).
pub fn load_from_file(app: &mut App, filename: &str) {
    match load_file(app, filename) {
        Ok(()) => {
            app.current_file = Some(filename.to_string());
            app.status_message = format!("{} を読み込みました", filename);
        }
        Err(e) => {
            let msg = e.to_string();
            app.status_message = if msg.contains("os error 123") {
                format!("読み込みエラー: {} (パスに \" や ' などの無効文字が含まれていませんか)", msg)
            } else {
                format!("読み込みエラー: {}", msg)
            };
        }
    }
}

pub fn import_csv_file(app: &mut App, filename: &str) {
    match import_csv(app, filename) {
        Ok(()) => {
            app.current_file = Some(filename.to_string());
            app.status_message = format!("{} をインポートしました", filename);
        }
        Err(e) => {
            app.status_message = format!("インポートエラー: {}", e);
        }
    }
}

pub fn export_csv_file(app: &mut App, filename: &str) {
    match export_csv(app, filename) {
        Ok(()) => {
            app.status_message = format!("{} へエクスポートしました", filename);
        }
        Err(e) => {
            app.status_message = format!("エクスポートエラー: {}", e);
        }
    }
}

/// Export the current sheet as a print-friendly HTML file and try to open
/// it in the user's default browser. On success the status message reports
/// whether the browser launch succeeded; the file is always written.
pub fn export_html_file(app: &mut App, filename: &str) {
    match export_html(app, filename) {
        Ok(()) => {
            let opened = open_in_browser(filename);
            app.status_message = if opened {
                format!("{} を出力してブラウザで開きました（Cmd/Ctrl+P で印刷）", filename)
            } else {
                format!("{} を出力しました（ブラウザで開いて Cmd/Ctrl+P で印刷）", filename)
            };
        }
        Err(e) => {
            app.status_message = format!("HTML エクスポートエラー: {}", e);
        }
    }
}

fn open_in_browser(path: &str) -> bool {
    use std::process::{Command, Stdio};
    let abs = std::fs::canonicalize(path).map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string());
    let cmd = if cfg!(target_os = "macos") {
        Some(("open", vec![abs]))
    } else if cfg!(target_os = "windows") {
        Some(("cmd", vec!["/C".to_string(), "start".to_string(), "".to_string(), abs]))
    } else if cfg!(unix) {
        Some(("xdg-open", vec![abs]))
    } else {
        None
    };
    if let Some((bin, args)) = cmd {
        Command::new(bin).args(args)
            .stdout(Stdio::null()).stderr(Stdio::null())
            .spawn().is_ok()
    } else { false }
}

/// Auto-adjust all columns with data.
pub fn autowidth_all(app: &mut App) {
    const MIN_WIDTH: usize = 4;
    const MAX_WIDTH: usize = 50;

    let max_row = app.sheet.max_row().unwrap_or(0);
    let max_col = app.sheet.max_col().unwrap_or(0);
    let mut adjusted = 0;

    for col in 0..=max_col {
        let width = calc_column_width(app, col, max_row, MIN_WIDTH, MAX_WIDTH);
        app.sheet.set_col_width(col, width);
        adjusted += 1;
    }

    app.status_message = format!("{} 列の幅を自動調整", adjusted);
}

fn calc_column_width(app: &App, col: usize, max_row: usize, min_width: usize, max_width: usize) -> usize {
    let mut width = min_width;

    let col_name = crate::formula::col_to_name(col);
    width = width.max(UnicodeWidthStr::width(col_name.as_str()) + 2);

    for row in 0..=max_row {
        let value = app.sheet.evaluate(col, row);
        let cell_width = UnicodeWidthStr::width(value.as_str()) + 2;
        width = width.max(cell_width);
    }

    width.min(max_width)
}

/// Search forward from current position.
pub fn search_forward(app: &mut App) {
    if app.last_search.is_empty() {
        app.status_message = "検索文字列がありません".to_string();
        return;
    }

    let term = app.last_search.clone();
    let term_upper = term.to_uppercase();
    let start_col = app.cursor_col;
    let start_row = app.cursor_row;

    for row in start_row..10000 {
        let col_start = if row == start_row { start_col + 1 } else { 0 };
        for col in col_start..256 {
            let value = app.sheet.evaluate(col, row);
            if value.to_uppercase().contains(&term_upper) {
                app.cursor_col = col;
                app.cursor_row = row;
                app.selection_anchor = None;
                app.adjust_view();
                app.status_message = format!("「{}」 -> {}", term, crate::formula::cell_name(col, row));
                return;
            }
        }
    }

    for row in 0..=start_row {
        let col_end = if row == start_row { start_col } else { 256 };
        for col in 0..col_end {
            let value = app.sheet.evaluate(col, row);
            if value.to_uppercase().contains(&term_upper) {
                app.cursor_col = col;
                app.cursor_row = row;
                app.selection_anchor = None;
                app.adjust_view();
                app.status_message = format!("「{}」 -> {} (折返し)", term, crate::formula::cell_name(col, row));
                return;
            }
        }
    }

    app.status_message = format!("見つかりません: {}", term);
}

/// Search backward from current position.
pub fn search_backward(app: &mut App) {
    if app.last_search.is_empty() {
        app.status_message = "検索文字列がありません".to_string();
        return;
    }

    let term = app.last_search.clone();
    let term_upper = term.to_uppercase();
    let start_col = app.cursor_col;
    let start_row = app.cursor_row;

    for row in (0..=start_row).rev() {
        let col_end = if row == start_row { start_col } else { 256 };
        for col in (0..col_end).rev() {
            let value = app.sheet.evaluate(col, row);
            if value.to_uppercase().contains(&term_upper) {
                app.cursor_col = col;
                app.cursor_row = row;
                app.selection_anchor = None;
                app.adjust_view();
                app.status_message = format!("「{}」 -> {}", term, crate::formula::cell_name(col, row));
                return;
            }
        }
    }

    for row in (start_row..10000).rev() {
        let col_start = if row == start_row { start_col + 1 } else { 0 };
        for col in (col_start..256).rev() {
            let value = app.sheet.evaluate(col, row);
            if value.to_uppercase().contains(&term_upper) {
                app.cursor_col = col;
                app.cursor_row = row;
                app.selection_anchor = None;
                app.adjust_view();
                app.status_message = format!("「{}」 -> {} (折返し)", term, crate::formula::cell_name(col, row));
                return;
            }
        }
    }

    app.status_message = format!("見つかりません: {}", term);
}

fn save_file(app: &App, filename: &str) -> Result<String, String> {
    let path = Path::new(filename);
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    let filename = if ext.is_empty() {
        format!("{}.json", filename)
    } else {
        filename.to_string()
    };

    let ext = Path::new(&filename).extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "csv" | "tsv" => {
            export_csv(app, &filename).map_err(|e| e.to_string())?;
            Ok(filename)
        }
        "xlsx" => {
            crate::xlsx::write_xlsx(&app.sheet, &filename)?;
            Ok(filename)
        }
        _ => {
            save_json(app, &filename).map_err(|e| e.to_string())?;
            Ok(filename)
        }
    }
}

fn load_file(app: &mut App, filename: &str) -> Result<(), String> {
    let path = Path::new(filename);
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "csv" | "tsv" => import_csv(app, filename).map_err(|e| e.to_string()),
        "xlsx" | "xlsm" => {
            let result = crate::xlsx::read_xlsx(filename)?;
            app.save_undo();
            app.sheet = result.sheet;
            if let Some(w) = result.warning {
                app.status_message = w;
            }
            Ok(())
        }
        _ => load_json(app, filename).map_err(|e| e.to_string()),
    }
}

fn save_json(app: &App, filename: &str) -> std::io::Result<()> {
    use crate::sheet::DEFAULT_COL_WIDTH;

    let mut col_widths = HashMap::new();
    for col in 0..=255 {
        let width = app.sheet.get_col_width(col);
        if width != DEFAULT_COL_WIDTH {
            let col_name = crate::formula::col_to_name(col);
            col_widths.insert(col_name, width);
        }
    }

    let mut cells = HashMap::new();
    for ((col, row), cell) in app.sheet.cells().iter() {
        let cell_name = crate::formula::cell_name(*col, *row);

        let evaluated = app.sheet.evaluate(*col, *row);

        let cell_data = match &cell.value {
            CellValue::Formula(_) => CellData {
                value: evaluated,
                formula: Some(cell.raw_input.clone()),
            },
            _ => CellData {
                value: cell.raw_input.clone(),
                formula: None,
            },
        };

        cells.insert(cell_name, cell_data);
    }

    let file_data = TblaFile {
        version: "1.0".to_string(),
        name: app.sheet.name.clone(),
        col_widths,
        cells,
    };

    let json = serde_json::to_string_pretty(&file_data)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let mut file = fs::File::create(filename)?;
    file.write_all(json.as_bytes())?;
    Ok(())
}

fn load_json(app: &mut App, filename: &str) -> std::io::Result<()> {
    let mut file = fs::File::open(filename)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let file_data: TblaFile = serde_json::from_str(&contents)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    app.save_undo();

    let mut sheet = crate::sheet::Sheet::new();
    sheet.name = file_data.name;

    for (col_name, width) in file_data.col_widths {
        if let Some((col, _, _, _)) = crate::formula::parse_cell_ref(&format!("{}1", col_name)) {
            sheet.set_col_width(col, width);
        }
    }

    for (cell_name, cell_data) in file_data.cells {
        if let Some((col, row, _, _)) = crate::formula::parse_cell_ref(&cell_name) {
            let input = cell_data.formula.unwrap_or(cell_data.value);
            sheet.set_cell(col, row, input);
        }
    }

    app.sheet = sheet;
    app.cursor_col = 0;
    app.cursor_row = 0;
    app.view_col = 0;
    app.view_row = 0;
    app.selection_anchor = None;
    Ok(())
}

fn export_csv(app: &App, filename: &str) -> std::io::Result<()> {
    let max_col = app.sheet.max_col().unwrap_or(0);
    let max_row = app.sheet.max_row().unwrap_or(0);

    let mut csv = String::new();
    for row in 0..=max_row {
        let mut row_values = Vec::new();
        for col in 0..=max_col {
            let value = app.sheet.evaluate(col, row);
            if value.contains(',') || value.contains('"') || value.contains('\n') {
                row_values.push(format!("\"{}\"", value.replace('"', "\"\"")));
            } else {
                row_values.push(value);
            }
        }
        csv.push_str(&row_values.join(","));
        csv.push('\n');
    }

    let mut file = fs::File::create(filename)?;
    file.write_all(csv.as_bytes())?;
    Ok(())
}

fn export_html(app: &App, filename: &str) -> std::io::Result<()> {
    let max_col = app.sheet.max_col().unwrap_or(0);
    let max_row = app.sheet.max_row().unwrap_or(0);

    let title = app.current_file.as_deref().unwrap_or("Sheet");
    let mut html = String::new();
    html.push_str("<!DOCTYPE html>\n<html lang=\"ja\"><head><meta charset=\"utf-8\">\n");
    html.push_str(&format!("<title>{}</title>\n", html_escape(title)));
    html.push_str(r#"<style>
:root { color-scheme: light; }
body { font-family: -apple-system, "Helvetica Neue", "Hiragino Sans", "Yu Gothic", sans-serif; margin: 1em; color: #111; }
h1 { font-size: 1.1em; font-weight: normal; margin: 0 0 .6em; color: #444; }
table { border-collapse: collapse; font-size: 12px; }
th, td { border: 1px solid #888; padding: 2px 6px; vertical-align: top; white-space: nowrap; }
thead th, th.rh { background: #f0f0f0; font-weight: 600; text-align: center; color: #333; }
td.num { text-align: right; font-variant-numeric: tabular-nums; }
td.txt { text-align: left; }
td.err { color: #b00; }
@media print {
  body { margin: 0.5cm; }
  thead { display: table-header-group; }   /* repeat header on each page */
  tr { page-break-inside: avoid; }
  table { font-size: 10px; }
}
</style></head><body>
"#);
    html.push_str(&format!("<h1>{}</h1>\n", html_escape(title)));
    html.push_str("<table><thead><tr><th class=\"rh\"></th>");
    for col in 0..=max_col {
        html.push_str(&format!("<th>{}</th>", crate::formula::col_to_name(col)));
    }
    html.push_str("</tr></thead><tbody>\n");

    for row in 0..=max_row {
        html.push_str(&format!("<tr><th class=\"rh\">{}</th>", row + 1));
        for col in 0..=max_col {
            let value = app.sheet.evaluate(col, row);
            let cell = app.sheet.get_cell_ref(col, row);
            let class = match cell.map(|c| &c.value) {
                Some(CellValue::Number(_)) | Some(CellValue::Formula(_)) => {
                    // Right-align if it evaluated to a numeric string.
                    if value.parse::<f64>().is_ok() { "num" } else { "txt" }
                }
                Some(CellValue::Error(_)) => "err",
                _ => "txt",
            };
            html.push_str(&format!("<td class=\"{}\">{}</td>", class, html_escape(&value)));
        }
        html.push_str("</tr>\n");
    }
    html.push_str("</tbody></table>\n</body></html>\n");

    let mut file = fs::File::create(filename)?;
    file.write_all(html.as_bytes())?;
    Ok(())
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn import_csv(app: &mut App, filename: &str) -> std::io::Result<()> {
    let mut file = fs::File::open(filename)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    app.save_undo();
    app.sheet = crate::sheet::Sheet::new();

    for (row, line) in contents.lines().enumerate() {
        let mut col = 0;
        let mut current = String::new();
        let mut in_quotes = false;
        let mut chars = line.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '"' {
                if in_quotes && chars.peek() == Some(&'"') {
                    current.push('"');
                    chars.next();
                } else {
                    in_quotes = !in_quotes;
                }
            } else if c == ',' && !in_quotes {
                if !current.is_empty() {
                    app.sheet.set_cell(col, row, current.clone());
                }
                current.clear();
                col += 1;
            } else {
                current.push(c);
            }
        }

        if !current.is_empty() {
            app.sheet.set_cell(col, row, current);
        }
    }

    app.cursor_col = 0;
    app.cursor_row = 0;
    app.view_col = 0;
    app.view_row = 0;
    app.selection_anchor = None;
    Ok(())
}

#[cfg(test)]
mod sanitize_tests {
    use super::sanitize_path_input;

    #[test]
    fn strips_double_quotes() {
        assert_eq!(sanitize_path_input("\"C:\\a\\b.xlsx\""), "C:\\a\\b.xlsx");
    }

    #[test]
    fn strips_single_quotes() {
        assert_eq!(sanitize_path_input("'C:\\a\\b.xlsx'"), "C:\\a\\b.xlsx");
    }

    #[test]
    fn leaves_unquoted_alone() {
        assert_eq!(sanitize_path_input("  C:\\a\\b.xlsx  "), "C:\\a\\b.xlsx");
    }

    #[test]
    fn does_not_strip_mismatched() {
        assert_eq!(sanitize_path_input("\"C:\\a\\b.xlsx"), "\"C:\\a\\b.xlsx");
    }

    #[test]
    fn handles_japanese_path() {
        let p = "\"C:\\Users\\n_fuk\\OneDrive\\デスクトップ\\Chapter1customers.xlsx\"";
        assert_eq!(
            sanitize_path_input(p),
            "C:\\Users\\n_fuk\\OneDrive\\デスクトップ\\Chapter1customers.xlsx"
        );
    }
}
