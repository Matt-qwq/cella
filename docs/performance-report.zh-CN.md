# 性能报告

> 由 `cargo xtask perf-report` 生成，对象是 Cella 0.13.0，提交 `29a35f71`（2026-11-19）。
> 机器：windows x86_64；rustc 1.97.1 (8bab26f4f 2026-07-14)。所有数字都是同一台空闲机器上的墙钟时间，本机跑动之间波动约 10%，
> 这也是门禁预算取"安静一次的两倍"的原因。方法与口径见 [performance.md](performance.md)。

## 场景：10 万行与 100 万行（release，单次最优）

```text

budgets xtask/perf-budgets.txt (scale 1.00)

scenario                 rows    measured     budget      of
------------------  ---------  ----------  ---------  ------
fill.values            100000      202 ms     400 ms     51%
fill.formulas          100000      150 ms     380 ms     40%
fill.chained           100000      194 ms     540 ms     36%
aggregate              100000       71 ms     200 ms     36%
edit.recalc            100000       91 ms     350 ms     26%
edit.undo              100000      2.1 ms     4.0 ms     52%
sort.desc              100000      385 ms     1.05 s     37%
save.xlsx              100000      190 ms     370 ms     51%
open.xlsx              100000      310 ms     670 ms     46%
rebuild.graph          100000       56 ms     170 ms     33%
open.csv               100000       67 ms     140 ms     48%
fill.values           1000000      2.10 s     4.10 s     51%
fill.formulas         1000000      1.26 s     3.55 s     35%
fill.chained          1000000      1.86 s     5.95 s     31%
aggregate             1000000      740 ms     1.95 s     38%
edit.recalc           1000000      851 ms     3.65 s     23%
edit.undo             1000000       25 ms      54 ms     46%
sort.desc             1000000      4.26 s    13.75 s     31%
save.xlsx             1000000      1.76 s     4.10 s     43%
open.xlsx             1000000      2.90 s     8.05 s     36%
rebuild.graph         1000000      758 ms     2.25 s     34%
open.csv              1000000      594 ms     1.25 s     48%

OK: all 22 scenarios within budget (xtask/perf-budgets.txt)

```

## 活堆内存：100 万行 x 6 列、200 万公式

```text

Cella memory — 1000000 rows x 6 columns

as built (built by the perf scenario's three fills)

  part                                 bytes    share  size
  ---------------------------------------------------  ----------
  sheet Sheet1 / cells             370636740    41.9%  370.6 MB
    band headers + rows             51750000     5.9%  51.8 MB
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
  calc / dependents map             56001160     6.3%  56.0 MB
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
  history / undo                   212319230    24.0%  212.3 MB
  history / redo                           0     0.0%  0 B
  journal                           15251076     1.7%  15.3 MB
                            -------------------------
  TOTAL                            884073538   100.0%  884.1 MB
  formula shapes                3 of 2000000  the trees the formulas share
  undo stack (1 steps)             212319230           retained beyond the live workbook
    AutoFill                       212319230           the newest first
    Sheet1 / stripes        85750004           retained 212316686 · row tables 55999904 · texts 32000000

the same workbook saved as xlsx (70371330 bytes) and opened again

  part                                 bytes    share  size
  ---------------------------------------------------  ----------
  sheet Sheet1 / cells             382181085    55.9%  382.2 MB
    band headers + rows             51874608     7.6%  51.9 MB
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
  calc / dependents map             56001160     8.2%  56.0 MB
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
  history / redo                           0     0.0%  0 B
  journal                           15251146     2.2%  15.3 MB
                            -------------------------
  TOTAL                            683297131   100.0%  683.3 MB
  formula shapes              181 of 2000000  the trees the formulas share

  saved and reopened against as built

  line                                      as built        reopened
  ------------------------------------------------------------------
  cells                                     370.6 MB        382.2 MB  +11.5 MB
  formula text                              133.4 MB        133.4 MB  +215 B
  formula trees                              12.0 MB         12.1 MB  +68.4 kB
  row side tables                            72.0 MB         72.0 MB  +128 B
  text dictionaries                          32.0 MB         43.1 MB  +11.1 MB
  graph nodes                               104.9 MB        104.9 MB  +0 B
  dependents map                             56.0 MB         56.0 MB  +0 B
  name cache                                     0 B             0 B  +0 B
  undo                                      212.3 MB             0 B  -212.3 MB
  saved snapshot                              2.5 kB           952 B  -1.6 kB
  journal                                    15.3 MB         15.3 MB  +70 B
  total                                     884.1 MB        683.3 MB  -200.8 MB


```

## 与 Excel、WPS、上游 GridCraft 的横评

> **Measured** 2026-11-18 on the development machine (Windows 11, AMD Ryzen, 32 GB), release
> builds. Every product ran the same file and the same edit; the answer cell is identical across
> all four. Run-to-run spread on this machine is about 10 %.

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

### The same operations in each engine

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

### What this does and does not say

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
