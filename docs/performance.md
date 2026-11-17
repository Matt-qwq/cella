# Performance

> **Last reviewed:** 2026-11-17 · **Last updated:** 2026-11-17 · **Change:** the memory and save round: every part of a workbook's live heap is measured (§ The memory and save round of 2026-11-17), the dependency graph's precedent index is a third of its bytes, and a worksheet's rows are built in parallel windows (save 3.7 s → 2.45 s at 1M rows; the XML build fell from 91 % to 46–49 % of a save). Earlier rounds: grouping, filtering and row visibility no longer rebuild the graph, `open.csv` is a scenario, and the dependency-graph build is four times cheaper · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

GridCraft is for people who work on real data sets — programmers, analysts, e-commerce operations —
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

Run it yourself:

```bash
# the human table, at 100k and 1M rows (the gate's sizes)
cargo run --release -p gridcraft-engine --example perf

# one size, machine-readable, best of three passes
cargo run --release -p gridcraft-engine --example perf -- 1000000 --json --repeat=3

# only some scenarios
cargo run --release -p gridcraft-engine --example perf -- --scenario=sort.desc,open.xlsx
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

A scenario that comes out over budget is measured **once more** before the run fails: a build that
started on the same machine is a slow pass, not a regression, and a real regression stays over
budget twice. Missing budgets are never retried — that is a mistake in the file, not a timing
question.

```text
scenario                 rows    measured     budget      of
------------------  ---------  ----------  ---------  ------
fill.values            100000      210 ms     420 ms     50%
save.xlsx             1000000      2.45 s     4.90 s     50%
edit.recalc           1000000      2.31 s     4.65 s     50%
```

The `of` column is the share of the budget the run used; `142%!` marks an overrun.

**It is deliberately not part of `cargo xtask ci`.** `ci` runs on every commit and needs no
release build; this gate builds the engine in release and takes minutes at 1M rows, so it runs:

- **before a release** (see [releasing.md](releasing.md)),
- **after any change to a hot path** — the cell store, recalculation, fill, sort, the xlsx reader
  and writer, `range.setValues` — where "it feels faster" is not a review comment.

Slow or shared machine: `GRIDCRAFT_PERF_SCALE=2 cargo xtask perf` (or `--scale=2`) multiplies
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
- The two tables are separate sources. Best-of-three reports the pass the machine was quietest for,
  which is why `open.xlsx` (the longest scenario, so the one a loaded pass distorts most) comes out
  at 8.6 s here against the 15.3 s of the single-pass ad-hoc example. The budgets are keyed to the
  harness, so compare like with like: `cargo xtask perf` against `xtask/perf-budgets.txt`.

Known hotspots and what is left of them are tracked in [`gaps.md` §2](gaps.md) — this page owns
the measurement, that entry owns the work.

## The performance round of 2026-11-16, measured on a quiet machine

Five batches took the numbers above and moved them. **Same workbook, same command sequence, release,
best of three passes on an otherwise idle machine** (`cargo run --release -p gridcraft-engine
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

## The memory and save round of 2026-11-17

Three batches of code (plus the shape-sharing design the numbers then commissioned), all
measured on this machine in release: **byte accounting for a live session**
(so "where does the memory go" stops being folklore), **the dependency graph's precedent index**
(the accounting immediately showed it was the largest single structure), and **the worksheet XML
build** (still serial, 66 % of a save). Every number below was taken through the harness on an idle
machine with `--repeat=3` except where a stashed same-binary A/B is named.

