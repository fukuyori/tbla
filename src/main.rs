mod cell;
mod date_util;
mod df_io;
mod df_view;
mod engine;
mod formula;
mod sheet;
mod ui;
mod commands;
mod menu;
mod xlsx;
mod xlsx_styles;
mod url_import;
mod sql_import;
mod width;

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
use menu::{MenuBar, MenuState, ContextMenu, Action, PopupItem, PopupMenu, PopupOutcome};

/// Operation modes
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Normal,
    Edit,
    Menu,
    Dialog,
    ContextMenu,
    /// WYSIWYG (":") cascading format popup — see `wysiwyg_menu_items`.
    Popup,
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
    Replace,
    Sort,
    Filter,
    SheetRename,
    TextColor,
    BgColor,
    NumberFormat,
    /// 書式 → セルの書式設定: one dialog covering number format, alignment,
    /// bold and colors. Choice fields are cycled with ←/→, colors are typed.
    CellFormat,
    /// 書式 → シートの既定書式: sheet-wide default number format that
    /// General cells inherit (l123's /Worksheet Global Format).
    SheetDefaultFormat,
    ConditionalAdd,
    AddComputedColumn,
    OpenCsvAsDf,
    SaveParquet,
    SqlQuery,
    GroupBy,
    /// Stage 1 of the URL-import flow: ask the user for the URL.
    FromUrl,
    /// Stage 2 of the URL-import flow: after the page is fetched and parsed,
    /// the user picks which `<table>` to import (1-based index into
    /// `pending_url_tables`) and where to put it (`s` = new sheet, `o` = overwrite).
    FromUrlPickTable,
    /// "データ → SQL から取り込み..." dialog. Fields: URI, query, destination.
    FromSql,
    /// 挿入 → 名前付き範囲を定義: fields are name and range text.
    NameDefine,
    /// 挿入 → 名前付き範囲の管理: shows the list, deletes the typed name.
    NameManage,
}

impl DialogKind {
    /// Title shown in the dialog box frame.
    pub fn title(&self) -> &'static str {
        match self {
            DialogKind::Open => "開く",
            DialogKind::SaveAs => "名前を付けて保存",
            DialogKind::ImportCsv => "CSVインポート",
            DialogKind::ExportCsv => "CSVエクスポート",
            DialogKind::Find => "検索",
            DialogKind::Goto => "ジャンプ",
            DialogKind::SetColWidth => "列幅を変更",
            DialogKind::PrintHtml => "印刷 (HTML)",
            DialogKind::Replace => "置換",
            DialogKind::Sort => "並べ替え",
            DialogKind::Filter => "フィルター",
            DialogKind::SheetRename => "シート名変更",
            DialogKind::TextColor => "文字色",
            DialogKind::BgColor => "背景色",
            DialogKind::NumberFormat => "数値書式",
            DialogKind::CellFormat => "セルの書式設定",
            DialogKind::SheetDefaultFormat => "シートの既定書式",
            DialogKind::ConditionalAdd => "条件付き書式",
            DialogKind::AddComputedColumn => "計算列を追加",
            DialogKind::OpenCsvAsDf => "CSV を DataFrame として開く",
            DialogKind::SaveParquet => "Parquet として保存",
            DialogKind::SqlQuery => "SQL クエリ",
            DialogKind::GroupBy => "グループ集計",
            DialogKind::FromUrl => "URLから取り込み",
            DialogKind::FromUrlPickTable => "取り込むテーブルを選択",
            DialogKind::FromSql => "SQL から取り込み",
            DialogKind::NameDefine => "名前付き範囲を定義",
            DialogKind::NameManage => "名前付き範囲の管理",
        }
    }
}

#[derive(Clone, Debug)]
pub struct DialogField {
    pub label: String,
    pub input: String,
    /// Non-empty ⇒ this is a choice field: the user picks `selected` from
    /// `options` with ←/→/Space, a mouse click, or by typing the option's
    /// first character. `input` is unused for choice fields.
    pub options: Vec<String>,
    pub selected: usize,
    /// For color-palette choice fields: one color per option (`None` =
    /// "no color"), rendered as a colored ■ swatch next to the name.
    pub swatches: Option<Vec<Option<crate::cell::RgbColor>>>,
}

impl DialogField {
    pub fn text(label: impl Into<String>, input: impl Into<String>) -> Self {
        DialogField {
            label: label.into(),
            input: input.into(),
            options: Vec::new(),
            selected: 0,
            swatches: None,
        }
    }

    pub fn choice(label: impl Into<String>, options: &[&str], selected: usize) -> Self {
        let selected = selected.min(options.len().saturating_sub(1));
        DialogField {
            label: label.into(),
            input: String::new(),
            options: options.iter().map(|s| s.to_string()).collect(),
            selected,
            swatches: None,
        }
    }

    /// Color-palette choice field pre-selected on `current`. If the cell's
    /// current color isn't in the palette (e.g. imported from xlsx), it is
    /// appended as a "現在" entry so committing untouched changes nothing.
    pub fn palette(
        label: impl Into<String>,
        palette: &[(&str, Option<crate::cell::RgbColor>)],
        current: Option<crate::cell::RgbColor>,
    ) -> Self {
        let mut options: Vec<String> = palette.iter().map(|(n, _)| n.to_string()).collect();
        let mut swatches: Vec<Option<crate::cell::RgbColor>> =
            palette.iter().map(|(_, c)| *c).collect();
        let selected = match swatches.iter().position(|c| *c == current) {
            Some(i) => i,
            None => {
                options.push("現在".to_string());
                swatches.push(current);
                options.len() - 1
            }
        };
        DialogField {
            label: label.into(),
            input: String::new(),
            options,
            selected,
            swatches: Some(swatches),
        }
    }

    pub fn is_choice(&self) -> bool {
        !self.options.is_empty()
    }

    /// Move the selection of a choice field by ±1, wrapping around.
    pub fn cycle(&mut self, delta: isize) {
        let n = self.options.len();
        if n == 0 { return; }
        self.selected = (self.selected as isize + delta).rem_euclid(n as isize) as usize;
    }

    /// Select the option matching a typed character (exact option text
    /// first — so '1' picks "1" not "10" — then prefix match).
    pub fn select_by_char(&mut self, c: char) {
        let s = c.to_string();
        if let Some(i) = self.options.iter().position(|o| **o == s) {
            self.selected = i;
        } else if let Some(i) = self.options.iter().position(|o| o.starts_with(c)) {
            self.selected = i;
        }
    }
}

#[derive(Clone, Debug)]
pub struct Dialog {
    pub kind: DialogKind,
    /// One or more input fields. UI renders them stacked above the formula
    /// bar; Tab / Shift+Tab cycles focus between them. Field 0 is always the
    /// initially-focused field.
    pub fields: Vec<DialogField>,
    pub focus: usize,
}

impl Dialog {
    pub fn single(kind: DialogKind, label: impl Into<String>, input: impl Into<String>) -> Self {
        Dialog {
            kind,
            fields: vec![DialogField::text(label, input)],
            focus: 0,
        }
    }

    pub fn multi(kind: DialogKind, fields: Vec<DialogField>) -> Self {
        Dialog { kind, fields, focus: 0 }
    }

    pub fn current_input_mut(&mut self) -> &mut String {
        &mut self.fields[self.focus].input
    }

    /// First field's input — kept for backward compatibility with single-field
    /// dialogs that just want one trimmed value.
    pub fn primary_input(&self) -> &str {
        &self.fields[0].input
    }

    pub fn next_field(&mut self) {
        self.focus = (self.focus + 1) % self.fields.len();
    }

    pub fn prev_field(&mut self) {
        if self.focus == 0 {
            self.focus = self.fields.len() - 1;
        } else {
            self.focus -= 1;
        }
    }
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
    pub last_replace: String,
    pub menu_bar: MenuBar,
    pub menu_state: MenuState,
    pub dialog: Option<Dialog>,
    pub context_menu: Option<ContextMenu>,
    /// WYSIWYG (":") popup menu state; Some iff mode == Mode::Popup.
    pub popup: Option<PopupMenu>,
    pub dragging: bool,
    pub last_click_at: Option<std::time::Instant>,
    pub last_click_cell: Option<(usize, usize)>,
    pub point_mode: Option<PointMode>,
    /// Active column-width drag: (column index, screen x where the drag began,
    /// the column's width at the start of the drag).
    pub column_resize: Option<(usize, u16, usize)>,
    /// Rows hidden by an active filter. Session-only — cleared on file save
    /// and not persisted in any file format.
    pub hidden_rows: std::collections::HashSet<usize>,
    /// Workbook structure: `sheet` is the currently active sheet's data;
    /// `other_sheets` holds the other sheets in workbook order (i.e. with
    /// the active sheet *removed*); `active_sheet_index` is where the active
    /// sheet sits in the logical workbook ordering. Switching sheets does a
    /// swap so call sites using `app.sheet` keep working transparently.
    pub other_sheets: Vec<Sheet>,
    pub active_sheet_index: usize,
    /// Tables fetched by the URL-import flow, awaiting the user's pick.
    /// Cleared once the second-stage dialog closes (committed or cancelled).
    pub pending_url_tables: Vec<url_import::ExtractedTable>,
    /// Source URL of `pending_url_tables` — used to name the new sheet when
    /// the user picks "new sheet" and the table has no `<caption>`.
    pub pending_url_source: String,
    /// Last SQL connection URI used (session-only) — pre-filled into the
    /// "SQL から取り込み" dialog so the user doesn't have to retype it
    /// between queries.
    pub last_sql_uri: String,
    /// Last SQL query (session-only).
    pub last_sql_query: String,
    /// Workbook-level named ranges (source of truth). Each sheet carries a
    /// derived, sheet-relative resolution map (`Sheet::names`) kept in sync
    /// via `sync_named_ranges`.
    pub named_ranges: Vec<NamedRange>,
}

