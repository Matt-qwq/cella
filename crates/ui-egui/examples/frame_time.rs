//! Frame time of the sheet: what one repaint of the whole window costs at a large row count.
//!
//! ```text
//! cargo run --release -p cella-ui-egui --example frame_time                       # 1M rows
//! cargo run --release -p cella-ui-egui --example frame_time -- --rows=100000 --frames=40
//! cargo run --release -p cella-ui-egui --example frame_time -- --json             # what
//!                                                                                 # `cargo xtask
//!                                                                                 # bench-ui` reads
//! ```
//!
//! This is the *interactive* ruler the throughput gate cannot be: `cargo xtask perf` prices
//! operations in milliseconds per command, while a person scrolling a million rows feels
//! milliseconds per **frame** — the same budget at 144 Hz (6.9 ms) and 60 Hz (16.7 ms) whether or
//! not the document changed. `docs/performance-ui.md` is the page this feeds.
//!
//! Each phase drives real input through the whole application (`SheetApp::logic` + `ui`, the
//! ribbon and panes included) with no GPU: an offscreen `egui_kittest` harness, one `step()` a
//! frame, the clock around it. The phases are what a session actually does:
//!
//! - `idle` — repaints at a fixed scroll position: the document did not change, so this is the
//!   floor (and what a still window costs when a timer or an animation asks for a frame).
//! - `page` — scrolls one screen a frame: every frame lays out and paints a new band of rows.
//! - `jump` — jumps to a different region of the sheet every frame: cold bands, a new used-range
//!   probe, the row/column headers at a new origin. The worst case a scrollbar drag creates.
//!
//! What this does **not** measure: the GPU (no wgpu here, so no present, no vsync, no upload), the
//! OS compositor, or a real window's input latency. It measures the CPU work of a frame, which is
//! the part this repository owns.

#![allow(clippy::disallowed_methods)] // a native-only benchmark: wasm never runs it, and the clock *is* the measurement

use std::process::ExitCode;
use std::time::Instant;

use cella_engine::Session;
use cella_ui_egui::SheetApp;
use serde_json::{Value, json};

/// Rows written per `range.setValues` call when the workbook is built.
const CHUNK: usize = 10_000;
/// Largest row count this harness accepts — an argument-derived size, so it is capped.
const MAX_ROWS: usize = 5_000_000;
/// Columns of data: the reference workbook's shape (`docs/performance.md`).
const COLS: usize = 6;
/// Frames a phase runs. Enough for a median to mean something, short enough to run three phases in
/// a few seconds.
const DEFAULT_FRAMES: usize = 30;
/// Window size in points: the size the UI tests and the snapshot example use.
const WINDOW: (f32, f32) = (1440.0, 900.0);
/// A phase's median may not exceed these (ms) — the loose budget of `docs/performance-ui.md`.
/// Measured on the development machine (1M rows, 1440×900): idle 0.92, page 1.67, jump 1.74. The
/// budgets are three to five times that: this measures the CPU half of a frame, and it must not
/// turn a shared or slower machine into a red gate that people learn to ignore. A real regression
/// — a per-cell store read, a used-range walk, layout that stopped being cached — is a multiple,
/// not a percentage.
const BUDGET_IDLE_MS: f64 = 3.0;
const BUDGET_PAGE_MS: f64 = 6.0;
const BUDGET_JUMP_MS: f64 = 8.0;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("frame_time: {e}");
            ExitCode::FAILURE
        }
    }
}

