mod cell;
mod date_util;
mod engine;
mod formula;
mod sheet;
mod ui;
mod commands;
mod menu;
mod xlsx;

use crossterm::{
    cursor::{Hide, Show},
    event::{
        self, Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind, MouseButton,
        EnableMouseCapture, DisableMouseCapture,
        KeyboardEnhancementFlags, PushKeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    },
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen, supports_keyboard_enhancement},
};
use std::io::{stdout, Result};

use sheet::Sheet;
use ui::UI;
use menu::{MenuBar, MenuState, ContextMenu, Action};

/// Operation modes
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Normal,
    Edit,
    Menu,
    Dialog,
    ContextMenu,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DialogKind {
    Open,
    SaveAs,
    ImportCsv,
    ExportCsv,
    Find,
    Goto,
    /// Set width of the column at `cursor_col` (target column captured when
    /// the dialog is opened so the user can move the cursor without losing
    /// the intended target — but currently we just read cursor_col on
    /// commit, since the cursor doesn't move while the dialog is open).
    SetColWidth,
    PrintHtml,
}

#[derive(Clone, Debug)]
pub struct Dialog {
    pub kind: DialogKind,
    pub label: String,
    pub input: String,
}

pub struct App {
    pub sheet: Sheet,
    pub mode: Mode,
    pub input_buffer: String,
    pub edit_cursor_pos: usize,  // character index in input_buffer (0..=chars().count())
    pub status_message: String,
    pub cursor_col: usize,
    pub cursor_row: usize,
    pub view_col: usize,
    pub view_row: usize,
    pub selection_anchor: Option<(usize, usize)>,
    pub clipboard: Option<ClipboardContent>,
    pub undo_stack: Vec<Sheet>,
    pub redo_stack: Vec<Sheet>,
    pub running: bool,
    pub current_file: Option<String>,
    pub edit_original: String,
    pub last_search: String,
    pub menu_bar: MenuBar,
    pub menu_state: MenuState,
    pub dialog: Option<Dialog>,
    pub context_menu: Option<ContextMenu>,
    pub dragging: bool,
    pub last_click_at: Option<std::time::Instant>,
    pub last_click_cell: Option<(usize, usize)>,
    pub point_mode: Option<PointMode>,
    /// Active column-width drag: (column index, screen x where the drag began,
    /// the column's width at the start of the drag).
    pub column_resize: Option<(usize, u16, usize)>,
}

/// Point mode (Excel-style formula reference selection).
/// While editing a formula at a position where a cell reference can be entered
/// (right after `=`, `(`, `,`, or an operator), arrow keys / mouse can be used
/// to point at a cell. The reference text is inserted into the input buffer
/// and updated live as the user moves the point cursor.
#[derive(Clone, Debug)]
pub struct PointMode {
    pub cursor: (usize, usize),         // grid cell currently being pointed at
    pub anchor: Option<(usize, usize)>, // anchor for range selection
    pub insert_pos: usize,              // character index in input_buffer where ref text starts
    pub inserted_chars: usize,          // character length of the inserted ref text
}

#[derive(Clone)]
pub struct ClipboardContent {
    pub cells: Vec<Vec<(String, crate::cell::CellValue)>>,
    pub start_col: usize,
    pub start_row: usize,
    pub width: usize,
    pub height: usize,
}

impl App {
    pub fn new() -> Self {
        App {
            sheet: Sheet::new(),
            mode: Mode::Normal,
            input_buffer: String::new(),
            edit_cursor_pos: 0,
            status_message: String::new(),
            cursor_col: 0,
            cursor_row: 0,
            view_col: 0,
            view_row: 0,
            selection_anchor: None,
            clipboard: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            running: true,
            current_file: None,
            edit_original: String::new(),
            last_search: String::new(),
            menu_bar: MenuBar::default(),
            menu_state: MenuState::default(),
            dialog: None,
            context_menu: None,
            dragging: false,
            last_click_at: None,
            last_click_cell: None,
            point_mode: None,
            column_resize: None,
        }
    }

