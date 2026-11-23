//! One number per command: the per-command timing catalogue.
//!
//! `cargo xtask perf` prices fifteen *scenarios*; this prices the **299 registered commands**, one
//! row each, on a workbook of the size the gate uses. It exists because "every operation, every
//! feature, should be observable" (the owner's call of 2026-11-23) starts with every command
//! having a number at all — the gate's eleven priced commands are 3.7 % of the surface, and the
//! unmeasured rest is where a slow operation hides.
//!
//! ```text
//! cargo run --release -p cella-engine --example bench_commands              # 100k and 1M rows
//! cargo run --release -p cella-engine --example bench_commands -- --json    # what `cargo xtask
//!                                                                          # bench-commands` reads
//! cargo run --release -p cella-engine --example bench_commands -- --rows=1000000
//! cargo run --release -p cella-engine --example bench_commands -- --only=home.bold,data.sort
//! cargo run --release -p cella-engine --example bench_commands -- --small     # one scale
//! cargo run --release -p cella-engine --example bench_commands -- --list
//! ```
//!
//! # What a row measures
//!
//! A command runs **once, alone, in a fresh session** holding a copy of one prepared workbook:
//! `rows` × 6 columns of values, a table over the first thousand rows, a chart, a defined name,
//! an applied AutoFilter, a selection, a clipboard block and one undo step (see [`prepare`]).
//! The workbook is prepared once per row count and shared by every command, so what is timed is
//! the command — the session, the parsing of its params and the workbook copy the command's first
//! write pays for itself are all inside the timed span, and nothing else is.
//!
//! Every command runs at **two scales**, because they answer different questions:
//!
//! - `small` — the fuzzer's params (`tests/param_gen`): a region near the top-left. This is "what
//!   does the command cost when the user points it at a few cells".
//! - `whole` — the same command, its region-naming parameters (`range`, `target`, `criteria`, …)
//!   pointed at `A1:` the bottom-right corner of the used area: what "select the column and format
//!   it" costs on a million rows. Without this column a command that walks its range shows a
//!   number that measures only its argument parsing.
//!
//! Two commands are therefore *not* comparable across scales unless the command names a region;
//! for the rest the two numbers are the same measurement and say so.
//!
//! # What a row does not measure
//!
//! Correctness (the command-sequence fuzzer's job), throughput across a session (`cargo xtask
//! perf`'s), frame time (the UI bench's), and the second half of `edit.paste` — a block written
//! from the clipboard is priced here, a block decoded from an OS clipboard is not. Commands that
//! read or write files, open or close documents or switch windows cannot run in a throwaway
//! session at all; they are listed as [`NOT_RUN`] with their reason and appear in
//! `xtask/bench-coverage.txt` as `excuse:` lines.

#![allow(clippy::disallowed_methods)] // a native-only benchmark: wasm never runs it, and the clock *is* the measurement

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use cella_engine::{DocState, EngineError, Session, command_specs};
use serde_json::{Value as Json, json};

// The generator of the command-sequence fuzzer, so a command's params have one definition: it
// reads a command's `params` doc, and `Scale` says how big a region-naming key may be.
#[path = "../tests/param_gen/mod.rs"]
mod param_gen;

use param_gen::{Rng, Scale, doc_params, quoted};

/// Version of the `--json` shape (`cargo xtask bench-commands` parses it).
const SCHEMA: u32 = 1;
/// Row counts measured when none are given on the command line: the two sizes the gate checks.
const DEFAULT_ROWS: [usize; 2] = [100_000, 1_000_000];
/// Largest row count this harness accepts — an argument-derived size, so it is capped.
const MAX_ROWS: usize = 5_000_000;
/// Rows per `range.setValues` call when preparing the workbook.
const CHUNK: usize = 10_000;
/// Rows the table, the AutoFilter and the defined name cover.
const OBJECT_ROWS: u32 = 1_000;
/// Upper bound for `--repeat` (input-derived work, so it is capped).
const MAX_REPEAT: usize = 10;
/// How many generated param sets a command gets before the catalogue records that the workbook
/// turned them all down. Four is enough for the keys the generator deliberately fuzzes wrong
/// (a sheet that does not exist, a name that is not valid) and small enough that the run stays
/// under a minute.
const TRIES: usize = 4;

