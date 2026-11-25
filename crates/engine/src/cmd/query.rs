//! Get & Transform: saved queries, their steps, and loading a query's rows onto a sheet.
//!
//! A query is a recipe (`cella_model::Query`); the pipeline that runs it is `cella_query`;
//! this module is the only part that touches files, because it is the only part allowed to.
//! Loading writes a real table onto a sheet, so a query's result is ordinary data: it prints,
//! charts, feeds a pivot table, and the next refresh rewrites it in place.

use cella_core::{CellRef, RangeRef, Value};
use cella_model::{MAX_MODEL_CELLS, Query, QueryLoad, QuerySource, QueryStep, query_name_is_free};
use cella_query::{MAX_CELLS, MAX_COLUMNS, MAX_ROWS, Table};
use serde_json::json;

use super::*;

/// The sheet a source names, or the active one when the source left the sheet out.
fn find_sheet<'a>(wb: &'a cella_model::Workbook, name: &str) -> Option<&'a cella_model::Sheet> {
    wb.sheet_index(name).and_then(|i| wb.sheet(i))
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "data.query.add",
            "Add Query",
            ["Data", "Get & Transform Data"],
            None,
            "{name?, source: {kind: csv|json|xlsx|parquet|folder|table|range, ...}, steps?, load?: sheet|model} -> {name, sheet, rows, cols}",
            has_doc,
            add
        ),
        cmd!(
            "data.query.refresh",
            "Refresh Query",
            ["Data", "Queries & Connections"],
            None,
            "{name?} (every query when absent) -> [{name, rows, cols, sheet}]",
            has_doc,
            refresh
        ),
        cmd!(
            "data.query.addStep",
            "Apply Step",
            ["Data", "Queries & Connections"],
            None,
            "{name, step} (a QueryStep as data.query.steps shows it) -> runs the pipeline, keeps the step only if it succeeds",
            has_doc,
            add_step
        ),
        cmd!(
            "data.query.setStep",
            "Edit Step",
            ["Data", "Queries & Connections"],
            None,
            "{name, index, step} -> replaces step index",
            has_doc,
            set_step
        ),
        cmd!("data.query.removeStep", "Remove Step", ["Data", "Queries & Connections"], None, "{name, index}", has_doc, remove_step),
        cmd!("data.query.setSource", "Change Source", ["Data", "Queries & Connections"], None, "{name, source}", has_doc, set_source),
        cmd!("data.query.rename", "Rename Query", ["Data", "Queries & Connections"], None, "{name, to}", has_doc, rename),
        cmd!(
            "data.query.delete",
            "Delete Query",
            ["Data", "Queries & Connections"],
            None,
            "{name} (the sheet and table it loaded stay: they are data now)",
            has_doc,
            delete
        ),
        cmd!(
            "data.query.load",
            "Load To",
            ["Data", "Queries & Connections"],
            None,
            "{name, sheet?} -> loads the query onto a sheet as a table",
            has_doc,
            load
        ),
        cmd!(
            query "data.query.preview",
            "Preview Query",
            ["Data", "Queries & Connections"],
            None,
            "{name, limit?: 100, step?, at?} -> {columns, rows, total, truncated, failed: {index, label, error}?}",
            has_doc,
            preview
        ),
        cmd!(
            "data.fromDatabase",
            "From Database",
            ["Data", "Get & Transform Data"],
            None,
            "{path, sql, name?} -> {name, sheet, rows, cols} - a SQLite file as a query (read-only, one statement)",
            has_doc,
            from_database
        ),
        cmd!(
            "data.fromWeb",
            "From Web",
            ["Data", "Get & Transform Data"],
            None,
            "{url, format?: csv|json, name?} -> {name, sheet, rows, cols} - a URL as a query",
            has_doc,
            from_web
        ),
        cmd!(query "data.query.list", "Queries", ["Data", "Queries & Connections"], None, "{} -> [{id, name, kind, path, sheet, columns, steps}]", has_doc, list),
        cmd!(query "data.query.steps", "Query Steps", ["Data", "Queries & Connections"], None, "{name} -> [QueryStep]", has_doc, steps_of),
    ]
}

// ---------------------------------------------------------------- param parsing

fn query_index(wb: &cella_model::Workbook, name: &str) -> Result<usize> {
    wb.queries.iter().position(|q| q.name == name).ok_or_else(|| EngineError::Other(format!("There is no query named \"{name}\".")))
}

