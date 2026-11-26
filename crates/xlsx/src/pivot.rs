//! PivotTables: pivot cache definitions and records (ECMA-376 Part 1 §18.10.1) and PivotTable
//! definitions (§18.10.1.73), written from the model's [`PivotTable`]s and read back into them.
//!
//! Writing: one cache per PivotTable, with a records snapshot of the source list (omitted, with
//! `saveData="0"`, for very large sources) and the records themselves, so a reader can render the
//! report without a refresh (`refreshOnLoad` would make Excel rebuild the cache on open and
//! re-anchor a number field's grouping at its minimum). Fields used on an axis get shared items (or, for date
//! grouping, `fieldGroup` group items) so the table's `<items>` can refer to them. The report
//! cells the engine wrote stay on the sheet as plain cells.
//!
//! Reading: the cache's source and field names plus the table's areas, functions, show-values-as,
//! layout flags, grand totals and style become a model [`PivotTable`]. Anything we don't model
//! (calculated fields without a readable formula, numeric grouping…) becomes a warning.
//!
//! A cache that is not a worksheet range — a data model (Power Pivot), an OLAP cube, a connection
//! — is a report Cella does not compute: it is read as a [`PivotTable`] with
//! [`PivotSourceKind::DataModel`] and a reason, its cached values stay the cells they are, and
//! its parts are carried by the pass-through byte for byte. An OLAP engine plugs in here: it fills
//! in the records instead of the range reader, and the report stops being read-only.

use crate::IoError;
use crate::read::Ctx;
use crate::write::{NS_MAIN, NS_REL, Out, Rels, XML_DECL, abs_range};
use crate::xml::{El, esc_attr};
use cella_core::date::datetime_from_serial;
use cella_core::{CellRef, DateSystem, RangeRef, Value, number_to_text};
use cella_formula::quote_sheet;
use cella_model::pivot_item;
use cella_model::{
    PivotCalcField, PivotCalcItem, PivotDateGroup, PivotField, PivotFilter, PivotFilterArea, PivotFunc, PivotLayout, PivotNumberGroup, PivotShowAs,
    PivotSort, PivotSourceKind, PivotTable, PivotValue, Workbook,
};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

mod read;
mod write;

/// Largest `xl/connections.xml` read to tell the workbook's own data model (`type="5"`, what a
/// cache names through `cacheSource/@connectionId`) from an ordinary external connection.
const MAX_CONNECTIONS: u64 = 8 * 1024 * 1024;

pub use read::{CachesIn, read_pivots};
pub use write::{PivotWriter, source_column, source_fields};

pub const CT_PIVOT_TABLE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotTable+xml";
pub const CT_PIVOT_CACHE_DEF: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheDefinition+xml";
pub const CT_PIVOT_CACHE_REC: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheRecords+xml";
/// Largest source block scanned for a cache (cells); bigger sources are not written.
const MAX_SOURCE_CELLS: u64 = 10_000_000;
/// Largest records snapshot written (cells); bigger caches are written without records.
const MAX_RECORD_CELLS: u64 = 2_000_000;
/// Most shared items kept per field (Excel's limit is 1,048,576 unique items).
const MAX_ITEMS: usize = 1_048_576;
/// Most calculated items written per PivotTable.
const MAX_CALC_ITEMS: usize = 256;
/// Most cache fields read.
const MAX_FIELDS: usize = 16_384;
/// The `x14:dataField` extension (Excel 2010 show-values-as additions such as rank).
pub(crate) const EXT_X14_DATA_FIELD: &str = "{E15A36E0-9728-4e99-A89B-3F7291B0FE68}";
pub(crate) const EXT_X14_PIVOT_TABLE: &str = "{962EF5D1-5CA2-4c93-8EF4-DBF5C05439D2}";
const NS_X14: &str = "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main";
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
// ================================================================ shared helpers

