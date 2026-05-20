use std::collections::HashMap;
use serde::{Deserialize, Serialize};

use crate::cell::{self, Cell, CellValue};
use crate::engine::Engine;

pub const DEFAULT_COL_WIDTH: usize = 10;
pub const MIN_COL_WIDTH: usize = 3;
pub const MAX_COL_WIDTH: usize = 50;

/// A conditional-formatting rule on a sheet. When `range` contains the cell
/// being rendered and its evaluated value satisfies `condition`, the rule's
/// colors are applied. Rules are stored per sheet and evaluated in order;
/// the first match wins.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConditionalFormat {
    /// Inclusive cell range the rule applies to.
    pub min_col: usize,
    pub min_row: usize,
    pub max_col: usize,
    pub max_row: usize,
    pub condition: CondCondition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_color: Option<crate::cell::RgbColor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg_color: Option<crate::cell::RgbColor>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CondCondition {
    /// Numeric comparison: value `op` `target`.
    Compare { op: CondOp, target: f64 },
    /// Two-color gradient between `min` and `max` (interpolated by the cell
    /// value). Rule must set both `bg_color` (= top color) and `text_color`
    /// is ignored; the start color is provided by `min_color`.
    ColorScale {
        min: f64,
        max: f64,
        #[serde(default = "default_min_color")]
        min_color: crate::cell::RgbColor,
        #[serde(default = "default_max_color")]
        max_color: crate::cell::RgbColor,
    },
    /// Excel-style data bar: render a horizontal bar inside the cell whose
    /// length is proportional to the value within `[min, max]`. `min` and
    /// `max` may be `None` to mean "auto-detect from the rule's range".
    DataBar {
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
        #[serde(default = "default_bar_color")]
        bar_color: crate::cell::RgbColor,
    },
}

fn default_bar_color() -> crate::cell::RgbColor { (99, 142, 198) } // Excel default blue

/// Resolved conditional formatting at one cell. Either a uniform fill
/// (`text_color` / `bg_color`) or a data-bar overlay.
#[derive(Clone, Default, Debug)]
pub struct CondResolved {
    pub text_color: Option<crate::cell::RgbColor>,
    pub bg_color: Option<crate::cell::RgbColor>,
    /// `(fill_fraction_0_to_1, bar_color)` for Excel-style data bars.
    pub data_bar: Option<(f64, crate::cell::RgbColor)>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub enum CondOp { Gt, Lt, Ge, Le, Eq, Ne }

fn default_min_color() -> crate::cell::RgbColor { (255, 235, 235) }
fn default_max_color() -> crate::cell::RgbColor { (220, 50, 50) }

#[derive(Clone, Serialize, Deserialize)]
pub struct Sheet {
    pub name: String,
    cells: HashMap<(usize, usize), Cell>,
    col_widths: HashMap<usize, usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditional_formats: Vec<ConditionalFormat>,
    /// Session-only Polars DataFrame view. When `Some`, the grid renders
    /// from the DataFrame and ignores `cells` for display. Cells remain
    /// untouched underneath so reverting to cell view is lossless.
    /// Not serialized: file save / load round-trips through `cells`.
    #[serde(skip)]
    pub df_view: Option<crate::df_view::DataFrameView>,
}

impl Sheet {
    pub fn new() -> Self {
        Sheet {
            name: "Sheet1".to_string(),
            cells: HashMap::new(),
            col_widths: HashMap::new(),
            conditional_formats: Vec::new(),
            df_view: None,
        }
    }

    /// True when a DataFrame view is currently active for this sheet.
    pub fn is_df_view(&self) -> bool { self.df_view.is_some() }

