//! The measuring side's two ledgers.
//!
//! `cargo xtask bench-commands` runs `crates/engine/examples/bench_commands.rs` in release and
//! writes `docs/performance-commands.md`: **one row per registered command**, at 100,000 and
//! 1,000,000 rows, with its parameters pointed both near the cursor and over the whole used area.
//! `cargo xtask perf` prices fifteen scenarios; this prices the command surface — the point of
//! "every operation, every feature, should be observable" is that no command is a blank.
//!
//! `cargo xtask bench-coverage [--check] [--write]` reads `xtask/bench-coverage.txt`, the ledger
//! that says where each command's cost is observed: `bench` (a number in the catalogue or the
//! gate), `ui` (frame time, the UI bench's business) or `excuse: why` (nothing to price, and why).
//! `--check` fails on a registered command with no line, on a line naming no registered command,
//! on a duplicate, on a `todo` left in place, and on a `bench` line the catalogue has no number
//! for; it runs as a step of `cargo xtask ci`, so a new command cannot ship unpriced by accident.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Where a command's cost is observed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// A number in `docs/performance-commands.md` (or, for a command the gate's scenarios cover
    /// better, in `xtask/perf-budgets.txt`).
    Bench,
    /// Frame time: scrolling, repainting, a dialog. Not throughput, and not this bench's.
    Ui,
    /// Nothing to price. The reason is required and is shown in `--check`'s summary.
    Excuse,
    /// A line `--write` added and nobody has filled in yet. `--check` fails on it: the point of
    /// the ledger is that a command is *classified*, not merely present.
    Todo,
}

impl Class {
    fn name(self) -> &'static str {
        match self {
            Class::Bench => "bench",
            Class::Ui => "ui",
            Class::Excuse => "excuse",
            Class::Todo => "todo",
        }
    }
}

/// One line of `xtask/bench-coverage.txt`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub class: Class,
    /// For [`Class::Excuse`]: why there is nothing to price. Empty otherwise.
    pub reason: String,
}

/// Reads the ledger: `<id> <class> [reason]`, `#` comments and blank lines skipped.
pub fn parse(text: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(2, char::is_whitespace);
        let Some(id) = parts.next() else { continue };
        let rest = parts.next().unwrap_or("").trim();
        let (word, reason) = match rest.split_once(':') {
            Some((word, reason)) => (word.trim(), reason.trim()),
            None => (rest, ""),
        };
        let class = match word {
            "bench" => Class::Bench,
            "ui" => Class::Ui,
            "todo" | "" => Class::Todo,
            "excuse" => Class::Excuse,
            _ => Class::Todo,
        };
        // A reason after a non-excuse class is kept, so a comment the writer wrote by hand
        // survives `--write`; `--check` only insists on it where it means something.
        out.push(Entry { id: id.to_string(), class, reason: reason.to_string() });
    }
    out
}

/// The ledger's text: the given leading comment block, then one line per entry, sorted by id.
pub fn render(header: &str, entries: &[Entry]) -> String {
    let mut sorted: Vec<&Entry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let mut s = String::new();
    if !header.trim().is_empty() {
        s.push_str(header.trim_end());
        s.push('\n');
    }
    let mut last_blank = true;
    for e in sorted {
        match e.class {
            Class::Excuse if !e.reason.is_empty() => {
                s.push_str(&format!("{:<44} excuse: {}\n", e.id, e.reason));
                last_blank = false;
            }
            Class::Excuse => {
                s.push_str(&format!("{:<44} excuse:\n", e.id));
                last_blank = false;
            }
            other => {
                s.push_str(&format!("{:<44} {}\n", e.id, other.name()));
                last_blank = false;
            }
        }
    }
    if !last_blank {
        // One trailing newline; the writer adds the platform's line endings.
        while s.ends_with('\n') {
            s.pop();
        }
        s.push('\n');
    }
    s
}

/// Everything wrong with the ledger, as messages that say what to do about it.
///
/// `priced` is the set of ids `docs/performance-commands.md` carries a number for (`None` when the
/// catalogue page has not been generated yet — then the `bench` cross-check is skipped, the way
/// `parity` skips until its page exists).
pub fn check(entries: &[Entry], registered: &[&str], priced: Option<&BTreeSet<String>>) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for e in entries {
        if !seen.insert(e.id.as_str()) {
            problems.push(format!("`{}` has more than one line — keep one", e.id));
        }
        if !registered.contains(&e.id.as_str()) {
            problems.push(format!("`{}` is not a registered command (renamed or removed?) — drop its line", e.id));
        }
        match e.class {
            Class::Todo => problems.push(format!(
                "`{}` is still `todo` — say where its cost is observed: `bench` (the command \
                 catalogue), `ui` (frame time) or `excuse: why`",
                e.id
            )),
            Class::Excuse if e.reason.is_empty() => problems.push(format!("`{}` is excused for no stated reason", e.id)),
            Class::Bench => {
                if let Some(priced) = priced
                    && !priced.contains(&e.id)
                {
                    problems.push(format!(
                        "`{}` says `bench` but docs/performance-commands.md has no usable number for \
                         it — price it there (or in `crates/engine/examples/perf.rs`, then regenerate \
                         the page), or say where its cost is observed",
                        e.id
                    ));
                }
            }
            _ => {}
        }
    }
    for id in registered {
        if !seen.contains(id) {
            problems.push(format!("`{id}` has no line"));
        }
    }
    problems
}

/// The summary line: how many commands each class holds.
pub fn summary(entries: &[Entry]) -> String {
    let count = |class: Class| entries.iter().filter(|e| e.class == class).count();
    format!(
        "{} commands: {} bench, {} ui, {} excuse, {} todo",
        entries.len(),
        count(Class::Bench),
        count(Class::Ui),
        count(Class::Excuse),
        count(Class::Todo)
    )
}

// ---------------------------------------------------------------- the catalogue page

/// One measurement of one command, as the example reports it.
#[derive(Clone, Debug)]
pub struct Measurement {
    pub id: String,
    pub rows: usize,
    pub scale: String,
    pub ms: f64,
    pub changed: bool,
    /// Whether the command is registered as an edit: a query that returns Ok without changing the
    /// workbook is doing its job, an edit that does so is not.
    pub writes: bool,
    pub outcome: String,
    pub error: Option<String>,
}

/// What the example's `--json` document says, parsed.
#[derive(Clone, Debug, Default)]
pub struct Catalogue {
    pub rows: Vec<usize>,
    pub scales: Vec<String>,
    pub commands: usize,
    pub repeat: usize,
    pub measurements: Vec<Measurement>,
    pub not_run: Vec<(String, String)>,
}

