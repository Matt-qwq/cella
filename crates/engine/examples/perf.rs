//! Performance scenarios on large workbooks — the measurement half of GridCraft's performance
//! gate (the gate itself is `cargo xtask perf`; what each scenario means is in
//! `docs/performance.md`).
//!
//! ```text
//! cargo run --release -p gridcraft-engine --example perf                # 100k and 1M rows
//! cargo run --release -p gridcraft-engine --example perf -- 1000000     # one row count
//! cargo run --release -p gridcraft-engine --example perf -- --json      # machine-readable
//! cargo run --release -p gridcraft-engine --example perf -- --scenario=fill.values,sort.desc
//! cargo run --release -p gridcraft-engine --example perf -- --repeat=3   # best of three passes
//! ```
//!
//! Every scenario runs against one session, in the order listed in [`STEPS`], because that is how
//! a real session looks: values first, formulas over them, then edits, sort, save and open. Each
//! step reports one number (its own elapsed time, in ms) and a short detail (cells recalculated,
//! bytes written, …).

#![allow(clippy::disallowed_methods)] // a native-only benchmark: wasm never runs it, and the clock *is* the measurement

use std::process::ExitCode;
use std::time::Instant;

use gridcraft_engine::Session;
use serde_json::{Value, json};

/// Version of the `--json` shape (the gate parses it).
const SCHEMA: u32 = 1;
/// Row counts measured when none are given on the command line: the two sizes the gate checks.
const DEFAULT_ROWS: [usize; 2] = [100_000, 1_000_000];
/// Largest row count this harness accepts — an argument-derived size, so it is capped.
const MAX_ROWS: usize = 5_000_000;
/// Rows per `range.setValues` call when filling the value columns.
const CHUNK: usize = 10_000;

/// One scenario: its id, what it measures, and the code that measures it.
type Step = fn(&mut Bench, usize) -> Result<(String, f64), String>;

/// The scenarios, in the order they run (later ones depend on what the earlier ones built).
const STEPS: &[(&str, &str, Step)] = &[
    ("fill.values", "write rows × 6 values through range.setValues", fill_values),
    ("fill.formulas", "fill G = B*C+D down rows", fill_formulas),
    ("fill.chained", "fill a running sum H = H(n-1) + G(n) down rows", fill_chained),
    ("aggregate", "three aggregate formulas over the filled columns", aggregate),
    ("edit.recalc", "edit one cell and recalculate what depends on it", edit_recalc),
    ("edit.undo", "undo that edit", edit_undo),
    ("sort.desc", "sort rows by one column, descending", sort_desc),
    ("save.xlsx", "save the workbook as xlsx", save_xlsx),
    ("open.xlsx", "open those bytes again (full recalc)", open_xlsx),
];

struct Bench {
    session: Session,
    /// The last saved workbook (its size and bytes), so the open scenario opens a real file.
    saved: Option<(u64, String)>,
}

struct Measurement {
    scenario: &'static str,
    rows: usize,
    ms: f64,
    detail: String,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("perf: {e}");
            ExitCode::FAILURE
        }
    }
}

struct Args {
    rows: Vec<usize>,
    json: bool,
    /// Scenario ids to run; `None` runs all of them.
    only: Option<Vec<String>>,
    /// Passes over the whole sequence; each scenario keeps its fastest pass.
    repeat: usize,
}

/// Upper bound for `--repeat` (input-derived work, so it is capped).
const MAX_REPEAT: usize = 10;

const USAGE: &str = "usage: perf [rows...] [--rows=N[,N...]] [--json] [--scenario=ID[,ID...]] [--repeat=N] [--list]

Runs the performance scenarios of GridCraft's engine against a session of N rows × 6 columns and
reports one number in ms per scenario. Default rows: 100000 and 1000000.

  --json              print one JSON object (the gate reads it) instead of a table
  --scenario=ID,...   run only those scenarios
  --repeat=N          run the sequence N times and keep each scenario's fastest pass (default 1)
  --list              list the scenario ids and what they measure
";

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut rows = Vec::new();
    let mut json = false;
    let mut only: Option<Vec<String>> = None;
    let mut repeat = 1;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) => (f, Some(v)),
            None => (a.as_str(), None),
        };
        match flag {
            "--json" => json = true,
            "--list" => {
                println!("{USAGE}");
                for (id, what, _) in STEPS {
                    println!("  {id:<16} {what}");
                }
                return Err(String::new());
            }
            "-h" | "--help" => {
                print!("{USAGE}");
                return Err(String::new());
            }
            "--rows" => {
                let value = match inline {
                    Some(v) => v,
                    None => it.next().ok_or("--rows needs a value")?,
                };
                for r in value.split(',') {
                    rows.push(rows_of(r)?);
                }
            }
            "--scenario" => {
                let value = match inline {
                    Some(v) => v,
                    None => it.next().ok_or("--scenario needs a value")?,
                };
                only = Some(split_ids(value));
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
            // A bare number is a row count (the original invocation: `perf 1000000`).
            _ if !flag.starts_with('-') => rows.push(rows_of(flag)?),
            _ => return Err(format!("unknown argument `{a}`\n\n{USAGE}")),
        }
    }
    if rows.is_empty() {
        rows.extend(DEFAULT_ROWS);
    }
    rows.sort_unstable();
    rows.dedup();
    if let Some(ids) = &only {
        for id in ids {
            if !STEPS.iter().any(|(known, _, _)| known == id) {
                return Err(format!("unknown scenario `{id}` (try --list)"));
            }
        }
    }
    let only = only.filter(|ids| !ids.is_empty());
    Ok(Args { rows, json, only, repeat })
}

