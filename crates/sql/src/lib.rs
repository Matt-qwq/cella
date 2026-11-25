//! The SQL engine the data model runs on.
//!
//! A model table is deliberately bigger than a sheet — that is what it is for — and a pivot or a
//! slicer over one means grouping a few hundred thousand rows in a few milliseconds. That is a
//! database's job, so Cella uses a real one (DuckDB, in memory, no server process) rather than a
//! hand-written executor with its own quiet bugs. This crate is the **only** place in the tree
//! that knows which engine that is; everything else talks to [`Engine`], so the engine stays
//! replaceable.
//!
//! # Why an engine is two orders of magnitude under the target
//!
//! Measured in the tree (`docs/sql-engine-research.md`, `gaps` §60): 5 million rows × 10 columns
//! group into a 768-cell report in **14.5 ms**, a slicer narrowing 64 regions to 4 then grouping
//! takes **9.5 ms**, and those rows arrive from a Parquet file in **109 ms**. Our own sheet path
//! takes 352 ms for a million rows.
//!
//! # The browser build has no engine, and says so
//!
//! DuckDB is C++ compiled into the binary (`bundled`), which does not build for `wasm32`. This
//! crate builds for every target: without the `duckdb` feature every call returns
//! [`SqlError::Unavailable`] and [`available`] is `false`. The interface tells the truth instead
//! of pretending, which is what a desktop-only data model means in practice (the product
//! decision of 11-25).
//!
//! `cargo xtask ci` leaves the feature off on purpose: the bundled engine is a several-minute C++
//! build (and a source download) that the browser build can never use, so the engine's own tests
//! run on demand — `cargo test -p cella-sql --features duckdb` — and with the desktop binary. What
//! the gate does run is everything that would break without the engine: the quoting, the type
//! inference and the "this build has no engine" answer.
//!
//! # What it is careful about
//!
//! * **It writes nothing.** [`Engine::load_file`] reads a path the caller chose; the engine is
//!   never given a path to write to.
//! * **A result is bounded.** A `SELECT *` over a million rows is refused against [`MAX_ROWS`]
//!   and [`MAX_CELLS`] *while* it is being read, not after it has allocated.
//! * **Names and paths are quoted, never trusted.** [`quote_ident`] and [`quote_literal`] double
//!   the quote character, so a table named `Sales"; DROP TABLE x --` is a name and not a second
//!   statement.
//! * **Slow paths are named.** Handing rows to the engine costs ~720 ns a row (measured), while a
//!   file the engine reads itself costs 22–99 ns a row. Big data belongs in a file the engine
//!   reads, and [`Engine::load_rows`] is for the sheet a person loaded once.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
#![forbid(unsafe_code)]

use cella_core::Value;

#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
mod duck;
#[cfg(test)]
mod tests;

/// Excel's grid: no result taller than this goes into a table.
pub const MAX_ROWS: usize = 1_048_576;
/// Excel's grid: no result wider than this goes into a table.
pub const MAX_COLUMNS: usize = 16_384;
/// Cells one materialised result may hold (4M: a 2,000 × 2,000 answer). A result is read into
/// memory once before it is used, so the honest limit is the same one a loaded query gets.
pub const MAX_CELLS: usize = 4_194_304;
/// Longest table or column name accepted. A name is for a person to read in a field list.
const MAX_NAME: usize = 255;

/// Whether this build has a SQL engine at all.
///
/// The web build answers `false`; the UI uses this to say "the data model needs the desktop app"
/// instead of showing an empty table.
pub fn available() -> bool {
    cfg!(all(feature = "duckdb", not(target_arch = "wasm32")))
}

/// What a query returned: column names and rows of the same width, exactly the shape the query
/// pipeline's [`Table`](../../cella_query/struct.Table.html) has, so a result can be loaded onto a
/// sheet or into a model without a second representation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    /// Column names in order.
    pub columns: Vec<String>,
    /// One entry per row, each exactly [`Table::width`] long.
    pub rows: Vec<Vec<Value>>,
}

impl Table {
    pub fn width(&self) -> usize {
        self.columns.len()
    }

