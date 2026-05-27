//! SQL query import (read-only) for PostgreSQL / MySQL / MariaDB / SQLite.
//!
//! `run_query(uri, query)` dispatches on the URI scheme:
//! - `postgresql://` or `postgres://` → `postgres` crate (TLS not yet wired)
//! - `mysql://` or `mariadb://`       → `mysql` crate (rustls TLS by default)
//! - `sqlite://` or `sqlite3://` or a bare path ending in `.db` / `.sqlite` /
//!   `.sqlite3` (with optional `file://` prefix) → `rusqlite` (bundled SQLite)
//!
//! Returns `QueryResult { columns, rows }` where `columns` are the result-set
//! column names and `rows` is a `Vec<Vec<String>>` with one entry per cell
//! (NULL becomes an empty string; binary / unsupported types become
//! `<bytea>` / `<{type-name}>` markers).

/// One executed query's result.
#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl QueryResult {
    pub fn row_count(&self) -> usize { self.rows.len() }
    pub fn col_count(&self) -> usize { self.columns.len() }
}

/// Run a single SELECT-style query against the database identified by `uri`.
pub fn run_query(uri: &str, query: &str) -> Result<QueryResult, String> {
    let kind = detect_db_kind(uri).ok_or_else(|| {
        "URI から DB の種類を判定できません \
         (postgresql:// / mysql:// / sqlite:// または .sqlite/.db のパス を指定してください)".to_string()
    })?;
    match kind {
        DbKind::Postgres => run_postgres(uri, query),
        DbKind::Mysql => run_mysql(uri, query),
        DbKind::Sqlite(path) => run_sqlite(&path, query),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum DbKind {
    Postgres,
    Mysql,
    /// SQLite carries the resolved local file path (the `sqlite://`/`file://`
    /// prefix has already been stripped).
    Sqlite(String),
}

fn detect_db_kind(uri: &str) -> Option<DbKind> {
    let lower = uri.to_ascii_lowercase();
    if lower.starts_with("postgresql://") || lower.starts_with("postgres://") {
        return Some(DbKind::Postgres);
    }
    if lower.starts_with("mysql://") || lower.starts_with("mariadb://") {
        return Some(DbKind::Mysql);
    }
    // sqlite://  /  sqlite3://  /  file://   → bare path follows
    for prefix in ["sqlite3://", "sqlite://", "file://"] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            // Use the original (case-preserved) path, just past the prefix.
            let original_rest = &uri[prefix.len()..];
            return Some(DbKind::Sqlite(strip_leading_slash_for_windows(original_rest, rest)));
        }
    }
    // Bare path ending in a SQLite extension.
    if lower.ends_with(".sqlite") || lower.ends_with(".sqlite3") || lower.ends_with(".db") {
        return Some(DbKind::Sqlite(uri.to_string()));
    }
    None
}

/// `sqlite:///C:/path/to.db` and `sqlite:///home/user/x.db` both decode to a
/// usable filesystem path. `sqlite:///C:/...` on Windows leaves a leading
/// slash that breaks `open()`, so strip it when followed by `<letter>:`.
fn strip_leading_slash_for_windows(original: &str, _lower: &str) -> String {
    let trimmed = original.trim_start_matches('/');
    // Heuristic: if after trimming we have "X:..." (drive letter), it was a
    // Windows path stored as sqlite:///C:/foo.db. Otherwise treat as POSIX
    // absolute (re-prefix the slash we just consumed).
    let bytes = trimmed.as_bytes();
    let looks_like_drive = bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':';
    if looks_like_drive {
        trimmed.to_string()
    } else if original.starts_with('/') {
        format!("/{}", trimmed)
    } else {
        trimmed.to_string()
    }
}

// ----------------------------------------------------------------------------
// PostgreSQL
// ----------------------------------------------------------------------------

