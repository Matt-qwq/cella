//! The shape ruler: the same commands at three sheet *shapes*, because rows are not the only axis.
//!
//! `cargo run --release -p cella-engine --example bench_shapes -- [--json] [--only=ID,...] [--shape=NAME]`
//!
//! Every other ruler in this repository varies **rows**: 100,000 to 1,000,000 of them, six columns
//! wide. That answers "does this scan the body faster or slower", and it cannot answer "does this
//! cost more on a thousand columns", "does this cost more on a hundred sheets", which are the
//! questions a command that is O(columns) or O(sheets) fails.
//!
//! Three shapes, each **eight million cells** so a number is comparable across them:
//!
//! | shape | rows | columns | sheets | cells |
//! |---|---|---|---|---|
//! | `tall` | 1,000,000 | 8 | 1 | 8,000,000 |
//! | `wide` | 8,000 | 1,000 | 1 | 8,000,000 |
//! | `sheets` | 10,000 | 8 | 100 | 8,000,000 |
//!
//! A command runs once per shape, alone in a fresh session holding a copy of that shape, with its
//! region-naming params over the whole used area — the same generator the command catalogue uses
//! (`crates/engine/tests/param_gen`), so the params are the same shape of question. What a row
//! says: the command, the shape, its wall clock, and how much the *document* grew (the engine's
//! memory report, sheets + workbook + calc — not the undo snapshot, which is not attributable per
//! command yet; see `docs/performance.md`).
//!
//! `cargo xtask bench-shapes` runs this and writes `docs/performance-shapes.md`.

#![allow(clippy::disallowed_methods)] // a native-only benchmark: wasm never runs it, and the clock *is* the measurement

use std::process::ExitCode;
use std::time::Instant;

use cella_engine::{DocState, EngineError, Session, command_specs};
use serde_json::{Value as Json, json};

#[path = "../tests/param_gen/mod.rs"]
mod param_gen;

use param_gen::{Rng, Scale, doc_params};

/// Cells every shape holds, so a number means the same thing in each column.
const CELLS: usize = 8_000_000;
/// Rows per `range.setValues` call when a shape is built.
const CHUNK: usize = 10_000;
/// How many generated param sets a command gets before the shape is recorded as turning them down.
const TRIES: usize = 4;

/// One shape: what it is, and the numbers its params are generated from.
struct Shape {
    name: &'static str,
    rows: usize,
    cols: usize,
    sheets: usize,
    why: &'static str,
}

/// The three shapes, each eight million cells.
const SHAPES: &[Shape] = &[
    Shape { name: "tall", rows: 1_000_000, cols: 8, sheets: 1, why: "a million rows, eight columns" },
    Shape { name: "wide", rows: 8_000, cols: 1_000, sheets: 1, why: "a thousand columns, eight thousand rows" },
    Shape { name: "sheets", rows: 10_000, cols: 8, sheets: 100, why: "a hundred sheets of ten thousand rows" },
];

