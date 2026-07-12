use calamine::{open_workbook, Data, Reader, Xlsx};
use rust_xlsxwriter::{Color as XColor, ConditionalFormat2ColorScale, ConditionalFormatCell, ConditionalFormatCellRule, ConditionalFormatDataBar, Format, FormatAlign, Formula, Workbook};
use std::path::Path;

use crate::cell::{Alignment, Cell, CellValue, DisplayFormat, RgbColor};
use crate::sheet::{CondCondition, CondOp, ConditionalFormat as CondFmt, Sheet};

/// Result of reading an xlsx file. Carries the loaded sheets in workbook
/// order plus an optional warning to surface to the user.
pub struct ReadResult {
    pub sheets: Vec<Sheet>,
    pub warning: Option<String>,
    /// Workbook-level defined names that map to a simple cell/range
    /// reference. Complex defined names (multi-area, formulas) are skipped.
    pub names: Vec<crate::NamedRange>,
}

/// Read an .xlsx file. ALL sheets are loaded in workbook order. Formulas
/// are preserved as `=…` raw input and their Excel-cached values are
/// stashed in `Cell::cached_value` so SUM / display continues to work for
/// functions tbla does not implement.
pub fn read_xlsx<P: AsRef<Path>>(path: P) -> Result<ReadResult, String> {
    let path_str = path.as_ref().to_string_lossy().to_string();
    let mut workbook: Xlsx<_> = open_workbook(path.as_ref())
        .map_err(|e| format!("ファイルを開けません: {}", e))?;

    let sheet_names = workbook.sheet_names().to_vec();
    if sheet_names.is_empty() {
        return Err("ブックにシートがありません".to_string());
    }

    // Best-effort: hand-parse styles for bg/font color + alignment. We
    // tolerate a failure (corrupted/encrypted xlsx) by treating it as
    // "no styles" rather than failing the entire read.
    let styles = crate::xlsx_styles::read_workbook_styles(&path_str).ok();

    let mut sheets: Vec<Sheet> = Vec::with_capacity(sheet_names.len());
    for name in &sheet_names {
        let range = workbook
            .worksheet_range(name)
            .map_err(|e| format!("シート読み込みエラー ({}): {}", name, e))?;
        let formulas = workbook.worksheet_formula(name).ok();
        let mut sheet = Sheet::new();
        sheet.name = name.clone();
        for (row_idx, row) in range.rows().enumerate() {
            for (col_idx, cell) in row.iter().enumerate() {
                let cached = data_to_cellvalue(cell);
                let formula_text = formulas.as_ref()
                    .and_then(|fr| fr.get_value((row_idx as u32, col_idx as u32)).cloned())
                    .filter(|s: &String| !s.is_empty());
                match (formula_text, cached) {
                    (Some(f), cached_val) => {
                        let input = if f.starts_with('=') { f } else { format!("={}", f) };
                        sheet.set_cell_with_cache(col_idx, row_idx, input, Some(cached_val));
                    }
                    (None, CellValue::Empty) => {}
                    (None, val) => {
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
        // Apply per-cell formatting + conditional formats parsed from the
        // xlsx (best effort).
        if let Some(styles_wb) = styles.as_ref() {
            if let Some(idx) = styles_wb.sheet_names.iter().position(|n| n == name) {
                if let Some(cell_styles) = styles_wb.sheet_styles.get(idx) {
                    for ((c, r), st) in cell_styles {
                        let cell = sheet.cell_format_mut(*c, *r);
                        if st.font_color.is_some() { cell.text_color = st.font_color; }
                        if st.bg_color.is_some() { cell.bg_color = st.bg_color; }
                        if !matches!(st.alignment, crate::cell::Alignment::Default) {
                            cell.alignment = st.alignment;
                        }
                        if st.bold { cell.bold = true; }
                        if st.italic { cell.italic = true; }
                        if st.underline { cell.underline = true; }
                    }
                }
                if let Some(cfs) = styles_wb.sheet_conditionals.get(idx) {
                    sheet.conditional_formats.extend(cfs.iter().cloned());
                }
            }
        }
        sheets.push(sheet);
    }

    let names = workbook
        .defined_names()
        .iter()
        .filter(|(n, _)| !n.starts_with("_xlnm")) // built-ins like Print_Area
        .filter_map(|(n, f)| parse_defined_name(n, f))
        .collect();

    Ok(ReadResult { sheets, warning: None, names })
}

/// Parse an xlsx defined-name target like `Sheet1!$A$1:$B$2` or
/// `'My Sheet'!$A$1` into a NamedRange. Returns None for anything more
/// complex (multi-area lists, constants, formulas).
fn parse_defined_name(name: &str, formula: &str) -> Option<crate::NamedRange> {
    let f = formula.trim().trim_start_matches('=');
    if f.contains(',') {
        return None;
    }
    let bang = f.rfind('!')?;
    let sheet = f[..bang].trim().trim_matches('\'').to_string();
    let refs = &f[bang + 1..];
    let parts: Vec<&str> = refs.split(':').collect();
    let (start, end) = match parts.as_slice() {
        [one] => {
            let (c, r, _, _) = crate::formula::parse_cell_ref(one)?;
            ((c, r), (c, r))
        }
        [a, b] => {
            let (c1, r1, _, _) = crate::formula::parse_cell_ref(a)?;
            let (c2, r2, _, _) = crate::formula::parse_cell_ref(b)?;
            ((c1.min(c2), r1.min(r2)), (c1.max(c2), r1.max(r2)))
        }
        _ => return None,
    };
    Some(crate::NamedRange { name: name.to_string(), sheet, start, end })
}

/// Write a single `Sheet` to an .xlsx file. Convenience wrapper around
/// `write_xlsx_sheets` for the single-sheet case (used by tests).
#[allow(dead_code)]
pub fn write_xlsx<P: AsRef<Path>>(sheet: &Sheet, path: P) -> Result<(), String> {
    write_xlsx_sheets(std::slice::from_ref(sheet), &[], path)
}

/// Write every sheet in `sheets` to an .xlsx file in the given order.
/// Formulas are written as Excel formulas (so the file recomputes when
/// opened) and tbla's last evaluated value is written as the cached result.
/// Column widths are preserved. `names` become workbook-level defined names.
pub fn write_xlsx_sheets<P: AsRef<Path>>(
    sheets: &[Sheet],
    names: &[crate::NamedRange],
    path: P,
) -> Result<(), String> {
    let mut workbook = Workbook::new();
    for nr in names {
        let quoted_sheet = if nr.sheet.chars().all(|c| c.is_alphanumeric() || c == '_') {
            nr.sheet.clone()
        } else {
            format!("'{}'", nr.sheet)
        };
        let abs = |c: usize, r: usize| {
            format!("${}${}", crate::formula::col_to_name(c), r + 1)
        };
        let target = if nr.start == nr.end {
            format!("={}!{}", quoted_sheet, abs(nr.start.0, nr.start.1))
        } else {
            format!(
                "={}!{}:{}",
                quoted_sheet,
                abs(nr.start.0, nr.start.1),
                abs(nr.end.0, nr.end.1),
            )
        };
        // Best effort — an invalid name must not fail the whole save.
        let _ = workbook.define_name(&nr.name, &target);
    }
    for sheet in sheets {
        let ws = workbook.add_worksheet();
        ws.set_name(&sheet.name)
            .map_err(|e| format!("シート名設定エラー ({}): {}", sheet.name, e))?;
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
                let format = build_format(&cell);
                let result = match (&cell.value, format.as_ref()) {
                    (CellValue::Formula(_), fmt) => {
                        let raw_input = cell.raw_input.trim_start_matches('=');
                        let display = sheet.evaluate(col, row);
                        let formula = Formula::new(raw_input).set_result(&display);
                        match fmt {
                            Some(f) => ws.write_formula_with_format(r, c, formula, f).map(|_| ()),
                            None => ws.write_formula(r, c, formula).map(|_| ()),
                        }
                    }
                    (CellValue::Number(n), fmt) => match fmt {
                        Some(f) => ws.write_number_with_format(r, c, *n, f).map(|_| ()),
                        None => ws.write_number(r, c, *n).map(|_| ()),
                    },
                    (CellValue::Boolean(b), fmt) => match fmt {
                        Some(f) => ws.write_boolean_with_format(r, c, *b, f).map(|_| ()),
                        None => ws.write_boolean(r, c, *b).map(|_| ()),
                    },
                    (CellValue::Text(s), fmt) => match fmt {
                        Some(f) => ws.write_string_with_format(r, c, s, f).map(|_| ()),
                        None => ws.write_string(r, c, s).map(|_| ()),
                    },
                    (CellValue::Error(e), fmt) => match fmt {
                        Some(f) => ws.write_string_with_format(r, c, e.to_string(), f).map(|_| ()),
                        None => ws.write_string(r, c, e.to_string()).map(|_| ()),
                    },
                    (CellValue::Empty, Some(f)) => ws.write_blank(r, c, f).map(|_| ()),
                    (CellValue::Empty, None) => Ok(()),
                };
                result.map_err(|e| format!("セル {} ({},{}) 書き込みエラー: {}", sheet.name, col, row, e))?;
            }
        }
        for col in 0..=max_col {
            let w = sheet.get_col_width(col) as f64;
            ws.set_column_width(col as u16, w)
                .map_err(|e| format!("列幅設定エラー ({}): {}", sheet.name, e))?;
        }
        // Conditional-formatting rules: translate each rule to the Excel
        // equivalent. Unsupported variants are best-effort.
        for rule in &sheet.conditional_formats {
            apply_conditional(ws, rule).map_err(|e| format!("条件付き書式エラー ({}): {}", sheet.name, e))?;
        }
    }
    workbook.save(path.as_ref())
        .map_err(|e| format!("保存エラー: {}", e))?;
    Ok(())
}

fn rgb_to_xcolor(rgb: RgbColor) -> XColor {
    XColor::RGB(((rgb.0 as u32) << 16) | ((rgb.1 as u32) << 8) | (rgb.2 as u32))
}

fn build_format(cell: &Cell) -> Option<Format> {
    if !cell.has_format() && cell.text_color.is_none() && cell.bg_color.is_none() {
        return None;
    }
    let mut f = Format::new();
    if cell.bold { f = f.set_bold(); }
    if cell.italic { f = f.set_italic(); }
    if cell.underline { f = f.set_underline(rust_xlsxwriter::FormatUnderline::Single); }
    if let Some(tc) = cell.text_color { f = f.set_font_color(rgb_to_xcolor(tc)); }
    if let Some(bc) = cell.bg_color { f = f.set_background_color(rgb_to_xcolor(bc)); }
    match cell.alignment {
        Alignment::Left => f = f.set_align(FormatAlign::Left),
        Alignment::Center => f = f.set_align(FormatAlign::Center),
        Alignment::Right => f = f.set_align(FormatAlign::Right),
        Alignment::Default => {}
    }
    // Number format. Numeric kinds get a "positive;negative" pair when the
    // cell asks for parenthesized / red negatives (Excel's [Red] syntax).
    let dec = |base: &str, d: &usize| if *d == 0 {
        base.to_string()
    } else {
        format!("{}.{}", base, "0".repeat(*d))
    };
    let numeric_base = match &cell.format {
        DisplayFormat::Number(d) => Some(dec("0", d)),
        DisplayFormat::Comma(d) => Some(dec("#,##0", d)),
        DisplayFormat::Currency(d) => Some(format!("¥{}", dec("#,##0", d))),
        DisplayFormat::Percent(d) => Some(format!("{}%", dec("0", d))),
        DisplayFormat::Scientific => Some("0.00E+00".to_string()),
        _ => None,
    };
    f = match (&cell.format, numeric_base) {
        (_, Some(base)) => {
            if cell.neg_parens || cell.neg_red {
                let red = if cell.neg_red { "[Red]" } else { "" };
                let neg = if cell.neg_parens {
                    format!("{}({})", red, base)
                } else {
                    format!("{}-{}", red, base)
                };
                f.set_num_format(format!("{};{}", base, neg))
            } else {
                f.set_num_format(base)
            }
        }
        (DisplayFormat::Date, _) => f.set_num_format("yyyy-mm-dd"),
        (DisplayFormat::DateTime, _) => f.set_num_format("yyyy-mm-dd hh:mm"),
        (DisplayFormat::Time, _) => f.set_num_format("hh:mm:ss"),
        (DisplayFormat::Text, _) => f.set_num_format("@"),
        _ => f,
    };
    Some(f)
}

fn apply_conditional(ws: &mut rust_xlsxwriter::Worksheet, rule: &CondFmt) -> Result<(), String> {
    let first_row = rule.min_row as u32;
    let first_col = rule.min_col as u16;
    let last_row = rule.max_row as u32;
    let last_col = rule.max_col as u16;
    match &rule.condition {
        CondCondition::Compare { op, target } => {
            let cell_rule = match op {
                CondOp::Gt => ConditionalFormatCellRule::GreaterThan(*target),
                CondOp::Lt => ConditionalFormatCellRule::LessThan(*target),
                CondOp::Ge => ConditionalFormatCellRule::GreaterThanOrEqualTo(*target),
                CondOp::Le => ConditionalFormatCellRule::LessThanOrEqualTo(*target),
                CondOp::Eq => ConditionalFormatCellRule::EqualTo(*target),
                CondOp::Ne => ConditionalFormatCellRule::NotEqualTo(*target),
            };
            let mut fmt = Format::new();
            if let Some(bg) = rule.bg_color { fmt = fmt.set_background_color(rgb_to_xcolor(bg)); }
            if let Some(fg) = rule.text_color { fmt = fmt.set_font_color(rgb_to_xcolor(fg)); }
            let cf = ConditionalFormatCell::new().set_rule(cell_rule).set_format(fmt);
            ws.add_conditional_format(first_row, first_col, last_row, last_col, &cf)
                .map_err(|e| e.to_string())?;
        }
        CondCondition::ColorScale { min_color, max_color, .. } => {
            let cf = ConditionalFormat2ColorScale::new()
                .set_minimum_color(rgb_to_xcolor(*min_color))
                .set_maximum_color(rgb_to_xcolor(*max_color));
            ws.add_conditional_format(first_row, first_col, last_row, last_col, &cf)
                .map_err(|e| e.to_string())?;
        }
        CondCondition::DataBar { bar_color, .. } => {
            let cf = ConditionalFormatDataBar::new()
                .set_fill_color(rgb_to_xcolor(*bar_color));
            ws.add_conditional_format(first_row, first_col, last_row, last_col, &cf)
                .map_err(|e| e.to_string())?;
        }
    }
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
        assert_eq!(result.sheets.len(), 1);
        let s2 = &result.sheets[0];

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
        let s = &result.sheets[0];
        // raw_input preserved as formula
        assert!(s.get_cell(0, 1).raw_input.starts_with('='));
        // Displayed value falls back to cached "0"
        assert_eq!(s.evaluate(0, 1), "0");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn read_users_book1_data_bar() {
        // Sanity-check against the user-provided file with a dataBar rule.
        // Skip silently if the file isn't present (CI / other users).
        let path = "/Users/fukuyori/Downloads/Book 1.xlsx";
        if !std::path::Path::new(path).exists() {
            eprintln!("skipping: {} not present", path);
            return;
        }
        let r = read_xlsx(path).expect("read Book 1.xlsx");
        assert_eq!(r.sheets.len(), 1);
        let s = &r.sheets[0];
        // Values
        assert_eq!(s.get_cell(0, 0).raw_input, "-1");
        assert_eq!(s.get_cell(1, 0).raw_input, "10");
        // Data bar conditional format
        assert!(!s.conditional_formats.is_empty(), "expected a conditional format");
        let cf = &s.conditional_formats[0];
        assert!(matches!(cf.condition, crate::sheet::CondCondition::DataBar { .. }),
            "expected DataBar, got {:?}", cf.condition);
        assert_eq!((cf.min_col, cf.min_row, cf.max_col, cf.max_row), (0, 0, 2, 2));
    }

    #[test]
    fn cell_formats_round_trip_through_xlsx() {
        // Write a sheet with bold + colors + alignment, read it back, and
        // verify the formatting survives.
        let mut s = Sheet::new();
        s.set_cell(0, 0, "Header".to_string());
        s.set_cell(0, 1, "100".to_string());
        s.set_cell(1, 1, "200".to_string());

        {
            let c = s.cell_format_mut(0, 0);
            c.bold = true;
            c.alignment = crate::cell::Alignment::Center;
            c.text_color = Some((10, 10, 200));
            c.bg_color = Some((255, 240, 200));
        }
        {
            let c = s.cell_format_mut(0, 1);
            c.alignment = crate::cell::Alignment::Right;
            c.bg_color = Some((220, 255, 220));
            c.italic = true;
            c.underline = true;
        }

        let path = tmp_path("formats_round_trip");
        write_xlsx(&s, &path).expect("write");

        let result = read_xlsx(&path).expect("read");
        let s2 = &result.sheets[0];

        let c00 = s2.get_cell(0, 0);
        assert_eq!(c00.raw_input, "Header");
        assert!(c00.bold, "bold should survive round-trip");
        assert!(matches!(c00.alignment, crate::cell::Alignment::Center),
            "center alignment should survive (got {:?})", c00.alignment);
        assert_eq!(c00.text_color, Some((10, 10, 200)));
        assert_eq!(c00.bg_color, Some((255, 240, 200)));

        let c01 = s2.get_cell(0, 1);
        assert!(matches!(c01.alignment, crate::cell::Alignment::Right));
        assert_eq!(c01.bg_color, Some((220, 255, 220)));
        assert!(c01.italic, "italic should survive round-trip");
        assert!(c01.underline, "underline should survive round-trip");

        std::fs::remove_file(&path).ok();
    }
}
