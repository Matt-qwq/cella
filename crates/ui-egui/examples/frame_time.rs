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
//! - `key` — a character typed into the selected cell: the frame that opens (or extends) the
//!   in-cell editor. What a keystroke costs before anything is committed.
//! - `commit` — Enter with the editor open: the value is written, every formula that reads it is
//!   recalculated, and the frame paints the result. The workbook carries one `SUM` over the whole
//!   edited column, so this is "type a number into a cell a million-row report watches".
//! - `undo` — Ctrl+Z over one of those commits: the same recalculation, backwards.
//! - `drag` — the pointer dragged across the grid with the button down: the selection grows and the
//!   frame paints it, which is what selecting a block by hand costs per frame.
//!
//! A second table prices the **view settings** — every command the coverage ledger marks `ui`
//! because its cost is a frame, not a command: frozen panes, Page Layout, gridlines off, a ruler,
//! zoom, right-to-left. Each one is turned on in a session of its own and the same two frames are
//! timed with it on — a still repaint and a scrolling one — so the answer is "a frame with this
//! setting costs X ms", and the baseline row (`(none)`) is what it costs with nothing changed.
//!
//! What this does **not** measure: the GPU (no wgpu here, so no present, no vsync, no upload), the
//! OS compositor, or a real window's input latency. It measures the CPU work of a frame, which is
//! the part this repository owns.

#![allow(clippy::disallowed_methods)] // a native-only benchmark: wasm never runs it, and the clock *is* the measurement

use std::process::ExitCode;
use std::time::Instant;

use cella_engine::{DocState, Session};
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
/// A keystroke and a drag are a frame like any other, with a little room for the editor.
const BUDGET_KEY_MS: f64 = 5.0;
const BUDGET_DRAG_MS: f64 = 5.0;
/// A commit and an undo pay the recalculation of the workbook's one watching `SUM` over a million
/// values, so their budget is a guard against "the whole graph is rebuilt on every keystroke",
/// not a frame budget: the number is the recalculation, and the recalculation is the product.
const BUDGET_COMMIT_MS: f64 = 250.0;
const BUDGET_UNDO_MS: f64 = 250.0;
/// The cell the interaction phases edit: a number in the column the watching formula sums.
const EDIT_CELL: &str = "B1";

/// The view settings the coverage ledger marks `ui`: every one of them is a state a frame paints,
/// so the command itself is free and the *repaint with it on* is the cost. Each is turned on in a
/// session of its own and the same still frame is timed with it; `(none)` is the baseline.
const CONFIGS: &[(&str, &str, &str)] = &[
    ("(none)", "no setting: the baseline", "{}"),
    ("formulas.showFormulas", "every cell shows its formula instead of its value", "{\"on\":true}"),
    ("pageLayout.printGridlines", "gridlines are printed", "{\"on\":true}"),
    ("pageLayout.printHeadings", "headings are printed", "{\"on\":true}"),
    ("view.arrangeAll", "every window arranged", "{}"),
    ("view.customViews", "the workbook's custom views", "{}"),
    ("view.formulaBar", "the formula bar is shown", "{\"on\":true}"),
    ("view.freezeFirstColumn", "column A frozen", "{}"),
    ("view.freezePanes", "panes frozen at B2", "{\"cell\":\"B2\"}"),
    ("view.freezeTopRow", "row 1 frozen", "{}"),
    ("view.gridlines", "gridlines off", "{\"on\":false}"),
    ("view.headings", "headings off", "{\"on\":false}"),
    ("view.hideWindow", "the window hidden", "{}"),
    ("view.macros", "the list of scripts", "{}"),
    ("view.normal", "Normal view", "{}"),
    ("view.pageBreakPreview", "Page Break Preview", "{}"),
    ("view.pageLayout", "Page Layout view", "{}"),
    ("view.rightToLeft", "the sheet right-to-left", "{\"on\":true}"),
    ("view.ruler", "rulers on", "{\"on\":true}"),
    ("view.showHorizontalScrollBar", "the horizontal scroll bar off", "{\"on\":false}"),
    ("view.showObjects", "objects hidden", "{\"on\":false}"),
    ("view.showOutlineSymbols", "outline symbols hidden", "{\"on\":false}"),
    ("view.showSheetTabs", "the sheet tabs off", "{\"on\":false}"),
    ("view.showVerticalScrollBar", "the vertical scroll bar off", "{\"on\":false}"),
    ("view.showZeros", "zeros hidden", "{\"on\":false}"),
    ("view.split", "split at the active cell", "{}"),
    ("view.switchWindows", "the window list", "{}"),
    ("view.unfreezePanes", "panes unfrozen", "{}"),
    ("view.unhideWindow", "a hidden window shown again", "{}"),
    ("view.zoom", "125% zoom", "{\"percent\":125}"),
    ("view.zoom100", "100% zoom", "{}"),
    ("view.zoomToSelection", "zoom to fit the selection", "{}"),
];

