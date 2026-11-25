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
fn a_measure_is_answered_by_the_engine_at_every_level() {
    if !cfg!(feature = "sql-engine") {
        return;
    }
    let mut s = related();
    run(&mut s, "data.measure.add", json!({"name": "Total", "table": "Sales", "formula": "SUM(Sales[Amount])"}));
    let name = make(&mut s, json!({"rows": ["Region[Name]"], "values": ["Total"]}));
    assert!(agree(&mut s, &name), "the engine should answer a measure report");
}

#[test]
fn a_ratio_measure_is_not_averaged_from_its_sub_totals() {
    if !cfg!(feature = "sql-engine") {
        return;
    }
    // The whole reason a measure needs every level computed: the ratio of the totals is not the
    // total of the child ratios. Two row levels make the difference visible.
    let mut s = related();
    run(&mut s, "data.measure.add", json!({"name": "Share", "table": "Sales", "formula": "DIVIDE(SUM(Sales[Amount]), COUNT(Sales[Amount]))"}));
    let name = make(&mut s, json!({"rows": ["Region[Name]", "Sales[Amount]"], "values": ["Share"]}));
    assert!(agree(&mut s, &name), "a ratio measure must be the engine's, computed per level");
}

#[test]
fn a_calculate_measure_agrees() {
    if !cfg!(feature = "sql-engine") {
        return;
    }
    // CALCULATE's filter becomes the aggregate's own `WHERE`, ANDed onto the cell's group.
    let mut s = related();
    run(&mut s, "data.measure.add", json!({"name": "West", "table": "Sales", "formula": "CALCULATE(SUM(Sales[Amount]), Region[Name] = \"West\")"}));
    let name = make(&mut s, json!({"rows": ["Region[Name]"], "values": ["West"]}));
    assert!(agree(&mut s, &name));
}

#[test]
fn a_measure_over_a_file_backed_fact_table_is_answered_in_sql() {
    if !cfg!(feature = "sql-engine") {
        return;
    }
    // The scan cannot walk a file-backed fact, so a measure on one can only be the engine's.
    let mut csv = String::from("RegionId,Amount\n");
    for i in 0..500 {
        let _ = std::fmt::Write::write_fmt(&mut csv, format_args!("{},{}\n", i % 4 + 1, i % 10 + 1));
    }
    let path = std::env::temp_dir().join(format!("cella-pivot-measure-{}.csv", std::process::id()));
    std::fs::write(&path, csv).expect("write the fact csv");
    let mut s = Session::new();
    s.new_workbook();
    run(&mut s, "data.model.loadFile", json!({"path": path.display().to_string(), "name": "Sales"}));
    run(
        &mut s,
        "data.query.add",
        json!({
            "name": "Region",
            "source": {"kind": "csv", "text": REGION_CSV},
            "steps": ["PromoteHeaders", {"ChangeType": {"columns": ["Id"], "to": "Number"}}],
            "load": "model",
        }),
    );
    run(&mut s, "data.relationship.add", json!({"from": {"table": "Sales", "column": "RegionId"}, "to": {"table": "Region", "column": "Id"}}));
    run(&mut s, "data.measure.add", json!({"name": "Total", "table": "Sales", "formula": "SUM(Sales[Amount])"}));
    let name = make(&mut s, json!({"rows": ["Region[Name]"], "values": ["Total"]}));
    let engine = s.model_sql_for();
    let d = s.doc().expect("a document");
    let sheet = d.wb.sheets.iter().position(|sh| sh.pivots.iter().any(|p| p.name == name)).expect("a sheet");
    let idx = d.wb.sheet(sheet).and_then(|sh| sh.pivots.iter().position(|p| p.name == name)).expect("an index");
    let pt = d.wb.sheet(sheet).and_then(|sh| sh.pivots.get(idx)).expect("the report").clone();
    let (out, _, used) = compute_detailed(&d.wb, &pt, sheet, engine.as_deref()).expect("the report");
    assert!(used, "the engine answered the measure");
    let grand = out.grid.last().and_then(|row| row.last()).map(|c| c.value.clone()).unwrap_or_default();
    // 500 rows, amounts repeating 1..10: 50 of each, so the sum is 50 * 55.
    assert_eq!(grand.as_f64().unwrap_or(-1.0), 2_750.0);
    let _ = std::fs::remove_file(&path);
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

// ---------------------------------------------------------------- relationships

const SALES_CSV: &str = "RegionId,Amount\n1,10\n2,5\n1,7\n3,1\n2,20\n";
const REGION_CSV: &str = "Id,Name\n1,West\n2,East\n3,North\n4,South\n";

/// Two model tables, related many-to-one: five sales and the four regions they name.
fn related() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    run(
        &mut s,
        "data.query.add",
        json!({
            "name": "Sales",
            "source": {"kind": "csv", "text": SALES_CSV},
            "steps": ["PromoteHeaders", {"ChangeType": {"columns": ["RegionId", "Amount"], "to": "Number"}}],
            "load": "model",
        }),
    );
    run(
        &mut s,
        "data.query.add",
        json!({
            "name": "Region",
            "source": {"kind": "csv", "text": REGION_CSV},
            "steps": ["PromoteHeaders", {"ChangeType": {"columns": ["Id"], "to": "Number"}}],
            "load": "model",
        }),
    );
    run(&mut s, "data.relationship.add", json!({"from": {"table": "Sales", "column": "RegionId"}, "to": {"table": "Region", "column": "Id"}}));
    s
}

