use crossterm::{
    cursor::{Hide, MoveTo, Show},
    queue,
    style::{Attribute, Color, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};
use std::cell::Cell;
use std::io::{stdout, BufWriter, Result, Write};
use unicode_width::UnicodeWidthStr;

use crate::{App, Mode};
use crate::cell::{Alignment, CellValue, RgbColor};
use crate::formula;
use crate::menu::{MenuBar, MenuState, SubItem, ContextMenu};

fn rgb_to_color(rgb: RgbColor) -> Color {
    Color::Rgb { r: rgb.0, g: rgb.1, b: rgb.2 }
}

const ROW_LABEL_WIDTH: usize = 5;

/// Synchronized Output (DEC mode 2026) begin/end sequences.
///
/// Detection priority:
/// - `$WTMUX` (fukuyori/wtmux on Windows): does not support mode 2026 and runs
///   on ConPTY where the sequence would just be ignored. We emit empty strings
///   so we don't add per-frame noise.
/// - `$TMUX`: wrap in DCS passthrough (`\ePtmux;\e…\e\\`) so the sequence
///   reaches the outer terminal even on tmux that does not handle 2026 natively.
/// - Otherwise: bare sequence.
fn sync_sequences() -> (&'static str, &'static str) {
    use std::sync::OnceLock;
    static SEQS: OnceLock<(&'static str, &'static str)> = OnceLock::new();
    *SEQS.get_or_init(|| {
        if std::env::var_os("WTMUX").is_some() {
            ("", "")
        } else if std::env::var_os("TMUX").is_some() {
            ("\x1bPtmux;\x1b\x1b[?2026h\x1b\\", "\x1bPtmux;\x1b\x1b[?2026l\x1b\\")
        } else {
            ("\x1b[?2026h", "\x1b[?2026l")
        }
    })
}

// Tracks whether the terminal cursor is currently shown, so we only emit
// Show/Hide commands when the desired state actually changes. Toggling the
// cursor on every frame causes visible blink/flicker through ConPTY-based
// multiplexers like wtmux.
thread_local! {
    /// Last (fg, bg) emitted by `set_colors`. `None` means "unknown / reset".
    static LAST_COLORS: Cell<(Option<Color>, Option<Color>)> = const { Cell::new((None, None)) };
}

/// Emit `SetForegroundColor` / `SetBackgroundColor` only when the value
/// actually changes from the previous call. Cuts down on the per-cell color
/// toggle traffic in the grid hot path, which dominates per-frame writes.
fn set_colors<W: Write>(stdout: &mut W, fg: Color, bg: Color) -> Result<()> {
    let (last_fg, last_bg) = LAST_COLORS.with(|c| c.get());
    let mut new_fg = last_fg;
    let mut new_bg = last_bg;
    if last_fg != Some(fg) {
        queue!(stdout, SetForegroundColor(fg))?;
        new_fg = Some(fg);
    }
    if last_bg != Some(bg) {
        queue!(stdout, SetBackgroundColor(bg))?;
        new_bg = Some(bg);
    }
    LAST_COLORS.with(|c| c.set((new_fg, new_bg)));
    Ok(())
}

/// Like `set_colors` but only sets the background; foreground is left alone.
fn set_bg<W: Write>(stdout: &mut W, bg: Color) -> Result<()> {
    let (last_fg, last_bg) = LAST_COLORS.with(|c| c.get());
    if last_bg != Some(bg) {
        queue!(stdout, SetBackgroundColor(bg))?;
        LAST_COLORS.with(|c| c.set((last_fg, Some(bg))));
    }
    Ok(())
}

fn reset_colors<W: Write>(stdout: &mut W) -> Result<()> {
    queue!(stdout, ResetColor)?;
    LAST_COLORS.with(|c| c.set((None, None)));
    Ok(())
}

/// Discard any cached color state. Call when stale state could have leaked in
/// (e.g., right after entering the alternate screen, after Clear). Currently
/// called once per `draw()` to keep the implementation conservative.
fn invalidate_color_cache() {
    LAST_COLORS.with(|c| c.set((None, None)));
}

fn set_cursor_visible<W: Write>(stdout: &mut W, want_visible: bool) -> Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    static CURSOR_VISIBLE: AtomicBool = AtomicBool::new(false);
    let cur = CURSOR_VISIBLE.load(Ordering::Relaxed);
    if cur != want_visible {
        if want_visible {
            queue!(stdout, Show)?;
        } else {
            queue!(stdout, Hide)?;
        }
        CURSOR_VISIBLE.store(want_visible, Ordering::Relaxed);
    }
    Ok(())
}

