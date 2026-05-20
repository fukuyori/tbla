//! Direct DataFrame I/O via Polars. Lets tbla open very large CSVs and
//! Parquet files without going through the cell-import path — type
//! inference, encoding handling, and chunked reading are all delegated
//! to Polars.

use polars::prelude::*;
use std::path::Path;

use crate::df_view::DataFrameView;

/// Read a Parquet file directly into a DataFrameView.
pub fn read_parquet<P: AsRef<Path>>(path: P) -> Result<DataFrameView, String> {
    let path_str = path.as_ref().to_string_lossy().into_owned();
    let pl_path = polars::prelude::PlRefPath::new(path_str);
    let lf = LazyFrame::scan_parquet(pl_path, Default::default())
        .map_err(|e| format!("Parquet open: {}", e))?;
    let df = lf.collect().map_err(|e| format!("Parquet read: {}", e))?;
    Ok(DataFrameView { df, computed: Vec::new() })
}

/// Write a DataFrame to a Parquet file. Uses Snappy compression by
/// default which gives ~10x size reduction over raw CSV for typical
/// numeric data.
pub fn write_parquet<P: AsRef<Path>>(view: &DataFrameView, path: P) -> Result<(), String> {
    let file = std::fs::File::create(path.as_ref())
        .map_err(|e| format!("Parquet create: {}", e))?;
    let mut df = view.df.clone();
    ParquetWriter::new(file)
        .with_compression(ParquetCompression::Snappy)
        .finish(&mut df)
        .map_err(|e| format!("Parquet write: {}", e))?;
    Ok(())
}

/// Read a CSV file into a DataFrameView using Polars' fast CSV reader.
/// Type inference and header detection are automatic. Suitable for files
/// the cell-based importer would choke on (10MB+ / millions of rows).
pub fn read_csv_as_dataframe<P: AsRef<Path>>(path: P) -> Result<DataFrameView, String> {
    let path_str = path.as_ref().to_string_lossy().into_owned();
    let pl_path = polars::prelude::PlRefPath::new(path_str);
    let lf = LazyCsvReader::new(pl_path)
        .with_has_header(true)
        .with_infer_schema_length(Some(1000))
        .finish()
        .map_err(|e| format!("CSV open: {}", e))?;
    let df = lf.collect().map_err(|e| format!("CSV read: {}", e))?;
    Ok(DataFrameView { df, computed: Vec::new() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::df_view::cells_to_dataframe;
    use crate::sheet::Sheet;

    fn tmp_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("tbla_df_io_test_{}_{}", std::process::id(), name));
        p
    }

    #[test]
    fn parquet_round_trip() {
        // Build a small DataFrame via the cells path
        let mut s = Sheet::new();
        s.set_cell(0, 0, "name".into());
        s.set_cell(1, 0, "score".into());
        s.set_cell(0, 1, "Alice".into());
        s.set_cell(1, 1, "95".into());
        s.set_cell(0, 2, "Bob".into());
        s.set_cell(1, 2, "82".into());
        let v = cells_to_dataframe(&s).unwrap();

        let mut path = tmp_path("roundtrip");
        path.set_extension("parquet");
        write_parquet(&v, &path).unwrap();

        let read_back = read_parquet(&path).unwrap();
        assert_eq!(read_back.rows(), 2);
        assert_eq!(read_back.cols(), 2);
        assert_eq!(read_back.header(0), "name");
        assert_eq!(read_back.value_at(0, 0), "Alice");
        assert_eq!(read_back.value_at(1, 1), "82");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn csv_as_dataframe() {
        let path = tmp_path("input.csv");
        std::fs::write(&path, "name,score\nAlice,95\nBob,82\nCharlie,78\n").unwrap();

        let v = read_csv_as_dataframe(&path).unwrap();
        assert_eq!(v.rows(), 3);
        assert_eq!(v.cols(), 2);
        assert_eq!(v.header(0), "name");
        assert_eq!(v.header(1), "score");
        assert!(v.is_numeric(1));
        assert_eq!(v.value_at(0, 0), "Alice");

        std::fs::remove_file(&path).ok();
    }
}