    pub fn save_undo(&mut self) {
        self.undo_stack.push(self.sheet.clone());
        self.redo_stack.clear();
        if self.undo_stack.len() > 100 {
            self.undo_stack.remove(0);
        }
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(self.sheet.clone());
            self.sheet = prev;
            self.status_message = "元に戻す".to_string();
        } else {
            self.status_message = "これ以上元に戻せません".to_string();
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.sheet.clone());
            self.sheet = next;
            self.status_message = "やり直し".to_string();
        } else {
            self.status_message = "やり直す操作がありません".to_string();
        }
    }

    pub fn move_cursor(&mut self, dx: isize, dy: isize, extend_selection: bool) {
        if extend_selection {
            if self.selection_anchor.is_none() {
                self.selection_anchor = Some((self.cursor_col, self.cursor_row));
            }
        } else {
            self.selection_anchor = None;
        }
        let new_col = (self.cursor_col as isize + dx).max(0).min(255) as usize;
        let new_row = (self.cursor_row as isize + dy).max(0).min(9999) as usize;
        self.cursor_col = new_col;
        self.cursor_row = new_row;
        self.adjust_view();
    }

    pub fn move_cursor_to(&mut self, col: usize, row: usize) {
        self.cursor_col = col.min(255);
        self.cursor_row = row.min(9999);
        self.adjust_view();
    }

    pub fn adjust_view(&mut self) {
        const ROW_LABEL_WIDTH: usize = 5;
        const HEADER_ROWS: usize = 3;   // menu + column header + (separator removed) — use 3 for safety
        const FOOTER_ROWS: usize = 2;   // formula bar + status bar

        let (term_width, term_height) = terminal::size().unwrap_or((80, 24));
        let available_width = (term_width as usize).saturating_sub(ROW_LABEL_WIDTH);
        let visible_rows = (term_height as usize).saturating_sub(HEADER_ROWS + FOOTER_ROWS);

        if self.cursor_col < self.view_col {
            self.view_col = self.cursor_col;
        } else {
            let mut x = 0;
            let mut col = self.view_col;
            let mut cursor_visible = false;

            while x < available_width && col <= 255 {
                let col_width = self.sheet.get_col_width(col);
                if col == self.cursor_col {
                    if x + col_width <= available_width {
                        cursor_visible = true;
                    }
                    break;
                }
                x += col_width;
                col += 1;
            }

            if !cursor_visible {
                self.view_col = self.cursor_col;
            }
        }

        if self.cursor_row < self.view_row {
            self.view_row = self.cursor_row;
        } else if self.cursor_row >= self.view_row + visible_rows {
            self.view_row = self.cursor_row.saturating_sub(visible_rows.saturating_sub(1));
        }
    }

    /// Convert screen position to cell coordinates. Returns None if outside grid.
    pub fn screen_to_cell(&self, screen_col: u16, screen_row: u16) -> Option<(usize, usize)> {
        const ROW_LABEL_WIDTH: usize = 5;
        const HEADER_ROWS: usize = 2; // menu bar (row 0), column headers (row 1) → grid starts at row 2

        let screen_col = screen_col as usize;
        let screen_row = screen_row as usize;

        let (term_width, term_height) = terminal::size().unwrap_or((80, 24));
        let grid_height = (term_height as usize).saturating_sub(HEADER_ROWS + 2);

        if screen_col < ROW_LABEL_WIDTH || screen_row < HEADER_ROWS {
            return None;
        }

        if screen_row >= HEADER_ROWS + grid_height {
            return None;
        }

        let mut x = ROW_LABEL_WIDTH;
        let mut col = self.view_col;
        while x < term_width as usize && col <= 255 {
            let col_width = self.sheet.get_col_width(col);
            if screen_col < x + col_width {
                let row = self.view_row + (screen_row - HEADER_ROWS);
                return Some((col, row));
            }
            x += col_width;
            col += 1;
        }

        None
    }

    /// If `(screen_col, screen_row)` falls on a column-width resize handle
    /// (the rightmost cell of any visible column header), return that column.
    /// Resize handles live on screen row 1 (the column-header row).
    pub fn screen_to_col_edge(&self, screen_col: u16, screen_row: u16) -> Option<usize> {
        const ROW_LABEL_WIDTH: usize = 5;
        if screen_row != 1 { return None; }
        let (term_width, _) = terminal::size().unwrap_or((80, 24));
        let mut x = ROW_LABEL_WIDTH;
        let mut col = self.view_col;
        while x < term_width as usize && col <= 255 {
            let col_width = self.sheet.get_col_width(col);
            let right_edge = x + col_width - 1;
            if (screen_col as usize) == right_edge {
                return Some(col);
            }
            x += col_width;
            col += 1;
        }
        None
    }

    /// Get selection bounds. Returns (min_col, min_row, max_col, max_row).
    /// If no selection_anchor, returns single-cell bounds at cursor.
    pub fn get_selection_bounds(&self) -> (usize, usize, usize, usize) {
        if let Some((ac, ar)) = self.selection_anchor {
            let min_col = ac.min(self.cursor_col);
            let max_col = ac.max(self.cursor_col);
            let min_row = ar.min(self.cursor_row);
            let max_row = ar.max(self.cursor_row);
            (min_col, min_row, max_col, max_row)
        } else {
            (self.cursor_col, self.cursor_row, self.cursor_col, self.cursor_row)
        }
    }

    pub fn has_selection(&self) -> bool {
        self.selection_anchor.is_some()
    }

    /// Clear current cell or selection.
    pub fn clear_target(&mut self) {
        let (min_col, min_row, max_col, max_row) = self.get_selection_bounds();
        self.save_undo();
        let mut count = 0;
        for col in min_col..=max_col {
            for row in min_row..=max_row {
                self.sheet.clear_cell(col, row);
                count += 1;
            }
        }
        self.status_message = format!("{} セルをクリア", count);
    }

    /// Copy current cell or selection to internal clipboard.
    pub fn copy(&mut self) {
        let (min_col, min_row, max_col, max_row) = self.get_selection_bounds();

        let width = max_col - min_col + 1;
        let height = max_row - min_row + 1;

        let mut cells = Vec::new();
        for row in min_row..=max_row {
            let mut row_data = Vec::new();
            for col in min_col..=max_col {
                let cell = self.sheet.get_cell(col, row);
                row_data.push((cell.raw_input.clone(), cell.value.clone()));
            }
            cells.push(row_data);
        }

        self.clipboard = Some(ClipboardContent {
            cells,
            start_col: min_col,
            start_row: min_row,
            width,
            height,
        });

        self.status_message = format!("コピー: {}x{} セル", width, height);

        // Also write to system clipboard as TSV
        let mut tsv = String::new();
        for row in min_row..=max_row {
            for col in min_col..=max_col {
                if col > min_col {
                    tsv.push('\t');
                }
                tsv.push_str(&self.sheet.evaluate(col, row));
            }
            tsv.push('\n');
        }
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(&tsv);
        }
    }

    /// Cut: copy then clear.
    pub fn cut(&mut self) {
        self.copy();
        self.clear_target();
        self.status_message = format!("{}", "切り取り");
    }

    /// Paste from internal clipboard (or from system clipboard if internal is empty).
    pub fn paste(&mut self) {
        if self.clipboard.is_some() {
            self.paste_internal();
        } else {
            self.paste_from_system();
        }
    }

    fn paste_internal(&mut self) {
        let clip = self.clipboard.clone().unwrap();
        self.save_undo();

        let paste_col = self.cursor_col;
        let paste_row = self.cursor_row;

        for (r_offset, row_data) in clip.cells.iter().enumerate() {
            for (c_offset, (raw_input, _value)) in row_data.iter().enumerate() {
                let dst_col = paste_col + c_offset;
                let dst_row = paste_row + r_offset;

                let adjusted = if raw_input.starts_with('=') {
                    let col_delta = (dst_col as isize) - (clip.start_col as isize) - (c_offset as isize);
                    let row_delta = (dst_row as isize) - (clip.start_row as isize) - (r_offset as isize);
                    formula::adjust_formula(raw_input, col_delta, row_delta)
                } else {
                    raw_input.clone()
                };

                self.sheet.set_cell(dst_col, dst_row, adjusted);
            }
        }

        self.status_message = format!("貼り付け: {}x{} セル", clip.width, clip.height);
    }

    fn paste_from_system(&mut self) {
        let text = if let Ok(mut clipboard) = arboard::Clipboard::new() {
            clipboard.get_text().unwrap_or_default()
        } else {
            self.status_message = "クリップボードを利用できません".to_string();
            return;
        };

        if text.is_empty() {
            self.status_message = "クリップボードが空です".to_string();
            return;
        }

        self.save_undo();

        let lines: Vec<&str> = text.lines().collect();
        let mut height = 0;
        let mut width = 0;

        for (r_offset, line) in lines.iter().enumerate() {
            let cells: Vec<&str> = if line.contains('\t') {
                line.split('\t').collect()
            } else {
                line.split(',').collect()
            };

            for (c_offset, cell_value) in cells.iter().enumerate() {
                let dst_col = self.cursor_col + c_offset;
                let dst_row = self.cursor_row + r_offset;
                self.sheet.set_cell(dst_col, dst_row, cell_value.to_string());
                width = width.max(c_offset + 1);
            }
            height = r_offset + 1;
        }

        self.status_message = format!("クリップボードから貼り付け: {}x{} セル", width, height);
    }

    /// Select all cells with data.
    pub fn select_all(&mut self) {
        let max_col = self.sheet.max_col().unwrap_or(0);
        let max_row = self.sheet.max_row().unwrap_or(0);
        self.selection_anchor = Some((0, 0));
        self.cursor_col = max_col;
        self.cursor_row = max_row;
        self.adjust_view();
        self.status_message = "すべて選択".to_string();
    }

    /// Begin editing the current cell, optionally with an initial character.
    pub fn begin_edit(&mut self, initial: Option<char>, preserve: bool) {
        let cell = self.sheet.get_cell(self.cursor_col, self.cursor_row);
        self.edit_original = cell.raw_input.clone();
        if preserve {
            self.input_buffer = cell.raw_input.clone();
        } else if let Some(c) = initial {
            self.input_buffer = c.to_string();
        } else {
            self.input_buffer.clear();
        }
        // Place text cursor at the end of the buffer
        self.edit_cursor_pos = self.input_buffer.chars().count();
        self.mode = Mode::Edit;
    }

    /// Commit current edit input to the cell.
    pub fn commit_edit(&mut self) {
        // Auto-completion for aggregate formulas (=sum / =avg / =min / =max / =count / =counta).
        // If the user typed only the function name (with optional empty parens),
        // detect the contiguous numeric block above (preferred) or to the left
        // and fill in the range argument automatically.
        if let Some(completed) = autocomplete_aggregate(
            &self.sheet,
            &self.input_buffer,
            self.cursor_col,
            self.cursor_row,
        ) {
            self.status_message = format!("自動補完: {}", completed);
            self.input_buffer = completed;
        }

        if self.input_buffer != self.edit_original {
            self.save_undo();
            self.sheet.set_cell(self.cursor_col, self.cursor_row, self.input_buffer.clone());
        }
        self.input_buffer.clear();
        self.edit_cursor_pos = 0;
        self.edit_original.clear();
        self.point_mode = None;
    }

    /// Cancel edit without committing.
    pub fn cancel_edit(&mut self) {
        self.input_buffer.clear();
        self.edit_cursor_pos = 0;
        self.edit_original.clear();
        self.point_mode = None;
        self.mode = Mode::Normal;
    }

    /// Returns true if the text cursor is at the end of the input AND the
    /// character before it is one that can be followed by a cell reference
    /// (e.g. `=`, `(`, `,`, operators). Used to decide whether arrow keys /
    /// mouse clicks should enter point mode.
    pub fn point_mode_allowed(&self) -> bool {
        if self.point_mode.is_some() {
            return true;
        }
        let chars: Vec<char> = self.input_buffer.chars().collect();
        if self.edit_cursor_pos != chars.len() {
            return false;
        }
        if self.edit_cursor_pos == 0 {
            return false;
        }
        let prev = chars[self.edit_cursor_pos - 1];
        matches!(prev, '=' | '(' | ',' | '+' | '-' | '*' | '/' | '^' | '&' | ':' | '<' | '>')
    }

    /// Build the reference text from a single cell or anchored range.
    fn build_ref_text(col: usize, row: usize, anchor: Option<(usize, usize)>) -> String {
        let cell = crate::formula::cell_name(col, row);
        match anchor {
            Some(a) if a != (col, row) => {
                let (min_c, max_c) = (a.0.min(col), a.0.max(col));
                let (min_r, max_r) = (a.1.min(row), a.1.max(row));
                format!(
                    "{}:{}",
                    crate::formula::cell_name(min_c, min_r),
                    crate::formula::cell_name(max_c, max_r),
                )
            }
            _ => cell,
        }
    }

    /// Update point mode to point at (new_col, new_row). If `extend` is true,
    /// preserves the existing anchor (or sets it to the previous point cursor)
    /// to form a range. Replaces the previously-inserted reference text in the
    /// input buffer with the new reference.
    pub fn point_mode_update(&mut self, new_col: usize, new_row: usize, extend: bool) {
        let (insert_pos, prev_inserted, prev_cursor, prev_anchor) =
            if let Some(pm) = &self.point_mode {
                (pm.insert_pos, pm.inserted_chars, pm.cursor, pm.anchor)
            } else {
                (
                    self.edit_cursor_pos,
                    0,
                    (self.cursor_col, self.cursor_row),
                    None,
                )
            };

        let new_anchor = if extend {
            prev_anchor.or(Some(prev_cursor))
        } else {
            None
        };

        let ref_text = Self::build_ref_text(new_col, new_row, new_anchor);
        let ref_chars = ref_text.chars().count();

        // Remove previous insertion, then insert new
        if prev_inserted > 0 {
            let start = self.input_byte_offset(insert_pos);
            let end = self.input_byte_offset(insert_pos + prev_inserted);
            self.input_buffer.drain(start..end);
        }
        let insert_byte = self.input_byte_offset(insert_pos);
        self.input_buffer.insert_str(insert_byte, &ref_text);

        self.edit_cursor_pos = insert_pos + ref_chars;

        self.point_mode = Some(PointMode {
            cursor: (new_col, new_row),
            anchor: new_anchor,
            insert_pos,
            inserted_chars: ref_chars,
        });

        // Scroll the view to keep the point cursor visible
        self.adjust_view_for_point();
    }

    /// Arrow-key entry point. Move (or start) the point cursor by (dx, dy).
    pub fn point_mode_arrow(&mut self, dx: isize, dy: isize, extend: bool) {
        let (cur_col, cur_row) = if let Some(pm) = &self.point_mode {
            pm.cursor
        } else {
            (self.cursor_col, self.cursor_row)
        };
        let new_col = (cur_col as isize + dx).max(0).min(255) as usize;
        let new_row = (cur_row as isize + dy).max(0).min(9999) as usize;
        self.point_mode_update(new_col, new_row, extend);
    }

    /// Exit point mode but keep the inserted reference text in the buffer.
    pub fn exit_point_mode(&mut self) {
        self.point_mode = None;
    }

    /// Scroll the view so the current point-mode cursor is visible.
    fn adjust_view_for_point(&mut self) {
        if let Some(pm) = self.point_mode.clone() {
            let saved = (self.cursor_col, self.cursor_row);
            self.cursor_col = pm.cursor.0;
            self.cursor_row = pm.cursor.1;
            self.adjust_view();
            self.cursor_col = saved.0;
            self.cursor_row = saved.1;
        }
    }

    /// Character length of the input buffer.
    pub fn input_char_len(&self) -> usize {
        self.input_buffer.chars().count()
    }

    /// Byte offset in input_buffer corresponding to a character index.
    fn input_byte_offset(&self, char_idx: usize) -> usize {
        self.input_buffer
            .char_indices()
            .nth(char_idx)
            .map(|(i, _)| i)
            .unwrap_or(self.input_buffer.len())
    }

    /// Insert a character at the text cursor and advance the cursor.
    pub fn input_insert(&mut self, c: char) {
        let byte_pos = self.input_byte_offset(self.edit_cursor_pos);
        self.input_buffer.insert(byte_pos, c);
        self.edit_cursor_pos += 1;
    }

    /// Delete the character before the text cursor (Backspace).
    pub fn input_backspace(&mut self) {
        if self.edit_cursor_pos == 0 {
            return;
        }
        let start = self.input_byte_offset(self.edit_cursor_pos - 1);
        let end = self.input_byte_offset(self.edit_cursor_pos);
        self.input_buffer.drain(start..end);
        self.edit_cursor_pos -= 1;
    }

    /// Delete the character at the text cursor (Delete).
    pub fn input_delete(&mut self) {
        let len = self.input_char_len();
        if self.edit_cursor_pos >= len {
            return;
        }
        let start = self.input_byte_offset(self.edit_cursor_pos);
        let end = self.input_byte_offset(self.edit_cursor_pos + 1);
        self.input_buffer.drain(start..end);
    }

    /// Move text cursor left.
    pub fn input_cursor_left(&mut self) {
        if self.edit_cursor_pos > 0 {
            self.edit_cursor_pos -= 1;
        }
    }

    /// Move text cursor right.
    pub fn input_cursor_right(&mut self) {
        let len = self.input_char_len();
        if self.edit_cursor_pos < len {
            self.edit_cursor_pos += 1;
        }
    }

    pub fn input_cursor_home(&mut self) {
        self.edit_cursor_pos = 0;
    }

    pub fn input_cursor_end(&mut self) {
        self.edit_cursor_pos = self.input_char_len();
    }

    /// Delete all characters from the text cursor to the end (Ctrl+K).
    pub fn input_kill_to_end(&mut self) {
        let start = self.input_byte_offset(self.edit_cursor_pos);
        self.input_buffer.truncate(start);
    }

    /// Dispatch an action (from menu or shortcut).
    pub fn dispatch(&mut self, action: Action) {
        match action {
            Action::FileNew => {
                self.save_undo();
                self.sheet = Sheet::new();
                self.cursor_col = 0;
                self.cursor_row = 0;
                self.view_col = 0;
                self.view_row = 0;
                self.selection_anchor = None;
                self.current_file = None;
                self.status_message = "新規シート".to_string();
            }
            Action::FileOpen => {
                self.dialog = Some(Dialog {
                    kind: DialogKind::Open,
                    label: "開くファイル名".to_string(),
                    input: String::new(),
                });
                self.mode = Mode::Dialog;
            }
            Action::FileSave => {
                if let Some(filename) = self.current_file.clone() {
                    commands::save_to_file(self, &filename);
                } else {
                    self.dispatch(Action::FileSaveAs);
                }
            }
            Action::FileSaveAs => {
                self.dialog = Some(Dialog {
                    kind: DialogKind::SaveAs,
                    label: "保存ファイル名".to_string(),
                    input: self.current_file.clone().unwrap_or_default(),
                });
                self.mode = Mode::Dialog;
            }
            Action::FileImportCsv => {
                self.dialog = Some(Dialog {
                    kind: DialogKind::ImportCsv,
                    label: "CSVファイル名".to_string(),
                    input: String::new(),
                });
                self.mode = Mode::Dialog;
            }
            Action::FileExportCsv => {
                self.dialog = Some(Dialog {
                    kind: DialogKind::ExportCsv,
                    label: "エクスポート先".to_string(),
                    input: String::new(),
                });
                self.mode = Mode::Dialog;
            }
            Action::FilePrintHtml => {
                let default_name = match &self.current_file {
                    Some(path) => {
                        let stem = std::path::Path::new(path).file_stem()
                            .and_then(|s| s.to_str()).unwrap_or("sheet");
                        format!("{}.html", stem)
                    }
                    None => "sheet.html".to_string(),
                };
                self.dialog = Some(Dialog {
                    kind: DialogKind::PrintHtml,
                    label: "出力先 HTML (保存後ブラウザで開きます)".to_string(),
                    input: default_name,
                });
                self.mode = Mode::Dialog;
            }
            Action::FileQuit => {
                self.running = false;
            }
            Action::EditUndo => self.undo(),
            Action::EditRedo => self.redo(),
            Action::EditCopy => self.copy(),
            Action::EditCut => self.cut(),
            Action::EditPaste => self.paste(),
            Action::EditClear => self.clear_target(),
            Action::EditSelectAll => self.select_all(),
            Action::EditFind => {
                self.dialog = Some(Dialog {
                    kind: DialogKind::Find,
                    label: "検索".to_string(),
                    input: self.last_search.clone(),
                });
                self.mode = Mode::Dialog;
            }
            Action::EditGoto => {
                self.dialog = Some(Dialog {
                    kind: DialogKind::Goto,
                    label: "ジャンプ先セル (例: A1)".to_string(),
                    input: String::new(),
                });
                self.mode = Mode::Dialog;
            }
            Action::EditFindNext => {
                if self.last_search.is_empty() {
                    self.status_message = "検索文字列がありません".to_string();
                } else {
                    commands::search_forward(self);
                }
            }
            Action::EditFindPrev => {
                if self.last_search.is_empty() {
                    self.status_message = "検索文字列がありません".to_string();
                } else {
                    commands::search_backward(self);
                }
            }
            Action::InsertRow => {
                self.save_undo();
                self.sheet.adjust_formulas_for_row_insert(self.cursor_row);
                self.sheet.insert_row(self.cursor_row);
                self.status_message = format!("行を挿入 (行 {})", self.cursor_row + 1);
            }
            Action::InsertCol => {
                self.save_undo();
                self.sheet.adjust_formulas_for_col_insert(self.cursor_col);
                self.sheet.insert_col(self.cursor_col);
                self.status_message = format!("列を挿入 (列 {})", crate::formula::col_to_name(self.cursor_col));
            }
            Action::DeleteRow => {
                self.save_undo();
                self.sheet.adjust_formulas_for_row_delete(self.cursor_row);
                self.sheet.delete_row(self.cursor_row);
                self.status_message = format!("行 {} を削除", self.cursor_row + 1);
            }
            Action::DeleteCol => {
                self.save_undo();
                self.sheet.adjust_formulas_for_col_delete(self.cursor_col);
                self.sheet.delete_col(self.cursor_col);
                self.status_message = format!("列 {} を削除", crate::formula::col_to_name(self.cursor_col));
            }
            Action::FormatAutoWidth => {
                commands::autowidth_all(self);
            }
            Action::FormatWiderCol => {
                self.sheet.adjust_col_width(self.cursor_col, 1);
                let w = self.sheet.get_col_width(self.cursor_col);
                self.status_message = format!("列幅: {}", w);
            }
            Action::FormatNarrowerCol => {
                self.sheet.adjust_col_width(self.cursor_col, -1);
                let w = self.sheet.get_col_width(self.cursor_col);
                self.status_message = format!("列幅: {}", w);
            }
            Action::FormatSetWidth => {
                let cur = self.sheet.get_col_width(self.cursor_col);
                let col_name = crate::formula::col_to_name(self.cursor_col);
                self.dialog = Some(Dialog {
                    kind: DialogKind::SetColWidth,
                    label: format!("列 {} の幅 (3-50)", col_name),
                    input: cur.to_string(),
                });
                self.mode = Mode::Dialog;
            }
            Action::HelpKeys => {
                self.status_message = "矢印=移動 / Tab/Enter=次セル / F2=編集 / Ctrl+C/X/V=コピー切取貼付 / Ctrl+Z=戻 / Ctrl+S=保存 / F10=メニュー".to_string();
            }
            Action::HelpAbout => {
                self.status_message = format!("tbla {} - ターミナル表計算エディタ", env!("CARGO_PKG_VERSION"));
            }
        }
    }

    /// Execute a dialog action with the current input.
    pub fn commit_dialog(&mut self) {
        let Some(dialog) = self.dialog.clone() else { return; };
        let input = dialog.input.trim().to_string();

        match dialog.kind {
            DialogKind::Open => {
                let input = commands::sanitize_path_input(&input);
                if !input.is_empty() {
                    commands::load_from_file(self, &input);
                }
            }
            DialogKind::SaveAs => {
                let input = commands::sanitize_path_input(&input);
                if !input.is_empty() {
                    commands::save_to_file(self, &input);
                }
            }
            DialogKind::ImportCsv => {
                let input = commands::sanitize_path_input(&input);
                if !input.is_empty() {
                    commands::import_csv_file(self, &input);
                }
            }
            DialogKind::ExportCsv => {
                let input = commands::sanitize_path_input(&input);
                if !input.is_empty() {
                    commands::export_csv_file(self, &input);
                }
            }
            DialogKind::Find => {
                if !input.is_empty() {
                    self.last_search = input.clone();
                    commands::search_forward(self);
                }
            }
            DialogKind::Goto => {
                if let Some((col, row, _, _)) = crate::formula::parse_cell_ref(&input) {
                    self.cursor_col = col;
                    self.cursor_row = row;
                    self.selection_anchor = None;
                    self.adjust_view();
                    self.status_message = format!("{} に移動", crate::formula::cell_name(col, row));
                } else {
                    self.status_message = "無効なセル参照です".to_string();
                }
            }
            DialogKind::SetColWidth => {
                match input.parse::<usize>() {
                    Ok(w) => {
                        self.sheet.set_col_width(self.cursor_col, w);
                        let actual = self.sheet.get_col_width(self.cursor_col);
                        let name = crate::formula::col_to_name(self.cursor_col);
                        self.status_message = if actual == w {
                            format!("列 {} 幅: {}", name, actual)
                        } else {
                            format!("列 {} 幅: {} (3-50 にクランプ)", name, actual)
                        };
                    }
                    Err(_) => {
                        self.status_message = "無効な数値です".to_string();
                    }
                }
            }
            DialogKind::PrintHtml => {
                let input = commands::sanitize_path_input(&input);
                if !input.is_empty() {
                    commands::export_html_file(self, &input);
                }
            }
        }

        self.dialog = None;
        self.mode = Mode::Normal;
    }
}

