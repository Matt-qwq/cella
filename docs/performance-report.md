# Performance report

> **Generated** by `cargo xtask perf-report` for Cella 0.13.0 at `201d2d41` (2026-11-18).
> Machine: windows x86_64; rustc 1.97.1 (8bab26f4f 2026-07-14). Every number is wall clock on one idle machine, and this
> machine's run-to-run spread is about 10 %, which is why a gate budget is twice a quiet pass.
> Method and caveats: [performance.md](performance.md).

## Scenarios, 100,000 and 1,000,000 rows (release, best of one pass)

```text

budgets D:\pycharm_projects\cella\xtask/perf-budgets.txt (scale 1.00)

scenario                 rows    measured     budget      of
------------------  ---------  ----------  ---------  ------
fill.values            100000      213 ms     400 ms     53%
fill.formulas          100000      160 ms     380 ms     42%
fill.chained           100000      215 ms     540 ms     40%
aggregate              100000       71 ms     200 ms     35%
edit.recalc            100000      102 ms     350 ms     29%
edit.undo              100000      1.9 ms     4.0 ms     48%
sort.desc              100000      391 ms     1.05 s     37%
save.xlsx              100000      165 ms     370 ms     45%
open.xlsx              100000      309 ms     670 ms     46%
rebuild.graph          100000       59 ms     170 ms     35%
open.csv               100000       64 ms     140 ms     46%
fill.values           1000000      2.46 s     4.10 s     60%
fill.formulas         1000000      1.43 s     3.55 s     40%
fill.chained          1000000      1.95 s     5.95 s     33%
aggregate             1000000      768 ms     1.95 s     39%
edit.recalc           1000000      946 ms     3.65 s     26%
edit.undo             1000000       30 ms      54 ms     55%
sort.desc             1000000      4.88 s    13.75 s     35%
save.xlsx             1000000      1.91 s     4.10 s     47%
open.xlsx             1000000      3.51 s     8.05 s     44%
rebuild.graph         1000000      790 ms     2.25 s     35%
open.csv              1000000      626 ms     1.25 s     50%

OK: all 22 scenarios within budget (D:\pycharm_projects\cella\xtask/perf-budgets.txt)

```

## Live heap, 1M rows x 6 columns with 2M formulas

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

## Measured against Excel and WPS

The external comparison - the same 70 MB file opened and recalculated by Excel 16.0.20527 and WPS
12.1.0.25222, with working set, private bytes and idle - is in
[performance.md](performance.md#compared-with-excel-and-wps-2026-11-18), with its method and
caveats. It is not generated here because it drives two other applications through COM.

## Not measured

No number below means no measurement, not a zero:

- **Startup**: cold and warm, with and without a document.
- **UI interaction latency**: keystroke to repainted cell, scrolling a million rows, opening a
  dialog, redrawing a chart. Only the frame paint (about 1.4 ms at 144 Hz) has a number.
- **Platforms**: macOS, Linux and wasm are unmeasured; everything above is one Windows machine.
- **Workload variety against Excel**: pivots, charts, print/PDF, CSV import, queries and Parquet,
  multi-sheet workbooks, and the 100M-cell paged case.
- **Long-run memory**: no hour-long session drift.

## Reproduce

```bash
cargo xtask perf                              # the scenario gate against xtask/perf-budgets.txt
cargo run --release -p cella-engine --example memory -- 1000000
cargo xtask perf-report                       # this page
```
