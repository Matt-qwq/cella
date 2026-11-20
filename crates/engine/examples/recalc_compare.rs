//! Compare the edit in `perf.rs` on built and reopened workbooks, with and without its aggregates.
//!
//! ```text
//! cargo run --release -p cella-engine --example recalc_compare -- --rows=100000 --repeat=4 --warmup=1
//! cargo run --release -p cella-engine --example recalc_compare -- --rows=2500 --check-full
//! cargo run --release -p cella-engine --example recalc_compare -- --rows=1000000 --aggregates=none,sum,sumifs,xlookup,all
//! cargo run --release -p cella-engine --example recalc_compare -- --rows=1000000 --aggregates=ordinary
//! cargo run --release -p cella-engine --example recalc_compare -- --rows=1000000 --repeat=4 --full-on-open
//! ```
//!
//! Each sample uses a fresh session. Setup, XLSX round-trip, and assertions are outside the edit
//! timer. `CELLA_PROFILE=1` also logs phases for setup/warmups; use the `timed edit` markers to find
//! the edit phases and turn profiling off when collecting comparable wall times.

#![allow(clippy::disallowed_methods)] // native-only benchmark; the clock is the measurement

use std::process::ExitCode;
use std::time::Instant;

use cella_engine::Session;
use serde_json::{Value, json};

const CHUNK: usize = 10_000;
const MAX_ROWS: usize = 1_000_000;
const LOOKUP_ROW_INDEX: usize = 77_777;
const EDIT: &str = "5";
const USAGE: &str = "usage: recalc_compare [--rows=N] [--repeat=N] [--warmup=N] [--aggregates=LIST] [--check-full] [--full-on-open]\n\
                    defaults: 100000 rows (3..=1000000), 1 measured round (1..=10), 0 discarded rounds (0..=2)\n\
                    --aggregates selects none,sum,sumifs,xlookup,all,ordinary (default: none,all); both workbook states run for each\n                    ordinary adds a K column of =H*2+B the B1 edit dirties through the chain: a million per-cell ordinary formulas, not chain-shaped\n\
                    --check-full adds an edit that changes the result, then verifies against full recalc, outside the timer\n\
                    --full-on-open measures cached and forced-recalc XLSX opens on the chain-only fixture";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Aggregates {
    None,
    Sum,
    Sumifs,
    Xlookup,
    All,
    /// The ordinary-formula workload: a K column of `=Hn*2+Bn` that the B1 edit dirties through the
    /// chain — a million per-cell ordinary formulas, recalculated serially (each reads a dirty cell,
    /// so the parallel wave leaves them to the pass) and not chain-shaped (the fast chain run does
    /// not take them). It is the workload that gives the graph a precedent with **two** dependents
    /// (Hn is read by Hn+1 and by Kn), the shape the dense one-dependent box refuses.
    Ordinary,
}

impl Aggregates {
    fn includes(self, one: Self) -> bool {
        self == Self::All || self == one
    }

    fn count(self) -> usize {
        match self {
            Self::None | Self::Ordinary => 0,
            Self::All => 3,
            _ => 1,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Ordinary => "ordinary",
            Self::None => "no-aggregate",
            Self::Sum => "sum",
            Self::Sumifs => "sumifs",
            Self::Xlookup => "xlookup",
            Self::All => "aggregate",
        }
    }
}

fn parse_aggregates(raw: &str) -> Result<Vec<Aggregates>, String> {
    let mut selected = Vec::new();
    for name in raw.split(',') {
        let aggregate = match name {
            "none" => Aggregates::None,
            "sum" => Aggregates::Sum,
            "sumifs" => Aggregates::Sumifs,
            "xlookup" => Aggregates::Xlookup,
            "all" => Aggregates::All,
            "ordinary" => Aggregates::Ordinary,
            _ => return Err(format!("unknown aggregate {name:?}; use none,sum,sumifs,xlookup,all,ordinary")),
        };
        if selected.contains(&aggregate) {
            return Err(format!("duplicate aggregate {name:?}"));
        }
        selected.push(aggregate);
    }
    Ok(selected)
}

#[derive(Clone, Copy)]
struct Case {
    reopened: bool,
    aggregates: Aggregates,
}