/// The scales every command is measured at, and what each one answers.
const SCALES: [(&str, &str); 2] = [("small", "params near the top-left, the fuzzer's"), ("whole", "region params over the whole used area")];

/// Commands the catalogue cannot run in a throwaway session, and why. Each is an `excuse:` line
/// of `xtask/bench-coverage.txt`; the coverage check fails if one of them is marked `bench` there
/// without a number in the generated catalogue.
const NOT_RUN: &[(&str, &str)] = &[
    ("file.new", "opens another document"),
    ("file.open", "opens another document (reads a file)"),
    ("file.close", "closes the document under test"),
    ("file.save", "writes files"),
    ("file.saveAs", "writes files"),
    ("file.exportCsv", "writes files"),
    ("file.exportHtml", "writes files"),
    ("file.exportPdf", "renders and writes a PDF"),
    ("file.print", "renders a PDF (the print path is `file.print`'s own benchmarks)"),
    ("insert.picture", "reads files"),
    ("pageLayout.background", "reads files"),
    ("window.activate", "switches windows"),
    ("view.newWindow", "adds a window"),
    ("view.closeWindow", "closes a window, and with the last one the document under test"),
    ("macro.debug", "holds an undo group open across calls"),
];

const USAGE: &str = "usage: bench_commands [rows...] [--rows=N[,N...]] [--json] [--only=ID[,ID...]] [--repeat=N]
                     [--small] [--whole] [--list]

Times every registered engine command once on a workbook of N rows × 6 columns, at two scales
(small params, and region params over the whole used area). Default rows: 100000 and 1000000.

  --json             print one JSON object (the catalogue page reads it) instead of a table
  --only=ID,...      measure only those command ids
  --repeat=N         measure each command N times and keep the fastest (default 1)
  --small/--whole    measure one scale only (default: both)
  --list             list the command ids this harness would measure
";

struct Args {
    rows: Vec<usize>,
    json: bool,
    only: Option<Vec<String>>,
    repeat: usize,
    scales: Vec<&'static str>,
}

#[derive(Clone, Debug)]
struct Measurement {
    id: String,
    rows: usize,
    scale: &'static str,
    ms: f64,
    /// Whether the workbook's `Arc` moved: a command that returns Ok without writing anything
    /// (a menu that only opens a dialog, a search that finds nothing) is a 0 ms row for a reason
    /// the page has to be able to show.
    changed: bool,
    /// Whether the command is registered as an edit (undoable). A query that changes nothing is
    /// doing its job; an edit that changes nothing did not.
    writes: bool,
    /// `ok`, `refused` (the command's `enabled` said no), `params` (it turned the generated
    /// params down), `no` (it ran and the answer was no: nothing found, no such table) or `panic`.
    outcome: &'static str,
    error: Option<String>,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bench_commands: {e}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut rows = Vec::new();
    let mut json = false;
    let mut only: Option<Vec<String>> = None;
    let mut repeat = 1;
    let mut scales: Vec<&'static str> = SCALES.iter().map(|(name, _)| *name).collect();
    let mut picked: Vec<&'static str> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) => (f, Some(v)),
            None => (a.as_str(), None),
        };
        match flag {
            "--json" => json = true,
            "--list" => {
                print!("{USAGE}");
                for (id, why) in NOT_RUN {
                    println!("  {id:<28} not run: {why}");
                }
                for spec in command_specs() {
                    if !NOT_RUN.iter().any(|(id, _)| *id == spec.id) {
                        println!("  {}", spec.id);
                    }
                }
                return Err(String::new());
            }
            "-h" | "--help" => {
                print!("{USAGE}");
                return Err(String::new());
            }
            "--small" | "--whole" => picked.push(if flag == "--small" { "small" } else { "whole" }),
            "--rows" => {
                let value = match inline {
                    Some(v) => v,
                    None => it.next().ok_or("--rows needs a value")?,
                };
                for r in value.split(',') {
                    rows.push(rows_of(r)?);
                }
            }
            "--only" => {
                let value = match inline {
                    Some(v) => v,
                    None => it.next().ok_or("--only needs a value")?,
                };
                only = Some(value.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect());
            }
            "--repeat" => {
                let value = match inline {
                    Some(v) => v,
                    None => it.next().ok_or("--repeat needs a value")?,
                };
                repeat = value.trim().parse().map_err(|_| format!("`{value}` is not a repeat count"))?;
                if !(1..=MAX_REPEAT).contains(&repeat) {
                    return Err(format!("--repeat must be 1..={MAX_REPEAT}"));
                }
            }
            // A bare number is a row count (the original invocation: `bench_commands 1000000`).
            _ if !flag.starts_with('-') => rows.push(rows_of(flag)?),
            _ => return Err(format!("unknown argument `{a}`\n\n{USAGE}")),
        }
    }
    if rows.is_empty() {
        rows.extend(DEFAULT_ROWS);
    }
    rows.sort_unstable();
    rows.dedup();
    if !picked.is_empty() {
        scales = picked;
    }
    if let Some(ids) = &only {
        for id in ids {
            if !command_specs().iter().any(|spec| spec.id == id) {
                return Err(format!("unknown command `{id}` (try --list)"));
            }
        }
    }
    let only = only.filter(|ids| !ids.is_empty());
    Ok(Args { rows, json, only, repeat, scales })
}

