//! Get & Transform: saved queries, their steps, and loading a query's rows onto a sheet.
//!
//! A query is a recipe (`gridcraft_model::Query`); the pipeline that runs it is `gridcraft_query`;
//! this module is the only part that touches files, because it is the only part allowed to.
//! Loading writes a real table onto a sheet, so a query's result is ordinary data: it prints,
//! charts, feeds a pivot table, and the next refresh rewrites it in place.

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::{MAX_MODEL_CELLS, Query, QueryLoad, QuerySource, QueryStep, query_name_is_free};
use gridcraft_query::{MAX_CELLS, MAX_COLUMNS, MAX_ROWS, Table};
use serde_json::json;

use super::*;

/// The sheet a source names, or the active one when the source left the sheet out.
fn find_sheet<'a>(wb: &'a gridcraft_model::Workbook, name: &str) -> Option<&'a gridcraft_model::Sheet> {
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
        cmd!(query "data.query.list", "Queries", ["Data", "Queries & Connections"], None, "{} -> [{id, name, kind, path, sheet, columns, steps}]", has_doc, list),
        cmd!(query "data.query.steps", "Query Steps", ["Data", "Queries & Connections"], None, "{name} -> [QueryStep]", has_doc, steps_of),
    ]
}

// ---------------------------------------------------------------- param parsing

fn query_index(wb: &gridcraft_model::Workbook, name: &str) -> Result<usize> {
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
        "range" => {
            let sheet = v.get("sheet").and_then(Json::as_str).unwrap_or("").to_string();
            let range = v
                .get("range")
                .and_then(Json::as_str)
                .and_then(RangeRef::parse)
                .ok_or_else(|| bad("data.query", "a range source needs a range like A1:C9"))?;
            Ok(QuerySource::Range { sheet, range })
        }
        other => Err(bad("data.query", format!("\"{other}\" is not a source kind (csv, json, xlsx, parquet, folder, table, range)"))),
    }
}

fn parse_steps(v: Option<&Json>) -> Result<Vec<QueryStep>> {
    let Some(v) = v else { return Ok(Vec::new()) };
    if v.is_null() {
        return Ok(Vec::new());
    }
    let steps: Vec<QueryStep> = serde_json::from_value(v.clone()).map_err(|e| bad("data.query", format!("steps is not a list of QueryStep: {e}")))?;
    if steps.len() > gridcraft_model::MAX_STEPS {
        return Err(bad("data.query", format!("a query holds at most {} steps", gridcraft_model::MAX_STEPS)));
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
            let opts = gridcraft_xlsx::CsvOptions { delimiter: *delimiter as u8, parse_values: false, ..Default::default() };
            let wb = gridcraft_xlsx::read_csv(&bytes, &opts).map_err(|e| EngineError::Other(e.to_string()))?;
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
            let (wb, _) = gridcraft_xlsx::read_xlsx(&bytes).map_err(|e| EngineError::Other(e.to_string()))?;
            let sh = match sheet {
                Some(name) => find_sheet(&wb, name).ok_or_else(|| EngineError::Other(format!("\"{path}\" has no sheet named \"{name}\".")))?,
                None => wb.sheet(0).ok_or(EngineError::NoDocument)?,
            };
            grid_from_sheet(sh, None)
        }
        QuerySource::Parquet { path } => {
            let bytes = read_source(Some(path), None, "parquet")?;
            let t = gridcraft_parquet::read_table(&bytes).map_err(|e| EngineError::Other(e.to_string()))?;
            let names: Vec<String> = t.columns.iter().map(|(n, _)| n.clone()).collect();
            let mut rows = t.rows;
            shift_parquet_dates(&t.columns, &mut rows);
            Table::new(names, rows).map_err(pipeline_error)
        }
        // The folder connector and the data model arrive with N3.4 and N3.3; refusing with the
        // round that brings them is more useful than a silent empty table.
        QuerySource::Folder { .. } => Err(EngineError::Other("A folder source arrives with N3.4. Import one file with a csv source for now.".into())),
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
fn shift_parquet_dates(columns: &[(String, gridcraft_parquet::ColumnType)], rows: &mut [Vec<Value>]) {
    use gridcraft_parquet::ColumnType;
    let offset = gridcraft_core::date::serial_from_ymd(gridcraft_core::DateSystem::D1900, 1970, 1, 1).unwrap_or(0.0);
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

fn pick_sheet<'a>(wb: &'a gridcraft_model::Workbook, name: &str) -> Result<&'a gridcraft_model::Sheet> {
    match find_sheet(wb, name) {
        Some(sh) => Ok(sh),
        None if name.is_empty() => wb.active().ok_or(EngineError::NoDocument),
        None => Err(EngineError::Other(format!("There is no sheet named \"{name}\"."))),
    }
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
fn grid_from_sheet(sh: &gridcraft_model::Sheet, range: Option<RangeRef>) -> Result<Table> {
    let r = match range {
        Some(r) => r,
        None => sh.used_range().ok_or_else(|| EngineError::Other("The source is empty: there are no rows to import.".into()))?,
    };
    let rows = grid_rows(sh, r)?;
    Table::from_rows(rows, false).map_err(pipeline_error)
}

fn grid_rows(sh: &gridcraft_model::Sheet, r: RangeRef) -> Result<Vec<Vec<Value>>> {
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

fn pipeline_error(e: gridcraft_query::QueryError) -> EngineError {
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
        if cx.wb.model.tables.len() >= gridcraft_model::MAX_MODEL_TABLES && !cx.wb.model.tables.iter().any(|t| t.name.eq_ignore_ascii_case(name)) {
            return Err(EngineError::Other(format!("The data model holds at most {} tables.", gridcraft_model::MAX_MODEL_TABLES)));
        }
        let t = gridcraft_model::ModelTable {
            name: name.to_string(),
            columns: table.columns.clone(),
            rows: table.rows.clone(),
            source_query: Some(name.to_string()),
        };
        match cx.wb.model.table_index(name) {
            Some(i) => cx.wb.model.tables[i] = t,
            None => cx.wb.model.tables.push(t),
        }
        // A measure in a cell reads the model, not a range, so the cell graph cannot see that the
        // rows behind it changed: this edit recalculates the workbook.
        cx.structural = true;
        Ok(())
    })
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
}