    pub fn height(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// A cell, or [`Value::Empty`] outside the table (a caller that trusts an index gets a blank,
    /// not a panic).
    pub fn cell(&self, row: usize, column: usize) -> &Value {
        self.rows.get(row).and_then(|r| r.get(column)).unwrap_or(&Value::Empty)
    }
}

/// Why a call to the engine failed.
///
/// The engine's own message is kept as text rather than as an engine type: this enum is compiled
/// into the browser build, where the engine is not.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SqlError {
    /// This build has no engine (the web build, or a native build with the feature off).
    #[error("this build has no SQL engine: the data model needs the desktop app")]
    Unavailable,
    /// The engine refused: bad SQL, a missing column, a file it cannot read.
    #[error("{0}")]
    Engine(String),
    /// The request was refused before it reached the engine (an empty query, a blank name).
    #[error("{0}")]
    Invalid(String),
    /// The result cannot fit in a table.
    #[error("the query returns {rows} rows and {columns} columns, more than a table may hold")]
    TooBig { rows: usize, columns: usize },
}

/// The file formats [`Engine::load_file`] reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileFormat {
    /// Parquet: typed columns and real nulls, what the engine reads fastest.
    Parquet,
    /// Delimited text. The engine sniffs the header, the delimiter and the type of each column.
    Csv,
}

/// The type a loaded column gets in the engine.
///
/// The choice is made by looking at the column's values once, the way the Parquet writer types a
/// sheet (`crates/parquet`), so the same data gets the same type whichever direction it travels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type {
    /// Whole numbers that fit a `BIGINT` and survive an exact round trip through `f64`.
    Int64,
    /// Any other number.
    Double,
    /// `TRUE`/`FALSE`.
    Bool,
    /// Everything else: text, errors (`#N/A`), a mix of numbers and text.
    Text,
}

impl Type {
    /// The engine's SQL name for the type.
    pub fn as_sql(self) -> &'static str {
        match self {
            Type::Int64 => "BIGINT",
            Type::Double => "DOUBLE",
            Type::Bool => "BOOLEAN",
            Type::Text => "VARCHAR",
        }
    }

    /// The type a column's values give it. A blank never decides anything; a column with no
    /// values at all is text, like a column of nothing.
    pub fn infer<'a>(values: impl IntoIterator<Item = &'a Value>) -> Type {
        let mut seen = false;
        let mut ints = true;
        let mut nums = true;
        let mut bools = true;
        for v in values {
            if v.is_empty() {
                continue;
            }
            seen = true;
            match v {
                // `2^53` is the largest integer an `f64` (a cell's number) holds exactly.
                Value::Number(n) if n.is_finite() => {
                    bools = false;
                    if n.fract() != 0.0 || n.abs() > 9_007_199_254_740_992.0 {
                        ints = false;
                    }
                }
                Value::Number(_) => {
                    bools = false;
                    ints = false;
                }
                Value::Bool(_) => {
                    ints = false;
                    nums = false;
                }
                _ => {
                    ints = false;
                    nums = false;
                    bools = false;
                }
            }
        }
        match (seen, ints, nums, bools) {
            (true, true, _, _) => Type::Int64,
            (true, false, true, _) => Type::Double,
            (true, false, false, true) => Type::Bool,
            _ => Type::Text,
        }
    }
}

/// The data model's SQL engine: one in-memory database per workbook.
///
/// Every method is total: a build without the engine, bad SQL, a result too big for a table and a
/// database error each come back as a [`SqlError`], never as a panic.
pub struct Engine {
    #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
    duck: duck::Duck,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Engine")
    }
}

impl Engine {
    /// A new, empty in-memory database, using up to eight threads.
    ///
    /// Nothing is created until the caller does it: a workbook with no model pays for no tables.
    pub fn in_memory() -> Result<Engine, SqlError> {
        #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
        {
            Ok(Engine { duck: duck::Duck::new()? })
        }
        #[cfg(not(all(feature = "duckdb", not(target_arch = "wasm32"))))]
        {
            Err(SqlError::Unavailable)
        }
    }

