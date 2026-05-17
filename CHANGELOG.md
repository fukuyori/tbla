# Changelog

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
this project adheres to [Semantic Versioning](https://semver.org/).

## [0.3.0] - 2026-05-17

### Added — Cell formatting
- Each cell can carry **alignment** (left/center/right/default), **bold**,
  **text color** (RGB), **background color** (RGB), and a **number format**
  (general / number / currency / percent / scientific / date / text).
- Formatting is **preserved across edits** — overwriting a cell's value
  keeps its bold / colors / alignment / number format.
- Menu actions (書式 menu): 太字切替 (`Ctrl+B`), 左/中央/右揃え, 揃え既定,
  文字色…, 背景色…, 数値書式…, 書式クリア.
- Color dialogs accept `#rrggbb`, `#rgb`, or `r,g,b` decimal forms;
  empty input clears the color.

### Added — Conditional formatting
- **Per-sheet rule list** evaluated at render time.
- **Comparison**: `>100`, `<-5`, `>=0`, `<=100`, `=42`, `<>0` → set
  background color when the cell's value matches.
- **Color scale**: `scale:min-max[,low_color,high_color]` interpolates a
  two-color gradient across the value range.
- **Data bar**: a new `DataBar { min, max, bar_color }` rule that draws
  a horizontal bar inside the cell whose length is proportional to the
  cell's value within the auto- or explicitly-set min/max. Rendered in
  the TUI as a background overlay on the value text.

### Added — Persistence (formatting)
- **JSON native** format v2 carries the new cell-formatting fields and
  per-sheet `conditional_formats` array; old files round-trip unchanged.
- **xlsx write**: cell formatting via `rust_xlsxwriter::Format`,
  conditional formatting (cell rules + 2-color scales + data bars) via
  the `ConditionalFormat*` API so files open with the same styling in
  Excel / LibreOffice.
- **xlsx read**: tbla now imports background color, font color,
  horizontal alignment, and bold by hand-parsing `xl/styles.xml`. New
  module `xlsx_styles.rs` (deps `zip` + `quick-xml`, both already in
  the transitive tree via calamine).
- **xlsx read — conditional formatting**: each sheet's
  `<conditionalFormatting>` is parsed, with `<dxfs>` resolved for
  cell-rule colors. Supported rule types: `cellIs` (mapped to `Compare`),
  `colorScale` (2- or 3-color → 2-color in tbla), and `dataBar`.

### Added — Data operations
- **Find & Replace** (`Ctrl+R` or 編集 → 置換…). Two-field dialog (find /
  replace), Tab cycles focus, case-insensitive substring substitution
  applied to every cell's raw input.
- **Row sort** (データ → 並べ替え…). Three-field dialog: column, direction
  (asc/desc), and whether to keep the first row as a header. Sort treats
  numbers numerically and strings case-insensitively; empty cells sort
  last for ascending.
- **Column filter** (データ → フィルター… / フィルター解除). Hides rows whose
  filter-column value doesn't match the criteria. Criteria use the same
  syntax as `COUNTIF` (`>10`, `<>foo`, bare value) plus `*substring*` for
  contains-match. Filter state is **session-only** — cleared on file save
  by design.

### Added — Multi-sheet workbook
- New **シート(S)** menu: 新規シート / シート名変更 / シート削除 / 次のシート (Ctrl+PgDn)
  / 前のシート (Ctrl+PgUp).
- **Tab bar** appears at the bottom of the grid when the workbook has
  more than one sheet; click a tab to switch.
- **Cross-sheet references**: formulas like `=Sheet2!A1+B1` or
  `=SUM(Sheet2!A1:A10)`. Sheet names are matched case-insensitively;
  missing sheets return `#REF!`. (For simplicity, exactly one level of
  indirection is supported — a foreign cell can be a literal value or a
  same-sheet formula, but not itself cross-sheet.)
- **JSON format v2** with `sheets[]` array; v1 single-sheet files still
  load transparently.
- **`.xlsx` multi-sheet I/O**: all sheets are read/written in workbook
  order on open and save (previously only the first sheet).

### Added — Dialog UX
- Dialog now supports an **arbitrary number of input fields**, with
  Tab / Shift+Tab cycling focus. Existing single-field dialogs continue
  to use the simple `Dialog::single` helper.

## [0.2.2] - 2026-05-17

### Fixed
- **Opening file paths with surrounding quotes** (Windows): paths pasted
  from Explorer's "Copy as path" (which wraps in `"…"`) no longer fail with
  `os error 123 / ERROR_INVALID_NAME`. Dialog input and the CLI argument
  now strip a single matching pair of `"` or `'` via the new
  `commands::sanitize_path_input` helper. A clearer hint is appended to
  the status message when the OS returns `os error 123`.
- **Status bar wrapping**: long file paths in the right-hand status
  segment are now shown as basename only, and the whole line is hard-
  clipped to terminal width — so absolute paths no longer wrap to a
  second line and cause flicker.
- **Color leak in grid (introduced by the cache changes below)**: the row
  label was changing terminal colors via raw `queue!` calls, bypassing
  the new color cache and causing the green row-label background to bleed
  into the adjacent cells on the cursor row. All grid color writes now
  go through the cache helpers.

### Changed — flicker reduction
- **Synchronized output (DEC mode 2026)**: each frame is wrapped in
  `\x1b[?2026h` / `\x1b[?2026l` so modern terminals (Windows Terminal,
  WezTerm, kitty, iTerm2, Alacritty 0.13+, Ghostty) present the buffered
  frame atomically. Inside `tmux`, the sequence is DCS-wrapped
  (`\ePtmux;\e…\e\\`) so it passes through to the host terminal.
  `$WTMUX` is detected and the sequence is skipped (wtmux runs on ConPTY
  and does not understand mode 2026).
- **Cursor visibility caching**: `Hide` / `Show` is only emitted when the
  desired state actually changes from the previous frame. Previously
  every frame queued an unconditional `Hide`, which produced visible
  cursor blink through ConPTY-based multiplexers like wtmux.
- **Buffered frame output**: stdout is wrapped in a 64 KiB `BufWriter`
  inside `UI::draw`, collapsing hundreds of per-cell writes into a
  single syscall on flush.
- **Color-change caching**: new `set_colors` / `set_bg` / `reset_colors`
  helpers in `ui.rs` skip `SetForegroundColor` / `SetBackgroundColor` /
  `ResetColor` when the value would be unchanged. Applied to the grid
  hot path, which dominates per-frame writes. Per-cell `ResetColor` was
  removed; cells inherit color from the previous cell and only
  differences are emitted.

## [0.2.1] - 2026-05-17

### Added — File I/O
- **Excel (.xlsx) read/write**: `File → Open / Save As` auto-detects `.xlsx`
  / `.xlsm` (read also handles `.xlsm`). Formulas are preserved on both
  sides — re-evaluated on open, re-emitted on save. Multi-sheet workbooks
  load only the first sheet with a status-bar warning. Column widths
  round-trip.
- **Cached-value fallback**: imported cells now carry an optional
  `cached_value` (Excel's last-computed result). If tbla's engine can't
  evaluate the formula — e.g. an unsupported Excel function like `BITAND`
  — display and aggregation fall back to the cached value. Editing the
  cell clears the override.
- New deps: `calamine = "0.30"` (read), `rust_xlsxwriter = "0.92"` (write).

### Changed
- **Selection background color** brightened from `RGB(60,60,120)`
  (low-contrast muted purple) to `RGB(60,110,200)` (clear blue) for
  better readability on dark terminal backgrounds.

## [0.2.0] - 2026-05-17

### Added — 44 new formula functions
- **Date/time (16)**: `TODAY`, `NOW`, `DATE`, `YEAR`, `MONTH`, `DAY`, `HOUR`,
  `MINUTE`, `SECOND`, `TIME`, `WEEKDAY`, `WEEKNUM`, `DATEDIF`, `EDATE`,
  `EOMONTH`, `DAYS`. Serial dates use the Power BI / OLE Automation
  convention (clean Gregorian, no Excel 1900 leap-year bug — matches Excel
  exactly from 1900-03-01 onward).
- **Multi-criteria aggregates (3)**: `SUMIFS`, `COUNTIFS`, `AVERAGEIFS`.
- **Rounding (4)**: `ROUNDUP`, `ROUNDDOWN`, `CEILING`, `FLOOR`.
- **Financial (7)**: `PMT`, `PV`, `FV`, `RATE`, `NPER`, `NPV`, `IRR`.
  Follow Excel cash-flow sign convention (received = positive, paid =
  negative). `RATE` and `IRR` use Newton-Raphson iteration.
- **Trigonometry (9)**: `SIN`, `COS`, `TAN`, `ASIN`, `ACOS`, `ATAN`,
  `ATAN2`, `RADIANS`, `DEGREES`.
- **Log / exp (5)**: `LN`, `LOG`, `LOG10`, `EXP`, `PI`.
- **Statistics (4)**: `STDEV` (= `STDEV.S`), `VAR` (= `VAR.S`), `MEDIAN`,
  `MODE`.
- **Random / multiples (5)**: `RAND`, `RANDBETWEEN`, `GCD`, `LCM`, `FACT`.

### Added — UX
- **Print via HTML export**: `Ctrl+P` (or File → 印刷 (HTML)...) writes
  a print-friendly HTML file with the current sheet (column letters /
  row numbers as table headers that repeat on every printed page,
  right-aligned numerics, errors in red) and opens it in the default
  browser via `open` / `xdg-open` / `start`. Browser's Cmd/Ctrl+P
  dialog handles margins, scaling, PDF output, etc.
- **Mouse column-width resize**: drag the `│` separator in the column
  header to resize. macOS Terminal.app users must hold ⌥ (Option) — see
  README for the Terminal.app input quirks.
- **Column-width dialog**: right-click → "列幅を変更..." (or 書式 menu →
  same item) opens a dialog pre-filled with the current width for direct
  numeric entry. Clamped to 3-50.
- **Kitty Keyboard Protocol** is now pushed at startup when the terminal
  supports it. This makes `Shift+Arrow` work reliably under mouse-capture
  on kitty / foot / WezTerm / Alacritty / Ghostty / iTerm2 3.5+.

### Changed
- **Floating-point equality now uses a relative tolerance** (~1e-12, ≈15
  significant digits) instead of `f64::EPSILON`. Applied to all numeric
  `=` / `<>` / `>=` / `<=` / `>` / `<` comparisons, to `SUMIF` /
  `COUNTIF` / `AVERAGEIF` / `SUMIFS` / `COUNTIFS` / `AVERAGEIFS` criteria,
  and to `VLOOKUP` / `HLOOKUP` / `MATCH` exact-match lookups. This makes
  `=(0.1+0.2)=0.3` return TRUE and fixes a class of off-by-one-ULP bugs
  that appeared at large magnitudes.

### Fixed
- `Shift+Arrow` range selection now works when terminal mouse capture is
  on (was previously stripped on some terminals by the mouse-capture mode
  interaction). macOS Terminal.app's keyboard profile drops the SHIFT
  modifier on `Shift+↑/↓` regardless — workaround documented in README.

### Notes
- New dependency: `chrono = "0.4"` (default-features off, `clock` only)
  for local-time access in `TODAY` / `NOW`.

## [0.1.0] - Initial release

- Core spreadsheet engine with 35 functions (SUM, AVERAGE, IF, VLOOKUP, …)
- TUI grid with menu bar, context menu, mouse support (click / drag /
  scroll), formula bar, point-mode reference selection, CJK / IME support
- File formats: native JSON, CSV/TSV import/export
- Undo/redo, copy/paste (system clipboard via arboard), find/goto