fn split_ids(value: &str) -> Vec<String> {
    value.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

fn rows_of(text: &str) -> Result<usize, String> {
    let rows: usize = text.trim().parse().map_err(|_| format!("`{text}` is not a row count"))?;
    if rows == 0 || rows > MAX_ROWS {
        return Err(format!("row count {rows} is out of range (1..={MAX_ROWS})"));
    }
    Ok(rows)
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
        eprintln!("perf: this is a debug build — the numbers are not comparable (use --release)");
    }
    let mut out = Vec::new();
    for &rows in &args.rows {
        let mut best = run_once(rows, args.only.as_deref())?;
        for pass in 1..args.repeat {
            eprintln!("perf: {rows} rows — pass {} of {}", pass + 1, args.repeat);
            let next = run_once(rows, args.only.as_deref())?;
            merge_best(&mut best, next);
        }
        out.extend(best);
    }
    if args.json {
        let doc = serde_json::to_string_pretty(&report(&args.rows, args.repeat, &out)).map_err(err)?;
        println!("{doc}");
    } else {
        print!("{}", render_table(&out, args.repeat));
    }
    Ok(())
}

/// Keep each scenario's fastest pass. Every pass reports the same scenarios in the same order.
fn merge_best(best: &mut [Measurement], next: Vec<Measurement>) {
    for (b, n) in best.iter_mut().zip(next) {
        if n.ms < b.ms {
            *b = n;
        }
    }
}

/// Build a session of `rows` rows × 6 columns and time every selected scenario on it.
fn run_once(rows: usize, only: Option<&[String]>) -> Result<Vec<Measurement>, String> {
    let mut b = Bench::new();
    let mut out = Vec::new();
    for (id, _what, step) in STEPS {
        if only.is_some_and(|ids| !ids.iter().any(|want| want == id)) {
            continue;
        }
        eprintln!("perf: {rows} rows — {id}");
        let (detail, ms) = step(&mut b, rows)?;
        out.push(Measurement { scenario: id, rows, ms, detail });
    }
    Ok(out)
}

impl Bench {
    fn new() -> Self {
        let mut session = Session::new();
        session.new_workbook();
        Self { session, saved: None }
    }

    fn exec(&mut self, id: &str, params: Value) -> Result<Value, String> {
        self.session.execute(id, params).map_err(err)
    }

