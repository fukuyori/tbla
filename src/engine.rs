use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use chrono::{Datelike, Duration, NaiveDate};
use crate::cell::{Cell, CellValue, CellError};
use crate::date_util;
use crate::formula;

pub struct Engine<'a> {
    cells: &'a HashMap<(usize, usize), Cell>,
    eval_stack: HashSet<(usize, usize)>,
}

impl<'a> Engine<'a> {
    pub fn new(cells: &'a HashMap<(usize, usize), Cell>) -> Self {
        Engine { cells, eval_stack: HashSet::new() }
    }

    pub fn evaluate_formula(&mut self, formula_str: &str) -> Result<CellValue, String> {
        let expr = formula_str.trim();
        if !expr.starts_with('=') {
            return Ok(CellValue::Text(expr.to_string()));
        }
        self.evaluate_expr(&expr[1..])
    }

    pub fn evaluate_cell(&mut self, col: usize, row: usize) -> Result<CellValue, String> {
        if self.eval_stack.contains(&(col, row)) {
            return Ok(CellValue::Error(CellError::Cycle));
        }
        let cell = self.cells.get(&(col, row));
        match cell {
            None => Ok(CellValue::Number(0.0)),
            Some(cell) => match &cell.value {
                CellValue::Empty => Ok(CellValue::Number(0.0)),
                CellValue::Number(n) => Ok(CellValue::Number(*n)),
                CellValue::Text(s) => Ok(CellValue::Text(s.clone())),
                CellValue::Boolean(b) => Ok(CellValue::Boolean(*b)),
                CellValue::Error(e) => Ok(CellValue::Error(e.clone())),
                CellValue::Formula(f) => {
                    self.eval_stack.insert((col, row));
                    let result = self.evaluate_formula(f);
                    self.eval_stack.remove(&(col, row));
                    // If evaluation errors (e.g. unsupported function imported
                    // from Excel) and the cell has a cached value from import,
                    // fall back to that so aggregates still work.
                    match (result, &cell.cached_value) {
                        (Ok(v), _) => Ok(v),
                        (Err(_), Some(cached)) => Ok(cached.clone()),
                        (Err(e), None) => Err(e),
                    }
                }
            }
        }
    }

    fn evaluate_expr(&mut self, expr: &str) -> Result<CellValue, String> {
        let expr = expr.trim();
        if let Some(result) = self.try_function(expr)? { return Ok(result); }
        if expr.starts_with('(') {
            if let Some(end) = find_matching_paren(expr, 0) {
                if end == expr.len() - 1 { return self.evaluate_expr(&expr[1..end]); }
            }
        }
        for op in [">=", "<=", "<>", "!=", "=", ">", "<"] {
            if let Some(pos) = find_operator(expr, op) {
                let left = self.evaluate_expr(&expr[..pos])?;
                let right = self.evaluate_expr(&expr[pos + op.len()..])?;
                return compare(left, right, op);
            }
        }
        if let Some(pos) = find_operator(expr, "&") {
            let left = self.evaluate_expr(&expr[..pos])?;
            let right = self.evaluate_expr(&expr[pos + 1..])?;
            return Ok(CellValue::Text(format!("{}{}", to_string(&left), to_string(&right))));
        }
        if let Some(pos) = find_operator_rtl(expr, &['+', '-']) {
            if pos > 0 {
                let left = self.evaluate_expr(&expr[..pos])?;
                let right = self.evaluate_expr(&expr[pos + 1..])?;
                let op = expr.chars().nth(pos).unwrap();
                return arithmetic(left, right, op);
            }
        }
        if let Some(pos) = find_operator_rtl(expr, &['*', '/']) {
            let left = self.evaluate_expr(&expr[..pos])?;
            let right = self.evaluate_expr(&expr[pos + 1..])?;
            let op = expr.chars().nth(pos).unwrap();
            return arithmetic(left, right, op);
        }
        if let Some(pos) = find_operator_rtl(expr, &['^']) {
            let left = self.evaluate_expr(&expr[..pos])?;
            let right = self.evaluate_expr(&expr[pos + 1..])?;
            return power(left, right);
        }
        if expr.starts_with('-') {
            let val = self.evaluate_expr(&expr[1..])?;
            return match val {
                CellValue::Number(n) => Ok(CellValue::Number(-n)),
                _ => Err("#VALUE!".to_string()),
            };
        }
        if let Ok(n) = expr.parse::<f64>() { return Ok(CellValue::Number(n)); }
        if expr.starts_with('"') && expr.ends_with('"') && expr.len() >= 2 {
            return Ok(CellValue::Text(expr[1..expr.len()-1].to_string()));
        }
        if expr.eq_ignore_ascii_case("TRUE") { return Ok(CellValue::Boolean(true)); }
        if expr.eq_ignore_ascii_case("FALSE") { return Ok(CellValue::Boolean(false)); }
        if let Some((col, row, _, _)) = formula::parse_cell_ref(expr) {
            return self.evaluate_cell(col, row);
        }
        Err("#NAME?".to_string())
    }

    fn try_function(&mut self, expr: &str) -> Result<Option<CellValue>, String> {
        let paren_pos = match expr.find('(') { Some(p) => p, None => return Ok(None) };
        if !expr.ends_with(')') { return Ok(None); }
        let func_name = expr[..paren_pos].trim().to_uppercase();
        let args_str = &expr[paren_pos + 1..expr.len() - 1];
        let result = match func_name.as_str() {
            "SUM" => self.func_sum(args_str)?,
            "AVERAGE" | "AVG" => self.func_average(args_str)?,
            "COUNT" => self.func_count(args_str)?,
            "COUNTA" => self.func_counta(args_str)?,
            "MIN" => self.func_min(args_str)?,
            "MAX" => self.func_max(args_str)?,
            "IF" => self.func_if(args_str)?,
            "SUMIF" => self.func_sumif(args_str)?,
            "COUNTIF" => self.func_countif(args_str)?,
            "AVERAGEIF" => self.func_averageif(args_str)?,
            "SUMIFS" => self.func_sumifs(args_str)?,
            "COUNTIFS" => self.func_countifs(args_str)?,
            "AVERAGEIFS" => self.func_averageifs(args_str)?,
            "VLOOKUP" => self.func_vlookup(args_str)?,
            "HLOOKUP" => self.func_hlookup(args_str)?,
            "INDEX" => self.func_index(args_str)?,
            "MATCH" => self.func_match(args_str)?,
            "LEFT" => self.func_left(args_str)?,
            "RIGHT" => self.func_right(args_str)?,
            "MID" => self.func_mid(args_str)?,
            "LEN" => self.func_len(args_str)?,
            "TRIM" => self.func_trim(args_str)?,
            "UPPER" => self.func_upper(args_str)?,
            "LOWER" => self.func_lower(args_str)?,
            "ABS" => self.func_abs(args_str)?,
            "ROUND" => self.func_round(args_str)?,
            "ROUNDUP" => self.func_roundup(args_str)?,
            "ROUNDDOWN" => self.func_rounddown(args_str)?,
            "CEILING" => self.func_ceiling(args_str)?,
            "FLOOR" => self.func_floor(args_str)?,
            "INT" => self.func_int(args_str)?,
            "MOD" => self.func_mod(args_str)?,
            "POWER" => self.func_power(args_str)?,
            "SQRT" => self.func_sqrt(args_str)?,
            "SIN" => self.func_sin(args_str)?,
            "COS" => self.func_cos(args_str)?,
            "TAN" => self.func_tan(args_str)?,
            "ASIN" => self.func_asin(args_str)?,
            "ACOS" => self.func_acos(args_str)?,
            "ATAN" => self.func_atan(args_str)?,
            "ATAN2" => self.func_atan2(args_str)?,
            "RADIANS" => self.func_radians(args_str)?,
            "DEGREES" => self.func_degrees(args_str)?,
            "LN" => self.func_ln(args_str)?,
            "LOG" => self.func_log(args_str)?,
            "LOG10" => self.func_log10(args_str)?,
            "EXP" => self.func_exp(args_str)?,
            "PI" => self.func_pi(args_str)?,
            "STDEV" | "STDEV.S" => self.func_stdev(args_str)?,
            "VAR" | "VAR.S" => self.func_var(args_str)?,
            "MEDIAN" => self.func_median(args_str)?,
            "MODE" | "MODE.SNGL" => self.func_mode(args_str)?,
            "RAND" => self.func_rand(args_str)?,
            "RANDBETWEEN" => self.func_randbetween(args_str)?,
            "GCD" => self.func_gcd(args_str)?,
            "LCM" => self.func_lcm(args_str)?,
            "FACT" => self.func_fact(args_str)?,
            "TODAY" => self.func_today(args_str)?,
            "NOW" => self.func_now(args_str)?,
            "DATE" => self.func_date(args_str)?,
            "YEAR" => self.func_year(args_str)?,
            "MONTH" => self.func_month(args_str)?,
            "DAY" => self.func_day(args_str)?,
            "HOUR" => self.func_hour(args_str)?,
            "MINUTE" => self.func_minute(args_str)?,
            "SECOND" => self.func_second(args_str)?,
            "TIME" => self.func_time(args_str)?,
            "WEEKDAY" => self.func_weekday(args_str)?,
            "WEEKNUM" => self.func_weeknum(args_str)?,
            "DATEDIF" => self.func_datedif(args_str)?,
            "EDATE" => self.func_edate(args_str)?,
            "EOMONTH" => self.func_eomonth(args_str)?,
            "DAYS" => self.func_days(args_str)?,
            "PMT" => self.func_pmt(args_str)?,
            "PV" => self.func_pv(args_str)?,
            "FV" => self.func_fv(args_str)?,
            "NPER" => self.func_nper(args_str)?,
            "RATE" => self.func_rate(args_str)?,
            "NPV" => self.func_npv(args_str)?,
            "IRR" => self.func_irr(args_str)?,
            "AND" => self.func_and(args_str)?,
            "OR" => self.func_or(args_str)?,
            "NOT" => self.func_not(args_str)?,
            "CONCATENATE" | "CONCAT" => self.func_concat(args_str)?,
            "IFERROR" => self.func_iferror(args_str)?,
            "ISBLANK" => self.func_isblank(args_str)?,
            "ISNUMBER" => self.func_isnumber(args_str)?,
            "ISTEXT" => self.func_istext(args_str)?,
            _ => return Ok(None),
        };
        Ok(Some(result))
    }

