use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};

use crate::cell::{Cell, CellValue};
use crate::formula::{self, FormulaEvaluator};

pub const MAX_COLS: usize = 26;
pub const MAX_ROWS: usize = 100;
pub const COL_WIDTH: usize = 10;
pub const LABEL_COL_WIDTH: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Navigate,  // Normal navigation
    Command,   // / commands
    Edit,      // Editing existing cell
    Enter,     // Entering new content
}

#[derive(Serialize, Deserialize)]
pub struct SpreadsheetData {
    cells: HashMap<String, Cell>,
}

pub struct Spreadsheet {
    pub cells: HashMap<(usize, usize), Cell>,
    pub cursor_col: usize,
    pub cursor_row: usize,
    pub view_start_row: usize,
    pub view_start_col: usize,
    pub mode: Mode,
    pub input_buffer: String,
    pub cursor_in_input: usize,
    pub status_message: String,
    pub clipboard: Option<Cell>,
    pub clipboard_pos: Option<(usize, usize)>,
}

impl Spreadsheet {
    pub fn new() -> Self {
        Spreadsheet {
            cells: HashMap::new(),
            cursor_col: 0,
            cursor_row: 0,
            view_start_row: 0,
            view_start_col: 0,
            mode: Mode::Navigate,
            input_buffer: String::new(),
            cursor_in_input: 0,
            status_message: "Ready".to_string(),
            clipboard: None,
            clipboard_pos: None,
        }
    }

    pub fn get_cell(&self, col: usize, row: usize) -> Cell {
        self.cells.get(&(col, row)).cloned().unwrap_or_default()
    }

    pub fn set_cell(&mut self, col: usize, row: usize, input: String) {
        let value = self.parse_input(&input);
        self.cells.insert(
            (col, row),
            Cell::new(input, value),
        );
    }

    pub fn clear_cell(&mut self, col: usize, row: usize) {
        self.cells.remove(&(col, row));
    }

    fn parse_input(&self, input: &str) -> CellValue {
        Self::parse_input_static(input)
    }

    fn parse_input_static(input: &str) -> CellValue {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return CellValue::Empty;
        }

        if trimmed.starts_with('=') {
            return CellValue::Formula(trimmed.to_string());
        }

        if let Ok(num) = trimmed.parse::<f64>() {
            return CellValue::Number(num);
        }

