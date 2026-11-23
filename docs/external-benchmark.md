# External benchmark: Cella, Excel, WPS and upstream GridCraft

> **Measured 2026-11-23** on the development machine (Windows 11, AMD Ryzen 7 9700X 8-core, 32 GB),
> release builds, one session, in this order: every product ran the same file, the same edit and the
> same full recalculation, and all three agreed on the answer cell. Three runs each; medians below,
> ranges in the notes. Run-to-run spread on this machine is about 10 %.

The file is `plan/bench/ref-1m.xlsx`: 1,000,000 rows x 8 columns, 2,000,000 formulas
(`G = B*C+D`, `H = H(n-1)+G(n)`), 70,371,330 bytes, written by Cella. The edit is `B1 = 3`, which
dirties `G1` and the whole `H` chain (about 1,000,000 cells); every product had automatic
calculation on, so the assignment pays for the recalculation. The full recalculation is F9 —
Cella's `formulas.calculateNow`, Excel's `CalculateFull` — on the freshly opened file. Excel and WPS
were driven through COM; Cella through its control channel (`plan/bench/external_ab.ps1` and
`plan/bench/f9_ab.ps1`).

| | Excel 16.0 | WPS 12.1 | Cella (this build) |
|---|---:|---:|---:|
| open the 70 MB file | 8.64 s | 11.05 s | **2.46 s** |
| **edit B1 -> the dependent chain** | 0.171 s | 14.94 s | **0.157 s** |
| **full recalculation (F9), 2M formulas** | **0.073 s** | — | 0.307 s |
| working set after opening | **577 MB** | 981 MB | 857 MB |
| private working set after opening | **518 MB** | 873 MB | 1,049 MB |
| H1000000 after the edit | 23,999,823,993,691 | the same | the same |

Ranges: Excel open 8.63–8.65 s, edit 0.160–0.192 s, F9 0.064–0.087 s; WPS open
10.89–11.26 s, edit 14.67–15.20 s (WPS was not asked for F9 — its edit already takes 15 s); Cella
open 2.43–2.49 s, edit 0.154–0.160 s, F9 **0.304–0.309 s** (the graph rebuild left the command in
round 36 — 1.046 → 0.406 s, certified on this file — and round 37's chain cursor took the
evaluation down again). Memory was read from the running processes after the document was open
(WMI/Get-Process), Excel as `EXCEL.EXE`, WPS as `et.exe`, Cella as `cella.exe`. The F9 pair was
measured interleaved (Excel, Cella, Excel, Cella) so both sides saw the same machine.

## The four operations round 41 added (2026-11-23, one pass each)

The command catalogue put Find/Replace, the AutoFilter and a six-million-cell paste at the top of
what a session spends, so they became gate scenarios and were replayed in the other two products
through COM (`plan/bench/gate_ext.ps1 -Only filter.apply,filter.clear,find.replaceAll,paste.block`).
**One pass each** (the table above is three passes and a median); the same 70 MB file, the same
machine, in this order — open, apply, clear, replace, paste:

| operation, 1M rows | Excel 16.0 | WPS 12.1 | Cella |
|---|---:|---:|---:|
| **apply an AutoFilter to the body** | 0.059 s | 0.091 s | **0.0008 s** |
| **clear it** | 0.007 s | 0.007 s | 0.0074 s |
| **Replace All (`Item` -> `Product`, every used cell)** | 13.512 s | 6.886 s | **4.48 s** |
| **paste 6M cells (copy `A1:F1000000`, paste at `L1`)** | 6.947 s | 3.881 s | **1.06 s** |
| open the file (control, this session) | 9.4 s | 12.0 s | — |

- **Applying a filter is not the same operation in the three products, and the numbers say so.**
  Cella's `data.filter` marks the range as filtered and hides rows lazily (860 µs); Excel and WPS
  build the filter structure and the dropdown state up front (59 ms and 91 ms). Both are honest for
  "select the body and press Filter" — the 70x is a design difference, not a speed one, and the one
  that matters to a person is the 7.4 ms to clear it, where all three agree.
- **Replace All: Cella is 3x Excel and 1.5x WPS.** A million scans and a million rewrites each;
  Excel's 13.5 s is the number that makes "find and replace in a big sheet" feel broken.
- **Paste 6M cells: Cella is 6.5x Excel and 3.7x WPS.** (Cella's block carries values without styles
  here — the workbook it runs on came back from a CSV; `paste_bench` measures the styled shape at
  1.3 s.)