    fn parse_range(&self, range_str: &str) -> Result<Vec<(usize, usize)>, String> {
        let parts: Vec<&str> = range_str.split(':').collect();
        if parts.len() == 2 {
            let (sc, sr, _, _) = formula::parse_cell_ref(parts[0]).ok_or("Invalid range")?;
            let (ec, er, _, _) = formula::parse_cell_ref(parts[1]).ok_or("Invalid range")?;
            let mut cells = Vec::new();
            for row in sr..=er { for col in sc..=ec { cells.push((col, row)); } }
            Ok(cells)
        } else if parts.len() == 1 {
            let (col, row, _, _) = formula::parse_cell_ref(parts[0]).ok_or("Invalid cell")?;
            Ok(vec![(col, row)])
        } else { Err("Invalid range".to_string()) }
    }

    fn get_numeric_values(&mut self, args_str: &str) -> Result<Vec<f64>, String> {
        let mut values = Vec::new();
        for arg in split_args(args_str) {
            if arg.contains(':') {
                for (col, row) in self.parse_range(&arg)? {
                    if let Ok(CellValue::Number(n)) = self.evaluate_cell(col, row) { values.push(n); }
                }
            } else if let Some((col, row, _, _)) = formula::parse_cell_ref(&arg) {
                if let Ok(CellValue::Number(n)) = self.evaluate_cell(col, row) { values.push(n); }
            } else if let Ok(n) = arg.parse::<f64>() { values.push(n); }
        }
        Ok(values)
    }

    fn matches_criteria(&mut self, col: usize, row: usize, criteria: &str) -> Result<bool, String> {
        let val = self.evaluate_cell(col, row)?;
        for op in [">=", "<=", "<>", "!=", ">", "<"] {
            if criteria.starts_with(op) {
                let target: f64 = criteria[op.len()..].trim().parse().map_err(|_| "#VALUE!")?;
                if let Ok(n) = to_number(&val) {
                    let eq = approx_eq(n, target);
                    return Ok(match op {
                        ">=" => n > target || eq,
                        "<=" => n < target || eq,
                        "<>" | "!=" => !eq,
                        ">" => n > target && !eq,
                        "<" => n < target && !eq,
                        _ => false,
                    });
                }
                return Ok(false);
            }
        }
        if let Ok(target) = criteria.parse::<f64>() {
            if let Ok(n) = to_number(&val) { return Ok(approx_eq(n, target)); }
            return Ok(false);
        }
        Ok(to_string(&val).to_uppercase() == criteria.to_uppercase())
    }

    // Functions
    fn func_sum(&mut self, args_str: &str) -> Result<CellValue, String> {
        let values = self.get_numeric_values(args_str)?;
        Ok(CellValue::Number(values.iter().sum()))
    }

    fn func_average(&mut self, args_str: &str) -> Result<CellValue, String> {
        let values = self.get_numeric_values(args_str)?;
        if values.is_empty() { return Ok(CellValue::Error(CellError::DivZero)); }
        Ok(CellValue::Number(values.iter().sum::<f64>() / values.len() as f64))
    }

    fn func_count(&mut self, args_str: &str) -> Result<CellValue, String> {
        let mut count = 0;
        for arg in split_args(args_str) {
            if arg.contains(':') {
                for (col, row) in self.parse_range(&arg)? {
                    if let Ok(CellValue::Number(_)) = self.evaluate_cell(col, row) { count += 1; }
                }
            } else if let Some((col, row, _, _)) = formula::parse_cell_ref(&arg) {
                if let Ok(CellValue::Number(_)) = self.evaluate_cell(col, row) { count += 1; }
            } else if arg.parse::<f64>().is_ok() { count += 1; }
        }
        Ok(CellValue::Number(count as f64))
    }

    fn func_counta(&mut self, args_str: &str) -> Result<CellValue, String> {
        let mut count = 0;
        for arg in split_args(args_str) {
            if arg.contains(':') {
                for (col, row) in self.parse_range(&arg)? {
                    if let Ok(val) = self.evaluate_cell(col, row) {
                        if !matches!(val, CellValue::Empty) { count += 1; }
                    }
                }
            } else if let Some((col, row, _, _)) = formula::parse_cell_ref(&arg) {
                if let Ok(val) = self.evaluate_cell(col, row) {
                    if !matches!(val, CellValue::Empty) { count += 1; }
                }
            } else if !arg.is_empty() { count += 1; }
        }
        Ok(CellValue::Number(count as f64))
    }

    fn func_min(&mut self, args_str: &str) -> Result<CellValue, String> {
        let values = self.get_numeric_values(args_str)?;
        if values.is_empty() { return Ok(CellValue::Number(0.0)); }
        Ok(CellValue::Number(values.iter().cloned().fold(f64::INFINITY, f64::min)))
    }

    fn func_max(&mut self, args_str: &str) -> Result<CellValue, String> {
        let values = self.get_numeric_values(args_str)?;
        if values.is_empty() { return Ok(CellValue::Number(0.0)); }
        Ok(CellValue::Number(values.iter().cloned().fold(f64::NEG_INFINITY, f64::max)))
    }

    fn func_if(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 2 { return Err("#VALUE!".to_string()); }
        let condition = self.evaluate_expr(&args[0])?;
        let is_true = to_bool(&condition)?;
        if is_true { self.evaluate_expr(&args[1]) }
        else if args.len() > 2 { self.evaluate_expr(&args[2]) }
        else { Ok(CellValue::Boolean(false)) }
    }

    fn func_sumif(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 2 { return Err("#VALUE!".to_string()); }
        let range_cells = self.parse_range(&args[0])?;
        let criteria = args[1].trim().trim_matches('"');
        let sum_range = if args.len() > 2 { self.parse_range(&args[2])? } else { range_cells.clone() };
        let mut sum = 0.0;
        for (i, (col, row)) in range_cells.iter().enumerate() {
            if self.matches_criteria(*col, *row, criteria)? {
                if let Some((sc, sr)) = sum_range.get(i) {
                    if let Ok(CellValue::Number(n)) = self.evaluate_cell(*sc, *sr) { sum += n; }
                }
            }
        }
        Ok(CellValue::Number(sum))
    }

    fn func_countif(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 2 { return Err("#VALUE!".to_string()); }
        let range_cells = self.parse_range(&args[0])?;
        let criteria = args[1].trim().trim_matches('"');
        let mut count = 0;
        for (col, row) in range_cells { if self.matches_criteria(col, row, criteria)? { count += 1; } }
        Ok(CellValue::Number(count as f64))
    }

    fn func_averageif(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 2 { return Err("#VALUE!".to_string()); }
        let range_cells = self.parse_range(&args[0])?;
        let criteria = args[1].trim().trim_matches('"');
        let avg_range = if args.len() > 2 { self.parse_range(&args[2])? } else { range_cells.clone() };
        let mut sum = 0.0; let mut count = 0;
        for (i, (col, row)) in range_cells.iter().enumerate() {
            if self.matches_criteria(*col, *row, criteria)? {
                if let Some((ac, ar)) = avg_range.get(i) {
                    if let Ok(CellValue::Number(n)) = self.evaluate_cell(*ac, *ar) { sum += n; count += 1; }
                }
            }
        }
        if count == 0 { Ok(CellValue::Error(CellError::DivZero)) } else { Ok(CellValue::Number(sum / count as f64)) }
    }

    fn func_vlookup(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 3 { return Err("#VALUE!".to_string()); }
        let lookup_val = self.evaluate_expr(&args[0])?;
        let range = self.parse_range(&args[1])?;
        let col_idx_val = self.evaluate_expr(&args[2])?;
        let col_idx = to_number(&col_idx_val)? as usize;
        let exact = if args.len() > 3 { let v = self.evaluate_expr(&args[3])?; !to_bool(&v)? } else { false };
        if col_idx == 0 { return Ok(CellValue::Error(CellError::Value)); }
        let min_col = range.iter().map(|(c, _)| *c).min().unwrap_or(0);
        let min_row = range.iter().map(|(_, r)| *r).min().unwrap_or(0);
        let max_row = range.iter().map(|(_, r)| *r).max().unwrap_or(0);
        for row in min_row..=max_row {
            let cell_val = self.evaluate_cell(min_col, row)?;
            let matches = if exact { cell_eq(&lookup_val, &cell_val) } else { cell_lte(&cell_val, &lookup_val) };
            if matches { return self.evaluate_cell(min_col + col_idx - 1, row); }
        }
        Ok(CellValue::Error(CellError::NA))
    }

    fn func_hlookup(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 3 { return Err("#VALUE!".to_string()); }
        let lookup_val = self.evaluate_expr(&args[0])?;
        let range = self.parse_range(&args[1])?;
        let row_idx_val = self.evaluate_expr(&args[2])?;
        let row_idx = to_number(&row_idx_val)? as usize;
        let exact = if args.len() > 3 { let v = self.evaluate_expr(&args[3])?; !to_bool(&v)? } else { false };
        if row_idx == 0 { return Ok(CellValue::Error(CellError::Value)); }
        let min_col = range.iter().map(|(c, _)| *c).min().unwrap_or(0);
        let max_col = range.iter().map(|(c, _)| *c).max().unwrap_or(0);
        let min_row = range.iter().map(|(_, r)| *r).min().unwrap_or(0);
        for col in min_col..=max_col {
            let cell_val = self.evaluate_cell(col, min_row)?;
            let matches = if exact { cell_eq(&lookup_val, &cell_val) } else { cell_lte(&cell_val, &lookup_val) };
            if matches { return self.evaluate_cell(col, min_row + row_idx - 1); }
        }
        Ok(CellValue::Error(CellError::NA))
    }

