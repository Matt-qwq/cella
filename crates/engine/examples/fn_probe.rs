//! One formula family's wait, closely: the calls that take a **block argument**, and the pieces of
//! the wait they pay for.
//!
//! `cargo run --release -p cella-engine --example fn_probe -- [--rows=N] [--repeat=N] [--only=TEXT]`
//!
//! `bench_functions` prices every registered function once, which answers "is this row slow" and
//! cannot answer "which of the five things this formula does is the slow one". This probe enters the
//! same workbook the function catalogue uses (a header row and the eight-column body, filled the
//! same way) and prints a handful of calls that differ by **one thing at a time**: a range against
//! the same range written as a whole column, `SUM` against `COUNT` (the same walk, no arithmetic),
//! a scan that finds its hit on the first row against one that has to reach the end, a block read
//! that the function folds itself against one it has to materialise first.
//!
//! Each call runs in a fresh session holding a copy of the workbook (the `cell.set` that enters the
//! formula, recalculates it and takes the undo snapshot is the number), `--repeat` times, and the
//! fastest run is the one printed.
//!
//! Like the two catalogues, this is outside `cargo xtask ci`: a release build against a million
//! rows.

#![allow(clippy::disallowed_methods)] // native-only probe; the clock is the measurement

use std::process::ExitCode;
use std::time::Instant;

use cella_engine::{DocState, Session};
use serde_json::{Value as Json, json};

/// Rows measured when none are given.
const DEFAULT_ROWS: usize = 1_000_000;
/// Largest row count this probe accepts (an argument-derived size, so it is capped).
const MAX_ROWS: usize = 5_000_000;
/// Rows per `range.setValues` call when filling the workbook (the catalogue's chunk).
const CHUNK: usize = 10_000;
/// The cell the formula goes into.
const TARGET: &str = "Z1";
/// The four regions a data row carries.
const REGIONS: [&str; 4] = ["North", "South", "East", "West"];

/// The calls, each a template with `{last}` for the last data row. The pairs next to each other
/// differ by one thing, so the difference between two lines is that one thing's price.
const CALLS: &[(&str, &str)] = &[
    // The floor: a formula that does nothing, so every other line carries it.
    ("floor (=1+1)", "=1+1"),
    // The same fold, three ways: an explicit range, the same range as a whole column, and the
    // fold with no arithmetic to do.
    ("SUM(A2:A{last})", "=SUM(A2:A{last})"),
    ("SUM(A:A)", "=SUM(A:A)"),
    ("COUNT(A2:A{last})", "=COUNT(A2:A{last})"),
    ("MAX(A2:A{last})", "=MAX(A2:A{last})"),
    // A block read that the function folds itself (no array built) against one that has to build
    // the array first: `MEDIAN` needs every value in hand, `SUM` does not.
    ("MEDIAN(A2:A{last})", "=MEDIAN(A2:A{last})"),
    ("LARGE(A2:A{last},1)", "=LARGE(A2:A{last},1)"),
    // Criteria scans: one column tested, two columns read.
    ("COUNTIF(A2:A{last},\">0\")", "=COUNTIF(A2:A{last},\">0\")"),
    ("SUMIF(A2:A{last},\">0\")", "=SUMIF(A2:A{last},\">0\")"),
    ("SUMIFS(A,B,\">250\")", "=SUMIFS(A2:A{last},B2:B{last},\">250\")"),
    ("SUMPRODUCT(A,B)", "=SUMPRODUCT(A2:A{last},B2:B{last})"),
    // Lookups: the hit on row 1, the hit in the middle, a miss that reads the whole column — and a
    // key that is text, which the round-39 narrow shape answers without reading the block.
    ("MATCH(2,...) first row", "=MATCH(2,A2:A{last},0)"),
    ("MATCH(500000,...) middle", "=MATCH(500000,A2:A{last},0)"),
    ("MATCH(-1,...) miss", "=MATCH(-1,A2:A{last},0)"),
    ("VLOOKUP first row", "=VLOOKUP(2,A2:B{last},2,FALSE)"),
    ("VLOOKUP middle", "=VLOOKUP(500000,A2:B{last},2,FALSE)"),
    ("XLOOKUP middle", "=XLOOKUP(500000,A2:A{last},B2:B{last})"),
    ("XLOOKUP text key", "=XLOOKUP(\"Item 500000\",D2:D{last},A2:A{last})"),
    // The catalogue's own spelling: the defaults spelled out, so the scan has to know that
    // `match_mode 0` and `search_mode 1` are the same lookup.
    ("XLOOKUP defaults spelled", "=XLOOKUP(\"K500000\",G2:G{last},B2:B{last},1,0,1)"),
    // The LAMBDA helpers: what a call costs before the body does anything (`LAMBDA(x,1)`), what a
    // row's own array costs (`x`), and what a worksheet function in the body costs (`SUM(x)`).
    ("MAP constant", "=MAP(A2:A{last},LAMBDA(x,1))"),
    ("MAP multiply", "=MAP(A2:A{last},LAMBDA(x,x*2))"),
    ("MAP sum of one", "=MAP(A2:A{last},LAMBDA(x,SUM(x)))"),
    ("BYROW constant", "=BYROW(A2:A{last},LAMBDA(x,1))"),
    ("BYROW identity", "=BYROW(A2:A{last},LAMBDA(x,x))"),
    ("BYROW sum", "=BYROW(A2:A{last},LAMBDA(x,SUM(x)))"),
    ("BYROW sum of 3", "=BYROW(A2:C{last},LAMBDA(x,SUM(x)))"),
];

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fn_probe: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut rows = DEFAULT_ROWS;
    let mut repeat = 3usize;
    let mut only: Option<String> = None;
    for arg in std::env::args().skip(1) {
        match arg.split_once('=') {
            Some(("--rows", v)) => rows = v.parse().ok().filter(|n: &usize| (3..=MAX_ROWS).contains(n)).ok_or("--rows out of range")?,
            Some(("--repeat", v)) => repeat = v.parse().ok().filter(|n: &usize| (1..=15).contains(n)).ok_or("--repeat out of range")?,
            Some(("--only", v)) => only = Some(v.to_string()),
            _ => return Err(format!("unknown argument `{arg}` (usage: fn_probe [--rows=N] [--repeat=N] [--only=TEXT])")),
        }
    }
    if cfg!(debug_assertions) {
        eprintln!("fn_probe: this is a debug build — the numbers are not comparable (use --release)");
    }
    let started = Instant::now();
    let base = prepare(rows)?;
    eprintln!("fn_probe: {} rows prepared in {:.1} s", fmt_rows(rows), started.elapsed().as_secs_f64());
    println!("\n{:<28} {:>12}  answer", "call", "fastest");
    for (label, template) in CALLS {
        if only.as_deref().is_some_and(|w| !label.contains(w)) {
            continue;
        }
        let formula = template.replace("{last}", &rows.to_string());
        let mut best: Option<(f64, String)> = None;
        for _ in 0..repeat {
            let mut s = Session::new();
            s.add_document(base.clone());
            let t0 = Instant::now();
            s.execute("cell.set", json!({"cell": TARGET, "input": formula})).map_err(|e| format!("{label}: {e}"))?;
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            let answer = s.execute("cell.get", json!({"cell": TARGET})).map(|v| shown(&v)).unwrap_or_default();
            if best.as_ref().is_none_or(|(b, _)| ms < *b) {
                best = Some((ms, answer));
            }
        }
        let (ms, answer) = best.unwrap_or((0.0, String::new()));
        println!("{:<28} {:>12}  {answer}", label, fmt_ms(ms));
    }
    Ok(())
}