    /// Cells the last recalculation touched (0 when the workbook did not recalculate).
    fn recalc_cells(&self) -> usize {
        self.session.doc().map(|d| d.calc.last_recalc_cells).unwrap_or(0)
    }
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Time `f` in milliseconds, propagating its error.
fn timed<T>(f: impl FnOnce() -> Result<T, String>) -> Result<(T, f64), String> {
    let start = Instant::now();
    let value = f()?;
    Ok((value, start.elapsed().as_secs_f64() * 1000.0))
}

fn fill_values(b: &mut Bench, rows: usize) -> Result<(String, f64), String> {
    let ((), ms) = timed(|| {
        let mut r = 0;
        while r < rows {
            let n = CHUNK.min(rows - r);
            let values: Vec<Value> = (0..n)
                .map(|i| {
                    let n = r + i;
                    json!([format!("Item {n}"), n as f64, (n % 97) as f64, (n % 13) as f64 * 1.5, 1.0, 2.0])
                })
                .collect();
            b.exec("range.setValues", json!({"range": format!("A{}", r + 1), "values": values}))?;
            r += n;
        }
        Ok(())
    })?;
    Ok((format!("{rows} rows × 6 columns"), ms))
}

fn fill_formulas(b: &mut Bench, rows: usize) -> Result<(String, f64), String> {
    let ((), ms) = timed(|| {
        b.exec("cell.set", json!({"cell": "G1", "input": "=B1*C1+D1"}))?;
        b.exec("edit.autoFill", json!({"source": "G1", "target": format!("G1:G{rows}")})).map(|_| ())
    })?;
    Ok((format!("G = B*C+D over {rows} rows"), ms))
}

fn fill_chained(b: &mut Bench, rows: usize) -> Result<(String, f64), String> {
    let ((), ms) = timed(|| {
        b.exec("cell.set", json!({"cell": "H1", "input": "=G1"}))?;
        b.exec("cell.set", json!({"cell": "H2", "input": "=H1+G2"}))?;
        b.exec("edit.autoFill", json!({"source": "H2", "target": format!("H2:H{rows}")})).map(|_| ())
    })?;
    Ok((format!("H = H(n-1)+G(n) over {rows} rows"), ms))
}

fn aggregate(b: &mut Bench, rows: usize) -> Result<(String, f64), String> {
    let ((), ms) = timed(|| {
        b.exec("cell.set", json!({"cell": "J1", "input": format!("=SUM(G1:G{rows})")}))?;
        b.exec("cell.set", json!({"cell": "J2", "input": "=SUMIFS(B:B,C:C,\">50\")"}))?;
        b.exec("cell.set", json!({"cell": "J3", "input": "=XLOOKUP(\"Item 77777\",A:A,B:B)"})).map(|_| ())
    })?;
    Ok((format!("SUM, SUMIFS and XLOOKUP over {rows} rows"), ms))
}

/// The edit: `B1` feeds `G` through `B*C+D`, and every `H` chains off the `G` above it.
fn edit_recalc(b: &mut Bench, _rows: usize) -> Result<(String, f64), String> {
    let ((), ms) = timed(|| b.exec("cell.set", json!({"cell": "B1", "input": "5"})).map(|_| ()))?;
    Ok((format!("{} cells recalculated", b.recalc_cells()), ms))
}

fn edit_undo(b: &mut Bench, _rows: usize) -> Result<(String, f64), String> {
    let ((), ms) = timed(|| b.exec("edit.undo", json!({})).map(|_| ()))?;
    Ok(("undo of the B1 edit".to_string(), ms))
}

fn sort_desc(b: &mut Bench, rows: usize) -> Result<(String, f64), String> {
    let ((), ms) = timed(|| b.exec("data.sortDescending", json!({"range": format!("A1:F{rows}"), "column": "C"})).map(|_| ()))?;
    Ok((format!("{rows} rows by column C, descending"), ms))
}

fn save_xlsx(b: &mut Bench, _rows: usize) -> Result<(String, f64), String> {
    let ((bytes, b64), ms) = timed(|| {
        let r = b.exec("file.saveBytes", json!({"format": "xlsx"}))?;
        let bytes = r.get("bytes").and_then(Value::as_u64).ok_or("saveBytes returned no byte count")?;
        let b64 = r.get("base64").and_then(Value::as_str).ok_or("saveBytes returned no bytes")?.to_string();
        Ok((bytes, b64))
    })?;
    b.saved = Some((bytes, b64));
    Ok((format!("{bytes} bytes"), ms))
}

fn open_xlsx(b: &mut Bench, _rows: usize) -> Result<(String, f64), String> {
    let (bytes, ms) = timed(|| {
        let (bytes, b64) = b.saved.take().ok_or("nothing saved to open (run save.xlsx first)")?;
        b.exec("file.open", json!({"name": "big.xlsx", "base64": b64}))?;
        Ok(bytes)
    })?;
    Ok((format!("opened those {bytes} bytes again"), ms))
}

/// The `--json` document: one row per (scenario, row count), in run order.
fn report(rows: &[usize], repeat: usize, results: &[Measurement]) -> Value {
    let results: Vec<Value> = results
        .iter()
        .map(|m| {
            json!({
                "scenario": m.scenario,
                "rows": m.rows,
                "ms": (m.ms * 10.0).round() / 10.0,
                "detail": m.detail,
            })
        })
        .collect();
    json!({
        "schema": SCHEMA,
        "release": !cfg!(debug_assertions),
        "rows": rows,
        "repeat": repeat,
        "results": results,
    })
}

/// A time in ms as a human unit, in a fixed width (`6.21 s`, `142.5 ms`, `151 us`).
fn fmt_time(ms: f64) -> String {
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

/// The stable human table: one row per (row count, scenario), in run order.
fn render_table(all: &[Measurement], repeat: usize) -> String {
    let mut s = String::new();
    let passes = if repeat > 1 { format!(", best of {repeat} passes") } else { String::new() };
    s += &format!(
        "GridCraft performance — {} scenario run(s), {} build{passes}\n\n",
        all.len(),
        if cfg!(debug_assertions) { "debug" } else { "release" }
    );
    let mut scenario_width = "scenario".len();
    for m in all {
        scenario_width = scenario_width.max(m.scenario.len());
    }
    let detail_width = all.iter().map(|m| m.detail.chars().count()).max().unwrap_or(0).clamp("detail".len(), 64);
    s += &format!("{:>9}  {:<scenario_width$}  {:>10}  {}\n", "rows", "scenario", "time", "detail");
    s += &format!("{:>9}  {:<scenario_width$}  {:>10}  {}\n", "---------", "-".repeat(scenario_width), "----------", "-".repeat(detail_width));
    for m in all {
        s += &format!("{:>9}  {:<scenario_width$}  {:>10}  {}\n", m.rows, m.scenario, fmt_time(m.ms), m.detail);
    }
    s
}
