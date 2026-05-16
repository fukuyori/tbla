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
    Currency(usize),
    Percent(usize),
    Scientific,
    Date,
    Text,
}

impl Default for DisplayFormat {
    fn default() -> Self {
        DisplayFormat::General
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cell {
    pub value: CellValue,
    pub raw_input: String,
    pub format: DisplayFormat,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            value: CellValue::Empty,
            raw_input: String::new(),
            format: DisplayFormat::General,
        }
    }
}

impl Cell {
    pub fn new(input: String, value: CellValue) -> Self {
        Cell {
            value,
            raw_input: input,
            format: DisplayFormat::General,
        }
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
        match &self.format {
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
            DisplayFormat::Currency(decimals) => {
                format!("${:.prec$}", n, prec = decimals)
            }
            DisplayFormat::Percent(decimals) => {
                format!("{:.prec$}%", n * 100.0, prec = decimals)
            }
            DisplayFormat::Scientific => {
                format!("{:.2e}", n)
            }
            DisplayFormat::Date => {
                // Excel serial date (days since 1900-01-01)
                // Simplified: just show the number
                format!("{:.0}", n)
            }
            DisplayFormat::Text => {
                format!("{}", n)
            }
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
