//! Where a big workbook's memory goes: the cell store, the formulas in it, the dependency graph
//! and the history, measured on the same reference workload the performance gate uses.
//!
//! The workbook is built through the engine's own commands, in the order
//! examples/perf.rs builds it (fill.values, fill.formulas, fill.chained), so the numbers describe
//! the same session the performance numbers do: rows x 6 columns of values, =B*C+D down G and
//! =H(n-1)+G(n) down H. The report is then printed for the live session, and again for the same
//! workbook saved as xlsx and opened back: a document read from a file keeps its formulas' text
//! and finds their expression trees lazily, which the report shows.
//!
//! ```text
//! cargo run --release -p cella-engine --example memory                # 100k and 1M rows
//! cargo run --release -p cella-engine --example memory -- 1000000     # one row count
//! ```
//!
//! The report counts live heap bytes by allocation class (see [MemoryReport]); it is arithmetic on
//! capacities and layouts, not a heap census, and allocator slack is not visible from inside the
//! program.
//!
//! The report is a *steady state*: before it is taken, the deferred-release queue is drained
//! (`Session::release_deferred`). An eviction does not free its snapshot on the edit's path, and a
//! harness stops right after its last fill, so without the drain the queue would still hold those
//! snapshots through the measurement — memory the document kept only because no command had come
//! after the edit. The bytes released are printed, and the report's `pending_release` line then
//! reads zero, so the queue is visible as something that existed rather than as work still owed.

use std::process::ExitCode;

use cella_engine::{MemoryReport, Session, SheetMemory};
use serde_json::{Value, json};

