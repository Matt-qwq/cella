//! Long-run memory drift: one session, one workbook, thousands of edit/undo/redo cycles with
//! periodic sorts and saves, sampling the engine's own memory report. A leak in the commit,
//! journal, undo or graph paths shows up as a rising total; a flat total over minutes is the
//! pass condition.
//!
//! ```text
//! cargo run --release -p cella-engine --example mem_drift -- --rows=100000 --seconds=240
//! ```

#![allow(clippy::disallowed_methods)] // native-only soak; the clock is the budget

use std::process::ExitCode;
use std::time::{Duration, Instant};

use cella_engine::Session;
use serde_json::{Value, json};

const CHUNK: usize = 10_000;
const MAX_ROWS: usize = 1_000_000;
const MAX_SECONDS: usize = 3_600;
const REPORT_EVERY: u64 = 25;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mem_drift: {e}");
            ExitCode::FAILURE
        }
    }
}

fn arg(name: &str, default: usize, max: usize) -> Result<usize, String> {
    let raw = std::env::args().find(|a| a.starts_with(name)).and_then(|a| a.split_once('=').map(|(_, v)| v.to_string()));
    match raw {
        None => Ok(default),
        Some(v) => v.parse::<usize>().map(|n| n.min(max)).map_err(|_| format!("{name} must be 0..={max}")),
    }
}

fn run() -> Result<(), String> {
    let rows = arg("--rows", 100_000, MAX_ROWS)?;
    let seconds = arg("--seconds", 240, MAX_SECONDS)? as u64;
    if rows < 100 {
        return Err("--rows must be at least 100 for the edit cycle to mean anything".into());
    }
    let mut s = Session::new();
    s.new_workbook();
    let mut r = 0;
    while r < rows {
        let n = CHUNK.min(rows - r);
        let values: Vec<Value> = (0..n)
            .map(|i| {
                let n = r + i;
                json!([format!("Item {n}"), n as f64, (n % 97) as f64, (n % 13) as f64 * 1.5, 1.0, 2.0])
            })
            .collect();
        exec(&mut s, "range.setValues", json!({"range": format!("A{}", r + 1), "values": values}))?;
        r += n;
    }
    exec(&mut s, "cell.set", json!({"cell": "G1", "input": format!("=B1*C1+D1")}))?;
    exec(&mut s, "edit.autoFill", json!({"source": "G1", "target": format!("G1:G{rows}")}))?;
    let started = Instant::now();
    let budget = Duration::from_secs(seconds);
    let mut cycle: u64 = 0;
    println!("cycle,elapsed_s,total_bytes,undo_steps,last_recalc_cells");
    let mut next_report: u64 = 1;
    while started.elapsed() < budget {
        cycle += 1;
        let v = (cycle % 9).to_string();
        exec(&mut s, "cell.set", json!({"cell": "B1", "input": v}))?;
        exec(&mut s, "edit.undo", json!({}))?;
        exec(&mut s, "edit.redo", json!({}))?;
        if cycle.is_multiple_of(50) {
            exec(&mut s, "data.sortDescending", json!({"range": format!("A1:F{rows}"), "column": "C"}))?;
            exec(&mut s, "edit.undo", json!({}))?;
        }
        if cycle.is_multiple_of(200) {
            let saved = exec(&mut s, "file.saveBytes", json!({"format": "xlsx"}))?;
            drop(saved);
        }
        if cycle >= next_report {
            next_report += REPORT_EVERY;
            let doc = s.doc().map_err(|e| e.to_string())?;
            let total = s.memory_report().total;
            let elapsed = started.elapsed().as_secs_f64();
            let undo = doc.undo.len();
            let recalc = doc.calc.last_recalc_cells;
            println!("{cycle},{elapsed:.1},{total},{undo},{recalc}");
        }
    }
    Ok(())
}

fn exec(s: &mut Session, id: &str, params: Value) -> Result<Value, String> {
    s.execute(id, params).map_err(|e| format!("{id}: {e}"))
}