/// A source from its JSON form: `{"kind": "csv", "path": "..."}`.
fn parse_source(v: &Json) -> Result<QuerySource> {
    let kind = v.get("kind").and_then(Json::as_str).ok_or_else(|| bad("data.query", "source.kind is missing"))?;
    let path = v.get("path").and_then(Json::as_str).map(str::to_string);
    let text = v.get("text").and_then(Json::as_str).map(str::to_string);
    let delimiter = v.get("delimiter").and_then(Json::as_str).and_then(|d| d.chars().next());
    match kind.to_ascii_lowercase().as_str() {
        "csv" | "text" => {
            if path.is_none() && text.is_none() {
                return Err(bad("data.query", "a csv source needs a path or text"));
            }
            let d = delimiter.unwrap_or(if kind.eq_ignore_ascii_case("text") { '\t' } else { ',' });
            Ok(QuerySource::Csv { path, text, delimiter: d })
        }
        "json" => {
            if path.is_none() && text.is_none() {
                return Err(bad("data.query", "a json source needs a path or text"));
            }
            Ok(QuerySource::Json { path, text })
        }
        "xlsx" | "workbook" => {
            let path = path.ok_or_else(|| bad("data.query", "an xlsx source needs a path"))?;
            Ok(QuerySource::Xlsx { path, sheet: v.get("sheet").and_then(Json::as_str).map(str::to_string) })
        }
        "parquet" => Ok(QuerySource::Parquet { path: path.ok_or_else(|| bad("data.query", "a parquet source needs a path"))? }),
        "sqlite" | "db" | "database" => {
            let path = path.ok_or_else(|| bad("data.query", "a sqlite source needs a path"))?;
            let sql = v
                .get("sql")
                .or_else(|| v.get("query"))
                .and_then(Json::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| bad("data.query", "a sqlite source needs a sql statement (e.g. SELECT * FROM orders)"))?;
            Ok(QuerySource::Sqlite { path, sql: sql.to_string() })
        }
        "http" | "web" | "url" => {
            let url = v
                .get("url")
                .or_else(|| v.get("path"))
                .and_then(Json::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| bad("data.query", "an http source needs a url"))?
                .to_string();
            let format = match v.get("format").and_then(Json::as_str) {
                None => cella_model::HttpFormat::from_url(&url),
                Some(f) => match f.to_ascii_lowercase().as_str() {
                    "csv" | "text" => cella_model::HttpFormat::Csv,
                    "json" => cella_model::HttpFormat::Json,
                    other => return Err(bad("data.query", format!("\"{other}\" is not a web format (csv, json)"))),
                },
            };
            Ok(QuerySource::Http { url, format })
        }
        "folder" => Ok(QuerySource::Folder {
            path: path.ok_or_else(|| bad("data.query", "a folder source needs a path"))?,
            filter: v.get("filter").and_then(Json::as_str).unwrap_or("*.csv").to_string(),
        }),
        "table" => Ok(QuerySource::Table {
            sheet: v.get("sheet").and_then(Json::as_str).unwrap_or("").to_string(),
            name: v
                .get("table")
                .or_else(|| v.get("name"))
                .and_then(Json::as_str)
                .ok_or_else(|| bad("data.query", "a table source needs a table name"))?
                .to_string(),
        }),
        "query" => Ok(QuerySource::Query { name: v.get("name").and_then(Json::as_str).unwrap_or("").trim().to_string() }),
        "range" => {
            let sheet = v.get("sheet").and_then(Json::as_str).unwrap_or("").to_string();
            let range = v
                .get("range")
                .and_then(Json::as_str)
                .and_then(RangeRef::parse)
                .ok_or_else(|| bad("data.query", "a range source needs a range like A1:C9"))?;
            Ok(QuerySource::Range { sheet, range })
        }
        other => Err(bad("data.query", format!("\"{other}\" is not a source kind (csv, json, xlsx, parquet, sqlite, http, folder, table, range)"))),
    }
}

fn parse_steps(v: Option<&Json>) -> Result<Vec<QueryStep>> {
    let Some(v) = v else { return Ok(Vec::new()) };
    if v.is_null() {
        return Ok(Vec::new());
    }
    let steps: Vec<QueryStep> = serde_json::from_value(v.clone()).map_err(|e| bad("data.query", format!("steps is not a list of QueryStep: {e}")))?;
    if steps.len() > cella_model::MAX_STEPS {
        return Err(bad("data.query", format!("a query holds at most {} steps", cella_model::MAX_STEPS)));
    }
    Ok(steps)
}

fn parse_load(v: Option<&Json>) -> Result<QueryLoad> {
    match v.and_then(Json::as_str).unwrap_or("sheet").to_ascii_lowercase().as_str() {
        "sheet" | "worksheet" => Ok(QueryLoad::Sheet { sheet: None }),
        "model" | "datamodel" => Ok(QueryLoad::Model),
        other => Err(bad("data.query", format!("\"{other}\" is not a load destination (sheet, model)"))),
    }
}

// ---------------------------------------------------------------- reading a source into a table

/// Reads a source into rows. This is the only I/O in the query path: the pipeline itself never
/// sees a file.
fn source_table(s: &mut Session, src: &QuerySource) -> Result<Table> {
    match src {
        QuerySource::Csv { path, text, delimiter } => {
            let bytes = read_source(path.as_deref(), text.as_deref(), "csv")?;
            let opts = cella_xlsx::CsvOptions { delimiter: *delimiter as u8, parse_values: false, ..Default::default() };
            let wb = cella_xlsx::read_csv(&bytes, &opts).map_err(|e| EngineError::Other(e.to_string()))?;
            let sh = wb.sheet(0).ok_or(EngineError::NoDocument)?;
            grid_from_sheet(sh, None)
        }
        QuerySource::Json { path, text } => {
            let bytes = read_source(path.as_deref(), text.as_deref(), "json")?;
            let text = String::from_utf8_lossy(&bytes);
            let rows = json_rows(&text)?;
            Table::from_rows(rows, false).map_err(pipeline_error)
        }
        QuerySource::Xlsx { path, sheet } => {
            let bytes = read_source(Some(path), None, "xlsx")?;
            let (wb, _) = cella_xlsx::read_xlsx(&bytes).map_err(|e| EngineError::Other(e.to_string()))?;
            let sh = match sheet {
                Some(name) => find_sheet(&wb, name).ok_or_else(|| EngineError::Other(format!("\"{path}\" has no sheet named \"{name}\".")))?,
                None => wb.sheet(0).ok_or(EngineError::NoDocument)?,
            };
            grid_from_sheet(sh, None)
        }
        QuerySource::Parquet { path } => {
            let bytes = read_source(Some(path), None, "parquet")?;
            let t = cella_parquet::read_table(&bytes).map_err(|e| EngineError::Other(e.to_string()))?;
            let names: Vec<String> = t.columns.iter().map(|(n, _)| n.clone()).collect();
            let mut rows = t.rows;
            shift_parquet_dates(&t.columns, &mut rows);
            Table::new(names, rows).map_err(pipeline_error)
        }
        QuerySource::Folder { path, filter } => folder_table(path, filter),
        QuerySource::Sqlite { path, sql } => crate::connect::sqlite_table(path, sql),
        QuerySource::Http { url, format } => {
            let bytes = crate::connect::http_bytes(url)?;
            if crate::connect::looks_like_html(&bytes) {
                return Err(EngineError::Other(format!(
                    "{url} answered with a web page, not data. A link that needs a login usually does that; use the file instead."
                )));
            }
            match format {
                cella_model::HttpFormat::Csv => {
                    let opts = cella_xlsx::CsvOptions { delimiter: b',', parse_values: false, ..Default::default() };
                    let wb = cella_xlsx::read_csv(&bytes, &opts).map_err(|e| EngineError::Other(format!("{url}: {e}")))?;
                    let sh = wb.sheet(0).ok_or(EngineError::NoDocument)?;
                    grid_from_sheet(sh, None)
                }
                cella_model::HttpFormat::Json => {
                    let text = String::from_utf8_lossy(&bytes);
                    let rows = json_rows(&text)?;
                    Table::from_rows(rows, false).map_err(pipeline_error)
                }
            }
        }
        QuerySource::Query { name } => query_table(s, name, 0, &mut Vec::new()),
        QuerySource::Table { sheet, name } => {
            let d = s.doc()?;
            let sh = pick_sheet(&d.wb, sheet)?;
            let table = sh.tables.iter().find(|t| t.name.eq_ignore_ascii_case(name)).ok_or_else(|| {
                EngineError::Other(format!(
                    "The workbook has no table named \"{name}\"{}.",
                    if sheet.is_empty() { String::new() } else { format!(" on sheet \"{sheet}\"") }
                ))
            })?;
            let names: Vec<String> = table.columns.iter().map(|c| c.name.clone()).collect();
            let range = table.data_range().unwrap_or(table.range);
            let rows = grid_rows(sh, range)?;
            Table::new(names, rows).map_err(pipeline_error)
        }
        QuerySource::Range { sheet, range } => {
            let d = s.doc()?;
            let sh = pick_sheet(&d.wb, sheet)?;
            let rows = grid_rows(sh, *range)?;
            Table::from_rows(rows, false).map_err(pipeline_error)
        }
    }
}