/// A named range: `name` refers to the inclusive rectangle `start..=end`
/// on the sheet called `sheet`. Names are unique case-insensitively.
#[derive(Clone, Debug)]
pub struct NamedRange {
    pub name: String,
    pub sheet: String,
    pub start: (usize, usize), // (col, row)
    pub end: (usize, usize),
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
            last_replace: String::new(),
            menu_bar: MenuBar::default(),
            menu_state: MenuState::default(),
            dialog: None,
            context_menu: None,
            popup: None,
            dragging: false,
            last_click_at: None,
            last_click_cell: None,
            point_mode: None,
            column_resize: None,
            hidden_rows: std::collections::HashSet::new(),
            other_sheets: Vec::new(),
            active_sheet_index: 0,
            pending_url_tables: Vec::new(),
            pending_url_source: String::new(),
            last_sql_uri: String::new(),
            last_sql_query: String::new(),
            named_ranges: Vec::new(),
        }
    }

    /// Number of sheets in the workbook (active + others).
    pub fn sheet_count(&self) -> usize {
        self.other_sheets.len() + 1
    }

    /// Active sheet's name (convenience).
    pub fn active_sheet_name(&self) -> &str {
        &self.sheet.name
    }

    /// Build the (name, &cells) slice that powers cross-sheet formula
    /// references. Includes every sheet EXCEPT the active one (foreign cells
    /// only).
    pub fn other_sheet_refs(&self) -> Vec<(String, &std::collections::HashMap<(usize, usize), crate::cell::Cell>)> {
        self.other_sheets.iter()
            .map(|s| (s.name.clone(), s.cells()))
            .collect()
    }

    /// Evaluate a cell on the active sheet with cross-sheet ref support.
    pub fn evaluate(&self, col: usize, row: usize) -> String {
        let others = self.other_sheet_refs();
        self.sheet.evaluate_with(col, row, &others)
    }

    /// All sheets in workbook order, with the active sheet inserted at its
    /// position. Used for the tab bar, save, and cross-sheet formula lookups.
    pub fn workbook_sheets(&self) -> Vec<&Sheet> {
        let mut v: Vec<&Sheet> = self.other_sheets.iter().collect();
        v.insert(self.active_sheet_index.min(v.len()), &self.sheet);
        v
    }

    /// Switch the active sheet to the given workbook-order index. No-op if
    /// the index is out of range or already active.
    pub fn switch_sheet(&mut self, target: usize) {
        let total = self.sheet_count();
        if target >= total || target == self.active_sheet_index { return; }
        // Step 1: put the current active sheet back into other_sheets at
        // its logical position, replacing it with a placeholder.
        let placeholder = Sheet::new();
        let current = std::mem::replace(&mut self.sheet, placeholder);
        self.other_sheets.insert(self.active_sheet_index, current);
        // Now other_sheets contains every sheet in workbook order.
        // Pop the target out and make it active.
        let new_active = self.other_sheets.remove(target);
        self.sheet = new_active;
        self.active_sheet_index = target;
        // Filters are sheet-local and shouldn't bleed across switches.
        self.hidden_rows.clear();
        self.selection_anchor = None;
        self.cursor_col = 0;
        self.cursor_row = 0;
        self.view_col = 0;
        self.view_row = 0;
    }

    /// Add a new empty sheet right after the active one and switch to it.
    /// Returns the new sheet's name.
    pub fn add_sheet(&mut self, name: Option<String>) -> String {
        let n = name.unwrap_or_else(|| {
            // Auto-name: Sheet2, Sheet3, ... avoiding duplicates.
            let existing: std::collections::HashSet<String> = self.workbook_sheets()
                .iter().map(|s| s.name.clone()).collect();
            let mut i = self.sheet_count() + 1;
            loop {
                let cand = format!("Sheet{}", i);
                if !existing.contains(&cand) { break cand; }
                i += 1;
            }
        });
        let mut new_sheet = Sheet::new();
        new_sheet.name = n.clone();
        let insert_at = self.active_sheet_index + 1;
        // Push current active back into others to make room, then move active.
        let placeholder = Sheet::new();
        let prev_active = std::mem::replace(&mut self.sheet, placeholder);
        self.other_sheets.insert(self.active_sheet_index, prev_active);
        self.other_sheets.insert(insert_at, new_sheet);
        self.sheet = self.other_sheets.remove(insert_at);
        self.active_sheet_index = insert_at;
        self.hidden_rows.clear();
        self.selection_anchor = None;
        self.cursor_col = 0; self.cursor_row = 0;
        self.view_col = 0; self.view_row = 0;
        self.sync_named_ranges();
        n
    }

    /// Delete the active sheet. If it's the only sheet, the call is ignored
    /// (workbook must always have at least one sheet).
    pub fn delete_active_sheet(&mut self) -> bool {
        if self.sheet_count() <= 1 { return false; }
        let deleted_name = self.sheet.name.clone();
        // Take the next sheet (or previous if active is last) as new active.
        let new_active_index = if self.active_sheet_index < self.other_sheets.len() {
            self.active_sheet_index
        } else {
            self.active_sheet_index - 1
        };
        let new_active = self.other_sheets.remove(new_active_index);
        self.sheet = new_active;
        self.active_sheet_index = new_active_index;
        self.hidden_rows.clear();
        self.selection_anchor = None;
        self.cursor_col = 0; self.cursor_row = 0;
        self.view_col = 0; self.view_row = 0;
        // Named ranges on the deleted sheet have nothing to refer to anymore.
        self.named_ranges.retain(|nr| nr.sheet != deleted_name);
        self.sync_named_ranges();
        true
    }

    /// Rename the active sheet. Returns false if `new_name` clashes with
    /// an existing sheet name (case-insensitive).
    pub fn rename_active_sheet(&mut self, new_name: &str) -> bool {
        let new_name = new_name.trim();
        if new_name.is_empty() { return false; }
        let lower = new_name.to_lowercase();
        for s in &self.other_sheets {
            if s.name.to_lowercase() == lower { return false; }
        }
        let old_name = std::mem::replace(&mut self.sheet.name, new_name.to_string());
        // Follow the rename in named-range definitions.
        for nr in &mut self.named_ranges {
            if nr.sheet == old_name {
                nr.sheet = self.sheet.name.clone();
            }
        }
        self.sync_named_ranges();
        true
    }

    /// Look up a named range case-insensitively.
    pub fn find_named_range(&self, name: &str) -> Option<&NamedRange> {
        let upper = name.trim().to_uppercase();
        self.named_ranges.iter().find(|nr| nr.name.to_uppercase() == upper)
    }

    /// Recompute every sheet's `names` resolution map from the workbook-level
    /// definitions. Cheap; call after any change to `named_ranges`, sheet
    /// renames/deletes, and file load.
    pub fn sync_named_ranges(&mut self) {
        fn resolved_for(nr: &NamedRange, sheet_name: &str) -> Option<String> {
            let single = nr.start == nr.end;
            if nr.sheet == sheet_name {
                Some(if single {
                    crate::formula::cell_name(nr.start.0, nr.start.1)
                } else {
                    format!(
                        "{}:{}",
                        crate::formula::cell_name(nr.start.0, nr.start.1),
                        crate::formula::cell_name(nr.end.0, nr.end.1),
                    )
                })
            } else if single {
                // Cross-sheet single cell rides the existing `Sheet!A1` path.
                Some(format!(
                    "{}!{}",
                    nr.sheet,
                    crate::formula::cell_name(nr.start.0, nr.start.1)
                ))
            } else {
                // Cross-sheet ranges are unsupported by the engine; leaving
                // the name unresolved yields an honest #NAME? instead of a
                // wrong-cell result.
                None
            }
        }
        let ranges = self.named_ranges.clone();
        let sync_one = |sheet: &mut Sheet| {
            sheet.names.clear();
            for nr in &ranges {
                if let Some(text) = resolved_for(nr, &sheet.name) {
                    sheet.names.insert(nr.name.to_uppercase(), text);
                }
            }
        };
        sync_one(&mut self.sheet);
        for s in &mut self.other_sheets {
            sync_one(s);
        }
    }

    /// Validate and store a named range on the active sheet, replacing any
    /// existing definition with the same (case-insensitive) name.
    /// Returns the normalized range text on success.
    pub fn define_named_range(
        &mut self,
        name: &str,
        range_text: &str,
    ) -> std::result::Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("名前を入力してください".to_string());
        }
        if name.chars().next().map_or(false, |c| c.is_ascii_digit()) {
            return Err("名前は数字で始められません".to_string());
        }
        if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err("名前に使えるのは英数字・日本語・_ のみです".to_string());
        }
        // Anything the reference parser would read as a cell address (A1,
        // XFD10, ABC_12 → ABC12, ...) can never be reached as a name inside
        // a formula, so refuse it outright.
        if crate::formula::parse_cell_ref(name).is_some() {
            return Err("セル参照と紛らわしい名前は使えません".to_string());
        }

        let parts: Vec<&str> = range_text.trim().split(':').collect();
        let (start, end) = match parts.as_slice() {
            [one] => {
                let (c, r, _, _) = crate::formula::parse_cell_ref(one)
                    .ok_or_else(|| format!("無効な範囲: {}", range_text))?;
                ((c, r), (c, r))
            }
            [a, b] => {
                let (c1, r1, _, _) = crate::formula::parse_cell_ref(a)
                    .ok_or_else(|| format!("無効な範囲: {}", range_text))?;
                let (c2, r2, _, _) = crate::formula::parse_cell_ref(b)
                    .ok_or_else(|| format!("無効な範囲: {}", range_text))?;
                ((c1.min(c2), r1.min(r2)), (c1.max(c2), r1.max(r2)))
            }
            _ => return Err(format!("無効な範囲: {}", range_text)),
        };

        let upper = name.to_uppercase();
        self.named_ranges.retain(|nr| nr.name.to_uppercase() != upper);
        let sheet = self.sheet.name.clone();
        self.named_ranges.push(NamedRange {
            name: name.to_string(),
            sheet,
            start,
            end,
        });
        self.sync_named_ranges();
        Ok(if start == end {
            crate::formula::cell_name(start.0, start.1)
        } else {
            format!(
                "{}:{}",
                crate::formula::cell_name(start.0, start.1),
                crate::formula::cell_name(end.0, end.1),
            )
        })
    }

    /// Delete a named range (case-insensitive). Returns true if one existed.
    pub fn delete_named_range(&mut self, name: &str) -> bool {
        let upper = name.trim().to_uppercase();
        let before = self.named_ranges.len();
        self.named_ranges.retain(|nr| nr.name.to_uppercase() != upper);
        let removed = self.named_ranges.len() != before;
        if removed {
            self.sync_named_ranges();
        }
        removed
    }

    /// One-line summary of the defined names for status/dialog display.
    pub fn named_range_summary(&self) -> String {
        self.named_ranges
            .iter()
            .map(|nr| nr.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
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
        let mut new_row = (self.cursor_row as isize + dy).max(0).min(9999) as usize;
        // When a filter is active, walking +/- 1 row should skip hidden rows.
        // For multi-step vertical jumps we still respect the visible-row
        // count, so PageUp/PageDown move by `dy` *visible* rows.
        if !self.hidden_rows.is_empty() && dy != 0 {
            let dir: isize = if dy > 0 { 1 } else { -1 };
            let mut remaining = dy.abs();
            let mut r = self.cursor_row as isize;
            while remaining > 0 {
                r += dir;
                if r < 0 { r = 0; break; }
                if r > 9999 { r = 9999; break; }
                if !self.hidden_rows.contains(&(r as usize)) {
                    remaining -= 1;
                }
            }
            new_row = r.max(0).min(9999) as usize;
        }
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
        // When the tab bar is visible we lose one more row.
        let tab_rows = if self.sheet_count() > 1 { 1 } else { 0 };
        let visible_rows = (term_height as usize).saturating_sub(HEADER_ROWS + FOOTER_ROWS + tab_rows);

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
        let tab_rows = if self.sheet_count() > 1 { 1 } else { 0 };
        let grid_height = (term_height as usize).saturating_sub(HEADER_ROWS + 2 + tab_rows);

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
                // Map the on-screen row offset back to a logical row,
                // skipping any rows hidden by an active filter.
                let target_offset = screen_row - HEADER_ROWS;
                let mut logical = self.view_row;
                let mut visible_seen = 0usize;
                while logical < 10000 {
                    if !self.hidden_rows.contains(&logical) {
                        if visible_seen == target_offset {
                            return Some((col, logical));
                        }
                        visible_seen += 1;
                    }
                    logical += 1;
                }
                return None;
            }
            x += col_width;
            col += 1;
        }

        None
    }

    /// If the click landed on a sheet tab in the tab bar, return that tab's
    /// workbook-order index. The tab bar lives at screen row `term_height-3`
    /// when the workbook has more than one sheet.
    pub fn screen_to_sheet_tab(&self, screen_col: u16, screen_row: u16) -> Option<usize> {
        if self.sheet_count() <= 1 { return None; }
        let (_, term_height) = terminal::size().unwrap_or((80, 24));
        if screen_row != term_height.saturating_sub(3) { return None; }
        // Mirror the layout used by draw_sheet_tabs: " name " segments
        // separated by single spaces.
        let mut x = 0u16;
        for (idx, sheet) in self.workbook_sheets().iter().enumerate() {
            let label_width = crate::width::str_width(format!(" {} ", sheet.name).as_str()) as u16;
            if screen_col >= x && screen_col < x + label_width {
                return Some(idx);
            }
            x += label_width + 1; // +1 for the inter-tab space
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

        let w = clip.width;
        let h = clip.height;

        // Decide where to paste and how big the target is. When a multi-cell
        // selection is active and larger than the clipboard, fill the whole
        // selection — extending arithmetic/text-number series so the increment
        // (増減) is carried through, Excel-style. Otherwise just drop the block
        // at the cursor (origin = selection top-left, which is the cursor when
        // there's no selection).
        let (sel_min_c, sel_min_r, sel_max_c, sel_max_r) = self.get_selection_bounds();
        let origin_col = sel_min_c;
        let origin_row = sel_min_r;
        let (dest_w, dest_h) = if self.has_selection() {
            (
                (sel_max_c - sel_min_c + 1).max(w),
                (sel_max_r - sel_min_r + 1).max(h),
            )
        } else {
            (w, h)
        };

        // Primary fill direction. A taller target extends each column downward
        // (and tiles columns sideways); otherwise a wider target extends each
        // row rightward (tiling rows downward). When the target matches the
        // clipboard size, this is a plain block paste.
        let vertical_fill = dest_h > h;
        let horizontal_fill = dest_w > w;

        for co in 0..dest_w {
            for ro in 0..dest_h {
                let dst_col = origin_col + co;
                let dst_row = origin_row + ro;

                let raw = match compute_fill(&clip, co, ro, vertical_fill, horizontal_fill) {
                    FillCell::Source { sc, sr } => {
                        let src = &clip.cells[sr][sc].0;
                        if src.starts_with('=') {
                            let col_delta =
                                dst_col as isize - (clip.start_col + sc) as isize;
                            let row_delta =
                                dst_row as isize - (clip.start_row + sr) as isize;
                            formula::adjust_formula(src, col_delta, row_delta)
                        } else {
                            src.clone()
                        }
                    }
                    FillCell::Literal(s) => s,
                };

                self.sheet.set_cell(dst_col, dst_row, raw);
            }
        }

        if dest_w > w || dest_h > h {
            self.status_message = format!("連続貼り付け: {}x{} セル", dest_w, dest_h);
        } else {
            self.status_message = format!("貼り付け: {}x{} セル", w, h);
        }
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
        // In DataFrame view, the initial content comes from the DataFrame
        // (header name for row 0, cell value otherwise). Out-of-bounds
        // editing is silently rejected.
        let existing = if let Some(view) = self.sheet.df_view.as_ref() {
            if self.cursor_col >= view.cols() {
                self.status_message = "範囲外のセルは編集できません".into();
                return;
            }
            if self.cursor_row == 0 {
                view.header(self.cursor_col)
            } else if self.cursor_row - 1 < view.rows() {
                view.value_at(self.cursor_col, self.cursor_row - 1)
            } else {
                self.status_message = "範囲外のセルは編集できません（行を追加する操作は未対応）".into();
                return;
            }
        } else {
            let cell = self.sheet.get_cell(self.cursor_col, self.cursor_row);
            cell.raw_input.clone()
        };
        self.edit_original = existing.clone();
        if preserve {
            self.input_buffer = existing;
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
        // DataFrame-view path: row 0 renames a column, other rows mutate
        // the underlying typed cell. Aggregate autocomplete is skipped
        // here because DataFrame cells aren't formulas.
        if self.sheet.df_view.is_some() {
            if self.input_buffer != self.edit_original {
                self.save_undo();
                let col = self.cursor_col;
                let row = self.cursor_row;
                let buf = self.input_buffer.clone();
                let view = self.sheet.df_view.as_mut().unwrap();
                let res = if row == 0 {
                    crate::df_view::rename_column(view, col, &buf)
                } else {
                    crate::df_view::set_cell(view, col, row - 1, &buf)
                };
                if let Err(e) = res {
                    self.status_message = format!("編集エラー: {}", e);
                }
            }
            self.input_buffer.clear();
            self.edit_cursor_pos = 0;
            self.edit_original.clear();
            self.point_mode = None;
            return;
        }

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

    /// F4 while editing a formula: cycle the `$` anchoring of the cell
    /// reference at (or just before) the text cursor, Excel/1-2-3 style:
    /// A1 → $A$1 → A$1 → $A1 → A1. Ranges cycle both endpoints together.
    pub fn cycle_ref_absolute(&mut self) {
        let chars: Vec<char> = self.input_buffer.chars().collect();
        if chars.first() != Some(&'=') {
            return;
        }
        let is_ref_char = |c: char| c.is_ascii_alphanumeric() || c == '$' || c == ':';
        let pos = self.edit_cursor_pos.min(chars.len());
        let mut start = pos;
        while start > 0 && is_ref_char(chars[start - 1]) {
            start -= 1;
        }
        let mut end = pos;
        while end < chars.len() && is_ref_char(chars[end]) {
            end += 1;
        }
        if start == end {
            return;
        }
        let token: String = chars[start..end].iter().collect();
        let Some(cycled) = cycle_ref_token(&token) else { return; };
        let cycled_chars = cycled.chars().count();

        let byte_start = self.input_byte_offset(start);
        let byte_end = self.input_byte_offset(end);
        self.input_buffer.replace_range(byte_start..byte_end, &cycled);
        self.edit_cursor_pos = start + cycled_chars;

        // Keep point mode consistent: if the cycled token is exactly the
        // reference point mode inserted, track its new length; otherwise the
        // buffer no longer matches what point mode believes it inserted.
        if let Some(pm) = self.point_mode.as_mut() {
            if pm.insert_pos == start {
                pm.inserted_chars = cycled_chars;
            } else {
                self.point_mode = None;
            }
        }
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
                self.dialog = Some(Dialog::single(DialogKind::Open, "開くファイル名", ""));
                self.mode = Mode::Dialog;
            }
            Action::FileSave => {
                if let Some(filename) = self.current_file.clone() {
                    // Filters are session-only; clear before save so the file
                    // doesn't capture hidden state and the user sees the full
                    // sheet again after the save.
                    self.hidden_rows.clear();
                    commands::save_to_file(self, &filename);
                } else {
                    self.dispatch(Action::FileSaveAs);
                }
            }
            Action::FileSaveAs => {
                self.dialog = Some(Dialog::single(
                    DialogKind::SaveAs,
                    "保存ファイル名",
                    self.current_file.clone().unwrap_or_default(),
                ));
                self.mode = Mode::Dialog;
            }
            Action::FileImportCsv => {
                self.dialog = Some(Dialog::single(DialogKind::ImportCsv, "CSVファイル名", ""));
                self.mode = Mode::Dialog;
            }
            Action::FileExportCsv => {
                self.dialog = Some(Dialog::single(DialogKind::ExportCsv, "エクスポート先", ""));
                self.mode = Mode::Dialog;
            }
            Action::FileOpenCsvAsDf => {
                self.dialog = Some(Dialog::single(
                    DialogKind::OpenCsvAsDf,
                    "CSV ファイル名 (Polars で読み込み、DataFrame ビューで開きます)",
                    "",
                ));
                self.mode = Mode::Dialog;
            }
            Action::FileSaveParquet => {
                let default_name = match &self.current_file {
                    Some(path) => {
                        let stem = std::path::Path::new(path).file_stem()
                            .and_then(|s| s.to_str()).unwrap_or("data");
                        format!("{}.parquet", stem)
                    }
                    None => "data.parquet".to_string(),
                };
                self.dialog = Some(Dialog::single(
                    DialogKind::SaveParquet,
                    "保存先 Parquet ファイル",
                    default_name,
                ));
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
                self.dialog = Some(Dialog::single(
                    DialogKind::PrintHtml,
                    "出力先 HTML (保存後ブラウザで開きます)",
                    default_name,
                ));
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
                self.dialog = Some(Dialog::single(DialogKind::Find, "検索", self.last_search.clone()));
                self.mode = Mode::Dialog;
            }
            Action::EditGoto => {
                self.dialog = Some(Dialog::single(
                    DialogKind::Goto,
                    "ジャンプ先 (セル例: A1 / 名前付き範囲)",
                    "",
                ));
                self.mode = Mode::Dialog;
            }
            Action::Recalc => {
                // Formulas are re-evaluated on every redraw, so an explicit
                // recalc only needs to trigger a frame; it also re-rolls
                // volatile functions (RAND / NOW / TODAY).
                self.status_message = "再計算しました".to_string();
            }
            Action::NameDefine => {
                let (min_c, min_r, max_c, max_r) = self.get_selection_bounds();
                let range_text = if (min_c, min_r) == (max_c, max_r) {
                    crate::formula::cell_name(min_c, min_r)
                } else {
                    format!(
                        "{}:{}",
                        crate::formula::cell_name(min_c, min_r),
                        crate::formula::cell_name(max_c, max_r),
                    )
                };
                self.dialog = Some(Dialog::multi(DialogKind::NameDefine, vec![
                    DialogField::text("名前 (数式・ジャンプで使用)", String::new()),
                    DialogField::text("範囲 (例: A1:B5)", range_text),
                ]));
                self.mode = Mode::Dialog;
            }
            Action::NameManage => {
                if self.named_ranges.is_empty() {
                    self.status_message = "名前付き範囲はありません".to_string();
                } else {
                    let label = format!(
                        "削除する名前 (定義済み: {})",
                        self.named_range_summary()
                    );
                    self.dialog = Some(Dialog::single(DialogKind::NameManage, label, ""));
                    self.mode = Mode::Dialog;
                }
            }
            Action::EditReplace => {
                self.dialog = Some(Dialog::multi(DialogKind::Replace, vec![
                    DialogField::text("検索 (find)", self.last_search.clone()),
                    DialogField::text("置換 (replace)", self.last_replace.clone()),
                ]));
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
            Action::DataSort => {
                let col = crate::formula::col_to_name(self.cursor_col);
                self.dialog = Some(Dialog::multi(DialogKind::Sort, vec![
                    DialogField::text("並べ替え列 (例: B)", col),
                    DialogField::text("順序 (asc / desc)", "asc"),
                    DialogField::text("ヘッダー行を含む (y / n)", "y"),
                ]));
                self.mode = Mode::Dialog;
            }
            Action::DataFilter => {
                let col = crate::formula::col_to_name(self.cursor_col);
                self.dialog = Some(Dialog::multi(DialogKind::Filter, vec![
                    DialogField::text("フィルター対象列 (例: B)", col),
                    DialogField::text("条件 (例: >100, =\"east\", *abc*)", String::new()),
                    DialogField::text("ヘッダー行を含む (y / n)", "y"),
                ]));
                self.mode = Mode::Dialog;
            }
            Action::SheetNew => {
                let name = self.add_sheet(None);
                self.status_message = format!("新規シート: {}", name);
            }
            Action::SheetDelete => {
                let prev = self.sheet.name.clone();
                if self.delete_active_sheet() {
                    self.status_message = format!("シート削除: {} (現在のシート: {})", prev, self.sheet.name);
                } else {
                    self.status_message = "最後のシートは削除できません".to_string();
                }
            }
            Action::SheetRename => {
                self.dialog = Some(Dialog::single(
                    DialogKind::SheetRename,
                    format!("シート名変更 ({} -> ?)", self.sheet.name),
                    self.sheet.name.clone(),
                ));
                self.mode = Mode::Dialog;
            }
            Action::SheetNext => {
                let next = (self.active_sheet_index + 1) % self.sheet_count();
                self.switch_sheet(next);
                self.status_message = format!("シート: {}", self.sheet.name);
            }
            Action::SheetPrev => {
                let total = self.sheet_count();
                let prev = (self.active_sheet_index + total - 1) % total;
                self.switch_sheet(prev);
                self.status_message = format!("シート: {}", self.sheet.name);
            }
            Action::DataToDataframe => {
                if self.sheet.is_df_view() {
                    self.status_message = "既に DataFrame ビューです".into();
                } else {
                    match crate::df_view::cells_to_dataframe(&self.sheet) {
                        Ok(v) => {
                            let rows = v.rows();
                            let cols = v.cols();
                            let dtypes = v.dtype_summary(6);
                            self.sheet.df_view = Some(v);
                            self.cursor_col = 0;
                            self.cursor_row = 0;
                            self.selection_anchor = None;
                            self.adjust_view();
                            self.status_message = format!(
                                "DataFrame ビュー: {} 行 × {} 列 ({})",
                                rows, cols, dtypes
                            );
                        }
                        Err(e) => {
                            self.status_message = format!("DataFrame 変換エラー: {}", e);
                        }
                    }
                }
            }
            Action::DataToCells => {
                if !self.sheet.is_df_view() {
                    self.status_message = "既にセルビューです".into();
                } else if let Some(view) = self.sheet.df_view.clone() {
                    // The cell store was preserved underneath during the
                    // DataFrame view; just drop the view to reveal it.
                    self.sheet.df_view = None;
                    self.status_message = format!(
                        "セルビューに戻しました ({} 行 × {} 列のデータを保持)",
                        view.rows(), view.cols()
                    );
                }
            }
            Action::DataAddComputed => {
                if !self.sheet.is_df_view() {
                    self.status_message = "計算列は DataFrame ビューでのみ追加できます（データ → DataFrame ビューに変換）".into();
                } else {
                    self.dialog = Some(Dialog::multi(DialogKind::AddComputedColumn, vec![
                        DialogField::text("列名 (例: revenue)", String::new()),
                        DialogField::text("式 (例: price * qty)", String::new()),
                    ]));
                    self.mode = Mode::Dialog;
                }
            }
            Action::DataSqlQuery => {
                if !self.sheet.is_df_view() {
                    self.status_message = "SQL クエリは DataFrame ビューでのみ使えます".into();
                } else {
                    self.dialog = Some(Dialog::single(
                        DialogKind::SqlQuery,
                        "SQL クエリ (例: SELECT * FROM df WHERE price > 100)",
                        "SELECT * FROM df ".to_string(),
                    ));
                    self.mode = Mode::Dialog;
                }
            }
            Action::DataGroupBy => {
                if !self.sheet.is_df_view() {
                    self.status_message = "グループ集計は DataFrame ビューでのみ使えます".into();
                } else {
                    self.dialog = Some(Dialog::multi(DialogKind::GroupBy, vec![
                        DialogField::text("グループ列 (カンマ区切り、例: category, region)", String::new()),
                        DialogField::text("集計 (col:func、例: amount:sum, score:avg)", String::new()),
                    ]));
                    self.mode = Mode::Dialog;
                }
            }
            Action::DataFromUrl => {
                self.dialog = Some(Dialog::single(
                    DialogKind::FromUrl,
                    "URL (http(s)://… 内の <table> を取り込みます)",
                    "",
                ));
                self.mode = Mode::Dialog;
            }
            Action::DataFromSql => {
                self.dialog = Some(Dialog::multi(DialogKind::FromSql, vec![
                    DialogField::text("接続URI (postgresql:// / mysql:// / sqlite:/// …)", self.last_sql_uri.clone()),
                    DialogField::text("SQL クエリ", self.last_sql_query.clone()),
                    DialogField::text("取り込み先 (s=新規シート / o=上書き)", "s"),
                ]));
                self.mode = Mode::Dialog;
            }
            Action::DataClearComputed => {
                if !self.sheet.is_df_view() {
                    self.status_message = "DataFrame ビューではありません".into();
                } else if let Some(view) = self.sheet.df_view.as_ref() {
                    if view.computed.is_empty() {
                        self.status_message = "計算列はありません".into();
                    } else {
                        match crate::df_view::clear_computed_columns(&self.sheet) {
                            Ok(v) => {
                                let n = view.computed.len();
                                self.sheet.df_view = Some(v);
                                self.status_message = format!("計算列 {} 件をクリアしました", n);
                            }
                            Err(e) => {
                                self.status_message = format!("クリアエラー: {}", e);
                            }
                        }
                    }
                }
            }
            Action::DataFilterClear => {
                let n = self.hidden_rows.len();
                self.hidden_rows.clear();
                self.status_message = if n == 0 {
                    "フィルター解除済み".to_string()
                } else {
                    format!("フィルター解除: {} 行を再表示", n)
                };
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
            Action::FormatBoldToggle => {
                let (min_c, min_r, max_c, max_r) = self.get_selection_bounds();
                self.save_undo();
                // Toggle based on the anchor cell's current state.
                let anchor_bold = self.sheet.get_cell(min_c, min_r).bold;
                let new_bold = !anchor_bold;
                self.sheet.apply_format(min_c, min_r, max_c, max_r, |c| c.bold = new_bold);
                self.status_message = if new_bold { "太字 ON".into() } else { "太字 OFF".into() };
            }
            Action::FormatItalicToggle => {
                let (min_c, min_r, max_c, max_r) = self.get_selection_bounds();
                self.save_undo();
                let new_val = !self.sheet.get_cell(min_c, min_r).italic;
                self.sheet.apply_format(min_c, min_r, max_c, max_r, |c| c.italic = new_val);
                self.status_message = if new_val { "斜体 ON".into() } else { "斜体 OFF".into() };
            }
            Action::FormatUnderlineToggle => {
                let (min_c, min_r, max_c, max_r) = self.get_selection_bounds();
                self.save_undo();
                let new_val = !self.sheet.get_cell(min_c, min_r).underline;
                self.sheet.apply_format(min_c, min_r, max_c, max_r, |c| c.underline = new_val);
                self.status_message = if new_val { "下線 ON".into() } else { "下線 OFF".into() };
            }
            Action::FormatStyleReset => {
                let (min_c, min_r, max_c, max_r) = self.get_selection_bounds();
                self.save_undo();
                self.sheet.apply_format(min_c, min_r, max_c, max_r, |c| {
                    c.bold = false;
                    c.italic = false;
                    c.underline = false;
                });
                self.status_message = "太字/斜体/下線を解除しました".into();
            }
            Action::FormatTextColorPick(color) => {
                let (min_c, min_r, max_c, max_r) = self.get_selection_bounds();
                self.save_undo();
                self.sheet.apply_format(min_c, min_r, max_c, max_r, |c| c.text_color = color);
                self.status_message = match color {
                    Some(rgb) => format!("文字色: {:?}", rgb),
                    None => "文字色をクリア".into(),
                };
            }
            Action::FormatBgColorPick(color) => {
                let (min_c, min_r, max_c, max_r) = self.get_selection_bounds();
                self.save_undo();
                self.sheet.apply_format(min_c, min_r, max_c, max_r, |c| c.bg_color = color);
                self.status_message = match color {
                    Some(rgb) => format!("背景色: {:?}", rgb),
                    None => "背景色をクリア".into(),
                };
            }
            Action::FormatAlignLeft | Action::FormatAlignCenter
            | Action::FormatAlignRight | Action::FormatAlignDefault => {
                let align = match action {
                    Action::FormatAlignLeft => crate::cell::Alignment::Left,
                    Action::FormatAlignCenter => crate::cell::Alignment::Center,
                    Action::FormatAlignRight => crate::cell::Alignment::Right,
                    _ => crate::cell::Alignment::Default,
                };
                let (min_c, min_r, max_c, max_r) = self.get_selection_bounds();
                self.save_undo();
                self.sheet.apply_format(min_c, min_r, max_c, max_r, |c| c.alignment = align);
                self.status_message = format!("揃え: {:?}", align);
            }
            Action::FormatTextColor => {
                let (min_c, min_r, _, _) = self.get_selection_bounds();
                let cur = self.sheet.get_cell(min_c, min_r).text_color;
                self.dialog = Some(Dialog::multi(DialogKind::TextColor, vec![
                    DialogField::palette("色", &TEXT_COLOR_PALETTE, cur),
                    DialogField::text("RGB 直接指定 (例: 255,255,255 / #fff、入力時はパレットより優先)", String::new()),
                ]));
                self.mode = Mode::Dialog;
            }
            Action::FormatBgColor => {
                let (min_c, min_r, _, _) = self.get_selection_bounds();
                let cur = self.sheet.get_cell(min_c, min_r).bg_color;
                self.dialog = Some(Dialog::multi(DialogKind::BgColor, vec![
                    DialogField::palette("色", &BG_COLOR_PALETTE, cur),
                    DialogField::text("RGB 直接指定 (例: 255,235,150 / #fee、入力時はパレットより優先)", String::new()),
                ]));
                self.mode = Mode::Dialog;
            }
            Action::FormatNumber => {
                let (min_c, min_r, _, _) = self.get_selection_bounds();
                let (kind_idx, dec) = format_to_choice(&self.sheet.get_cell(min_c, min_r).format);
                self.dialog = Some(Dialog::multi(DialogKind::NumberFormat, vec![
                    DialogField::choice("種別", &FORMAT_KIND_OPTIONS, kind_idx),
                    DialogField::choice("小数桁数", &DECIMALS_OPTIONS, dec),
                ]));
                self.mode = Mode::Dialog;
            }
            Action::FormatSheetDefault => {
                let (kind_idx, dec) = format_to_choice(&self.sheet.default_format);
                self.dialog = Some(Dialog::multi(DialogKind::SheetDefaultFormat, vec![
                    DialogField::choice("種別", &FORMAT_KIND_OPTIONS, kind_idx),
                    DialogField::choice("小数桁数", &DECIMALS_OPTIONS, dec),
                ]));
                self.mode = Mode::Dialog;
            }
            Action::FormatNegStyle(parens, red) => {
                let (min_c, min_r, max_c, max_r) = self.get_selection_bounds();
                self.save_undo();
                self.sheet.apply_format(min_c, min_r, max_c, max_r, |c| {
                    c.neg_parens = parens;
                    c.neg_red = red;
                });
                let idx = neg_to_choice(parens, red);
                self.status_message = format!("負数の表示: {}", NEG_OPTIONS[idx]);
            }
            Action::FormatCellDialog => {
                // Pre-fill every field from the selection's anchor cell so
                // committing an untouched dialog is a no-op for uniform ranges.
                let (min_c, min_r, _, _) = self.get_selection_bounds();
                let cell = self.sheet.get_cell(min_c, min_r).clone();
                let (kind_idx, dec) = format_to_choice(&cell.format);
                let align_idx = match cell.alignment {
                    crate::cell::Alignment::Default => 0,
                    crate::cell::Alignment::Left => 1,
                    crate::cell::Alignment::Center => 2,
                    crate::cell::Alignment::Right => 3,
                };
                self.dialog = Some(Dialog::multi(DialogKind::CellFormat, vec![
                    DialogField::choice("種別", &FORMAT_KIND_OPTIONS, kind_idx),
                    DialogField::choice("小数桁数", &DECIMALS_OPTIONS, dec),
                    DialogField::choice("負数", &NEG_OPTIONS, neg_to_choice(cell.neg_parens, cell.neg_red)),
                    DialogField::choice("揃え", &ALIGN_OPTIONS, align_idx),
                    DialogField::choice("太字", &BOLD_OPTIONS, if cell.bold { 1 } else { 0 }),
                    DialogField::choice("斜体", &BOLD_OPTIONS, if cell.italic { 1 } else { 0 }),
                    DialogField::choice("下線", &BOLD_OPTIONS, if cell.underline { 1 } else { 0 }),
                    DialogField::palette("文字色", &TEXT_COLOR_PALETTE, cell.text_color),
                    DialogField::palette("背景色", &BG_COLOR_PALETTE, cell.bg_color),
                ]));
                self.mode = Mode::Dialog;
            }
            Action::FormatClear => {
                let (min_c, min_r, max_c, max_r) = self.get_selection_bounds();
                self.save_undo();
                self.sheet.apply_format(min_c, min_r, max_c, max_r, |c| {
                    c.alignment = crate::cell::Alignment::Default;
                    c.bold = false;
                    c.italic = false;
                    c.underline = false;
                    c.neg_parens = false;
                    c.neg_red = false;
                    c.text_color = None;
                    c.bg_color = None;
                    c.format = crate::cell::DisplayFormat::General;
                });
                self.status_message = "書式をクリアしました".into();
            }
            Action::FormatConditional => {
                let (min_c, min_r, max_c, max_r) = self.get_selection_bounds();
                let range = format!(
                    "{}:{}",
                    crate::formula::cell_name(min_c, min_r),
                    crate::formula::cell_name(max_c, max_r),
                );
                self.dialog = Some(Dialog::multi(DialogKind::ConditionalAdd, vec![
                    DialogField::text("対象範囲 (例: A1:B10)", range),
                    DialogField::text("条件 (例: >100, <=0, =\"NG\", scale:0-100)", ">0"),
                    DialogField::text("背景色 RGB (例: 255,200,200 または #fee)", "255,200,200"),
                ]));
                self.mode = Mode::Dialog;
            }
            Action::FormatConditionalClear => {
                let n = self.sheet.conditional_formats.len();
                if n > 0 { self.save_undo(); }
                self.sheet.conditional_formats.clear();
                self.status_message = format!("条件付き書式 {} 件を削除", n);
            }
            Action::FormatSetWidth => {
                let cur = self.sheet.get_col_width(self.cursor_col);
                let col_name = crate::formula::col_to_name(self.cursor_col);
                self.dialog = Some(Dialog::single(
                    DialogKind::SetColWidth,
                    format!("列 {} の幅 (3-50)", col_name),
                    cur.to_string(),
                ));
                self.mode = Mode::Dialog;
            }
            Action::HelpKeys => {
                self.status_message = "矢印=移動 / F2=編集 / Ctrl+C/X/V=コピー切取貼付 / Ctrl+Z=戻 / Ctrl+S=保存 / メニュー=「/」か F10 / 書式=「:」 / F4=$切替 / F5=ジャンプ / F9=再計算".to_string();
            }
            Action::HelpAbout => {
                self.status_message = format!("tbla {} - ターミナル表計算エディタ", env!("CARGO_PKG_VERSION"));
            }
        }
    }

    /// Execute a dialog action with the current input.
    pub fn commit_dialog(&mut self) {
        let Some(dialog) = self.dialog.clone() else { return; };
        let input = dialog.primary_input().trim().to_string();

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
                    self.hidden_rows.clear();
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
                } else if let Some(nr) = self.find_named_range(&input).cloned() {
                    if nr.sheet != self.sheet.name {
                        if let Some(idx) = self
                            .workbook_sheets()
                            .iter()
                            .position(|s| s.name == nr.sheet)
                        {
                            self.switch_sheet(idx);
                        }
                    }
                    self.cursor_col = nr.start.0;
                    self.cursor_row = nr.start.1;
                    self.selection_anchor = if nr.start != nr.end {
                        Some((nr.end.0, nr.end.1))
                    } else {
                        None
                    };
                    self.adjust_view();
                    self.status_message = format!("{} ({}) に移動", nr.name, nr.sheet);
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
            DialogKind::Sort => {
                let col_in = dialog.fields.get(0).map(|f| f.input.trim().to_string()).unwrap_or_default();
                let dir_in = dialog.fields.get(1).map(|f| f.input.trim().to_lowercase()).unwrap_or_else(|| "asc".into());
                let hdr_in = dialog.fields.get(2).map(|f| f.input.trim().to_lowercase()).unwrap_or_else(|| "y".into());
                let col = match crate::formula::col_from_name(&col_in) {
                    Some(c) => c,
                    None => {
                        self.status_message = format!("無効な列名: {}", col_in);
                        self.dialog = None;
                        self.mode = Mode::Normal;
                        return;
                    }
                };
                let descending = dir_in.starts_with('d');
                let header = matches!(hdr_in.as_str(), "y" | "yes" | "true" | "t" | "1");
                let n = commands::sort_rows(self, col, descending, header);
                self.status_message = format!(
                    "列 {} で{}並べ替え: {} 行を並べ替え{}",
                    crate::formula::col_to_name(col),
                    if descending { "降順" } else { "昇順" },
                    n,
                    if header { "（先頭行はヘッダーとして固定）" } else { "" },
                );
            }
            DialogKind::Filter => {
                let col_in = dialog.fields.get(0).map(|f| f.input.trim().to_string()).unwrap_or_default();
                let crit = dialog.fields.get(1).map(|f| f.input.trim().to_string()).unwrap_or_default();
                let hdr_in = dialog.fields.get(2).map(|f| f.input.trim().to_lowercase()).unwrap_or_else(|| "y".into());
                let col = match crate::formula::col_from_name(&col_in) {
                    Some(c) => c,
                    None => {
                        self.status_message = format!("無効な列名: {}", col_in);
                        self.dialog = None;
                        self.mode = Mode::Normal;
                        return;
                    }
                };
                let header = matches!(hdr_in.as_str(), "y" | "yes" | "true" | "t" | "1");
                let hidden = commands::apply_filter(self, col, &crit, header);
                self.status_message = format!(
                    "列 {} でフィルター: {} 行を非表示",
                    crate::formula::col_to_name(col),
                    hidden
                );
            }
            DialogKind::TextColor | DialogKind::BgColor => {
                // A typed RGB value (field 1) wins over the palette (field 0).
                let rgb_in = dialog.fields.get(1).map(|f| f.input.trim().to_string()).unwrap_or_default();
                let parsed: Option<Option<crate::cell::RgbColor>> = if !rgb_in.is_empty() {
                    parse_rgb_input(&rgb_in).map(Some)
                } else {
                    Some(dialog.fields.first()
                        .and_then(|f| f.swatches.as_ref().and_then(|sw| sw.get(f.selected)).copied())
                        .flatten())
                };
                let is_text = dialog.kind == DialogKind::TextColor;
                let name = if is_text { "文字色" } else { "背景色" };
                if let Some(color) = parsed {
                    let (min_c, min_r, max_c, max_r) = self.get_selection_bounds();
                    self.save_undo();
                    self.sheet.apply_format(min_c, min_r, max_c, max_r, |c| {
                        if is_text { c.text_color = color; } else { c.bg_color = color; }
                    });
                    self.status_message = match color {
                        Some(rgb) => format!("{}: {:?}", name, rgb),
                        None => format!("{}をクリア", name),
                    };
                } else {
                    self.status_message = "RGB の指定が無効です（例: 255,200,200 または #fee）".into();
                }
            }
            DialogKind::NumberFormat => {
                let kind_idx = dialog.fields.first().map(|f| f.selected).unwrap_or(0);
                let dec = dialog.fields.get(1).map(|f| f.selected).unwrap_or(2);
                let fmt = format_from_choice(kind_idx, dec);
                let (min_c, min_r, max_c, max_r) = self.get_selection_bounds();
                self.save_undo();
                self.sheet.apply_format(min_c, min_r, max_c, max_r, |c| c.format = fmt.clone());
                self.status_message = format!("書式: {:?}", fmt);
            }
            DialogKind::SheetDefaultFormat => {
                let kind_idx = dialog.fields.first().map(|f| f.selected).unwrap_or(0);
                let dec = dialog.fields.get(1).map(|f| f.selected).unwrap_or(2);
                let fmt = format_from_choice(kind_idx, dec);
                self.save_undo();
                self.sheet.default_format = fmt.clone();
                self.status_message = format!(
                    "シート {} の既定書式: {:?}（標準のセルに適用）",
                    self.sheet.name, fmt
                );
            }
            DialogKind::CellFormat => {
                let get = |i: usize| dialog.fields.get(i).map(|f| f.selected).unwrap_or(0);
                let fmt = format_from_choice(get(0), get(1));
                let (neg_parens, neg_red) = neg_from_choice(get(2));
                let align = match get(3) {
                    1 => crate::cell::Alignment::Left,
                    2 => crate::cell::Alignment::Center,
                    3 => crate::cell::Alignment::Right,
                    _ => crate::cell::Alignment::Default,
                };
                let bold = get(4) == 1;
                let italic = get(5) == 1;
                let underline = get(6) == 1;
                let color_of = |i: usize| -> Option<crate::cell::RgbColor> {
                    dialog.fields.get(i)
                        .and_then(|f| f.swatches.as_ref().and_then(|sw| sw.get(f.selected)).copied())
                        .flatten()
                };
                let (text_color, bg_color) = (color_of(7), color_of(8));
                let (min_c, min_r, max_c, max_r) = self.get_selection_bounds();
                self.save_undo();
                self.sheet.apply_format(min_c, min_r, max_c, max_r, |c| {
                    c.format = fmt.clone();
                    c.neg_parens = neg_parens;
                    c.neg_red = neg_red;
                    c.alignment = align;
                    c.bold = bold;
                    c.italic = italic;
                    c.underline = underline;
                    c.text_color = text_color;
                    c.bg_color = bg_color;
                });
                self.status_message = "書式を適用しました".into();
            }
            DialogKind::ConditionalAdd => {
                let range_in = dialog.fields.get(0).map(|f| f.input.trim().to_string()).unwrap_or_default();
                let cond_in = dialog.fields.get(1).map(|f| f.input.trim().to_string()).unwrap_or_default();
                let color_in = dialog.fields.get(2).map(|f| f.input.trim().to_string()).unwrap_or_default();
                match parse_conditional_format(&range_in, &cond_in, &color_in) {
                    Ok(cf) => {
                        self.save_undo();
                        self.sheet.conditional_formats.push(cf);
                        self.status_message = format!("条件付き書式を追加 ({})", range_in);
                    }
                    Err(e) => {
                        self.status_message = format!("条件付き書式エラー: {}", e);
                    }
                }
            }
            DialogKind::OpenCsvAsDf => {
                let input = commands::sanitize_path_input(&input);
                if input.is_empty() {
                    self.status_message = "ファイル名が空です".into();
                } else {
                    match crate::df_io::read_csv_as_dataframe(&input) {
                        Ok(view) => {
                            self.save_undo();
                            let stem = std::path::Path::new(&input)
                                .file_stem().and_then(|n| n.to_str())
                                .unwrap_or("data").to_string();
                            let mut s = crate::sheet::Sheet::new();
                            s.name = stem;
                            let rows = view.rows();
                            let cols = view.cols();
                            s.df_view = Some(view);
                            self.sheet = s;
                            self.other_sheets = Vec::new();
                            self.active_sheet_index = 0;
                            self.cursor_col = 0; self.cursor_row = 0;
                            self.view_col = 0; self.view_row = 0;
                            self.selection_anchor = None;
                            self.hidden_rows.clear();
                            self.current_file = Some(input.clone());
                            self.status_message = format!(
                                "CSV を DataFrame として読込: {} 行 × {} 列", rows, cols
                            );
                        }
                        Err(e) => {
                            self.status_message = format!("CSV 読込エラー: {}", e);
                        }
                    }
                }
            }
            DialogKind::SaveParquet => {
                let input = commands::sanitize_path_input(&input);
                if input.is_empty() {
                    self.status_message = "ファイル名が空です".into();
                } else {
                    let view = if let Some(v) = self.sheet.df_view.clone() {
                        Ok(v)
                    } else {
                        crate::df_view::cells_to_dataframe(&self.sheet)
                    };
                    match view {
                        Ok(v) => {
                            match crate::df_io::write_parquet(&v, &input) {
                                Ok(()) => self.status_message = format!(
                                    "{} に Parquet 保存: {} 行 × {} 列", input, v.rows(), v.cols()
                                ),
                                Err(e) => self.status_message = format!("Parquet 保存エラー: {}", e),
                            }
                        }
                        Err(e) => self.status_message = format!("DataFrame 変換に失敗: {}", e),
                    }
                }
            }
            DialogKind::SqlQuery => {
                if self.sheet.df_view.is_none() {
                    self.status_message = "DataFrame ビューではありません".into();
                } else {
                    self.save_undo();
                    let view = self.sheet.df_view.as_mut().unwrap();
                    match crate::df_view::run_sql(view, &input) {
                        Ok(msg) => self.status_message = msg,
                        Err(e) => self.status_message = e,
                    }
                    self.cursor_col = 0; self.cursor_row = 0;
                    self.view_col = 0; self.view_row = 0;
                    self.selection_anchor = None;
                }
            }
            DialogKind::FromUrl => {
                let url = input.clone();
                if url.is_empty() {
                    // nothing to do
                } else {
                    self.status_message = format!("URL から取得中: {}", url);
                    match url_import::fetch_url(&url) {
                        Ok(html) => {
                            let tables = url_import::extract_tables(&html);
                            if tables.is_empty() {
                                self.status_message =
                                    "ページに <table> が見つかりませんでした".into();
                            } else {
                                // Build a multi-line preview of the tables.
                                let mut preview_lines = Vec::new();
                                for (i, t) in tables.iter().enumerate().take(20) {
                                    let cap = t.caption.as_deref().unwrap_or("");
                                    let cap_part = if cap.is_empty() {
                                        String::new()
                                    } else {
                                        format!(" — {}", cap)
                                    };
                                    preview_lines.push(format!(
                                        "{}: {}×{}{} [{}]",
                                        i + 1,
                                        t.row_count(),
                                        t.col_count(),
                                        cap_part,
                                        t.preview(),
                                    ));
                                }
                                let extra = if tables.len() > 20 {
                                    format!(" (他 {} 件は省略)", tables.len() - 20)
                                } else {
                                    String::new()
                                };
                                self.status_message =
                                    format!("{} 件のテーブルを検出{}", tables.len(), extra);
                                self.pending_url_tables = tables;
                                self.pending_url_source = url;
                                self.dialog = Some(Dialog::multi(
                                    DialogKind::FromUrlPickTable,
                                    vec![
                                        DialogField::text(format!(
                                                "テーブル番号 (1-{}) — {}",
                                                self.pending_url_tables.len(),
                                                preview_lines.join(" / "),
                                            ), "1"),
                                        DialogField::text("取り込み先 (s=新規シート / o=上書き)", "s"),
                                    ],
                                ));
                                self.mode = Mode::Dialog;
                                // Keep dialog open — early-return out of commit
                                // so the bottom of this function doesn't close it.
                                return;
                            }
                        }
                        Err(e) => {
                            self.status_message = format!("URL 取得エラー: {}", e);
                        }
                    }
                }
            }
            DialogKind::FromUrlPickTable => {
                let idx_str = dialog.fields.get(0)
                    .map(|f| f.input.trim().to_string()).unwrap_or_default();
                let dest = dialog.fields.get(1)
                    .map(|f| f.input.trim().to_lowercase()).unwrap_or_default();
                let total = self.pending_url_tables.len();
                let idx = idx_str.parse::<usize>().ok()
                    .filter(|n| *n >= 1 && *n <= total);
                match idx {
                    None => {
                        self.status_message = format!(
                            "テーブル番号は 1 〜 {} の整数で指定してください",
                            total
                        );
                    }
                    Some(n) => {
                        let table = self.pending_url_tables[n - 1].clone();
                        let sheet_name = table
                            .caption
                            .clone()
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| {
                                derive_sheet_name_from_url(&self.pending_url_source, n)
                            });
                        let overwrite = dest.starts_with('o');
                        self.save_undo();
                        if overwrite {
                            let mut new_sheet = crate::sheet::Sheet::new();
                            new_sheet.name = sheet_name.clone();
                            populate_sheet_from_table(&mut new_sheet, &table);
                            self.sheet = new_sheet;
                            self.cursor_col = 0;
                            self.cursor_row = 0;
                            self.view_col = 0;
                            self.view_row = 0;
                            self.selection_anchor = None;
                            self.status_message = format!(
                                "テーブル {} を読み込みました ({} 行 × {} 列, 上書き)",
                                n, table.row_count(), table.col_count(),
                            );
                        } else {
                            let actual_name = self.add_sheet(Some(sheet_name));
                            populate_sheet_from_table(&mut self.sheet, &table);
                            self.status_message = format!(
                                "テーブル {} を新規シート \"{}\" に読み込みました ({} 行 × {} 列)",
                                n, actual_name, table.row_count(), table.col_count(),
                            );
                        }
                        self.pending_url_tables.clear();
                        self.pending_url_source.clear();
                    }
                }
            }
            DialogKind::FromSql => {
                let uri = dialog.fields.get(0)
                    .map(|f| f.input.trim().to_string()).unwrap_or_default();
                let query = dialog.fields.get(1)
                    .map(|f| f.input.trim().to_string()).unwrap_or_default();
                let dest = dialog.fields.get(2)
                    .map(|f| f.input.trim().to_lowercase()).unwrap_or_default();
                if uri.is_empty() || query.is_empty() {
                    self.status_message = "接続URI と SQL クエリの両方を入力してください".into();
                } else {
                    self.status_message = "クエリ実行中…".into();
                    match sql_import::run_query(&uri, &query) {
                        Ok(result) => {
                            // Remember inputs for next time.
                            self.last_sql_uri = uri.clone();
                            self.last_sql_query = query.clone();
                            let sheet_name = derive_sheet_name_from_sql_uri(&uri);
                            let overwrite = dest.starts_with('o');
                            self.save_undo();
                            if overwrite {
                                let mut new_sheet = crate::sheet::Sheet::new();
                                new_sheet.name = sheet_name.clone();
                                populate_sheet_from_query_result(&mut new_sheet, &result);
                                self.sheet = new_sheet;
                                self.cursor_col = 0;
                                self.cursor_row = 0;
                                self.view_col = 0;
                                self.view_row = 0;
                                self.selection_anchor = None;
                                self.status_message = format!(
                                    "SQL 結果を読み込みました ({} 行 × {} 列, 上書き)",
                                    result.row_count(), result.col_count(),
                                );
                            } else {
                                let actual_name = self.add_sheet(Some(sheet_name));
                                populate_sheet_from_query_result(&mut self.sheet, &result);
                                self.status_message = format!(
                                    "SQL 結果を新規シート \"{}\" に読み込みました ({} 行 × {} 列)",
                                    actual_name, result.row_count(), result.col_count(),
                                );
                            }
                        }
                        Err(e) => {
                            self.status_message = format!("SQL エラー: {}", e);
                        }
                    }
                }
            }
            DialogKind::GroupBy => {
                let groups = dialog.fields.get(0).map(|f| f.input.clone()).unwrap_or_default();
                let aggs = dialog.fields.get(1).map(|f| f.input.clone()).unwrap_or_default();
                if self.sheet.df_view.is_none() {
                    self.status_message = "DataFrame ビューではありません".into();
                } else {
                    self.save_undo();
                    let view = self.sheet.df_view.as_mut().unwrap();
                    match crate::df_view::run_group_by(view, &groups, &aggs) {
                        Ok(msg) => self.status_message = msg,
                        Err(e) => self.status_message = e,
                    }
                    self.cursor_col = 0; self.cursor_row = 0;
                    self.view_col = 0; self.view_row = 0;
                    self.selection_anchor = None;
                }
            }
            DialogKind::AddComputedColumn => {
                let name = dialog.fields.get(0).map(|f| f.input.clone()).unwrap_or_default();
                let expr = dialog.fields.get(1).map(|f| f.input.clone()).unwrap_or_default();
                if let Some(view) = self.sheet.df_view.as_mut() {
                    match crate::df_view::add_computed_column(view, &name, &expr) {
                        Ok(msg) => self.status_message = msg,
                        Err(e) => self.status_message = e,
                    }
                } else {
                    self.status_message = "DataFrame ビューではありません".to_string();
                }
            }
            DialogKind::SheetRename => {
                if input.is_empty() {
                    self.status_message = "シート名を入力してください".to_string();
                } else if self.rename_active_sheet(&input) {
                    self.status_message = format!("シート名を {} に変更", input);
                } else {
                    self.status_message = format!("{} は既に使われています", input);
                }
            }
            DialogKind::NameDefine => {
                let name = dialog.fields.get(0)
                    .map(|f| f.input.trim().to_string()).unwrap_or_default();
                let range = dialog.fields.get(1)
                    .map(|f| f.input.trim().to_string()).unwrap_or_default();
                match self.define_named_range(&name, &range) {
                    Ok(normalized) => {
                        self.status_message =
                            format!("名前 {} = {} を定義しました", name, normalized);
                    }
                    Err(e) => {
                        self.status_message = e;
                    }
                }
            }
            DialogKind::NameManage => {
                if input.is_empty() {
                    self.status_message =
                        format!("定義済みの名前: {}", self.named_range_summary());
                } else if self.delete_named_range(&input) {
                    self.status_message = format!("名前 {} を削除しました", input);
                } else {
                    self.status_message = format!("名前 {} は定義されていません", input);
                }
            }
            DialogKind::Replace => {
                // Replace cares about exact strings (incl. whitespace), so
                // we read the raw field inputs rather than the trimmed primary.
                let find = dialog.fields.get(0).map(|f| f.input.clone()).unwrap_or_default();
                let replace = dialog.fields.get(1).map(|f| f.input.clone()).unwrap_or_default();
                if find.is_empty() {
                    self.status_message = "検索文字列を入力してください".to_string();
                } else {
                    self.last_search = find.clone();
                    self.last_replace = replace.clone();
                    let count = commands::replace_all(self, &find, &replace);
                    self.status_message = if count == 0 {
                        format!("該当なし: {:?}", find)
                    } else {
                        format!("{} 件置換しました", count)
                    };
                }
            }
        }

        self.dialog = None;
        self.mode = Mode::Normal;
    }
}

