use serde::{Deserialize, Serialize};

/// Cell value types (Excel-compatible)
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum CellValue {
    Empty,
    Number(f64),
    Text(String),
    Boolean(bool),
    Formula(String),
    Error(CellError),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum CellError {
    DivZero,    // #DIV/0!
    Value,      // #VALUE!
    Ref,        // #REF!
    Name,       // #NAME?
    Num,        // #NUM!
    NA,         // #N/A
    Cycle,      // Circular reference
}

impl CellError {
    pub fn to_string(&self) -> &'static str {
        match self {
            CellError::DivZero => "#DIV/0!",
            CellError::Value => "#VALUE!",
            CellError::Ref => "#REF!",
            CellError::Name => "#NAME?",
            CellError::Num => "#NUM!",
            CellError::NA => "#N/A",
            CellError::Cycle => "#CYCLE!",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum DisplayFormat {
    General,
    Number(usize),      // decimal places
    /// Thousands-separated (1,234,567.89) — Lotus 1-2-3 "," format.
    Comma(usize),
    Currency(usize),
    Percent(usize),
    Scientific,
    Date,               // yyyy-mm-dd from an Excel-style serial value
    DateTime,           // yyyy-mm-dd hh:mm
    Time,               // hh:mm:ss from the serial's day fraction
    Text,
}

impl DisplayFormat {
    /// Short l123-style tag shown in the formula bar, e.g. `F2` = 数値
    /// 2桁, `C0` = 通貨 0桁, `,2` = カンマ区切り. None for General.
    pub fn tag(&self) -> Option<String> {
        Some(match self {
            DisplayFormat::General => return None,
            DisplayFormat::Number(d) => format!("F{}", d),
            DisplayFormat::Comma(d) => format!(",{}", d),
            DisplayFormat::Currency(d) => format!("C{}", d),
            DisplayFormat::Percent(d) => format!("P{}", d),
            DisplayFormat::Scientific => "S".into(),
            DisplayFormat::Date => "D".into(),
            DisplayFormat::DateTime => "DT".into(),
            DisplayFormat::Time => "TM".into(),
            DisplayFormat::Text => "T".into(),
        })
    }
}

impl Default for DisplayFormat {
    fn default() -> Self {
        DisplayFormat::General
    }
}

/// Horizontal alignment of a cell's display value. `Default` means "auto":
/// numbers right-aligned, text left-aligned. Explicit settings override.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub enum Alignment {
    Default,
    Left,
    Center,
    Right,
}

impl Default for Alignment {
    fn default() -> Self { Alignment::Default }
}

/// Serializable RGB color tuple used for `Cell.text_color` / `Cell.bg_color`.
/// Stored as `[r, g, b]` integers in JSON.
pub type RgbColor = (u8, u8, u8);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cell {
    pub value: CellValue,
    pub raw_input: String,
    pub format: DisplayFormat,
    /// Optional fallback value used when this cell's formula can't be
    /// evaluated by tbla's engine (e.g., an unsupported function imported
    /// from Excel). Cleared whenever the user edits the cell. Stored as a
    /// CellValue so SUM/aggregates can still use it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_value: Option<CellValue>,
    /// Manual cell-format overrides. Each is optional/`Default` so older
    /// serialized cells round-trip without these fields present.
    #[serde(default, skip_serializing_if = "is_default_alignment")]
    pub alignment: Alignment,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub underline: bool,
    /// Negative numbers render wrapped in parentheses instead of a minus
    /// sign (Lotus `/Range Format Other Parentheses`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub neg_parens: bool,
    /// Negative numbers render in red (Excel's `[Red]` convention /
    /// Lotus `Other Color Negative`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub neg_red: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_color: Option<RgbColor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg_color: Option<RgbColor>,
}

fn is_default_alignment(a: &Alignment) -> bool { matches!(a, Alignment::Default) }
fn is_false(b: &bool) -> bool { !*b }

impl Default for Cell {
    fn default() -> Self {
        Cell {
            value: CellValue::Empty,
            raw_input: String::new(),
            format: DisplayFormat::General,
            cached_value: None,
            alignment: Alignment::Default,
            bold: false,
            italic: false,
            underline: false,
            neg_parens: false,
            neg_red: false,
            text_color: None,
            bg_color: None,
        }
    }
}

impl Cell {
    pub fn new(input: String, value: CellValue) -> Self {
        Cell {
            value,
            raw_input: input,
            format: DisplayFormat::General,
            cached_value: None,
            alignment: Alignment::Default,
            bold: false,
            italic: false,
            underline: false,
            neg_parens: false,
            neg_red: false,
            text_color: None,
            bg_color: None,
        }
    }

    pub fn with_cached(mut self, cached: Option<CellValue>) -> Self {
        self.cached_value = cached;
        self
    }

    /// True if any non-default formatting is set. Used by serializers to
    /// skip writing format-only cells when nothing's there.
    pub fn has_format(&self) -> bool {
        !matches!(self.format, DisplayFormat::General)
            || !matches!(self.alignment, Alignment::Default)
            || self.bold
            || self.italic
            || self.underline
            || self.neg_parens
            || self.neg_red
            || self.text_color.is_some()
            || self.bg_color.is_some()
    }

    pub fn is_empty(&self) -> bool {
        matches!(self.value, CellValue::Empty)
    }

    pub fn display(&self, width: usize) -> String {
        let text = match &self.value {
            CellValue::Empty => String::new(),
            CellValue::Number(n) => self.format_number(*n),
            CellValue::Text(s) => s.clone(),
            CellValue::Boolean(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
            CellValue::Formula(_) => "=...".to_string(), // Should be evaluated
            CellValue::Error(e) => e.to_string().to_string(),
        };

        if text.len() > width {
            if matches!(self.value, CellValue::Number(_)) {
                // Numbers show ### if too wide
                "#".repeat(width)
            } else {
                // Text gets truncated
                text[..width].to_string()
            }
        } else {
            text
        }
    }

    pub fn format_number(&self, n: f64) -> String {
        format_number_with(n, &self.format, self.neg_parens)
    }
}

/// Insert thousands separators into the integer digits of an already
/// formatted non-negative number string (e.g. "1234567.89" → "1,234,567.89").
fn group_thousands(s: &str) -> String {
    let (int_part, frac_part) = match s.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (s, None),
    };
    let digits: Vec<char> = int_part.chars().collect();
    let mut grouped = String::with_capacity(int_part.len() + int_part.len() / 3);
    for (i, c) in digits.iter().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(*c);
    }
    match frac_part {
        Some(f) => format!("{}.{}", grouped, f),
        None => grouped,
    }
}

/// Time-of-day from a serial value's day fraction, as (h, m, s).
fn serial_time(n: f64) -> (u32, u32, u32) {
    let frac = n.fract().abs();
    let total = (frac * 86400.0).round() as u64 % 86400;
    ((total / 3600) as u32, (total % 3600 / 60) as u32, (total % 60) as u32)
}

/// Format a number per `fmt`. The single source of truth for numeric cell
/// display; `neg_parens` wraps negative numeric values in parentheses
/// instead of the minus sign (numeric kinds only).
pub fn format_number_with(n: f64, fmt: &DisplayFormat, neg_parens: bool) -> String {
    // Numeric kinds honor the parentheses option; date/time/text don't.
    let numeric_kind = matches!(
        fmt,
        DisplayFormat::General | DisplayFormat::Number(_) | DisplayFormat::Comma(_)
            | DisplayFormat::Currency(_) | DisplayFormat::Percent(_) | DisplayFormat::Scientific
    );
    if neg_parens && numeric_kind && n < 0.0 {
        return format!("({})", format_number_with(-n, fmt, false));
    }
    match fmt {
        DisplayFormat::General => {
            if n == n.floor() && n.abs() < 1e10 {
                format!("{:.0}", n)
            } else if n.abs() < 0.0001 || n.abs() >= 1e10 {
                format!("{:.2e}", n)
            } else {
                // Remove trailing zeros
                let s = format!("{:.6}", n);
                let s = s.trim_end_matches('0').trim_end_matches('.');
                s.to_string()
            }
        }
        DisplayFormat::Number(decimals) => {
            format!("{:.prec$}", n, prec = decimals)
        }
        DisplayFormat::Comma(decimals) => {
            let s = format!("{:.prec$}", n.abs(), prec = decimals);
            let sign = if n < 0.0 { "-" } else { "" };
            format!("{}{}", sign, group_thousands(&s))
        }
        DisplayFormat::Currency(decimals) => {
            let s = format!("{:.prec$}", n.abs(), prec = decimals);
            let sign = if n < 0.0 { "-" } else { "" };
            format!("{}${}", sign, group_thousands(&s))
        }
        DisplayFormat::Percent(decimals) => {
            format!("{:.prec$}%", n * 100.0, prec = decimals)
        }
        DisplayFormat::Scientific => {
            format!("{:.2e}", n)
        }
        DisplayFormat::Date => {
            match crate::date_util::serial_to_date(n) {
                Some(d) => d.format("%Y-%m-%d").to_string(),
                None => format!("{:.0}", n),
            }
        }
        DisplayFormat::DateTime => {
            match crate::date_util::serial_to_date(n) {
                Some(d) => {
                    let (h, m, _) = serial_time(n);
                    format!("{} {:02}:{:02}", d.format("%Y-%m-%d"), h, m)
                }
                None => format!("{:.4}", n),
            }
        }
        DisplayFormat::Time => {
            let (h, m, s) = serial_time(n);
            format!("{:02}:{:02}:{:02}", h, m, s)
        }
        DisplayFormat::Text => {
            format!("{}", n)
        }
    }
}

/// Parse raw input into CellValue
pub fn parse_input(input: &str) -> CellValue {
    let trimmed = input.trim();
    
    if trimmed.is_empty() {
        return CellValue::Empty;
    }

    // Formula starts with =
    if trimmed.starts_with('=') {
        return CellValue::Formula(trimmed.to_string());
    }

    // Boolean
    if trimmed.eq_ignore_ascii_case("true") {
        return CellValue::Boolean(true);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return CellValue::Boolean(false);
    }

    // Number
    if let Ok(n) = trimmed.parse::<f64>() {
        return CellValue::Number(n);
    }

    // Percentage (e.g., "50%")
    if trimmed.ends_with('%') {
        if let Ok(n) = trimmed[..trimmed.len()-1].trim().parse::<f64>() {
            return CellValue::Number(n / 100.0);
        }
    }

    // Text
    CellValue::Text(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comma_and_currency_group_thousands() {
        assert_eq!(format_number_with(1234567.891, &DisplayFormat::Comma(2), false), "1,234,567.89");
        // {:.0} rounds half-to-even: 1234.5 → 1234.
        assert_eq!(format_number_with(-1234.5, &DisplayFormat::Comma(0), false), "-1,234");
        assert_eq!(format_number_with(1234.5, &DisplayFormat::Currency(2), false), "$1,234.50");
        assert_eq!(format_number_with(999.0, &DisplayFormat::Comma(0), false), "999");
    }

    #[test]
    fn negative_parentheses() {
        assert_eq!(format_number_with(-1234.5, &DisplayFormat::Comma(2), true), "(1,234.50)");
        assert_eq!(format_number_with(-0.15, &DisplayFormat::Percent(1), true), "(15.0%)");
        assert_eq!(format_number_with(1234.5, &DisplayFormat::Number(2), true), "1234.50");
        // Date/time kinds ignore the option.
        assert_eq!(format_number_with(45292.0, &DisplayFormat::Date, true), "2024-01-01");
    }

    #[test]
    fn date_time_formats() {
        // 45292 = 2024-01-01 (matches Excel from 1900-03-01 onward).
        assert_eq!(format_number_with(45292.0, &DisplayFormat::Date, false), "2024-01-01");
        assert_eq!(format_number_with(45292.5, &DisplayFormat::DateTime, false), "2024-01-01 12:00");
        assert_eq!(format_number_with(45292.75, &DisplayFormat::Time, false), "18:00:00");
    }

    #[test]
    fn format_tags() {
        assert_eq!(DisplayFormat::General.tag(), None);
        assert_eq!(DisplayFormat::Number(2).tag().as_deref(), Some("F2"));
        assert_eq!(DisplayFormat::Comma(0).tag().as_deref(), Some(",0"));
        assert_eq!(DisplayFormat::Currency(2).tag().as_deref(), Some("C2"));
        assert_eq!(DisplayFormat::Time.tag().as_deref(), Some("TM"));
    }
}