    fn func_index(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 2 { return Err("#VALUE!".to_string()); }
        let range = self.parse_range(&args[0])?;
        let row_num_val = self.evaluate_expr(&args[1])?;
        let row_num = to_number(&row_num_val)? as usize;
        let col_num = if args.len() > 2 { let v = self.evaluate_expr(&args[2])?; to_number(&v)? as usize } else { 1 };
        let min_col = range.iter().map(|(c, _)| *c).min().unwrap_or(0);
        let min_row = range.iter().map(|(_, r)| *r).min().unwrap_or(0);
        if row_num == 0 || col_num == 0 { return Ok(CellValue::Error(CellError::Value)); }
        self.evaluate_cell(min_col + col_num - 1, min_row + row_num - 1)
    }

    fn func_match(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 2 { return Err("#VALUE!".to_string()); }
        let lookup_val = self.evaluate_expr(&args[0])?;
        let range = self.parse_range(&args[1])?;
        let _match_type = if args.len() > 2 { let v = self.evaluate_expr(&args[2])?; to_number(&v)? as i32 } else { 1 };
        for (i, (col, row)) in range.iter().enumerate() {
            let cell_val = self.evaluate_cell(*col, *row)?;
            if cell_eq(&lookup_val, &cell_val) { return Ok(CellValue::Number((i + 1) as f64)); }
        }
        Ok(CellValue::Error(CellError::NA))
    }

    fn func_left(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.is_empty() { return Err("#VALUE!".to_string()); }
        let val = self.evaluate_expr(&args[0])?;
        let text = to_string(&val);
        let num = if args.len() > 1 { let v = self.evaluate_expr(&args[1])?; to_number(&v)? as usize } else { 1 };
        Ok(CellValue::Text(text.chars().take(num).collect()))
    }

    fn func_right(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.is_empty() { return Err("#VALUE!".to_string()); }
        let val = self.evaluate_expr(&args[0])?;
        let text = to_string(&val);
        let num = if args.len() > 1 { let v = self.evaluate_expr(&args[1])?; to_number(&v)? as usize } else { 1 };
        let len = text.chars().count();
        Ok(CellValue::Text(text.chars().skip(len.saturating_sub(num)).collect()))
    }

    fn func_mid(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 3 { return Err("#VALUE!".to_string()); }
        let val = self.evaluate_expr(&args[0])?;
        let text = to_string(&val);
        let start_val = self.evaluate_expr(&args[1])?;
        let start = to_number(&start_val)? as usize;
        let num_val = self.evaluate_expr(&args[2])?;
        let num = to_number(&num_val)? as usize;
        if start == 0 { return Ok(CellValue::Error(CellError::Value)); }
        Ok(CellValue::Text(text.chars().skip(start - 1).take(num).collect()))
    }

    fn func_len(&mut self, args_str: &str) -> Result<CellValue, String> {
        let val = self.evaluate_expr(args_str)?;
        Ok(CellValue::Number(to_string(&val).chars().count() as f64))
    }

    fn func_trim(&mut self, args_str: &str) -> Result<CellValue, String> {
        let val = self.evaluate_expr(args_str)?;
        Ok(CellValue::Text(to_string(&val).split_whitespace().collect::<Vec<_>>().join(" ")))
    }

    fn func_upper(&mut self, args_str: &str) -> Result<CellValue, String> {
        let val = self.evaluate_expr(args_str)?;
        Ok(CellValue::Text(to_string(&val).to_uppercase()))
    }

    fn func_lower(&mut self, args_str: &str) -> Result<CellValue, String> {
        let val = self.evaluate_expr(args_str)?;
        Ok(CellValue::Text(to_string(&val).to_lowercase()))
    }

    fn func_abs(&mut self, args_str: &str) -> Result<CellValue, String> {
        let val = self.evaluate_expr(args_str)?;
        Ok(CellValue::Number(to_number(&val)?.abs()))
    }

    fn func_round(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        let val = self.evaluate_expr(&args[0])?;
        let n = to_number(&val)?;
        let decimals = if args.len() > 1 { let v = self.evaluate_expr(&args[1])?; to_number(&v)? as i32 } else { 0 };
        let factor = 10f64.powi(decimals);
        Ok(CellValue::Number((n * factor).round() / factor))
    }

    fn func_int(&mut self, args_str: &str) -> Result<CellValue, String> {
        let val = self.evaluate_expr(args_str)?;
        Ok(CellValue::Number(to_number(&val)?.floor()))
    }

    fn func_mod(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 2 { return Err("#VALUE!".to_string()); }
        let dividend_val = self.evaluate_expr(&args[0])?;
        let divisor_val = self.evaluate_expr(&args[1])?;
        let dividend = to_number(&dividend_val)?;
        let divisor = to_number(&divisor_val)?;
        if divisor == 0.0 { return Ok(CellValue::Error(CellError::DivZero)); }
        Ok(CellValue::Number(dividend % divisor))
    }

    fn func_power(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 2 { return Err("#VALUE!".to_string()); }
        let base_val = self.evaluate_expr(&args[0])?;
        let exp_val = self.evaluate_expr(&args[1])?;
        Ok(CellValue::Number(to_number(&base_val)?.powf(to_number(&exp_val)?)))
    }

    fn func_sqrt(&mut self, args_str: &str) -> Result<CellValue, String> {
        let val = self.evaluate_expr(args_str)?;
        let n = to_number(&val)?;
        if n < 0.0 { return Ok(CellValue::Error(CellError::Num)); }
        Ok(CellValue::Number(n.sqrt()))
    }

    fn func_and(&mut self, args_str: &str) -> Result<CellValue, String> {
        for arg in split_args(args_str) {
            let val = self.evaluate_expr(&arg)?;
            if !to_bool(&val)? { return Ok(CellValue::Boolean(false)); }
        }
        Ok(CellValue::Boolean(true))
    }

    fn func_or(&mut self, args_str: &str) -> Result<CellValue, String> {
        for arg in split_args(args_str) {
            let val = self.evaluate_expr(&arg)?;
            if to_bool(&val)? { return Ok(CellValue::Boolean(true)); }
        }
        Ok(CellValue::Boolean(false))
    }

    fn func_not(&mut self, args_str: &str) -> Result<CellValue, String> {
        let val = self.evaluate_expr(args_str)?;
        Ok(CellValue::Boolean(!to_bool(&val)?))
    }

    fn func_concat(&mut self, args_str: &str) -> Result<CellValue, String> {
        let mut result = String::new();
        for arg in split_args(args_str) {
            let val = self.evaluate_expr(&arg)?;
            result.push_str(&to_string(&val));
        }
        Ok(CellValue::Text(result))
    }

    fn func_iferror(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 2 { return Err("#VALUE!".to_string()); }
        match self.evaluate_expr(&args[0]) {
            Ok(CellValue::Error(_)) | Err(_) => self.evaluate_expr(&args[1]),
            Ok(val) => Ok(val),
        }
    }

    fn func_isblank(&mut self, args_str: &str) -> Result<CellValue, String> {
        Ok(CellValue::Boolean(matches!(self.evaluate_expr(args_str)?, CellValue::Empty)))
    }

    fn func_isnumber(&mut self, args_str: &str) -> Result<CellValue, String> {
        Ok(CellValue::Boolean(matches!(self.evaluate_expr(args_str)?, CellValue::Number(_))))
    }

    fn func_istext(&mut self, args_str: &str) -> Result<CellValue, String> {
        Ok(CellValue::Boolean(matches!(self.evaluate_expr(args_str)?, CellValue::Text(_))))
    }

    // --- Multi-criteria aggregates ---

    fn ifs_matching_indices(&mut self, args: &[String], skip_first: usize) -> Result<Vec<usize>, String> {
        // Expects pairs (range, criteria) starting at index `skip_first`.
        let pair_args = &args[skip_first..];
        if pair_args.is_empty() || pair_args.len() % 2 != 0 {
            return Err("#VALUE!".to_string());
        }
        let mut indices: Option<Vec<usize>> = None;
        for chunk in pair_args.chunks(2) {
            let range = self.parse_range(&chunk[0])?;
            let criteria_val = self.evaluate_expr(&chunk[1])?;
            let criteria = to_string(&criteria_val);
            let criteria = criteria.trim_matches('"').to_string();
            let mut local = Vec::new();
            for (i, (col, row)) in range.iter().enumerate() {
                if self.matches_criteria(*col, *row, &criteria)? {
                    local.push(i);
                }
            }
            indices = Some(match indices {
                None => local,
                Some(prev) => prev.into_iter().filter(|i| local.contains(i)).collect(),
            });
        }
        Ok(indices.unwrap_or_default())
    }

    fn func_sumifs(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 3 || (args.len() - 1) % 2 != 0 { return Err("#VALUE!".to_string()); }
        let sum_range = self.parse_range(&args[0])?;
        let idxs = self.ifs_matching_indices(&args, 1)?;
        let mut sum = 0.0;
        for i in idxs {
            if let Some((c, r)) = sum_range.get(i) {
                if let Ok(CellValue::Number(n)) = self.evaluate_cell(*c, *r) { sum += n; }
            }
        }
        Ok(CellValue::Number(sum))
    }

    fn func_countifs(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 2 || args.len() % 2 != 0 { return Err("#VALUE!".to_string()); }
        let idxs = self.ifs_matching_indices(&args, 0)?;
        Ok(CellValue::Number(idxs.len() as f64))
    }

    fn func_averageifs(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 3 || (args.len() - 1) % 2 != 0 { return Err("#VALUE!".to_string()); }
        let avg_range = self.parse_range(&args[0])?;
        let idxs = self.ifs_matching_indices(&args, 1)?;
        let mut sum = 0.0; let mut count = 0;
        for i in idxs {
            if let Some((c, r)) = avg_range.get(i) {
                if let Ok(CellValue::Number(n)) = self.evaluate_cell(*c, *r) { sum += n; count += 1; }
            }
        }
        if count == 0 { Ok(CellValue::Error(CellError::DivZero)) }
        else { Ok(CellValue::Number(sum / count as f64)) }
    }