    /// Runs a query and reads every row into a [`Table`].
    ///
    /// The result is capped by [`MAX_ROWS`] and [`MAX_CELLS`] as it is read: a query that would
    /// return more is a [`SqlError::TooBig`], not an allocation.
    pub fn query(&self, sql: &str) -> Result<Table, SqlError> {
        check_sql(sql)?;
        #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
        {
            self.duck.query(sql)
        }
        #[cfg(not(all(feature = "duckdb", not(target_arch = "wasm32"))))]
        {
            Err(SqlError::Unavailable)
        }
    }

    /// Runs a statement that returns no rows (a `CREATE`, a `DROP`, an `INSERT`), and reports how
    /// many rows it changed.
    pub fn execute(&self, sql: &str) -> Result<u64, SqlError> {
        check_sql(sql)?;
        #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
        {
            self.duck.execute(sql)
        }
        #[cfg(not(all(feature = "duckdb", not(target_arch = "wasm32"))))]
        {
            Err(SqlError::Unavailable)
        }
    }

    /// Creates `name` from rows Cella already holds (a sheet range, a query's result) and returns
    /// how many rows it took.
    ///
    /// This is the **slow** way in — measured at ~720 ns a row, because every value crosses the
    /// engine's own interface — so it is for the one-time "add this to the model" action, not for
    /// a refresh loop. A file the engine can read itself belongs in [`Engine::load_file`], at
    /// 22–99 ns a row.
    pub fn load_rows(&mut self, name: &str, columns: &[String], rows: &[Vec<Value>]) -> Result<usize, SqlError> {
        check_name(name)?;
        if columns.is_empty() {
            return Err(SqlError::Invalid("a table needs at least one column".into()));
        }
        for column in columns {
            check_name(column)?;
        }
        if columns.len() > MAX_COLUMNS {
            return Err(SqlError::TooBig { rows: rows.len(), columns: columns.len() });
        }
        #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
        {
            self.duck.load_rows(name, columns, rows)
        }
        #[cfg(not(all(feature = "duckdb", not(target_arch = "wasm32"))))]
        {
            let _ = rows;
            Err(SqlError::Unavailable)
        }
    }

    /// Creates (or replaces) `name` from one or more files **the engine reads itself**.
    ///
    /// Several paths are read as one table, which is how a folder of Parquet parts becomes a
    /// dataset. Nothing is copied into memory Cella controls: this is the fast way in.
    pub fn load_file(&mut self, name: &str, paths: &[String], format: FileFormat) -> Result<(), SqlError> {
        check_name(name)?;
        if paths.is_empty() {
            return Err(SqlError::Invalid("no file to load".into()));
        }
        for path in paths {
            if path.is_empty() {
                return Err(SqlError::Invalid("a file path cannot be empty".into()));
            }
            if path.contains('\0') {
                return Err(SqlError::Invalid("a file path cannot contain a NUL".into()));
            }
        }
        #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
        {
            self.duck.load_file(name, paths, format)
        }
        #[cfg(not(all(feature = "duckdb", not(target_arch = "wasm32"))))]
        {
            let _ = (paths, format);
            Err(SqlError::Unavailable)
        }
    }
}

/// A table or column name as SQL quotes it: the name's own double quotes are doubled, so
/// `Sales"; DROP TABLE t --` stays a name.
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// A text literal as SQL quotes it (a file path, a slicer value): the quote character is doubled.
pub fn quote_literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn check_sql(sql: &str) -> Result<(), SqlError> {
    if sql.trim().is_empty() {
        return Err(SqlError::Invalid("a query cannot be empty".into()));
    }
    Ok(())
}

fn check_name(name: &str) -> Result<(), SqlError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(SqlError::Invalid("a name cannot be blank".into()));
    }
    if trimmed.chars().count() > MAX_NAME {
        return Err(SqlError::Invalid(format!("a name cannot be longer than {MAX_NAME} characters")));
    }
    if trimmed.contains('\0') {
        return Err(SqlError::Invalid("a name cannot contain a NUL".into()));
    }
    Ok(())
}
