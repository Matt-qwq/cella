//! Get & Transform end to end: a source, a step pipeline, a loaded table — and what happens when
//! a step does not fit, a file moves, or the workbook is saved and reopened.

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

/// The cells of a sheet, rows of display text, for an exact comparison.
fn grid(s: &Session, sheet: &str) -> Vec<Vec<String>> {
    let d = s.doc().expect("doc");
    let sh = d.wb.sheet_index(sheet).and_then(|i| d.wb.sheet(i)).unwrap_or_else(|| panic!("no sheet {sheet}"));
    let Some(u) = sh.used_range() else { return Vec::new() };
    (u.start.row..=u.end.row).map(|r| (u.start.col..=u.end.col).map(|c| sh.value(gridcraft_core::CellRef::new(r, c)).display()).collect()).collect()
}

/// The cells of a table object's own range, rows of display text: a table's shape is its range,
/// which is what a refresh has to keep true (a cleared cell that is still in the store is not the
/// table's business).
fn table_grid(s: &Session, table: &str) -> Vec<Vec<String>> {
    let d = s.doc().expect("doc");
    let (si, ti) = d.wb.table(table).expect("table");
    let sh = d.wb.sheet(si).expect("sheet");
    let r = sh.tables.get(ti).expect("table").range;
    (r.start.row..=r.end.row)
        .map(|row| (r.start.col..=r.end.col).map(|col| sh.value(gridcraft_core::CellRef::new(row, col)).display()).collect())
        .collect()
}

const CSV: &str = "Region,Amount,Day\nWest,10,2024-03-04\nEast,5,2024-03-05\nWest,7,2024-03-06\n";

#[test]
fn a_csv_query_loads_a_table_onto_its_own_sheet() {
    let mut s = session();
    let r = run(
        &mut s,
        "data.query.add",
        json!({
            "name": "Sales",
            "source": {"kind": "csv", "text": CSV},
            "steps": ["PromoteHeaders", {"ChangeType": {"columns": ["Amount"], "to": "Number"}}],
        }),
    );
    assert_eq!(r["rows"], 3);
    assert_eq!(r["cols"], 3);
    assert_eq!(r["sheet"], "Sales");
    assert_eq!(
        grid(&s, "Sales"),
        vec![vec!["Region", "Amount", "Day"], vec!["West", "10", "2024-03-04"], vec!["East", "5", "2024-03-05"], vec!["West", "7", "2024-03-06"],]
    );
}

#[test]
fn the_steps_are_what_the_listing_and_the_steps_command_report() {
    let mut s = session();
    run(
        &mut s,
        "data.query.add",
        json!({
            "name": "Sales",
            "source": {"kind": "csv", "text": CSV},
            "steps": ["PromoteHeaders", {"Sort": {"keys": [["Amount", true]]}}],
        }),
    );
    let list = run(&mut s, "data.query.list", json!({}));
    assert_eq!(list[0]["name"], "Sales");
    assert_eq!(list[0]["kind"], "Csv");
    assert_eq!(list[0]["sheet"], "Sales");
    assert_eq!(list[0]["columns"], json!(["Region", "Amount", "Day"]));
    assert_eq!(list[0]["steps"], json!(["Promoted Headers", "Sorted Rows"]));
    let steps = run(&mut s, "data.query.steps", json!({"name": "Sales"}));
    assert_eq!(steps[1], json!({"Sort": {"keys": [["Amount", true]]}}));
}

#[test]
fn a_refresh_reads_the_source_again_and_rewrites_the_table_in_place() {
    let mut s = session();
    run(&mut s, "data.query.add", json!({"name": "Sales", "source": {"kind": "csv", "text": CSV}, "steps": ["PromoteHeaders"]}));
    assert_eq!(grid(&s, "Sales").len(), 4);
    // The file changed: one row fewer, and a different first value.
    let shorter = "Region,Amount\nNorth,1\nSouth,2\n";
    run(&mut s, "data.query.setSource", json!({"name": "Sales", "source": {"kind": "csv", "text": shorter}}));
    // The table's own range is the new one: a refresh that shrinks must not keep the old width.
    assert_eq!(table_grid(&s, "Sales"), vec![vec!["Region", "Amount"], vec!["North", "1"], vec!["South", "2"]]);
    // The table object followed the data: its range is the new range, not the old one.
    let d = s.doc().expect("doc");
    let sh = d.wb.sheet_index("Sales").and_then(|i| d.wb.sheet(i)).expect("sheet");
    assert_eq!(sh.tables.len(), 1);
    assert_eq!(sh.tables[0].range.a1(), "A1:B3");
}

#[test]
fn a_step_that_names_a_missing_column_is_refused_and_the_query_is_left_alone() {
    let mut s = session();
    run(&mut s, "data.query.add", json!({"name": "Sales", "source": {"kind": "csv", "text": CSV}, "steps": ["PromoteHeaders"]}));
    let err = s
        .execute("data.query.addStep", json!({"name": "Sales", "step": {"Filter": {"column": "Nope", "op": "Equals", "value": "1"}}}))
        .expect_err("a filter on a missing column must fail");
    let msg = err.to_string();
    assert!(msg.contains("step 1 (Filtered Rows)"), "{msg}");
    assert!(msg.contains("Nope"), "{msg}");
    // Nothing was kept: the pipeline is still the one that runs.
    let before = run(&mut s, "data.query.steps", json!({"name": "Sales"}));
    assert_eq!(before, json!(["PromoteHeaders"]));
    assert_eq!(grid(&s, "Sales").len(), 4);
}

