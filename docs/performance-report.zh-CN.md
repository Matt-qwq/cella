# 性能报告

> 由 `cargo xtask perf-report` 生成，对象是 Cella 0.13.0，提交 `55758e2`（2026-11-22）。
> 机器：windows x86_64；rustc 1.97.1 (8bab26f4f 2026-07-14)。所有数字都是同一台空闲机器上的墙钟时间，每个场景取三次中最快的一次
> （本机跑动之间波动约 10%，这也是门禁预算取"安静一次的两倍"的原因）。方法与口径见
> [performance.md](performance.md)。

## 场景：10 万行与 100 万行（release，三次取最优）

```text

budgets xtask/perf-budgets.txt (scale 1.00)

scenario                 rows    measured     budget      of
------------------  ---------  ----------  ---------  ------
fill.values            100000      100 ms     250 ms     40%
fill.formulas          100000      120 ms     290 ms     41%
fill.chained           100000      165 ms     370 ms     45%
fill.autofill          100000       84 ms     190 ms     44%
aggregate              100000       42 ms     110 ms     38%
edit.recalc            100000       61 ms     160 ms     38%
edit.undo              100000      100 us     3.0 ms      3%
f9.calculateNow        100000       76 ms     180 ms     42%
sort.desc              100000      188 ms     580 ms     32%
sort.formulas          100000      260 ms     760 ms     34%
save.xlsx              100000      155 ms     300 ms     52%
open.xlsx              100000      268 ms     520 ms     51%
rebuild.graph          100000       57 ms     110 ms     51%
open.csv               100000       72 ms     130 ms     56%
fill.values           1000000      1.07 s     3.05 s     35%
fill.formulas         1000000      992 ms     2.55 s     39%
fill.chained          1000000      1.49 s     3.55 s     42%
fill.autofill         1000000      387 ms     885 ms     44%
aggregate             1000000      217 ms     1.15 s     19%
edit.recalc           1000000      441 ms     1.45 s     30%
edit.undo             1000000      1.5 ms      52 ms      3%
f9.calculateNow       1000000      638 ms     1.60 s     40%
sort.desc             1000000      1.68 s     6.70 s     25%
sort.formulas         1000000      2.45 s     8.65 s     28%
save.xlsx             1000000      1.63 s     3.40 s     48%
open.xlsx             1000000      2.90 s     5.90 s     49%
rebuild.graph         1000000      755 ms     1.45 s     52%
open.csv              1000000      667 ms     1.25 s     53%
save.repeat            100000      156 ms     370 ms     42%
save.repeat           1000000      1.65 s     3.20 s     52%
mem_drift              100000      3.3 ms      11 ms     30%

OK: all 31 scenarios within budget (xtask/perf-budgets.txt)

```

## 活堆内存：100 万行 x 6 列、200 万公式

