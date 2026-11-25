//! The DuckDB implementation: the one file in the tree that names the engine.
//!
//! Written so the rest of the crate never sees a `duckdb` type — an engine error crosses the
//! boundary as its message, and a result as [`Table`], which is why `captures no engine` builds
//! (the browser) can compile the same public surface.

use cella_core::{CellError, Value, number_to_text};
use duckdb::Connection;
use duckdb::types::{TimeUnit, Value as DuckValue, ValueRef};

use crate::{FileFormat, MAX_CELLS, MAX_ROWS, SqlError, Table, Type, quote_ident, quote_literal};

/// 1970-01-01 as a serial number in the 1900 date system: the engine counts days from the epoch,
/// a spreadsheet counts them from 1899-12-30 (and calls 1900-01-01 day 1).
const SERIAL_1970: f64 = 25569.0;

/// One in-memory DuckDB database.
pub(crate) struct Duck {
    conn: Connection,
}

impl Duck {
    /// A new empty database. Threads are capped at eight: the machine's core count is what a
    /// query is worth, and a large count makes small queries pay for scheduling.
    pub(crate) fn new() -> Result<Duck, SqlError> {
        let conn = Connection::open_in_memory().map_err(engine)?;
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, 8);
        conn.execute_batch(&format!("SET threads TO {threads};")).map_err(engine)?;
        Ok(Duck { conn })
    }

    pub(crate) fn execute(&self, sql: &str) -> Result<u64, SqlError> {
        let changed = self.conn.execute(sql, []).map_err(engine)?;
        Ok(changed as u64)
    }

    pub(crate) fn query(&self, sql: &str) -> Result<Table, SqlError> {
        let mut stmt = self.conn.prepare(sql).map_err(engine)?;
        let mut rows = stmt.query([]).map_err(engine)?;

        // The column list is read from the *executed* statement: a `CREATE` in the same call
        // could have changed the schema a prepared statement was bound against.
        let mut columns: Vec<String> = Vec::new();
        match rows.as_ref() {
            Some(executed) => {
                for i in 0..executed.column_count() {
                    let name = executed.column_name(i).map_err(engine)?;
                    columns.push(name.clone());
                }
            }
            None => return Err(SqlError::Engine("the engine did not execute the statement".into())),
        }
        if columns.len() > crate::MAX_COLUMNS {
            return Err(SqlError::TooBig { rows: 0, columns: columns.len() });
        }

        let width = columns.len();
        let mut out = Table { columns, rows: Vec::new() };
        while let Some(row) = rows.next().map_err(engine)? {
            let next = out.rows.len().saturating_add(1);
            if next > MAX_ROWS || next.saturating_mul(width) > MAX_CELLS {
                return Err(SqlError::TooBig { rows: next, columns: width });
            }
            let mut values = Vec::with_capacity(width);
            for i in 0..width {
                let value = row.get_ref(i).map_err(engine)?;
                values.push(to_value(value));
            }
            out.rows.push(values);
        }
        Ok(out)
    }

    pub(crate) fn load_rows(&self, name: &str, columns: &[String], rows: &[Vec<Value>]) -> Result<usize, SqlError> {
        let types: Vec<Type> = (0..columns.len()).map(|c| Type::infer(rows.iter().filter_map(|r| r.get(c)))).collect();

        let mut ddl = String::with_capacity(32 + columns.len() * 32);
        ddl.push_str("CREATE TABLE ");
        ddl.push_str(&quote_ident(name));
        ddl.push_str(" (");
        for (i, column) in columns.iter().enumerate() {
            if i > 0 {
                ddl.push_str(", ");
            }
            ddl.push_str(&quote_ident(column));
            ddl.push(' ');
            ddl.push_str(types.get(i).map_or("VARCHAR", |t| t.as_sql()));
        }
        ddl.push_str(");");
        self.conn.execute_batch(&ddl).map_err(engine)?;

        let mut appender = self.conn.appender(name).map_err(engine)?;
        // One buffer for the whole load: the appender reads a row's parameters, so only the
        // *values* are new per row and the vector that holds them is not.
        let mut params: Vec<DuckValue> = Vec::with_capacity(columns.len());
        for row in rows {
            params.clear();
            for (i, ty) in types.iter().enumerate() {
                params.push(param(row.get(i).unwrap_or(&Value::Empty), *ty));
            }
            let refs: Vec<&dyn duckdb::ToSql> = params.iter().map(|v| v as &dyn duckdb::ToSql).collect();
            appender.append_row(refs.as_slice()).map_err(engine)?;
        }
        // `flush` is the call that reports a constraint failure: dropping the appender would
        // swallow it (`duckdb`'s own note on `Appender`).
        appender.flush().map_err(engine)?;
        Ok(rows.len())
    }

    pub(crate) fn load_file(&self, name: &str, paths: &[String], format: FileFormat) -> Result<(), SqlError> {
        let list = paths.iter().map(|p| quote_literal(p)).collect::<Vec<_>>().join(", ");
        let reader = match format {
            // `auto_detect` is what makes a CSV of unknown columns readable without a schema
            // step: the engine sniffs the header and the type of each column.
            FileFormat::Parquet => format!("read_parquet([{list}])"),
            FileFormat::Csv => format!("read_csv([{list}], header = true, auto_detect = true)"),
        };
        let sql = format!("CREATE OR REPLACE TABLE {} AS SELECT * FROM {reader};", quote_ident(name));
        self.conn.execute_batch(&sql).map_err(engine)?;
        Ok(())
    }
}