fn run_postgres(uri: &str, query: &str) -> Result<QueryResult, String> {
    let mut client = postgres::Client::connect(uri, postgres::NoTls)
        .map_err(|e| format!("PostgreSQL 接続失敗: {}", e))?;
    let rows = client.query(query, &[])
        .map_err(|e| format!("クエリ実行失敗: {}", e))?;

    let columns: Vec<String> = rows.first()
        .map(|r| r.columns().iter().map(|c| c.name().to_string()).collect())
        .unwrap_or_default();

    let mut out_rows = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut cells = Vec::with_capacity(row.columns().len());
        for i in 0..row.columns().len() {
            cells.push(pg_cell_to_string(row, i));
        }
        out_rows.push(cells);
    }
    Ok(QueryResult { columns, rows: out_rows })
}

fn pg_cell_to_string(row: &postgres::Row, idx: usize) -> String {
    use postgres::types::Type;
    let col = &row.columns()[idx];
    let t = col.type_();
    // Try the common types first; fall back to a stringly fetch.
    macro_rules! try_typed {
        ($T:ty) => {
            match row.try_get::<_, Option<$T>>(idx) {
                Ok(Some(v)) => return v.to_string(),
                Ok(None) => return String::new(),
                Err(_) => {}
            }
        };
    }
    match *t {
        Type::BOOL => try_typed!(bool),
        Type::INT2 => try_typed!(i16),
        Type::INT4 => try_typed!(i32),
        Type::INT8 => try_typed!(i64),
        Type::FLOAT4 => try_typed!(f32),
        Type::FLOAT8 => try_typed!(f64),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN => try_typed!(String),
        Type::BYTEA => {
            return match row.try_get::<_, Option<Vec<u8>>>(idx) {
                Ok(Some(b)) => format!("<bytea {} bytes>", b.len()),
                Ok(None) => String::new(),
                Err(_) => "<bytea>".to_string(),
            };
        }
        _ => {}
    }
    // Last-resort: ask Postgres for a String. Works for many text-like types.
    match row.try_get::<_, Option<String>>(idx) {
        Ok(Some(s)) => s,
        Ok(None) => String::new(),
        Err(_) => format!("<{}>", t.name()),
    }
}

// ----------------------------------------------------------------------------
// MySQL / MariaDB
// ----------------------------------------------------------------------------

fn run_mysql(uri: &str, query: &str) -> Result<QueryResult, String> {
    use mysql::prelude::*;
    let opts = mysql::Opts::from_url(uri)
        .map_err(|e| format!("URI 解析失敗: {}", e))?;
    let mut conn = mysql::Conn::new(opts)
        .map_err(|e| format!("MySQL 接続失敗: {}", e))?;
    let mut result = conn.query_iter(query)
        .map_err(|e| format!("クエリ実行失敗: {}", e))?;

    // Snapshot column names while the result is alive.
    let columns: Vec<String> = result
        .columns()
        .as_ref()
        .iter()
        .map(|c| c.name_str().to_string())
        .collect();

    let mut out_rows = Vec::new();
    while let Some(row_res) = result.next() {
        let row = row_res.map_err(|e| format!("行読み出し失敗: {}", e))?;
        let mut cells = Vec::with_capacity(columns.len());
        for i in 0..columns.len() {
            let v: mysql::Value = row.as_ref(i).cloned().unwrap_or(mysql::Value::NULL);
            cells.push(mysql_value_to_string(&v));
        }
        out_rows.push(cells);
    }
    Ok(QueryResult { columns, rows: out_rows })
}

fn mysql_value_to_string(v: &mysql::Value) -> String {
    use mysql::Value;
    match v {
        Value::NULL => String::new(),
        Value::Bytes(b) => match std::str::from_utf8(b) {
            Ok(s) => s.to_string(),
            Err(_) => format!("<binary {} bytes>", b.len()),
        },
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Double(d) => d.to_string(),
        Value::Date(y, mo, d, h, mi, s, us) => {
            if *h == 0 && *mi == 0 && *s == 0 && *us == 0 {
                format!("{:04}-{:02}-{:02}", y, mo, d)
            } else if *us == 0 {
                format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, mo, d, h, mi, s)
            } else {
                format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:06}", y, mo, d, h, mi, s, us)
            }
        }
        Value::Time(neg, days, h, mi, s, us) => {
            let sign = if *neg { "-" } else { "" };
            let total_h = *days * 24 + *h as u32;
            if *us == 0 {
                format!("{}{:02}:{:02}:{:02}", sign, total_h, mi, s)
            } else {
                format!("{}{:02}:{:02}:{:02}.{:06}", sign, total_h, mi, s, us)
            }
        }
    }
}

