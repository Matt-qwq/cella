//! The data model: tables a query loads into memory, the relationships between them, and the
//! measures defined over them.
//!
//! A model table is a query's result kept **off the grid**: no sheet, no cells, just rows in the
//! workbook's model. That is what lets a workbook hold a fact table of a million rows plus the
//! lookups beside it, join them by a relationship, and answer a measure over the join.

use cella_core::Value;
use cella_model::{MAX_MEASURES, MAX_RELATIONSHIPS, Measure, Relationship};

use serde_json::json;

use super::*;

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "data.model.tables", "Model Tables", ["Data", "Queries & Connections"], None, "{} -> [{name, columns, rows, query}]", has_doc, tables),
        cmd!(
            "data.model.removeTable",
            "Remove From Model",
            ["Data", "Queries & Connections"],
            None,
            "{table} (its relationships go with it; measures that used it stay and report the error)",
            has_doc,
            remove_table
        ),
        cmd!(
            "data.relationship.add",
            "Add Relationship",
            ["Data", "Queries & Connections"],
            None,
            "{from: {table, column}, to: {table, column}} (many → one; filters flow from \"to\" to \"from\")",
            has_doc,
            relationship_add
        ),
        cmd!(
            "data.relationship.remove",
            "Remove Relationship",
            ["Data", "Queries & Connections"],
            None,
            "{from: {table, column}, to: {table, column}} | {index}",
            has_doc,
            relationship_remove
        ),
        cmd!(query "data.relationship.list", "Relationships", ["Data", "Queries & Connections"], None, "{} -> [{from_table, from_column, to_table, to_column}]", has_doc, relationship_list),
        cmd!(
            "data.measure.add",
            "Add Measure",
            ["Data", "Queries & Connections"],
            None,
            "{name, table, formula, format?} (DAX subset: SUM/AVERAGE/COUNT/COUNTA/MIN/MAX/DISTINCTCOUNT, CALCULATE, DIVIDE, arithmetic)",
            has_doc,
            measure_add
        ),
        cmd!("data.measure.set", "Edit Measure", ["Data", "Queries & Connections"], None, "{name, formula?, format?, table?}", has_doc, measure_set),
        cmd!("data.measure.remove", "Delete Measure", ["Data", "Queries & Connections"], None, "{name}", has_doc, measure_remove),
        cmd!(query "data.measure.list", "Measures", ["Data", "Queries & Connections"], None, "{} -> [{name, table, formula, format}]", has_doc, measure_list),
        cmd!(
            query "data.measure.evaluate",
            "Evaluate Measure",
            ["Data", "Queries & Connections"],
            None,
            "{name, filters?: [{table, column, value}]} -> {value, display}",
            has_doc,
            measure_evaluate
        ),
        cmd!(
            query "data.model.query",
            "Run SQL",
            ["Data", "Queries & Connections"],
            None,
            "{sql} -> {columns, rows} — a read-only query over the model's tables, run by the engine the model lives on (desktop only; `data.model.tables` names them)",
            has_doc,
            sql_query
        ),
        cmd!(
            "data.model.loadFile",
            "Add From File",
            ["Data", "Queries & Connections"],
            None,
            "{paths: [path] | path, name?, format?: parquet|csv, replace?: bool} -> {name, rows, cols} — the rows stay in the file and the engine reads them, so the table may hold more rows than the model's cache does (desktop only)",
            has_doc,
            load_file
        ),
    ]
}

/// The model, or an error that says what is missing.
fn model(s: &Session) -> Result<&cella_model::DataModel> {
    let d = s.doc()?;
    if d.wb.model.tables.is_empty() {
        return Err(EngineError::Other("This workbook has no data model yet: load a query into the model first.".into()));
    }
    Ok(&d.wb.model)
}

fn table_param(p: &Json, key: &str) -> Result<(String, String)> {
    let v = p.get(key).ok_or_else(|| bad("data.relationship", format!("missing {key}")))?;
    let table = v.get("table").and_then(Json::as_str).ok_or_else(|| bad("data.relationship", format!("{key}.table is missing")))?;
    let column = v.get("column").and_then(Json::as_str).ok_or_else(|| bad("data.relationship", format!("{key}.column is missing")))?;
    Ok((table.to_string(), column.to_string()))
}

