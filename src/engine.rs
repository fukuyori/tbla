use std::collections::{HashMap, HashSet};
use crate::cell::{Cell, CellValue, CellError};
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
                    result
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
            "INT" => self.func_int(args_str)?,
            "MOD" => self.func_mod(args_str)?,
            "POWER" => self.func_power(args_str)?,
            "SQRT" => self.func_sqrt(args_str)?,
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
                    return Ok(match op {
                        ">=" => n >= target, "<=" => n <= target,
                        "<>" | "!=" => (n - target).abs() >= f64::EPSILON,
                        ">" => n > target, "<" => n < target, _ => false,
                    });
                }
                return Ok(false);
            }
        }
        if let Ok(target) = criteria.parse::<f64>() {
            if let Ok(n) = to_number(&val) { return Ok((n - target).abs() < f64::EPSILON); }
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
        (CellValue::Number(l), CellValue::Number(r)) => (l - r).abs() < f64::EPSILON,
        (CellValue::Text(l), CellValue::Text(r)) => l.to_uppercase() == r.to_uppercase(),
        _ => false,
    }
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
        (CellValue::Number(l), CellValue::Number(r)) => match op {
            "=" => (l - r).abs() < f64::EPSILON, "<>" | "!=" => (l - r).abs() >= f64::EPSILON,
            ">" => l > r, "<" => l < r, ">=" => l >= r, "<=" => l <= r, _ => return Err("#VALUE!".to_string()),
        },
        (CellValue::Text(l), CellValue::Text(r)) => match op {
            "=" => l == r, "<>" | "!=" => l != r,
            ">" => l > r, "<" => l < r, ">=" => l >= r, "<=" => l <= r, _ => return Err("#VALUE!".to_string()),
        },
        _ => {
            if let (Ok(l), Ok(r)) = (to_number(&left), to_number(&right)) {
                match op {
                    "=" => (l - r).abs() < f64::EPSILON, "<>" | "!=" => (l - r).abs() >= f64::EPSILON,
                    ">" => l > r, "<" => l < r, ">=" => l >= r, "<=" => l <= r, _ => return Err("#VALUE!".to_string()),
                }
            } else { return Err("#VALUE!".to_string()); }
        }
    };
    Ok(CellValue::Boolean(result))
}