/// A Parquet `DATE` is a day count from 1970-01-01; a sheet's date is a serial from 1899-12-30.
/// Read unconverted, every date in an imported file would be 25,569 days off, so the import shifts
/// the date columns once, here, where the file's own column types are still known.
fn shift_parquet_dates(columns: &[(String, cella_parquet::ColumnType)], rows: &mut [Vec<Value>]) {
    use cella_parquet::ColumnType;
    let offset = cella_core::date::serial_from_ymd(cella_core::DateSystem::D1900, 1970, 1, 1).unwrap_or(0.0);
    if offset == 0.0 || rows.is_empty() {
        return;
    }
    for (c, (_, ty)) in columns.iter().enumerate() {
        if *ty != ColumnType::Date {
            continue;
        }
        for row in rows.iter_mut() {
            if let Some(Value::Number(n)) = row.get_mut(c) {
                *n += offset;
            }
        }
    }
}

fn pick_sheet<'a>(wb: &'a cella_model::Workbook, name: &str) -> Result<&'a cella_model::Sheet> {
    match find_sheet(wb, name) {
        Some(sh) => Ok(sh),
        None if name.is_empty() => wb.active().ok_or(EngineError::NoDocument),
        None => Err(EngineError::Other(format!("There is no sheet named \"{name}\"."))),
    }
}

/// Another query's result: its source, then its steps, run without loading anything. This is what
/// makes a pipeline readable — one query cleans, the next reports from the clean table — and what
/// makes a cycle possible, so the walk is bounded and remembers the names it has been through.
fn query_table(s: &mut Session, name: &str, depth: u32, seen: &mut Vec<String>) -> Result<Table> {
    if name.trim().is_empty() {
        return Err(bad("data.query", "a query source needs a name"));
    }
    if depth > 16 {
        return Err(EngineError::Other(format!("The queries nest deeper than 16 (at \"{name}\").")));
    }
    if seen.iter().any(|n| n.eq_ignore_ascii_case(name)) {
        return Err(EngineError::Other(format!("The query \"{name}\" is used by itself, directly or through another query.")));
    }
    let (source, steps) = {
        let d = s.doc()?;
        let q =
            d.wb.queries
                .iter()
                .find(|q| q.name.eq_ignore_ascii_case(name))
                .ok_or_else(|| EngineError::Other(format!("There is no query named \"{name}\".")))?;
        (q.source.clone(), q.steps.clone())
    };
    seen.push(name.to_string());
    let table = match &source {
        QuerySource::Query { name: inner } => query_table(s, inner, depth + 1, seen)?,
        other => source_table(s, other)?,
    };
    seen.pop();
    cella_query::apply(table, &steps).map_err(pipeline_error)
}

/// Every file in a folder, stacked: the files that match the filter, read the way a single csv
/// source is, with a leading Source.Name column naming the file each row came from.
///
/// Like every other source the result carries **one header row and then data**: the first file
/// supplies the names, each file's own header row is dropped (they must agree — stacking two
/// shapes would silently mis-align a column), and a PromoteHeaders step turns the names into the
/// table's columns exactly as it does for one csv.
fn folder_table(path: &str, filter: &str) -> Result<Table> {
    let files = crate::io::list_files(path, filter)?;
    if files.is_empty() {
        return Err(EngineError::Other(format!("No file in {path} matches {filter}.")));
    }
    let mut header: Option<Vec<String>> = None;
    let mut rows: Vec<Vec<Value>> = Vec::new();
    for file in &files {
        let bytes = crate::io::read_file(file)?;
        let opts = cella_xlsx::CsvOptions { delimiter: 0, parse_values: false, ..Default::default() };
        let wb = cella_xlsx::read_csv(&bytes, &opts).map_err(|e| EngineError::Other(format!("{file}: {e}")))?;
        let sh = wb.sheet(0).ok_or(EngineError::NoDocument)?;
        let grid = grid_from_sheet(sh, None).map_err(|e| EngineError::Other(format!("{file}: {e}")))?;
        let mut lines = grid.rows.into_iter();
        let names: Vec<String> = lines.next().unwrap_or_default().iter().map(|v| v.display()).collect();
        match &header {
            None => {
                let mut names = names.clone();
                names.insert(0, "Source.Name".to_string());
                rows.push(names.iter().map(|n| Value::text(n.clone())).collect());
                header = Some(names);
            }
            Some(want) => {
                let want_width = want.len().saturating_sub(1);
                if names.len() != want_width {
                    return Err(EngineError::Other(format!("{file} has {} columns where the first file had {want_width}.", names.len())));
                }
            }
        }
        let name = std::path::Path::new(file).file_name().and_then(|n| n.to_str()).unwrap_or(file).to_string();
        for line in lines {
            let mut out = Vec::with_capacity(line.len() + 1);
            out.push(Value::text(name.clone()));
            out.extend(line);
            rows.push(out);
        }
        if rows.len() > MAX_ROWS {
            return Err(EngineError::Other(format!("The folder holds more than {MAX_ROWS} rows; narrow the filter or the folder.")));
        }
    }
    Table::from_rows(rows, false).map_err(pipeline_error)
}

