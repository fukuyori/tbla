use calamine::{open_workbook, Data, Reader, Xlsx};
use rust_xlsxwriter::{Formula, Workbook};
use std::path::Path;

use crate::cell::CellValue;
use crate::sheet::Sheet;

/// Result of reading an xlsx file. Carries the loaded sheet plus an optional
/// warning to surface to the user (e.g., multi-sheet workbooks).
pub struct ReadResult {
    pub sheet: Sheet,
    pub warning: Option<String>,
}

/// Read an .xlsx file into a single `Sheet`. If the workbook has multiple
/// sheets, only the first is loaded and a warning is returned. Formulas
/// are preserved as `=…` raw input and their Excel-cached values are
/// stashed in `Cell::cached_value` so SUM / display continues to work for
/// functions tbla does not implement.
pub fn read_xlsx<P: AsRef<Path>>(path: P) -> Result<ReadResult, String> {
    let mut workbook: Xlsx<_> = open_workbook(path.as_ref())
        .map_err(|e| format!("ファイルを開けません: {}", e))?;

    let sheet_names = workbook.sheet_names().to_vec();
    if sheet_names.is_empty() {
        return Err("ブックにシートがありません".to_string());
    }
    let primary = sheet_names[0].clone();

    let range = workbook
        .worksheet_range(&primary)
        .map_err(|e| format!("シート読み込みエラー: {}", e))?;
    let formulas = workbook.worksheet_formula(&primary).ok();

    let mut sheet = Sheet::new();
    sheet.name = primary.clone();

    for (row_idx, row) in range.rows().enumerate() {
        for (col_idx, cell) in row.iter().enumerate() {
            // Excel-cached value as a CellValue (used as fallback for
            // unsupported formulas, and as the literal value for non-formula
            // cells).
            let cached = data_to_cellvalue(cell);
            // Check whether this cell has a formula at the same position.
            let formula_text = formulas
                .as_ref()
                .and_then(|fr| fr.get_value((row_idx as u32, col_idx as u32)).cloned())
                .filter(|s: &String| !s.is_empty());

            match (formula_text, cached) {
                (Some(f), cached_val) => {
                    let input = if f.starts_with('=') { f } else { format!("={}", f) };
                    sheet.set_cell_with_cache(col_idx, row_idx, input, Some(cached_val));
                }
                (None, CellValue::Empty) => { /* skip */ }
                (None, val) => {
                    // Plain value: write as literal raw_input so editing
                    // and round-tripping work naturally.
                    let raw = match &val {
                        CellValue::Number(n) => format_number_raw(*n),
                        CellValue::Text(s) => s.clone(),
                        CellValue::Boolean(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
                        CellValue::Error(e) => e.to_string().to_string(),
                        CellValue::Empty | CellValue::Formula(_) => continue,
                    };
                    sheet.set_cell(col_idx, row_idx, raw);
                }
            }
        }
    }

    // Column widths: calamine exposes them via internal API on Xlsx; not
    // currently part of the public Reader trait, so we skip importing widths
    // for now. tbla's autowidth can be triggered manually by the user.

    let warning = if sheet_names.len() > 1 {
        Some(format!(
            "{} 枚のシートのうち最初の {:?} のみ読み込みました（他: {:?}）",
            sheet_names.len(),
            primary,
            &sheet_names[1..]
        ))
    } else {
        None
    };

    Ok(ReadResult { sheet, warning })
}

/// Write the current `Sheet` to an .xlsx file. Formulas are written as
/// Excel formulas (so the file recomputes when opened) and tbla's last
/// evaluated value is written as the cached result. Column widths are
/// preserved.
pub fn write_xlsx<P: AsRef<Path>>(sheet: &Sheet, path: P) -> Result<(), String> {
    let mut workbook = Workbook::new();
    let ws = workbook.add_worksheet();
    ws.set_name(&sheet.name)
        .map_err(|e| format!("シート名設定エラー: {}", e))?;

    let max_row = sheet.max_row().unwrap_or(0);
    let max_col = sheet.max_col().unwrap_or(0);

    for row in 0..=max_row {
        for col in 0..=max_col {
            let cell = sheet.get_cell(col, row);
            if cell.raw_input.is_empty() && matches!(cell.value, CellValue::Empty) {
                continue;
            }
            let r = row as u32;
            let c = col as u16;
            let result = match &cell.value {
                CellValue::Formula(_) => {
                    // Compute current display value as the cached result so
                    // viewers that don't recompute (rare) still see something.
                    let raw_input = cell.raw_input.trim_start_matches('=');
                    let display = sheet.evaluate(col, row);
                    let formula = Formula::new(raw_input)
                        .set_result(&display);
                    ws.write_formula(r, c, formula).map(|_| ())
                }
                CellValue::Number(n) => ws.write_number(r, c, *n).map(|_| ()),
                CellValue::Boolean(b) => ws.write_boolean(r, c, *b).map(|_| ()),
                CellValue::Text(s) => ws.write_string(r, c, s).map(|_| ()),
                CellValue::Error(e) => ws.write_string(r, c, e.to_string()).map(|_| ()),
                CellValue::Empty => Ok(()),
            };
            result.map_err(|e| format!("セル ({},{}) 書き込みエラー: {}", col, row, e))?;
        }
    }

    // Column widths: rust_xlsxwriter takes width in character units.
    // tbla's units match Excel's "character widths" closely enough.
    for col in 0..=max_col {
        let w = sheet.get_col_width(col) as f64;
        ws.set_column_width(col as u16, w)
            .map_err(|e| format!("列幅設定エラー: {}", e))?;
    }

    workbook.save(path.as_ref())
        .map_err(|e| format!("保存エラー: {}", e))?;
    Ok(())
}

fn data_to_cellvalue(d: &Data) -> CellValue {
    match d {
        Data::Empty => CellValue::Empty,
        Data::String(s) => CellValue::Text(s.clone()),
        Data::Float(f) => CellValue::Number(*f),
        Data::Int(i) => CellValue::Number(*i as f64),
        Data::Bool(b) => CellValue::Boolean(*b),
        Data::DateTime(dt) => CellValue::Number(dt.as_f64()),
        Data::DateTimeIso(s) => CellValue::Text(s.clone()),
        Data::DurationIso(s) => CellValue::Text(s.clone()),
        Data::Error(e) => CellValue::Text(format!("#{:?}", e)),
    }
}

fn format_number_raw(n: f64) -> String {
    // Avoid scientific notation in `raw_input` so round-trip preserves
    // the user's literal. Use the shortest representation that parses
    // back to the same f64.
    if n == n.floor() && n.abs() < 1e15 {
        format!("{:.0}", n)
    } else {
        let s = format!("{}", n);
        s
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::sheet::Sheet;

    fn tmp_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("tbla_xlsx_test_{}_{}.xlsx", std::process::id(), name));
        p
    }

    #[test]
    fn round_trip_values_and_formulas() {
        let mut s = Sheet::new();
        s.set_cell(0, 0, "ヘッダー".to_string());
        s.set_cell(0, 1, "10".to_string());
        s.set_cell(0, 2, "20".to_string());
        s.set_cell(0, 3, "=SUM(A2:A3)".to_string());
        s.set_cell(1, 0, "TRUE".to_string());
        s.set_col_width(0, 15);

        let path = tmp_path("round_trip");
        write_xlsx(&s, &path).expect("write");

        let result = read_xlsx(&path).expect("read");
        let s2 = result.sheet;

        assert_eq!(s2.get_cell(0, 0).raw_input, "ヘッダー");
        assert_eq!(s2.get_cell(0, 1).raw_input, "10");
        assert_eq!(s2.get_cell(0, 2).raw_input, "20");
        // Formula round-trips with leading '='
        assert!(s2.get_cell(0, 3).raw_input.starts_with('='));
        // Re-evaluated by tbla
        assert_eq!(s2.evaluate(0, 3), "30");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn unsupported_formula_falls_back_to_cached() {
        // Write a sheet manually where the formula is something tbla doesn't
        // implement, and a cached result is provided. Reading should fall
        // back to the cached value when evaluating fails.
        use rust_xlsxwriter::{Formula, Workbook};
        let path = tmp_path("unsupported");
        {
            let mut wb = Workbook::new();
            let ws = wb.add_worksheet();
            ws.write_number(0, 0, 1.0).unwrap();
            // BITAND isn't in tbla's engine. Cached result = 0.
            let f = Formula::new("BITAND(1,2)").set_result("0");
            ws.write_formula(1, 0, f).unwrap();
            wb.save(&path).unwrap();
        }

        let result = read_xlsx(&path).expect("read");
        let s = result.sheet;
        // raw_input preserved as formula
        assert!(s.get_cell(0, 1).raw_input.starts_with('='));
        // Displayed value falls back to cached "0"
        assert_eq!(s.evaluate(0, 1), "0");

        std::fs::remove_file(&path).ok();
    }
}