## The second batch round 43 added (2026-11-23, one pass each)

A column edit on the other axis, and a conditional-format rule over six million cells. Same protocol
as above: same file, same machine, one pass each, in this order — open, insert a column at C, delete
it, apply a data bar to `A1:F1000001`:

| operation, 1M rows | Excel 16.0 | WPS 12.1 | Cella |
|---|---:|---:|---:|
| **insert a column at C** (shift 1M rows right) | 0.941 s | 0.497 s | **0.017 s** |
| **delete it again** | 1.365 s | 0.452 s | **0.013 s** |
| **apply a data bar over 6M cells** | 0.165 s | 0.035 s | **0.0011 s** |
| open the file (control, this session) | 13.5 s | 12.3 s | — |

- **A column insert/delete is 29–105× cheaper here**, and the reason is in the store: Cella keeps a
  sheet's cells in row bands with a column offset, so moving a column is bookkeeping, while both
  other products rewrite a million shifted cells. This is the one place in the comparison where the
  data structure, not the code, is the answer.
- **The data bar is not the same operation.** Cella stores the rule (1.1 ms) and builds its
  statistics when the grid first paints it — the UI's ruler, in
  [performance-ui.md](performance-ui.md). Excel's 165 ms and WPS's 35 ms include their own
  structure build. The honest reading is "the rule costs Cella nothing measurable; what it costs to
  *paint* is not in this table".
- **Not replayed:** `pivot.refresh` (each product builds a PivotTable through a different API, so the
  comparable operation is Insert PivotTable, not Refresh — Cella's own row is the engine's) and
  `print.pdf` (pagination, fonts and page caps differ by product; the 1M row export stops at
  Cella's 2,000-page cap).

## What this says

- **Cella opens this file 3.5x faster than Excel and 4.5x faster than WPS.** That has been true
  since the read path was rewritten and it is not close.
- **Cella's edit is under Excel's for the first time: 0.157 s against 0.171 s.** Both recalculate
  the million-deep `H` chain inside the assignment (Excel is synchronous here:
  `CalculationState` is already `xlDone` when `Range.Value = 3` returns, so both numbers mean "the
  chain is up to date").
- **Full recalculation: 0.307 s against Excel's 0.073 s (4.2x), down from 1.05 s (16x) and 0.40 s
  (5.6x) in the two rounds before.** Round 36
  stopped `formulas.calculateNow` rebuilding the dependency graph — every edit already maintains
  it, and the rebuild was 0.69 s of a 1.05 s F9 in the gate. What is left is the evaluation of
  2,000,000 formulas, a million of them one chain deep: ~0.31 s, and the chain is why it does not
  parallelise. Excel keeps its tree across a recalculation too, so the remaining 5.6x is
  per-formula and per-command cost — round 37's subject, not a rebuild question (the evaluator was
  already refuted as the target in round 26).
  The 0.270 s of the 11-22 session is not attributed to one round here: this file is not the gate's
  workbook, and the gate's own paired A/B is what certifies a change (its `edit.recalc` went
  693 → 594 ms with round 37's chain cursor, 344 → 283 ms with round 39's lookup).
- **Memory**: 857 MB of working set against Excel's 577, and 1,049 MB private against 518 — 1.5x and
  2.0x, where the 11-22 build was 879 / 1,137. The round-34 change (the formula text is printed, not
  stored) took ~110 MB out of both; what remains is the undo step (~174 MB at 1M rows), the
  replayable journal (~15 MB) and the calculation graph (~286 MB), each of which buys something.
- Every product agrees on `H1000000`, which is the point of running the same file through all
  three.

## Previous measurement (2026-11-21)

Kept because the older documents cite it. Same protocol, same file; the memory numbers are the
build before round 34.

| | Excel 16.0 | WPS 12.1 | Cella 0.13.0+ |
|---|---:|---:|---:|
| open the 70 MB file | 9.01 s | 11.11 s | 2.61 s |
| edit B1 -> the dependent chain | 0.168 s | 15.37 s | 0.220 s |
| working set after opening | 579 MB | 1,004 MB | 988 MB |
| private working set after opening | 532 MB | 896 MB | 1,249 MB |

## Previous measurement (2026-11-18)

Kept because it is what the older documents cite, and because the difference is instructive: the
same protocol produced a very different Excel edit time.

