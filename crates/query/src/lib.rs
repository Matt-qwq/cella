//! The query pipeline: a table of values in, the same table with steps applied, a table of values
//! out.
//!
//! The steps are the document's (`gridcraft_model::QueryStep`); this crate is the engine that
//! runs them. It does **no I/O**: reading a file, a sheet or a folder into the first `Table`
//! belongs to whoever has the bytes (the engine command), which keeps the whole pipeline testable
//! without a file system and keeps `crates/query` buildable for WebAssembly.
//!
//! Two properties are the point of the crate, and every step is written to keep them:
//!
//! * **Total.** Any well-formed step list runs to completion or returns an error naming the step;
//!   it never panics, never loops on hostile input, and never allocates from a number a file chose
//!   before checking it against `MAX_CELLS`.
//! * **Honest.** A step that names a column that isn't there is an error, not a silent no-op: a
//!   pipeline that quietly drops a step's effect is worse than one that stops.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use gridcraft_core::date::{datetime_from_serial, serial_from_ymd};
use gridcraft_core::{CellError, DateSystem, Value};
use gridcraft_model::{QueryColumnType, QueryFilterOp, QueryStep};

mod steps;
#[cfg(test)]
mod tests;

/// Excel's grid: no table can be loaded if it is taller than this.
pub const MAX_ROWS: usize = 1_048_576;
/// Excel's grid: no table can be loaded if it is wider than this.
pub const MAX_COLUMNS: usize = 16_384;
/// The cell budget one table may occupy (4M cells: a 2,000 × 2,000 import): a query result is
/// materialised in memory *twice* (once for the pipeline, once for the sheet), so the honest limit
/// is smaller than rows × columns.
pub const MAX_CELLS: usize = 4_194_304;

/// Rows of values with named columns — what a step list operates on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    /// Column names in order. Never contains a name twice (a step that would create a duplicate
    /// is an error).
    pub columns: Vec<String>,
    /// One entry per row, each exactly [`Table::width`] long.
    pub rows: Vec<Vec<Value>>,
}

impl Table {
    /// A table with the given columns and rows. Rows are padded with [`Value::Empty`] and
    /// truncated to the header's width, so a ragged source (a short CSV line) still yields a
    /// rectangular table.
    pub fn new(columns: Vec<String>, rows: Vec<Vec<Value>>) -> Result<Table, QueryError> {
        let mut t = Table { columns, rows };
        for row in &mut t.rows {
            row.resize(t.columns.len(), Value::Empty);
        }
        t.check_size()?;
        Ok(t)
    }

    /// A table from raw rows, the way a file arrives: `header` takes the first row as the column
    /// names (blank cells become `Column1`, `Column2`… and duplicates get a number appended, the
    /// way Excel names them), otherwise the columns are named `Column1` upward.
    pub fn from_rows(rows: Vec<Vec<Value>>, header: bool) -> Result<Table, QueryError> {
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        if header {
            let first = rows.first().cloned().unwrap_or_default();
            let names: Vec<String> = (0..width).map(|i| first.get(i).map(cell_text).unwrap_or_default()).collect();
            let names = unique_headers(names);
            let rest = rows.into_iter().skip(1).collect();
            return Table::new(names, rest);
        }
        let names = unique_headers(vec![String::new(); width]);
        Table::new(names, rows)
    }

    /// A table with no columns and no rows.
    pub fn empty() -> Table {
        Table { columns: Vec::new(), rows: Vec::new() }
    }

    pub fn width(&self) -> usize {
        self.columns.len()
    }

    pub fn height(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty() && self.columns.is_empty()
    }

    /// The index of `name`, matched exactly first and case-insensitively after, since a person
    /// types a column name without caring about its case.
    pub fn index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c == name).or_else(|| self.columns.iter().position(|c| c.eq_ignore_ascii_case(name)))
    }

    /// Refuses a table a sheet could not hold (and a table that would cost more memory than the
    /// import budget allows). Called by every step that can grow the table.
    pub(crate) fn check_size(&self) -> Result<(), QueryError> {
        let (rows, columns) = (self.rows.len(), self.columns.len());
        if rows > MAX_ROWS || columns > MAX_COLUMNS || rows.saturating_mul(columns) > MAX_CELLS {
            return Err(QueryError::TooLarge { rows, columns, limit: MAX_CELLS });
        }
        Ok(())
    }
}

/// Runs every step in order.
pub fn apply(table: Table, steps: &[QueryStep]) -> Result<Table, QueryError> {
    let mut t = table;
    for (index, step) in steps.iter().enumerate() {
        t = steps::apply_step(t, step, index)?;
    }
    Ok(t)
}