fn tables(s: &mut Session, _: &Json) -> Result<Json> {
    // The small metadata first, then one count per file-backed table from the engine: its rows
    // are not in the document, so `height()` cannot answer.
    let listed: Vec<(String, Vec<String>, usize, Option<String>, bool)> = {
        let d = s.doc()?;
        d.wb.model.tables.iter().map(|t| (t.name.clone(), t.columns.clone(), t.height(), t.source_query.clone(), t.is_file_backed())).collect()
    };
    let mut out = Vec::with_capacity(listed.len());
    for (name, columns, rows, query, file) in listed {
        let rows = if file {
            let counted = (|s: &mut Session| -> Result<f64> {
                let answer = s
                    .model_engine()?
                    .query(&format!("SELECT COUNT(*) AS n FROM {}", cella_sql::quote_ident(&name)))
                    .map_err(|e| EngineError::Other(e.to_string()))?;
                Ok(answer.rows.first().and_then(|r| r.first()).and_then(Value::as_f64).unwrap_or(0.0))
            })(s);
            counted.map(|n| n.max(0.0) as u64).unwrap_or(0)
        } else {
            rows as u64
        };
        out.push(json!({"name": name, "columns": columns, "rows": rows, "query": query, "file": file}));
    }
    Ok(Json::Array(out))
}

fn remove_table(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "table").ok_or_else(|| bad("data.model.removeTable", "missing table"))?.to_string();
    edit(s, |cx| {
        let Some(i) = cx.wb.model.table_index(&name) else {
            return Err(EngineError::Other(format!("The model has no table named \"{name}\".")));
        };
        cx.wb.model.tables.remove(i);
        // Relationships that pointed at it cannot mean anything any more; a measure that used it
        // stays, and reports the missing table when it is evaluated: dropping a user's formula
        // because a table went away would lose their work.
        cx.wb.model.relationships.retain(|r| !r.from_table.eq_ignore_ascii_case(&name) && !r.to_table.eq_ignore_ascii_case(&name));
        Ok(())
    })?;
    s.model_changed();
    Ok(json!({"removed": name}))
}

fn relationship_add(s: &mut Session, p: &Json) -> Result<Json> {
    let (from_table, from_column) = table_param(p, "from")?;
    let (to_table, to_column) = table_param(p, "to")?;
    let r = Relationship { from_table, from_column, to_table, to_column };
    edit(s, |cx| {
        if cx.wb.model.tables.len() < 2 {
            return Err(EngineError::Other("A relationship needs two tables in the model.".into()));
        }
        if cx.wb.model.relationships.len() >= MAX_RELATIONSHIPS {
            return Err(EngineError::Other(format!("A workbook holds at most {MAX_RELATIONSHIPS} relationships.")));
        }
        if let Some(problem) = cx.wb.model.relationship_problem(&r, None) {
            return Err(EngineError::Other(problem));
        }
        cx.wb.model.relationships.push(r.clone());
        // A relationship changes what every measure over the model means.
        cx.structural = true;
        Ok(())
    })?;
    Ok(json!({"from": format!("{}[{}]", r.from_table, r.from_column), "to": format!("{}[{}]", r.to_table, r.to_column)}))
}

fn relationship_remove(s: &mut Session, p: &Json) -> Result<Json> {
    let index = u32_param(p, "index").map(|i| i as usize);
    let pair = match index {
        Some(_) => None,
        None => Some((table_param(p, "from")?, table_param(p, "to")?)),
    };
    let removed = edit(s, |cx| {
        let at = match (index, &pair) {
            (Some(i), _) => (i < cx.wb.model.relationships.len()).then_some(i),
            (None, Some(((ft, fc), (tt, tc)))) => cx.wb.model.relationships.iter().position(|r| {
                r.from_table.eq_ignore_ascii_case(ft)
                    && r.from_column.eq_ignore_ascii_case(fc)
                    && r.to_table.eq_ignore_ascii_case(tt)
                    && r.to_column.eq_ignore_ascii_case(tc)
            }),
            _ => None,
        };
        let Some(at) = at else { return Err(EngineError::Other("There is no such relationship.".into())) };
        let r = cx.wb.model.relationships.remove(at);
        Ok(json!({"from": format!("{}[{}]", r.from_table, r.from_column), "to": format!("{}[{}]", r.to_table, r.to_column)}))
    })?;
    Ok(removed)
}

fn relationship_list(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    Ok(Json::Array(
        d.wb.model
            .relationships
            .iter()
            .map(|r| {
                json!({
                    "from_table": r.from_table,
                    "from_column": r.from_column,
                    "to_table": r.to_table,
                    "to_column": r.to_column,
                })
            })
            .collect(),
    ))
}