#[test]
fn a_json_query_reads_objects_as_columns() {
    let mut s = session();
    let body = r#"[{"Region":"West","Amount":10},{"Region":"East","Amount":5,"Note":"late"}]"#;
    run(
        &mut s,
        "data.query.add",
        json!({"name": "Orders", "source": {"kind": "json", "text": body}, "steps": ["PromoteHeaders", {"Sort": {"keys": [["Region", false]]}}]}),
    );
    // The JSON parser returns an object's keys alphabetically, so the columns are Amount, Region,
    // Note whatever order the file wrote them in; the sort then takes the Region column, A to Z.
    assert_eq!(table_grid(&s, "Orders"), vec![vec!["Amount", "Region", "Note"], vec!["5", "East", "late"], vec!["10", "West", ""],]);
}

#[test]
fn a_workbook_table_is_a_source_that_follows_its_own_data() {
    let mut s = session();
    run(&mut s, "range.setValues", json!({"range": "A1", "values": [["Region", "Amount"], ["West", "1"], ["East", "2"]]}));
    run(&mut s, "insert.table", json!({"range": "A1:B3", "header": true}));
    run(
        &mut s,
        "data.query.add",
        json!({"name": "Copy", "source": {"kind": "table", "table": "Table1"}, "steps": [{"Sort": {"keys": [["Region", false]]}}]}),
    );
    assert_eq!(table_grid(&s, "Copy"), vec![vec!["Region", "Amount"], vec!["East", "2"], vec!["West", "1"]]);
}

#[test]
fn deleting_a_query_keeps_the_table_it_loaded() {
    let mut s = session();
    run(&mut s, "data.query.add", json!({"name": "Sales", "source": {"kind": "csv", "text": CSV}, "steps": ["PromoteHeaders"]}));
    run(&mut s, "data.query.delete", json!({"name": "Sales"}));
    assert_eq!(run(&mut s, "data.query.list", json!({})), json!([]));
    // The data is data now: the sheet and its table stay.
    assert_eq!(grid(&s, "Sales").len(), 4);
}

#[test]
fn a_query_and_its_table_survive_undo_and_redo() {
    let mut s = session();
    run(&mut s, "data.query.add", json!({"name": "Sales", "source": {"kind": "csv", "text": CSV}, "steps": ["PromoteHeaders"]}));
    s.execute("edit.undo", json!({})).expect("undo");
    assert_eq!(run(&mut s, "data.query.list", json!({})), json!([]));
    s.execute("edit.redo", json!({})).expect("redo");
    let list = run(&mut s, "data.query.list", json!({}));
    assert_eq!(list[0]["name"], "Sales");
    assert_eq!(grid(&s, "Sales").len(), 4);
}

#[test]
fn a_query_and_its_loaded_table_survive_save_and_reopen() {
    let mut s = session();
    run(&mut s, "data.query.add", json!({"name": "Sales", "source": {"kind": "csv", "text": CSV}, "steps": ["PromoteHeaders"]}));
    let path = std::env::temp_dir().join("gridcraft_query_roundtrip.xlsx");
    let p = path.to_string_lossy().to_string();
    run(&mut s, "file.saveAs", json!({"path": p}));

    let mut back = Session::new();
    run(&mut back, "file.open", json!({"path": p}));
    let list = run(&mut back, "data.query.list", json!({}));
    assert_eq!(list[0]["name"], "Sales", "the query came back with the file");
    assert_eq!(list[0]["steps"], json!(["Promoted Headers"]));
    assert_eq!(table_grid(&back, "Sales").len(), 4, "the table it loaded came back too");
    // And it still runs: the source it names is inline text, so nothing outside the file is needed.
    run(&mut back, "data.query.refresh", json!({"name": "Sales"}));
    assert_eq!(table_grid(&back, "Sales").len(), 4);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_model_load_is_refused_with_the_round_that_brings_it() {
    let mut s = session();
    let err = s
        .execute("data.query.add", json!({"name": "Sales", "source": {"kind": "csv", "text": CSV}, "load": "model"}))
        .expect_err("the data model is N3.3");
    let msg = err.to_string();
    assert!(msg.contains("N3.3"), "{msg}");
}

#[test]
fn a_missing_source_file_names_the_file() {
    let mut s = session();
    let err = s
        .execute("data.query.add", json!({"name": "Sales", "source": {"kind": "csv", "path": "does-not-exist.csv"}}))
        .expect_err("a missing file is an error");
    assert!(err.to_string().contains("does-not-exist.csv"), "{err}");
}

#[test]
fn a_rename_moves_the_query_and_the_table_it_owns() {
    let mut s = session();
    run(&mut s, "data.query.add", json!({"name": "Sales", "source": {"kind": "csv", "text": CSV}, "steps": ["PromoteHeaders"]}));
    run(&mut s, "data.query.rename", json!({"name": "Sales", "to": "Sales 2024"}));
    let list = run(&mut s, "data.query.list", json!({}));
    assert_eq!(list[0]["name"], "Sales 2024");
    // A refresh after the rename finds the table again instead of making a second one.
    run(&mut s, "data.query.refresh", json!({"name": "Sales 2024"}));
    let d = s.doc().expect("doc");
    let sh = d.wb.sheet_index("Sales").and_then(|i| d.wb.sheet(i)).expect("sheet");
    assert_eq!(sh.tables.len(), 1);
}

#[test]
fn every_query_command_survives_empty_params() {
    // Programmatic calls never open a dialog: an empty params object is an error, never a hang.
    for id in [
        "data.query.add",
        "data.query.refresh",
        "data.query.addStep",
        "data.query.setStep",
        "data.query.removeStep",
        "data.query.setSource",
        "data.query.rename",
        "data.query.delete",
        "data.query.load",
        "data.query.list",
        "data.query.steps",
    ] {
        let mut s = session();
        let _ = s.execute(id, json!({}));
    }
}
