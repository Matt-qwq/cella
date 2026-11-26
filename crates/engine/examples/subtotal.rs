//! Prices `data.subtotal` — what the report costs, and what one row insert per group used to
//! cost (gaps §25).
//!
//! The command found every group boundary before it inserted a row for it, and it inserted them
//! one at a time with `home.insertRows`: every group paid a copy of the workbook, a rewrite of
//! every formula and a full recalculation of the sheet, so the report was Θ(groups × rows) where
//! it should be Θ(rows). This probe prices the pieces — one structural insert at the top of a
//! million rows, and the command itself over sorted data.
//!
//! ```text
//! cargo run --release -p cella-engine --example subtotal -- [rows] [groups]
//! ```

#![allow(clippy::disallowed_methods)] // a native-only benchmark: wasm never runs it

use std::time::Instant;

use cella_engine::Session;
use serde_json::json;

/// A sheet of `rows` rows in two columns: a sorted group column (one run a group) and a value.
fn build(rows: u32, groups: u32) -> Session {
    let mut s = Session::new();
    s.new_workbook();
    let per = (rows / groups.max(1)).max(1);
    let chunk = 10_000u32;
    let mut r = 0u32;
    while r < rows {
        let n = chunk.min(rows - r);
        let vals: Vec<Vec<f64>> = (0..n).map(|i| vec![f64::from((r + i) / per), f64::from((r + i) % 997)]).collect();
        s.execute("range.setValues", json!({"range": format!("A{}", r + 1), "values": vals})).expect("set");
        r += n;
    }
    s
}

fn main() {
    let rows: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(1_000_000);
    let groups: u32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(100);

    // One structural edit at the top: what a group used to pay on its own.
    let mut s = build(rows, groups.max(1));
    let t = Instant::now();
    s.execute("home.insertRows", json!({"rows": "2:2"})).expect("insert");
    println!("one home.insertRows at row 2 over {rows} rows: {:?}", t.elapsed());

    // The report itself, on a sheet that has not been edited.
    for (rows, groups) in [(100_000u32, 4u32), (1_000_000, 4), (rows, groups)] {
        let mut s = build(rows, groups);
        let t = Instant::now();
        let out = s.execute("data.subtotal", json!({"range": format!("A1:B{rows}"), "groupBy": "A", "function": "sum", "columns": ["B"]}));
        match out {
            Ok(v) => println!("data.subtotal {rows} rows, {groups} groups: {:?} -> {v}", t.elapsed()),
            Err(e) => println!("data.subtotal {rows} rows, {groups} groups failed after {:?}: {e}", t.elapsed()),
        }
    }
}