    /// Output of resolving conditional formatting at one cell. `text_color`
    /// and `bg_color` are uniform-fill colors; `data_bar` is `(fill_fraction,
    /// bar_color)` for data-bar style rendering where the cell's content is
    /// drawn over a partially-filled bar background.
    pub fn lookup_conditional(&self, col: usize, row: usize, value_str: &str)
        -> CondResolved
    {
        let mut out = CondResolved::default();
        for rule in &self.conditional_formats {
            if col < rule.min_col || col > rule.max_col { continue; }
            if row < rule.min_row || row > rule.max_row { continue; }
            let n = value_str.trim().parse::<f64>().ok();
            match &rule.condition {
                CondCondition::Compare { op, target } => {
                    let Some(v) = n else { continue; };
                    let hit = match op {
                        CondOp::Gt => v > *target,
                        CondOp::Lt => v < *target,
                        CondOp::Ge => v >= *target,
                        CondOp::Le => v <= *target,
                        CondOp::Eq => (v - target).abs() < 1e-12 * v.abs().max(target.abs()).max(1.0),
                        CondOp::Ne => (v - target).abs() >= 1e-12 * v.abs().max(target.abs()).max(1.0),
                    };
                    if hit {
                        out.text_color = rule.text_color;
                        out.bg_color = rule.bg_color;
                        return out;
                    }
                }
                CondCondition::ColorScale { min, max, min_color, max_color } => {
                    let Some(v) = n else { continue; };
                    let (lo, hi) = if min <= max { (*min, *max) } else { (*max, *min) };
                    let t = if (hi - lo).abs() < f64::EPSILON {
                        0.5
                    } else {
                        ((v - lo) / (hi - lo)).clamp(0.0, 1.0)
                    };
                    let lerp = |a: u8, b: u8| -> u8 {
                        let af = a as f64; let bf = b as f64;
                        (af + (bf - af) * t).round() as u8
                    };
                    out.text_color = rule.text_color;
                    out.bg_color = Some((
                        lerp(min_color.0, max_color.0),
                        lerp(min_color.1, max_color.1),
                        lerp(min_color.2, max_color.2),
                    ));
                    return out;
                }
                CondCondition::DataBar { min, max, bar_color } => {
                    let Some(v) = n else { continue; };
                    // Auto-detect min/max from the range when not given.
                    let (lo, hi) = self.databar_range(rule, *min, *max);
                    let t = if (hi - lo).abs() < f64::EPSILON { 0.0 }
                        else { ((v - lo) / (hi - lo)).clamp(0.0, 1.0) };
                    out.data_bar = Some((t, *bar_color));
                    return out;
                }
            }
        }
        out
    }

    /// Resolve a DataBar rule's min/max, using rule-provided values when
    /// set, otherwise scanning the rule's range for actual min/max.
    fn databar_range(&self, rule: &ConditionalFormat, min: Option<f64>, max: Option<f64>) -> (f64, f64) {
        if let (Some(lo), Some(hi)) = (min, max) { return (lo, hi); }
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for r in rule.min_row..=rule.max_row {
            for c in rule.min_col..=rule.max_col {
                let v = self.evaluate(c, r).trim().parse::<f64>().ok();
                if let Some(v) = v {
                    if v < lo { lo = v; }
                    if v > hi { hi = v; }
                }
            }
        }
        if !lo.is_finite() { lo = 0.0; }
        if !hi.is_finite() { hi = 1.0; }
        (min.unwrap_or(lo), max.unwrap_or(hi))
    }

    pub fn get_col_width(&self, col: usize) -> usize {
        *self.col_widths.get(&col).unwrap_or(&DEFAULT_COL_WIDTH)
    }

    pub fn set_col_width(&mut self, col: usize, width: usize) {
        let width = width.max(MIN_COL_WIDTH).min(MAX_COL_WIDTH);
        if width == DEFAULT_COL_WIDTH {
            self.col_widths.remove(&col);
        } else {
            self.col_widths.insert(col, width);
        }
    }

    pub fn adjust_col_width(&mut self, col: usize, delta: isize) {
        let current = self.get_col_width(col) as isize;
        let new_width = (current + delta).max(MIN_COL_WIDTH as isize).min(MAX_COL_WIDTH as isize) as usize;
        self.set_col_width(col, new_width);
    }

