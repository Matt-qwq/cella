//! `cargo xtask perf`: the performance gate.
//!
//! Runs the engine's measurement harness (`crates/engine/examples/perf.rs`) in release mode and
//! compares every scenario with the budgets checked in at `xtask/perf-budgets.txt`. A scenario
//! that is slower than its budget fails the command, so a throughput regression is caught by a gate
//! instead of by somebody remembering the old number.
//!
//! - `cargo xtask perf` — measure, compare, fail on a regression (the pre-release gate)
//! - `cargo xtask perf --update` — record the current numbers as the new budgets
//! - `--rows=100000,1000000` / `--scenario=fill.values,sort.desc` — run a subset
//! - `--repeat=N` — let the harness keep each scenario's fastest of N passes (use it with
//!   `--update` on a machine whose load fluctuates)
//!
//! A scenario that is over budget is measured once more before the run fails: a build that started
//! on the same machine is a slow pass, not a regression, and a real one stays over budget twice.
//! - `--budgets=PATH` — a different budget file; `--scale=F` (or `CELLA_PERF_SCALE`)
//!   multiplies every budget, for a slower or shared machine
//!
//! Two scenarios the harness cannot express the gate runs itself, each as its own release
//! subprocess (the engine side stays a pure library consumer):
//! - `mem_drift` — the long-run soak of `crates/engine/examples/mem_drift.rs`, 100k rows for
//!   240 s (edit/undo/redo cycles with a sort every 50 and a save every 200). Its budget line is
//!   the soak's per-cycle cost; the leak assertion — the engine's own reported total flat within
//!   ±2% over the second half — is part of this gate, not of the budget file.
//! - `save.repeat` — the save micro-benchmark of `xtask/examples/save_bench.rs`: the reference
//!   workbook (the exact state `save.xlsx` measures) saved six times in one process, best pass
//!   kept. That repetition holds the ~1% noise floor at which a save change can be certified
//!   instead of argued about.
//!
//! It is deliberately **not** part of `cargo xtask ci`: a release build of the engine is not part
//! of the per-commit gate. See docs/performance.md.

use std::path::{Path, PathBuf};
use std::process::Stdio;

/// Headroom `--update` records on top of a measurement, so a budget is a regression threshold and
/// not a copy of one noisy sample. 2x: a loaded pass on the dev machine (three agents building at
/// once) reached 1.5x the quiet number, so this is the smallest round value that keeps the gate
/// honest about *noise* — it fails on a real regression (2x and worse), and the recorded table in
/// docs/performance.md is what shows smaller deltas.
const HEADROOM: f64 = 2.0;
/// No recorded budget is tighter than this: scenarios that finish in microseconds are all noise,
/// and a 0.2 ms budget would flap on a loaded machine.
const MIN_BUDGET_MS: f64 = 1.0;
/// The rows the gate measures (the harness default).
const DEFAULT_ROWS: [usize; 2] = [100_000, 1_000_000];

/// The long-run drift soak the gate runs after the scenarios: the exact shape the 2026-11-20
/// round measured (`crates/engine/examples/mem_drift.rs` at 100k rows for 240 s — ~45,900
/// edit/undo/redo cycles, a sort every 50, a save every 200).
const DRIFT_SCENARIO: &str = "mem_drift";
const DRIFT_ROWS: usize = 100_000;
const DRIFT_SECONDS: usize = 240;
/// How flat the soak's second half must be: the largest share of its median the engine's own
/// reported total may reach. The 2026-11-20 soak held 0.02%; a real leak rises by whole percents.
const DRIFT_MAX_SHARE: f64 = 0.02;
/// Second-half samples below which the flatness check refuses to pass: the soak prints one line
/// every 25 cycles, so a healthy 240 s run has hundreds and a broken one must not slip by.
const DRIFT_MIN_REPORTS: usize = 8;

/// The save micro-benchmark: `xtask/examples/save_bench.rs` builds the workbook state
/// `save.xlsx` measures, saves it six times in one process and keeps the best pass — the ~1%
/// noise floor that makes a save change certifiable instead of arguable.
const SAVE_SCENARIO: &str = "save.repeat";
const SAVE_SAVES: usize = 6;

/// One line of `xtask/perf-budgets.txt`.
#[derive(Debug, Clone, PartialEq)]
pub struct Budget {
    pub scenario: String,
    pub rows: usize,
    pub ms: f64,
}

/// One number the harness reported.
#[derive(Debug, Clone, PartialEq)]
pub struct Measured {
    pub scenario: String,
    pub rows: usize,
    pub ms: f64,
}