/// Return true if the given cell holds a number (or a formula that evaluates
/// to a number). Used by the aggregate auto-completion to decide range bounds.
fn is_numeric_cell(sheet: &Sheet, col: usize, row: usize) -> bool {
    let cell = sheet.get_cell(col, row);
    match &cell.value {
        crate::cell::CellValue::Number(_) => true,
        crate::cell::CellValue::Formula(_) => {
            sheet.evaluate(col, row).parse::<f64>().is_ok()
        }
        _ => false,
    }
}

/// Find the contiguous numeric block adjacent to (col, row). Tries the cells
/// directly above first (preferred), then to the left. Returns the bounds
/// `(start_col, start_row, end_col, end_row)` of the range, or None.
fn detect_aggregate_range(
    sheet: &Sheet,
    col: usize,
    row: usize,
) -> Option<(usize, usize, usize, usize)> {
    // Try UP: walk up from (col, row-1) while cells are numeric.
    if row > 0 && is_numeric_cell(sheet, col, row - 1) {
        let mut start_row = row - 1;
        while start_row > 0 && is_numeric_cell(sheet, col, start_row - 1) {
            start_row -= 1;
        }
        return Some((col, start_row, col, row - 1));
    }

    // Then LEFT: walk left from (col-1, row).
    if col > 0 && is_numeric_cell(sheet, col - 1, row) {
        let mut start_col = col - 1;
        while start_col > 0 && is_numeric_cell(sheet, start_col - 1, row) {
            start_col -= 1;
        }
        return Some((start_col, row, col - 1, row));
    }

    None
}

