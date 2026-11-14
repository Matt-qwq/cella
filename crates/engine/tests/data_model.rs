//! The data model end to end: two queries loaded into it, a relationship between them, a measure
//! over the join, and a cell that reads the measure — through a refresh and through save/reopen.

use gridcraft_engine::Session;
use serde_json::{Value, json};

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.execute(cmd, params).unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

const SALES: &str = "RegionId,Amount\n1,10\n2,5\n1,7\n";
const REGION: &str = "Id,Name\n1,West\n2,East\n";

/// Both tables in the model, related, with a measure over the join.
fn model_session() -> Session {
    let mut s = session();
    run(
        &mut s,
        "data.query.add",
        json!({
            "name": "Sales",
            "source": {"kind": "csv", "text": SALES},
            "steps": ["PromoteHeaders", {"ChangeType": {"columns": ["RegionId", "Amount"], "to": "Number"}}],
            "load": "model",
        }),
    );
    run(
        &mut s,
        "data.query.add",
        json!({
            "name": "Region",
            "source": {"kind": "csv", "text": REGION},
            "steps": ["PromoteHeaders", {"ChangeType": {"columns": ["Id"], "to": "Number"}}],
            "load": "model",
        }),
    );
    run(&mut s, "data.relationship.add", json!({"from": {"table": "Sales", "column": "RegionId"}, "to": {"table": "Region", "column": "Id"}}));
    run(&mut s, "data.measure.add", json!({"name": "Total", "table": "Sales", "formula": "SUM(Sales[Amount])"}));
    s
}

#[test]
fn a_query_can_load_into_the_model_instead_of_a_sheet() {
    let s = model_session();
    let tables = run(&mut { s }, "data.model.tables", json!({}));
    let names: Vec<&str> = tables.as_array().map(|a| a.iter().filter_map(|t| t["name"].as_str()).collect()).unwrap_or_default();
    assert_eq!(names, ["Sales", "Region"]);
    assert_eq!(tables[0]["rows"], 3);
    assert_eq!(tables[0]["columns"], json!(["RegionId", "Amount"]));
}

#[test]
fn a_model_load_creates_no_sheet() {
    let mut s = session();
    run(&mut s, "data.query.add", json!({"name": "Sales", "source": {"kind": "csv", "text": SALES}, "steps": ["PromoteHeaders"], "load": "model"}));
    let d = s.doc().expect("doc");
    assert_eq!(d.wb.sheets.len(), 1, "the workbook still has only its own sheet");
    assert_eq!(d.wb.model.tables.len(), 1);
}

#[test]
fn a_measure_evaluates_with_and_without_a_dimension_filter() {
    let mut s = model_session();
    let all = run(&mut s, "data.measure.evaluate", json!({"name": "Total"}));
    assert_eq!(all["value"], 22.0);
    // The filter names a dimension column; it reaches the fact rows through the relationship.
    let west = run(&mut s, "data.measure.evaluate", json!({"name": "Total", "filters": [{"table": "Region", "column": "Name", "value": "West"}]}));
    assert_eq!(west["value"], 17.0);
    // And a filter straight on the fact table works too.
    let direct = run(&mut s, "data.measure.evaluate", json!({"name": "Total", "filters": [{"table": "Sales", "column": "RegionId", "value": 2}]}));
    assert_eq!(direct["value"], 5.0);
}

#[test]
fn the_relationships_and_measures_are_listed() {
    let mut s = model_session();
    let rels = run(&mut s, "data.relationship.list", json!({}));
    assert_eq!(rels[0]["from_table"], "Sales");
    assert_eq!(rels[0]["to_column"], "Id");
    let ms = run(&mut s, "data.measure.list", json!({}));
    assert_eq!(ms[0]["name"], "Total");
    assert_eq!(ms[0]["formula"], "SUM(Sales[Amount])");
}

