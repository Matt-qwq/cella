//! Queries — Get & Transform (Power Query class): where the data comes from, what was done to it,
//! and where the result loads.
//!
//! The types here are **document data only**: they are what a workbook stores and what the file
//! writer round-trips. The interpreter that turns a source into rows and runs the steps lives in
//! `gridcraft-query` (the same split as `Table` in this crate and the pivot engine in
//! `gridcraft-calc`).
//!
//! A query is deliberately not "the data": without a cache, a reopened workbook shows the table
//! its last run wrote onto a sheet (see [`QueryLoad`]). That is Excel's own model — *load to*
//! is what materialises a query, and a query that was never loaded has nothing to show but its
//! steps.

use gridcraft_core::RangeRef;
use serde::{Deserialize, Serialize};

/// What a URL serves, as the table it becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpFormat {
    /// Delimited text (comma unless the query's steps say otherwise).
    Csv,
    /// A JSON array of objects, or of arrays.
    Json,
}

impl HttpFormat {
    /// The variant's name, spelled the way serde derives it: the stored part is the model's own
    /// serde shape, and a test holds the hand-written codec to that. The JSON API of a command
    /// reads it case-insensitively, so "csv" and "Csv" both work there.
    pub fn as_str(self) -> &'static str {
        match self {
            HttpFormat::Csv => "Csv",
            HttpFormat::Json => "Json",
        }
    }

    /// The format a URL names, from its extension: what a person pasting a link expects. A URL
    /// with no useful extension reads as CSV, the common case for an endpoint that serves one.
    pub fn from_url(url: &str) -> HttpFormat {
        let path = url.split(['?', '#']).next().unwrap_or(url);
        if path.to_ascii_lowercase().ends_with(".json") { HttpFormat::Json } else { HttpFormat::Csv }
    }
}

/// Most steps one query may hold. A pipeline is read by a person; a thousand steps is a file that
/// is trying to make the app do work, and every step is a linear pass over the rows.
pub const MAX_STEPS: usize = 200;

/// Longest step/column name kept. The grid's own limit is 32,767 characters for a cell's text;
/// a column name that long is a file's idea of a joke and the writer would have to quote it.
pub const MAX_NAME: usize = 255;

/// Where a query's rows come from.
///
/// `path` is absolute or relative to the workbook, as the file was opened; `text` is the same
/// data inline, for a query that must not depend on a file that may move (a pasted table, a
/// test, a workbook that has to stand alone).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum QuerySource {
    /// A delimited text file (CSV, TSV, or any other single-character delimiter).
    Csv { path: Option<String>, text: Option<String>, delimiter: char },
    /// A JSON file: an array of objects (columns are their keys, in first-seen order) or an
    /// array of arrays (the first row is the header when `promote` is set).
    Json { path: Option<String>, text: Option<String> },
    /// One sheet of an XLSX workbook; `sheet` selects by name, the first sheet when absent.
    Xlsx { path: String, sheet: Option<String> },
    /// A Parquet file (flat columns only — the reader refuses nested ones).
    Parquet { path: String },
    /// Every file in a folder whose name matches `filter` (`*.csv`), stacked with a source
    /// column (`Folder Path`), the way Excel does it.
    Folder { path: String, filter: String },
    /// A SQLite database file: `path` is the file, `sql` is one statement whose result set
    /// becomes the table (its columns are the statement's output columns, so no PromoteHeaders
    /// step is needed). Opened **read-only**, so a query can never change the database.
    ///
    /// Reading it needs the engine's `connectors` feature (a bundled SQLite); a build without it
    /// keeps the query in the document and reports that it cannot run it, which is what the web
    /// build does.
    Sqlite { path: String, sql: String },
    /// A URL: the bytes it serves are read as `format`. The workbook therefore depends on the
    /// network when it refreshes — never when it opens, because a source is only read by a
    /// refresh (or by a preview, which is a refresh of the first steps).
    Http { url: String, format: HttpFormat },
    /// A table that already lives in this workbook.
    Table { sheet: String, name: String },
    /// A range of a sheet in this workbook (`Sheet1!A1:C9`).
    Range { sheet: String, range: RangeRef },
    /// Another query's result, as a stage of this one: the reference queries that make a pipeline
    /// readable — clean once, then report from the clean table.
    ///
    /// Unlike a file, whose first row is data like any other, a query arrives **with its columns
    /// already named**: the upstream query's steps have run, so this one needs no PromoteHeaders.
    Query { name: String },
}