        CellValue::Text(trimmed.to_string())
    }

    pub fn evaluate_cell(&self, col: usize, row: usize) -> String {
        let cell = self.get_cell(col, row);
        match &cell.value {
            CellValue::Empty => String::new(),
            CellValue::Number(n) => cell.format_number(*n),
            CellValue::Text(s) => s.clone(),
            CellValue::Formula(_) => {
                let mut evaluator = FormulaEvaluator::new(&self.cells);
                match evaluator.evaluate_cell(col, row) {
                    Ok(n) => cell.format_number(n),
                    Err(e) => e,
                }
            }
        }
    }

    pub fn cell_name(&self) -> String {
        formula::cell_name(self.cursor_col, self.cursor_row)
    }

    pub fn move_cursor(&mut self, dx: isize, dy: isize) {
        let new_col = (self.cursor_col as isize + dx).max(0).min((MAX_COLS - 1) as isize) as usize;
        let new_row = (self.cursor_row as isize + dy).max(0).min((MAX_ROWS - 1) as isize) as usize;
        
        self.cursor_col = new_col;
        self.cursor_row = new_row;
        
        // Adjust view if cursor moves out of visible area
        let (term_width, term_height) = crossterm::terminal::size().unwrap_or((80, 24));
        let visible_rows = (term_height as usize).saturating_sub(6);
        let visible_cols = ((term_width as usize).saturating_sub(LABEL_COL_WIDTH)) / COL_WIDTH;

        if self.cursor_row < self.view_start_row {
            self.view_start_row = self.cursor_row;
        } else if self.cursor_row >= self.view_start_row + visible_rows {
            self.view_start_row = self.cursor_row - visible_rows + 1;
        }

        if self.cursor_col < self.view_start_col {
            self.view_start_col = self.cursor_col;
        } else if self.cursor_col >= self.view_start_col + visible_cols {
            self.view_start_col = self.cursor_col - visible_cols + 1;
        }
    }

    pub fn goto_cell(&mut self, col: usize, row: usize) {
        self.cursor_col = col.min(MAX_COLS - 1);
        self.cursor_row = row.min(MAX_ROWS - 1);
        self.view_start_col = self.cursor_col;
        self.view_start_row = self.cursor_row;
    }

    pub fn copy_cell(&mut self) {
        let cell = self.get_cell(self.cursor_col, self.cursor_row);
        self.clipboard = Some(cell);
        self.clipboard_pos = Some((self.cursor_col, self.cursor_row));
    }

    pub fn paste_cell(&mut self) {
        if let (Some(cell), Some((src_col, src_row))) = (&self.clipboard, self.clipboard_pos) {
            let col_offset = self.cursor_col as isize - src_col as isize;
            let row_offset = self.cursor_row as isize - src_row as isize;

            let new_input = match &cell.value {
                CellValue::Formula(f) => {
                    formula::adjust_formula(f, col_offset, row_offset)
                }
                _ => cell.raw_input.clone(),
            };

            self.set_cell(self.cursor_col, self.cursor_row, new_input);
        }
    }

    pub fn insert_row(&mut self, at_row: usize) {
        // Collect all cells first to avoid borrow issues
        let old_cells: Vec<_> = self.cells.drain().collect();
        
        for ((col, row), cell) in old_cells {
            if row >= at_row {
                // Adjust formula references
                let new_input = match &cell.value {
                    CellValue::Formula(f) => formula::adjust_formula(f, 0, 1),
                    _ => cell.raw_input.clone(),
                };
                let new_value = Self::parse_input_static(&new_input);
                self.cells.insert((col, row + 1), Cell::new(new_input, new_value));
            } else {
                self.cells.insert((col, row), cell);
            }
        }
    }

    pub fn delete_row(&mut self, at_row: usize) {
        let old_cells: Vec<_> = self.cells.drain().collect();
        
        for ((col, row), cell) in old_cells {
            if row == at_row {
                continue; // Delete this row
            } else if row > at_row {
                // Move up and adjust formula references
                let new_input = match &cell.value {
                    CellValue::Formula(f) => formula::adjust_formula(f, 0, -1),
                    _ => cell.raw_input.clone(),
                };
                let new_value = Self::parse_input_static(&new_input);
                self.cells.insert((col, row - 1), Cell::new(new_input, new_value));
            } else {
                self.cells.insert((col, row), cell);
            }
        }
    }

    pub fn insert_col(&mut self, at_col: usize) {
        let old_cells: Vec<_> = self.cells.drain().collect();
        
        for ((col, row), cell) in old_cells {
            if col >= at_col {
                let new_input = match &cell.value {
                    CellValue::Formula(f) => formula::adjust_formula(f, 1, 0),
                    _ => cell.raw_input.clone(),
                };
                let new_value = Self::parse_input_static(&new_input);
                self.cells.insert((col + 1, row), Cell::new(new_input, new_value));
            } else {
                self.cells.insert((col, row), cell);
            }
        }
    }

    pub fn delete_col(&mut self, at_col: usize) {
        let old_cells: Vec<_> = self.cells.drain().collect();
        
        for ((col, row), cell) in old_cells {
            if col == at_col {
                continue;
            } else if col > at_col {
                let new_input = match &cell.value {
                    CellValue::Formula(f) => formula::adjust_formula(f, -1, 0),
                    _ => cell.raw_input.clone(),
                };
                let new_value = Self::parse_input_static(&new_input);
                self.cells.insert((col - 1, row), Cell::new(new_input, new_value));
            } else {
                self.cells.insert((col, row), cell);
            }
        }
    }

    pub fn save_to_file(&self, path: &str) -> io::Result<()> {
        let mut data = SpreadsheetData {
            cells: HashMap::new(),
        };

        for ((col, row), cell) in &self.cells {
            let key = formula::cell_name(*col, *row);
            data.cells.insert(key, cell.clone());
        }

        let json = serde_json::to_string_pretty(&data)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        
        let mut file = fs::File::create(path)?;
        file.write_all(json.as_bytes())?;
        Ok(())
    }

    pub fn load_from_file(&mut self, path: &str) -> io::Result<()> {
        let mut file = fs::File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        let data: SpreadsheetData = serde_json::from_str(&contents)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        self.cells.clear();
        for (key, cell) in data.cells {
            if let Some((col, row)) = formula::parse_cell_ref(&key) {
                self.cells.insert((col, row), cell);
            }
        }

        Ok(())
    }
}