// ----------------------------------------------------------------------------
// SQLite
// ----------------------------------------------------------------------------

fn run_sqlite(path: &str, query: &str) -> Result<QueryResult, String> {
    let conn = rusqlite::Connection::open(path)
        .map_err(|e| format!("SQLite を開けません ({}): {}", path, e))?;
    let mut stmt = conn.prepare(query)
        .map_err(|e| format!("クエリ準備失敗: {}", e))?;
    let columns: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let col_count = columns.len();
    let mut rows = stmt.query([])
        .map_err(|e| format!("クエリ実行失敗: {}", e))?;
    let mut out_rows = Vec::new();
    while let Some(row) = rows.next().map_err(|e| format!("行読み出し失敗: {}", e))? {
        let mut cells = Vec::with_capacity(col_count);
        for i in 0..col_count {
            cells.push(sqlite_cell_to_string(row, i));
        }
        out_rows.push(cells);
    }
    Ok(QueryResult { columns, rows: out_rows })
}

fn sqlite_cell_to_string(row: &rusqlite::Row, idx: usize) -> String {
    use rusqlite::types::ValueRef;
    match row.get_ref(idx) {
        Ok(ValueRef::Null) => String::new(),
        Ok(ValueRef::Integer(n)) => n.to_string(),
        Ok(ValueRef::Real(f)) => f.to_string(),
        Ok(ValueRef::Text(b)) => String::from_utf8_lossy(b).into_owned(),
        Ok(ValueRef::Blob(b)) => format!("<blob {} bytes>", b.len()),
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_postgres() {
        assert_eq!(detect_db_kind("postgresql://u:p@h:5432/db"), Some(DbKind::Postgres));
        assert_eq!(detect_db_kind("postgres://u:p@h/db"), Some(DbKind::Postgres));
    }

    #[test]
    fn detects_mysql() {
        assert_eq!(detect_db_kind("mysql://u:p@h:3306/db"), Some(DbKind::Mysql));
        assert_eq!(detect_db_kind("mariadb://u@h/db"), Some(DbKind::Mysql));
    }

    #[test]
    fn detects_sqlite_url_form() {
        assert_eq!(
            detect_db_kind("sqlite:///home/u/data.db"),
            Some(DbKind::Sqlite("/home/u/data.db".to_string())),
        );
        // Windows-style path stored as sqlite:///C:/...
        assert_eq!(
            detect_db_kind("sqlite:///C:/Users/me/data.db"),
            Some(DbKind::Sqlite("C:/Users/me/data.db".to_string())),
        );
    }

    #[test]
    fn detects_sqlite_bare_path() {
        assert_eq!(
            detect_db_kind("data.sqlite"),
            Some(DbKind::Sqlite("data.sqlite".to_string())),
        );
        assert_eq!(
            detect_db_kind("C:\\Users\\me\\data.db"),
            Some(DbKind::Sqlite("C:\\Users\\me\\data.db".to_string())),
        );
    }

    #[test]
    fn rejects_unknown_scheme() {
        assert_eq!(detect_db_kind("oracle://h/db"), None);
        assert_eq!(detect_db_kind("plain-text"), None);
    }

    #[test]
    fn sqlite_end_to_end() {
        // Verify the SQLite path actually works with an in-memory DB.
        let r = run_sqlite(":memory:",
            "WITH t(a, b) AS (VALUES (1, 'hello'), (2, NULL)) SELECT * FROM t");
        let r = r.expect("query");
        assert_eq!(r.columns, vec!["a", "b"]);
        assert_eq!(r.rows, vec![
            vec!["1".to_string(), "hello".to_string()],
            vec!["2".to_string(), String::new()],
        ]);
    }
}