// Colors. All theme-independent (truecolor RGB) so tbla looks the same
// across every terminal — `BLACK` / `WHITE` are deliberately
// avoided because terminals like macOS Terminal.app remap "white" to
// `#bbbbbb`, making text washed out and selection contrast unreliable.
const BLACK: Color = Color::Rgb { r: 0, g: 0, b: 0 };
const WHITE: Color = Color::Rgb { r: 230, g: 230, b: 230 };
const DARK_GREY: Color = Color::Rgb { r: 120, g: 120, b: 120 };
const GREEN: Color = Color::Rgb { r: 0, g: 170, b: 0 };
const ORANGE: Color = Color::Rgb { r: 255, g: 136, b: 0 };
const MENU_BG: Color = Color::Rgb { r: 220, g: 220, b: 220 };
const MENU_FG: Color = BLACK;
const MENU_SEL_BG: Color = Color::Rgb { r: 0, g: 100, b: 200 };
const MENU_SEL_FG: Color = WHITE;
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

fn center_to_width(s: &str, target_width: usize) -> String {
    let current = UnicodeWidthStr::width(s);
    if current >= target_width {
        return truncate_to_width(s, target_width);
    }
    let total_pad = target_width - current;
    let left = total_pad / 2;
    let right = total_pad - left;
    format!("{}{}{}", " ".repeat(left), s, " ".repeat(right))
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
        // Buffer the whole frame into a single write — far fewer syscalls
        // means less observable "drawing" through ConPTY / wtmux.
        let raw = stdout();
        let mut stdout = BufWriter::with_capacity(64 * 1024, raw.lock());
        // Color cache is only valid within a single sequence of writes; clear
        // it conservatively at the top of each frame.
        invalidate_color_cache();
        let (term_width, term_height) = terminal::size()?;
        // Layout: row 0 = menu bar, row 1 = column headers, grid, [tab bar],
        // formula bar, status bar. Tab bar is only shown when there are 2+
        // sheets so single-sheet workbooks lose no vertical space.
        let tab_height: usize = if app.sheet_count() > 1 { 1 } else { 0 };
        let grid_height = (term_height as usize).saturating_sub(4 + tab_height);
        let visible_cols = Self::calc_visible_cols(app, term_width as usize);

        let cursor_color = Self::cursor_color(app.mode);

        // Begin synchronized update (DEC mode 2026): tells modern terminals to
        // buffer the frame and present it atomically, eliminating tearing/flicker.
        // Empty when running under wtmux (which does not support it).
        let (sync_begin, sync_end) = sync_sequences();
        if !sync_begin.is_empty() {
            write!(stdout, "{}", sync_begin)?;
        }
        queue!(stdout, MoveTo(0, 0))?;

        Self::draw_menu_bar(&mut stdout, app, term_width)?;
        Self::draw_column_headers(&mut stdout, app, &visible_cols, term_width)?;
        Self::draw_grid(&mut stdout, app, grid_height, &visible_cols, term_width, cursor_color)?;
        if tab_height > 0 {
            Self::draw_sheet_tabs(&mut stdout, app, term_height, term_width)?;
        }
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
        // Only toggle visibility when it actually changes — see set_cursor_visible.
        if app.mode == Mode::Edit {
            if let Some((cx, cy)) = Self::editing_cursor_pos(app, &visible_cols) {
                queue!(stdout, MoveTo(cx, cy))?;
                set_cursor_visible(&mut stdout, true)?;
            } else {
                set_cursor_visible(&mut stdout, false)?;
            }
        } else if app.mode == Mode::Dialog {
            // Place the cursor at the end of the focused dialog input.
            if let Some(dialog) = &app.dialog {
                let n = dialog.fields.len();
                let f = &dialog.fields[dialog.focus];
                // Field i (0-based) renders at term_height - (n + 1 - i) so
                // field 0 is on top and the bottom (hint) line stays at -1.
                let line_from_bottom = (n + 1 - dialog.focus) as u16;
                let prefix = format!(" {}: ", f.label);
                let x = display_width(&prefix) + display_width(&f.input);
                queue!(stdout, MoveTo(x as u16, term_height - line_from_bottom))?;
                set_cursor_visible(&mut stdout, true)?;
            } else {
                set_cursor_visible(&mut stdout, false)?;
            }
        } else {
            set_cursor_visible(&mut stdout, false)?;
        }

        // End synchronized update — present the buffered frame atomically.
        if !sync_end.is_empty() {
            write!(stdout, "{}", sync_end)?;
        }
        stdout.flush()?;
        Ok(())
    }

    fn draw_menu_bar(stdout: &mut impl Write, app: &App, term_width: u16) -> Result<()> {
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

    fn draw_open_menu(stdout: &mut impl Write, bar: &MenuBar, state: &MenuState) -> Result<()> {
        let Some(idx) = state.open else { return Ok(()); };
        let positions = bar.bar_positions();
        let (x_start, _) = positions[idx];
        let width = bar.submenu_width(idx);
        let menu = &bar.menus[idx];

        for (i, item) in menu.items.iter().enumerate() {
            queue!(stdout, MoveTo(x_start as u16, (i + 1) as u16))?;
            // In bar navigation (slash menu, not yet descended) the dropdown
            // is a preview: no item is highlighted until the menu is entered.
            let is_sel = state.dropped && i == state.item && !item.is_separator();
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
                SubItem::Item { shortcut, .. } => {
                    let label = item.display_label().unwrap_or_default();
                    let label_w = display_width(&label);
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

    fn draw_context_menu(stdout: &mut impl Write, cm: &ContextMenu) -> Result<()> {
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

    fn draw_dialog(stdout: &mut impl Write, app: &App, term_height: u16, term_width: u16) -> Result<()> {
        let Some(dialog) = &app.dialog else { return Ok(()); };

        // Each field gets its own row above the hint line. Field 0 sits at
        // the top of the dialog area, last field just above the hint.
        let n = dialog.fields.len();
        let multi = n > 1;
        for (i, f) in dialog.fields.iter().enumerate() {
            let line_from_bottom = (n + 1 - i) as u16;
            queue!(
                stdout,
                MoveTo(0, term_height - line_from_bottom),
                SetBackgroundColor(MENU_BG),
                SetForegroundColor(MENU_FG)
            )?;
            let cursor = if i == dialog.focus { "_" } else { " " };
            let trailing = if multi && i == n - 1 {
                "  (Tab で切替, Enter で実行)"
            } else {
                ""
            };
            let prompt = format!(" {}: {}{}{} ", f.label, f.input, cursor, trailing);
            let display = pad_to_width(&prompt, term_width as usize, false);
            write!(stdout, "{}", display)?;
            queue!(stdout, ResetColor)?;
        }

        // Hint line
        queue!(
            stdout,
            MoveTo(0, term_height - 1),
            SetBackgroundColor(BLACK),
            SetForegroundColor(GREEN)
        )?;
        let hint = " Enter: 実行   Esc: キャンセル ".to_string();
        let line = pad_to_width(&hint, term_width as usize, false);
        write!(stdout, "{}", line)?;
        queue!(stdout, ResetColor)?;
        Ok(())
    }

    fn draw_column_headers(stdout: &mut impl Write, _app: &App, visible_cols: &[(usize, usize)], term_width: u16) -> Result<()> {
        queue!(
            stdout,
            MoveTo(0, 1),
            SetBackgroundColor(GREEN),
            SetForegroundColor(BLACK),
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

    fn draw_grid(stdout: &mut impl Write, app: &App, grid_height: usize, visible_cols: &[(usize, usize)], term_width: u16, cursor_color: Color) -> Result<()> {
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

        // Build the list of visible logical rows starting at view_row,
        // skipping rows hidden by an active filter, up to grid_height rows.
        let visible_rows: Vec<usize> = (app.view_row..usize::MAX)
            .filter(|r| !app.hidden_rows.contains(r))
            .take(grid_height)
            .collect();

        for (row_idx, &actual_row) in visible_rows.iter().enumerate() {
            let screen_row = (row_idx + 2) as u16;

            // Start with a fully-cleared line in the default cell background
            // so any residual content (e.g. left over from IME composition or
            // a previous wider render) is wiped.
            queue!(stdout, MoveTo(0, screen_row))?;
            set_bg(stdout, BLACK)?;
            queue!(stdout, Clear(ClearType::CurrentLine))?;

            // Row label
            queue!(stdout, MoveTo(0, screen_row))?;
            set_colors(stdout, BLACK, GREEN)?;
            write!(stdout, "{:>width$}", actual_row + 1, width = ROW_LABEL_WIDTH)?;

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
                // When the sheet is in DataFrame view, override the value /
                // number-ness so the grid reads from the DataFrame. Row 0
                // shows column headers (rendered bold for visual cue).
                let is_df_view = app.sheet.df_view.is_some();
                let is_df_header_row = is_df_view && actual_row == 0;
                let (df_value, df_is_number) = if let Some(view) = app.sheet.df_view.as_ref() {
                    if actual_row == 0 {
                        (Some(view.header(actual_col)), false)
                    } else if actual_row - 1 < view.rows() && actual_col < view.cols() {
                        (Some(view.value_at(actual_col, actual_row - 1)), view.is_numeric(actual_col))
                    } else {
                        (Some(String::new()), false)
                    }
                } else {
                    (None, false)
                };

                let is_number = if is_df_view {
                    df_is_number
                } else {
                    matches!(cell.value, CellValue::Number(_) | CellValue::Formula(_))
                };

                // Compute the value to display and the width it would need.
                // For editing, account for at least one extra column for the
                // block cursor when the text cursor is at the end of input.
                let (value, value_display_width) = if is_editing && !is_df_view {
                    let input = app.input_buffer.clone();
                    let cursor_at_end =
                        app.edit_cursor_pos >= input.chars().count();
                    let extra = if cursor_at_end { 1 } else { 0 };
                    let w = display_width(&input) + extra;
                    (input, w)
                } else if let Some(v) = df_value {
                    let w = display_width(&v);
                    (v, w)
                } else {
                    let v = app.evaluate(actual_col, actual_row);
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

                // Look up any cell-level format overrides, conditional
                // formatting, and decide the base colors. Selection / cursor
                // / point-mode highlights take precedence over user-set
                // formatting so the selected cell always reads as selected.
                let manual_bg = cell.bg_color.map(rgb_to_color);
                let manual_fg = cell.text_color.map(rgb_to_color);
                let cond = {
                    let v = app.sheet.evaluate(actual_col, actual_row);
                    app.sheet.lookup_conditional(actual_col, actual_row, &v)
                };
                let cond_bg = cond.bg_color.map(rgb_to_color);
                let cond_fg = cond.text_color.map(rgb_to_color);
                let user_bg = manual_bg.or(cond_bg);
                let user_fg = manual_fg.or(cond_fg);
                let data_bar = cond.data_bar; // (fraction, rgb)

                let (bg, fg) = if is_cursor {
                    (cursor_color, BLACK)
                } else if is_point_cursor {
                    (POINT_CURSOR_BG, BLACK)
                } else if is_in_point_range {
                    (POINT_RANGE_BG, WHITE)
                } else if is_selected {
                    (SELECTION_BG, WHITE)
                } else {
                    (user_bg.unwrap_or(BLACK), user_fg.unwrap_or(WHITE))
                };

                if is_editing {
                    // Render with a block cursor: the character under the
                    // text cursor is shown with inverted colors. Three
                    // segments are written: left, cursor char, right, then
                    // padding + trailing space to fill the cell.
                    let view = compute_edit_view(&value, app.edit_cursor_pos, content_width);
                    let used_w = view.width();
                    let pad = content_width.saturating_sub(used_w);

                    set_colors(stdout, fg, bg)?;
                    write!(stdout, "{}", view.left)?;

                    // Inverted cursor cell
                    set_colors(stdout, bg, fg)?;
                    write!(stdout, "{}", view.cursor_char)?;

                    // Restore and finish
                    set_colors(stdout, fg, bg)?;
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

                    set_colors(stdout, fg, bg)?;
                    let bold_active = cell.bold || is_df_header_row;
                    if bold_active {
                        queue!(stdout, SetAttribute(Attribute::Bold))?;
                    }

                    // Cell-level alignment overrides the auto right/left
                    // default; explicit Default falls back to the auto rule.
                    // DataFrame headers center for visual cue.
                    let alignment_effective = if is_df_header_row {
                        Alignment::Center
                    } else {
                        cell.alignment
                    };
                    let right_align = match alignment_effective {
                        Alignment::Left => false,
                        Alignment::Right => true,
                        Alignment::Center => false, // handled below
                        Alignment::Default => is_number,
                    };
                    let formatted = if matches!(alignment_effective, Alignment::Center) {
                        center_to_width(&content, content_width)
                    } else {
                        pad_to_width(&content, content_width, right_align)
                    };
                    if let Some((frac, bar_rgb)) = data_bar {
                        // Data-bar overlay: render the first `filled` cells
                        // with bar color background, the rest with normal bg.
                        let bar_color = rgb_to_color(bar_rgb);
                        let filled = (content_width as f64 * frac).round() as usize;
                        let chars: Vec<char> = formatted.chars().collect();
                        for (i, ch) in chars.iter().enumerate() {
                            let target_bg = if i < filled { bar_color } else { bg };
                            set_colors(stdout, fg, target_bg)?;
                            write!(stdout, "{}", ch)?;
                        }
                        set_colors(stdout, fg, bg)?;
                        write!(stdout, " ")?;
                    } else {
                        write!(stdout, "{} ", formatted)?;
                    }
                    if bold_active {
                        queue!(stdout, SetAttribute(Attribute::Reset))?;
                        // Reset attribute also clears colors on some terms;
                        // re-apply so the trailing space stays correct.
                        set_colors(stdout, fg, bg)?;
                    }
                }

                // Note: no ResetColor here — next cell will set its own colors
                // via set_colors(), which makes the cache do useful work across
                // adjacent same-colored cells.
                used += total_width;
                col_idx += 1 + covered_count;
            }

            let remaining = (term_width as usize).saturating_sub(used);
            if remaining > 0 {
                set_bg(stdout, BLACK)?;
                write!(stdout, "{:width$}", "", width = remaining)?;
            }
        }
        // If filter or end-of-sheet means visible_rows has fewer entries than
        // grid_height, blank out the trailing rows so stale content from
        // previous frames doesn't linger.
        for tail in visible_rows.len()..grid_height {
            let screen_row = (tail + 2) as u16;
            queue!(stdout, MoveTo(0, screen_row))?;
            set_bg(stdout, BLACK)?;
            queue!(stdout, Clear(ClearType::CurrentLine))?;
        }
        reset_colors(stdout)?;
        Ok(())
    }

    fn draw_sheet_tabs(stdout: &mut impl Write, app: &App, term_height: u16, term_width: u16) -> Result<()> {
        // Drawn at term_height - 3 (just above formula bar). One line tall.
        queue!(
            stdout,
            MoveTo(0, term_height - 3),
            SetBackgroundColor(BLACK),
            SetForegroundColor(WHITE),
        )?;
        // Sheets are rendered as " name " segments separated by | with the
        // active tab inverted.
        let mut used = 0usize;
        for (idx, sheet) in app.workbook_sheets().iter().enumerate() {
            let label = format!(" {} ", sheet.name);
            let w = display_width(&label);
            if used + w + 1 > term_width as usize {
                // Truncation marker for overflow.
                queue!(stdout, SetBackgroundColor(BLACK), SetForegroundColor(DARK_GREY))?;
                write!(stdout, " …")?;
                used += 2;
                break;
            }
            if idx == app.active_sheet_index {
                queue!(stdout, SetBackgroundColor(WHITE), SetForegroundColor(BLACK))?;
            } else {
                queue!(stdout, SetBackgroundColor(DARK_GREY), SetForegroundColor(WHITE))?;
            }
            write!(stdout, "{}", label)?;
            queue!(stdout, SetBackgroundColor(BLACK))?;
            write!(stdout, " ")?;
            used += w + 1;
        }
        // Fill remaining width with black.
        let remaining = (term_width as usize).saturating_sub(used);
        if remaining > 0 {
            queue!(stdout, SetBackgroundColor(BLACK))?;
            write!(stdout, "{:width$}", "", width = remaining)?;
        }
        queue!(stdout, ResetColor)?;
        Ok(())
    }

    fn draw_formula_bar(stdout: &mut impl Write, app: &App, term_height: u16, term_width: u16) -> Result<()> {
        queue!(
            stdout,
            MoveTo(0, term_height - 2),
            SetBackgroundColor(GREEN),
            SetForegroundColor(BLACK),
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
                        let evaluated = app.evaluate(app.cursor_col, app.cursor_row);
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

    fn draw_status_bar(stdout: &mut impl Write, app: &App, term_height: u16, term_width: u16) -> Result<()> {
        queue!(
            stdout,
            MoveTo(0, term_height - 1),
            SetBackgroundColor(BLACK),
            SetForegroundColor(GREEN),
        )?;

        let mode_str = match app.mode {
            Mode::Normal => "通常",
            Mode::Edit => "編集",
            Mode::Menu => "メニュー",
            Mode::Dialog => "入力",
            Mode::ContextMenu => "コンテキスト",
        };
        let file_str = app.current_file.as_deref()
            .map(|p| std::path::Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(p)
                .to_string())
            .unwrap_or_else(|| "[新規]".to_string());

        let left = if !app.status_message.is_empty() {
            format!(" {} ", app.status_message)
        } else {
            String::new()
        };
        // DataFrame view indicator: shows row/col count and dtype digest.
        let df_str = if let Some(v) = app.sheet.df_view.as_ref() {
            format!(" DF {}×{} [{}] |", v.rows(), v.cols(), v.dtype_summary(4))
        } else {
            String::new()
        };
        let right = format!("{} {} | {} | F10:メニュー ", df_str, mode_str, file_str);

        let term_w = term_width as usize;
        // Right side gets priority — clip it first if even it is too wide.
        let right = truncate_to_width(&right, term_w);
        let right_w = display_width(&right);
        let left_max = term_w.saturating_sub(right_w);
        let left = truncate_to_width(&left, left_max);
        let left_w = display_width(&left);
        let padding = term_w.saturating_sub(left_w + right_w);
        write!(stdout, "{}{:width$}{}", left, "", right, width = padding)?;
        queue!(stdout, ResetColor)?;
        Ok(())
    }
}