/// If `input` is a bare aggregate-function reference such as `=sum`, `=avg`,
/// `=MIN()`, or `=Average( )`, build a completed formula with the auto-detected
/// range. Returns None when no completion applies (already has arguments, not
/// a supported function, or no adjacent numeric data).
fn autocomplete_aggregate(
    sheet: &Sheet,
    input: &str,
    col: usize,
    row: usize,
) -> Option<String> {
    let trimmed = input.trim();
    if !trimmed.starts_with('=') {
        return None;
    }
    let body = trimmed[1..].trim();

    // Accept `funcname`, `funcname()`, `funcname(  )`, or `funcname(`.
    let func_name = if let Some(idx) = body.find('(') {
        let name = body[..idx].trim();
        let rest = body[idx + 1..].trim_end_matches(')').trim();
        if !rest.is_empty() {
            return None; // already has arguments
        }
        name
    } else {
        body
    };

    if func_name.is_empty() {
        return None;
    }

    // Canonicalize the name. Aliases map to their engine-supported form.
    let canonical = match func_name.to_uppercase().as_str() {
        "SUM" => "SUM",
        "AVG" | "AVERAGE" => "AVERAGE",
        "MIN" => "MIN",
        "MAX" => "MAX",
        "COUNT" => "COUNT",
        "COUNTA" => "COUNTA",
        _ => return None,
    };

    let (sc, sr, ec, er) = detect_aggregate_range(sheet, col, row)?;
    let range = if sc == ec && sr == er {
        crate::formula::cell_name(sc, sr)
    } else {
        format!(
            "{}:{}",
            crate::formula::cell_name(sc, sr),
            crate::formula::cell_name(ec, er)
        )
    };

    Some(format!("={}({})", canonical, range))
}