fn read_source(path: Option<&str>, text: Option<&str>, what: &str) -> Result<Vec<u8>> {
    if let Some(t) = text {
        return Ok(t.as_bytes().to_vec());
    }
    let Some(path) = path else {
        // Programmatic calls never open a dialog: say what is missing instead.
        return Err(bad("data.query", format!("the {what} source needs a path or text")));
    };
    crate::io::read_file(path)
}

/// A JSON file as rows: an array of objects (columns are their keys, in the order the JSON
/// parser returns them, which is alphabetical) or an array of arrays. Anything else is refused,
/// naming what the file actually holds.
fn json_rows(text: &str) -> Result<Vec<Vec<Value>>> {
    let v: Json = serde_json::from_str(text).map_err(|e| bad("data.query", format!("the JSON is not valid: {e}")))?;
    let arr = v.as_array().ok_or_else(|| bad("data.query", "the JSON must be an array of objects or of arrays"))?;
    if arr.first().is_some_and(Json::is_object) {
        let mut keys: Vec<String> = Vec::new();
        for (i, o) in arr.iter().enumerate() {
            let Some(m) = o.as_object() else {
                return Err(bad("data.query", format!("item {} of the JSON is not an object", i + 1)));
            };
            for k in m.keys() {
                if !keys.iter().any(|e| e == k) {
                    keys.push(k.clone());
                }
            }
        }
        let mut rows: Vec<Vec<Value>> = Vec::with_capacity(arr.len().saturating_add(1));
        rows.push(keys.iter().map(|k| Value::text(k.clone())).collect());
        for o in arr {
            let m = o.as_object().ok_or_else(|| bad("data.query", "the JSON mixes objects with other values"))?;
            rows.push(keys.iter().map(|k| json_value(m.get(k))).collect());
        }
        return Ok(rows);
    }
    let mut rows: Vec<Vec<Value>> = Vec::with_capacity(arr.len());
    for (i, row) in arr.iter().enumerate() {
        let Some(cells) = row.as_array() else {
            return Err(bad("data.query", format!("row {} of the JSON is not an array", i + 1)));
        };
        rows.push(cells.iter().map(|c| json_value(Some(c))).collect());
    }
    Ok(rows)
}

fn json_value(v: Option<&Json>) -> Value {
    match v {
        None | Some(Json::Null) => Value::Empty,
        Some(Json::Bool(b)) => Value::Bool(*b),
        Some(Json::Number(n)) => n.as_f64().map_or(Value::Empty, Value::number),
        Some(Json::String(s)) => Value::text(s.clone()),
        Some(other) => Value::text(other.to_string()),
    }
}

/// The sheet's used range (or the range given), bounded before anything is allocated.
fn grid_from_sheet(sh: &cella_model::Sheet, range: Option<RangeRef>) -> Result<Table> {
    let r = match range {
        Some(r) => r,
        None => sh.used_range().ok_or_else(|| EngineError::Other("The source is empty: there are no rows to import.".into()))?,
    };
    let rows = grid_rows(sh, r)?;
    Table::from_rows(rows, false).map_err(pipeline_error)
}

fn grid_rows(sh: &cella_model::Sheet, r: RangeRef) -> Result<Vec<Vec<Value>>> {
    let (rows, cols) = (r.height() as usize, r.width() as usize);
    if rows > MAX_ROWS || cols > MAX_COLUMNS || rows.saturating_mul(cols) > MAX_CELLS {
        return Err(EngineError::Other(format!(
            "The source is too large for one table: {rows} rows and {cols} columns (the limit is {MAX_CELLS} cells)."
        )));
    }
    let mut out = Vec::with_capacity(rows);
    for row in r.start.row..=r.end.row {
        let mut line = Vec::with_capacity(cols);
        for col in r.start.col..=r.end.col {
            line.push(sh.value(CellRef::new(row, col)));
        }
        out.push(line);
    }
    Ok(out)
}

fn pipeline_error(e: cella_query::QueryError) -> EngineError {
    EngineError::Other(e.to_string())
}

// ---------------------------------------------------------------- loading a table onto a sheet

/// Keeps a query's result in the workbook's data model instead of on a sheet.
///
/// The rows are the cache a reopened workbook shows before anything is refreshed, which is why
/// they are written here and not recomputed on open: a model pivot has to draw before the source
/// files are back.
fn load_into_model(s: &mut Session, name: &str, table: &Table) -> Result<()> {
    let rows = table.height();
    let cols = table.width();
    edit(s, |cx| {
        let existing: usize = cx
            .wb
            .model
            .tables
            .iter()
            .filter(|t| !t.name.eq_ignore_ascii_case(name))
            .fold(0, |n: usize, t| n.saturating_add(t.width().saturating_mul(t.height())));
        let cells = rows.saturating_mul(cols);
        if cells > MAX_MODEL_CELLS || existing.saturating_add(cells) > MAX_MODEL_CELLS {
            return Err(EngineError::Other(format!(
                "The data model would hold more than {MAX_MODEL_CELLS} cells (this query adds {cells}). Load it to a sheet instead."
            )));
        }
        if cx.wb.model.tables.len() >= cella_model::MAX_MODEL_TABLES && !cx.wb.model.tables.iter().any(|t| t.name.eq_ignore_ascii_case(name)) {
            return Err(EngineError::Other(format!("The data model holds at most {} tables.", cella_model::MAX_MODEL_TABLES)));
        }
        let t = cella_model::ModelTable {
            name: name.to_string(),
            columns: table.columns.clone(),
            rows: std::sync::Arc::new(table.rows.clone()),
            // A query types its own columns from the values it produced; the file field is empty.
            column_types: Vec::new(),
            source_query: Some(name.to_string()),
            // A query's result is cached here: it may come from a URL, a worksheet or a dozen
            // steps, and only a file (which the engine reads itself) is ever file-backed.
            source_file: None,
        };
        match cx.wb.model.table_index(name) {
            Some(i) => cx.wb.model.tables[i] = t,
            None => cx.wb.model.tables.push(t),
        }
        // A measure in a cell reads the model, not a range, so the cell graph cannot see that the
        // rows behind it changed: this edit recalculates the workbook.
        cx.structural = true;
        Ok(())
    })?;
    // The engine's copy of the table is stale from here.
    s.model_changed();
    Ok(())
}