    pub fn get_cell(&self, col: usize, row: usize) -> Cell {
        self.cells.get(&(col, row)).cloned().unwrap_or_default()
    }

    pub fn get_cell_ref(&self, col: usize, row: usize) -> Option<&Cell> {
        self.cells.get(&(col, row))
    }

    pub fn set_cell(&mut self, col: usize, row: usize, input: String) {
        // Preserve any existing cell-formatting (alignment, colors, bold,
        // number format) when the value is rewritten. Only the value-bearing
        // fields are replaced; the cached value from xlsx import is cleared
        // because a fresh user edit invalidates it.
        let prior_format = self.cells.get(&(col, row)).cloned();
        if input.trim().is_empty() {
            // If the cell had explicit formatting, keep it as an "empty
            // formatted slot" — Excel does the same. Otherwise remove.
            if let Some(prev) = prior_format {
                if prev.has_format() {
                    let mut blank = Cell::default();
                    blank.format = prev.format;
                    blank.alignment = prev.alignment;
                    blank.bold = prev.bold;
                    blank.text_color = prev.text_color;
                    blank.bg_color = prev.bg_color;
                    self.cells.insert((col, row), blank);
                    return;
                }
            }
            self.cells.remove(&(col, row));
        } else {
            let value = cell::parse_input(&input);
            let mut cell = Cell::new(input, value);
            if let Some(prev) = prior_format {
                cell.format = prev.format;
                cell.alignment = prev.alignment;
                cell.bold = prev.bold;
                cell.text_color = prev.text_color;
                cell.bg_color = prev.bg_color;
            }
            self.cells.insert((col, row), cell);
        }
    }

    /// Set a cell while preserving (or supplying) a cached value. Used by
    /// the xlsx importer to keep Excel's computed result as a fallback when
    /// tbla's engine can't evaluate the imported formula.
    pub fn set_cell_with_cache(&mut self, col: usize, row: usize, input: String, cached: Option<CellValue>) {
        if input.trim().is_empty() && cached.is_none() {
            self.cells.remove(&(col, row));
        } else {
            let value = cell::parse_input(&input);
            self.cells.insert((col, row), Cell::new(input, value).with_cached(cached));
        }
    }

    pub fn clear_cell(&mut self, col: usize, row: usize) {
        self.cells.remove(&(col, row));
    }

    /// Get a mutable reference to a cell's formatting fields. If the cell
    /// doesn't exist yet, creates an empty one in place so formatting can
    /// be applied to blank cells. Returns a closure-friendly mutable
    /// reference.
    pub fn cell_format_mut(&mut self, col: usize, row: usize) -> &mut Cell {
        self.cells.entry((col, row)).or_insert_with(Cell::default)
    }

    /// Apply a function to every cell in the given inclusive rectangle.
    /// Used to apply formatting changes to a selection. Creates empty
    /// formatted cells where none exist.
    pub fn apply_format<F: FnMut(&mut Cell)>(&mut self, min_col: usize, min_row: usize, max_col: usize, max_row: usize, mut f: F) {
        for r in min_row..=max_row {
            for c in min_col..=max_col {
                let cell = self.cell_format_mut(c, r);
                f(cell);
                // Drop the cell if it ended up with no value and no formatting.
                if matches!(cell.value, CellValue::Empty) && !cell.has_format() {
                    self.cells.remove(&(c, r));
                }
            }
        }
    }

    pub fn cells(&self) -> &HashMap<(usize, usize), Cell> {
        &self.cells
    }

    pub fn evaluate(&self, col: usize, row: usize) -> String {
        self.evaluate_with(col, row, &[])
    }