/// What `cell.get` answered, in one short string: the number, the text, or the error.
fn shown(v: &Json) -> String {
    if let Some(o) = v.get("value").and_then(Json::as_object) {
        if let Some(e) = o.get("error").and_then(Json::as_str) {
            return format!("#{e}");
        }
        if let Some(n) = o.get("number").and_then(Json::as_f64) {
            return format!("{n}");
        }
        if let Some(t) = o.get("text").and_then(Json::as_str) {
            return format!("\"{t}\"");
        }
        if let Some(b) = o.get("bool").and_then(Json::as_bool) {
            return format!("{b}");
        }
    }
    v.get("text").and_then(Json::as_str).unwrap_or_default().to_string()
}

fn fmt_ms(ms: f64) -> String {
    if ms >= 1_000.0 {
        format!("{:.2} s", ms / 1_000.0)
    } else if ms >= 10.0 {
        format!("{ms:.0} ms")
    } else if ms >= 1.0 {
        format!("{ms:.1} ms")
    } else {
        format!("{:.0} us", ms * 1_000.0)
    }
}

fn fmt_rows(rows: usize) -> String {
    if rows >= 1_000_000 { format!("{:.1}M", rows as f64 / 1e6) } else { format!("{}k", rows / 1_000) }
}

/// The workbook every call starts from, built exactly as the function catalogue builds it: a header
/// row and `rows - 1` data rows of eight columns — a number, a quantity, a price, an item name, a
/// region, a date, a key and a value.
fn prepare(rows: usize) -> Result<DocState, String> {
    let mut s = Session::new();
    s.new_workbook();
    let err = |e: cella_engine::EngineError| e.to_string();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Id", "Qty", "Price", "Item", "Region", "Date", "Key", "Value"]]}))
        .map_err(err)?;
    let data_rows = rows.saturating_sub(1);
    let mut r = 0;
    while r < data_rows {
        let n = CHUNK.min(data_rows - r);
        let values: Vec<Json> = (0..n)
            .map(|i| {
                let id = r + i + 1;
                json!([
                    id as f64,
                    (id % 1_000 + 1) as f64,
                    (id % 97) as f64 + 0.5,
                    format!("Item {id:06}"),
                    REGIONS[id % REGIONS.len()],
                    format!("2026-{:02}-{:02}", 1 + id % 12, 1 + id % 28),
                    format!("K{id:06}"),
                    (id % 1_000) as f64
                ])
            })
            .collect();
        s.execute("range.setValues", json!({"range": format!("A{}", r + 2), "values": values})).map_err(err)?;
        r += n;
    }
    s.doc().map_err(err).cloned()
}