impl Case {
    fn name(self) -> String {
        let state = if self.reopened { "reopened" } else { "built" };
        format!("{state}/{}", self.aggregates.label())
    }
}

/// With an even number of cases, this order balances position and immediate predecessor over
/// a complete cycle. At four cases its rounds are exactly the original default schedule.
fn case_index(round: usize, position: usize, cases: usize) -> usize {
    let offset = if position.is_multiple_of(2) { cases - position / 2 } else { position.div_ceil(2) };
    (round + offset) % cases
}

struct Options {
    rows: usize,
    repeat: usize,
    warmup: usize,
    check_full: bool,
    full_on_open: bool,
    aggregates: Vec<Aggregates>,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("recalc_compare: {e}");
            ExitCode::FAILURE
        }
    }
}

fn bounded(raw: &str, name: &str, min: usize, max: usize) -> Result<usize, String> {
    raw.parse::<usize>().ok().filter(|n| (min..=max).contains(n)).ok_or_else(|| format!("{name} must be {min}..={max}"))
}

fn options() -> Result<Option<Options>, String> {
    let mut opts =
        Options { rows: 100_000, repeat: 1, warmup: 0, check_full: false, full_on_open: false, aggregates: vec![Aggregates::None, Aggregates::All] };
    let mut aggregates_given = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            "--check-full" => opts.check_full = true,
            "--full-on-open" => opts.full_on_open = true,
            _ => match arg.split_once('=') {
                Some(("--rows", raw)) => opts.rows = bounded(raw, "--rows", 3, MAX_ROWS)?,
                Some(("--repeat", raw)) => opts.repeat = bounded(raw, "--repeat", 1, 10)?,
                Some(("--warmup", raw)) => opts.warmup = bounded(raw, "--warmup", 0, 2)?,
                Some(("--aggregates", raw)) => {
                    opts.aggregates = parse_aggregates(raw)?;
                    aggregates_given = true;
                }
                _ => return Err(format!("unknown argument {arg}\n{USAGE}")),
            },
        }
    }
    if opts.full_on_open && (opts.check_full || aggregates_given) {
        return Err("--full-on-open cannot be combined with --check-full or --aggregates".into());
    }
    if opts.check_full && opts.aggregates.contains(&Aggregates::Ordinary) {
        return Err("--check-full cannot be combined with --aggregates=ordinary: the workload's full recalc covers three columns, not the chain fixture's two".into());
    }
    Ok(Some(opts))
}

fn run() -> Result<(), String> {
    let Some(opts) = options()? else { return Ok(()) };
    if cfg!(debug_assertions) {
        eprintln!("recalc_compare: debug build — use --release for comparable timings");
    }
    let expected = Expected::new(opts.rows);
    if opts.full_on_open {
        return measure_open(&opts, &expected);
    }
    let cases: Vec<Case> =
        [false, true].into_iter().flat_map(|reopened| opts.aggregates.iter().map(move |&aggregates| Case { reopened, aggregates })).collect();
    let mut samples = vec![Vec::<f64>::new(); cases.len()];
    println!("round,position,case,rows,edit_ms,recalc_cells,undo_before,open_recalc_cells,tail");
    for round in 0..opts.warmup + opts.repeat {
        let warmup = round < opts.warmup;
        let label = if warmup { format!("warmup {}", round + 1) } else { format!("round {}", round + 1 - opts.warmup) };
        for position in 0..cases.len() {
            let index = case_index(round, position, cases.len());
            let case = cases.get(index).ok_or("bad case order")?;
            let name = case.name();
            let m = measure(*case, &opts, &expected, &label, &name).map_err(|e| format!("{label} {name}: {e}"))?;
            if !warmup {
                println!(
                    "{},{},{},{},{:.3},{},{},{},{}",
                    round + 1 - opts.warmup,
                    position + 1,
                    name,
                    opts.rows,
                    m.ms,
                    m.recalc_cells,
                    m.undo_before,
                    m.open_recalc_cells.map_or_else(String::new, |n| n.to_string()),
                    expected.tail
                );
                samples.get_mut(index).ok_or("bad sample index")?.push(m.ms);
            }
        }
    }
    for (case, times) in cases.iter().zip(&mut samples) {
        print_median(&case.name(), times)?;
    }
    Ok(())
}

