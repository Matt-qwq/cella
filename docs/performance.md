# Performance

> **Last reviewed:** 2026-11-16 · **Last updated:** 2026-11-16 · **Change:** new (the performance gate: `cargo xtask perf`, `xtask/perf-budgets.txt`, the scenario harness, this page) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

GridCraft is for people who work on real data sets — programmers, analysts, e-commerce operations —
so throughput on a large workbook *is* a product feature. A number someone measured once is
folklore within a month. This page is the number, the harness that produces it, and the gate that
refuses a regression.

The reference workbook throughout: **1,000,000 rows × 6 columns, 72 MB as .xlsx** — the shape of a
real export, not a synthetic grid.

## Scenarios

`crates/engine/examples/perf.rs` runs one session through nine named scenarios, in this order,
because that is how a real session looks (values, formulas over them, edits, sort, save, open).
Each reports one number: its own wall-clock time in ms.

| id | what it does | why it is measured |
|---|---|---|
| `fill.values` | writes rows × 6 values through `range.setValues` in 10,000-row chunks | the paste/import path: JSON decode, cell store packing |
| `fill.formulas` | fills `G = B*C+D` down every row | formula parse, dependency graph build, recalculation of 1M new formulas |
| `fill.chained` | fills a running sum `H = H(n-1)+G(n)` — every row depends on the row above | the worst case for the dependency graph: one chain a million deep |
| `aggregate` | `SUM(G1:G{n})`, `SUMIFS(B:B,C:C,">50")`, `XLOOKUP(...)` | whole-column aggregates and lookups, the queries analysts actually run |
| `edit.recalc` | edits `B1` (every `G`, hence every `H`, depends on it) | incremental recalculation after one keystroke — the number a user feels |
| `edit.undo` | undoes that edit | undo must not be a snapshot of the world |
| `sort.desc` | `data.sortDescending` over `A1:F{n}` by one column | sort is the slowest interactive operation we have (see below) |
| `save.xlsx` | `file.saveBytes` as xlsx | the write path: pack, compress, stream |
| `open.xlsx` | opens those bytes again | the read path plus the full recalculation of every formula |

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
fill.values            100000      351 ms     710 ms     49%
sort.desc             1000000      9.48 s    19.00 s     50%
edit.recalc           1000000      2.29 s     4.60 s     50%
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
| `edit.undo` | 303 ms | 4.03 s |
| `sort.desc` | 715 ms | 9.48 s |
| `save.xlsx` | 341 ms | 4.00 s |
| `open.xlsx` | 684 ms | 8.60 s |

Reading it:

- **Sort and open+recalc are the two slow scenarios** and the two that matter most to a user with a
  big sheet. Everything else is already in the "do it again without thinking" range.
- Undo of a *formula* edit is not free (it recalculates what depends on the cell on the way back),
  so `edit.undo` is measured right after `edit.recalc` rather than at the end of a run.
- The 100k column is there because the two sizes can move in different directions: a change that
  helps at 1M by making an allocation cheaper can still cost at 100k, where the whole workbook fits
  in cache.
- The two tables are separate sources. Best-of-three reports the pass the machine was quietest for,
  which is why `open.xlsx` (the longest scenario, so the one a loaded pass distorts most) comes out
  at 8.6 s here against the 15.3 s of the single-pass ad-hoc example. The budgets are keyed to the
  harness, so compare like with like: `cargo xtask perf` against `xtask/perf-budgets.txt`.

Known hotspots and what is left of them are tracked in [`gaps.md` §2](gaps.md) — this page owns
the measurement, that entry owns the work.

## Adding a scenario

1. Add a `fn` to `crates/engine/examples/perf.rs` and an entry to its `STEPS` table (id, what it
   measures, function). The id is the budget key: `<area>.<thing>`, e.g. `filter.auto`.
2. Run `cargo xtask perf --update --repeat=3` — the new scenario is recorded; the gate fails until
   it is.
3. Add the row to the table above if it is one a reader should recognise.

The harness stays dependency-free (engine + serde_json only), opens no dialogs, and caps anything
derived from its arguments — the row count at 5,000,000 and `--repeat` at 10.

## Revision history

- **2026-11-16:** created. `cargo xtask perf`, the nine named scenarios, `xtask/perf-budgets.txt`
  and the `--json` document the gate parses (schema 1).