/// Rows measured when none are given on the command line: the two sizes the perf gate uses.
const DEFAULT_ROWS: [usize; 2] = [100_000, 1_000_000];
/// Largest row count this harness accepts (an argument-derived size, so it is capped).
const MAX_ROWS: usize = 5_000_000;
/// Rows per range.setValues call when filling the value columns (as perf.rs does).
const CHUNK: usize = 10_000;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("memory: {e}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "usage: memory [rows...]

Builds the reference workload (rows x 6 columns of values, then =B*C+D down G and =H(n-1)+G(n)
down H, through the same commands examples/perf.rs uses) and prints where its memory goes, once
for the live session and once for the workbook saved as xlsx and opened again.

Default rows: 100000 and 1000000. The largest accepted is 5000000.
";

fn parse_rows(args: &[String]) -> Result<Vec<usize>, String> {
    let mut rows = Vec::new();
    for a in args {
        if a == "--help" || a == "-h" {
            print!("{USAGE}");
            return Err(String::new());
        }
        let n: usize = a.parse().map_err(|_| format!("not a row count: {a}\n{USAGE}"))?;
        rows.push(n.clamp(1, MAX_ROWS));
    }
    if rows.is_empty() {
        rows.extend(DEFAULT_ROWS);
    }
    Ok(rows)
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    for rows in parse_rows(&args)? {
        measure(rows)?;
    }
    Ok(())
}

fn measure(rows: usize) -> Result<(), String> {
    eprintln!("memory: {rows} rows — building");
    let mut session = build(rows)?;
    // Steady state: the build's last command was an edit, so whatever its history budget evicted
    // is still waiting for a free(). Drain it first, and say so, rather than report memory the
    // document would give back on its next cheap command.
    let released = session.release_deferred();
    let live = session.memory_report();
    println!();
    println!("Cella memory — {rows} rows x 6 columns{}", if cfg!(debug_assertions) { " (debug build)" } else { "" });
    println!();
    println!("  deferred release queue: {} released before the report below", human(released));
    println!("  (an evicted history step waits for a later cheap command or the host's idle drain; a session");
    println!("   that stops after a bulk edit would keep it, so the queue line below is zero by construction)");
    print_report("as built (built by the perf scenario's three fills, its release queue drained)", &live);
    print_shapes(&session);
    print_undo(&session);

    eprintln!("memory: {rows} rows — saving and opening again");
    let saved = session.execute("file.saveBytes", json!({"format": "xlsx"})).map_err(err)?;
    let b64 = saved.get("base64").and_then(Value::as_str).ok_or("saveBytes returned no bytes")?.to_string();
    let bytes = saved.get("bytes").and_then(Value::as_u64).unwrap_or(0);
    session.execute("file.open", json!({"name": "big.xlsx", "base64": b64})).map_err(err)?;
    let reopened = session.memory_report();
    print_report(&format!("the same workbook saved as xlsx ({bytes} bytes) and opened again"), &reopened);
    print_shapes(&session);
    print_difference(&live, &reopened);
    Ok(())
}

/// What the undo stack holds and what each step costs beyond the live workbook: a step that keeps
/// every band its edit rewrote is charged those bands, so the line says how many steps there are
/// and which one is the big one.
fn print_undo(session: &Session) {
    let Ok(doc) = session.doc() else { return };
    let total: usize = doc.undo.iter().map(|e| e.retained_bytes).sum();
    println!("  {:<26}{:>16}{:>9}  retained beyond the live workbook", format!("undo stack ({} steps)", doc.undo.len()), total, "");
    for entry in doc.undo.iter().rev().take(4) {
        println!("  {:<26}{:>16}{:>9}  the newest first", format!("  {}", entry.label), entry.retained_bytes, "");
        for (index, sheet) in entry.wb.sheets.iter().enumerate() {
            let live_cells = doc.wb.sheets.get(index).map(|live| &live.cells);
            let retained = live_cells.map_or(0, |live| sheet.cells.retained_bytes_against(live));
            let m = sheet.cells.memory();
            println!(
                "    {} / stripes{:>16}{:>9}  retained {} · row tables {} · texts {}",
                sheet.name,
                m.band_headers.saturating_add(m.bitmaps).saturating_add(m.payloads).saturating_add(m.kind_runs).saturating_add(m.style_runs),
                "",
                retained,
                m.row_extras,
                m.texts
            );
        }
    }
}

/// How many formulas the workbook holds and how many parsed trees they share: the shape work's
/// headline number (SHAPE-SHARING.md §0 — the reference workload's two million formulas are two
/// shapes, `=RC[-5]*RC[-4]+RC[-3]` and `=SUM(R2C7:RC[-1])`).
fn print_shapes(session: &Session) {
    let Ok(doc) = session.doc() else { return };
    let mut formulas = 0usize;
    let mut shapes: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    let mut trees = 0usize;
    for sheet in &doc.wb.sheets {
        for (_, f, _) in sheet.cells.formulas() {
            formulas = formulas.saturating_add(1);
            match &f.shape {
                Some(shape) => {
                    shapes.insert(std::sync::Arc::as_ptr(shape) as usize);
                }
                None => trees = trees.saturating_add(1),
            }
        }
    }
    println!("  {:<26}{:>16}  the trees the formulas share", "formula shapes", format!("{} of {formulas}", shapes.len()));
    if trees > 0 {
        println!("  {:<26}{:>16}  {trees} formulas keep a tree of their own", "", "");
    }
}

/// Builds the reference workload through the engine's commands: the same three fills, in the same
/// order and through the same commands as examples/perf.rs.
fn build(rows: usize) -> Result<Session, String> {
    let mut session = Session::new();
    session.new_workbook();
    // fill.values: rows x 6 values.
    let mut r = 0;
    while r < rows {
        let n = CHUNK.min(rows - r);
        let values: Vec<Value> = (0..n)
            .map(|i| {
                let n = r + i;
                json!([format!("Item {n}"), n as f64, (n % 97) as f64, (n % 13) as f64 * 1.5, 1.0, 2.0])
            })
            .collect();
        session.execute("range.setValues", json!({"range": format!("A{}", r + 1), "values": values})).map_err(err)?;
        r += n;
    }
    // fill.formulas: G = B*C+D down every row.
    session.execute("cell.set", json!({"cell": "G1", "input": "=B1*C1+D1"})).map_err(err)?;
    session.execute("edit.autoFill", json!({"source": "G1", "target": format!("G1:G{rows}")})).map_err(err)?;
    // fill.chained: H = H(n-1) + G(n) down every row.
    session.execute("cell.set", json!({"cell": "H1", "input": "=G1"})).map_err(err)?;
    session.execute("cell.set", json!({"cell": "H2", "input": "=H1+G2"})).map_err(err)?;
    session.execute("edit.autoFill", json!({"source": "H2", "target": format!("H2:H{rows}")})).map_err(err)?;
    Ok(session)
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn print_report(title: &str, report: &MemoryReport) {
    let total = report.total.max(1);
    println!();
    println!("{title}");
    println!();
    println!("  {:<26}{:>16}{:>9}  size", "part", "bytes", "share");
    println!("  {:<26}{:>16}{:>9}  ----------", "--------------------------", "----------------", "---------");
    for sheet in &report.sheets {
        print_sheet(sheet, total);
    }
    row("workbook / styles", report.workbook.styles, total);
    row("workbook / names", report.workbook.names, total);
    row("workbook / other", report.workbook.other, total);
    print_calc(report, total);
    row("history / saved snapshot", report.saved, total);
    row("history / undo", report.undo, total);
    row("history / deferred release", report.pending_release, total);
    row("history / redo", report.redo, total);
    row("journal", report.journal, total);
    println!("  {:<26}{:>16}{:>9}", "", "----------------", "---------");
    row("TOTAL", report.total, total);
}

fn print_sheet(sheet: &SheetMemory, total: usize) {
    row(&format!("sheet {} / cells", sheet.name), sheet.cells.total(), total);
    let c = sheet.cells;
    for (name, part) in [
        ("band headers + rows", c.band_headers),
        ("band index", c.band_index),
        ("bitmaps", c.bitmaps),
        ("payloads", c.payloads),
        ("kind runs", c.kind_runs),
        ("style runs", c.style_runs),
        ("style ids", c.style_ids),
        ("number tables", c.number_dictionaries),
        ("text dictionaries", c.texts),
        ("row side tables", c.row_extras),
        ("formula text", c.formula_text),
        ("formula trees", c.formula_exprs),
        ("array values", c.arrays),
        ("wide payloads", c.wide),
        ("row/column maps", c.maps),
        ("page cache", c.page_cache),
    ] {
        if part > 0 {
            row(&format!("  {name}"), part, total);
        }
    }
    row(&format!("sheet {} / lines", sheet.name), sheet.lines, total);
    row(&format!("sheet {} / objects", sheet.name), sheet.objects, total);
}

fn print_calc(report: &MemoryReport, total: usize) {
    let calc = &report.calc;
    row("calc / graph nodes", calc.graph.nodes, total);
    row("calc / precedent areas", calc.graph.areas, total);
    row("calc / dependents map", calc.graph.cell_deps, total);
    row("calc / range index", calc.graph.cols, total);
    row("calc / wide range index", calc.graph.wide, total);
    row("calc / legacy arrays", calc.graph.legacy_ranges, total);
    row("calc / flag sets", calc.graph.flags, total);
    row("calc / unsettled ranges", calc.graph.unsettled, total);
    row("calc / graph buffers", calc.graph.scratch, total);
    row("calc / cycles", calc.circular, total);
    row("calc / areas buffer", calc.scratch_areas, total);
    row("calc / unshared trees", calc.unshared, total);
    row("calc / name cache", calc.names, total);
    row("calc / pass (at rest)", calc.pass, total);
    println!("  {:<26}{:>16}{:>9}  many", "  graph dependents lists", report.calc.graph.many_lists, "");
}

fn row(label: &str, bytes: usize, total: usize) {
    let share = bytes as f64 * 100.0 / total as f64;
    println!("  {:<26}{:>16}{:>8.1}%  {}", label, bytes, share, human(bytes));
}

/// A byte count as a human unit in a fixed shape (1.99 GB, 512.0 MB, 4.0 kB, 64 B).
fn human(bytes: usize) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.2} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.1} kB", b / 1e3)
    } else {
        format!("{bytes} B")
    }
}