#[cfg(test)]
mod autocomplete_tests {
    use super::*;

    fn sheet_with(cells: &[(usize, usize, &str)]) -> Sheet {
        let mut s = Sheet::new();
        for (c, r, v) in cells {
            s.set_cell(*c, *r, v.to_string());
        }
        s
    }

    #[test]
    fn completes_sum_using_column_above() {
        // A1..A3 are numbers, cursor at A4 typing =sum
        let s = sheet_with(&[(0, 0, "10"), (0, 1, "20"), (0, 2, "30")]);
        let out = autocomplete_aggregate(&s, "=sum", 0, 3);
        assert_eq!(out.as_deref(), Some("=SUM(A1:A3)"));
    }

    #[test]
    fn completes_average_alias_avg() {
        let s = sheet_with(&[(0, 0, "1"), (0, 1, "2")]);
        let out = autocomplete_aggregate(&s, "=avg", 0, 2);
        assert_eq!(out.as_deref(), Some("=AVERAGE(A1:A2)"));
    }

    #[test]
    fn completes_max_using_row_left_when_above_empty() {
        // B5..D5 are numbers, cursor at E5
        let s = sheet_with(&[(1, 4, "5"), (2, 4, "7"), (3, 4, "9")]);
        let out = autocomplete_aggregate(&s, "=max", 4, 4);
        assert_eq!(out.as_deref(), Some("=MAX(B5:D5)"));
    }

    #[test]
    fn prefers_above_over_left_when_both_have_data() {
        // A column above AND row to the left both have numbers
        let s = sheet_with(&[
            (1, 0, "10"),
            (1, 1, "20"),
            (0, 2, "5"),
        ]);
        // cursor at B3 typing =sum: above (B1,B2) wins over left (A3)
        let out = autocomplete_aggregate(&s, "=sum", 1, 2);
        assert_eq!(out.as_deref(), Some("=SUM(B1:B2)"));
    }

    #[test]
    fn keeps_existing_arguments() {
        let s = sheet_with(&[(0, 0, "1")]);
        let out = autocomplete_aggregate(&s, "=sum(A1:A5)", 0, 5);
        assert_eq!(out, None);
    }

    #[test]
    fn handles_empty_parens_and_whitespace() {
        let s = sheet_with(&[(0, 0, "1"), (0, 1, "2")]);
        assert_eq!(
            autocomplete_aggregate(&s, "=SUM()", 0, 2).as_deref(),
            Some("=SUM(A1:A2)")
        );
        assert_eq!(
            autocomplete_aggregate(&s, "= sum (  )", 0, 2).as_deref(),
            Some("=SUM(A1:A2)")
        );
    }

    #[test]
    fn no_adjacent_data_returns_none() {
        let s = Sheet::new();
        assert_eq!(autocomplete_aggregate(&s, "=sum", 5, 5), None);
    }

    #[test]
    fn non_numeric_above_blocks_extension() {
        // Header text "Total" interrupts the run upward.
        let s = sheet_with(&[(0, 0, "Total"), (0, 1, "10"), (0, 2, "20")]);
        let out = autocomplete_aggregate(&s, "=sum", 0, 3);
        assert_eq!(out.as_deref(), Some("=SUM(A2:A3)"));
    }

    #[test]
    fn single_cell_range_uses_bare_reference() {
        // Only one numeric cell above
        let s = sheet_with(&[(0, 0, "Header"), (0, 1, "5")]);
        let out = autocomplete_aggregate(&s, "=sum", 0, 2);
        assert_eq!(out.as_deref(), Some("=SUM(A2)"));
    }