pub fn parse_catalogue(text: &str) -> Result<Catalogue, String> {
    let doc: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("bench_commands wrote bad JSON: {e}"))?;
    let num = |v: &serde_json::Value, key: &str| v.get(key).and_then(serde_json::Value::as_u64).unwrap_or(0);
    let list = |v: &serde_json::Value, key: &str| -> Vec<serde_json::Value> {
        v.get(key).and_then(serde_json::Value::as_array).cloned().unwrap_or_default()
    };
    let mut out = Catalogue { commands: num(&doc, "commands") as usize, repeat: num(&doc, "repeat") as usize, ..Catalogue::default() };
    out.rows = list(&doc, "rows").iter().filter_map(serde_json::Value::as_u64).map(|n| n as usize).collect();
    out.scales = list(&doc, "scales").iter().filter_map(serde_json::Value::as_str).map(str::to_string).collect();
    for m in list(&doc, "measurements") {
        let id = m.get("id").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
        if id.is_empty() {
            continue;
        }
        out.measurements.push(Measurement {
            id,
            rows: num(&m, "rows") as usize,
            scale: m.get("scale").and_then(serde_json::Value::as_str).unwrap_or_default().to_string(),
            ms: m.get("ms").and_then(serde_json::Value::as_f64).unwrap_or(0.0),
            changed: m.get("changed").and_then(serde_json::Value::as_bool).unwrap_or(false),
            writes: m.get("writes").and_then(serde_json::Value::as_bool).unwrap_or(true),
            outcome: m.get("outcome").and_then(serde_json::Value::as_str).unwrap_or("ok").to_string(),
            error: m.get("error").and_then(serde_json::Value::as_str).map(str::to_string),
        });
    }
    for s in list(&doc, "notRun") {
        let id = s.get("id").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
        let why = s.get("why").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
        if !id.is_empty() {
            out.not_run.push((id, why));
        }
    }
    Ok(out)
}

fn fmt_ms(ms: f64) -> String {
    if ms >= 1_000.0 {
        format!("{:.2} s", ms / 1_000.0)
    } else if ms >= 10.0 {
        format!("{ms:.0} ms")
    } else if ms >= 1.0 {
        format!("{ms:.1} ms")
    } else {
        format!("{:.0} us", (ms * 1_000.0).max(0.0))
    }
}

fn note(m: &Measurement) -> String {
    let said = match &m.error {
        Some(e) => one_line(e, 60),
        None => String::new(),
    };
    match m.outcome.as_str() {
        "ok" if m.changed => String::new(),
        // A query that returns Ok without writing did its job (a menu, a listing, a statistic).
        "ok" if !m.writes => String::new(),
        "ok" => "no change".to_string(),
        "refused" => format!("refused: {said}"),
        "params" => format!("params: {said}"),
        "no" => format!("no: {said}"),
        "panic" => format!("PANIC: {said}"),
        other => format!("{other}: {said}"),
    }
}

/// Whether a row is a cost that was observed at all: the command ran, or it said the answer is no.
/// A row that only turned the generated params down (`params`, `refused`) or panicked says nothing
/// about the command's cost on real work.
fn usable(m: &Measurement) -> bool {
    matches!(m.outcome.as_str(), "ok" | "no")
}

fn one_line(text: &str, max: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let cut: String = flat.chars().take(max).collect();
    format!("{cut}…")
}

/// Facts the page header names, so a number is never from nowhere.
pub struct Facts<'a> {
    pub version: &'a str,
    pub sha: &'a str,
    pub date: &'a str,
    /// `"windows x86_64"`, from the build that measured.
    pub machine: String,
}

/// `docs/performance-commands.md`: the header, the slowest commands, then every command's row.
pub fn render_catalogue(f: &Facts, c: &Catalogue) -> String {
    let mut s = String::new();
    s += "# Command timing catalogue\n\n";
    s += "<!-- Generated by `cargo xtask bench-commands` from crates/engine/examples/bench_commands.rs. Don't edit by hand. -->\n\n";
    s += &format!("> **Generated** for Cella {} at `{}` ({}). Machine: {}. ", f.version, f.sha, f.date, f.machine);
    if c.repeat > 1 {
        s += &format!("Each number is the fastest of {} passes. ", c.repeat);
    }
    s += "Method, the two scales and what a row does not measure: [performance.md](performance.md).\n\n";
    s += "**One row per registered command.** The scenario gate (`cargo xtask perf`) prices the \
          operations a session spends its seconds on; this prices the command surface, so that no \
          command is a blank. A cell is the command's own wall clock in ms on a workbook of that \
          many rows × 6 value columns, run alone in a fresh session (a table, a chart, a defined \
          name, a clipboard block and one undo step are in place; see the example's `prepare`).\n\n";
    s += "| scale | the parameters the command is given |\n|---|---|\n";
    s += "| `small` | near the cursor, the fuzzer's params: \"what does it cost when the user points it at a few cells\" |\n";
    s += "| `whole` | its region-naming params (`range`, `target`, `criteria`, …) cover the whole used area: \"select the column and format it\" |\n\n";
    s += "A command that names no region has the same cost at both scales — that is a fact about the \
          command, not a missing measurement. A note says how the row went: `params` or `refused` \
          means the command turned the generated params down (no cost was observed), `no` means it \
          ran and the answer was no (nothing found, no such table), `no change` means an edit that \
          changed nothing, and `PANIC` would mean a caught panic. `-` means that configuration was \
          not measured at all.\n\n";

    // The slowest, which is the part a person reads first.
    let widest = c.rows.iter().copied().max().unwrap_or(0);
    let mut slowest: Vec<&Measurement> = c.measurements.iter().filter(|m| m.rows == widest && usable(m)).collect();
    slowest.sort_by(|a, b| b.ms.total_cmp(&a.ms));
    slowest.truncate(20);
    if widest > 0 && !slowest.is_empty() {
        s += &format!("## The twenty slowest commands at {} rows\n\n", thousands(widest));
        s += "| command | scale | time | note |\n|---|---|---|---|\n";
        for m in &slowest {
            s += &format!("| `{}` | {} | {} | {} |\n", m.id, m.scale, fmt_ms(m.ms), note(m));
        }
        s += "\n";
    }

    // The whole catalogue, in registration order (the order the example measured).
    let mut ids: Vec<&str> = Vec::new();
    for m in &c.measurements {
        if !ids.contains(&m.id.as_str()) {
            ids.push(&m.id);
        }
    }
    s += "## Every command\n\n";
    let mut head = String::from("| command |");
    let mut sep = String::from("|---|");
    for rows in &c.rows {
        for scale in &c.scales {
            head += &format!(" {} {} |", thousands(*rows), scale);
            sep += "---|";
        }
    }
    head += " note |\n";
    sep += "---|\n";
    s += &head;
    s += &sep;
    for id in &ids {
        let mut line = format!("| `{id}` |");
        // A note body and the scales it came from: the same note at both scales is one note, and
        // one that differs per scale says which.
        let mut notes: Vec<(String, Vec<&str>)> = Vec::new();
        for rows in &c.rows {
            for scale in &c.scales {
                match c.measurements.iter().find(|m| m.id == *id && m.rows == *rows && m.scale == *scale) {
                    Some(m) => {
                        line += &format!(" {} |", fmt_ms(m.ms));
                        let body = note(m);
                        if body.is_empty() {
                            continue;
                        }
                        match notes.iter_mut().find(|(b, _)| *b == body) {
                            Some((_, scales)) => {
                                if !scales.contains(&scale.as_str()) {
                                    scales.push(scale);
                                }
                            }
                            None => notes.push((body, vec![scale])),
                        }
                    }
                    None => line += " - |",
                }
            }
        }
        let said: Vec<String> =
            notes.iter().map(|(body, scales)| if c.scales.len() > 1 { format!("{}: {body}", scales.join("/")) } else { body.clone() }).collect();
        line += &format!(" {} |\n", said.join("; "));
        s += &line;
    }
    s += &format!("\n{} of them measured", c.commands);
    let unpriced = unpriced_commands(c);
    if !unpriced.is_empty() {
        s += &format!(", {} with no usable number yet (they are listed below)", unpriced.len());
    }
    s += ".\n\n";

    if !unpriced.is_empty() {
        s += "## No number yet, and what the command said\n\n";
        s += "Every row of these commands was refused or errored, so the table above carries a \
              failure time, not a cost. Most want an object this workbook does not carry (a \
              PivotTable, a script, a control); the rest want params the generator cannot shape. \
              Turning them into numbers is the catalogue's next job, and `xtask/bench-coverage.txt` \
              records each one as excused until it is done.\n\n";
        s += "| command | what it said |\n|---|---|\n";
        for (id, why) in &unpriced {
            s += &format!("| `{id}` | {why} |\n");
        }
        s += "\n";
    }

    if !c.not_run.is_empty() {
        s += "## Not measured, and why\n\n";
        s += "A command that opens or closes a document, reads or writes a file or switches windows \
              cannot run in a throwaway session. Each is an `excuse:` line in \
              [../xtask/bench-coverage.txt](../xtask/bench-coverage.txt).\n\n";
        s += "| command | why |\n|---|---|\n";
        for (id, why) in &c.not_run {
            s += &format!("| `{id}` | {why} |\n");
        }
        s += "\n";
    }
    s
}