| | Excel 16.0.20527 | WPS 12.1.0.25222 | upstream GridCraft v0.19.1 | Cella 0.13.0 |
|---|---:|---:|---:|---:|
| open the 70 MB file | 7.8 s | 11.7 s | 21.4 s (open + full recalc) | **3.1 s** (3.33 s with a full recalc after it) |
| full recalculation of 2M formulas | **0.31 s** | 16.9 s | included above | 1.30 s |
| edit B1 -> the dependent chain | **0.08 s** | 16.9 s | 3.64 s | 0.36 s |
| working set after opening | **588 MB** | 987 MB | n/a (engine only) | 996 MB |
| private working set after opening | **468 MB** | 864 MB | n/a | 1,341 MB |
| idle, no document | **~35 MB** | 288 MB | n/a | 196 MB |
| H1000000 after the edit | 23,999,823,993,691 | the same | the same | the same |

Two runs each; the runs differed as follows (Excel open 7.9/7.71 s, edit 0.08/0.07 s; WPS open
11.3/12.14 s, edit 17.19/16.6 s; Cella open 2.88/3.30 s, edit 0.29/0.42 s).

## The same operations in each engine (2026-11-18, not re-run)

Upstream GridCraft v0.19.1 cannot be driven through the control channel here (its app opens a DX12
window and the control port dies on a hidden desktop), so it ran **its own engine benchmark
script** (`crates/engine/examples/perf.rs`, the direct ancestor of ours) on the same 1,000,000
rows, while Cella ran the equivalent operations in its gate harness. The harness mechanics are not
identical — upstream sets one row per `range.setValues`, ours chunks 10,000 — so read these as
engine-scale ratios, not lab-grade totals. **Nothing in this table was re-measured on 2026-11-21**;
it is the 11-18 session's record.

| operation, 1M rows | upstream v0.19.1 | Cella 0.13.0 | ratio |
|---|---:|---:|---:|
| fill `G = B*C+D` | 5.48 s | **1.43 s** | 3.8x |
| fill the chained running sum | 5.21 s | **1.95 s** | 2.7x |
| three aggregates (`SUM`, `SUMIFS`, `XLOOKUP`) | 0.99 s | **0.77 s** | 1.3x |
| edit B1 -> recalc ~1,000,000 cells | 3.64 s | **0.95 s** | 3.8x |
| sort 1M rows | 11.19 s | **4.88 s** | 2.3x |
| save xlsx | 7.65 s (64.9 MB) | **1.91 s** (70.4 MB) | 4.0x |

Upstream v0.19.1 has moved on since this repository forked at v0.11.1, and the comparison above is
not a score against it: it was never tracked as a target. It is here so a reader can see that this
line's own work (the query pipeline, the data model, the memory round, the recalculation round) did
not leave the core slow.

## Reproduce

```powershell
# every product, one file, one edit (prints product,open_s,edit_s,h1000000,ws_mb,private_mb,ok)
powershell -File plan/bench/external_ab.ps1 -Product excel -Path plan/bench/ref-1m.xlsx
powershell -File plan/bench/external_ab.ps1 -Product wps   -Path plan/bench/ref-1m.xlsx
powershell -File plan/bench/external_ab.ps1 -Product cella -Path plan/bench/ref-1m.xlsx

# F9 alone (prints product,run,seconds,ok), the gate's operations replayed in Excel/WPS,
# and the whole report in one command:
powershell -File plan/bench/f9_ab.ps1 -Product cella -Path plan/bench/ref-1m.xlsx -Repeat 3
powershell -File plan/bench/gate_ext.ps1 -Product excel -Repeat 3
powershell -File plan/bench/report.ps1
```

`gate_ext.ps1` replays the gate's ten mappable operations (bulk write, formula fill, chained
fill, autofill, three aggregates, one edit, both sorts, save, open) through Excel's COM so the
report can put Cella's gate numbers beside Excel's. `rebuild.graph`, `mem_drift`, `save.repeat`,
`edit.undo` and `open.csv` have no comparable external operation; WPS did not finish the 1M bulk
write in fifteen minutes, so it appears in the open/edit/memory tables only.

`report.ps1` waits for an idle machine, runs `cargo xtask perf-report` (the 31-scenario gate and
the live heap), then open/edit/memory and F9 for every product three times, keeps only the final
numbers, and compiles a single-file HTML report (the local specboard compiler) to
`plan/bench/f9-board/out/report/<date>_v<N>/index.html` — double-click it, no network needed.

## Not measured

Startup, UI interaction latency, other platforms, and workload variety (pivots, charts, print) are
**not measured here** (see `performance-report.md` for what the gate does cover, and
`performance.md` for the interaction harnesses that sit outside the gate).