impl QuerySource {
    /// A short name for the pane and for error messages ("Csv", "Xlsx", "Folder").
    pub fn kind(&self) -> &'static str {
        match self {
            QuerySource::Csv { .. } => "Csv",
            QuerySource::Json { .. } => "Json",
            QuerySource::Xlsx { .. } => "Xlsx",
            QuerySource::Parquet { .. } => "Parquet",
            QuerySource::Folder { .. } => "Folder",
            QuerySource::Sqlite { .. } => "Sqlite",
            QuerySource::Http { .. } => "Http",
            QuerySource::Table { .. } => "Table",
            QuerySource::Range { .. } => "Range",
            QuerySource::Query { .. } => "Query",
        }
    }

    /// The file the source names, when it names one (for "the file moved" errors and for the
    /// Queries pane's second line).
    pub fn path(&self) -> Option<&str> {
        match self {
            QuerySource::Csv { path, .. } | QuerySource::Json { path, .. } => path.as_deref(),
            QuerySource::Xlsx { path, .. } | QuerySource::Parquet { path } | QuerySource::Folder { path, .. } => Some(path),
            QuerySource::Sqlite { path, .. } => Some(path),
            // A URL is shown in the pane's second line exactly as a path is: it is where the rows
            // come from, and it is the thing that breaks when it breaks.
            QuerySource::Http { url, .. } => Some(url),
            QuerySource::Query { name } => Some(name),
            QuerySource::Table { .. } | QuerySource::Range { .. } => None,
        }
    }
}

/// The types a column can be changed to. `Text` is the escape hatch: every value has a text form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueryColumnType {
    Text,
    Number,
    WholeNumber,
    Date,
    Boolean,
}

impl QueryColumnType {
    /// The name the UI prints, in the Power Query vocabulary.
    pub fn label(self) -> &'static str {
        match self {
            QueryColumnType::Text => "Text",
            QueryColumnType::Number => "Decimal Number",
            QueryColumnType::WholeNumber => "Whole Number",
            QueryColumnType::Date => "Date",
            QueryColumnType::Boolean => "True/False",
        }
    }
}

/// How `Filter` compares. Comparison operators are numeric when both sides parse as numbers and
/// textual otherwise, so "10" sorts after "9" exactly when a person means it to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueryFilterOp {
    Equals,
    NotEquals,
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
    IsBlank,
    IsNotBlank,
}

/// The aggregate a `GroupBy` computes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueryAggregate {
    Count,
    Sum,
    Average,
    Min,
    Max,
}

/// One operation in a query's pipeline. Steps are applied **in order**, each to the whole table,
/// so the list reads like the history it is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum QueryStep {
    /// Take the first row as the column names (and remove it from the data).
    PromoteHeaders,
    /// Put the column names back into the data as a first row.
    DemoteHeaders,
    /// Change column types: every column when `columns` is empty.
    ChangeType { columns: Vec<String>, to: QueryColumnType },
    /// Drop these columns.
    RemoveColumns { columns: Vec<String> },
    /// Drop everything **but** these columns.
    KeepColumns { columns: Vec<String> },
    /// Rename one column.
    RenameColumn { from: String, to: String },
    /// Keep the rows a column matches (`value` is ignored by the blank tests).
    Filter { column: String, op: QueryFilterOp, value: Option<String> },
    /// Order the rows; the sort is stable, and later keys break earlier ties.
    Sort { keys: Vec<(String, bool)> },
    /// Remove duplicate rows, compared over `columns` only (all columns when empty).
    RemoveDuplicates { columns: Vec<String> },
    /// Replace every exact match of `from` in a column.
    ReplaceValues { column: String, from: String, to: String },
    /// Copy the last non-blank value down into the blanks of a column.
    FillDown { column: String },
    /// Turn column groups into rows: `attributes` are kept as columns, `values` become
    /// Attribute/Value pairs. Empty `values` unpivots every column not in `attributes`.
    Unpivot { attributes: Vec<String>, values: Vec<String> },
    /// Group rows by `by`, aggregating one column per entry of `aggregates`
    /// (`aggregate`, `column`, result name).
    GroupBy { by: Vec<String>, aggregates: Vec<(QueryAggregate, Option<String>, String)> },
}

impl QueryStep {
    /// The step's Power Query name, for the pane's step list and error messages.
    pub fn label(&self) -> &'static str {
        match self {
            QueryStep::PromoteHeaders => "Promoted Headers",
            QueryStep::DemoteHeaders => "Demoted Headers",
            QueryStep::ChangeType { .. } => "Changed Type",
            QueryStep::RemoveColumns { .. } => "Removed Columns",
            QueryStep::KeepColumns { .. } => "Kept Columns",
            QueryStep::RenameColumn { .. } => "Renamed Column",
            QueryStep::Filter { .. } => "Filtered Rows",
            QueryStep::Sort { .. } => "Sorted Rows",
            QueryStep::RemoveDuplicates { .. } => "Removed Duplicates",
            QueryStep::ReplaceValues { .. } => "Replaced Values",
            QueryStep::FillDown { .. } => "Filled Down",
            QueryStep::Unpivot { .. } => "Unpivoted Columns",
            QueryStep::GroupBy { .. } => "Grouped Rows",
        }
    }
}