/// Parse an RGB color string in any of these forms:
/// - `255,128,64` (comma-separated decimals, each 0-255)
/// - `#rrggbb` (6-hex with optional leading `#`)
/// - `#rgb` (3-hex shorthand, each digit doubled)
fn parse_rgb_input(s: &str) -> Option<crate::cell::RgbColor> {
    let s = s.trim();
    if s.is_empty() { return None; }
    // Hex form.
    let hex = s.trim_start_matches('#');
    if hex.chars().all(|c| c.is_ascii_hexdigit()) {
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some((r, g, b));
        }
        if hex.len() == 3 {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 0x11;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 0x11;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 0x11;
            return Some((r, g, b));
        }
    }
    // Decimal "r,g,b" form.
    let parts: Vec<&str> = s.split(',').map(|p| p.trim()).collect();
    if parts.len() == 3 {
        let r: u8 = parts[0].parse().ok()?;
        let g: u8 = parts[1].parse().ok()?;
        let b: u8 = parts[2].parse().ok()?;
        return Some((r, g, b));
    }
    None
}

/// Choice labels for `DisplayFormat` kinds, shared by the 数値書式 and
/// セルの書式設定 dialogs. Order must match `format_from_choice`.
const FORMAT_KIND_OPTIONS: [&str; 10] =
    ["標準", "数値", "カンマ", "通貨", "%", "指数", "日付", "日時", "時刻", "文字列"];