| scenario (1M rows × 6 columns) | 11-16 recorded | 11-17 gate run | change |
|---|---|---|---|
| save 72 MB | 3.7 s | **2.45 s** | −34 % (the round's one large move) |
| open 72 MB | 5.4–5.6 s | 5.13 s | −5–8 % (machine quiet; not a claim) |
| sort 1M rows | 7.9 s | 7.46 s | within noise |
| fill values / formulas / chained | 2.2 / 2.8 / 4.0 s | 2.28 / 2.74 / 3.91 s | within noise |
| edit one cell → recalc | 2.3 s | 2.31 s | within noise |
| undo | 30 ms | 27 ms | within noise |
| aggregates | 1.1 s | 1.00 s | within noise |
| dependency-graph rebuild (2M formulas) | 1.7 s | 1.64 s | within noise |
| open 57 MB CSV | 0.65 s | 0.61 s | within noise |

The graph index does have a real speed effect, but only a same-binary A/B can show it (stash the
diff, measure, restore): `fill.formulas` **3.61–3.85 → 3.13–3.21 s**, `fill.chained` **5.24–5.55 →
4.35–4.63 s** (−17 %, 2M links into the index), `rebuild.graph` and `edit.recalc` flat. The gate
table above is the record; these two are the honest size of the graph change.

### Where the live heap goes

`cargo run --release -p gridcraft-engine --example memory -- 1000000` builds the reference workbook
through the same three fills and prints one line per allocation class. It is arithmetic on lengths,
capacities and layouts — not a heap census — and `total == Σ parts` is a test in each crate.

| part (1M × 6, 2M formulas) | before the graph index | now | share of the 2.56 GB |
|---|---:|---:|---:|
| cells | 1,267.7 MB | 1,267.7 MB | 49.5 % |
| · formula trees (`Arc<Expr>`, 2M) | 768.0 MB | 768.0 MB | 30.0 % |
| · formula text + `Formula` | 165.4 MB | 165.4 MB | 6.5 % |
| · row side tables (`RowExtras`) | 144.0 MB | 144.0 MB | 5.6 % |
| calc: graph nodes (2M) | 172.0 MB | 172.0 MB | 6.7 % |
| calc: dependents map | **478.2 MB** | **142.6 MB** | 5.6 % |
| calc: precedent-area arena | 201.3 MB | 201.3 MB | 7.9 % |
| history / undo stack | 737.6 MB | 737.6 MB | 28.8 % |
| journal (base64 of the saved file) | 42.1 MB | 42.1 MB | 1.6 % |
| **total** | **2.90 GB** | **2.56 GB** | −11.6 % |

Saved and reopened (70.4 MB .xlsx): **1.84 GB** (was 2.18 GB). The trees do **not** disappear on
open — a full recalc parses every formula and hands the tree back to its cell — so a reopen buys the
undo stack and a tighter number table, not the formula representation. Text dictionaries grow 11 MB
(the reader builds per-band `Arc<str>`s) and the journal grows to 93.8 MB (an open's params keep the
whole file). At 100k rows the same proportions hold (515.9 → 473.9 MB), so the split is
scale-invariant.

What each batch did, in one line each:

- **Accounting** — `StoreMemory` (bands, stripes, value dictionaries, texts, row extras, formula
  text, trees, arrays, page cache), `GraphMemory`/`CalcMemory` (nodes, areas, the dependents map and
  its long lists, spills, cycles), `Session::memory_report` adding the workbook and the history
  stacks, and `examples/memory.rs` (`crates/model/src/store.rs`, `crates/calc/src/recalc/graph.rs`,
  `crates/engine/src/memory.rs`). It also fixed `Band::heap_bytes`, which under-counted a band
  (two text-dictionary vectors, overflow vectors, `len` instead of `capacity`), so the decoded-band
  page cache now charges exactly what the report charges.
- **The precedent index** — the 5M-entry `Key → Deps` map became one table per sheet keyed by
  `CellRef`, with a bare one-to-one pair for the cell exactly one formula reads and a `Vec` from the
  second on, plus a separate cross-sheet table so the rare case costs the common entry nothing:
  **95.6 → 28.5 bytes an entry**, 478 MB → 143 MB (`crates/calc/src/recalc/graph.rs`). A sorted
  arena would have been smaller (80 MB) and was rejected: a fill inserts out of order, so each
  insert would memmove a megabyte-long tail.
- **Parallel rows** — the shared string table is now filled by a pre-pass over row windows in the
  file's first-use order, and `<sheetData>` is built in parallel row windows that concatenate to the
  byte the serial walk produced; per-sheet part numbering stays serial and in sheet order
  (`crates/xlsx/src/sheet_write.rs`, `write.rs`). Package bytes identical, the golden save test
  unchanged (37,185 B), and a test writes the same workbook with 1/2/8 windows and compares the
  whole package.

What is left, in the order the accounting ranks it:

- **Formula trees, 768 MB.** One interned tree per formula *shape* (relative references as offsets)
  saves ≈512 MB and shrinks the recalc pass's working set; the validated design, the 27 call sites
  and the seven landing steps are in `plan/SHAPE-SHARING.md` (local). It is its own round.
- **The undo stack, 737.6 MB.** Every fill rewrites every band, so each history entry retains a whole
  copy of the sheet, and the budget always keeps the latest. The fix is a snapshot representation
  (compressed bands or per-band deltas) with undo latency kept usable (27 ms today), not a constant.
- **Graph nodes (172 MB) and the area arena (201 MB, 81 MB of it capacity a fill never fills).**
  Packing a node's flags into the length word is −34 MB; a `u32` sheet in `Area` is −32 MB; trimming
  the doubling tail needs a realloc trigger the fill loop will not thrash. Each is a measured step
  of its own.
- **The journal, 42–94 MB**, keeps the saved file as base64 in the open's params.
- `edit.recalc` over a single dependency chain stays sequential — its cost is per-formula lookups.

## Adding a scenario

1. Add a `fn` to `crates/engine/examples/perf.rs` and an entry to its `STEPS` table (id, what it
   measures, function). The id is the budget key: `<area>.<thing>`, e.g. `filter.auto`.
2. Run `cargo xtask perf --update --repeat=3` — the new scenario is recorded; the gate fails until
   it is.
3. Add the row to the table above if it is one a reader should recognise.

The harness stays dependency-free (engine + serde_json only), opens no dialogs, and caps anything
derived from its arguments — the row count at 5,000,000 and `--repeat` at 10.

## Revision history

- **2026-11-17:** the memory and save round. A session reports its live heap by part (`StoreMemory`,
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