/// The commands this ruler prices: the ones whose cost can plausibly follow columns or sheets, plus
/// two that should not (a control: if a row shape is flat for them, the ruler agrees with the
/// catalogue).
const COMMANDS: &[(&str, &str)] = &[
    ("edit.find", "scans every used cell"),
    ("edit.clearFormats", "walks every cell's style"),
    ("home.formatCells", "writes a style per cell"),
    ("edit.clearAll", "clears the used area"),
    ("data.sortAscending", "sorts the body by one column"),
    ("range.move", "moves a block"),
    ("home.autofitColumnWidth", "measures every column"),
    ("file.saveBytes", "serialises the workbook"),
    ("home.conditionalFormat", "a conditional-format rule over the area"),
    ("data.filter", "an AutoFilter over the area"),
    ("edit.copy", "copies the area"),
    ("sheet.rename", "per-sheet work (should follow sheets, not cells)"),
];

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bench_shapes: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json_out = args.iter().any(|a| a == "--json");
    let only: Option<Vec<String>> =
        args.iter().find_map(|a| a.strip_prefix("--only=")).map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect());
    // `--save=DIR` writes the three shapes out as .xlsx and stops: the external replay (Excel, WPS)
    // needs the same workbooks on disk, and this ruler is the thing that defines them.
    let save_dir = args.iter().find_map(|a| a.strip_prefix("--save=")).map(|v| v.trim_end_matches(['\\', '/']).to_string());
    // `--shape=wide` prices one shape alone: a change that only touches a wide sheet is that
    // shape's question, and building the other two to answer it is two thirds of the wait.
    let shape_only = args.iter().find_map(|a| a.strip_prefix("--shape=")).map(|v| v.trim().to_string());
    if let Some(want) = &shape_only
        && !SHAPES.iter().any(|s| s.name == want)
    {
        return Err(format!("no such shape `{want}` (tall, wide, sheets)"));
    }
    if args.iter().any(|a| a == "--list") {
        for (id, what) in COMMANDS {
            println!("{id}\t{what}");
        }
        return Ok(());
    }
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("usage: bench_shapes [--json] [--only=ID,...] [--shape=NAME] [--list] [--save=DIR]");
        return Ok(());
    }
    if cfg!(debug_assertions) {
        eprintln!("bench_shapes: this is a debug build — the numbers are not comparable (use --release)");
    }
    let wanted: Vec<(&str, &str)> = COMMANDS.iter().copied().filter(|(id, _)| only.as_ref().is_none_or(|w| w.iter().any(|x| x == id))).collect();
    if wanted.is_empty() {
        return Err("no command matched".to_string());
    }
    if let Some(dir) = &save_dir {
        for shape in SHAPES {
            let base = prepare(shape)?;
            let mut s = Session::new();
            s.add_document(base);
            let path = format!("{dir}/{}.xlsx", shape.name);
            let started = Instant::now();
            s.execute("file.save", json!({"path": path})).map_err(err)?;
            eprintln!("bench_shapes: wrote {path} in {:.1} s", started.elapsed().as_secs_f64());
        }
        return Ok(());
    }
    let mut measurements: Vec<Json> = Vec::new();
    for shape in SHAPES {
        if shape_only.as_deref().is_some_and(|want| want != shape.name) {
            continue;
        }
        eprintln!("bench_shapes: building {} — {} ({} cells)", shape.name, shape.why, shape.rows * shape.cols * shape.sheets);
        let base = prepare(shape)?;
        eprintln!("bench_shapes: {} — {} commands", shape.name, wanted.len());
        for (id, what) in &wanted {
            eprintln!("bench_shapes: {} — {id}", shape.name);
            let m = measure(&base, id, shape)?;
            let mut o = json!({
                "id": id,
                "what": what,
                "shape": shape.name,
                "ms": m.ms,
                "bytes": m.bytes,
                "outcome": m.outcome,
            });
            if let Some(d) = &m.detail {
                o["detail"] = json!(d);
            }
            measurements.push(o);
        }
    }
    if json_out {
        let doc = json!({
            "schema": 1,
            "cells": CELLS,
            "shapes": SHAPES.iter().map(|s| json!({
                "shape": s.name, "rows": s.rows, "cols": s.cols, "sheets": s.sheets, "why": s.why
            })).collect::<Vec<_>>(),
            "measurements": measurements,
        });
        println!("{}", serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?);
    } else {
        println!("\n{CELLS} cells per shape\n");
        println!("{:<26} {:>12} {:>12} {:>12}", "command", "tall", "wide", "sheets");
        for (id, _) in &wanted {
            let cell = |shape: &str| {
                measurements.iter().find(|m| m["id"] == json!(id) && m["shape"] == json!(shape)).map_or_else(
                    || "-".to_string(),
                    |m| match m["outcome"].as_str().unwrap_or("") {
                        "ok" => fmt_ms(m["ms"].as_f64().unwrap_or(0.0)),
                        other => format!("({other})"),
                    },
                )
            };
            println!("{:<26} {:>12} {:>12} {:>12}", id, cell("tall"), cell("wide"), cell("sheets"));
        }
    }
    Ok(())
}