/// How one measurement stands against its budget.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verdict {
    Within(f64),
    Over(f64),
    /// No budget recorded for this (scenario, rows): every scenario must have one.
    Missing,
}

const HEADER: &str = "\
# Cella performance budgets — `cargo xtask perf` fails when a scenario is slower than its
# budget. What each scenario measures is in docs/performance.md.
#
# Recorded with `cargo xtask perf --update` (best of three passes × 2, never below 1 ms) on the
# dev machine, release build: a scenario must be TWICE as slow as this to fail. A slower or shared
# machine: CELLA_PERF_SCALE=2 cargo xtask perf. Re-record only for an intended change.
#
# Format: <scenario> <rows> <budget ms>
";

fn parse_budgets(text: &str) -> Result<Vec<Budget>, String> {
    let mut out: Vec<Budget> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let at = |what: &str| format!("perf-budgets line {}: {what} (`{line}`)", i + 1);
        let mut parts = line.split_whitespace();
        let (Some(scenario), Some(rows), Some(ms), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
            return Err(at("expected `<scenario> <rows> <budget ms>`"));
        };
        let rows: usize = rows.parse().map_err(|_| at("`{rows}` is not a row count"))?;
        let ms: f64 = ms.parse().map_err(|_| at("`{ms}` is not a budget in ms"))?;
        if ms <= 0.0 {
            return Err(at("the budget must be a positive number of milliseconds"));
        }
        if out.iter().any(|b| b.scenario == scenario && b.rows == rows) {
            return Err(at("duplicate entry"));
        }
        out.push(Budget { scenario: scenario.to_string(), rows, ms });
    }
    if out.is_empty() {
        return Err("perf-budgets: no entries — record them with `cargo xtask perf --update`".into());
    }
    Ok(out)
}

/// The budget file for `budgets`, sorted and column-aligned so `--update` produces a readable diff.
fn render_budgets(budgets: &[Budget]) -> String {
    let mut rows: Vec<&Budget> = budgets.iter().collect();
    rows.sort_by(|a, b| a.rows.cmp(&b.rows).then_with(|| a.scenario.cmp(&b.scenario)));
    let scenario_width = rows.iter().map(|b| b.scenario.len()).max().unwrap_or(0).max(8);
    let rows_width = rows.iter().map(|b| b.rows.to_string().len()).max().unwrap_or(0).max(4);
    let mut s = String::from(HEADER);
    for b in rows {
        s += &format!("{:<scenario_width$}  {:>rows_width$}  {}\n", b.scenario, b.rows, fmt_ms_num(b.ms));
    }
    s
}

/// A budget as a plain number with a decimal point only when it needs one (`800`, `14250`, `0.5`).
fn fmt_ms_num(ms: f64) -> String {
    let rounded = (ms * 100.0).round() / 100.0;
    format!("{rounded}")
}

fn verdict(m: &Measured, budgets: &[Budget], scale: f64) -> Verdict {
    match budgets.iter().find(|b| b.scenario == m.scenario && b.rows == m.rows) {
        None => Verdict::Missing,
        Some(b) => {
            let budget = b.ms * scale;
            if m.ms > budget { Verdict::Over(budget) } else { Verdict::Within(budget) }
        }
    }
}

/// The budget `--update` records for a measurement: the number plus headroom, rounded up to a
/// readable step, and never below the 1 ms floor.
fn budget_for(ms: f64) -> f64 {
    let target = (ms * HEADROOM).max(MIN_BUDGET_MS);
    let step = if target < 2.0 {
        0.1
    } else if target < 20.0 {
        0.5
    } else if target < 100.0 {
        1.0
    } else if target < 1_000.0 {
        10.0
    } else if target < 10_000.0 {
        50.0
    } else {
        250.0
    };
    ((target / step).ceil() * step * 100.0).round() / 100.0
}

struct Args {
    update: bool,
    rows: Vec<usize>,
    only: Option<Vec<String>>,
    repeat: Option<usize>,
    budgets: Option<PathBuf>,
    scale: f64,
}

/// Upper bound for `--repeat` (input-derived work, so it is capped).
const MAX_REPEAT: usize = 10;

