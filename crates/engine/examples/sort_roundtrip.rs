//! Benchmark and validate sort, undo, and redo on the same body under three formula workloads.
//!
//! cargo run --release -p cella-engine --example sort_roundtrip -- --rows=100000 --repeat=3
//! cargo run --release -p cella-engine --example sort_roundtrip -- --rows=1000000 --modes=values,embedded,outside
//!
//! Each sample builds a fresh workbook. Only commands are timed; validation between commands
//! reads a bounded sample of fixture rows and does not keep a snapshot of the workbook.

#![allow(clippy::disallowed_methods)] // native benchmark; the clock is the measurement

use std::process::ExitCode;
use std::time::Instant;

use cella_core::{CellRef, Value};
use cella_engine::Session;
use serde_json::{Value as Json, json};

const MAX_ROWS: u32 = 1_000_000;
const CHUNK: u32 = 10_000;
const KEYS: u32 = 97;
const MAX_SAMPLES: u32 = 128;
const USAGE: &str = "usage: sort_roundtrip [--rows=N] [--repeat=N] [--warmup=N] [--modes=values,embedded,outside]\n\
                    defaults: 100000 rows (2..=1000000), 1 measured round (1..=10), 0 warmup rounds (0..=2), all three modes";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Values,
    Embedded,
    Outside,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Self::Values => "values",
            Self::Embedded => "embedded",
            Self::Outside => "outside",
        }
    }
}

struct Options {
    rows: u32,
    repeat: usize,
    warmup: usize,
    modes: Vec<Mode>,
}

fn bounded<T: std::str::FromStr + PartialOrd + std::fmt::Display>(raw: &str, min: T, max: T) -> Result<T, String> {
    let n: T = raw.parse().map_err(|_| format!("expected an integer between {min} and {max}"))?;
    if n < min || n > max {
        return Err(format!("expected an integer between {min} and {max}, got {n}"));
    }
    Ok(n)
}

fn modes(raw: &str) -> Result<Vec<Mode>, String> {
    let mut out = Vec::new();
    for name in raw.split(',') {
        let mode = match name {
            "values" => Mode::Values,
            "embedded" => Mode::Embedded,
            "outside" => Mode::Outside,
            _ => return Err(format!("unknown mode {name:?}; use values,embedded,outside")),
        };
        if out.contains(&mode) {
            return Err(format!("duplicate mode {name:?}"));
        }
        out.push(mode);
    }
    Ok(out)
}

fn options(args: impl IntoIterator<Item = String>) -> Result<Option<Options>, String> {
    let mut opts = Options { rows: 100_000, repeat: 1, warmup: 0, modes: vec![Mode::Values, Mode::Embedded, Mode::Outside] };
    for arg in args {
        if matches!(arg.as_str(), "--help" | "-h") {
            return Ok(None);
        }
        match arg.split_once('=') {
            Some(("--rows", raw)) => opts.rows = bounded(raw, 2, MAX_ROWS)?,
            Some(("--repeat", raw)) => opts.repeat = bounded(raw, 1, 10)?,
            Some(("--warmup", raw)) => opts.warmup = bounded(raw, 0, 2)?,
            Some(("--modes", raw)) => opts.modes = modes(raw)?,
            _ => return Err(format!("unknown argument {arg:?}\n{USAGE}")),
        }
    }
    Ok(Some(opts))
}

fn exec(s: &mut Session, id: &str, params: Json) -> Result<Json, String> {
    s.execute(id, params).map_err(|e| format!("{id}: {e}"))
}

fn timed(s: &mut Session, id: &str, params: Json) -> Result<(Json, f64), String> {
    let start = Instant::now();
    let reply = exec(s, id, params)?;
    Ok((reply, start.elapsed().as_secs_f64() * 1000.0))
}

fn build(rows: u32, mode: Mode) -> Result<Session, String> {
    let mut s = Session::new();
    s.new_workbook();
    let mut offset = 0;
    while offset < rows {
        let end = offset.saturating_add(CHUNK).min(rows);
        let values: Vec<_> = (offset..end)
            .map(|id| {
                let source = if id.is_multiple_of(17) { Json::Null } else { json!(id + 200) };
                json!([id % KEYS, id, id + 100, id * 2 + 100, source, id % 31])
            })
            .collect();
        exec(&mut s, "range.setValues", json!({"range": format!("A{}", offset + 1), "values": values}))?;
        offset = end;
    }
    // E1 is stored but blank. Other multiples of 17 in E are genuinely absent.
    exec(&mut s, "home.bold", json!({"range": "E1", "on": true}))?;
    let formula_col = match mode {
        Mode::Values => None,
        Mode::Embedded => Some('D'),
        Mode::Outside => Some('G'),
    };
    if let Some(col) = formula_col {
        let source = format!("{col}1");
        exec(&mut s, "cell.set", json!({"cell": source, "input": "=B1+C1"}))?;
        exec(&mut s, "edit.autoFill", json!({"source": source, "target": format!("{col}1:{col}{rows}")}))?;
    }
    Ok(s)
}

