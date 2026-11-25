//! The PivotTable has two aggregators — the record scan and the model's SQL engine — and they
//! have to agree.
//!
//! A model-sourced report is answered by the engine (`compute_with_source_sql`), while sheet
//! sources, related tables, DAX measures and the functions SQL cannot compose (product, standard
//! deviation) stay on the scan. Every test here computes the same report both ways and compares the
//! grids cell by cell, so "the fast path is on" is proven by `compute_detailed`'s flag *and* by the
//! answer being identical — a fast path that quietly disagrees would be worse than the slow one.

use std::fmt::Write as _;

use cella_core::Value;
use cella_engine::Session;
use cella_engine::pivot::{Output, compute, compute_detailed};
use serde_json::{Value as Json, json};

fn run(s: &mut Session, cmd: &str, params: Json) -> Json {
    s.execute(cmd, params).unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

/// A model table with something in every corner: four regions, six months, a card with a blank in
/// it, whole and fractional amounts, and a date column to group by.
fn model() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    let mut csv = String::from("Region,Month,Amount,Qty,Day\n");
    for i in 0..300 {
        let region = ["West", "East", "North", "South"][i % 4];
        let month = i % 6 + 1;
        let amount = if i % 7 == 0 { String::new() } else { format!("{}", (i % 17) as f64 * 1.5 + 1.0) };
        let qty = i % 5 + 1;
        let day = 45000 + i % 400;
        let _ = writeln!(csv, "{region},{month},{amount},{qty},{day}");
    }
    run(
        &mut s,
        "data.query.add",
        json!({
            "name": "Sales",
            "source": {"kind": "csv", "text": csv},
            "steps": [
                "PromoteHeaders",
                {"ChangeType": {"columns": ["Month", "Qty", "Day", "Amount"], "to": "Number"}},
            ],
            "load": "model",
        }),
    );
    s
}

/// Inserts a report over the model table and returns its name.
fn make(s: &mut Session, spec: Json) -> String {
    let mut params = spec;
    params["source"] = json!("Sales");
    params["destination"] = json!("new");
    let out = run(s, "insert.pivotTable", params);
    out["name"].as_str().expect("a report name").to_string()
}

/// The report both ways, the grids compared. Returns whether the engine answered.
fn agree(s: &mut Session, pivot: &str) -> bool {
    let engine = s.model_sql_for();
    let d = s.doc().expect("a document");
    let sheet = d.wb.sheets.iter().position(|sh| sh.pivots.iter().any(|p| p.name == pivot)).expect("a sheet");
    let idx = d.wb.sheet(sheet).and_then(|sh| sh.pivots.iter().position(|p| p.name == pivot)).expect("an index");
    let pt = d.wb.sheet(sheet).and_then(|sh| sh.pivots.get(idx)).expect("the report").clone();
    let (sql_out, _, used) = compute_detailed(&d.wb, &pt, sheet, engine.as_deref()).expect("the engine's report");
    let scanned = compute(&d.wb, &pt, sheet).expect("the scanned report");
    agree_grids(&sql_out, &scanned).unwrap_or_else(|e| panic!("{pivot}: {e}"));
    used
}

fn agree_grids(a: &Output, b: &Output) -> Result<(), String> {
    if a.width != b.width {
        return Err(format!("width {} against {}", a.width, b.width));
    }
    if a.row_kinds != b.row_kinds {
        return Err("the row kinds differ".into());
    }
    if a.body_offset != b.body_offset {
        return Err(format!("the body starts at {} against {}", a.body_offset, b.body_offset));
    }
    if a.grid.len() != b.grid.len() {
        return Err(format!("{} rows against {}", a.grid.len(), b.grid.len()));
    }
    for (i, (ra, rb)) in a.grid.iter().zip(&b.grid).enumerate() {
        if ra.len() != rb.len() {
            return Err(format!("row {i}: {} cells against {}", ra.len(), rb.len()));
        }
        for (j, (ca, cb)) in ra.iter().zip(rb).enumerate() {
            if ca.kind != cb.kind {
                return Err(format!("({i}, {j}): {:?} against {:?}", ca.kind, cb.kind));
            }
            if !same(&ca.value, &cb.value) {
                return Err(format!("({i}, {j}): {:?} against {:?}", ca.value, cb.value));
            }
        }
    }
    Ok(())
}