fn rows_of(text: &str) -> Result<usize, String> {
    let rows: usize = text.trim().parse().map_err(|_| format!("`{text}` is not a row count"))?;
    if rows == 0 || rows > MAX_ROWS {
        return Err(format!("row count {rows} is out of range (1..={MAX_ROWS})"));
    }
    Ok(rows)
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&args) {
        Ok(a) => a,
        // `--list`/`--help` already printed everything, and neither is an error.
        Err(e) if e.is_empty() => return Ok(()),
        Err(e) => return Err(e),
    };
    if cfg!(debug_assertions) {
        eprintln!("bench_commands: this is a debug build — the numbers are not comparable (use --release)");
    }
    let wanted: Vec<&str> = command_specs()
        .iter()
        .map(|spec| spec.id)
        .filter(|id| !NOT_RUN.iter().any(|(skip, _)| skip == id))
        .filter(|id| args.only.as_ref().is_none_or(|ids| ids.iter().any(|want| want == id)))
        .collect();
    if wanted.is_empty() {
        return Err("no command matched".to_string());
    }
    let mut measurements: Vec<Measurement> = Vec::new();
    for &rows in &args.rows {
        eprintln!("bench_commands: preparing {} rows × 6 columns", rows);
        let base = prepare(rows)?;
        for (scale_name, why) in SCALES.iter() {
            if !args.scales.iter().any(|want| want == scale_name) {
                continue;
            }
            eprintln!("bench_commands: {rows} rows — {scale_name} ({why})");
            for id in &wanted {
                let mut best: Option<Measurement> = None;
                for pass in 0..args.repeat {
                    if args.repeat > 1 {
                        eprintln!("bench_commands: {rows} rows — {id} ({scale_name}), pass {} of {}", pass + 1, args.repeat);
                    } else {
                        // Printed before it runs, so a command that never returns names itself.
                        eprintln!("bench_commands: {rows} rows — {id} ({scale_name})");
                    }
                    let m = measure(&base, id, rows, scale_name)?;
                    if best.as_ref().is_none_or(|b| m.ms < b.ms) {
                        best = Some(m);
                    }
                }
                if let Some(m) = best {
                    measurements.push(m);
                }
            }
        }
    }
    if args.json {
        let doc = serde_json::to_string_pretty(&report(&args, &wanted, &measurements)).map_err(err)?;
        println!("{doc}");
    } else {
        print!("{}", render_table(&measurements));
    }
    Ok(())
}

/// The `--json` document: the measurements, the commands that were not run, and how the run was
/// configured — so a page can say what it is looking at without being told out of band.
fn report(args: &Args, wanted: &[&str], measurements: &[Measurement]) -> Json {
    let rows: Vec<Json> = measurements
        .iter()
        .map(|m| {
            let mut o = json!({
                "id": m.id,
                "rows": m.rows,
                "scale": m.scale,
                "ms": m.ms,
                "changed": m.changed,
                "writes": m.writes,
                "outcome": m.outcome,
            });
            if let Some(e) = &m.error {
                o["error"] = json!(e);
            }
            o
        })
        .collect();
    json!({
        "schema": SCHEMA,
        "rows": args.rows,
        "scales": args.scales,
        "repeat": args.repeat,
        "debug": cfg!(debug_assertions),
        "commands": wanted.len(),
        "notRun": NOT_RUN.iter().map(|(id, why)| json!({"id": id, "why": why})).collect::<Vec<_>>(),
        "measurements": rows,
    })
}