#[test]
fn the_engine_joins_a_related_tables_column_the_way_the_scan_does() {
    if !cfg!(feature = "sql-engine") {
        return;
    }
    let mut s = related();
    let name = make(&mut s, json!({"rows": ["Region[Name]"], "values": ["Sales[Amount]"]}));
    assert!(agree(&mut s, &name), "the engine should answer a report grouped by a dimension");
}

#[test]
fn a_filter_on_a_dimension_column_agrees() {
    if !cfg!(feature = "sql-engine") {
        return;
    }
    let mut s = related();
    let name = make(&mut s, json!({"rows": ["Region[Name]"], "values": ["Sales[Amount]"]}));
    // A slicer on the dimension: the label maps back to `Region[Name]`'s own values, and the
    // dimension is a joined table — so the filter has to name its alias.
    run(&mut s, "pivot.filter", json!({"pivot": name, "field": "Region[Name]", "selected": ["West", "North"]}));
    assert!(agree(&mut s, &name));
}

#[test]
fn a_file_backed_fact_table_joins_its_dimension_in_sql() {
    if !cfg!(feature = "sql-engine") {
        return;
    }
    // The fact table is a file the engine reads (the document caches none of it), the dimension is
    // a cached query table: only SQL can join them for a report.
    let mut csv = String::from("RegionId,Amount\n");
    let mut total = 0.0;
    for i in 0..1_000 {
        let region = i % 4 + 1;
        let amount = (i % 10 + 1) as f64;
        total += amount;
        let _ = std::fmt::Write::write_fmt(&mut csv, format_args!("{region},{amount:.0}\n"));
    }
    let path = std::env::temp_dir().join(format!("cella-pivot-join-{}.csv", std::process::id()));
    std::fs::write(&path, csv).expect("write the fact csv");
    let mut s = Session::new();
    s.new_workbook();
    run(&mut s, "data.model.loadFile", json!({"path": path.display().to_string(), "name": "Sales"}));
    run(
        &mut s,
        "data.query.add",
        json!({
            "name": "Region",
            "source": {"kind": "csv", "text": REGION_CSV},
            "steps": ["PromoteHeaders", {"ChangeType": {"columns": ["Id"], "to": "Number"}}],
            "load": "model",
        }),
    );
    run(&mut s, "data.relationship.add", json!({"from": {"table": "Sales", "column": "RegionId"}, "to": {"table": "Region", "column": "Id"}}));
    let name = make(&mut s, json!({"rows": ["Region[Name]"], "values": ["Sales[Amount]"]}));
    run(&mut s, "pivot.refresh", json!({"pivot": name, "replace": true}));
    // The scan cannot walk a file-backed fact table, so a report with numbers can only be the
    // engine's; its grand total is the file's sum.
    let engine = s.model_sql_for();
    let d = s.doc().expect("a document");
    let sheet = d.wb.sheets.iter().position(|sh| sh.pivots.iter().any(|p| p.name == name)).expect("a sheet");
    let idx = d.wb.sheet(sheet).and_then(|sh| sh.pivots.iter().position(|p| p.name == name)).expect("an index");
    let pt = d.wb.sheet(sheet).and_then(|sh| sh.pivots.get(idx)).expect("the report").clone();
    let (out, _, used) = compute_detailed(&d.wb, &pt, sheet, engine.as_deref()).expect("the report");
    assert!(used, "the engine answered by joining the dimension");
    let grand = out.grid.last().and_then(|row| row.last()).map(|c| c.value.clone()).unwrap_or_default();
    assert_eq!(grand.as_f64().unwrap_or(-1.0), total, "the report's grand total is the file's sum");
    let _ = std::fs::remove_file(&path);
}