/// Independent oracle: sort by descending `id % KEYS`, preserving ascending `id` within a tie.
fn sorted_id(rows: u32, mut index: u32) -> Option<u32> {
    for key in (0..KEYS.min(rows)).rev() {
        let group = (rows - 1 - key) / KEYS + 1;
        if index < group {
            return Some(key + index * KEYS);
        }
        index -= group;
    }
    None
}

fn sample_rows(rows: u32, sorted: bool) -> Vec<u32> {
    let n = rows.min(MAX_SAMPLES);
    let mut sample: Vec<u32> = (0..n).map(|i| i * (rows - 1) / (n - 1)).collect();
    // Inspect the styled blank and an adjacent pair of tied rows, including at 1M rows.
    if sorted {
        sample.push(rows - ((rows - 1) / KEYS + 1));
        sample.push(1);
    }
    if let Some(hole) = (rows > 17).then(|| if sorted { (18..KEYS.min(rows)).map(|key| (rows - 1 - key) / KEYS + 1).sum() } else { 17 }) {
        sample.push(hole);
    }
    sample.sort_unstable();
    sample.dedup();
    sample
}

fn check_workbook(s: &Session, rows: u32, mode: Mode, sorted: bool) -> Result<u64, String> {
    let doc = s.doc().map_err(|e| e.to_string())?;
    let sheet = doc.wb.sheet(0).ok_or("fixture has no sheet")?;
    let mut digest = 0xcbf29ce484222325u64;
    for row in sample_rows(rows, sorted) {
        let id = if sorted { sorted_id(rows, row).ok_or("sort oracle has no row")? } else { row };
        let value = |col: u32| sheet.value(CellRef::new(row, col));
        for (col, expected) in [(0, id % KEYS), (1, id), (2, id + 100), (3, id * 2 + 100), (5, id % 31)] {
            if value(col) != Value::Number(f64::from(expected)) {
                return Err(format!("row {}, column {}: expected {expected}, got {:?}", row + 1, col + 1, value(col)));
            }
        }
        let fifth = sheet.cell(CellRef::new(row, 4));
        match fifth {
            Some(c) if id == 0 && c.value.is_empty() && c.style != cella_model::StyleId::DEFAULT => {}
            None if id != 0 && id.is_multiple_of(17) => {}
            Some(c) if !id.is_multiple_of(17) && c.value == Value::Number(f64::from(id + 200)) => {}
            other => return Err(format!("row {}: wrong payload in column E for source ID {id}: {other:?}", row + 1)),
        }
        let formula_col = match mode {
            Mode::Values => None,
            Mode::Embedded => Some(3),
            Mode::Outside => Some(6),
        };
        if let Some(col) = formula_col {
            let cell = sheet.cell(CellRef::new(row, col)).ok_or_else(|| format!("row {}: formula cell is missing", row + 1))?;
            let expected = format!("B{}+C{}", row + 1, row + 1);
            if cell.formula.as_ref().map(|f| f.text.as_str()) != Some(expected.as_str()) || cell.value != Value::Number(f64::from(id * 2 + 100)) {
                return Err(format!("row {}: wrong reference or cached result: {cell:?}", row + 1));
            }
        }
        for token in [row, id] {
            digest = (digest ^ u64::from(token)).wrapping_mul(0x100000001b3);
        }
    }
    Ok(digest)
}

struct Sample {
    times: [f64; 3],
    dirty: [usize; 3],
    undo_restore: Option<usize>,
    redo_restore: Option<usize>,
    before: u64,
    sorted: u64,
}

fn roundtrip(rows: u32, mode: Mode) -> Result<Sample, String> {
    let mut s = build(rows, mode)?;
    let before = check_workbook(&s, rows, mode, false)?;
    let range = format!("A1:F{rows}");
    let (sort, sort_ms) = timed(&mut s, "data.sort", json!({"range": range, "header": false, "keys": [{"column": "A", "order": "desc"}]}))?;
    if sort.get("sorted").and_then(Json::as_str) != Some(range.as_str()) {
        return Err(format!("unexpected sort reply: {sort}"));
    }
    let sort_dirty = s.doc().map_err(|e| e.to_string())?.calc.last_recalc_cells;
    let sorted = check_workbook(&s, rows, mode, true)?;
    let (undo, undo_ms) = timed(&mut s, "edit.undo", json!({}))?;
    require_history(&undo, "undone")?;
    let doc = s.doc().map_err(|e| e.to_string())?;
    let (undo_dirty, undo_restore) = (doc.calc.last_recalc_cells, doc.calc.last_restore_cells);
    if check_workbook(&s, rows, mode, false)? != before {
        return Err("undo checksum differs from the unsorted fixture".into());
    }
    let (redo, redo_ms) = timed(&mut s, "edit.redo", json!({}))?;
    require_history(&redo, "redone")?;
    let doc = s.doc().map_err(|e| e.to_string())?;
    let (redo_dirty, redo_restore) = (doc.calc.last_recalc_cells, doc.calc.last_restore_cells);
    if check_workbook(&s, rows, mode, true)? != sorted {
        return Err("redo checksum differs from the sorted fixture".into());
    }
    if undo_dirty != 0 || redo_dirty != 0 {
        return Err(format!("history unexpectedly recalculated cells: undo={undo_dirty}, redo={redo_dirty}"));
    }
    Ok(Sample { times: [sort_ms, undo_ms, redo_ms], dirty: [sort_dirty, undo_dirty, redo_dirty], undo_restore, redo_restore, before, sorted })
}