/// Negative-number display: (label, neg_parens, neg_red).
const NEG_OPTIONS: [&str; 4] = ["標準", "赤", "(括弧)", "(括弧)赤"];

fn neg_from_choice(idx: usize) -> (bool, bool) {
    match idx {
        1 => (false, true),
        2 => (true, false),
        3 => (true, true),
        _ => (false, false),
    }
}

fn neg_to_choice(parens: bool, red: bool) -> usize {
    match (parens, red) {
        (false, false) => 0,
        (false, true) => 1,
        (true, false) => 2,
        (true, true) => 3,
    }
}

/// Choice labels for decimal places 0-10 (index == value).
const DECIMALS_OPTIONS: [&str; 11] =
    ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10"];

const ALIGN_OPTIONS: [&str; 4] = ["自動", "左揃え", "中央揃え", "右揃え"];
const BOLD_OPTIONS: [&str; 2] = ["OFF", "ON"];

/// Color palettes for the セルの書式設定 dialog. Strong tones for text,
/// pale tones for backgrounds. Custom RGB values are still available via
/// the standalone 文字色... / 背景色... dialogs.
const TEXT_COLOR_PALETTE: [(&str, Option<crate::cell::RgbColor>); 9] = [
    ("なし", None),
    ("黒", Some((0, 0, 0))),
    ("白", Some((255, 255, 255))),
    ("赤", Some((200, 30, 30))),
    ("青", Some((40, 80, 220))),
    ("緑", Some((0, 140, 0))),
    ("橙", Some((255, 136, 0))),
    ("紫", Some((140, 60, 180))),
    ("灰", Some((130, 130, 130))),
];
const BG_COLOR_PALETTE: [(&str, Option<crate::cell::RgbColor>); 9] = [
    ("なし", None),
    ("白", Some((255, 255, 255))),
    ("赤", Some((255, 200, 200))),
    ("黄", Some((255, 240, 170))),
    ("緑", Some((205, 240, 205))),
    ("青", Some((205, 225, 255))),
    ("桃", Some((255, 215, 235))),
    ("水", Some((210, 240, 250))),
    ("灰", Some((225, 225, 225))),
];