/// The commands whose every row was refused or errored — no cost was observed for them — with the
/// first thing the command said about it.
fn unpriced_commands(c: &Catalogue) -> Vec<(String, String)> {
    let mut ids: Vec<String> = Vec::new();
    for m in &c.measurements {
        if !ids.contains(&m.id) {
            ids.push(m.id.clone());
        }
    }
    let mut out = Vec::new();
    for id in ids {
        let rows: Vec<&Measurement> = c.measurements.iter().filter(|m| m.id == id).collect();
        if rows.iter().any(|m| usable(m)) {
            continue;
        }
        let why = rows.iter().find_map(|m| m.error.as_ref()).map(|e| one_line(e, 110)).unwrap_or_else(|| "no row".to_string());
        out.push((id, why));
    }
    out
}

// ---------------------------------------------------------------- the function catalogue

/// One measurement of one worksheet function, as `crates/engine/examples/bench_functions.rs`
/// reports it.
#[derive(Clone, Debug)]
pub struct FnMeasurement {
    pub name: String,
    pub template: String,
    pub rows: usize,
    /// `small` (ten-row ranges), `whole` (the whole column) or `scalar` (no range argument, so one
    /// call covers both scales).
    pub scale: String,
    pub ms: f64,
    /// `ok`, `error` (the formula answered with a `#` error) or `refused` (it would not go in).
    pub outcome: String,
    /// Whether that error is the function's documented answer (a CUBE function with no
    /// connection, `NA`, a stub).
    pub expected: bool,
    pub detail: Option<String>,
}

/// What the example's `--json` document says, parsed.
#[derive(Clone, Debug, Default)]
pub struct FunctionCatalogue {
    pub rows: Vec<usize>,
    pub repeat: usize,
    pub measurements: Vec<FnMeasurement>,
    /// The cost of entering `=1+1`, per row count: the floor a scalar row sits on.
    pub floors: Vec<(usize, f64)>,
}

pub fn parse_functions(text: &str) -> Result<FunctionCatalogue, String> {
    let doc: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("bench_functions wrote bad JSON: {e}"))?;
    let mut out = FunctionCatalogue {
        repeat: doc.get("repeat").and_then(serde_json::Value::as_u64).unwrap_or(1) as usize,
        rows: doc
            .get("rows")
            .and_then(serde_json::Value::as_array)
            .map(|a| a.iter().filter_map(serde_json::Value::as_u64).map(|n| n as usize).collect())
            .unwrap_or_default(),
        ..FunctionCatalogue::default()
    };
    for m in doc.get("measurements").and_then(serde_json::Value::as_array).cloned().unwrap_or_default() {
        let name = m.get("name").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
        if name.is_empty() {
            continue;
        }
        out.measurements.push(FnMeasurement {
            name,
            template: m.get("template").and_then(serde_json::Value::as_str).unwrap_or_default().to_string(),
            rows: m.get("rows").and_then(serde_json::Value::as_u64).unwrap_or(0) as usize,
            scale: m.get("scale").and_then(serde_json::Value::as_str).unwrap_or_default().to_string(),
            ms: m.get("ms").and_then(serde_json::Value::as_f64).unwrap_or(0.0),
            outcome: m.get("outcome").and_then(serde_json::Value::as_str).unwrap_or("ok").to_string(),
            expected: m.get("expected").and_then(serde_json::Value::as_bool).unwrap_or(false),
            detail: m.get("detail").and_then(serde_json::Value::as_str).map(str::to_string),
        });
    }
    for f in doc.get("floors").and_then(serde_json::Value::as_array).cloned().unwrap_or_default() {
        let rows = f.get("rows").and_then(serde_json::Value::as_u64).unwrap_or(0) as usize;
        let ms = f.get("ms").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
        if rows > 0 {
            out.floors.push((rows, ms));
        }
    }
    Ok(out)
}

/// `xtask/bench-functions.txt`: one `name<TAB>template[<TAB>error]` line per function.
pub fn parse_calls_table(text: &str) -> Result<Vec<(String, String, bool)>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let mut parts = line.split('\t');
        let name = parts.next().unwrap_or("").trim().to_string();
        let template = parts.next().map(str::trim).unwrap_or_default().to_string();
        if name.is_empty() || template.is_empty() {
            return Err(format!("xtask/bench-functions.txt:{}: not `name<TAB>template`", n + 1));
        }
        out.push((name, template, parts.next().map(str::trim) == Some("error")));
    }
    Ok(out)
}

/// What `bench-functions --check` refuses: a registered function with no line, a line naming no
/// registered function, a duplicate, and a call that does not start with the function's own name
/// (a copy-paste that would measure the wrong function under the right heading).
pub fn check_calls(calls: &[(String, String, bool)], registered: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();
    for (name, template, _) in calls {
        if !seen.insert(name.clone()) {
            problems.push(format!("`{name}` has two lines"));
        }
        if !registered.iter().any(|r| r == name) {
            problems.push(format!("`{name}` is not a registered function (renamed or removed?)"));
        }
        let head = template.split('(').next().unwrap_or("").trim();
        if !head.eq_ignore_ascii_case(name) {
            problems.push(format!("`{name}`'s call starts with `{head}(`, not `{name}(`"));
        }
    }
    for r in registered {
        if !seen.contains(r) {
            problems.push(format!("`{r}` is registered and has no line in the call table"));
        }
    }
    problems
}