fn measure_add(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("data.measure.add", "missing name"))?.trim().to_string();
    let table = str_param(p, "table").unwrap_or("").trim().to_string();
    let formula = str_param(p, "formula").ok_or_else(|| bad("data.measure.add", "missing formula"))?.trim().to_string();
    let format = str_param(p, "format").map(str::to_string);
    model(s)?;
    edit(s, |cx| {
        if cx.wb.model.measures.len() >= MAX_MEASURES {
            return Err(EngineError::Other(format!("A workbook holds at most {MAX_MEASURES} measures.")));
        }
        if !cx.wb.model.measure_name_is_free(&name, None) {
            return Err(EngineError::Other(format!("\"{name}\" is already the name of a measure or a table (or is blank).")));
        }
        // The home table only groups the field list, so it may be left out; it defaults to the
        // model's first table.
        let home = if table.is_empty() { cx.wb.model.tables.first().map(|t| t.name.clone()).unwrap_or_default() } else { table };
        let added = Measure { name: name.clone(), table: home, formula, format };
        let formula = added.formula.clone();
        cx.wb.model.measures.push(added);
        // Prove the formula before keeping it: a measure that names a table or column the model
        // does not have is a formula that will never compute, and finding that out now (while the
        // person is looking at the formula) is the whole point of validating it here.
        if let Err(e) = cella_dax::validate(&cx.wb.model, &formula) {
            cx.wb.model.measures.retain(|m| !m.name.eq_ignore_ascii_case(&name));
            return Err(EngineError::Other(e.to_string()));
        }
        cx.structural = true;
        Ok(())
    })?;
    Ok(json!({"name": name}))
}

fn measure_set(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("data.measure.set", "missing name"))?.to_string();
    let formula = str_param(p, "formula").map(|f| f.trim().to_string());
    let format = str_param(p, "format").map(str::to_string);
    let table = str_param(p, "table").map(str::to_string);
    edit(s, |cx| {
        let Some(m) = cx.wb.model.measures.iter_mut().find(|m| m.name.eq_ignore_ascii_case(&name)) else {
            return Err(EngineError::Other(format!("There is no measure named \"{name}\".")));
        };
        let before = m.formula.clone();
        if let Some(f) = formula {
            m.formula = f;
        }
        let now = m.formula.clone();
        if let Some(f) = format {
            m.format = Some(f);
        }
        if let Some(t) = table {
            m.table = t;
        }
        // The same rule as adding: an edit that does not compute is not kept.
        if let Err(e) = cella_dax::validate(&cx.wb.model, &now) {
            if let Some(m) = cx.wb.model.measures.iter_mut().find(|m| m.name.eq_ignore_ascii_case(&name)) {
                m.formula = before;
            }
            return Err(EngineError::Other(e.to_string()));
        }
        cx.structural = true;
        Ok(())
    })?;
    Ok(json!({"name": name}))
}

fn measure_remove(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("data.measure.remove", "missing name"))?.to_string();
    edit(s, |cx| {
        let before = cx.wb.model.measures.len();
        cx.wb.model.measures.retain(|m| !m.name.eq_ignore_ascii_case(&name));
        if cx.wb.model.measures.len() == before {
            return Err(EngineError::Other(format!("There is no measure named \"{name}\".")));
        }
        Ok(())
    })?;
    Ok(json!({"removed": name}))
}

fn measure_list(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    Ok(Json::Array(d.wb.model.measures.iter().map(|m| json!({"name": m.name, "table": m.table, "formula": m.formula, "format": m.format})).collect()))
}

/// A value as JSON for a control-channel answer: empty is null, numbers and booleans keep their
/// type, an error is its text exactly as the grid shows it.
pub(crate) fn value_json(v: &Value) -> Json {
    match v {
        Value::Empty => Json::Null,
        Value::Number(n) => Json::from(*n),
        Value::Bool(b) => Json::from(*b),
        other => json!(other.display()),
    }
}