/// Build the WYSIWYG (":") popup menu tree — tbla's take on Lotus 1-2-3 /
/// l123's `:Format` WYSIWYG menu: fast keyboard-driven formatting of the
/// current selection, with the color palettes applied directly (no dialog).
fn wysiwyg_menu_items() -> Vec<PopupItem> {
    let color_sub = |palette: &[(&str, Option<crate::cell::RgbColor>)], text: bool| -> Vec<PopupItem> {
        palette.iter().enumerate().map(|(i, (name, color))| {
            let mnemonic = char::from_digit(i as u32, 10).unwrap_or(' ');
            let action = if text {
                Action::FormatTextColorPick(*color)
            } else {
                Action::FormatBgColorPick(*color)
            };
            PopupItem::color(format!("{} {}", mnemonic, name), mnemonic, *color, action)
        }).collect()
    };
    vec![
        PopupItem::submenu("書式", 'F', vec![
            PopupItem::action("太字 切替", 'B', Action::FormatBoldToggle),
            PopupItem::action("斜体 切替", 'I', Action::FormatItalicToggle),
            PopupItem::action("下線 切替", 'U', Action::FormatUnderlineToggle),
            PopupItem::submenu("文字色", 'T', color_sub(&TEXT_COLOR_PALETTE, true)),
            PopupItem::submenu("背景色", 'G', color_sub(&BG_COLOR_PALETTE, false)),
            PopupItem::submenu("揃え", 'A', vec![
                PopupItem::action("左揃え", 'L', Action::FormatAlignLeft),
                PopupItem::action("中央揃え", 'C', Action::FormatAlignCenter),
                PopupItem::action("右揃え", 'R', Action::FormatAlignRight),
                PopupItem::action("既定に戻す", 'D', Action::FormatAlignDefault),
            ]),
            PopupItem::submenu("負数の表示", 'N', vec![
                PopupItem::action("標準 (-123)", 'S', Action::FormatNegStyle(false, false)),
                PopupItem::action("赤", 'R', Action::FormatNegStyle(false, true)),
                PopupItem::action("括弧 (123)", 'P', Action::FormatNegStyle(true, false)),
                PopupItem::action("括弧+赤", 'B', Action::FormatNegStyle(true, true)),
            ]),
            PopupItem::action("スタイル解除 (太字/斜体/下線)", 'R', Action::FormatStyleReset),
        ]),
        PopupItem::submenu("列幅", 'C', vec![
            PopupItem::action("自動調整", 'A', Action::FormatAutoWidth),
            PopupItem::action("広げる", 'W', Action::FormatWiderCol),
            PopupItem::action("狭める", 'N', Action::FormatNarrowerCol),
            PopupItem::action("変更...", 'S', Action::FormatSetWidth),
        ]),
        PopupItem::action("セルの書式設定...", 'E', Action::FormatCellDialog),
        PopupItem::action("数値書式...", 'N', Action::FormatNumber),
        PopupItem::action("書式クリア", 'X', Action::FormatClear),
    ]
}

