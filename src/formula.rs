/// Parse cell reference like "A1", "$A$1", etc.
/// Returns (col, row) as 0-indexed, and (col_absolute, row_absolute)
pub fn parse_cell_ref(cell_ref: &str) -> Option<(usize, usize, bool, bool)> {
    let cell_ref = cell_ref.trim().to_uppercase();
    if cell_ref.is_empty() {
        return None;
    }

    let mut col_abs = false;
    let mut row_abs = false;
    let mut col_str = String::new();
    let mut row_str = String::new();
    let mut in_col = true;

    for c in cell_ref.chars() {
        if c == '$' {
            if in_col && col_str.is_empty() {
                col_abs = true;
            } else {
                row_abs = true;
            }
        } else if c.is_ascii_alphabetic() && in_col {
            col_str.push(c);
        } else if c.is_ascii_digit() {
            in_col = false;
            row_str.push(c);
        }
    }

    if col_str.is_empty() || row_str.is_empty() {
        return None;
    }

    // Convert column letters to number
    let col = col_str.chars().fold(0usize, |acc, c| {
        acc * 26 + (c as usize - 'A' as usize + 1)
    }) - 1;

    let row: usize = row_str.parse().ok()?;
    if row == 0 {
        return None;
    }

    Some((col, row - 1, col_abs, row_abs))
}