struct Args {
    rows: usize,
    frames: usize,
    json: bool,
    check: bool,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut out = Args { rows: 1_000_000, frames: DEFAULT_FRAMES, json: false, check: false };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) => (f, Some(v)),
            None => (a.as_str(), None),
        };
        match flag {
            "--rows" => {
                let v = inline.map(str::to_string).or_else(|| it.next().cloned()).ok_or("--rows needs a value")?;
                out.rows = rows_of(&v)?;
            }
            "--frames" => {
                let v = inline.map(str::to_string).or_else(|| it.next().cloned()).ok_or("--frames needs a value")?;
                out.frames = v.trim().parse().map_err(|_| format!("`{v}` is not a frame count"))?;
                if !(3..=1000).contains(&out.frames) {
                    return Err("--frames must be 3..=1000".to_string());
                }
            }
            "--json" => out.json = true,
            "--check" => out.check = true,
            "-h" | "--help" => {
                println!("usage: frame_time [--rows=N] [--frames=N] [--json] [--check]");
                return Err(String::new());
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(out)
}

fn rows_of(text: &str) -> Result<usize, String> {
    let rows: usize = text.trim().parse().map_err(|_| format!("`{text}` is not a row count"))?;
    if rows == 0 || rows > MAX_ROWS {
        return Err(format!("row count {rows} is out of range (1..={MAX_ROWS})"));
    }
    Ok(rows)
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&args) {
        Ok(a) => a,
        Err(e) if e.is_empty() => return Ok(()),
        Err(e) => return Err(e),
    };
    if cfg!(debug_assertions) {
        eprintln!("frame_time: this is a debug build — the numbers are not comparable (use --release)");
    }
    let session = workbook(args.rows)?;
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(WINDOW.0, WINDOW.1)).with_pixels_per_point(1.0).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    // Warm-up: the first frames build the layout, the fonts and the caches every later frame
    // reuses. They are not what a session pays per frame.
    harness.run_steps(6);
    let row_px = harness.state().grid.rect.map(|r| r.height()).unwrap_or(WINDOW.1);
    let mut phases: Vec<Phase> = Vec::new();

    phases.push(phase("idle", args.frames, &mut harness, |_| {}));
    phases.push(phase("page", args.frames, &mut harness, move |h| {
        let y = h.state().view().scroll.y + row_px;
        if let Some(view) = h.state_mut().view_mut() {
            view.scroll.y = y;
        }
    }));
    let rows_wanted = args.rows.max(1);
    let mut jump_at = 0usize;
    phases.push(phase("jump", args.frames, &mut harness, move |h| {
        // A different region every frame: 60k rows apart, so each frame reads bands the last one
        // did not, and the scrollbar's used-range probe runs on a new origin.
        jump_at = (jump_at + 60_000) % rows_wanted;
        if let Some(view) = h.state_mut().view_mut() {
            view.scroll.y = jump_at as f32;
        }
    }));

    let over: Vec<&Phase> = phases.iter().filter(|p| p.median() > p.budget).collect();
    if args.json {
        let doc = json!({
            "schema": 1,
            "rows": args.rows,
            "cols": COLS,
            "frames": args.frames,
            "window": [WINDOW.0, WINDOW.1],
            "release": !cfg!(debug_assertions),
            "phases": phases.iter().map(Phase::json).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?);
    } else {
        println!("\n{} rows × {COLS} columns, {}×{} pt, {} frames a phase\n", args.rows, WINDOW.0, WINDOW.1, args.frames);
        println!("{:<8} {:>9} {:>9} {:>9} {:>9} {:>10}", "phase", "median", "min", "p90", "max", "budget");
        for p in &phases {
            println!(
                "{:<8} {:>9} {:>9} {:>9} {:>9} {:>10}",
                p.name,
                fmt_ms(p.median()),
                fmt_ms(p.min()),
                fmt_ms(p.p90()),
                fmt_ms(p.max()),
                fmt_ms(p.budget)
            );
        }
    }
    if !over.is_empty() {
        let names: Vec<String> = over.iter().map(|p| format!("{} {} > {}", p.name, fmt_ms(p.median()), fmt_ms(p.budget))).collect();
        return Err(format!("{} phase(s) over budget: {}", over.len(), names.join(", ")));
    }
    if args.check {
        println!("\nOK: every phase within its budget");
    }
    Ok(())
}

/// One measured phase: the per-frame times and the budget its median may not exceed.
struct Phase {
    name: &'static str,
    frames: Vec<f64>,
    budget: f64,
}

impl Phase {
    fn sorted(&self) -> Vec<f64> {
        let mut v = self.frames.clone();
        v.sort_by(f64::total_cmp);
        v
    }
    fn median(&self) -> f64 {
        let v = self.sorted();
        v.get(v.len() / 2).copied().unwrap_or(0.0)
    }
    fn min(&self) -> f64 {
        self.sorted().first().copied().unwrap_or(0.0)
    }
    fn p90(&self) -> f64 {
        let v = self.sorted();
        let at = (v.len() as f64 * 0.9).floor() as usize;
        v.get(at.min(v.len().saturating_sub(1))).copied().unwrap_or(0.0)
    }
    fn max(&self) -> f64 {
        self.sorted().last().copied().unwrap_or(0.0)
    }
    fn json(&self) -> Value {
        json!({
            "phase": self.name,
            "frames": self.frames.len(),
            "median_ms": round(self.median()),
            "min_ms": round(self.min()),
            "p90_ms": round(self.p90()),
            "max_ms": round(self.max()),
            "budget_ms": self.budget,
        })
    }
}

/// Times `frames` steps, after each of which `before` moves the window.
fn phase(
    name: &'static str,
    frames: usize,
    harness: &mut egui_kittest::Harness<'static, SheetApp>,
    mut before: impl FnMut(&mut egui_kittest::Harness<'static, SheetApp>),
) -> Phase {
    // A slower machine: CELLA_UI_SCALE=2 multiplies every budget, the same escape hatch the
    // throughput gate has.
    let scale = std::env::var("CELLA_UI_SCALE").ok().and_then(|s| s.trim().parse::<f64>().ok()).filter(|s| *s > 0.0).unwrap_or(1.0);
    let budget = match name {
        "idle" => BUDGET_IDLE_MS,
        "page" => BUDGET_PAGE_MS,
        _ => BUDGET_JUMP_MS,
    } * scale;
    let mut times = Vec::with_capacity(frames);
    for _ in 0..frames {
        before(harness);
        let started = Instant::now();
        harness.run_steps(1);
        times.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    Phase { name, frames: times, budget }
}

fn round(ms: f64) -> f64 {
    (ms * 1000.0).round() / 1000.0
}

fn fmt_ms(ms: f64) -> String {
    if ms >= 10.0 { format!("{ms:.2} ms") } else { format!("{ms:.3} ms") }
}

/// The workbook the phases run on: `rows` × 6 columns of values, one selection at `A1`.
fn workbook(rows: usize) -> Result<Session, String> {
    let mut s = Session::new();
    s.new_workbook();
    let mut r = 0;
    while r < rows {
        let n = CHUNK.min(rows - r);
        let values: Vec<Value> = (0..n)
            .map(|i| {
                let n = r + i;
                json!([format!("Item {n}"), n as f64, (n % 97) as f64, (n % 13) as f64 * 1.5, 1.0, 2.0])
            })
            .collect();
        s.execute("range.setValues", json!({"range": format!("A{}", r + 1), "values": values})).map_err(|e| e.to_string())?;
        r += n;
    }
    s.execute("selection.set", json!({"range": "A1"})).map_err(|e| e.to_string())?;
    Ok(s)
}