```text

Cella memory — 1000000 rows x 6 columns

  deferred release queue: 301.7 MB released before the report below
  (an evicted history step waits for a later cheap command or the host's idle drain; a session
   that stops after a bulk edit would keep it, so the queue line below is zero by construction)

as built (built by the perf scenario's three fills, its release queue drained)

  part                                 bytes    share  size
  ---------------------------------------------------  ----------
  sheet Sheet1 / cells             301692356    38.8%  301.7 MB
    band headers + rows             52250000     6.7%  52.2 MB
    band index                        375168     0.0%  375.2 kB
    bitmaps                          8000000     1.0%  8.0 MB
    payloads                        16000000     2.1%  16.0 MB
    kind runs                        8000000     1.0%  8.0 MB
    style runs                       4000000     0.5%  4.0 MB
    style ids                          62500     0.0%  62.5 kB
    number tables                   33004944     4.2%  33.0 MB
    text dictionaries               32000000     4.1%  32.0 MB
    row side tables                 71999872     9.3%  72.0 MB
    formula text                    63999872     8.2%  64.0 MB
    formula trees                   12000000     1.5%  12.0 MB
  sheet Sheet1 / lines                   192     0.0%  192 B
  sheet Sheet1 / objects                1598     0.0%  1.6 kB
  workbook / styles                     1990     0.0%  2.0 kB
  workbook / names                         0     0.0%  0 B
  workbook / other                       960     0.0%  960 B
  calc / graph nodes               104857928    13.5%  104.9 MB
  calc / precedent areas           124999960    16.1%  125.0 MB
  calc / dependents map             56001416     7.2%  56.0 MB
  calc / range index                       0     0.0%  0 B
  calc / wide range index                  0     0.0%  0 B
  calc / legacy arrays                     0     0.0%  0 B
  calc / flag sets                         0     0.0%  0 B
  calc / unsettled ranges                  0     0.0%  0 B
  calc / graph buffers                    64     0.0%  64 B
  calc / cycles                            0     0.0%  0 B
  calc / areas buffer                     96     0.0%  96 B
  calc / unshared trees                    0     0.0%  0 B
  calc / name cache                        0     0.0%  0 B
  calc / pass (at rest)                    0     0.0%  0 B
    graph dependents lists                 0           many
  history / saved snapshot              2544     0.0%  2.5 kB
  history / undo                   173777590    22.4%  173.8 MB
  history / deferred release               0     0.0%  0 B
  history / redo                           0     0.0%  0 B
  journal                           15251076     2.0%  15.3 MB
                            -------------------------
  TOTAL                            776587770   100.0%  776.6 MB
  formula shapes                3 of 2000000  the trees the formulas share
  undo stack (1 steps)             173777590           retained beyond the live workbook
    AutoFill                       173777590           the newest first
    Sheet1 / stripes        86250004           retained 173775046 · row tables 55999904 · texts 32000000

the same workbook saved as xlsx (70371330 bytes) and opened again

  part                                 bytes    share  size
  ---------------------------------------------------  ----------
  sheet Sheet1 / cells             313236614    51.0%  313.2 MB
    band headers + rows             52374608     8.5%  52.4 MB
    band index                        375168     0.1%  375.2 kB
    bitmaps                          8020768     1.3%  8.0 MB
    payloads                        16041536     2.6%  16.0 MB
    kind runs                        8020768     1.3%  8.0 MB
    style runs                       4010384     0.7%  4.0 MB
    style ids                          62500     0.0%  62.5 kB
    number tables                   33199336     5.4%  33.2 MB
    text dictionaries               43063098     7.0%  43.1 MB
    row side tables                 72000000    11.7%  72.0 MB
    formula text                    64000000    10.4%  64.0 MB
    formula trees                   12068448     2.0%  12.1 MB
  sheet Sheet1 / lines                   192     0.0%  192 B
  sheet Sheet1 / objects                1598     0.0%  1.6 kB
  workbook / styles                     1990     0.0%  2.0 kB
  workbook / names                         0     0.0%  0 B
  workbook / other                       960     0.0%  960 B
  calc / graph nodes               104857928    17.1%  104.9 MB
  calc / precedent areas           124999960    20.3%  125.0 MB
  calc / dependents map             56001416     9.1%  56.0 MB
  calc / range index                       0     0.0%  0 B
  calc / wide range index                  0     0.0%  0 B
  calc / legacy arrays                     0     0.0%  0 B
  calc / flag sets                         0     0.0%  0 B
  calc / unsettled ranges                  0     0.0%  0 B
  calc / graph buffers                    64     0.0%  64 B
  calc / cycles                            0     0.0%  0 B
  calc / areas buffer                     96     0.0%  96 B
  calc / unshared trees                    0     0.0%  0 B
  calc / name cache                        0     0.0%  0 B
  calc / pass (at rest)                    0     0.0%  0 B
    graph dependents lists                 0           many
  history / saved snapshot               952     0.0%  952 B
  history / undo                           0     0.0%  0 B
  history / deferred release               0     0.0%  0 B
  history / redo                           0     0.0%  0 B
  journal                           15251146     2.5%  15.3 MB
                            -------------------------
  TOTAL                            614352916   100.0%  614.4 MB
  formula shapes              181 of 2000000  the trees the formulas share

  saved and reopened against as built

  line                                      as built        reopened
  ------------------------------------------------------------------
  cells                                     301.7 MB        313.2 MB  +11.5 MB
  formula text                               64.0 MB         64.0 MB  +128 B
  formula trees                              12.0 MB         12.1 MB  +68.4 kB
  row side tables                            72.0 MB         72.0 MB  +128 B
  text dictionaries                          32.0 MB         43.1 MB  +11.1 MB
  graph nodes                               104.9 MB        104.9 MB  +0 B
  dependents map                             56.0 MB         56.0 MB  +0 B
  name cache                                     0 B             0 B  +0 B
  undo                                      173.8 MB             0 B  -173.8 MB
  deferred release                               0 B             0 B  +0 B
  saved snapshot                              2.5 kB           952 B  -1.6 kB
  journal                                    15.3 MB         15.3 MB  +70 B
  total                                     776.6 MB        614.4 MB  -162.2 MB


```

## 与 Excel、WPS、上游 GridCraft 的横评

