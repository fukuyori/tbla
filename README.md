# tbla

A terminal spreadsheet editor with standard keyboard and mouse operation.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Latest Release](https://img.shields.io/github/v/release/fukuyori/tbla)](https://github.com/fukuyori/tbla/releases/latest)

[日本語](README_ja.md)

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
- **Formula engine** — 35+ functions (SUM, VLOOKUP, IF, etc.)
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
| `Tab` / `Shift+Tab` | Move right / left |
| `Enter` / `Shift+Enter` | Move down / up |
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

## Menu Bar

- **File**: New, Open, Save, Save As, CSV Import/Export, Quit
- **Edit**: Undo, Redo, Cut, Copy, Paste, Clear, Select All, Find, Find Next, Go To
- **Insert**: Insert Row/Column, Delete Row/Column
- **Format**: Auto-fit column width, Widen column, Narrow column
- **Help**: Key reference, About

## Supported Functions

### Math & Statistics
`SUM`, `AVERAGE`, `COUNT`, `COUNTA`, `MIN`, `MAX`, `ABS`, `ROUND`, `INT`, `MOD`, `POWER`, `SQRT`

### Conditional
`IF`, `SUMIF`, `COUNTIF`, `AVERAGEIF`, `IFERROR`

### Lookup
`VLOOKUP`, `HLOOKUP`, `INDEX`, `MATCH`

### Text
`LEFT`, `RIGHT`, `MID`, `LEN`, `TRIM`, `UPPER`, `LOWER`, `CONCATENATE`

### Logical
`AND`, `OR`, `NOT`

### Information
`ISBLANK`, `ISNUMBER`, `ISTEXT`

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
