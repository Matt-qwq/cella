//! The model's SQL engine against the pivot's own path, at the model's size limit.
//!
//! ```text
//! cargo run -p cella-engine --example model_sql_probe --release --features sql-engine
//! ```
//!
//! It builds a million-row CSV, loads it into the data model through a query (the way a person
//! does it), then asks the same 768-cell report of two engines: `data.model.query` (SQL over the
//! model) and `insert.pivotTable` + `pivot.refresh` (our own grouping). The first SQL query also
//! pays the one-time cost of handing the model's rows to the engine, which is printed separately
//! because it is paid once and not on every drag.
//!
//! `ROWS` overrides the row count (the model is capped at 4M cells, so 1M rows × 4 columns is the
//! largest it takes today).

#![allow(clippy::disallowed_methods)] // native-only probe; the clock is the measurement

use std::fmt::Write as _;
use std::time::Instant;

use cella_engine::Session;
use serde_json::json;

fn run(s: &mut Session, cmd: &str, params: serde_json::Value) -> serde_json::Value {
    s.execute(cmd, params).unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

fn best<F: FnMut() -> usize>(passes: usize, mut f: F) -> (f64, usize) {
    let mut fastest = f64::MAX;
    let mut rows = 0;
    for _ in 0..passes {
        let t = Instant::now();
        rows = f();
        fastest = fastest.min(t.elapsed().as_secs_f64());
    }
    (fastest, rows)
}

fn main() {
    let rows: usize = std::env::var("ROWS").ok().and_then(|v| v.parse().ok()).unwrap_or(1_000_000);
    println!("=== a {rows}-row model table, 4 columns ===");

    // ---- the source a person would point Cella at
    let path = std::env::temp_dir().join(format!("cella-model-sql-{rows}.csv"));
    let t = Instant::now();
    {
        let mut csv = String::with_capacity(rows * 24);
        csv.push_str("Region,Month,Product,Amount\n");
        for i in 0..rows {
            let region = i % 64;
            let month = (i / 64) % 12;
            let product = (i / 768) % 1000;
            let amount = (i % 9973) as f64 * 0.01 + 1.0;
            let _ = writeln!(csv, "Region {region},{month},{product},{amount:.2}");
        }
        std::fs::write(&path, csv).expect("write the csv");
    }
    println!("wrote the CSV in {:.2} s", t.elapsed().as_secs_f64());

    let mut s = Session::new();
    s.new_workbook();

    // ---- into the model, through a query (this is the slow, one-time way in)
    let t = Instant::now();
    run(
        &mut s,
        "data.query.add",
        json!({
            "name": "Sales",
            "source": {"kind": "csv", "path": path.display().to_string()},
            "steps": [
                "PromoteHeaders",
                {"ChangeType": {"columns": ["Month", "Product"], "to": "WholeNumber"}},
                {"ChangeType": {"columns": ["Amount"], "to": "Number"}},
            ],
            "load": "model",
        }),
    );
    let load = t.elapsed().as_secs_f64();
    println!("query -> model: {:.2} s ({:.0} ns a cell)", load, load * 1e9 / (rows * 4) as f64);

    // ---- the first SQL query also hands the rows to the engine
    let t = Instant::now();
    let answer = run(&mut s, "data.model.query", json!({"sql": "SELECT COUNT(*) AS n FROM Sales"}));
    let handover = t.elapsed().as_secs_f64();
    println!("first SQL query (model -> engine included): {:.2} s -> {} rows", handover, answer["rows"][0][0]);

    // ---- the queries a pivot and a slicer run, in the engine
    let report = "SELECT Region, Month, SUM(Amount) AS amount, COUNT(*) AS n FROM Sales GROUP BY 1, 2 ORDER BY 1, 2";
    let slicer = "SELECT Region, Month, SUM(Amount) AS amount FROM Sales WHERE Region IN ('Region 1','Region 7','Region 23','Region 40') GROUP BY 1, 2 ORDER BY 1, 2";
    for (label, sql) in [("group-by 64 x 12", report), ("slicer: 4 of 64 regions", slicer)] {
        let (seconds, cells) = best(5, || {
            let answer = run(&mut s, "data.model.query", json!({"sql": sql}));
            answer["rows"].as_array().map_or(0, Vec::len)
        });
        println!("  SQL {label}: {:.1} ms ({cells} rows)", seconds * 1e3);
    }

    // ---- the same report through our own pivot engine, over the same model table
    let t = Instant::now();
    let inserted = run(
        &mut s,
        "insert.pivotTable",
        json!({
            "source": "Sales",
            "rows": ["Sales[Region]"],
            "columns": ["Sales[Month]"],
            "values": ["Sales[Amount]"],
            "destination": "new",
        }),
    );
    println!("insert.pivotTable over the model: {:.2} s", t.elapsed().as_secs_f64());
    let name = inserted["name"].as_str().unwrap_or("").to_string();
    let (seconds, _) = best(3, || {
        run(&mut s, "pivot.refresh", json!({"pivot": name, "replace": true}));
        0
    });
    println!("  our pivot refresh (grouping included): {:.1} ms", seconds * 1e3);

    // What is left of the refresh: the report itself (the engine's answer laid out as cells)
    // against writing that report onto the sheet.
    let engine = s.model_sql_for();
    let d = s.doc().expect("a document");
    let at = d.wb.sheets.iter().enumerate().find_map(|(si, sh)| sh.pivots.iter().position(|p| p.name == name).map(|pi| (si, pi)));
    if let Some((si, pi)) = at {
        let pt = d.wb.sheet(si).and_then(|sh| sh.pivots.get(pi)).expect("the report").clone();
        let t = Instant::now();
        let (out, _src, used) = cella_engine::pivot::compute_detailed(&d.wb, &pt, si, engine.as_deref()).expect("the report");
        println!("  the report alone: {:.1} ms (engine: {used}; {} rows x {} columns)", t.elapsed().as_secs_f64() * 1e3, out.grid.len(), out.width);
    }

    // A keystroke on a workbook whose model holds a million rows: every command copies the
    // workbook for its undo entry, and the model's rows travel with it unless they are shared.
    let t = Instant::now();
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "1"}));
    println!("  one cell edit (the workbook is copied for undo): {:.1} ms", t.elapsed().as_secs_f64() * 1e3);

    let _ = std::fs::remove_file(&path);
}