    // --- Rounding ---

    fn func_roundup(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.is_empty() { return Err("#VALUE!".to_string()); }
        let val = self.evaluate_expr(&args[0])?;
        let n = to_number(&val)?;
        let digits = if args.len() > 1 { let v = self.evaluate_expr(&args[1])?; to_number(&v)? as i32 } else { 0 };
        let factor = 10f64.powi(digits);
        let scaled = n * factor;
        let rounded = if scaled >= 0.0 { scaled.ceil() } else { scaled.floor() };
        Ok(CellValue::Number(rounded / factor))
    }

    fn func_rounddown(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.is_empty() { return Err("#VALUE!".to_string()); }
        let val = self.evaluate_expr(&args[0])?;
        let n = to_number(&val)?;
        let digits = if args.len() > 1 { let v = self.evaluate_expr(&args[1])?; to_number(&v)? as i32 } else { 0 };
        let factor = 10f64.powi(digits);
        let scaled = n * factor;
        let rounded = if scaled >= 0.0 { scaled.floor() } else { scaled.ceil() };
        Ok(CellValue::Number(rounded / factor))
    }

    fn func_ceiling(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 2 { return Err("#VALUE!".to_string()); }
        let n_val = self.evaluate_expr(&args[0])?;
        let s_val = self.evaluate_expr(&args[1])?;
        let n = to_number(&n_val)?;
        let s = to_number(&s_val)?;
        if s == 0.0 { return Ok(CellValue::Number(0.0)); }
        if n.signum() != s.signum() && n != 0.0 { return Ok(CellValue::Error(CellError::Num)); }
        Ok(CellValue::Number((n / s).ceil() * s))
    }

    fn func_floor(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 2 { return Err("#VALUE!".to_string()); }
        let n_val = self.evaluate_expr(&args[0])?;
        let s_val = self.evaluate_expr(&args[1])?;
        let n = to_number(&n_val)?;
        let s = to_number(&s_val)?;
        if s == 0.0 { return Ok(CellValue::Error(CellError::DivZero)); }
        if n.signum() != s.signum() && n != 0.0 { return Ok(CellValue::Error(CellError::Num)); }
        Ok(CellValue::Number((n / s).floor() * s))
    }

    // --- Trig ---

    fn one_number(&mut self, args_str: &str) -> Result<f64, String> {
        let val = self.evaluate_expr(args_str)?;
        to_number(&val)
    }

    fn func_sin(&mut self, args_str: &str) -> Result<CellValue, String> { Ok(CellValue::Number(self.one_number(args_str)?.sin())) }
    fn func_cos(&mut self, args_str: &str) -> Result<CellValue, String> { Ok(CellValue::Number(self.one_number(args_str)?.cos())) }
    fn func_tan(&mut self, args_str: &str) -> Result<CellValue, String> { Ok(CellValue::Number(self.one_number(args_str)?.tan())) }
    fn func_asin(&mut self, args_str: &str) -> Result<CellValue, String> {
        let n = self.one_number(args_str)?;
        if !(-1.0..=1.0).contains(&n) { return Ok(CellValue::Error(CellError::Num)); }
        Ok(CellValue::Number(n.asin()))
    }
    fn func_acos(&mut self, args_str: &str) -> Result<CellValue, String> {
        let n = self.one_number(args_str)?;
        if !(-1.0..=1.0).contains(&n) { return Ok(CellValue::Error(CellError::Num)); }
        Ok(CellValue::Number(n.acos()))
    }
    fn func_atan(&mut self, args_str: &str) -> Result<CellValue, String> { Ok(CellValue::Number(self.one_number(args_str)?.atan())) }
    fn func_atan2(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 2 { return Err("#VALUE!".to_string()); }
        let x = to_number(&self.evaluate_expr(&args[0])?)?;
        let y = to_number(&self.evaluate_expr(&args[1])?)?;
        // Excel's ATAN2(x, y) returns atan(y/x); std uses (y, x) order.
        Ok(CellValue::Number(y.atan2(x)))
    }
    fn func_radians(&mut self, args_str: &str) -> Result<CellValue, String> { Ok(CellValue::Number(self.one_number(args_str)?.to_radians())) }
    fn func_degrees(&mut self, args_str: &str) -> Result<CellValue, String> { Ok(CellValue::Number(self.one_number(args_str)?.to_degrees())) }

    // --- Log / Exp / PI ---

    fn func_ln(&mut self, args_str: &str) -> Result<CellValue, String> {
        let n = self.one_number(args_str)?;
        if n <= 0.0 { return Ok(CellValue::Error(CellError::Num)); }
        Ok(CellValue::Number(n.ln()))
    }
    fn func_log10(&mut self, args_str: &str) -> Result<CellValue, String> {
        let n = self.one_number(args_str)?;
        if n <= 0.0 { return Ok(CellValue::Error(CellError::Num)); }
        Ok(CellValue::Number(n.log10()))
    }
    fn func_log(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.is_empty() { return Err("#VALUE!".to_string()); }
        let n = to_number(&self.evaluate_expr(&args[0])?)?;
        if n <= 0.0 { return Ok(CellValue::Error(CellError::Num)); }
        let base = if args.len() > 1 { to_number(&self.evaluate_expr(&args[1])?)? } else { 10.0 };
        if base <= 0.0 || base == 1.0 { return Ok(CellValue::Error(CellError::Num)); }
        Ok(CellValue::Number(n.log(base)))
    }
    fn func_exp(&mut self, args_str: &str) -> Result<CellValue, String> { Ok(CellValue::Number(self.one_number(args_str)?.exp())) }
    fn func_pi(&mut self, _args_str: &str) -> Result<CellValue, String> { Ok(CellValue::Number(std::f64::consts::PI)) }

    // --- Statistical ---