fn format_from_choice(kind: usize, dec: usize) -> crate::cell::DisplayFormat {
    use crate::cell::DisplayFormat as F;
    let dec = dec.min(10);
    match kind {
        1 => F::Number(dec),
        2 => F::Comma(dec),
        3 => F::Currency(dec),
        4 => F::Percent(dec),
        5 => F::Scientific,
        6 => F::Date,
        7 => F::DateTime,
        8 => F::Time,
        9 => F::Text,
        _ => F::General,
    }
}

/// Inverse of `format_from_choice`: (kind index, decimal places) for
/// pre-selecting the dialog from a cell's current format. Kinds without
/// decimals report 2 so switching to 数値/通貨/パーセント starts sensibly.
fn format_to_choice(fmt: &crate::cell::DisplayFormat) -> (usize, usize) {
    use crate::cell::DisplayFormat as F;
    match fmt {
        F::General => (0, 2),
        F::Number(d) => (1, (*d).min(10)),
        F::Comma(d) => (2, (*d).min(10)),
        F::Currency(d) => (3, (*d).min(10)),
        F::Percent(d) => (4, (*d).min(10)),
        F::Scientific => (5, 2),
        F::Date => (6, 2),
        F::DateTime => (7, 2),
        F::Time => (8, 2),
        F::Text => (9, 2),
    }
}

/// Parse a conditional-formatting rule from the dialog inputs. Accepted
/// condition forms:
/// - Comparison: `>100`, `<-5`, `>=0`, `<=100`, `=42`, `<>0`
/// - Color scale: `scale:0-100`, `scale:0..100,blue,red`
fn parse_conditional_format(range_in: &str, cond_in: &str, color_in: &str)
    -> std::result::Result<crate::sheet::ConditionalFormat, String>
{
    // Range A1:B10 or single cell A1
    let (min_col, min_row, max_col, max_row) = if let Some((a, b)) = range_in.split_once(':') {
        let (c1, r1, _, _) = crate::formula::parse_cell_ref(a.trim())
            .ok_or_else(|| format!("無効な範囲: {}", range_in))?;
        let (c2, r2, _, _) = crate::formula::parse_cell_ref(b.trim())
            .ok_or_else(|| format!("無効な範囲: {}", range_in))?;
        (c1.min(c2), r1.min(r2), c1.max(c2), r1.max(r2))
    } else {
        let (c, r, _, _) = crate::formula::parse_cell_ref(range_in.trim())
            .ok_or_else(|| format!("無効な範囲: {}", range_in))?;
        (c, r, c, r)
    };

    let cond_trim = cond_in.trim();
    let condition = if let Some(rest) = cond_trim.strip_prefix("scale:") {
        // scale:min-max[,min_color,max_color]
        let mut parts = rest.split(',');
        let range_part = parts.next().ok_or("scale: にレンジが必要")?;
        let (min_s, max_s) = range_part.split_once('-').or_else(|| range_part.split_once(".."))
            .ok_or("scale: は min-max 形式")?;
        let min: f64 = min_s.trim().parse().map_err(|_| "scale min が数値ではありません")?;
        let max: f64 = max_s.trim().parse().map_err(|_| "scale max が数値ではありません")?;
        let mc1 = parts.next().and_then(|s| parse_rgb_input(s.trim()))
            .unwrap_or((255, 245, 235));
        let mc2 = parts.next().and_then(|s| parse_rgb_input(s.trim()))
            .unwrap_or((220, 60, 60));
        crate::sheet::CondCondition::ColorScale { min, max, min_color: mc1, max_color: mc2 }
    } else {
        let (op, num_str) = if let Some(rest) = cond_trim.strip_prefix(">=") { (crate::sheet::CondOp::Ge, rest) }
            else if let Some(rest) = cond_trim.strip_prefix("<=") { (crate::sheet::CondOp::Le, rest) }
            else if let Some(rest) = cond_trim.strip_prefix("<>") { (crate::sheet::CondOp::Ne, rest) }
            else if let Some(rest) = cond_trim.strip_prefix("!=") { (crate::sheet::CondOp::Ne, rest) }
            else if let Some(rest) = cond_trim.strip_prefix('>') { (crate::sheet::CondOp::Gt, rest) }
            else if let Some(rest) = cond_trim.strip_prefix('<') { (crate::sheet::CondOp::Lt, rest) }
            else if let Some(rest) = cond_trim.strip_prefix('=') { (crate::sheet::CondOp::Eq, rest) }
            else { return Err(format!("条件の演算子が必要: {}", cond_trim)); };
        let target: f64 = num_str.trim().trim_matches('"').parse()
            .map_err(|_| format!("数値が必要: {}", num_str))?;
        crate::sheet::CondCondition::Compare { op, target }
    };

    let bg_color = if color_in.is_empty() { None } else { parse_rgb_input(color_in) };
    if !color_in.is_empty() && bg_color.is_none() {
        return Err(format!("色の指定が無効: {}", color_in));
    }
    Ok(crate::sheet::ConditionalFormat {
        min_col, min_row, max_col, max_row,
        condition,
        text_color: None,
        bg_color,
    })
}

/// Populate an empty `Sheet` from an `ExtractedTable` (URL import).
fn populate_sheet_from_table(sheet: &mut Sheet, table: &url_import::ExtractedTable) {
    for (row, cells) in table.rows.iter().enumerate() {
        for (col, value) in cells.iter().enumerate() {
            if !value.is_empty() {
                sheet.set_cell(col, row, value.clone());
            }
        }
    }
}

/// Populate a sheet from a SQL `QueryResult`. Row 0 = column names; data
/// rows follow. Empty cells stay empty (so `Cell::value` is `Empty`).
fn populate_sheet_from_query_result(sheet: &mut Sheet, result: &sql_import::QueryResult) {
    for (col, name) in result.columns.iter().enumerate() {
        if !name.is_empty() {
            sheet.set_cell(col, 0, name.clone());
        }
    }
    for (row_idx, row) in result.rows.iter().enumerate() {
        for (col, value) in row.iter().enumerate() {
            if !value.is_empty() {
                sheet.set_cell(col, row_idx + 1, value.clone());
            }
        }
    }
}