/// The page: what every worksheet function costs to enter, at two argument scales and two sizes.
pub fn render_functions(f: &Facts, c: &FunctionCatalogue) -> String {
    let mut s = String::new();
    s += "# Function timing catalogue\n\n";
    s += "<!-- Generated by `cargo xtask bench-functions` from crates/engine/examples/bench_functions.rs. Don't edit by hand. -->\n\n";
    s += &format!("> **Generated** for Cella {} at `{}` ({}). Machine: {}. ", f.version, f.sha, f.date, f.machine);
    if c.repeat > 1 {
        s += &format!("Each number is the fastest of {} passes. ", c.repeat);
    }
    s += "Method and what a row does not measure: [performance.md](performance.md).\n\n";
    s += "**One row per registered worksheet function.** The command catalogue prices the command \
          surface and the scenario gate prices the operations a session spends its seconds on; \
          this prices the formulas inside the cells — a user's spreadsheet is mostly functions, and \
          before this page not one of them had a number.\n\n";
    s += "A cell is the wall clock of `cell.set` with that formula: parse it, evaluate it, write the \
          cell. `small` gives every range argument ten rows; `whole` gives it the whole column \
          (the workbook's data is a header and `rows − 1` rows of eight columns). `scalar` means \
          the call has no range argument, so the same call is both scales. A template's `$R`, \
          `$T` … stands for one of those ranges; the exact calls are in \
          [../xtask/bench-functions.txt](../xtask/bench-functions.txt).\n\n";
    s += "A note reads `error by design` for a function whose documented answer is an error here \
          (a CUBE function with no connection, `NA`), `error` for a call the engine answered with a \
          `#` error (the call is the thing to fix, not the function), and `refused` when the \
          formula would not go in at all.\n\n";
    if !c.floors.is_empty() {
        let said: Vec<String> = c.floors.iter().map(|(rows, ms)| format!("{} ms at {} rows", fmt_ms(*ms), thousands(*rows))).collect();
        s += &format!(
            "**The floor.** Entering a formula that does nothing (`=1+1`) costs {} — that is the \
             session taking its undo snapshot and the recalculation bookkeeping, not the function. \
             Every scalar row is mostly this floor: read a scalar number as \"the floor, plus \
             whatever the function itself added\".\n\n",
            said.join(" and ")
        );
    }

    let names: Vec<&str> = {
        let mut v: Vec<&str> = Vec::new();
        for m in &c.measurements {
            if !v.contains(&m.name.as_str()) {
                v.push(m.name.as_str());
            }
        }
        v
    };
    let widest = c.rows.iter().copied().max().unwrap_or(0);
    let cell = |name: &str, rows: usize, scale: &str| -> Option<&FnMeasurement> {
        c.measurements.iter().find(|m| m.name == name && m.rows == rows && (m.scale == scale || m.scale == "scalar"))
    };
    let numeral = |m: Option<&FnMeasurement>| -> String {
        match m {
            None => "-".to_string(),
            Some(m) if m.outcome == "ok" || m.expected => fmt_ms(m.ms),
            Some(_) => "-".to_string(),
        }
    };
    let fn_note = |name: &str| -> String {
        let rows: Vec<&FnMeasurement> = c.measurements.iter().filter(|m| m.name == name).collect();
        let mut said = Vec::new();
        if let Some(m) = rows.iter().find(|m| m.outcome == "error" && m.expected) {
            said.push(format!("error by design: {}", m.detail.clone().unwrap_or_default()));
        } else if let Some(m) = rows.iter().find(|m| m.outcome == "error") {
            said.push(format!("error: {}", m.detail.clone().unwrap_or_default()));
        } else if let Some(m) = rows.iter().find(|m| m.outcome == "refused") {
            said.push(format!("refused: {}", one_line(m.detail.as_deref().unwrap_or(""), 60)));
        }
        if rows.iter().all(|m| m.scale == "scalar") {
            said.push("scalar args (one call)".to_string());
        }
        said.join("; ")
    };

    // The slowest, at the widest row count: the part a person reads first.
    let mut slowest: Vec<&FnMeasurement> = c.measurements.iter().filter(|m| m.rows == widest && (m.outcome == "ok" || m.expected)).collect();
    slowest.sort_by(|a, b| b.ms.total_cmp(&a.ms));
    slowest.truncate(25);
    if widest > 0 && !slowest.is_empty() {
        s += &format!("## The twenty-five slowest at {} rows\n\n", thousands(widest));
        s += "| function | call | time |\n|---|---|---|\n";
        for m in &slowest {
            s += &format!("| `{}` | `{}` | {} |\n", m.name, one_line(&m.template, 80), fmt_ms(m.ms));
        }
        s += "\n";
    }

    s += "## Every function\n\n";
    let mut head = String::from("| function | call |");
    let mut sep = String::from("|---|---|");
    for rows in &c.rows {
        head += &format!(" {} small | {} whole |", thousands(*rows), thousands(*rows));
        sep += "---|---|";
    }
    head += " note |\n";
    sep += "---|\n";
    s += &head;
    s += &sep;
    for name in &names {
        let template = c.measurements.iter().find(|m| m.name == *name).map(|m| m.template.clone()).unwrap_or_default();
        let mut line = format!("| `{name}` | `{}` |", one_line(&template, 60));
        for rows in &c.rows {
            line += &format!(" {} | {} |", numeral(cell(name, *rows, "small")), numeral(cell(name, *rows, "whole")));
        }
        line += &format!(" {} |\n", fn_note(name));
        s += &line;
    }
    let measured = names.iter().filter(|n| cell(n, widest, "whole").is_some_and(|m| m.outcome == "ok")).count();
    let by_design = names.iter().filter(|n| c.measurements.iter().any(|m| m.name == **n && m.expected)).count();
    let broken = names.iter().filter(|n| c.measurements.iter().any(|m| m.name == **n && m.outcome != "ok" && !m.expected)).count();
    s += &format!(
        "\n{} functions, {} with a number at {} rows; {} answer with an error by design, {} do not answer with a number yet.\n",
        names.len(),
        measured,
        thousands(widest),
        by_design,
        broken
    );
    if broken > 0 {
        s += "\n## The calls that do not produce a number\n\n\
              Each of these is a line in [../xtask/bench-functions.txt](../xtask/bench-functions.txt) whose \
              call the engine answered with an error. The number in the table is real work done before \
              the answer; fixing the call is a line in that file.\n\n";
        s += "| function | what it said |\n|---|---|\n";
        for name in &names {
            if let Some(m) = c.measurements.iter().find(|m| m.name == **name && m.outcome != "ok" && !m.expected) {
                s += &format!("| `{name}` | {} |\n", one_line(m.detail.as_deref().unwrap_or(""), 90));
            }
        }
    }
    s
}

/// `1000000` → `1,000,000` (a page reads better with the separators).
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (at, ch) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

// ---------------------------------------------------------------- running the harness

