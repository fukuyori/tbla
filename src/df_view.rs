//! Polars DataFrame view for a sheet (Phase 1: convert + read-only display).
//!
//! When `Sheet.df_view` is `Some`, the grid renders from this view instead of
//! the cell HashMap. The underlying `cells` is preserved untouched so the user
//! can switch back to the cell view without losing their work.

use polars::prelude::*;
use polars::sql::SQLContext;

use crate::sheet::Sheet;

/// A user-added computed column built from a Polars SQL expression. The
/// expression text is the right-hand side of the `name = expr` form the
/// user types (e.g. "price * qty", `IF(qty > 0, total / qty, 0)`).
#[derive(Clone, Debug)]
pub struct ComputedColumn {
    pub name: String,
    pub expr: String,
}

/// Read-only DataFrame view of a sheet. Held inside `Sheet.df_view`.
#[derive(Clone, Debug)]
pub struct DataFrameView {
    pub df: DataFrame,
    /// Computed columns applied so far, in insertion order. Kept around so
    /// the view can be rebuilt deterministically (e.g. when adding another
    /// computed column on top, or reverting to the raw cells).
    pub computed: Vec<ComputedColumn>,
}

impl DataFrameView {
    pub fn rows(&self) -> usize { self.df.height() }
    pub fn cols(&self) -> usize { self.df.width() }

    /// Display value at (col, row) in the DataFrame. Returns "" for out of
    /// range or null cells. Formats per the column's dtype.
    pub fn value_at(&self, col: usize, row: usize) -> String {
        let Some(series) = self.df.columns().get(col) else { return String::new(); };
        if row >= series.len() { return String::new(); }
        match series.get(row) {
            Ok(AnyValue::Null) => String::new(),
            Ok(AnyValue::String(s)) => s.to_string(),
            Ok(AnyValue::StringOwned(s)) => s.to_string(),
            Ok(AnyValue::Int64(n)) => n.to_string(),
            Ok(AnyValue::Int32(n)) => n.to_string(),
            Ok(AnyValue::UInt64(n)) => n.to_string(),
            Ok(AnyValue::UInt32(n)) => n.to_string(),
            Ok(AnyValue::Float64(n)) => format_float(n),
            Ok(AnyValue::Float32(n)) => format_float(n as f64),
            Ok(AnyValue::Boolean(b)) => (if b { "TRUE" } else { "FALSE" }).to_string(),
            Ok(other) => other.to_string(),
            Err(_) => String::new(),
        }
    }