    /// Evaluate a cell with knowledge of other sheets in the same workbook,
    /// enabling cross-sheet formula references like `Sheet2!A1`.
    pub fn evaluate_with(
        &self,
        col: usize,
        row: usize,
        other_sheets: &[(String, &HashMap<(usize, usize), Cell>)],
    ) -> String {
        let cell = self.get_cell(col, row);
        match &cell.value {
            CellValue::Empty => String::new(),
            CellValue::Number(n) => cell.format_number(*n),
            CellValue::Text(s) => s.clone(),
            CellValue::Boolean(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
            CellValue::Error(e) => e.to_string().to_string(),
            CellValue::Formula(f) => {
                let mut engine = if other_sheets.is_empty() {
                    Engine::new(&self.cells)
                } else {
                    Engine::with_workbook(&self.cells, other_sheets)
                };
                let formatted = |v: CellValue| match v {
                    CellValue::Number(n) => cell.format_number(n),
                    CellValue::Text(s) => s,
                    CellValue::Boolean(b) => if b { "TRUE" } else { "FALSE" }.to_string(),
                    CellValue::Error(e) => e.to_string().to_string(),
                    CellValue::Empty => String::new(),
                    CellValue::Formula(_) => "ERR".to_string(),
                };
                match (engine.evaluate_formula(f), &cell.cached_value) {
                    (Ok(result), _) => formatted(result),
                    // Fallback to imported value when our engine can't handle
                    // the formula (e.g. unsupported function from Excel).
                    (Err(_), Some(cached)) => formatted(cached.clone()),
                    (Err(e), None) => e,
                }
            }
        }
    }

    pub fn max_row(&self) -> Option<usize> {
        self.cells.keys().map(|(_, r)| *r).max()
    }

    pub fn max_col(&self) -> Option<usize> {
        self.cells.keys().map(|(c, _)| *c).max()
    }

    pub fn max_col_in_row(&self, row: usize) -> Option<usize> {
        self.cells.keys()
            .filter(|(_, r)| *r == row)
            .map(|(c, _)| *c)
            .max()
    }

    pub fn max_row_in_col(&self, col: usize) -> Option<usize> {
        self.cells.keys()
            .filter(|(c, _)| *c == col)
            .map(|(_, r)| *r)
            .max()
    }

    pub fn first_non_empty_col_in_row(&self, row: usize) -> Option<usize> {
        self.cells.keys()
            .filter(|(_, r)| *r == row)
            .map(|(c, _)| *c)
            .min()
    }

    pub fn first_non_empty_row_in_col(&self, col: usize) -> Option<usize> {
        self.cells.keys()
            .filter(|(c, _)| *c == col)
            .map(|(_, r)| *r)
            .min()
    }

    // Row operations
    pub fn delete_row(&mut self, row: usize) {
        self.cells.retain(|(_, r), _| *r != row);
        
        let cells_to_move: Vec<_> = self.cells
            .iter()
            .filter(|((_, r), _)| *r > row)
            .map(|((c, r), cell)| ((*c, *r), cell.clone()))
            .collect();

        for ((c, r), _) in &cells_to_move {
            self.cells.remove(&(*c, *r));
        }

        for ((c, r), cell) in cells_to_move {
            self.cells.insert((c, r - 1), cell);
        }
    }

    pub fn insert_row(&mut self, row: usize) {
        let cells_to_move: Vec<_> = self.cells
            .iter()
            .filter(|((_, r), _)| *r >= row)
            .map(|((c, r), cell)| ((*c, *r), cell.clone()))
            .collect();

        for ((c, r), _) in &cells_to_move {
            self.cells.remove(&(*c, *r));
        }

        for ((c, r), cell) in cells_to_move {
            self.cells.insert((c, r + 1), cell);
        }
    }

    // Column operations
    pub fn delete_col(&mut self, col: usize) {
        self.cells.retain(|(c, _), _| *c != col);
        
        let cells_to_move: Vec<_> = self.cells
            .iter()
            .filter(|((c, _), _)| *c > col)
            .map(|((c, r), cell)| ((*c, *r), cell.clone()))
            .collect();

        for ((c, r), _) in &cells_to_move {
            self.cells.remove(&(*c, *r));
        }

        for ((c, r), cell) in cells_to_move {
            self.cells.insert((c - 1, r), cell);
        }
    }

    pub fn insert_col(&mut self, col: usize) {
        let cells_to_move: Vec<_> = self.cells
            .iter()
            .filter(|((c, _), _)| *c >= col)
            .map(|((c, r), cell)| ((*c, *r), cell.clone()))
            .collect();

        for ((c, r), _) in &cells_to_move {
            self.cells.remove(&(*c, *r));
        }

        for ((c, r), cell) in cells_to_move {
            self.cells.insert((c + 1, r), cell);
        }
    }

    /// Adjust all formulas in the sheet for a row insertion
    pub fn adjust_formulas_for_row_insert(&mut self, inserted_row: usize) {
        let keys: Vec<_> = self.cells.keys().cloned().collect();
        for (col, row) in keys {
            if let Some(cell) = self.cells.get(&(col, row)) {
                if cell.raw_input.starts_with('=') {
                    let adjusted = crate::formula::adjust_formula_for_row_insert(&cell.raw_input, inserted_row);
                    if adjusted != cell.raw_input {
                        let value = crate::cell::parse_input(&adjusted);
                        self.cells.insert((col, row), Cell::new(adjusted, value));
                    }
                }
            }
        }
    }

    /// Adjust all formulas in the sheet for a row deletion
    pub fn adjust_formulas_for_row_delete(&mut self, deleted_row: usize) {
        let keys: Vec<_> = self.cells.keys().cloned().collect();
        for (col, row) in keys {
            if let Some(cell) = self.cells.get(&(col, row)) {
                if cell.raw_input.starts_with('=') {
                    let adjusted = crate::formula::adjust_formula_for_row_delete(&cell.raw_input, deleted_row);
                    if adjusted != cell.raw_input {
                        let value = crate::cell::parse_input(&adjusted);
                        self.cells.insert((col, row), Cell::new(adjusted, value));
                    }
                }
            }
        }
    }

    /// Adjust all formulas in the sheet for a column insertion
    pub fn adjust_formulas_for_col_insert(&mut self, inserted_col: usize) {
        let keys: Vec<_> = self.cells.keys().cloned().collect();
        for (col, row) in keys {
            if let Some(cell) = self.cells.get(&(col, row)) {
                if cell.raw_input.starts_with('=') {
                    let adjusted = crate::formula::adjust_formula_for_col_insert(&cell.raw_input, inserted_col);
                    if adjusted != cell.raw_input {
                        let value = crate::cell::parse_input(&adjusted);
                        self.cells.insert((col, row), Cell::new(adjusted, value));
                    }
                }
            }
        }
    }

    /// Adjust all formulas in the sheet for a column deletion
    pub fn adjust_formulas_for_col_delete(&mut self, deleted_col: usize) {
        let keys: Vec<_> = self.cells.keys().cloned().collect();
        for (col, row) in keys {
            if let Some(cell) = self.cells.get(&(col, row)) {
                if cell.raw_input.starts_with('=') {
                    let adjusted = crate::formula::adjust_formula_for_col_delete(&cell.raw_input, deleted_col);
                    if adjusted != cell.raw_input {
                        let value = crate::cell::parse_input(&adjusted);
                        self.cells.insert((col, row), Cell::new(adjusted, value));
                    }
                }
            }
        }
    }

    // Cell shift operations (within a row)
    /// Shift cells right from (col, row) to make space for a new cell
    pub fn shift_cells_right(&mut self, col: usize, row: usize) {
        let cells_to_move: Vec<_> = self.cells
            .iter()
            .filter(|((c, r), _)| *r == row && *c >= col)
            .map(|((c, r), cell)| ((*c, *r), cell.clone()))
            .collect();

        for ((c, r), _) in &cells_to_move {
            self.cells.remove(&(*c, *r));
        }

        for ((c, r), cell) in cells_to_move {
            self.cells.insert((c + 1, r), cell);
        }
    }

    // Cell shift operations (within a column)
    /// Shift cells down from (col, row) to make space for a new cell
    pub fn shift_cells_down(&mut self, col: usize, row: usize) {
        let cells_to_move: Vec<_> = self.cells
            .iter()
            .filter(|((c, r), _)| *c == col && *r >= row)
            .map(|((c, r), cell)| ((*c, *r), cell.clone()))
            .collect();

        for ((c, r), _) in &cells_to_move {
            self.cells.remove(&(*c, *r));
        }

        for ((c, r), cell) in cells_to_move {
            self.cells.insert((c, r + 1), cell);
        }
    }
}

#[cfg(test)]
mod format_tests {
    use super::*;
    use crate::cell::{Alignment, DisplayFormat};