fn capture(mut cmd: Command, dir: &Path) -> Result<String, String> {
    cmd.current_dir(dir);
    // stderr stays ours: the harness prints one line per command before it runs, which is how a
    // command that never returns names itself in the log instead of hanging the gate silently.
    cmd.stderr(Stdio::inherit());
    let out = cmd.output().map_err(|e| format!("run {cmd:?}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{cmd:?} failed ({}); see its output above", out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut c = Command::new("git");
    c.arg("-C").arg(root).args(args);
    let out = c.output().map_err(|e| format!("git {args:?}: {e}"))?;
    if !out.status.success() {
        return Err(format!("git {args:?}: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `cargo xtask bench-commands [--rows=N,N] [--repeat=N] [--only=ID,...] [--out PATH] [--json-out PATH] [--from PATH]`
pub fn run_commands(root: &Path, args: &[&str]) -> Result<(), String> {
    let mut out: PathBuf = root.join("docs/performance-commands.md");
    let mut json_out: Option<PathBuf> = None;
    let mut from: Option<PathBuf> = None;
    let mut extra: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "--out" => out = PathBuf::from(it.next().ok_or("--out needs a path")?),
            "--json-out" => json_out = Some(PathBuf::from(it.next().ok_or("--json-out needs a path")?)),
            "--from" => from = Some(PathBuf::from(it.next().ok_or("--from needs a path")?)),
            "--rows" | "--repeat" | "--only" => {
                let value = it.next().ok_or(format!("{a} needs a value"))?;
                extra.push(format!("{a}={value}"));
            }
            other if other.starts_with("--rows=") || other.starts_with("--repeat=") || other.starts_with("--only=") => extra.push(other.to_string()),
            other => {
                return Err(format!(
                    "bench-commands: unknown argument `{other}` (usage: cargo xtask bench-commands \
                     [--rows=N,N] [--repeat=N] [--only=ID,...] [--out PATH] [--json-out PATH] [--from PATH])"
                ));
            }
        }
    }
    // `--from`: re-render a measurement's JSON without spending minutes measuring it again.
    let text = match &from {
        Some(path) => std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?,
        None => {
            eprintln!("bench-commands: measuring the release build — this takes minutes and wants a quiet machine");
            let mut cmd = crate::cargo();
            cmd.args(["run", "--release", "-q", "-p", "cella-engine", "--example", "bench_commands", "--", "--json"]);
            cmd.args(&extra);
            let text = capture(cmd, root)?;
            if let Some(path) = &json_out {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
                }
                std::fs::write(path, &text).map_err(|e| format!("write {}: {e}", path.display()))?;
            }
            text
        }
    };
    let catalogue = parse_catalogue(&text)?;
    if catalogue.measurements.is_empty() {
        return Err("bench-commands: the harness measured nothing".to_string());
    }
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).map_err(|e| format!("read Cargo.toml: {e}"))?;
    let version = crate::version::read(&manifest)?;
    let sha = git(root, &["rev-parse", "--short", "HEAD"])?;
    let date = git(root, &["log", "-1", "--format=%cs"])?;
    let facts = Facts { version: &version, sha: &sha, date: &date, machine: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH) };
    let page = render_catalogue(&facts, &catalogue);
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    }
    let text = crate::eol::for_write(&out, &page);
    std::fs::write(&out, text).map_err(|e| format!("write {}: {e}", out.display()))?;
    println!(
        "bench-commands: {} commands × {} row counts × {} scales -> {} ({} lines)",
        catalogue.commands,
        catalogue.rows.len(),
        catalogue.scales.len(),
        shown(root, &out),
        page.lines().count()
    );
    Ok(())
}

/// `cargo xtask bench-functions [--rows=N,N] [--repeat=N] [--only=NAME,...] [--out PATH]
/// [--json-out PATH] [--from PATH]`, or `--check` for the call table's completeness.
pub fn run_functions(root: &Path, args: &[&str]) -> Result<(), String> {
    let mut out: PathBuf = root.join("docs/performance-functions.md");
    let mut json_out: Option<PathBuf> = None;
    let mut from: Option<PathBuf> = None;
    let mut extra: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "--check" => return functions_check(root),
            "--out" => out = PathBuf::from(it.next().ok_or("--out needs a path")?),
            "--json-out" => json_out = Some(PathBuf::from(it.next().ok_or("--json-out needs a path")?)),
            "--from" => from = Some(PathBuf::from(it.next().ok_or("--from needs a path")?)),
            "--rows" | "--repeat" | "--only" => {
                let value = it.next().ok_or(format!("{a} needs a value"))?;
                extra.push(format!("{a}={value}"));
            }
            other if other.starts_with("--rows=") || other.starts_with("--repeat=") || other.starts_with("--only=") => extra.push(other.to_string()),
            other => {
                return Err(format!(
                    "bench-functions: unknown argument `{other}` (usage: cargo xtask bench-functions \
                     [--rows=N,N] [--repeat=N] [--only=NAME,...] [--out PATH] [--json-out PATH] \
                     [--from PATH] | --check)"
                ));
            }
        }
    }
    // `--from`: re-render a measurement's JSON without spending half a minute measuring again.
    let text = match &from {
        Some(path) => std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?,
        None => {
            eprintln!("bench-functions: measuring the release build — seconds on a quiet machine");
            let target = root.join("target/bench-functions.json");
            if let Some(dir) = target.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
            }
            let target_arg = target.display().to_string();
            let mut cmd = crate::cargo();
            cmd.args(["run", "--release", "-q", "-p", "cella-engine", "--example", "bench_functions", "--", "--json-out", &target_arg]);
            cmd.args(&extra);
            let _table = capture(cmd, root)?;
            let text = std::fs::read_to_string(&target).map_err(|e| format!("read {}: {e}", target.display()))?;
            if let Some(path) = &json_out {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
                }
                std::fs::write(path, &text).map_err(|e| format!("write {}: {e}", path.display()))?;
            }
            text
        }
    };
    let catalogue = parse_functions(&text)?;
    if catalogue.measurements.is_empty() {
        return Err("bench-functions: the harness measured nothing".to_string());
    }
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).map_err(|e| format!("read Cargo.toml: {e}"))?;
    let version = crate::version::read(&manifest)?;
    let sha = git(root, &["rev-parse", "--short", "HEAD"])?;
    let date = git(root, &["log", "-1", "--format=%cs"])?;
    let facts = Facts { version: &version, sha: &sha, date: &date, machine: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH) };
    let page = render_functions(&facts, &catalogue);
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    }
    let text = crate::eol::for_write(&out, &page);
    std::fs::write(&out, text).map_err(|e| format!("write {}: {e}", out.display()))?;
    println!(
        "bench-functions: {} functions × {} row counts -> {} ({} lines)",
        catalogue.measurements.iter().map(|m| m.name.as_str()).collect::<BTreeSet<&str>>().len(),
        catalogue.rows.len(),
        shown(root, &out),
        page.lines().count()
    );
    Ok(())
}

/// The call table must name exactly the registered functions, once each, with a call that starts
/// with its own name. A new function therefore cannot ship unmeasured by accident.
pub fn functions_check(root: &Path) -> Result<(), String> {
    let path = root.join("xtask/bench-functions.txt");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let calls = parse_calls_table(&text)?;
    // The functions a formula can call are the library's specs plus the forms the evaluator
    // implements itself (IF, OFFSET, SUBTOTAL, TEXT…), which the library does not carry.
    let mut registered: Vec<String> = cella_functions::all().iter().map(|s| s.name.to_string()).collect();
    for name in cella_engine::calc::SPECIAL_FUNCTIONS {
        if !registered.iter().any(|r| r == name) {
            registered.push((*name).to_string());
        }
    }
    let problems = check_calls(&calls, &registered);
    if problems.is_empty() {
        println!("bench-functions: {} calls cover all {} registered functions", calls.len(), registered.len());
        return Ok(());
    }
    for p in &problems {
        eprintln!("  {p}");
    }
    Err(format!("{} problem(s) in xtask/bench-functions.txt", problems.len()))
}

