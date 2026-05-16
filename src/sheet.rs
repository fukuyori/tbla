use std::collections::HashMap;
use serde::{Deserialize, Serialize};

use crate::cell::{self, Cell, CellValue};
use crate::engine::Engine;

pub const DEFAULT_COL_WIDTH: usize = 10;
pub const MIN_COL_WIDTH: usize = 3;
pub const MAX_COL_WIDTH: usize = 50;

#[derive(Clone, Serialize, Deserialize)]
pub struct Sheet {
    pub name: String,
    cells: HashMap<(usize, usize), Cell>,
    col_widths: HashMap<usize, usize>,
}

impl Sheet {
    pub fn new() -> Self {
        Sheet {
            name: "Sheet1".to_string(),
            cells: HashMap::new(),
            col_widths: HashMap::new(),
        }
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
        if input.trim().is_empty() {
            self.cells.remove(&(col, row));
        } else {
            let value = cell::parse_input(&input);
            self.cells.insert((col, row), Cell::new(input, value));
        }
    }

    pub fn clear_cell(&mut self, col: usize, row: usize) {
        self.cells.remove(&(col, row));
    }

    pub fn cells(&self) -> &HashMap<(usize, usize), Cell> {
        &self.cells
    }

    pub fn evaluate(&self, col: usize, row: usize) -> String {
        let cell = self.get_cell(col, row);
        match &cell.value {
            CellValue::Empty => String::new(),
            CellValue::Number(n) => cell.format_number(*n),
            CellValue::Text(s) => s.clone(),
            CellValue::Boolean(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
            CellValue::Error(e) => e.to_string().to_string(),
            CellValue::Formula(f) => {
                let mut engine = Engine::new(&self.cells);
                match engine.evaluate_formula(f) {
                    Ok(result) => match result {
                        CellValue::Number(n) => cell.format_number(n),
                        CellValue::Text(s) => s,
                        CellValue::Boolean(b) => if b { "TRUE" } else { "FALSE" }.to_string(),
                        CellValue::Error(e) => e.to_string().to_string(),
                        CellValue::Empty => String::new(),
                        CellValue::Formula(_) => "ERR".to_string(),
                    },
                    Err(e) => e,
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