fn render_table(measurements: &[Measurement]) -> String {
    let mut s = format!("\n{:<32} {:>6} {:>7} {:>10} {:>6}  {}\n", "command", "rows", "scale", "time", "wrote", "outcome");
    s +=
        &format!("{:<32} {:>6} {:>7} {:>10} {:>6}  {}\n", "-".repeat(32), "-".repeat(6), "-".repeat(7), "-".repeat(10), "-".repeat(6), "-".repeat(7));
    for m in measurements {
        let outcome = match &m.error {
            Some(e) => format!("{}: {}", m.outcome, e),
            None => m.outcome.to_string(),
        };
        s += &format!("{:<32} {:>6} {:>7} {:>10} {:>6}  {}\n", m.id, m.rows, m.scale, fmt_ms(m.ms), if m.changed { "yes" } else { "-" }, outcome);
    }
    s
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

/// The workbook every command starts from: `rows` × 8 value columns, plus one of each object a
/// command needs to have something to work on — a table, a chart, a PivotTable with its slicer and
/// timeline, a shape, a text box, an equation, a WordArt, a button, a slider, a sparkline, a note,
/// a comment, a checkbox and a script.
///
/// Prepared once per row count and cloned per command ([`measure`]), so a command does not pay
/// for the data the sheet already holds. The data and the undo step are required (a base without
/// them would not be the workbook this page claims to measure); the objects are best-effort, and a
/// command that could not be placed says so on stderr instead of failing the run.
fn prepare(rows: usize) -> Result<DocState, String> {
    let mut s = Session::new();
    s.new_workbook();
    let sheet = s.doc().map_err(err)?.wb.sheets.first().map(|sh| sh.name.clone()).unwrap_or_else(|| "Sheet1".into());
    // Row 1 is the header, so a table and a PivotTable have named columns to work with.
    s.execute("range.setValues", json!({"range": "A1", "values": [["Item", "Qty", "Mod", "Half", "Region", "Date", "One", "Two"]]})).map_err(err)?;
    let data_rows = rows.saturating_sub(1);
    let mut r = 0;
    while r < data_rows {
        let n = CHUNK.min(data_rows - r);
        let values: Vec<Json> = (0..n)
            .map(|i| {
                let n = r + i;
                json!([
                    format!("Item {n}"),
                    n as f64,
                    (n % 97) as f64,
                    (n % 13) as f64 * 1.5,
                    REGIONS[n % REGIONS.len()],
                    format!("2026-{:02}-{:02}", 1 + n % 12, 1 + n % 28),
                    1.0,
                    2.0
                ])
            })
            .collect();
        s.execute("range.setValues", json!({"range": format!("A{}", r + 2), "values": values})).map_err(err)?;
        r += n;
    }
    let objects: u32 = OBJECT_ROWS.min(rows as u32);
    best_effort(&mut s, "insert.table", json!({"range": format!("A1:H{objects}"), "header": true, "name": "Sales"}));
    best_effort(&mut s, "insert.chart", json!({"range": "A1:D10", "type": "column", "at": "J2"}));
    best_effort(&mut s, "formulas.defineName", json!({"name": "Data", "refersTo": format!("={}!$A$1:$H${objects}", quoted(&sheet))}));
    best_effort(&mut s, "data.filter", json!({"range": format!("A1:H{objects}")}));
    // The object zoo. Each is placed in the empty columns to the right of the data, where nothing
    // a command does to A:H can reach it; the PivotTable's own name is what `pivot.*` takes.
    let pivot = match best_effort_value(
        &mut s,
        "insert.pivotTable",
        json!({"source": "Sales", "destination": "N2", "rows": ["Region"], "values": [{"field": "Qty", "func": "sum"}]}),
    ) {
        Some(v) => v.get("name").and_then(Json::as_str).map(str::to_string),
        None => None,
    };
    if let Some(p) = &pivot {
        best_effort(&mut s, "insert.slicer", json!({"pivot": p, "field": "Region", "at": "R2"}));
        best_effort(&mut s, "insert.timeline", json!({"pivot": p, "field": "Date", "at": "R20"}));
        best_effort(&mut s, "insert.pivotChart", json!({"pivot": p, "at": "V2"}));
    }
    best_effort(&mut s, "insert.shape", json!({"kind": "roundedRectangle", "at": "R38", "text": "hi"}));
    best_effort(&mut s, "insert.textBox", json!({"at": "R42", "text": "note"}));
    best_effort(&mut s, "insert.equation", json!({"at": "R46", "text": "a+b"}));
    best_effort(&mut s, "insert.wordArt", json!({"style": 0, "text": "Go", "at": "V40"}));
    best_effort(&mut s, "insert.button", json!({"at": "V44", "text": "Run", "command": "cell.set", "params": {"cell": "Z1", "input": "1"}}));
    best_effort(&mut s, "insert.slider", json!({"at": "V48", "link": "$Z$2", "min": 0, "max": 100, "value": 5}));
    best_effort(&mut s, "insert.sparkline", json!({"range": "B2:E2", "location": "Z5", "type": "line"}));
    best_effort(&mut s, "insert.checkbox", json!({"range": "Z10:Z12"}));
    best_effort(&mut s, "review.newNote", json!({"cell": "A1", "text": "note"}));
    best_effort(&mut s, "review.newComment", json!({"cell": "B1", "text": "comment"}));
    best_effort(&mut s, "draw.stroke", json!({"points": [[200.0, 200.0], [260.0, 240.0], [300.0, 200.0]], "color": "#FF0000"}));
    best_effort(
        &mut s,
        "automate.newScript",
        json!({"name": param_gen::SCRIPT_NAME, "commands": [{"command": "cell.set", "params": {"cell": "Z20", "input": "1"}}]}),
    );
    best_effort(
        &mut s,
        "ext.add",
        json!({"name": param_gen::EXTENSION_NAME, "commands": [{"command": "cell.set", "params": {"cell": "Z21", "input": "2"}}]}),
    );
    best_effort(&mut s, "data.scenarioAdd", json!({"name": param_gen::SCENARIO_NAME, "changing": "B2:B10", "values": [1, 2, 3, 4, 5, 6, 7, 8, 9]}));
    best_effort(&mut s, "selection.set", json!({"range": "A1:C10"}));
    best_effort(&mut s, "edit.copy", json!({}));
    // Two formulas the formula-auditing commands can trace (J6 depends on J5, J5 on B2:B10).
    s.execute("cell.set", json!({"cell": "J5", "input": "=SUM(B2:B10)"})).map_err(err)?;
    s.execute("cell.set", json!({"cell": "J6", "input": "=J5*2"})).map_err(err)?;
    // Two ordinary edits with one undone, so `edit.undo` and `edit.redo` each have a step to work
    // on: without them they are refused ("nothing to undo") and a 0 ms row would say nothing.
    s.execute("cell.set", json!({"cell": "J1", "input": "5"})).map_err(err)?;
    s.execute("cell.set", json!({"cell": "J1", "input": "6"})).map_err(err)?;
    s.execute("edit.undo", json!({})).map_err(err)?;
    let doc = s.doc().map_err(err)?.clone();
    Ok(doc)
}

/// The four regions a data row carries, so a slicer has tiles and a PivotTable has row items.
const REGIONS: [&str; 4] = ["North", "South", "East", "West"];

/// Place one of the workbook's optional objects. A failure is reported and stepped over: the
/// catalogue's numbers are about the commands, not about this setup.
fn best_effort(s: &mut Session, id: &str, params: Json) {
    if let Err(e) = s.execute(id, params) {
        eprintln!("bench_commands: preparing `{id}`: {e} (continuing without it)");
    }
}

/// [`best_effort`] for the objects whose id or name a later command has to name (a PivotTable's
/// name, a control's id).
fn best_effort_value(s: &mut Session, id: &str, params: Json) -> Option<Json> {
    match s.execute(id, params) {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("bench_commands: preparing `{id}`: {e} (continuing without it)");
            None
        }
    }
}