fn print_median(name: &str, times: &mut [f64]) -> Result<(), String> {
    times.sort_unstable_by(f64::total_cmp);
    let mid = times.len() / 2;
    let upper = *times.get(mid).ok_or("no samples")?;
    let median = if times.len().is_multiple_of(2) { (*times.get(mid.saturating_sub(1)).ok_or("no lower midpoint")? + upper) / 2.0 } else { upper };
    eprintln!("{name}: median {median:.3} ms (n={})", times.len());
    Ok(())
}

fn exec(session: &mut Session, id: &str, params: Value) -> Result<Value, String> {
    session.execute(id, params).map_err(|e| format!("{id}: {e}"))
}

fn set(session: &mut Session, cell: &str, input: &str) -> Result<(), String> {
    exec(session, "cell.set", json!({"cell": cell, "input": input})).map(|_| ())
}

/// Keep these inputs and setup commands aligned with `perf.rs`; the gate and the A/B are separate
/// so its original scenario timing and JSON schema are unaffected.
fn build(rows: usize, aggregates: Aggregates) -> Result<Session, String> {
    let mut session = Session::new();
    session.new_workbook();
    for start in (0..rows).step_by(CHUNK) {
        let end = (start + CHUNK).min(rows);
        let values: Vec<Value> =
            (start..end).map(|n| json!([format!("Item {n}"), n as f64, (n % 97) as f64, (n % 13) as f64 * 1.5, 1.0, 2.0])).collect();
        exec(&mut session, "range.setValues", json!({"range": format!("A{}", start + 1), "values": values}))?;
    }
    set(&mut session, "G1", "=B1*C1+D1")?;
    exec(&mut session, "edit.autoFill", json!({"source": "G1", "target": format!("G1:G{rows}")}))?;
    set(&mut session, "H1", "=G1")?;
    set(&mut session, "H2", "=H1+G2")?;
    exec(&mut session, "edit.autoFill", json!({"source": "H2", "target": format!("H2:H{rows}")}))?;
    if aggregates.includes(Aggregates::Sum) {
        set(&mut session, "J1", &format!("=SUM(G1:G{rows})"))?;
    }
    if aggregates.includes(Aggregates::Sumifs) {
        set(&mut session, "J2", "=SUMIFS(B:B,C:C,\">50\")")?;
    }
    if aggregates.includes(Aggregates::Xlookup) {
        set(&mut session, "J3", "=XLOOKUP(\"Item 77777\",A:A,B:B)")?;
    }
    if aggregates == Aggregates::Ordinary {
        // The ordinary-formula column: each cell reads the chain beside it and its own row, so the
        // B1 edit dirties all of it — a million per-cell ordinary formulas next to the chain, serial
        // in the pass (not leaves) and not a chain shape (not the fast run).
        set(&mut session, "K1", "=H1*2+B1")?;
        exec(&mut session, "edit.autoFill", json!({"source": "K1", "target": format!("K1:K{rows}")}))?;
    }
    Ok(session)
}

fn open_params(session: &mut Session) -> Result<Value, String> {
    let mut saved = exec(session, "file.saveBytes", json!({"format": "xlsx"}))?;
    let base64 = saved.get_mut("base64").map(Value::take).filter(Value::is_string).ok_or("saveBytes returned no base64")?;
    Ok(json!({"name": "recalc-compare.xlsx", "base64": base64}))
}

fn reopen(mut session: Session) -> Result<Session, String> {
    let params = open_params(&mut session)?;
    // Do not give file.open an existing document: it normally adds a second one. Use a fresh
    // session so the previous undo/journal and workbook aren't retained during the reopened edit.
    drop(session);
    let mut session = Session::new();
    exec(&mut session, "file.open", params)?;
    Ok(session)
}