    fn func_stdev(&mut self, args_str: &str) -> Result<CellValue, String> {
        let v = self.get_numeric_values(args_str)?;
        if v.len() < 2 { return Ok(CellValue::Error(CellError::DivZero)); }
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        let var = v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (v.len() as f64 - 1.0);
        Ok(CellValue::Number(var.sqrt()))
    }
    fn func_var(&mut self, args_str: &str) -> Result<CellValue, String> {
        let v = self.get_numeric_values(args_str)?;
        if v.len() < 2 { return Ok(CellValue::Error(CellError::DivZero)); }
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        let var = v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (v.len() as f64 - 1.0);
        Ok(CellValue::Number(var))
    }
    fn func_median(&mut self, args_str: &str) -> Result<CellValue, String> {
        let mut v = self.get_numeric_values(args_str)?;
        if v.is_empty() { return Ok(CellValue::Error(CellError::Num)); }
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = v.len();
        let m = if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 };
        Ok(CellValue::Number(m))
    }
    fn func_mode(&mut self, args_str: &str) -> Result<CellValue, String> {
        let v = self.get_numeric_values(args_str)?;
        if v.is_empty() { return Ok(CellValue::Error(CellError::NA)); }
        let mut best_val = 0.0; let mut best_count = 0;
        for (i, x) in v.iter().enumerate() {
            let count = v.iter().filter(|y| (*y - x).abs() < 1e-12).count();
            if count > best_count || (count == best_count && i == 0) {
                best_val = *x; best_count = count;
            }
        }
        if best_count < 2 { return Ok(CellValue::Error(CellError::NA)); }
        Ok(CellValue::Number(best_val))
    }

    // --- Random / multiples ---

    fn func_rand(&mut self, _args_str: &str) -> Result<CellValue, String> {
        Ok(CellValue::Number(next_random()))
    }
    fn func_randbetween(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 2 { return Err("#VALUE!".to_string()); }
        let lo = to_number(&self.evaluate_expr(&args[0])?)?.floor() as i64;
        let hi = to_number(&self.evaluate_expr(&args[1])?)?.floor() as i64;
        if hi < lo { return Ok(CellValue::Error(CellError::Num)); }
        let range = (hi - lo + 1) as f64;
        let pick = (next_random() * range).floor() as i64;
        Ok(CellValue::Number((lo + pick) as f64))
    }
    fn func_gcd(&mut self, args_str: &str) -> Result<CellValue, String> {
        let vals = self.get_numeric_values(args_str)?;
        if vals.is_empty() { return Err("#VALUE!".to_string()); }
        let mut acc: u64 = 0;
        for v in vals {
            if v < 0.0 { return Ok(CellValue::Error(CellError::Num)); }
            acc = gcd_u64(acc, v.floor() as u64);
        }
        Ok(CellValue::Number(acc as f64))
    }
    fn func_lcm(&mut self, args_str: &str) -> Result<CellValue, String> {
        let vals = self.get_numeric_values(args_str)?;
        if vals.is_empty() { return Err("#VALUE!".to_string()); }
        let mut acc: u64 = 1;
        for v in vals {
            if v < 0.0 { return Ok(CellValue::Error(CellError::Num)); }
            let n = v.floor() as u64;
            if n == 0 { return Ok(CellValue::Number(0.0)); }
            let g = gcd_u64(acc, n);
            acc = acc / g * n;
        }
        Ok(CellValue::Number(acc as f64))
    }
    fn func_fact(&mut self, args_str: &str) -> Result<CellValue, String> {
        let n = self.one_number(args_str)?.floor();
        if n < 0.0 || n > 170.0 { return Ok(CellValue::Error(CellError::Num)); }
        let mut acc: f64 = 1.0;
        for i in 2..=(n as u64) { acc *= i as f64; }
        Ok(CellValue::Number(acc))
    }

    // --- Date / Time ---

    fn func_today(&mut self, _args_str: &str) -> Result<CellValue, String> {
        Ok(CellValue::Number(date_util::today_serial()))
    }
    fn func_now(&mut self, _args_str: &str) -> Result<CellValue, String> {
        Ok(CellValue::Number(date_util::now_serial()))
    }
    fn func_date(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 3 { return Err("#VALUE!".to_string()); }
        let y = to_number(&self.evaluate_expr(&args[0])?)? as i32;
        let m = to_number(&self.evaluate_expr(&args[1])?)? as i32;
        let d = to_number(&self.evaluate_expr(&args[2])?)? as i32;
        // Excel: years 0-1899 add 1900. Years >= 1900 are literal.
        let y = if (0..1900).contains(&y) { y + 1900 } else { y };
        // Normalize month/day overflow Excel-style by using NaiveDate arithmetic.
        let base = NaiveDate::from_ymd_opt(y, 1, 1).ok_or("#NUM!")?;
        let months_offset = m - 1;
        let date = add_months(base, months_offset).ok_or("#NUM!")?;
        let date = date.checked_add_signed(Duration::days((d - 1) as i64)).ok_or("#NUM!")?;
        Ok(CellValue::Number(date_util::date_to_serial(date)))
    }
    fn func_year(&mut self, args_str: &str) -> Result<CellValue, String> {
        let s = self.one_number(args_str)?;
        let d = date_util::serial_to_date(s).ok_or("#NUM!")?;
        Ok(CellValue::Number(d.year() as f64))
    }
    fn func_month(&mut self, args_str: &str) -> Result<CellValue, String> {
        let s = self.one_number(args_str)?;
        let d = date_util::serial_to_date(s).ok_or("#NUM!")?;
        Ok(CellValue::Number(d.month() as f64))
    }
    fn func_day(&mut self, args_str: &str) -> Result<CellValue, String> {
        let s = self.one_number(args_str)?;
        let d = date_util::serial_to_date(s).ok_or("#NUM!")?;
        Ok(CellValue::Number(d.day() as f64))
    }
    fn func_hour(&mut self, args_str: &str) -> Result<CellValue, String> {
        let s = self.one_number(args_str)?;
        let frac = s - s.floor();
        let secs = (frac * 86400.0).round() as i64 % 86400;
        Ok(CellValue::Number((secs / 3600) as f64))
    }
    fn func_minute(&mut self, args_str: &str) -> Result<CellValue, String> {
        let s = self.one_number(args_str)?;
        let frac = s - s.floor();
        let secs = (frac * 86400.0).round() as i64 % 86400;
        Ok(CellValue::Number(((secs % 3600) / 60) as f64))
    }
    fn func_second(&mut self, args_str: &str) -> Result<CellValue, String> {
        let s = self.one_number(args_str)?;
        let frac = s - s.floor();
        let secs = (frac * 86400.0).round() as i64 % 86400;
        Ok(CellValue::Number((secs % 60) as f64))
    }
    fn func_time(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 3 { return Err("#VALUE!".to_string()); }
        let h = to_number(&self.evaluate_expr(&args[0])?)?;
        let m = to_number(&self.evaluate_expr(&args[1])?)?;
        let s = to_number(&self.evaluate_expr(&args[2])?)?;
        let total = h * 3600.0 + m * 60.0 + s;
        Ok(CellValue::Number((total / 86400.0).rem_euclid(1.0)))
    }
    fn func_weekday(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.is_empty() { return Err("#VALUE!".to_string()); }
        let s = to_number(&self.evaluate_expr(&args[0])?)?;
        let mode = if args.len() > 1 { to_number(&self.evaluate_expr(&args[1])?)? as i32 } else { 1 };
        let d = date_util::serial_to_date(s).ok_or("#NUM!")?;
        // Sun=0..Sat=6
        let dow = d.weekday().num_days_from_sunday() as i32;
        let result = match mode {
            1 => dow + 1,           // 1=Sun..7=Sat
            2 => ((dow + 6) % 7) + 1, // 1=Mon..7=Sun
            3 => (dow + 6) % 7,     // 0=Mon..6=Sun
            _ => return Ok(CellValue::Error(CellError::Num)),
        };
        Ok(CellValue::Number(result as f64))
    }
    fn func_weeknum(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.is_empty() { return Err("#VALUE!".to_string()); }
        let s = to_number(&self.evaluate_expr(&args[0])?)?;
        let start_dow_input = if args.len() > 1 { to_number(&self.evaluate_expr(&args[1])?)? as i32 } else { 1 };
        let d = date_util::serial_to_date(s).ok_or("#NUM!")?;
        // start_dow_input: 1 = week starts Sun, 2 = week starts Mon
        let start_dow: u32 = match start_dow_input { 1 => 0, 2 => 1, _ => return Ok(CellValue::Error(CellError::Num)) };
        let jan1 = NaiveDate::from_ymd_opt(d.year(), 1, 1).ok_or("#NUM!")?;
        let jan1_dow = jan1.weekday().num_days_from_sunday();
        let offset = (7 + jan1_dow - start_dow) % 7;
        let day_of_year = d.ordinal0() as i64;
        let week = (day_of_year + offset as i64) / 7 + 1;
        Ok(CellValue::Number(week as f64))
    }
    fn func_datedif(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 3 { return Err("#VALUE!".to_string()); }
        let s1 = to_number(&self.evaluate_expr(&args[0])?)?;
        let s2 = to_number(&self.evaluate_expr(&args[1])?)?;
        let unit_val = self.evaluate_expr(&args[2])?;
        let unit = to_string(&unit_val).to_uppercase();
        let unit = unit.trim_matches('"').to_string();
        let d1 = date_util::serial_to_date(s1).ok_or("#NUM!")?;
        let d2 = date_util::serial_to_date(s2).ok_or("#NUM!")?;
        if d2 < d1 { return Ok(CellValue::Error(CellError::Num)); }
        let result = match unit.as_str() {
            "D" => (d2 - d1).num_days() as f64,
            "M" => {
                let mut months = (d2.year() - d1.year()) * 12 + (d2.month() as i32 - d1.month() as i32);
                if d2.day() < d1.day() { months -= 1; }
                months as f64
            }
            "Y" => {
                let mut years = d2.year() - d1.year();
                if (d2.month(), d2.day()) < (d1.month(), d1.day()) { years -= 1; }
                years as f64
            }
            "MD" => {
                let day_in_d2_month = d2.day() as i32;
                let day_in_d1 = d1.day() as i32;
                if day_in_d2_month >= day_in_d1 {
                    (day_in_d2_month - day_in_d1) as f64
                } else {
                    // borrow from previous month: days in (d2.month-1)
                    let prev_month_days = days_in_month(d2.year(), if d2.month() == 1 { 12 } else { d2.month() - 1 }) as i32;
                    (prev_month_days - day_in_d1 + day_in_d2_month) as f64
                }
            }
            "YM" => {
                let mut m = d2.month() as i32 - d1.month() as i32;
                if d2.day() < d1.day() { m -= 1; }
                ((m + 12) % 12) as f64
            }
            "YD" => {
                let anniversary = NaiveDate::from_ymd_opt(d2.year(), d1.month(), d1.day().min(28))
                    .unwrap_or(d1);
                let mut diff = (d2 - anniversary).num_days();
                if diff < 0 {
                    let prev = NaiveDate::from_ymd_opt(d2.year() - 1, d1.month(), d1.day().min(28))
                        .unwrap_or(d1);
                    diff = (d2 - prev).num_days();
                }
                diff as f64
            }
            _ => return Ok(CellValue::Error(CellError::Num)),
        };
        Ok(CellValue::Number(result))
    }
    fn func_edate(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 2 { return Err("#VALUE!".to_string()); }
        let s = to_number(&self.evaluate_expr(&args[0])?)?;
        let months = to_number(&self.evaluate_expr(&args[1])?)? as i32;
        let d = date_util::serial_to_date(s).ok_or("#NUM!")?;
        let new = add_months(d, months).ok_or("#NUM!")?;
        Ok(CellValue::Number(date_util::date_to_serial(new)))
    }
    fn func_eomonth(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 2 { return Err("#VALUE!".to_string()); }
        let s = to_number(&self.evaluate_expr(&args[0])?)?;
        let months = to_number(&self.evaluate_expr(&args[1])?)? as i32;
        let d = date_util::serial_to_date(s).ok_or("#NUM!")?;
        let shifted = add_months(d.with_day(1).ok_or("#NUM!")?, months).ok_or("#NUM!")?;
        let last_day = days_in_month(shifted.year(), shifted.month());
        let eom = NaiveDate::from_ymd_opt(shifted.year(), shifted.month(), last_day).ok_or("#NUM!")?;
        Ok(CellValue::Number(date_util::date_to_serial(eom)))
    }
    fn func_days(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 2 { return Err("#VALUE!".to_string()); }
        let end = to_number(&self.evaluate_expr(&args[0])?)?;
        let start = to_number(&self.evaluate_expr(&args[1])?)?;
        Ok(CellValue::Number((end.floor() - start.floor()) as f64))
    }

    // --- Financial ---
    //
    // Excel cash-flow convention: money paid OUT is negative, money received
    // is positive. `type` = 0 (default) means end-of-period payments,
    // `type` = 1 means beginning-of-period.

    fn func_pmt(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 3 { return Err("#VALUE!".to_string()); }
        let rate = to_number(&self.evaluate_expr(&args[0])?)?;
        let nper = to_number(&self.evaluate_expr(&args[1])?)?;
        let pv = to_number(&self.evaluate_expr(&args[2])?)?;
        let fv = if args.len() > 3 { to_number(&self.evaluate_expr(&args[3])?)? } else { 0.0 };
        let ty = if args.len() > 4 { to_number(&self.evaluate_expr(&args[4])?)? } else { 0.0 };
        let pmt = if rate == 0.0 {
            -(pv + fv) / nper
        } else {
            let r1 = (1.0 + rate).powf(nper);
            -(rate * (pv * r1 + fv)) / ((r1 - 1.0) * (1.0 + rate * ty))
        };
        Ok(CellValue::Number(pmt))
    }
    fn func_pv(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 3 { return Err("#VALUE!".to_string()); }
        let rate = to_number(&self.evaluate_expr(&args[0])?)?;
        let nper = to_number(&self.evaluate_expr(&args[1])?)?;
        let pmt = to_number(&self.evaluate_expr(&args[2])?)?;
        let fv = if args.len() > 3 { to_number(&self.evaluate_expr(&args[3])?)? } else { 0.0 };
        let ty = if args.len() > 4 { to_number(&self.evaluate_expr(&args[4])?)? } else { 0.0 };
        let pv = if rate == 0.0 {
            -(fv + pmt * nper)
        } else {
            let r1 = (1.0 + rate).powf(nper);
            -(fv + pmt * (1.0 + rate * ty) * (r1 - 1.0) / rate) / r1
        };
        Ok(CellValue::Number(pv))
    }
    fn func_fv(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 3 { return Err("#VALUE!".to_string()); }
        let rate = to_number(&self.evaluate_expr(&args[0])?)?;
        let nper = to_number(&self.evaluate_expr(&args[1])?)?;
        let pmt = to_number(&self.evaluate_expr(&args[2])?)?;
        let pv = if args.len() > 3 { to_number(&self.evaluate_expr(&args[3])?)? } else { 0.0 };
        let ty = if args.len() > 4 { to_number(&self.evaluate_expr(&args[4])?)? } else { 0.0 };
        let fv = if rate == 0.0 {
            -(pv + pmt * nper)
        } else {
            let r1 = (1.0 + rate).powf(nper);
            -(pv * r1 + pmt * (1.0 + rate * ty) * (r1 - 1.0) / rate)
        };
        Ok(CellValue::Number(fv))
    }
    fn func_nper(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 3 { return Err("#VALUE!".to_string()); }
        let rate = to_number(&self.evaluate_expr(&args[0])?)?;
        let pmt = to_number(&self.evaluate_expr(&args[1])?)?;
        let pv = to_number(&self.evaluate_expr(&args[2])?)?;
        let fv = if args.len() > 3 { to_number(&self.evaluate_expr(&args[3])?)? } else { 0.0 };
        let ty = if args.len() > 4 { to_number(&self.evaluate_expr(&args[4])?)? } else { 0.0 };
        if rate == 0.0 {
            if pmt == 0.0 { return Ok(CellValue::Error(CellError::Num)); }
            return Ok(CellValue::Number(-(pv + fv) / pmt));
        }
        let adj = pmt * (1.0 + rate * ty) / rate;
        let arg = (adj - fv) / (pv + adj);
        if arg <= 0.0 { return Ok(CellValue::Error(CellError::Num)); }
        Ok(CellValue::Number(arg.ln() / (1.0 + rate).ln()))
    }
    fn func_rate(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 3 { return Err("#VALUE!".to_string()); }
        let nper = to_number(&self.evaluate_expr(&args[0])?)?;
        let pmt = to_number(&self.evaluate_expr(&args[1])?)?;
        let pv = to_number(&self.evaluate_expr(&args[2])?)?;
        let fv = if args.len() > 3 { to_number(&self.evaluate_expr(&args[3])?)? } else { 0.0 };
        let ty = if args.len() > 4 { to_number(&self.evaluate_expr(&args[4])?)? } else { 0.0 };
        let guess = if args.len() > 5 { to_number(&self.evaluate_expr(&args[5])?)? } else { 0.1 };
        // Newton-Raphson on f(r) = pv*(1+r)^n + pmt*(1 + r*ty)*((1+r)^n - 1)/r + fv
        let mut r = guess;
        for _ in 0..100 {
            let f = pv_at_rate(r, nper, pmt, pv, fv, ty);
            // numerical derivative
            let dr = 1e-6_f64.max(r.abs() * 1e-6);
            let f1 = pv_at_rate(r + dr, nper, pmt, pv, fv, ty);
            let slope = (f1 - f) / dr;
            if slope.abs() < 1e-14 { return Ok(CellValue::Error(CellError::Num)); }
            let r_next = r - f / slope;
            if (r_next - r).abs() < 1e-10 { return Ok(CellValue::Number(r_next)); }
            r = r_next;
        }
        Ok(CellValue::Error(CellError::Num))
    }
    fn func_npv(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.len() < 2 { return Err("#VALUE!".to_string()); }
        let rate = to_number(&self.evaluate_expr(&args[0])?)?;
        let mut flows: Vec<f64> = Vec::new();
        for arg in args.iter().skip(1) {
            if arg.contains(':') {
                for (col, row) in self.parse_range(arg)? {
                    if let Ok(CellValue::Number(n)) = self.evaluate_cell(col, row) { flows.push(n); }
                }
            } else if let Some((c, r, _, _)) = formula::parse_cell_ref(arg) {
                if let Ok(CellValue::Number(n)) = self.evaluate_cell(c, r) { flows.push(n); }
            } else if let Ok(n) = arg.parse::<f64>() { flows.push(n); }
        }
        let mut npv = 0.0;
        for (i, c) in flows.iter().enumerate() {
            npv += c / (1.0 + rate).powi(i as i32 + 1);
        }
        Ok(CellValue::Number(npv))
    }
    fn func_irr(&mut self, args_str: &str) -> Result<CellValue, String> {
        let args = split_args(args_str);
        if args.is_empty() { return Err("#VALUE!".to_string()); }
        let mut flows: Vec<f64> = Vec::new();
        if args[0].contains(':') {
            for (col, row) in self.parse_range(&args[0])? {
                if let Ok(CellValue::Number(n)) = self.evaluate_cell(col, row) { flows.push(n); }
            }
        } else if let Some((c, r, _, _)) = formula::parse_cell_ref(&args[0]) {
            if let Ok(CellValue::Number(n)) = self.evaluate_cell(c, r) { flows.push(n); }
        }
        if flows.len() < 2 { return Ok(CellValue::Error(CellError::Num)); }
        let guess = if args.len() > 1 { to_number(&self.evaluate_expr(&args[1])?)? } else { 0.1 };
        let mut r = guess;
        for _ in 0..200 {
            let mut f = 0.0; let mut df = 0.0;
            for (i, c) in flows.iter().enumerate() {
                let denom = (1.0 + r).powi(i as i32);
                f += c / denom;
                if i > 0 { df -= (i as f64) * c / denom / (1.0 + r); }
            }
            if df.abs() < 1e-14 { return Ok(CellValue::Error(CellError::Num)); }
            let r_next = r - f / df;
            if (r_next - r).abs() < 1e-10 { return Ok(CellValue::Number(r_next)); }
            r = r_next;
        }
        Ok(CellValue::Error(CellError::Num))
    }
}

