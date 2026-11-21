# Performance

> **Last reviewed:** 2026-11-20 · **Last updated:** 2026-11-20 · **Change:** round 27, three knives certified on a quiet machine — a precedent two formulas read is a box (the 2M-dirty edit −18.4 %), a value-only edit is priced from a band copy tag instead of a per-row walk (−7.2 % built), and the store compares before it writes a band and finds a row with one bit probe (−20.1 % built, −21.4 % reopened). Earlier: the evaluator round, refuted by measurement — per-shape bytecode built and equivalence-proven and sign-tested a no-win (the per-cell cost is kernel-bound, not traversal-bound); the attribution then found the real pool in undo bookkeeping, and an evicted undo step now releases on later cheap commands: the built-case edit −30 % (8/8 pairs, p=0.0039), the gate's aggregate 443 ms, edit.recalc unchanged (its sequence pays the eviction earlier). Earlier: budget lock (P2) — the gate's budgets re-recorded from the quiet 2026-11-20 pass (twice the current numbers, so a green run sits at ~50% of every budget) and the gate grew two scenarios: the `mem_drift` soak and the `save.repeat` best-of-6 micro-benchmark; startup and input-to-state stay with the plan/bench harnesses (reasoning in The gate). Earlier: rounds 3–10: packed SUMIFS and SUM, formula-only graph synchronization, one rebuild on forced XLSX open, and sparse sorting validation. Earlier: isolated the three aggregations and added batched source-row reads for sort. added a four-case chained-edit benchmark and phase profiling for sort. the value read inside a recalculated chain's eval (11-18, eleventh): `CellStore::value` reads the value off the packed row instead of rebuilding the `Cell` — no formula `Arc`, no live band `Arc`, so four atomics and a rebuild a read become none — the 1M chain's **eval 161 → 118 ms**, `edit.recalc` **461 → 428 ms**, the fills unchanged. Earlier: the recalculated chain again, two hot-path costs of the pass's build (11-18, tenth): the slot table borrows the graph's shapes instead of an `Arc` a cell and keeps keys and shapes in one `Vec` (32 B a slot), and a pass asks the graph's node table once for a sheet's run of cells instead of once a cell — the 1M build **66-74 → 41-45 ms**, `edit.recalc` **693 → 639 ms**. Earlier: the memory and performance line closed (11-18): a 1M × 6 workbook is **2.90 → 0.971 GB** live (0.77 GB reopened) — row side tables 144 → 72 MB, the band's number index 66 → 33 MB, the journal 42 → 15 MB, after the audits of the calc graph's three tables and the one undo step found structure rather than slack; **measured against Excel 16.0.20527 and WPS 12.1.0.25222** on a 70 MB file (open 3.3 s against 7.9–8.2 / 10.7; recalculate 0.75 s against 0.31 / 15.3; idle 246 MB against ~35 / 288), with **OpenGL the Windows default** (183 MB lighter at the same 144 Hz); and two recalculation changes took the 1M chained edit **0.85 → 0.75 s**. Earlier: the memory round (§ The memory round of 2026-11-17): byte accounting per part, the dependency graph compacted twice, the save build parallelised, formula **shape sharing** (one interned tree per shape; the per-cell tree is gone — `Formula` 48 B, `calc::Node` 16 B), a **structural edit rewritten once per shape** (insert a row above 2M formulas 8.29 → 3.97 s, delete 8.61 → 3.59 s, rename a sheet 8.58 → 4.86 s), and an **undo that restores a paged store** (gated so it never costs the edit). A 1M × 6 workbook went **2.90 → 1.14 GB** live (reopened 0.95 GB) and no scenario is slower (save 3.7 → 2.05 s, open 5.5 → 4.02 s, fill formulas 2.8 → 1.77 s, recalc 2.3 → 1.80 s, graph rebuild 1.7 → 1.12 s). Earlier rounds: grouping/filtering no longer rebuild the graph, `open.csv` is a scenario, and the graph build is four times cheaper · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

**The one-page report is [performance-report.md](performance-report.md)** (and its Chinese
companion [performance-report.zh-CN.md](performance-report.zh-CN.md)): `cargo xtask perf-report`
regenerates both from the harnesses' own output. The external comparison against Excel 16, WPS 12.1
and upstream GridCraft v0.19.1 lives in [external-benchmark.md](external-benchmark.md).

Cella is for people who work on real data sets — programmers, analysts, e-commerce operations —
so throughput on a large workbook *is* a product feature. A number someone measured once is
folklore within a month. This page is the number, the harness that produces it, and the gate that
refuses a regression.

The reference workbook throughout: **1,000,000 rows × 6 columns, 72 MB as .xlsx** — the shape of a
real export, not a synthetic grid.

## Scenarios

`crates/engine/examples/perf.rs` runs one session through eleven named scenarios, in this order,
because that is how a real session looks (values, formulas over them, edits, sort, save, open).
Each reports one number: its own wall-clock time in ms.

| id | what it does | why it is measured |
|---|---|---|
| `fill.values` | writes rows × 6 values through `range.setValues` in 10,000-row chunks | the paste/import path: JSON decode, cell store packing |
| `fill.formulas` | fills `G = B*C+D` down every row | formula parse, dependency graph build, recalculation of 1M new formulas |
| `fill.chained` | fills a running sum `H = H(n-1)+G(n)` — every row depends on the row above | the worst case for the dependency graph: one chain a million deep |
| `aggregate` | `SUM(G1:G{n})`, `SUMIFS(B:B,C:C,">50")`, `XLOOKUP(...)` | whole-column aggregates and lookups, the queries analysts actually run |
| `edit.recalc` | edits `B1` (every `G`, hence every `H`, depends on it) | incremental recalculation after one keystroke — the number a user feels |
| `edit.undo` | undoes that edit | undo restores a snapshot, so it must not recalculate or rebuild anything |
| `sort.desc` | `data.sortDescending` over `A1:F{n}` by one column | sort is the slowest interactive operation we have (see below) |
| `save.xlsx` | `file.saveBytes` as xlsx | the write path: pack, compress, stream |
| `open.xlsx` | opens those bytes again | the read path, the dependency-graph build, and whatever recalculation the file asks for (a package with `calcId` and no `fullCalcOnLoad` asks for none, which is the normal case) |
| `rebuild.graph` | `Calc::rebuild` over every formula of the workbook, and nothing evaluated | the dependency-graph build on its own: an open and a structural edit both pay it, and it used to be the larger half of an open |
| `open.csv` | `file.exportCsv` to a temp path, then `file.open` of that file | the CSV import path — the daily action for a data or e-commerce user, so decode plus insertion is a feature of its own. It opens a workbook of its own (a CSV has no formulas), which is why it runs last |

Two further scenarios the gate measures itself, outside the harness (each is its own release
subprocess, so the engine side stays a pure library consumer):

