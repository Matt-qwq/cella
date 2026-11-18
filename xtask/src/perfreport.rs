//! `cargo xtask perf-report`: one generated page from the harnesses themselves.
//!
//! It runs the scenario gate (`cargo xtask perf`) and the live-heap example in release, and writes
//! `docs/performance-report.md` with their own output, so the numbers on the page are the numbers
//! the tools printed - the page cannot drift the way a hand-copied table does. What needs another
//! application (the Excel/WPS comparison) or another platform stays curated in
//! `docs/performance.md` and is linked, and what is *not* measured is listed on the page itself.

use std::path::Path;
use std::process::Command;

/// Runs `cmd` in `dir` and keeps its stdout; a failure carries the tail of its stderr.
fn capture(mut cmd: Command, dir: &Path) -> Result<String, String> {
    cmd.current_dir(dir);
    let out = cmd.output().map_err(|e| format!("run {cmd:?}: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = err.lines().rev().take(6).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        return Err(format!("{cmd:?} failed:\n{}", tail.join("\n")));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut c = Command::new("git");
    c.arg("-C").arg(root).args(args);
    Ok(capture(c, root)?.trim().to_string())
}

pub fn run(root: &Path) -> Result<(), String> {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).map_err(|e| format!("read Cargo.toml: {e}"))?;
    let version = crate::version::read(&manifest)?;
    let sha = git(root, &["rev-parse", "--short", "HEAD"])?;
    let date = git(root, &["log", "-1", "--format=%cs"])?;
    let mut rv = Command::new("rustc");
    rv.arg("-V");
    let rustc = capture(rv, root)?;
    let exe = std::env::current_exe().map_err(|e| format!("current exe: {e}"))?;

    eprintln!("perf-report: the scenario gate — this measures the release build and takes a few minutes");
    let mut perf = Command::new(&exe);
    perf.arg("perf");
    let table = capture(perf, root)?;

    eprintln!("perf-report: the live heap at 1,000,000 x 6");
    let mut mem = Command::new("cargo");
    mem.args(["run", "--release", "-q", "-p", "cella-engine", "--example", "memory", "--", "1000000"]);
    let heap = capture(mem, root)?;

    let page = render(&version, &sha, &date, rustc.trim(), &table, &heap);
    let path = root.join("docs/performance-report.md");
    let text = crate::eol::for_write(&path, &page);
    std::fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))?;
    println!("perf-report: wrote {} ({} lines)", path.display(), page.lines().count());
    Ok(())
}

fn render(version: &str, sha: &str, date: &str, rustc: &str, table: &str, heap: &str) -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    format!(
        r#"# Performance report

> **Generated** by `cargo xtask perf-report` for Cella {version} at `{sha}` ({date}).
> Machine: {os} {arch}; {rustc}. Every number is wall clock on one idle machine, and this
> machine's run-to-run spread is about 10 %, which is why a gate budget is twice a quiet pass.
> Method and caveats: [performance.md](performance.md).

## Scenarios, 100,000 and 1,000,000 rows (release, best of one pass)

```text
{table}
```

## Live heap, 1M rows x 6 columns with 2M formulas

```text
{heap}
```

## Measured against Excel and WPS

The external comparison - the same 70 MB file opened and recalculated by Excel 16.0.20527 and WPS
12.1.0.25222, with working set, private bytes and idle - is in
[performance.md](performance.md#compared-with-excel-and-wps-2026-11-18), with its method and
caveats. It is not generated here because it drives two other applications through COM.

## Not measured

No number below means no measurement, not a zero:

- **Startup**: cold and warm, with and without a document.
- **UI interaction latency**: keystroke to repainted cell, scrolling a million rows, opening a
  dialog, redrawing a chart. Only the frame paint (about 1.4 ms at 144 Hz) has a number.
- **Platforms**: macOS, Linux and wasm are unmeasured; everything above is one Windows machine.
- **Workload variety against Excel**: pivots, charts, print/PDF, CSV import, queries and Parquet,
  multi-sheet workbooks, and the 100M-cell paged case.
- **Long-run memory**: no hour-long session drift.

## Reproduce

```bash
cargo xtask perf                              # the scenario gate against xtask/perf-budgets.txt
cargo run --release -p cella-engine --example memory -- 1000000
cargo xtask perf-report                       # this page
```
"#
    )
}