// Free functions
fn split_args(args_str: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    let mut in_string = false;
    for c in args_str.chars() {
        match c {
            '"' => { in_string = !in_string; current.push(c); }
            '(' if !in_string => { depth += 1; current.push(c); }
            ')' if !in_string => { depth -= 1; current.push(c); }
            ',' if depth == 0 && !in_string => { args.push(current.trim().to_string()); current = String::new(); }
            _ => current.push(c),
        }
    }
    if !current.is_empty() { args.push(current.trim().to_string()); }
    args
}

fn to_number(val: &CellValue) -> Result<f64, String> {
    match val {
        CellValue::Number(n) => Ok(*n),
        CellValue::Boolean(b) => Ok(if *b { 1.0 } else { 0.0 }),
        CellValue::Empty => Ok(0.0),
        CellValue::Text(s) => s.parse().map_err(|_| "#VALUE!".to_string()),
        CellValue::Error(e) => Err(e.to_string().to_string()),
        CellValue::Formula(_) => Err("#VALUE!".to_string()),
    }
}

fn to_string(val: &CellValue) -> String {
    match val {
        CellValue::Number(n) => if *n == n.floor() && n.abs() < 1e10 { format!("{:.0}", n) } else { format!("{}", n) },
        CellValue::Text(s) => s.clone(),
        CellValue::Boolean(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        CellValue::Empty => String::new(),
        CellValue::Error(e) => e.to_string().to_string(),
        CellValue::Formula(_) => String::new(),
    }
}

fn to_bool(val: &CellValue) -> Result<bool, String> {
    match val {
        CellValue::Boolean(b) => Ok(*b),
        CellValue::Number(n) => Ok(*n != 0.0),
        CellValue::Text(s) => {
            if s.eq_ignore_ascii_case("true") { Ok(true) }
            else if s.eq_ignore_ascii_case("false") { Ok(false) }
            else { Err("#VALUE!".to_string()) }
        }
        _ => Err("#VALUE!".to_string()),
    }
}

fn cell_eq(a: &CellValue, b: &CellValue) -> bool {
    match (a, b) {
        (CellValue::Number(l), CellValue::Number(r)) => approx_eq(*l, *r),
        (CellValue::Text(l), CellValue::Text(r)) => l.to_uppercase() == r.to_uppercase(),
        _ => false,
    }
}

/// Approximate equality for f64 spreadsheet values. Uses a relative tolerance
/// of ~1e-12 (≈15 significant digits — what Excel exposes) with a small
/// absolute floor so values near zero compare cleanly. Used by every numeric
/// `=` / `<>` / `>=` / `<=` / SUMIF criteria / VLOOKUP exact match in the
/// engine so that `0.1 + 0.2` equals `0.3`, etc.
pub fn approx_eq(a: f64, b: f64) -> bool {
    if a == b { return true; }
    if !a.is_finite() || !b.is_finite() { return false; }
    let diff = (a - b).abs();
    let scale = a.abs().max(b.abs()).max(1.0);
    diff <= scale * 1e-12
}

fn cell_lte(a: &CellValue, b: &CellValue) -> bool {
    match (a, b) {
        (CellValue::Number(l), CellValue::Number(r)) => l <= r,
        _ => false,
    }
}

fn find_operator(expr: &str, op: &str) -> Option<usize> {
    let mut depth = 0;
    let mut in_string = false;
    let chars: Vec<char> = expr.chars().collect();
    let op_chars: Vec<char> = op.chars().collect();
    for i in 0..chars.len() {
        if chars[i] == '"' { in_string = !in_string; }
        else if !in_string {
            if chars[i] == '(' { depth += 1; }
            else if chars[i] == ')' { depth -= 1; }
            else if depth == 0 && i + op_chars.len() <= chars.len() {
                if chars[i..i + op_chars.len()].iter().zip(op_chars.iter()).all(|(a, b)| a == b) {
                    return Some(i);
                }
            }
        }
    }
    None
}

fn find_operator_rtl(expr: &str, ops: &[char]) -> Option<usize> {
    let mut depth = 0;
    let mut in_string = false;
    let chars: Vec<char> = expr.chars().collect();
    for i in (0..chars.len()).rev() {
        if chars[i] == '"' { in_string = !in_string; }
        else if !in_string {
            if chars[i] == ')' { depth += 1; }
            else if chars[i] == '(' { depth -= 1; }
            else if depth == 0 && ops.contains(&chars[i]) {
                if (chars[i] == '+' || chars[i] == '-') && i > 0 && chars[i - 1].to_ascii_uppercase() == 'E' { continue; }
                if chars[i] == '-' && i == 0 { continue; }
                return Some(i);
            }
        }
    }
    None
}

fn find_matching_paren(expr: &str, start: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in expr.chars().enumerate().skip(start) {
        if c == '(' { depth += 1; }
        else if c == ')' { depth -= 1; if depth == 0 { return Some(i); } }
    }
    None
}

fn arithmetic(left: CellValue, right: CellValue, op: char) -> Result<CellValue, String> {
    let l = to_number(&left)?;
    let r = to_number(&right)?;
    let result = match op {
        '+' => l + r, '-' => l - r, '*' => l * r,
        '/' => { if r == 0.0 { return Ok(CellValue::Error(CellError::DivZero)); } l / r }
        _ => return Err("#VALUE!".to_string()),
    };
    Ok(CellValue::Number(result))
}

fn power(left: CellValue, right: CellValue) -> Result<CellValue, String> {
    Ok(CellValue::Number(to_number(&left)?.powf(to_number(&right)?)))
}

fn compare(left: CellValue, right: CellValue, op: &str) -> Result<CellValue, String> {
    let result = match (&left, &right) {
        (CellValue::Number(l), CellValue::Number(r)) => num_compare(*l, *r, op)?,
        (CellValue::Text(l), CellValue::Text(r)) => match op {
            "=" => l == r, "<>" | "!=" => l != r,
            ">" => l > r, "<" => l < r, ">=" => l >= r, "<=" => l <= r, _ => return Err("#VALUE!".to_string()),
        },
        _ => {
            if let (Ok(l), Ok(r)) = (to_number(&left), to_number(&right)) {
                num_compare(l, r, op)?
            } else { return Err("#VALUE!".to_string()); }
        }
    };
    Ok(CellValue::Boolean(result))
}

/// Numeric comparison with a relative tolerance on equality boundaries. Used
/// by every `=`, `<>`, `>=`, `<=`, `>`, `<` between two numbers in the
/// engine. See `approx_eq` for the tolerance.
fn num_compare(l: f64, r: f64, op: &str) -> Result<bool, String> {
    let eq = approx_eq(l, r);
    Ok(match op {
        "=" => eq,
        "<>" | "!=" => !eq,
        ">" => l > r && !eq,
        "<" => l < r && !eq,
        ">=" => l > r || eq,
        "<=" => l < r || eq,
        _ => return Err("#VALUE!".to_string()),
    })
}

// --- Helpers used by date / math / financial functions ---

fn gcd_u64(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd_u64(b, a % b) }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    };
    let first = NaiveDate::from_ymd_opt(year, month, 1);
    match (first, next) {
        (Some(f), Some(n)) => (n - f).num_days() as u32,
        _ => 30,
    }
}