fn require_history(reply: &Json, key: &str) -> Result<(), String> {
    if !reply.get(key).and_then(Json::as_str).is_some_and(|label| !label.is_empty()) {
        return Err(format!("empty {key} label: {reply}"));
    }
    Ok(())
}

fn median(mut values: Vec<f64>) -> Result<f64, String> {
    values.sort_unstable_by(f64::total_cmp);
    let mid = values.len() / 2;
    let upper = *values.get(mid).ok_or("no timing samples")?;
    Ok(if values.len().is_multiple_of(2) { (*values.get(mid - 1).ok_or("no lower median")? + upper) / 2.0 } else { upper })
}

fn run() -> Result<(), String> {
    let Some(opts) = options(std::env::args().skip(1))? else {
        println!("{USAGE}");
        return Ok(());
    };
    if cfg!(debug_assertions) {
        eprintln!("sort_roundtrip: debug build; use --release for comparable timings");
    }
    let mut samples = vec![Vec::new(); opts.modes.len()];
    println!(
        "round,position,mode,rows,sort_ms,undo_ms,redo_ms,sort_recalc,undo_recalc,redo_recalc,undo_restored,redo_restored,before_checksum,sorted_checksum"
    );
    for round in 0..(opts.warmup + opts.repeat) {
        for position in 0..opts.modes.len() {
            let index = (round + position) % opts.modes.len();
            let mode = *opts.modes.get(index).ok_or("bad mode order")?;
            let sample = roundtrip(opts.rows, mode).map_err(|e| format!("{} round {}: {e}", mode.name(), round + 1))?;
            if round >= opts.warmup {
                let [sort, undo, redo] = sample.times;
                let [sort_dirty, undo_dirty, redo_dirty] = sample.dirty;
                let count = |n: Option<usize>| n.map_or_else(String::new, |n| n.to_string());
                println!(
                    "{},{},{},{},{sort:.3},{undo:.3},{redo:.3},{sort_dirty},{undo_dirty},{redo_dirty},{},{},{:016x},{:016x}",
                    round + 1 - opts.warmup,
                    position + 1,
                    mode.name(),
                    opts.rows,
                    count(sample.undo_restore),
                    count(sample.redo_restore),
                    sample.before,
                    sample.sorted,
                );
                samples.get_mut(index).ok_or("bad sample index")?.push(sample.times);
            }
        }
    }
    for (mode, times) in opts.modes.iter().zip(samples) {
        let [mut sorts, mut undos, mut redos] = [Vec::new(), Vec::new(), Vec::new()];
        for [sort, undo, redo] in times {
            sorts.push(sort);
            undos.push(undo);
            redos.push(redo);
        }
        eprintln!("{}: medians sort={:.3} ms undo={:.3} ms redo={:.3} ms", mode.name(), median(sorts)?, median(undos)?, median(redos)?);
    }
    Ok(())
}

fn main() -> ExitCode {
    if let Err(e) = run() {
        eprintln!("sort_roundtrip: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oracle_retains_original_order_for_ties() {
        for rows in [2, 97, 98, 241] {
            let mut expected: Vec<u32> = (0..rows).collect();
            expected.sort_by_key(|id| std::cmp::Reverse(id % KEYS));
            assert_eq!((0..rows).map(|i| sorted_id(rows, i)).collect::<Option<Vec<_>>>(), Some(expected));
        }
    }

    #[test]
    fn sort_history_preserves_the_three_fixture_shapes() {
        for mode in [Mode::Values, Mode::Embedded, Mode::Outside] {
            let sample = roundtrip(241, mode).unwrap();
            assert_ne!(sample.before, sample.sorted);
            let [sort_dirty, undo_dirty, redo_dirty] = sample.dirty;
            assert_eq!((undo_dirty, redo_dirty), (0, 0));
            assert_eq!(sort_dirty == 0, mode == Mode::Values);
            assert!(sort_dirty <= 241);
        }
    }
}
