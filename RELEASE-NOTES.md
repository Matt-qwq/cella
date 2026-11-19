# Cella 0.13.0

The performance release. A million-row workbook takes a third of the memory it did, and one edit
that recalculates two million formulas takes **0.4 s instead of 0.9 s**.

- **Memory:** 2.90 → **0.88 GB** live (2.18 → 0.68 GB reopened), 1M rows × 6 columns, 2M formulas.
- **Recalculation:** 893 → **~400 ms** (−56 %), ordinary chains and a column of running totals alike.
- **Window:** OpenGL is the Windows default — 183 MB lighter at the same frame rate.
- **Fix:** a formula moved by *Remove Duplicates* kept its stale shape, so it resolved from its old
  row in memory and its new row after a save — the same file, two answers.

| Same 70 MB file, same machine | Cella | Excel 16 | WPS 12.1 |
|---|---:|---:|---:|
| Open | **3.3 s** | 8.0 s | 10.7 s |
| Edit → recalc | 0.33 s | **0.08 s** | 16.8 s |
| Working set | 1.05 GB | **0.59 GB** | 0.99 GB |

Method, the GridCraft v0.19.1 comparison and what is *not* measured:
[`docs/external-benchmark.md`](docs/external-benchmark.md),
[`docs/performance-report.md`](docs/performance-report.md)
([中文](docs/performance-report.zh-CN.md)).

## Installing

- **Windows x64:** `cella-0.13.0-windows-x64.msi`, or the portable zip beside it. Unsigned.
- **Upgrading from an earlier build:** uninstall it first. Cella's MSI UpgradeCode changed on
  2026-11-18, so old installations are not upgraded over.
- macOS and Linux packages follow when those machines run the gate.

## Not verified in this build

The pinned corpus was not re-run (last run 2026-11-03: 3,084 workbooks, 0 reader failures), and
macOS/Linux were not gated. Everything else is: `cargo xtask ci` 9/9, `cargo xtask perf` 22/22
in budget, `cargo xtask oracle` green in openpyxl, Excel 16 and WPS.