fn parse_args(args: &[&str]) -> Result<Args, String> {
    let mut out = Args { update: false, rows: DEFAULT_ROWS.to_vec(), only: None, repeat: None, budgets: None, scale: env_scale() };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) => (f, Some(v)),
            None => (*a, None),
        };
        let mut value = |what: &str| -> Result<String, String> {
            match inline {
                Some(v) => Ok(v.to_string()),
                None => it.next().map(|s| (*s).to_string()).ok_or_else(|| format!("{what} needs a value")),
            }
        };
        match flag {
            "--update" => out.update = true,
            "--rows" => {
                let v = value("--rows")?;
                out.rows = v
                    .split(',')
                    .map(|r| r.trim().parse::<usize>().map_err(|_| format!("--rows: `{r}` is not a row count")))
                    .collect::<Result<Vec<_>, _>>()?;
                out.rows.sort_unstable();
                out.rows.dedup();
            }
            "--scenario" => {
                let v = value("--scenario")?;
                let ids: Vec<String> = v.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect();
                out.only = (!ids.is_empty()).then_some(ids);
            }
            "--repeat" => {
                let v = value("--repeat")?;
                let n: usize = v.parse().map_err(|_| format!("--repeat: `{v}` is not a count"))?;
                if !(1..=MAX_REPEAT).contains(&n) {
                    return Err(format!("--repeat must be 1..={MAX_REPEAT}"));
                }
                out.repeat = Some(n);
            }
            "--budgets" => out.budgets = Some(PathBuf::from(value("--budgets")?)),
            "--scale" => {
                let v = value("--scale")?;
                out.scale = v.parse().map_err(|_| format!("--scale: `{v}` is not a number"))?;
                if out.scale <= 0.0 {
                    return Err("--scale must be positive".into());
                }
            }
            other => {
                return Err(format!(
                    "perf: unknown argument `{other}` (usage: cargo xtask perf [--update] [--rows=N,N] [--scenario=ID,...] [--repeat=N] [--budgets=PATH] [--scale=F])"
                ));
            }
        }
    }
    Ok(out)
}

/// Keep the faster of two measurements of the same scenario. Keyed by (scenario, rows), not by
/// position: a re-measure covers a subset of the table, and pairing by index could land a
/// re-measured `sort.desc` on the wrong line.
fn keep_min(measured: &mut [Measured], again: &[Measured]) {
    for a in again {
        let Some(m) = measured.iter_mut().find(|m| m.scenario == a.scenario && m.rows == a.rows) else {
            continue;
        };
        if a.ms < m.ms {
            *m = a.clone();
        }
    }
}

fn env_scale() -> f64 {
    std::env::var("CELLA_PERF_SCALE").ok().and_then(|s| s.trim().parse().ok()).filter(|s| *s > 0.0).unwrap_or(1.0)
}

/// The budgets file as the report shows it: relative to the repository root when it lives there,
/// so a generated page carries no machine's absolute path.
fn shown(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).display().to_string()
}

pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let args = parse_args(args)?;
    let path = args.budgets.clone().unwrap_or_else(|| root.join("xtask/perf-budgets.txt"));
    // Read the budgets first, so a missing or malformed file fails before minutes of measuring.
    let budgets = if args.update {
        std::fs::read_to_string(&path).ok().and_then(|t| parse_budgets(&t).ok()).unwrap_or_default()
    } else {
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e} (record budgets with `cargo xtask perf --update`)", path.display()))?;
        parse_budgets(&text)?
    };
    eprintln!("perf: measuring in release — this builds the engine and takes a few minutes");
    // Three measurement legs: the harness scenarios, the save micro-benchmark, then the drift
    // soak. Each is a release subprocess; each retries only itself.
    let mut measured = measure(&args, None)?;
    let mut save = if wants(&args.only, SAVE_SCENARIO) { measure_save(&args, None)? } else { Vec::new() };
    let drift = if wants(&args.only, DRIFT_SCENARIO) { Some(measure_drift()?) } else { None };
    if args.update {
        let mut all: Vec<Measured> = measured.clone();
        all.extend(save.iter().cloned());
        all.extend(drift.iter().cloned());
        if all.is_empty() {
            return Err("perf: the filter selected no scenarios — nothing to record".into());
        }
        let mut next: Vec<Budget> = all.iter().map(|m| Budget { scenario: m.scenario.clone(), rows: m.rows, ms: budget_for(m.ms) }).collect();
        // Entries for scenarios this run did not touch (a filtered `--update`) are kept as they are.
        for b in budgets {
            if !next.iter().any(|n| n.scenario == b.scenario && n.rows == b.rows) {
                next.push(b);
            }
        }
        let budgets = next;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        }
        let text = crate::eol::for_write(&path, &render_budgets(&budgets));
        std::fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))?;
        print!("{}", render_check(&all, &budgets, 1.0, false));
        println!("\nrecorded {} budgets in {}", budgets.len(), path.display());
        return Ok(());
    }
    // A scenario over budget is measured once more before it fails the run: a build that started
    // on the same machine is a slow pass, not a regression, and a real regression stays over budget
    // twice. Missing budgets are a mistake in the file, not a timing question, so they never
    // retry. The re-measure matches by (scenario, rows), so the harness's subset and the soak
    // never cross wires.
    let mut suspects: Vec<String> = Vec::new();
    for m in &measured {
        if matches!(verdict(m, &budgets, args.scale), Verdict::Over(_)) && !suspects.contains(&m.scenario) {
            suspects.push(m.scenario.clone());
        }
    }
    if !suspects.is_empty() {
        eprintln!("perf: {} scenario(s) over budget — measuring them once more: {}", suspects.len(), suspects.join(", "));
        let again = measure(&args, Some(&suspects))?;
        keep_min(&mut measured, &again);
    }
    let mut save_rows: Vec<usize> = Vec::new();
    for m in &save {
        if matches!(verdict(m, &budgets, args.scale), Verdict::Over(_)) && !save_rows.contains(&m.rows) {
            save_rows.push(m.rows);
        }
    }
    if !save_rows.is_empty() {
        eprintln!(
            "perf: {SAVE_SCENARIO} over budget — saving it once more at {} rows",
            save_rows.iter().map(usize::to_string).collect::<Vec<_>>().join(", ")
        );
        let again = measure_save(&args, Some(&save_rows))?;
        keep_min(&mut save, &again);
    }
    let mut drift = drift;
    if drift.as_ref().is_some_and(|d| matches!(verdict(d, &budgets, args.scale), Verdict::Over(_))) {
        eprintln!("perf: {DRIFT_SCENARIO} over budget — running the soak once more");
        let again = measure_drift()?;
        if let Some(d) = drift.as_mut()
            && again.ms < d.ms
        {
            *d = again;
        }
    }
    // The verdict table: the harness scenarios in run order, then the save bench, then the soak.
    measured.extend(save);
    if let Some(d) = drift {
        measured.push(d);
    }
    if measured.is_empty() {
        return Err("perf: the filter selected no scenarios — nothing was measured".into());
    }
    println!("\nbudgets {} (scale {:.2})", shown(root, &path), args.scale);
    print!("{}", render_check(&measured, &budgets, args.scale, true));
    let mut slow: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for m in &measured {
        match verdict(m, &budgets, args.scale) {
            Verdict::Within(_) => {}
            Verdict::Over(budget) => slow.push(format!(
                "  {} @ {} rows: {} > {} ({:.0}% of budget)",
                m.scenario,
                m.rows,
                fmt_time(m.ms),
                fmt_time(budget),
                m.ms / budget * 100.0
            )),
            Verdict::Missing => missing.push(format!("  {} @ {} rows", m.scenario, m.rows)),
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "no budget for {} scenario(s) in {}:\n{}\nrecord one with `cargo xtask perf --update`",
            missing.len(),
            path.display(),
            missing.join("\n")
        ));
    }
    if !slow.is_empty() {
        return Err(format!(
            "{} of {} scenarios are over budget ({}):\n{}\nif the change is intended, re-record with `cargo xtask perf --update`",
            slow.len(),
            measured.len(),
            path.display(),
            slow.join("\n")
        ));
    }
    println!("\nOK: all {} scenarios within budget ({})", measured.len(), shown(root, &path));
    Ok(())
}

/// The measured/budget table. `compare` false prints the measurements alone (what `--update` records).
fn render_check(measured: &[Measured], budgets: &[Budget], scale: f64, compare: bool) -> String {
    let mut s = format!("\n{:<18}  {:>9}  {:>10}  {:>9}  {:>6}\n", "scenario", "rows", "measured", "budget", "of");
    s += &format!("{:<18}  {:>9}  {:>10}  {:>9}  {:>6}\n", "-".repeat(18), "-".repeat(9), "-".repeat(10), "-".repeat(9), "-".repeat(6));
    for m in measured {
        let budget = budgets.iter().find(|b| b.scenario == m.scenario && b.rows == m.rows).map(|b| b.ms * scale);
        let of = match (compare, verdict(m, budgets, scale), budget) {
            (true, Verdict::Missing, _) | (true, _, None) | (false, _, _) => "-".to_string(),
            (true, v, Some(b)) => {
                let pct = m.ms / b * 100.0;
                if matches!(v, Verdict::Over(_)) { format!("{pct:.0}%!") } else { format!("{pct:.0}%") }
            }
        };
        let budget = budget.map(fmt_time).unwrap_or_else(|| "-".to_string());
        s += &format!("{:<18}  {:>9}  {:>10}  {:>9}  {:>6}\n", m.scenario, m.rows, fmt_time(m.ms), budget, of);
    }
    s
}

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