/// Pick a sheet name from the SQL connection URI. For `postgresql://h/db`
/// we use `db`; falls back to the host, then to `"SQL"`. Trimmed to 31 chars.
fn derive_sheet_name_from_sql_uri(uri: &str) -> String {
    let without_scheme = uri.split_once("://").map(|(_, r)| r).unwrap_or(uri);
    // strip credentials@
    let after_creds = without_scheme.rsplit_once('@').map(|(_, r)| r).unwrap_or(without_scheme);
    // database name is the last path segment (before any ?query)
    let path = after_creds.split_once('/').map(|(_, p)| p).unwrap_or("");
    let path_no_query = path.split('?').next().unwrap_or("");
    let candidate = if !path_no_query.is_empty() {
        // For sqlite paths take just the file stem; otherwise the last segment.
        let last = path_no_query.rsplit(['/', '\\']).next().unwrap_or(path_no_query);
        let stem = last.rsplit_once('.').map(|(s, _)| s).unwrap_or(last);
        stem.to_string()
    } else {
        // No path: use host
        after_creds.split(['/','?',':']).next().unwrap_or("SQL").to_string()
    };
    let candidate = if candidate.is_empty() { "SQL".to_string() } else { candidate };
    let mut out: String = candidate.chars().take(31).collect();
    if out.is_empty() { out = "SQL".to_string(); }
    out
}