#[test]
fn a_relationship_that_would_loop_is_refused() {
    let mut s = model_session();
    let err = s
        .execute("data.relationship.add", json!({"from": {"table": "Region", "column": "Id"}, "to": {"table": "Sales", "column": "RegionId"}}))
        .expect_err("the other direction would close the loop");
    assert!(err.to_string().contains("loop"), "{err}");
    // A duplicate is refused too, and says so.
    let err = s
        .execute("data.relationship.add", json!({"from": {"table": "Sales", "column": "RegionId"}, "to": {"table": "Region", "column": "Id"}}))
        .expect_err("the same relationship twice");
    assert!(err.to_string().contains("already exists"), "{err}");
}

#[test]
fn a_measure_that_does_not_compute_is_refused_before_it_is_kept() {
    let mut s = model_session();
    // A table the model does not have: the formula would never compute, so it is refused now.
    let err = s
        .execute("data.measure.add", json!({"name": "Broken", "formula": "SUM(Nowhere[Amount])"}))
        .expect_err("a formula naming a missing table must not be kept");
    assert!(err.to_string().contains("Nowhere"), "{err}");
    let list = run(&mut s, "data.measure.list", json!({}));
    assert_eq!(list.as_array().map(Vec::len), Some(1), "only Total is there");

    // An edit that breaks the formula is rolled back, so the measure still evaluates.
    let err = s.execute("data.measure.set", json!({"name": "Total", "formula": "SUM(Sales[Nope])"})).expect_err("no such column");
    assert!(err.to_string().contains("Nope"), "{err}");
    let v = run(&mut s, "data.measure.evaluate", json!({"name": "Total"}));
    assert_eq!(v["value"], 22.0, "the old formula still runs");
}

#[test]
fn a_measure_may_use_another_measure_and_a_missing_name_says_which() {
    let mut s = model_session();
    run(&mut s, "data.measure.add", json!({"name": "Doubled", "formula": "Total * 2"}));
    let v = run(&mut s, "data.measure.evaluate", json!({"name": "Doubled"}));
    assert_eq!(v["value"], 44.0);
    let err = s.execute("data.measure.add", json!({"name": "Bad", "formula": "Total + Missing"})).expect_err("no such measure");
    assert!(err.to_string().contains("Missing"), "{err}");
}

#[test]
fn a_measure_in_a_cell_recalcs_when_the_model_refreshes() {
    let mut s = model_session();
    run(&mut s, "range.setValues", json!({"range": "Sheet1!A1", "values": [["=MEASURE(\"Total\")"]]}));
    let d = s.doc().expect("doc");
    let sh = d.wb.sheet(0).expect("sheet");
    assert_eq!(sh.value(gridcraft_core::CellRef::new(0, 0)), gridcraft_core::Value::number(22.0));
    // The source file changed: a fourth row, and the query refilled from it.
    run(&mut s, "data.query.setSource", json!({"name": "Sales", "source": {"kind": "csv", "text": "RegionId,Amount\n1,10\n2,5\n1,7\n2,8\n"}}));
    let d = s.doc().expect("doc");
    let sh = d.wb.sheet(0).expect("sheet");
    let v = sh.value(gridcraft_core::CellRef::new(0, 0));
    assert_eq!(v, gridcraft_core::Value::number(30.0), "the cell follows the model, not a stale total");
}