    #[test]
    fn unknown_function_is_ignored() {
        let s = sheet_with(&[(0, 0, "1")]);
        assert_eq!(autocomplete_aggregate(&s, "=foobar", 0, 1), None);
    }

    #[test]
    fn formula_result_counts_as_numeric() {
        let s = sheet_with(&[(0, 0, "10"), (0, 1, "=A1*2")]);
        let out = autocomplete_aggregate(&s, "=sum", 0, 2);
        assert_eq!(out.as_deref(), Some("=SUM(A1:A2)"));
    }
}

fn handle_key(app: &mut App, key: KeyEvent) {
    match app.mode {
        Mode::Normal => handle_normal_mode(app, key),
        Mode::Edit => handle_edit_mode(app, key),
        Mode::Menu => handle_menu_mode(app, key),
        Mode::Dialog => handle_dialog_mode(app, key),
        Mode::ContextMenu => handle_context_menu_mode(app, key),
    }
}

fn handle_normal_mode(app: &mut App, key: KeyEvent) {
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    // Ctrl shortcuts
    if ctrl {
        match key.code {
            KeyCode::Char('c') | KeyCode::Char('C') => { app.dispatch(Action::EditCopy); return; }
            KeyCode::Char('x') | KeyCode::Char('X') => { app.dispatch(Action::EditCut); return; }
            KeyCode::Char('v') | KeyCode::Char('V') => { app.dispatch(Action::EditPaste); return; }
            KeyCode::Char('z') | KeyCode::Char('Z') => {
                if shift { app.dispatch(Action::EditRedo); } else { app.dispatch(Action::EditUndo); }
                return;
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => { app.dispatch(Action::EditRedo); return; }
            KeyCode::Char('s') | KeyCode::Char('S') => { app.dispatch(Action::FileSave); return; }
            KeyCode::Char('o') | KeyCode::Char('O') => { app.dispatch(Action::FileOpen); return; }
            KeyCode::Char('n') | KeyCode::Char('N') => { app.dispatch(Action::FileNew); return; }
            KeyCode::Char('q') | KeyCode::Char('Q') => { app.dispatch(Action::FileQuit); return; }
            KeyCode::Char('p') | KeyCode::Char('P') => { app.dispatch(Action::FilePrintHtml); return; }
            KeyCode::Char('f') | KeyCode::Char('F') => { app.dispatch(Action::EditFind); return; }
            KeyCode::Char('g') | KeyCode::Char('G') => { app.dispatch(Action::EditGoto); return; }
            KeyCode::Char('a') | KeyCode::Char('A') => { app.dispatch(Action::EditSelectAll); return; }
            // Vim-style cell movement with Ctrl modifier (hjkl).
            // Shift extends the selection (same semantics as Shift+arrow).
            KeyCode::Char('h') | KeyCode::Char('H') => { app.move_cursor(-1, 0, shift); return; }
            KeyCode::Char('j') | KeyCode::Char('J') => { app.move_cursor(0, 1, shift); return; }
            KeyCode::Char('k') | KeyCode::Char('K') => { app.move_cursor(0, -1, shift); return; }
            KeyCode::Char('l') | KeyCode::Char('L') => { app.move_cursor(1, 0, shift); return; }
            KeyCode::Home => {
                if shift && app.selection_anchor.is_none() {
                    app.selection_anchor = Some((app.cursor_col, app.cursor_row));
                } else if !shift {
                    app.selection_anchor = None;
                }
                app.cursor_col = 0;
                app.cursor_row = 0;
                app.adjust_view();
                return;
            }
            KeyCode::End => {
                if shift && app.selection_anchor.is_none() {
                    app.selection_anchor = Some((app.cursor_col, app.cursor_row));
                } else if !shift {
                    app.selection_anchor = None;
                }
                app.cursor_col = app.sheet.max_col().unwrap_or(0);
                app.cursor_row = app.sheet.max_row().unwrap_or(0);
                app.adjust_view();
                return;
            }
            KeyCode::Right => { move_to_data_edge(app, 1, 0, shift); return; }
            KeyCode::Left => { move_to_data_edge(app, -1, 0, shift); return; }
            KeyCode::Down => { move_to_data_edge(app, 0, 1, shift); return; }
            KeyCode::Up => { move_to_data_edge(app, 0, -1, shift); return; }
            _ => {}
        }
    }

    // Alt: open menu
    if alt {
        if let KeyCode::Char(c) = key.code {
            if app.menu_bar.activate_by_mnemonic(c, &mut app.menu_state) {
                app.mode = Mode::Menu;
                return;
            }
        }
    }

    match key.code {
        KeyCode::F(10) => {
            app.menu_state.open_first();
            app.mode = Mode::Menu;
        }
        KeyCode::F(2) => {
            app.begin_edit(None, true);
        }
        KeyCode::F(3) => {
            app.dispatch(Action::EditFindNext);
        }
        KeyCode::Left => app.move_cursor(-1, 0, shift),
        KeyCode::Right => app.move_cursor(1, 0, shift),
        KeyCode::Up => app.move_cursor(0, -1, shift),
        KeyCode::Down => app.move_cursor(0, 1, shift),
        KeyCode::Tab => app.move_cursor(1, 0, false),
        KeyCode::BackTab => app.move_cursor(-1, 0, false),
        KeyCode::Enter => app.move_cursor(0, 1, false),
        KeyCode::Home => {
            if shift && app.selection_anchor.is_none() {
                app.selection_anchor = Some((app.cursor_col, app.cursor_row));
            } else if !shift {
                app.selection_anchor = None;
            }
            app.cursor_col = 0;
            app.adjust_view();
        }
        KeyCode::End => {
            if shift && app.selection_anchor.is_none() {
                app.selection_anchor = Some((app.cursor_col, app.cursor_row));
            } else if !shift {
                app.selection_anchor = None;
            }
            app.cursor_col = app.sheet.max_col_in_row(app.cursor_row).unwrap_or(0);
            app.adjust_view();
        }
        KeyCode::PageDown => {
            let (_, term_height) = terminal::size().unwrap_or((80, 24));
            let page = (term_height as usize).saturating_sub(5).max(1) as isize;
            app.move_cursor(0, page, shift);
        }
        KeyCode::PageUp => {
            let (_, term_height) = terminal::size().unwrap_or((80, 24));
            let page = (term_height as usize).saturating_sub(5).max(1) as isize;
            app.move_cursor(0, -page, shift);
        }
        KeyCode::Delete | KeyCode::Backspace => {
            app.clear_target();
        }
        KeyCode::Esc => {
            app.selection_anchor = None;
        }
        KeyCode::Char(c) => {
            // Any printable char starts edit mode (Excel-style)
            if !ctrl && !alt {
                app.begin_edit(Some(c), false);
            }
        }
        _ => {}
    }
}

/// Ctrl+arrow: jump to next data/empty boundary in the given direction.
fn move_to_data_edge(app: &mut App, dx: isize, dy: isize, shift: bool) {
    if shift && app.selection_anchor.is_none() {
        app.selection_anchor = Some((app.cursor_col, app.cursor_row));
    } else if !shift {
        app.selection_anchor = None;
    }

    let mut col = app.cursor_col as isize;
    let mut row = app.cursor_row as isize;
    let max_col = 255isize;
    let max_row = 9999isize;

    let current_is_empty = app.sheet.get_cell_ref(app.cursor_col, app.cursor_row).is_none();

    if current_is_empty {
        // Move to next non-empty cell
        loop {
            col += dx;
            row += dy;
            if col < 0 || col > max_col || row < 0 || row > max_row {
                col -= dx;
                row -= dy;
                break;
            }
            if app.sheet.get_cell_ref(col as usize, row as usize).is_some() {
                break;
            }
        }
    } else {
        // Move to last non-empty cell before an empty one
        loop {
            let next_col = col + dx;
            let next_row = row + dy;
            if next_col < 0 || next_col > max_col || next_row < 0 || next_row > max_row {
                break;
            }
            if app.sheet.get_cell_ref(next_col as usize, next_row as usize).is_none() {
                // Step into the empty region, then to next non-empty
                col = next_col;
                row = next_row;
                loop {
                    let nc = col + dx;
                    let nr = row + dy;
                    if nc < 0 || nc > max_col || nr < 0 || nr > max_row {
                        break;
                    }
                    if app.sheet.get_cell_ref(nc as usize, nr as usize).is_some() {
                        col = nc;
                        row = nr;
                        break;
                    }
                    col = nc;
                    row = nr;
                }
                break;
            }
            col = next_col;
            row = next_row;
        }
    }

    app.cursor_col = col.max(0) as usize;
    app.cursor_row = row.max(0) as usize;
    app.adjust_view();
}

fn handle_edit_mode(app: &mut App, key: KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    // Ctrl shortcuts in edit mode
    if ctrl {
        match key.code {
            KeyCode::Char('z') | KeyCode::Char('Z') => {
                app.cancel_edit();
                app.dispatch(Action::EditUndo);
                return;
            }
            // Emacs/readline-style text cursor movement
            KeyCode::Char('a') | KeyCode::Char('A') => {
                app.input_cursor_home();
                return;
            }
            KeyCode::Char('e') | KeyCode::Char('E') => {
                app.input_cursor_end();
                return;
            }
            KeyCode::Char('f') | KeyCode::Char('F') => {
                app.input_cursor_right();
                return;
            }
            KeyCode::Char('b') | KeyCode::Char('B') => {
                app.input_cursor_left();
                return;
            }
            KeyCode::Char('k') | KeyCode::Char('K') => {
                app.input_kill_to_end();
                return;
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                app.input_delete();
                return;
            }
            _ => {}
        }
    }

    match key.code {
        KeyCode::Esc => {
            // First Esc in point mode exits point mode (keeps inserted text).
            // Second Esc cancels the edit entirely.
            if app.point_mode.is_some() {
                app.exit_point_mode();
            } else {
                app.cancel_edit();
            }
        }
        KeyCode::Enter => {
            app.exit_point_mode();
            app.commit_edit();
            app.mode = Mode::Normal;
            let dy: isize = if shift { -1 } else { 1 };
            app.move_cursor(0, dy, false);
        }
        KeyCode::Tab => {
            app.exit_point_mode();
            app.commit_edit();
            app.mode = Mode::Normal;
            app.move_cursor(1, 0, false);
        }
        KeyCode::BackTab => {
            app.exit_point_mode();
            app.commit_edit();
            app.mode = Mode::Normal;
            app.move_cursor(-1, 0, false);
        }
        KeyCode::Up => {
            if app.point_mode_allowed() {
                app.point_mode_arrow(0, -1, shift);
            } else {
                app.exit_point_mode();
                app.commit_edit();
                app.mode = Mode::Normal;
                app.move_cursor(0, -1, false);
            }
        }
        KeyCode::Down => {
            if app.point_mode_allowed() {
                app.point_mode_arrow(0, 1, shift);
            } else {
                app.exit_point_mode();
                app.commit_edit();
                app.mode = Mode::Normal;
                app.move_cursor(0, 1, false);
            }
        }
        KeyCode::Left => {
            if app.point_mode_allowed() {
                app.point_mode_arrow(-1, 0, shift);
            } else {
                app.input_cursor_left();
            }
        }
        KeyCode::Right => {
            if app.point_mode_allowed() {
                app.point_mode_arrow(1, 0, shift);
            } else {
                app.input_cursor_right();
            }
        }
        KeyCode::Home => {
            app.exit_point_mode();
            app.input_cursor_home();
        }
        KeyCode::End => {
            app.exit_point_mode();
            app.input_cursor_end();
        }
        KeyCode::Backspace => {
            app.exit_point_mode();
            app.input_backspace();
        }
        KeyCode::Delete => {
            app.exit_point_mode();
            app.input_delete();
        }
        KeyCode::Char(c) => {
            if !ctrl {
                // Typing any character exits point mode but keeps the
                // inserted reference text, then appends the typed character.
                app.exit_point_mode();
                app.input_insert(c);
            }
        }
        _ => {}
    }
}

fn handle_menu_mode(app: &mut App, key: KeyEvent) {
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    match key.code {
        KeyCode::Esc => {
            app.menu_state.close();
            app.mode = Mode::Normal;
        }
        KeyCode::Left => app.menu_state.move_left(&app.menu_bar),
        KeyCode::Right => app.menu_state.move_right(&app.menu_bar),
        KeyCode::Up => app.menu_state.move_up(&app.menu_bar),
        KeyCode::Down => app.menu_state.move_down(&app.menu_bar),
        KeyCode::Enter => {
            if let Some(action) = app.menu_state.activate(&app.menu_bar) {
                app.menu_state.close();
                app.mode = Mode::Normal;
                app.dispatch(action);
            }
        }
        KeyCode::Char(c) => {
            // Mnemonic
            if alt {
                if app.menu_bar.activate_by_mnemonic(c, &mut app.menu_state) {
                    return;
                }
            }
            // Try item mnemonic in current submenu
            if let Some(action) = app.menu_state.activate_by_mnemonic(&app.menu_bar, c) {
                app.menu_state.close();
                app.mode = Mode::Normal;
                app.dispatch(action);
            } else if app.menu_bar.activate_by_mnemonic(c, &mut app.menu_state) {
                // Switched top-level menu
            }
        }
        _ => {}
    }
}

fn handle_dialog_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.dialog = None;
            app.mode = Mode::Normal;
        }
        KeyCode::Enter => {
            app.commit_dialog();
        }
        KeyCode::Backspace => {
            if let Some(d) = app.dialog.as_mut() {
                d.input.pop();
            }
        }
        KeyCode::Char(c) => {
            if !key.modifiers.contains(KeyModifiers::CONTROL) {
                if let Some(d) = app.dialog.as_mut() {
                    d.input.push(c);
                }
            }
        }
        _ => {}
    }
}