fn shown(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).display().to_string()
}

/// The ledger's path, in the repository.
fn ledger(root: &Path) -> PathBuf {
    root.join("xtask/bench-coverage.txt")
}

/// The default header of a ledger `--write` creates.
const HEADER: &str = "\
# Cella bench coverage — where each command's cost is observed.
#
# `cargo xtask bench-coverage --check` (a step of `cargo xtask ci`) fails when a registered
# command has no line here, when a line names no registered command, when a command has two
# lines, when a line still says `todo`, or when a line says `bench` but
# docs/performance-commands.md has no number for it.
#
# One line per registered command, three choices:
#   bench           a number in docs/performance-commands.md (or, where a scenario prices it
#                   better, in xtask/perf-budgets.txt)
#   ui              frame time, not throughput — the UI bench is the ruler for it (not built yet)
#   excuse: <why>   nothing to price, and why
#
# Check with `cargo xtask bench-coverage --check`; regenerate the missing lines as `todo` with
# `cargo xtask bench-coverage --write` and fill them in.
";

/// `cargo xtask bench-ui [--rows=N] [--frames=N] [--out PATH]`: the frame-time ruler, independent
/// of the throughput gate. Runs `crates/ui-egui/examples/frame_time.rs` in release (it renders the
/// whole window offscreen with no GPU) and writes `docs/performance-ui.md`; a phase whose median is
/// over its budget fails the run, so the page and the check are one artefact.
pub fn run_ui(root: &Path, args: &[&str]) -> Result<(), String> {
    let mut out: PathBuf = root.join("docs/performance-ui.md");
    let mut rows = 1_000_000usize;
    let mut frames = 30usize;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let a: &str = a;
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) => (f, Some(v.to_string())),
            None => (a, None),
        };
        match flag {
            "--out" => out = PathBuf::from(it.next().ok_or("--out needs a path")?),
            "--rows" => {
                let v = match inline {
                    Some(v) => v,
                    None => it.next().map(|s| (*s).to_string()).ok_or("--rows needs a value")?,
                };
                rows = v.trim().parse().map_err(|_| format!("`{v}` is not a row count"))?;
            }
            "--frames" => {
                let v = match inline {
                    Some(v) => v,
                    None => it.next().map(|s| (*s).to_string()).ok_or("--frames needs a value")?,
                };
                frames = v.trim().parse().map_err(|_| format!("`{v}` is not a frame count"))?;
            }
            other => {
                return Err(format!("bench-ui: unknown argument `{other}` (usage: cargo xtask bench-ui [--rows=N] [--frames=N] [--out PATH])"));
            }
        }
    }
    eprintln!("bench-ui: the release build of the UI crate, then {frames} frames a phase — this takes a few minutes the first time");
    let mut cmd = crate::cargo();
    cmd.args([
        "run",
        "--release",
        "-q",
        "-p",
        "cella-ui-egui",
        "--example",
        "frame_time",
        "--",
        "--json",
        &format!("--rows={rows}"),
        &format!("--frames={frames}"),
    ]);
    let text = capture(cmd, root)?;
    let page_json: serde_json::Value = serde_json::from_str(text.trim()).map_err(|e| format!("frame_time did not print JSON: {e}"))?;
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).map_err(|e| format!("read Cargo.toml: {e}"))?;
    let version = crate::version::read(&manifest)?;
    let sha = git(root, &["rev-parse", "--short", "HEAD"])?;
    let date = git(root, &["log", "-1", "--format=%cs"])?;
    let facts = Facts { version: &version, sha: &sha, date: &date, machine: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH) };
    let page = render_ui(&facts, &page_json);
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    }
    let text = crate::eol::for_write(&out, &page);
    std::fs::write(&out, text).map_err(|e| format!("write {}: {e}", out.display()))?;
    println!("bench-ui: {} rows, {} frames a phase -> {}", thousands(rows), frames, shown(root, &out));
    // The example's own budget check is the pass/fail: re-read it from the phases it reported.
    let over: Vec<String> = page_json
        .get("phases")
        .and_then(serde_json::Value::as_array)
        .map(|phases| {
            phases
                .iter()
                .filter(|p| {
                    p.get("median_ms").and_then(serde_json::Value::as_f64).unwrap_or(0.0)
                        > p.get("budget_ms").and_then(serde_json::Value::as_f64).unwrap_or(f64::MAX)
                })
                .map(|p| {
                    format!(
                        "{} {:.2} ms > {} ms",
                        p.get("phase").and_then(serde_json::Value::as_str).unwrap_or("?"),
                        p.get("median_ms").and_then(serde_json::Value::as_f64).unwrap_or(0.0),
                        p.get("budget_ms").and_then(serde_json::Value::as_f64).unwrap_or(0.0)
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    if !over.is_empty() {
        return Err(format!("bench-ui: {} phase(s) over budget: {}", over.len(), over.join(", ")));
    }
    println!("bench-ui: every phase within budget");
    Ok(())
}

/// `docs/performance-ui.md` from the example's own numbers.
fn render_ui(f: &Facts, doc: &serde_json::Value) -> String {
    let rows = doc.get("rows").and_then(serde_json::Value::as_u64).unwrap_or(0);
    let frames = doc.get("frames").and_then(serde_json::Value::as_u64).unwrap_or(0);
    let window = doc
        .get("window")
        .and_then(serde_json::Value::as_array)
        .map(|w| w.iter().filter_map(serde_json::Value::as_f64).map(|n| format!("{n:.0}")).collect::<Vec<_>>().join("×"))
        .unwrap_or_else(|| "?".into());
    let mut s = String::new();
    s += "# Frame time\n\n";
    s += "<!-- Generated by `cargo xtask bench-ui` from crates/ui-egui/examples/frame_time.rs. Don't edit by hand. -->\n\n";
    s += &format!("> **Generated** for Cella {} at `{}` ({}). Machine: {}. ", f.version, f.sha, f.date, f.machine);
    s += "Whole-window CPU time of one frame, rendered offscreen with no GPU.\n\n";
    s += "`cargo xtask perf` prices operations per command; this prices **one repaint**, which is what \
          a person actually waits on while scrolling. The window is ";
    s += &format!("{window} pt over {} rows × 6 columns, {frames} frames a phase, after six warm-up frames. ", thousands(rows as usize));
    s += "A phase's median is the number; the budget is the point at which the CPU cost of a frame \
          would start dropping frames at 60 Hz on this class of machine — loose on purpose, because \
          this measures the CPU half of a frame and not the GPU, the compositor or a real window's \
          input latency.\n\n";
    s += "| phase | what it does |\n|---|---|\n";
    s += "| `idle` | repaints with the document unchanged: the floor every frame pays |\n";
    s += "| `page` | scrolls one screen a frame: a new band of rows laid out and painted every frame |\n";
    s += "| `jump` | jumps 60,000 rows every frame: cold bands, a new used-range origin — the worst case a scrollbar drag creates |\n\n";
    s += "| phase | median | min | p90 | max | budget | of budget |\n|---|---|---|---|---|---|---|\n";
    let mut over = Vec::new();
    for p in doc.get("phases").and_then(serde_json::Value::as_array).cloned().unwrap_or_default() {
        let name = p.get("phase").and_then(serde_json::Value::as_str).unwrap_or("?");
        let median = p.get("median_ms").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
        let min = p.get("min_ms").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
        let p90 = p.get("p90_ms").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
        let max = p.get("max_ms").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
        let budget = p.get("budget_ms").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
        let share = if budget > 0.0 { median / budget * 100.0 } else { 0.0 };
        if median > budget {
            over.push((name.to_string(), median, budget));
        }
        s += &format!(
            "| `{name}` | {} | {} | {} | {} | {} | {} |\n",
            fmt_ms(median),
            fmt_ms(min),
            fmt_ms(p90),
            fmt_ms(max),
            fmt_ms(budget),
            if share > 100.0 { format!("{share:.0}%!") } else { format!("{share:.0}%") }
        );
    }
    s += "\n";
    if over.is_empty() {
        s += "Every phase is within budget. ";
    } else {
        s += "**Over budget:** ";
        for (name, median, budget) in &over {
            s += &format!("`{name}` {} against {}; ", fmt_ms(*median), fmt_ms(*budget));
        }
    }
    s += "What a regression looks like: a frame that reads the store per visible cell, a used-range \
          walk per frame, or layout work that stopped being cached shows up in `page` and `jump` \
          long before it shows up in any command's time.\n\n";
    s += "## What this does not measure\n\n";
    s += "- **The GPU**: no wgpu here, so no present, no vsync, no texture upload. The repository \
          owns the CPU half of a frame; the graphics driver owns the other one.\n";
    s += "- **A real window**: input latency, the compositor and DPI changes are the desktop \
          shell's, and `plan/bench/` has the scripts that measured them once (startup ~1.02 s, \
          input-to-state ~40 ms round trip).\n";
    s += "- **Other platforms**: one Windows machine.\n";
    s
}

/// `cargo xtask bench-coverage [--check] [--write] [--path PATH]`
pub fn run_coverage(root: &Path, args: &[&str]) -> Result<(), String> {
    let mut verify = false;
    let mut write = false;
    let mut path = ledger(root);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "--check" => verify = true,
            "--write" => write = true,
            "--path" => path = PathBuf::from(it.next().ok_or("--path needs a path")?),
            other => return Err(format!("bench-coverage: unknown argument `{other}` (usage: cargo xtask bench-coverage [--check] [--write])")),
        }
    }
    let registered: Vec<&str> = cella_engine::command_specs().iter().map(|spec| spec.id).collect();
    let existing = std::fs::read_to_string(&path).ok();
    let header = existing.as_deref().map(header_of).unwrap_or_else(|| HEADER.to_string());
    let mut entries = existing.as_deref().map(parse).unwrap_or_default();
    if write {
        let known: BTreeSet<String> = registered.iter().map(|id| (*id).to_string()).collect();
        let stale: Vec<String> = entries.iter().map(|e| e.id.clone()).filter(|id| !known.contains(id)).collect();
        entries.retain(|e| known.contains(&e.id));
        let kept = entries.len();
        // A new command that already has a number in the catalogue starts `bench`; everything else
        // starts `todo`, so the check keeps failing until a person classifies it.
        let priced = catalogue_ids(root);
        for id in &registered {
            if entries.iter().any(|e| e.id == **id) {
                continue;
            }
            let class = if priced.contains(*id) { Class::Bench } else { Class::Todo };
            entries.push(Entry { id: (*id).to_string(), class, reason: String::new() });
        }
        let text = render(&header, &entries);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        }
        let text = crate::eol::for_write(&path, &text);
        std::fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))?;
        println!(
            "bench-coverage: {} ({} entries, {} added, {} stale dropped, {})",
            shown(root, &path),
            entries.len(),
            entries.len() - kept,
            stale.len(),
            summary(&entries)
        );
        for id in &stale {
            println!("  dropped `{id}`: not a registered command");
        }
        return Ok(());
    }
    let priced = catalogue_ids(root);
    let problems = check(&entries, &registered, Some(&priced));
    if verify {
        if problems.is_empty() {
            println!("bench-coverage: {} ({})", shown(root, &path), summary(&entries));
            return Ok(());
        }
        let mut msg = format!("{} problem(s) in {} ({}):", problems.len(), shown(root, &path), summary(&entries));
        for p in problems.iter().take(40) {
            msg += &format!("\n  - {p}");
        }
        if problems.len() > 40 {
            msg += &format!("\n  … and {} more", problems.len() - 40);
        }
        msg += "\n\nWhere a command's cost is observed: `bench` (docs/performance-commands.md), `ui` \
                (frame time) or `excuse: why`. `cargo xtask bench-coverage --write` adds the lines \
                that are missing.";
        return Err(msg);
    }
    println!("bench-coverage: {} ({})", shown(root, &path), summary(&entries));
    for p in problems {
        println!("  - {p}");
    }
    Ok(())
}