/// Run one command once, in a fresh session holding a copy of `base`, and time it. A command whose
/// generated params the workbook turns down is retried with another draw ([`TRIES`]): the params
/// generator also produces deliberately wrong values (a sheet that does not exist, a name that is
/// not valid), which is what the fuzzer wants and what a catalogue must look past.
fn measure(base: &DocState, id: &str, rows: usize, scale: &'static str) -> Result<Measurement, String> {
    let mut best: Option<Measurement> = None;
    for attempt in 0..TRIES {
        let m = measure_once(base, id, rows, scale, attempt)?;
        let ok = m.outcome == "ok";
        if best.as_ref().is_none_or(|b| rank(&m) < rank(b)) {
            best = Some(m);
        }
        if ok {
            break;
        }
    }
    best.ok_or_else(|| format!("`{id}` was not measured"))
}

/// How good a row is: one that ran beats one the command said no to, which beats params it turned
/// down. Between two rows of the same rank the faster one wins.
fn rank(m: &Measurement) -> (u8, u64) {
    let kind = match m.outcome {
        "ok" => 0,
        "no" => 1,
        "refused" => 2,
        "params" => 3,
        _ => 4,
    };
    (kind, (m.ms * 1000.0) as u64)
}

fn measure_once(base: &DocState, id: &str, rows: usize, scale: &'static str, attempt: usize) -> Result<Measurement, String> {
    let mut s = Session::new();
    s.add_document(base.clone());
    // Untimed setup the clipboard needs: `edit.paste` and `edit.pasteSpecial` read the session's
    // clipboard, which is not part of the document a copy of `base` brings.
    let copied = s.execute("selection.set", json!({"range": "A1:C10"})).and_then(|_| s.execute("edit.copy", json!({})));
    if copied.is_err() {
        // A base workbook without a sheet would be a bug in `prepare`, not a command's failure.
        return Err(format!("clipboard setup failed: {}", copied.err().map(|e| e.to_string()).unwrap_or_default()));
    }
    let spec = command_specs().iter().find(|spec| spec.id == id).ok_or_else(|| format!("unknown command `{id}`"))?;
    let wb = s.doc().map_err(err)?.wb.clone();
    // One seed per command and scale: two runs generate the same params, and at the two row
    // counts a command's params differ only in the size of the region they name. The retry moves
    // the seed, so every attempt asks a different question.
    let mut rng = Rng::new(seed_of(id, scale).wrapping_add((attempt as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)));
    let params = doc_params(&mut rng, &wb, id, spec.params, scale_for(scale, rows));
    let before = wb.clone();
    let started = Instant::now();
    let result = s.execute(id, params);
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    let after = s.doc().map_err(err)?.wb.clone();
    let (outcome, error) = match result {
        Ok(_) => ("ok", None),
        Err(EngineError::Disabled(_, why)) => ("refused", Some(why)),
        Err(EngineError::BadParams { msg, .. }) => ("params", Some(msg)),
        Err(EngineError::Internal(_, msg)) => ("panic", Some(msg)),
        Err(e) => ("no", Some(e.to_string())),
    };
    Ok(Measurement { id: id.to_string(), rows, scale, ms, changed: !Arc::ptr_eq(&before, &after), writes: spec.undoable, outcome, error })
}