/// The pivot item label of a value (blank shows as "(blank)").
fn label(v: &Value) -> String {
    match v {
        Value::Empty => "(blank)".into(),
        Value::Number(n) => number_to_text(*n),
        Value::Text(t) => t.to_string(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Value::Error(e) => e.as_str().into(),
        Value::Array(_) => label(&v.scalar()),
    }
}
fn same(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}
fn func_name(f: PivotFunc) -> &'static str {
    match f {
        PivotFunc::Sum => "sum",
        PivotFunc::Count => "count",
        PivotFunc::Average => "average",
        PivotFunc::Max => "max",
        PivotFunc::Min => "min",
        PivotFunc::Product => "product",
        PivotFunc::CountNumbers => "countNums",
        PivotFunc::StdDev => "stdDev",
        PivotFunc::StdDevP => "stdDevp",
        PivotFunc::Var => "var",
        PivotFunc::VarP => "varp",
    }
}
fn func_from(s: &str) -> Option<PivotFunc> {
    Some(match s {
        "sum" => PivotFunc::Sum,
        "count" => PivotFunc::Count,
        "average" => PivotFunc::Average,
        "max" => PivotFunc::Max,
        "min" => PivotFunc::Min,
        "product" => PivotFunc::Product,
        "countNums" => PivotFunc::CountNumbers,
        "stdDev" => PivotFunc::StdDev,
        "stdDevp" => PivotFunc::StdDevP,
        "var" => PivotFunc::Var,
        "varp" => PivotFunc::VarP,
        _ => return None,
    })
}
fn func_caption(f: PivotFunc) -> &'static str {
    match f {
        PivotFunc::Sum => "Sum",
        PivotFunc::Count => "Count",
        PivotFunc::Average => "Average",
        PivotFunc::Max => "Max",
        PivotFunc::Min => "Min",
        PivotFunc::Product => "Product",
        PivotFunc::CountNumbers => "Count",
        PivotFunc::StdDev => "StdDev",
        PivotFunc::StdDevP => "StdDevp",
        PivotFunc::Var => "Var",
        PivotFunc::VarP => "Varp",
    }
}
fn group_by_name(g: PivotDateGroup) -> &'static str {
    match g {
        PivotDateGroup::Years => "years",
        PivotDateGroup::Quarters => "quarters",
        PivotDateGroup::Months => "months",
        PivotDateGroup::Days => "days",
        PivotDateGroup::None => "range",
    }
}
fn group_caption(g: PivotDateGroup) -> &'static str {
    match g {
        PivotDateGroup::Years => "Years",
        PivotDateGroup::Quarters => "Quarters",
        PivotDateGroup::Months => "Months",
        PivotDateGroup::Days => "Days",
        PivotDateGroup::None => "Group",
    }
}
/// `2024-03-05T00:00:00` for a date serial.
fn iso(sys: DateSystem, serial: f64) -> Option<String> {
    let d = datetime_from_serial(sys, serial)?;
    Some(format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", d.year, d.month, d.day, d.hour, d.minute, d.second))
}
/// `3/5/2024` for a date serial (group boundary captions).
fn mdy(sys: DateSystem, serial: f64) -> String {
    match datetime_from_serial(sys, serial) {
        Some(d) => format!("{}/{}/{}", d.month, d.day, d.year),
        None => number_to_text(serial),
    }
}
// ================================================================ writing

/// Identity of a shared item (text items match case-insensitively, as in Excel).
#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Blank,
    Num(u64),
    Text(String),
    Bool(bool),
    Err(&'static str),
}
fn key(v: &Value) -> Key {
    match v {
        Value::Empty => Key::Blank,
        Value::Number(n) => Key::Num(if *n == 0.0 { 0 } else { n.to_bits() }),
        Value::Text(t) => Key::Text(t.to_lowercase()),
        Value::Bool(b) => Key::Bool(*b),
        Value::Error(e) => Key::Err(e.as_str()),
        Value::Array(_) => key(&v.scalar()),
    }
}
