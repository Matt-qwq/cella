//! The two connectors that need the outside world: a **SQLite file** and a **URL**.
//!
//! Everything else the query pipeline reads is a local file (CSV, JSON, XLSX, Parquet, a folder of
//! them), which is why the pipeline itself lives in a pure crate: it never sees a path. These two
//! are the exceptions, so they live here — behind the crate's "connectors" feature, because both
//! cost something the browser build cannot pay: a bundled SQLite (a C compiler on every platform we
//! ship) and TLS. A build without the feature keeps the query in the document and reports that it
//! cannot read it, instead of refusing to compile.
//!
//! Both are **read-only by construction**: SQLite is opened with SQLITE_OPEN_READ_ONLY, and the
//! HTTP side only issues a GET. A workbook's refresh must never be able to change something the
//! user did not point it at.

#[cfg(feature = "connectors")]
use gridcraft_core::Value;
use gridcraft_query::Table;
#[cfg(feature = "connectors")]
use gridcraft_query::{MAX_COLUMNS, MAX_ROWS};

use crate::{EngineError, Result};

#[cfg(feature = "connectors")]
use std::time::Duration;

/// What a build without the connectors tells the user, in the words the pane shows.
#[cfg(not(feature = "connectors"))]
const NOT_BUILT: &str = "This build cannot read SQLite or web sources (it was built without the connectors feature).";

/// Longest statement accepted. A query is a statement a person wrote, not a dump of a schema.
#[cfg(feature = "connectors")]
const MAX_SQL: usize = 64 * 1024;

/// Longest reply read from a URL. A source that answers with more is a file, and a file should be
/// downloaded by the user's own tool and pointed at with a path.
#[cfg(feature = "connectors")]
const MAX_HTTP_BYTES: u64 = 64 << 20;

/// How long one URL may take, connect to last byte.
#[cfg(feature = "connectors")]
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------- SQLite

/// Runs one statement against a database file and returns its result set.
///
/// The columns are the statement's output columns, so the table arrives named: no PromoteHeaders
/// step is needed, the same as a query source.
///
/// Two honest refusals: **binary columns** (a BLOB has no cell form — the same rule the Parquet
/// reader applies to nested and INT96 columns) and a result set larger than the pipeline's own
/// caps. Integer columns arrive as doubles, which is what a cell holds and what Excel does with a
/// SQL integer too.
#[cfg(feature = "connectors")]
pub fn sqlite_table(path: &str, sql: &str) -> Result<Table> {
    use rusqlite::types::ValueRef;
    use rusqlite::{Connection, OpenFlags};

    if path.trim().is_empty() {
        return Err(EngineError::Other("A database source needs a path.".into()));
    }
    let sql = sql.trim();
    if sql.is_empty() {
        return Err(EngineError::Other(format!("The database source for {path} has no statement to run.")));
    }
    if sql.len() > MAX_SQL {
        return Err(EngineError::Other(format!("A statement may be at most {MAX_SQL} bytes long.")));
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| EngineError::Other(format!("Cannot open the database {path}: {e}")))?;
    let mut stmt = conn.prepare(sql).map_err(|e| EngineError::Other(format!("The statement could not be prepared: {e}")))?;
    let width = stmt.column_count();
    if width > MAX_COLUMNS {
        return Err(EngineError::Other(format!("The statement returns {width} columns; a table holds at most {MAX_COLUMNS}.")));
    }
    let names: Vec<String> =
        stmt.column_names().enumerate().map(|(i, n)| if n.trim().is_empty() { format!("Column{}", i + 1) } else { (*n).to_string() }).collect();
    let mut rows: Vec<Vec<Value>> = Vec::new();
    let mut rs = stmt.query([]).map_err(|e| EngineError::Other(format!("The statement failed: {e}")))?;
    while let Some(row) = rs.next().map_err(|e| EngineError::Other(format!("The statement failed: {e}")))? {
        if rows.len() >= MAX_ROWS {
            return Err(EngineError::Other(format!("The statement returns more than {MAX_ROWS} rows; narrow it with a WHERE clause or a LIMIT.")));
        }
        let mut line = Vec::with_capacity(width);
        for i in 0..width {
            let v = row.get_ref(i).map_err(|e| EngineError::Other(format!("The statement failed: {e}")))?;
            line.push(match v {
                ValueRef::Null => Value::Empty,
                ValueRef::Integer(i) => Value::Number(i as f64),
                ValueRef::Real(f) => Value::Number(f),
                ValueRef::Text(t) => Value::text(String::from_utf8_lossy(t).into_owned()),
                ValueRef::Blob(_) => {
                    return Err(EngineError::Other(format!(
                        "Column \"{}\" holds binary data, which a cell cannot hold. Select the columns you need, or CAST that one to text.",
                        names.get(i).map(String::as_str).unwrap_or("?")
                    )));
                }
            });
        }
        rows.push(line);
    }
    Table::new(names, rows).map_err(|e| EngineError::Other(e.to_string()))
}

#[cfg(not(feature = "connectors"))]
pub fn sqlite_table(_path: &str, _sql: &str) -> Result<Table> {
    Err(EngineError::Other(NOT_BUILT.into()))
}

// ---------------------------------------------------------------- HTTP

/// Reads a URL with a GET: a size cap and a timeout, and nothing else.
///
/// Only http and https are accepted — a file URL would be a path dressed up as a link, and a data
/// URL is how a workbook smuggles a payload.
#[cfg(feature = "connectors")]
pub fn http_bytes(url: &str) -> Result<Vec<u8>> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err(EngineError::Other(format!("\"{url}\" is not a web address: a source may be http:// or https://.")));
    }
    let req = ureq::get(url).config().timeout_global(Some(HTTP_TIMEOUT)).build();
    let mut resp = req.call().map_err(|e| {
        EngineError::Other(match e {
            ureq::Error::StatusCode(code) => format!("{url} answered {code}."),
            other => format!("{url} could not be read: {other}"),
        })
    })?;
    let body =
        resp.body_mut().with_config().limit(MAX_HTTP_BYTES).read_to_vec().map_err(|e| EngineError::Other(format!("{url} could not be read: {e}")))?;
    Ok(body)
}

#[cfg(not(feature = "connectors"))]
pub fn http_bytes(_url: &str) -> Result<Vec<u8>> {
    Err(EngineError::Other(NOT_BUILT.into()))
}

/// Refuses a reply that is a web page: an endpoint behind a login answers 200 with HTML, and "the
/// CSV has no columns" is a worse message than "that address served a page, not data".
pub fn looks_like_html(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(512)];
    let t = String::from_utf8_lossy(head);
    let t = t.trim_start();
    t.starts_with('<') && t.to_ascii_lowercase().contains("html")
}
