# External benchmark: Cella, Excel, WPS and upstream GridCraft

> **Measured 2026-11-21** on the development machine (Windows 11, AMD Ryzen 7 9700X 8-core, 32 GB),
> release builds, one session, in this order: every product ran the same file and the same edit,
> and all three returned the same answer cell. Medians of five runs (Excel, Cella) and three (WPS,
> which spends ~15 s on the edit); ranges in the notes. Run-to-run spread on this machine is
> about 10 %.

The file is `plan/bench/ref-1m.xlsx`: 1,000,000 rows x 8 columns, 2,000,000 formulas
(`G = B*C+D`, `H = H(n-1)+G(n)`), 70,371,330 bytes, written by Cella. The edit is `B1 = 3`, which
dirties `G1` and the whole `H` chain (about 1,000,000 cells); every product had automatic
calculation on, so the assignment pays for the recalculation. Excel and WPS were driven through
COM; Cella through its control channel (a normal desktop this time — the 11-18 run used a hidden
one, which costs a ~40 ms frame-batched round trip; see performance.md).

| | Excel 16.0 | WPS 12.1 | Cella (this build) |
|---|---:|---:|---:|
| open the 70 MB file | 9.01 s | 11.11 s | **2.61 s** |
| **edit B1 -> the dependent chain** | **0.168 s** | 15.37 s | **0.220 s** |
| working set after opening | **579 MB** | 1,004 MB | 988 MB |
| private working set after opening | **532 MB** | 896 MB | 1,249 MB |
| H1000000 after the edit | 23,999,823,993,691 | the same | the same |

Ranges: Excel open 8.93–9.27 s, edit 0.161–0.235 s (one 5th run at 0.235; the median is 0.168);
WPS open 10.98–11.31 s, edit 15.09–15.60 s; Cella open 2.54–2.64 s, edit 0.219–0.229 s. Memory was
read from the running processes after the document was open (WMI/Get-Process), Excel as
`EXCEL.EXE`, WPS as `et.exe`, Cella as `cella.exe`.

## What this says

- **Cella opens this file 3.5x faster than Excel and 4.3x faster than WPS.** That has been true
  since the read path was rewritten and it is not close.
- **Excel still wins the edit, but by 1.3x, not by 4.4x.** 0.168 s against our 0.220 s, on an
  operation that recalculates a million chained formulas inside the assignment (Excel is
  synchronous here: `CalculationState` is already `xlDone` when `Range.Value = 3` returns, so both
  numbers mean "the chain is up to date"). The 11-18 table below recorded 0.08 s against our
  0.36 s; rounds 26–27 took our side down (an evicted undo step releases on later cheap commands,
  the closure holds a two-formula precedent in a box, a band prices itself from a copy tag, and the
  store compares the packed row before it copies a band), while Excel measured 0.161–0.235 s in
  five runs today on the same protocol. The table above is one session's numbers and is the one to
  quote.
- **Memory is where Excel still wins clearly**: 579 MB of working set against our 988, and 532 MB
  private against 1,249 — on a document that also carries one undo step (~212 MB at 1M rows) and
  the replayable journal (~15 MB). WPS sits between the two on working set and behind both on
  speed.
- Every product agrees on `H1000000`, which is the point of running the same file through all
  three.

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
```

## Not measured

Startup, UI interaction latency, other platforms, and workload variety (pivots, charts, print) are
**not measured here** (see `performance-report.md` for what the gate does cover, and
`performance.md` for the interaction harnesses that sit outside the gate).
