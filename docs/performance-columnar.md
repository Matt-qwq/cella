# Column storage: what it would buy, what it would cost

An evaluation, not a plan to execute. Written 2026-11-25, after the two storage knives landed and
the cheap variants were measured away (`docs/gaps.md` §48, §50). Numbers in this page are from
`crates/model/examples/read_probe` and `cargo xtask bench-functions` (five passes a row) on the
development machine, which is noisy: treat the ratio, not the millisecond.

## What is measured today

A whole-column formula reads its column through a **row-major** store: each cell costs a chain — the
band, the row, the row's offset in the payload stripe, the payload, the kind, the number.

| read | ns a cell | source |
|---|---:|---|
| one column, folded in the store (`CellStore::fold_column`) | **10.5** | `read_probe` |
| one column, packed walk (`visit_packed`) | 14.3 | `read_probe` |
| a row-wise block of eight columns (`visit_packed`) | **5.0** | `read_probe` |
| a contiguous `Vec<f64>` folded, black-boxed | **2.2** | `read_probe` |
| a row read out of eight contiguous columns | 0.2 | `read_probe` |

The last two are the two ends of a column-major store: **what a column read could become (2.2)** and
**what a row read would become (0.2 for the sweep, because eight sequential streams prefetch)**. The
feared regression on row-wise work does not appear in the synthetic; the win on columnar work is
about **4.8×**.

Attribution of the 10.5 (all measured, `gaps` §50): 2.3 the payload→number chain, 1.4 the kind's run,
1.1 the formula test — and **8–9 ns the row walk itself**. The row walk is not a load chain to
shorten; it is what a row-major store charges to answer a column question.

## What it would buy on the page

At 1M rows, if a column read fell from 10.5 to 2.2 ns a cell (the ceiling; realistically 3–4 with the
fold and the bookkeeping):

| call | today | at the ceiling |
|---|---:|---:|
| `SUM` (two columns) | 20 ms | **~6** |
| `COUNT` / `MAX` / `MIN` / `AVERAGE` (two columns) | 20–21 | **~6–7** |
| `COUNTIF` / `SUMIF` / `AVERAGEIF` | 15–16 | **~8** (the criterion's own work stays) |
| `MEDIAN` | 48 | **~25** (the sort dominates) |
| `SUMPRODUCT` | 21 | **~10** |

The functions already at 15–16 ms are near the store's floor; this is the only way under it.

## What it would cost

**A full column-major store is not a knife; it is a rewrite.** The store's public surface is 70
functions, and its callers reach it through 380 sites: `cells.set` 251, `cells.get` 57, `iter_range`
52, `visit_packed` 18, `visit_values` 5, `put_row` 5 (counted at `1a46743`). Every one of those is
written against `(row, col)`, and the workloads are mixed:

- **row-major today:** the grid's viewport read, the xlsx writer and reader, undo's journal, the
  clipboard, sort/filter's bulk edits, the pivot cache.
- **columnar today:** whole-column formulas — `SUM(A:A)`, `MATCH`'s key column, `COUNTIF`, `MEDIAN`.

A store that is columnar for the second list and row-major for the first is **two layouts and an
invariant between them**, which is where this class of change usually goes wrong (a stale copy shows a
wrong number; that is the worst failure a spreadsheet can have).

## The three shapes

| shape | what it is | win | cost | risk |
|---|---|---|---|---|
| **A. Column-major store** | rebuild `CellStore` around per-column arrays | the full 4.8× | 280+ call sites, weeks, xlsx/undo/paging all move | high — a wrong number is worse than a slow one |
| **B. Per-column sidecar cache** | `store.rs` keeps, per column, a contiguous `Vec<f64>` + presence bitmap, built lazily for whole-column reads and invalidated inside the store's own mutators | the same 4.8× for cached columns (after the first read) | contained to the store: a hook in ~15 mutators, a cache with a size cap, invalidation on every write | medium — it is a cache, so the failure is a stale read; it must be invalidated by *construction* (every write path calls `note_write(col)`) and tested with a fuzz that writes and re-reads |
| **C. Do nothing** | the current numbers are the layout's price | — | — | none |

**B is the only one worth considering now**, because its blast radius is the store's own file: no
caller changes, no new invariant across crates. Its open questions:

1. **Where the cache lives.** `CellStore` is `BTreeMap<u32, Arc<Band>>` with page support
   (`pages`, `page_file`); a cached column must be dropped when its band is paged, when the column map
   shifts (`shift_cols`), when a row is inserted or deleted, and whenever a cell in that column is
   written. All of those live in `store.rs` — good — but "all of them" is the whole correctness
   argument, so it needs one test per mutator, plus a fuzz: random writes to random cells of a cached
   column, then a full compare against an uncached walk.
2. **What the cache holds.** Numbers only (`Vec<f64>` + a bitmap for "not a number") covers `SUM`,
   `COUNT`, `AVERAGE`, `MIN`, `MAX`, `MEDIAN`, `SUMIF`'s target column and `COUNTIF`'s test column, and
   costs 9 bytes a cell (8 + a bit) — 9 MB for a million-row column, with a cap and an LRU. Texts and
   errors fall back to the walk.
3. **When it is built.** On the second whole-column read of a column (so a one-off `SUM(A:A)` does not
   pay 10.5 ms to build a cache it never uses again), or on demand through a store API the aggregates
   ask for.
4. **The pilot.** `SUM`/`COUNT`/`AVERAGE`/`MIN`/`MAX` over one column, cached, measured at 1M rows and
   on `filter.apply`/`struct.deleteColumn` (the rows of the gate that write a lot) before anything else
   is wired.

## Recommendation

- **Do not start A.** Weeks of work, the worst failure mode, and the payoff is 4–14 ms on formulas
  run over a million rows — not what most users are waiting on.
- **B is worth a pilot only if the 15–20 ms aggregate rows are on the critical path for someone.**
  The page says they are the fastest such rows on any spreadsheet we know of; the honest framing is
  that this is an engineering-marvel chase, not a user-facing one.
- **What would change the recommendation:** a real workload where a whole-column aggregate over a
  million rows is interactive (a live dashboard over 1M rows, recalculating on every keystroke). We do
  not have one; if the owner has one, B's pilot is ~a day, and its cache is the part that must be
  proven before it is trusted.

Sources: `docs/gaps.md` §39/§40 (the row walk), §45 (the spill store), §46 (the walk's 62 %), §48
(the stride read), §50 (inline numbers refuted). Instruments: `crates/model/examples/read_probe`,
`crates/engine/examples/fn_probe` (`CELLA_PROFILE=1` splits eval from the spill lay-out).
