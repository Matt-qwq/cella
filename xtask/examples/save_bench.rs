//! `cargo run --release -p xtask --example save_bench -- [--rows=N[,N...]] [--saves=N] [--json]`
//!
//! The save micro-benchmark (the R12 follow-up): build the reference workbook to the exact state
//! `save.xlsx` measures in `crates/engine/examples/perf.rs` — the six value columns, the `G` and `H`
//! formula fills, the three aggregates, the `B1` edit, the descending sort — then save it
//! `--saves` times inside this one process and keep the fastest pass. Repeating in-process is what
//! squeezes the noise: the machine's ~10% run-to-run spread comes from background work and cold
//! caches, and a best-of-six over one warm session holds ~1%, which is the floor at which a save
//! change can be certified instead of argued about.
//!
//! The gate runs it in release (`cargo xtask perf`); a debug build prints a warning and its
//! numbers are not comparable with anything. Output: the same JSON shape the perf harness prints,
//! so the gate reads it with the parser it already has. It lives in xtask (not
//! `crates/engine/examples`) because the gate measures it, and the engine side stays untouched.

#![allow(clippy::disallowed_methods)] // a native-only benchmark: wasm never runs it, and the clock *is* the measurement

#[cfg(not(feature = "engine"))]
fn main() {
    eprintln!("save_bench: build xtask with the `engine` feature (on by default) to measure saves");
}