/// Writes `table` onto a sheet as a table object named after the query, replacing whatever the
/// last run wrote. Returns the sheet's name.
///
/// A refresh cannot simply rewrite the cells: the table object's own range, column names and
/// header cells have to change with the shape, and a table that kept its old width would rename
/// the cells that are left behind. So the old table is turned back into a range (keeping its
/// style), the old contents are cleared, and a new table of the same name and style is put on the
/// new rows.
fn load_table(s: &mut Session, name: &str, sheet: Option<&str>, table: &Table) -> Result<String> {
    if table.width() == 0 || table.height() == 0 {
        return Err(EngineError::Other(format!("The query \"{name}\" produced no data to load.")));
    }
    let target = match sheet {
        Some(n) if !n.is_empty() => {
            if find_sheet(&s.doc()?.wb, n).is_none() {
                return Err(EngineError::Other(format!("There is no sheet named \"{n}\".")));
            }
            n.to_string()
        }
        // A query owns one sheet: reuse the one it made before, otherwise make one named after it.
        _ => {
            let existing = s.doc()?.wb.queries.iter().find(|q| q.name == name).and_then(|q| q.sheet()).map(str::to_string);
            match existing {
                Some(n) if find_sheet(&s.doc()?.wb, &n).is_some() => n,
                _ => fresh_sheet(s, name)?,
            }
        }
    };
    let own = table_name(name);
    let old = {
        let d = s.doc()?;
        find_sheet(&d.wb, &target).and_then(|sh| sh.tables.iter().find(|t| t.name == own).map(|t| (t.range, t.style.clone())))
    };
    let style = match &old {
        Some((range, style)) => {
            s.execute("table.convertToRange", json!({"sheet": target, "table": own}))?;
            s.execute("edit.clearContents", json!({"sheet": target, "range": range.a1()}))?;
            style.clone()
        }
        None => "TableStyleMedium2".to_string(),
    };
    let values = table_values(table);
    let (rows, cols) = (values.len(), table.width());
    let range = RangeRef::new(CellRef::new(0, 0), CellRef::new(rows as u32 - 1, cols as u32 - 1));
    // `raw` writes every text cell as text: a query's column is text until a step says otherwise,
    // and re-typing it here would turn "2024-03-04" into a serial number behind the pipeline's
    // back.
    s.execute("range.setValues", json!({"sheet": target, "range": "A1", "values": values, "raw": true}))?;
    s.execute("insert.table", json!({"sheet": target, "range": range.a1(), "header": true, "name": own, "style": style}))?;
    let _ = s.execute("home.autofitColumnWidth", json!({"sheet": target, "cols": RangeRef::cols(0, cols as u32 - 1).a1()}));
    Ok(target)
}

/// The name of the table object a query owns. A table name cannot hold spaces and must not look
/// like a cell reference, so the query's name is squeezed into one.
fn table_name(name: &str) -> String {
    let mut out: String = name.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
    if out.is_empty() || out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out = format!("Query_{out}");
    }
    out.chars().take(100).collect()
}

fn fresh_sheet(s: &mut Session, name: &str) -> Result<String> {
    let stem: String = name.chars().take(27).collect();
    let mut candidate = name.chars().take(31).collect::<String>();
    let mut n = 1;
    {
        let d = s.doc()?;
        while find_sheet(&d.wb, &candidate).is_some() {
            n += 1;
            candidate = format!("{stem} ({n})");
        }
    }
    let r = s.execute("home.insertSheet", json!({"name": candidate}))?;
    Ok(r.get("name").and_then(Json::as_str).unwrap_or(&candidate).to_string())
}

fn table_values(table: &Table) -> Vec<Json> {
    let mut out: Vec<Json> = Vec::with_capacity(table.rows.len() + 1);
    out.push(Json::Array(table.columns.iter().map(|c| json!(c)).collect()));
    for row in &table.rows {
        out.push(Json::Array(row.iter().map(value_json).collect()));
    }
    out
}

fn value_json(v: &Value) -> Json {
    match v {
        Value::Empty => Json::Null,
        Value::Number(n) => Json::from(*n),
        Value::Bool(b) => Json::from(*b),
        // An error cell is written as its text ("#N/A"): that is what Excel shows, and a table
        // cell cannot hold an error while the query that made it is not doing the writing.
        other => json!(other.display()),
    }
}

// ---------------------------------------------------------------- running a query

struct Ran {
    table: Table,
    sheet: Option<String>,
    /// The rows a file-backed model load took, when the query loaded a file instead of running
    /// its steps: the table a query then holds is the model's definition, with no rows here.
    file_rows: Option<usize>,
}

impl Ran {
    fn rows(&self) -> usize {
        self.file_rows.unwrap_or_else(|| self.table.height())
    }
}