    /// Header at column index, or empty string if out of range.
    pub fn header(&self, col: usize) -> String {
        self.df.get_column_names()
            .get(col)
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    /// True if the column at `col` is numeric (used to decide right-align).
    pub fn is_numeric(&self, col: usize) -> bool {
        self.df.columns().get(col)
            .map(|s| s.dtype().is_primitive_numeric())
            .unwrap_or(false)
    }

    /// Compact list of dtype names, for status-bar display.
    pub fn dtype_summary(&self, max: usize) -> String {
        let names: Vec<String> = self.df.dtypes().iter().take(max)
            .map(|d| format!("{}", d)).collect();
        let suffix = if self.df.width() > max { format!(", … +{} more", self.df.width() - max) } else { String::new() };
        format!("{}{}", names.join(", "), suffix)
    }
}

fn format_float(n: f64) -> String {
    if n.is_nan() { return "NaN".to_string(); }
    if n == n.floor() && n.abs() < 1e15 { format!("{:.0}", n) }
    else {
        let s = format!("{:.6}", n);
        let s = s.trim_end_matches('0').trim_end_matches('.');
        s.to_string()
    }
}

/// Convert the sheet's cells into a typed DataFrame. Row 0 is treated as
/// the header row; rows 1..=max_row are the data. Each column's type is
/// inferred from its values: all-int → Int64, all-numeric → Float64,
/// all-bool → Boolean, otherwise Utf8. Empty cells become nulls.
pub fn cells_to_dataframe(sheet: &Sheet) -> Result<DataFrameView, String> {
    let max_col = sheet.max_col().ok_or("シートが空です")?;
    let max_row = sheet.max_row().ok_or("シートが空です")?;
    if max_row == 0 {
        return Err("ヘッダー行のみで、データ行がありません".to_string());
    }

    // Header names (row 0). Replace empty headers with col1, col2, ... and
    // de-duplicate by suffixing.
    let mut headers: Vec<String> = (0..=max_col).map(|c| {
        let raw = sheet.get_cell(c, 0).raw_input;
        let h = raw.trim().to_string();
        if h.is_empty() { format!("col{}", c + 1) } else { h }
    }).collect();
    {
        let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for h in headers.iter_mut() {
            let count = seen.entry(h.clone()).or_insert(0);
            if *count > 0 {
                let dedup = format!("{}_{}", h, *count + 1);
                *h = dedup;
            }
            *count += 1;
        }
    }

    // Per-column raw string values (1..=max_row inclusive).
    let mut columns_raw: Vec<Vec<Option<String>>> = (0..=max_col)
        .map(|_| Vec::with_capacity(max_row))
        .collect();
    for r in 1..=max_row {
        for c in 0..=max_col {
            // Use evaluated value so formulas produce typed data.
            let evaluated = sheet.evaluate(c, r);
            // Also check whether the cell literally exists / has content.
            let cell = sheet.get_cell_ref(c, r);
            let raw = match cell {
                Some(_) => evaluated,
                None => String::new(),
            };
            let trimmed = raw.trim().to_string();
            columns_raw[c].push(if trimmed.is_empty() { None } else { Some(trimmed) });
        }
    }

    // Type inference per column.
    let mut series_vec: Vec<Column> = Vec::with_capacity(headers.len());
    for (col_idx, raw) in columns_raw.iter().enumerate() {
        let header = headers[col_idx].clone();
        let s = infer_series(&header, raw)?;
        series_vec.push(s.into());
    }

    let height = if series_vec.is_empty() { 0 } else { series_vec[0].len() };
    let df = DataFrame::new(height, series_vec)
        .map_err(|e| format!("DataFrame の構築に失敗: {}", e))?;
    Ok(DataFrameView { df, computed: Vec::new() })
}

fn infer_series(name: &str, values: &[Option<String>]) -> Result<Series, String> {
    // Try each type in order of specificity.
    let non_empty: Vec<&str> = values.iter().filter_map(|v| v.as_deref()).collect();
    if non_empty.is_empty() {
        return Ok(Series::new(name.into(), vec![None::<String>; values.len()]));
    }

    // Boolean
    if non_empty.iter().all(|s| matches!(s.to_uppercase().as_str(), "TRUE" | "FALSE")) {
        let v: Vec<Option<bool>> = values.iter()
            .map(|o| o.as_ref().map(|s| s.to_uppercase() == "TRUE"))
            .collect();
        return Ok(Series::new(name.into(), v));
    }

    // Int64
    if non_empty.iter().all(|s| s.parse::<i64>().is_ok()) {
        let v: Vec<Option<i64>> = values.iter()
            .map(|o| o.as_ref().and_then(|s| s.parse::<i64>().ok()))
            .collect();
        return Ok(Series::new(name.into(), v));
    }

    // Float64
    if non_empty.iter().all(|s| s.parse::<f64>().is_ok()) {
        let v: Vec<Option<f64>> = values.iter()
            .map(|o| o.as_ref().and_then(|s| s.parse::<f64>().ok()))
            .collect();
        return Ok(Series::new(name.into(), v));
    }

    // Fall back to Utf8.
    let v: Vec<Option<String>> = values.iter().cloned().collect();
    Ok(Series::new(name.into(), v))
}

/// Add a computed column to the view by running a Polars SQL `SELECT *,
/// (expr) AS "name" FROM df`. On success, replaces `view.df` with the
/// result and records the expression in `view.computed`. Returns a brief
/// description for the status bar.
pub fn add_computed_column(view: &mut DataFrameView, name: &str, expr: &str) -> Result<String, String> {
    let name = name.trim();
    let expr = expr.trim();
    if name.is_empty() { return Err("列名が空です".to_string()); }
    if expr.is_empty() { return Err("式が空です".to_string()); }
    if view.df.get_column_names().iter().any(|n| n.as_str() == name) {
        return Err(format!("列名「{}」は既に存在します", name));
    }

    // Build SQL: SELECT *, (expr) AS "name" FROM df
    let sql = format!(r#"SELECT *, ({expr}) AS "{name}" FROM df"#);

    let mut ctx = SQLContext::new();
    ctx.register("df", view.df.clone().lazy());
    let new_df = ctx.execute(&sql)
        .and_then(|lf| lf.collect())
        .map_err(|e| format!("SQL 評価エラー: {}", e))?;

    let rows = new_df.height();
    let col_count = new_df.width();
    view.df = new_df;
    view.computed.push(ComputedColumn {
        name: name.to_string(),
        expr: expr.to_string(),
    });
    Ok(format!("計算列「{}」を追加: 式 = {} （{} 行 × {} 列）", name, expr, rows, col_count))
}

/// Update a single cell in the DataFrame view. `row` is the **data** row
/// (excluding the header — caller subtracts 1 when called from the grid).
///
/// If the new string parses into the column's current dtype, the value is
/// stored as that type. Otherwise the column is widened to Utf8 once and
/// the string is stored verbatim. Empty input becomes null.
pub fn set_cell(view: &mut DataFrameView, col: usize, row: usize, new_value: &str) -> Result<(), String> {
    let cols = view.df.width();
    let rows = view.df.height();
    if col >= cols { return Err(format!("列 {} は範囲外", col)); }
    if row >= rows { return Err(format!("行 {} は範囲外", row)); }

    let col_name = view.df.get_column_names()[col].to_string();
    let series = view.df.column(&col_name)
        .map_err(|e| e.to_string())?
        .as_materialized_series()
        .clone();
    let dtype = series.dtype().clone();

    // Compute the replacement: try to keep the existing dtype; on parse
    // failure, fall back to widening to Utf8.
    let trimmed = new_value.trim();
    let widen_or_apply: Result<Series, ()> = match &dtype {
        DataType::Int64 => parse_i64(&series, row, trimmed),
        DataType::Int32 => parse_i32(&series, row, trimmed),
        DataType::UInt64 => parse_u64(&series, row, trimmed),
        DataType::Float64 => parse_f64(&series, row, trimmed),
        DataType::Float32 => parse_f32(&series, row, trimmed),
        DataType::Boolean => parse_bool(&series, row, trimmed),
        DataType::String => Ok(replace_string(&series, row, trimmed)),
        _ => Err(()),
    };

    let new_series = match widen_or_apply {
        Ok(s) => s,
        Err(_) => {
            // Widen: cast entire column to Utf8, then set the cell.
            let widened = series.cast(&DataType::String)
                .map_err(|e| format!("型変換に失敗: {}", e))?;
            replace_string(&widened, row, trimmed)
        }
    };

    view.df.with_column(new_series.into_column())
        .map_err(|e| format!("DataFrame 更新エラー: {}", e))?;
    Ok(())
}

macro_rules! parse_numeric {
    ($name:ident, $t:ty, $accessor:ident) => {
        fn $name(series: &Series, row: usize, text: &str) -> Result<Series, ()> {
            let parsed: Option<$t> = if text.is_empty() {
                None
            } else {
                match text.parse::<$t>() {
                    Ok(v) => Some(v),
                    Err(_) => return Err(()),
                }
            };
            let ca = series.$accessor().map_err(|_| ())?;
            let mut v: Vec<Option<$t>> = ca.into_iter().collect();
            if row < v.len() { v[row] = parsed; }
            Ok(Series::new(series.name().clone(), v))
        }
    };
}

parse_numeric!(parse_i64, i64, i64);
parse_numeric!(parse_i32, i32, i32);
parse_numeric!(parse_u64, u64, u64);
parse_numeric!(parse_f64, f64, f64);
parse_numeric!(parse_f32, f32, f32);

fn replace_string(series: &Series, row: usize, text: &str) -> Series {
    let ca = series.str().expect("series is Utf8");
    let mut v: Vec<Option<&str>> = ca.into_iter().collect();
    let new_val = if text.is_empty() { None } else { Some(text) };
    if row < v.len() { v[row] = new_val; }
    Series::new(series.name().clone(), v)
}

fn parse_bool(series: &Series, row: usize, text: &str) -> Result<Series, ()> {
    let parsed = if text.is_empty() {
        None
    } else {
        match text.to_uppercase().as_str() {
            "TRUE" | "T" | "1" | "YES" | "Y" => Some(true),
            "FALSE" | "F" | "0" | "NO" | "N" => Some(false),
            _ => return Err(()),
        }
    };
    let ca = series.bool().map_err(|_| ())?;
    let mut v: Vec<Option<bool>> = ca.into_iter().collect();
    if row < v.len() { v[row] = parsed; }
    Ok(Series::new(series.name().clone(), v))
}

/// Rename a column. Used when editing row 0 (header row) in DataFrame view.
pub fn rename_column(view: &mut DataFrameView, col: usize, new_name: &str) -> Result<(), String> {
    let new_name = new_name.trim();
    if new_name.is_empty() { return Err("列名が空です".to_string()); }
    let names = view.df.get_column_names_owned();
    if col >= names.len() { return Err(format!("列 {} は範囲外", col)); }
    let old_name = names[col].clone();
    if old_name.as_str() == new_name { return Ok(()); }
    if names.iter().any(|n| n.as_str() == new_name) {
        return Err(format!("列名「{}」は既に存在します", new_name));
    }
    view.df.rename(&old_name, new_name.into())
        .map_err(|e| format!("列名変更エラー: {}", e))?;
    Ok(())
}

/// Execute an arbitrary Polars SQL query against the current DataFrame
/// (referenced as table `df`) and replace the view's df with the result.
/// Returns a status-bar friendly summary. The previous `computed` list is
/// discarded because the schema may have changed completely.
pub fn run_sql(view: &mut DataFrameView, sql: &str) -> Result<String, String> {
    let sql = sql.trim();
    if sql.is_empty() { return Err("SQL が空です".to_string()); }

    let mut ctx = SQLContext::new();
    ctx.register("df", view.df.clone().lazy());
    let new_df = ctx.execute(sql)
        .and_then(|lf| lf.collect())
        .map_err(|e| format!("SQL 評価エラー: {}", e))?;

    let rows = new_df.height();
    let cols = new_df.width();
    view.df = new_df;
    view.computed.clear();
    Ok(format!("SQL 実行: {} 行 × {} 列", rows, cols))
}

/// Build and execute a GROUP BY query from a small DSL. Returns the same
/// status string as run_sql on success.
///
/// `groups` is a comma-separated list of column names: `category, region`.
/// `aggs` is comma-separated `col:func` items: `amount:sum, score:avg`.
/// Supported aggregation functions: `sum, avg, min, max, count, stddev, var`.
pub fn run_group_by(view: &mut DataFrameView, groups: &str, aggs: &str) -> Result<String, String> {
    let groups: Vec<&str> = groups.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    let aggs_parsed: Vec<(&str, &str)> = aggs.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| match s.split_once(':') {
            Some((col, func)) => (col.trim(), func.trim()),
            None => (s, "sum"),
        })
        .collect();
    if groups.is_empty() && aggs_parsed.is_empty() {
        return Err("グループ列または集計を 1 つは指定してください".to_string());
    }

    // Build the SELECT list.
    let mut select_parts: Vec<String> = groups.iter().map(|g| format!("\"{}\"", g)).collect();
    for (col, func) in &aggs_parsed {
        let func_upper = func.to_uppercase();
        let func_name = match func_upper.as_str() {
            "SUM" | "AVG" | "MIN" | "MAX" | "COUNT" => func_upper.clone(),
            "MEAN" => "AVG".to_string(),
            "STDEV" | "STDDEV" => "STDDEV".to_string(),
            "VAR" | "VARIANCE" => "VAR".to_string(),
            _ => return Err(format!("未知の集計関数: {}", func)),
        };
        let alias = format!("{}_{}", col, func_upper.to_lowercase());
        select_parts.push(format!("{}(\"{}\") AS \"{}\"", func_name, col, alias));
    }
    let select_clause = select_parts.join(", ");

    let group_clause = if groups.is_empty() {
        String::new()
    } else {
        format!(" GROUP BY {}", groups.iter().map(|g| format!("\"{}\"", g)).collect::<Vec<_>>().join(", "))
    };

    let sql = format!("SELECT {} FROM df{}", select_clause, group_clause);
    run_sql(view, &sql)
}

/// Drop every computed column added so far by re-running the base
/// `cells_to_dataframe` conversion. Useful as a single "reset" action;
/// avoids the need for per-column delete UI in Phase 2.
pub fn clear_computed_columns(sheet: &Sheet) -> Result<DataFrameView, String> {
    cells_to_dataframe(sheet)
}

/// Inverse: rebuild cells from the DataFrame, writing headers to row 0 and
/// data to rows 1..=df.height(). Reserved for future write-back support
/// from a possibly-edited DataFrame view; not used in Phase 1.
#[allow(dead_code)]
pub fn dataframe_to_cells(view: &DataFrameView, sheet: &mut Sheet) {
    // Clear existing cells in the region we're about to write.
    let cols = view.cols();
    let rows = view.rows();
    for r in 0..=rows {
        for c in 0..cols {
            sheet.clear_cell(c, r);
        }
    }
    // Headers
    for c in 0..cols {
        sheet.set_cell(c, 0, view.header(c));
    }
    // Data
    for r in 0..rows {
        for c in 0..cols {
            let v = view.value_at(c, r);
            if !v.is_empty() {
                sheet.set_cell(c, r + 1, v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sheet::Sheet;

    fn sheet_with(items: &[(usize, usize, &str)]) -> Sheet {
        let mut s = Sheet::new();
        for (c, r, v) in items {
            s.set_cell(*c, *r, v.to_string());
        }
        s
    }

    #[test]
    fn convert_with_type_inference() {
        let s = sheet_with(&[
            (0, 0, "name"),  (1, 0, "score"), (2, 0, "active"),
            (0, 1, "Alice"), (1, 1, "95"),    (2, 1, "TRUE"),
            (0, 2, "Bob"),   (1, 2, "82"),    (2, 2, "FALSE"),
            (0, 3, "Charlie"), (1, 3, "78"),  (2, 3, "TRUE"),
        ]);
        let v = cells_to_dataframe(&s).expect("convert");
        assert_eq!(v.rows(), 3);
        assert_eq!(v.cols(), 3);
        assert_eq!(v.header(0), "name");
        assert_eq!(v.header(1), "score");
        assert_eq!(v.header(2), "active");
        assert!(v.is_numeric(1));
        assert!(!v.is_numeric(0));
        // Booleans recognized
        assert_eq!(v.value_at(2, 0), "TRUE");
        assert_eq!(v.value_at(2, 1), "FALSE");
        assert_eq!(v.value_at(1, 0), "95");
    }

    #[test]
    fn empty_headers_become_col_names() {
        let s = sheet_with(&[
            (0, 1, "10"),
            (1, 1, "20"),
        ]);
        let v = cells_to_dataframe(&s).expect("convert");
        assert_eq!(v.header(0), "col1");
        assert_eq!(v.header(1), "col2");
    }

    #[test]
    fn float_inference() {
        let s = sheet_with(&[
            (0, 0, "x"),
            (0, 1, "1.5"),
            (0, 2, "2"),
            (0, 3, "3.14"),
        ]);
        let v = cells_to_dataframe(&s).expect("convert");
        assert!(v.is_numeric(0));
        assert_eq!(v.value_at(0, 2), "3.14");
    }

    #[test]
    fn add_computed_column_arithmetic() {
        let s = sheet_with(&[
            (0, 0, "price"), (1, 0, "qty"),
            (0, 1, "100"),   (1, 1, "3"),
            (0, 2, "250"),   (1, 2, "2"),
        ]);
        let mut v = cells_to_dataframe(&s).expect("convert");
        let msg = add_computed_column(&mut v, "revenue", "price * qty").expect("add");
        assert!(msg.contains("revenue"));
        assert_eq!(v.cols(), 3);
        assert_eq!(v.computed.len(), 1);
        // Header in column 2 should be "revenue"
        assert_eq!(v.header(2), "revenue");
        // Values
        assert_eq!(v.value_at(2, 0), "300");
        assert_eq!(v.value_at(2, 1), "500");
    }

    #[test]
    fn computed_column_can_reference_earlier_computed() {
        let s = sheet_with(&[
            (0, 0, "price"), (1, 0, "qty"),
            (0, 1, "100"),   (1, 1, "3"),
        ]);
        let mut v = cells_to_dataframe(&s).expect("convert");
        add_computed_column(&mut v, "revenue", "price * qty").expect("first");
        add_computed_column(&mut v, "tax", "revenue * 0.1").expect("second");
        assert_eq!(v.cols(), 4);
        assert_eq!(v.header(3), "tax");
        assert_eq!(v.value_at(3, 0), "30");
    }

    #[test]
    fn computed_column_duplicate_name_rejected() {
        let s = sheet_with(&[
            (0, 0, "price"),
            (0, 1, "100"),
        ]);
        let mut v = cells_to_dataframe(&s).expect("convert");
        let err = add_computed_column(&mut v, "price", "price * 2").unwrap_err();
        assert!(err.contains("既に存在"));
    }

    #[test]
    fn run_sql_basic_filter() {
        let s = sheet_with(&[
            (0, 0, "name"), (1, 0, "score"),
            (0, 1, "Alice"), (1, 1, "95"),
            (0, 2, "Bob"),   (1, 2, "60"),
            (0, 3, "Charlie"), (1, 3, "75"),
        ]);
        let mut v = cells_to_dataframe(&s).unwrap();
        let msg = run_sql(&mut v, "SELECT * FROM df WHERE score >= 75").unwrap();
        assert!(msg.contains("2 行"));
        assert_eq!(v.rows(), 2);
        assert_eq!(v.value_at(0, 0), "Alice");
        assert_eq!(v.value_at(0, 1), "Charlie");
    }

    #[test]
    fn run_group_by_sum_and_avg() {
        let s = sheet_with(&[
            (0, 0, "category"), (1, 0, "amount"),
            (0, 1, "A"),  (1, 1, "10"),
            (0, 2, "B"),  (1, 2, "20"),
            (0, 3, "A"),  (1, 3, "30"),
            (0, 4, "B"),  (1, 4, "40"),
        ]);
        let mut v = cells_to_dataframe(&s).unwrap();
        run_group_by(&mut v, "category", "amount:sum, amount:avg").unwrap();
        assert_eq!(v.rows(), 2);
        // Columns: category, amount_sum, amount_avg
        assert_eq!(v.cols(), 3);
        assert_eq!(v.header(0), "category");
        // Sort order varies; we just verify both A and B are present
        let mut amounts: Vec<(String, String, String)> = (0..v.rows())
            .map(|r| (v.value_at(0, r), v.value_at(1, r), v.value_at(2, r)))
            .collect();
        amounts.sort();
        assert_eq!(amounts[0].0, "A");
        assert_eq!(amounts[0].1, "40");
        assert_eq!(amounts[0].2, "20");
        assert_eq!(amounts[1].0, "B");
        assert_eq!(amounts[1].1, "60");
        assert_eq!(amounts[1].2, "30");
    }

    #[test]
    fn unknown_aggregation_function_rejected() {
        let s = sheet_with(&[(0, 0, "x"), (0, 1, "1")]);
        let mut v = cells_to_dataframe(&s).unwrap();
        let err = run_group_by(&mut v, "", "x:bogus").unwrap_err();
        assert!(err.contains("未知"));
    }

    #[test]
    fn set_cell_preserves_int_type() {
        let s = sheet_with(&[
            (0, 0, "score"),
            (0, 1, "10"),
            (0, 2, "20"),
        ]);
        let mut v = cells_to_dataframe(&s).unwrap();
        set_cell(&mut v, 0, 0, "99").unwrap();
        assert_eq!(v.value_at(0, 0), "99");
        // Still numeric (right-aligned)
        assert!(v.is_numeric(0));
    }

    #[test]
    fn set_cell_widens_to_string_on_parse_failure() {
        let s = sheet_with(&[
            (0, 0, "score"),
            (0, 1, "10"),
            (0, 2, "20"),
        ]);
        let mut v = cells_to_dataframe(&s).unwrap();
        assert!(v.is_numeric(0));
        // Writing non-numeric text should widen the column
        set_cell(&mut v, 0, 0, "N/A").unwrap();
        assert_eq!(v.value_at(0, 0), "N/A");
        // Numeric cell preserved as a string after widening
        assert_eq!(v.value_at(0, 1), "20");
        assert!(!v.is_numeric(0));
    }

    #[test]
    fn rename_column_changes_header() {
        let s = sheet_with(&[
            (0, 0, "score"), (1, 0, "name"),
            (0, 1, "10"),    (1, 1, "Alice"),
        ]);
        let mut v = cells_to_dataframe(&s).unwrap();
        rename_column(&mut v, 0, "points").unwrap();
        assert_eq!(v.header(0), "points");
        // Duplicate rejected
        let err = rename_column(&mut v, 0, "name").unwrap_err();
        assert!(err.contains("既に存在"));
    }

    #[test]
    fn set_cell_empty_becomes_null() {
        let s = sheet_with(&[
            (0, 0, "score"),
            (0, 1, "10"),
            (0, 2, "20"),
        ]);
        let mut v = cells_to_dataframe(&s).unwrap();
        set_cell(&mut v, 0, 0, "").unwrap();
        // Null renders as empty
        assert_eq!(v.value_at(0, 0), "");
    }

    #[test]
    fn round_trip_through_dataframe() {
        let s = sheet_with(&[
            (0, 0, "a"),  (1, 0, "b"),
            (0, 1, "1"),  (1, 1, "2"),
            (0, 2, "10"), (1, 2, "20"),
        ]);
        let v = cells_to_dataframe(&s).expect("convert");
        let mut s2 = Sheet::new();
        dataframe_to_cells(&v, &mut s2);
        assert_eq!(s2.get_cell(0, 0).raw_input, "a");
        assert_eq!(s2.get_cell(1, 1).raw_input, "2");
        assert_eq!(s2.get_cell(1, 2).raw_input, "20");
    }
}