/// Convert column index to letters (0 -> A, 25 -> Z, 26 -> AA)
pub fn col_to_name(col: usize) -> String {
    let mut result = String::new();
    let mut col = col;
    loop {
        result.insert(0, (b'A' + (col % 26) as u8) as char);
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    result
}

/// Convert (col, row) to cell name
pub fn cell_name(col: usize, row: usize) -> String {
    format!("{}{}", col_to_name(col), row + 1)
}

/// Adjust a formula when copying/pasting
#[allow(dead_code)]
pub fn adjust_formula(formula: &str, col_offset: isize, row_offset: isize) -> String {
    let mut result = String::new();
    let mut i = 0;
    let chars: Vec<char> = formula.chars().collect();

    while i < chars.len() {
        // Check for string literal
        if chars[i] == '"' {
            result.push(chars[i]);
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                result.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                result.push(chars[i]);
                i += 1;
            }
            continue;
        }

        // Check for potential cell reference
        let ref_start = i;
        let mut col_abs = false;
        let mut row_abs = false;
        let mut col_str = String::new();
        let mut row_str = String::new();

        // Handle $ for column
        if chars[i] == '$' {
            col_abs = true;
            i += 1;
        }

        // Collect column letters
        while i < chars.len() && chars[i].is_ascii_alphabetic() {
            col_str.push(chars[i].to_ascii_uppercase());
            i += 1;
        }

        // Handle $ for row
        if i < chars.len() && chars[i] == '$' {
            row_abs = true;
            i += 1;
        }

        // Collect row digits
        while i < chars.len() && chars[i].is_ascii_digit() {
            row_str.push(chars[i]);
            i += 1;
        }

        // Check if we found a valid cell reference
        if !col_str.is_empty() && !row_str.is_empty() {
            if let (Some(col), Some(row)) = (
                Some(col_str.chars().fold(0usize, |acc, c| acc * 26 + (c as usize - 'A' as usize + 1)) - 1),
                row_str.parse::<usize>().ok().map(|r| r.saturating_sub(1))
            ) {
                // Adjust reference
                let new_col = if col_abs {
                    col
                } else {
                    ((col as isize) + col_offset).max(0) as usize
                };

                let new_row = if row_abs {
                    row
                } else {
                    ((row as isize) + row_offset).max(0) as usize
                };

                // Build adjusted reference
                if col_abs {
                    result.push('$');
                }
                result.push_str(&col_to_name(new_col));
                if row_abs {
                    result.push('$');
                }
                result.push_str(&(new_row + 1).to_string());
                continue;
            }
        }

        // Not a cell reference, output original characters
        for j in ref_start..i {
            result.push(chars[j]);
        }

        if i < chars.len() && i == ref_start {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

/// Adjust formula when a row is inserted
/// All references at or below inserted_row are shifted down by 1
pub fn adjust_formula_for_row_insert(formula: &str, inserted_row: usize) -> String {
    adjust_formula_for_structure_change(formula, StructureChange::RowInsert(inserted_row))
}

/// Adjust formula when a row is deleted
/// References to deleted_row become #REF!, references below are shifted up
pub fn adjust_formula_for_row_delete(formula: &str, deleted_row: usize) -> String {
    adjust_formula_for_structure_change(formula, StructureChange::RowDelete(deleted_row))
}

/// Adjust formula when a column is inserted
/// All references at or to the right of inserted_col are shifted right by 1
pub fn adjust_formula_for_col_insert(formula: &str, inserted_col: usize) -> String {
    adjust_formula_for_structure_change(formula, StructureChange::ColInsert(inserted_col))
}

/// Adjust formula when a column is deleted
/// References to deleted_col become #REF!, references to the right are shifted left
pub fn adjust_formula_for_col_delete(formula: &str, deleted_col: usize) -> String {
    adjust_formula_for_structure_change(formula, StructureChange::ColDelete(deleted_col))
}

enum StructureChange {
    RowInsert(usize),
    RowDelete(usize),
    ColInsert(usize),
    ColDelete(usize),
}

fn adjust_formula_for_structure_change(formula: &str, change: StructureChange) -> String {
    let mut result = String::new();
    let mut i = 0;
    let chars: Vec<char> = formula.chars().collect();

    while i < chars.len() {
        // Skip string literals
        if chars[i] == '"' {
            result.push(chars[i]);
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                result.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                result.push(chars[i]);
                i += 1;
            }
            continue;
        }

        // Try to parse cell reference
        let ref_start = i;
        let mut col_abs = false;
        let mut row_abs = false;
        let mut col_str = String::new();
        let mut row_str = String::new();

        // Handle $ for column
        if chars[i] == '$' {
            col_abs = true;
            i += 1;
        }

        // Collect column letters
        while i < chars.len() && chars[i].is_ascii_alphabetic() {
            col_str.push(chars[i].to_ascii_uppercase());
            i += 1;
        }

        // Handle $ for row
        if i < chars.len() && chars[i] == '$' {
            row_abs = true;
            i += 1;
        }

        // Collect row digits
        while i < chars.len() && chars[i].is_ascii_digit() {
            row_str.push(chars[i]);
            i += 1;
        }

        // Check if we found a valid cell reference
        if !col_str.is_empty() && !row_str.is_empty() {
            let col = col_str.chars().fold(0usize, |acc, c| acc * 26 + (c as usize - 'A' as usize + 1)) - 1;
            if let Ok(row_1based) = row_str.parse::<usize>() {
                if row_1based > 0 {
                    let row = row_1based - 1;
                    
                    // Apply structure change
                    let (new_col, new_row, is_ref_error) = match change {
                        StructureChange::RowInsert(inserted_row) => {
                            if row >= inserted_row {
                                (col, row + 1, false)
                            } else {
                                (col, row, false)
                            }
                        }
                        StructureChange::RowDelete(deleted_row) => {
                            if row == deleted_row {
                                (col, row, true)  // #REF!
                            } else if row > deleted_row {
                                (col, row - 1, false)
                            } else {
                                (col, row, false)
                            }
                        }
                        StructureChange::ColInsert(inserted_col) => {
                            if col >= inserted_col {
                                (col + 1, row, false)
                            } else {
                                (col, row, false)
                            }
                        }
                        StructureChange::ColDelete(deleted_col) => {
                            if col == deleted_col {
                                (col, row, true)  // #REF!
                            } else if col > deleted_col {
                                (col - 1, row, false)
                            } else {
                                (col, row, false)
                            }
                        }
                    };

                    if is_ref_error {
                        result.push_str("#REF!");
                    } else {
                        // Build adjusted reference preserving $ markers
                        if col_abs {
                            result.push('$');
                        }
                        result.push_str(&col_to_name(new_col));
                        if row_abs {
                            result.push('$');
                        }
                        result.push_str(&(new_row + 1).to_string());
                    }
                    continue;
                }
            }
        }

        // Not a valid cell reference, output original characters
        for j in ref_start..i {
            result.push(chars[j]);
        }

        if i < chars.len() && i == ref_start {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cell_ref() {
        assert_eq!(parse_cell_ref("A1"), Some((0, 0, false, false)));
        assert_eq!(parse_cell_ref("B2"), Some((1, 1, false, false)));
        assert_eq!(parse_cell_ref("$A$1"), Some((0, 0, true, true)));
        assert_eq!(parse_cell_ref("$A1"), Some((0, 0, true, false)));
        assert_eq!(parse_cell_ref("A$1"), Some((0, 0, false, true)));
        assert_eq!(parse_cell_ref("AA1"), Some((26, 0, false, false)));
    }

    #[test]
    fn test_col_to_name() {
        assert_eq!(col_to_name(0), "A");
        assert_eq!(col_to_name(25), "Z");
        assert_eq!(col_to_name(26), "AA");
    }

    #[test]
    fn test_adjust_formula() {
        assert_eq!(adjust_formula("=A1+B1", 1, 1), "=B2+C2");
        assert_eq!(adjust_formula("=$A$1+B1", 1, 1), "=$A$1+C2");
    }

    #[test]
    fn test_row_insert() {
        // Insert at row 2 (0-indexed)
        assert_eq!(adjust_formula_for_row_insert("=A1", 2), "=A1");  // row 0 < 2
        assert_eq!(adjust_formula_for_row_insert("=A2", 2), "=A2");  // row 1 < 2
        assert_eq!(adjust_formula_for_row_insert("=A3", 2), "=A4");  // row 2 >= 2
        assert_eq!(adjust_formula_for_row_insert("=A$3", 2), "=A$4"); // absolute also shifts
        assert_eq!(adjust_formula_for_row_insert("=SUM(A1:A5)", 2), "=SUM(A1:A6)");
    }

    #[test]
    fn test_row_delete() {
        // Delete row 2 (0-indexed)
        assert_eq!(adjust_formula_for_row_delete("=A1", 2), "=A1");  // row 0 < 2
        assert_eq!(adjust_formula_for_row_delete("=A3", 2), "=#REF!"); // row 2 == 2
        assert_eq!(adjust_formula_for_row_delete("=A4", 2), "=A3");  // row 3 > 2
        assert_eq!(adjust_formula_for_row_delete("=A$3", 2), "=#REF!"); // absolute also affected
    }

    #[test]
    fn test_col_insert() {
        // Insert at col B (index 1)
        assert_eq!(adjust_formula_for_col_insert("=A1", 1), "=A1");  // col 0 < 1
        assert_eq!(adjust_formula_for_col_insert("=B1", 1), "=C1");  // col 1 >= 1
        assert_eq!(adjust_formula_for_col_insert("=$B1", 1), "=$C1"); // absolute also shifts
    }

    #[test]
    fn test_col_delete() {
        // Delete col B (index 1)
        assert_eq!(adjust_formula_for_col_delete("=A1", 1), "=A1");  // col 0 < 1
        assert_eq!(adjust_formula_for_col_delete("=B1", 1), "=#REF!"); // col 1 == 1
        assert_eq!(adjust_formula_for_col_delete("=C1", 1), "=B1");  // col 2 > 1
    }
}
