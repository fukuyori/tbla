# Changelog

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
this project adheres to [Semantic Versioning](https://semver.org/).

## [0.4.2] - 2026-07-12

### Added — l123 由来の書式機能
- **カンマ書式**: 桁区切り表示 `1,234,567.89`（Lotus の `,` 書式）。通貨書式も
  桁区切り付きになりました。
- **日時・時刻書式**: シリアル値を `2024-01-01 12:00` / `18:00:00` で表示。
  日付書式も従来の数値表示から `2024-01-01` 形式の実表示になりました。
- **負数の表示**: 赤 / 括弧 `(123)` / 括弧+赤。セルの書式設定ダイアログの
  「負数」行、`:` メニューの `書式 → 負数の表示`。xlsx へは
  `0.00;[Red](0.00)` 形式で書き出し。
- **書式タグ**: 数式バーのセル名の隣に現在セルの書式を l123 流の短いタグで
  常時表示（`(F2)` `(,0)` `(C2)` `(P1)` `(S)` `(D)` `(DT)` `(TM)` `(T)`）。
- **シートの既定書式** (`書式 → シートの既定書式...`): 「標準」のセルが
  継承するシート全体の既定数値書式（l123 の `/Worksheet Global Format`）。
  セルを標準に戻すと再び既定を継承。JSON 保存にも対応。

### Added — WYSIWYG 書式メニュー（`:` キー、l123 の `:Format` にならう）
- 通常モードで `:`（全角 `：` も可）を押すと、カスケード式の書式ポップアップが
  開きます。頭文字キー数打で書式を直接適用: `:FB`=太字、`:FI`=斜体、`:FU`=下線、
  `:FT3`=文字色を赤、`:FG0`=背景色クリア、`:FAL`=左揃え、`:FR`=スタイル解除、
  `:CA`=列幅自動調整、`:E`=セルの書式設定ダイアログ、`:X`=書式クリア。
- `↑`/`↓`+`Enter`、`←`/`Esc` で1階層戻る、マウスクリックにも対応。色サブメニュー
  は色見本 ■ 付き。セル先頭に `:` を入力したいときは従来どおり `F2` から。

### Added — 斜体・下線
- セル書式に斜体・下線を追加（Lotus 1-2-3 WYSIWYG の Bold/Italic/Underline
  三点セット）。書式メニュー・`:` メニュー・セルの書式設定ダイアログから設定
  でき、JSON 保存と xlsx 読み書きにも対応（`<i/>`/`<u/>` をパース、
  rust_xlsxwriter で書き出し）。書式クリアで解除されます。

### Changed — ダイアログを中央のボックス表示に
- すべてのダイアログ（開く / 検索 / ジャンプ / 並べ替え / 書式設定 …）が、
  画面下部の入力行ではなく **画面中央のタイトル付きボックス** として
  表示されるようになりました。`[ OK ]` / `[ キャンセル ]` ボタンと
  操作ヒントもボックス内に表示されます。
- **マウス対応**: 選択肢・色見本のクリックで選択、ボタンのクリックで
  適用/取消、ホイールで選択肢の切替ができます。
- 長い入力はフィールド幅に収まるよう末尾側を表示（編集中の末尾が常に見える）。

### Added — セルの書式設定ダイアログ
- 書式 → セルの書式設定... （右クリックメニューにも追加）: 数値書式・
  小数桁数・揃え・太字・文字色・背景色を 1 つのダイアログで一括設定。
  選択範囲の左上セルの現在書式で初期化されるので、変更したい項目だけ
  切り替えて OK すれば範囲全体に適用できます。
- 文字色・背景色は **色見本パレット** から選択（文字色は濃色系、背景色は
  淡色系の各 8 色 + なし）。パレットに無い色が設定されたセルでは
  「現在」の選択肢が自動追加され、そのまま OK しても色は変わりません。
  カスタム RGB は従来どおり 文字色... / 背景色... ダイアログで指定できます。
- ダイアログに選択式フィールドを導入: `←`/`→`/`Space` で選択肢を切替、
  `Tab`/`Shift+Tab` に加えて `↑`/`↓` でも項目を移動できます。選択肢は
  行内に全て表示され、現在値がハイライトされます。数字などの頭文字キーで
  直接選択も可能（小数桁数は `0`〜`9` を押すだけ）。
- 数値書式... ダイアログも選択式に: 種別 (標準/数値/通貨/パーセント/
  指数/日付/文字列) と小数桁数 (0-10) を選ぶだけになり、英字のタイプ入力は
  不要になりました。現在の書式が初期選択されます。
- 文字色... / 背景色... ダイアログもパレット + RGB 入力の 2 段構成に:
  8 色 + なし の色見本から選ぶか、RGB を直接入力（入力時はパレットより
  優先）。現在の色が初期選択されます。

## [0.4.1] - 2026-07-11

### Added — East Asian Ambiguous width support
- 曖昧幅文字（①、○、→、─、※ など）を使っても表がずれなくなりました。
  起動時に代替スクリーン上へ「○」を出力してカーソル位置を問い合わせ、
  ターミナルが曖昧幅文字を 1 セル / 2 セルどちらで描画するかを自動判定します。
- 環境変数 `TBLA_AMBIGUOUS_WIDE=1` / `0` で自動判定を上書きできます。
- 幅計算を `src/width.rs` に一元化し、グリッド・メニュー・ダイアログ・
  シートタブ・列幅自動調整のすべてが同じ答えを使うようにしました。
  セル溢れ表示の「…」も曖昧幅として正しく数えます。

### Added — Series-fill paste (連続貼り付け)
- コピー元より大きい範囲を選択して貼り付けると、選択範囲全体を
  Excel 風に埋めます。数値・数値入りテキストの等差系列は増分を
  引き継いで延長し、数式は参照を調整しながらタイル展開します。

## [0.4.0] - 2026-07-11

### Added — Lotus 1-2-3 style operability (inspired by [l123](https://github.com/duane1024/l123))
- **Slash menu**: `/` (or IME full-width `／`) in normal mode enters the
  menu bar with the highlighted menu's dropdown shown as a preview.
  Mnemonic letters (now displayed on every submenu item, e.g. 並べ替え(S))
  descend without Enter — `/D S` = データ → 並べ替え, `/F S` = 保存; a
  letter with no top-level match runs the previewed menu's item (`/N` =
  新規). `Esc` backs out one level at a time. A literal leading `/` in a
  cell can still be typed via `F2`.