/// A measurement: the wall clock, the document's growth, and how the row went.
struct Measured {
    ms: f64,
    bytes: i64,
    outcome: &'static str,
    detail: Option<String>,
}

/// How many times a slow row is measured before one of the runs is published.
///
/// One sample is not a measurement on this machine: the wide shape's `range.move` reads 2.2 s and
/// 2.7 s in the same afternoon, and a page built from single samples moved by 25 % between runs that
/// changed nothing. A row that takes longer than [`SLOW_MS`] is measured [`SAMPLES`] times **with the
/// same params** — a fresh session holding a copy of the shape each time — and the fastest run is the
/// one published, which is the "best of three" the scenario gate records budgets with. The params
/// come from a seed that depends on the attempt, so re-drawing them for a sample would price a
/// different question (and once drew a sort the body was already ordered for: 1.9 ms).
const SAMPLES: usize = 3;
const SLOW_MS: f64 = 50.0;

/// How many samples a slow row gets: [`SAMPLES`], or `CELLA_SHAPE_SAMPLES` when a page run on a busy
/// machine needs more. The rows it matters for are the allocation-bound ones — `edit.copy` formats a
/// block of cells into one `String` and has measured 540 ms and 944 ms in two full passes of the
/// same code — where a single best-of-three still inherits the machine it woke up on.
fn samples_wanted() -> usize {
    std::env::var("CELLA_SHAPE_SAMPLES").ok().and_then(|v| v.trim().parse::<usize>().ok()).map_or(SAMPLES, |n| n.clamp(1, 15))
}

fn measure(base: &DocState, id: &str, shape: &Shape) -> Result<Measured, String> {
    let mut best: Option<Measured> = None;
    for attempt in 0..TRIES {
        let m = measure_attempt(base, id, shape, attempt, 1)?;
        if best.as_ref().is_none_or(|b| rank(&m) < rank(b)) {
            best = Some(m);
        }
        let Some(found) = best.as_ref() else { continue };
        if found.outcome != "ok" {
            // The command turned these params down: another draw may run it.
            continue;
        }
        // A row that ran is the answer. A slow one is measured again — the same params, the fastest
        // of [`SAMPLES`] runs — because a single sample of it is the machine, not the command.
        if found.ms >= SLOW_MS
            && let Ok(sampled) = measure_attempt(base, id, shape, attempt, samples_wanted())
        {
            best = Some(sampled);
        }
        break;
    }
    best.ok_or_else(|| format!("`{id}` was not measured"))
}

/// How good a row is: one that ran beats one the command said no to, which beats params it turned
/// down. Between two rows of the same rank the faster one wins.
fn rank(m: &Measured) -> (u8, u64) {
    let kind = match m.outcome {
        "ok" => 0,
        "no" => 1,
        "refused" => 2,
        "params" => 3,
        _ => 4,
    };
    (kind, (m.ms * 1000.0) as u64)
}