/// Whether the perf harness has anything to do under this filter: no filter runs everything,
/// and a filter that names only the scenarios the gate measures itself (`mem_drift`,
/// `save.repeat`) leaves it nothing — the harness does not know those ids (its `--scenario`
/// validation rejects them).
fn harness_has_work(only: Option<&[String]>) -> bool {
    only.is_none_or(|ids| ids.iter().any(|id| id.as_str() != DRIFT_SCENARIO && id.as_str() != SAVE_SCENARIO))
}

/// Whether this run's `--scenario` filter includes `id`. No filter means every scenario,
/// including the ones the gate measures itself.
fn wants(only: &Option<Vec<String>>, id: &str) -> bool {
    only.as_deref().is_none_or(|ids| ids.iter().any(|want| want == id))
}

/// Run the harness in release and read its `--json` document. `only` overrides the command
/// line's scenario filter (the re-measure of the scenarios that came out over budget).
fn measure(args: &Args, only: Option<&[String]>) -> Result<Vec<Measured>, String> {
    let ids = only.or(args.only.as_deref());
    if !harness_has_work(ids) {
        // A run filtered down to the gate's own scenarios: the harness has nothing to do.
        return Ok(Vec::new());
    }
    let retry = only.is_some();
    let mut cmd = crate::cargo();
    cmd.args(["run", "--release", "-q", "-p", "cella-engine", "--example", "perf", "--", "--json"]);
    cmd.arg(format!("--rows={}", args.rows.iter().map(usize::to_string).collect::<Vec<_>>().join(",")));
    if let Some(ids) = ids {
        // The gate's own ids are not the harness's: pass the filter without them.
        let kept: Vec<String> = ids.iter().filter(|id| id.as_str() != DRIFT_SCENARIO && id.as_str() != SAVE_SCENARIO).cloned().collect();
        cmd.arg(format!("--scenario={}", kept.join(",")));
    }
    // A re-measure is one pass: the min of the two is taken, so repeating inside it would only cost time.
    if let Some(n) = args.repeat.filter(|_| !retry) {
        cmd.arg(format!("--repeat={n}"));
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::inherit());
    let out = cmd.output().map_err(|e| format!("cargo run --example perf: {e}"))?;
    if !out.status.success() {
        return Err(format!("the perf harness failed ({}); its output is above", out.status));
    }
    parse_results(&String::from_utf8_lossy(&out.stdout))
}

fn parse_results(text: &str) -> Result<Vec<Measured>, String> {
    let doc: serde_json::Value = serde_json::from_str(text.trim()).map_err(|e| format!("the perf harness did not print JSON: {e}"))?;
    let results = doc.get("results").and_then(serde_json::Value::as_array).ok_or("the perf harness printed no `results`")?;
    let mut out = Vec::new();
    for r in results {
        let scenario = r.get("scenario").and_then(serde_json::Value::as_str).ok_or("a result has no `scenario`")?;
        let rows = r.get("rows").and_then(serde_json::Value::as_u64).ok_or("a result has no `rows`")?;
        let ms = r.get("ms").and_then(serde_json::Value::as_f64).ok_or("a result has no `ms`")?;
        let rows = usize::try_from(rows).map_err(|_| format!("row count {rows} does not fit this platform"))?;
        out.push(Measured { scenario: scenario.to_string(), rows, ms });
    }
    if out.is_empty() {
        return Err("the perf harness reported no results".into());
    }
    Ok(out)
}

/// Run the long-run memory soak in release and read its CSV — the exact shape the 2026-11-20
/// round measured: 100k rows, 240 s of edit/undo/redo cycles with a sort every 50 and a save
/// every 200.
fn measure_drift() -> Result<Measured, String> {
    eprintln!("perf: {DRIFT_SCENARIO} — a {DRIFT_SECONDS} s soak at {DRIFT_ROWS} rows; the engine's own total must stay flat");
    let mut cmd = crate::cargo();
    cmd.args(["run", "--release", "-q", "-p", "cella-engine", "--example", "mem_drift", "--"]);
    cmd.arg(format!("--rows={DRIFT_ROWS}"));
    cmd.arg(format!("--seconds={DRIFT_SECONDS}"));
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::inherit());
    let out = cmd.output().map_err(|e| format!("cargo run --example mem_drift: {e}"))?;
    if !out.status.success() {
        return Err(format!("the mem_drift soak failed ({}); its output is above", out.status));
    }
    parse_drift(&String::from_utf8_lossy(&out.stdout))
}

