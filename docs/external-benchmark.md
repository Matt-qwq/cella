# External benchmark: Cella, Excel, WPS and upstream GridCraft

> **Measured** 2026-11-18 on the development machine (Windows 11, AMD Ryzen, 32 GB), release
> builds. Every product ran the same file and the same edit; the answer cell is identical across
> all four. Run-to-run spread on this machine is about 10 %.

> **Update 2026-11-21 — this table predates rounds 26–27.** The Cella column was measured on
> 2026-11-18 at 0.13.0. Two rounds since then changed the machinery under exactly these
> operations, and every change was certified by an eight-pair sign test
> (`plan/ab-r27-*.csv`, `plan/ab-r27-paste-*.csv`): an evicted undo step releases on later cheap
> commands (round 26); the closure keeps a two-formula precedent in a box, so the 2M-dirty edit
> went **866 → 707 ms (−18.4 %)**; a band prices a snapshot from its copy tag instead of a per-row
> walk (**−7.2 % built**); the store compares the packed row before copying a band, finds a row
> with one bit probe and stops double-probing in `set` (**−20.1 % built, −21.4 % reopened**); and
> the same knife took **a 6M-cell paste from 2,011 to 1,300 ms (−35.4 %)**. In the gate's own
> harness the 1M chained `edit.recalc` now runs **725 ms** against its 1.45 s budget.
>
> **What has not changed:** Excel's 0.08 s for this edit has not been beaten, and the external
> columns — and the Cella column itself — have **not** been re-measured together since 2026-11-18.
> The gate's synthetic 1M edit is now 251 ms on our side, but that is a different workload from
> the reference file, so it is a direction, not a ratio. Until the whole table is re-run in one
> session with Excel and WPS, read the numbers above as a dated measurement, not a current claim.

The file is `plan/bench/ref-1m.xlsx`: 1,000,000 rows x 8 columns, 2,000,000 formulas
(`G = B*C+D`, `H = H(n-1)+G(n)`), 70,371,330 bytes, written by Cella. The edit is `B1 = 3`, which
dirties `G1` and the whole `H` chain (about 1,000,000 cells); every product had automatic
calculation on, so the assignment pays for the recalculation.

| | Excel 16.0.20527 | WPS 12.1.0.25222 | upstream GridCraft v0.19.1 | Cella 0.13.0 |
|---|---:|---:|---:|---:|
| open the 70 MB file | 7.8 s | 11.7 s | 21.4 s (open + full recalc) | **3.1 s** (3.33 s with a full recalc after it) |
| full recalculation of 2M formulas | **0.31 s** | 16.9 s | included above | 1.30 s |
| **edit B1 -> the dependent chain** | **0.08 s** | 16.9 s | 3.64 s | 0.36 s |
| working set after opening | **588 MB** | 987 MB | n/a (engine only) | 996 MB |
| private working set after opening | **468 MB** | 864 MB | n/a | 1,341 MB |
| idle, no document | **~35 MB** | 288 MB | n/a | 196 MB |
| H1000000 after the edit | 23,999,823,993,691 | the same | the same | the same |

Two runs each, and the table quotes both where they differ (Excel open 7.9/7.71 s, edit
0.08/0.07 s; WPS open 11.3/12.14 s, edit 17.19/16.6 s; Cella open 2.88/3.30 s, edit 0.29/0.42 s).
Memory was read from the running process (WMI) after the document was open; the Excel and WPS
figures are from the same day and file.

## The same operations in each engine

Excel and WPS were driven through COM; Cella through its control channel on a hidden desktop (no
window is put in front of anyone). Upstream GridCraft v0.19.1 cannot be driven that way here (its
app opens a DX12 window, and the control port dies on a hidden desktop), so it ran **its own
engine benchmark script** (`crates/engine/examples/perf.rs`, the direct ancestor of ours) on the
same 1,000,000 rows. Cella ran the equivalent operations in its gate harness, which builds the
same three columns. The harness mechanics are not identical - upstream sets one row per
`range.setValues`, ours chunks 10,000 - so read these as engine-scale ratios, not lab-grade totals:

| operation, 1M rows | upstream v0.19.1 | Cella 0.13.0 | ratio |
|---|---:|---:|---:|
| fill `G = B*C+D` | 5.48 s | **1.43 s** | 3.8x |
| fill the chained running sum | 5.21 s | **1.95 s** | 2.7x |
| three aggregates (`SUM`, `SUMIFS`, `XLOOKUP`) | 0.99 s | **0.77 s** | 1.3x |
| edit B1 -> recalc ~1,000,000 cells | 3.64 s | **0.95 s** | 3.8x |
| sort 1M rows | 11.19 s | **4.88 s** | 2.3x |
| save xlsx | 7.65 s (64.9 MB) | **1.91 s** (70.4 MB) | 4.0x |

## What this does and does not say

- Cella opens this file 2.5x faster than Excel and 3.8x faster than WPS, recalculates the chain
  4.4x faster than upstream and 47x faster than WPS, and holds 996 MB of working set against
  Excel's 588 and WPS's 987 - 1.3x behind the leader, on a document that also carries the undo
  step (212 MB) and the replayable journal (15 MB).
- **Excel still wins the one thing a chain edit measures**: 0.08 s against our 0.36 s, 4.4x. That
  is the honest remaining gap; it is the whole reason the evaluator is the next work item.
- Upstream v0.19.1 has moved on since this repository forked at v0.11.1, and the comparison above
  is not a score against it: it was never tracked as a target. It is here so a reader can see that
  this line's own work (the query pipeline, the data model, the memory round, the recalculation
  round) did not leave the core slow.
- Startup, UI interaction latency, other platforms and workload variety (pivots, charts, print)
  are **not measured** (see `performance-report.md`).
