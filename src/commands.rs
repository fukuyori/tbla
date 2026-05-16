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
            app.status_message = format!("読み込みエラー: {}", e);
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
        "csv" => {
            export_csv(app, &filename).map_err(|e| e.to_string())?;
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
        "csv" => import_csv(app, filename).map_err(|e| e.to_string()),
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