/// Numbers as a spreadsheet compares them: a sum is added up in a different order on each path, so
/// the last bits may differ; everything else has to be equal.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0),
        _ => a == b,
    }
}

#[test]
fn the_engine_answers_a_one_field_report_the_way_the_scan_does() {
    if !cfg!(feature = "sql-engine") {
        return;
    }
    let mut s = model();
    let name = make(&mut s, json!({"rows": ["Sales[Region]"], "values": ["Sales[Amount]"]}));
    assert!(agree(&mut s, &name), "the engine should answer a plain sum over a model table");
}

#[test]
fn the_engine_answers_a_cross_tab_the_way_the_scan_does() {
    if !cfg!(feature = "sql-engine") {
        return;
    }
    let mut s = model();
    let name = make(&mut s, json!({"rows": ["Sales[Region]"], "columns": ["Sales[Month]"], "values": ["Sales[Amount]"]}));
    assert!(agree(&mut s, &name));
}

#[test]
fn the_engine_answers_sub_totals_and_two_values_the_way_the_scan_does() {
    if !cfg!(feature = "sql-engine") {
        return;
    }
    let mut s = model();
    let name = make(
        &mut s,
        json!({
            "rows": ["Sales[Region]", "Sales[Month]"],
            "columns": ["Sales[Qty]"],
            "values": ["Sales[Amount]", {"field": "Sales[Qty]", "func": "count"}],
        }),
    );
    assert!(agree(&mut s, &name));
}

#[test]
fn every_function_the_engine_can_compose_agrees() {
    if !cfg!(feature = "sql-engine") {
        return;
    }
    let mut s = model();
    for func in ["sum", "count", "average", "max", "min"] {
        let name = make(&mut s, json!({"rows": ["Sales[Region]"], "values": [{"field": "Sales[Amount]", "func": func}]}));
        assert!(agree(&mut s, &name), "the engine should answer `{func}`");
    }
}

#[test]
fn a_date_grouped_field_agrees() {
    if !cfg!(feature = "sql-engine") {
        return;
    }
    let mut s = model();
    // The engine groups by the raw serial number; the item's label and bucket are still computed
    // here, so a year/quarter/month caption has to come out the same from both paths.
    for group in ["years", "quarters", "months"] {
        let name = make(&mut s, json!({"rows": [{"field": "Sales[Day]", "group": group}], "values": ["Sales[Amount]"]}));
        assert!(agree(&mut s, &name), "`{group}` should agree");
    }
}

#[test]
fn a_slicers_selection_agrees() {
    if !cfg!(feature = "sql-engine") {
        return;
    }
    let mut s = model();
    let name = make(&mut s, json!({"rows": ["Sales[Region]"], "values": ["Sales[Amount]"]}));
    run(&mut s, "pivot.filter", json!({"pivot": name, "field": "Sales[Region]", "selected": ["West", "East"]}));
    // A filter names labels; the engine turns them back into the column's own values.
    assert!(agree(&mut s, &name), "a filtered report should still be the engine's");
}

#[test]
fn the_functions_the_engine_cannot_compose_stay_on_the_scan() {
    let mut s = model();
    for func in ["product", "stdDev", "var"] {
        let name = make(&mut s, json!({"rows": ["Sales[Region]"], "values": [{"field": "Sales[Amount]", "func": func}]}));
        // Both paths answer (the scan does the work twice); the point is that the engine says no.
        assert!(!agree(&mut s, &name), "`{func}` must not be sent to the engine");
    }
}

#[test]
fn a_report_with_no_model_behind_it_is_unaffected() {
    // A sheet-sourced report never looks at the engine, so it computes the same either way.
    let mut s = Session::new();
    s.new_workbook();
    run(&mut s, "range.setValues", json!({"range": "A1:B4", "values": [["Region", "Amount"], ["West", 1], ["East", 2], ["West", 3]]}));
    let out = run(&mut s, "insert.pivotTable", json!({"source": "Sheet1!A1:B4", "rows": ["Region"], "values": ["Amount"], "destination": "new"}));
    let name = out["name"].as_str().expect("a name").to_string();
    assert!(!agree(&mut s, &name), "a sheet source is not the engine's");
}