    #[test]
    fn formatting_persists_across_set_cell() {
        let mut s = Sheet::new();
        s.set_cell(0, 0, "100".to_string());
        // Apply some formatting
        let c = s.cell_format_mut(0, 0);
        c.bold = true;
        c.alignment = Alignment::Right;
        c.bg_color = Some((255, 200, 200));
        // Now re-set the cell with a new value
        s.set_cell(0, 0, "200".to_string());
        let c = s.get_cell(0, 0);
        assert_eq!(c.raw_input, "200");
        assert!(c.bold);
        assert!(matches!(c.alignment, Alignment::Right));
        assert_eq!(c.bg_color, Some((255, 200, 200)));
    }

    #[test]
    fn apply_format_to_range() {
        let mut s = Sheet::new();
        s.apply_format(0, 0, 2, 2, |c| c.bold = true);
        for r in 0..=2 {
            for col in 0..=2 {
                assert!(s.get_cell(col, r).bold);
            }
        }
    }

    #[test]
    fn conditional_format_compare() {
        let mut s = Sheet::new();
        s.conditional_formats.push(ConditionalFormat {
            min_col: 0, min_row: 0, max_col: 5, max_row: 5,
            condition: CondCondition::Compare { op: CondOp::Gt, target: 50.0 },
            text_color: None,
            bg_color: Some((255, 0, 0)),
        });
        assert_eq!(s.lookup_conditional(0, 0, "100").bg_color, Some((255, 0, 0)));
        assert_eq!(s.lookup_conditional(0, 0, "10").bg_color, None);
        // Out of range cell
        assert_eq!(s.lookup_conditional(10, 0, "100").bg_color, None);
    }

