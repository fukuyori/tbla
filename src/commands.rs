use crate::App;
use crate::cell::CellValue;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use serde::{Deserialize, Serialize};
use unicode_width::UnicodeWidthStr;

/// JSON file format for tbla (multi-sheet, version 2). The single-sheet
/// version 1 format is still read for backward compatibility.
#[derive(Serialize, Deserialize)]
struct TblaWorkbookFile {
    version: String,
    #[serde(default)]
    active: usize,
    sheets: Vec<TblaSheetFile>,
}

#[derive(Serialize, Deserialize)]
struct TblaSheetFile {
    name: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    col_widths: HashMap<String, usize>,
    cells: HashMap<String, CellData>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    conditional_formats: Vec<crate::sheet::ConditionalFormat>,
}

/// Legacy single-sheet format kept for backward compatibility.
#[derive(Serialize, Deserialize)]
struct TblaFile {
    #[allow(dead_code)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    format: Option<crate::cell::DisplayFormat>,
    #[serde(default, skip_serializing_if = "is_default_alignment")]
    alignment: crate::cell::Alignment,
    #[serde(default, skip_serializing_if = "is_false")]
    bold: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text_color: Option<crate::cell::RgbColor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bg_color: Option<crate::cell::RgbColor>,
}

fn is_default_alignment(a: &crate::cell::Alignment) -> bool { matches!(a, crate::cell::Alignment::Default) }
fn is_false(b: &bool) -> bool { !*b }

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
/// Replace all occurrences of `find` with `replace` across every cell
/// that contains the find substring (case-insensitive). The substitution
/// is applied to the cell's `raw_input` — so the user's literal text or
/// formula is rewritten, and re-parsed. Returns the number of cells that
/// were modified.
pub fn replace_all(app: &mut App, find: &str, replace: &str) -> usize {
    if find.is_empty() { return 0; }
    let find_lower = find.to_lowercase();
    let mut targets: Vec<(usize, usize, String)> = Vec::new();
    for ((c, r), cell) in app.sheet.cells().iter() {
        let raw = &cell.raw_input;
        if raw.to_lowercase().contains(&find_lower) {
            let new = replace_case_insensitive(raw, find, replace);
            if new != *raw {
                targets.push((*c, *r, new));
            }
        }
    }
    if targets.is_empty() { return 0; }
    app.save_undo();
    let count = targets.len();
    for (c, r, new) in targets {
        app.sheet.set_cell(c, r, new);
    }
    count
}