/// One view setting's frames: a still repaint, and a repaint while the sheet scrolls, because a
/// setting that changes nothing in a still frame can still change what scrolling lays out.
struct ConfigRow {
    id: &'static str,
    what: &'static str,
    /// `baseline`, `on`, or why the command would not turn it on.
    outcome: String,
    idle: Vec<f64>,
    page: Vec<f64>,
}

impl ConfigRow {
    fn idle_median(&self) -> f64 {
        median(&self.idle)
    }
    fn page_median(&self) -> f64 {
        median(&self.page)
    }
}

fn median(v: &[f64]) -> f64 {
    let mut v = v.to_vec();
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(0.0)
}

/// A still repaint and a scrolling repaint with each view setting on, in a session of its own.
fn measure_configs(doc: &DocState, frames: usize) -> Result<Vec<ConfigRow>, String> {
    let mut out = Vec::with_capacity(CONFIGS.len());
    for (id, what, params) in CONFIGS {
        let mut s = Session::new();
        s.add_document(doc.clone());
        let mut h = app_harness(s);
        h.run_steps(6);
        let outcome = if *id == "(none)" {
            "baseline".to_string()
        } else {
            let p = serde_json::from_str(params).map_err(|e| format!("{id} params: {e}"))?;
            match h.state_mut().session.run(id, p) {
                Ok(_) => "on".to_string(),
                Err(e) => format!("not turned on: {e}"),
            }
        };
        h.run_steps(2);
        let row_px = h.state().grid.rect.map(|r| r.height()).unwrap_or(WINDOW.1);
        let mut idle = Vec::with_capacity(frames);
        for _ in 0..frames {
            let started = Instant::now();
            h.run_steps(1);
            idle.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        let mut page = Vec::with_capacity(frames);
        for _ in 0..frames {
            let y = h.state().view().scroll.y + row_px;
            if let Some(view) = h.state_mut().view_mut() {
                view.scroll.y = y;
            }
            let started = Instant::now();
            h.run_steps(1);
            page.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        out.push(ConfigRow { id, what, outcome, idle, page });
    }
    Ok(out)
}

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
    /// Price the view settings too (on unless `--no-configs`).
    configs: bool,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut out = Args { rows: 1_000_000, frames: DEFAULT_FRAMES, json: false, check: false, configs: true };
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
            "--no-configs" => out.configs = false,
            "-h" | "--help" => {
                println!("usage: frame_time [--rows=N] [--frames=N] [--json] [--check] [--no-configs]");
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
    let (session, doc) = workbook(args.rows)?;
    let mut harness = app_harness(session);
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

    // The interaction half. The workbook gains one formula first — a whole-column SUM over the
    // column these phases edit — so a commit and an undo pay the recalculation a report cell pays,
    // not a value write. Every phase checks that its action actually happened: a frame in which
    // nothing was typed, committed or undone is a frame of `idle`, and timing it as `commit` would
    // be a lie the page could not show.
    harness
        .state_mut()
        .session
        .run("cell.set", json!({"cell": "H1", "input": format!("=SUM(B1:B{rows_wanted})")}))
        .map_err(|e| format!("place the watched formula: {e:?}"))?;
    harness.state_mut().session.run("selection.set", json!({"range": EDIT_CELL})).map_err(|e| format!("select {EDIT_CELL}: {e:?}"))?;
    harness.state_mut().editor = None;
    harness.run_steps(1);

    phases.push(phase("key", args.frames, &mut harness, |h| {
        h.event(egui::Event::Text("5".to_string()));
    }));
    if harness.state().editor.is_none() {
        return Err("the `key` phase typed a character and no editor opened — the frame it timed did nothing".to_string());
    }

    let before = watched_sum(&mut harness);
    phases.push(phase("commit", args.frames, &mut harness, |h| {
        // The editor is opened outside the timer: what is measured is Enter and what Enter costs.
        if h.state().editor.is_none() {
            h.event(egui::Event::Text("7".to_string()));
            h.run_steps(1);
        }
        h.event(enter());
    }));
    if harness.state().message.is_some() {
        return Err(format!("the `commit` phase raised a message: {:?}", harness.state().message.clone()));
    }
    if watched_sum(&mut harness) == before {
        return Err("the `commit` phase committed nothing — the watching SUM did not change".to_string());
    }

    phases.push(phase("undo", args.frames, &mut harness, |h| {
        if h.state().editor.is_none() {
            h.event(egui::Event::Text("3".to_string()));
            h.run_steps(1);
        }
        h.event(enter());
        h.run_steps(1); // the commit this undo will step back over, untimed
        h.event(ctrl_z());
    }));
    if harness.state().message.is_some() {
        return Err(format!("the `undo` phase raised a message: {:?}", harness.state().message.clone()));
    }

    // The drag: press once at the grid's first cell, then move with the button held. Each frame is
    // a pointer move, so the selection grows and the frame paints the new block.
    let rect = harness.state().grid.rect.unwrap_or(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(WINDOW.0, WINDOW.1)));
    let origin = rect.min + egui::vec2(48.0, 40.0);
    harness.event(egui::Event::PointerMoved(origin));
    harness.run_steps(1);
    harness.event(egui::Event::PointerButton { pos: origin, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    harness.run_steps(1);
    let debug = std::env::var_os("CELLA_FRAME_DEBUG").is_some();
    let path = |step: usize| origin + egui::vec2(30.0 * step as f32, 18.0 * step as f32);
    let mut step = 0usize;
    phases.push(phase("drag", args.frames, &mut harness, move |h| {
        step = (step + 1) % 12;
        let p = path(step);
        if debug {
            let sel = h.state().session.active().map(|d| (d.selection.active.a1(), d.selection.current().a1()));
            eprintln!("frame_time: drag to {p:?}, selection {sel:?}");
        }
        h.event(egui::Event::PointerMoved(p));
    }));
    // The release carries the position the pointer is at: a release at the press origin reads as a
    // click there, which would collapse the selection again before the check below.
    let last = path(args.frames % 12);
    harness.event(egui::Event::PointerButton { pos: last, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
    harness.run_steps(1);
    if harness.state().session.active().is_none_or(|d| d.selection.is_single_cell()) {
        return Err("the `drag` phase moved the pointer and the selection stayed one cell — the frames it timed painted nothing new".to_string());
    }

    // The view settings: every command the ledger marks `ui` is a state a frame paints, so what it
    // costs is a repaint with it on, not the command that set it.
    let configs = if args.configs { measure_configs(&doc, args.frames)? } else { Vec::new() };

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
            "configs": configs
                .iter()
                .map(|c| json!({
                    "id": c.id,
                    "what": c.what,
                    "outcome": c.outcome,
                    "idle_ms": round(c.idle_median()),
                    "page_ms": round(c.page_median()),
                }))
                .collect::<Vec<_>>(),
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
        if !configs.is_empty() {
            println!("\nview settings — {} frames each, still and scrolling\n", args.frames);
            println!("{:<34} {:>9} {:>9}  what", "command", "still", "scroll");
            for c in &configs {
                println!("{:<34} {:>9} {:>9}  {} ({})", c.id, fmt_ms(c.idle_median()), fmt_ms(c.page_median()), c.what, c.outcome);
            }
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
        "key" => BUDGET_KEY_MS,
        "drag" => BUDGET_DRAG_MS,
        "commit" => BUDGET_COMMIT_MS,
        "undo" => BUDGET_UNDO_MS,
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

/// Enter, as the keyboard sends it: the editor commits on the pressed event.
fn enter() -> egui::Event {
    egui::Event::Key { key: egui::Key::Enter, pressed: true, modifiers: egui::Modifiers::NONE, repeat: false, physical_key: None }
}

/// Ctrl+Z as the grid's shortcut table sees it. `command` is set as well as `ctrl` because the grid
/// routes Ctrl letters through `ribbon::shortcut` on `m.command`.
fn ctrl_z() -> egui::Event {
    let modifiers = egui::Modifiers { ctrl: true, command: true, ..egui::Modifiers::NONE };
    egui::Event::Key { key: egui::Key::Z, pressed: true, modifiers, repeat: false, physical_key: None }
}

/// The value of the formula the interaction phases watch (`H1 = SUM(B1:B<rows>)`).
fn watched_sum(harness: &mut egui_kittest::Harness<'static, SheetApp>) -> f64 {
    harness.state_mut().session.run("cell.get", json!({"cell": "H1"})).ok().and_then(|v| v.get("value").and_then(Value::as_f64)).unwrap_or(f64::NAN)
}

fn round(ms: f64) -> f64 {
    (ms * 1000.0).round() / 1000.0
}

fn fmt_ms(ms: f64) -> String {
    if ms >= 10.0 { format!("{ms:.2} ms") } else { format!("{ms:.3} ms") }
}

/// The workbook the phases run on: `rows` × 6 columns of values, one selection at `A1`.
/// The offscreen application: the same `SheetApp` the window runs, driven by `egui_kittest`, with
/// no GPU (see the module doc).
fn app_harness(session: Session) -> egui_kittest::Harness<'static, SheetApp> {
    egui_kittest::Harness::builder().with_size(egui::vec2(WINDOW.0, WINDOW.1)).with_pixels_per_point(1.0).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    )
}

fn workbook(rows: usize) -> Result<(Session, DocState), String> {
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
    // The document, so the view-setting table can build a session of its own per setting.
    let doc = s.doc().map_err(|e| e.to_string())?.clone();
    Ok((s, doc))
}