/// The file a query loads as it stands: a Parquet file, or a delimited one whose only step is
/// naming its header row — which is what the engine's own reader does. Anything else is a
/// pipeline and has to run (`data.query.addStep` and the editor keep it in memory).
fn file_model(source: &QuerySource, steps: &[QueryStep]) -> Option<cella_model::ModelFile> {
    if steps.len() > 1 || !steps.iter().all(|s| matches!(s, QueryStep::PromoteHeaders)) {
        return None;
    }
    match source {
        QuerySource::Parquet { path } => Some(cella_model::ModelFile { paths: vec![path.clone()], format: cella_model::ModelFileFormat::Parquet }),
        QuerySource::Csv { path: Some(path), text: None, .. } => {
            Some(cella_model::ModelFile { paths: vec![path.clone()], format: cella_model::ModelFileFormat::Csv })
        }
        _ => None,
    }
}
/// Runs one query: source, steps, then the load. The query's stored `columns` learn the new header
/// names, which is what the pane shows before a refresh.
fn run_query(s: &mut Session, name: &str) -> Result<Ran> {
    let (source, steps, load) = {
        let d = s.doc()?;
        let q = d.wb.queries.iter().find(|q| q.name == name).ok_or_else(|| EngineError::Other(format!("There is no query named \"{name}\".")))?;
        (q.source.clone(), q.steps.clone(), q.load.clone())
    };
    // A file the engine can read as it stands goes into the model *as the file*: the document
    // keeps the path and the engine reads the rows, which is what lets the table be bigger than
    // the sheet (and than the rows the document caches). A pipeline, or a file the engine
    // refuses, runs in memory as it always did.
    if let QueryLoad::Model = load
        && let Some(file) = file_model(&source, &steps)
    {
        match super::model::load_model_file(s, name, &file.paths, file.format, true) {
            Ok((rows, cols)) => {
                let columns = {
                    let d = s.doc()?;
                    d.wb.model.table(name).map(|t| t.columns.clone()).unwrap_or_else(|| vec![String::new(); cols])
                };
                let nm = name.to_string();
                edit(s, |cx| {
                    if let Some(q) = cx.wb.queries.iter_mut().find(|q| q.name == nm) {
                        q.columns = columns.clone();
                    }
                    Ok(())
                })?;
                return Ok(Ran { table: Table { columns, rows: Vec::new() }, sheet: None, file_rows: Some(rows as usize) });
            }
            Err(e) => log::warn!("a query's file could not be read by the engine, so its steps run in memory: {e}"),
        }
    }
    let table = source_table(s, &source)?;
    let table = cella_query::apply(table, &steps).map_err(pipeline_error)?;
    let sheet = match &load {
        QueryLoad::Sheet { sheet } => Some(load_table(s, name, sheet.as_deref(), &table)?),
        QueryLoad::Model => {
            load_into_model(s, name, &table)?;
            None
        }
    };
    let columns = table.columns.clone();
    let sheet_for_query = sheet.clone();
    let nm = name.to_string();
    edit(s, |cx| {
        if let Some(q) = cx.wb.queries.iter_mut().find(|q| q.name == nm) {
            q.columns = columns;
            if let QueryLoad::Sheet { sheet } = &mut q.load {
                *sheet = sheet_for_query.clone();
            }
        }
        Ok(())
    })?;
    Ok(Ran { table, sheet, file_rows: None })
}

// ---------------------------------------------------------------- the commands

// ---------------------------------------------------------------- Get Data: web and database

/// A name for a query made from a URL or a file path: the last meaningful segment, made free by
/// numbering. "sales.csv" from a URL is a better name than "Query3", and the pane is where names
/// are read.
fn derived_name(s: &Session, from: &str, fallback: &str) -> String {
    let tail = from.split(['?', '#']).next().unwrap_or(from).trim_end_matches('/');
    let stem = tail.rsplit(['/', '\\']).next().unwrap_or("");
    let base = stem.rsplit_once('.').map_or(stem, |(b, _)| b);
    let base = base.trim();
    let base = if base.is_empty() { fallback } else { base };
    let queries = s.doc().map(|d| d.wb.queries.clone()).unwrap_or_default();
    if cella_model::query_name_is_free(&queries, base, None) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base} ({n})");
        if cella_model::query_name_is_free(&queries, &candidate, None) {
            return candidate;
        }
        n += 1;
    }
}

/// data.fromWeb: a query whose source is a URL, loaded onto its own sheet. The dialog is only for
/// people; an agent passes the URL and gets the same query a step pipeline can build on.
fn from_web(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(url) = str_param(p, "url").map(str::to_string) else {
        s.ui_requests.push(crate::UiRequest::Dialog("fromWeb".into(), json!({})));
        return ok();
    };
    let mut source = json!({"kind": "http", "url": url});
    if let Some(f) = str_param(p, "format") {
        source["format"] = json!(f);
    }
    let name = str_param(p, "name").map(str::to_string).unwrap_or_else(|| derived_name(s, &url, "Web Query"));
    s.execute("data.query.add", json!({"name": name, "source": source, "steps": ["PromoteHeaders"], "load": "sheet"}))
}

/// data.fromDatabase: a statement against a SQLite file, as a query. The columns are the
/// statement's own, so no PromoteHeaders step: a SELECT names its columns.
fn from_database(s: &mut Session, p: &Json) -> Result<Json> {
    let (Some(path), Some(sql)) = (str_param(p, "path").map(str::to_string), str_param(p, "sql").map(str::to_string)) else {
        s.ui_requests.push(crate::UiRequest::Dialog("fromDatabase".into(), json!({})));
        return ok();
    };
    let name = str_param(p, "name").map(str::to_string).unwrap_or_else(|| derived_name(s, &path, "Database Query"));
    s.execute("data.query.add", json!({"name": name, "source": {"kind": "sqlite", "path": path, "sql": sql}, "load": "sheet"}))
}

fn add(s: &mut Session, p: &Json) -> Result<Json> {
    let source = parse_source(p.get("source").ok_or_else(|| bad("data.query.add", "missing source"))?)?;
    let steps = parse_steps(p.get("steps"))?;
    let load = parse_load(p.get("load"))?;
    let name = match str_param(p, "name") {
        Some(n) => {
            if !query_name_is_free(&s.doc()?.wb.queries, n, None) {
                return Err(EngineError::Other(format!("\"{n}\" is already the name of a query (or is blank).")));
            }
            n.trim().to_string()
        }
        None => {
            let d = s.doc()?;
            let mut n = 1;
            loop {
                let candidate = format!("Query{n}");
                if query_name_is_free(&d.wb.queries, &candidate, None) {
                    break candidate;
                }
                n += 1;
            }
        }
    };
    edit(s, |cx| {
        let id = cx.wb.queries.iter().map(|q| q.id).max().map_or(1, |m| m.saturating_add(1));
        cx.wb.queries.push(Query { id, name: name.clone(), source: source.clone(), steps: steps.clone(), load: load.clone(), columns: Vec::new() });
        Ok(())
    })?;
    let ran = run_query(s, &name)?;
    Ok(json!({"name": name, "sheet": ran.sheet, "rows": ran.rows(), "cols": ran.table.width()}))
}