fn replace_case_insensitive(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() { return haystack.to_string(); }
    let mut out = String::with_capacity(haystack.len());
    let needle_lower = needle.to_lowercase();
    let chars: Vec<char> = haystack.chars().collect();
    let needle_chars: Vec<char> = needle.chars().collect();
    let needle_len = needle_chars.len();

    let mut i = 0;
    while i < chars.len() {
        if i + needle_len <= chars.len() {
            let slice: String = chars[i..i + needle_len].iter().collect();
            if slice.to_lowercase() == needle_lower {
                out.push_str(replacement);
                i += needle_len;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Sort the data area's rows by the values in `sort_col`. When `header` is
/// true, the first row is left in place. The selection is ignored — we always
/// operate on the bounding rectangle of all data, top to bottom.
///
/// Formulas inside the sort range are NOT rewritten when their rows move, so
/// formulas referencing other cells in the same row by ABSOLUTE row number
/// can break. Relative-only formulas (e.g. `=A1+B1` when A and B are in the
/// same row) stay correct because they keep their relative offsets.
pub fn sort_rows(app: &mut App, sort_col: usize, descending: bool, header: bool) -> usize {
    let max_row = match app.sheet.max_row() { Some(r) => r, None => return 0 };
    let max_col = match app.sheet.max_col() { Some(c) => c, None => return 0 };
    let first = if header { 1 } else { 0 };
    if first > max_row { return 0; }

    // Collect rows to sort as (sort_key, full_row_cells).
    #[derive(Clone)]
    enum Key { Num(f64), Text(String), Empty }
    impl Key {
        fn from(s: &str) -> Self {
            let t = s.trim();
            if t.is_empty() { return Key::Empty; }
            if let Ok(n) = t.parse::<f64>() { return Key::Num(n); }
            Key::Text(t.to_lowercase())
        }
    }
    fn order(a: &Key, b: &Key) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        match (a, b) {
            (Key::Empty, Key::Empty) => Equal,
            (Key::Empty, _) => Greater, // empties go last in ascending
            (_, Key::Empty) => Less,
            (Key::Num(x), Key::Num(y)) => x.partial_cmp(y).unwrap_or(Equal),
            (Key::Text(x), Key::Text(y)) => x.cmp(y),
            (Key::Num(_), Key::Text(_)) => Less,    // numbers before text
            (Key::Text(_), Key::Num(_)) => Greater,
        }
    }

    let mut rows: Vec<(Key, Vec<Option<crate::cell::Cell>>)> = Vec::with_capacity(max_row - first + 1);
    for r in first..=max_row {
        let key = Key::from(&app.sheet.evaluate(sort_col, r));
        let mut row_cells: Vec<Option<crate::cell::Cell>> = Vec::with_capacity(max_col + 1);
        for c in 0..=max_col {
            row_cells.push(app.sheet.get_cell_ref(c, r).cloned());
        }
        rows.push((key, row_cells));
    }

    let mut sorted = rows.clone();
    sorted.sort_by(|a, b| {
        let cmp = order(&a.0, &b.0);
        if descending { cmp.reverse() } else { cmp }
    });
    if sorted.iter().zip(rows.iter()).all(|(a, b)| std::ptr::eq(a, b)) {
        // Trivially already sorted (rare false positive); still run save_undo
    }

    app.save_undo();
    // Write back the sorted rows.
    for (i, (_, row_cells)) in sorted.into_iter().enumerate() {
        let r = first + i;
        for (c, opt) in row_cells.into_iter().enumerate() {
            match opt {
                Some(cell) => {
                    app.sheet.set_cell_with_cache(c, r, cell.raw_input, cell.cached_value);
                }
                None => {
                    app.sheet.clear_cell(c, r);
                }
            }
        }
    }
    max_row - first + 1
}

/// Hide rows that don't match the given criteria. Criteria syntax follows
/// the same shape as `COUNTIF`: bare value (`=`), `>10`, `>=A`, `<>foo`,
/// or `*text*` for substring contains. Returns the count of rows hidden.
/// Replaces any prior filter state.
pub fn apply_filter(app: &mut App, filter_col: usize, criteria: &str, header: bool) -> usize {
    app.hidden_rows.clear();
    if criteria.is_empty() { return 0; }
    let max_row = match app.sheet.max_row() { Some(r) => r, None => return 0 };
    let first = if header { 1 } else { 0 };
    let crit = parse_filter_criteria(criteria);
    let mut hidden = 0;
    for r in first..=max_row {
        let value = app.sheet.evaluate(filter_col, r);
        if !crit.matches(&value) {
            app.hidden_rows.insert(r);
            hidden += 1;
        }
    }
    hidden
}

enum FilterOp { Eq, Ne, Gt, Lt, Ge, Le, Contains }
struct FilterCriteria { op: FilterOp, target: String }

impl FilterCriteria {
    fn matches(&self, value: &str) -> bool {
        match self.op {
            FilterOp::Contains => {
                value.to_lowercase().contains(&self.target.to_lowercase())
            }
            _ => {
                let v_num = value.parse::<f64>().ok();
                let t_num = self.target.parse::<f64>().ok();
                if let (Some(v), Some(t)) = (v_num, t_num) {
                    match self.op {
                        FilterOp::Eq => (v - t).abs() < 1e-12 * v.abs().max(t.abs()).max(1.0),
                        FilterOp::Ne => (v - t).abs() >= 1e-12 * v.abs().max(t.abs()).max(1.0),
                        FilterOp::Gt => v > t,
                        FilterOp::Lt => v < t,
                        FilterOp::Ge => v >= t,
                        FilterOp::Le => v <= t,
                        FilterOp::Contains => unreachable!(),
                    }
                } else {
                    let v = value.to_lowercase();
                    let t = self.target.to_lowercase();
                    match self.op {
                        FilterOp::Eq => v == t,
                        FilterOp::Ne => v != t,
                        FilterOp::Gt => v > t,
                        FilterOp::Lt => v < t,
                        FilterOp::Ge => v >= t,
                        FilterOp::Le => v <= t,
                        FilterOp::Contains => unreachable!(),
                    }
                }
            }
        }
    }
}

fn parse_filter_criteria(s: &str) -> FilterCriteria {
    let s = s.trim();
    // Substring match: *foo* or starts/ends with *
    if s.starts_with('*') && s.ends_with('*') && s.len() >= 2 {
        return FilterCriteria { op: FilterOp::Contains, target: s.trim_matches('*').to_string() };
    }
    for op in &[">=", "<=", "<>", "!=", "=", ">", "<"] {
        if let Some(rest) = s.strip_prefix(op) {
            let target = rest.trim().trim_matches('"').to_string();
            let kind = match *op {
                ">=" => FilterOp::Ge,
                "<=" => FilterOp::Le,
                "<>" | "!=" => FilterOp::Ne,
                "=" => FilterOp::Eq,
                ">" => FilterOp::Gt,
                "<" => FilterOp::Lt,
                _ => FilterOp::Eq,
            };
            return FilterCriteria { op: kind, target };
        }
    }
    // Bare value → equality
    FilterCriteria { op: FilterOp::Eq, target: s.trim_matches('"').to_string() }
}

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
            // Materialize the workbook in order (active sheet at its logical
            // position) so multi-sheet workbooks round-trip.
            let sheets: Vec<crate::sheet::Sheet> = app.workbook_sheets()
                .iter().map(|&s| s.clone()).collect();
            crate::xlsx::write_xlsx_sheets(&sheets, &filename)?;
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
            if result.sheets.is_empty() {
                return Err("ブックにシートがありません".to_string());
            }
            app.save_undo();
            let mut sheets = result.sheets;
            let active = sheets.remove(0);
            app.sheet = active;
            app.other_sheets = sheets;
            app.active_sheet_index = 0;
            app.cursor_col = 0; app.cursor_row = 0;
            app.view_col = 0; app.view_row = 0;
            app.selection_anchor = None;
            app.hidden_rows.clear();
            if let Some(w) = result.warning { app.status_message = w; }
            Ok(())
        }
        _ => load_json(app, filename).map_err(|e| e.to_string()),
    }
}