fn measure_evaluate(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("data.measure.evaluate", "missing name"))?.to_string();
    let filters = p.get("filters").and_then(Json::as_array).cloned().unwrap_or_default();
    let d = s.doc()?;
    let m = d.wb.model.measure(&name).cloned().ok_or_else(|| EngineError::Other(format!("There is no measure named \"{name}\".")))?;
    let mut ctx = cella_dax::Context::new();
    for f in &filters {
        let table = f.get("table").and_then(Json::as_str).unwrap_or("");
        let column = f.get("column").and_then(Json::as_str).unwrap_or("");
        let value = match f.get("value") {
            Some(Json::Number(n)) => Value::number(n.as_f64().unwrap_or(0.0)),
            Some(Json::Bool(b)) => Value::Bool(*b),
            Some(Json::String(s)) => Value::text(s.clone()),
            _ => Value::Empty,
        };
        ctx = ctx.filter(table, column, value);
    }
    let value = cella_dax::evaluate(&d.wb.model, &m.formula, &ctx).map_err(|e| EngineError::Other(e.to_string()))?;
    Ok(json!({"value": value_json(&value), "display": value.display()}))
}

/// `data.model.query`: SQL over the model's tables, answered by the engine the model lives on.
///
/// Read-only by design: the database is the one holding the model, so a statement that could
/// write a file, attach another database or change rows is refused before it reaches the engine
/// ([`cella_sql::is_read_only`]). The tables are named as `data.model.tables` lists them; quote a
/// name with spaces or capitals (`SELECT * FROM "My Table"`).
fn sql_query(s: &mut Session, p: &Json) -> Result<Json> {
    let sql = str_param(p, "sql").map(str::trim).filter(|q| !q.is_empty()).ok_or_else(|| bad("data.model.query", "missing sql"))?;
    model(s)?;
    if !cella_sql::is_read_only(sql) {
        return Err(bad("data.model.query", "only a query (SELECT …) runs here: the model's SQL is read-only"));
    }
    let table = s.model_engine()?.query(sql).map_err(|e| bad("data.model.query", e.to_string()))?;
    let rows: Vec<Json> = table.rows.iter().map(|r| Json::Array(r.iter().map(value_json).collect())).collect();
    Ok(json!({"columns": table.columns, "rows": rows}))
}

/// The paths a `data.model.loadFile` (or a query's own load) names: `paths` or a single `path`.
fn file_paths(p: &Json) -> Result<Vec<String>> {
    let mut paths: Vec<String> = match p.get("paths") {
        Some(Json::Array(list)) => list.iter().filter_map(Json::as_str).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect(),
        _ => Vec::new(),
    };
    if let Some(one) = str_param(p, "path").map(str::trim).filter(|t| !t.is_empty()) {
        paths.push(one.to_string());
    }
    if paths.is_empty() {
        return Err(bad("data.model.loadFile", "missing `path` (or `paths`, for several files read as one table)"));
    }
    Ok(paths)
}

/// The format a file load names, or the one its extension suggests (a `.csv` is text, anything
/// else is read as Parquet).
fn file_format(named: Option<&str>, paths: &[String]) -> Result<cella_model::ModelFileFormat> {
    Ok(match named.map(|f| f.to_ascii_lowercase()) {
        Some(f) if f == "csv" || f == "text" => cella_model::ModelFileFormat::Csv,
        Some(f) if f == "parquet" => cella_model::ModelFileFormat::Parquet,
        Some(other) => return Err(bad("data.model.loadFile", format!("\"{other}\" is not a file format (parquet, csv)"))),
        None => {
            let first = paths.first().map(|p| p.to_ascii_lowercase()).unwrap_or_default();
            if first.ends_with(".csv") || first.ends_with(".tsv") || first.ends_with(".txt") {
                cella_model::ModelFileFormat::Csv
            } else {
                cella_model::ModelFileFormat::Parquet
            }
        }
    })
}

/// The name a file load gets: the one it was given, or the first file's stem ("sales.parquet"
/// becomes "sales").
fn file_name(named: Option<&str>, paths: &[String]) -> String {
    match named.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => n.to_string(),
        None => {
            let file = paths.first().map(|p| p.rsplit(['/', '\\']).next().unwrap_or(p)).unwrap_or("Table");
            file.rsplit_once('.').map(|(stem, _)| stem).filter(|stem| !stem.is_empty()).unwrap_or(file).to_string()
        }
    }
}
/// `data.model.loadFile`: a model table whose rows are a file's, not the document's.
///
/// The engine reads the file itself (`read_parquet`/`read_csv`), which costs it ~22–99 ns a row
/// against the ~720 a row a table handed over cell by cell costs — that difference is the whole
/// reason a model table can be bigger than the 4M-cell cache the document keeps. The document
/// stores the paths and the column names; a reopened workbook re-reads the file, and a file that
/// has moved away is an error a report shows rather than a silently empty table.
fn load_file(s: &mut Session, p: &Json) -> Result<Json> {
    let paths = file_paths(p)?;
    let format = file_format(str_param(p, "format"), &paths)?;
    let name = file_name(str_param(p, "name"), &paths);
    let (rows, cols) = load_model_file(s, &name, &paths, format, bool_param(p, "replace").unwrap_or(false))?;
    Ok(json!({"name": name, "rows": rows, "cols": cols}))
}

