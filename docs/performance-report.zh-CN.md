# 性能报告

> 由 `cargo xtask perf-report` 生成，对象是 Cella 0.13.0，提交 `65bcef4`（2026-11-21）。
> 机器：windows x86_64；rustc 1.97.1 (8bab26f4f 2026-07-14)。所有数字都是同一台空闲机器上的墙钟时间，本机跑动之间波动约 10%，
> 这也是门禁预算取"安静一次的两倍"的原因。方法与口径见 [performance.md](performance.md)。

## 场景：10 万行与 100 万行（release，单次最优）

```text

budgets xtask/perf-budgets.txt (scale 1.00)

scenario                 rows    measured     budget      of
------------------  ---------  ----------  ---------  ------
fill.values            100000      159 ms     250 ms     64%
fill.formulas          100000      170 ms     290 ms     59%
fill.chained           100000      219 ms     370 ms     59%
fill.autofill          100000       98 ms     190 ms     52%
aggregate              100000       68 ms     110 ms     62%
edit.recalc            100000       95 ms     160 ms     59%
edit.undo              100000      200 us     3.0 ms      7%
sort.desc              100000      256 ms     580 ms     44%
sort.formulas          100000      392 ms     760 ms     52%
save.xlsx              100000      195 ms     300 ms     65%
open.xlsx              100000      407 ms     520 ms     78%
rebuild.graph          100000       68 ms     110 ms     62%
open.csv               100000       90 ms     130 ms     69%
fill.values           1000000      1.51 s     3.05 s     50%
fill.formulas         1000000      1.39 s     2.55 s     55%
fill.chained          1000000      1.96 s     3.55 s     55%
fill.autofill         1000000      422 ms     885 ms     48%
aggregate             1000000      414 ms     1.15 s     36%
edit.recalc           1000000      663 ms     1.45 s     46%
edit.undo             1000000      2.1 ms      52 ms      4%
sort.desc             1000000      2.27 s     6.70 s     34%
sort.formulas         1000000      3.88 s     8.65 s     45%
save.xlsx             1000000      3.39 s     3.40 s    100%
open.xlsx             1000000      4.41 s     5.90 s     75%
rebuild.graph         1000000      940 ms     1.45 s     65%
open.csv              1000000      775 ms     1.25 s     62%
save.repeat            100000      189 ms     370 ms     51%
save.repeat           1000000      2.28 s     3.20 s     71%
mem_drift              100000      4.0 ms      11 ms     36%

OK: all 29 scenarios within budget (xtask/perf-budgets.txt)

```

## 活堆内存：100 万行 x 6 列、200 万公式

```text

Cella memory — 1000000 rows x 6 columns

  deferred release queue: 340.4 MB released before the report below
  (an evicted history step waits for a later cheap command or the host's idle drain; a session
   that stops after a bulk edit would keep it, so the queue line below is zero by construction)

as built (built by the perf scenario's three fills, its release queue drained)

  part                                 bytes    share  size
  ---------------------------------------------------  ----------
  sheet Sheet1 / cells             371136740    42.0%  371.1 MB
    band headers + rows             52250000     5.9%  52.2 MB
    band index                        375168     0.0%  375.2 kB
    bitmaps                          8000000     0.9%  8.0 MB
    payloads                        16000000     1.8%  16.0 MB
    kind runs                        8000000     0.9%  8.0 MB
    style runs                       4000000     0.5%  4.0 MB
    style ids                          62500     0.0%  62.5 kB
    number tables                   33004944     3.7%  33.0 MB
    text dictionaries               32000000     3.6%  32.0 MB
    row side tables                 71999872     8.1%  72.0 MB
    formula text                   133444256    15.1%  133.4 MB
    formula trees                   12000000     1.4%  12.0 MB
  sheet Sheet1 / lines                   192     0.0%  192 B
  sheet Sheet1 / objects                1598     0.0%  1.6 kB
  workbook / styles                     1990     0.0%  2.0 kB
  workbook / names                         0     0.0%  0 B
  workbook / other                       960     0.0%  960 B
  calc / graph nodes               104857928    11.9%  104.9 MB
  calc / precedent areas           124999960    14.1%  125.0 MB
  calc / dependents map             56001416     6.3%  56.0 MB
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
  history / undo                   212444230    24.0%  212.4 MB
  history / deferred release               0     0.0%  0 B
  history / redo                           0     0.0%  0 B
  journal                           15251076     1.7%  15.3 MB
                            -------------------------
  TOTAL                            884698794   100.0%  884.7 MB
  formula shapes                3 of 2000000  the trees the formulas share
  undo stack (1 steps)             212444230           retained beyond the live workbook
    AutoFill                       212444230           the newest first
    Sheet1 / stripes        86250004           retained 212441686 · row tables 55999904 · texts 32000000

the same workbook saved as xlsx (70371330 bytes) and opened again

  part                                 bytes    share  size
  ---------------------------------------------------  ----------
  sheet Sheet1 / cells             382681085    56.0%  382.7 MB
    band headers + rows             52374608     7.7%  52.4 MB
    band index                        375168     0.1%  375.2 kB
    bitmaps                          8020768     1.2%  8.0 MB
    payloads                        16041536     2.3%  16.0 MB
    kind runs                        8020768     1.2%  8.0 MB
    style runs                       4010384     0.6%  4.0 MB
    style ids                          62500     0.0%  62.5 kB
    number tables                   33199336     4.9%  33.2 MB
    text dictionaries               43063098     6.3%  43.1 MB
    row side tables                 72000000    10.5%  72.0 MB
    formula text                   133444471    19.5%  133.4 MB
    formula trees                   12068448     1.8%  12.1 MB
  sheet Sheet1 / lines                   192     0.0%  192 B
  sheet Sheet1 / objects                1598     0.0%  1.6 kB
  workbook / styles                     1990     0.0%  2.0 kB
  workbook / names                         0     0.0%  0 B
  workbook / other                       960     0.0%  960 B
  calc / graph nodes               104857928    15.3%  104.9 MB
  calc / precedent areas           124999960    18.3%  125.0 MB
  calc / dependents map             56001416     8.2%  56.0 MB
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
  journal                           15251146     2.2%  15.3 MB
                            -------------------------
  TOTAL                            683797387   100.0%  683.8 MB
  formula shapes              181 of 2000000  the trees the formulas share

  saved and reopened against as built

  line                                      as built        reopened
  ------------------------------------------------------------------
  cells                                     371.1 MB        382.7 MB  +11.5 MB
  formula text                              133.4 MB        133.4 MB  +215 B
  formula trees                              12.0 MB         12.1 MB  +68.4 kB
  row side tables                            72.0 MB         72.0 MB  +128 B
  text dictionaries                          32.0 MB         43.1 MB  +11.1 MB
  graph nodes                               104.9 MB        104.9 MB  +0 B
  dependents map                             56.0 MB         56.0 MB  +0 B
  name cache                                     0 B             0 B  +0 B
  undo                                      212.4 MB             0 B  -212.4 MB
  deferred release                               0 B             0 B  +0 B
  saved snapshot                              2.5 kB           952 B  -1.6 kB
  journal                                    15.3 MB         15.3 MB  +70 B
  total                                     884.7 MB        683.8 MB  -200.9 MB


```

## 与 Excel、WPS、上游 GridCraft 的横评

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

### What this says

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
```

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