fn save_json(app: &App, filename: &str) -> std::io::Result<()> {
    use crate::sheet::DEFAULT_COL_WIDTH;

    let serialize_sheet = |sheet: &crate::sheet::Sheet| -> TblaSheetFile {
        let mut col_widths = HashMap::new();
        for col in 0..=255 {
            let width = sheet.get_col_width(col);
            if width != DEFAULT_COL_WIDTH {
                col_widths.insert(crate::formula::col_to_name(col), width);
            }
        }
        let mut cells = HashMap::new();
        for ((col, row), cell) in sheet.cells().iter() {
            let cell_name = crate::formula::cell_name(*col, *row);
            let (value, formula) = match &cell.value {
                CellValue::Formula(_) => (sheet.evaluate(*col, *row), Some(cell.raw_input.clone())),
                _ => (cell.raw_input.clone(), None),
            };
            cells.insert(cell_name, CellData {
                value, formula,
                format: if matches!(cell.format, crate::cell::DisplayFormat::General) { None } else { Some(cell.format.clone()) },
                alignment: cell.alignment,
                bold: cell.bold,
                text_color: cell.text_color,
                bg_color: cell.bg_color,
            });
        }
        TblaSheetFile {
            name: sheet.name.clone(),
            col_widths,
            cells,
            conditional_formats: sheet.conditional_formats.clone(),
        }
    };

    let sheets: Vec<TblaSheetFile> = app.workbook_sheets().iter().map(|s| serialize_sheet(s)).collect();
    let file_data = TblaWorkbookFile {
        version: "2.0".to_string(),
        active: app.active_sheet_index,
        sheets,
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

    // Try the new (multi-sheet) format first, fall back to the legacy
    // single-sheet format.
    let workbook = match serde_json::from_str::<TblaWorkbookFile>(&contents) {
        Ok(wb) if !wb.sheets.is_empty() => wb,
        _ => {
            let legacy: TblaFile = serde_json::from_str(&contents)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            TblaWorkbookFile {
                version: "2.0".to_string(),
                active: 0,
                sheets: vec![TblaSheetFile {
                    name: legacy.name,
                    col_widths: legacy.col_widths,
                    cells: legacy.cells,
                    conditional_formats: Vec::new(),
                }],
            }
        }
    };

    fn build_sheet(data: TblaSheetFile) -> crate::sheet::Sheet {
        let mut s = crate::sheet::Sheet::new();
        s.name = data.name;
        for (col_name, width) in data.col_widths {
            if let Some((col, _, _, _)) = crate::formula::parse_cell_ref(&format!("{}1", col_name)) {
                s.set_col_width(col, width);
            }
        }
        for (cell_name, cell_data) in data.cells {
            if let Some((col, row, _, _)) = crate::formula::parse_cell_ref(&cell_name) {
                let input = cell_data.formula.unwrap_or(cell_data.value);
                s.set_cell(col, row, input);
                let cell = s.cell_format_mut(col, row);
                if let Some(fmt) = cell_data.format { cell.format = fmt; }
                cell.alignment = cell_data.alignment;
                cell.bold = cell_data.bold;
                cell.text_color = cell_data.text_color;
                cell.bg_color = cell_data.bg_color;
            }
        }
        s.conditional_formats = data.conditional_formats;
        s
    }

    app.save_undo();
    let mut sheets: Vec<crate::sheet::Sheet> = workbook.sheets.into_iter().map(build_sheet).collect();
    let active = workbook.active.min(sheets.len().saturating_sub(1));
    let active_sheet = sheets.remove(active);
    app.sheet = active_sheet;
    app.other_sheets = sheets;
    app.active_sheet_index = active;
    app.cursor_col = 0;
    app.cursor_row = 0;
    app.view_col = 0;
    app.view_row = 0;
    app.selection_anchor = None;
    app.hidden_rows.clear();
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

#[cfg(test)]
mod data_ops_tests {
    use super::*;
    use crate::App;
    use crate::sheet::Sheet;

    fn app_with(cells: &[(usize, usize, &str)]) -> App {
        let mut a = App::new();
        a.sheet = Sheet::new();
        for (c, r, v) in cells {
            a.sheet.set_cell(*c, *r, v.to_string());
        }
        a
    }

    #[test]
    fn replace_all_substitutes_case_insensitive() {
        let mut a = app_with(&[
            (0, 0, "Hello world"),
            (0, 1, "HELLO again"),
            (0, 2, "no match"),
        ]);
        let n = replace_all(&mut a, "hello", "Hi");
        assert_eq!(n, 2);
        assert_eq!(a.sheet.get_cell(0, 0).raw_input, "Hi world");
        assert_eq!(a.sheet.get_cell(0, 1).raw_input, "Hi again");
        assert_eq!(a.sheet.get_cell(0, 2).raw_input, "no match");
    }

    #[test]
    fn sort_rows_ascending_with_header() {
        // header row A1; data sorted by column A
        let mut a = app_with(&[
            (0, 0, "name"), (1, 0, "score"),
            (0, 1, "Charlie"), (1, 1, "30"),
            (0, 2, "Alice"),   (1, 2, "10"),
            (0, 3, "Bob"),     (1, 3, "20"),
        ]);
        sort_rows(&mut a, 0, false, true);
        // Header preserved
        assert_eq!(a.sheet.get_cell(0, 0).raw_input, "name");
        // Rows sorted by name asc: Alice, Bob, Charlie
        assert_eq!(a.sheet.get_cell(0, 1).raw_input, "Alice");
        assert_eq!(a.sheet.get_cell(0, 2).raw_input, "Bob");
        assert_eq!(a.sheet.get_cell(0, 3).raw_input, "Charlie");
        // Companion column moved with the row
        assert_eq!(a.sheet.get_cell(1, 1).raw_input, "10");
        assert_eq!(a.sheet.get_cell(1, 2).raw_input, "20");
        assert_eq!(a.sheet.get_cell(1, 3).raw_input, "30");
    }

    #[test]
    fn sort_rows_descending_numeric() {
        let mut a = app_with(&[
            (0, 0, "5"),
            (0, 1, "1"),
            (0, 2, "10"),
            (0, 3, "3"),
        ]);
        sort_rows(&mut a, 0, true, false);
        assert_eq!(a.sheet.get_cell(0, 0).raw_input, "10");
        assert_eq!(a.sheet.get_cell(0, 1).raw_input, "5");
        assert_eq!(a.sheet.get_cell(0, 2).raw_input, "3");
        assert_eq!(a.sheet.get_cell(0, 3).raw_input, "1");
    }

    #[test]
    fn filter_hides_non_matching_rows() {
        let mut a = app_with(&[
            (0, 0, "fruit"), (1, 0, "kind"),
            (0, 1, "apple"), (1, 1, "fruit"),
            (0, 2, "kale"),  (1, 2, "veg"),
            (0, 3, "pear"),  (1, 3, "fruit"),
        ]);
        // Filter column B (=1) for "fruit" with header row
        let hidden = apply_filter(&mut a, 1, "fruit", true);
        assert_eq!(hidden, 1); // kale row hidden
        assert!(a.hidden_rows.contains(&2));
        assert!(!a.hidden_rows.contains(&0)); // header
        assert!(!a.hidden_rows.contains(&1));
        assert!(!a.hidden_rows.contains(&3));
    }

    #[test]
    fn filter_numeric_comparison() {
        let mut a = app_with(&[
            (0, 0, "5"),
            (0, 1, "15"),
            (0, 2, "25"),
        ]);
        let hidden = apply_filter(&mut a, 0, ">=15", false);
        assert_eq!(hidden, 1);
        assert!(a.hidden_rows.contains(&0));
    }

    #[test]
    fn filter_substring() {
        let mut a = app_with(&[
            (0, 0, "user@example.com"),
            (0, 1, "admin@example.com"),
            (0, 2, "noreply@other.org"),
        ]);
        let hidden = apply_filter(&mut a, 0, "*example*", false);
        assert_eq!(hidden, 1);
        assert!(a.hidden_rows.contains(&2));
    }
}
