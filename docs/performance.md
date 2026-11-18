# Performance

> **Last reviewed:** 2026-11-17 · **Last updated:** 2026-11-17 · **Change:** the memory round (§ The memory round of 2026-11-17): byte accounting per part, the dependency graph compacted twice, the save build parallelised, formula **shape sharing** (one interned tree per shape; the per-cell tree is gone — `Formula` 48 B, `calc::Node` 16 B), a **structural edit rewritten once per shape** (insert a row above 2M formulas 8.29 → 3.97 s, delete 8.61 → 3.59 s, rename a sheet 8.58 → 4.86 s), and an **undo that restores a paged store** (gated so it never costs the edit). A 1M × 6 workbook went **2.90 → 1.14 GB** live (reopened 0.95 GB) and no scenario is slower (save 3.7 → 2.05 s, open 5.5 → 4.02 s, fill formulas 2.8 → 1.77 s, recalc 2.3 → 1.80 s, graph rebuild 1.7 → 1.12 s). Earlier rounds: grouping/filtering no longer rebuild the graph, `open.csv` is a scenario, and the graph build is four times cheaper · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

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

A scenario that comes out over budget is measured **once more** before the run fails: a build that
started on the same machine is a slow pass, not a regression, and a real regression stays over
budget twice. Missing budgets are never retried — that is a mistake in the file, not a timing
question.

```text
scenario                 rows    measured     budget      of
------------------  ---------  ----------  ---------  ------
fill.values            100000      200 ms     400 ms     50%
save.xlsx             1000000      1.97 s     3.95 s     50%
edit.recalc           1000000      1.81 s     3.65 s     50%
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
| · dependents map | 478.2 MB | 142.6 MB | 12.6 % |
| · precedent-area arena | 201.3 MB | 125.0 MB | 11.0 % |
| **history / undo** | **737.6 MB** | **212.3 MB** | 21.3 % |
| journal (recent commands) | 42.1 MB | **15.3 MB** | 1.6 % |
| **total** | **2.90 GB** | **971 MB** | |

Reopened from the 70 MB xlsx: **0.77 GB** (769.9 MB: the reader's bands were already finished, so the number table round above moved the as-built figure only) (was 2.18 GB; 868.8 MB before the row side tables were split). The workbook as built holds **3 shapes for
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

### Compared with Excel and WPS, 2026-11-18

Every number above is ours. The same workload was put in front of Excel 16.0.20527 (Microsoft 365)
and WPS 12.1.0.25222 through COM: `examples/bench` writes the file (1M x 8, 2M formulas,
70.4 MB), all three open it, edit B1 and recalculate the sheet, and the three agree cell for cell.

| | Excel 16.0.20527 | WPS 12.1.0.25222 | Cella 0.12 |
|---|---:|---:|---:|
| open the 70 MB workbook | 7.9-8.2 s | 10.7 s | **3.3 s** |
| recalculate 2M formulas after the edit | **0.31 s** | 15.3 s | 0.90 s |
| working set after opening | **588 MB** | 987 MB | 1,213 MB |
| private working set | **468 MB** | 864 MB | 1,538 MB |
| with no document open | **~35 MB** | 288 MB | 382 MB |

Against the baselines the document costs 553 MB (Excel), 700 MB (WPS) and 830 MB (Cella) - the
engines are within 1.5x of each other, and our 971 MB live figure (undo 212, journal 15) is in the
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

The calc part is 372.4 MB at 1M × 6: nodes 104.9, dependents 142.6, the area arena 125.0. Audited
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
hot path is what would carry the risk. At two million formulas 372 MB is not where the product stops
working; recorded here so the next reader does not re-run the audit.

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

## Adding a scenario

1. Add a `fn` to `crates/engine/examples/perf.rs` and an entry to its `STEPS` table (id, what it
   measures, function). The id is the budget key: `<area>.<thing>`, e.g. `filter.auto`.
2. Run `cargo xtask perf --update --repeat=3` — the new scenario is recorded; the gate fails until
   it is.
3. Add the row to the table above if it is one a reader should recognise.

The harness stays dependency-free (engine + serde_json only), opens no dialogs, and caps anything
derived from its arguments — the row count at 5,000,000 and `--repeat` at 10.

## Revision history

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
