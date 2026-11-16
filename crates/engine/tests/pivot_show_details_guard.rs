//! Show Details refuses a report that no longer matches its source (adoption of upstream's
//! `check_current`): the report's cells must still be what the definition computes, or the
//! records handed back would be another cell's.

use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

fn run(s: &mut Session, command: &str, params: Json) -> Json {
    s.execute(command, params).unwrap()
}

/// A source list on `Sheet1` and a PivotTable over it on `Report`, its corner at `A3`.
fn drill_session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    run(
        &mut s,
        "range.setValues",
        json!({"range": "A1", "values": [
            ["Region", "Month", "Product", "Sales"],
            ["East", "Jan", "Apples", 100.0],
            ["East", "Feb", "Pears", 150.0],
            ["West", "Jan", "Apples", 200.0],
            ["West", "Feb", "Pears", 250.0],
        ]}),
    );
    run(&mut s, "selection.set", json!({"range": "A1"}));
    run(&mut s, "home.insertSheet", json!({"name": "Report"}));
    run(
        &mut s,
        "insert.pivotTable",
        json!({"source": "Sheet1!A1:D5", "destination": "Report!A3", "name": "P1", "rows": ["Region"], "columns": ["Month"], "values": ["Sales"]}),
    );
    s
}

fn details(s: &mut Session) -> Result<Json, String> {
    s.execute("pivot.showDetails", json!({"pivot": "P1", "cell": "B5"})).map_err(|e| e.to_string())
}

#[test]
fn show_details_works_on_a_fresh_report() {
    let mut s = drill_session();
    // East × Feb is 150: one record, its own.
    let r = details(&mut s).unwrap();
    assert_eq!(r["name"], "East - Feb");
    assert_eq!(r["rows"], 1);
    assert_eq!(r["skipped"], 0);
}

#[test]
fn show_details_refuses_a_report_edited_after_its_refresh() {
    let mut s = drill_session();
    // Fresh: the cell is the report's own value.
    assert_eq!(details(&mut s).unwrap()["rows"], 1);
    // A hand edit of a report cell (Excel keeps its cache; without one the report is the cache).
    run(&mut s, "sheet.activate", json!({"sheet": "Report"}));
    run(&mut s, "cell.set", json!({"cell": "B5", "input": 7}));
    let e = details(&mut s).unwrap_err();
    assert!(e.contains("no longer matches its source"), "{e}");
    assert!(e.contains("Refresh the PivotTable"), "{e}");
    // A hand edit anywhere else in the report refuses too, not only in the clicked cell.
    run(&mut s, "cell.set", json!({"cell": "B5", "input": 150}));
    run(&mut s, "cell.set", json!({"cell": "A5", "input": "Northeast"}));
    let e = details(&mut s).unwrap_err();
    assert!(e.contains("no longer matches its source"), "{e}");
}

#[test]
fn a_refresh_makes_show_details_work_again() {
    let mut s = drill_session();
    run(&mut s, "sheet.activate", json!({"sheet": "Report"}));
    run(&mut s, "cell.set", json!({"cell": "B5", "input": 7}));
    assert!(details(&mut s).is_err());
    run(&mut s, "pivot.refresh", json!({"pivot": "P1"}));
    let r = details(&mut s).unwrap();
    assert_eq!(r["name"], "East - Feb");
    assert_eq!(r["rows"], 1);
}