/// What the save/reopen round trip changed, line by line: the reader keeps a formula's text and
/// parses its tree on demand, so the trees are gone until something needs them, while the graph the
/// open built is still there. Printed rather than explained, so a surprise stays visible.
fn print_difference(live: &MemoryReport, reopened: &MemoryReport) {
    println!();
    println!("  saved and reopened against as built");
    println!();
    println!("  {:<34}{:>16}{:>16}", "line", "as built", "reopened");
    println!("  {:<34}{:>16}{:>16}", "----------------------------------", "----------------", "----------------");
    let fields: [(&str, fn(&MemoryReport) -> usize); 13] = [
        ("cells", |r| r.sheets.iter().map(|s| s.cells.total()).sum()),
        ("formula text", |r| r.sheets.iter().map(|s| s.cells.formula_text).sum()),
        ("formula trees", |r| r.sheets.iter().map(|s| s.cells.formula_exprs).sum()),
        ("row side tables", |r| r.sheets.iter().map(|s| s.cells.row_extras).sum()),
        ("text dictionaries", |r| r.sheets.iter().map(|s| s.cells.texts).sum()),
        ("graph nodes", |r| r.calc.graph.nodes),
        ("dependents map", |r| r.calc.graph.cell_deps),
        ("name cache", |r| r.calc.names),
        ("undo", |r| r.undo),
        ("deferred release", |r| r.pending_release),
        ("saved snapshot", |r| r.saved),
        ("journal", |r| r.journal),
        ("total", |r| r.total),
    ];
    for (name, field) in fields {
        let (a, b) = (field(live), field(reopened));
        let delta = if b >= a { format!("+{}", human(b - a)) } else { format!("-{}", human(a - b)) };
        println!("  {:<34}{:>16}{:>16}  {}", name, human(a), human(b), delta);
    }
    println!();
}
