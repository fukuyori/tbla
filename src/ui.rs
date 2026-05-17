use crossterm::{
    cursor::{Hide, MoveTo, Show},
    queue,
    style::{Color, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};
use std::io::{stdout, Result, Write};
use unicode_width::UnicodeWidthStr;

use crate::{App, Mode};
use crate::cell::CellValue;
use crate::formula;
use crate::menu::{MenuBar, MenuState, SubItem, ContextMenu};

const ROW_LABEL_WIDTH: usize = 5;

// Colors
const GREEN: Color = Color::Rgb { r: 0, g: 170, b: 0 };
const ORANGE: Color = Color::Rgb { r: 255, g: 136, b: 0 };
const MENU_BG: Color = Color::Rgb { r: 220, g: 220, b: 220 };
const MENU_FG: Color = Color::Black;
const MENU_SEL_BG: Color = Color::Rgb { r: 0, g: 100, b: 200 };
const MENU_SEL_FG: Color = Color::White;
const SELECTION_BG: Color = Color::Rgb { r: 60, g: 110, b: 200 };
// Point mode (Excel-style formula reference selection) highlight
const POINT_CURSOR_BG: Color = Color::Rgb { r: 80, g: 150, b: 255 };
const POINT_RANGE_BG: Color = Color::Rgb { r: 40, g: 80, b: 160 };

/// Truncate string to fit within max_width (display width) - keeps left side
fn truncate_to_width(s: &str, max_width: usize) -> String {
    let mut result = String::new();
    let mut width = 0;
    for c in s.chars() {
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        if width + w > max_width {
            break;
        }
        result.push(c);
        width += w;
    }
    result
}

/// Truncate string to fit within max_width - keeps right side (for editing)
fn truncate_from_end(s: &str, max_width: usize) -> String {
    let total_width = UnicodeWidthStr::width(s);
    if total_width <= max_width {
        return s.to_string();
    }

    let skip_width = total_width - max_width;
    let mut skipped = 0;
    let mut result = String::new();

    for c in s.chars() {
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        if skipped < skip_width {
            skipped += w;
        } else {
            result.push(c);
        }
    }
    result
}

/// Pad string to target display width
fn pad_to_width(s: &str, target_width: usize, align_right: bool) -> String {
    let current = UnicodeWidthStr::width(s);
    if current >= target_width {
        return truncate_to_width(s, target_width);
    }
    let padding = target_width - current;
    if align_right {
        format!("{}{}", " ".repeat(padding), s)
    } else {
        format!("{}{}", s, " ".repeat(padding))
    }
}

fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Visible breakdown of an in-cell edit buffer with a "block" cursor
/// (the character under the cursor is shown with inverted colors).
pub struct EditView {
    pub left: String,        // visible portion before the cursor
    pub cursor_char: char,   // character under the cursor (' ' if at end)
    pub right: String,       // visible portion after the cursor
}

impl EditView {
    pub fn width(&self) -> usize {
        display_width(&self.left)
            + char_width(self.cursor_char)
            + display_width(&self.right)
    }
}

fn char_width(c: char) -> usize {
    unicode_width::UnicodeWidthChar::width(c).unwrap_or(1)
}

/// Compute which portion of the edit buffer is visible, given the available
/// width. The cursor character is always included; surrounding context scrolls
/// to keep the cursor visible.
pub fn compute_edit_view(input: &str, cursor_pos: usize, available_width: usize) -> EditView {
    let chars: Vec<char> = input.chars().collect();
    let cursor_pos = cursor_pos.min(chars.len());

    let cursor_char = if cursor_pos < chars.len() {
        chars[cursor_pos]
    } else {
        ' '
    };
    let cursor_w = char_width(cursor_char);

    let left_full: String = chars[..cursor_pos].iter().collect();
    let right_full: String = if cursor_pos < chars.len() {
        chars[cursor_pos + 1..].iter().collect()
    } else {
        String::new()
    };

    let left_w = display_width(&left_full);
    let right_w = display_width(&right_full);
    let total_w = left_w + cursor_w + right_w;

    if total_w <= available_width {
        return EditView {
            left: left_full,
            cursor_char,
            right: right_full,
        };
    }

    // Need to scroll: reserve a small right-side context, give the rest to the left.
    let remaining = available_width.saturating_sub(cursor_w);
    let right_reserve = (remaining / 4).min(right_w);
    let left_budget = remaining.saturating_sub(right_reserve);

    let visible_left = truncate_from_end(&left_full, left_budget);
    let used_left_w = display_width(&visible_left);
    let right_budget = available_width
        .saturating_sub(used_left_w)
        .saturating_sub(cursor_w);
    let visible_right = truncate_to_width(&right_full, right_budget);

    EditView {
        left: visible_left,
        cursor_char,
        right: visible_right,
    }
}

pub struct UI;

impl UI {
    fn cursor_color(mode: Mode) -> Color {
        match mode {
            Mode::Edit => ORANGE,
            _ => GREEN,
        }
    }

    /// Compute the screen position (column, row) of the in-cell text cursor
    /// during edit mode. The position points at the character under the text
    /// cursor in the cell, which is also where any IME composition window
    /// should appear.
    fn editing_cursor_pos(app: &App, visible_cols: &[(usize, usize)]) -> Option<(u16, u16)> {
        if app.mode != Mode::Edit {
            return None;
        }
        if app.cursor_row < app.view_row {
            return None;
        }

        // Find the starting screen column of the cursor cell
        let mut x = ROW_LABEL_WIDTH;
        let mut cell_total_width: Option<usize> = None;
        let mut idx = 0;
        while idx < visible_cols.len() {
            let (col, col_width) = visible_cols[idx];
            if col == app.cursor_col {
                cell_total_width = Some(col_width);
                break;
            }
            x += col_width;
            idx += 1;
        }
        cell_total_width?;

        // Account for spillover: the cell's effective width may extend into
        // adjacent empty cells, so the cursor character can appear past the
        // owner column's right edge.
        let mut total_width = cell_total_width.unwrap();
        let input = &app.input_buffer;
        let cursor_at_end = app.edit_cursor_pos >= input.chars().count();
        let extra = if cursor_at_end { 1 } else { 0 };
        let value_display_width = display_width(input) + extra;

        if value_display_width > total_width.saturating_sub(1) {
            let mut next_idx = idx + 1;
            while next_idx < visible_cols.len() {
                let (next_col, next_col_width) = visible_cols[next_idx];
                let next_is_empty = app.sheet.get_cell_ref(next_col, app.cursor_row).is_none();
                if !next_is_empty {
                    break;
                }
                total_width += next_col_width;
                if total_width >= value_display_width + 1 {
                    break;
                }
                next_idx += 1;
            }
        }

        let content_width = total_width.saturating_sub(1);
        let view = compute_edit_view(input, app.edit_cursor_pos, content_width);
        let text_x = x + display_width(&view.left);

        let screen_row = 2 + (app.cursor_row - app.view_row);
        Some((text_x as u16, screen_row as u16))
    }

    /// Calculate how many columns fit in the terminal and their positions
    fn calc_visible_cols(app: &App, term_width: usize) -> Vec<(usize, usize)> {
        let mut cols = Vec::new();
        let mut used_width = ROW_LABEL_WIDTH;
        let mut col = app.view_col;

        while used_width < term_width && col <= 255 {
            let col_width = app.sheet.get_col_width(col);
            if used_width + col_width > term_width {
                break;
            }
            cols.push((col, col_width));
            used_width += col_width;
            col += 1;
        }

        cols
    }

    pub fn draw(app: &App) -> Result<()> {
        let mut stdout = stdout();
        let (term_width, term_height) = terminal::size()?;
        // Layout: row 0 = menu bar, row 1 = column headers, grid, then formula bar + status
        let grid_height = (term_height as usize).saturating_sub(4);
        let visible_cols = Self::calc_visible_cols(app, term_width as usize);

        let cursor_color = Self::cursor_color(app.mode);

        queue!(stdout, Hide)?;
        queue!(stdout, MoveTo(0, 0))?;

        Self::draw_menu_bar(&mut stdout, app, term_width)?;
        Self::draw_column_headers(&mut stdout, app, &visible_cols, term_width)?;
        Self::draw_grid(&mut stdout, app, grid_height, &visible_cols, term_width, cursor_color)?;
        Self::draw_formula_bar(&mut stdout, app, term_height, term_width)?;
        Self::draw_status_bar(&mut stdout, app, term_height, term_width)?;

        // Overlays
        if app.mode == Mode::Menu {
            Self::draw_open_menu(&mut stdout, &app.menu_bar, &app.menu_state)?;
        }
        if let Some(cm) = &app.context_menu {
            Self::draw_context_menu(&mut stdout, cm)?;
        }
        if app.mode == Mode::Dialog {
            Self::draw_dialog(&mut stdout, app, term_height, term_width)?;
        }

        // Position the OS-level terminal cursor so the IME composition window
        // (which appears at the cursor) shows up at the editing location.
        if app.mode == Mode::Edit {
            if let Some((cx, cy)) = Self::editing_cursor_pos(app, &visible_cols) {
                queue!(stdout, MoveTo(cx, cy), Show)?;
            } else {
                queue!(stdout, Hide)?;
            }
        } else if app.mode == Mode::Dialog {
            // Place the cursor at the end of the dialog input
            if let Some(dialog) = &app.dialog {
                let prefix = format!(" {}: ", dialog.label);
                let x = display_width(&prefix) + display_width(&dialog.input);
                queue!(stdout, MoveTo(x as u16, term_height - 2), Show)?;
            } else {
                queue!(stdout, Hide)?;
            }
        } else {
            queue!(stdout, Hide)?;
        }

        stdout.flush()?;
        Ok(())
    }

    fn draw_menu_bar(stdout: &mut std::io::Stdout, app: &App, term_width: u16) -> Result<()> {
        queue!(
            stdout,
            MoveTo(0, 0),
            SetBackgroundColor(MENU_BG),
            SetForegroundColor(MENU_FG),
        )?;

        // Clear background
        write!(stdout, "{:width$}", "", width = term_width as usize)?;

        let positions = app.menu_bar.bar_positions();
        for (idx, menu) in app.menu_bar.menus.iter().enumerate() {
            let (x, _w) = positions[idx];
            queue!(stdout, MoveTo(x as u16, 0))?;
            let is_open = app.menu_state.open == Some(idx);
            if is_open {
                queue!(stdout, SetBackgroundColor(MENU_SEL_BG), SetForegroundColor(MENU_SEL_FG))?;
            } else {
                queue!(stdout, SetBackgroundColor(MENU_BG), SetForegroundColor(MENU_FG))?;
            }
            write!(stdout, "{}", menu.label)?;
        }

        queue!(stdout, ResetColor)?;
        Ok(())
    }

    fn draw_open_menu(stdout: &mut std::io::Stdout, bar: &MenuBar, state: &MenuState) -> Result<()> {
        let Some(idx) = state.open else { return Ok(()); };
        let positions = bar.bar_positions();
        let (x_start, _) = positions[idx];
        let width = bar.submenu_width(idx);
        let menu = &bar.menus[idx];

        for (i, item) in menu.items.iter().enumerate() {
            queue!(stdout, MoveTo(x_start as u16, (i + 1) as u16))?;
            let is_sel = i == state.item && !item.is_separator();
            if is_sel {
                queue!(stdout, SetBackgroundColor(MENU_SEL_BG), SetForegroundColor(MENU_SEL_FG))?;
            } else {
                queue!(stdout, SetBackgroundColor(MENU_BG), SetForegroundColor(MENU_FG))?;
            }
            match item {
                SubItem::Separator => {
                    let line = "─".repeat(width.saturating_sub(2));
                    write!(stdout, " {} ", line)?;
                }
                SubItem::Item { label, shortcut, .. } => {
                    let label_w = display_width(label);
                    let inner = width.saturating_sub(2);
                    let line = if let Some(sc) = shortcut {
                        let sc_w = display_width(sc);
                        let pad = inner.saturating_sub(label_w + sc_w + 1);
                        format!(" {}{}{} {}", label, " ".repeat(pad), " ", sc)
                    } else {
                        let pad = inner.saturating_sub(label_w);
                        format!(" {}{}", label, " ".repeat(pad + 1))
                    };
                    write!(stdout, "{}", pad_to_width(&line, width, false))?;
                }
            }
        }
        queue!(stdout, ResetColor)?;
        Ok(())
    }

    fn draw_context_menu(stdout: &mut std::io::Stdout, cm: &ContextMenu) -> Result<()> {
        let width = cm.width;
        // Top border
        queue!(stdout, MoveTo(cm.col, cm.row), SetBackgroundColor(MENU_BG), SetForegroundColor(MENU_FG))?;
        let top = format!("┌{}┐", "─".repeat(width.saturating_sub(2)));
        write!(stdout, "{}", top)?;

        for (i, (label, _)) in cm.items.iter().enumerate() {
            queue!(stdout, MoveTo(cm.col, cm.row + 1 + i as u16))?;
            let is_sel = i == cm.selected;
            if is_sel {
                queue!(stdout, SetBackgroundColor(MENU_SEL_BG), SetForegroundColor(MENU_SEL_FG))?;
            } else {
                queue!(stdout, SetBackgroundColor(MENU_BG), SetForegroundColor(MENU_FG))?;
            }
            let label_w = display_width(label);
            let inner = width.saturating_sub(2);
            let pad = inner.saturating_sub(label_w);
            write!(stdout, "│{}{}│", label, " ".repeat(pad))?;
        }

        queue!(
            stdout,
            MoveTo(cm.col, cm.row + 1 + cm.items.len() as u16),
            SetBackgroundColor(MENU_BG),
            SetForegroundColor(MENU_FG)
        )?;
        let bottom = format!("└{}┘", "─".repeat(width.saturating_sub(2)));
        write!(stdout, "{}", bottom)?;

        queue!(stdout, ResetColor)?;
        Ok(())
    }

    fn draw_dialog(stdout: &mut std::io::Stdout, app: &App, term_height: u16, term_width: u16) -> Result<()> {
        let Some(dialog) = &app.dialog else { return Ok(()); };

        // Render as bottom prompt (overlays the formula bar area)
        queue!(
            stdout,
            MoveTo(0, term_height - 2),
            SetBackgroundColor(MENU_BG),
            SetForegroundColor(MENU_FG)
        )?;
        let prompt = format!(" {}: {}_ ", dialog.label, dialog.input);
        let display = pad_to_width(&prompt, term_width as usize, false);
        write!(stdout, "{}", display)?;
        queue!(stdout, ResetColor)?;

        // Hint line
        queue!(
            stdout,
            MoveTo(0, term_height - 1),
            SetBackgroundColor(Color::Black),
            SetForegroundColor(GREEN)
        )?;
        let hint = " Enter: 実行   Esc: キャンセル ".to_string();
        let line = pad_to_width(&hint, term_width as usize, false);
        write!(stdout, "{}", line)?;
        queue!(stdout, ResetColor)?;
        Ok(())
    }

    fn draw_column_headers(stdout: &mut std::io::Stdout, _app: &App, visible_cols: &[(usize, usize)], term_width: u16) -> Result<()> {
        queue!(
            stdout,
            MoveTo(0, 1),
            SetBackgroundColor(GREEN),
            SetForegroundColor(Color::Black),
        )?;

        write!(stdout, "{:width$}", "", width = ROW_LABEL_WIDTH)?;

        let mut used = ROW_LABEL_WIDTH;
        for &(col, col_width) in visible_cols {
            let col_name = formula::col_to_name(col);
            // Reserve the last column of each header slot for a resize-handle
            // separator. The column name is centered in the remaining width.
            let name_width = col_width.saturating_sub(1).max(1);
            write!(stdout, "{:^width$}", col_name, width = name_width)?;
            write!(stdout, "│")?;
            used += col_width;
        }

        let remaining = (term_width as usize).saturating_sub(used);
        write!(stdout, "{:width$}", "", width = remaining)?;

        queue!(stdout, ResetColor)?;
        Ok(())
    }

    fn draw_grid(stdout: &mut std::io::Stdout, app: &App, grid_height: usize, visible_cols: &[(usize, usize)], term_width: u16, cursor_color: Color) -> Result<()> {
        let has_selection = app.has_selection();
        let (sel_min_col, sel_min_row, sel_max_col, sel_max_row) = if has_selection {
            app.get_selection_bounds()
        } else {
            (usize::MAX, usize::MAX, 0, 0)
        };

        // Point-mode highlight bounds (Excel-style reference selection)
        let pm_cursor = app.point_mode.as_ref().map(|pm| pm.cursor);
        let pm_range = app.point_mode.as_ref().and_then(|pm| {
            pm.anchor.map(|a| {
                (
                    a.0.min(pm.cursor.0),
                    a.1.min(pm.cursor.1),
                    a.0.max(pm.cursor.0),
                    a.1.max(pm.cursor.1),
                )
            })
        });

        for row in 0..grid_height {
            let actual_row = app.view_row + row;
            let screen_row = (row + 2) as u16;

            // Start with a fully-cleared line in the default cell background
            // so any residual content (e.g. left over from IME composition or
            // a previous wider render) is wiped.
            queue!(
                stdout,
                MoveTo(0, screen_row),
                SetBackgroundColor(Color::Black),
                Clear(ClearType::CurrentLine)
            )?;

            // Row label
            queue!(
                stdout,
                MoveTo(0, screen_row),
                SetBackgroundColor(GREEN),
                SetForegroundColor(Color::Black),
            )?;
            write!(stdout, "{:>width$}", actual_row + 1, width = ROW_LABEL_WIDTH)?;
            queue!(stdout, ResetColor)?;

            let mut used = ROW_LABEL_WIDTH;
            let mut col_idx = 0;

            while col_idx < visible_cols.len() {
                // Pin the cursor to the expected start of this cell so that
                // any small mismatch between our display-width math and the
                // terminal's actual glyph width (common with double-width
                // CJK characters) doesn't cause cumulative column drift.
                queue!(stdout, MoveTo(used as u16, screen_row))?;

                let (actual_col, col_width) = visible_cols[col_idx];

                let is_cursor = actual_col == app.cursor_col && actual_row == app.cursor_row;
                let is_selected = has_selection
                    && actual_col >= sel_min_col && actual_col <= sel_max_col
                    && actual_row >= sel_min_row && actual_row <= sel_max_row;
                let is_editing = is_cursor && app.mode == Mode::Edit;

                let cell = app.sheet.get_cell(actual_col, actual_row);
                let is_number = matches!(cell.value, CellValue::Number(_) | CellValue::Formula(_));

                // Compute the value to display and the width it would need.
                // For editing, account for at least one extra column for the
                // block cursor when the text cursor is at the end of input.
                let (value, value_display_width) = if is_editing {
                    let input = app.input_buffer.clone();
                    let cursor_at_end =
                        app.edit_cursor_pos >= input.chars().count();
                    let extra = if cursor_at_end { 1 } else { 0 };
                    let w = display_width(&input) + extra;
                    (input, w)
                } else {
                    let v = app.sheet.evaluate(actual_col, actual_row);
                    let w = display_width(&v);
                    (v, w)
                };

                // Check whether the content overflows its own column.
                // Numbers never spill over (they show ###). Text may extend into
                // adjacent empty cells (Excel-style).
                let needs_spillover = value_display_width > col_width.saturating_sub(1);
                let allow_spillover = needs_spillover && (is_editing || !is_number);

                let mut total_width = col_width;
                let mut covered_count = 0usize;

                if allow_spillover {
                    let mut next_idx = col_idx + 1;
                    while next_idx < visible_cols.len() {
                        let (next_col, next_col_width) = visible_cols[next_idx];
                        let next_is_empty = app.sheet.get_cell_ref(next_col, actual_row).is_none();
                        let next_is_cursor = next_col == app.cursor_col && actual_row == app.cursor_row;
                        let next_is_selected = has_selection
                            && next_col >= sel_min_col && next_col <= sel_max_col
                            && actual_row >= sel_min_row && actual_row <= sel_max_row;
                        let next_is_point = pm_cursor == Some((next_col, actual_row))
                            || pm_range
                                .map(|(c1, r1, c2, r2)| {
                                    next_col >= c1
                                        && next_col <= c2
                                        && actual_row >= r1
                                        && actual_row <= r2
                                })
                                .unwrap_or(false);

                        if !next_is_empty || next_is_cursor || next_is_selected || next_is_point {
                            break;
                        }
                        total_width += next_col_width;
                        covered_count += 1;
                        if total_width >= value_display_width + 1 {
                            break;
                        }
                        next_idx += 1;
                    }
                }

                let content_width = total_width.saturating_sub(1);

                let is_point_cursor = pm_cursor == Some((actual_col, actual_row));
                let is_in_point_range = pm_range
                    .map(|(c1, r1, c2, r2)| {
                        actual_col >= c1
                            && actual_col <= c2
                            && actual_row >= r1
                            && actual_row <= r2
                    })
                    .unwrap_or(false);

                let (bg, fg) = if is_cursor {
                    (cursor_color, Color::Black)
                } else if is_point_cursor {
                    (POINT_CURSOR_BG, Color::Black)
                } else if is_in_point_range {
                    (POINT_RANGE_BG, Color::White)
                } else if is_selected {
                    (SELECTION_BG, Color::White)
                } else {
                    (Color::Black, GREEN)
                };

                if is_editing {
                    // Render with a block cursor: the character under the
                    // text cursor is shown with inverted colors. Three
                    // segments are written: left, cursor char, right, then
                    // padding + trailing space to fill the cell.
                    let view = compute_edit_view(&value, app.edit_cursor_pos, content_width);
                    let used_w = view.width();
                    let pad = content_width.saturating_sub(used_w);

                    queue!(stdout, SetBackgroundColor(bg), SetForegroundColor(fg))?;
                    write!(stdout, "{}", view.left)?;

                    // Inverted cursor cell
                    queue!(stdout, SetBackgroundColor(fg), SetForegroundColor(bg))?;
                    write!(stdout, "{}", view.cursor_char)?;

                    // Restore and finish
                    queue!(stdout, SetBackgroundColor(bg), SetForegroundColor(fg))?;
                    write!(stdout, "{}{} ", view.right, " ".repeat(pad))?;
                } else {
                    let content = if value_display_width > content_width {
                        if is_number {
                            "#".repeat(content_width)
                        } else {
                            let truncated = truncate_to_width(&value, content_width.saturating_sub(1));
                            format!("{}…", truncated)
                        }
                    } else {
                        value
                    };

                    queue!(stdout, SetBackgroundColor(bg), SetForegroundColor(fg))?;

                    let formatted = if is_number {
                        pad_to_width(&content, content_width, true)
                    } else {
                        pad_to_width(&content, content_width, false)
                    };
                    write!(stdout, "{} ", formatted)?;
                }

                queue!(stdout, ResetColor)?;
                used += total_width;
                col_idx += 1 + covered_count;
            }

            let remaining = (term_width as usize).saturating_sub(used);
            if remaining > 0 {
                queue!(stdout, SetBackgroundColor(Color::Black))?;
                write!(stdout, "{:width$}", "", width = remaining)?;
                queue!(stdout, ResetColor)?;
            }
        }

        Ok(())
    }

    fn draw_formula_bar(stdout: &mut std::io::Stdout, app: &App, term_height: u16, term_width: u16) -> Result<()> {
        queue!(
            stdout,
            MoveTo(0, term_height - 2),
            SetBackgroundColor(GREEN),
            SetForegroundColor(Color::Black),
        )?;

        let cell_name = formula::cell_name(app.cursor_col, app.cursor_row);

        let content = match app.mode {
            Mode::Edit => {
                // Insert ▏ at the text cursor position
                let chars: Vec<char> = app.input_buffer.chars().collect();
                let pos = app.edit_cursor_pos.min(chars.len());
                let left: String = chars[..pos].iter().collect();
                let right: String = chars[pos..].iter().collect();
                format!(" {} | fx: {}▏{} ", cell_name, left, right)
            }
            _ => {
                let cell = app.sheet.get_cell(app.cursor_col, app.cursor_row);
                let suffix = match &cell.value {
                    CellValue::Formula(_) => {
                        let evaluated = app.sheet.evaluate(app.cursor_col, app.cursor_row);
                        format!("{} → {}", cell.raw_input, evaluated)
                    }
                    _ => cell.raw_input.clone(),
                };
                if app.has_selection() {
                    let (min_c, min_r, max_c, max_r) = app.get_selection_bounds();
                    let start = formula::cell_name(min_c, min_r);
                    let end = formula::cell_name(max_c, max_r);
                    let cols = max_c - min_c + 1;
                    let rows = max_r - min_r + 1;
                    format!(" {} | 選択 {}:{} ({}×{}) | fx: {} ", cell_name, start, end, cols, rows, suffix)
                } else {
                    format!(" {} | fx: {} ", cell_name, suffix)
                }
            }
        };

        let display = pad_to_width(&content, term_width as usize, false);
        write!(stdout, "{}", display)?;
        queue!(stdout, ResetColor)?;
        Ok(())
    }

    fn draw_status_bar(stdout: &mut std::io::Stdout, app: &App, term_height: u16, term_width: u16) -> Result<()> {
        queue!(
            stdout,
            MoveTo(0, term_height - 1),
            SetBackgroundColor(Color::Black),
            SetForegroundColor(GREEN),
        )?;

        let mode_str = match app.mode {
            Mode::Normal => "通常",
            Mode::Edit => "編集",
            Mode::Menu => "メニュー",
            Mode::Dialog => "入力",
            Mode::ContextMenu => "コンテキスト",
        };
        let file_str = app.current_file.as_deref().unwrap_or("[新規]");

        let left = if !app.status_message.is_empty() {
            format!(" {} ", app.status_message)
        } else {
            String::new()
        };
        let right = format!(" {} | {} | F10:メニュー ", mode_str, file_str);

        let left_w = display_width(&left);
        let right_w = display_width(&right);
        let padding = (term_width as usize).saturating_sub(left_w + right_w);
        write!(stdout, "{}{:width$}{}", left, "", right, width = padding)?;
        queue!(stdout, ResetColor)?;
        Ok(())
    }
}
