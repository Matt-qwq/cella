//! Paste a `rows × cols` block of values with `edit.copy`/`edit.paste` and report the paste.
//!
//! ```text
//! cargo run --release -p cella-engine --example paste_bench -- --rows=1000000 --cols=6
//! CELLA_PROFILE=1 cargo run --release -p cella-engine --example paste_bench   # phases
//! ```
//!
//! The setup (building the block with `range.setValues`, the copy) is outside the number: an
//! append-style paste into six empty columns is the shape the P4 trigger measured (6M cells,
//! 2.13–2.32 s at 0.36 µs/cell). The last line is `paste: <ms> ms (<cells> cells, <ns> ns/cell)`,
//! which is what `bench/ab_gate.ps1`-style paired runs read. `CELLA_PROFILE=1` adds the coarse
//! phase split the paste command prints (cells built against the store write).

use std::process::ExitCode;
use std::time::Instant;

use cella_engine::Session;
use serde_json::{Value, json};

/// Rows per `range.setValues` call when the source block is built.
const CHUNK: usize = 10_000;
/// Largest row count this harness accepts — an argument-derived size, so it is capped.
const MAX_ROWS: usize = 1_000_000;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("paste_bench: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut rows = MAX_ROWS;
    let mut cols = 6usize;
    for arg in std::env::args().skip(1) {
        match arg.split_once('=') {
            Some(("--rows", v)) => rows = v.parse().ok().filter(|n: &usize| (3..=MAX_ROWS).contains(n)).ok_or("--rows must be 3..=1000000")?,
            Some(("--cols", v)) => cols = v.parse().ok().filter(|n: &usize| (1..=16).contains(n)).ok_or("--cols must be 1..=16")?,
            _ => return Err(format!("unknown argument `{arg}` (usage: paste_bench [--rows=N] [--cols=N])")),
        }
    }
    let mut s = Session::new();
    s.new_workbook();
    let start = Instant::now();
    for row in (0..rows).step_by(CHUNK) {
        let end = (row + CHUNK).min(rows);
        let values: Vec<Value> = (row..end).map(|n| json!(source_row(n, cols))).collect();
        s.execute("range.setValues", json!({"range": format!("A{}", row + 1), "values": values})).map_err(|e| e.to_string())?;
    }
    eprintln!("setup {rows} rows x {cols}: {:.0} ms", start.elapsed().as_secs_f64() * 1000.0);
    let last_col = column_name(cols - 1);
    let start = Instant::now();
    s.execute("edit.copy", json!({"range": format!("A1:{last_col}{rows}")})).map_err(|e| e.to_string())?;
    eprintln!("copy: {:.1} ms", start.elapsed().as_secs_f64() * 1000.0);
    let at = format!("{}1", column_name(cols + 2));
    s.execute("selection.set", json!({"range": at})).map_err(|e| e.to_string())?;
    let start = Instant::now();
    s.execute("edit.paste", json!({"at": at})).map_err(|e| e.to_string())?;
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    let cells = (rows as f64) * (cols as f64);
    eprintln!("paste: {ms:.1} ms ({} cells, {:.0} ns/cell)", cells as usize, ms * 1e6 / cells);
    Ok(())
}

/// One source row: a text key, a count, a modulus, a half-step, and constants, so a paste has to
/// carry every kind of scalar.
fn source_row(n: usize, cols: usize) -> Vec<Value> {
    (0..cols)
        .map(|c| match c % 6 {
            0 => json!(format!("Item {n}")),
            1 => json!(n as f64),
            2 => json!((n % 97) as f64),
            3 => json!((n % 13) as f64 * 1.5),
            4 => json!(1.0),
            _ => json!(2.0),
        })
        .collect()
}

/// A zero-based column index as A1 letters.
fn column_name(mut col: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (col % 26) as u8);
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}
