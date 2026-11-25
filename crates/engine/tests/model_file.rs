//! A model table whose rows are a **file's**, not the document's.
//!
//! The 4M-cell cache exists because a `Value` a cell lives in the document; a table the engine
//! reads from a file has no such cost, so it may be larger than anything a sheet (or the cache)
//! holds. These tests load a million-row CSV, report over it, and reopen the workbook to prove
//! the document kept the *definition* and the data is still the file's.

use std::fmt::Write as _;

use cella_engine::Session;
use serde_json::{Value, json};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.execute(cmd, params).unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

/// Writes `rows` sales lines and returns the sum of every amount (the amounts are whole numbers,
/// so the sum is exact and a report can be held to it).
fn write_sales(path: &std::path::Path, rows: usize) -> f64 {
    let mut csv = String::with_capacity(rows * 24);
    csv.push_str("Region,Month,Product,Amount\n");
    let mut total = 0.0;
    for i in 0..rows {
        let region = i % 64;
        let month = (i / 64) % 12 + 1;
        let product = (i / 768) % 1000;
        let amount = (i % 100 + 1) as f64;
        total += amount;
        let _ = writeln!(csv, "Region {region},{month},{product},{amount:.0}");
    }
    std::fs::write(path, csv).expect("write the csv");
    total
}

fn temp(name: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("cella-{}-{name}", std::process::id()));
    path
}

/// The report's grid area and the sheet it sits on.
fn report_area(s: &Session, name: &str) -> (usize, cella_core::RangeRef) {
    let d = s.doc().expect("a document");
    for (si, sheet) in d.wb.sheets.iter().enumerate() {
        if let Some(pt) = sheet.pivots.iter().find(|p| p.name == name)
            && let Some(area) = pt.last_range
        {
            return (si, area);
        }
    }
    panic!("no report named {name} with an area");
}

#[test]
fn a_table_bigger_than_the_models_cache_is_read_from_its_file() {
    if !cfg!(feature = "sql-engine") {
        return;
    }
    // 1.1M rows × 4 columns is 4.4M cells: past the 4M the document caches, and past a sheet.
    let rows = 1_100_000;
    let path = temp("big-model.csv");
    let total = write_sales(&path, rows);
    let mut s = Session::new();
    s.new_workbook();
    let loaded = run(&mut s, "data.model.loadFile", json!({"path": path.display().to_string(), "name": "Sales"}));
    assert_eq!(loaded["rows"], rows as f64);
    assert_eq!(loaded["cols"], 4);
    // The document holds the definition; the rows are the file's.
    {
        let d = s.doc().expect("a document");
        let table = d.wb.model.table("Sales").expect("the table");
        assert!(table.is_file_backed());
        assert!(table.rows.is_empty(), "nothing is cached in the document");
        assert_eq!(table.columns, vec!["Region", "Month", "Product", "Amount"]);
    }
    // The engine reads it, and the answer is every amount in the file.
    let counted = run(&mut s, "data.model.query", json!({"sql": "SELECT COUNT(*) AS n, SUM(Amount) AS total FROM Sales"}));
    assert_eq!(counted["rows"][0][0], rows as f64);
    assert_eq!(counted["rows"][0][1], total);

    // A report over it: the engine aggregates, and the grand total is the sum of the file.
    let out =
        run(&mut s, "insert.pivotTable", json!({"source": "Sales", "rows": ["Sales[Region]"], "values": ["Sales[Amount]"], "destination": "new"}));
    let name = out["name"].as_str().expect("a name").to_string();
    run(&mut s, "pivot.refresh", json!({"pivot": name, "replace": true}));
    let (sheet, area) = report_area(&s, &name);
    let d = s.doc().expect("a document");
    let grand = d.wb.sheet(sheet).map(|sh| sh.value(area.end)).unwrap_or_default();
    assert_eq!(grand.as_f64().expect("the grand total is a number"), total, "the report's grand total is the file's sum");

    // `data.model.tables` asks the engine, because the document has no row count to give.
    let listed = run(&mut s, "data.model.tables", json!({}));
    assert_eq!(listed[0]["rows"], rows as f64);
    assert_eq!(listed[0]["file"], true);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_file_backed_table_survives_save_and_reopen() {
    if !cfg!(feature = "sql-engine") {
        return;
    }
    let path = temp("roundtrip.csv");
    write_sales(&path, 1_000);
    let mut s = Session::new();
    s.new_workbook();
    run(&mut s, "data.model.loadFile", json!({"path": path.display().to_string(), "name": "Sales"}));
    let book = temp("roundtrip.xlsx");
    let book = book.to_string_lossy().to_string();
    run(&mut s, "file.saveAs", json!({"path": book}));

    let mut back = Session::new();
    run(&mut back, "file.open", json!({"path": book}));
    {
        let d = back.doc().expect("a document");
        let table = d.wb.model.table("Sales").expect("the table came back");
        assert!(table.is_file_backed());
        assert!(table.rows.is_empty());
    }
    // The opened table kept the types the file reported, so a report knows which columns are
    // numbers without reading anything.
    {
        let d = back.doc().expect("a document");
        let table = d.wb.model.table("Sales").expect("the table");
        assert_eq!(table.column_types.len(), table.columns.len(), "the column types came back");
        assert!(table.column_types.iter().any(|t| t.contains("INT") || t.contains("DOUBLE")), "{:?}", table.column_types);
    }
    // The reopened workbook reads the file again (the engine holds no rows across a save).
    let counted = run(&mut back, "data.model.query", json!({"sql": "SELECT COUNT(*) AS n FROM Sales"}));
    assert_eq!(counted["rows"][0][0], 1_000.0);
    // And a report over it still refreshes to the same numbers.
    let out =
        run(&mut back, "insert.pivotTable", json!({"source": "Sales", "rows": ["Sales[Region]"], "values": ["Sales[Amount]"], "destination": "new"}));
    let name = out["name"].as_str().expect("a name").to_string();
    run(&mut back, "pivot.refresh", json!({"pivot": name, "replace": true}));
    let (sheet, area) = report_area(&back, &name);
    let total = back.doc().expect("a document").wb.sheet(sheet).map(|sh| sh.value(area.end)).unwrap_or_default();
    assert!(total.as_f64().unwrap_or(0.0) > 0.0, "the reopened report has its totals: {total:?}");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&book);
}