fn engine(e: duckdb::Error) -> SqlError {
    SqlError::Engine(e.to_string())
}

/// One result cell as a spreadsheet value.
///
/// A date and a timestamp are serial numbers, because that is what a spreadsheet cell's date is:
/// keeping a time zone here would invent one. A type with no cell equivalent (a blob, a list, a
/// struct, an interval) is `#VALUE!` rather than a plausible-looking string, so a query that
/// reached into a nested column says so.
fn to_value(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Empty,
        ValueRef::Boolean(b) => Value::Bool(b),
        ValueRef::TinyInt(x) => Value::number(f64::from(x)),
        ValueRef::SmallInt(x) => Value::number(f64::from(x)),
        ValueRef::Int(x) => Value::number(f64::from(x)),
        ValueRef::BigInt(x) => Value::number(x as f64),
        ValueRef::UTinyInt(x) => Value::number(f64::from(x)),
        ValueRef::USmallInt(x) => Value::number(f64::from(x)),
        ValueRef::UInt(x) => Value::number(f64::from(x)),
        ValueRef::UBigInt(x) => Value::number(x as f64),
        ValueRef::Float(x) => Value::number(f64::from(x)),
        ValueRef::Double(x) => Value::number(x),
        // `SUM` of a `BIGINT` column is a `HUGEINT` in DuckDB (it widens to avoid overflowing),
        // so a plain sum of whole numbers must come back as a number, not an error. A cell holds
        // an `f64`, so beyond 2^53 the last digits are the same ones Excel would round.
        ValueRef::HugeInt(x) => Value::number(x as f64),
        ValueRef::UHugeInt(x) => Value::number(x as f64),
        // `Decimal`'s display is its exact digits, which parse back to the nearest `f64`.
        ValueRef::Decimal(d) => match d.to_string().parse::<f64>() {
            Ok(n) => Value::number(n),
            Err(_) => Value::Error(CellError::Num),
        },
        ValueRef::Date32(days) => Value::number(SERIAL_1970 + f64::from(days)),
        ValueRef::Timestamp(unit, v) => Value::number(SERIAL_1970 + seconds(unit, v) / 86_400.0),
        ValueRef::Time64(unit, v) => Value::number(seconds(unit, v) / 86_400.0),
        ValueRef::Text(bytes) => Value::text(String::from_utf8_lossy(bytes).into_owned()),
        // Blobs, geometry, lists, structs, maps, unions, enums, intervals: no cell equivalent.
        _ => Value::Error(CellError::Value),
    }
}

fn seconds(unit: TimeUnit, v: i64) -> f64 {
    let v = v as f64;
    match unit {
        TimeUnit::Second => v,
        TimeUnit::Millisecond => v / 1e3,
        TimeUnit::Microsecond => v / 1e6,
        TimeUnit::Nanosecond => v / 1e9,
    }
}

/// One cell as the parameter its column's type takes.
///
/// The type was inferred from these same values, so a `BIGINT` column holds only the whole
/// numbers a cell can hold exactly; anything that is not a number in a text column is written as
/// the grid shows it (an error as `#N/A`, a boolean as `TRUE`), which is what a person reading
/// the loaded table expects.
fn param(value: &Value, ty: Type) -> DuckValue {
    match (value, ty) {
        (Value::Empty, _) => DuckValue::Null,
        // A spilled array is not a scalar; a null is the honest value for one.
        (Value::Array(_), _) => DuckValue::Null,
        (Value::Number(n), Type::Int64) if n.is_finite() => DuckValue::BigInt(n.trunc() as i64),
        (Value::Number(n), Type::Double) => DuckValue::Double(*n),
        (Value::Bool(b), Type::Bool) => DuckValue::Boolean(*b),
        (Value::Text(t), Type::Text) => DuckValue::Text(t.to_string()),
        (Value::Number(n), Type::Text) => DuckValue::Text(number_to_text(*n)),
        (other, Type::Text) => DuckValue::Text(other.display()),
        (Value::Bool(b), _) => DuckValue::Text(if *b { "TRUE".to_string() } else { "FALSE".to_string() }),
        (other, _) => DuckValue::Text(other.display()),
    }
}
