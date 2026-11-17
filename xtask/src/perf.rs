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

/// Keep the faster of two measurements of the same scenario (every pass reports the same order).
fn keep_min(measured: &mut [Measured], again: &[Measured]) {
    for (m, a) in measured.iter_mut().zip(again) {
        if a.ms < m.ms {
            *m = a.clone();
        }
    }
}

fn env_scale() -> f64 {
    std::env::var("CELLA_PERF_SCALE").ok().and_then(|s| s.trim().parse().ok()).filter(|s| *s > 0.0).unwrap_or(1.0)
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
    let mut measured = measure(&args, None)?;
    if args.update {
        let mut next: Vec<Budget> = measured.iter().map(|m| Budget { scenario: m.scenario.clone(), rows: m.rows, ms: budget_for(m.ms) }).collect();
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
        print!("{}", render_check(&measured, &budgets, 1.0, false));
        println!("\nrecorded {} budgets in {}", budgets.len(), path.display());
        return Ok(());
    }
    // A scenario over budget is measured once more before it fails the run: a build that started
    // on the same machine is a slow pass, not a regression, and a real regression stays over budget
    // twice. Missing budgets are a mistake in the file, not a timing question, so they never retry.
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
    println!("\nbudgets {} (scale {:.2})", path.display(), args.scale);
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
    println!("\nOK: all {} scenarios within budget ({})", measured.len(), path.display());
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

/// Run the harness in release and read its `--json` document. `only` overrides the command
/// line's scenario filter (the re-measure of the scenarios that came out over budget).
fn measure(args: &Args, only: Option<&[String]>) -> Result<Vec<Measured>, String> {
    let retry = only.is_some();
    let mut cmd = crate::cargo();
    cmd.args(["run", "--release", "-q", "-p", "cella-engine", "--example", "perf", "--", "--json"]);
    cmd.arg(format!("--rows={}", args.rows.iter().map(usize::to_string).collect::<Vec<_>>().join(",")));
    if let Some(ids) = only.or(args.only.as_deref()) {
        cmd.arg(format!("--scenario={}", ids.join(",")));
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
}