#[test]
fn the_model_survives_save_and_reopen() {
    let mut s = model_session();
    run(&mut s, "range.setValues", json!({"range": "Sheet1!A1", "values": [["=MEASURE(\"Total\", \"Region[Name]\", \"East\")"]]}));
    let path = std::env::temp_dir().join("gridcraft_model_roundtrip.xlsx");
    let p = path.to_string_lossy().to_string();
    run(&mut s, "file.saveAs", json!({"path": p}));

    let mut back = Session::new();
    run(&mut back, "file.open", json!({"path": p}));
    let d = back.doc().expect("doc");
    assert_eq!(d.wb.model.tables.len(), 2, "both model tables came back");
    assert_eq!(d.wb.model.relationships.len(), 1);
    assert_eq!(d.wb.model.measures.len(), 1);
    // The rows are the cache, so the numbers are there before anything is refreshed.
    let east = run(&mut back, "data.measure.evaluate", json!({"name": "Total", "filters": [{"table": "Region", "column": "Name", "value": "East"}]}));
    assert_eq!(east["value"], 5.0);
    // And the cell that reads the measure recalculated on open.
    let d = back.doc().expect("doc");
    assert_eq!(d.wb.sheet(0).expect("sheet").value(gridcraft_core::CellRef::new(0, 0)), gridcraft_core::Value::number(5.0));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_model_pivot_survives_save_and_reopen_and_recalcs_after_a_refresh() {
    // The N3 exit criterion, measured: a pipeline (two CSVs) with a relationship and a measure,
    // read by a PivotTable, saved, reopened, refreshed after the source changed.
    let mut s = model_session();
    run(&mut s, "insert.pivotTable", json!({"source": "Sales", "destination": "Sheet1!E3", "rows": ["Region[Name]"], "values": ["Total"]}));
    let at = |s: &Session, a: &str| s.doc().expect("doc").wb.sheet(0).expect("sheet").value(gridcraft_core::CellRef::parse(a).expect("ref"));
    // Labels sort: East is the first item row, West the second.
    assert_eq!(at(&s, "F4"), gridcraft_core::Value::number(5.0), "East");
    assert_eq!(at(&s, "F5"), gridcraft_core::Value::number(17.0), "West");
    assert_eq!(at(&s, "F6"), gridcraft_core::Value::number(22.0), "grand total");

    let path = std::env::temp_dir().join("gridcraft_model_pivot_roundtrip.xlsx");
    let p = path.to_string_lossy().to_string();
    run(&mut s, "file.saveAs", json!({"path": p}));

    let mut back = Session::new();
    run(&mut back, "file.open", json!({"path": p}));
    let d = back.doc().expect("doc");
    assert_eq!(d.wb.model.tables.len(), 2);
    assert_eq!(d.wb.model.measures.len(), 1);
    assert_eq!(at(&back, "F6"), gridcraft_core::Value::number(22.0), "the report came back with its numbers");

    // The source file changed: the query refills the model, and a refresh recomputes the report.
    run(&mut back, "data.query.setSource", json!({"name": "Sales", "source": {"kind": "csv", "text": "RegionId,Amount\n1,10\n2,5\n1,7\n2,8\n"}}));
    let name = back.doc().expect("doc").wb.sheet(0).and_then(|sh| sh.pivots.first().map(|p| p.name.clone())).expect("the report came back");
    run(&mut back, "pivot.refresh", json!({"pivot": name}));
    assert_eq!(at(&back, "F6"), gridcraft_core::Value::number(30.0), "the grand total follows the model");
    assert_eq!(at(&back, "F5"), gridcraft_core::Value::number(17.0), "West is unchanged");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn deleting_the_query_takes_its_model_table_and_relationships() {
    let mut s = model_session();
    run(&mut s, "data.query.delete", json!({"name": "Sales"}));
    let tables = run(&mut s, "data.model.tables", json!({}));
    assert_eq!(tables.as_array().map(Vec::len), Some(1), "only Region is left");
    let rels = run(&mut s, "data.relationship.list", json!({}));
    assert_eq!(rels.as_array().map(Vec::len), Some(0), "its relationship went with it");
    // A measure that used the table stays: dropping a person's formula because a table moved
    // would lose their work.
    assert_eq!(run(&mut s, "data.measure.list", json!({})).as_array().map(Vec::len), Some(1));
}

#[test]
fn every_model_command_survives_empty_params() {
    for id in [
        "data.model.tables",
        "data.model.removeTable",
        "data.relationship.add",
        "data.relationship.remove",
        "data.relationship.list",
        "data.measure.add",
        "data.measure.set",
        "data.measure.remove",
        "data.measure.list",
        "data.measure.evaluate",
    ] {
        let mut s = session();
        let _ = s.execute(id, json!({}));
    }
}