fn scale_for(name: &str, rows: usize) -> Scale {
    match name {
        "small" => Scale::Small,
        _ => Scale::Whole { rows: rows.clamp(1, MAX_ROWS) as u32, cols: 6 },
    }
}

/// FNV-1a over the id and the scale name: a stable seed, so two runs of the catalogue generate
/// the same params for a command (the alternative — hashing `RandomState` — would not).
fn seed_of(id: &str, scale: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.bytes().chain(b"|".iter().copied()).chain(scale.bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_not_run_entry_names_a_registered_command() {
        for (id, why) in NOT_RUN {
            assert!(command_specs().iter().any(|spec| spec.id == *id), "NOT_RUN names unknown command `{id}`");
            assert!(!why.is_empty(), "`{id}` is not run for no stated reason");
        }
    }

    #[test]
    fn seeds_differ_per_command_and_per_scale() {
        let a = seed_of("home.bold", "small");
        assert_ne!(a, seed_of("home.bold", "whole"));
        assert_ne!(a, seed_of("home.italic", "small"));
        assert_eq!(a, seed_of("home.bold", "small"));
    }

    #[test]
    fn the_catalogue_covers_every_command_except_the_stated_ones() {
        // What `cargo xtask bench-commands` writes and the coverage ledger reads: a command that
        // is neither measured nor excused here would be a hole in both.
        let measured = command_specs().iter().filter(|spec| !NOT_RUN.iter().any(|(id, _)| *id == spec.id)).count();
        assert_eq!(measured + NOT_RUN.len(), command_specs().len(), "a NOT_RUN id is not registered, or a command is in neither list");
    }
}