/// Runs one query: source, steps, then the load. The query's stored `columns` learn the new header
/// names, which is what the pane shows before a refresh.
fn run_query(s: &mut Session, name: &str) -> Result<Ran> {
    let (source, steps, load) = {
        let d = s.doc()?;
        let q = d.wb.queries.iter().find(|q| q.name == name).ok_or_else(|| EngineError::Other(format!("There is no query named \"{name}\".")))?;
        (q.source.clone(), q.steps.clone(), q.load.clone())
    };
    let table = source_table(s, &source)?;
    let table = gridcraft_query::apply(table, &steps).map_err(pipeline_error)?;
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
    Ok(Ran { table, sheet })
}

// ---------------------------------------------------------------- the commands

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
    Ok(json!({"name": name, "sheet": ran.sheet, "rows": ran.table.height(), "cols": ran.table.width()}))
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
        out.push(json!({"name": name, "sheet": ran.sheet, "rows": ran.table.height(), "cols": ran.table.width()}));
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
        if q.steps.len() >= gridcraft_model::MAX_STEPS {
            return Err(EngineError::Other(format!("\"{name}\" already holds the most steps a query can ({}).", gridcraft_model::MAX_STEPS)));
        }
        (q.steps.len(), q.steps.clone())
    };
    steps.push(step);
    let ran = set_steps_and_run(s, &name, steps, "data.query.addStep")?;
    Ok(json!({"name": name, "index": index, "rows": ran.table.height(), "cols": ran.table.width(), "sheet": ran.sheet}))
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
    Ok(json!({"name": name, "rows": ran.table.height(), "cols": ran.table.width(), "sheet": ran.sheet}))
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
    Ok(json!({"name": name, "rows": ran.table.height(), "cols": ran.table.width(), "sheet": ran.sheet}))
}

fn set_source(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("data.query.setSource", "missing name"))?.to_string();
    let source = parse_source(p.get("source").ok_or_else(|| bad("data.query.setSource", "missing source"))?)?;
    let nm = name.clone();
    edit(s, |cx| {
        if let Some(q) = cx.wb.queries.iter_mut().find(|q| q.name == nm) {
            q.source = source;
        }
        Ok(())
    })?;
    let ran = run_query(s, &name)?;
    Ok(json!({"name": name, "rows": ran.table.height(), "cols": ran.table.width(), "sheet": ran.sheet}))
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
    Ok(json!({"name": name, "sheet": ran.sheet, "rows": ran.table.height(), "cols": ran.table.width()}))
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
        match gridcraft_query::apply_step(table.clone(), step, index) {
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
        match gridcraft_query::apply_step(table.clone(), &step, at) {
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
    use gridcraft_parquet::ColumnType;

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