fn add_months(date: NaiveDate, months: i32) -> Option<NaiveDate> {
    let total = date.year() * 12 + (date.month() as i32 - 1) + months;
    let year = total.div_euclid(12);
    let month = (total.rem_euclid(12) + 1) as u32;
    let last_day = days_in_month(year, month);
    NaiveDate::from_ymd_opt(year, month, date.day().min(last_day))
}

fn pv_at_rate(r: f64, n: f64, pmt: f64, pv: f64, fv: f64, ty: f64) -> f64 {
    if r == 0.0 {
        pv + pmt * n + fv
    } else {
        let r1 = (1.0 + r).powf(n);
        pv * r1 + pmt * (1.0 + r * ty) * (r1 - 1.0) / r + fv
    }
}

fn next_random() -> f64 {
    static STATE: OnceLock<Mutex<u64>> = OnceLock::new();
    let mutex = STATE.get_or_init(|| {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E3779B97F4A7C15);
        Mutex::new(seed | 1)
    });
    let mut s = mutex.lock().unwrap();
    // xorshift64*
    *s ^= *s >> 12;
    *s ^= *s << 25;
    *s ^= *s >> 27;
    let r = s.wrapping_mul(0x2545F4914F6CDD1D);
    (r >> 11) as f64 / (1u64 << 53) as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::{Cell, CellValue, parse_input};
    use std::collections::HashMap;

    fn cells_from(items: &[(usize, usize, &str)]) -> HashMap<(usize, usize), Cell> {
        let mut m = HashMap::new();
        for (c, r, s) in items {
            m.insert((*c, *r), Cell::new(s.to_string(), parse_input(s)));
        }
        m
    }

    fn eval(cells: &HashMap<(usize, usize), Cell>, formula: &str) -> CellValue {
        let mut e = Engine::new(cells);
        e.evaluate_formula(formula).unwrap_or(CellValue::Error(CellError::Value))
    }

    fn n(v: CellValue) -> f64 {
        if let CellValue::Number(x) = v { x } else { panic!("expected number, got {:?}", v) }
    }

    #[test]
    fn roundup_rounddown() {
        let m = HashMap::new();
        assert!((n(eval(&m, "=ROUNDUP(1.1, 0)")) - 2.0).abs() < 1e-9);
        assert!((n(eval(&m, "=ROUNDDOWN(1.9, 0)")) - 1.0).abs() < 1e-9);
        assert!((n(eval(&m, "=ROUNDUP(-1.1, 0)")) - -2.0).abs() < 1e-9);
        assert!((n(eval(&m, "=ROUNDDOWN(-1.9, 0)")) - -1.0).abs() < 1e-9);
        assert!((n(eval(&m, "=ROUNDUP(1.234, 1)")) - 1.3).abs() < 1e-9);
    }

    #[test]
    fn ceiling_floor() {
        let m = HashMap::new();
        assert!((n(eval(&m, "=CEILING(7, 5)")) - 10.0).abs() < 1e-9);
        assert!((n(eval(&m, "=FLOOR(7, 5)")) - 5.0).abs() < 1e-9);
        assert!((n(eval(&m, "=CEILING(2.3, 0.5)")) - 2.5).abs() < 1e-9);
    }

    #[test]
    fn sumifs_two_criteria() {
        // A1:A4 categories, B1:B4 regions, C1:C4 amounts
        let m = cells_from(&[
            (0, 0, "fruit"), (1, 0, "east"), (2, 0, "10"),
            (0, 1, "fruit"), (1, 1, "west"), (2, 1, "20"),
            (0, 2, "veg"),   (1, 2, "east"), (2, 2, "30"),
            (0, 3, "fruit"), (1, 3, "east"), (2, 3, "40"),
        ]);
        // fruit AND east → 10 + 40 = 50
        assert_eq!(n(eval(&m, "=SUMIFS(C1:C4, A1:A4, \"fruit\", B1:B4, \"east\")")), 50.0);
        // count fruit = 3
        assert_eq!(n(eval(&m, "=COUNTIFS(A1:A4, \"fruit\")")), 3.0);
        // avg of east category = (10 + 30 + 40)/3
        assert!((n(eval(&m, "=AVERAGEIFS(C1:C4, B1:B4, \"east\")")) - 80.0/3.0).abs() < 1e-9);
    }

    #[test]
    fn trig_and_pi() {
        let m = HashMap::new();
        assert!((n(eval(&m, "=SIN(0)")) - 0.0).abs() < 1e-9);
        assert!((n(eval(&m, "=COS(0)")) - 1.0).abs() < 1e-9);
        assert!((n(eval(&m, "=PI()")) - std::f64::consts::PI).abs() < 1e-12);
        assert!((n(eval(&m, "=RADIANS(180)")) - std::f64::consts::PI).abs() < 1e-12);
        assert!((n(eval(&m, "=DEGREES(PI())")) - 180.0).abs() < 1e-9);
    }

    #[test]
    fn log_exp() {
        let m = HashMap::new();
        assert!((n(eval(&m, "=LN(EXP(1))")) - 1.0).abs() < 1e-9);
        assert!((n(eval(&m, "=LOG10(1000)")) - 3.0).abs() < 1e-9);
        assert!((n(eval(&m, "=LOG(8, 2)")) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn stats_basic() {
        let m = cells_from(&[(0, 0, "1"), (0, 1, "2"), (0, 2, "3"), (0, 3, "4"), (0, 4, "5")]);
        assert!((n(eval(&m, "=MEDIAN(A1:A5)")) - 3.0).abs() < 1e-9);
        // sample stdev of 1..5 = sqrt(2.5)
        assert!((n(eval(&m, "=STDEV(A1:A5)")) - 2.5f64.sqrt()).abs() < 1e-9);
        assert!((n(eval(&m, "=VAR(A1:A5)")) - 2.5).abs() < 1e-9);
    }

    #[test]
    fn mode_picks_repeat() {
        let m = cells_from(&[(0, 0, "1"), (0, 1, "2"), (0, 2, "2"), (0, 3, "3")]);
        assert_eq!(n(eval(&m, "=MODE(A1:A4)")), 2.0);
    }

    #[test]
    fn gcd_lcm_fact() {
        let m = HashMap::new();
        assert_eq!(n(eval(&m, "=GCD(12, 18)")), 6.0);
        assert_eq!(n(eval(&m, "=LCM(4, 6)")), 12.0);
        assert_eq!(n(eval(&m, "=FACT(5)")), 120.0);
        assert_eq!(n(eval(&m, "=FACT(0)")), 1.0);
    }

    #[test]
    fn approx_eq_relative_tolerance() {
        // Classic 0.1 + 0.2 == 0.3 case
        assert!(approx_eq(0.1 + 0.2, 0.3));
        // Scale matters: 1e10 + 0.0001 should be equal to 1e10 at our tolerance
        assert!(approx_eq(1e10 + 0.0001, 1e10));
        // But a difference of 1 on top of 1e6 should NOT be equal
        assert!(!approx_eq(1_000_001.0, 1_000_000.0));
        // Zero comparisons use the absolute floor (scale clamped to 1.0)
        assert!(approx_eq(0.0, 1e-13));
        assert!(!approx_eq(0.0, 1e-6));
        // NaN is never equal to anything (IEEE 754 rule preserved)
        assert!(!approx_eq(f64::NAN, f64::NAN));
        assert!(!approx_eq(f64::NAN, 1.0));
        // Infinities compare via plain == (so INF == INF is true, -INF != INF)
        assert!(approx_eq(f64::INFINITY, f64::INFINITY));
        assert!(!approx_eq(f64::INFINITY, f64::NEG_INFINITY));
        assert!(!approx_eq(f64::INFINITY, 1e100));
    }

    #[test]
    fn fuzzy_equality_in_formulas() {
        let m = HashMap::new();
        // = on small-difference values returns TRUE
        assert_eq!(eval(&m, "=(0.1+0.2)=0.3"), CellValue::Boolean(true));
        // <> consistent with =
        assert_eq!(eval(&m, "=(0.1+0.2)<>0.3"), CellValue::Boolean(false));
        // >= and <= treat near-equal as equal (boundary)
        assert_eq!(eval(&m, "=(0.1+0.2)>=0.3"), CellValue::Boolean(true));
        assert_eq!(eval(&m, "=(0.1+0.2)<=0.3"), CellValue::Boolean(true));
        // Strict > / < should be FALSE on boundary
        assert_eq!(eval(&m, "=(0.1+0.2)>0.3"), CellValue::Boolean(false));
        assert_eq!(eval(&m, "=(0.1+0.2)<0.3"), CellValue::Boolean(false));
        // Genuine strict inequality still works
        assert_eq!(eval(&m, "=1>0.5"), CellValue::Boolean(true));
        assert_eq!(eval(&m, "=0.5<1"), CellValue::Boolean(true));
    }

    #[test]
    fn fuzzy_equality_in_criteria() {
        // SUMIF criteria "=0.3" should match a cell holding 0.1+0.2
        let m = cells_from(&[(0, 0, "=0.1+0.2"), (1, 0, "100")]);
        assert_eq!(n(eval(&m, "=SUMIF(A1:A1, 0.3, B1:B1)")), 100.0);
        assert_eq!(n(eval(&m, "=COUNTIF(A1:A1, 0.3)")), 1.0);
        // ">=" boundary
        assert_eq!(n(eval(&m, "=COUNTIF(A1:A1, \">=0.3\")")), 1.0);
        // Strict ">" boundary excludes equal
        assert_eq!(n(eval(&m, "=COUNTIF(A1:A1, \">0.3\")")), 0.0);
    }

    #[test]
    fn date_serial_powerbi_convention() {
        let m = HashMap::new();
        // Power BI / OLE Automation (clean Gregorian, NO 1900 leap year bug):
        //   1899-12-30 = 0
        //   1899-12-31 = 1
        //   1900-01-01 = 2
        //   1900-02-28 = 60
        //   1900-03-01 = 61   (matches Excel from here on)
        //   2024-01-01 = 45292
        assert_eq!(n(eval(&m, "=DATE(1900, 1, 1)")), 2.0);
        assert_eq!(n(eval(&m, "=DATE(1900, 2, 28)")), 60.0);
        assert_eq!(n(eval(&m, "=DATE(1900, 3, 1)")), 61.0);
        assert_eq!(n(eval(&m, "=DATE(2024, 1, 1)")), 45292.0);
        // There is no fake 1900-02-29 serial — DATE(1900, 2, 29) normalizes to 1900-03-01.
        assert_eq!(n(eval(&m, "=DATE(1900, 2, 29)")), 61.0);
        // Weekday of a known date: 2024-01-01 is a Monday.
        // WEEKDAY mode 2 (1=Mon..7=Sun) → 1
        assert_eq!(n(eval(&m, "=WEEKDAY(DATE(2024, 1, 1), 2)")), 1.0);
    }

    #[test]
    fn date_basic_roundtrip() {
        let m = HashMap::new();
        // DATE(2024, 1, 1) -> serial, then YEAR/MONTH/DAY should round-trip.
        let s = n(eval(&m, "=DATE(2024, 1, 1)"));
        let cells = HashMap::new();
        let mut e = Engine::new(&cells);
        let year = e.evaluate_formula(&format!("=YEAR({})", s)).unwrap();
        let mon = e.evaluate_formula(&format!("=MONTH({})", s)).unwrap();
        let day = e.evaluate_formula(&format!("=DAY({})", s)).unwrap();
        assert_eq!(n(year), 2024.0);
        assert_eq!(n(mon), 1.0);
        assert_eq!(n(day), 1.0);
    }

    #[test]
    fn time_components() {
        let m = HashMap::new();
        let t = n(eval(&m, "=TIME(13, 30, 0)"));
        assert!((t - (13.5 / 24.0)).abs() < 1e-9);
        let h = n(eval(&m, &format!("=HOUR({})", t)));
        let mi = n(eval(&m, &format!("=MINUTE({})", t)));
        assert_eq!(h, 13.0);
        assert_eq!(mi, 30.0);
    }

    #[test]
    fn datedif_units() {
        let m = HashMap::new();
        // 2020-01-01 to 2024-03-15
        let s1 = n(eval(&m, "=DATE(2020, 1, 1)"));
        let s2 = n(eval(&m, "=DATE(2024, 3, 15)"));
        let y = n(eval(&m, &format!("=DATEDIF({}, {}, \"Y\")", s1, s2)));
        let mo = n(eval(&m, &format!("=DATEDIF({}, {}, \"M\")", s1, s2)));
        let d = n(eval(&m, &format!("=DATEDIF({}, {}, \"D\")", s1, s2)));
        assert_eq!(y, 4.0);
        assert_eq!(mo, 50.0);
        assert_eq!(d, 1535.0);
    }

    #[test]
    fn edate_eomonth() {
        let m = HashMap::new();
        let s = n(eval(&m, "=DATE(2024, 1, 31)"));
        // EDATE +1 month from Jan 31 -> Feb 29 (2024 is leap)
        let ed = n(eval(&m, &format!("=EDATE({}, 1)", s)));
        let y = n(eval(&m, &format!("=YEAR({})", ed)));
        let mo = n(eval(&m, &format!("=MONTH({})", ed)));
        let d = n(eval(&m, &format!("=DAY({})", ed)));
        assert_eq!((y, mo, d), (2024.0, 2.0, 29.0));
        // EOMONTH from Jan 15, +1 month -> Feb 29 (2024)
        let s = n(eval(&m, "=DATE(2024, 1, 15)"));
        let eom = n(eval(&m, &format!("=EOMONTH({}, 1)", s)));
        let d = n(eval(&m, &format!("=DAY({})", eom)));
        assert_eq!(d, 29.0);
    }

    #[test]
    fn financial_pmt_pv_fv_consistency() {
        let m = HashMap::new();
        // Loan: borrow 100,000 at 5% annual / 12 monthly over 30 years.
        let pmt = n(eval(&m, "=PMT(0.05/12, 360, 100000)"));
        // standard monthly payment ~ -536.82
        assert!((pmt - (-536.82)).abs() < 0.01);
        // PV check
        let pv = n(eval(&m, &format!("=PV(0.05/12, 360, {})", pmt)));
        assert!((pv - 100000.0).abs() < 0.01);
        // FV at end should be ~0 (pv positive = loan received, pmt negative = payments out)
        let fv = n(eval(&m, &format!("=FV(0.05/12, 360, {}, 100000)", pmt)));
        assert!(fv.abs() < 0.01);
    }

    #[test]
    fn financial_nper_rate() {
        let m = HashMap::new();
        // 100k loan, -536.82 monthly @ 5%/12 -> 360 months
        let nper = n(eval(&m, "=NPER(0.05/12, -536.82, 100000)"));
        assert!((nper - 360.0).abs() < 0.1);
        // Same scenario solved for rate -> ~0.05/12
        let rate = n(eval(&m, "=RATE(360, -536.82, 100000)"));
        assert!((rate - 0.05/12.0).abs() < 1e-5);
    }

    #[test]
    fn npv_irr() {
        let m = cells_from(&[
            (0, 0, "-1000"), (0, 1, "300"), (0, 2, "400"), (0, 3, "500"),
        ]);
        // NPV at 10%; note Excel's NPV discounts first value at t=1.
        let npv = n(eval(&m, "=NPV(0.1, A1:A4)"));
        // Compute expected: sum of -1000/1.1 + 300/1.21 + 400/1.331 + 500/1.4641
        let expected = -1000.0/1.1 + 300.0/1.21 + 400.0/1.331 + 500.0/1.4641;
        assert!((npv - expected).abs() < 1e-6);
        // IRR should make NPV (Excel-style, first flow at t=0) zero
        let m2 = cells_from(&[
            (0, 0, "-1000"), (0, 1, "300"), (0, 2, "400"), (0, 3, "500"),
        ]);
        let irr = n(eval(&m2, "=IRR(A1:A4)"));
        // 8.21% ish
        assert!((irr - 0.0821).abs() < 0.01);
    }
}