> **Measured 2026-11-22** on the development machine (Windows 11, AMD Ryzen 7 9700X 8-core, 32 GB),
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
| open the 70 MB file | 8.67 s | 10.86 s | **2.69 s** |
| **edit B1 -> the dependent chain** | **0.236 s** | 16.36 s | 0.270 s |
| **full recalculation (F9), 2M formulas** | **0.072 s** | — | 0.40 s |
| working set after opening | **580 MB** | 1,001 MB | 879 MB |
| private working set after opening | **530 MB** | 894 MB | 1,137 MB |
| H1000000 after the edit | 23,999,823,993,691 | the same | the same |

Ranges: Excel open 8.62–8.72 s, edit 0.144–0.242 s, F9 0.062–0.082 s (median 0.072); WPS open
10.84–11.15 s, edit 15.23–16.92 s (WPS was not asked for F9 — its edit already takes 15 s); Cella
open 2.63–2.73 s, edit 0.253–0.276 s, F9 **0.396–0.438 s** (median 0.401 — re-measured after
round 36, which removed the graph rebuild from the command; it was 1.03–1.07 s before). Memory was
read from the running processes after the document was open (WMI/Get-Process), Excel as
`EXCEL.EXE`, WPS as `et.exe`, Cella as `cella.exe`. The F9 pair was measured interleaved
(Excel, Cella, Excel, Cella) so both sides saw the same machine; the removal itself is certified by
a four-round alternating A/B on this file — **1.046 s → 0.406 s, −61.2 %, faster in 12 of 12
runs**.

### What this says

- **Cella opens this file 3.2x faster than Excel and 4x faster than WPS.** That has been true
  since the read path was rewritten and it is not close.
- **Excel still wins the edit, but by 1.1x.** 0.236 s against our 0.270 s, on an operation that
  recalculates a million chained formulas inside the assignment (Excel is synchronous here:
  `CalculationState` is already `xlDone` when `Range.Value = 3` returns, so both numbers mean "the
  chain is up to date").
- **Full recalculation: 0.40 s against Excel's 0.072 s (5.6x), down from 1.05 s (16x).** Round 36
  stopped `formulas.calculateNow` rebuilding the dependency graph — every edit already maintains
  it, and the rebuild was 0.69 s of a 1.05 s F9 in the gate. What is left is the evaluation of
  2,000,000 formulas, a million of them one chain deep: ~0.40 s, and the chain is why it does not
  parallelise. Excel keeps its tree across a recalculation too, so the remaining 5.6x is
  per-formula and per-command cost — round 37's subject, not a rebuild question (the evaluator was
  already refuted as the target in round 26).
- **Memory**: 879 MB of working set against Excel's 580, and 1,137 MB private against 530 — 1.5x and
  2.1x, where the 11-21 build was 988 / 1,249. The round-34 change (the formula text is printed, not
  stored) took ~110 MB out of both; what remains is the undo step (~174 MB at 1M rows), the
  replayable journal (~15 MB) and the calculation graph (~286 MB), each of which buys something.
- Every product agrees on `H1000000`, which is the point of running the same file through all
  three.

### Previous measurement (2026-11-21)

Kept because the older documents cite it. Same protocol, same file; the memory numbers are the
build before round 34.

| | Excel 16.0 | WPS 12.1 | Cella 0.13.0+ |
|---|---:|---:|---:|
| open the 70 MB file | 9.01 s | 11.11 s | 2.61 s |
| edit B1 -> the dependent chain | 0.168 s | 15.37 s | 0.220 s |
| working set after opening | 579 MB | 1,004 MB | 988 MB |
| private working set after opening | 532 MB | 896 MB | 1,249 MB |

### Previous measurement (2026-11-18)

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

### The same operations in each engine (2026-11-18, not re-run)

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

### Reproduce

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

### Not measured

Startup, UI interaction latency, other platforms, and workload variety (pivots, charts, print) are
**not measured here** (see `performance-report.md` for what the gate does cover, and
`performance.md` for the interaction harnesses that sit outside the gate).


## 没有测量的项目

下面没有数字，代表**没有测**，不是 0：

- **启动时间**：冷启动、热启动，带文档 / 不带文档。
- **界面交互延迟**：按键到单元格重绘、100 万行滚动、打开对话框、图表重绘。只有帧绘制（约 1.4 ms @144 Hz）有数字。
- **平台**：macOS、Linux、wasm 都没有测；以上全部来自一台 Windows 机器。
- **对 Excel 的多负载对比**：透视表、图表、打印/PDF、CSV 导入、查询与 Parquet、多工作表、1 亿单元格分页。
- **长时间运行内存**：没有一小时级别的漂移观察。

## 复现

```bash
cargo xtask perf                              # 场景门，对照 xtask/perf-budgets.txt
cargo run --release -p cella-engine --example memory -- 1000000
cargo xtask perf-report                       # 重新生成本页与英文版
```