/// Why a pipeline stopped.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum QueryError {
    /// The result does not fit in a sheet.
    #[error("too much data for one table: {rows} rows and {columns} columns (the limit is {limit} cells)")]
    TooLarge { rows: usize, columns: usize, limit: usize },
    /// A step named a column the table doesn't have.
    #[error("step {index} ({step}): there is no column named \"{column}\"")]
    NoColumn { index: usize, step: &'static str, column: String },
    /// A step cannot be applied to this table (a blank name, a duplicate, nothing to promote).
    #[error("step {index} ({step}): {reason}")]
    Invalid { index: usize, step: &'static str, reason: String },
}

// ---------------------------------------------------------------- helpers shared by the steps

/// A value's text, the way the grid shows it (numbers with Excel's General format, errors as
/// `#N/A`).
pub(crate) fn cell_text(v: &Value) -> String {
    match v {
        Value::Empty => String::new(),
        other => other.display(),
    }
}

/// Names for a header row: blank cells become `Column<n>` and a name used twice gets the smallest
/// number that makes it unique (`Amount`, `Amount2`), which is what Excel's imports do.
pub(crate) fn unique_headers(raw: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(raw.len());
    for (i, name) in raw.into_iter().enumerate() {
        let base = {
            let t = name.trim();
            if t.is_empty() { format!("Column{}", i + 1) } else { t.to_string() }
        };
        let mut candidate = base.clone();
        let mut n = 1u32;
        while out.iter().any(|c| c.eq_ignore_ascii_case(&candidate)) {
            n = n.saturating_add(1);
            candidate = format!("{base}{n}");
        }
        out.push(candidate);
    }
    out
}

/// Parses a value into the type a step asks for. A value that cannot be converted becomes
/// `#VALUE!`, which is what Power Query does too and what the loaded table then shows.
pub(crate) fn convert(v: &Value, to: QueryColumnType) -> Value {
    match v {
        Value::Empty => Value::Empty,
        Value::Error(_) => v.clone(),
        _ => {
            let text = cell_text(v);
            let trimmed = text.trim();
            match to {
                QueryColumnType::Text => Value::text(text.clone()),
                QueryColumnType::Number => match trimmed.parse::<f64>() {
                    Ok(n) if n.is_finite() => Value::number(n),
                    _ => Value::Error(CellError::Value),
                },
                QueryColumnType::WholeNumber => match trimmed.parse::<f64>() {
                    Ok(n) if n.is_finite() => Value::number(n.trunc()),
                    _ => Value::Error(CellError::Value),
                },
                QueryColumnType::Boolean => parse_bool(trimmed).map_or(Value::Error(CellError::Value), Value::Bool),
                QueryColumnType::Date => parse_date(trimmed),
            }
        }
    }
}

fn parse_bool(text: &str) -> Option<bool> {
    match text.to_ascii_lowercase().as_str() {
        "true" | "yes" | "y" | "1" => Some(true),
        "false" | "no" | "n" | "0" => Some(false),
        _ => None,
    }
}

/// `YYYY-MM-DD` (and `YYYY/MM/DD`) become a serial number; a number is already a serial, and
/// anything else is a `#VALUE!`. Only the ISO order is accepted: guessing whether `03/04/2024` is
/// March or April is how a data platform loses a day.
fn parse_date(text: &str) -> Value {
    if let Ok(n) = text.parse::<f64>() {
        return if n.is_finite() { Value::number(n) } else { Value::Error(CellError::Value) };
    }
    let parts: Vec<&str> = text.split(['-', '/']).collect();
    if parts.len() != 3 || parts[0].len() != 4 {
        return Value::Error(CellError::Value);
    }
    let (Ok(y), Ok(m), Ok(d)) = (parts[0].parse::<i64>(), parts[1].parse::<i64>(), parts[2].parse::<i64>()) else {
        return Value::Error(CellError::Value);
    };
    if parts[1].len() > 2 || parts[2].len() > 2 {
        return Value::Error(CellError::Value);
    }
    // Validate by converting back: `serial_from_ymd` rolls 2024-13-01 over into 2025-01-01, and a
    // data platform that silently moves a date by a year is worse than one that says #VALUE!.
    let Some(serial) = serial_from_ymd(DateSystem::D1900, y, m, d) else {
        return Value::Error(CellError::Value);
    };
    match datetime_from_serial(DateSystem::D1900, serial) {
        Some(dt) if i64::from(dt.year) == y && i64::from(dt.month) == m && i64::from(dt.day) == d => Value::number(serial),
        _ => Value::Error(CellError::Value),
    }
}

/// A value that reads as a number (10 typed in a text column) compares as one, so a filter for
/// "> 9" keeps 10 as well as 100; everything else compares as text, ignoring case (a person's
/// "west" matches "West"). Blanks sort after every value, as Excel sorts them.
pub(crate) fn compare(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ => {}
    }
    let (x, y) = (a.to_number().ok(), b.to_number().ok());
    match (x, y) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        _ => cell_text(a).to_lowercase().cmp(&cell_text(b).to_lowercase()),
    }
}

/// Whether a cell matches a filter. A blank never matches a text or a comparison test (there is
/// nothing to compare); `IsBlank` and `NotEquals` are how a person asks for blanks.
pub(crate) fn matches(v: &Value, op: QueryFilterOp, want: Option<&str>) -> bool {
    let want = want.unwrap_or("");
    let text = cell_text(v).to_lowercase();
    let needle = want.to_lowercase();
    let ord = || compare(v, &Value::text(want));
    match op {
        QueryFilterOp::IsBlank => v.is_empty(),
        QueryFilterOp::IsNotBlank => !v.is_empty(),
        QueryFilterOp::Equals => !v.is_empty() && ord() == std::cmp::Ordering::Equal,
        QueryFilterOp::NotEquals => v.is_empty() || ord() != std::cmp::Ordering::Equal,
        QueryFilterOp::Contains => text.contains(&needle),
        QueryFilterOp::NotContains => !text.contains(&needle),
        QueryFilterOp::StartsWith => text.starts_with(&needle),
        QueryFilterOp::EndsWith => text.ends_with(&needle),
        QueryFilterOp::Greater => !v.is_empty() && ord() == std::cmp::Ordering::Greater,
        QueryFilterOp::GreaterOrEqual => !v.is_empty() && ord() != std::cmp::Ordering::Less,
        QueryFilterOp::Less => !v.is_empty() && ord() == std::cmp::Ordering::Less,
        QueryFilterOp::LessOrEqual => !v.is_empty() && ord() != std::cmp::Ordering::Greater,
    }
}
