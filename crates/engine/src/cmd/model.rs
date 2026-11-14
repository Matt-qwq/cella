//! The data model: tables a query loads into memory, the relationships between them, and the
//! measures defined over them.
//!
//! A model table is a query's result kept **off the grid**: no sheet, no cells, just rows in the
//! workbook's model. That is what lets a workbook hold a fact table of a million rows plus the
//! lookups beside it, join them by a relationship, and answer a measure over the join.

use gridcraft_core::Value;
use gridcraft_model::{MAX_MEASURES, MAX_RELATIONSHIPS, Measure, Relationship};

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
    ]
}

/// The model, or an error that says what is missing.
fn model(s: &Session) -> Result<&gridcraft_model::DataModel> {
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
    let d = s.doc()?;
    Ok(Json::Array(
        d.wb.model
            .tables
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "columns": t.columns,
                    "rows": t.height(),
                    "query": t.source_query,
                })
            })
            .collect(),
    ))
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
        if let Err(e) = gridcraft_dax::validate(&cx.wb.model, &formula) {
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
        if let Err(e) = gridcraft_dax::validate(&cx.wb.model, &now) {
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
    let mut ctx = gridcraft_dax::Context::new();
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
    let value = gridcraft_dax::evaluate(&d.wb.model, &m.formula, &ctx).map_err(|e| EngineError::Other(e.to_string()))?;
    Ok(json!({"value": value_json(&value), "display": value.display()}))
}