fn refresh(s: &mut Session, p: &Json) -> Result<Json> {
    let names: Vec<String> = match str_param(p, "name") {
        Some(n) => {
            query_index(&s.doc()?.wb, n)?;
            vec![n.to_string()]
        }
        None => s.doc()?.wb.queries.iter().map(|q| q.name.clone()).collect(),
    };
    let mut out = Vec::with_capacity(names.len());
    for name in &names {
        let ran = run_query(s, name)?;
        out.push(json!({"name": name, "sheet": ran.sheet, "rows": ran.rows(), "cols": ran.table.width()}));
    }
    Ok(Json::Array(out))
}

/// Replaces a query's steps, then runs it: a step that does not fit is rolled back and its error
/// reported, so a pipeline can never be left in a state that does not run.
fn set_steps_and_run(s: &mut Session, name: &str, steps: Vec<QueryStep>, cmd: &'static str) -> Result<Ran> {
    let before = {
        let d = s.doc()?;
        let q = d.wb.queries.get(query_index(&d.wb, name)?).ok_or_else(|| bad(cmd, "gone"))?;
        q.steps.clone()
    };
    let nm = name.to_string();
    let next = steps.clone();
    edit(s, |cx| {
        if let Some(q) = cx.wb.queries.iter_mut().find(|q| q.name == nm) {
            q.steps = next;
        }
        Ok(())
    })?;
    match run_query(s, name) {
        Ok(ran) => Ok(ran),
        Err(e) => {
            let nm = name.to_string();
            edit(s, |cx| {
                if let Some(q) = cx.wb.queries.iter_mut().find(|q| q.name == nm) {
                    q.steps = before;
                }
                Ok(())
            })?;
            Err(e)
        }
    }
}

fn add_step(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("data.query.addStep", "missing name"))?.to_string();
    let step: QueryStep = serde_json::from_value(p.get("step").cloned().unwrap_or(Json::Null))
        .map_err(|e| bad("data.query.addStep", format!("step is not a QueryStep: {e}")))?;
    let (index, mut steps) = {
        let d = s.doc()?;
        let q = d.wb.queries.get(query_index(&d.wb, &name)?).ok_or_else(|| bad("data.query.addStep", "gone"))?;
        if q.steps.len() >= cella_model::MAX_STEPS {
            return Err(EngineError::Other(format!("\"{name}\" already holds the most steps a query can ({}).", cella_model::MAX_STEPS)));
        }
        (q.steps.len(), q.steps.clone())
    };
    steps.push(step);
    let ran = set_steps_and_run(s, &name, steps, "data.query.addStep")?;
    Ok(json!({"name": name, "index": index, "rows": ran.rows(), "cols": ran.table.width(), "sheet": ran.sheet}))
}

fn set_step(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("data.query.setStep", "missing name"))?.to_string();
    let index = u32_param(p, "index").ok_or_else(|| bad("data.query.setStep", "missing index"))? as usize;
    let step: QueryStep = serde_json::from_value(p.get("step").cloned().unwrap_or(Json::Null))
        .map_err(|e| bad("data.query.setStep", format!("step is not a QueryStep: {e}")))?;
    let steps = {
        let d = s.doc()?;
        let q = d.wb.queries.get(query_index(&d.wb, &name)?).ok_or_else(|| bad("data.query.setStep", "gone"))?;
        let mut steps = q.steps.clone();
        let slot = steps.get_mut(index).ok_or_else(|| bad("data.query.setStep", format!("\"{name}\" has no step {index}")))?;
        *slot = step;
        steps
    };
    let ran = set_steps_and_run(s, &name, steps, "data.query.setStep")?;
    Ok(json!({"name": name, "rows": ran.rows(), "cols": ran.table.width(), "sheet": ran.sheet}))
}

fn remove_step(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("data.query.removeStep", "missing name"))?.to_string();
    let index = u32_param(p, "index").ok_or_else(|| bad("data.query.removeStep", "missing index"))? as usize;
    let steps = {
        let d = s.doc()?;
        let q = d.wb.queries.get(query_index(&d.wb, &name)?).ok_or_else(|| bad("data.query.removeStep", "gone"))?;
        let mut steps = q.steps.clone();
        if index >= steps.len() {
            return Err(bad("data.query.removeStep", format!("\"{name}\" has no step {index}")));
        }
        steps.remove(index);
        steps
    };
    let ran = set_steps_and_run(s, &name, steps, "data.query.removeStep")?;
    Ok(json!({"name": name, "rows": ran.rows(), "cols": ran.table.width(), "sheet": ran.sheet}))
}

fn set_source(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("data.query.setSource", "missing name"))?.to_string();
    let source = parse_source(p.get("source").ok_or_else(|| bad("data.query.setSource", "missing source"))?)?;
    let before = {
        let d = s.doc()?;
        let q = d.wb.queries.get(query_index(&d.wb, &name)?).ok_or_else(|| bad("data.query.setSource", "gone"))?;
        q.source.clone()
    };
    let nm = name.clone();
    let next = source.clone();
    edit(s, |cx| {
        if let Some(q) = cx.wb.queries.iter_mut().find(|q| q.name == nm) {
            q.source = next;
        }
        Ok(())
    })?;
    // A source that does not read back (a file that moved, a query that would read itself) is put
    // back: a query whose recipe cannot run is a query that cannot be refreshed again either.
    match run_query(s, &name) {
        Ok(ran) => Ok(json!({"name": name, "rows": ran.rows(), "cols": ran.table.width(), "sheet": ran.sheet})),
        Err(e) => {
            let nm = name.clone();
            edit(s, |cx| {
                if let Some(q) = cx.wb.queries.iter_mut().find(|q| q.name == nm) {
                    q.source = before;
                }
                Ok(())
            })?;
            Err(e)
        }
    }
}