/// The comment block at the top of an existing ledger, kept as it was written.
fn header_of(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            out.push_str(line);
            out.push('\n');
        } else {
            break;
        }
    }
    out
}

/// The ids `docs/performance-commands.md` carries a *usable* number for; empty when the page is
/// missing. A row whose every configuration was turned down appears in the page twice — once in
/// the table, once under "No number yet" — and only the second list counts against it.
fn catalogue_ids(root: &Path) -> BTreeSet<String> {
    let mut every = BTreeSet::new();
    let mut unpriced = BTreeSet::new();
    let Ok(text) = std::fs::read_to_string(root.join("docs/performance-commands.md")) else {
        return every;
    };
    let mut section = String::new();
    for line in text.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            section = heading.trim().to_string();
            continue;
        }
        // A catalogue row starts `| \`id\` |`; the header and separator rows do not.
        let Some(rest) = line.strip_prefix("| `") else { continue };
        let Some((id, _)) = rest.split_once('`') else { continue };
        if id.contains('/') || id.contains(' ') {
            continue;
        }
        if section == "Every command" {
            every.insert(id.to_string());
        } else if section == "No number yet, and what the command said" {
            unpriced.insert(id.to_string());
        }
    }
    for id in unpriced {
        every.remove(&id);
    }
    every
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(id: &str, class: Class, reason: &str) -> Entry {
        Entry { id: id.into(), class, reason: reason.into() }
    }

    #[test]
    fn lines_parse_into_a_class_and_a_reason() {
        let text = "# a comment\n\nhome.bold bench\nselection.set ui\nfile.open  excuse: reads files\nhome.save   todo\n";
        let entries = parse(text);
        assert_eq!(
            entries,
            [
                e("home.bold", Class::Bench, ""),
                e("selection.set", Class::Ui, ""),
                e("file.open", Class::Excuse, "reads files"),
                e("home.save", Class::Todo, "")
            ]
        );
    }

    #[test]
    fn an_unclassified_line_is_todo_rather_than_lost() {
        let entries = parse("home.bold\n");
        assert_eq!(entries[0].class, Class::Todo);
    }

    #[test]
    fn every_problem_says_what_to_do_about_it() {
        let entries = [
            e("home.bold", Class::Bench, ""),
            e("home.bold", Class::Bench, ""),
            e("home.gone", Class::Ui, ""),
            e("home.todo", Class::Todo, ""),
            e("file.open", Class::Excuse, ""),
            e("data.sort", Class::Bench, ""),
        ];
        let registered = ["home.bold", "home.todo", "file.open", "data.sort", "home.new"];
        let priced: BTreeSet<String> = ["home.bold", "data.sort"].iter().map(|s| s.to_string()).collect();
        let problems = check(&entries, &registered, Some(&priced));
        let all = problems.join("\n");
        assert!(all.contains("`home.bold` has more than one line"), "{all}");
        assert!(all.contains("`home.gone` is not a registered command"), "{all}");
        assert!(all.contains("`home.todo` is still `todo`"), "{all}");
        assert!(all.contains("`file.open` is excused for no stated reason"), "{all}");
        assert!(all.contains("`home.new` has no line"), "{all}");
        // `data.sort` is bench and priced: not a problem. Five problems in all.
        assert_eq!(problems.len(), 5, "{all}");
    }

    #[test]
    fn a_bench_line_the_catalogue_does_not_price_is_a_problem() {
        let entries = [e("data.sort", Class::Bench, "")];
        let priced: BTreeSet<String> = ["home.bold"].iter().map(|s| s.to_string()).collect();
        let problems = check(&entries, &["data.sort"], Some(&priced));
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("no usable number for it"), "{problems:?}");
        // Without a generated page the cross-check is skipped, not failed.
        assert!(check(&entries, &["data.sort"], None).is_empty());
    }

    #[test]
    fn the_ledger_is_written_sorted_and_reads_back_the_same() {
        let entries = [e("home.bold", Class::Bench, ""), e("file.open", Class::Excuse, "reads files"), e("data.sort", Class::Ui, "")];
        let text = render("# header\n", &entries);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "# header");
        assert_eq!(lines[1].trim_end(), "data.sort                                    ui");
        assert!(text.contains("file.open") && text.contains("excuse: reads files"));
        let parsed = parse(&text);
        assert_eq!(parsed.len(), 3);
        assert_eq!(header_of(&text).trim(), "# header");
    }

    #[test]
    fn the_catalogue_parses_and_renders_one_row_per_command() {
        let json = r#"{"schema":1,"rows":[1000],"scales":["small","whole"],"repeat":1,"commands":2,
            "notRun":[{"id":"file.open","why":"reads files"}],
            "measurements":[
              {"id":"home.bold","rows":1000,"scale":"small","ms":0.0123,"changed":true,"outcome":"ok"},
              {"id":"home.bold","rows":1000,"scale":"whole","ms":1234.5,"changed":true,"outcome":"ok"},
              {"id":"data.sort","rows":1000,"scale":"small","ms":0.0,"changed":false,"outcome":"refused","error":"nothing to sort"},
              {"id":"data.sort","rows":1000,"scale":"whole","ms":4.5,"changed":true,"outcome":"ok"}]}"#;
        let c = parse_catalogue(json).unwrap_or_default();
        assert_eq!((c.rows.clone(), c.scales.len(), c.commands), (vec![1000], 2, 2));
        let facts = Facts { version: "0.0.0", sha: "deadbeef", date: "2026-11-23", machine: "test".into() };
        let page = render_catalogue(&facts, &c);
        assert!(page.contains("| `home.bold` | 12 us | 1.23 s |  |"), "{page}");
        assert!(page.contains("| `data.sort` | 0 us | 4.5 ms | small: refused: nothing to sort |"), "{page}");
        assert!(page.contains("## The twenty slowest commands at 1,000 rows"), "{page}");
        assert!(page.contains("| `file.open` | reads files |"), "{page}");
        assert!(page.contains("2 of them measured."), "{page}");
        // One row in the full table, plus the two rows of the slowest table (one per scale) —
        // a command that names no region appears once per scale there, because the scale is the row.
        assert_eq!(page.lines().filter(|l| l.starts_with("| `home.bold`")).count(), 3, "{page}");
    }

    #[test]
    fn thousands_groups_the_digits() {
        assert_eq!(thousands(1_000_000), "1,000,000");
        assert_eq!(thousands(100_000), "100,000");
        assert_eq!(thousands(999), "999");
    }

    #[test]
    fn the_call_table_reads_name_template_and_the_error_flag() {
        let calls = parse_calls_table("# a comment\n\nSUM\tSUM($R)\nNA\tNA()\terror\n").expect("parses");
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], ("SUM".to_string(), "SUM($R)".to_string(), false));
        assert!(calls[1].2);
        assert!(parse_calls_table("SUM\n").is_err());
    }

    #[test]
    fn the_check_names_every_way_a_call_table_can_be_wrong() {
        let complete = vec!["SUM".to_string(), "AVERAGE".to_string()];
        assert!(check_calls(&[("SUM".into(), "SUM($R)".into(), false), ("AVERAGE".into(), "AVERAGE($R)".into(), false)], &complete).is_empty());
        let registered = vec!["SUM".to_string(), "AVERAGE".to_string(), "MEDIAN".to_string()];
        let problems = check_calls(
            &[
                ("SUM".into(), "SUM($R)".into(), false),
                ("SUM".into(), "SUM(1)".into(), false),
                ("NOPE".into(), "NOPE(1)".into(), false),
                ("AVERAGE".into(), "SUM($R)".into(), false),
            ],
            &registered,
        );
        assert!(problems.iter().any(|p| p.contains("two lines")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("not a registered function")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("starts with `SUM(`")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("no line")), "{problems:?}");
    }

    #[test]
    fn the_function_page_carries_every_reading_and_the_floor() {
        let json = r##"{
            "schema": 1, "rows": [100000], "repeat": 1, "scales": ["small", "whole"],
            "floors": [{"rows": 100000, "ms": 0.2}],
            "measurements": [
                {"name": "SUM", "template": "SUM($R)", "rows": 100000, "scale": "small", "ms": 0.1, "outcome": "ok", "expected": false},
                {"name": "SUM", "template": "SUM($R)", "rows": 100000, "scale": "whole", "ms": 4.0, "outcome": "ok", "expected": false},
                {"name": "NA", "template": "NA()", "rows": 100000, "scale": "scalar", "ms": 0.1, "outcome": "error", "expected": true, "detail": "#N/A"},
                {"name": "TRANSPOSE", "template": "TRANSPOSE($R)", "rows": 100000, "scale": "whole", "ms": 3.0, "outcome": "error", "expected": false, "detail": "#SPILL!"}
            ]
        }"##;
        let c = parse_functions(json).expect("parses");
        assert_eq!(c.floors, vec![(100_000, 0.2)]);
        let facts = Facts { version: "0.0.0", sha: "deadbee", date: "2026-11-23", machine: "test".to_string() };
        let page = render_functions(&facts, &c);
        assert!(page.contains("The floor"), "{page}");
        assert!(page.contains("| `SUM` | `SUM($R)` | 100 us | 4.0 ms |"), "{page}");
        assert!(page.contains("error by design: #N/A"), "{page}");
        assert!(page.contains("error: #SPILL!"), "{page}");
        assert!(page.contains("3 functions, 1 with a number at 100,000 rows"), "{page}");
    }
}
