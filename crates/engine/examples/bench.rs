//! `cargo run --release -p cella-engine --example bench -- [rows] [out.xlsx]`
//!
//! Writes the reference workload — the same shape `examples/memory.rs` measures: `rows` rows of six
//! values in A–F, `=B*C+D` down G and the chained `=H(n-1)+G(n)` down H. It exists so the same file
//! can be opened by Cella, Excel and WPS and compared: a number measured against ourselves is not a
//! benchmark.

use std::time::Instant;

use cella_engine::Session;
use serde_json::json;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rows: usize = args.first().and_then(|a| a.parse().ok()).unwrap_or(1_000_000);
    let out = args.get(1).cloned().unwrap_or_else(|| "bench.xlsx".to_string());
    match run(rows, &out) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bench: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// A monotonic reading for the report at the end. The workspace lint that forbids `Instant` exists
/// because it traps on wasm; this example writes a file for desktop benchmarks and never builds for
/// wasm, so the one call is allowed here rather than wrapped in a `cfg`.
#[allow(clippy::disallowed_methods)]
fn now() -> Instant {
    Instant::now()
}

fn exec(session: &mut Session, id: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
    session.execute(id, params).map_err(|e| e.to_string())
}

fn run(rows: usize, out: &str) -> Result<(), String> {
    let t = now();
    let mut session = Session::new();
    session.new_workbook();
    const CHUNK: usize = 10_000;
    let mut r = 0;
    while r < rows {
        let n = CHUNK.min(rows - r);
        let values: Vec<serde_json::Value> = (0..n)
            .map(|i| {
                let n = r + i;
                json!([format!("Item {n}"), n as f64, (n % 97) as f64, (n % 13) as f64 * 1.5, 1.0, 2.0])
            })
            .collect();
        exec(&mut session, "range.setValues", json!({"range": format!("A{}", r + 1), "values": values}))?;
        r += n;
    }
    let values = t.elapsed();
    exec(&mut session, "cell.set", json!({"cell": "G1", "input": "=B1*C1+D1"}))?;
    exec(&mut session, "edit.autoFill", json!({"source": "G1", "target": format!("G1:G{rows}")}))?;
    exec(&mut session, "cell.set", json!({"cell": "H1", "input": "=G1"}))?;
    exec(&mut session, "cell.set", json!({"cell": "H2", "input": "=H1+G2"}))?;
    exec(&mut session, "edit.autoFill", json!({"source": "H2", "target": format!("H2:H{rows}")}))?;
    let formulas = t.elapsed();
    let saved = exec(&mut session, "file.save", json!({"path": out}))?;
    println!(
        "bench: {rows} rows -> {out}: values {:.2} s, formulas {:.2} s, save {:.2} s, {} bytes",
        values.as_secs_f64(),
        (formulas - values).as_secs_f64(),
        t.elapsed().as_secs_f64() - formulas.as_secs_f64(),
        saved.get("bytes").and_then(serde_json::Value::as_u64).unwrap_or(0)
    );
    Ok(())
}