fn handle_context_menu_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.context_menu = None;
            app.mode = Mode::Normal;
        }
        KeyCode::Up => {
            if let Some(cm) = app.context_menu.as_mut() {
                cm.move_up();
            }
        }
        KeyCode::Down => {
            if let Some(cm) = app.context_menu.as_mut() {
                cm.move_down();
            }
        }
        KeyCode::Enter => {
            let action = app.context_menu.as_ref().and_then(|cm| cm.activate());
            app.context_menu = None;
            app.mode = Mode::Normal;
            if let Some(a) = action {
                app.dispatch(a);
            }
        }
        _ => {}
    }
}

fn handle_mouse(app: &mut App, mouse: MouseEvent) {
    let col = mouse.column;
    let row = mouse.row;

    // Context menu mode: handle clicks
    if app.mode == Mode::ContextMenu {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let action = app.context_menu.as_ref().and_then(|cm| cm.hit_test(col, row));
                if action.is_some() {
                    app.context_menu = None;
                    app.mode = Mode::Normal;
                    app.dispatch(action.unwrap());
                } else {
                    app.context_menu = None;
                    app.mode = Mode::Normal;
                }
            }
            _ => {}
        }
        return;
    }

    // Menu mode: handle clicks
    if app.mode == Mode::Menu {
        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
            // Check menu bar (row 0)
            if row == 0 {
                if let Some(idx) = app.menu_bar.hit_test_bar(col) {
                    app.menu_state.open_index(idx);
                    return;
                }
            }
            // Check submenu
            if let Some(action) = app.menu_state.hit_test(&app.menu_bar, col, row) {
                app.menu_state.close();
                app.mode = Mode::Normal;
                app.dispatch(action);
                return;
            }
            // Click outside: close menu
            app.menu_state.close();
            app.mode = Mode::Normal;
        }
        return;
    }

    // Dialog mode ignores mouse
    if app.mode == Mode::Dialog {
        return;
    }

    // Menu bar click while in Normal/Edit
    if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
        if row == 0 {
            if let Some(idx) = app.menu_bar.hit_test_bar(col) {
                if app.mode == Mode::Edit {
                    app.commit_edit();
                }
                app.menu_state.open_index(idx);
                app.mode = Mode::Menu;
                return;
            }
        }
    }

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            // Column-width resize handle on the header row takes precedence
            // over cell selection / menu opening.
            if let Some(rcol) = app.screen_to_col_edge(col, row) {
                if app.mode == Mode::Edit {
                    app.commit_edit();
                    app.mode = Mode::Normal;
                }
                let w = app.sheet.get_col_width(rcol);
                app.column_resize = Some((rcol, col, w));
                return;
            }

            if let Some((c, r)) = app.screen_to_cell(col, row) {
                // Excel-style point mode: while editing a formula at a
                // reference-allowing position, clicking on a cell inserts the
                // reference instead of committing the edit.
                if app.mode == Mode::Edit && app.point_mode_allowed() {
                    app.point_mode_update(c, r, false);
                    app.dragging = true;
                    return;
                }

                if app.mode == Mode::Edit {
                    app.commit_edit();
                    app.mode = Mode::Normal;
                }

                // Double-click detection: same cell within 400ms → enter edit mode (preserve content)
                let now = std::time::Instant::now();
                let is_double = matches!(
                    (app.last_click_at, app.last_click_cell),
                    (Some(prev_time), Some(prev_pos))
                        if now.duration_since(prev_time).as_millis() < 400 && prev_pos == (c, r)
                );

                app.last_click_at = Some(now);
                app.last_click_cell = Some((c, r));

                if is_double {
                    app.cursor_col = c;
                    app.cursor_row = r;
                    app.selection_anchor = None;
                    app.adjust_view();
                    app.begin_edit(None, true);
                    app.dragging = false;
                    // Reset so a third click doesn't re-trigger
                    app.last_click_at = None;
                    app.last_click_cell = None;
                } else {
                    app.selection_anchor = None;
                    app.cursor_col = c;
                    app.cursor_row = r;
                    app.dragging = true;
                    app.adjust_view();
                }
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if let Some((rcol, start_x, start_w)) = app.column_resize {
                let delta = col as i32 - start_x as i32;
                let new_w = (start_w as i32 + delta).max(0) as usize;
                app.sheet.set_col_width(rcol, new_w);
                let w = app.sheet.get_col_width(rcol);
                app.status_message = format!("列 {} 幅: {}", crate::formula::col_to_name(rcol), w);
                return;
            }
            if app.dragging {
                if let Some((c, r)) = app.screen_to_cell(col, row) {
                    // In edit mode with active point mode, drag extends the
                    // referenced range instead of moving the cell cursor.
                    if app.mode == Mode::Edit && app.point_mode.is_some() {
                        app.point_mode_update(c, r, true);
                        return;
                    }
                    if app.selection_anchor.is_none() {
                        app.selection_anchor = Some((app.cursor_col, app.cursor_row));
                    }
                    app.cursor_col = c;
                    app.cursor_row = r;
                    app.adjust_view();
                }
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            if app.column_resize.is_some() {
                app.column_resize = None;
                return;
            }
            app.dragging = false;
            // If anchor == cursor, clear it (just a click, not drag)
            if let Some((ac, ar)) = app.selection_anchor {
                if ac == app.cursor_col && ar == app.cursor_row {
                    app.selection_anchor = None;
                }
            }
        }
        MouseEventKind::Down(MouseButton::Right) => {
            if let Some((c, r)) = app.screen_to_cell(col, row) {
                if app.mode == Mode::Edit {
                    app.commit_edit();
                    app.mode = Mode::Normal;
                }
                // If click is inside selection, keep selection; otherwise move cursor
                let inside_sel = if app.selection_anchor.is_some() {
                    let (min_c, min_r, max_c, max_r) = app.get_selection_bounds();
                    c >= min_c && c <= max_c && r >= min_r && r <= max_r
                } else {
                    c == app.cursor_col && r == app.cursor_row
                };
                if !inside_sel {
                    app.selection_anchor = None;
                    app.cursor_col = c;
                    app.cursor_row = r;
                    app.adjust_view();
                }
                let (term_width, term_height) = terminal::size().unwrap_or((80, 24));
                let cm = ContextMenu::new(col, row, term_width, term_height);
                app.context_menu = Some(cm);
                app.mode = Mode::ContextMenu;
            }
        }
        MouseEventKind::ScrollUp => {
            let scroll = 3;
            app.view_row = app.view_row.saturating_sub(scroll);
            app.cursor_row = app.cursor_row.saturating_sub(scroll);
        }
        MouseEventKind::ScrollDown => {
            let scroll = 3;
            app.view_row = (app.view_row + scroll).min(9999);
            app.cursor_row = (app.cursor_row + scroll).min(9999);
        }
        _ => {}
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    let mut stdout = stdout();
    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, Hide, EnableMouseCapture)?;

    // Enable Kitty Keyboard Protocol when the terminal supports it. Without
    // this, some terminals strip the SHIFT modifier from arrow keys once
    // mouse tracking is on, breaking Shift+Arrow range selection. Supported
    // by kitty, foot, WezTerm, Alacritty 0.13+, Ghostty, and recent iTerm2.
    let keyboard_enhancement = supports_keyboard_enhancement().unwrap_or(false);
    if keyboard_enhancement {
        let _ = execute!(
            stdout,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
    }

    let mut app = App::new();

    if args.len() > 1 {
        let filename = commands::sanitize_path_input(&args[1]);
        commands::load_from_file(&mut app, &filename);
    }

    UI::draw(&app)?;

    while app.running {
        if event::poll(std::time::Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == event::KeyEventKind::Press {
                        handle_key(&mut app, key);
                        UI::draw(&app)?;
                    }
                }
                Event::Mouse(mouse) => {
                    handle_mouse(&mut app, mouse);
                    UI::draw(&app)?;
                }
                Event::Resize(_, _) => {
                    UI::draw(&app)?;
                }
                _ => {}
            }
        }
    }

    if keyboard_enhancement {
        let _ = execute!(stdout, PopKeyboardEnhancementFlags);
    }
    execute!(stdout, Show, DisableMouseCapture, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;
    Ok(())
}