    #[test]
    fn conditional_format_color_scale() {
        let mut s = Sheet::new();
        s.conditional_formats.push(ConditionalFormat {
            min_col: 0, min_row: 0, max_col: 0, max_row: 10,
            condition: CondCondition::ColorScale {
                min: 0.0, max: 100.0,
                min_color: (255, 255, 255),
                max_color: (0, 0, 0),
            },
            text_color: None,
            bg_color: None,
        });
        // At min → white
        assert_eq!(s.lookup_conditional(0, 0, "0").bg_color, Some((255, 255, 255)));
        // At max → black
        assert_eq!(s.lookup_conditional(0, 1, "100").bg_color, Some((0, 0, 0)));
        // Mid → grey
        let bg = s.lookup_conditional(0, 2, "50").bg_color;
        let (r, g, b) = bg.unwrap();
        assert!((127..=128).contains(&r));
        assert_eq!(r, g);
        assert_eq!(g, b);
    }

    #[test]
    fn number_format_applies() {
        let mut s = Sheet::new();
        s.set_cell(0, 0, "1234.5".to_string());
        let c = s.cell_format_mut(0, 0);
        c.format = DisplayFormat::Currency(2);
        let cell = s.get_cell(0, 0);
        assert!(matches!(cell.format, DisplayFormat::Currency(2)));
        // Format affects display:
        assert!(cell.format_number(1234.5).starts_with('$'));
    }
}
