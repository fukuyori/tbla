# Changelog

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
this project adheres to [Semantic Versioning](https://semver.org/).

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