fn rename(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("data.query.rename", "missing name"))?.to_string();
    let to = str_param(p, "to").ok_or_else(|| bad("data.query.rename", "missing to"))?.trim().to_string();
    let id = {
        let d = s.doc()?;
        let q = d.wb.queries.get(query_index(&d.wb, &name)?).ok_or_else(|| bad("data.query.rename", "gone"))?;
        if !query_name_is_free(&d.wb.queries, &to, Some(q.id)) {
            return Err(EngineError::Other(format!("\"{to}\" is already the name of a query (or is blank).")));
        }
        q.id
    };
    let old_table = table_name(&name);
    let new_table = table_name(&to);
    let new_to = to.clone();
    edit(s, |cx| {
        if let Some(q) = cx.wb.queries.iter_mut().find(|q| q.id == id) {
            q.name = new_to;
        }
        // The table the query owns follows its name, so the next refresh finds it again.
        for i in 0..cx.wb.sheets.len() {
            if let Some(sh) = cx.wb.sheet_mut(i) {
                for t in &mut sh.tables {
                    if t.name == old_table {
                        t.name = new_table.clone();
                    }
                }
            }
        }
        Ok(())
    })?;
    Ok(json!({"name": to}))
}

fn delete(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("data.query.delete", "missing name"))?.to_string();
    let id = {
        let d = s.doc()?;
        d.wb.queries.get(query_index(&d.wb, &name)?).map(|q| q.id).ok_or_else(|| bad("data.query.delete", "gone"))?
    };
    edit(s, |cx| {
        cx.wb.queries.retain(|q| q.id != id);
        // A model table exists only because its query does: the query is the recipe that fills it.
        // A sheet table stays, because those cells are data a person can see and keep.
        let model_name = name.clone();
        cx.wb.model.tables.retain(|t| !(t.source_query.as_deref() == Some(model_name.as_str()) || t.name.eq_ignore_ascii_case(&model_name)));
        cx.wb.model.relationships.retain(|r| !r.from_table.eq_ignore_ascii_case(&name) && !r.to_table.eq_ignore_ascii_case(&name));
        Ok(())
    })?;
    s.model_changed();
    Ok(json!({"deleted": name}))
}

fn load(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("data.query.load", "missing name"))?.to_string();
    let sheet = str_param(p, "sheet").map(str::to_string);
    query_index(&s.doc()?.wb, &name)?;
    let nm = name.clone();
    let sh = sheet.clone();
    edit(s, |cx| {
        if let Some(q) = cx.wb.queries.iter_mut().find(|q| q.name == nm) {
            q.load = QueryLoad::Sheet { sheet: sh };
        }
        Ok(())
    })?;
    let ran = run_query(s, &name)?;
    Ok(json!({"name": name, "sheet": ran.sheet, "rows": ran.rows(), "cols": ran.table.width()}))
}

// ---------------------------------------------------------------- preview

/// What the editor shows: the rows as of the last step that ran, and where the pipeline stopped.
fn preview(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("data.query.preview", "missing name"))?;
    let limit = u32_param(p, "limit").unwrap_or(100).clamp(1, 1_000) as usize;
    let (source, steps) = {
        let d = s.doc()?;
        let q = d.wb.queries.get(query_index(&d.wb, name)?).ok_or_else(|| bad("data.query.preview", "gone"))?;
        (q.source.clone(), q.steps.clone())
    };
    let mut table = source_table(s, &source)?;
    let mut failed: Option<Json> = None;
    for (index, step) in steps.iter().enumerate() {
        match cella_query::apply_step(table.clone(), step, index) {
            Ok(next) => table = next,
            Err(e) => {
                failed = Some(json!({"index": index, "label": step.label(), "error": e.to_string()}));
                break;
            }
        }
    }
    // A step the editor is still composing is run against the data as it stands, and it decides
    // the failure: the caller is asking "what would this do?".
    if let Some(trial) = p.get("step").filter(|v| !v.is_null()) {
        let step: QueryStep =
            serde_json::from_value(trial.clone()).map_err(|e| bad("data.query.preview", format!("step is not a QueryStep: {e}")))?;
        let at = u32_param(p, "at").map_or(steps.len(), |i| i as usize);
        match cella_query::apply_step(table.clone(), &step, at) {
            Ok(next) => {
                table = next;
                failed = None;
            }
            Err(e) => failed = Some(json!({"index": at, "label": step.label(), "error": e.to_string()})),
        }
    }
    let total = table.height();
    let rows: Vec<Json> = table.rows.iter().take(limit).map(|r| Json::Array(r.iter().map(value_json).collect())).collect();
    Ok(json!({
        "columns": table.columns,
        "rows": rows,
        "total": total,
        "truncated": total > limit,
        "failed": failed,
    }))
}

fn list(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    Ok(Json::Array(
        d.wb.queries
            .iter()
            .map(|q| {
                json!({
                    "id": q.id,
                    "name": q.name,
                    "kind": q.source.kind(),
                    "path": q.source.path(),
                    "sheet": q.sheet(),
                    "columns": q.columns,
                    "steps": q.steps.iter().map(|s| s.label()).collect::<Vec<_>>(),
                })
            })
            .collect(),
    ))
}

fn steps_of(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("data.query.steps", "missing name"))?;
    let d = s.doc()?;
    let q = d.wb.queries.get(query_index(&d.wb, name)?).ok_or_else(|| bad("data.query.steps", "gone"))?;
    serde_json::to_value(&q.steps).map_err(|e| EngineError::Other(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cella_parquet::ColumnType;

    #[test]
    fn a_parquet_date_column_is_shifted_to_the_sheets_own_epoch() {
        let columns = vec![("day".to_string(), ColumnType::Date), ("n".to_string(), ColumnType::Int64)];
        // 1970-01-01 is day 0 in the file and serial 25,569 in a 1900-date workbook.
        let mut rows = vec![vec![Value::number(0.0), Value::number(7.0)], vec![Value::Empty, Value::Empty]];
        shift_parquet_dates(&columns, &mut rows);
        assert_eq!(rows[0][0], Value::number(25_569.0));
        // A column that is not a date is left exactly as the file has it.
        assert_eq!(rows[0][1], Value::number(7.0));
        // A null stays a null.
        assert_eq!(rows[1][0], Value::Empty);
    }
}