#[cfg(feature = "engine")]
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("save_bench: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(feature = "engine")]
mod bench {
    use std::time::Instant;

    use cella_engine::Session;
    use serde_json::{Value, json};

    /// Version of the `--json` shape (the gate parses it; same as the perf harness).
    const SCHEMA: u32 = 1;
    /// Row counts measured when none are given: the two sizes the gate checks.
    const DEFAULT_ROWS: [usize; 2] = [100_000, 1_000_000];
    /// Largest row count this bench accepts — an argument-derived size, so it is capped.
    const MAX_ROWS: usize = 5_000_000;
    /// Rows per `range.setValues` call when filling the value columns (the harness's chunk).
    const CHUNK: usize = 10_000;
    /// Largest `--saves` this bench accepts — an argument-derived count, so it is capped.
    const MAX_SAVES: usize = 10;
    /// The scenario id the gate files this under.
    const SCENARIO: &str = "save.repeat";

    const USAGE: &str = "usage: save_bench [--rows=N[,N...]] [rows...] [--saves=N] [--json]

Builds the reference workbook (the state `save.xlsx` measures in the perf harness) and saves it
`--saves` times in this one process, keeping the fastest pass. Default rows: 100000 and 1000000.

  --json            print one JSON object (the gate reads it) instead of a line per size
  --saves=N         save N times and keep the best (default 6)
";

    struct Args {
        rows: Vec<usize>,
        saves: usize,
        json: bool,
    }

    fn parse_args(args: &[String]) -> Result<Args, String> {
        let mut rows = Vec::new();
        let mut saves = 6;
        let mut json = false;
        let mut it = args.iter();
        while let Some(a) = it.next() {
            let (flag, inline) = match a.split_once('=') {
                Some((f, v)) => (f, Some(v)),
                None => (a.as_str(), None),
            };
            match flag {
                "--json" => json = true,
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
                "--saves" => {
                    let value = match inline {
                        Some(v) => v,
                        None => it.next().ok_or("--saves needs a value")?,
                    };
                    saves = value.trim().parse().map_err(|_| format!("`{value}` is not a save count"))?;
                    if !(1..=MAX_SAVES).contains(&saves) {
                        return Err(format!("--saves must be 1..={MAX_SAVES}"));
                    }
                }
                // A bare number is a row count (the perf harness accepts the same).
                _ if !flag.starts_with('-') => rows.push(rows_of(flag)?),
                _ => return Err(format!("unknown argument `{a}`\n\n{USAGE}")),
            }
        }
        if rows.is_empty() {
            rows.extend(DEFAULT_ROWS);
        }
        rows.sort_unstable();
        rows.dedup();
        Ok(Args { rows, saves, json })
    }

    fn rows_of(text: &str) -> Result<usize, String> {
        let rows: usize = text.trim().parse().map_err(|_| format!("`{text}` is not a row count"))?;
        if rows == 0 || rows > MAX_ROWS {
            return Err(format!("row count {rows} is out of range (1..={MAX_ROWS})"));
        }
        Ok(rows)
    }

    fn exec(session: &mut Session, id: &str, params: Value) -> Result<Value, String> {
        session.execute(id, params).map_err(|e| e.to_string())
    }

    /// The workbook the perf harness has at the moment `save.xlsx` runs: values, both formula
    /// fills, the three aggregates, the `B1` edit and the descending sort. Built with the same
    /// commands and the same chunking, so the two scenarios stay comparable.
    fn build(session: &mut Session, rows: usize) -> Result<(), String> {
        let mut r = 0;
        while r < rows {
            let n = CHUNK.min(rows - r);
            let values: Vec<Value> = (0..n)
                .map(|i| {
                    let n = r + i;
                    json!([format!("Item {n}"), n as f64, (n % 97) as f64, (n % 13) as f64 * 1.5, 1.0, 2.0])
                })
                .collect();
            exec(session, "range.setValues", json!({"range": format!("A{}", r + 1), "values": values}))?;
            r += n;
        }
        exec(session, "cell.set", json!({"cell": "G1", "input": "=B1*C1+D1"}))?;
        exec(session, "edit.autoFill", json!({"source": "G1", "target": format!("G1:G{rows}")}))?;
        exec(session, "cell.set", json!({"cell": "H1", "input": "=G1"}))?;
        exec(session, "cell.set", json!({"cell": "H2", "input": "=H1+G2"}))?;
        exec(session, "edit.autoFill", json!({"source": "H2", "target": format!("H2:H{rows}")}))?;
        exec(session, "cell.set", json!({"cell": "J1", "input": format!("=SUM(G1:G{rows})")}))?;
        exec(session, "cell.set", json!({"cell": "J2", "input": "=SUMIFS(B:B,C:C,\">50\")"}))?;
        exec(session, "cell.set", json!({"cell": "J3", "input": "=XLOOKUP(\"Item 77777\",A:A,B:B)"}))?;
        exec(session, "cell.set", json!({"cell": "B1", "input": "5"}))?;
        exec(session, "data.sortDescending", json!({"range": format!("A1:F{rows}"), "column": "C"}))?;
        Ok(())
    }

    struct Saved {
        rows: usize,
        ms: f64,
        bytes: u64,
    }

    /// One size: build the workbook once, then save `saves` times and keep the fastest.
    fn measure(rows: usize, saves: usize) -> Result<Saved, String> {
        let mut session = Session::new();
        session.new_workbook();
        build(&mut session, rows)?;
        let mut best_ms = f64::INFINITY;
        let mut bytes = 0;
        for pass in 0..saves {
            let start = Instant::now();
            let saved = exec(&mut session, "file.saveBytes", json!({"format": "xlsx"}))?;
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            let count = saved.get("bytes").and_then(Value::as_u64).ok_or("saveBytes returned no byte count")?;
            drop(saved); // the base64 copy goes out of scope; only the byte count is kept
            eprintln!("save_bench: {rows} rows — save {} of {saves}: {ms:.1} ms", pass + 1);
            if ms < best_ms {
                best_ms = ms;
                bytes = count;
            }
        }
        Ok(Saved { rows, ms: best_ms, bytes })
    }

    /// The `--json` document: the same shape the perf harness prints, one row per size.
    fn report(rows: &[usize], saves: usize, results: &[Saved]) -> Value {
        let results: Vec<Value> = results
            .iter()
            .map(|s| {
                json!({
                    "scenario": SCENARIO,
                    "rows": s.rows,
                    "ms": (s.ms * 10.0).round() / 10.0,
                    "detail": format!("best of {saves} saves, {} bytes", s.bytes),
                })
            })
            .collect();
        json!({
            "schema": SCHEMA,
            "release": !cfg!(debug_assertions),
            "rows": rows,
            "repeat": saves,
            "results": results,
        })
    }

    pub fn main() -> Result<(), String> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let args = match parse_args(&args) {
            Ok(a) => a,
            // `--help` already printed everything, and it is not an error.
            Err(e) if e.is_empty() => return Ok(()),
            Err(e) => return Err(e),
        };
        if cfg!(debug_assertions) {
            eprintln!("save_bench: this is a debug build — the numbers are not comparable (use --release)");
        }
        let mut results = Vec::new();
        for &rows in &args.rows {
            results.push(measure(rows, args.saves)?);
        }
        if args.json {
            let doc = serde_json::to_string_pretty(&report(&args.rows, args.saves, &results)).map_err(|e| e.to_string())?;
            println!("{doc}");
        } else {
            for s in &results {
                let build = if cfg!(debug_assertions) { "debug" } else { "release" };
                println!("save_bench: {build}, best of {} saves — {} rows: {:.2} s, {} bytes", args.saves, s.rows, s.ms / 1000.0, s.bytes);
            }
        }
        Ok(())
    }
}

#[cfg(feature = "engine")]
fn run() -> Result<(), String> {
    bench::main()
}