/// Pick a reasonable sheet name from the source URL when the table has no
/// `<caption>`. `host` or `host/path[N]` keeps it human-readable; the
/// 31-char Excel sheet-name limit is respected.
fn derive_sheet_name_from_url(url: &str, table_index_1based: usize) -> String {
    // Crude parse: strip scheme, take everything up to the first '/'.
    let without_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let (host, _path) = without_scheme.split_once('/').unwrap_or((without_scheme, ""));
    let base = if host.is_empty() { "URL" } else { host };
    let candidate = format!("{}[{}]", base, table_index_1based);
    if candidate.chars().count() <= 31 {
        candidate
    } else {
        // Trim host to fit within 31 chars including the [N] suffix.
        let suffix = format!("[{}]", table_index_1based);
        let allowed = 31usize.saturating_sub(suffix.chars().count());
        let trimmed: String = base.chars().take(allowed).collect();
        format!("{}{}", trimmed, suffix)
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
/// One destination cell produced by the series-aware paste. Either copied
/// from a source cell (so formula references can be re-based relative to the
/// paste position) or a freshly-computed literal value from extending a series.
enum FillCell {
    Source { sc: usize, sr: usize },
    Literal(String),
}

/// Decide what goes into the destination cell at clipboard-relative offset
/// `(co, ro)`. Inside the original block it returns the matching source cell;
/// beyond it, it extends the series along the primary fill direction, falling
/// back to tiling (repeat) when no numeric/text-number pattern is detected.
fn compute_fill(
    clip: &ClipboardContent,
    co: usize,
    ro: usize,
    vertical_fill: bool,
    horizontal_fill: bool,
) -> FillCell {
    let w = clip.width;
    let h = clip.height;

    if vertical_fill {
        let sc = co % w; // tile columns sideways
        if ro < h {
            return FillCell::Source { sc, sr: ro };
        }
        let values: Vec<crate::cell::CellValue> =
            (0..h).map(|r| clip.cells[r][sc].1.clone()).collect();
        let raws: Vec<String> = (0..h).map(|r| clip.cells[r][sc].0.clone()).collect();
        if let Some(s) = extrapolate_series(&values, &raws, ro) {
            return FillCell::Literal(s);
        }
        FillCell::Source { sc, sr: ro % h }
    } else if horizontal_fill {
        let sr = ro % h; // tile rows downward
        if co < w {
            return FillCell::Source { sc: co, sr };
        }
        let values: Vec<crate::cell::CellValue> =
            (0..w).map(|c| clip.cells[sr][c].1.clone()).collect();
        let raws: Vec<String> = (0..w).map(|c| clip.cells[sr][c].0.clone()).collect();
        if let Some(s) = extrapolate_series(&values, &raws, co) {
            return FillCell::Literal(s);
        }
        FillCell::Source { sc: co % w, sr }
    } else {
        // Plain block paste (target == clipboard size).
        FillCell::Source { sc: co % w, sr: ro % h }
    }
}

/// Extend a series of `values`/`raws` (length n) to index `idx >= n`.
/// Tries an arithmetic numeric progression first, then a "text + trailing
/// integer" progression. Returns None when neither applies (caller tiles).
fn extrapolate_series(
    values: &[crate::cell::CellValue],
    raws: &[String],
    idx: usize,
) -> Option<String> {
    numeric_extrapolate(values, idx).or_else(|| text_number_extrapolate(raws, idx))
}

/// Linear extrapolation of an all-numeric line. Needs at least two values so a
/// step can be derived; a single value has no defined increment (Excel repeats
/// it, which is handled by the tiling fallback).
fn numeric_extrapolate(values: &[crate::cell::CellValue], idx: usize) -> Option<String> {
    let n = values.len();
    if n < 2 {
        return None;
    }
    let nums: Vec<f64> = values
        .iter()
        .map(|v| match v {
            crate::cell::CellValue::Number(x) => Some(*x),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let step = (nums[n - 1] - nums[0]) / (n as f64 - 1.0);
    Some(format_fill_number(nums[0] + step * idx as f64))
}

/// Extend a line of strings that share a common prefix and end in an integer
/// (e.g. "Item1", "Item2" → "Item3"). Skips formulas. The numeric suffix
/// follows the same linear step as the numeric case.
fn text_number_extrapolate(raws: &[String], idx: usize) -> Option<String> {
    let n = raws.len();
    if n < 2 {
        return None;
    }
    if raws.iter().any(|s| s.trim_start().starts_with('=')) {
        return None;
    }
    let parsed: Vec<(String, i64, usize)> = raws
        .iter()
        .map(|s| split_trailing_int(s))
        .collect::<Option<Vec<_>>>()?;
    let prefix = &parsed[0].0;
    if !parsed.iter().all(|(p, _, _)| p == prefix) {
        return None;
    }
    let first = parsed[0].1 as f64;
    let last = parsed[n - 1].1 as f64;
    let step = (last - first) / (n as f64 - 1.0);
    let val = (first + step * idx as f64).round() as i64;
    // Preserve zero-padding width when every sample shares it (e.g. 01, 02).
    let width = parsed[0].2;
    let pad = if width > 0 && parsed.iter().all(|(_, _, w)| *w == width) {
        width
    } else {
        0
    };
    if val >= 0 && pad > 0 {
        Some(format!("{}{:0>pad$}", prefix, val, pad = pad))
    } else {
        Some(format!("{}{}", prefix, val))
    }
}

/// Split a string into (prefix, trailing integer, digit-width). Returns None
/// when there's no trailing run of ASCII digits. The sign is treated as part
/// of the prefix so widths stay simple and round-trips are stable.
fn split_trailing_int(s: &str) -> Option<(String, i64, usize)> {
    // ASCII digits are one byte each, so the count is also a byte offset.
    let digit_count = s.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    if digit_count == 0 {
        return None;
    }
    let digit_start = s.len() - digit_count;
    let digits = &s[digit_start..];
    let num: i64 = digits.parse().ok()?;
    Some((s[..digit_start].to_string(), num, digit_count))
}

/// Render an extrapolated number back to a cell input: whole numbers as
/// integers, otherwise trimmed to a sensible number of decimals.
fn format_fill_number(n: f64) -> String {
    if n.is_finite() && (n - n.round()).abs() < 1e-9 {
        format!("{}", n.round() as i64)
    } else {
        let s = format!("{:.10}", n);
        let s = s.trim_end_matches('0').trim_end_matches('.');
        s.to_string()
    }
}

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
mod l123_ops_tests {
    use super::*;

    #[test]
    fn f4_cycles_single_ref() {
        assert_eq!(cycle_ref_token("B2").as_deref(), Some("$B$2"));
        assert_eq!(cycle_ref_token("$B$2").as_deref(), Some("B$2"));
        assert_eq!(cycle_ref_token("B$2").as_deref(), Some("$B2"));
        assert_eq!(cycle_ref_token("$B2").as_deref(), Some("B2"));
    }

    #[test]
    fn f4_cycles_range_endpoints_together() {
        assert_eq!(cycle_ref_token("A1:B5").as_deref(), Some("$A$1:$B$5"));
        assert_eq!(cycle_ref_token("$A$1:$B$5").as_deref(), Some("A$1:B$5"));
    }

    #[test]
    fn f4_rejects_non_refs() {
        assert_eq!(cycle_ref_token("SUM"), None);
        assert_eq!(cycle_ref_token(""), None);
        assert_eq!(cycle_ref_token("A1:B2:C3"), None);
    }

    #[test]
    fn cycle_ref_absolute_edits_buffer_at_cursor() {
        let mut app = App::new();
        app.input_buffer = "=SUM(A1:B5)".to_string();
        app.edit_cursor_pos = 10; // just after "B5"
        app.cycle_ref_absolute();
        assert_eq!(app.input_buffer, "=SUM($A$1:$B$5)");
        assert_eq!(app.edit_cursor_pos, 14);
        // Not a formula → untouched.
        let mut app = App::new();
        app.input_buffer = "hello A1".to_string();
        app.edit_cursor_pos = 8;
        app.cycle_ref_absolute();
        assert_eq!(app.input_buffer, "hello A1");
    }

    #[test]
    fn define_and_resolve_named_range() {
        let mut app = App::new();
        app.sheet.set_cell(0, 0, "10".to_string());
        app.sheet.set_cell(0, 1, "20".to_string());
        let normalized = app.define_named_range("売上", "A1:A2").expect("define");
        assert_eq!(normalized, "A1:A2");
        app.sheet.set_cell(1, 0, "=SUM(売上)".to_string());
        assert_eq!(app.sheet.evaluate(1, 0), "30");
        // Redefine replaces, delete removes.
        app.define_named_range("売上", "A1").expect("redefine");
        assert_eq!(app.named_ranges.len(), 1);
        assert!(app.delete_named_range("売上"));
        assert!(!app.delete_named_range("売上"));
        // Scalar use of a deleted name reports #NAME?.
        app.sheet.set_cell(2, 0, "=売上".to_string());
        assert_eq!(app.sheet.evaluate(2, 0), "#NAME?");
    }

    #[test]
    fn named_range_name_validation() {
        let mut app = App::new();
        assert!(app.define_named_range("", "A1").is_err());
        assert!(app.define_named_range("1abc", "A1").is_err());
        assert!(app.define_named_range("A1", "B2").is_err()); // looks like a ref
        assert!(app.define_named_range("a b", "A1").is_err()); // space
        assert!(app.define_named_range("合計_2024", "A1").is_ok());
        assert!(app.define_named_range("x", "nope").is_err()); // bad range
    }

    #[test]
    fn cross_sheet_single_cell_name_resolves() {
        let mut app = App::new();
        app.sheet.set_cell(0, 0, "42".to_string());
        let mut other = Sheet::new();
        other.name = "Data".to_string();
        other.set_cell(0, 0, "7".to_string());
        app.other_sheets.push(other);
        // Define a name pointing at Data!A1 by defining it while Data is active.
        app.named_ranges.push(NamedRange {
            name: "基準値".to_string(),
            sheet: "Data".to_string(),
            start: (0, 0),
            end: (0, 0),
        });
        app.sync_named_ranges();
        app.sheet.set_cell(1, 0, "=基準値*2".to_string());
        assert_eq!(app.evaluate(1, 0), "14");
    }
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

/// Cycle a reference token's `$` anchoring one step:
/// (rel,rel) → (abs,abs) → (rel row-abs) → (col-abs rel) → (rel,rel).
/// `token` may be a single ref ("B2") or a range ("B2:C5"); ranges cycle all
/// endpoints in lockstep based on the first endpoint's current state.
/// Returns None when the token isn't a cell reference (e.g. a function name).
fn cycle_ref_token(token: &str) -> Option<String> {
    let parts: Vec<&str> = token.split(':').collect();
    if parts.is_empty() || parts.len() > 2 || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let mut parsed = Vec::new();
    for p in &parts {
        parsed.push(crate::formula::parse_cell_ref(p)?);
    }
    let (_, _, col_abs, row_abs) = parsed[0];
    let (next_col_abs, next_row_abs) = match (col_abs, row_abs) {
        (false, false) => (true, true),
        (true, true) => (false, true),
        (false, true) => (true, false),
        (true, false) => (false, false),
    };
    let rebuilt: Vec<String> = parsed
        .iter()
        .map(|(col, row, _, _)| {
            format!(
                "{}{}{}{}",
                if next_col_abs { "$" } else { "" },
                crate::formula::col_to_name(*col),
                if next_row_abs { "$" } else { "" },
                row + 1,
            )
        })
        .collect();
    Some(rebuilt.join(":"))
}

#[cfg(test)]
mod series_paste_tests {
    use super::*;
    use crate::cell::CellValue;

    /// Build a ClipboardContent from a column of raw inputs at A1 (0,0).
    fn clip_col(raws: &[&str]) -> ClipboardContent {
        let cells: Vec<Vec<(String, CellValue)>> = raws
            .iter()
            .map(|s| vec![(s.to_string(), crate::cell::parse_input(s))])
            .collect();
        ClipboardContent { cells, start_col: 0, start_row: 0, width: 1, height: raws.len() }
    }

    /// Build a ClipboardContent from a single row of raw inputs at A1.
    fn clip_row(raws: &[&str]) -> ClipboardContent {
        let row: Vec<(String, CellValue)> = raws
            .iter()
            .map(|s| (s.to_string(), crate::cell::parse_input(s)))
            .collect();
        ClipboardContent { cells: vec![row], start_col: 0, start_row: 0, width: raws.len(), height: 1 }
    }

    /// Resolve the literal a vertical fill would place at row offset `ro`.
    fn vfill(clip: &ClipboardContent, ro: usize) -> String {
        match compute_fill(clip, 0, ro, true, false) {
            FillCell::Literal(s) => s,
            FillCell::Source { sc, sr } => clip.cells[sr][sc].0.clone(),
        }
    }

    fn hfill(clip: &ClipboardContent, co: usize) -> String {
        match compute_fill(clip, co, 0, false, true) {
            FillCell::Literal(s) => s,
            FillCell::Source { sc, sr } => clip.cells[sr][sc].0.clone(),
        }
    }

    #[test]
    fn numeric_step_one() {
        let c = clip_col(&["1", "2"]);
        assert_eq!(vfill(&c, 0), "1");
        assert_eq!(vfill(&c, 1), "2");
        assert_eq!(vfill(&c, 2), "3");
        assert_eq!(vfill(&c, 5), "6");
    }

    #[test]
    fn numeric_step_two() {
        let c = clip_col(&["2", "4"]);
        assert_eq!(vfill(&c, 2), "6");
        assert_eq!(vfill(&c, 3), "8");
    }

    #[test]
    fn numeric_decreasing() {
        let c = clip_col(&["10", "8"]);
        assert_eq!(vfill(&c, 2), "6");
        assert_eq!(vfill(&c, 3), "4");
    }

    #[test]
    fn numeric_fractional_step() {
        let c = clip_col(&["1", "1.5"]);
        assert_eq!(vfill(&c, 2), "2");
        assert_eq!(vfill(&c, 3), "2.5");
    }

    #[test]
    fn single_numeric_repeats() {
        // No second sample → no defined increment → tile (repeat).
        let c = clip_col(&["7"]);
        assert_eq!(vfill(&c, 1), "7");
        assert_eq!(vfill(&c, 2), "7");
    }

    #[test]
    fn text_with_trailing_number() {
        let c = clip_col(&["Item1", "Item2"]);
        assert_eq!(vfill(&c, 2), "Item3");
        assert_eq!(vfill(&c, 4), "Item5");
    }

    #[test]
    fn text_zero_padded() {
        let c = clip_col(&["No01", "No02"]);
        assert_eq!(vfill(&c, 2), "No03");
        assert_eq!(vfill(&c, 9), "No10");
    }

    #[test]
    fn plain_text_tiles() {
        let c = clip_col(&["a", "b"]);
        assert_eq!(vfill(&c, 2), "a"); // 2 % 2
        assert_eq!(vfill(&c, 3), "b");
    }

    #[test]
    fn formulas_tile_not_series() {
        // Formulas must not be treated as text-number series; they tile and the
        // paste loop re-bases their references separately.
        let c = clip_col(&["=A1+1", "=A2+1"]);
        match compute_fill(&c, 0, 2, true, false) {
            FillCell::Source { sc, sr } => { assert_eq!((sc, sr), (0, 0)); }
            FillCell::Literal(s) => panic!("expected tiled source, got literal {s}"),
        }
    }

    #[test]
    fn horizontal_fill_extends_right() {
        let c = clip_row(&["1", "2"]);
        assert_eq!(hfill(&c, 2), "3");
        assert_eq!(hfill(&c, 4), "5");
    }
}

fn handle_key(app: &mut App, key: KeyEvent) {
    match app.mode {
        Mode::Normal => handle_normal_mode(app, key),
        Mode::Edit => handle_edit_mode(app, key),
        Mode::Menu => handle_menu_mode(app, key),
        Mode::Dialog => handle_dialog_mode(app, key),
        Mode::Popup => handle_popup_mode(app, key),
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
            KeyCode::Char('r') | KeyCode::Char('R') => { app.dispatch(Action::EditReplace); return; }
            KeyCode::Char('b') | KeyCode::Char('B') => { app.dispatch(Action::FormatBoldToggle); return; }
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
            KeyCode::PageDown => { app.dispatch(Action::SheetNext); return; }
            KeyCode::PageUp => { app.dispatch(Action::SheetPrev); return; }
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
        KeyCode::F(5) => {
            app.dispatch(Action::EditGoto);
        }
        KeyCode::F(9) => {
            app.dispatch(Action::Recalc);
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
            if !ctrl && !alt {
                // Lotus 1-2-3 style: `/` opens the menu bar for letter-key
                // navigation instead of starting a cell edit. A literal `/`
                // as the first character can still be typed via F2.
                // '／' covers a Japanese IME left on while hitting the key.
                if c == '/' || c == '／' {
                    app.menu_state.open_bar();
                    app.mode = Mode::Menu;
                    app.status_message =
                        "メニュー: 頭文字キーで選択 / Esc で戻る".to_string();
                    return;
                }
                // Lotus 1-2-3 WYSIWYG style: `:` opens the format popup.
                // A literal leading `:` can still be typed via F2.
                // '：' covers a Japanese IME left on while hitting the key.
                if c == ':' || c == '：' {
                    let (tw, th) = terminal::size().unwrap_or((80, 24));
                    app.popup = Some(PopupMenu::open(wysiwyg_menu_items(), 1, 1, tw, th));
                    app.mode = Mode::Popup;
                    app.status_message =
                        "書式メニュー: 頭文字キーで選択 / Esc で戻る".to_string();
                    return;
                }
                // Any printable char starts edit mode (Excel-style)
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
        KeyCode::F(4) => {
            app.cycle_ref_absolute();
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

    // Bar navigation (slash-menu style): a top-level menu is highlighted and
    // its dropdown shown as a preview (no item selected). A top-level letter
    // switches + descends; a letter with no top-level match runs the matching
    // item of the previewed menu. Enter / Down descends into the preview.
    if !app.menu_state.dropped {
        match key.code {
            KeyCode::Esc => {
                app.menu_state.close();
                app.mode = Mode::Normal;
            }
            KeyCode::Left => app.menu_state.move_left(&app.menu_bar),
            KeyCode::Right => app.menu_state.move_right(&app.menu_bar),
            KeyCode::Enter | KeyCode::Down => {
                app.menu_state.dropped = true;
            }
            KeyCode::Char(c) => {
                // open_index (via activate_by_mnemonic) drops the submenu,
                // so a single letter descends one level without Enter.
                if app.menu_bar.activate_by_mnemonic(c, &mut app.menu_state) {
                    return;
                }
                if let Some(action) = app.menu_state.activate_by_mnemonic(&app.menu_bar, c) {
                    app.menu_state.close();
                    app.mode = Mode::Normal;
                    app.dispatch(action);
                }
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc => {
            // Back out one level: dropdown → bar navigation → (next Esc) close.
            app.menu_state.dropped = false;
            app.menu_state.item = 0;
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
        KeyCode::Tab | KeyCode::Down => {
            if let Some(d) = app.dialog.as_mut() {
                if d.fields.len() > 1 { d.next_field(); }
            }
        }
        KeyCode::BackTab | KeyCode::Up => {
            if let Some(d) = app.dialog.as_mut() {
                if d.fields.len() > 1 { d.prev_field(); }
            }
        }
        KeyCode::Left => {
            if let Some(d) = app.dialog.as_mut() {
                let focus = d.focus;
                d.fields[focus].cycle(-1);
            }
        }
        KeyCode::Right => {
            if let Some(d) = app.dialog.as_mut() {
                let focus = d.focus;
                d.fields[focus].cycle(1);
            }
        }
        KeyCode::Backspace => {
            if let Some(d) = app.dialog.as_mut() {
                let focus = d.focus;
                if !d.fields[focus].is_choice() {
                    d.current_input_mut().pop();
                }
            }
        }
        KeyCode::Char(c) => {
            if !key.modifiers.contains(KeyModifiers::CONTROL) {
                if let Some(d) = app.dialog.as_mut() {
                    let focus = d.focus;
                    if d.fields[focus].is_choice() {
                        // Space cycles; any other char jumps to the option
                        // starting with it (digits pick decimals directly).
                        if c == ' ' {
                            d.fields[focus].cycle(1);
                        } else {
                            d.fields[focus].select_by_char(c);
                        }
                    } else {
                        d.current_input_mut().push(c);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Close the WYSIWYG popup and return to READY.
fn close_popup(app: &mut App) {
    app.popup = None;
    app.mode = Mode::Normal;
}

/// Run the outcome of activating a popup item: execute an action leaf
/// (closing the popup first) or stay open after descending into a submenu.
fn apply_popup_outcome(app: &mut App, outcome: PopupOutcome) {
    if let PopupOutcome::Action(action) = outcome {
        close_popup(app);
        app.dispatch(action);
    }
}

fn handle_popup_mode(app: &mut App, key: KeyEvent) {
    let (tw, th) = terminal::size().unwrap_or((80, 24));
    match key.code {
        KeyCode::Esc => {
            if let Some(p) = app.popup.as_mut() {
                if !p.pop() {
                    close_popup(app);
                }
            }
        }
        KeyCode::Up => {
            if let Some(p) = app.popup.as_mut() {
                let top = p.top_mut();
                top.selected = if top.selected == 0 {
                    top.items.len() - 1
                } else {
                    top.selected - 1
                };
            }
        }
        KeyCode::Down => {
            if let Some(p) = app.popup.as_mut() {
                let top = p.top_mut();
                top.selected = (top.selected + 1) % top.items.len();
            }
        }
        KeyCode::Left => {
            if let Some(p) = app.popup.as_mut() {
                p.pop();
            }
        }
        KeyCode::Right => {
            // Descend only — Right on an action leaf does nothing.
            if let Some(p) = app.popup.as_mut() {
                if p.top().items.get(p.top().selected).map(|i| i.is_submenu()).unwrap_or(false) {
                    let outcome = p.activate(tw, th);
                    apply_popup_outcome(app, outcome);
                }
            }
        }
        KeyCode::Enter => {
            if let Some(p) = app.popup.as_mut() {
                let outcome = p.activate(tw, th);
                apply_popup_outcome(app, outcome);
            }
        }
        KeyCode::Char(c) => {
            if !key.modifiers.contains(KeyModifiers::CONTROL) {
                if let Some(p) = app.popup.as_mut() {
                    let outcome = p.activate_mnemonic(c, tw, th);
                    apply_popup_outcome(app, outcome);
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

    // WYSIWYG popup mode: click activates items (descend or execute);
    // click outside any level closes the popup.
    if app.mode == Mode::Popup {
        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
            let (tw, th) = crossterm::terminal::size().unwrap_or((80, 24));
            let hit = app.popup.as_ref().and_then(|p| p.hit_test(col, row));
            match hit {
                Some((level, item)) => {
                    if let Some(p) = app.popup.as_mut() {
                        p.stack.truncate(level + 1);
                        p.top_mut().selected = item;
                        let outcome = p.activate(tw, th);
                        apply_popup_outcome(app, outcome);
                    }
                }
                None => close_popup(app),
            }
        }
        return;
    }

    // Dialog mode: click selects choice options / focuses text fields /
    // presses the OK・キャンセル buttons; the wheel cycles choice options.
    if app.mode == Mode::Dialog {
        let (tw, th) = crossterm::terminal::size().unwrap_or((80, 24));
        let hit = app.dialog.as_ref().map(|d| UI::dialog_hit_test(d, tw, th, col, row));
        let Some(hit) = hit else { return; };
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => match hit {
                ui::DialogHit::Option(i, oi) => {
                    if let Some(d) = app.dialog.as_mut() {
                        d.focus = i;
                        d.fields[i].selected = oi;
                    }
                }
                ui::DialogHit::Field(i) => {
                    if let Some(d) = app.dialog.as_mut() { d.focus = i; }
                }
                ui::DialogHit::Ok => app.commit_dialog(),
                ui::DialogHit::Cancel => {
                    app.dialog = None;
                    app.mode = Mode::Normal;
                }
                ui::DialogHit::Inside | ui::DialogHit::Outside => {}
            },
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let delta = if mouse.kind == MouseEventKind::ScrollDown { 1 } else { -1 };
                if let (Some(d), ui::DialogHit::Option(i, _) | ui::DialogHit::Field(i)) =
                    (app.dialog.as_mut(), hit)
                {
                    if d.fields[i].is_choice() {
                        d.focus = i;
                        d.fields[i].cycle(delta);
                    }
                }
            }
            _ => {}
        }
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
            // Sheet tab click → switch sheet.
            if let Some(target) = app.screen_to_sheet_tab(col, row) {
                if app.mode == Mode::Edit { app.commit_edit(); app.mode = Mode::Normal; }
                app.switch_sheet(target);
                app.status_message = format!("シート: {}", app.sheet.name);
                return;
            }
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

/// Probe whether the terminal renders East Asian Ambiguous characters as
/// double width: print one at the origin and read back the cursor column.
/// Runs on the alternate screen before the first draw, so nothing visible
/// survives. Returns None if the terminal doesn't answer the position query.
fn probe_ambiguous_wide(stdout: &mut impl std::io::Write) -> Option<bool> {
    use crossterm::cursor::MoveTo;
    execute!(stdout, MoveTo(0, 0)).ok()?;
    write!(stdout, "○").ok()?;
    stdout.flush().ok()?;
    let (col, _) = crossterm::cursor::position().ok()?;
    Some(col >= 2)
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

    // Decide how East Asian Ambiguous characters (①, ○, →, ─, …) are
    // counted. They render double-width in many Japanese terminal setups and
    // single-width elsewhere, so the wrong guess misaligns the grid.
    // TBLA_AMBIGUOUS_WIDE=1/0 forces the mode; otherwise print one on the
    // alternate screen and ask the terminal where the cursor landed.
    let ambiguous_wide = match std::env::var("TBLA_AMBIGUOUS_WIDE") {
        Ok(v) => matches!(v.trim(), "1" | "true" | "yes" | "on"),
        Err(_) => probe_ambiguous_wide(&mut stdout).unwrap_or(false),
    };
    width::set_ambiguous_wide(ambiguous_wide);

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