- **F4 — cycle reference anchoring**: while editing a formula, cycles the
  reference at the cursor through `A1` → `$A$1` → `A$1` → `$A1`; ranges
  cycle both endpoints. Works on a reference just inserted by point mode.
- **F5 — GOTO**: opens the ジャンプ dialog (same as `Ctrl+G`); the dialog
  now also accepts named ranges (jumps, selects the range, switches sheet).
- **F9 — recalculate**: forces a redraw / re-rolls volatile functions
  (`RAND`, `NOW`, ...). Also available as データ → 再計算.
- **Named ranges**: 挿入 → 名前付き範囲を定義... / 名前付き範囲の管理...
  (`/I N`, `/I M`). Names (case-insensitive, Japanese OK) usable in
  formulas (`=SUM(売上)`, `=税率*B2`) and as GOTO targets. Persisted in
  the `.json` workbook format and round-tripped to/from `.xlsx` defined
  names. Cross-sheet names resolve in formulas for single cells only.

### Fixed
- Formula engine operator splitting was byte/char inconsistent and could
  panic on multibyte text outside string literals (e.g. `=税率*100` with
  a named range); `find_operator` / `find_operator_rtl` /
  `find_matching_paren` now return byte offsets.
- 編集メニューの「やり直し」のニーモニックが「置換」と重複していたのを
  `Y` に変更。

## [0.3.3] - 2026-05-22

### Added — HTML table import from URL
- `データ → URLから取り込み...` (`Alt+D`, `U`) fetches a web page and
  extracts its `<table>` elements into a sheet.
- **Two-stage dialog**:
  1. URL を入力
  2. ページ取得後、検出されたテーブル一覧（行数 × 列数 / `<caption>` / 先頭行プレビュー）が
     表示され、テーブル番号と取り込み先（`s` = 新規シート / `o` = 上書き）を選択
- **Charset auto-detection**: HTTP `Content-Type: charset=...` → UTF-8 BOM →
  `<meta charset>` → strict UTF-8 → Shift-JIS / CP932 フォールバック。
  日本語サイトの Shift-JIS ページもそのまま読み込めます。
- **Sheet naming**: テーブルに `<caption>` があればそれをシート名に、無ければ
  ホスト名 + テーブル番号（例: `example.com[2]`）を採用（Excel の 31 文字制限内に収める）。
- **Sane limits**: HTTP timeout 30 秒、ボディ上限 20 MB、UA は `tbla/<version> (table import)`。
- **Caveats (v1)**: `colspan` / `rowspan` は未展開（ソースのテキストをそのまま 1 セルに格納）。

### Added — SQL query import (read-only)
- `データ → SQL から取り込み...` (`Alt+D`, `L`) runs a SELECT against a
  PostgreSQL / MySQL / MariaDB / SQLite database and loads the result into
  a sheet.
- **Multi-DB via URI scheme**:
  - `postgresql://user:pass@host:5432/db` / `postgres://…`
  - `mysql://user:pass@host:3306/db` / `mariadb://…` (rustls TLS supported)
  - `sqlite:///path/to/file.db` / `sqlite3://…` / `file://…`, or a bare
    path ending in `.sqlite` / `.sqlite3` / `.db` (e.g. `data.sqlite` or
    `C:\Users\me\data.db`); `:memory:` also works
- **Dialog**: three fields — URI, SQL query, destination (`s` = 新規シート /
  `o` = 上書き). The URI and query are remembered for the next call within
  the session so iterating is quick.
- **Result shape**: row 0 = column names, then one row per result row.
  `NULL` becomes an empty cell. Binary / unsupported types are surfaced
  as `<bytea N bytes>` / `<{type-name}>` markers so they don't break
  display.