/// The soak's CSV: `cycle,elapsed_s,total_bytes,undo_steps,last_recalc_cells`, a header line and
/// then one sample every 25 cycles. The measurement is the per-cycle cost in ms — a fixed-time
/// soak has no wall-clock number of its own; a slower engine simply completes fewer cycles. The
/// leak assertion is the flatness: over the second half of the run the engine's own reported
/// total must stay within `DRIFT_MAX_SHARE` of its median. The first half is warm-up (the undo
/// budget ages the initial fills out, and the total declines by design), so it is not judged.
fn parse_drift(text: &str) -> Result<Measured, String> {
    let mut reports: Vec<(u64, f64, f64)> = Vec::new();
    for line in text.lines() {
        let mut it = line.split(',');
        let (Some(cycle), Some(elapsed), Some(total)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        let (Ok(cycle), Ok(elapsed), Ok(total)) = (cycle.parse::<u64>(), elapsed.parse::<f64>(), total.parse::<f64>()) else {
            continue;
        };
        if cycle > 0 && total > 0.0 {
            reports.push((cycle, elapsed, total));
        }
    }
    let Some((last_cycle, last_elapsed, _)) = reports.last().copied() else {
        return Err("mem_drift reported no samples (it prints one every 25 cycles)".into());
    };
    let second_half: Vec<f64> = reports[reports.len() / 2..].iter().map(|r| r.2).collect();
    if second_half.len() < DRIFT_MIN_REPORTS {
        return Err(format!(
            "mem_drift printed only {} second-half sample(s) in {DRIFT_SECONDS} s — not enough to judge flatness",
            second_half.len()
        ));
    }
    let mut sorted = second_half.clone();
    sorted.sort_unstable_by(|a, b| a.total_cmp(b));
    let median = sorted[sorted.len() / 2];
    let band = second_half.iter().map(|t| (t - median).abs() / median).fold(0.0_f64, f64::max);
    if band > DRIFT_MAX_SHARE {
        return Err(format!(
            "mem_drift drifted: the engine's total moved {:.1}% off its second-half median ({:.1} MB) — a leak, not noise",
            band * 100.0,
            median / 1_000_000.0
        ));
    }
    eprintln!("perf: {DRIFT_SCENARIO} — {last_cycle} cycles in {DRIFT_SECONDS} s, second half flat within {:.2}%", band * 100.0);
    Ok(Measured { scenario: DRIFT_SCENARIO.to_string(), rows: DRIFT_ROWS, ms: last_elapsed * 1000.0 / last_cycle as f64 })
}

/// Run the save micro-benchmark in release and read its `--json` document — the same shape the
/// harness prints, so the results parse identically. `only` narrows the re-measure to the row
/// counts that came out over budget.
fn measure_save(args: &Args, only: Option<&[usize]>) -> Result<Vec<Measured>, String> {
    let mut rows = args.rows.clone();
    if let Some(only) = only {
        rows.retain(|r| only.contains(r));
    }
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let mut cmd = crate::cargo();
    cmd.args(["run", "--release", "-q", "-p", "xtask", "--example", "save_bench", "--"]);
    cmd.arg(format!("--rows={}", rows.iter().map(usize::to_string).collect::<Vec<_>>().join(",")));
    cmd.arg(format!("--saves={SAVE_SAVES}"));
    cmd.arg("--json");
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::inherit());
    let out = cmd.output().map_err(|e| format!("cargo run --example save_bench: {e}"))?;
    if !out.status.success() {
        return Err(format!("the save micro-benchmark failed ({}); its output is above", out.status));
    }
    parse_results(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(scenario: &str, rows: usize, ms: f64) -> Budget {
        Budget { scenario: scenario.into(), rows, ms }
    }

    fn m(scenario: &str, rows: usize, ms: f64) -> Measured {
        Measured { scenario: scenario.into(), rows, ms }
    }

    #[test]
    fn budgets_round_trip() {
        let text = render_budgets(&[b("sort.desc", 1_000_000, 14_250.0), b("fill.values", 100_000, 800.0)]);
        let parsed = parse_budgets(&text).expect("parse what we rendered");
        assert_eq!(parsed, vec![b("fill.values", 100_000, 800.0), b("sort.desc", 1_000_000, 14_250.0)], "sorted: rows, then id");
        assert!(text.starts_with("# Cella performance budgets"), "{text}");
    }

    #[test]
    fn parsing_rejects_bad_lines() {
        assert!(parse_budgets("fill.values 100000\n").is_err(), "two columns");
        assert!(parse_budgets("fill.values rows 800\n").is_err(), "row count");
        assert!(parse_budgets("fill.values 100000 fast\n").is_err(), "budget");
        assert!(parse_budgets("fill.values 100000 800 extra\n").is_err(), "trailing column");
        assert!(parse_budgets("fill.values 100000 0\n").is_err(), "zero budget");
        let dup = parse_budgets("a 1 2\na 1 3\n").expect_err("duplicates are a mistake");
        assert!(dup.contains("duplicate"), "{dup}");
        assert!(parse_budgets("# only comments\n\n").is_err(), "no entries");
        assert_eq!(parse_budgets("# c\n\n  a   1   2 \n").expect("one entry"), vec![b("a", 1, 2.0)]);
    }

    #[test]
    fn verdict_compares_against_the_scaled_budget() {
        let budgets = [b("fill.values", 100_000, 800.0)];
        assert_eq!(verdict(&m("fill.values", 100_000, 500.0), &budgets, 1.0), Verdict::Within(800.0));
        assert_eq!(verdict(&m("fill.values", 100_000, 800.0), &budgets, 1.0), Verdict::Within(800.0), "equal is within");
        assert_eq!(verdict(&m("fill.values", 100_000, 801.0), &budgets, 1.0), Verdict::Over(800.0));
        assert_eq!(verdict(&m("fill.values", 100_000, 1_400.0), &budgets, 2.0), Verdict::Within(1_600.0), "--scale has slack");
        assert_eq!(verdict(&m("fill.values", 1_000_000, 500.0), &budgets, 1.0), Verdict::Missing, "rows are part of the key");
    }

    #[test]
    fn update_records_headroom_and_a_floor() {
        assert_eq!(budget_for(4_480.0), 9_000.0, "4480 ms + 100%, rounded up to a 250 ms step");
        assert_eq!(budget_for(9_780.0), 19_750.0, "rounded up to a 250 ms step");
        assert_eq!(budget_for(151.0), 310.0, "302 rounded up to a 10 ms step");
        assert_eq!(budget_for(0.151), MIN_BUDGET_MS, "microsecond scenarios get the floor");
        assert!(budget_for(6.0) > 6.0);
    }

    #[test]
    fn results_are_read_back_from_the_harness_json() {
        let doc = r#"{"schema":1,"release":true,"rows":[100000],"results":[
            {"scenario":"fill.values","rows":100000,"ms":637.7,"detail":"100000 rows x 6 columns"},
            {"scenario":"sort.desc","rows":100000,"ms":1471.2,"detail":"sorted"}]}"#;
        assert_eq!(parse_results(doc).expect("parse"), vec![m("fill.values", 100_000, 637.7), m("sort.desc", 100_000, 1_471.2)]);
        assert!(parse_results("not json").is_err());
        assert!(parse_results(r#"{"rows":[]}"#).is_err(), "no results");
        assert!(parse_results(r#"{"results":[{"rows":1}]}"#).is_err(), "no scenario");
    }

    #[test]
    fn a_retry_only_moves_a_scenario_that_got_faster() {
        let mut first = [m("a", 1_000, 20.0), m("b", 1_000, 30.0)];
        keep_min(&mut first, &[m("a", 1_000, 10.0), m("b", 1_000, 40.0)]);
        assert_eq!(first, [m("a", 1_000, 10.0), m("b", 1_000, 30.0)], "the slower re-measure is ignored");
    }

    #[test]
    fn a_retry_never_lands_on_the_wrong_line() {
        // Keyed by (scenario, rows), not by position: a re-measure covers a subset of the table.
        let mut first = [m("a", 1_000, 20.0), m("b", 1_000, 30.0), m("a", 2_000, 40.0)];
        keep_min(&mut first, &[m("b", 1_000, 10.0), m("a", 2_000, 50.0)]);
        assert_eq!(first[0].ms, 20.0, "a @ 1000 was not re-measured");
        assert_eq!(first[1].ms, 10.0, "the faster re-measure of b @ 1000 is kept");
        assert_eq!(first[2].ms, 40.0, "the slower re-measure of a @ 2000 is ignored");
    }

    #[test]
    fn the_table_shows_over_budget_scenarios() {
        let budgets = [b("fill.values", 1_000, 10.0), b("sort.desc", 1_000, 100.0)];
        let measured = [m("fill.values", 1_000, 5.0), m("sort.desc", 1_000, 200.0)];
        let table = render_check(&measured, &budgets, 1.0, true);
        assert!(table.contains("50%"), "{table}");
        assert!(table.contains("200%!"), "{table}");
    }

    #[test]
    fn arguments_parse() {
        let a = parse_args(&["--update", "--rows=1000,2000,1000", "--scenario=a,b", "--scale=2.5"]).expect("parse");
        assert!(a.update);
        assert_eq!(a.rows, vec![1_000, 2_000]);
        assert_eq!(a.only, Some(vec!["a".into(), "b".into()]));
        assert_eq!(a.scale, 2.5);
        assert_eq!(parse_args(&["--rows", "7"]).expect("space form").rows, vec![7]);
        assert_eq!(parse_args(&["--repeat=3"]).expect("repeat").repeat, Some(3));
        assert!(parse_args(&["--repeat=0"]).is_err(), "a zero-pass update is a mistake");
        assert!(parse_args(&["--repeat=99"]).is_err(), "capped");
        assert!(parse_args(&["--nope"]).is_err());
        assert!(parse_args(&["--scale=0"]).is_err());
    }

    #[test]
    fn the_harness_skips_a_run_filtered_to_the_gate_own_scenarios() {
        // No filter runs everything; a mixed filter runs the harness's share; a drift-only
        // filter must not start the harness (its --scenario validation rejects mem_drift).
        assert!(harness_has_work(None), "no filter: the harness runs everything");
        assert!(harness_has_work(Some(&["fill.values".into(), DRIFT_SCENARIO.into()])), "a mixed filter keeps the harness's share");
        assert!(!harness_has_work(Some(&[DRIFT_SCENARIO.into()])), "a drift-only run starts no harness");
        assert!(!harness_has_work(Some(&[SAVE_SCENARIO.into(), DRIFT_SCENARIO.into()])), "the gate's own scenarios start no harness");
    }

    #[test]
    fn an_unfiltered_run_wants_every_scenario() {
        assert!(wants(&None, DRIFT_SCENARIO));
        assert!(wants(&None, SAVE_SCENARIO));
        assert!(!wants(&Some(vec!["fill.values".into()]), DRIFT_SCENARIO));
        assert!(wants(&Some(vec!["fill.values".into(), DRIFT_SCENARIO.into()]), DRIFT_SCENARIO));
        assert!(!wants(&Some(vec![SAVE_SCENARIO.into()]), DRIFT_SCENARIO), "a save-only run skips the soak");
    }

    #[test]
    fn the_drift_csv_parses_and_passes_a_flat_second_half() {
        // The 2026-11-20 shape: the undo budget ages the initial fills out (the first half
        // declines by design), then the total sits flat.
        let mut csv = String::from("cycle,elapsed_s,total_bytes,undo_steps,last_recalc_cells\n");
        for i in 1..=40u64 {
            let (cycle, elapsed, total) = if i <= 20 {
                (i * 25, i as f64 * 6.0, 129_000_000.0 - i as f64 * 4_000_000.0)
            } else {
                (i * 25, i as f64 * 6.0, 44_700_000.0 + (i % 2) as f64 * 900.0)
            };
            csv += &format!("{cycle},{elapsed:.1},{total},10,1\n");
        }
        let measured = parse_drift(&csv).expect("a flat second half passes");
        assert_eq!(measured.scenario, DRIFT_SCENARIO);
        assert_eq!(measured.rows, DRIFT_ROWS);
        assert!((measured.ms - 240.0).abs() < 0.05, "{:?}", measured.ms); // 1,000 cycles in 240 s
    }

    #[test]
    fn a_rising_second_half_fails_the_drift_check() {
        let mut csv = String::from("cycle,elapsed_s,total_bytes,undo_steps,last_recalc_cells\n");
        for i in 1..=40u64 {
            let total = if i <= 20 { 44_700_000.0 } else { 44_700_000.0 + i as f64 * 2_000_000.0 };
            csv += &format!("{},{}.0,{total},10,1\n", i * 25, i * 6);
        }
        let err = parse_drift(&csv).expect_err("a leak rises");
        assert!(err.contains("drifted"), "{err}");
    }

    #[test]
    fn a_drift_run_with_no_samples_or_too_few_is_an_error() {
        assert!(parse_drift("").is_err());
        let one = "cycle,elapsed_s,total_bytes,undo_steps,last_recalc_cells\n25,2.6,44700000,10,1\n";
        assert!(parse_drift(one).is_err(), "one sample is not a flatness proof");
    }
}
