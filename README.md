# tbla

A terminal spreadsheet editor with standard keyboard and mouse operation.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Latest Release](https://img.shields.io/github/v/release/fukuyori/tbla)](https://github.com/fukuyori/tbla/releases/latest)

[日本語](README_ja.md) ｜ [📖 詳細ガイド (日本語)](GUIDE_ja.md)

![tbla screenshot](images/screenshot.png)

## Overview

tbla is a terminal-based spreadsheet editor. It uses familiar keyboard
shortcuts and mouse operation (click, drag, scroll, right-click menus), with a
menu bar at the top of the screen for file and edit operations.

## Features

- **Standard navigation** — Arrow keys, Tab, Enter, Page Up/Down, Ctrl+Home/End
- **Mouse support** — Click to select, drag for range selection, scroll wheel,
  right-click context menu
- **Menu bar** — File / Edit / Insert / Format / Help (press F10 or click)
- **Excel-style point mode** — Pick cell references with arrows / mouse while
  editing a formula
- **Formula engine** — 70+ functions (SUM, VLOOKUP, IF, date, financial, trig, stats, etc.)
- **Absolute/Relative references** — $A$1, $A1, A$1, A1
- **Formula adjustment** — References auto-update on row/column insert/delete
- **Copy & paste** — Ctrl+C / Ctrl+X / Ctrl+V (also writes to system clipboard)
- **Undo/Redo** — Ctrl+Z / Ctrl+Y
- **File formats** — JSON (native), CSV/TSV, **Excel (.xlsx) read/write**
- **Unicode / IME support** — Proper handling of CJK characters and IME composition

## Installation

### From Binary

Download the latest release from
[GitHub Releases](https://github.com/fukuyori/tbla/releases/latest).

### From Source

```bash
git clone https://github.com/fukuyori/tbla.git
cd tbla
cargo build --release
```

The binary will be at `target/release/tbla`.

## Quick Start

```bash
# Start with empty sheet
tbla

# Open existing file
tbla data.json

# Open CSV file
tbla data.csv
```

## Key Bindings

### Navigation

| Key | Action |
|-----|--------|
| `↑` `↓` `←` `→` | Move cursor |
| `Tab` / `Shift+Tab` | Move right / left |
| `Enter` / `Shift+Enter` | Move down / up |
| `Home` / `End` | Beginning / end of row (last cell with data) |
| `Page Up` / `Page Down` | One page up / down |
| `Ctrl+Home` | Go to A1 |
| `Ctrl+End` | Go to the last cell with data |
| `Ctrl+↑` `↓` `←` `→` | Jump to next data edge |
| `Ctrl+PgUp` / `Ctrl+PgDn` | Previous / next sheet |
| `Shift+arrow` | Extend selection |
| `Ctrl+A` | Select all |
| `Ctrl+H` / `Ctrl+J` / `Ctrl+K` / `Ctrl+L` | Left / Down / Up / Right (vim-style home-row) |
| `Ctrl+Shift+H/J/K/L` | Same, extending the selection |

### Editing

| Key | Action |
|-----|--------|
| Any printable key | Start editing (overwrite cell) |
| `F2` | Edit current cell (preserve content) |
| Double-click on cell | Edit current cell (preserve content) |
| `Enter` / `Tab` | Commit and move to next cell |
| `↑` / `↓` | Commit and move up/down |
| `Esc` | Cancel edit |
| `Delete` / `Backspace` | Clear cell/selection (normal) or delete char (editing) |

### Text cursor inside the edit buffer

| Key | Action |
|-----|--------|
| `←` / `→` | Move one character left/right (or enter point mode in a formula) |
| `Home` / `End` | Beginning / end of input |
| `Ctrl+B` / `Ctrl+F` | Move one character left / right |
| `Ctrl+A` / `Ctrl+E` | Beginning / end of input |
| `Backspace` | Delete character before cursor |
| `Delete` / `Ctrl+D` | Delete character at cursor |
| `Ctrl+K` | Delete from cursor to end of line |

### Aggregate-formula auto-completion

When you commit a bare `=SUM`, `=AVG` / `=AVERAGE`, `=MIN`, `=MAX`, `=COUNT`,
or `=COUNTA` (with optional empty parens) by pressing Enter, the range
argument is filled in automatically from the adjacent numeric data.

| Input | Adjacent data | Completed |
|-------|---------------|-----------|
| `=sum` | A1:A3 numeric, cursor at A4 | `=SUM(A1:A3)` |
| `=avg` | same | `=AVERAGE(A1:A3)` |
| `=max()` | B5:D5 numeric, cursor at E5 (nothing above) | `=MAX(B5:D5)` |
| `=min` | no adjacent data | left as-is |

**Detection rules:**
1. **Up first**: if the cell directly above is numeric, extend up while the run stays numeric.
2. **Then left**: if the cell directly to the left is numeric, extend left.
3. **Up wins** when both directions have data.
4. The run stops at any text or empty cell.
5. A single-cell run is written as `=SUM(A1)`; multi-cell as `=SUM(A1:A3)`.
6. Formula cells that evaluate to a number count as numeric.

### Formula reference selection (point mode)

Excel-style: while editing a formula, whenever the text cursor sits right
after `=`, `(`, `,`, or an operator (`+` `-` `*` `/` `^` `&` `:` `<` `>`),
arrow keys and the mouse can pick a cell reference directly.

| Action | Behavior |
|--------|----------|
| `←` `→` `↑` `↓` | Move the reference cell by one (text updates live) |
| `Shift+arrow` | Extend the referenced range (e.g. `A1` → `A1:A3`) |
| Click on a cell | Insert that cell as the reference |
| Drag across cells | Insert the dragged range |
| Type any character | Exit point mode, keep the inserted reference |
| `Esc` | Exit point mode (press again to cancel the edit) |
| `Enter` / `Tab` | Commit the edit |

While in point mode, the referenced cell is highlighted **blue** (and the
range in darker blue).

### File operations

| Key | Action |
|-----|--------|
| `Ctrl+N` | New sheet |
| `Ctrl+O` | Open file |
| `Ctrl+S` | Save |
| `Ctrl+Q` | Quit |

### Clipboard

| Key | Action |
|-----|--------|
| `Ctrl+C` | Copy |
| `Ctrl+X` | Cut |
| `Ctrl+V` | Paste |

### Search / Replace

| Key | Action |
|-----|--------|
| `Ctrl+F` | Find |
| `F3` | Find next |
| `Ctrl+R` | Replace (Tab cycles find / replace fields) |
| `Ctrl+G` | Go to cell |

### Menu

| Key | Action |
|-----|--------|
| `F10` | Open menu bar |
| `Alt+F` / `Alt+E` / ... | Open the menu by mnemonic |

## Mouse Operation

| Action | Behavior |
|--------|----------|
| Left click on cell | Move cursor |
| Drag from cell | Range selection |
| Mouse wheel | Scroll up/down |
| Right click | Context menu (cut/copy/paste/insert/delete/column width) |
| Click on menu bar | Open that menu |
| Drag the `│` separator in the column header | Resize column width (in macOS Terminal.app hold ⌥ while dragging) |

## Platform notes

### macOS — Macs without Home/End/PgUp/PgDn keys

On a Magic Keyboard etc., use **`Fn+arrow`**. The OS converts these to the
standard keys before tbla sees them — no extra setup.

| Press | Sent as | Action |
|-------|---------|--------|
| `Fn+←` | Home | Beginning of row |
| `Fn+→` | End | End of row |
| `Fn+↑` | PgUp | Page up |
| `Fn+↓` | PgDn | Page down |
| `Fn+Ctrl+←` | Ctrl+Home | A1 |
| `Fn+Ctrl+→` | Ctrl+End | Last data cell |

### macOS — Terminal.app input quirks

The default macOS Terminal.app has two known limitations (iTerm2 / WezTerm
/ Alacritty / kitty all work fine).

**1. `Shift+↑/↓` drops the SHIFT modifier** (left/right work)

Vertical Shift-arrow selection won't extend. Pick one workaround:

- **Alternate keys**: `Ctrl+Shift+K` (extend up) / `Ctrl+Shift+J` (extend down)
- **Patch Terminal.app**: Settings → Profiles → Keyboard, click `+`
  - Key `↑`, Modifier `Shift`, Action `Send Text`, Value `\033[1;2A`
  - Key `↓`, Modifier `Shift`, Action `Send Text`, Value `\033[1;2B`
- **Switch terminal** — anything modern works.

**2. Mouse clicks / drags don't reach the application** (only Moved arrives)

Terminal.app intercepts mouse buttons for its own text selection. Hold
**⌥ (Option)** while clicking / dragging to force the events through —
required for column-width drag, range-drag-selection, etc.

### macOS — `F1`–`F12` are media keys

If `F2` (start editing) doesn't work, your function keys are being eaten
by the OS as media keys. Either press **`Fn+F2`**, enable System Settings
→ Keyboard → "Use F1, F2, etc. keys as standard function keys", or just
double-click the cell to start editing.

## Menu Bar

- **File**: New, Open, Save, Save As, CSV Import/Export, Print (HTML), Quit
- **Edit**: Undo, Redo, Cut, Copy, Paste, Clear, Select All, Find, Find Next, Go To
- **Insert**: Insert Row/Column, Delete Row/Column
- **Sheet**: New sheet, Rename, Delete, Next sheet, Previous sheet
- **Data**: Sort…, Filter…, Clear filter
- **Format**: Auto-fit / Widen / Narrow / Set column width, Bold toggle, Left/Center/Right align, Text color, Background color, Number format…, Clear formatting, Conditional formatting…
- **Help**: Key reference, About

## Supported Functions

### Aggregate
`SUM`, `AVERAGE` (= `AVG`), `COUNT`, `COUNTA`, `MIN`, `MAX`

### Math
`ABS`, `ROUND`, `ROUNDUP`, `ROUNDDOWN`, `CEILING`, `FLOOR`, `INT`, `MOD`, `POWER`, `SQRT`

### Trigonometry / angle
`SIN`, `COS`, `TAN`, `ASIN`, `ACOS`, `ATAN`, `ATAN2`, `RADIANS`, `DEGREES`

### Logarithm / exponent
`LN`, `LOG`, `LOG10`, `EXP`, `PI`

### Statistics
`STDEV` (= `STDEV.S`), `VAR` (= `VAR.S`), `MEDIAN`, `MODE`

### Random / multiples
`RAND`, `RANDBETWEEN`, `GCD`, `LCM`, `FACT`

### Conditional aggregate
`IF`, `SUMIF`, `COUNTIF`, `AVERAGEIF`, `SUMIFS`, `COUNTIFS`, `AVERAGEIFS`, `IFERROR`

### Date / time
`TODAY`, `NOW`, `DATE`, `YEAR`, `MONTH`, `DAY`, `HOUR`, `MINUTE`, `SECOND`, `TIME`,
`WEEKDAY`, `WEEKNUM`, `DATEDIF`, `EDATE`, `EOMONTH`, `DAYS`

Dates are stored as serial numbers (days since 1899-12-30). `=DATE(2024, 1, 1)`
returns a serial value that can be decomposed with `=YEAR(A1)` etc. The
fractional part of `NOW()` is the time component.

### Financial
`PMT`, `PV`, `FV`, `RATE`, `NPER`, `NPV`, `IRR`

Follows Excel cash-flow sign convention: money received = positive, money paid
= negative. Example: $100,000 mortgage at 5% APR, monthly compounding, 30
years → `=PMT(0.05/12, 360, 100000)` ≈ -536.82.

### Lookup
`VLOOKUP`, `HLOOKUP`, `INDEX`, `MATCH`

### Text
`LEFT`, `RIGHT`, `MID`, `LEN`, `TRIM`, `UPPER`, `LOWER`, `CONCATENATE` (= `CONCAT`)

### Logical
`AND`, `OR`, `NOT`

### Information
`ISBLANK`, `ISNUMBER`, `ISTEXT`

## Data Operations

### Sort

`Data → Sort…` opens a three-field dialog:

| Field | Value |
|-------|-------|
| Sort column | Column letter (defaults to cursor column) |
| Order | `asc` (default) or `desc` |
| Header row | `y` to pin the first row (default), `n` to sort everything |

Numbers sort numerically; strings sort case-insensitively; empty cells go
last in ascending order. **Caveat**: formulas inside the sort range are
*compared* by their value but their references are NOT rewritten when rows
move. Same-row relative references (e.g. `=A1+B1`) stay correct;
absolute-row references can break.

### Filter

`Data → Filter…` hides rows whose value in the filter column doesn't match.

| Syntax | Example |
|--------|---------|
| Equality | `100` or `=fruit` |
| Comparison | `>10` `<=100` `<>0` |
| Substring | `*example*` (wildcards on both sides) |

`Data → Clear filter` shows all rows again. **Filter state is session-only**
— it's automatically cleared when you save the file.

### DataFrame view (experimental)

`Data → Convert to DataFrame view` reinterprets the sheet as a typed
Polars `DataFrame`, suited to large analytical data.

| Action | Effect |
|--------|--------|
| Data → Convert to DataFrame view | Row 0 = headers, columns auto-typed (Int64 / Float64 / Boolean / Utf8) |
| Data → Back to cell view | Restore the cell view (cells are preserved underneath, so this is lossless) |

- **Editable**: row 0 edits rename the column; data-row edits update the
  typed value (with auto-widening to Utf8 if the new text doesn't parse).
- Status bar reports row/col counts and the column-type digest, e.g.
  `DF 1000×8 [Int64, Utf8, Float64, …]`.
- Header row is rendered bold and centered.

**Computed columns**: `Data → Add computed column…` lets you add a
derived column. Examples:

| Column | Expression |
|--------|-----------|
| `revenue` | `price * qty` |
| `tax` | `revenue * 0.1` (can reference earlier computed columns) |
| `grade` | `CASE WHEN score >= 80 THEN 'A' ELSE 'B' END` |

Expressions are evaluated by Polars's SQL engine, so `CASE WHEN`,
arithmetic, and built-in functions (`ROUND`, `COALESCE`, …) are all
available. `Data → Clear computed columns` resets the view.

**Direct I/O**:
- **`.parquet`** files open as a DataFrame view via `File → Open` (or
  CLI arg) and save via `File → Save As`. Compressed with Snappy —
  typical numeric data is ~10× smaller than CSV.
- **`File → Open CSV as DataFrame…`** uses Polars' fast columnar CSV
  reader for large files (10 MB / millions of rows) that would choke
  the cell-based import path.

**Analytical operations** (DataFrame view only):

| Menu | What it does | Example |
|------|-------------|---------|
| Data → SQL query… | Run any Polars SQL against the `df` table | `SELECT * FROM df WHERE price > 100` |
| Data → Group aggregate… | Builds GROUP BY SQL behind the scenes | Group: `category` / Agg: `amount:sum, score:avg` |

Supported aggregations: `sum / avg / min / max / count / stddev / var`.
The SQL result replaces the current DataFrame; Ctrl+Z restores the
previous view.

Planned next: pivot, join across DataFrames.

### Multi-sheet workbook

| Action | How |
|--------|-----|
| `Ctrl+PgDn` / `Ctrl+PgUp` | Next / previous sheet |
| Click a tab at the bottom | Switch to that sheet |
| `Sheet → New sheet` | Insert a new sheet after the active one |
| `Sheet → Rename…` | Rename the active sheet |
| `Sheet → Delete sheet` | Delete the active sheet (last sheet protected) |

**Cross-sheet references** in formulas:

```
=Sheet2!A1          # single cell in another sheet
=SUM(Sheet2!A1:A10) # aggregate a foreign range
='Sales 2024'!B5    # quote names that contain spaces
```

Implementation limit: exactly **one level** of indirection is supported.
A foreign cell can be a literal value or a same-sheet formula, but a
foreign-sheet formula that itself references a third sheet returns `#REF!`.
Same-sheet formulas can still nest as deeply as you like.

## Cell & conditional formatting

### Cell formatting (manual)

Applied to the current selection (or the active cell if no selection):

| Action | Effect |
|--------|--------|
| `Ctrl+B` | Toggle bold |
| Format → Left / Center / Right align | Override the auto numeric-right / text-left default |
| Format → Text color / Background color | RGB dialog (`255,200,200`, `#fee`, `#ffeedd`; empty input clears) |
| Format → Number format… | Choose general / number / currency / percent / scientific / date / text + decimals |
| Format → Clear formatting | Reset alignment, bold, colors, number format to defaults |

Formatting **survives value edits** — replacing `100` with `200` keeps the
cell's bold/colors/alignment/number format.

### Conditional formatting

Rules attached to a sheet are evaluated at render time. Add via
`Format → Conditional formatting…` (three fields):

| Field | Example |
|-------|---------|
| Range | `B2:B100`, or a single cell like `A1` |
| Condition | `>100` / `<=0` / `=42` / `<>0` or `scale:0-100` |
| Background color | `255,200,200` or `#fee` |

**Color scale**: write `scale:min-max,minR,minG,minB,maxR,maxG,maxB` — for
example `scale:0-100,255,255,255,220,50,50` for white → red. Colors are
interpolated between the two endpoints. Without colors specified, a default
light-to-red gradient is used.

`Format → Clear conditional formatting` removes every rule on the active
sheet.

### Persistence

- **Native JSON**: cell formatting (colors, bold, alignment, number format)
  and per-sheet conditional formats round-trip cleanly.
- **xlsx write**: cell formatting is emitted via `rust_xlsxwriter::Format`;
  conditional formatting via `ConditionalFormatCell` /
  `ConditionalFormat2ColorScale` so the file opens with the same styling
  in Excel / LibreOffice.
- **xlsx read**: background color, font color, horizontal alignment, and
  bold are imported by hand-parsing `xl/styles.xml`. **Conditional
  formatting is also imported**: `cellIs` comparisons (resolved against
  `<dxfs>` for colors), `colorScale` (2- or 3-color), and `dataBar`
  (rendered in tbla as a horizontal bar overlay). Excel files made by
  others open with colors and conditional rules intact. Borders, italic,
  underline, font size, and theme/indexed colors are not yet imported.

## Printing

tbla doesn't print directly from the terminal — instead it **exports a
print-friendly HTML file and opens it in your default browser**. You then
use the browser's print dialog (Cmd/Ctrl+P) for margins, page numbers, PDF
output, scaling, and so on.

- `Ctrl+P` or `File` → `Print (HTML)...`
- Enter the output filename (defaults to `<sheet>.html`) → Enter
- The file opens automatically in your default browser (macOS `open`,
  Linux `xdg-open`, Windows `start`)
- If auto-open isn't available on your system, just drop the generated
  HTML onto a browser window

Styling:
- Column letters (A, B, C…) and row numbers repeat on every printed page
- Numbers right-aligned, text left-aligned, errors in red
- Inline CSS — no external dependencies, works offline

## Calculation Conventions

### Floating-point comparison (relative tolerance)

Numeric `=`, `<>`, `>`, `<`, `>=`, `<=`, the criteria of `SUMIF` /
`COUNTIF` / `*IFS`, and exact-match `VLOOKUP` / `MATCH` all use a
**~15-significant-digit relative tolerance** instead of raw IEEE 754
equality.

Examples:
```
=(0.1+0.2)=0.3          → TRUE   (raw f64 is 0.30000000000000004)
=(0.1+0.2)>=0.3         → TRUE   (boundary counts as equal)
=(0.1+0.2)>0.3          → FALSE  (strict > excludes the equal band)
=COUNTIF(A1, ">=0.3")   → 1      (when A1 = `=0.1+0.2`)
```

The tolerance is `max(|a|, |b|, 1.0) * 1e-12` — it scales with magnitude.
You get roughly 3 digits of headroom for accumulated rounding, so even
deeply nested formulas stay stable.

### Date serial numbers (Power BI convention)

`DATE`, `TODAY`, `NOW`, etc. return serial values where **1899-12-30 = 0**.

| Date       | Serial | vs Excel               |
|------------|--------|------------------------|
| 1899-12-30 | 0      | n/a                    |
| 1900-01-01 | 2      | Excel: 1 (off by 1)    |
| 1900-02-28 | 60     | Excel: 59 (off by 1)   |
| 1900-03-01 | 61     | **matches Excel**       |
| 2024-01-01 | 45292  | **matches Excel**       |

Dates from 1900-03-01 onward match Excel exactly. January / February 1900
are 1 lower because Excel pretends 1900-02-29 existed; tbla follows the
Power BI convention and **fixes this 1900 leap-year bug** (pure Gregorian).
`WEEKDAY` is correct year-round.

## File Formats

### Native Format (JSON)

tbla uses JSON as its native format, storing:
- Cell values and formulas
- Column widths
- Sheet name

```json
{
  "version": "1.0",
  "name": "Sheet1",
  "cells": {
    "A1": "Hello",
    "B1": "=SUM(A2:A10)"
  },
  "col_widths": {
    "A": 15
  }
}
```

### CSV/TSV

- Import via File → CSV Import (or `Alt+F`, `I`)
- Export via File → CSV Export (or `Alt+F`, `E`)
- System clipboard uses TSV format

### Excel (.xlsx)

`File → Open / Save As` auto-detects `.xlsx` / `.xlsm` and reads / writes
in Excel format.

**Read**:
- Cell values (string / number / boolean / error)
- Formulas (e.g. `=SUM(...)` preserved and re-evaluated by tbla's engine)
- Multi-sheet workbooks: **only the first sheet** is loaded; the names of
  the others are surfaced as a status-bar warning

**Write**:
- Values and formulas (formulas written as `=SUM(...)` so Excel recalculates
  on open)
- Column widths
- tbla's last computed value is embedded as the cached result, so viewers
  that don't recompute still see the right number

**Fallback**: formulas using Excel functions tbla doesn't implement
(e.g. `BITAND`) keep working — Excel's last-saved value is used for display
and aggregation. Editing the cell clears the override.

## License

MIT License. See [LICENSE](LICENSE) for details.

## Author

[@fukuyori](https://github.com/fukuyori)