fn measure_open(opts: &Options, expected: &Expected) -> Result<(), String> {
    let mut built = build(opts.rows, Aggregates::None)?;
    let cached = open_params(&mut built)?;
    let doc = built.doc_mut().map_err(|e| e.to_string())?;
    std::sync::Arc::make_mut(&mut doc.wb).calc.full_calc_on_load = true;
    let forced = open_params(&mut built)?;
    drop(built);

    let cases = [("cached", false, cached), ("forced", true, forced)];
    let mut samples = [Vec::<f64>::new(), Vec::<f64>::new()];
    println!("round,position,case,rows,open_ms,recalc_cells,tail");
    for round in 0..opts.warmup + opts.repeat {
        for position in 0..cases.len() {
            let index = case_index(round, position, cases.len());
            let (name, forced, fixture) = cases.get(index).ok_or("bad open case order")?;
            let params = fixture.clone();
            let mut session = Session::new();
            let start = Instant::now();
            exec(&mut session, "file.open", params)?;
            let ms = start.elapsed().as_secs_f64() * 1000.0;

            let doc = session.doc().map_err(|e| e.to_string())?;
            let recalc_cells = doc.calc.last_recalc_cells;
            let wanted = if *forced { 2 * opts.rows } else { 0 };
            if doc.wb.calc.full_calc_on_load != *forced || recalc_cells != wanted {
                return Err(format!(
                    "{name}: expected fullCalcOnLoad={forced} and {wanted} recalculated cells, got {} and {recalc_cells}",
                    doc.wb.calc.full_calc_on_load
                ));
            }
            expected.check(&mut session, opts.rows, Aggregates::None, false, 0.0)?;
            if round >= opts.warmup {
                println!("{},{},{name},{},{ms:.3},{recalc_cells},{}", round + 1 - opts.warmup, position + 1, opts.rows, expected.tail);
                samples.get_mut(index).ok_or("bad open sample index")?.push(ms);
            }
        }
    }
    for ((name, _, _), times) in cases.iter().zip(&mut samples) {
        print_median(name, times)?;
    }
    Ok(())
}

struct Expected {
    tail: f64,
    sumifs: f64,
    lookup: Value,
}

impl Expected {
    fn new(rows: usize) -> Self {
        // Each term is an integer or half-integer, and the sum is exactly representable as f64
        // for all supported row counts. B1 starts at 0; C1 and D1 are 0, so B1=5 changes no result.
        let tail = (0..rows).map(|n| (n * (n % 97)) as f64 + (n % 13) as f64 * 1.5).sum();
        let sumifs = (0..rows).filter(|n| n % 97 > 50).map(|n| n as f64).sum();
        let lookup = if LOOKUP_ROW_INDEX < rows { json!(LOOKUP_ROW_INDEX as f64) } else { json!({"error": "#N/A"}) };
        Self { tail, sumifs, lookup }
    }

    fn check(&self, session: &mut Session, rows: usize, aggregates: Aggregates, edited: bool, g1: f64) -> Result<(), String> {
        if aggregates == Aggregates::Ordinary {
            // The ordinary-formula workload adds a K column that answers 2*H + B in every row; the
            // B1 edit (the same one every other case times) changes only K1, since C1 is zero and
            // the chain never moves.
            let b1 = if edited { 5.0 } else { 0.0 };
            check_cell(session, "B1", &json!(b1))?;
            check_cell(session, "G1", &json!(g1))?;
            check_cell(session, &format!("H{rows}"), &json!(self.tail + g1))?;
            check_cell(session, "K1", &json!(2.0 * g1 + b1))?;
            check_cell(session, &format!("K{rows}"), &json!(2.0 * (self.tail + g1) + (rows - 1) as f64))?;
            return Ok(());
        }
        check_cell(session, "B1", &json!(if edited { 5.0 } else { 0.0 }))?;
        check_cell(session, "G1", &json!(g1))?;
        check_cell(session, &format!("H{rows}"), &json!(self.tail + g1))?;
        if aggregates.includes(Aggregates::Sum) {
            check_cell(session, "J1", &json!(self.tail + g1))?;
        }
        if aggregates.includes(Aggregates::Sumifs) {
            check_cell(session, "J2", &json!(self.sumifs))?;
        }
        if aggregates.includes(Aggregates::Xlookup) {
            check_cell(session, "J3", &self.lookup)?;
        }
        Ok(())
    }
}

fn check_cell(session: &mut Session, cell: &str, expected: &Value) -> Result<(), String> {
    let result = exec(session, "cell.get", json!({"cell": cell}))?;
    match result.get("value") {
        Some(actual) if actual == expected => Ok(()),
        actual => Err(format!("{cell}: expected {expected}, got {actual:?}")),
    }
}