/// A query's own file load: the same table `data.model.loadFile` makes, so "load this file into the
/// model" is one path whether a person runs the command or a query does.
#[allow(clippy::too_many_arguments)]
pub(crate) fn load_model_file(
    s: &mut Session,
    name: &str,
    paths: &[String],
    format: cella_model::ModelFileFormat,
    replace: bool,
) -> Result<(u64, usize)> {
    let previous = {
        let d = s.doc()?;
        d.wb.model.tables.iter().find(|t| t.name.eq_ignore_ascii_case(name)).cloned()
    };
    if previous.is_some() && !replace {
        return Err(bad("data.model.loadFile", format!("\"{name}\" is already a table in the model (pass replace: true to reload it)")));
    }
    edit(s, |cx| {
        if cx.wb.model.tables.len() >= cella_model::MAX_MODEL_TABLES && cx.wb.model.table_index(name).is_none() {
            return Err(EngineError::Other(format!("The data model holds at most {} tables.", cella_model::MAX_MODEL_TABLES)));
        }
        let loaded = cella_model::ModelTable {
            name: name.to_string(),
            columns: Vec::new(),
            rows: Default::default(),
            column_types: Vec::new(),
            source_query: None,
            source_file: Some(cella_model::ModelFile { paths: paths.to_vec(), format }),
        };
        match cx.wb.model.table_index(name) {
            Some(i) => cx.wb.model.tables[i] = loaded,
            None => cx.wb.model.tables.push(loaded),
        }
        cx.structural = true;
        Ok(())
    })?;
    s.model_changed();

    // The engine reads the file here: a missing or unreadable one is refused, and the definition
    // does not stay behind (the table it replaced comes back).
    let described = (|s: &mut Session| -> Result<(Vec<String>, Vec<String>)> {
        let answer = s
            .model_engine()?
            .query(&format!("DESCRIBE SELECT * FROM {}", cella_sql::quote_ident(name)))
            .map_err(|e| EngineError::Other(format!("The model's table \"{name}\" could not be read from {}: {e}", paths.join(", "))))?;
        // `DESCRIBE` answers (column_name, column_type, null, key, default, extra): the names are
        // the table's columns and the types are what the field list and the default function read.
        let mut columns = Vec::with_capacity(answer.rows.len());
        let mut types = Vec::with_capacity(answer.rows.len());
        for row in &answer.rows {
            if let Some(name) = row.first() {
                columns.push(name.display());
                types.push(row.get(1).map(|v| v.display()).unwrap_or_default());
            }
        }
        if columns.is_empty() {
            return Err(EngineError::Other(format!("The file {} has no columns.", paths.join(", "))));
        }
        Ok((columns, types))
    })(s);
    let (columns, types) = match described {
        Ok(columns) => columns,
        Err(e) => {
            let _ = edit(s, |cx| {
                cx.wb.model.tables.retain(|t| !t.name.eq_ignore_ascii_case(name));
                if let Some(before) = previous.clone() {
                    cx.wb.model.tables.push(before);
                }
                cx.structural = true;
                Ok(())
            });
            s.model_changed();
            return Err(e);
        }
    };
    let rows = (|s: &mut Session| -> Result<f64> {
        let answer = s
            .model_engine()?
            .query(&format!("SELECT COUNT(*) AS n FROM {}", cella_sql::quote_ident(name)))
            .map_err(|e| EngineError::Other(e.to_string()))?;
        Ok(answer.rows.first().and_then(|r| r.first()).and_then(Value::as_f64).unwrap_or(0.0))
    })(s)
    .unwrap_or(0.0)
    .max(0.0);
    let cols = columns.len();
    // The columns are the document's now, and the engine's copy is already right: bumping the
    // generation here would make the next query re-read the file.
    edit(s, |cx| {
        if let Some(i) = cx.wb.model.table_index(name) {
            cx.wb.model.tables[i].columns = columns.clone();
            cx.wb.model.tables[i].column_types = types.clone();
        }
        Ok(())
    })?;
    Ok((rows as u64, cols))
}