| id | what it does | why it is measured |
|---|---|---|
| `save.repeat` | `xtask/examples/save_bench.rs` builds the workbook state `save.xlsx` measures, saves it six times in one process, keeps the best pass | the save micro-benchmark: in-process repetition holds ~1% noise, the floor at which a save change can be certified (single-pass scenarios carry the machine's ~10%) |
| `mem_drift` | `crates/engine/examples/mem_drift.rs` runs 100k rows for 240 s of edit/undo/redo cycles with a sort every 50 and a save every 200 | a leak in the commit, journal, undo or graph paths shows up as a rising total; the gate asserts flatness (±2% of the second-half median) and budgets the per-cycle cost |

Run it yourself:

```bash
# the human table, at 100k and 1M rows (the gate's sizes)
cargo run --release -p cella-engine --example perf

# one size, machine-readable, best of three passes
cargo run --release -p cella-engine --example perf -- 1000000 --json --repeat=3

# only some scenarios
cargo run --release -p cella-engine --example perf -- --scenario=sort.desc,open.xlsx
```

`--release` is not optional: a debug build is 10–30× slower and the harness says so. `--repeat=N`
runs the whole sequence N times and keeps each scenario's **fastest** pass, which is the only honest
way to read a shared machine (a benchmark measures how fast the code can go, not how busy the box
was). Numbers are per scenario, and later scenarios build on what earlier ones did — running a
subset measures those steps only, not the whole pipeline.

## The gate

```bash
cargo xtask perf              # measure, compare with the budgets, exit non-zero on a regression
cargo xtask perf --update     # record the current numbers as the new budgets
cargo xtask perf --update --repeat=3   # record them from best-of-three (preferred)
```

Budgets live in [`xtask/perf-budgets.txt`](../xtask/perf-budgets.txt), three columns — scenario,
rows, budget in ms — and every (scenario, rows) pair that the harness reports must have one, so
adding a scenario without a budget fails the gate instead of silently going unmeasured.

Why budgets and not "look at the table": nobody reads a table before merging. A budget is a
threshold that fails a release, and a recorded number that shows up in the diff when someone changes
it. `--update` records the measurement **plus 100%**, with a 1 ms floor (microsecond scenarios are
all noise). Twice, not 50%, on purpose: a pass on the dev machine while three other agents were
building reached 1.5× the quiet number, so a tighter budget would fail honest builds and get
ignored. A budget is therefore a *regression* threshold — it catches the 2× and worse changes
(an accidental full recalc, snapshot undo, a quadratic sort), and the recorded table below is what
shows smaller deltas. Re-record with `--update` only when the new number is the one you mean to
keep.

**Re-recorded 2026-11-20** from a quiet best-of-three release pass. The previous table was the
2026-11-16 one, and the 0.13.0 numbers filled only 23–52% of it — a regression had to be twice
the *old* number before anything failed. The new table is twice the current quiet numbers, so a
green run sits at ~50% of every budget: the ×2 rule applied to where the code actually is.

The gate now covers **25 checks**: the 22 harness measurements at 100k and 1M rows, plus the two
below that the harness cannot express and the gate runs as its own release subprocesses
(`--scenario=mem_drift,save.repeat` runs them alone):

- **`mem_drift`** — the long-run soak (100k rows, 240 s, ~44–48k edit/undo/redo cycles, a sort
  every 50, a save every 200). Its budget line is the soak's per-cycle cost in ms — a fixed-time
  soak has no wall-clock number of its own; a slower engine simply completes fewer cycles. The
  leak assertion lives in the gate, not the budget file: the engine's own reported total must
  stay within ±2% of its second-half median (the 2026-11-20 soaks held 0.13%; the first half is
  warm-up and is not judged). A full gate run therefore costs ~6–7 minutes: ~30 s of scenarios,
  ~40 s of save bench, a 240 s soak.
- **`save.repeat`** — the save micro-benchmark (the R12 follow-up): `xtask/examples/save_bench.rs`
  builds the workbook state `save.xlsx` measures, saves it six times in one process and keeps the
  best pass. In-process repetition is what squeezes the machine's ~10% run-to-run spread to ~1%,
  the floor at which a save change can be certified instead of argued about.

**Deliberately not in the gate: startup and input-to-state.** The two numbers the interaction
harnesses produced — startup-to-accept **~1.02 s** and synthetic-input-to-state **~40 ms** channel
round trip / **~355 ms** heavy Enter — stay measured by `plan/bench/startup_r14.ps1` and
`plan/bench/input_r16.ps1`. Putting them inside `cargo xtask perf` would make a Rust-only,
cross-platform gate own GUI process orchestration: a hidden Win32 desktop (`CreateDesktop`, so
nothing appears on the user's screen), the full GUI app built in release, a TCP control-channel
client with frame-batched polling, and a Windows-only step in a gate that runs everywhere the app
ships. That is harness machinery, not a budget — the numbers stay recorded (§ Interaction and
long-run measurements) and are re-measured by the scripts whenever startup or input latency is
touched. Nothing is half-wired into the gate.

A scenario that comes out over budget is measured **once more** before the run fails: a build that
started on the same machine is a slow pass, not a regression, and a real regression stays over
budget twice. Missing budgets are never retried — that is a mistake in the file, not a timing
question.

```text
scenario                 rows    measured     budget      of
------------------  ---------  ----------  ---------  ------
fill.values            100000      131 ms     250 ms     52%
save.repeat           1000000      1.60 s     3.20 s     50%
edit.recalc           1000000      703 ms     1.45 s     48%
```

The `of` column is the share of the budget the run used; `142%!` marks an overrun.

**It is deliberately not part of `cargo xtask ci`.** `ci` runs on every commit and needs no
release build; this gate builds the engine in release and takes minutes at 1M rows, so it runs:

- **before a release** (see [releasing.md](releasing.md)),
- **after any change to a hot path** — the cell store, recalculation, fill, sort, the xlsx reader
  and writer, `range.setValues` — where "it feels faster" is not a review comment.

Slow or shared machine: `CELLA_PERF_SCALE=2 cargo xtask perf` (or `--scale=2`) multiplies
every budget. Use it to make the gate fit the hardware it runs on, never to excuse a regression —
re-record with `--update` only when the new number is the one you intend to keep.

## Measured on the dev machine

2026-11-16, release build, 1,000,000 rows × 6 columns, from the ad-hoc example the harness replaced
(same command sequence, same machine class):

| scenario | 1M rows |
|---|---|
| fill 1M values | 4.48 s |
| fill 1M formulas | 5.49 s |
| 1M chained formulas | 6.21 s |
| aggregates (SUM, SUMIFS, XLOOKUP) | 1.15 s |
| edit one cell → recalc 1M cells | 2.47 s |
| sort 1M rows | 11.40 s |
| save 72 MB xlsx | 4.22 s |
| open 72 MB + full recalc | 15.29 s |
| undo | 151 µs |

The same scenarios through the harness, best of three passes, on the same machine while other work
was building in parallel (so these are pessimistic — they are what the checked-in budgets were
recorded from, and the reason a budget is a threshold and not a target):

| scenario | 100k rows | 1M rows |
|---|---|---|
| `fill.values` | 351 ms | 4.02 s |
| `fill.formulas` | 430 ms | 4.37 s |
| `fill.chained` | 484 ms | 5.13 s |
| `aggregate` | 88 ms | 914 ms |
| `edit.recalc` | 205 ms | 2.29 s |
| `edit.undo` | 2 ms | 32 ms |
| `sort.desc` | 715 ms | 9.48 s |
| `save.xlsx` | 341 ms | 4.00 s |
| `open.xlsx` | 684 ms | 8.60 s |
| `rebuild.graph` | 292 ms | 1.50 s |
| `open.csv` | 153 ms | 1.11 s |

Reading it:

- **Sort and open+recalc are the two slow scenarios** and the two that matter most to a user with a
  big sheet. Everything else is already in the "do it again without thinking" range.
- **Undo is the cheapest row on the page, and that is the point.** A history entry is a snapshot of
  the workbook, values included, so an undo restores values rather than recomputing them: it
  evaluates nothing (`last_recalc_cells` is 0 afterwards). It used to *rebuild* the dependency graph
  from the restored workbook — a million rows of formulas re-parsed to change one cell — which cost
  4.03 s at 1M rows; it now re-inserts only the formulas the undone edit rewrote (`Calc::restore`,
  `CellStore::formula_changes_against`), and what is left of the undo is the history entry's byte
  accounting instead. If this row goes back up, the graph is being rebuilt again.
- `edit.undo` is measured right after `edit.recalc` rather than at the end of a run, because the
  redo stack and the byte budget are then in the state a user's edit leaves them in.
- The 100k column is there because the two sizes can move in different directions: a change that
  helps at 1M by making an allocation cheaper can still cost at 100k, where the whole workbook fits
  in cache.
- **CSV import is the cheap open now.** `open.csv` reads 2.8 MB in 153 ms and 30 MB in 1.11 s (1M rows × 6 cells), against 1.97 s for the same file before the reader scanned bytes into row batches. It was measured on its own, after the byte scanner landed, so it is not from the parallel-build session the rest of that table was recorded on.
- **An A/B that edits a constant leaves the incremental cache poisoned on Windows.** Two
  rebuilds of one crate (edit, measure, revert, rebuild) can leave objects that reference symbols
  from the other incremental session, and mingw's `ld` then fails the *test* link with `undefined
  reference to anon.<hash>.<n>.llvm.<hash>` — no code change involved. `CARGO_INCREMENTAL=0` links
  fine and `cargo clean -p <crate>` fixes it; do that before re-running `cargo xtask ci` after a
  constant-flip A/B.
- The two tables are separate sources. Best-of-three reports the pass the machine was quietest for,
  which is why `open.xlsx` (the longest scenario, so the one a loaded pass distorts most) comes out
  at 8.6 s here against the 15.3 s of the single-pass ad-hoc example. The budgets are keyed to the
  harness, so compare like with like: `cargo xtask perf` against `xtask/perf-budgets.txt`.

Known hotspots and what is left of them are tracked in [`gaps.md` §2](gaps.md) — this page owns
the measurement, that entry owns the work.

## The performance round of 2026-11-16, measured on a quiet machine

Five batches took the numbers above and moved them. **Same workbook, same command sequence, release,
best of three passes on an otherwise idle machine** (`cargo run --release -p cella-engine
--example perf -- 1000000 --repeat=3`):

| scenario (1M rows × 6 columns, 72 MB .xlsx) | before | after | change |
|---|---|---|---|
| open 72 MB + rebuild + whatever the file asks to recalculate | 15.29 s | **5.4–5.6 s** | −64% |
| sort 1M rows | 11.40 s | **7.9 s** | −31% |
| fill 1M values | 4.48 s | **2.2 s** | −50% |
| fill 1M formulas | 5.49 s | **2.8 s** | −49% |
| 1M chained formulas | 6.21 s | **4.0 s** | −36% |
| save 72 MB | 4.22 s | **3.7 s** | −13% |
| edit one cell → recalc 1M cells | 2.47 s | **2.3 s** | −6% |
| undo of that edit | 4.03 s | **30 ms** | −99% |
| aggregates | 1.15 s | **1.1 s** | −5% |
| dependency-graph rebuild, 2M formulas | 3.87 s | **1.7 s** | −56% |
| **open a 57 MB CSV (1M rows)** | 1.97 s | **0.65 s** | −67% |

The 100k-row numbers the gate uses, all at 1/4 to 1/5 of the budget: fill values 213 ms, formulas
309 ms, chained 396 ms, aggregate 108 ms, edit+recalc 225 ms, undo 3 ms, sort 625 ms, save 356 ms,
open 473 ms, rebuild 147 ms, open.csv 66 ms.


What each batch did, in one line each:

- **Opening and saving** — the reader parsed and printed every formula twice and the writer re-printed
  the tree it already had, then built fresh `String`s through `format!`; both paths now build once and
  append (`crates/xlsx/src/{fmla,xml}.rs`, `write.rs`, `sheet_read.rs`, `sheet_write.rs`).
- **The dependency graph** — three one-element `HashSet`s and a `Vec<Area>` per formula were ~250 MB of
  churn per million rows; dependencies are now an inline `One/Two/Many` enum, precedent areas live in
  one arena, flags in one `u8`, and a formula tree is walked with a reusable buffer
  (`crates/calc/src/{graph.rs,walk.rs}`).
- **Undo and redo** — a history entry is already a workbook snapshot, so undo never recalculated; it did
  rebuild the precedent graph of **every** formula to fix the few the edit touched. `Calc::restore`
  now re-inserts only the formulas the two versions disagree on, found for free through the store's
  shared bands (`crates/model/src/store.rs`, `crates/calc/src/recalc.rs`).
- **Filling** — `range.setValues` writes a row at a time through the store's whole-row path and only
  builds an input string where one is needed; two recalc guards skip work a value-only bulk edit was
  paying for; `edit.autoFill` shifts formulas in one walk instead of cloning the tree
  (`crates/engine/src/cmd/edit.rs`, `crates/calc/src/recalc.rs`, `crates/formula/src/adjust.rs`).
- **Sorting** — one flat key buffer, custom-list position resolved once per cell, borrowed comparisons,
  whole-row write-back, and a sort no longer treated as a structural edit
  (`crates/engine/src/cmd/data.rs`).
- **Presentational edits** — grouping, filtering and hiding or showing rows were marked structural: each
  rebuilt the graph and evaluated every formula (3.2–3.5 s on the reference workbook). They change no
  cell's content, so they now go through `Calc::visibility_changed`, which evaluates only the formulas
  whose value reads which rows are hidden — `SUBTOTAL(101–111)` and `AGGREGATE`, recorded as the graph's
  `VISIBILITY` flag while the graph is built — and their dependents
  (`crates/engine/src/cmd/mod.rs`, `crates/calc/src/recalc.rs`, `crates/calc/src/recalc/graph.rs`).
  Measured on the reference workbook: `data.filter` (off), `data.filterBy`, `data.clearFilter`,
  `data.group` and `data.ungroup` went from 2.9–3.5 s each (2,000,000 formulas evaluated) to
  0.6 ms–0.5 s with 0–2 formulas evaluated; the 0.4 s left is one `SUBTOTAL(109)` over a million
  filtered rows, against ~0.3–0.5 s for a single `SUM` over the same range.

The budgets in `xtask/perf-budgets.txt` are recorded at roughly twice these numbers (the file's
convention) so a real regression fails the gate and ordinary machine noise does not.

## The memory round of 2026-11-17

One day, three waves, all measured on this machine in release and best of three on an idle machine:
**byte accounting for a live session** (so "where does the memory go" stops being folklore), **the
save build parallelised and the dependency graph compacted**, and **formula shape sharing** — one
interned tree per formula *shape* instead of one per cell. A second wave finished the design (a
structural edit rewritten once per shape; the old per-cell tree deleted) and added the graph's last
table and a paged undo. The result: a 1,000,000-row × 6-column workbook holds **1.14 GB instead of
2.90 GB**, reopens at 0.95 GB instead of 2.18 GB, and **every scenario is faster**:

| scenario (1M rows × 6 columns) | before the round | after | |
|---|---:|---:|---|
| save 72 MB | 3.7 s | **2.05 s** | −45 % |
| open 72 MB | 5.4–5.6 s | **4.02 s** | −26 % |
| fill 1M formulas (`G = B*C+D`) | 2.8 s | **1.77 s** | −37 % |
| fill 1M chained (`H = H(n-1)+G(n)`) | 4.0 s | **2.96 s** | −26 % |
| edit one cell → recalc 1M | 2.3 s | **1.80 s** | −22 % |
| dependency-graph rebuild (2M formulas) | 1.7 s | **1.12 s** | −34 % |
| sort 1M rows | 7.9 s | **6.78 s** | −14 % |
| fill 1M values | 2.2 s | **2.04 s** | −7 % |
| aggregates | 1.1 s | 954 ms | −13 % |
| undo of the edit | 30 ms | **27 ms** | — |
| open 57 MB CSV | 0.65 s | 609 ms | — |

And the edits the second wave rewrote at shape level, on the same reference workbook (2M formulas):

| one command | before | after | |
|---|---:|---:|---|
| `home.insertRows {"1:1"}` | 8.29 s | **3.97 s** | 2.1× |
| `home.deleteRows {"500000:500000"}` | 8.61 s | **3.59 s** | 2.4× |
| `sheet.rename` over a 1M-formula column | 8.58 s | **4.86 s** | 1.8× |
| `data.sortDescending` (2M formulas permuted) | 9.92 s | 10.2 s | unchanged (already shape-level) |

At 100k rows the same direction and proportion: save 356 → 184 ms, open 473 → 334 ms, edit+recalc
225 → 172 ms, sort 625 → 513 ms, rebuild 147 → 83 ms, fill formulas 309 → 189 ms.

The last two steps (boxing a formula's legacy array range, then deleting its per-cell tree) took
the memory table below from 1.27 to **1.17 GB** and were within budget on every scenario in their
own gate run (`open.xlsx` 3.68 s, `save.xlsx` 1.85 s, `rebuild.graph` 1.16 s in that pass). A
gate run minutes later on the same box read 4.58 / 2.20 / 1.33 — this machine's ±10 % spread is
exactly why `xtask/perf-budgets.txt` records twice the measurement and why the table above quotes
the quietest run of each scenario rather than any single pass.

### Where the live heap goes

`cargo run --release -p cella-engine --example memory -- 1000000` builds the reference workbook
through the same three fills and prints one line per allocation class. It is arithmetic on lengths,
capacities and layouts — not a heap census — and `total == Σ parts` is a test in each crate.

| part (1M × 6, 2M formulas) | before the round | now | share |
|---|---:|---:|---:|
| **cells** | **1,267.7 MB** | **370.6 MB** | 37.2 % |
| · formula trees (`Arc<Expr>`, 2M) | 768.0 MB | **12.0 MB** | 1.0 % |
| · formula text + `Formula` (48 B) | 165.4 MB | **133.4 MB** | 11.4 % |
| · row side tables (`RowExtras`) | 144.0 MB | **72.0 MB** | 6.8 % |
| · number tables / text dictionaries | 98.0 MB | **65.0 MB** | 6.5 % |
| · band headers, payloads, runs, bitmaps | 92.3 MB | 92.3 MB | 7.9 % |
| **calc** | **851.4 MB** | **372.4 MB** | 32.8 % |
| · graph nodes (2M, 16 B) | 172.0 MB | **104.9 MB** | 9.2 % |
| · dependents map | 478.2 MB | 56.0 MB | 6.3 % |
| · precedent-area arena | 201.3 MB | 125.0 MB | 11.0 % |
| **history / undo** | **737.6 MB** | **212.3 MB** | 21.3 % |
| journal (recent commands) | 42.1 MB | **15.3 MB** | 1.6 % |
| **total** | **2.90 GB** | **884 MB** | |

Reopened from the 70 MB xlsx: **0.68 GB** (683.3 MB: the reader's bands were already finished, so the number table round above moved the as-built figure only) (was 2.18 GB; 868.8 MB before the row side tables were split). The workbook as built holds **3 shapes for
2,000,000 formulas**, and `edit.autoFill` over a million rows now costs **zero parses** (the whole
reference workbook costs three: the typed source formulas). After a save and reopen it holds 181 —
the writer still writes one plain `<f>` per cell by design (§ `plan/SHAPE-SHARING.md` 5.4), so a
reopened file parses once per read chunk.

Two numbers are worth reading carefully. **Undo's 212.3 MB is a charge, not wholly heap**: about
480 MB of the original 737.6 MB was formula trees shared with the live bands and the graph, so the
snapshot's own heap was ≈257 MB — the shape work cut the charge because the shared tree is now one
`Arc<Shape>` charged once, and the formula struct's shrink took another 24 MB. And **formula text
is 133.4 MB against 165.4 MB at the start** even though it now holds the same 2M spellings: the
cell's `Formula` is 48 B (the shape, not the tree, and a boxed legacy-array range), where it was
64 B with a 56-byte `Arc<Expr>` per cell.

What each wave did, in a paragraph each:

- **The band's number index** — a band keeps two tables for its numbers: the values (8 bytes a
  distinct number) and an open-addressing index of 2-byte slots that only *adding* a cell needs.
  `Band::finish` already dropped the index and shrank the vectors, and the file reader called it
  when a batch was complete — but a band built by a fill or a paste never did, so at a million rows
  each of 15,625 bands kept a 1,024-byte index and a quarter of its number table as spare capacity:
  **66.0 MB of number tables, 33.0 MB of it waste**. A store now remembers the band it is adding
  to and finishes the previous one on the move (`band_for_write`), and every bulk write finishes
  the run it just wrote: **66.0 → 33.0 MB** and live heap **1.06 → 0.998 GB**. The reopened figure
  does not move (796.8 MB) because the reader's batches already ended with a finished band. A/B on
  the same binary: fill.formulas 1.44/1.45 s, fill.chained 2.35/2.42 s, edit.recalc 908/925 ms,
  rebuild.graph 968/957 ms — inside this machine's spread, so the round records no speed change.

- **The row side table** — every row with a formula carried one heap allocation: a `RowExtras` of
  three vectors (72 B) plus the formula vector's spare capacity. `Vec` grows by doubling, and for
  its first push it takes four 16-byte slots, so a row of two formulas held four — 144 B a row, or
  **144 MB at a million rows**. The rare parts (an array value, the wide payload table) moved out of
  line into a `RareRow` behind one boxed pointer, and the formula vector is now sized exactly: a
  `shrink_to_fit` after an edit and one in `put_row` (the file reader's path, which kept the
  doubling's spare slots for every row of a reopened workbook). **144.0 → 72.0 MB both as built and
  reopened** (1.14 → **1.06 GB**, reopened 868.8 → **796.8 MB**), and the four hot scenarios are
  faster for it: `fill.formulas` 1.77 → 1.38 s, `fill.chained` 2.96 → 2.27 s, `edit.recalc`
  1.80 → 0.87 s, `rebuild.graph` 1.12 → 0.89 s (best of three on a quiet machine; the recorded
  budgets are unchanged and still bound twice these).

- **Accounting** — `StoreMemory` (bands, stripes, value dictionaries, texts, row extras, formula
  text, trees, arrays, page cache), `GraphMemory`/`CalcMemory` (nodes, areas, the dependents map and
  its long lists, spills, cycles), `Session::memory_report` adding the workbook and the history
  stacks, and `examples/memory.rs` (`crates/model/src/store.rs`, `crates/calc/src/recalc/graph.rs`,
  `crates/engine/src/memory.rs`). It also fixed `Band::heap_bytes`, which under-counted a band
  (two text-dictionary vectors, overflow vectors, `len` instead of `capacity`), so the decoded-band
  page cache now charges exactly what the report charges.
- **The graph** — the precedent index went from one 5M-entry `Key → Deps` map (95.6 B an entry, every
  entry a single dependent) to one table per sheet keyed by `CellRef`, a bare pair for the
  one-to-one case and a `Vec` from the second on, plus a separate cross-sheet table: 478.2 →
  142.6 MB, and `fill.chained` 5.24–5.55 → 4.35–4.63 s in a stashed same-binary A/B. Then nodes
  172.0 → 138.4 MB (the flag byte moved into the run's length word, 41 → 33 B a slot, an over-long
  run made unconstructable) and the area arena 201.3 → 125.0 MB (a packed 20 B internal area, and a
  `settle` at the graph's quiescent points that trims a fill's doubling tail to `len + len/4` with a
  "only if it frees an eighth" guard). `edit.recalc` 2.31 → 1.68 s came from the bytes the pass no
  longer walks.
- **The save build** — the shared string table is filled by a pre-pass over row windows in the file's
  first-use order, and `<sheetData>` is built in parallel row windows that concatenate to the byte
  the serial walk produced; per-sheet part numbering stays serial and in sheet order
  (`crates/xlsx/src/sheet_write.rs`, `write.rs`). The XML build fell from **91 % to 46–49 % of a
  save** (3.7 → 2.45 s on its own), package bytes identical, and a test writes the same workbook with
  1/2/8 windows and compares the whole package.
- **Shapes** — a formula's *shape* is its tree with every relative reference stored as an offset from
  the formula's own cell, interned once per workbook (keyed by the R1C1 print, `Weak` entries) while
  the cell keeps its exact A1 text. `Shape`/`ShapeTable` in `crates/formula`, `Formula.shape` beside
  the old tree, and then the consumers: the evaluator carries the cell it is evaluating for
  (`Reference::range_at`), the dependency walk and `may_return_array` take a base, the `xlsx` reader
  interns per read chunk, and fill, copy, paste and sort reuse the `Arc` and print the cell's text
  instead of cloning a tree. `crates/calc`, `crates/model`, `crates/xlsx`, `crates/engine`. The
  master invariant — a cell's `text` must equal `print(expr_at(at))`, asserted by the command-sequence
  fuzz — caught a real bug immediately (a shape left anchored at its original cell after a row insert).

The second wave (shape-level `adjust`, the graph's last table, the paged undo) also came with one
refused trade: making the *newest* history step spill cut 110 MB but cost **+35 % on `fill.chained`
and +40 % on `edit.recalc`** inside the edit, and then one decode pass on the next save or sort after
an undo (1.89 → 2.60 s, 7.01 → 8.10 s). It is now gated by the same cost ceiling as any other step,
so at 1M the step stays live and nothing is paid; the capability is exercised by a forced-spill test.

Three regressions were found and fixed before this was recorded, and they are the reason the numbers
above are all wins rather than a memory-for-speed trade: the first cut resolved a tree per formula
per evaluation (+22 % on `edit.recalc`), and a save called `needs_array` on a *resolved* tree for
every formula (+24 % on `save.xlsx`); both now read the shape's canonical tree, since the question is
about sizes, which a shape does not change. The legacy `@` analysis got the same treatment for the
read path (`open.xlsx` 5.01 → 4.05 s in the same-binary A/B), and it now skips a formula whose every
part is certainly one value without building anything.

### The idle window, audited 2026-11-18

An empty Cella window holds **307 MB** on DirectX 12, **230 MB** on Vulkan and **238 MB** on OpenGL
(`WGPU_BACKEND=vulkan`, `=gl`). The engine is not it: `cella-cli mcp`, the same build
without a window, is **8.1 MB**, and a **bare eframe window with one label** - built and measured
beside it - holds 236-256 MB on DirectX 12 and 91-109 MB on OpenGL. So the windowing stack is the
largest part and the backend is the largest lever inside it.

The same file in all three backends, on a **visible** and on a **hidden desktop** (a desktop created
by the harness, so a measurement never puts a window in front of the user). Two runs each, the edit
is B3 = 5:

| backend | idle (hidden) | after opening 70 MB (hidden) | `ui.screenshot` (hidden) | `ui.screenshot` (visible) |
|---|---:|---:|---:|---:|
| DirectX 12 | 317 MB | 1,200 MB | 135 ms | 140 ms |
| Vulkan | **240 MB** | 1,123 MB | 137 ms | **hangs** in this build |
| OpenGL | **173 MB** | **1,056 MB** | **315 ms** | 135 ms |
| recalculate 2M formulas | | | 0.84-0.90 s in all three | |

Memory is solid - OpenGL and Vulkan save 70-145 MB over DirectX 12 in both environments - and the
engine cost is identical. Latency could not be read from a screenshot, so the app now measures
itself: `CELLA_FRAME_STATS=1` repaints continuously and prints one line every sixty frames with
the time our own `ui` spends per frame and the interval between frames. With the 70 MB workbook
open, on the visible desktop, at 144 Hz:

| backend | paint per frame | paint max | frame interval | working set, document open |
|---|---:|---:|---:|---:|
| DirectX 12 | 1.41 ms | 1.95 ms | 7.0 ms | 1,233 MB |
| OpenGL | **1.38 ms** | 1.67 ms | 6.9 ms | **1,050 MB** |

Both hold the display's refresh rate with the same paint cost, so **OpenGL is not slower here and
costs 183 MB less with this document open** - and it is the **default** as of 2026-11-18
(`graphics::choose_backends`): a machine whose DirectX 12 offers only WARP keeps both backends so
wgpu still finds one, and any failure to create the GL device is retried with DirectX 12. A run
with no override confirms it: the log reads `backends Backends(GL)`, idle 246 MB, 1,051 MB with
the document open, paint 1.41 ms and a 7.0 ms interval (144 Hz). The risk this takes on is other
machines' OpenGL drivers, which is exactly what the retry path covers. Vulkan stays off (its
readback hung on the visible desktop, on top of the known Intel `igvk64.dll` crash, #120). Wayland is not a
render backend at all - it is the Linux window protocol (the app ships `wayland` and `x11`),
so on Linux the choice is Vulkan or OpenGL under either protocol, which this machine cannot measure.


Inside our own ~130 MB over the bare window: **fonts 66.7 MB** (19 faces; the six script fallbacks
are 53 MB - Chinese 19.7, Japanese 13.8, Hangul 13.5, Devanagari 5.3, Hebrew 0.9, Thai 0.4 - and
the Latin set is 12.8) and ~60 MB of UI state. The glyph atlas is 1.0 MB (`font_image_size`:
8192 x 32), so the font bytes are the files. The set is locked by a test now
(`the_font_set_stays_inside_its_budget`: 70 MB total, 15 MB Latin).

### Compared with Excel and WPS, 2026-11-18

Every number above is ours. The same workload was put in front of Excel 16.0.20527 (Microsoft 365)
and WPS 12.1.0.25222 through COM: `examples/bench` writes the file (1M x 8, 2M formulas,
70.4 MB), all three open it, edit B1 and recalculate the sheet, and the three agree cell for cell.

| | Excel 16.0.20527 | WPS 12.1.0.25222 | Cella 0.12 |
|---|---:|---:|---:|
| open the 70 MB workbook | 7.9-8.2 s | 10.7 s | **3.3 s** |
| recalculate 2M formulas after the edit | **0.31 s** | 15.3 s | 0.90 s |
| working set after opening | **588 MB** | 987 MB | 1,054 MB |
| private working set | **468 MB** | 864 MB | 1,330 MB |
| with no document open | **~35 MB** | 288 MB | 246 MB |

Against the baselines the document costs 553 MB (Excel), 700 MB (WPS) and 808 MB (Cella) - the
engines are within 1.5x of each other, and our 884 MB live figure (undo 212, journal 15) is in the
right place. Opening is ours by 2.4-3.2x; recalculation is Excel's by 2.9x (and ours by 17x over
WPS); memory is nobody's friend at the top end, and the reason we are last is the **382 MB the
window holds with nothing open** - the same build headless (`cella-cli mcp`) is 8.1 MB, so that
is UI startup allocation and it is the next memory target. Detail and caveats in
[target-app-parity.md](target-app-parity.md#measured-against-excel-and-wps-2026-11-18).

Memory alone is not a score. The same file pair gives **bytes per populated cell** (Excel 69,
WPS 88, Cella 104) and **one edit that recomputes 999,999 chained formulas** (Excel 0.07 s,
WPS 14.8 s, Cella 0.853 s), i.e. 51.7k / 0.19k / 2.7k formulas a second per MB of document
memory: our extra 277 MB is not buying speed, and what it does buy is the undo of that same
edit, the exact formula text and the replayable journal. The 382 MB idle window buys nothing
(the headless build is 8.1 MB with the same latency) - that is the number to treat as waste.


### The undo step, audited 2026-11-18

`examples/memory` prints the undo stack now: how many steps it holds and what each retains
beyond the live workbook. After the reference workload's fills, 1M × 6 holds **one step,
212.3 MB** — and that step is the sheet as it was before the last fill: stripes (bitmaps, payloads,
kind and style runs, band headers) 85.8 MB, the rows' side tables 56.0 MB, the text table
32.0 MB (one `Arc<str>` per distinct text, shared with the live store's own table) and the
rest line metadata.

That is what an undo of a fill has to restore, and none of it is slack: a value delta (old payload,
kind and style per changed cell) for a fill that rewrites the sheet is the same bytes as the stripes
it would replace, and the step is already kept as one — at 1M the byte budget holds exactly this one
and drops the next. Paging it was measured in the second wave and refused: the step's 10,400
bands cost about a second to encode and priced +35 % into `fill.chained` and +40 % into
`edit.recalc`.

### The graph's three structures, audited 2026-11-18

The calc part is 285.9 MB at 1M × 6: nodes 104.9, dependents 56.0, the area arena 125.0. Audited
against the structures themselves, not guessed at:

| part | size | what it is |
|---|---:|---|
| graph nodes | 104.9 MB | 2,000,000 nodes in a per-sheet `HashMap<CellRef, Node>`. A node is 16 B and an entry is its 8-byte key, the node and hashbrown's control byte — but hashbrown rounds its bucket count up to a power of two, so 2M entries take **4,194,304 buckets**: the table is ~2× the entries, 25 B a slot at a 0.87 load |
| dependents | 142.6 MB | ~7.3M slots for the ~2M precedent cells of 2M formulas (each names two or three), the same power-of-two rounding on `(CellRef, CellRef)` tables |
| area arena | 125.0 MB | 6.25M slots of 20 B for 5M live areas — already packed (20 B, where the evaluator's own `Area` is 24) with a small tail |

None of it is capacity slack. `shrink_to_fit` after a build was written, measured and **reverted**:
the bucket count a 2M-entry table picks is already the smallest hashbrown can, and the same is true of
the dependents table and the arena. The only way down is a different structure — a sorted
`Vec<(CellRef, Node)>` per sheet (24 B an entry, ~48 MB) or a CSR dependents table (an offset array
plus a flat list, ~12 B a reference, ~24 MB) — and both pay for it in the edit: a single-cell edit
into a sorted vector is an O(n) insert or a delta layer to reconcile at `settle`, and the recalc
hot path is what would carry the risk. At two million formulas the graph is not where the product stops
working; recorded here so the next reader does not re-run the audit.

### Recalculation throughput, 2026-11-18

The edit the comparison above measures - one cell into a chain of 999,999 formulas - is the number
a user feels after a keystroke, and it was the last engine cost with a clear shape: **0.85 / 0.83 s
before, 0.75 / 0.77 s after** (-9-10 %, two runs each side of a same-binary A/B on an idle machine).
Two changes, each proven by the tests beside it:

- **The scalar fast path is shared.** The closure's fast path and its generic path were two
  implementations of the same arithmetic, and a chained fill's inline path kept an eight-deep chain
  in a stack array instead of walking the recursion through a per-formula frame (`INLINE_CHAIN = 8`,
  `crates/calc/src/eval/eval.rs`); the array path now calls the same `arith` as the scalar one, so
  equivalence is by construction (the array tests keep it honest).
- **The closure pass probes each cell once.** The empty-precedent-table checks moved out of the
  per-cell loop (one test per pass, `Walker` in `crates/calc/src/recalc/graph.rs`), `SeenCells`
  bitmaps are sized once (`DirtyBox::grow`), a `Vec` with a head index replaced a reversed stack,
  `calculate_at` no longer takes a per-formula frame, and `legacy_of` short-circuits on the legacy
  count. A bitmap-doubling regression the change itself introduced was caught by the fuzz and fixed.

Profile after (same file, `CELLA_PROFILE=1`): closure **164 ms**, eval **408 ms**, write **63 ms** of a
**574 ms** pass, so the evaluation of two million expressions is ~85 % of it. What is left of the
plan is priced and deferred: a value-slot/write-back merge (est. -60-100 ms) and a dense slot index
(-30-40 ms), both edits to the hot path, and **bytecode linearisation** - the one change that would
move the gap to Excel - which is a rewrite of `eval.rs` (810 lines) and `eval/*.rs` (1.9k) and was
deliberately not attempted in this round.


**Two more steps the same day, measured in the gate's own harness** (`edit.recalc` at 1M, best of
three, two runs each side of a same-binary A/B):

- **Dense slot index** (`54a0cacd`): a pass's cells are looked up by arithmetic in a per-sheet box
  table instead of a hash probe per cell, with the hash table kept when the cells do not fill their
  box (three cells far apart would zero-fill megabytes). **893 / 893 ms -> 665 / 692 ms**, which is
  the ~200 ms four million hash probes were costing: the audit that called this path memory-latency
  bound was right, and its own estimate (-30-40 ms) was ten times too low.
- **Chain runs** (`983c8f04`): a column of running totals (`H(n) = H(n-1) + G(n)`, one fill's
  shape) is accumulated in one loop instead of one evaluation a row, reading its operands through
  the evaluator's own read and its arithmetic through the kernel `Expr::Binary` itself uses (so
  blanks, coercion, errors, `#DIV/0!`, `#NUM!` and precision-as-displayed cannot disagree). 999,999
  of the edit's 1,000,001 formulas take it; eval **218.3 -> 178.8 ms**, the command **712 -> 646 ms**
  (best of three). Iterative calculation and any shape with a `$`, a name, a function or a non-adjacent
  reference stay on the general path.

Together: **893 -> 646 ms (-28 %)** on this harness. The lesson beside the number: the chain run
measured 265.8 -> 87.1 ms in a development build — three times what release gave — because a
development build does not inline the general evaluator's own fast path. **A shortcut that removes
work the release compiler already removes only shows its real size in release**. This one was
reverted when a first release pass showed no gain, then re-landed on two further passes each side and
a new cycle test (a circular column reports the same values through both paths).

**Then two more, and the command lands at 460 ms** (same harness, `edit.recalc` at 1M, two runs each
side of a same-binary A/B unless noted):

- **The slot-table build** (`cde71870`): a slot's shape is **borrowed from the graph's node** instead
  of `Arc::clone`d (a million atomic increments and as many decrements a pass, for shapes the graph
  owns) and the two parallel `Vec`s became one `Vec<Entry>` of 32 B; the per-cell `graph.node` probe
  became one sheet-table probe a sheet run (`Graph::nodes_of`). Build **66-74 -> 40-47 ms**,
  `edit.recalc` **641/652 -> 589/596 ms**. The closure does not move (it is not its structure).
- **The single-precedent dependents box** (`5eb57d38`): 84 % of the closure was one hash probe a
  cell on a cold 34 MB table. A per-sheet box table (4-8 B a cell, hash fallback when the cells do
  not fill it) answers in arithmetic: closure **190 -> 27 ms**, the command **589/596 -> 465/460 ms**.
  The first attempt at it was reverted: the box grew by copying every entry per new precedent, and
  `fill.formulas` at 100k went 164 ms -> 37.96 s. The kept version copies **once a doubling** of an
  explicit capacity and absorbs refused cells in one batch a command; the fill it is accepted on is
  **100k 154 ms** (164 before) and **1M 1.27/1.28 s**.

- **A value read borrows its band** (`b636f657`): `Sheet::value` rebuilt a `Cell` to return one
  value — an `Arc<Band>` clone and an `Arc<Formula>` clone, then both dropped, four atomics a read,
  a million of them for the chain's second operand. A value-only path (`CellStore::value` /
  `value_with_formula`, borrowing a live band's packed row; a paged band still decodes) took eval
  **179 -> 116-118 ms** and the command **465/460 -> 421/406 ms**. `fill.values`, `fill.formulas`
  and `fill.chained` are unchanged; `Sheet::value` is also what the grid paints through.

- **The write-back moves its values** (`f006f993`): the pass collected its results into a fresh
  `Vec<(Key, Value)>`, then cloned every value again into a per-sheet batch — two temporaries (48 MB
  at 1M) and a million `Value` clones. The store is now fed from a batch the pass owns and reuses
  across passes, with each value moved out of its slot (`mem::take`): write **70 -> 45-50 ms**, the
  command **421/406 -> 407/390 ms**. The store's order check stays: measured at 0.55-0.68 ms for a
  million values, about 1 % of the write, and it is what catches a wrong-row batch.

**From 893 to 390-407 ms, -56 %**, on the shape of edit this page has measured all day: a cell at the top
of a million-row chain. The phase split after it is closure 27, eval 117 (with the chain run), write
45-50 ms. What is left is the evaluator's own 117 ms.

**Then the read inside the eval** (`value-only-read`): every row of the chain reads its second
operand through `Sheet::value`, which went through `CellStore::get` — a band `Arc` clone, the cell
rebuilt in full (a formula `Arc` clone with it), and both dropped: **four atomics and a `Cell` a
read, for a value**. `CellStore::value` / `value_ref` / `value_with_formula` now read the value off
the packed row: a live band is borrowed where it is, only a paged band is decoded, and no formula
`Arc` is cloned. `Sheet::value` reads through it and keeps its rule (an empty value with a formula
is the cell's own answer, otherwise the spill shows through). Five paired release runs a side, same
harness and command, medians: the chain's **eval 161 -> 118 ms** (-27 %), `edit.recalc`
**461 -> 428 ms** (410-490 either way a pass, so the read is what moved and the command follows it
partly), and the fills do not move — `fill.values` 1.94/1.94 s, `fill.formulas` 1.23/1.25,
`fill.chained` 1.79/1.79 (best of three passes a side).
The comparison table above predates this change: its Cella column is the pre-round build through the
control channel (0.90 s). The gate's `edit.recalc` on the same file went 0.87 -> 0.75 s, so the row is
now 2.4x behind Excel (0.31 s) rather than 2.9x, and the effective-memory table's 0.853 s / 2.3M
formulas a second / 2.7k per MB become 0.76 s / 2.6M / 3.2k. The Pareto conclusion does not move:
Excel recomputes the same chain in 0.07 s, and a tenth of a second is not what closes that.
What is left, in the order the accounting ranks it:

- **The calc graph, 372.4 MB** — audited 2026-11-18 (above): structure, not slack. A CSR table is
  the only route down and it trades the edit path.
- **The undo stack, 241.6 MB charged** (the snapshot's own heap is a fraction of that). At 1M rows the
  budget keeps exactly one entry — the newest — and compressing that one turns a 23 ms undo into
  4.99 s (measured), which the 54 ms gate forbids. A step the budget would *drop* is now kept as
  compressed pages when its encode projects under 60 ms (`CellStore::spill_retained_against`), which
  at 100k rows turns a depth of 4 into 15 for 13 % more bytes. The design that could have both is
  restoring *into a paged store* — attach the page file and let the 64 MiB page cache decode bands on
  demand, so undo costs ~0 and the snapshot is ~72 MB immediately, with the decode paid by whoever
  next walks every band. Not attempted.
- **Graph nodes 138.4 MB and the area arena 125.0 MB.** The node is back to 16 B (its flags live in
  the run's length word, and the shape pointer replaced the tree). A per-sheet node table (25 B a
  slot) is a further ≈33 MB and changes 13 call sites; the arena is at 1.25× its live length by
  construction.
- **Formula text 133.4 MB.** It is the file spelling, the JSON envelope, the formula bar and the
  save's fast path — the shape design deliberately keeps it (§ 5.4), and 24 B of struct per formula
  is what is left of the per-cell cost.
- **A chained million recalculation takes 0.75 s** (audited above): evaluating two million
  expressions is ~85 % of the pass; a value-slot merge (est. -60-100 ms) and a dense slot index
  (-30-40 ms) are priced, and bytecode linearisation is the only route to Excel's 0.31 s (deferred).
- **A structural edit still prints every moved cell's text.** The rewrite is shape-level now (one
  tree per shape, one print per changed cell, per-cell only where a reference straddles the edit or
  a span has one `$` end on the edited axis), which measures 2.1–2.4×; the printing is the floor.
- **Undo at 1M keeps one live step (241.6 MB).** The paged restore is landed and proved by a
  forced-spill test: an undo of a spilled step hands back a *paged* workbook (page file attached,
  `paged_band_count() > 0`), `last_recalc_cells == 0`, a save right after it byte-identical, and the
  bands decode lazily — a value edit records no delta and decodes nothing. At 1M the gate refuses to
  spill, because the part that would have to go is ~10,400 bands ≈ 1 s of encode inside the edit; the
  measurements that made it refuse are in the round's notes. A workbook *opened* paged (>200 MiB file)
  still undoes through the band-walking diff, because such a step never records a delta.
- **Sort was the one command that reads a permutation of a paged store** (a band per line, evicting
  the one it just read: minutes at 1M). It now takes the sheet back into memory first. An audit of the
  other bulk commands (Remove Duplicates, fill/autofill, `range.setValues`, paste including
  transpose, clear-all, and the print/clipboard exports) found all of them band-ordered: a band is 64
  consecutive rows, and a write materialises the band it lands in. Anything added later that reorders
  rows has to do what sort does.
- **The journal, 15.3 MB** at 1M rows — about forty `range.setValues` entries of ten thousand
  rows each, kept so they replay, under a **16 MB** budget (it was 64: a hundred bulk entries
  nobody read back cost 42 MB). A *single* huge parameter no longer
  survives: a string past 64 KiB is journalled as `<omitted: N chars>` (the base64 of an opened
  file was 94 MB of it), which took the reopened workbook's total from 954 to **869 MB**.
- **A structural edit still prints every moved cell's text** (`rewrite_all_formulas` walks the shape
  trees in place now, but each moved cell with a relative reference has a different A1 spelling).
  Making the rewrite shape-level is the design's S5; the printing cannot be removed, so the win is
  bounded.
- `edit.recalc` over a single dependency chain stays sequential — its cost is per-formula lookups.

## Isolating a chained edit and profiling sort

The gate's `edit.recalc` runs after `aggregate`: the edit of B1 invalidates the two-million-formula
workbook's running-total chain **and** the three formulas in J1:J3. A chain-only edit of a freshly
opened reference file is not interchangeable with that number. To vary the two factors independently
in the same binary, run:

```text
cargo run --release -p cella-engine --example recalc_compare -- --rows=1000000 --repeat=4 --warmup=1
cargo run --release -p cella-engine --example recalc_compare -- --rows=2500 --check-full
```

Each case starts from a new session and reports the wall time of `cell.set`, number of recalculated
cells, undo depth before editing, and, for reopened sessions, the number of cells calculated on
open. Setup and the XLSX round trip are untimed. Kept samples are emitted as CSV; each case's median
is printed separately. The timed gate-compatible B1 edit leaves G1's result unchanged because C1
is zero. The optional check makes a second, untimed edit that must change the chain's final value,
then compares the result with a full recalculation. This headless harness does not measure input to
paint. Use `CELLA_PROFILE=1` in a separate run to inspect phase timings, not for wall-time A/B.

On Windows (Ryzen 7 9700X, release, 1M rows, four interleaved samples per case after one warm-up
round), the median edit times were:

| Workbook before the edit | Chain only (1,000,001 recalculated) | With J1:J3 (1,000,004 recalculated) |
| --- | ---: | ---: |
| Built in memory | 408 ms | 908 ms |
| Reopened from XLSX | 325 ms | 1,008 ms |

No formulas recalculated on open in these samples. The different formula workload explains most of
the gap; reopening does not uniformly speed up the command. These are headless timings, not a
rerun of the desktop comparison with Excel.

To isolate the three formulas, add `--aggregates=none,sum,sumifs,xlookup,all`. With three measured
samples per case after one warm-up round (Windows, release, 1M rows), the first-round engine gave:

| Before editing | None | `SUM` only | `SUMIFS` only | `XLOOKUP` only | All three |
| --- | ---: | ---: | ---: | ---: | ---: |
| Built in memory | 400 ms | 549 ms | 497 ms | 409 ms | 905 ms |
| Reopened from XLSX | 304 ms | 613 ms | 550 ms | 461 ms | 1,000 ms |

These are separate command timings, not additive per-formula costs. The full ten-case order is
balanced over ten rounds; the three-round run was a hotspot screen, not a comparative product claim.
A single-column prototype that pre-evaluated only formula coordinates barely moved the reopened
all-aggregate case (~998 to ~984 ms across the two run batches) and added a temporary coordinate
list proportional to the number of formulas. It was not kept. A more promising follow-up is to
avoid redundant reads in the fold without building a coordinate list, or to reduce `SUMIFS` range
materialization while preserving error and spill behavior.

For sort, the same `CELLA_PROFILE=1` emits one native-only `sort:` summary per command, with
separate milestones for key extraction, comparison, reading/moving cells, formula detection,
writing cells and collecting changed coordinates. Read it alongside `commit:` and `recalc:`: the
usual `sort.desc` orders A:F while the G/H formulas are outside the sorted range but still depend
on values in it. A filtered gate run must include all setup scenarios listed here; `--scenario`
does not run prerequisites automatically:

```text
cargo run --release -p cella-engine --example perf -- 1000000 --scenario=fill.values,fill.formulas,fill.chained,aggregate,edit.recalc,edit.undo,sort.desc
```

The first profile on this workload put source cell reads and movement at 1,521 ms, destination
writes at 896 ms and comparison at 40 ms. Searching for the first formula used to collect and sort
all formulas and took 105 ms in that sample. Formula presence now short-circuits without building
that list, and `CellStore::get` borrows a live band while rebuilding the requested `Cell`. With both
changes, the interleaved release median (four samples per variant, profiling disabled) went from
**399.5 to 388.8 ms** at 100k and **4,726 to 4,553 ms** at 1M. These are combined gains, not a
separate speedup claim for each change. Movement and recalculation remain larger targets than the
comparator on this fixture.

A second round introduced a prepared row reader: sort borrows or decodes one band per source row,
reuses the column mapping and appends straight into the existing movement buffer (no per-row
vector). Column sort keeps its original cell reads. In a new four-sample interleaved A/B, the
median moved from **382.0 to 361.4 ms** at 100k and from **4,508 to 4,292 ms** at 1M. The candidate
binary in this A/B also contained the rejected aggregation experiment, so the exact timing is for
that combined build. A follow-up 1M-only A/B on the final binaries, without that experiment, gave
**4,431 to 4,156 ms** (two samples per side). The process peak working sets across the complete
setup/edit/sort sequence were 1,693–1,694 MiB before and 1,693–1,696 MiB after (rounded); this is
not a sort-only allocation measurement.

### Round 3: packed SUMIFS and sort bookkeeping

A guarded `SUMIFS` path for equal-length, same-sheet single columns and one literal numeric
ordering criterion uses a bit mask instead of materializing both value arrays. Formula/array
inputs, spills and unsupported criteria still go through the general evaluator. Interleaved
1M-row release samples (two per variant) put the reopened SUMIFS-only edit at **523 → 444 ms**
and all three aggregates at **931 → 861 ms**. The combined-build sort measurements did not
isolate a wall-time effect from used-range bookkeeping, but that check now collapses adjacent
sheet IDs before sorting: the six-million-key sort supplies one ID rather than six million.

### Round 4: sort row-write lookup experiment (rejected)

Returning the presence change directly from the band's write removed two destination lookups per
cell in an existing row. The targeted test covered offsetting insertion/removal and used-range
invalidation, but the 1M-row interleaved release median was **3.58 → 3.63 s** and the 100k result
was essentially flat. The candidate was reverted rather than kept on a microarchitectural hunch.

### Round 5: packed single-column SUM over formulas

A large single-column `SUM` can walk packed cells in logical row order: first settle its dirty
formulas, then fold packed constants and live formula values without staging coordinates. A new
spill leaves the accumulator untouched and sends the calculation to the general path. Two serial,
interleaved 1M samples per variant put the reopened SUM-only edit at **589 → 468 ms** and all three
aggregates at **862 → 752 ms**. These are command times, not input-to-paint timings.

### Round 6: moving materialized constants (rejected)

Moving a decoded constant into its output array rather than cloning it removed one text `Arc`
clone/drop, but 1M XLOOKUP-only interleaved results were mixed: built **402 → 381 ms**, reopened
**434 → 460 ms** (two samples each, one candidate reopened sample above 500 ms). No reliable gain
was demonstrated. The value/spill precedence regression was retained; the production move was not.

### Round 7: formula-only graph synchronization

Changed-cell graph updates inspect the stored formula without rebuilding a full `Cell`. The
invalid-formula write remains on the formula path; node and spill removal are unchanged. With
aggregate formulas excluded to isolate the formula read from the new SUM fold, a 1M sort with
outside G/H dependencies went from **3.73 to 3.24 s** (two interleaved samples per variant).

### Round 8: one dependency graph build on forced-open XLSX

`recalc_on_open` now lets the full recalculation build the graph itself when the XLSX requests
`fullCalcOnLoad`. Ordinary cached opens take their existing path. With
`recalc_compare --rows=1000000 --full-on-open`, two interleaved samples per variant put the forced
open at **4.53 to 3.67 s**. The benchmark opens actual XLSX bytes and checks that it recalculated
all 2M formulas; the cached file is checked for zero eager evaluations. A targeted test also
checks the build count, spills, dynamic references and incremental edits after open.

### Round 9: sorting with sparse rows and immediate undo/redo

The `sort_roundtrip` example separately times sorting a 97-key stable permutation, undo, and redo
with values only, formulas inside the sort body, or dependent formulas in the adjacent column.
Its generated fixture checks sampled values, holes, styled blanks, formula addresses, and the
restored states outside each timer. The sparse fixture found a bulk `range.setValues` bug: a blank
would switch to per-cell writes without flushing the accumulated prefix. The command now flushes
that prefix before clearing the blank.

| 1M rows, three samples per case | Sort | Undo | Redo |
| --- | ---: | ---: | ---: |
| Values only | 1,659 ms | 22 ms | 21 ms |
| Formulas inside the body | 3,331 ms | 126 ms | 134 ms |
| Dependent formulas outside the body | 2,432 ms | 26 ms | 24 ms |

The workload has a different fixture and dependency layout from `sort.desc`; compare modes
within this table, not timings between fixtures. It does not measure UI interaction.

```text
cargo run --release -p cella-engine --example sort_roundtrip -- --rows=1000000 --repeat=3
```

## The evaluator round of 2026-11-20: refuted by measurement, one bookkeeping win

Three waves against the chained edit's remaining gap to Excel (the 11-18 session measured 0.36 s
against Excel's 0.08 s on the reference edit; **both sides were re-measured on 2026-11-21 and the
gap is 0.220 s against 0.168 s — 1.3×, not 4.4×** — see [external-benchmark.md](external-benchmark.md)):

- **Per-shape bytecode, built and refuted.** Every shape compiles once into flat opcodes over the
  same kernels the tree uses, executed on a slot stack — equivalence-proven both ways (a new
  differential suite, the 595-example oracle, the cycle test, the fuzz gate) — and then
  sign-tested a **no-win**: the ordinary per-cell workload ran **298 → 328 ms (0/8 pairs, +10 %)**.
  The per-cell cost is kernel-bound (the Host reads and `binary_values`), not traversal-bound;
  changing the traversal cannot pay. The path stays off the default, on the branch
  (`perf/p1-evaluator`, `CELLA_EVAL=bytecode`), with its two-way equivalence suite.
- **Two more calc-side knives, reverted the same way:** a monomorphized `Host` measured the trait
  boundary at **≤5 ns a cell** (16 pairs, medians flipped sign), and comparing at `Sheet::value`
  to skip unchanged writes **cost more than the band copy it saves** (write 43–60 → 80–106 ms).
- **The attribution found the pool: undo bookkeeping.** A built-case edit paid **130–180 ms inside
  `push_history`** — evicting the oldest undo step frees a ~148 MB snapshot right there. An evicted
  step now hands back to a bounded pending-release queue that later cheap commands drain: the
  built-case edit **778 → 544 ms (−30 %, 8/8 pairs, p = 0.0039)**; the gate's `aggregate` 1M is
  443 ms (39 % of budget); `edit.recalc` is unchanged at ~771 ms because its scenario pays the
  eviction in an earlier command. Retained-byte accounting (17–34 ms a command) could not be made
  cheaper engine-side without changing eviction decisions — the walk is the model's
  `retained_bytes_against`, the next pool's owner.
- **The measured map after this round:** the remaining per-command pools are the model's pricing
  walk (17–34 ms), the 2M-dirty closure (217–231 ms), and the model store's read/write kernel —
  the evaluator itself is measured now, not guessed at.

## Round 27: three knives, all sign-tested (2026-11-21)

Certified on an otherwise idle machine with the eight-pair interleaved sign test (a discarded prime
run, odd/even alternating first side, 7/8 ⇒ p = 0.035). Baseline: `main` at `90c4781` plus the
measurement-tool commit; candidates: one per branch. Reproduce with
`plan/bench/ab_gate_recalc.ps1`; raw samples in `plan/ab-r27-*.csv`.

| knife | case | base | cand | delta | fav | p | t | verdict |
|---|---|---:|---:|---:|---:|---:|---:|---|
| closure: a two-formula precedent is a box of second dependents | reopened/ordinary | 866 ms | 707 ms | **−18.35 %** | 8/8 | 0.0039 | −7.38 | **GAIN** |
| pricing: a band's copy tag answers the retained walk | built/no-aggregate | 320 ms | 297 ms | **−7.23 %** | 7/8 | 0.0352 | −2.98 | **GAIN** |
| pricing (second case) | reopened/no-aggregate | 338 ms | 321 ms | −4.83 % | 6/8 | 0.1445 | −1.79 | direction agrees, not certified |
| kernel: compare before writing a band | built/no-aggregate | 314 ms | 251 ms | **−20.08 %** | 8/8 | 0.0039 | −16.01 | **GAIN** |
| kernel: same | reopened/no-aggregate | 353 ms | 277 ms | **−21.40 %** | 7/8 | 0.0352 | −3.15 | **GAIN** |
| kernel: the 6M-cell paste | paste of 1M×6 | 2,011 ms | 1,300 ms | **−35.36 %** | 8/8 | 0.0039 | −21.65 | **GAIN** |

What each knife is: the closure's per-sheet box now holds a precedent's **second** dependent (a
cell two formulas read used to fall into the hash `many` list and pay a cold probe a cell); a
`Band` carries a 24-byte copy tag (`id`, `parent`, `rows_touched`) so pricing a snapshot skips the
per-row walk when the band is provably the same rows, with the tag's bytes subtracted from the
stripe count so the charge is byte-for-byte what it was; and the store compares the packed row
before it copies a band (four write paths), finds a row with one bit probe instead of a binary
search over 48-byte headers, and stops double-probing in `set`.

Gate after the merge: **25/25 within budget**, `mem_drift` 39,401 cycles flat within 0.12 %.
Per-crate: model 170, calc 172, engine 620; fuzz gate 3/3; `cargo xtask wasm` ok; fmt and
clippy -D warnings clean. The model micro-benchmark also moved: `value()` 82.6 → 38.4 ns,
`set` 203.8 → 126.1 ns/cell, `set_values` 1M all-unchanged 49.5–85.0 → 26.9–40.0 ms (same-process
interleaved A/B, direction only). The paste of 6M cells was then certified on its own paired
gate (`plan/bench/ab_paste.ps1`, `crates/engine/examples/paste_bench.rs`): **2,011 → 1,300 ms
(−35.36 %, 8/8, p = 0.0039, t = −21.65)** — the largest number a person actually feels, since
paste is one command they wait on. Its baseline binary had to be built from `90c4781` plus that
one example file, because the harness is new.

## Round 28: the sort (2026-11-21)

A 1M-row sort is the largest wait left after the chained edit, and it was measured before it was
touched (gate, 1M, six value columns): extract keys 67 ms, compare 37, **gather 626**, **write
738**, metadata 35 — then the recalculation's per-cell graph update **282 ms** on a body that
holds no formulas at all, the closure 200, and the run 792 (of which the H chain's eval is 465).

Two knives, both certified by the eight-pair sign test on a quiet machine
(`plan/ab-r28-sort-1m-*.csv`, `plan/ab-r28-sort-100k-*.csv`):

| case | base | cand | delta | fav | p | verdict |
|---|---:|---:|---:|---:|---:|---|
| `sort.desc` 1M | 3,013 ms | **2,633 ms** | **−12.62 %** | 8/8 | 0.0039 | **GAIN** |
| `sort.desc` 100k | 278 ms | **247 ms** | **−11.42 %** | 7/8 | 0.035 | **GAIN** |
| `edit.recalc` 1M (bystander) | 582 ms | 595 ms | +2.30 % | 3/8 | 0.86 | noise |
| `fill.values` 1M (bystander) | 1,526 ms | 1,509 ms | −1.10 % | 3/8 | 0.86 | noise |

1. **The store's row write probes its band once a cell, not three times** (`b8ad983`):
   `set_row` takes `Band::put`'s "had / has" answer instead of a rank probe before and after each
   write, `row_or_insert` asks the O(1) `row_bits` popcount before the binary search, and a batch
   already in column order is not sorted again. Write phase 738 → 652 ms.
2. **A values-only sort tells the recalculation the graph did not move** (`d760296`): when the
   sorted body holds no formulas the commit skips the 6M per-cell `formula()` probes and the
   matching graph removals while keeping the same seeds and the same dependents (round 31 widened
   this into `Calc::cells_moved`, which also takes the handful of addresses whose formula shape
   really did change; a values-only sort passes an empty list). The per-cell segment 282 → 12 ms.

**Rejected on measurement:** an in-place cycle permutation (save the rows about to be overwritten,
walk the permutation) looked like it would remove the 288 MB scratch buffer; it made the write
order jump across bands and measured **2,989 → 3,877 ms**, so it was reverted uncommitted. Band
locality is worth more than the allocation.

Gate after the merge: **25/25 within budget**, `sort.desc` 1M **2.71 s** (40 %) and 100k
**248 ms** (43 %), `mem_drift` 60,301 cycles flat within 0.12 %. model 170, calc 172, engine 622,
the fuzz gate 3/3, wasm ok, fmt and clippy clean; `sort_roundtrip`'s three modes agree and undo /
redo recalculate nothing.

**What is left in a sort:** the moved-formula graph relabel for a body that *does* hold formulas
(embedded measures it at 0.29–0.37 s; it needs a precedents template on each `Shape` so a node can
be translated by a permutation instead of re-walking the tree), and gather (706 ms) / write
(652 ms), whose next step is a band-level row-replacement kernel.

## Round 32: the clutter a sort leaves for the closure (2026-11-21)

`sort.formulas` had fallen to 3.19 s, and the attribution said the dirty closure was the
largest remaining piece at **541 ms**: of the **8M changed cells** a 1M-row sort hands the closure
as seeds, **6M are plain values that can never hold a formula node**, and every one of them paid a
`has_node` probe anyway.

| inside the closure | ms | share |
|---|---:|---:|
| the whole closure (8M seeds, 2M with a node) | 541 | 100 % |
| seed probe `has_node` (8M, two hash levels = one DRAM miss each, 69 ns) | **323** | 60 % |
| seed dependents (dense box, a per-cell column hash, whole-column intervals) | 143 | 26 % |
| dedupe, queue, result list | 75 | 14 % |

**The knife:** the sort already walked those 8M cells and counted which ones hold a formula, so it
hands the recalculation that list. `Calc::cells_moved_seeded(wb, changed, used, relabelled, formula_seeds)`
takes `node_seeds`: those seeds skip the graph probe, **but every seed still has its
dependents expanded**, so a value cell's formulas recalculate exactly as before, and a formula cell
the list covers is queued even when no changed cell feeds it (`=NOW()`, `$Z$1`). Every other call site
passes `None` and behaves as it did.

One shape mattered more than the idea: handing the list in **gather's source order** scattered the
2M formula seeds across the graph and measured **686 ms, worse than the probes it saved**; filling
it in **write-back order (row-major, with a reserve from the gather's count)** landed 213–222 ms.

| | before | after |
|---|---:|---:|
| closure, 1M | 541 ms | **213 ms** |
| `sort.formulas` 1M | 3,382 ms | **2,926 ms** |
| `sort.formulas` 100k | 309 ms | **289 ms** |

Eight-pair sign test with `-Repeat 2` (see below): **1M 3,360 → 2,912 ms (−13.33 %, 8/8,
p = 0.0039)**, 100k 309 → 289 (−6.38 %, 8/8); `sort.desc` and `edit.recalc` came out slightly
faster (7/8 each, the same path serving them). Gate **27/27** with `sort.formulas` at 3.02 s (35 %).

**A method note worth keeping:** the first two 1M attempts at this were INCONCLUSIVE — 6/8 with a
paired σ of 615 ms, then 4/8 with 567 — on a machine whose *own baseline* produced a 2.78 s
outlier in the same window. Running each sample as a best-of-two (`ab_gate.ps1 -Repeat 2`) took σ
to 135 ms and the same change certified 8/8. **A 1M-row scenario on this box needs `-Repeat 2`
before a hypothesis is called a no-win.**

**Equivalence, because a closure change is where "did not recalculate" hides:** the fuzz gate's own
full-vs-incremental invariant (3/3 in debug, 3/3 twice at 120×120 release),
`recalc_compare --check-full` over built/reopened × aggregate/no-aggregate, `sort_roundtrip`
three modes at 1M (its *outside* mode is exactly "a formula outside the block reads a value the
sort moved"), and four new tests written red-first by breaking the change on purpose in two
different ways.

**Still in the closure:** 213 ms — dependents expansion 143 (a per-cell column hash for the second
box, whole-column interval queries) and dedupe/queue/result 75. Next candidates with numbers:
index the columns directly instead of hashing a column per cell (estimated 60–120 ms), and replace
the write-back's Key list with per-row lane bitmaps (recovering the ~50 ms and 32 MB the list
costs).

## Round 31: the sort's formula side, now that the gate can see it (2026-11-21)

The gate only measured a **values** sort (`A1:F`), and a real table's sorted block holds formulas.
`sort.formulas` sorts `A1:H` by column B — `G = B*C+D` and the chained `H` move with their rows —
and it cost **4.19 s at 1M** against the values sort's 1.91 s.

Attribution first (1M, 8M rewritten cells of which 2M are formulas):

| stage | ms | |
|---|---:|---|
| gather (source-order read + `Formula::copied_to`) | 789 | of which **487 ms is re-printing** 2M texts at 243 ns each |
| write cells | 854 | 110 ns a cell — the same price the values sort pays; the extra is 8 columns, not formulas |
| **the sort itself** | **1,816** | |
| graph update loop (8M per-cell visits) | **1,114** | `node_for` 164, insert/remove 496, and ~450 of `formula()` probes that miss |
| graph settle (arena compaction) | 56 | garbage left by 2M node unlinks |
| dirty closure (8M seeds → 2M dirty) | 591 | |
| run (2M formulas really evaluated) | 858 | eval 690 + write 172 |

So the formula body costs **2.64 s more** than the values body, and only 0.36 s of that is real
work: **1.66 s is bookkeeping for rows having moved** (1.17 graph, 0.49 printing).

**Knife A — a sort re-indexes only the addresses whose formula shape changed** (`eaf8480`). A graph
node is *(shape, address)*: what a formula reads is its relative offsets resolved at its cell. A
permutation that hands an address back **the same shape** needs no work at all — not a translated
node, none. Gather now records, per row, the shape identity before and after the move (pointer
equality; a dedup table of up to eight patterns, falling back to the per-cell path beyond that),
and only the addresses whose pattern changed are handed to the new
`Calc::cells_moved(wb, changed, used, relabelled)`; the other 8M cells are still seeds, so every
dependent still recalculates. In the gate's block **two addresses** change shape (the chain head
moved, and one chain cell landed on row 1, whose relative reference crosses the top and becomes
Excel's `#REF!` private shape). Graph update 1,114 + 56 → **0 ms**; 1M 4,613 → 3,368 (−26.99 %,
8/8), 100k 421 → 331 (−21.34 %, 8/8).

**Knife B — printing a moved formula stops paying `core::fmt` a reference** (`90e73d4`). Each
reference's row number went through `write!`; it now appends digits into a stack buffer the way the
column letters already did, and `print_at` reserves 24 bytes instead of growing a `String` from
zero three times. Gather 870 → 717 ms; 1M 3,456 → 3,278 (−5.15 %, 8/8), 100k −4.84 % (7/8).

**Together: 1M `sort.formulas` 4,613 → 3,278 ms (−28.94 %), 100k 421 → 316 (−24.94 %)**; the values
sort and `edit.recalc` stay in the noise. The gate's `sort.formulas` now sits at **3.44 s** (40 % of
budget) and `sort.desc` at 2.09 s; **27/27**. `sort_roundtrip`'s embedded mode went 2.13–2.35 s →
**1.66 s** and the three modes still agree. model 171 / calc 172 / engine 628, the fuzz gate 3/3
(twice at 120×120), wasm ok, `clippy --workspace --all-targets` clean.

**Still open in a sort:** the dirty closure over 8M seeds (591 ms — it does not know which changed
cells have no formula at all, worth an estimated 150–200 ms), `write_cells` (854 ms at the same
per-cell price as the values path; the band-level rebuild was rejected twice in round 30), and the
per-formula ~140 ns of `String` + `Arc` churn that a "one text per cell" model implies.

## Round 30: the sort's two remaining kernels (2026-11-21)

Round 28 took the sort −12.6 %; the attribution said the money was in gather (789 ms) and write
(819 ms) of a 1M-row sort, and two more knives took them to 228–281 and 445–469 ms. Eight-pair
sign test on a quiet machine (`target/r30-ab/plan/ab-r30-*.csv`):

| case | base | cand | delta | fav | p | verdict |
|---|---:|---:|---:|---:|---:|---|
| `sort.desc` 1M | 2,656 ms | **1,931 ms** | **−27.30 %** | 8/8 | 0.0039 | **GAIN** (t = −40.19) |
| `sort.desc` 100k | 250 ms | **203 ms** | **−18.85 %** | 8/8 | 0.0039 | **GAIN** |
| `fill.values` / `edit.recalc` 1M | 1,516 / 593 | 1,503 / 581 | −0.9 % / −2.1 % | 5/8, 6/8 | 0.36, 0.15 | no regression |

1. **A rewritten cell whose runs did not move stops re-walking the band's stripe offsets**
   (`a5a7c86`, model): `Band::put` called `shift_offsets` unconditionally, and that walks all 64
   rows of the band — 6M cells × 64 = 384M row visits in one sort. Only a cell whose kind/style run
   length actually changed needs them. The row write-back's used-box also took three exclusive
   locks a row; one. Write 819 → 445–469 ms (−45 %).
2. **A sort reads its body in source order and scatters each line into its destination slot**
   (`f581cc7`, engine): it used to read the source rows *in destination order*, which is one
   random band hop a row (the store packs cells into bands) — 1M random stripe visits. Reading in
   source order walks the bands once, and each line writes its six cells into the destination
   buffer. The write-back stays in destination order and band-local, which is the red line the
   rejected cycle permutation broke. Gather 789 → 228–281 ms (−65 %).

**Rejected on measurement:** rebuilding whole rows at band level instead of putting cells one at a
time, in two shapes (re-serialize the band: 750 ms; splice lanes in place: 478–580 ms) — both
slower than the 455 ms per-cell path, because band copies are shared with undo snapshots, row
extras get rebuilt, and the columns outside the sorted block have to be laid down again. Reverted
uncommitted.

Gate after the merge: **25/25**, `sort.desc` 1M **2.06 s** (31 % of budget) and 100k **226 ms**
(39 %). model 171, engine 625, the fuzz gate 3/3, wasm ok, fmt and clippy clean;
`sort_roundtrip`'s three modes agree at 1M (values 641–643 ms, embedded 2,130–2,349, outside
1,092–1,166) and the package checksums match. Two rounds of sort work took the gate's 1M number
from **3.80 s to 2.06 s**.

**Still open:** a formula-bearing body's moved-formula relabel (embedded 2.13–2.35 s against
values 0.64 s — about 1.5 s of node re-insertion, and the gate never sees it), and the write
back's remaining ~450 ms (undo-shared band copies ~45, per-cell value dictionaries ~36, the rest
is `put`'s bookkeeping).

## Round 29: the memory a performance win had quietly borrowed (2026-11-21)

Re-measuring the live heap after round 28 caught a **regression nothing else would have caught**:
the reference workbook reported **1.23 GB** where the 11-18 record is 884 MB, and the whole
difference sat in the `history / undo` line — **552.8 MB against 212.3 MB**.

- **The cause was round 26's own win.** Deferring the release of an evicted history snapshot so a
  bulk edit does not pay a 249–296 ms `free()` keeps up to `PENDING_RELEASE_MAX = 2` steps in
  `DocState::pending_release`, drained **by later cheap commands**. A user who does one bulk
  operation and then stops issues no later command, so two ~170 MB snapshots stayed resident
  forever. The memory report added that queue to the undo line, which is why the line moved.
- **The fix lives entirely off the command path.** `Session::has_pending_release` and
  `Session::release_deferred() -> usize` (one `mem::take`, no clock, no thread, wasm-safe); the
  UI calls the drain from `SheetApp::release_idle` on the first line of `logic()`, the shared
  per-frame entry for the native and web hosts, and it returns immediately when the queue is
  empty. The report grew its own `pending_release` line so `undo` no longer carries a queue that
  is not undo. The memory example drains before it prints and says how much it released.
- **Result:** **884.7 MB** total, `undo` **212.4 MB**, `pending_release` 0 after a **340.4 MB**
  release — within a decimal of the 11-18 numbers, so the borrowed bytes were exactly the queue.
- **No performance cost, as required:** eight-pair sign test on the merged tree gave
  `fill.values` −0.91 % (5/8, p = 0.36) and `edit.recalc` +0.37 % (5/8, p = 0.36) — both
  INCONCLUSIVE, i.e. the change does not touch the command path at all. Gate **25/25**, engine
  624, the UI's real-frame interaction suite 45/45, wasm ok, lint clean.
- **The honest limit:** a headless long-lived host (`cella-cli mcp`) has no frame loop, so it
  keeps up to two deferred snapshots unless it calls `release_deferred` itself; that is stated in
  the method's documentation rather than papered over. Wiring it to MCP request boundaries is
  `crates/mcp`'s own change.

### What the remaining memory is, and what it is not

With the bug gone the 884.7 MB is design, not slack: cells 371 MB (formula text 133), the
calculation graph 286 MB, one undo snapshot 212 MB, the journal 15 MB. The graph buys the
recalculation speed and compressing it means a different precedent representation — its own round,
measured against the gate. The undo snapshot is a 1M-row AutoFill; its compression is refused by
the 60 ms encode gate (15,625 bands against `COMPRESS_MAX_BANDS = 600`) and keeping it would put
~1.5 s back into the edit path, which is exactly what round 26 removed. Formula text (133 MB) is
the one piece with a structural answer left — Excel shares the text of a filled region while Cella
shares only the expression tree (2M formulas, 3 shapes) — worth about 100 MB, and it needs its own
round for the formula bar and the xlsx writer.

## Two audits that did not become work (2026-11-21)

- **The frame audit, re-run on the current build** (`plan/bench/p3_cases.ps1 -Case paint`; hidden
  desktop, the 1M reference workbook, fifteen PageDowns, a jump to A500000, a jump back to A1, six
  more PageDowns): **paint average 1.59 ms** (four windows: 1.50 / 1.61 / 1.65 / 1.59), **worst
  single frame 3.94 ms**, working set 984 MB; `goTo A500000` 31.4 ms and back to A1 16.1 ms
  through the control channel. Against the 2026-11-20 sample (avg 1.67–1.77, max 2.45–2.64) the
  average is a little better and the worst frame is worse — one spike in 240 frames. Both numbers
  sit far under the budget a frame actually has (6.9 ms at 144 Hz, 16.7 at 60), so **no frame is
  dropped at either rate** and this stays a watch item, not work.
- **The +75 MB/h memory drift did not reproduce.** An hour of `examples/mem_drift` at 100k rows
  with the process's working set sampled every minute: **168.2 MB at minute 1, 170.4 at 30,
  136.7 at 59** — +4.6 MB/h over the first half, −69.7 over the second, **−32.6 MB/h overall**,
  and the engine's own totals fell from 60.4 MB (cycle 101) to 56.6 MB (cycle 48,851). The
  original +75 MB/h was measured while two agents were compiling in parallel (that report records
  two polluted re-measures of its own). Recorded as not-reproduced with the conditions, not fixed;
  `plan/soak-rss-finding.md` keeps the numbers and the command.

## Interaction and long-run measurements (2026-11-20)

These harnesses close the gaps the gate does not cover (since 2026-11-20 the drift soak *is* one
of the gate's scenarios; the other two remain outside it — see The gate for why). All run on the
development machine's release build; the app runs on a hidden desktop so nothing appears on
screen.

- **Startup** (`plan/bench/startup_r14.ps1`): process start → control port accept → first
  `ui.inspect`. Startup is a fixed **~1.02 s** block before the port binds, with or without a
  document; `--sample sales` adds <20 ms; opening the 70 MB reference workbook then costs its
  own reader time (~3.1–4.4 s depending on column count). Once the port accepts, the UI answers
  within ~5–7 ms.
- **Synthetic input → model/UI state** (`plan/bench/input_r16.ps1`): click a cell (its pointer
  event queues and lands on a later frame — the script waits 200 ms), `ui.edit.begin`, then
  time `ui.edit.commit` and poll `ui.inspect` for the closed editor. A control request's round
  trip is **~40–45 ms** on the hidden desktop (frame-batched), a light edit commits in
  **~46 ms**, and a `B1` edit dirtying ~1M cells in the reference workbook commits in
  **~355 ms** (engine ~310–320 ms + channel). This is state observation, not physical paint;
  `ui.screenshot` was rejected as a proxy (hidden-desktop artefacts), and true input-to-visible
  needs DWM/PresentMon or a camera. The per-request channel floor is the number agent drivers
  should know: a hundred sequential `cell.get`s pay ~4 s of frame waits.
- **Long-run drift** (`cargo run --release -p cella-engine --example mem_drift -- --rows=100000
  --seconds=240`): one session, ~45,900 edit/undo/redo cycles with periodic sorts and saves.
  Steady state settles at **~44.7 MB** and stays flat within **0.02%** over the second half
  (~23,000 cycles); the startup decline from 129 MB is the undo budget aging the large fill
  steps out, by design. No leak found in the commit, journal, undo or graph paths. The gate now
  runs this same soak as its `mem_drift` scenario (re-measured 2026-11-20: 48,326 cycles, flat
  within 0.13%).

## Adding a scenario

1. Add a `fn` to `crates/engine/examples/perf.rs` and an entry to its `STEPS` table (id, what it
   measures, function). The id is the budget key: `<area>.<thing>`, e.g. `filter.auto`.
2. Run `cargo xtask perf --update --repeat=3` — the new scenario is recorded; the gate fails until
   it is.
3. Add the row to the table above if it is one a reader should recognise.

A scenario the harness cannot express — it needs another process, a long soak, or one repeated
operation — belongs to the gate itself, in `xtask/src/perf.rs`: a `measure_*` leg that runs a
release subprocess and returns the same `Measured` rows. The id must be excluded from the
harness filter (`harness_has_work` and the `--scenario` pass-through in `measure`) and listed
in `wants`, then `cargo xtask perf --update --scenario=<id>` records its budget the same way.
`save.repeat` (its own example, `xtask/examples/save_bench.rs`) and `mem_drift` (an existing
engine example, run as a subprocess) are the two patterns.

The harness stays dependency-free (engine + serde_json only), opens no dialogs, and caps anything
derived from its arguments — the row count at 5,000,000 and `--repeat` at 10.

## Revision history

- **2026-11-21 (round 32, the closure):** of the 8M changed cells a 1M sort seeds, 6M are values
  that cannot hold a node and each paid a `has_node` probe (323 of the closure's 541 ms). The sort
  hands the recalculation its exact formula-cell list (others pass `None`): closure **541 →
  213 ms**, `sort.formulas` **3,360 → 2,912 ms (−13.33 %, 8/8, p=0.0039)**, 100k −6.38 %; the list must
  be filled in write order (source order measured 686 ms). Also recorded: the frame audit (paint
  avg 1.59 ms, worst frame 3.94) and the +75 MB/h drift **did not reproduce** over an hour
  (−32.6 MB/h overall). Method note: 1M scenarios on this box need `-Repeat 2` — two single-pass
  runs called this a no-win at σ 615/567 ms, best-of-two certified it at σ 135.
- **2026-11-21 (round 31, the sort's formula side):** the gate grew `sort.formulas` (a body that
  holds formulas; 4.19 s at 1M against the values sort's 1.91). Attribution put 1.66 s of the
  2.64 s difference into bookkeeping (graph re-index 1.17, re-printing 0.49) and 0.36 into real
  work. A sort now re-indexes **only the addresses whose formula shape changed** (pointer identity
  per row, eight patterns, per-cell fallback) and printing stops paying `core::fmt` a reference:
  **1M 4,613 → 3,278 ms (−28.94 %, 8/8), 100k −24.94 %**; gate **27/27**, embedded 2.13–2.35 →
  1.66 s. Earlier: **2026-11-21 (round 30, the sort's kernels):** `Band::put` stops re-walking stripe offsets
  when a cell's runs did not move (384M row visits a sort) and the sort reads its body in source
  order instead of destination order (one random band hop a row): **1M sort 2,656 → 1,931 ms
  (−27.30 %, 8/8, p=0.0039, t=−40.19)**, 100k −18.85 % (8/8); band-level row rebuild was built
  and reverted (750 / 478–580 ms against 455). Gate 25/25, `sort.desc` 1M at 2.06 s — 3.80 → 2.06
  across two rounds. Earlier: **2026-11-21 (round 29, the borrowed memory):** the heap re-measured 1.23 GB against the 884 MB
  record and the difference was round 26's deferred release holding two evicted snapshots for a
  user who never issues a later command. An idle frame now drains them
  (`Session::release_deferred`, `SheetApp::release_idle`), the report grew a `pending_release`
  line: **884.7 MB / undo 212.4 MB / 340.4 MB released**, with no command-path change
  (fill.values −0.91 %, edit.recalc +0.37 %, both INCONCLUSIVE; gate 25/25). Earlier:
  **2026-11-21 (round 28, the sort):** the store's row write probes its band once a cell and a
  values-only sort skips the per-cell graph update — **1M sort 3,013 → 2,633 ms (−12.62 %, 8/8,
  p=0.0039)**, 100k 278 → 247 (−11.42 %, 7/8); bystanders in the noise; gate 25/25 with
  `sort.desc` at 2.71 s. An in-place cycle permutation was built and reverted (write order lost
  band locality, 2,989 → 3,877 ms). Earlier: **2026-11-21 (round 27, three knives and a paste):** the 6M-cell paste **2,011 → 1,300 ms
  (−35.36 %, 8/8, p=0.0039, t=−21.65)**, certified with a purpose-built paired gate
  (plan/bench/ab_paste.ps1); closure second-dependent box closure second-dependent box (−18.35 %,
  8/8, p=0.0039), band copy tag for retained pricing (−7.23 % built, 7/8; reopened 6/8 direction
  only), store compare-before-write and one-bit row find (−20.08 % built 8/8, −21.40 % reopened
  7/8). Gate 25/25; model 170 / calc 172 / engine 620; fuzz 3/3; wasm and lint clean. Raw:
  plan/ab-r27-*.csv. Earlier: **2026-11-20 (the evaluator round):** refuted by measurement and recorded so it is never
  re-priced by guess: bytecode per shape (no-win, +10 %, kernel-bound per-cell cost), a
  monomorphized Host (≤5 ns a cell), compare-at-`Sheet::value`-to-skip-writes (costs more than
  the copy); the win was bookkeeping — an evicted undo step releases on later cheap commands
  (built-case edit 778 → 544 ms, −30 %, 8/8 pairs, p=0.0039; gate `aggregate` 443 ms;
  `edit.recalc` unchanged, its sequence pays the eviction inside `aggregate`). Evidence:
  plan/p1-wave{2,3}-*.md, plan/ab-p1*.csv. Earlier: budget lock, P2 — the gate's budgets re-recorded from a quiet best-of-three
  release pass — the 2026-11-16 table let the 0.13.0 numbers pass at 23–52% of budget, so a
  regression only tripped at nearly 2× the old numbers; the new ones are twice the current quiet
  numbers, verified 22/22 green. Two scenarios added to the gate: `mem_drift` (the 240 s soak at
  100k rows; per-cycle budget 11 ms, flatness ±2% asserted in the gate itself; 43.9k–48.3k cycles
  across the runs, held 0.13%) and `save.repeat` (in-process best-of-6 save micro-benchmark,
  `xtask/examples/save_bench.rs`; budgets 100k 370 ms, 1M 3.20 s). The over-budget re-measure is
  now keyed by (scenario, rows), fixing a positional-pairing hazard in `keep_min`. Startup and
  input-to-state stay with the plan/bench harnesses: porting them needs hidden-desktop GUI
  orchestration that does not belong in a cross-platform, Rust-only gate (reasoning in The gate).
  Final gate: **25/25 green**.
- **2026-11-20 (statistical recertification):** every kept gain was re-measured with a paired
  sign-test A/B gate (8 pairs, rules fixed in advance). Certified: sort's formula-only graph
  reads **−9.6%** (8/8), packed `SUMIFS` **−20.8%** (8/8), the three-aggregate chained edit
  **−18.4%** (8/8), forced-open one-build **−15.6%** (8/8), bulk-import journal/scan **−8.2%**
  (8/8), save streaming **direction certified** (7/8; steady-state −1.5% under best-of-6,
  typical-pass −6~−8% under single-pass medians), sort presence/row reads direction certified
  (4-vs-4 complete separation twice, magnitude −4~−6%). Packed `SUM` first failed at 6/8 on
  interference (paired σ 435 ms) and was **re-certified in a quiet window: −20.0% (8/8, paired
  σ 4.9 ms)**. Certifiable floor
  per scenario comes from same-binary A/A runs: paired σ 3.3–7.9% of the median.
- **2026-11-20 (rounds 11–20):** two measured wins kept — save streams worksheet windows into
  the packer's one deflate stream (byte-identical archives; three independent measurements
  **−7.8/−6.1/−3.6%**, same direction each time, point estimate **−4~−8%** — below the gate's
  statistical certification floor, kept on its byte-identity guards and attribution; deflate is
  now the pinned binding leg) and bulk imports drop the journal deep-clone plus the empty-graph
  per-key scan (**`fill.values` −8.2%, sign-test certified at 8/8 pairs, p=0.004**). Two
  candidates were implemented, measured slower or
  flat, and reverted in full (a streaming formula walk for opens; row-batched formula reads for
  sorts) — on identity-mapped stores the materialising walks they replaced are already
  near-optimal. The previously unmeasured items now have numbers: startup **~1.02 s** fixed,
  input-to-state **~40 ms** channel / **~355 ms** heavy Enter, and **no memory drift** over a
  45.9k-cycle soak. A paired sign-test A/B gate (`plan/bench/ab_gate.ps1`) now calibrates the
  machine's noise floor (A/A: paired σ 3.3–7.9% per scenario) and decides keeps. Final gates:
  clippy clean, 1,596 lib tests, fuzz 12×100, wasm, perf 22/22 on unchanged budgets.
- **2026-11-20 (rounds 14, 16, 17):** closed the unmeasured items. Startup is a fixed **~1.02 s**
  (document-independent, hidden desktop); synthetic-input-to-state gives a **~40–45 ms**
  control-channel round trip, **~46 ms** light edit commit and **~355 ms** for an edit dirtying
  ~1M cells; a 240-second soak (**~45.9k edit/undo/redo cycles, ~917 sorts, ~229 saves, 100k
  rows**) holds **~44.7 MB ±0.02%** in steady state — no drift. Round 15 (row-batched formula
  reads) was rejected on measurement and reverted: the per-key reads it targeted are far cheaper
  than estimated on cache-warm data, and the sort's recalc segment is graph work.
- **2026-11-20 (round 12):** worksheet XML now streams window by window into the packer's one
  incremental deflate stream (byte-identical: chunked==whole test plus every package golden), and
  the shared-string pre-pass walks packed values instead of rebuilding every `Cell` (paged stores
  keep the non-caching scanner). `save.xlsx` 1M **1.83 → 1.69 s (−8%)**; native attribution shows
  the remaining binding leg is the deflate itself (strings 136 ms, rows 511 ms, streamed 1.22 s),
  which the byte-stable output contract pins in place.
- **2026-11-20 (round 11, rejected):** a streaming ordered formula walk (no materialized `Vec`,
  no per-formula `Arc` clones, paged bands through the non-caching scan channel) passed its
  equivalence tests but lost the release A/B: `rebuild.graph` 750 → 835 ms, forced-open and cached
  opens worse by 100–230 ms. On the identity-mapped stores these paths run on, the materializing
  walk is already near-optimal — the sort input is pre-ordered (nearly free), rebuild's second
  collect moves without re-cloning — while 2M callback dispatches and the exact-count pass cost
  more than they save. Reverted in full.
- **2026-11-20 (rounds 3–10 gate):** the final tree passed clippy (`-D warnings`, four crates and
  both benchmark examples), 1,116 lib tests, the command-sequence fuzz at 12 seeds × 100 steps, the
  wasm check, and all 22 perf budgets. Against this session's starting release numbers, 1M rows:
  `sort.desc` **4.86 → 3.64 s**, `edit.recalc` **0.90 → 0.73 s**, `aggregate`
  **0.75 → 0.59 s**, forced-open XLSX **4.53 → 3.67 s**. A sparse-row workload found and fixed a
  bulk `range.setValues` bug that dropped a row's prefix cells before a blank. Rounds 4 and 6 were
  measured and rejected. Still not measured: startup, input-to-paint latency, non-Windows
  platforms, long-run memory drift.
- **2026-11-20:** measured `SUM`, `SUMIFS` and `XLOOKUP` independently; kept batched source-row
  reads for sort, rejected the single-column formula-coordinate allocation experiment.
- **2026-11-19:** separated chained-edit timing by workbook state and aggregate workload; added
  native sort phase timings, a short-circuit formula-presence query, and borrowed live-band reads.
- **2026-11-18 (eleventh):** the dense single-precedent dependents table, **rebuilt and kept**. The
  closure's 1M walk goes **190 -> 27 ms** (CELLA_PROFILE, 1,000,001 cells), and with the slot-table
  change below the command goes **589/596 -> 465/460 ms**. The ninth attempt's maintenance and sparse
  fallback are unchanged; growth is not. The box carries an explicit cell `capacity` and is copied
  **once a doubling** of it; a cell it refuses keeps its hash entry, and one batch in `settle` (at
  most one pass over a sheet's entries a command) takes those in when the box around them is worth
  its cells. The shape that killed the ninth attempt is the one it was accepted on:
  `fill.formulas` at **100k 154 ms** (164 before) and **1M 1.27/1.28 s**, against a 3 s ceiling. Its
  memory was not priced when it was accepted and turned out to matter: the dependents table went
  **142.6 -> 56.0 MB**, so the workbook's own ledger (this time from `xtask perf-report`, which runs
  `examples/memory` on the same build) reads **884 MB live and 683 MB reopened**, against 971 and
  769.9 before the box. The audit above was right that the old table was not slack; it was the
  structure, and the box changed the structure.
- **2026-11-18 (tenth):** the build of a pass's slot table, two costs the audit put at 36.7 and 35 ms
  on the 1M fill. The table's `Vec<Key>` and `Vec<Source>` became one `Vec<Entry>` (32 B a slot, the
  shape a slot reads **borrowed from the graph's node** rather than `Arc::clone`d: a million atomic
  increments and as many decrements a pass, for shapes the graph owns); and the per-cell `graph.node`
  probe became one sheet-table probe a sheet run (`Graph::nodes_of`, `dirty` being row-major so its
  cells of one sheet arrive together). Build **66-74 -> 40-47 ms** (CELLA_PROFILE, `trees N slots`),
  `edit.recalc` **641/652 -> 589/596 ms**, same-binary A/B, two runs each side. The closure is
  unchanged (as it must be: this moves no dependents). No structure a cell carries was added.
- **2026-11-18 (ninth):** a dense single-precedent dependents table for the closure (84 % of the
  phase was one hash probe a cell) was built, tested and **reverted on measurement**: at 100k rows
  `fill.formulas` went 164 ms -> 37.96 s, a box grown by copying all entries per new precedent.
  The audit stands (the probes cost the 158 ms); the growth strategy does not. Next attempts on the
  closure must grow geometrically and are accepted against a 100k fill, not just the unit tests.
- **2026-11-18 (eighth):** the recalculated chain, two steps. A **dense slot index** (a pass's cells
  looked up by arithmetic, hash fallback when they do not fill their box) takes `edit.recalc` at 1M
  **893 -> 665-692 ms**; a **chain run** (a column of running totals accumulated in one loop through
  the same `binary_values` kernel) takes eval **218 -> 179 ms** and the command **712 -> 646 ms**.
  Both same-binary A/Bs in the gate harness, best of three, two runs each side. The chain run measured
  three times larger in a dev build, which is the lesson recorded above.
- **2026-11-18 (seventh):** the chained edit, 9-10 % faster (**0.85/0.83 -> 0.75/0.77 s**, two
  runs each side of a same-binary A/B). The scalar fast path and the generic array path share one
  implementation and a chained fill keeps eight links in a stack array (`INLINE_CHAIN`), and the
  closure pass probes each cell once: hoisted empty-table checks, `SeenCells` bitmaps sized once, a
  `Vec` with a head index instead of a reversed stack, no per-formula frame in `calculate_at`, and a
  `legacy_of` short-circuit (with a bitmap-doubling regression of its own caught by the fuzz). The
  comparison table's Cella column predates it - the gap to Excel is 2.4x now, not 2.9x.
- **2026-11-18 (sixth):** measured against Excel 16 on the same file: open **3.3 s vs 8.0 s**,
  recalculate **0.90 s vs 0.31 s**, working set **1,213 vs 588 MB**, private bytes **1,538 vs
  468 MB**, idle **382 vs 35 MB**. Our own accounting (971 MB) is in the right place; the app's
  baseline is not, and the recalculate gap is 2.9x.
- **2026-11-18 (fifth):** the journal budget 64 → 16 MB. The cap kept a hundred bulk entries at 1M rows (42 MB); sixteen megabytes still holds about forty, or thousands of single-cell commands. Journal 42.1 → **15.3 MB**, live heap 998 → **971 MB**, reopened 796.8 → **769.9 MB**.
- **2026-11-18 (fourth):** the undo step audited. `examples/memory` prints the stack (one
  step, 212.3 MB after the two memory rounds of the same day; the charged figure was 241.6). Its
  parts are the pre-edit sheet itself — stripes 85.8, row side tables 56.0, the text table 32.0,
  line metadata — which is what restoring a fill needs; a value delta would cost the same and
  paging was already refused. No change made beyond the diagnostic.
- **2026-11-18 (third):** the calc graph audited. Nodes 104.9 MB, dependents 142.6 MB and the arena
  125.0 MB are power-of-two hash tables at their smallest bucket count — a `shrink_to_fit` after
  a build was written, measured and reverted because it changes nothing. A sorted vector per sheet
  or a CSR dependents table is the only route down (~250 MB) and it pays in the edit path.
- **2026-11-18 (second):** the band's number index. Bands built by a fill or a paste never called
  `Band::finish`, so each kept its 1,024-byte number index and a quarter of its number table as
  slack: number tables **66.0 → 33.0 MB**, live heap 1.06 → **0.998 GB** (reopened unchanged at
  796.8 MB). `band_for_write` finishes the band being left; every bulk write finishes its run.
- **2026-11-18:** the row side table, 144.0 → **72.0 MB**. A row with formulas carried one heap
  allocation of three vectors (72 B) plus the formula vector's doubling — four 16-byte slots for
  two entries — 144 B a row. The rare parts (an array value, the wide payload table) moved out of
  line behind one boxed pointer and the formula vector is sized exactly (a `shrink_to_fit` after an
  edit and one in `put_row`, the reader's path, which kept the spare slots for every row of a
  reopened workbook). 1M × 6 live heap **1.14 → 1.06 GB**, reopened **868.8 → 796.8 MB**;
  fill.formulas 1.77 → 1.38 s, fill.chained 2.96 → 2.27 s, edit.recalc 1.80 → 0.87 s,
  rebuild.graph 1.12 → 0.89 s (best of three; the recorded budgets are unchanged).
- **2026-11-17 (third wave):** the design's last step and two structural items. A structural edit
  (insert/delete/sort/rename) rewrites **once per shape** and prints the changed cells — insert a row
  above 2M formulas **8.29 → 3.97 s**, delete **8.61 → 3.59 s**, rename a sheet **8.58 → 4.86 s**; the
  node table became per sheet keyed by `CellRef` (**138.4 → 104.9 MB**, 25 B a slot); and an undo
  restores **a paged store** instead of decoding it (history 241.6 MB unchanged at 1M by the cost
  gate; landed, proved by a forced-spill test; a sort of a paged store used to take minutes and now
  takes the sheet back into memory first). Total **1.14 GB**; final gate: fill formulas 1.77 s,
  chained 2.96 s, recalc 1.80 s, rebuild 1.12 s, sort 6.78 s, save 2.05 s, open 4.02 s.
- **2026-11-17 (second wave):** formula **shape sharing**, then the deletion of the old
  representation (`Formula` 72 → 56 → 48 B, `calc::Node` 24 → 16 B, no per-cell tree at all), and the
  graph/save work around it. One
  interned tree per formula shape (relative references as offsets from the cell), the cell keeping
  its exact A1 text: **2M formulas hold 3 shapes**, `edit.autoFill` over 1M rows costs 0 parses,
  trees 768 → 12 MB, and after the old per-cell tree was deleted (`Formula` 48 B, `Node` 16 B)
  total **2.90 → 1.17 GB** (reopened 0.95 GB). The evaluator reads a shape in
  place (`range_at`), fill/copy/paste/sort/read share the `Arc`, and the save stopped analysing a
  resolved tree per formula. With the graph index/packing and the parallel sheet build: save 3.7 →
  **1.97 s**, open 5.4–5.6 → **4.08 s**, fill formulas 2.8 → 2.15 s, recalc 2.3 → 1.81 s, graph
  rebuild 1.7 → 1.25 s, sort 7.9 → 6.85 s — no scenario slower. Three regressions were caught by
  re-measuring the merged tree on an idle machine and fixed before recording (§ The memory round of
  2026-11-17).
- **2026-11-17 (first wave):** the memory and save round. A session reports its live heap by part (`StoreMemory`,
  `GraphMemory`, `Session::memory_report`, `examples/memory.rs`); at 1M rows × 6 columns it is
  **2.56 GB** (cells 1.27 GB with 768 MB of formula trees, graph 516 MB, undo 738 MB), and 1.84 GB
  reopened. The precedent index went 478.2 → 142.6 MB (per sheet, `CellRef`-keyed, a bare pair for
  the one-to-one case) and `fill.chained` 5.24–5.55 → 4.35–4.63 s in a same-binary A/B. Worksheet
  rows are built in parallel windows with the string table resolved by a pre-pass: the XML build
  fell from 91 % to 46–49 % of a save, **save 3.7 → 2.45 s**, 100k 356 → 211 ms, bytes identical
  (§ The memory and save round of 2026-11-17).
- **2026-11-16:** grouping, filtering and row visibility are no longer structural edits: 3.2–3.5 s →
  0.6 ms–5 ms on the reference workbook with no `SUBTOTAL` in it, and ~0.4 s with a `SUBTOTAL(109)`
  over the filtered million rows (the formula itself, measured against a `SUM` over the same range at
  ~0.3–0.5 s). 16 commands moved off `structural` (§ The performance round of 2026-11-16).
- **2026-11-16:** `rebuild.graph` added: the graph build is measured on its own (2M formulas of the
  reference workbook, 1.50 s at 1M rows, from 3.87 s). The graph now keeps its precedent areas in
  one arena, its dependents inline per cell (`Deps`), and walks a formula without allocating
  (`calc::walk`), which also makes `fill.formulas` and `open.xlsx` cheaper (§ Measured on the dev
  machine).
- **2026-11-16:** created. `cargo xtask perf`, the nine named scenarios, `xtask/perf-budgets.txt`
  and the `--json` document the gate parses (schema 1).
