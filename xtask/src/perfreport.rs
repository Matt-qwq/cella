//! `cargo xtask perf-report`: one generated page from the harnesses themselves.
//!
//! It runs the scenario gate (`cargo xtask perf`) and the live-heap example in release, and writes
//! `docs/performance-report.md` and its Chinese companion `docs/performance-report.zh-CN.md` from
//! their own output, so the numbers on the page are the numbers the tools printed - the page cannot
//! drift the way a hand-copied table does. The external comparison lives in
//! `docs/external-benchmark.md` (it needs Excel, WPS and a second build) and is embedded here, and
//! what is *not* measured is listed on the page itself.

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

/// The external benchmark, demoted one heading level so it nests inside a report section.
fn external(root: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(root.join("docs/external-benchmark.md")).map_err(|e| format!("read docs/external-benchmark.md: {e}"))?;
    let body = text.split_once("\n").map(|(_, rest)| rest).unwrap_or("").trim_start();
    Ok(body.replace("\n## ", "\n### "))
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

    let benchmark = external(root)?;
    let facts = Facts { version: &version, sha: &sha, date: &date, rustc: rustc.trim(), table: &table, heap: &heap, benchmark: &benchmark };
    for (path, page) in [("docs/performance-report.md", render_en(&facts)), ("docs/performance-report.zh-CN.md", render_zh(&facts))] {
        let path = root.join(path);
        let text = crate::eol::for_write(&path, &page);
        std::fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))?;
        println!("perf-report: wrote {} ({} lines)", path.display(), page.lines().count());
    }
    Ok(())
}

struct Facts<'a> {
    version: &'a str,
    sha: &'a str,
    date: &'a str,
    rustc: &'a str,
    table: &'a str,
    heap: &'a str,
    benchmark: &'a str,
}

fn render_en(f: &Facts) -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let (version, sha, date, rustc, table, heap, benchmark) = (f.version, f.sha, f.date, f.rustc, f.table, f.heap, f.benchmark);
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

## Measured against Excel, WPS and upstream GridCraft

{benchmark}

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
cargo xtask perf-report                       # this page and its Chinese companion
```
"#
    )
}

fn render_zh(f: &Facts) -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let (version, sha, date, rustc, table, heap, benchmark) = (f.version, f.sha, f.date, f.rustc, f.table, f.heap, f.benchmark);
    format!(
        r#"# 性能报告

> 由 `cargo xtask perf-report` 生成，对象是 Cella {version}，提交 `{sha}`（{date}）。
> 机器：{os} {arch}；{rustc}。所有数字都是同一台空闲机器上的墙钟时间，本机跑动之间波动约 10%，
> 这也是门禁预算取"安静一次的两倍"的原因。方法与口径见 [performance.md](performance.md)。

## 场景：10 万行与 100 万行（release，单次最优）

```text
{table}
```

## 活堆内存：100 万行 x 6 列、200 万公式

```text
{heap}
```

## 与 Excel、WPS、上游 GridCraft 的横评

{benchmark}

## 没有测量的项目

下面没有数字，代表**没有测**，不是 0：

- **启动时间**：冷启动、热启动，带文档 / 不带文档。
- **界面交互延迟**：按键到单元格重绘、100 万行滚动、打开对话框、图表重绘。只有帧绘制（约 1.4 ms @144 Hz）有数字。
- **平台**：macOS、Linux、wasm 都没有测；以上全部来自一台 Windows 机器。
- **对 Excel 的多负载对比**：透视表、图表、打印/PDF、CSV 导入、查询与 Parquet、多工作表、1 亿单元格分页。
- **长时间运行内存**：没有一小时级别的漂移观察。

## 复现

```bash
cargo xtask perf                              # 场景门，对照 xtask/perf-budgets.txt
cargo run --release -p cella-engine --example memory -- 1000000
cargo xtask perf-report                       # 重新生成本页与英文版
```
"#
    )
}
