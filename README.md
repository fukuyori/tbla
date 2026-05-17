# tbla

A terminal spreadsheet editor with standard keyboard and mouse operation.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Latest Release](https://img.shields.io/github/v/release/fukuyori/tbla)](https://github.com/fukuyori/tbla/releases/latest)

[日本語](README_ja.md)

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
- **File formats** — JSON (native), CSV/TSV import/export
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
| `Ctrl+H` / `Ctrl+J` / `Ctrl+K` / `Ctrl+L` | Left / Down / Up / Right (home-row navigation) |
| `Ctrl+Shift+H/J/K/L` | Same, extending the selection |
| `Tab` / `Shift+Tab` | Move right / left |
| `Enter` / `Shift+Enter` | Move down / up |

### macOS without dedicated Home/End keys

On Mac keyboards that lack Home/End, use **`Fn+arrow`** — the OS converts
these to Home/End/PgUp/PgDn at the keyboard level, so no extra setup is
needed.

| Press | Sent as | Action |
|-------|---------|--------|
| `Fn+←` | Home | Beginning of row |
| `Fn+→` | End | End of row (last data column) |
| `Fn+↑` | PgUp | Page up |
| `Fn+↓` | PgDn | Page down |

For A1 / last data cell, use `Ctrl+Home` / `Ctrl+End` (i.e. `Fn+Ctrl+←` /
`Fn+Ctrl+→` on Mac).

### `Shift+↑/↓` not working in macOS Terminal.app

The default Terminal.app profile drops the SHIFT modifier on `Shift+↑` /
`Shift+↓`, so vertical range extension via Shift+Arrow doesn't reach the
application (left/right work fine). Pick one workaround:

- **Alternate keys**: `Ctrl+Shift+K` (extend up) / `Ctrl+Shift+J` (extend down)
- **Patch Terminal.app**: Settings → Profiles → *your profile* → Keyboard tab,
  add via `+`:
  - Key `↑`, Modifier `Shift`, Action `Send Text`, Value `\033[1;2A`
  - Key `↓`, Modifier `Shift`, Action `Send Text`, Value `\033[1;2B`
- **Use a different terminal**: iTerm2 / WezTerm / Alacritty / kitty all
  handle `Shift+Arrow` correctly out of the box.

| `Home` | Beginning of row |
| `End` | Last cell with data in row |
| `Ctrl+Home` | Go to A1 |
| `Ctrl+End` | Last cell with data |
| `Ctrl+↑` `↓` `←` `→` | Jump to next data edge |
| `Page Up` / `Page Down` | Scroll one page |
| `Shift+arrow` | Extend selection |
| `Ctrl+A` | Select all |

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

> On macOS Terminal.app and iTerm, `F1`–`F12` are often captured by the OS as
> media keys. Press `Fn+F2`, change the terminal's function-key setting, or
> double-click a cell instead.

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

### Search

| Key | Action |
|-----|--------|
| `Ctrl+F` | Find |
| `F3` | Find next |
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
| Right click | Context menu (cut/copy/paste/insert/delete) |
| Click on menu bar | Open that menu |
| Drag the `│` separator in the column header | Resize column width (in macOS Terminal.app hold ⌥ while dragging) |

## Menu Bar

- **File**: New, Open, Save, Save As, CSV Import/Export, Quit
- **Edit**: Undo, Redo, Cut, Copy, Paste, Clear, Select All, Find, Find Next, Go To
- **Insert**: Insert Row/Column, Delete Row/Column
- **Format**: Auto-fit column width, Widen column, Narrow column
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

## License

MIT License. See [LICENSE](LICENSE) for details.

## Author

[@fukuyori](https://github.com/fukuyori)