- **Sheet name**: derived from the URI's last path segment (database name
  for postgres/mysql, file stem for sqlite), trimmed to 31 chars.

### New dependencies
- `ureq = "2"` (with `tls` feature → rustls) for synchronous HTTPS GET.
- `scraper = "0.20"` for HTML / CSS-selector parsing.
- `rusqlite = "0.31"` with `bundled` feature (SQLite compiled in, no
  system library required).
- `postgres = "0.19"` (sync). **TLS intentionally off in v1** — works for
  local / VPC databases out of the box; cloud-managed Postgres requiring
  TLS will need a follow-up enabling `postgres-native-tls` or
  `postgres-rustls`.
- `mysql = "25"` with `rustls-tls`. TLS works automatically when the
  server requires it.

### Notes
- Adds ~3.5 MB to the binary and ~30 s to a clean build (first time the
  TLS / SQL crates compile).
- The SQL query field is single-line. For multi-statement work, use a view /
  stored procedure or a single `SELECT` per import.

## [0.3.2] - 2026-05-22

### Fixed
- **CSV / text / JSON load no longer fails with `stream did not contain
  valid UTF-8`**. The native JSON loader and the CSV / TSV importer now
  auto-detect the file encoding instead of assuming strict UTF-8:
  1. **UTF-8 BOM** (`EF BB BF`) — stripped, decoded as UTF-8.
  2. **UTF-16 LE / BE BOM** — decoded as UTF-16.
  3. **Strict UTF-8** — used as-is if valid.
  4. **Fallback: Shift-JIS / CP932** — covers Excel-on-Japanese-Windows
     CSV exports, the most common non-UTF-8 case our users hit.

  Unmappable bytes are replaced rather than failing — partial data beats a
  hard error for a spreadsheet import.

### New dependencies
- `encoding_rs = "0.8"` for the Shift-JIS and UTF-16 codecs.

## [0.3.1] - 2026-05-17

### Added — Polars DataFrame view (analytical layer)
A new DataFrame view backed by Polars makes tbla viable for million-row
analytical work without giving up the spreadsheet UI. The view is opt-in
per sheet and round-trips losslessly to/from the existing cell model.

**Conversion + read-only display**
- `データ → DataFrame ビューに変換` builds a typed `DataFrame` from the
  current sheet (row 0 = headers; columns auto-inferred to Int64 /
  Float64 / Boolean / Utf8; empty cells become nulls).
- `データ → セルビューに戻す` reverts; the underlying cells were
  preserved while in DataFrame mode so this is lossless.
- Status bar shows `DF N×M [Int64, Utf8, …]` so the row count and
  column types are visible at a glance.
- Header row renders bold and centered.

**Editing in DataFrame view**
- Row 0 edits rename the column. Empty / duplicate names are rejected.
- Data-row edits update the underlying Series. If the new string parses
  into the column's dtype, it's stored as that type; otherwise the
  column is **widened to Utf8 once** and the value is stored verbatim.
  Empty input becomes null.
- Undo (Ctrl+Z) covers DataFrame edits — each edit records a snapshot.

**Computed columns**
- `データ → 計算列を追加…` adds a derived column from a Polars-SQL
  expression: e.g. `revenue = price * qty`, `tax = revenue * 0.1`,
  `grade = CASE WHEN score >= 80 THEN 'A' ELSE 'B' END`. Earlier
  computed columns can be referenced by later ones.
- `データ → 計算列をクリア` resets to the freshly-converted DataFrame.

**Analytical operations**
- `データ → SQL クエリ…` runs an arbitrary Polars SQL query against the
  active DataFrame (referenced as table `df`):
  - `SELECT * FROM df WHERE price > 100`
  - `SELECT category, SUM(amount) FROM df GROUP BY category ORDER BY 2 DESC`
  - `SELECT * EXCLUDE name FROM df`
- `データ → グループ集計…` builds the GROUP BY SQL behind the scenes:
  fields are group columns (comma separated) and `col:func` aggregations
  (`sum / avg / min / max / count / stddev / var`).

**Direct CSV / Parquet I/O**
- `.parquet` files are first-class: opening a `.parquet` via `File →
  Open` (or as a CLI arg) loads directly into a DataFrame view. Saving
  to `.parquet` uses Snappy compression — typical numeric data is ~10×
  smaller than CSV.
- `ファイル → CSV を DataFrame として開く…` uses Polars's fast,
  chunked, columnar CSV reader with auto type inference, suitable for
  10 MB / millions-of-rows files the cell importer would choke on.
- `ファイル → Parquet として保存…` writes the active sheet as Parquet
  (auto-converts cells → DataFrame on the fly when needed).

### New dependencies
- `polars = "0.47"` with features `lazy, csv, strings, dtype-full, fmt,
  sql, parquet, streaming`. Adds ~30 MB to the release binary and a
  one-time ~2-minute first build; subsequent builds are incremental.

### Notes
- Row insertion / deletion / column add-drop are not yet UI-exposed in
  DataFrame view. Use SQL queries for those structural changes.

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
