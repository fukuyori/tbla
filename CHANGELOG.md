# Changelog

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
this project adheres to [Semantic Versioning](https://semver.org/).

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