/// One set of params, run `samples` times in a fresh session each time; the fastest run is the
/// answer, and its document growth is the one reported.
fn measure_attempt(base: &DocState, id: &str, shape: &Shape, attempt: usize, samples: usize) -> Result<Measured, String> {
    let spec = command_specs().iter().find(|spec| spec.id == id).ok_or_else(|| format!("unknown command `{id}`"))?;
    let mut rng = Rng::new(seed_of(id, shape.name).wrapping_add((attempt as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)));
    let params = doc_params(&mut rng, &base.wb, id, spec.params, Scale::Whole { rows: shape.rows as u32, cols: shape.cols as u32 });
    let mut best: Option<Measured> = None;
    for _ in 0..samples.max(1) {
        let mut s = Session::new();
        s.add_document(base.clone());
        let mem_before = document_bytes(&s);
        let started = Instant::now();
        let result = s.execute(id, params.clone());
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        let mem_after = document_bytes(&s);
        let (outcome, detail) = match result {
            Ok(_) => ("ok", None),
            Err(EngineError::Disabled(_, why)) => ("refused", Some(why)),
            Err(EngineError::BadParams { msg, .. }) => ("params", Some(msg)),
            Err(EngineError::Internal(_, msg)) => ("refused", Some(format!("panic guard: {msg}"))),
            Err(e) => ("no", Some(e.to_string())),
        };
        let m = Measured { ms, bytes: mem_after as i64 - mem_before as i64, outcome, detail };
        if best.as_ref().is_none_or(|b| rank(&m) < rank(b)) {
            best = Some(m);
        }
    }
    best.ok_or_else(|| format!("`{id}` was not measured"))
}

/// The live document's own bytes (sheets, workbook parts, calculation state) — not the history: the
/// report's undo line is not attributable per command (see `docs/performance.md`).
fn document_bytes(s: &Session) -> usize {
    let r = s.memory_report();
    let sheets: usize = r.sheets.iter().map(cella_engine::SheetMemory::total).sum();
    sheets + r.workbook.total() + r.calc.total()
}

/// A stable seed, so two runs of the ruler generate the same params for a command and shape.
fn seed_of(id: &str, shape: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.bytes().chain(b"|".iter().copied()).chain(shape.bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
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

/// The workbook one shape needs: a header row and `rows - 1` data rows of `cols` columns — a number,
/// a quantity, a price, a name, a region, a date, a key and a value, repeated — on `sheets` sheets.
fn prepare(shape: &Shape) -> Result<DocState, String> {
    let mut s = Session::new();
    s.new_workbook();
    let first = s.doc().map_err(err)?.wb.sheets.first().map(|sh| sh.name.clone()).unwrap_or_else(|| "Sheet1".into());
    for sheet in 0..shape.sheets {
        if sheet > 0 {
            // A new sheet becomes the active one, so the header and the body land on it.
            s.execute("home.insertSheet", json!({})).map_err(|e| format!("{} sheet {sheet}: {e}", shape.name))?;
        }
        // A header row of `cols` names, then the body.
        let headers: Vec<Json> = (0..shape.cols).map(|c| json!(format!("C{c}"))).collect();
        s.execute("range.setValues", json!({"range": "A1", "values": [headers]})).map_err(|e| format!("{} header: {e}", shape.name))?;
        let mut r = 1;
        while r < shape.rows {
            let n = CHUNK.min(shape.rows - r);
            let values: Vec<Json> = (0..n)
                .map(|i| {
                    let id = r + i + 1;
                    let row: Vec<Json> = (0..shape.cols)
                        .map(|c| match c % 8 {
                            0 => json!(format!("Item {id}")),
                            1 => json!(id as f64),
                            2 => json!((id % 97) as f64),
                            3 => json!((id % 13) as f64 * 1.5),
                            4 => json!("North"),
                            5 => json!(format!("2026-{:02}-{:02}", 1 + id % 12, 1 + id % 28)),
                            6 => json!(format!("K{id:06}")),
                            _ => json!((id % 1000) as f64),
                        })
                        .collect();
                    Json::Array(row)
                })
                .collect();
            s.execute("range.setValues", json!({"range": format!("A{}", r + 1), "values": values}))
                .map_err(|e| format!("{} body at row {}: {e}", shape.name, r))?;
            r += n;
        }
    }
    // The first sheet is the one every command runs on, so the workbook starts there.
    let _ = s.execute("sheet.activate", json!({"sheet": first})).map_err(err);
    let _ = s.execute("selection.set", json!({"range": "A1"})).map_err(err);
    let doc = s.doc().map_err(err)?.clone();
    Ok(doc)
}