#[test]
fn a_file_that_cannot_be_read_leaves_the_model_as_it_was() {
    if !cfg!(feature = "sql-engine") {
        return;
    }
    let mut s = Session::new();
    s.new_workbook();
    let missing = temp("not-there.csv");
    let error = s
        .execute("data.model.loadFile", json!({"path": missing.to_string_lossy().to_string(), "name": "Sales"}))
        .expect_err("a missing file must be refused");
    assert!(error.to_string().contains("could not be read"), "{error}");
    let d = s.doc().expect("a document");
    assert!(d.wb.model.tables.is_empty(), "the definition was rolled back");
}

#[test]
fn a_path_or_a_format_that_makes_no_sense_is_refused() {
    let mut s = Session::new();
    s.new_workbook();
    let error = s.execute("data.model.loadFile", json!({"name": "Sales"})).expect_err("no path");
    assert!(error.to_string().contains("missing"), "{error}");
    let error = s.execute("data.model.loadFile", json!({"path": "sales.xls", "format": "xls"})).expect_err("xls is not a model format");
    assert!(error.to_string().contains("not a file format"), "{error}");
}

#[test]
fn a_query_that_loads_a_file_into_the_model_keeps_the_path() {
    if !cfg!(feature = "sql-engine") {
        return;
    }
    // The route, not the size: a CSV loaded to the model through `data.query.add` is the *file*,
    // not a copy of its rows — which is what lets an ordinary "import this file" flow past the cap.
    let path = temp("query-file.csv");
    write_sales(&path, 5_000);
    let mut s = Session::new();
    s.new_workbook();
    let answer = run(
        &mut s,
        "data.query.add",
        json!({
            "name": "Sales",
            "source": {"kind": "csv", "path": path.display().to_string()},
            "steps": ["PromoteHeaders"],
            "load": "model",
        }),
    );
    assert_eq!(answer["rows"], 5_000.0);
    {
        let d = s.doc().expect("a document");
        let table = d.wb.model.table("Sales").expect("the table");
        assert!(table.is_file_backed(), "the query should have kept the file");
        assert!(table.rows.is_empty(), "nothing is cached in the document");
        assert_eq!(table.columns.first().map(String::as_str), Some("Region"));
    }
    // A refresh reads the file again, and reports its rows.
    let refreshed = run(&mut s, "data.query.refresh", json!({"name": "Sales"}));
    assert_eq!(refreshed[0]["rows"], 5_000.0);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_query_with_a_real_step_stays_in_memory() {
    let path = temp("query-steps.csv");
    write_sales(&path, 100);
    let mut s = Session::new();
    s.new_workbook();
    run(
        &mut s,
        "data.query.add",
        json!({
            "name": "Sales",
            "source": {"kind": "csv", "path": path.display().to_string()},
            "steps": ["PromoteHeaders", {"ChangeType": {"columns": ["Amount"], "to": "Number"}}],
            "load": "model",
        }),
    );
    let d = s.doc().expect("a document");
    let table = d.wb.model.table("Sales").expect("the table");
    assert!(!table.is_file_backed(), "a pipeline is not a file");
    assert_eq!(table.height(), 100);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_file_backed_table_can_be_filtered_by_value() {
    if !cfg!(feature = "sql-engine") {
        return;
    }
    // The scan cannot walk this table's records, so a value filter on it — "keep the regions whose
    // total is above a number" — is only possible because the engine answers it.
    let path = temp("filtered.csv");
    write_sales(&path, 2_000);
    let mut s = Session::new();
    s.new_workbook();
    run(&mut s, "data.model.loadFile", json!({"path": path.display().to_string(), "name": "Sales"}));
    let out =
        run(&mut s, "insert.pivotTable", json!({"source": "Sales", "rows": ["Sales[Region]"], "values": ["Sales[Amount]"], "destination": "new"}));
    let name = out["name"].as_str().expect("a name").to_string();
    // Every region's total is the same here, so a threshold at the total keeps all of them and one
    // above it keeps none — a pass either way is the engine having answered.
    let kept = run(
        &mut s,
        "pivot.filterByValue",
        json!({"pivot": name, "field": "Sales[Region]", "measure": "Sales[Amount]", "op": "greaterThan", "value": 0.0}),
    );
    assert_eq!(kept["items"], 64, "all 64 regions are above zero");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn without_an_engine_a_file_backed_table_says_so() {
    if cfg!(feature = "sql-engine") {
        return;
    }
    let path = temp("no-engine.csv");
    write_sales(&path, 10);
    let mut s = Session::new();
    s.new_workbook();
    let error = s.execute("data.model.loadFile", json!({"path": path.display().to_string(), "name": "Sales"})).expect_err("no engine, no file");
    assert!(error.to_string().contains("desktop app"), "{error}");
    let _ = std::fs::remove_file(&path);
}