/// Where a query's result goes when it runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueryLoad {
    /// Onto a sheet as a table. `sheet: None` means "the sheet this query already owns", and a
    /// query that has never run creates one named after the query.
    Sheet { sheet: Option<String> },
    /// Into the workbook's data model only (no cells). Relationships and measures live there;
    /// the engine lands in N3.3 and refuses this load until then, with the reason.
    Model,
}

/// A saved query.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Query {
    pub id: u32,
    /// Unique across the workbook, and the name of the table a sheet load creates.
    pub name: String,
    pub source: QuerySource,
    pub steps: Vec<QueryStep>,
    pub load: QueryLoad,
    /// The column names the last run produced: what the pane shows before a refresh, and what
    /// the editor's step list offers. Empty until the query has run.
    #[serde(default)]
    pub columns: Vec<String>,
}

impl Query {
    /// A fresh query over `source`, numbered past everything the workbook already has.
    pub fn new(existing: &[Query], name: String, source: QuerySource, load: QueryLoad) -> Self {
        let id = existing.iter().map(|q| q.id).max().map_or(1, |m| m.saturating_add(1));
        Self { id, name, source, steps: Vec::new(), load, columns: Vec::new() }
    }

    /// The sheet this query loads onto, if it has ever been given one.
    pub fn sheet(&self) -> Option<&str> {
        match &self.load {
            QueryLoad::Sheet { sheet } => sheet.as_deref(),
            QueryLoad::Model => None,
        }
    }
}

/// Whether `name` is a usable query name: not blank, not longer than [`MAX_NAME`], and not one
/// the workbook already uses (compared case-insensitively, the way Excel compares them).
pub fn query_name_is_free(queries: &[Query], name: &str, except_id: Option<u32>) -> bool {
    let n = name.trim();
    !n.is_empty() && n.chars().count() <= MAX_NAME && !queries.iter().any(|q| Some(q.id) != except_id && q.name.eq_ignore_ascii_case(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn csv() -> QuerySource {
        QuerySource::Csv { path: Some("/tmp/a.csv".into()), text: None, delimiter: ',' }
    }

    #[test]
    fn a_query_round_trips_through_json() {
        let q = Query {
            id: 3,
            name: "Sales".into(),
            source: csv(),
            steps: vec![
                QueryStep::PromoteHeaders,
                QueryStep::ChangeType { columns: vec!["Amount".into()], to: QueryColumnType::Number },
                QueryStep::Filter { column: "Region".into(), op: QueryFilterOp::Equals, value: Some("West".into()) },
                QueryStep::Sort { keys: vec![("Amount".into(), true)] },
                QueryStep::GroupBy { by: vec!["Region".into()], aggregates: vec![(QueryAggregate::Sum, Some("Amount".into()), "Total".into())] },
                QueryStep::Unpivot { attributes: vec!["Region".into()], values: Vec::new() },
            ],
            load: QueryLoad::Sheet { sheet: Some("Sales (2)".into()) },
            columns: vec!["Region".into(), "Total".into()],
        };
        let text = serde_json::to_string(&q).unwrap();
        let back: Query = serde_json::from_str(&text).unwrap();
        assert_eq!(q, back);
    }

    #[test]
    fn ids_do_not_reuse_a_number_after_a_deletion() {
        let a = Query::new(&[], "Query1".into(), csv(), QueryLoad::Model);
        let b = Query::new(std::slice::from_ref(&a), "Query2".into(), csv(), QueryLoad::Model);
        assert_eq!((a.id, b.id), (1, 2));
        let mut c = b.clone();
        c.id = 9;
        assert_eq!(Query::new(&[a, b, c], "Query3".into(), csv(), QueryLoad::Model).id, 10);
    }

    #[test]
    fn names_are_free_case_insensitively_and_not_when_blank() {
        let q = Query::new(&[], "Sales".into(), csv(), QueryLoad::Model);
        assert!(!query_name_is_free(std::slice::from_ref(&q), "sales", None));
        assert!(query_name_is_free(std::slice::from_ref(&q), "sales", Some(q.id)));
        assert!(!query_name_is_free(&[q], "   ", None));
    }

    #[test]
    fn a_step_names_itself_the_way_the_pane_shows_it() {
        assert_eq!(QueryStep::PromoteHeaders.label(), "Promoted Headers");
        assert_eq!(QueryStep::Filter { column: "a".into(), op: QueryFilterOp::IsBlank, value: None }.label(), "Filtered Rows");
    }
}