struct Measurement {
    ms: f64,
    recalc_cells: usize,
    undo_before: usize,
    open_recalc_cells: Option<usize>,
}

fn measure(case: Case, opts: &Options, expected: &Expected, label: &str, name: &str) -> Result<Measurement, String> {
    eprintln!("recalc_compare: {label} {name} setup");
    let built = build(opts.rows, case.aggregates)?;
    let mut session = if case.reopened { reopen(built)? } else { built };
    let doc = session.doc().map_err(|e| e.to_string())?;
    let undo_before = doc.undo.len();
    let open_recalc_cells = case.reopened.then_some(doc.calc.last_recalc_cells);
    expected.check(&mut session, opts.rows, case.aggregates, false, 0.0)?;

    eprintln!("recalc_compare: {label} {name} timed edit begin");
    let start = Instant::now();
    set(&mut session, "B1", EDIT)?;
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    eprintln!("recalc_compare: {label} {name} timed edit end");

    let recalc_cells = session.doc().map_err(|e| e.to_string())?.calc.last_recalc_cells;
    // The ordinary workload's B1 edit dirties the chain plus the K column beside it; every other
    // case's B1 edit dirties the chain and G1 only.
    let wanted = if case.aggregates == Aggregates::Ordinary { 2 * opts.rows + 1 } else { opts.rows + 1 + case.aggregates.count() };
    if recalc_cells != wanted {
        return Err(format!("expected {wanted} recalculated cells, got {recalc_cells}"));
    }
    expected.check(&mut session, opts.rows, case.aggregates, true, 0.0)?;
    if opts.check_full {
        // The original B1 edit leaves G1 unchanged (C1 is zero). Make a separate edit that must
        // change every running total; otherwise stale results could pass the full-recalc check.
        eprintln!("recalc_compare: {label} {name} untimed result-changing edit and full recalc");
        set(&mut session, "C1", "2")?;
        expected.check(&mut session, opts.rows, case.aggregates, true, 10.0)?;
        let full = exec(&mut session, "formulas.calculateNow", json!({}))?;
        let wanted = 2 * opts.rows + case.aggregates.count();
        if full.get("cells").and_then(Value::as_u64) != Some(wanted as u64) {
            return Err(format!("expected {wanted} cells in full recalc, got {full}"));
        }
        expected.check(&mut session, opts.rows, case.aggregates, true, 10.0)?;
    }
    Ok(Measurement { ms, recalc_cells, undo_before, open_recalc_cells })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_selector_accepts_each_workload_once() {
        let modes = parse_aggregates("none,sum,sumifs,xlookup,all").unwrap();
        assert_eq!(modes.iter().map(|mode| mode.label()).collect::<Vec<_>>(), ["no-aggregate", "sum", "sumifs", "xlookup", "aggregate"]);
        assert_eq!(modes.iter().map(|mode| mode.count()).collect::<Vec<_>>(), [0, 1, 1, 1, 3]);
        assert!(parse_aggregates("").is_err());
        assert!(parse_aggregates("sum,sum").is_err());
        assert!(parse_aggregates("all,").is_err());
        let ordinary = parse_aggregates("ordinary").unwrap();
        assert_eq!(ordinary.iter().map(|mode| mode.label()).collect::<Vec<_>>(), ["ordinary"]);
        assert_eq!(ordinary.iter().map(|mode| mode.count()).collect::<Vec<_>>(), [0]);
    }

    #[test]
    fn case_order_balances_each_supported_even_width() {
        for cases in (2..=10).step_by(2) {
            let mut positions = vec![vec![0; cases]; cases];
            let mut follows = vec![vec![0; cases]; cases];
            for round in 0..cases {
                let order: Vec<_> = (0..cases).map(|position| case_index(round, position, cases)).collect();
                for (position, &index) in order.iter().enumerate() {
                    positions[index][position] += 1;
                }
                for pair in order.windows(2) {
                    follows[pair[0]][pair[1]] += 1;
                }
            }
            assert!(positions.iter().flatten().all(|&count| count == 1));
            for (earlier, row) in follows.iter().enumerate() {
                assert!(row.iter().enumerate().all(|(later, &count)| count == usize::from(earlier != later)));
            }
        }
    }
}
