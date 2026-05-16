use calamine::{open_workbook, Reader, Xlsx, Data};
use rust_xlsxwriter::Workbook;
use std::path::Path;

use crate::sheet::Sheet;

/// Read xlsx file and return Sheet
pub fn read_xlsx<P: AsRef<Path>>(path: P) -> Result<Sheet, String> {
    let path = path.as_ref();
    
    let mut workbook: Xlsx<_> = open_workbook(path)
        .map_err(|e| format!("Failed to open file: {}", e))?;
    
    // Get first sheet name
    let sheet_names = workbook.sheet_names().to_vec();
    if sheet_names.is_empty() {
        return Err("No sheets found in workbook".to_string());
    }
    
    let sheet_name = &sheet_names[0];
    
    // Read the sheet
    let range = workbook.worksheet_range(sheet_name)
        .map_err(|e| format!("Failed to read sheet: {}", e))?;
    
    let mut sheet = Sheet::new();
    sheet.name = sheet_name.clone();
    
    for (row_idx, row) in range.rows().enumerate() {
        for (col_idx, cell) in row.iter().enumerate() {
            let value = match cell {
                Data::Empty => continue,
                Data::String(s) => s.clone(),
                Data::Float(f) => {
                    // Format float nicely
                    if f.fract() == 0.0 {
                        format!("{}", *f as i64)
                    } else {
                        format!("{}", f)
                    }
                }
                Data::Int(i) => format!("{}", i),
                Data::Bool(b) => if *b { "TRUE".to_string() } else { "FALSE".to_string() },
                Data::Error(e) => format!("#{:?}", e),
                Data::DateTime(dt) => format!("{}", dt),
                Data::DateTimeIso(s) => s.clone(),
                Data::DurationIso(s) => s.clone(),
            };
            
            if !value.is_empty() {
                sheet.set_cell(col_idx, row_idx, value);
            }
        }
    }
    
    Ok(sheet)
}

/// Write Sheet to xlsx file
pub fn write_xlsx<P: AsRef<Path>>(sheet: &Sheet, path: P) -> Result<(), String> {
    let path = path.as_ref();
    
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    
    // Set sheet name
    worksheet.set_name(&sheet.name)
        .map_err(|e| format!("Failed to set sheet name: {}", e))?;
    
    // Find the extent of data
    let max_row = sheet.max_row().unwrap_or(0);
    let max_col = sheet.max_col().unwrap_or(0);
    
    // Write cells
    for row in 0..=max_row {
        for col in 0..=max_col {
            let cell = sheet.get_cell(col, row);
            if cell.raw_input.is_empty() {
                continue;
            }
            
            // Try to parse as number
            if let Ok(num) = cell.raw_input.parse::<f64>() {
                worksheet.write_number(row as u32, col as u16, num)
                    .map_err(|e| format!("Failed to write cell: {}", e))?;
            } else if cell.raw_input.eq_ignore_ascii_case("TRUE") {
                worksheet.write_boolean(row as u32, col as u16, true)
                    .map_err(|e| format!("Failed to write cell: {}", e))?;
            } else if cell.raw_input.eq_ignore_ascii_case("FALSE") {
                worksheet.write_boolean(row as u32, col as u16, false)
                    .map_err(|e| format!("Failed to write cell: {}", e))?;
            } else {
                worksheet.write_string(row as u32, col as u16, &cell.raw_input)
                    .map_err(|e| format!("Failed to write cell: {}", e))?;
            }
        }
    }
    
    // Save
    workbook.save(path)
        .map_err(|e| format!("Failed to save file: {}", e))?;
    
    Ok(())
}
