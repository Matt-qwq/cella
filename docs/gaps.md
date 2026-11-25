# Gaps

> **Last reviewed:** 2026-11-26 · **Last updated:** 2026-11-26 · **Change:** 11-26: **a 112 MB archival workbook is no longer refused by the part-size guard** — the owner's file needed 619 MB in one part and 1.32 GB in all where the caps allowed 512 MB and 1 GB; the caps are 2 GiB and 4 GiB now and the message says what opens the data (§73). Earlier: round 92 (11-26): **a numeric range axis is a bucket SQL groups by** — the last computation on the report path that needed the scan, so for a model source every operation is the engine's now: refresh, value filters, Show Details (§72). Round 91: Show Details asks the engine (§71). Earlier: round 47 (11-24): the measuring round closed its last three holes — **every one of the 517 functions has a number** (the nine that answered with an error were bad calls, not broken functions; the `FORECAST.ETS` family now says on the page that it is fitted at the engine's 200,000-point cap) and **the thirty-one `ui` commands are priced per setting** (**§30**: a right-to-left sheet costs 2.97× the baseline scrolling frame) — and **§28's first knife landed**: `edit.clearAll` 9.13 s → **1.40 s** on a 1000-column sheet, 1.14 s → 311 ms on a million rows, because the store's per-cell removal was quadratic in a row's width. The shape ruler's "1.0 ms tall" row was a params bug (the optional `range` was skipped) and is corrected. Earlier: round 46 (11-23): **the shape ruler** entered three entries — **§28 `edit.clearAll` takes 9.13 s on a 1000-column sheet** (8,859× the same cells as a million rows; `edit.clearFormats` 3.21 s, `range.move` 14.48 s follow it) and **§29 saving a hundred sheets costs 18× the same cells in one** (23.72 s against 1.3 s) — and the per-command memory reading is attributable again (§26 is now the report's `undo` line). Earlier: round 45 (11-23): the round-44 memory reading was withdrawn and the `FORECAST.ETS` limit entered as §27. Earlier: round 27 (11-21): three sign-tested knives landed (§2) — the closure's second-dependent box (−18.4 % on the 2M-dirty edit), band-copy-tag pricing (−7.2 %), and the store's compare-before-write plus one-bit row find (−20.1 %/−21.4 %). Earlier: round 26 (11-20): the evaluator is refuted by measurement (§2) — bytecode built and equivalence-proven, sign-tested a no-win (the per-cell cost is kernel-bound, not traversal-bound); the chain edit's missing time was undo bookkeeping, and an evicted step now releases on later cheap commands (built-case edit −30 %, 8/8, p=0.0039; the gate's aggregate 443 ms). Earlier: round 25 (11-20): the 256 × 200 release soak's three findings are **fixed** with red-first regressions — two incremental-vs-full spill divergences in calc, and §3's suppressed-filter hidden rows, the same root cause as the seed-97 case, closed to its design question; the perf gate is locked at twice the 11-20 quiet pass and grew `mem_drift` and `save.repeat` (22 → 25 scenarios, all green); and the unmeasured workloads have numbers (performance.md). Earlier: round 23 closed the memory and performance line (§2): a 1M × 6 workbook is **0.884 GB** live / 0.683 GB reopened (2.90 GB before the round), the row side tables (144 → 72 MB), the band's number index (66 → 33 MB) and the journal (42 → 15 MB) taking the last steps; **measured against Excel 16 and WPS 12.1** on one 70 MB file (open 3.3 vs 8.0 / 10.7 s, recalc 0.75 vs 0.31 / 15.3 s, idle 246 vs 35 / 288 MB) with **OpenGL the Windows default** (183 MB lighter at the same 144 Hz); two recalculation changes took the 1M chain 0.85 → 0.75 s; and the soak found one reference bug (§6: a formula moved by Remove Duplicates kept its stale shape), fixed with its test. Earlier: the memory round (§2), three waves: byte accounting per part; the dependency graph compacted twice (dependents 478.2 → 142.6 MB, nodes 172 → 104.9, areas 201 → 125); the save build parallelised; formula **shape sharing** (2M formulas → 3 shapes, the per-cell tree deleted, `Formula` 48 B, `Node` 16 B); a **structural edit rewritten once per shape** (insert a row above 2M formulas 8.29 → 3.97 s, delete 8.61 → 3.59 s, rename a sheet 8.58 → 4.86 s); and an **undo that restores a paged store**, gated so it never costs the edit (a sort of a paged store used to take minutes and now takes the sheet back into memory). A 1M × 6 workbook is **2.90 → 0.971 GB** live (reopened 0.77 GB) and no scenario is slower: save 3.7 → 2.05 s, open 5.5 → 4.02 s, fill formulas 2.8 → 1.77 s, recalc 2.3 → 1.80 s, rebuild 1.7 → 1.12 s. Earlier: minor (11-14: the per-cell half of the performance round — `MODE` 2.66 s → 15 ms, `VLOOKUP` 2.9–4.9× and text `MATCH` 7.2×, wildcard `COUNTIF` 2.0×, grid paint 1.60 → 1.07 ms/frame, all in §2; and §3's filtered-sheet save bugs were found by a 150-seed soak. 11-13: the performance round (§2) and three save-fidelity bugs in filtered sheets found by the command-sequence soak (§3), one case left recorded there with a six-step repro; round 18: a real-world corpus run — **3,084 workbooks opened, 635 save/reopen with 0 mismatches, 16 files identical in Excel 16** — so #1 is met for beta; the pinned corpus remains for CI. Round 17: `.xls` **embedded charts, autoshapes and text boxes** are read — group kind and variants, series ranges, cached names, title, legend and the `msosptChart` anchor — and **verified in Excel 16** (five chart kinds; five shapes with their text, fill and line); §7 loses its charts and shapes items and is met for beta, leaving OLE objects and chart sheets as full scope. **RC4 is not an ECMA-376 Standard cipher**: [MS-OFFCRYPTO] 2.3.4.5 requires AES and Excel refuses a hand-made RC4 `.xlsx` at 40 and 128 bits, so "RC4 write" is really encrypted `.xls` output and waits for a `.xls` writer. No score change) · **Target:** Microsoft Excel (Microsoft 365); earlier: the accessibility check names every object without alternative text (WordArt and equations included) |

Every known shortfall against Excel, one entry each, ranked by user impact (what blocks someone
using Cella for real work first). This is the work list: pick from the top, check the open
PRs first (50 open on 2026-10-10; several already address entries here), and update the entry
when you land work. Estimates are Opus 5.5 agent-hours. Area detail lives in the linked parity
docs; the summary is in [target-app-parity.md](target-app-parity.md).

Kinds: **F** feature · **U** UI/UX · **FF** file format · **H** hardware · **L** localization ·
**P** performance · **S** stability.

## Closed by the upstream-adoption round (2026-11-16)

Eighteen batches taken from `storytold/gridcraft` v0.19.1 (our fork diverged at v0.11.1). The
table below is what this closed; everything else in this file still stands.

| was open | now |
| --- | --- |
| a workbook's own data store was one string literal in one defined name — past Excel's 255-character literal and 8,192-unit formula limits, and XML-illegal characters were dropped on save | `_gc_<key>_<n>` chunks with an escape for XML-illegal characters; legacy `_sc_*` still read (B1) |
| "Set precision as displayed" was stored and round-tripped but never applied — a `fullPrecision="0"` file computed at full precision | the option rounds typed constants on entry and forces results to their cell's format on every pass (C1) |
| a deeply recursive formula could overflow the native stack, which aborts the process instead of panicking | nesting cap, work budget, LAMBDA fuel and call cap, 896 KiB stack guard: an error value (C2) |
| PivotTable Show Details on a hand-edited (stale) report returned **another cell's** records | refused with "refresh the PivotTable" (E7) |
| no Solver Sensitivity report; no saved Solver models | Sensitivity report (textbook-verified duals, allowables, reduced costs) and `data.solver{Save,Load,Reset}Model` (E7) |
| no WordArt | `insert.wordArt`/`wordArt.set`, ten presets, in-place edit, xlsx round trip (W9) |
| no chart data tables | `c:dTable` modelled, drawn under column/bar/line/area charts (H13) |
| no macro recording, running or trust policy | recorder, runner with one undo step per run and rollback on a failed step, trust policy, `macro.debug` (M11) |
| no form controls | buttons, sliders and spin buttons modelled, round-tripped as ctrlProps + AlternateContent + VML, painted and clickable (C12) |
| the JSON workbook was a raw dump of the live model: a rename changed the format and unknown fields were lost | versioned envelope with retained unknown fields and read caps (J8) |
| every window on a document shared one selection and one active sheet | one view per window (N15) |
| no undo grouping | `group_begin`/`group_end`/`group_rollback`, Undo/Redo refused inside a group (E10) |
| view/outline settings silently reset on open+save (`workbookView` flags, `tabRatio`, `filterPrivacy`, `refMode`, `outlinePr`, `readingOrder`) | all modelled and written back (X1) |
| pre-dynamic-array formulas evaluated as if they were dynamic; Google Sheets exports kept their `__xludf.DUMMYFUNCTION` wrapper | legacy implicit intersection applied on read; the wrapper unwrapped when every function inside is known (X2) |
| numbers had no reference corpus, so rendering divergences were invisible; `B1`/`B2` codes were leniently ignored | 707 upstream golden cases byte-identical, Hijri `B2`, invalid codes rejected (F17) |
| statistics/mathematics, financial and engineering functions were shallower than upstream's (MOD, GCD/LCM, MINVERSE, distributions, LINEST, PMT family, base conversions…) | ported with their Excel-observed tests; `ECMA.CEILING` added (F16, F18) |
| twelve small command ids were missing (`range.move`, `selection.nextCorner`, `formulas.pasteName`/`traceError`, `pivot.options`, `data.outlineSettings`, four `view.show*`, `window.list`) | added, and `formulas.*` now refuses to edit the store's own `_gc_*` chunks (E6) |

Deliberately not taken: `craft-gridcraft-format` and the craftrules shared crates (our own envelope
instead), upstream's `.xls` reader (ours reads charts, shapes, comments, CF, hyperlinks and freeze
panes; theirs does not), its chart writer (we patch unmodelled options through instead of
regenerating), equations on `ratex` (**owner's decision**) and Python cells with `crates/sandbox`
(**owner's decision** — the project has "PY out of scope" recorded).

## Beta blockers

No alpha blockers: every core workflow passes the [alpha gate](roadmap.md#alpha-gate).


### 1. XLSX proven on a local real-world corpus; the pinned corpus remains (FF)
- **Missing:** the pinned, sha256-verified corpus for CI (0–1 h once a URL or checkout exists). The
  real-world proof itself landed on 11-03 (below).
- **Landed 11-02 (Excel half):** the owner activated **Excel 16**, and [`cargo xtask oracle`](oracle.md)
  now opens every fixture in Excel as well as WPS: it refreshes every PivotTable, recalculates,
  saves a copy and reads it back, comparing every cell, format, merge, table, CF/DV/chart count
  and defined name with the model. **All five fixtures pass in both applications** — no repair
  prompt, no lost content — including a PivotTable with a grouped numeric field, a value filter
  and a slicer. The probe tolerates only Excel's own localized PivotTable chrome (`Row Labels`,
  `Grand Total`) because Excel re-renders those in its UI language.
- **Landed 11-03 (the real-world corpus run):** a local 9.6 GB synced work folder with 3,199
  workbooks was run through `cargo xtask corpus`: **3,084 readable workbooks opened, no reader
  failure on a valid file, and 635 save → reopen cycles with zero digest mismatches** (the digest
  now covers values, formula text, styles, merges, comments, links, validation/conditional-format
  ranges, tables, names, charts, shapes and pictures). **Excel 16 reopened Cella's copies and
  found them identical**: 12 files compared every formula and constant, 4 more (in-cell picture,
  comments, external links, a 3.4M-cell workbook) matched sheet names/used ranges/cells, and all 37
  readable `.xls` files converted and compared equal in content (one differs only in
  formatting-only columns, exactly as Excel's own `.xls`→`.xlsx` conversion does). The 115 failures
  were 4 invalid/truncated files (openpyxl also refuses them) and 111 password-protected files; 449
  cloud-only placeholders were skipped. Details: `plan/corpus-run-2026-11-03.md` (local).
- **Still missing:** the pinned corpus as a durable, shareable artefact for CI (the repo records no
  corpus URL, and `storytold/craftrules` is private). The run is one command:
  `cargo xtask corpus <dir> [--recursive] [--all] [--keep <dir>]` opens every workbook, prints each
  warning/error, saves/reopens (one in five by default), compares the digest and reads the copy back
  through openpyxl, never writing into the directory; `--keep` keeps the saved copies and a
  `saved-copies.tsv` for an Excel-side check (see [oracle.md](oracle.md#the-corpus-run)). The
  owner's remaining manual checks are [excel-checks.md](excel-checks.md) #7/#8/#10.
- **Impact:** the main format; a repair prompt or silent loss destroys trust.
- **Estimate:** 0–1 h to pin a corpus for CI; the proof itself is met. [file-format-parity.md](file-format-parity.md)

### 2. Large workbooks: what remains of the open/save problem (P, H)
- **Landed (10-24):** the read path streams a worksheet as it decompresses, decodes windows on
  eight threads, packs row bands inside those threads (`BandBatch`), and inflates the next window
  while the last decodes; the write path walks the store once and compresses at level 3. The 118 MB
  report of 31,709,565 cells now opens in 4.1 s (6.1 s on the desktop) at 549 MB of working set and
  saves in 10.8 s; the 1.28 GB report opens in 5.2 s at 569 MB.
- **Landed (10-26):** the store's rows are band-level stripes now (bitmaps, kinds, payloads and
  styles one vector a band with a slice a row) instead of four vectors a row: a range scan costs
  2.9 ns a cell instead of 3.9, packing is 10% cheaper, and the 118 MB report opens in 1.37 s
  holding 155 MiB of cells (191 MiB), digest unchanged; details and the A/B in
  [`roadmap.md`](roadmap.md#m19--cold-paging--6090-h). The 163 MiB first reported was the
  breakdown counting the text and style dictionaries twice, now fixed.
- **Measured (10-26) at 100 million cells:** a 551 MiB, 10-sheet export opens in 6.4 s and holds
  2.4 GB (24 B/cell); its per-band dictionaries are 1.3 GB of that, because every id and amount is
  distinct. Report-shaped data at the same count extrapolates to ~490 MiB. So the M19 acceptance
  (≤ 600 MB) holds at report density and paging is what the unique-key export needs.
- **Landed (10-27): the 100k-dependent edit gate.** An edit of A1 with 100,000 `=$A$1*2` cells costs
  **65–75 ms** against the 100 ms gate (was 245 ms; a full recalculation of the same sheet fell from
  281–289 to 134–147 ms), single-threaded, 0.65–0.75 µs a formula. The pass reads parsed formulas and
  legacy array ranges from the dependency graph instead of rebuilding 100k cells, keeps one `Slot`
  per result (in-progress is an empty value), writes values row-major through
  `CellStore::set_values`, shares each row's `Arc<RowExtras>` in a band copied for undo, charges undo
  from a cached `Formula::expr_bytes`, diffs before looking for legacy array anchors, and reads a
  single-cell reference without an area vector. Details and the phase split in
  [`roadmap.md`](roadmap.md#m18--vectorized-aggregation-kernels--3045-h).
- **Landed (10-27, second round): multi-threaded recalculation of independent formulas.** A pass now
  evaluates its leaves in up to eight shards (one expression/result map each), writes them in row
  order, and repeats for the next layer (up to 16 waves); a whole-column precedent, a dynamic or
  array formula in the dirty set, or iterative calculation keeps the pass sequential, and a spill or
  cycle rolls the parallel results back. `CellStore::set_values` clones touched bands in workers for
  large batches. An edit of A1 with 100,000 `=$A$1*2` cells is **20–25 ms** end to end (was 65–75),
  200k is ~35–42 ms, 500k ~100–132 ms and 1M ~208–277 ms in `_gridcraft-bench/calcbench`; the 100k
  full recalculation is 89–114 ms. Details in [`roadmap.md`](roadmap.md#m18--vectorized-aggregation-kernels--3045-h).
- **Landed (10-28): bulk edits and whole-sheet data commands.** `Calc::cells_changed` used to scan
  a 64×64 block for blocked arrays **per changed cell**; a 10M-cell Remove Duplicates therefore did
  ~40 billion block visits. It now scans the union of the changed blocks once (or keeps the per-cell
  scan when the changed cells are scattered), and the same 10M-cell Remove Duplicates fell from
  **91 s to 12 s** (1M×3: 15.4 → 5.3 s); its recalculation phase went from ~80 s to 1.4 s. Sort on
  the same 1M×10 sheet is **3.2 s** (recalculation 0.02 s) and Fill Down on 1M rows **1.45 s**.
  `data.sort`, `data.removeDuplicates` and `range.setValues` share a **12M-cell cap** (1M×10 fits;
  1M×20 is refused in 1.3 s with a clear error), and `CellStore::style`/Remove Duplicates parse a
  style's number format once rather than once a cell.
- **Landed (10-28): the per-band text index goes away when a band is finished.** The sorted
  pointer→slot index is only needed while cells are being added, so `Band::finish` drops it and the
  first edit rebuilds it; a completed band keeps one `Arc<str>` per distinct text. The 100M-cell
  unique-key export drops from 2,411 to **1,911 MiB** live (texts 640 → 301 MiB; number tables 679 → 519 MiB), digest unchanged.
- **Landed (10-28): cold paging.** Completed bands are compressed into per-sheet `GPB1` page files
  (zlib, fast) and read back through a 4,096-band LRU; the engine pages packages over 200 MB, so the
  55 MB 1M×10 sheet stays live. The 100M-cell export opens with **16.7 MiB live and 183.1 MiB peak**
  (was 1.91 GB live) and 729.5 MiB of compressed pages on disk; its digest is cell-for-cell the same
  with and without paging. The 31.7M report stays live at 49.7 MiB and opens in 1.47 s. Page files
  are removed with the store; a corrupt or truncated page is refused.
- **Landed (10-28): row inserts/deletes are a logical→physical map.** `CellStore::shift_rows`
  changes a segment list instead of moving cells; a gap row gets a fresh physical key when written,
  and reads/iteration/used range/formulas map through. Insert one row above a 1M×20 sheet is
  **3.7 ms command / 35.8 ms commit**, with no cell moved. The formula rewrite no longer walks every
  cell to find none (it reads the store's formula side tables), and iteration skips empty bands.
- **Landed (10-28): the column sibling.** `ColMap` materializes only the used width, allocates a
  fresh physical column for a written insert (or reuses a deleted one), and maps reads/iteration/
  formulas/used range. A column insert at A in the same 1M×20 sheet is **9.8 ms / 42.5 ms commit**,
  no cell moved. Save/reopen tests cover both axes; column-mapped value writes take the
  value-at-a-time path.
- **Landed (10-29): row/column line metadata is a physical-id map too.** `Sheet.rows`/`cols` are a
  `LineMap`: the store's logical→physical runs with the entries in 128-line `Arc` chunks. A row or
  column insert/delete moves runs instead of reindexing every custom height, hidden line or
  outline entry, an undo snapshot shares untouched chunks, and JSON/XLSX still store logical lines.
  Insert inheritance, delete, undo and a mapped-sheet XLSX round trip are covered (model 62,
  engine 419, xlsx 265 tests). The UI also caches the derived `LineIndex`/outline axes per
  `(document, revision, sheet)`, so a 1M-custom-height sheet no longer rebuilds them every frame.
- **Landed 10-29: per-sheet save.** Worksheet XML deflates on a background worker as each sheet
  is written; the archive mixes raw/pre-deflated parts in order, and wasm uses inline/sequential
  paths. The 100M-cell save now peaks at **1,814.3 MiB in 28.42 s** (was 6,049.8 MiB / 26.37 s),
  and `real_117m` at 1,283.9 MiB in 3.48 s; output hashes are unchanged. Only the deferred
  page-file rewrite and post-beta streaming save remain.
- **Landed (10-29): writer ownership.** `ZipOut::add` takes each part `Vec` instead of copying it,
  `write_zip` moves each part into its deflate worker and drops it there, and the zip64 decision is
  taken before compression from a whole-archive bound. Output is byte-identical (37,153 bytes pinned
  by test); a worker panic fails the save instead of writing an empty part. The writer still holds
  every sheet's XML until layout, so per-sheet release (S4b) remains.
- **Landed (10-29): spill beside the document.** A paging read creates the page file in the
  document's folder (temp fallback), with `CELLA_SPILL=temp|off`, a UNC skip, a sanitized name,
  Unix 0600 + unlink-after-open and Windows delete-on-close. A write failure latches spilling off
  while existing pages stay readable; no file means the workbook opens live with one warning.
- **Landed (10-29): byte-budget page cache and uncached save scan.** The decoded-band cache is
  64 MiB by default (`CELLA_PAGE_CACHE_MB` clamped 8–512), shared per page file, charged at
  decode with generation/`Weak` eviction and `PageCacheStats`. `iter_scan`/`visit_scan` decode one
  band at a time without touching it, and the XLSX writer uses that path: a save leaves the
  viewport's cache exactly unchanged and no longer collects a 16k-wide band.
- **Landed (10-29): mapped value writes and packed folds.** `set_values` streams through the row
  map's logical runs and keeps a run-merged parallel plan; 1M values after a row insert write in
  **35 ms** (was 131–135), after a column insert 33 ms (was 116–142). `visit_packed` follows the
  column map's runs, so a dense block is **2.5–3.3 ns/cell** (was 30.6), and the owned/formula gate
  resolves a band once (1M paged rows 0.84 ms, was 9.5). `diff` keeps its documented physical
  contract; `diff_logical` is the O(cells) map-independent diagnostic for tests/fuzz.
- **Landed (11-13): the per-frame and per-cell work.** `CellStore::used_range` is cached (a
  1M-row call was **6.5 ms** and sized both scrollbars every repaint; now 10 ns, grown on write and
  re-walked only when a mutation can shrink it). Conditional-format statistics, chart drawings (with
  the hit regions the click test reads) and sparkline paths live on the grid state keyed by
  (document uid, revision, sheet) and are dropped on any edit — grid paint on a 100k-row data bar
  falls **10.7 → 1.4 ms/frame**. A defined name that stands for cells is a precedent rather than a
  dynamic reference, so an unrelated edit no longer re-seeds every formula that mentions one
  (100k-row name column: **100,000 cells/158 ms → 1 cell/5 µs**), and `Evaluator::name` reads the
  same `NameCache` instead of scanning and re-parsing per cell (**100k evaluations 117 → 76 ms**, at
  parity with the reference written inline: 31 vs 35 ms per 100k-cell edit). `check_arrays` asks
  the store whether the sheet holds a multi-cell array instead of diffing it (**1M-row keystroke
  4.6 → 0.9 ms**). A rebuild hands each formula the tree it parsed back to its cell, and a save
  writes a formula whose file spelling is its own text without rebuilding it (**200k formula cells:
  12 % of a save**, bytes identical). No behaviour change; the engine suite is green again (a stale
  `custom_filter_blanks` had been reading the pre-bitmap hidden flag since `d2c55e7a`).
- **Landed (11-17): a session reports its live heap by part.** `CellStore::memory` → `StoreMemory`
  (bands, stripes, value dictionaries, texts, row extras, formula text and trees, arrays, page
  cache), `Graph::memory`/`Calc::memory` → nodes, areas, the dependents map and its long lists,
  spills, cycles and the name cache, and `Session::memory_report` adding the workbook and the
  history stacks; `crates/engine/examples/memory.rs` prints the table (<1 s at 1M rows) and
  `total == Σ parts` is a test in each crate. The reference workbook (1M × 6, 2M formulas) holds
  **2.56 GB live**: cells 1.27 GB (formula trees 768 MB, text + `Formula` 165 MB, row side tables
  144 MB), the graph 516 MB, the undo stack 738 MB, the journal 42 MB; **1.84 GB reopened**. The
  same change made `Band::heap_bytes` the page cache's decoded-band charge, which had under-counted
  a band (two text-dictionary vectors, overflow vectors, `len` vs `capacity`).
- **Landed (11-17): the precedent index is per sheet and a one-to-one pair.** The 5M-entry
  `Key → Deps` map was the graph's cost — 478.2 MB, 95.6 B an entry, every entry `Deps::One`: it is
  now one table per sheet keyed by `CellRef`, a bare pair for the cell exactly one formula reads, a
  `Vec` from the second on, plus a separate cross-sheet table — **142.6 MB, 28.5 B an entry**.
  `fill.formulas` 3.61–3.85 → 3.13–3.21 s and `fill.chained` 5.24–5.55 → 4.35–4.63 s (a stashed
  same-binary A/B); `rebuild.graph` and `edit.recalc` flat, 154 calc tests green. A sorted arena
  would have been 80 MB and was rejected: a fill inserts out of order, so every insert would
  memmove a megabyte-long tail.
- **Landed (11-17): worksheet rows are built in parallel windows.** The shared string table is
  filled by a pre-pass over row windows in the file's first-use order, then `<sheetData>` is built
  in parallel windows concatenated in row order; per-sheet part numbering and `Out` mutations stay
  serial and in sheet order. The XML build fell from **91 % to 46–49 %** of a save: **save 3.7 →
  2.45 s** at 1M rows, 356 → 211 ms at 100k, package bytes identical (the golden 37,185 B, and a
  test compares 1/2/8 windows on the whole package).
- **Landed (11-17, second wave): formula shape sharing.** A formula's *shape* is its tree with every
  relative reference stored as an offset from the formula's cell, interned once per workbook (keyed
  by the R1C1 print, `Weak` entries) while the cell keeps its exact A1 text (`crates/formula/src/shape.rs`,
  `Formula.shape`, `Reference::range_at`). The evaluator carries the cell it evaluates for, the
  dependency walk and `may_return_array` take a base, the reader interns per read chunk, and fill,
  copy, paste, sort and `edit.autoFill` reuse the `Arc` and print the cell's text: **2M formulas hold
  3 shapes**, a 1M-row autofill costs **0 parses**, trees **768 → 12 MB**, and the workbook went
  **2.90 → 1.17 GB** live (reopened 0.95 GB) once the old per-cell tree was deleted (`Formula`
  72 → 56 → 48 B, `calc::Node` 24 → 16 B). Every scenario improved: save 3.7 → 1.97 s, open
  5.4–5.6 → 4.08 s, fill formulas 2.8 → 2.15 s, recalc 2.3 → 1.81 s, sort 7.9 → 6.85 s, graph
  rebuild 1.7 → 1.25 s. The fuzz's new master invariant (`text == print(expr_at(at))`) caught a real
  bug (a shape anchored at its original cell after a row insert), and three speed regressions found
  by re-measuring the merged tree (a per-evaluation resolve, a save analysing a resolved tree per
  formula, a legacy-`@` analysis per cell) were fixed before recording.
- **Landed (11-17, second wave): the graph's nodes and areas pack.** Node 41 → 33 B a slot (the flag
  byte inside the run's length word, an over-long run unconstructable), the area arena 24 → 20 B
  internally and trimmed to `len + len/4` at the graph's quiescent points: nodes 172.0 → 138.4 MB,
  areas 201.3 → 125.0 MB, `edit.recalc` 2.31 → 1.68 s.
- **Landed (11-17, second wave): the undo step that the budget would drop is kept compressed.** Not a
  byte reduction at 1M (the budget keeps one newest entry, and compressing it turns a 23 ms undo into
  4.99 s, measured), but at 100k rows a depth of **4 → 15 for +13 % bytes**, behind a cost gate
  (`≤ 600 bands`, ≈60 ms of encode) so a refusal costs nothing (`CellStore::spill_retained_against`).
- **Landed (11-17, third wave): a structural edit rewrites once per shape.** `rewrite_all_formulas`
  is now one tree rewrite per (shape, side of the edit) plus a print per changed cell, with a per-cell
  fallback only where a relative reference straddles the edit or a span has one `$` end on the edited
  axis (the design's own `SUM($G$2:G2)` family); `sheet.rename`, `delete_sheet`,
  `formulas.renameName` and `table.rename` go through the same `rewrite_shapes`. Insert a row above
  2M formulas **8.29 → 3.97 s**, delete **8.61 → 3.59 s**, rename a sheet over a 1M-formula column
  **8.58 → 4.86 s**; sort unchanged (it was already shape-level). Proven by a 14-formula × 14-edit
  suite that strips every shape and asserts the two saved packages are **byte identical**, and a
  20k-case proof that the shape-level adjust equals the per-cell one.
- **Landed (11-17, third wave): the graph's node table is per sheet and `CellRef`-keyed** (33 → 25 B
  a slot, `Node` 16 B; a values-only sheet takes no table at all): nodes 138.4 → **104.9 MB**, total
  1.169 → **1.1359 GB**. Ratchet: ≤ 26 B a slot.
- **Landed (11-17, third wave): an undo restores a paged store.** A spilled step records the cells
  `Calc::restore` would have to re-insert (`formula_delta` at spill time), so undo hands back the
  page file instead of decoding it: `edit.undo` 26 ms with **0 pages decoded**, history 241.6 →
  131.9 MB *when it applies*. At 1M the cost gate refuses (the unfitting part is ~10,400 bands ≈ 1 s
  of encode inside the edit — measured at +35 % on `fill.chained`, +40 % on `edit.recalc`), so the
  step stays live and the charge is unchanged; the path is proved by a forced-spill test (paged
  workbook, `last_recalc_cells == 0`, byte-identical save, a sort afterwards not pathological). A sort
  of a paged store used to read a permutation band by band (minutes at 1M) and now takes the sheet
  back into memory first; an audit found every other bulk command band-ordered.
- **Measured 11-17, what is left of the memory bill:** formula text 133.4 MB (the file spelling, the
  JSON envelope, the formula bar and the save's fast path — kept by design; 24 B of struct a formula
  is what remains of the per-cell cost); graph nodes 138.4 MB (a per-sheet node table would take it
  to ≈105 MB and changes 13 call sites) and the area arena 125.0 MB (at 1.25× live by construction);
  the undo stack 241.6 MB *charged* (its own heap is a fraction of that; at 1M the structural answer
  is restoring into a paged store — attach the page file and let the byte-budget page cache decode
  bands on demand — not attempted); the row side table is 72.0 MB (it was 144.0 — the rare part
  boxed and the formula vector sized exactly); the journal's 42 MB of replayed bulk edits (a single huge
  parameter is boxed to a marker now, so the 94 MB base64 case is gone); and a structural
  edit still prints every moved cell's text (S5's floor: one print per moved cell); and the undo of a
  workbook *opened* paged (>200 MiB file) still walks the bands, because such a step never records a
  delta.
- **Landed (11-18): the chained edit, 9-10 % faster** - the last engine cost with a clear shape.
  Two changes, each tested beside the code: the closure's fast path and its generic array path share
  one scalar implementation, and a chained fill keeps eight links in a stack array (`INLINE_CHAIN`)
  instead of a per-formula frame; and the closure pass probes each cell once - the empty-table checks
  hoisted per pass, `SeenCells` bitmaps sized once, a `Vec` with a head index for the queue, no
  per-formula frame in `calculate_at`, and `legacy_of` short-circuited (a bitmap-doubling regression
  of its own was caught by the fuzz and fixed). On the 70 MB reference workbook's edit (999,999
  chained formulas) **0.85/0.83 -> 0.75/0.77 s**, two runs each side of a same-binary A/B; profile
  after: closure 164 ms, eval 408 ms, write 63 ms. Priced and deferred: a value-slot/write-back merge
  (est. -60-100 ms), a dense slot index (-30-40 ms), and for the 2.4x gap to Excel, bytecode
  linearisation (a rewrite of `eval.rs` and `eval/*.rs`).
- **Landed (11-18, round 24): the chained edit 893 -> 646 ms** in the gate's harness, two steps: a
  **dense slot index** (a pass's cells looked up by arithmetic in a per-sheet box table, hash
  fallback when they do not fill their box — 893 -> 665/692 ms, the ~200 ms four million hash probes
  cost) and a **chain run** (a column of running totals accumulated in one loop through the same
  `binary_values` kernel; 999,999 of the edit's 1,000,001 formulas take it, eval 218 -> 179 ms,
  command 712 -> 646 ms). Both same-binary A/Bs, best of three, two runs each side. Iterative
  calculation and any shape with `$`, a name or a function keep the general path; a cycle test holds
  the two to the same values. The chain run measured three times larger in a dev build — the lesson
  (a shortcut that removes work release already removed only shows its real size in release) is in
  `performance.md`. What is left of this edit: closure ~190 ms (1M dependents probes), write-back
  ~70 ms, eval ~179 ms.
- **Landed (11-18): the closure's hash probe is gone — 232.9 -> 27.4 ms at 1M** (same binary,
  the gate harness's `edit.recalc`, 1,000,001 dirty cells; the command 1.25 s -> 539 ms). The retry
  of the reverted dense single-precedent table keeps the maintenance and the sparse fallback and
  rewrites growth: a per-sheet box of `CellRef -> CellRef` slots with an explicit `capacity`, grown
  by **copying once a doubling** of that capacity (never per line), while a cell the box refuses keeps
  the hash entry and one batch in `settle` — **at most one pass over a sheet's entries per command**
  — takes them into the box when the box around all of them is worth its cells. Two measurements
  shaped it: the box refuses a cell more than twice the budget away (that refusal is the whole of the
  last attempt's O(n²)), and a batch rebuild is allowed the cells of the box it replaces, so a sheet
  whose precedents arrive a column at a time keeps its index instead of dumping 5M entries back into
  the hash. 100k `fill.formulas` **155-171 ms** (was 164), 1M **1.30 s**; a 100k link test asserts
  the copy count, not a stopwatch. Memory: two cells a dependent at rest (16 B against ~17 B a hash
  entry), a 16M-cell ceiling a sheet.
- **Landed (11-18, second wave): 646 -> 460 ms.** The slot-table build **66-74 -> 40-47 ms** (a slot
  borrows its shape from the graph instead of cloning an `Arc` a cell, one `Vec<Entry>` instead of
  two parallel vectors, and one sheet-table probe a sheet run instead of a `graph.node` probe a
  cell) and the closure **190 -> 27 ms** (the single-precedent dependents table: 84 % of the phase
  was one hash probe a cell on a cold 34 MB table). `edit.recalc` at 1M **589/596 -> 465/460 ms**,
  and from the start of the round **893 -> 460 ms (-48 %)**. The dependents box was built twice: the
  first attempt copied the box once per new precedent and took a 100k `fill.formulas` to 37.96 s;
  the kept one copies once a doubling of an explicit capacity and is accepted against **100k 154 ms**
  and **1M 1.27/1.28 s**. What is left of the edit: eval 179 ms (chain run), write 70 ms.
- **Landed (11-18, third wave): 460 -> 406-421 ms.** `Sheet::value` rebuilt a `Cell` per read (an
  `Arc<Band>` and an `Arc<Formula>` cloned and dropped — four atomics, a million times for the
  chain's second operand); a value-only path borrows a live band's packed row instead and a paged
  band still decodes. eval **179 -> 116-118 ms**, the command **465/460 -> 421/406 ms**, the fills
  unchanged. The grid paints through the same read, so the display path pays less too. From the
  start of the round: **893 -> 406 ms (-55 %)** against Excel's 0.31 s for the same edit.
- **Landed (11-18, fourth wave): the write-back, 70 -> 45-50 ms.** The pass cloned its results into a
  second list (two temporaries, 48 MB at 1M, a million `Value` clones) before the store saw them;
  it now feeds a batch it owns and reuses, moving each value out of its slot. `edit.recalc`
  **421/406 -> 407/390 ms**. From the start of the round: **893 -> 390-407 ms (-56 %)**, against
  Excel's 0.31 s for the same edit. The store's order check measured 0.55-0.68 ms and stays.
- **The memory line closed at 0.884 GB on 11-18** (from 1.14 GB at the end of 11-17): row side
  tables 144 -> 72 MB, the band's number index 66 -> 33 MB, the journal 42 -> 15 MB; the audits of
  the calc graph's three tables and of the one undo step found structure, not slack, and changed
  nothing. Measured against Excel 16 and WPS 12.1 on the same 70 MB file, with the idle window
  identified as the one part buying nothing (246 MB; **OpenGL is the Windows default** now, 183 MB
  lighter at the same 144 Hz).
- **Landed (11-20, round 26): the evaluator is measured, not guessed at anymore.** Per-shape
  **bytecode** was built (compile once per shape, a slot-stack interpreter, every opcode the same
  kernel the tree uses) and equivalence-proven both ways — and sign-tested a **no-win** (−10 %,
  0/8): the per-cell cost is kernel-bound (Host reads and `binary_values`), not traversal-bound,
  so changing the traversal cannot pay. A monomorphized `Host` measured the trait boundary at
  ≤5 ns a cell and reverted with it; comparing at `Sheet::value` to skip unchanged writes costs
  more than the band copy it saves. What the attribution did find: **evicting an undo step freed a
  ~148 MB snapshot inside the edit (120–180 ms of pure destructor)**, so an evicted step now hands
  back to a bounded pending-release queue that later cheap commands drain — the built-case edit
  **778 → 544 ms (−30 %, 8/8 pairs, p=0.0039)**, the gate's aggregate 1M 443 ms; `edit.recalc`
  is unchanged (its scenario pays the eviction in an earlier command). The refuted bytecode stays
  off the default on the branch (`perf/p1-evaluator`, `CELLA_EVAL=bytecode`) with its two-way
  equivalence suite. The remaining measured pools: the model's pricing walk (17–34 ms a command,
  `retained_bytes_against`), the 2M-dirty closure (217–231 ms), and the model store's read/write
  kernel — the old "bytecode linearisation closes the gap to Excel" pricing is withdrawn.
- **Landed (11-21, round 27): three knives, each certified on an idle machine** (eight-pair
  interleaved sign test; `plan/ab-r27-*.csv`). The closure's per-sheet box took the **second**
  dependent of a precedent two formulas read, which used to fall into the hash `many` list and pay
  a cold probe a cell: the 2M-dirty edit **866 → 707 ms (−18.35 %, 8/8, p = 0.0039)**. A `Band`
  now carries a 24-byte copy tag so a snapshot's retained price skips the per-row walk when the
  band provably holds the same rows, with the tag's bytes subtracted from the stripe count so the
  charge and every eviction decision are byte-identical: **−7.23 % built (7/8)**. And the store
  compares the packed row before copying a band (all four write paths), finds a row with one bit
  probe instead of a search over 48-byte headers, and stops double-probing in `set`: **−20.08 %
  built (8/8), −21.40 % reopened (7/8)**; the same-process model A/B moved `value()` 82.6 →
  38.4 ns and `set` 203.8 → 126.1 ns/cell. The same knife was certified on the workload a person
  waits on: **a 6M-cell paste 2,011 → 1,300 ms (−35.36 %, 8/8, p = 0.0039, t = −21.65)** in its
  own paired gate (`plan/bench/ab_paste.ps1`, `crates/engine/examples/paste_bench.rs`; the
  baseline side needed that one example file replayed onto `90c4781` because the harness is new).
  Gate 25/25 after the merge.
- **Measured (11-22, round 35): the close-out, and the one operation where the gap is 16x.** The gate
  is **29/29** on an idle machine (1M: `edit.recalc` 544 ms, `sort.formulas` 2.53 s, `save.xlsx`
  1.80 s, `open.xlsx` 2.83 s; `mem_drift` flat within 0.13 %). Round 34's save cost, A/B on the same
  machine a commit apart (`save_bench`, best of six, three runs a side): **1.59 → 1.63 s, +2.5 %**.
  **F9 on the 70 MB file, against Excel for the first time**: Cella `formulas.calculateNow`
  **1.05 s** (1.03–1.07) against Excel `CalculateFull` **0.064 s** (0.062–0.082) — **16x** — and
  **0.71 s of our 1.05 s is the dependency-graph rebuild** (`rebuild.graph` at 1M in the same gate;
  `formulas.calculateNow` rebuilds before it evaluates), so the **evaluation proper is ~0.34 s
  (≈5x)**. Excel keeps its tree across a recalculation. The old "full recalculation 1.30 s vs Excel
  0.31 s" line is retired: 0.31 s was a different protocol, and the evaluator is not the gap (round
  26 refuted it). **This was the next measured gap and it was a rebuild design question** — the
  candidate list's first item then, not an evaluator rewrite. Round 36 answered it (below).
- **Measured (11-22, round 36): the F9 rebuild is gone — 0.40 s against Excel's 0.072 s (5.6x),
  from 16x.** `Calc::recalc_all_keeping_graph` makes the full pass over the graph every edit already
  maintains; `formulas.calculateNow` calls it, and `Calc::recalc_all` (which still rebuilds) stays
  for structural edits, `fullCalcOnLoad` and the fuzzer's ground truth. Same-file A/B, four
  alternating rounds of three runs a side: **1.046 → 0.406 s (−61.2 %), faster in 12 of 12 runs**;
  the Excel pair was measured interleaved (0.072 s median). The gate grew `f9.calculateNow`
  (100k 82 ms / 180 ms, 1M **832 ms / 1.60 s**) and stands at **31/31**; `Calc::rebuilds` became a
  public diagnostic so the engine test asserts the graph is kept, and a from-scratch rebuild after
  edits that take every graph path is the test's oracle (red first). What is left of F9 is the
  evaluation itself, and round 37's bisect split it: a fresh workbook of the same shape recalculates
  in **414 ms** (the million-deep chain is **104 ms** of it, store-read bound at 60–105 ns a cell),
  and the gate's three whole-column aggregates add **613 / 848 / 773 ms** as they are added — the
  same 139 / 109 / 116 ms they measure on their own. **The aggregate kernels are the pool**, not the
  chain and not the command's wrapper (0.5 ms); that is K1 in `plan/PERF-STAGE-2.md`.
- **Measured (11-23, round 40): the three-way table grows to fifteen actions — and shows four
  places where Cella is behind.** `calc.countif`, `calc.vlookup`, `calc.sumproduct`,
  `struct.insertRow` and `struct.deleteRow` are now in the gate (**41 rows**) and replayed in Excel
  and WPS. On 1M rows: **VLOOKUP 68 ms against Excel's 7.3 (9x)**, **COUNTIF 47 against 17**,
  **SUMPRODUCT 91 against WPS's 62**, and **inserting a row above two million formulas 1.92 s
  against Excel's 0.96 s** (delete 1.90 against 0.87). The first two are the round-39 `XLOOKUP` fix
  not yet applied to `VLOOKUP`/`COUNTIF` (their whole-column paths still materialise the column);
  the row insert rewrites every reference below it where Excel moves rows and adjusts the graph
  incrementally. Each is a round of its own; the gate now refuses to lose ground on any of them.
- **Measured (11-23, round 39): a whole-column `XLOOKUP` stops building the column.**
  `aggregate`'s lookup read `A:A` into an `Array` before comparing anything — 75 ms to build a
  million `Value`s, 45 ms to scan them — so a needle on the first row paid for the whole column.
  The evaluator's reference-returning `xlookup` now answers the narrow shape (a literal text needle
  without wildcards, three arguments, one column each over the same rows) by walking the stored
  cells and returning a reference to the matched row: **`aggregate` 184.7 → 126.7 ms (−31.4 %,
  6/6, p = 0.016)**, **`edit.recalc` 344.5 → 283.2 ms (−17.8 %, 6/6)**; the fast path declines a
  needle from a cell, wildcards, a mode argument, a formula or a spill in the column, and a number
  needle, and a differential test holds both spellings equal. Gate **31/31** (`aggregate` 125 ms /
  11 %, `edit.recalc` 312 ms / 22 %). Against Excel's 50 ms the aggregate is **2.5x** now, from
  3.5x; what is left of it is `SUM` (~55 ms) and `SUMIFS` (~63 ms), one packed pass each. The same
  shape is still open for `MATCH`/`VLOOKUP` over a whole column.
- **Measured (11-22, round 37): a whole-column read stops building a `Cell` a cell — the aggregates
  were the pool.** `edit.recalc` 1M 610 ms and `aggregate` 364 ms were mostly three whole-column
  formulas (139 / 109 / 116 ms each), not the chain (104 ms) and not the command wrapper (0.5 ms).
  `CellStore::visit_values` walks values without a `Cell` (113 → 50 ms a million cells),
  `PassHost::settled_in` stops the block builder asking the store which cells hold formulas, and
  `visit_packed_settled` folds a settled formula column from packed values, and `SUMIFS` now walks
  its criteria and target columns in one pass: **`aggregate` 348 → 175 ms (−49.7 %), `edit.recalc`
  567 → 400 ms (−29.5 %)**, i.e. 6.97x → 3.5x and 4.39x → 3.1x Excel, gate **31/31**. Left:
  the chain's operand read went next (a column cursor: `edit.recalc` 693 → 594 ms, −14.3 %, 6/6),
  and `XLOOKUP`'s million-text scan (~100 ms of the 175) is what remains of the aggregates. The
  values-only edit went before that: the fill now
  reports whether it wrote or replaced a formula (`Band::put` answers it, `Sheet::set_cell_reporting`
  passes it up), so the generic commit's 135 ms probe a million cells is skipped — **`fill.autofill`
  550 → 388 ms (−29.5 %, 6/6)**, pinned by a regression test and the fuzzer. Left: the store write
  273 ms of that 388, the chain's operand read (40–60 ms), the copy-on-write a write pays
  (60–120 ms).
- **Measured (11-22, round 35): the external table re-run in full** (`plan/bench/external_ab.ps1`
  and `f9_ab.ps1`): open **2.69 s** against Excel 8.67 / WPS 10.86; edit **0.270 s** against 0.236 /
  16.36 — the honest "1.1x", not the 4.4x the old documents quoted; memory **WS 879 MB** against
  580 (1.5x) and **private 1,137 MB** against 530 (2.1x), ~110 MB lighter than the 11-21 build
  (988 / 1,249) thanks to round 34. Details: docs/external-benchmark.md.
- **Landed (11-21, round 34): the formula text is no longer stored per cell — −108.1 MB.** A
  filled formula's text is `Shape::print_at(at)`, the invariant the shape work already pins, so
  `Formula` is **48 → 32 bytes** and `text` (`Option<Box<String>>`) exists only for a formula with
  no shape (a JSON document's, an unparseable one) or a shape that clamps at its cell. **Live heap
  884.7 → 776.6 MB (−12.2 %)**, reopened 683.8 → 614.4; the `formula text` line 133.4 → 64.0 MB and
  `undo` 212.4 → 173.8 (the snapshot shares the smaller formulas). Reads go through `text_at` /
  `input_text_at`; the XLSX writer prints at save inside its parallel row windows, the JSON writer
  materialises the text into the `Value` tree (package bytes unchanged, the golden test holds) and
  the reader re-interns shapes; equality compares a shape by spelling; the structural rewrite's
  per-cell fallback gained the old positions it needs from `cell_before` (block move, cell
  insert/delete), which fixed a column move that left a reference pointing at the hole and an
  `insertCells` that re-shifted formulas twice. model 172 / calc 174 / xlsx 479 / engine 633 green.
- **Landed (11-21, round 33): the fill's per-command overhead, −27.14 %.** `fill.values`
  was the tightest scenario in the gate (69 % of budget), and attributing it first showed it is
  **a metric, not the user's action**: a hundred `range.setValues` calls spend 34 % of their
  time in per-command overhead and 16 % in the harness's own JSON, while a real fill handle
  (`edit.autoFill`) drags one value down 1M rows in **157 ms**. Two knives on the overhead:
  a one-way `wrote_formula` flag makes `has_formulas` O(1) (the commit's scan 137 → 6 ms,
  and a sort's `detect_formulas` too), and a bulk value write refits only the rows whose height
  can change (autofit 243 → 1.2 ms, with a null that must restore a wrapped cell's height and an
  Alt+Enter text whose new wrapped style must still grow the row). **1M 1,464 → 1,067 ms
  (−27.14 %, 8/8, p = 0.0039)**, 100k −19.84 %, re-certified independently at −23.85 %; gate
  **29/29** and `fill.values` now at 1.29 s (42 %). The gate grew **`fill.autofill`** — the drag
  itself, 1M 449 ms in a workbook already holding eight columns — because the values half of a
  fill had no number at all. `fill.formulas`, `fill.chained` and `edit.recalc` were unchanged and no
  credit is claimed for them.
- **Landed (11-21, round 32): the closure's seeds, −13.33 % more.** A 1M sort hands the dirty
  closure 8M changed cells as seeds, and **6M of them are plain values that can never hold a
  formula node** — each paid a `has_node` probe, 323 of the closure's 541 ms (dependents 143,
  dedupe/queue 75). The sort already knows which cells hold a formula, so it passes that list
  (`Calc::cells_moved_seeded`; every other call site passes `None`): seeds skip the probe, but
  **every seed still has its dependents expanded**, so a moved value's formulas recalculate and a
  covered formula cell is queued even when nothing feeds it. One shape mattered: the list has to
  be filled in **write-back order** — gather's source order scattered the 2M seeds across the
  graph and measured **686 ms, worse than the probes**. Closure **541 → 213 ms**, `sort.formulas`
  **1M 3,360 → 2,912 ms (−13.33 %, 8/8, p = 0.0039)**, 100k 309 → 289; gate 27/27 with
  `sort.formulas` at 3.02 s. Left: dependents expansion 143 ms (a column hash a cell, whole-column
  intervals) and dedupe/queue 75.
- **Landed (11-21, round 31): the sort's formula side, −28.94 %.** The gate only saw a values
  sort, so a new scenario (`sort.formulas`, sorting `A1:H` by column B with the `G`/`H` formulas in
  the block) was added before the work: **4.19 s at 1M** against 1.91 for the values sort, and the
  attribution put **1.66 s of that 2.64 s difference into bookkeeping** (graph re-index 1,170 ms,
  re-printing 487) and 0.36 into real recalculation. A sort now re-indexes **only the addresses
  whose formula shape changed** — a permutation that hands an address back the same shape needs no
  node work at all (pointer identity per row, at most eight patterns, per-cell fallback;
  `Calc::cells_moved`) — and `print_at` stops paying `core::fmt` a reference. **1M `sort.formulas`
  4,613 → 3,278 ms (−28.94 %, 8/8), 100k 421 → 316 (−24.94 %)**, values sort and `edit.recalc` in
  the noise; gate **27/27**, `sort.formulas` at 3.44 s (40 %), embedded 2.13–2.35 → 1.66 s. Left:
  the dirty closure over 8M seeds (591 ms; it cannot tell which changed cells hold no formula),
  `write_cells` (854 ms at the values path's per-cell price), and ~140 ns a formula of `String` +
  `Arc` churn that the one-text-a-cell model implies.
- **Landed (11-21, round 30): the sort's gather and write, −27.3 % more.** Reading the body in
  **source order** instead of destination order removed one random band hop a row (gather 789 →
  228–281 ms), and `Band::put` stopped re-walking a band's stripe offsets (384M row visits a sort)
  when a cell's run lengths did not change (write 819 → 445–469 ms): **1M `sort.desc` 2,656 →
  1,931 ms (−27.30 %, 8/8, p = 0.0039, t = −40.19)**, 100k 250 → 203 (−18.85 %, 8/8), bystanders
  in the noise. Rejected on measurement: rebuilding whole rows at band level (750 ms and
  478–580 ms against the 455 ms per-cell path). With round 28 the gate's 1M sort went **3.80 s →
  2.06 s**; gate 25/25. Still open: the moved-formula relabel for a formula body (embedded
  2.13–2.35 s against values 0.64 s, invisible to the gate), and the write's remaining ~450 ms.
- **Landed (11-21, round 29): a memory regression a performance win had borrowed, found by
  re-measuring.** The live heap reported **1.23 GB** against the 884 MB record and the whole
  difference was the `history / undo` line, because round 26's deferred release keeps up to two
  evicted snapshots for a **later cheap command** to free — and a user who does one bulk edit and
  stops never issues one. `Session::release_deferred()` (plus `has_pending_release`) is drained by
  `SheetApp::release_idle` from the first line of the UI's per-frame `logic()`; the report grew a
  `pending_release` line so `undo` no longer carries it. **884.7 MB / undo 212.4 / 340.4 MB
  released**, no command-path change (fill.values −0.91 %, edit.recalc +0.37 %, both
  INCONCLUSIVE; gate 25/25; engine 624; the UI interaction suite 45/45). Honest limit: a headless
  host with no frame loop keeps the two snapshots unless it calls the method — documented, and
  wiring it into MCP is that crate's change. **What the 884.7 MB is:** cells 371 (formula text
  133), graph 286, one undo snapshot 212, journal 15 — graph and undo buy speed and are refused
  trades; formula text is the one structural piece left (~100 MB, its own round).
- **Landed (11-21, round 28): the 1M sort, −12.6 % certified.** The gate's `sort.desc` went
  **3,013 → 2,633 ms (8/8, p = 0.0039)** at 1M and 278 → 247 at 100k (7/8) in the eight-pair sign
  test: the store's row write takes `Band::put`'s answer instead of ranking the row before and
  after every cell, `row_or_insert` asks the O(1) `row_bits` bit before the binary search, a batch
  already in column order is not re-sorted (write 738 → 652 ms); and a sort whose body holds no
  formulas tells the recalculation the graph did not move, skipping 6M per-cell `formula()`
  probes and graph removals (282 → 12 ms). An in-place cycle permutation was built and **reverted
  on measurement** (write order jumped across bands: 2,989 → 3,877 ms). What is left: the
  moved-formula relabel for a body that does hold formulas (0.29–0.37 s; needs a precedents
  template on each `Shape`), and gather 706 / write 652 ms (a band-level row-replacement kernel).
- **Missing:** dependent chains that never become a wave of leaves remain sequential; whole-sheet
  clear/copy/paste still clone the stored cells (they scale with the data, not the file); the
  per-band value dictionaries still cost full price when values do not repeat;
  page-aware compaction and the 64 MiB byte-budget page cache landed 10-29 (identity early-out,
  gap-scoped candidates, snapshot-safe page replacement, `document.compact` and a save hook,
  `PageCacheStats`); a page-file rewrite remains;
  whole-column references are already clipped (`SUM(A:A)` folds a million numeric cells in 21.5 ms,
  a ten-row used range in 0.0 ms). The cheap wins that round measured were taken on 11-14, again
  measured before and after: `MODE`/`MODE.MULT` counted with a linear scan (`MODE` over 100k
  distinct values took **2.66 s**, ~5·10⁹ comparisons) — now **15 ms**, a hash count that keeps the
  first-occurrence order; `VLOOKUP`/`HLOOKUP` copied the whole key column per call and `MATCH`
  cloned the haystack before the host's text index could answer — the searches read the column in
  place now (**VLOOKUP exact 2.9×, approximate 4.9×, MATCH 7.2×**), and `XMATCH`/`XLOOKUP` use the
  index too; `criterion::matches` cloned the cell and re-tokenised its wildcard pattern per visited
  cell (**wildcard `COUNTIF` 2.0×, `SUMIFS` 1.4×** — a numeric `SUMIF` was already allocation-free
  and does not move); and the grid's per-cell path stopped cloning a `Cell`/`Style`, formatting a
  font-family name, copying one-line text and laying out `"0"` per cell (**grid paint 1.60 → 1.07 ms**
  min, 2.25 → 1.25 ms median on a 1,600-cell frame; the offscreen snapshot is byte-identical).
  What is left there: a *borrowed* cell accessor in the store (the UI reads `sh.spill` itself to
  stand in for `Sheet::value`), `display_text`'s per-cell `String`, `check_arrays` still walking the
  store on a sheet that holds a multi-cell array, and `VLOOKUP`'s linear scan for the first match,
  which is its semantics. The 159 cells that still differ from the cached values of the
  1.28 GB report are Excel's stale cache — 276 cells of the same `IFERROR(XLOOKUP(...))` shape in the
  same workbook cache `#N/A` — not a calculation difference.
- **Evidence:** `crates/xlsx/src/sheet_read.rs`, `crates/model/src/store.rs`, `crates/model/src/line.rs`,
  `crates/engine/src/memory.rs` + `crates/engine/examples/memory.rs` (the live-heap report),
  `docs/roadmap.md` (M17–M21), and the harness kept beside the fork (`_gridcraft-bench/`).
- **Impact:** finance and data users routinely have big workbooks.
- **Status (10-29): the large-workbook beta gate is met.** Recalc, edit latency, 1M-row operations,
  bounded data commands and memory all pass their targets; M19 cold paging and the M20 row/column
  maps landed. Remaining work is base hardening, not capacity: M19 spill placement, flat-memory
  save/scan (and the deferred page-file rewrite) are wave 0 in
  [`roadmap.md`](roadmap.md#execution-queue). The M21 Parquet read half is post-beta. The old
  estimate is superseded by those waves.

### 3. Some unmodelled XLSX content is dropped on save (FF)
- **Landed (11-13, found by the command-sequence soak at 120 steps):** three ways a sheet with an
  applied filter saved rows that the model did not hold. The filter's hidden rows are a bitmap over
  logical rows, and a row insert or delete moved the line map but not the bitmap (the rows below an
  insert stayed hidden one row early); AutoFit wrote a height for a row the filter is hiding, so the
  row ended up in the line map and the bitmap at once, and the file wrote `hidden` with a height the
  reader then could not turn back into a filtered row (fixed on 2026-11-17: `heal_filter_hidden` moves
  only the hidden bit into the bitmap and leaves the height where it is, so the file bytes are
  unchanged); and a delete could leave a filtered row on a table's
  totals row, which no filter covers. Fixed by `FilterHidden::insert_rows`/`delete_rows`/`retain`,
  an autofit guard, and `Sheet::filter_row_ranges` (now also what the reader's
  `heal_filter_hidden` uses), each with a save/reload regression test. `cargo test --workspace`
  green (2783 → 2786 with the new tests).
- **Landed (11-20, found by the 256 × 200 release soak at seed 173 — the same defect as the
  seed-97 case, with a wider trigger surface):** a sheet AutoFilter and a table filter can both
  exist on one sheet, and when a table overlapped the sheet's AutoFilter the writer dropped the
  whole `autoFilter` element (its fold handles only a filter fully inside one table) but still
  wrote `hidden="1"` for the bitmap rows — so a reload left hand-hidden rows that no filter owned
  (`Grades (2): rows differ after reload`). `written_filter_ranges` in `sheet_write.rs` now writes
  hidden bitmap rows only inside the ranges a reloaded sheet's `Sheet::filter_row_ranges` can
  express, so save/reopen agrees; a sheet whose filter is live is byte-identical to before. The
  wider path needs no column delete — `sheet.move copy:true` + `data.filter` + two `insert.table`s
  + autofill + a three-step `edit.undo` + `data.filterBy` reproduces it in eight steps from
  `file.new {"sample":"grades"}`; the seed-97 six-step case (`CELLA_FUZZ_SEED=97
  CELLA_FUZZ_STEPS=120`) is the same defect. Regression:
  `a_sheet_copied_under_its_own_filter_keeps_its_rows_through_a_save` (red first). **Still open,
  the design question:** which filter owns the bitmap — a *suppressed* sheet filter's hidden view
  is lost on reload (the rows come back visible; the format cannot express it), so restoring the
  view needs a re-filter (2–4 h).
- **Landed (10-30, fills and pastes):** a whole-row/whole-column fill narrowed its target to the
  *sheet's* used-range box, so one stray cell far down the sheet made `Ctrl+D` over two columns write
  1,048,576 rows (2,097,102 cells where the sheet held 84, 830 ms); it now narrows to the content in
  **its own lines** (spilled arrays included), 50 cells in under a millisecond. `pasteSpecial`
  looked a moved cell up by scanning a list (`O(cells²)`): 25/90/332/1,322 ms at 10k/20k/40k/80k rows
  → 10/20/67/97 ms. Both pinned by time tests with 10× budgets. Measured with them: structural edits
  scale linearly (delete an empty column 0.42/2.4/18.7/32.9 ms at 20k/200k/1M/2.1M cells; a filled one
  2.6/25.1/157/293 ms), 2.1M unique cells hold 171 MB (≈75 B/cell), and a **save transiently doubles
  cell memory** because the writer serialises through a second, unshared copy.
- **Landed (10-29):** pass-through keeps slicers and their caches, timelines and their caches,
  table-attached query tables, and unknown `extLst` children on the workbook, worksheet, table,
  PivotTable and pivot cache definition. Part bytes come back byte-identical, rels are re-pointed
  with their ids preserved, and every kept fragment is keyed to its owner (`table:`, `pivot:`,
  `pivotcache:`): a deleted, renamed or replaced owner drops its extras, the parts only they reach
  and the rels in the same save, so no dangling relationship is written. A strict package validator
  (every rel target exists, every part has a content type, no orphans, no case-only duplicate
  names, no stale `Override`, no duplicate rel ids) runs inside `write_xlsx` for every save in the
  crate's tests. The earlier set (#330) still holds: custom XML, external links, connections, sheet
  query tables, printer settings, controls/ActiveX/OLE/backgrounds, header-footer pictures,
  workbook and worksheet `extLst`. **Chart sheets are modelled since 11-14** (they used to be kept
  whole and invisible): the part is read into a sheet of its own kind and written back as a real
  `chartsheet`, with page setup, margins, header/footer, printer settings and `extLst` kept as
  fragments; a part whose chart is missing or unmodelled still falls back to the passthrough rather
  than being half-modelled.
- **Missing:** the data model (Power Pivot `xl/model/…`, `powerPivotData`), chart `extLst` and chart
  details (a chart part has no stable identity to key a fragment on; chart depth is #4), Strict Open
  XML content. A renamed table or PivotTable loses its extras by policy — misattaching a query table
  is worse than dropping it — which is a product decision worth revisiting.
- **Not verified:** whether Excel opens a re-saved slicer/timeline workbook without a repair prompt
  (needs the owner's Excel half-day, #1). No Excel run has happened.
- **Evidence:** `crates/xlsx/src/passthrough.rs` (what is kept, and why the rest isn't);
  `crates/xlsx/src/testing.rs` (`check_package`); the matrix in
  [file-format-parity.md](file-format-parity.md).
- **Impact:** open-edit-save no longer simplifies workbooks with slicers, timelines or query tables;
  Power Pivot workbooks still lose the model.
- **Estimate:** 0–2 h left of the 6–10, plus the Excel check folded into #1.
  [file-format-parity.md](file-format-parity.md)

### 4. Charts lose options on round trip; shallow chart model (F, FF)
- **Landed (10-29):** the model carries, and reads/writes/draws, axis scaling (min, max, major unit,
  log base, reversed, crossing, display units, number format), data labels (value, percentage,
  series name, category name, legend key, position, separator, number format, per-series visibility,
  per-point deletion), error bars per series (fixed, percentage, sample standard deviation, standard
  error, custom `numRef`/`numLit`, both directions, `noEndCap`) and per-point fill/line/marker
  colour. Four commands: `chart.setAxis`, `chart.setDataLabels`, `chart.setErrorBars`,
  `chart.formatPoint` — a chart kind that cannot store an option refuses it rather than holding a
  dead value. Catalog 274 → 279 / 290 → 295.
- **Landed (10-29, second round):** an edit **patches** the chart part instead of regenerating it
  (`crates/xlsx/src/chart_patch.rs`): only the changed element is rewritten, so everything we do not
  model survives an edit — manual plot-area and label layout, label callouts and leader lines,
  `dTable`, `serLines`, `overlap`, date-axis units, minor units and gridlines with their formatting,
  axis `spPr`/`txPr`, legend entry and title-run formatting, series `extLst`, `printSettings` — with
  the test asserting `part.replace(<patched element>, "")` equals the original bytes. Charts of
  unmodelled kinds (3-D, surface, pie of pie, bar of pie, Pareto, filled map) are kept whole
  (`Sheet::kept_charts`, `Obj::RawChart`) instead of being dropped or flattened. The secondary axis
  is modelled (`Chart::y2_axis`), written, patched, drawn and addressable as `chart.setAxis axis:
  "y2"`; category-axis bounds crop the drawn window.
- **Landed (10-30, UI half):** charts, pictures, shapes and slicers resize from all eight handles
  (before, only the bottom-right corner resized and any other drawn handle *moved* the object): one
  `object.move` per frame carrying the anchor and the extent, Shift keeps the aspect ratio, 20 pt
  minimum, Esc cancels without an undo entry, one drag is one undo step, arrow keys nudge 10 pt
  (1 pt with Ctrl) and the size survives save/reopen. The **Format pane** drives
  `chart.setAxis`/`setDataLabels`/`setErrorBars`/`formatPoint` for the chart area, either axis and
  each series, with per-field Auto; an option the chart kind cannot store is not offered and the
  section shows the command's reason (chartex charts, a pie's axes, bounds on a label axis, error
  bars on a radar).
- **Landed (10-30, chartex + point labels):** a modelled **chartex** chart (histogram, waterfall,
  box & whisker, funnel, treemap, sunburst) is now **patched** instead of regenerated — an edit
  rewrites only the element it touched (titles, legend position, gridlines, `cx:valScaling`,
  per-series fill/outline and label flags, `quartileMethod`) and every other element, including the
  hidden `_xlchart` name table, stays byte-identical; a change the part cannot express (data
  reference, kind, series set) regenerates it. **Per-point data labels** are modelled (`PointLabel`:
  rich text or a `strRef`, the four show flags, position) on `Series::point_labels`, read from
  `c:dLbl` inside a series and inside a `dPt`, written back with `idx`/`tx`/`dLblPos`/`show*`, drawn
  at the right point and exposed as `chart.setDataLabels` with `point` + `text`.
- **Corrected (10-30):** the claim that a chart's custom error-bar references are lost on an edit was
  **wrong** — they already survived read → edit → save byte-for-byte; a regression test now pins
  read → patch → save → read.
- **Landed (10-30, style parts):** a chart's own `styleN.xml`/`colorsN.xml` are **read and written back
  byte-identically** for both chartex and classic parts (classic parts with style/colour relationships
  are now kept too, which real Excel files usually have); they survive a field patch and are dropped
  only when the part must be regenerated (a kind, series-set or reference change), which is stated in
  `chart-parity.md`.
- **Settled (10-30):** a **per-point label prefix/suffix does not exist in the schema** — `c:dLbl`
  carries `idx`/`delete`/`layout`/`tx` (`strRef` or rich text)/`numFmt`/`spPr`/`txPr`/`dLblPos`/show flags/
  `separator`, and chartex's `cx:dataLabel` has **no text element at all**. A fixed rich text replaces the
  composed label, which is exactly what `PointLabel::text` already does, so the claim is retired rather
  than faked.
- **Landed 11-02 (secondary axis and series order):** `chart.setSeriesAxis {series, secondary?,
  type?, subtype?}` moves one series to the secondary value axis and optionally changes its kind
  (the model side of Change Chart Type → Secondary Axis), turning the chart into a combo; the
  Format pane's Series Options carries the checkbox. Reading a combo chart back used to return its
  series in the writer's **group order** (primary bars, primary lines, secondary), so a series
  index from before a save pointed at another series after reopen — the reader now sorts by
  `c:order`/`c:idx`, and a secondary line group keeps its markers when written.
- **Landed 11-02 (per-point marker size):** `PointFormat::marker_size` (2–72 points) is read from
  `c:dPt/c:marker/c:size/@val`, written, patched, drawn at that radius and settable from
  `chart.formatPoint {markerSize}` and the Format pane's Data Point section (default 5). A point's
  own marker now writes even when the series marker is switched off.
- **Landed 11-02 (click-select):** a chart element is selected by clicking it, with the pane's
  dropdown reaching the same elements: the chart is laid out into its primitives **and the boxes a
  click can land on** (`cella_chart::draw` → `Drawing { prims, hits }`), so the hit test is the
  drawing's own geometry and cannot drift from it. A bar, area band, marker, a line's data point,
  a pie/doughnut/sunburst slice (an exact wedge region), a legend entry or an axis label band names
  the series and, when one point was hit, that point; the first click on an unselected chart
  selects the chart, the next picks the element (Excel's two-step gesture), and a click on the
  frame's padding goes back to the chart area. The Data Point section follows the clicked point, so
  a bar can be formatted without touching the point number.
- **Landed 11-02 (label font):** a data label's own **font** — size, bold and colour — is modelled
  at the chart, series and point level (`c:txPr` on `c:dLbls` and on `c:dLbl`), read, written,
  patched, drawn and settable from `chart.setDataLabels {fontSize, bold, color}` and the Format
  pane's Data Labels and Data Point sections. Levels combine **field by field**: a point that sets
  only a colour keeps its series' size and the chart's weight, and the renderer draws the size,
  weight and colour it ends up with. **A chartex chart's label font is modelled since 11-14** in
  the series' own `cx:txPr` (chart → series → point, field by field, drawn), together with per-point
  `cx:dataPt/cx:spPr` fills and outlines and the label position and visibility flags; what chartex
  has no element for (percentage, separator, per-point text, markers) is still refused with the
  reason. The typeface, italic and the label's own fill/callout stay unmodelled
  (kept while their element is untouched, lost when that label's font is edited).
- **Missing:** a label's typeface, italic, fill and callout shape are not modelled; date-axis units
  are kept but not editable; from-cells labels, callouts added through the UI, chart templates and
  an element-level pane for shapes/pictures are not there.
- **Evidence:** `Chart` (`crates/model/src/features.rs`) holds the option families above;
  `chart_options_roundtrip` and `chart_unmodelled_kinds` pin build → save → reopen → second save
  byte equality; the patch path is pinned per chart kind; `ui-egui` `chart_format_pane` and
  `chart_resize` (11 tests, including save → reopen of a dragged chart) pin the UI half. Smooth
  lines (#62), Switch Row/Column (#116) and trendlines (read, written, drawn) landed; multi-level
  category axes (year beside month) are read, drawn and written back.
- **Impact:** charts are in most business workbooks.
- **Estimate:** 0–2 h for beta (30–45 h full). [chart-parity.md](chart-parity.md)

### 5. Text in non-Latin scripts doesn't render or export (L)
- **Landed 10-29 (font discovery + PDF embedding):** `cella-core::fonts` finds a font file
  (`CRAFT_FONTS_DIR`, then the platform's font directories, plain `read_dir`, no fontconfig) and
  answers per-character coverage from `cmap`; `cella-pdf` reads TrueType, subsets it (composite
  closure, rebuilt tables) and embeds a `CIDFontType2` with `FontFile2`, `Identity-H`, `/W` and a
  `ToUnicode` CMap, so Latin Extended, Greek, Cyrillic, Hebrew, Arabic, Devanagari, Thai, CJK and
  Hangul text reaches the page instead of vanishing. ASCII/WinAnsi documents keep their previous
  bytes because the engine only resolves a font when a character falls outside WinAnsi.
- **Landed 10-30 (shaping engine):** `cella-core::text` resolves paragraph levels and reordering
  (`unicode-bidi`) and shapes each run (`rustybuzz`), returning positioned glyphs in **visual order**
  with logical UTF-8 byte clusters, mark offsets and a bounded cache. Arabic joins and ligates,
  Devanagari reorders and merges conjuncts, Thai marks take zero advance, `fi` ligates and RTL
  mirrors brackets; 26 tests pin levels, visual chunk order, cluster tiling and hostile input.
- **Landed 10-30 (both paths wired):** the **grid** draws each cell's bidi runs in visual order
  (one engine per app, registered on the faces egui paints with, chosen by a bounded cmap probe so
  the per-char fallback cannot substitute a glyph the engine did not position), AutoFit measures the
  shaped width, the **in-cell editor** walks the caret through visual clusters and maps a click to the
  cluster under it, and a Devanagari fallback face makes Hindi cells draw at all. **PDF export**
  shapes too: glyphs are placed with `TJ`/`Ts` on one text matrix, `ToUnicode` maps the glyph that
  carries a cluster to the whole logical cluster text (a ligature never loses a character, a
  multi-glyph cluster never repeats one) and a `CIDToGIDMap` is written only when one glyph must
  extract differently — so WinAnsi text stays **byte-identical**. `pdftotext` now returns Cyrillic,
  Arabic, Hebrew, Devanagari and Thai in logical order.
- **Landed 11-02 (wrapped cells and chart labels):** a word-wrapped cell now shapes like a
  one-line one. Each paragraph is shaped once to measure its clusters (logical prefix widths from
  the shaped glyphs), broken after spaces, tabs and hyphens and between wide CJK characters, and
  each resulting line is shaped on its own, so its bidi runs, joining and marks are resolved for
  that line; a word wider than the cell breaks at the furthest cluster boundary that fits, so a
  line always progresses. General-aligned right-to-left wrapped text aligns right, and each line
  is aligned on its own (centre/right take the line's width off), as the old `LayoutJob` did.
  Printable ASCII keeps the unshaped wrap bit for bit, and text over 16,384 bytes or a face that
  cannot cover it falls back to it. **Chart labels** (titles, axis and series labels) are shaped
  through the same engine, and the chart layout measures them with the shaped width, so a label's
  box and its glyphs agree. Shape and text-box text is shaped and wrapped the same way (11-02),
  with the outline halo drawn per line. The PDF's `text_width` remains.
- **Still missing:** CJK and Arabic in one document (no single face covers both), fonts shipped per
  platform, per-run font fallback, mirrored printing and right-to-left sheet tabs, and shaped
  *measurement* in the PDF's `text_width`; CFF/OTF (`CIDFontType0`) and TTC faces past index 0 are not embedded; a
  mixed-direction line cannot extract back to source order through an extractor that re-applies the
  UBA (documented, not fixable in the file for Xpdf 4.00).
- **Evidence:** the matrix in [localization-parity.md](localization-parity.md); #142, #54; UI 997
  tests (18 new), PDF 43 (10 new), `pdftotext` samples per script.
- **Impact:** text in Arabic, Hebrew, Devanagari and Thai now draws and prints in the right order;
  what remains is font coverage and the shapes of layout (wrapping, labels, mirrored print).
- **Estimate:** 2–5 h left of the beta slice (font coverage and the RTL print flag); the rest is
  localization work. [localization-parity.md](localization-parity.md)

### 6. Calculation correctness bugs (F)
- **Closed 10-30.** Missing: nothing known. The four this entry tracked have landed: 15-significant-digit
  equality (#57, so `=0.1+0.2=0.3` is TRUE and MATCH and COUNTIF agree), spill updates after
  structural edits and cleared blockers (#133, #139; PRs #148, #157), 3-D span deletion (#140),
  and qualified names after a sheet rename (#134). Iterative calculation for circular references
  landed too (no Options dialog yet, which is §13's).
- **Soak (10-30):** a 400-seed × 200-command release soak found class-level defects the reported bugs
  never showed. Fixed here: a `u16` payload index in the cell store's per-row wide table **wrapped past
  65,535 entries** on a row rewritten thousands of times, so rewriting one cell's text aliased another
  cell's value (a *full* recalc disagreeing with memory is what exposed it; the table now compacts to
  one entry per distinct value its row still uses); a formula that read its own cell was marked
  `#CIRC!` only after the pass, so later formulas in the round read its pre-override number and an
  eight-round cap could leave a spill from one state with cells from another; a `COUNTIF` reading a
  spill the same pass discarded as circular; a defined name reading a discarded spill (a name hides its
  range from the dependency graph, so dynamic formulas and used-range readers are re-evaluated on any
  spill change); and `#SPILL!` after an edit moved the used range. Each has a direct regression test
  that asserts the two paths agree **and** the right value.
- **Soak (11-18): a formula that Remove Duplicates moved kept its stale shape.** Remove Duplicates
  copies the rows it keeps to the top of its range instead of deleting rows, so a formula inside the
  range lands on a new row. Its interned *shape* — the tree whose relative references are offsets from
  the cell the formula was written at — stayed where it was, so the cell resolved its references from
  the old row: `=XLOOKUP(D15,G1:G20,G1:G20)` written at J11 and compacted to J10 read from row 11 and
  showed `#REF!` in memory, while the file (which holds the text only) calculated 0 after a reload,
  the same file both times. A moved formula is re-based at its new cell now — the same absolute
  references, carrying the file-rewrite hint, the array range and the dynamic mark — and `data.sort`
  was already correct because it routes every moved cell through `Formula::copied_to`. Regression
  test: `a_row_compacted_by_remove_duplicates_keeps_the_references_its_text_names`; the sweep that
  found it (8 seeds × 150 steps) and a 16 × 120 sweep pass.
- **Soak, save/reload (10-30):** an array entry over more cells than the evaluator lays out left the
  array `#NUM!` while cells inside its declared range kept values that the reader's own array rule
  drops — the entry is refused now (as Excel refuses it), so that state cannot be created; a written
  `defaultColWidth="0"` was silently dropped by the reader, and `home.columnWidth` accepted widths
  past the format's 255-character maximum, which were saved as 1785 px behind the user's back; the
  legacy-array entry now marks every cell it clears as a change, so a formula reading part of the
  range recalculates instead of keeping a value a full recalc disagrees with.
- **Soak, names (10-30):** a table resized over a blank column kept **two columns named `Column3`**
  (the reconciler deduped through a name→column map that cannot hold a duplicate), and the writer's
  documented repair then renamed the later one to `Column32` **at the file boundary** — a rename by a
  rule nothing in memory had applied, which no undo can reach. One function owns the naming rule now and
  the whole header row is reconciled after a structural edit, so model, cells and file cannot disagree.
- **Oracle suite (10-30):** `crates/calc/tests/formula_oracle.rs` runs **595 documented examples over
  275 functions** through the real calc path (`cella_calc::evaluate`), plus dedicated tests for
  real spills, `#SPILL!` unblocking, the 1904 date system, 3-D references and qualified names. Each
  row cites the published example it comes from. It is **documentation-derived, not
  Excel-computed**, and it is labelled that way: it can say the documented behaviour holds, not that
  installed Excel agrees. It found no implementation defect — every disagreement was a
  transcription error resolved by the documented rule. An Excel-computed corpus run stays part of
  gate #1.
- **Impact:** wrong numbers are worse than missing features.
- **Estimate:** 2–5 h, all of it re-running the suite against an Excel-computed corpus; Excel 16 is
  available now, the corpus is gate #1's. [function-parity.md](function-parity.md#formula-language)

### 7. Legacy `.xls`: OLE objects and chart sheets; encrypted `.xls` cannot be written (FF)
- **Landed 10-29 (encryption write):** `file.save`/`file.saveAs`/`file.saveBytes` write **Agile**
  encryption (AES-256-CBC, SHA-512, spin count 100,000, random salts, HMAC-SHA512 integrity, a
  version-3 compound file), and a workbook opened with a password re-encrypts with it on a plain
  `file.save` — the silent loss of a file's protection is fixed. `password: ""` removes protection;
  a password on a format that cannot carry one is refused, never dropped. Standard (RC4/AES-128) and
  certificate encryption are not written.
- **Landed 10-30 (Standard encryption write):** `file.save`/`saveAs`/`saveBytes` take `encryption`
  (`agile` by default, `standard` on request; an unknown name is an error listing both), and a
  workbook opened with a Standard file **re-saves as Standard**, so a save never silently changes the
  form. Standard is [MS-OFFCRYPTO] 2.3.2 — a 224-byte binary header (AES-128, SHA-1, `PROV_RSA_AES`)
  and AES-128-**ECB** over the zero-padded package, with **no integrity field** — not Agile's
  segmented CBC, which is what the earlier sketch assumed and would have produced files our own
  reader refuses.
- **Landed 10-30 (encryption hardening):** keys and salts come from the **operating system's CSPRNG**
  (`getrandom`, native targets only) instead of stretched SipHash seeds; the Agile `dataIntegrity`
  HMAC-SHA-512 is **verified on read** after the password check (both fields decrypted with the
  package key under their own block keys' IVs, HMAC computed with the `keyData` hash over the
  plaintext package, constant-time compare, and the segments must cover the stated package size), so
  a flipped byte, a tampered key or a truncated package is an error; `DecryptError::Corrupted` is a
  distinct failure, so a damaged file no longer reads as a wrong password. The landed writer had
  encrypted those two fields with a password-derived key and one shared block key, which
  [MS-OFFCRYPTO] 2.3.4.14 does not allow — Excel would have rejected our files, and our own verifier
  Excel's; both directions are fixed. The **web build refuses** a password-protected save
  ("needs a secure random source this build does not have") rather than writing predictable salts,
  because `getrandom`'s `wasm_js` backend would add JavaScript glue and the repo is Rust only. An
  independent CFB reader (a dev-only dependency, no shared code) validates the written container at
  every size class and through the DIFAT chain, and found a real defect: the mini FAT's last sector
  was zero-padded, and a zero entry names mini sector 0 (now `FREESECT`).
- **Landed 10-29 (`.xls` styles):** `FONT`, `FORMAT`, `PALETTE`, `XF`, `STYLE` build a real style
  table (family, size, bold/italic/strike/underline, palette colour, number format, fill pattern and
  colours, four borders, alignment/wrap/rotation/indent, protection, named styles), cells pick up
  their XF, and `NOTE` + `TXO`/`CONTINUE` give comments with author and text. Layouts were calibrated
  against an independent BSD-licensed BIFF8 reader over 70 Excel-authored files on the dev machine
  (metadata only; no file or data copied into the repo, per the clean-room rule).
- **Landed 10-30 (`.xls` records):** data validation (`Dv`/`DVal` — seven kinds, eight
  operators, literal and range lists, prompt and error text, the three error styles, dropdown and
  allow-blank flags), conditional formats (`CondFmt`/`CF`/`CFEx` with a full `DXFN`/`DXFN12`
  differential style: number format, font, alignment, borders, pattern, protection), hyperlinks
  (`HLINK`/`HLINKTOOLTIP` through [MS-OSHARED] URL, file and composite monikers with the tooltip) and
  freeze panes (`Pane` + `Window2`, plus the scrolled top-left cell and the active sheet from
  `Window1`). Each is asserted equal to what the `.xlsx` reader produces for the same model, and every
  fixture is a byte array built in code. BIFF5 is refused with a clear message, because its record
  layouts are **not in [MS-XLS]** (§1.6 specifies BIFF8 only).
- **Landed 11-02 (CF12):** `.xls` colour scales, data bars and icon sets are read. The earlier block
  was the `CF12` record's tail: `rgbTemplateParms` is **16 bytes** ([MS-XLS] 2.4.43 says
  `cbTemplateParm` MUST be 16), and `rgbCT` follows it as a `CFGradient` (2 control points of
  `CFGradientInterpItem` + `CFGradientItem`, 2 or 3 points), a `CFDatabar` (`CFColor` + two `CFVO`s)
  or a `CFMultistate` (icon set index + one `CFMStateItem` per icon). Thresholds come back as
  `CfValueKind` (min/max, number, percent, percentile, formula), colours as `Color` (RGB, palette or
  theme with a tint), and the icon-set index maps through the 17 ECMA `ST_IconSetType` names.
  Settled by giving Excel 16 the workbook and reading the bytes it wrote: a colour scale, a data bar
  and an icon set, then variants with number/percent bounds, 5 icons, reverse, icon-only and
  `ShowValue = false`. Two findings the specification alone would have got wrong: Excel writes a
  percentile it was given as a **type-7 threshold with no formula and the number itself** (read as
  that number), and its data-bar `fShowValue` bit is set when the value is **hidden** (the opposite
  of the field's description; `ShowValue = false` wrote `0x02`, the default `0x00`, and Excel shows
  the values on reopening the file). The reader was checked end-to-end by opening three
  Excel-authored `.xls` files and comparing the rules with the `.xlsx` copies Excel wrote for the
  same workbooks.
- **Landed 11-02 (pictures):** `.xls` pictures are read from OfficeArt: every sheet's drawings are
  the concatenation of its `MSODRAWING` payloads (one `OfficeArtDgContainer` tree, possibly split
  across records), and the workbook's `MSODRAWINGGROUP` holds the blip store
  (`OfficeArtBStoreContainer` → `OfficeArtFBSE` → a PNG/JPEG/DIB/TIFF blip, whose payload is the
  UID(s), a one-byte tag and the image bytes). A shape whose `OfficeArtFSP` names
  `msosptPictureFrame` and whose `OfficeArtFOPT` `pib` names a blip becomes an `Image`: the
  `OfficeArtClientAnchorSheet` gives the cell and the offsets in 1/1024 of a column width and 1/256
  of a row height, and its move/size bits map to `AnchorMode` the way Excel writes them (0x0000 =
  move and size, 0x0002 = move only, 0x0003 = free-floating — checked against Excel-authored
  files; the field descriptions suggest the opposite, so the capture is the reference). Charts,
  autoshapes, controls and vector (EMF/WMF) pictures are skipped with one warning; malformed or
  truncated drawings are skipped, never a panic.
- **Landed 11-03 (charts):** embedded charts are read from their chart substream inside the
  worksheet ([MS-XLS] 2.1.7.20.1): the chart group's kind (`Bar`/`Line`/`Pie`/`Area`/`Scatter`/
  `Radar`, with the transpose/stacked/percentage/doughnut variants), each `Series` with its four
  `BRAI` reference formulas (name, values, categories, bubble sizes) decoded through the same
  formula reader as cells, the cached `SeriesText` name, the chart title from the `ObjectLink` with
  `wLinkObj = 1`, and the legend. A chart's anchor comes from the `msosptChart` shape in the sheet
  drawing — the drawing is one tree split across `MSODRAWING` records, and the substreams are in
  shape order. 3-D, surface, bar-of-pie, chart sheets, shapes and OLE objects stay out with a
  warning. **Verified in Excel 16:** an Excel-authored `.xls` with five kinds (stacked column, bar,
  area, doughnut, radar) converts to `.xlsx` and reopens with the same chart count, kinds, titles
  and cell anchors (radar comes back as radar-with-markers, the writer's style); a three-chart
  workbook's exported chart renders with its title, series, category labels and legend.
- **Landed 11-03 (shapes):** autoshapes and text boxes are read into model shapes. Excel 2007+
  writes a DrawingML fragment (`drs/shapexml.xml` in an `OfficeArtTertiaryFOPT` record) for each
  shape, which carries the preset geometry, fill, line, text and text formatting; it goes through
  the same shape reader as an `.xlsx` drawing part. A shape without one (an older writer) is
  rebuilt from its `OfficeArtFSP` type and `OfficeArtClientAnchorSheet`: rectangle, rounded
  rectangle, ellipse, triangle, line, arrow and text box; an unknown type is skipped with a
  warning. The reader also handles two layout facts: a drawing is split across `MSODRAWING`
  records at arbitrary points (a container takes the bytes present), and the last shape's OfficeArt
  can follow a text box's text in `Continue` records (the text reader appends an OfficeArt tail
  found there). **Verified in Excel 16:** an Excel-authored `.xls` with a rectangle ("Box", red
  fill, blue line), an oval, a right arrow, a text box ("Note") and a line converts to `.xlsx` and
  reopens with 5 shapes at the same left/top, the same text, fill and line (a block arrow becomes
  the model's arrowheaded line; a right edge inside a column rounds by a few points).
- **Still missing:** OLE objects, controls and chart sheets (dropped with a warning; OLE has no
  model object); BIFF5 is refused; `.xls` is never written. RC4/encrypted `.xls` output needs a
  `.xls` writer and is not scheduled.
- **Evidence:** #121: Excel 97-2003 (BIFF8) `.xls` files now open (values, formulas, number
  formats, merges, sizes, names, styles, comments, validation, conditional formats (including
  colour scales, data bars and icon sets), hyperlinks, frozen panes, pictures, embedded charts,
  autoshapes and text boxes); OLE objects, controls and chart sheets are not read, BIFF5 is refused
  and `.xls` is never written.
  Password-encrypted `.xlsx` workbooks (Agile and Standard encryption) open with their password
  since #11 and are **saved back protected** with the scheme they had. RC4 is **not** an ECMA-376
  Standard cipher: [MS-OFFCRYPTO] 2.3.4.5 requires AES for an OOXML package, and Excel refuses a
  hand-made RC4 package at both 40 and 128 bits (tested 11-03), so RC4 belongs to Office binary
  documents (2.3.5/2.3.6) and an encrypted `.xls` write waits for a `.xls` writer. Encrypted `.xls`
  (FILEPASS) is still refused with a message.
- **Impact:** common in finance, government and anything older than 2007; encrypted files are
  routine in business.
- **Estimate:** 0–2 h for beta (`.xls` import is met: values, formats, pictures, charts and
  shapes are read and verified in Excel 16). OLE objects need a model object the app does not
  have, and a chart-sheet reader is 2–4 h, both full scope rather than gate work; encrypted `.xls`
  write is out of scope without an `.xls` writer. [file-format-parity.md](file-format-parity.md)

### 8. PivotTables: data-model pivots and a real-world corpus (F)
- **Landed 11-14 (a data-model report is carried, not dropped).** Opening a Power Pivot / OLAP
  workbook used to lose the report **and the whole `xl/model/` data model** on the next save. Now
  such a PivotTable is read as `PivotSourceKind::DataModel` with a `read_only_reason`, its report
  cells left exactly as the file's cached values, and its `pivotTable`/`pivotCacheDefinition`/
  `pivotCacheRecords`/`xl/model/*` parts reproduced byte for byte (cache ids reserved so a
  computed report cannot collide; the model is related from the workbook, which is what keeps it
  alive when the report is deleted). Every recompute or reshape command — refresh, addField,
  moveField, layout, filter, sort, group, collapse, valueSettings, grandTotals, calculated field or
  item, changeSource, showDetails — refuses with the reason, `pivot.fields`/`pivot.list` report
  `kind`/`readOnly`/`readOnlyReason`, and `data.refreshAll` lists them under `skipped` instead of
  failing. `pivot.delete` still works. **What a real data-model pivot still needs is N3** (see §12):
  a records source, typed cache fields and measures, OLAP filter/sort/group semantics, a refresh
  that queries the model, and a writer that regenerates the report instead of carrying it.
- **Landed 10-30 (slicers and timelines):** a slicer and a timeline are **model objects** now — cache
  field, the PivotTables they filter, item values with their selected flags, name, caption, style,
  column count, timeline unit and anchor — read into the model and written once from it, so
  `insert.slicer` (pivot or table), `insert.timeline`, `slicer.setSelection`, `slicer.setStyle`,
  `slicer.move` and `slicer.delete` drive a real filter: `setSelection` writes the field's
  `PivotFilter` on every PivotTable in the cache list and refreshes, in one undo step, and the
  selection survives save/reopen. Catalog 279 → 280 (94.9 %). Everything Cella cannot model (an
  OLAP cache, item properties, an unknown attribute or relationship, a timeline's date range) stays
  byte-identical on the pass-through path, all-or-nothing per kind.
- **Landed 10-30 (placement):** a written slicer or timeline is placed and wired the way [MS-XLSX]
  defines — an anchored `xdr:graphicFrame` naming the view in its sheet's drawing part (read back as
  the anchor), one slicers/timelines part per sheet related from the worksheet and listed in its
  `x14:slicerList`/`x15:timelineRefs`, the caches in the workbook's `slicerCaches`/`timelineCacheRefs`
  — and **table slicers are written** with `x15:tableSlicerCache`, taking the selection from the
  column's AutoFilter where Excel keeps it. Verifying against the schema also corrected a forbidden
  cache→slicer relationship, a non-existent cache child, the workbook extension URIs, the timeline
  content/relationship types and `pivotTable/@tabId` (a `sheetId`, not a sheet index). The writer
  omits the optional `mc:AlternateContent` fallback shape because the specification does not define
  its content, and it refuses a timeline whose field holds no dates (no bounds to write).
- **Landed 10-30 (UI):** a slicer draws on the sheet as Excel draws it — a caption band with a filter
  button and value tiles in the model's column count, selected tiles accent-filled with a check, an
  `N of M` footer and wheel scrolling when the body overflows, and the same eight resize handles as
  charts and pictures. A timeline draws its caption, unit and a range track. Clicking a tile runs
  `slicer.toggle`, the filter button `slicer.clear`, a drag or an arrow nudge `slicer.move` (now with
  `dx`/`dy`, so it no longer has to snap to a cell, and it reaches timelines too), Delete removes it,
  a right-click menu carries delete/select-all/clear and a Slicer Styles submenu, and Tab/Enter move
  between tiles without the grid stealing the keys.
- **Landed 10-30 (timeline ranges):** `PivotTimeline` carries a `TimelineRange`, written as
  `<state><selection startDate endDate>` with `filterType="dateBetween"` and read back exactly, and
  `slicer.setRange` filters the report through the slicer path in one undo step (clamping a range past
  the field's own bounds and saying so). The UI's timeline handles are still not draggable — the model
  can hold the range now, wiring the drag is UI work in the next round. Fidelity note: the pivot
  filters by item labels, so a range covering part of a month shows the whole month; a real
  Excel-authored timeline with non-zero refresh versions stays on the kept path.
- **Landed 10-30 (names and a report-side filter):** a table slicer follows its table's rename and
  its column's header rename (`Slicer.column`, and `caption` while it still shows the column's
  name), so a rename can neither empty its tile list nor leave its filter pointing at a column that
  no longer exists;
  `insert.slicer` now applies a `caption` the caller passes (it was documented but dropped). And a
  field selection changed from the PivotTable itself (`pivot.filter`, and any update that rewrites
  the report's filters) is mirrored onto a slicer connected to that field **when every connected
  report agrees on the selection and every label names a tile**. A grouped date label or a
  number-format label has no tile, so the widget keeps its state rather than showing a filter the
  model does not hold; a slicer whose reports disagree is left alone; timelines keep their own range.
- **Landed 11-02 (timeline drag):** a timeline's selected range is now edited on the sheet. The
  track is the field's own date axis (`cmd::slicer::timeline_axis` scans the report's source once
  per document revision, and the grid caches it), the range draws as a band with its own handles,
  and dragging a handle resizes it while dragging the band slides it without changing its length.
  Both ends snap to whole days, stay inside the axis and never cross; each drag runs
  `slicer.setRange` and coalesces into one undo step, and Esc puts the range back. A timeline whose
  report is gone has no axis, so its handles stay decoration rather than inventing a date.
- **Landed 11-02 (a filter's area):** a field can sit in the Filters area and on an axis at once (a
  file says so with a `pageFields` entry), but a filter carried no area, so the writer and the
  refresh guessed it from whether the field was in Rows/Columns — a page filter on an axis field
  collapsed into hidden items and lost its row. `PivotFilter` now records its area (`Auto` for
  documents written before it existed, `Page` for the Filters area, `Axis` for hidden items), the
  reader sets it (`pageFields` vs `item/@h`), the writer and the refresh honour it, and moving a
  filtered field onto an axis turns a page filter into an axis filter.
- **Landed 11-02 (GETPIVOTDATA):** `GETPIVOTDATA(data_field, pivot_table, field, item…)` reads the
  cell the pairs name out of the report's own labels — so it works on a file opened without a
  refresh, where no PivotTable cache exists to recompute from. Text matches case-insensitively, a
  number by value or by its formatted text, `"Grand Total"` names either total axis, and no pairs
  returns the grand total. The formula depends on the report's whole written area, so a refresh
  recalculates it; the function is in the Insert Function list. Each layout is covered (compact
  indentation, outline/tabular columns, Σ Values in Rows, subtotals on top or off) and the lookup
  survives save/reopen; the file-format side is unchanged (it reads cells the report already
  wrote). Differences kept: a report whose grand totals are turned off has no total cell to return
  (`#REF!`; Excel reads its cache), and a subtotal drawn on its own line below the group is named
  by the parent item only in the layouts that put the subtotal on the item's row.
- **Landed 11-02 (number grouping):** a number field can be grouped the way Excel's Group Field
  dialog does it — `pivot.group {start, end, interval}` on a field in Rows or Columns (start/end
  default to the field's own extent, and are **clamped** to it, reported as `clamped`, because
  that is where Excel anchors and re-anchors a number group) — and the report buckets its values
  in half-open steps: whole steps read `0-9`, `10-19`, a fractional one shows the half-open end,
  values outside the bounds become `<start`/next-boundary items, sorted outermost. The model holds
  the range (`PivotField.number_group`), the cache writes [MS-XLSX] `rangePr startNum`/`endNum`/
  `groupInterval` (no `groupBy`; the default is `range`) with the base field's distinct values in
  `sharedItems` and one caption per group item, and a saved report reads the range back and
  rebuilds the same labels on refresh. `pivot.fields` reports it; a range past 100,000 groups, a
  non-positive interval, a range that misses the data or a text field is refused by name.
  **Verified in Excel 16 (11-02):** the oracle opens, refreshes and re-saves the grouped pivot
  with every cell agreeing (only Excel's localized chrome differs).
- **Landed 11-02 (label and value filters):** `pivot.filterByLabel` keeps the items of a row or
  column field whose caption matches — equals, not equals, begins/ends with, contains, does not
  contain, the four comparisons and between/not between, case-insensitively — and
  `pivot.filterByValue` keeps the items whose aggregate of a chosen value field (sum, count,
  average… by the value field's own function) passes the same comparisons. A value filter
  aggregates over the records the report's other filters keep, across the column axis. Both are
  evaluated when set and stored as the field's hidden items (`item/@h`), the representation the
  file already uses for an axis filter and the slicer path already drives, so the report, a
  save/reopen, a refresh and Excel all agree on the visible items; the rule itself is kept in the
  model (`PivotTable.label_filters`/`value_filters`, reported by `pivot.fields`) and not written
  as Excel's `<filters>` element — a refresh after new data keeps the item set instead of
  re-evaluating the rule, and Excel's field dropdown will show hidden items rather than "filtered
  by value". A rule and an item selection on one field replace each other; a calculated field as
  the measure is refused by name. Guarded: between needs both bounds, and the field must be on an
  axis.
- **Landed 11-02 (calculated items):** `pivot.calculatedItem {field, name, formula, delete?}` adds
  an item of a row or column field computed from that field's own items (`=North+South`, bare,
  quoted or `Field[item]`), the way Excel's Fields, Items & Sets ▸ Calculated Item does. The
  formula is parsed by a small shared grammar (`crates/model/src/pivot_item.rs`: numbers,
  `+ - * / ^`, parentheses and item captions; no functions, cells or other fields), resolved
  against the field's real items (an unknown caption, a real item's caption, another **calculated**
  item's caption — so no cycles — or a grouped field is refused by name), and stored in the model
  (`PivotTable.calculated_items`) in the dialog form. The engine appends the item after the real
  ones (so it sorts last, as Excel shows it), gives it a synthetic report path under every parent
  group on its axis, and evaluates the formula on each report cell's item aggregates — one value
  per value field, so the report, subtotals and show-values-as see an ordinary item. As in Excel,
  the grand totals stay the source totals (the item is not added to them) and record-level filters
  leave it alone. The file encoding is Excel's own, captured from an Excel-authored pivot
  (`plan/excel/calculated-items.md`): the caption is the field's **last** shared item with
  `f="1"`, the table lists `<item f="1" x>`, and `<calculatedItems>` is a child of the **cache
  definition** after `</cacheFields>` with the `Field[item]` formula, the item's index and the
  field's axis position. Reading takes that back into the dialog form and drops a definition it
  cannot express with a warning (another field's item, another field's index, a bad formula, a
  field not on an axis). Removing the field from the axes, grouping it, or a source change that
  loses the field or its items drops the item with them. **Verified in Excel 16 (11-02):** the
  `pivot` oracle fixture now carries `Region[East]+Region[West]`; Excel opens the file, refreshes
  the pivot, **keeps the `<calculatedItems>` definition**, recomputes the item (116,320 = 55,300 +
  61,020) and re-saves with every compared cell equal, and our reader reads Excel's own copy back
  as `=East+West`.
- **Missing:** data-model (OLAP) pivots and a real-world corpus for the PivotTable gate.
  (`insert.pivotChart` landed 10-11 and is in the catalog, so PivotCharts are no longer part of
  this gap.) The "Generate GetPivotData" click option (Excel writes a `GETPIVOTDATA` formula when a
  pivot cell is clicked) is not wired into the grid yet.
- **Evidence:** `parity-checklist.md` missing `insert.pivotChart`, `data.relationships`; the slicer
  evidence is `crates/model/src/slicer.rs`, `crates/xlsx/src/slicer.rs` (the modelled-vs-kept contract
  is documented at the top), `slicer_tests.rs`, engine `cmd/slicer.rs`. The written slicer part is
  opened, refreshed and re-read in Excel 16 on the `pivot` fixture; the number-grouping, hidden-item
  and calculated-item encodings do pass [`cargo xtask oracle`](oracle.md) on the `pivot` fixture **in Excel 16**
  (open, refresh every pivot, recalculate, save, re-read: every compared cell equal, and Excel's own
  copy still carries the `<calculatedItems>` definition and the recomputed item), as well as
  openpyxl's readback and WPS 12.0's open/recalculate/save.
- **Impact:** pivots are the analyst's main tool.
- **Estimate:** 0–1 h for beta (the real-world corpus is counted once under #1). The full scope
  keeps data-model (OLAP) pivots and writing Excel's own `<filters>` rules.

### 9. Keyboard-first users: key tips, shortcuts, context menus (U)
- **Landed (10-29):** key tips now cover the whole ribbon — 417 sequences across every tab, group
  and button (contextual tabs switch with their context), served by one JSON source of truth
  (`crates/ui-egui/src/keytips/keytips.json`) and a generated table kept in step by
  `cargo xtask keytips --check`. `Alt` shows the badges, a typed sequence activates, `Esc` cancels
  at any depth. Tests enforce sequence length ≤ 4, per-scope uniqueness, no shadowing (two-way
  prefix), tab reachability, that every badge resolves to a catalog command, and one end-to-end
  test per sequence (419). `Shift+F10` opens the context menu at the selection for cells and for
  row/column headers, navigable with arrows/Home/End/Enter/Space/Esc; the grid keeps focus and the
  selection, and Space cannot start an edit behind the menu. Six Excel shortcuts were added on
  commands that already existed (`Alt+;`, `Ctrl+Shift+O`, `Ctrl+F2`, `Ctrl+Shift+F12`, `Alt+F5`,
  `Ctrl+Alt+F5`). The full Excel → command-id → status audit is in [ui-parity.md](ui-parity.md).
- **Landed 11-03 (object menus):** `Shift+F10` with a chart, picture or shape selected opens the
  same menu the mouse does (delete, alternative text, size and position, bring to front, reset),
  navigable with arrows/Enter/Esc, with focus staying on the object; the cell menu is used when
  nothing is selected.
- **Still missing:** the Menu key itself (egui-winit 0.36 drops `KeyCode::ContextMenu` before the app
  and egui has no `Key` variant for it), QAT/title bar/Backstage badges, the View tab's two combo
  boxes, and per-item badges in menus whose items share one command id. Remaining Excel shortcuts
  map to features we do not have yet (Quick Analysis, extend mode, pane cycling, sheet groups, Paste
  Name, Repeat, outline symbols, formula-bar expand, window keys).
- **Estimate:** 0–6 h of the 12–20 left (the missing extras above). [ui-parity.md](ui-parity.md)

### 10. Crashes and freezes reported from the field (S)
- **Out of scope (11-14):** the machines these reports came from are not this repository's — they
  belong upstream — so this entry cannot be closed here and is not a beta gate for us. The local net
  is the soak (`crates/engine/tests/command_sequence_fuzz.rs`: 6 seeds × 40 steps by default for the
  gate, `CELLA_FUZZ_SEEDS=150 CELLA_FUZZ_STEPS=120` for a real soak) plus the fuzz regression
  files. What it finds is kept: the 10-26 round fixed four, and the 11-13 soak found three
  save-fidelity bugs in filtered sheets and one open case (§3).
- **Evidence:** #120 (Intel UHD crash on Windows), #117 (error on close, macOS), #166 (freeze opening
  from Finder; PRs #164, #167), #76 (flashing on Windows 11), #130 (caught panic in table styles).
- **Landed (10-26): a 100-seed command-sequence soak.** 200 commands a seed in release build, every
  invariant checked per step (`crates/engine/tests/command_sequence_fuzz.rs`). Found and fixed:
  Merge Across of whole columns ran 1,048,576 merges and then re-scanned the sheet per row (a
  freeze); a table's total row could grow into the next table; `data.subtotal` took a dozen undo
  steps per group instead of one; a table column named with `!` was saved as a sheet-qualified
  formula and came back changed. The harness now ends an episode before the 128 MB undo budget
  trims it, traces steps with `CELLA_FUZZ_TRACE`, and names the differing sheet field.
- **Fixed (10-26, same soak):** an array that spills over its own source (`=TRANSPOSE(J23:L28)`
  in J24) was laid out anyway, with some elements `#CIRC!`; it is now `#CIRC!` at the anchor, as a
  formula that reads a cell it writes. And two arrays whose areas overlap kept their cells across
  F9 instead of being re-laid out in row-major order (`=SEQUENCE(7,4)` at K7 beside `=SEQUENCE(6,5)`
  at L2).
- **Fixed (10-27): a 500-seed soak is green.** 200 commands a seed (100,000 commands, release,
  49 minutes) over the three samples, every invariant checked per step. The three open seeds from
  the 100-seed run are gone: on read, a dynamic anchor's `ref` is rebuilt into `spill_ranges` and
  the cached interior values become the spill map, not cells that block the array, so a reload
  keeps the array that won a contested area; F9 prefers the anchors that actually spill, and an
  owner that grows back after a spill it reads is laid out later reclaims its cells. `#SPILL!`/
  `#CALC!` cached as `#VALUE!` is compared as what the file can store, not as a changed value.
  The `_sc_*` names (Watch Window list, custom views, scripts, scenarios) are view state and
  formulas can no longer resolve them. Further seeds found and fixed: a formula that reads its own
  cell through a range and swallows the `#CIRC!` (`COUNTIF` over its own column) is `#CIRC!`
  instead of a number that depended on evaluation order; a filter installed over a selection that
  covers a table is narrowed to the table's filter range, so its criteria reach the table part and
  SUBTOTAL still skips the hidden rows after opening; a multi-cell legacy array that cannot spill
  still refuses edits of its cells; Insert/Delete Cells that would cut a merged area in two is
  refused; the harness tells a byte-capped (trimmed) undo history from a real undo bug, and
  ignores view-only freeze in undo comparisons.
- **Remaining caveat:** the file format cannot cache `#SPILL!`/`#CALC!` (Cella writes
  `#VALUE!`, following the claim that Excel refuses those error codes; **needs verification on a
  licensed Excel**) or `#CIRC!` (written as 0). A *literal* `#SPILL!` pasted as a value therefore
  comes back `#VALUE!`; formulas and spilled values are recalculated and agree.
- **Re-run 11-02 after the timeline/secondary-axis work:** 100 seeds × 200 steps (20,000 commands)
  in the debug build, 68.3 s wall, **0 invariant failures, 0 slow seeds** (`soak: 100 seeds × 200
  steps, 68.3 s wall, 0 invariant failures, 0 slow seeds, slowest seed 7 (6620 ms)`). The three
  samples were exercised against undo/redo, XLSX round-trip and full recalc.
- **Re-run 11-02 after the three PivotTable changes (GETPIVOTDATA, number grouping, label/value
  filters):** 100 seeds × 200 steps at commit `3574f5d2`, debug test profile, **131 s wall, 3 tests
  passed, 0 invariant failures**. Seeds are only valid at the commit that generated them, so this
  line records that commit.
  
- **Next:** the trust track's soak is closed; the 100k-dependent gate is met, independent formulas
  recalculate on up to eight threads (20–25 ms at 100k, 208–277 ms at 1M) and the bulk data
  commands are bounded (12M cells; Remove Duplicates 1M×10 in 12 s, Sort in 3.2 s, gaps §2). Cold
  paging (M19) is the next large-workbook task. Replays and details in `plan/HANDOVER.md`. The
  Windows GPU/driver reports (#120, #76) and the macOS close/open reports (#117, #166) still need
  the hardware they were reported on; they cannot be closed here.
- **Estimate:** 12–16 h for this soak and its fixes; done.
### 28. `edit.clearAll` took 9 seconds on a 1000-column sheet — **done 11-24** (1.2 ms)

**What:** the shape ruler holds the cell count at eight million and moves it around. `edit.clearAll`
over the used area cost **9.13 s** as 8,000 rows × 1,000 columns, **14 ms** as a hundred sheets of
10,000 rows, and **1.14 s** as a million rows × 8 columns (the row catalogue's number for the same
eight million cells).

*Correction, 11-24:* the ruler's first tall row said **1.0 ms**, and that was the ruler, not the
product — the param generator skipped the optional `range` on that shape, so the command cleared the
one-cell selection. Region-naming keys are now always given (`crates/engine/tests/param_gen`), which
is what made the tall and wide rows the same question.

**Why it matters:** a 1000-column sheet is what an export from a database looks like, and "select all
and delete" is the first thing anyone does to one. Nine seconds was the longest wait in the program
after Subtotal.

**First cut (11-24):** the cost was the store's per-cell removal. Every take shifts the band's row
side tables, so a row of a thousand cells shifted the same 64-row band a thousand times — quadratic
in a row's width, and the reason the wide shape was eight times the tall one. Clear All over the whole
used area now replaces the store instead of emptying it (`crates/engine/src/cmd/edit.rs`):
**9.13 s → 1.40 s** on the wide shape, 1.14 s → **311 ms** on the tall one, 14 ms → **4.7 ms**.

**Second cut (11-24):** the remaining 1.40 s was not the clear at all — it was building the eight
million addresses the recalculation would be told about, and the recalculation then marking them one
by one. Nothing survives a clear of the whole used area, so the command now says so in one word: the
commit takes the structural path and rebuilds the graph instead of being handed a list. **1.40 s →
1.2 ms** wide, 311 ms → **7.4 ms** tall, 4.7 ms → **3.4 ms** on a hundred sheets.

**The trade:** a workbook whose *other* sheets hold many formulas now pays a full recalculation after
such a clear, where before it paid only for the dependents of the cleared cells. That is the honest
reading of "this sheet is empty now", and the case it costs is a workbook big enough that its other
sheets' formulas are the work.

**Evidence:** [performance-shapes.md](performance-shapes.md) and
[performance.md](performance.md) §"Shape: rows are not the only axis".

### 29. Saving a hundred sheets — **the ruler measured JSON, not xlsx** (retracted 11-24)

**What it said:** `file.saveBytes` on eight million cells in a hundred sheets of ten thousand rows
took **23.72 s**, against 1.68 s as a million rows × 8 columns and 1.34 s as 8,000 × 1,000 — "the
same cells, eighteen times the time", attributed to the writer's per-sheet cost.

**Why that was wrong:** the shape ruler generates each command's params from its doc string, and a
hint with alternatives (`xlsx|csv|tsv|json|html`) was drawn **at random for each shape**. The
hundred-sheet row drew **json**: it wrote **1.95 GB** of JSON in 15.8 s. The other two shapes drew
`xlsx` and wrote 30–48 MB. The three columns of that row were three different commands, so the "18×"
was the difference between two formats, not between one sheet and a hundred.

**Proof:** with `format: "xlsx"` pinned in `crates/engine/tests/param_gen`, the same workbook writes
43 MB in **1.48 s**, and every `write: sheet` line reports 13–16 ms for its 80,000 cells — a
hundredth of the reported figure. The writer has no per-sheet problem to find.

**Now:** `file.saveBytes` is **1.79 s** as a million rows × 8 columns, **1.41 s** as 8,000 × 1,000
and **1.48 s** as a hundred sheets of ten thousand rows. Excel takes 2.761 / 1.714 / 2.502 s for the
same three workbooks and WPS 4.957 / 1.024 / 1.494 s: **Cella is faster than both on all three.**

**The lesson:** a ruler that draws a *mode* at random measures a different command per row. The
generator now pins the format, and a future hint whose alternatives change the work — not just a
detail — has to be pinned the same way.

**Evidence:** [performance-shapes.md](performance-shapes.md).

### 30. A right-to-left sheet costs three frames for one (P, measured 11-24)

**What:** the frame ruler's second table turns each of the thirty-one view settings on and times the
same two frames with it. `view.rightToLeft` is the only one that changes what a frame costs:
**4.3 ms** to scroll one screen against **1.4 ms** with nothing changed (2.97×), and 1.2 ms for a
still frame against 0.8 ms. The other thirty sit on the baseline; two are cheaper because they show
fewer cells (`view.zoomToSelection` 0.5 ms, `view.pageLayout` 0.9 ms).

**Why it matters:** a right-to-left sheet is what a Hebrew or Arabic workbook looks like, and 4.3 ms
of CPU is a third of a 60 Hz frame before anything moves.

**Fix (not diagnosed yet):** the renderer mirrors every cell's rectangle as it paints instead of
laying the band out once in mirrored order, so the mirror is paid per cell per frame.
**Estimate:** 2–4 h to diagnose and price.

**Evidence:** [performance-ui.md](performance-ui.md) §"What a view setting costs per frame".

### 31. `edit.find` materialises every match — **fixed 11-24** (was 3.5–5.7 s in eight million cells)

**Was:** `edit.find` over the whole used area took **3.48 s** on a million rows × 8 columns and
**5.68 s** on 8,000 rows × 1,000 columns (33 ms on a hundred sheets, where the search is scoped to
the active sheet's 80,000 cells). A common word had a million matches, and the command built a `Vec`
of `(sheet, cell, text)` for every one of them before returning the first — so *Find Next*, which
needs one hit, paid for all of them.

**Now:** the single-hit form stops at the first match after the active cell and wraps by walking the
rows above it, so it costs the distance to that cell: **0.21 ms / 0.07 ms / 0.05 ms** on the same
three shapes, against Excel's 3 ms / 70 ms. Only `all` collects. Two things fell out of the same
walk: the matcher allocates nothing per cell (it lowers the needle once instead of both sides eight
million times), and a text value under a format that leaves text alone is borrowed rather than
rebuilt — the difference between allocating eight million strings and none. The scan also goes
through `iter_scan` (the save path's flat walk) instead of `iter`, so a thousand-column sheet no
longer builds a 256,000-cell vector per band.

**Behaviour change:** the single-hit result no longer carries `count`. That number was the total,
which is the thing the early stop stops paying for; `all` still returns it.

**Evidence:** [performance-shapes.md](performance-shapes.md).

### 32. Formatting a range over four million cells is refused (P, measured 11-24)

**What:** `home.formatCells` over the whole used area at 1M rows is an error — "selection too large to
format cell by cell" (`crates/engine/src/cmd/format.rs`, a 4,000,000-cell cap). Excel applies a format
to a whole sheet. The cap exists because a style is stored per cell, so the command is O(cells) and
would be seconds.

**Why it matters:** "select all, bold" on a large sheet is an ordinary action and it fails.

**Fix (not diagnosed yet):** a range-level style (a rectangle or a column band) that the renderer, the
writer and `cell.get` expand lazily, instead of one style id per cell. **Estimate:** 6–10 h; a design
question, not a knife.

**Evidence:** [performance-shapes.md](performance-shapes.md).

### 33. Sorting a thousand-column sheet takes 1.94 s — **target met 11-24**

**Was:** `data.sortAscending` over eight million cells took **9.98 s** on 8,000 rows × 1,000 columns
and **1.46 s** on a million rows × 8 columns (Excel 0.146 s and 0.347 s, WPS 0.758 s and 1.334 s).

**What changed, in three cuts:** the write-back used to put every cell back through `put`, which
keeps the band's run-length kind and style stripes straight one cell at a time, and a run that splits
re-points the offsets of all 64 rows of the band — 6.1 s. A sort is a permutation of its block, so
when the block holds every cell the sheet stores in its columns, the write-back now **builds a fresh
store in destination order** (9.98 → 3.35 s). Then `Band::text_slot`'s sorted pointer index became a
map past 256 texts, which is the shape a write-back produces: one word arriving from each source band
as its own `Arc` (`gaps` §38) — 3.35 → 2.08 s. And the gather skips its `LineShapes` bookkeeping on a
sheet that has never held a formula.

**Now: 0.83 s** wide and **1.19 s** tall (round 52: the shape page's five-sample run, on a machine that
read 8–20 % slower than round 51's on the rows this change does not touch). The gather was not paying
"the store's per-cell read price" — it was paying a quadratic row walk (`gaps` §39), and fixing that
took the sort's wide sheet from **1.81 → 0.83 s** without touching the sort. Excel is 0.146 s; what is
left is the gather's remaining per-cell cost and the formula relabel for a body that holds formulas.

**Evidence:** [performance-shapes.md](performance-shapes.md).

### 34. Moving a block took 16 s — **target met 11-24** (was 15.9 s, now 1.11 s)

**Was:** `range.move` over eight million cells took **15.9 s** on 8,000 rows × 1,000 columns and
**3.7 s** on a million rows × 8 columns (Excel 0.507 s and 0.676 s, WPS 1.380 s and 1.334 s). Five
things added up, each measured on its own:

- the cut removed the block **cell by cell**, and removing a cell shifts the rows of its band after
  it: **11 s** of the 16;
- the tab-separated text a clipboard holds was built for a *move*, which never reads it — and
  `range_text` formats every cell: **1.1 s**;
- the overwrite check walked the whole destination to find the few cells the source does not cover:
  **1.3 s**;
- the paste wrote a cell at a time into rows the cut had just emptied, so every cell inserted into
  the middle of the band's stripes: **4.4 s**;
- the moved cells went through a `BTreeMap` keyed by address — 8M insertions to build it, 8M lookups
  to read it: **2.4 s**.

**Now:** a cut whose range spans every stored column takes the rows whole (descending, so a row's
slice sits at the end of its band and dropping it moves nothing; a band that loses every row it
holds is dropped instead of edited); `range.move` cuts without building the clipboard text; the
overwrite check walks only the strips of the destination the source does not cover; the paste writes
each destination row in one store call; and the moved block is a sorted `Vec` walked by a cursor.

**Measured (round 51):** **2.03–2.04 s** wide and **1.30–1.31 s** tall — the ruler's best of three (15.9
and 3.7 before), stable to 10 ms between runs where single samples had spread over 2.2–2.9 s. That was
1.5 % over the 2 s target, and the profile said the cut was **1.22 s** of it.

**The sixth cause, found by doubting that attribution (round 52):** the cut was not paying "the
store's per-cell price" — it was paying a **quadratic row walk** (`gaps` §39). Every row walker asked
the kind and style run stripes for each cell from the first run, and a thousand-column row carries a
couple of hundred runs. A run position on the cursor: the cut **1.22 → 0.25 s**, and a move's paste
loop stopped asking per cell for notes, hyperlinks, validations and pictures a whole paste cannot
change.

**Measured (round 52):** **1.11 s** wide and 1.39 s tall (that row spread 1.28–1.39 s across three
passes of the same code — nothing in this change touches the tall shape's eight-column rows), against
a 2 s target and Excel's 0.507 s. What is left of the wide move is the pasted cells' construction
(~0.38 s), the store write (~0.32 s) and the changed list — not one read or one write of the block.

**Evidence:** [performance-shapes.md](performance-shapes.md).

### 35. `edit.copy` still formats a million cells to make the clipboard text (P, measured 11-24)

**What:** `edit.copy` over eight million cells takes **0.78 s** on 8,000 rows × 1,000 columns and
**8.5 ms** on a million rows × 8 columns (Excel 0.043 s and 0.004 s). The block is capped at two
million cells of clipboard text (`display::clipboard_range`), and that text is now read a row at a
time with the number format parsed once per style rather than once per cell — which took the tall
shape from 100 ms to 8.5 ms and the wide one from 0.89 s to 0.78 s.

**What remains:** about **390 ns a cell** in `range_text`, almost all of it `format_value` building a
`String` per cell that is then copied into the tab-separated text. Two ways out, both outside the
command layer: a formatter that writes into the caller's buffer (a `numfmt` API change), or a lazy
clipboard — build the text when somebody reads it, the way Excel's delayed-render clipboard does.

**Round 52:** **557 ms** wide (was 852), from the row-walk fix in `gaps` §39 — `range_text` reads its
rows through the cursor that was walking the run stripes per cell. This is the page's noisiest row: an
allocation-bound `String` per cell has measured 540, 944 and 557 ms in three passes of the same code,
so its five-sample value is the one published.

**Evidence:** [performance-shapes.md](performance-shapes.md).

### 36. `edit.clearFormats` — **target met 11-24** (was 3.45 s, now 36 ms)

**Was:** `edit.clearFormats` over eight million cells took **3.45 s** on 8,000 rows × 1,000 columns
(Excel 0.203 s, WPS 0.788 s). It read a whole [`Cell`] — value, formula `Arc` and style — for every
cell to learn that most of them had no style, then wrote a styled one at a time through `put`.

**Now: 36 ms** wide, **173 ms** tall, 1.0 ms on a hundred sheets. Styles are run-length encoded in the
store, so `CellStore::clear_styles_rows` + `Band::set_row_style` collapse a row's runs into one default
run with a single splice of the style stripe, and the rows after it move by the difference; rows
already all-default are skipped without touching a stripe.

**The trade:** the changed list is how the recalculation hears about a style change — `CELL("format")`
and `CELL("protect")` read one — so the run-level path is taken only when **no formula can read a
format**. A workbook that holds one keeps the cell-by-cell clear, which is still 1.8 s on a wide
sheet; a test covers both paths.

**Evidence:** [performance-shapes.md](performance-shapes.md).

### 37. The command catalogue could not finish: `formulas.createFromSelection` ran for 20+ minutes — **fixed 11-24**

**Was:** `cargo xtask bench-commands` priced 413 of the 422 commands and then stopped moving on
`formulas.createFromSelection` with the whole used area (a million rows × 6 columns) as its range;
isolated, the command alone did not finish in four minutes. It is not one of the commands this round
touched, and the page it blocked stayed at round 47's.

**Why:** "Create from Selection" with a left column of labels makes **one defined name per row**. The
edit wrote them one at a time, and each write walked the whole growing name list:

    for (name, f) in made {
        cx.wb.names.retain(|x| !(x.name.eq_ignore_ascii_case(&name) && x.scope.is_none()));
        ...
    }

A million names is a million walks of a list growing to a million: quadratic, µs per name, hours.

**Now:** the replacement is one pass — a `HashSet` of the names being written, one `retain` over the
existing list, then the extend. Measured with the catalogue's own params: **698 ms** at a million
rows (was: no answer), 54 ms at a hundred thousand. The document grows by **126.5 MB**, which is what
a million defined names cost — that is the command's real price, not a bug.

**The catalogue finishes**: 422 commands × 2 row counts × 2 scales, `docs/performance-commands.md`
redrawn. Test: ten thousand rows beside a label column create ten thousand names, and asking again
creates none.

### 38. A band stored one text per cell, and its index was a sorted vector (P, found and fixed 11-24)

**What:** building eight million cells straight into a store cost **24 ns a cell when the text
repeated and 401 when every cell carried its own `Arc`** — the same word, a different pointer. Every
row of a sort's write-back looks like that: a text arrives from each source band as its own `Arc`, so
a destination band of sixty-four rows holds hundreds of "pointers to the same word".

**Why:** `Band::text_slot` kept its pointer index as a **sorted vector** and inserted into it, which
is O(texts) per new text. That is the right shape for the handful of labels a normal band holds (the
comment in the struct says so, and the number table — hundreds of entries — is a hash for the same
reason), and quadratic for a band holding one text per cell: a column of unique keys, an import with
an id column, or a sort's write-back.

**Now:** past **256 texts** the index becomes a `HashMap`, where a hit and a new text are both O(1).
The probe: 401 → **112 ns a cell**, level with the numbers-only case at the same width. The sort's
write-back on a thousand-column sheet: **1835 → 628 ms**, and `data.sortAscending` **9.98 s → 1.94 s**
(Excel 0.146 s) — the 2 s target round 48 missed.

**What remains:** the same probe shows the numbers case at width 1,000 costs **120 ns a cell against
49 at width 8**, with nothing per-cell changed — a band a thousand columns wide no longer fits L1 and
every cell touches four stripes. That is the **write** side; the read side's wide/tall gap had a
different cause, found a round later (`gaps` §39). Moving packed rows instead of rebuilding cells is
still the guess for the write, and it is a page-format change.

**Evidence:** a probe over `CellStore::set_row` (removed after the measurement) and
[performance-shapes.md](performance-shapes.md).

### 39. A row walk asked the run stripes for every cell from the first run (P, found and fixed 11-24)

**What:** taking eight million cells out of a store — the cut half of a move — cost **1.20 s** on
8,000 rows × 1,000 columns against **0.30 s** on 1,000,000 rows × 8 columns. Same cells, same code,
four times the price. Round 51 wrote that 1.20 s off as "the store's per-cell price, so the next step
is packed rows" (`§33`/`§34`). It was not a price: it was **quadratic in the cells a row holds**.

**Why:** a row's kinds and its styles are run-length encoded — `(value, length)` pairs in cell order —
and `PackCursor`, the cursor every row walk goes through (`iter_range`, `row`, `row_reader`,
`iter_scan`, `visit_values`, and a cut's `take_rows`), asked `BandRow::kind_at` and
`BandRow::style_at` for each cell. Both walked the row's runs **from the first** to answer, so a cell
cost the runs before it. A row of the shape ruler's wide sheet alternates a number, a quantity, a
price, a name, a region, a date, a key and a value, so it carries a couple of hundred kind runs:
1,000 cells × ~125 runs a cell = 125,000 run steps a row, × 8,000 rows = 10⁹ steps.

**Now:** the cursor carries a run position per stripe (`RunPos`: the run it last answered from and
that run's first cell — two words, borrowing nothing, so a walker that owns its band across
`next()` calls can hold it), and a cell's kind and style are read off it in O(1) amortised.
`Band::cell_at`/`value_at` split into `cell_of`/`value_of`, which take the kind (and style) the walk
already knows, so the single-cell readers keep their own one-run walk.

**Measured:** the cut **1.20 s → 0.25 s** on the wide shape. `range.move` **2.04 s → 1.11 s** (target
2 s, met with half to spare), `data.sortAscending` **1.81 s → 0.83 s**, `file.saveBytes` **1.32 s →
0.86 s**, `edit.copy` **852 ms → 557 ms**. Every one of those walks rows. The untouched rows on the
same page read 8–20 % *slower* than round 51's (`edit.clearFormats` 34 → 39 ms, `edit.clearAll` 1.1 →
1.4, `sheet.rename` 65 → 148 µs), so the machine was the slower one and the wins above are floors.

**Test:** a row of four hundred alternating kinds (a text every third cell) and four styles is read
whole and through windows that start and end **inside** a run, through `row()`, `iter_range` and
`visit_values`, against the value and the style each column must hold.

**Left:** the same run walk in `Band::compact_wide` and in `row_same_as`/`diff_bands` (both ask
`kind_at` per cell); neither is on a measured path, so neither was changed.

### 40. A formula's block argument was built before the formula could say it needed three cells (P, round 53)

**What:** at a million rows, with the functions catalogue's own calls, `VLOOKUP` took **78 ms**,
`MATCH` **56 ms**, `SUM` **51 ms**, `COUNT` **50**, `MAX` **50**, `MIN` **50**, `AVERAGE` **50**,
`SUMIFS` **39**, `SUMIF` **30**, `COUNTIF` **26**, `SUMPRODUCT` **25**, `LARGE` **28**, `MEDIAN`
**56**, and `XLOOKUP` **79**. Excel answers a VLOOKUP into a million-row table in about a
millisecond when the key is near the top, and its `SUM` over the same column is 20–50 ms.

**Why, three separate causes:**

1. **The exact lookups are library calls.** `MATCH(k, A:A, 0)` and `VLOOKUP(k, A:B, n, FALSE)`
   reached `cella_functions` with their blocks materialised: the evaluator built every cell into a
   `Value` first (two million of them for `A:B`) and only then looked for the key. A key on row 1
   paid for the whole table.
2. **A fold asked who owned the block before it folded it.** `visit_aggregate` began with
   `has_owned_cells` — a walk of every row to learn whether any cell holds a formula or an array —
   and then folded the same cells again. On a column of values that is 6–10 ms of a 25 ms `SUM`,
   spent to answer a question the fold itself hits in its first owned cell.
3. **One column is one cell a row, and the row walk charged it the row's setup.** The general
   packed walk costs **16.9 ns a cell for a one-column read against 4.4 for a row of eight** — same
   cells, one arrangement apart (`crates/model/examples/read_probe.rs`). Every whole-column argument
   is that shape.

**Now:** the two lookups answer from a scan of the key column that stops at the hit
(`first_match_in_column`, the hook `XLOOKUP`'s text path got in round 39, generalised to numbers),
returning `#N/A` when the scan completes without one; a wildcard key, an empty key
(`MATCH("", …)` matches a *blank*), an approximate match type, a horizontal block or a block holding
formulas all decline to the library, so the answer cannot differ. The fold runs first on a copy of
the accumulator and hands over to the owned paths at the first cell the evaluator owns.
`CellStore::visit_column_cells` hoists the row walk's invariants for one column, which
`visit_packed`/`visit_values` take when the range is one column and the column map is the identity.

**Measured:** the pre-change code checked out beside the new one, same params, five passes a row, one
after the other — **VLOOKUP 78 → 17 ms (−78 %), MATCH 56 → 17 (−70 %), SUM 51 → 30 (−41 %), MAX 50 →
30 (−40), MIN 50 → 30 (−40), COUNT 50 → 31 (−38), SUMIFS 39 → 33, AVERAGE 50 → 43, LARGE 28 → 25,
SUMIF 30 → 27; MEDIAN 56 → 53, XLOOKUP 79 → 75, COUNTIF 26 → 25, SUMPRODUCT 25 → 24 follow the
one-column walk, not their own fix.** Two rows this round cannot touch sat still across the same pair
(`BITAND` 238 → 236 ms, `BYROW` 1.15 → 1.16 s), so the machine was steady while the two were measured.
A hit on row 1 is **0.6–0.9 ms** where it was 25–40; a hit in the middle is the scan; a miss is one
pass instead of the array plus the pass.

**A retraction inside the round:** the first certificate was "best of two runs a side" with one pass a
row, and it read −51 % for `SUM` and −26 % for `SUMPRODUCT`. Re-measured with five passes and the
pre-change code actually checked out beside the new one, those are −41 % and −4 %: a single pass on
this machine (77 % load all afternoon) is worth more swing than the change it was measuring.

**Equivalence:** `crates/engine/tests/xlookup_column.rs` compares both spellings of each lookup on
one sheet — the literal key (fast path) against the key in a cell (general path) — over text in two
cases, a number column, a text key that spells a number, a wildcard, an approximate match type, a
column past the table, a blank return cell, a formula in the key column, and a 5,000-row table.

**Left (the next knife):** `XLOOKUP` with more than three arguments still builds the block (75 ms);
`COUNTIF`/`SUMIF` build one array to test it (25/27 ms); `MEDIAN`/`SUMPRODUCT`/`LARGE` need every
value in hand (53/24/25 ms) and pay the array build; `SUM`'s 30 ms is now the fold itself over two
million cells. `LET`/`LAMBDA` over a whole column is not measured at all yet.

### 41. The groupers keyed every row by a string they built (P, found and fixed 11-24)

**What:** over a million rows, `PIVOTBY` took **5.16 s**, `GROUPBY` **2.48 s**, `UNIQUE` **1.49 s**
and `BYROW` **1.14 s** — the four dynamic-array functions a person uses to turn a data dump into a
summary, and the four that `docs/performance-plan.md` ranked as P1's second knife.

**Why, for the three that group:** a row's key was a `String`. `row_key` spelled the row's values out
as text — a length, a colon and the value, with text lowercased — and the map from a key to its rows
was keyed by that string, so every function built **a string a row**: a million allocations, a million
hashes of a small string, and (in `PIVOTBY`) two of them a row for the cell map. `UNIQUE` and
`GROUPBY` also collected the keys into a `Vec<Vec<Value>>` first, another million allocations.

**Now:** `KeyIds` numbers each distinct row key in first-seen order and answers a row's id from a hash
of its values: two `Vec`s and a map from a hash to its first id, with a chain of ids for collisions, so
a row costs a hash and no allocation. Two rows that hash alike are compared value by value, so a
collision costs a comparison rather than a wrong group. The rule the hash and the comparison share is
exactly the one the string key had: numbers by value (`-0` is `0`, two NaNs equal only when their bits
are), text **case-insensitively**, errors by code, blanks equal to each other, an array by its first
value. `PIVOTBY` also stopped gathering each bucket into a `Vec<&Value>` — a built-in aggregate walks
the block's rows directly (`aggregate_rows`) — and gathers the totals' rows once instead of walking
every cell.

**Measured:** `PIVOTBY` **5.16 s → 989 ms**, `GROUPBY` **2.48 → 785 ms**, `UNIQUE` **1.49 → 310 ms**
(−80 %, −66 %, −77 %). and `BYROW` did not move (1.18 → 1.15 s): it calls the LAMBDA once a row, and each call builds a
`Vec` of the row's values, an `Arc<Array>` for them and a bound-argument `Vec` with a cloned
parameter name — a microsecond a row, which is the evaluator's next item, not this one's.

**Test:** the old string key runs beside the new ids over the pairs that make the rule non-obvious —
`"a"`/`"A"`, `-0`/`0`, two NaNs, the Kelvin sign against `"k"`, `"ß"` against `"SS"`, blanks,
errors and mixed-type rows — and the two groupings must be identical, which is what a hash or a
comparison that disagrees with the rule would break.

### 42. A LAMBDA call cost a microsecond before the body ran (P, found and fixed 11-25)

**What:** `BYROW` over a million rows cost **1.15 s** — a microsecond a row — and `MAP`'s elements
**0.53 µs** each. `fn_probe`'s new rows said the body was not the problem: a LAMBDA whose body is a
constant (`LAMBDA(x,1)`) still cost **0.48–0.59 µs a call**, and a body that calls a worksheet
function (`LAMBDA(x,SUM(x))`) added another ~0.3 µs.

**Why, three things a call:**
1. `call_lambda_values` swapped the whole environment out on every call —
   `mem::replace(&mut self.env, l.env.clone())` — and for a `LAMBDA(x, …)` written inline, `l.env`
   is empty: the vector the call was about to use was **dropped (capacity and all) and rebuilt**,
   a call, for a million calls.
2. Every helper built a one-entry `Vec` to bind the argument (`vec![(p.clone(), …)]`), a String
   clone included.
3. `aggregate_value` looked up `SUM` **and** `COUNT` by name for every argument, though `SUM` and
   `AVERAGE` add with `SUM` alone and `COUNT` counts with `COUNT` alone.

**Now:** the environment is kept when the lambda captured nothing (a captured one still replaces it,
which is what lexical scope needs), `call_lambda_one` binds the parameter straight into it, and
`aggregate_value` looks up only the function a kind adds by. `MAP` with one array and one parameter
takes the same path.

**Measured (five passes a row):** `BYROW(A1:C1000001, LAMBDA(x,SUM(x)))` **1.15 s → 984 ms (−14 %)**;
`BYROW` with one column **988 → 825 ms**; a constant body **592 → 525 ms**; `MAP(x,SUM(x))`
**885 → 705 ms (−20 %)**; `MAP(x,x*2)` **541 → 432**; `SCAN` **601 → 526 (−13 %)**; `REDUCE`
**170 → 133 ms**. The rows that do not run a LAMBDA did not move (`PIVOTBY` 989 → 990, `GROUPBY`
785 → 782, `UNIQUE` 310 → 305, `SORT` 256 → 257, `VSTACK` 192 → 190), which is what says the
machine was steady.

**Left:** one `String` is still allocated a call for the parameter's name in the environment; the
environment would have to carry `Arc<str>` for that to go, which is a type change across the
evaluator for a few percent. `BYROW`'s remaining microsecond is mostly the row's own `Vec` and
`Arc<Array>` — the argument has to be an owned array because the LAMBDA may keep it.

### 43. The lookups only answered the short spellings (P, found and fixed 11-25)

**What:** round 53 taught `MATCH` and `VLOOKUP` to scan the key column, but the catalogue's own calls
— and most of the ones people write — spell the options out: `XLOOKUP($K,$KG,$R,1,0,1)` is six
arguments, `XMATCH($K,$KG,0,1)` four, and both fell straight through to the general path, which
builds the block first. So `XLOOKUP` read **74 ms** and `XMATCH` **53 ms** while `MATCH` and
`VLOOKUP` — the short spellings round 53 taught — read 17.

**Why:** `xlookup_scalar_column` demanded exactly three arguments (no `if_not_found`, no modes), and
`XMATCH` had no narrow shape at all: the dispatcher went from `MATCH` straight to `builtin`.

**Now:** `XLOOKUP` takes three to six arguments — `if_not_found` is answered only on a miss (lazily,
as the general path answers it), and the documented defaults (exact match, first to last) may be
spelled out. `XMATCH` got the scan `MATCH` has: `match_mode 0` and `search_mode 1`, or the defaults,
answer from the key column and stop at the hit. Every other mode declines to the library — last to
first, a binary search, a wildcard mode or a lower-than mode — as do wildcard keys, so the answer
cannot differ.

**Measured:** `XLOOKUP` **74 → 16 ms (−78 %)**, `XMATCH` **53 → 16 ms (−70 %)**; `MATCH` and
`VLOOKUP` stayed at 16–17, and the rows this round cannot touch did not move (`SUM` 30 → 29,
`COUNTIF` 23 → 23, `SUMIF` 27 → 27, `MEDIAN` 51 → 49, `SUMPRODUCT` 23 → 22, `BYROW` 984 → 983,
`PIVOTBY` 990 → 996, `BITAND` 231 → 231).

**Test:** `crates/engine/tests/xlookup_column.rs` grew XMATCH both ways (text, number, a miss, the
modes spelled, a wildcard mode staying general) and XLOOKUP with the defaults spelled and an
`if_not_found` on a miss, a number and a text.

**Left, and it is the last of P1's first item:** `COUNTIF` (23 ms) and `SUMIF` (27 ms) build an array
to test it, and `MEDIAN`/`SUMPRODUCT` (49/22 ms) need every value in hand. A scan would only reach
~19 ms because the store's one-column walk costs that much a million rows (`gaps` §39/§40); getting
them under 10 ms needs a tighter column walk in the store, which is its own knife.

### 44. The criteria functions tested a copy of the column (P, found and fixed 11-25)

**What:** `COUNTIF` read **23 ms** and `SUMIF` **27 ms** over a million rows — and round 53 had
already said why: they are library functions that take an `Array`, so the evaluator built the whole
block into one (a million `Value`s) and *then* asked the criterion of the copy.

**Why it was not just a one-line wiring:** the criterion is a parsed matcher (`crates/functions/src/
criteria.rs`: the operator, the operand, the wildcard tokens), and its `matches` took a `&Value` — so
a host that walks stored pages would have had to build a `Value` a cell to ask it, which is the
allocation the array build was already paying.

**Now:** the matcher is written once over a `CellView` — a number, a borrowed text, a boolean, an
error, a blank — so a host can ask it of a packed cell with no allocation. `COUNTIF` and `SUMIF` in
the evaluator answer from a single walk of the stored column (the hook `countif_column` /
`sumif_columns`; blanks are the slots the walk does not meet, counted when the criterion matches
one), and `SUMIF` answers its one-column form (`SUMIF(A:A, ">0", A:A)` and the two-argument
`SUMIF(A:A, ">0")`) from one walk too.

**Measured:** `COUNTIF` **23 → 18 ms (−22 %)**, `SUMIF` **27 → 18 ms (−33 %)**. Both now sit at the
walk's own price (~19 ns a cell), which is the floor a scan can reach until the store's one-column
walk gets tighter.

**The differential found two bugs of its own:** `crates/engine/tests/criteria_scan.rs` asks the same
question as `COUNTIF`/`COUNTIFS` (and `SUMIF`/`SUMIFS`) with one pair over a table of numbers,
numeric text, text in two cases, a wildcard literal, a boolean, an error, a blank slot and an empty
text. It caught that a **matched error cell has to be the answer** (a `SUMIF` whose matching target
holds `#DIV/0!` returns the error, not a sum that skips it), and that the two-column form has to
remember a target's error until its row's criterion is known — both fixed and pinned.

**Left:** `AVERAGEIF` (27 ms) still takes the library path; `SUM`/`MAX`/`MIN` are folds already; and
`MEDIAN`/`SUMPRODUCT` (50/22 ms) need every value in hand. The 10 ms target for the criteria family
is the store's one-column walk, not theirs.

**Round 59 finished the family:** `AVERAGEIF` **27 → 17 ms**, `AVERAGEIFS` **28 → 18**,
`COUNTIFS` **24 → 18**, `SUMIFS` **31 → 18** — seven spellings of one question, all at the walk's
price now. One host hook (`criteria_columns(target, criteria, criterion, fold)`) covers the sum, the
count an average divides by, the same-column form and the two-column form; the evaluator passes each
function's own argument order. The one difference Excel documents is kept and pinned by tests: a text
criterion past 255 UTF-16 units is `#VALUE!` for `COUNTIF`/`SUMIF`/`AVERAGEIF` and is **truncated**
for the `*IFS` forms, so each path parses its own criterion.

### 45. A million-cell dynamic array pays for its **spill**, not its lift (P, measured 11-25; the plan's guess refuted)

**What the plan said** (`performance-plan.md` item 10): "array lifting — `BITAND`/`BITOR`/`BITXOR`
243/246/240 ms at a million rows, ≈120 ns an element; the lifting is element-wise boxing; process it
in blocks (SIMD-friendly) — ≤80 ms". So the next knife was supposed to be a tighter lift.

**What the instruments said.** `fn_probe` grew three rows that separate the halves of one call —
the same million-element result folded into a scalar (no spill laid out) and left as an array:

| call | ms | what it is |
|---|---:|---|
| `SUM(A2:A1000001)` | **16** | reading one column and folding it |
| `SUM(BITAND(A2:A, B2:B))` | **87** | the same, plus a million-element **lift** (≈71 ms of it) |
| `BITAND(A2:A, B2:B)` | **230** | the same **plus a million spilled cells** (≈143 ms of it) |

So of the 230 ms, **~71 is the lift and ~143 is laying the spill out** — the plan's 120-ns-an-element
premise is wrong, and a faster lift could not have reached 80 ms.

**Where the 143 goes** (by ablation, one thing at a time): **~48 ms** inserting a million values into
`Sheet::spill`, which is a `BTreeMap<CellRef, Value>` — a node a cell — and **~95 ms** the rest of the
lay-out: the footprint check (a walk of the area asking whether anything is in the way), the block
iteration and value clones, and the dependency query over the area.

**Two fixes built, measured, reverted** (the rule this repository writes down):
- The footprint check asked "is anything in the way" a cell at a time — `holds_content` plus a
  `spill` map lookup a cell. Rewritten as a bitmap walk plus a range query over the spill map it
  measured **231 → 223 ms**, inside this machine's noise. (It did find a real bug in the range query:
  a `BTreeMap` keyed by cell answers a *superset* for a one-column rectangle, since it also finds keys
  in other columns of the rows it spans — fixed while it lived, then reverted with the rest.)
- Laying the spill out through one sorted block (`BTreeMap::extend`) measured **229 → 234**: no win,
  reverted.

**What it means:** item 10's target needs the **spill store** changed, not the lift — a per-range
array (`RangeRef` + `Arc<Array>`) instead of a per-cell `BTreeMap`, with every reader of `sheet.spill`
(save, display, the calc host, the commands that clear a spill) moved to it. That is a design knife
of its own, and this round is the measurement that scopes it.

**Instrument left behind:** the three `fn_probe` rows above, and `SPILL_NO_INSERT`-style ablation is
not kept — the numbers are in this entry.

### 46. A column read spent 62 % of its time finding the rows (P, found and fixed 11-25)

**What:** a one-column read of a million rows cost **15.9 ns a cell** — the floor under every
whole-column formula (`SUM`, the criteria family, the lookups' scans, `MEDIAN`).

**Where it went** (by ablation — each variant built and measured):
| variant | ns/cell |
|---|---:|
| the row walk alone (no kind run, no payload, no formula test) | **9.8** |
| + the kind's run | 11.2 |
| + the payload and the number | 14.8 |
| + the formula test | 15.9 |

So **62 % of a column read was the enumeration**: per row the store looked the row up by key — a
`row_bits` test, a popcount and an index into the band's rows — when a band's rows *are* the keys it
holds, in order, on a sheet whose row map is the identity.

**Now:** walking the band's rows in order instead of looking each key up:

- `visit_packed`, one column: **15.9 → 11.9 ns a cell (−25 %)**; `visit_values`: ~17.5 → 14.4.
- On the function page, five passes a row: **`SUM`/`COUNT`/`MAX`/`MIN`/`AVERAGE` 29–32 → 25–26 ms**,
  **`COUNTIF`/`SUMIF`/`AVERAGEIF` 17–18 → 15–16**, **`MATCH`/`VLOOKUP`/`XLOOKUP` 17 → 15–16**,
  `MEDIAN` 52 → 48, `LARGE` 26 → 23, `SUMPRODUCT` 23 → 21. A mapped row map (a row inserted or
  deleted) keeps the old loop, which resolves logical to physical rows.

**What the ablation says about the goal:** a million-row column read costs **9.8 ms even doing
nothing**, so the plan's 10 ms target for this family *is* the enumeration. Two ways past it, both
design knives: a per-band flag that says "every row here is dense and starts at column 0" (then a
column's payloads are a fixed stride apart and a fold reads one load a row), or the spill-store
change of `gaps` §45.

**Test:** the model suite (174) and the two differentials that read columns through the walk
(`criteria_scan`, `xlookup_column`) pass unchanged — the walk is the one every one of them uses.

**Three more ways to the same 11.9 ns, tried and refuted** (measured, reverted):
- skipping the presence bitmap for a row that is full from column 0 (`cells == columns`) — the
  bitmap test and rank it replaces are not where the time is: **12.0 ns, no change**;
- skipping the formula search for a row with no side table (`extras.is_none()`) — part of the same
  1.1 ns the ablation attributed to the formula test: no change;
- inlining the row loop into the packed path so the per-cell work is not passed back through a
  closure — **14.5 ns, worse** (the loop no longer fits the inliner's budget the way the closure
  version does).

So the 11.9 ns is the callback dispatch plus the row iteration itself: a column read hands a million
cells to a caller's closure, and that is the floor of this shape. Going under it means handing over
something other than a cell at a time — the dense-band stride read or the spill store.

### 47. A whole-column sum folds inside the store (P, found and fixed 11-25)

**What:** `SUM`, `COUNT`, `MAX`, `MIN` and `AVERAGE` over a million rows read **25–26 ms** each, and
the plan's target for the family was 15.

**What was tried first, and refuted by measurement:** giving a column read its own tight loop with the
arithmetic inside — `CellStore::fold_column` — measured **11.5 ns a cell against the packed walk's
12.4** on the store directly. So the callback a cell was only about **1 ns** of the 11.9 the walk
costs; the row iteration is the rest, and no amount of moving the callback about touches it.

**Now:** the fold exists and is wired: `visit_aggregate` takes it for a one-column block of values
(the walk stays for a row, a two-column block, or a column that holds formulas). It is worth what the
page measures — the arithmetic, the count `AVERAGE` divides by and the extreme `MAX`/`MIN` keep all
move into the store's loop, and a formula or an array in the column declines so the evaluator's own
paths take the block.

**Measured:** `SUM` **25 → 20 ms**, `COUNT` **26 → 21**, `MAX` **26 → 20**, `MIN` **26 → 21**,
`AVERAGE` 25 → 24 (the page's call is two columns, and an average pays the count as well).

**What it settles about the aim:** a million-row column read costs 11.5 ns a cell whatever the
callback does, so a two-column `SUM` cannot go under ~23 ms by moving work around — the 15 ms aim in
this round's brief is retracted. It needs the **dense-band stride read** (a band marked "every row
full from column 0", so a column's payloads are a stride apart and the fold never visits a row) or
nothing.

### 48. The column-read wall is a chain of three loads a cell (P, measured 11-25)

**What was tried:** the dense-band stride read — a band whose rows are all full from column 0 and all
one kind (`Band::uniform`), where a row's `i`th stored cell *is* its `i`th column, so the fold takes
`payloads[cells_at + col]` with no presence bitmap and no run cursor.

**It works, and it is not enough.** On the store directly, five-pass probe:

| sheet width | `fold_column` before | after |
|---|---:|---:|
| 1 column | 11.5 | **8.1 ns/cell** |
| 2 columns | 11.5 | **8.6** |
| 8 columns | 11.5 | **9.9** |

and the page did not move: `SUM` 20 → 21 ms, `COUNT`/`MAX` 21, `MIN` 20, `AVERAGE` 24 → 21 — inside
this machine's ±2 ms. Reverted, per the rule.

**The width sweep is the finding.** The same fold over one column costs 8.1 ns a cell whether the
sheet is one column wide or eight (9.9), so this is **not** bandwidth and not the cache lines a
strided column touches: it is **latency**, a chain of three dependent loads a cell — the row's
`cells_at`, the payload it indexes, and the number that payload slots into. Three L2-latency loads
are ~25 cycles, which is the 8 ns. The bitmap, the run cursor, the callback and the kind were each
measured and are each about 1 ns of it.

**What that means:** the strip below ~8 ns/cell needs the **payload to hold the number inline**
(8 bytes in the stripe instead of a `u32` index into the `numbers` table), which removes one link
from the chain — a storage-format change of the same order as the spill store, and not a loop. Until
then, "a million-row column read in 10 ms" is a property of the layout, not of the code, and the
`SUM` family's 20–21 ms is that layout's price.

**Round 66 measured that proposal and refuted it:** the two chains cost the same (2.3 ns a cell
each — see §50). The number table is a few hundred entries, always in cache, so the load it adds is
free. What is left is the row walk, not a load chain.

### 49. The spill lay-out is 75–78 ms of a million-cell array, priced phase by phase (P, measured 11-25)

**What the phase timer says** (`fn_probe` with `CELLA_PROFILE=1`, one pass, 1M rows; the probe rows
`column, spilling` / `column, folded` were added for this):

| call | eval | spills | total |
|---|---:|---:|---:|
| `=SUM(A2:A1000001)` | 10.6 | — | **12 ms** |
| `=A2:A1000001` (spills a million cells) | 64.4 | **75.0** | **142 ms** |
| `=SUM(BITAND(A2:A,B2:B))` | 84.6 | — | **86 ms** |
| `=BITAND(A2:A,B2:B)` (spills a million cells) | 166.5 | **77.8** | **247 ms** |

So round 58's "143 ms is the spill" was half right: the **lay-out is 75–78 ms**, and the rest of the
difference is the **eval side**, which pays ~54 ms to materialise the range into an `Array` (a
`Vec<Value>`, 24 bytes a cell) — `=A2:A1000001` costs 10.6 ms read + 53.8 ms building the array it
hands to the cell, while `=SUM(A2:A1000001)` never builds it at all.

**The shape of the fix is now measured, not guessed:** keep the values as the `Arc<Array>` the
evaluator already produced — `Sheet::spill_values: BTreeMap<CellRef, Arc<Array>>` beside
`spill_ranges` — instead of a `BTreeMap<CellRef, Value>` with a node a cell. Expected `=A2:A` 142 →
~70 ms and `BITAND` 247 → ~175, with `SORT`/`FILTER`/`UNIQUE`/`VSTACK`/`DROP` sharing the fraction.

**The cost of that change is its blast radius:** 60 non-test sites read `sheet.spill` — 30 in `calc`
(lay out, clear, evict, contested areas, the "is there a spill here" questions), 17 in `engine`
(display, cut/copy/paste, print, pivot, solver, analyze, format, inspect, memory accounting), 5 in
`xlsx` (write and read them back), 6 in `model` (`value`/`cell` fallbacks, `used_range`), 1 in
`ui-egui` (the grid). The migration plan, the two helpers that carry most of it (`Sheet::spilled`)
and the traps (blank spills showing 0, contested-area eviction order, `spilled_nothing`'s cell-by-cell
compare, save must stay byte-identical) are written up in `plan/spill-store.md`.

**Landed the same day.** `Sheet::spill_values: BTreeMap<CellRef, Arc<Array>>` beside `spill_ranges`,
with `Sheet::spilled(c)` and `Sheet::spilled_in(range)` carrying the readers, and all sixty sites
moved: model's `value`/`value_ref`, calc's lay-out (one insert; clearing drops one entry), the
presence questions the ranges answer themselves, the engine's display/cut/copy/paste/print/pivot/
solver/analyze/memory, the xlsx reader (one array built from the file's cached cells) and writer, and
the grid. Measured with the probe and the five-pass page:

| call (1M rows) | before | after |
|---|---:|---:|
| `=A2:A1000001` (spills) | 142 ms | **77** |
| `=BITAND(A2:A,B2:B)` (spills) | 247 | **173** |
| `DROP` | 749 | **574** (−23 %) |
| `CHOOSECOLS` | 267 | **150** (−44 %) |
| `TOCOL` | 139 | **96** (−31 %) |
| `VSTACK` | 189 | **139** (−26 %) |
| `WRAPROWS` | 197 | **151** (−23 %) |
| `BITAND`/`BITOR` | 228 | **180** (−21 %) |
| `SORT` | 255 | **207** (−19 %) |
| `HSTACK` | 331 | **284** (−14 %) |
| `SORTBY` | 284 | **240** (−15 %) |
| `MAP` | 423 | **335** (−21 %) |
| `UNIQUE` | 302 | **274** (−9 %) |

Every function whose result spills moved; the rows that do not spill stayed put within the machine's
noise (`SUM` 20 → 22, `COUNTIF` 15 → 16, `XLOOKUP` 15 → 16, `PIVOTBY` 983 → 1.03 s). Gates: model
174, calc 175, engine 649, xlsx 479, ui 231, the criteria/lookup differentials, the command-sequence
fuzz, wasm, `perf` 59/59.

### 50. Inline numbers would buy nothing — the number table is free (P, measured and refuted 11-25)

**What was proposed** (the next knife after §48): make the payload slot hold the number itself — a
`u64` payload stripe instead of a `u32` index into the band's `numbers` table — so a column read's
chain of three dependent loads becomes two. The cost would be four more bytes a cell and the loss of
the table's deduplication.

**Measured, and refuted.** `read_probe` now runs the two chains over the same data and the same loop,
black-boxed so the optimiser cannot fold them away:

| chain | ns/cell |
|---|---:|
| payload → the number table (what the store does) | **2.3** |
| the number in the payload slot (one load less) | **2.3** |

**Nothing.** A band's number table is a few hundred entries — tens of kilobytes, always in L1 — so
the extra load is hidden by the memory subsystem and the out-of-order engine. The idea is dead on
arrival and its memory cost (four bytes a cell, plus losing deduplication) is not paid.

**The corrected attribution of a column read (all measured):** the whole read is 10.5 ns a cell with
the store's fold and 14.3 with the packed walk; the payload→number chain is 2.3 of it, the kind's run
1.4 and the formula test 1.1. The remaining **8–9 ns is the row walk itself** — resolving a band, its
row, the row's offset in the payload stripe, and calling the caller once a cell. That is not a load
chain to be shortened: it is the shape of a row-major store answering a column question, and the only
way under it is to keep a column contiguous (column-major storage, or a per-column sidecar) — a
redesign of a different order from the two knives just landed.

**Instrument left behind:** the two-chain measurement in `read_probe`, so the next proposal of this
kind is priced before it is built.

### 51. Column storage: measured, sized, and not recommended (evaluation, 11-25)

**What:** whether the row-major store should become columnar, to take the whole-column formulas under
their 10.5 ns-a-cell floor. The full evaluation, with every number and the three shapes compared, is
[performance-columnar.md](performance-columnar.md); this is the summary.

**Measured:** a one-column fold costs **10.5 ns a cell** in the store today, a row-wise block 5.0, a
contiguous `Vec<f64>` fold **2.2** and a row read out of eight contiguous columns **0.2** (eight
sequential streams prefetch) — so the ceiling is a **4.8×** win on the columnar workloads and no
measurable regression on the row-wise ones. At the ceiling `SUM` would be ~6 ms, the criteria family
~8, `MEDIAN` ~25 (the sort dominates).

**Sized:** a full column-major store is not a knife but a rewrite — 70 public store functions, 380
call sites (`cells.set` 251, `cells.get` 57, `iter_range` 52, `visit_packed` 18, `visit_values` 5,
`put_row` 5), and the mixed workloads make a hybrid "two layouts and an invariant", whose failure
mode is a **wrong number**.

**The one shape worth a pilot:** a per-column sidecar cache inside `store.rs` (a contiguous
`Vec<f64>` plus a presence bitmap per cached column, built on the second whole-column read, capped,
invalidated by every mutator). Contained to one file, no caller changes; the risk is a stale read, so
it needs a hook in every mutator and a write-then-read fuzz before it is trusted.

**Recommendation:** do not start the rewrite. The pilot is worth ~a day *only* if a whole-column
aggregate over a million rows is on someone's interactive path — a live dashboard over 1M rows. The
page already has these rows among the fastest we know of; the remaining chase is engineering, not
user-facing.

### 52. A pivot refresh formatted every number to decide it had seen it (P, found and fixed 11-25)

**What the owner's question turned up.** "Users have several million rows and drag pivot fields and
slicers all day" is the one interactive workload this repository had not measured. `pivot.refresh` —
what a slicer drag runs — cost **889 ms at a million rows** (1.65 s budget, 54 % of it).

**Where it went** (phase timer, then reading the loop): the source read was 132 ms and everything
after it 740. Inside that, the record loop called `make_item` for **every value of every axis field**:
a number was formatted with `display::format` (numfmt, ~300 ns) *and* had a `String` key built
(`"n" + bits`) **even when the item was already known**. A million records × the fields, on a report
with **five groups**. The accumulator map also keyed by two `Vec<u32>`s while a record enters *every
prefix pair*, so one row field and one column field allocated eight vectors a record.

**Now:** the key is an enum — blank / number bits / grouped / text / boolean / error — so the loop pays
no formatting and no allocation for numbers, and the label is built only when an item is new; the
accumulators key by two interned path ids (a trie node per `(parent, item)`).

**Measured:** `pivot.refresh` **889 → 384 ms at 1M (−57 %)**, and **42 ms at 100k** of a 160 ms budget.
The budgets are tightened to 600 ms / 80 ms so the win is held. What is left of the 384 is the source
read (~122 ms — one `sh.value` a cell, the next knife) and the report's own build.

**The product context this exposed, recorded here because it decides the next work:** a Cella sheet
holds 1,048,576 rows like Excel's, so "several million rows" cannot be one sheet's range — a pivot's
`Range` source is capped there, and the other source kind (`DataModel`, a Power Pivot/OLAP model) is
carried but **never recomputed** ("the seam an OLAP engine plugs into"). So for the owner's users the
options are the 1M-row ceiling, a data model behind that seam, or a multi-sheet source — a product
decision, not a performance one.

### 53. The groupers' own work is 27 ms — their argument arrays are the second (P, measured 11-25)

**What the owner's workload led to.** "Several million rows, dragging slicers" is a **grouping** over
a lot of records, so the question was where a grouper's second goes. `fn_probe` gained rows for the
catalogue's own shapes; fastest of three, a million rows:

| call | time |
|---|---:|
| `SUM(MOD(A2:A,4))` — the same lift, folded at once | **199 ms** |
| `GROUPBY(MOD(A2:A,4), C2:C, SUM)` — four groups | **649 ms** |
| `GROUPBY(D2:D, C2:C, SUM)` — four groups, **text** keys | **1.80 s** |
| `GROUPBY(A2:A, C2:C, SUM)` — a million distinct numbers | **1.36 s** |
| `PIVOTBY(D2:D, MOD(A2:A,4), C2:C, SUM)` | **4.00 s** |

**Inside the grouper, nothing is slow.** Temporary timers in `groupby` (removed again) said: `prepare`
**2 ms**, the key build **15 ms**, the whole body **27 ms** at a million rows. The wait is not the
grouping.

**It is the evaluator building the arguments.** `arr_arg` is `as_array(v).into_owned()`, so every array
argument is **copied** (a million `Value`s, 24 MB, twice a call), and a whole-column lift such as
`MOD(A2:A, 4)` costs **199 ms** on its own — 2.3× what `BITAND`'s lift of the same shape costs (88 ms
including the fold), so the lift path is uneven as well. A text key column costs about a second more
than a numeric one at the same group count: building and copying a million `Value::Text`s is the
expensive part, not hashing them.

**What that means:** the fix for "drag a slicer over a million rows" is not a faster `GROUPBY` — it is a
grouper that never materialises its inputs. The rows are in the store; the host can stream the key and
value columns into the group loop the way the criteria scans stream one column (`gaps` §44), leaving the
grouping and the report as they are. That is the same shape of knife that took `COUNTIF` from 26 to
15 ms: keep the work in the walk.

**Round 70 corrected two things and refuted a third.**

- **The "four regions" row was not four groups.** `fn_probe`'s `D` column holds an item name that is
  *unique per row*, so `GROUPBY(D2:D, C2:C, SUM)` is a **million-group** report. Timed from inside the
  function (timers since removed): args copy 12.5 ms, key build **127 ms**, and the rest of its 1.78 s
  is the report itself — sorting a million text keys and building a million lines.
- **So the "27 ms body" holds for a *few-group* report only** (measured on the `MOD(A,4)` key): the few
  groups cost ~590 ms *before* the call and 27 inside it; the million-group report costs ~1.7 s
  *inside* it. The earlier sentence in this entry over-generalised and is retracted.
- **The per-group `Vec`s were not the cost either:** grouping the rows into a flat CSR array (counts,
  offsets, one row array) measured **1.78 → 1.78 s** and was reverted, because the line build still
  allocates and, more to the point, the time is in sorting the keys and writing the lines.

**What is left to cut, in order of size:** the ~590 ms a **few-group** grouper pays before its own body
(the evaluator's array build — not yet attributed inside `calc`), and for million-group reports the
sort-and-write. Both are the next rounds' work.

**Round 71 attributed the rest, and found a doubling.** With `calc`'s argument loop and the pass's
phases instrumented (all removed again):

| phase of `GROUPBY(MOD(A2:A,4), C2:C, SUM)` at 1M | time |
|---|---:|
| argument 0 (`MOD` lifted over the column) | **193 ms** |
| argument 1 (`C2:C` turned into an array) | 19 ms |
| the `GROUPBY` call itself | ~100 ms |
| **one evaluation ("eval 1 formulas")** | **~320 ms** |
| **the whole pass ("run")** | **~650 ms** |

The pass's own prints account for all of the 320 (trees 0, eval 320, write 0, spills 0) while `run` is
twice that — because **a formula whose result spills is evaluated twice**: two `eval 1 formulas` lines
of 320 and 328 ms, `run` 648 ms, and the second lays out nothing (`spills 0.0`). The discarded-range
and trim-dependent counts both printed **zero**, so the requeue is the small bucket that carries the
formula's own key.

**That is the next knife and it is a doubling, not a percentage:** every formula that spills pays for
the same work twice. Halving it takes `GROUPBY` 649 → ~330 ms and `PIVOTBY` 4.0 → ~2 s at a million
rows before anything else is touched.

**Left behind:** the probe rows (and the two that price a text column against a number column), so the
next round starts from the same numbers.

**Fixed the same day (§54).** The requeue was the graph marking the formula **dynamic**: the bare
aggregate name in `GROUPBY(rf, vals, SUM)` parses as a name, and an unresolvable name is conservatively
dynamic — so every pass that laid out a spill evaluated the grouper again. `GROUPBY` 649 → **326 ms**
(few groups), 1.78 → **928 ms** (a million text groups), `PIVOTBY` 4.00 → **2.09 s**.

### 54. A grouper was "dynamic" because its aggregate is spelled without quotes (P, found and fixed 11-25)

**What:** the doubling in §53. `GROUPBY(rf, vals, SUM)` puts `SUM` in the formula as a bare name, and
`precedents_at` (`crates/calc/src/eval/special.rs`) answers an unresolvable `Expr::Name` with
`dynamic = true` — "an unknown name may come back as a table or a sheet-scoped name". A dynamic formula
is evaluated again by every pass that lays out a spill (the `dynamic_again` bucket in
`crates/calc/src/recalc.rs`), so every grouper ran **twice**.

**Now:** a name that is a registered function's own, resolves to no definition and to no table, is no
reference at all — a call is `Expr::Call`, so the only thing it can be is that argument. It stays out of
the dynamic set. A name that *is* defined keeps the old conservative answer even when its definition
does not parse (`Rate` defined as `Sheet1!B1`); the first attempt at this rule lost that case and a test
caught it.

**Measured** (`fn_probe`, fastest of two; page five passes):

| call | before | after |
|---|---:|---:|
| `GROUPBY(MOD(A,4), C, SUM)` — four groups | 649 ms | **326** |
| `GROUPBY(A, C, SUM)` — a million number groups | 1.36 s | **711 ms** |
| `GROUPBY(D, C, SUM)` — a million text groups | 1.78 s | **928 ms** |
| `PIVOTBY(D, MOD(A,4), C, SUM)` | 4.00 s | **2.09 s** |
| page: `PIVOTBY` | 1.03 s | **519 ms** |

Functions that spill but whose text carries no bare name (`SORT`, `FILTER`, `UNIQUE`, `BYROW`, `DROP`)
did not move — they were never double-evaluated, which is the control that says the name was the cause
and not the spill.

**Pinned by** `crates/calc/src/tests.rs::a_groupers_bare_aggregate_name_is_not_a_dynamic_reference`: an
unrelated edit does not recalculate a grouper over fixed ranges, and a defined-but-unparsable name still
does. calc 176, engine 649, wasm, `perf` 59/59.

### 55. A lifted `MOD` formatted and parsed every number to leave it alone (P, found and fixed 11-25)

**How it was found.** With the grouper at 326 ms, the argument lift was 193 of them — and `MOD`'s lift
was the outlier among the lifts `fn_probe` prices: `=SUM(ABS(A2:A))` 54 ms, `=SUM(INT(A2:A))` 54,
`=SUM(POWER(A2:A,1))` 55, `=SUM(A2:A*1)` 34 — but `=SUM(MOD(A2:A,4))` **194 ms**, 3.5× the others.

**Cause:** `modulo` ends with `clean(r)` — Excel's fifteen-significant-digit snap — and `clean` was
`to_sig_digits`, which **formatted the number to a string and parsed it back**:

```rust
let s = format!("{:.*e}", sig - 1, x);   // a heap allocation
s.parse().unwrap_or(x)                   // and a parse
```

That is ~150 ns a call, and `MOD`, `ROUND`, `TRUNC`, `QUOTIENT`, `MROUND`, `CEILING` and friends call it
**per element** — a million times for a whole-column call.

**Now:** an integer with no more digits than `sig` significant ones is already what the round trip
produces, so it returns unchanged (a compare and a multiply-by-power check); anything else takes the
format-and-parse as before.

**Measured:**

| call (1M rows) | before | after |
|---|---:|---:|
| `SUM(MOD(A2:A,4))` — the lift | 194 ms | **72** |
| `SUM(POWER(A2:A,1))` — the control | 55 | 54 |
| `GROUPBY(MOD(A,4), C, SUM)` | 326 | **196** |
| `PIVOTBY` (`fn_probe`) | 2.09 s | **1.90 s** |
| page: `PIVOTBY` | 519 ms | **393 ms** |

Functions 172, calc 176, engine 649, clippy, wasm, `perf` 59/59.

### 56. A pivot's source read does not want a block walk (P, measured and refuted 11-25)

**What was tried:** `pivot.refresh` spends ~122 ms of its 384 reading the source with `sh.value(a cell)`
— a store lookup and a spill check each, five million of them for a million rows of five columns. Two
rewrites were built and measured against the gate's own scenario:

| reading the source | `pivot.refresh` at 1M |
|---|---:|
| `sh.value` a cell (as it was) | **382 ms** |
| one block walk into a `Vec<Value>` grid (+ a `Vec<bool>` of answered cells) | **427 ms** |
| a row at a time through `iter_range` (+ two `Vec`s a row) | **797 ms** |

Both reverted. The first pays 120 MB of grid and writes every cell twice; the second allocates two
vectors a row — and the per-row walk re-resolves the band a million times. **`sh.value` a cell is
already 24 ns**, which is the store's one-cell read price, and neither rewrite beat it: the report's own
`Source` — a `Vec<Vec<Value>>`, 24 bytes a cell plus a `Vec` a row — is what the read is paying for, and
it stays the same size either way.

**What would actually be cheaper:** `Source` as one flat `Vec<Value>` and a width, so a record is a
slice of it and the row loop is a memcpy into the right offset. That is a wider change (every reader of
`src.rows` in `crates/engine/src/pivot.rs`) and it is the next round's work if the pivot refresh is
worth more time than the ~120 ms it would save.

**Landed the same day.** `Source` keeps `data: Vec<Value>` and `width`, with `row`/`rows`/`height` on
it; the sheet and model readers append into that one block and the report walks slices. Five passes
against the gate's own scenario:

| `pivot.refresh` | before | after |
|---|---:|---:|
| 100,000 rows | 43 ms | **34 ms (−21 %)** |
| 1,000,000 rows | 382 ms | **352 ms (−8 %)** |

The smaller sheet gains more, which is what per-row overhead looks like when it is a larger share of
the run. Engine 649, clippy, fmt, wasm, `perf` 59/59.

### 57. A `GROUPBY` built its report a line at a time (P, found and fixed 11-25)

**Where the million-group report's second went.** With the sort measured at **25 ms** for a million
keys and the key build at 127 (text) / ~40 (numbers), what was left was the report itself: `groupby`
built a `Vec<Vec<Value>>` — an allocation a line, a million of them for a report keyed by an item
column — and then copied every line into an array through `from_rows`.

**Now:** the report is written into **one block** in the order it reads, and every group's aggregates
are computed once into a second flat block (which is also what a value-column sort needs to look at,
instead of sorting lines of `Value`s).

**Measured** (`fn_probe`, fastest of two; page five passes):

| call (1M rows) | before | after |
|---|---:|---:|
| `GROUPBY(A, C, SUM)` — a million number groups | 709 ms | **583** |
| `GROUPBY(D, C, SUM)` — a million text groups | 928 ms | **750** |
| `GROUPBY(MOD(A,4), C, SUM)` — four groups (control) | 196 | 197 |
| page: `GROUPBY` | 703 ms | **585 ms** |

`PIVOTBY` did not move: its report still goes through `from_rows`, and the same change is waiting
there. Functions 172, calc 176, engine 649, clippy, wasm, `perf` 59/59.

**Round 78 itemised the rest of a million-group report — and there is no dominant cost left**
(`fn_probe`, 1M rows, `GROUPBY(A, C, SUM)`, 552 ms end to end; timers removed again):

| phase | time |
|---|---:|
| the evaluator: argument materialisation (`C2:C` and `A2:A` into arrays, the pass, the commit) | ~150 ms |
| the key build (`KeyIds::push` a record) | ~40 |
| `keys.values` — one `Vec<Value>` a group, a million of them | **69** |
| the group rows (`Vec` a group) | **30** |
| per-group aggregates | **68** |
| the sort of a million keys | **25** |
| the report's emit | **30** |
| prepare (records in, filter out) and the probe's own session overhead | ~140 |

Every item is 25–70 ms, which is 5–12 % each: the phase that dominated — the double evaluation (§53/§54)
and the line-at-a-time report (§57) — are gone, and what is left is a dozen small ones. The two that
would pay next are `keys.values` (a `Vec` a group where a flat block would do, the same shape as §57)
and the per-group aggregates; together they are ~137 ms of 552, or a 9 % win at this size.

**Then `PIVOTBY`'s own cell copy:**' `body_line` cloned each grid cell's row list — up to a million
entries a cell, sixteen cells copied for a four-by-four report. Borrowed now: `fn_probe` 1.90 →
**1.77 s** (−7 %), the page 398 → 389 ms (inside the noise; the page's grid has less to copy). What is
left of `PIVOTBY`'s second is the same unexplained block `GROUPBY` has.

### 59. The data model wants a professional SQL engine (evaluation, 11-25)

**What the owner decided:** use an existing SQL engine rather than a hand-written executor — hidden bugs in a hand-rolled query engine are the risk to avoid. The survey, the candidates, Cella's constraints and the spike that decides between them are in [sql-engine-research.md](sql-engine-research.md); this is the summary.

**The one number that matters:** DuckDB is the best engine for a 5M-row group-by and the **only candidate that fails a Cella rule** — its Rust crate does not build for `wasm32` (the browser build would lose the feature). DataFusion is pure Rust, Apache-2.0 and wasm-proven by a third party; SQLite is the risk-free floor (public domain, everywhere, ~1–2 MB) that trades speed for certainty; Polars' SQL layer is a transpiler with partial coverage and no upstream wasm; GlareDB is young; chDB needs an external library, which fails "no installs".

**Recommendation:** put the engine behind one internal interface (`open`/`query`) so the choice is replaceable, then **spike DataFusion and DuckDB on the same 5M-row Parquet and the same SQL** — five passes each, recording query ms, load ms, memory, build time and the wasm build's size or failure — and pick with numbers. Within ~2× of DuckDB with wasm green → DataFusion; a large gap with a desktop-only data model acceptable as a product statement → DuckDB; both unacceptable on build time or size → SQLite behind the same interface.

### 60. The engine is in the tree, and our rows are the slow part (measurement, 11-25)

**What landed:** `crates/sql` (L2) — one interface in front of DuckDB: `Engine::in_memory`,
`load_rows` (Cella's `Value` rows), `load_file` (a Parquet/CSV file the engine reads itself),
`query` (a bounded `Table` back), `execute`. The engine is an optional native-only dependency
(`duckdb`, `bundled`); the crate compiles for `wasm32` and every call returns `Unavailable` there,
which is what `available()` reports to the UI — the desktop-only data model the owner accepted.
Names and paths are quoted by doubling the quote, so a table called `Sales"; DROP TABLE x --` is a
name; a result is capped by `MAX_ROWS`/`MAX_CELLS` while it is read. Ten engine tests run with
`cargo test -p cella-sql --features duckdb`; the type-inference, quoting and "no engine" tests run
in `ci` without the feature.

**The number nobody had — the bridge.** Handing Cella's own rows (`Vec<Value>` a row) to the engine
costs **716–735 ns a row**: **1M = 0.72 s, 5M = 3.68 s**. Measured out of tree, where the values
were converted separately, it splits as ~200 ns for `Value` → engine parameter and ~375 for the
append; the in-tree figure is the whole of `load_rows`, cell by cell. **A file the engine reads
itself costs 22–99 ns a row:** Parquet → in-memory table is **99 ms at 1M and 109 ms at 5M** (the
scan is vectorised and threaded, so 5M costs barely more than 1M). That is an architectural fact,
not a micro-optimisation: **big data enters the model as a file the engine reads**, and `load_rows`
is the one-time "add this sheet to the model" action.

**Queries, in the tree** (five passes, best): a 768-cell group-by **6.4 ms at 1M, 14.5 ms at 5M**; a
slicer (4 of 64 regions) then the same group-by **4.7 / 9.5 ms**; top-20 products **1.7 / 4.2 ms**.
Our own sheet path takes 352 ms for a million rows, so the engine is ~55× faster at 1M and two
orders of magnitude under the 200–300 ms target at 5M. The integration is what is left.

**The spike's data was wrong.** Its synthetic sales table used `i % 64` for the region and `i % 12`
for the month, which makes **192** distinct pairs, not 768 — the round-80 group-by was timing a
quarter of the report. The probe derives every key from its own divisor now. The old numbers stand
(they were 192-cell reports, not 768), but no page should quote a cell count that was not checked.

**Next:** the model has to keep its rows *in* the engine (today `ModelTable::rows` is a
`Vec<Vec<Value>>` capped at 4M cells) and a pivot's cells have to be the engine's answer, re-queried
when a slicer moves. The engine itself needs nothing more.

### 61. The model's tables live in the engine, and `data.model.query` is the way in (11-25)

**What landed:** `crates/engine` gained the data model's SQL engine behind a `sql-engine` feature —
the desktop and CLI apps turn it on (`cella-engine/sql-engine` → `cella-sql/duckdb`), the browser
build leaves it off, where the command answers "needs the desktop app" instead of pretending. One
command comes with it: **`data.model.query {sql} → {columns, rows}`**, read-only SQL over the
model's tables, reachable from the grid's command set, the control channel and MCP alike.

**How the copy stays honest.** The engine holds a copy of every model table, filled on first use
(~720 ns a row, once) and rebuilt whenever the model changes: a query loading into the model, a
table removed, a query deleted, an undo/redo (a step can put an older model back), or a switch of
document (the copy is keyed by the document's id). A test proves the copy follows a refresh
(22 → 101) and that a table removed from the model disappears from the engine.

**Read-only on purpose.** The database is the one the model lives in, so a statement that is not a
query never reaches the engine: `cella_sql::is_read_only` accepts `SELECT`, `WITH`, `DESCRIBE`,
`SHOW`, `SUMMARIZE` and `VALUES` (comments skipped), and the engine runs one prepared statement. A
query box must not be a way to write a file (`COPY … TO`), attach a database or drop a table.

**Numbers** (`cargo run -p cella-engine --example model_sql_probe --release --features
sql-engine`; the model is capped at 4M cells, which is 1M rows × 4 columns):

| case | 200k rows | 1M rows (the cap) |
|---|---:|---:|
| query → model, the one-time load | 0.25 s | 1.27 s |
| the first SQL query (model → engine included) | 0.10 s | 0.40 s |
| **`data.model.query`: the 768-cell group-by** | **3.9 ms** | **5.4 ms** |
| the same report with a slicer (4 of 64 regions) | 2.9 ms | 3.6 ms |
| **our own `pivot.refresh` of that report** | **127 ms** | **640 ms** |

So the aggregation the pivot spends **640 ms** on takes **5.4 ms** in the engine — 118× — and a
slicer narrowing the same data costs 3.6 ms more, which is what makes a drag one query. What is
**not** done yet: the pivot does not ask the engine anything. Its cells still come from `compute`'s
own scan; the next round feeds it this command's answer and the 4M-cell cap goes with it (the rows
no longer have to be stored in the document).

### 62. The pivot asks the engine, and the model's rows stop being copied (measurement, 11-25)

**What landed:** a report over a model table is aggregated by the SQL engine
(`compute_with_source_sql`), and a cell edit on a workbook that has a model is cheap again. At
1M rows × 4 columns, release: **`pivot.refresh` 640 ms → 8.0 ms**, `insert.pivotTable` 0.25 → 0.14 s,
and **one cell edit 109 ms → 0.1 ms**.

**How the engine answers a report** — three queries, no layout changes:

* one `GROUP BY` over the row and column fields, `ORDER BY MIN(row_number() OVER ())`: the earliest
  record each leaf holds is exactly the order the item first appears in, so the report lists its
  items the way a scan does;
* **the leaves are the aggregation, and every sub-total is composed from them** at the end
  (sum of sums, count of counts, min of mins; `COUNT`/`SUM` pairs for an average). A function the
  scan offers that is not additive — product, standard deviation, variance — keeps the scan.
* **labels stay in Rust**: SQL groups by the raw date serial, and `classify`/`item_of` still turn
  it into `Jan`/`Qtr1`/`2024`, so a grouped date field is identical on both paths by construction;
* a filter names *labels*, so one `DISTINCT` per filtered field turns them back into the raw values
  a `WHERE … IN (…)` can name (a blank becomes `IS NULL`);
* a related table's columns, a DAX measure, a text column summed, or an engine error falls back to
  the scan, which answers everything (`Ok(None)` in `sql_aggregate`, a `log::warn!` for the error
  case). `compute_detailed` reports which path ran, so a test can see it.

**Proven by a differential test** (`crates/engine/tests/pivot_sql.rs`, 8 cases): the same report
computed both ways and compared cell by cell — one field, a cross-tab, two row fields with
sub-totals, two value fields, sum/count/average/max/min, years/quarters/months, a slicer's
selection, and the three ways the engine says no (product, stddev, a sheet source).

**The source read had to go too.** A report's first act was `read_source_kind`, which copies a
model table's rows into a `Source` — 370 ms of a 372 ms refresh. `read_source_shape` now builds the
headers and column types from `DESCRIBE` (the engine's own typing, the same rule the writer uses)
and leaves the rows where they are; the scan reads them only when the engine declined.

**And the copy every edit makes.** `cmd::commit` clones the whole `Workbook` for its undo entry, so
a million-row model was copied **109 ms on every keystroke** — a model workbook was unusable. The
rows are `Arc<Vec<Vec<Value>>>` now, so a clone shares them; nothing mutates a table's rows in
place (a refresh replaces the table), and `#[serde]` writes the same JSON. `gaps` §61's "the model
may hold millions of rows" only became true here.

**What is left:** the model's rows still live in the document (4M-cell cap) — the next step is a
table loaded from a file that the engine reads itself, which needs the table's data to move behind
the engine, and only then can a report do the joins (relationships) and the measures in SQL.

### 63. A model table can be bigger than the cache: its rows stay in the file (11-25)

**What landed:** `data.model.loadFile {paths, name?, format?}` — a model table whose rows are a
Parquet or CSV file's, read by the engine itself. The document keeps the paths, the column names and
the column types the file reported (`DESCRIBE`); no cell is copied into the workbook, so the
4M-cell cache stops bounding the model.

**The numbers** (`model_sql_probe`, release, a 5M-row × 4-column CSV — 20M cells, five times the
cache):

| step | time |
|---|---:|
| write the 109 MB CSV | 0.44 s |
| **`data.model.loadFile` (the engine reads it)** | **0.11 s** |
| the 768-cell group-by over it | 16.1 ms |
| **`pivot.refresh` over 5M rows** | **17.6 ms** |

For comparison, the same report over a 1M-row table cached in the document is 9.0 ms: a file-backed
table is five times bigger and 2× the report time, which is the engine scanning a file instead of
memory.

**What changed around it:**
- `ModelTable` gained `source_file` (paths + format) and `column_types`, both `serde`-defaulted, so
  a part written before this reads unchanged; the hand-written codec in `crates/xlsx` round-trips
  them, and a test saves → reopens a file-backed model and re-reads the file.
- `Session::model_engine` registers a file-backed table with `read_parquet`/`read_csv` instead of
  handing rows over one at a time (22–99 ns a row against ~720).
- A report reads the table's *shape* from the stored column types, so no report runs `DESCRIBE`;
  the field list's "is this column a number" (and therefore its default sum-vs-count) comes from
  the same list.
- What SQL cannot aggregate — a related table, a measure, a product — still falls back to the
  record scan, which now reads a file-backed table's rows **out of the engine**. That fallback is
  bounded by the engine's one-answer cap, so a table past it reports what is wrong instead of
  quietly counting nothing. A relationship *to* a file-backed table is refused with a sentence
  saying the join is not written yet, and a value filter on one says to select items with a slicer.
- `data.model.tables` asks the engine for a file-backed table's row count (the document has none).

**Proven:** `crates/engine/tests/model_file.rs`, five cases — a 4.4M-cell CSV (past the cache)
whose report's grand total equals the file's sum, save → reopen, a missing file that leaves the
model as it was, a path or format that makes no sense, and the browser build's "needs the desktop
app".

### 64. A report joins its dimensions in SQL (11-25)

**What landed:** a report over a model table with relationships is answered by the engine, which
joins the related tables itself. That is what lets a **file-backed fact table** (whose rows the
document does not hold) have a report at all, and a dimension's column be a row field, a filter or
a slicer.

**How:** one plan (`plan_source`) lists the tables a source is made of — the base table and every
dimension a relationship joins to it, breadth first, aliased `t0`, `t1`… — and the *same* plan
gives the source's headers, the row walk (the scan) and the SQL, so the three cannot disagree
about what a header means:

```sql
FROM (SELECT row_number() OVER () AS __cella_ord, * FROM "Sales") AS t0
LEFT JOIN "Region" AS t1 ON t0."RegionId" = t1."Id"
```

A dimension's column is `t1."Name"` in the `GROUP BY`, in the `SELECT` and in a filter's
`SELECT DISTINCT`; a slicer's label maps back to `Region[Name]`'s own values against the joined
table. A table related twice is added once, a chain (fact → product → category) is two joins, and
the shape a report reads (`read_source_shape`) is the same plan's headers with each table's column
types — the file's for a file-backed table, one `DESCRIBE` for a cached one (never a scan of its
rows).

**Proven:** three new differential cases (`crates/engine/tests/pivot_sql.rs`, eleven in all): a
report grouped by a dimension's column, a filter on a dimension's column, and a **file-backed fact
joined to a cached dimension**, whose grand total is the file's sum — a report the record scan
cannot answer at all.

**Numbers** (the probe, release, 5M-row file-backed fact × 4 columns): `loadFile` 0.13 s, a
768-cell report over it **6.4 ms**, and the same report **grouped by the joined dimension 17.7 ms**
— the join is ~11 ms of it.

**What is left: measures.** A measure is DAX evaluated per cell (`CALCULATE`, `DIVIDE`, aggregate
functions), and translating it into a `SELECT` expression over this same plan is the next step;
until then a report with a measure takes the scan, which a table past the cache cannot do — and
`model_source_with` now says so in one sentence instead of quietly producing blanks.

### 65. A measure is a formula, so every level is computed by SQL (11-25)

**What landed:** a report whose value field is a measure is answered by the engine. Before this a
measure was the one thing that always took the record scan — and on a file-backed fact table (whose
rows the document does not hold) a measure report could not be computed at all.

**Why it needs a different query.** The engine's other paths compose each sub-total from the leaf
groups: sums add up, counts add up. A measure is a *formula*, and a formula does not compose that
way — the average of two averages is not the average of the two groups, and a sum of two ratios is
not the ratio of the sums. So the report asks SQL for **every level at once**:

```sql
SELECT t1."Name", t0."Month", GROUPING(t1."Name"), GROUPING(t0."Month"), MIN(t0.__cella_ord),
       SUM(t0."Amount") / NULLIF(COUNT(t0."Amount"), 0)
FROM (SELECT row_number() OVER () AS __cella_ord, * FROM "Sales") AS t0
LEFT JOIN "Region" AS t1 ON t0."RegionId" = t1."Id"
GROUP BY GROUPING SETS ((t1."Name", t0."Month"), (t1."Name"), ())
```

`GROUPING(col)` says which columns each answer was grouped by, so every row is routed to its level.

**The DAX-to-SQL translation** (`translate` in `pivot.rs`, over the tree `cella_dax` now exposes):
`SUM`/`AVERAGE`/`MIN`/`MAX` over a numeric column; `COUNT`/`COUNTA` as `NULLIF(COUNT(x), 0)` (an
aggregate over no rows is **blank** in this language and 0 in SQL, so the zero is turned back into a
blank); `DISTINCTCOUNT` as `COUNT(DISTINCT x)`; `DIVIDE(a, b[, alt])` and `/` as `a / NULLIF(b, 0)`
(division by zero is blank here, an error in SQL); `BLANK()` as `NULL`; arithmetic as SQL
arithmetic; another measure by translating its formula (depth-capped, like the evaluator); and
`CALCULATE(expr, f…)` by ANDing each filter's `t1."Name" = <value>` into the aggregate's
`FILTER (WHERE …)`. What the translation does not carry — a text column summed, a column of a table
the source does not read, a grouped (date) axis — keeps the per-cell evaluation, which is the scan.

**Numbers** (the probe, release, 5M-row file-backed fact): a report with a ratio measure at two row
levels is **27.9 ms** (`pivot.refresh`); the same report on one row level, grouped by a dimension,
is 19.9 ms.

**Proven:** four more differential cases (`crates/engine/tests/pivot_sql.rs`, fifteen in all): a
measure answered at every level, a ratio measure at two row levels (where composing from children
would be wrong), a `CALCULATE` measure, and a measure over a file-backed fact — the one the scan
cannot do at all.

### 66. A measure over a grouped axis is grouped by the date part in SQL (11-25)

**What landed:** the last combination a model report could not put in SQL — a **measure** with an
axis field that is **grouped** (a year, a quarter, a month, a day of the month). §65's translation
grouped by the column itself, which would have put every raw date in an item of its own.

**How:** the axis expression becomes the date part, from the spreadsheet serial
(`to_timestamp((serial - 25569) * 86400)`, or `24107` for the 1904 system), so the report's item *is*
the bucket:

```sql
GROUP BY GROUPING SETS ((year(to_timestamp((t0."Day" - 25569) * 86400))), ())
```

and the caption is still built here, from the bucket number, with the same rules a scan uses
(`2024`, `Qtr1`, `Mar`, `5-Jun`) — `intern_group` makes the item a scan's `classify` would have
made (`class: 0`, the bucket number as its sort key). A numeric-range grouping (a banded axis) is
not translated and keeps the scan.

**Proven:** two more differential cases (17 in all): a measure over a year / quarter / month axis
(every caption compared cell by cell), and a measure with a month-grouped **column** axis against a
dimension's rows — date grouping and a join in one query.

**Cost** (the probe, release, 5M rows): the same ratio measure over a month axis is **58.7 ms**
against 29.0 ms over two plain row levels — the date conversion is ~30 ms of the report at five
million rows.

### 67. An ordinary "load this file into the model" takes the file route (11-25)

**What landed:** `data.query.add` / `data.query.refresh` with `load: model` now puts a file-backed
table in the model when the source is **one file** and its steps are only `PromoteHeaders` — the header
promotion the engine's own reader does. The prompt is the one a person uses ("Import from file →
load to model"), so the big-table route is now what that flow does, not a command an agent has to know.

**How:** `run_query` asks `file_model(source, steps)` first; when it answers, the table is
`load_model_file`'s (the same function `data.model.loadFile` runs: register the path, `DESCRIBE` the
columns, keep the definition), and the query's `columns` learn the names the file gave. A file the
engine refuses — an encoding, a delimiter auto-detect gets wrong — is logged and the pipeline runs in
memory exactly as before, so nothing that used to load stops loading; it just stops being bigger
than the cache.

**Proven:** `crates/engine/tests/model_file.rs` (7 cases now): a 5,000-row CSV added through
`data.query.add {load: "model"}` keeps the path (the document caches no rows) and a refresh reads it
again; a query with a `ChangeType` step stays an in-memory table.

### 68. A value filter asks the engine (11-25)

**What landed:** `pivot.filterByValue` ("keep the regions whose total is above ten") is answered by
the engine on a model table: one query groups the field by its own values — or by the bucket a
grouped field stands for — and the measure's aggregate is compared with the filter's test. The scan
used to walk every record for it, which a file-backed table cannot do at all, so this is the first
way to filter one by value.

The list of kept labels is the scan's: a differential test computes both and compares them
(`crates/engine/tests/pivot_sql.rs`, 18 cases now). What SQL cannot answer — a numeric-range axis, a
product or a standard deviation — falls back to the scan as before.

### 69. One Solver test is red, and it was red before the data-model work (S, found 11-25)

`crates/engine/tests/solver_sensitivity.rs::reset_all_removes_the_sheets_model_and_the_saved_cells_bring_it_back`
fails at **`da5a683`** (checked out and run there, so it is not a regression of rounds 81–89): the
second `data.solverSaveModel` is refused for a **spill** ("The saved model would cover H11, which
holds the values of another formula") where the test expects the column's own "already holds data"
message. Which of the two refusals comes first is a Solver behaviour question — Excel asks about the
column first — so either the check's order or the test's expectation is wrong. It is the one test
that would stop `cargo test --workspace` today.

### 70. The pane can put a file into the model (U, 11-25)

Queries & Connections gained **"Add from file to model…"**: it opens the file picker, runs
`data.model.loadFile` with the chosen path, and says how many rows arrived. A model table's line now
ends in **· file** when its rows are a file's (and its row count is the engine's), which is what
tells a person the table is not the document's.

The same round fixed the pane's own layout rule: the **Data model** section was drawn only when a
query sat beside it, so a file added with no query would have been invisible. It now draws whenever
the model holds something.

Proven by `crates/ui-egui/tests/queries_pane_language.rs`: a picker that answers with a CSV, a click
on the button, the model holding the table (`file: true`, 2 rows, 2 columns) and the pane drawing
the model section — plus the button's labels in both languages (`sql-engine` is a passthrough
feature of the UI crate for that one test; the gate builds without it). The web build has no engine,
so the button answers with the "needs the desktop app" message instead of pretending.

### 71. Show Details asks the engine (11-25)

**What landed:** `pivot.showDetails` on a model table is answered by one `SELECT … WHERE … LIMIT`:
the cell's items become the predicate, and an item's caption — which may cover several dates, or a
formatted number — is turned back into the values it stands for by labelling the field's distinct
values here (the same trick a filter's `WHERE` uses). The count is read first, so "N records left
out" is the number rather than a guess. It is the last report action that refused a file-backed
table.

`Ok(None)` on a sheet source, an engine-less build, or a value SQL cannot spell: the scan answers as
it always did.

**Proven:** `crates/engine/tests/model_file.rs` (9 cases now): one region of a 500-row file drills to
its 8 records and the grand total to all 500.

### 72. A numeric range is a bucket SQL groups by (11-25)

**What landed:** the last computation on the report path that still needed the scan — a **numeric
range axis** ("by 5 from 5 to 20", with the `<5` and `>20` tails). The axis expression is the
bucket's index:

```sql
CASE WHEN t0."Amount" < 5 THEN -1
     WHEN t0."Amount" > 20 OR t0."Amount" >= 5 + 4 * 5 THEN -2
     ELSE floor((t0."Amount" - 5) / 5) END
```

and the caption is still the model's own (`PivotNumberGroup::captions`), so a scan's report and a
query's cannot print different ranges. `-1`/`-2` are the two tails, which the model names `<start`
and `>next boundary` and sorts first and last.

**Where that leaves the report path:** for a model source **every** operation is the engine's now —
refreshing (aggregates, sub-totals, joins, measures at every level, date axes, numeric ranges),
value filters, and Show Details. What still reads records is a value field the SQL cannot compose
(product, standard deviation, variance) and a text column summed; both say so when a table is past
the scan's bound.

**Proven:** `crates/engine/tests/pivot_sql.rs` (19 cases): a measure over a numeric range compares
its captions and numbers with the scan's.

### 73. A real 112 MB workbook was refused by the part-size guard (S, 11-26)

**What happened:** the owner double-clicked a workbook from their archives and Cella would not open
it, while WPS does:

> We can't open '…个人档案修改结果 2026-11-25 11_38.xlsx': the file is too large:
> xl/sharedStrings.xml expands to 648953695 bytes (a guard against damaged or malicious files).
> For a file you trust, raise the limit with the CELLA_MAX_PART_MB and CELLA_MAX_TOTAL_MB
> environment variables.

That is the guard from #1 working as written — and it was wrong to refuse it. **Measured:** 112,798,714
bytes on disk, **1,384,149,295 bytes unpacked (1.32 GB)**, largest part `xl/sharedStrings.xml`
648,953,695 bytes (619 MB), two sheets of 76 columns × ~100,000 rows (**7.58 M cells**, 23.6 M cells
in the workbook). The 512 MB part / 1 GB total caps were both below it. This was the second real
workbook refused that way: the 11-03 corpus run's only 黄埔 failure is the same shape (72.8 MB file,
`xl/worksheets/sheet2.xml` 608 MB, 622 MB in all), recorded there as "the defence working, not a data
defect" — which is exactly the judgement the owner's report overturns.

**What the caps cost:** the guard bounds memory, and a read's memory tracks the unpacked size —
measured 1.3 GB peak for 1.38 GB of parts (CLI `info`, 2.5 s; the app opened it to a live window in
3.2 s, `--control` answering, sheet 保存前的数据 rendering 7.58 M cells). 512 MB was picked when the
largest workbook in view unpacked to 607 MB, not from what real archives do.

**What changed:** `MAX_PART` 512 MB → **2 GiB**, `MAX_TOTAL` 1 GB → **4 GiB** (still overridable,
still clamped at 16 GB, still refusing the zip bomb before a byte of it is decompressed). The
messages were rewritten for the person holding the file: they name the part and its size in whole
megabytes, say both limits and what they are for, say **what opens the data** (fewer rows per part,
CSV), and only then offer `CELLA_MAX_PART_MB`/`CELLA_MAX_TOTAL_MB` — the file-path route used to hand
back a run of padding spaces in the middle of that sentence and had no advice at all. Rounding is
honest both ways now: a part over the limit never reads as the limit ("more than 2048 MB").

**The owner's answer, and what it changed:** "300m还是很常见". A number in the source is a guess at
what other people keep, and this one was already one round out of date. So the cap stopped being the
answer altogether: **the file a person picked gets the room its own directory declares** — the zip
central directory says what every part unpacks to, a tenth of slack on top, and an 8 GiB ceiling so a
package claiming a bomb's 10 GB is still refused rather than believed. `file.open {force}` carries it;
the interface passes it for every open a person started (File ▸ Open, a double-click, a drop, a file
named on the command line), while an open no interface asked for — an agent, a script, a query source —
keeps the defaults. A workbook that was opened with room says so in the info bar, with the sizes, so a
slow open is explained rather than mysterious:

> This workbook's parts unpack to 1319 MB (its largest part xl/sharedStrings.xml to 618 MB), more than
> Cella's usual 100 MB in one part and 200 MB in all. It was opened with room for that, which takes
> about as much memory as it unpacks to.
>
> (That line came from the check that proves the path: the 112.8 MB workbook opened in the app with the
> limits deliberately set to 100/200 MB. With the shipped defaults it opens without any line at all,
> and so does the 219.4 MB one, whose 1,895 MB fits them.)

**Measured on the owner's two biggest archives** (release build, this machine):

| workbook | on disk | unpacked | ratio | CLI | app | peak RSS |
| --- | --- | --- | --- | --- | --- | --- |
| 白云京溪-个人档案修改结果 (5 sheets, 23.6 M cells) | 112.8 MB | 1,320 MB | 11.7× | 2.03 s | 3.2 s to a live window | 1.07 GB (live read) |
| 荔湾华林-市平台就诊记录 (5 sheets, 62.7 M cells) | 219.4 MB | 1,895 MB | 8.6× | 5.76 s | 17.3 s first, 6.5–7.0 s after | 258–311 MB (paged read) |

The second row is the paging paying off: the reader spills completed bands beside the document, so a
219 MB workbook costs a quarter of a gigabyte rather than two — memory follows the page file, not the
workbook. (The threshold is 200 MiB of file, so the 112.8 MB one is read live and the 219.4 MB one is
not.) Each of its four big sheets is 12 columns × ~1.046 M rows — a table at Excel's own row limit,
four of them in one file — and the first open pays for writing the spill: 17.3 s then, 6.5–7.0 s on
later opens.

**Proven:** `crates/xlsx` `a_part_declaring_a_large_size_reads_when_the_cap_allows_it` (a part that
declares 700 MB, over the old cap, is read rather than refused), `zip_bomb_is_rejected` (still
refused), and `crates/engine` `a_part_over_the_size_limit_says_how_to_get_the_data_in` +
`a_file_whose_part_is_over_the_limit_says_the_same` (the double-click route: part, size, both
limits, remedies, no byte counts) + `the_limits_admit_the_archival_report_that_was_refused` +
`force_opens_a_workbook_whose_parts_are_past_the_limits` (the same bytes refused unasked, opened with
`force`, and the info-bar line naming the part) + `the_room_grows_with_the_file_and_stops_at_the_ceiling`
(it never goes *below* the defaults, so a file that fits gets no line) +
`the_open_command_passes_force_through` (the command's parameter reaches both routes).
**Not run:** the 3,199-workbook corpus again; the three workbooks above were opened by hand, and the
force path was proved on a real 219 MB file by setting the limits to 100/200 MB, which the app ignored
as it should.

## After beta

### 11. Localization: two languages, deliberately (L)
- **Rescoped 11-14 to English + Simplified Chinese.** The 18 catalogs that had accumulated
  (12,290 lines, verified by nobody) are gone; the enum is `Language::En | Zh`, an old `ui.json`
  saying `"ja"` loads as English with every other preference intact. Chinese covers **1,739 of
  ~2,200 strings** and **all 362 engine command labels**; every `tr()` key has an entry (437 of
  them) and the only accepted English literals in the UI are the `Cella` brand. Formula syntax,
  separators and number/date formats stay **en-US by decision** — Excel's own Simplified Chinese
  build does the same, so #45 as filed is closed by scope. Two CI guards hold it: a strict test over
  the engine's command specs, and `cargo xtask i18n` (unwrapped literals **and** wrapped keys with
  no entry).
- **Finished 11-14 (four surface waves).** A rendered-app audit — 60+ dialogs opened and their
  whole accessibility tree read, every Backstage page, the command palette, the panes, a tooltip
  sweep — found the last residue and fixed it: Format Cells showed Page Setup's paper-size word for a
  type size, its specimen word for the sample, and the outline-level word for Outline (the catalog
  is keyed by English text, so a word used in two contexts needs a `context|label` key: five call
  sites now use `Format Cells|…`); 36 command-menu group names were half English in the palette; the
  Backstage template tiles drew English for ever; the formula bar's expand button said *expand*
  while expanded; the filter's `(Blanks)` entry, the status-bar count, the ruler hints and the page
  presets were built at run time. Catalog: **1,813 entries**, all 451 `tr()` keys and all 362
  command labels covered, and the rendered Chinese build was walked page by page.
- **Landed 11-14 (function help):** the Insert Function descriptions and the argument names the
  formula tooltip prints are Chinese for **every** function in the catalog, not the most-used ones
  (159 entries before). A test walks the catalog and fails on a description or argument with no
  Chinese entry, so the next function cannot land without its help — and a one-letter argument
  (x, A, n) is exempt because it is a symbol, not a word.
- **What is left:** text that is English on purpose (brand, file formats, font names, format codes,
  function names, protocol/param errors). The
  old 20–35 h estimate was the gap to a heuristic ~2,200-strings figure; the measured leaf work
  turned out to be ~70 entries.
  [localization-parity.md](localization-parity.md)

### 12. Get & Transform, external data, data model (F)
- No Power Query *editor*, From Web/database sources, relationships or data model
  (`data.fromWeb`, `data.relationships` missing). **Estimate:** 60–100 h, of which the pipeline
  landed on 11-14 (below); what is left is the editor pane, the folder and model loads, the
  relationships and measures of N3.3, and a dependency decision for Web/SQLite.
- **Landed 11-14 (N3.1, the query pipeline).** A query is document data (`Query`,
  `QuerySource` of seven kinds, thirteen `QueryStep`s, `QueryLoad`) and the pipeline is a crate
  (`crates/query`, L2, no I/O): `apply(table, steps)` runs promote/demote headers, change type,
  keep/remove columns, rename, filter, sort, remove duplicates, replace values, fill down, unpivot
  and group by — each total, each error naming its step index and the column it wanted, and the
  table capped by declared size (4M cells) before anything is allocated. Eleven commands
  (`data.query.add/refresh/addStep/setStep/removeStep/setSource/rename/delete/load/list/steps`)
  run a query and **load it onto a sheet as a real table** (the style carried across a refresh, the
  table replaced when the shape changes so no renamed header is left behind), so the result prints,
  charts and feeds a pivot table and the next refresh rewrites it in place; a step that does not
  fit is rolled back rather than kept. Sources: CSV/TSV, JSON, XLSX, **Parquet (read; the reader is
  new)**, a table or range of the workbook. `range.setValues` gained `raw` so a loaded column keeps
  the type the pipeline gave it instead of being re-typed as a date on the way out. Persistence is
  our own part, `xl/cella/queries.json`, read leniently and dropped when the model has none —
  Excel ignores the part (and **drops it if it saves the file**: its own queries live in a
  proprietary `customXml` DataMashup blob, which clean-room rules put out of reach). Tests: 28 in
  the pipeline crate, 13 end-to-end through the engine (including save → reopen → refresh), 10 for
  the part, 20 for the Parquet reader.
- **Landed 11-14 (N3.2, Queries & Connections).** The pane lists every query with its source,
  the sheet it loads onto, its steps and a ten-row preview; Refresh, Refresh All (which now
  refreshes the queries before the reports), Delete and per-step Remove are buttons on it, and
  the pane's own title, buttons and all thirteen step labels read Chinese. The new
  `data.query.preview` runs the pipeline **one step at a time** and returns the rows as of the
  last step that worked plus the index, label and message of the one that failed, so a broken
  step is marked where it stops instead of failing the preview; a trial `step`/`at` previews a
  step before it is added.
- **Landed 11-14 (N3.3, the data model).** A query can load into the **data model** instead of a
  sheet: its rows are kept in the workbook (capped at 4M cells across tables, cached so a reopened
  file shows numbers before a refresh, dropped when the query it came from is deleted) and never
  touch the grid. `data.relationship.add/remove/list` link two tables many→one, refusing a repeat,
  a column related to itself and any loop the filter context could not resolve;
  `data.measure.add/set/remove/list/evaluate` define measures. The language is a new L2 crate,
  `crates/dax`: `SUM`/`AVERAGE`/`COUNT`/`COUNTA`/`MIN`/`MAX`/`DISTINCTCOUNT` over `Table[Column]`,
  `CALCULATE` with equality filters, `DIVIDE`, `BLANK`, arithmetic, and measures that reference other
  measures (cycle- and depth-bounded). A filter context **propagates through relationships**, which
  is what makes two files mean something together: a filter on `Region[Name]` restricts `Sales`
  through `Sales[RegionId] → Region[Id]`. On the grid, `=MEASURE("Total", "Region[Name]", "West")`
  (a Cella function, not Excel's) evaluates a measure in a cell and recalculates when the model
  changes, because the function is treated as volatile — the rows it reads live outside the cell
  graph. **Adding or editing a measure validates its formula first** and rolls back a change that
  would never compute, the same rule a query step follows. Where the subset deliberately differs
  from DAX, the crate says so and the tests pin it: a blank operand keeps a result blank instead of
  reading as 0, text that looks numeric is `#VALUE!` in that cell rather than being coerced, an
  error cell inside an aggregate is skipped (counted by `COUNTA`, not by `COUNT`) so one `#N/A`
  cannot wipe a million-row total, and `DISTINCTCOUNT` does not count blanks. Proven end to end: two
  CSV queries → model tables → relationship → measure → a cell, then a source edit, a refresh and a
  recalc (22 → 30), then save → reopen with the same answer.
- **Landed 11-14 (N3.4, the folder connector and the pane's data model).** A `folder` source reads
  every file in a directory whose name matches a wildcard (`*.csv`, `?` one character, matched
  case-insensitively, no recursion, at most 10,000 files), stacks them with a leading `Source.Name`
  column naming the file each row came from, and refuses a file whose column count differs from the
  first one's by name. The Queries & Connections pane gained **New query from file…** (pick a file,
  and the extension decides the source: csv/text/json/xlsx/parquet — so the flow the pane offers is
  the query flow rather than the Data tab's *open this file as a workbook*), and a **Data model**
  section that lists the model's tables with their row × column counts and every measure as
  `name = formula`, which is what makes a `MEASURE()` cell in the grid explicable. **Reference
  queries** close the pipeline: a `query` source reads another query's result (with its columns
  already named, so no PromoteHeaders), depth-bounded and cycle-detecting, so one query can clean a
  folder and the next report from the clean table. A source that does not read back — a file that
  moved, a query that would read itself — is **rolled back**, the same rule a step and a measure
  follow: a recipe that cannot run is not kept.
- **Landed 11-14 (N3.4, the PivotTable over the model).** A report whose `source` is a **model
  table** is computed, not carried: rows come from the model, the field list is `Table[Column]`
  (the table's own columns plus one hop out along a relationship where it is the many side), and a
  value field may be a **measure**, evaluated by `cella-dax` per cell with the row and column
  members as the filter context — so a measure answers per item, per subtotal and for the grand
  total in every layout, and a measure that cannot compute is one `#VALUE!` cell rather than a
  failed refresh. A measure is refused as a row, column or filter field (it carries its own
  aggregate) and refused altogether by a report over a worksheet range, with a message saying to
  build it on the model table. The definition is kept in a part of our own,
  `xl/cella/pivots.json` (a model report's cache would have to point at the binary
  `xl/model/*`, which this implementation keeps but does not author), so it survives save → reopen
  → refresh; Excel sees the cells it wrote and no PivotTable, the same limitation the queries part
  has. The exit criterion is now measured: two CSV queries → model tables → relationship → measure
  → a PivotTable → save → reopen → 22, then a source change, a refresh and **30**.
  **What a model report does not do yet, each refused with a reason rather than guessed:** a field
  reaches **one relationship hop** (a date or number grouping on a dimension column, or a second
  hop, would need a range or a path in the filter context that `crates/dax` does not have), a
  **grouped** axis field does not narrow a measure (same reason), the **Filters area** does not
  narrow measure cells (a filter holds display labels and the context holds typed equalities), a
  **value filter** on a measure is refused, and a **slicer** synced to a model report is skipped
  (`cmd/slicer.rs` still reads sheet sources only). The field list does work: the PivotTable Fields
  pane lists the model's `Table[Column]` fields (with samples) and its measures, and ticking one
  adds it to the report.
- **Landed 11-14 (the two connectors that need the outside world, after the owner's dependency
  decision):** a query's source may now be a **SQLite file** (`{"kind":"sqlite","path":…,"sql":…}`,
  opened `SQLITE_OPEN_READ_ONLY`, one statement, its output columns are the table's, BLOB columns
  refused by name, 64 KiB statement cap) or a **URL** (`{"kind":"http","url":…,"format":"csv"|"json"}`,
  30 s timeout, 64 MiB reply cap, only http/https so a `file:` or `data:` URL cannot dress a path up as
  a link, and an HTML reply — the login page an endpoint answers with — refused as "a web page, not
  data"). Both read *only on refresh*, never on open. The Data tab gained **From Database** and a real
  **From Web** (`data.fromDatabase`, `data.fromWeb`; the second used to be a catalog entry with
  nothing behind it), each a small dialog that creates a **query**, not a one-shot import, so the
  result has steps, a refresh and a name derived from what it read (`sales.csv` → "sales", then
  "sales (2)").
- **What it cost, measured:** the desktop binary went from **46.7 MB to 54.6 MB (+7.9 MB)** with
  bundled SQLite and rustls — inside the 10–30 MB the estimate guessed, and the reason the feature is
  a feature rather than a default of every crate.
- **How the dependency is paid for:** both live behind the engine's `connectors` feature — bundled
  SQLite (a C compiler on every platform we ship) and ureq with rustls (pure-Rust TLS, no OpenSSL).
  The feature is **off by default**, because the browser build has no filesystem and no sockets, and
  the desktop app, the CLI and MCP turn it on. A build without it keeps the query in the document,
  round-trips it, and reports "this build cannot read SQLite or web sources" when it runs — the same
  bargain as a preserved part. `cargo xtask` gained the one step that runs those tests with the
  feature named, or they would never run at all.
- **Landed 11-14 (the seams a model pivot had):** a dimension **two or more hops** away is now a
  field (facts → product → category; breadth first, four hops at most, loops and self-relationships
  skipped), so a snowflake schema reports like a star; a **grouped** row or column field (years,
  quarters, months, day-of-month, number buckets) now narrows the measure by the members its bucket
  covers instead of being left out of the context; and the **Filters area** narrows measure cells,
  which it did not before — a report filtered to West used to show the whole table's numbers, which
  is the worst kind of wrong because the report looked filtered. Both use a new "is one of these"
  filter in `cella-dax` (`Context::filter_any_of`); a bucket wider than 4,096 distinct members
  narrows nothing rather than costing seconds per cell.
- **What is still not done:** `data.getData`/`data.fromTextCsv` remain one-shot imports (a table and
  no query) by design — the pane's button is the query path; adding a step from the pane is
  command-side only, there is no step parameter editor; the model's relationships and measures have
  no UI (commands only; a field list is the natural next round); a **calculated item** does not
  narrow a measure (it is a caption over other items, not a member the model has); a value filter
  set on a measure is refused; and slicers skip model pivots.
- **Landed 11-14 (the file half):** a workbook's `xl/model/*` (the Power Pivot data model) is kept
  and written back byte for byte, related from the workbook with the file's own `powerPivotData`
  type, and it survives deleting every report that used it; PivotTables built on it are carried
  read-only (see §8). What is still missing is the engine: no records, no measures, no refresh, no
  recomputation. The seam is `PivotSourceKind::DataModel` + `read_only_reason` — swapping a model
  reader in there is what turns a carried report into a computed one.

### 13. Analysis tools: Solver, Analysis ToolPak, Forecast Sheet (F) — **the Solver half landed 11-14**
- Solver takes Excel's **AllDifferent** relation (dif in a constraint string, Excel's own code 6):
  a group of variable cells that must all hold different values, which is how a scheduling or roster
  model is written — and the reason it is a relation and not a pile of pairwise not-equal
  constraints is that pairwise inequalities make a linear model non-linear. The gap between two
  values is 1e-3, well above the 1e-6 feasibility tolerance, so equality is a real violation and not
  a rounding artefact.
- The **Solver Options** the dialog was missing (method, population size, mutation rate, random
  seed, convergence, generations without improvement, require-bounds, keep-values, save-model,
  integer optimality) are real Solver parameters now, with a fixed seed so a run repeats.
- **What was already there, checked before writing any code:** integer/binary constraints, every
  stop reason (infeasible, iteration limit, unbounded, not linear), and the **whole** Forecast Sheet
  option set — seasonality (auto or 2–8760), confidence interval, Forecast Start (the hindcast of
  #785), line or column chart, statistics, fill-missing, aggregate, end and periods — offered by the
  dialog. The estimate in this section was wrong about them; nothing was added for show.
- **Limits accepted with the AllDifferent work:** under Simplex LP a group is solved exactly only
  for binary variables (Σx ≤ 1 and Σx ≥ n−1); a non-binary group reports *not linear* with a
  dif-specific message. GRG with integer variables reports *infeasible* rather than backtracking
  when the snapped answer breaks a group. Still missing: the **Sensitivity** and **Population**
  reports.
- `data.solver`, `data.analysisToolPak` missing. Forecast Sheet (`data.forecastSheet`) and the
  FORECAST.ETS family are in, including Forecast Start (hindcast, #785). **Estimate:** 15–25 h.
- Solver (`data.solver`, #327) has Simplex LP with integer/binary branch and bound, a local
  nonlinear method (augmented Lagrangian, not GRG), the Evolutionary method, and Answer and
  Limits reports. Report refusal leaves the solution, saved model and history unchanged; a
  successful solve and its reports share one Undo step (#1050). AllDifferent constraints,
  the Solver Options dialog, the Sensitivity and Population reports and Load/Save Model remain.
  The Analysis ToolPak
  (`data.analysisToolPak`) has **all 19 tools** since 11-02: z-Test and the two-factor ANOVAs, plus
  **Sampling** (periodic every k-th value, or a seeded random sample), **Random Number Generation**
  (uniform, normal, Bernoulli, binomial, Poisson, pattern and discrete, one seeded SplitMix64 so a
  report repeats) and **Fourier Analysis** (a power-of-two list of real numbers or `a+bi` text,
  radix-2 FFT, complex text out). The z-Test/ANOVA p and critical values come from the worksheet
  functions, and the ANOVA layouts follow Excel's SUMMARY + Source of Variation blocks. Forecast
  Sheet (`data.forecastSheet`) and the FORECAST.ETS family are in; the sheet has no Forecast Start
  (hindcast) option yet. **Estimate:** 4–10 h (Solver refinements and the Forecast Sheet options).

### 14. Page Layout view and print polish (U, F) — **the page options landed 11-14**
- Page Setup ▸ Sheet gained what the file's pageSetup element carries and the dialog was missing:
  **page order** (down then over — Excel's default — or over then down, with pages numbered in
  printing order), **black and white** (colours printed as greys) and **draft quality** (no charts,
  pictures or shapes). All three survive save/reopen with a round-trip test, and the print path in
  crates/engine/src/paper.rs honours them rather than only storing them.
- **Checked before writing code, because the docs were stale:** Different First Page / Odd & Even
  pages, print ranges, headings and gridlines on every page, and centre-on-page already existed and
  are honoured per page. The three options above were the genuine gap.
- **Limits accepted:** black-and-white greys fills, borders, data bars, fonts, chart primitives,
  sparklines and shape fill/line, but leaves **embedded pictures** in colour; the Page Layout *view*
  still paints colours and draft content (only print and PDF honour both); driver-specific tray and
  paper options still need hardware.
- **Still missing:** header/footer pictures, and dragging a print area on the Page Layout rulers
  (margins are draggable already).
- Page Layout view landed (#329): paper view with rulers and in-place header/footer editing.
  Margins are dragged on the rulers (#792). Remaining: pictures in headers/footers, Different First
  Page / Odd & Even pages, print ranges and driver tray/options UI. Desktop submits directly through
  craft-print with printer choice, copies, duplex and grayscale; browser printing stays available.
  **Estimate:** 5–10 h (the two page-parity options are the beta-visible part).

### 15. Remaining functions and an oracle suite (F)
- **9 missing by name** (514 of Microsoft's 523 present, 505 working): DETECTLANGUAGE, TRANSLATE,
  IMAGE, RTD, STOCKHISTORY, CALL, REGISTER.ID, COPILOT and PY; nine stubs (7 CUBE, WEBSERVICE,
  FILTERXML). Measured 11-02 with `cella-cli functions --json` (515 specs, one of them
  USDOLLAR, which is not on Microsoft's list). No Excel-computed oracle corpus; #137, #138.
  **Estimate:** 13–22 h. [function-parity.md](function-parity.md)

### 16. Collaboration and review (F)
- **Decided 11-14: no co-authoring, no cloud storage of our own.** Asked and answered: the owner does
  not need collaboration (real-time editing, shared links, a hosted service). The review half that
  a single person uses stays as it is — comments, threads, the review pane, protection, Show Changes
  is the one piece still absent. What a workbook shared through someone else's drive does is
  documented in file-format-parity.md; nothing here builds a server.

### 17. Insert objects (F)
- Ink strokes now survive XLSX save/reopen as editable polylines. Inserted icons still lose their
  identity/artwork on save (#551); native Office InkML import is not implemented.
- **Landed 11-14: equations.** A drawing shape whose `txBody` carries `a14:m` is read as an
  equation object (`model/src/equation.rs`), its OMML kept **verbatim** through a save, and
  `insert.equation` / `equation.setText` create and retype one (a retyped equation is written as
  plain `m:r/m:t` runs — the file's real math survives only while the text is untouched, which the
  field docs say). Cella paints the **linear** text: full 2-D math layout is out of scope, and
  the command is **command/control-channel only** until there is an equation editor to type into.
  Equations do not yet follow row/column inserts, are not in the z-order/Align/Arrange commands,
  and are not in the PDF export; the accessibility check lists one (it is named by its source).
- **WordArt landed:** `insert.wordArt` / `wordArt.set` make decorative text as a transparent
  shape (`ShapeKind::WordArt`) whose text body carries the no-warp marker
  (`a:prstTxWarp prst="textNoShape"`), so it reads back as WordArt and is never flattened into a
  text box; the Insert › Text gallery paints its ten presets in code, and the text is edited
  through the text-box path. The PDF export draws its text the way the grid paints it: the drop
  shadow as one offset copy and the outline as eight, under the filled text.
- SmartArt, signature line, objects, 3-D models, screenshot; 8 shape kinds vs ~170; pictures in
  cells (#61, PR #105). **Estimate:** 25–40 h (equations and WordArt are no longer part of it).

### 18. Accessibility (U)
- **Landed 11-02 (alt text + checker):** charts and shapes carry alternative text now — the model
  (`Chart::alt`, `Shape::alt`), the file (`xdr:cNvPr/@descr`, escaped, read back on charts, kept
  charts, pictures and shapes), the command `object.setAltText` and a check that notices when it
  is missing. Review ▸ Check Accessibility reports **every object without alternative text** —
  charts, pictures, shapes, WordArt, equations and pictures in cells — with the reason it is listed
  and the name a screen reader would announce (`accessibleName`: an object that carries text is
  named by it — a shape's label, a WordArt's words, an equation's source — so the list marks those
  "Text used as alternative text" and the nameless ones "Missing alternative text"), plus tables
  without a header row, merged cells and sheets still on their default name, each with its sheet
  and object/range; the dialog **goes to** each one and takes the alt text inline.
- **Landed 11-02 (the grid is walkable):** while a screen reader is attached the visible cells are
  an **AccessKit grid** — one `GridCell` node per visible cell, with the displayed value as its
  value, its address (and formula) as its description, its row and column indexes, and the active
  cell marked selected and named as the grid's active descendant; the grid itself is labelled with
  the sheet, says which cell is active and how many are selected, and carries bounds of its own,
  with a `ColumnHeader`/`RowHeader` node per visible header (its letters or number and its index). Frozen rows and columns are
  included and the tree follows scrolling; nothing is built when no reader is attached (one branch
  a frame) and the walk is capped at 2,000 cells a frame, so a reader scrolls for the rest as a
  sighted user does.
- **Landed 11-02 (high contrast):** View ▸ Show ▸ **High Contrast** (`view.highContrast`, keytip
  WVI — Cella's own, Excel takes it from the OS) turns the chrome into a black, white and
  yellow palette: black window, ribbon, tabs and status bar with white text and yellow separators,
  a yellow selection border and header highlight, cyan accents, and a white sheet page that keeps
  its own black text (as Excel's high-contrast themes do — the document is not the chrome). egui's
  own widgets go dark with it so menus and dialogs stay readable. The preference is kept across
  restarts (`ui.json`).
- **Landed 11-03 (the status bar):** an Accessibility entry shows the issue count from
  `review.checkAccessibility` (cached per document revision, since the check walks every sheet) and
  opens the same dialog as the Review tab; it is focusable and keyboard reachable. **Estimate:**
  0–1 h, none of it blocking beta. [ui-parity.md](ui-parity.md#accessibility)

### 19. XLSB/ODS beyond values; minor formats (FF)
- XLSB and ODS import values only, no write; no `.prn`, `.dif`, `.slk`, XML Spreadsheet 2003; CSV
  writes raw instead of displayed values; templates saved with the workbook content type.
  **Estimate:** 15–23 h.

### 20. Multiple windows; pen pressure (H)
- **Landed 11-14 (multiple windows).** View ▸ New Window opens a real second OS window on the
  **same** document — verified on the desktop with the control channel: one window before, two after,
  titled "Book1" and "Book1:2" the way Excel numbers them, one document behind both. The engine owns
  the window list (`WindowId`, `DocWindow`, never empty while a document is open; `Session::windows`
  is the order the Window menu shows) and closes the document with its last window; the egui shell
  realises each window with `show_viewport_immediate`, paints the whole chrome and grid per window,
  and keeps the main window last so the control channel's screenshot and pointer mapping stay
  defined. Scroll position is per window; the active cell and sheet stay the document's. New
  commands: `view.closeWindow` and `window.activate`; `view.hideWindow`/`unhideWindow`/
  `switchWindows` name windows instead of documents (the old `hidden_windows` conflated the two and
  is gone). The browser build has one window by design: `view.newWindow` is disabled with that
  reason and refuses in `run`, documented in its params doc.
- **Still missing:** pressure and tilt in ink (needs a tablet to verify); per-window active cell
  (both windows show the same cursor on purpose, so the document has one selection); window geometry
  is not saved; `view.arrangeAll` still does nothing.
  [hardware-parity.md](hardware-parity.md)

### 21. Automation and add-ins (F)
- VBA out of scope (dropped on open, #6); no Office Scripts compatibility; no plugin or custom
  function API. **Estimate:** 25–40 h plus an owner decision.
- **Landed 11-14 (N4):** scripts are **editable** (a step's command and parameters are a field in
  the Macros dialog, with add and remove), and the engine exposes the steps
  (`automate.scriptSteps`, `automate.editStep`, `automate.appendStep`, `automate.removeStep`,
  `automate.renameScript`), so a recorded script replays as the same workbook — proved by a test
  that compares the two runs cell by cell.
- **Landed 11-14 (N4, the declarative extension tier):** an **extension** is a named command
  sequence a workbook carries (`ext.add/remove/list/run`), so Insert ▸ Add-ins is a real command
  instead of a catalog entry with nothing behind it. It runs through the same permission checks a
  script step passes (no file access, no links, one level only — an extension may not call another
  extension or a script), and automation that arrives with a file starts **disabled** until the
  workbook is trusted, exactly as scripts do.
- **Also landed 11-14 (N4):** `insert.table` refuses a table that only **partly** overlaps the
  sheet's AutoFilter. It used to drop the filter and turn the rows it hid into hand-hidden ones, so a
  reload silently changed what the filter meant; a table on or inside the filtered range is still
  the ordinary case. Found by the command-sequence fuzz after N4 changed its seed sequences.
- **Decided 11-14: there will be no in-app plugin host.** An installable extension that adds
  *code* to the app existed to give people who cannot program a button; that job is now done by an
  agent, which costs nothing to ship, reads the user's files and is not limited to what we thought
  to expose. A host would add 10–30 MB per platform, a C toolchain on every one we ship, a browser
  build that cannot have the feature, a key-and-permission model, and an interface we could never
  break — for a distribution convenience nobody in this product's life has asked for. So: the
  **outside-in surface is the extension model** — a program that speaks the control channel (with the
  same commands the menus run), covered by the Agent Control dialog and docs/mcp.md. What replaced
  "install my button" is a **script**: a procedure captured once, edited in the Macros dialog,
  replayable by a person or an agent.
- **Repositioned 11-14: recording is the capture format, not a macro language.** With an agent
  usually on the other end, a recorded script's job is to be the *faithful, diffable, replayable
  record of a procedure* the agent can replay, vary and verify — not a way for a non-coder to build
  an app. automate.scriptFromJournal (and a button in the Action Journal) turns what already ran
  into a script, which is the shortest path from "I did it once" to "it is a procedure": do the
  steps (or let an agent do them), save them, edit the ones that need editing, replay.

### 22. AI features (F)
- **Decision 11-14: no in-app assistant, ever — the agent surface is CLI and MCP.** A model inside
  the window would need a network client (a TLS stack, and only on native targets, so the web build
  would silently lose a feature), a key store, and a second way to run a command next to the
  registry; and the thing it buys — "ask in English, get edits" — an external agent already does
  better, because it can read the files, the docs and the user's own subscription. Building it would
  also invite the one thing the roadmap refuses: **AI features that cannot be verified against
  engine state**. So the work is not "ship a chat box" but "make the outside-in surface excellent",
  and that is what landed (11-14): the Agent Control dialog now shows the port the window is
  listening on (or how to start with `--control`), the four ways in with a copy button each, a real
  request and its answer, and the command count; `inspect.whatChanged` answers "what changed" cell
  by cell from the saved/opened snapshot, so an agent's claim can be checked; MCP went to 31 tools
  with paging, batching and the data platform; and every one of the 393 commands is proved reachable
  from the CLI and from MCP by id.
- **Still open (not this decision):** natural-language formulas and Analyze Data would be built-in
  features that need a model *inside* the app; if that ever becomes a goal it is a new decision, not
  a leftover of this one. **Estimate:** 30–50 h and the same fork.
- **Landed 11-14 (N4):** the engine answers the question an assistant has to answer first —
  `inspect.whatChanged {sheet?, limit?}` compares the workbook with the state it was **saved or
  opened** as (the same snapshot the modified marker reads) and returns the cells that differ, old
  and new values, the formulas either side, the sheets added or removed, and whether the answer was
  truncated. It is a diff of the document, not of the undo log, so it survives an agent that cannot
  see the session's history. **Still missing:** a model in the app — an in-app assistant needs a
  provider and a network client, which is the owner decision above.

### 23. Mobile (platform)
- **Decided 11-14: not doing it.** Asked and answered: the owner does not need an iOS/iPadOS/Android
  layout, and it was the largest open item after the four feature waves (30–60 h of touch layout,
  on-screen keyboards and a WebAssembly shell). The web build still runs in a tablet's browser; what
  does not exist is a layout designed for touch. If that changes it is a new decision.

### 24. Fonts and look (U) — **done 11-14 (#169, #170, #54, #173)**
- The window frame is decided in one place (`crates/ui-egui/src/chrome.rs`): Windows keeps its system
  caption and the app's strip stops repeating the workbook name under it (two titles in two rows);
  macOS keeps the caption band with the strip drawing the name; Linux gets the app's own frame, with
  minimize/restore/close in the strip and the edges dragged there, because every Linux desktop paints
  a caption the app cannot match. The title bar's separator line is gone, so the strip and the
  ribbon's tab row read as one surface (a caption and the Quick Access Toolbar). Extra windows get
  the same policy.
- Icons (drawn in code, original): structural strokes and the mark an icon is about are two named
  weights scaled with the box (rect and circle outlines used to be a fixed 1 px while their lines
  scaled, so big ribbon icons had hairline boxes); a glyph's text is centred on its ink, which had
  drawn every letter icon about a fifth of an em high; the alignment trio is symmetric; Zoom has its
  own lens instead of reusing Search; Group/Ungroup's outline badge stays in its box; Comma style
  reads as digits; paper-white shapes keep a neutral edge that stays visible on the dark ribbon; the
  window-button glyphs join the set. Four tests walk every icon (inside its box, centred, legible,
  paper edges visible on both themes) and a theme test measures every chrome ink on every surface.
- Font weights beyond regular/bold (#168), system font list (#4), Calibri metrics (#29), macOS
  chrome (#169, #170), Linux title bar (#54), icon consistency across apps (#173). **Estimate:** 7–12 h.

### 25. `data.subtotal` rebuilds the sheet one total row at a time (P)

**What:** `data.subtotal` inserts each group's total row with `home.insertRows`, so every group pays
a shift of every row below it (`crates/engine/src/cmd/data.rs::subtotal`). On a million rows that is
**Θ(groups × rows)**: 57 ms for four groups at 100k, 680 ms for four at 1M, **8.70 s for a hundred**,
and the command catalogue's own worst row (28.4 s) is about 330 adjacent groups. On *unsorted* data
every row is its own group (Subtotal totals adjacent equal values, as in Excel), so the command does
not finish — the first probe had to be killed.

**Why it matters:** Data ▸ Subtotal is what a report does before a chart, and a hundred groups is an
ordinary report. Eight seconds is the longest a person waits on any one command in this program.

**Fix:** compute every group boundary first and rebuild the body once (the aggregates are ~0.7 s of
`SUBTOTAL` evaluation for a hundred groups), instead of inserting a row per group. The sheet body is
rebuilt, so the undo step and the graph rewrite change shape with it — a design change, not a patch.

**Evidence:** [performance.md](performance.md) §"`data.subtotal`: one row insert per group" — the
scaling table, a probe over sorted data, and where the 28.4 s comes from. **Estimate:** 4–7 h with
the paired sign test.

### 26. `memory_report`'s undo line is not attributable to a command (P, 11-23)

**What:** the command catalogue measured "what does this command add to memory" as
`Session::memory_report().total` after minus before, and it was wrong: in one process the first row
is sane and every row after it grows about **710 MB**, the same for a sort, a font step and a solver
report. The report's parts show the jump is entirely in **`undo`**: after the first write in a
session it charges **~693 MB at 100,000 rows for every command**, whatever that command did. A
snapshot is supposed to be charged "as what it retains beyond the live workbook"; here it is charged
the whole store (or a shared page file) once, whichever command happens to run first.

**Why it matters:** the memory axis is the one that would have caught "10 ms but 167 MB", and the
wrong version named five commands as memory hogs. The catalogue now publishes the **document's own
memory** (`sheets + workbook + calc`, which is attributable and gives a sort **+57.9 MB** at a
million rows), but "what does a command cost the *history*" is still unmeasured, and it is the
number that decides whether a 1M-row edit can be undone on an 8 GB machine.

**Fix:** find why a snapshot's charge jumps by the whole store after the first write — a shared
band or page file counted twice, or the snapshot charging the base's bands. The engine's own test
for `memory_report` should assert that two consecutive edits in one session are charged the same
way. Then the catalogue can publish `undo` per command again. **Estimate:** 2–4 h to diagnose.

**Evidence:** [performance.md](performance.md) §"What each command adds to the document" — the
parts dump that located the jump, and the table the fixed ruler now produces.

### 27. `FORECAST.ETS` refuses a whole-column series (F)

**What:** the function catalogue found it: `FORECAST.ETS(target, values, timeline)` answers `#NUM!` for
every series longer than ~100 points, and for any target inside the timeline rather than at its end;
`FORECAST.ETS.CONFINT`, `.SEASONALITY` and `.STAT` fail with it. A 1,000-point probe at every target
tried (50, 500, 990, 1000) failed except the last, and 105 points failed where 100 passed.
`bench-functions` records the call as an error row with what the engine said.

**Why it matters:** a forecast over a year of daily data is ~365 points — the ordinary case. Excel
accepts it.

**Fix:** find the guard (a `#NUM!` on a length or a target check in `crates/functions/src/ets.rs`),
decide whether it is Excel's behaviour to begin with, and either widen it or document it. **Estimate:**
1–2 h to diagnose.

**Evidence:** [performance-functions.md](performance-functions.md) and
[performance.md](performance.md) §"The function catalogue".

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-25 | minor | §53 (new, measured): the groupers' **own work is 27 ms** at a million rows (`prepare` 2, keys 15) — what costs 0.65–4 s is the evaluator building their arguments: each array argument is copied (24 MB twice a call), a whole-column `MOD` lift is **199 ms** against `BITAND`'s 88, and a text key column costs a second more than a numeric one at the same group count. The next knife is a grouper that streams from the store instead of materialising |
| 2026-11-25 | minor | §52 (new, fixed): a pivot refresh **formatted every number to decide it had seen it** — `make_item` ran `display::format` and built a `String` key per record per axis field. The key is an enum now and the accumulators key by interned path ids: **`pivot.refresh` 889 → 384 ms at 1M (−57 %)**, 42 ms at 100k; budgets tightened to 600/80 ms. It also records that a sheet holds 1,048,576 rows (like Excel) and a `DataModel` source is carried but never recomputed — the product decision behind "several million rows" |
| 2026-11-25 | minor | §51 (new, evaluation): column storage measured and sized — a contiguous Vec<f64> fold costs **2.2 ns/cell** against the store's 10.5 (4.8× ceiling; SUM ~6 ms), but a full column-major store is 70 public functions and 380 call sites with a wrong-number failure mode. The only shape worth a pilot is a per-column sidecar cache inside store.rs; recommendation: do not start it. [performance-columnar.md](performance-columnar.md) |
| 2026-11-25 | minor | §50 (new): inline numbers refuted before they were built — 
ead_probe runs the two chains (payload → number table against the number in the payload slot) and both cost **2.3 ns a cell**: a band's table is a few hundred entries, always in L1, so the load is free. The corrected attribution: of a 10.5 ns column read, 2.3 is the chain, 1.4 the kind run, 1.1 the formula test, and **8–9 is the row walk itself** |
| 2026-11-25 | minor | §49 (fixed): a spill is **one array at its anchor** now, not a BTreeMap entry a cell — =A2:A (1M, spills) **142 → 77 ms**, BITAND 247 → 173, and on the page DROP 749 → **574**, CHOOSECOLS 267 → **150**, TOCOL 139 → 96, VSTACK 189 → 139, SORT 255 → 207, HSTACK 331 → 284, MAP 423 → 335. Sixty reader sites moved to Sheet::spilled/spilled_in; no function that does not spill moved |
| 2026-11-25 | minor | §49 (new): the spill lay-out is **75–78 ms** of a million-cell array, priced phase by phase (=A2:A 142 ms = 10.6 read + 53.8 building the array + 75 laying it out; =BITAND 247 = 166.5 eval + 77.8 spill). So the fix is to keep the evaluator's Arc<Array> as the spill's value instead of a BTreeMap node a cell — 60 non-test reader sites across five crates, inventoried and ordered in plan/spill-store.md |
| 2026-11-25 | minor | §48 (new): the column-read wall is a chain of **three dependent loads a cell** (the row's `cells_at`, the payload, the number). The dense-band stride read works — 11.5 → **8.1 ns a cell** on a one-column sheet, 9.9 on eight — and moves the page by nothing (`SUM` 20 → 21 ms, inside ±2), so it is reverted. The width sweep is the proof it is latency, not bandwidth: 8.1 ns at one column wide against 9.9 at eight. Under it needs the number inline in the payload stripe — a storage format change, not a loop |
| 2026-11-25 | minor | §47 (new): a whole-column sum folds inside the store — `CellStore::fold_column` runs the arithmetic in its own loop, so `SUM` **25 → 20 ms**, `COUNT` **26 → 21**, `MAX` **26 → 20**, `MIN` **26 → 21**. The same fold costs 11.5 ns a cell against the packed walk's 12.4, which is the round's other finding: the callback a cell was ~1 ns of the walk. The 15 ms aim is retracted — a two-column sum cannot go under ~23 ms without a dense-band stride read |
| 2026-11-25 | minor | §46 (new): a column read spent 62 % of its time finding the rows — by ablation the row walk alone cost 9.8 ns a cell of a 15.9 ns read. Walking the band's rows in order instead of looking each key up: **15.9 → 11.9 ns a cell (−25 %)**, and on the page `SUM`/`COUNT`/`MAX`/`MIN` 29–32 → **25–26 ms**, the criteria family 17–18 → **15–16**, the lookups 17 → **15–16**. A million-row column read costs 9.8 ms doing *nothing*, which is what the plan's 10 ms target is up against |
| 2026-11-25 | minor | §44 (extended): the whole single-criterion family answers from the scan — AVERAGEIF **27 → 17 ms**, AVERAGEIFS **28 → 18**, COUNTIFS **24 → 18**, SUMIFS **31 → 18** — all seven spellings at the walk's price, with Excel's 255-unit difference between the two families kept and tested |
| 2026-11-25 | minor | §45 (new): a million-cell dynamic array pays for its **spill**, not its lift. One call measured three ways: `SUM(A2:A)` 16 ms, `SUM(BITAND(A2:A,B2:B))` 87 (the lift ≈71), `BITAND(A2:A,B2:B)` 230 (the spill ≈143). Plan item 10's ≤80 ms target therefore needs the spill store (`BTreeMap<CellRef, Value>`, a node a cell) redesigned, not a tighter lift. Two fixes were built, measured (231 → 223 and 229 → 234 — both inside the noise) and reverted |
| 2026-11-25 | minor | §44 (new): the criteria functions tested a copy of the column — the criterion matcher takes a `CellView` now (a number, a borrowed text, a boolean, an error, a blank) so a host can ask it of a stored cell with no allocation, and `COUNTIF`/`SUMIF` answer from one walk: **COUNTIF 23 → 18 ms, SUMIF 27 → 18 ms**. The differential test (`criteria_scan.rs`) caught two bugs of the new path — a matched error must be the answer, and a target's error must wait for its row's verdict |
| 2026-11-25 | minor | §43 (new): the lookups only answered the short spellings — `XLOOKUP(k,A:A,B:B,1,0,1)` and `XMATCH(k,A:A,0,1)` fell to the general path and built the block, while the three-argument `MATCH`/`VLOOKUP` scanned. **XLOOKUP 74 → 16 ms, XMATCH 53 → 16 ms**, with the rows the round cannot touch unmoved |
| 2026-11-25 | minor | §42 (new): a LAMBDA call cost a microsecond before the body ran — the environment was swapped out and rebuilt every call, the bound argument went through a one-entry `Vec`, and `aggregate_value` looked up `SUM` and `COUNT` for every argument. **BYROW 1.15 s → 984 ms, MAP 532 → 422 ms, SCAN 601 → 524 ms, REDUCE 170 → 133 ms**, and the rows that run no LAMBDA did not move |
| 2026-11-24 | minor | §41 (new): the groupers keyed every row by a string they built. `row_key` spelled a row's values out as text and the key-to-rows map was keyed by it — a million allocations at a million rows. A hashed `KeyIds` (two `Vec`s, a map of first ids and a collision chain) took **PIVOTBY 5.16 s → 989 ms, GROUPBY 2.48 → 785 ms, UNIQUE 1.49 → 310 ms**, with the old string key kept as the reference a test compares the ids against. `BYROW`'s 1.14 s is the evaluator's per-row LAMBDA cost and is left named |
| 2026-11-24 | minor | §40 (new): a formula's block argument was built before the formula could say it needed three cells. The exact lookups (`MATCH`, `VLOOKUP`, `XLOOKUP` with a scalar key) now scan the key column and stop at the hit — **VLOOKUP 78 → 17 ms, MATCH 56 → 17 ms** at a million rows, and a hit on row 1 costs 0.6–0.9 ms; a fold of a values column stops asking `has_owned_cells` first (**SUM 51 → 30 ms, MAX/MIN −40 %, COUNT −38 %**), and one-column reads take a peeled walk. The rest of the catalogue's rows move by 4–15 % with them; two untouched control rows sat still (−1 %, +1 %) |
| 2026-11-24 | minor | §39 (new) and §33/§34/§35/§38 updated: a row walk asked the kind and style run stripes for **every cell from the first run**, so a thousand-column row was quadratic — the cut half of a move cost 1.20 s where the same cells cost 0.30 s on eight-column rows. A run position on the walk cursor took the move **2.04 → 1.11 s**, the sort **1.81 → 0.83 s**, the save **1.32 → 0.86 s** and the copy **852 → 557 ms**. Round 51's "the cut is the store's per-cell price, so packed rows are next" is retracted: moving packed rows is still the guess for the *write* side, and nothing needs it for the move's target |
| 2026-11-16 | minor | §18/§17: the accessibility check lists every object without alternative text — **WordArt and equations included** — with the reason it is there and the name a screen reader reads (the object's own text when it carries one); the PDF export draws a WordArt's text outline (eight offset copies) and drop shadow (one) the way the grid paints them. Two tests, one per half |
| 2026-11-03 | minor | §1: a local real-world corpus (9.6 GB, 3,199 workbooks) was run: 3,084 readable workbooks opened with no reader failure, 635 save/reopen with 0 mismatches (digest now covers formula text, merges, comments, links, CF/DV, tables, names, charts, shapes, pictures), and Excel 16 found Cella's copies identical on 16 `.xlsx` files plus all 37 readable `.xls`. #1 is met for beta; 4 invalid files (openpyxl also refuses), 111 encrypted and 449 cloud placeholders were out of reach, and the pinned corpus remains for CI (0–1 h) |
| 2026-11-03 | minor | §7: `.xls` autoshapes and text boxes are read — Excel 2007+'s DrawingML fragment when present (geometry, fill, line, text, formatting), otherwise the OfficeArt type and anchor — and verified in Excel 16 with five shapes (same text/fill/line/positions; a block arrow becomes the model's arrowheaded line). The reader now tolerates drawings split across records and a trailing shape stored in a text box's Continue records. #7 is met for beta; OLE objects and chart sheets are full scope |
| 2026-11-03 | minor | §7: `.xls` charts are read (the chart substream: kind and variants, series `BRAI` ranges, cached names, title, legend, and the anchor from the `msosptChart` shape) and verified in Excel 16 — a five-kind workbook converts and reopens with the same kinds, titles and anchors. Shapes, OLE objects and chart sheets remain, so #7 falls to 1–3 h. RC4 is re-scoped: Excel refuses a hand-made RC4 `.xlsx` (ECMA-376 Standard requires AES), so encrypted `.xls` output waits for a `.xls` writer |
| 2026-11-03 | minor | Beta-blocker bookkeeping after round 16: §1 loses the Excel half (Excel 16 verifies every fixture), §7's evidence no longer says encryption cannot be written, §8's slicer evidence is Excel-16-verified and its estimate drops to 0–1 h (the corpus is #1's), §9's object context menus landed, §14 and §18 are updated (the status-bar accessibility entry landed, print polish is what remains), and the beta blockers are down to the corpus, `.xls` charts/RC4 and the field machines. No score change |
| 2026-11-02 | minor | §1: the corpus run is one command — `cargo xtask corpus <dir>` (open every workbook, per-file warnings, save/reopen digest of names/cells/values/formulas/formats, openpyxl readback, temp-only writes, caps) and it is green on a fresh oracle directory; the real-world corpus itself is still missing because `storytold/craftrules` is private. Added `docs/excel-checks.md`, the owner's executable checklist for #1/#7/#8/#10 |
| 2026-11-02 | minor | §13: the Analysis ToolPak is **complete (19/19)** — Sampling (periodic/random with a seed), Random Number Generation (uniform, normal, Bernoulli, binomial, Poisson, pattern, discrete; one seeded SplitMix64) and Fourier Analysis (power-of-two real or `a+bi` input, radix-2 FFT, complex text out) joined the z-Test and the two-factor ANOVAs. Remaining in the gap: Solver refinements and the Forecast Sheet options |
| 2026-11-02 | minor | §13: both **two-factor ANOVAs** landed (`anovaTwoFactor` without replication: Rows/Columns/Error and a rows-then-columns SUMMARY; `anovaTwoFactorRep` with replication: Sample/Columns/Interaction/Within/Total and one SUMMARY block per row level, `rowsPerSample` replicates per cell). 16 of 19 tools; Fourier Analysis, Random Number Generation and Sampling remain |
| 2026-11-02 | minor | §13: Analysis ToolPak **z-Test: Two Sample for Means** landed (known variances, hypothesized difference, z / P(Z<=z) one-tail / z critical one-tail / two-tail rows through `NORM.S.DIST`/`NORM.S.INV`, in the t-tests' layout; dialog fields added). 14 of 19 tools; Fourier, Random Number Generation, Sampling and the two two-factor ANOVAs remain |
| 2026-11-02 | minor | §5: word-wrapped cells and chart labels are shaped — each paragraph measured once, broken at spaces/hyphens/CJK boundaries and shaped line by line (bidi per line, Arabic joining, marks), with right-aligned RTL General text and per-line alignment; printable ASCII keeps the unshaped wrap bit for bit. Chart layout measures labels with the shaped width. Text boxes and the PDF's `text_width` remain |
| 2026-11-02 | minor | §18: `Shift+F10` on a selected chart, picture, shape or slicer opens the object's own context menu at its frame (the right-click menu), while a cell selection keeps the cell menu; model helper + dispatch test. The accessibility/keyboard tail on the sheet is closed |
| 2026-11-02 | minor | §18: the status bar has an Accessibility entry with the issue count from `review.checkAccessibility` (cached per document revision), opening the same Check Accessibility dialog as the Review tab; what remains of the section is keyboard access to the sheet's object context menus |
| 2026-11-02 | minor | §7: `.xls` **pictures** landed — OfficeArt drawings (`MSODRAWING`, one `DgContainer` tree when concatenated) and the workbook's blip store (`MSODRAWINGGROUP` → `FBSE` → PNG/JPEG/DIB/TIFF), with `msosptPictureFrame` shapes turned into `Image`s at their `OfficeArtClientAnchorSheet` cell and offsets. The move/size bits map the way Excel writes them (0x0000 move+size, 0x0002 move only, 0x0003 free-floating), established from Excel-authored files. Charts, shapes, controls and vector pictures are skipped; malformed drawings never panic. §7 is down to charts and RC4 writing |
| 2026-11-02 | minor | §7: `.xls` **CF12** landed — colour scales, data bars and icon sets are read from `CondFmt12` + `CF12` (`rgbTemplateParms` is 16 bytes, `rgbCT` a `CFGradient`/`CFDatabar`/`CFMultistate`, thresholds as `CfValueKind`, colours as `Color`, the 17 ECMA icon-set names). Settled against Excel 16-authored files, which also showed that Excel writes a percentile as a type-7 literal and that its data-bar show-value bit is inverted; three Excel `.xls` files read end-to-end and match the `.xlsx` copies Excel wrote. §7 is down to charts/pictures and RC4 writing (4–11 h) |
| 2026-11-02 | minor | §8: calculated items landed (`pivot.calculatedItem`, item captions as the field's last shared items with `f="1"`, `<calculatedItems>` on the cache with the `Field[item]` formula, the item sorted last and evaluated on the cell's item aggregates, out of the grand totals), **verified in Excel 16**: the oracle pivot carries `Region[East]+Region[West]`, Excel keeps the definition, computes 116,320 and re-saves with every cell equal. §8 is now only data-model pivots and the corpus |
| 2026-11-02 | major | §1: Excel 16 is activated and `cargo xtask oracle` now uses it as the gate probe (open, refresh every PivotTable, recalc, save, re-read, compare every cell) on all five fixtures — no repair, no lost content; the corpus half is still missing. §8: the probe found and pinned the number-group encoding (no `refreshOnLoad`, base values in `sharedItems`, next-boundary tail, min-extent anchoring); calculated items are now observable in Excel |
| 2026-11-02 | minor | §8: recorded why calculated items wait (no primary source here settles `calculatedItems`' placement and index rules, and a guessed part risks an Excel repair prompt); the engine half is understood |
| 2026-11-02 | minor | §8: the `pivot` oracle fixture now carries a grouped numeric field (`rangePr` + 13 group captions) and a value filter's hidden items, and both probes pass (openpyxl readback; WPS opens, recalculates, re-saves) — well-formedness evidence, not the Excel check |
| 2026-11-02 | minor | §1: the corpus half cannot start on this machine — no corpus URL in the repo, no `craftrules` checkout, no network (github.com timed out) — so it is recorded instead of attempted; §10: a fresh 100 seeds × 200 steps soak at `3574f5d2` (after the three PivotTable changes) is green, 131 s, 0 failures |
| 2026-11-02 | minor | §8: label and value filters landed (`pivot.filterByLabel`/`pivot.filterByValue`, evaluated when set and stored as the field's hidden items, so the report/reopen/refresh agree; the rule stays in the model and is not written as Excel's `<filters>`); calculated items and the Excel check are what remain |
| 2026-11-02 | minor | §8: number grouping landed (`PivotNumberGroup`, `pivot.group` start/end/interval, `<start`/`>end` items, `rangePr` + `groupBy="range"` in the cache, round-trips through save/reopen); calculated items, value/label filters and the Excel check are what remain |
| 2026-11-02 | minor | §8: GETPIVOTDATA reads a report cell by its field items (every layout, Σ Values in Rows, subtotals, page filters, numbers and grand totals), depends on the report's written area so a refresh recalculates it, and is in the Insert Function list; calculated items, value/label filters and number grouping are what remain |
| 2026-11-02 | minor | 100 seeds × 200 steps (20,000 commands) command-sequence soak green in 68.3 s, 0 invariant failures; the hardware reports still need their machines. A WPS-authored `.xls` writes proprietary `0x087C`/`0x087D` records, not Excel `CF12`, so CF12 still needs gate #1 |
| 2026-11-02 | minor | `chart.setSeriesAxis` moves a series to the secondary axis (and optionally changes its kind); the Format pane has the checkbox; combo series are read back in `c:order`, not group order |
| 2026-10-30 | minor | §8: a table slicer follows a table and column rename, `insert.slicer` applies `caption`, and a report-side `pivot.filter` mirrors onto a connected slicer; corrected the stale "widget UI / timeline range / PivotCharts missing" list (`insert.pivotChart` landed 10-11) and dropped the stale "table slicer is not written" line |
| 2026-10-28 | minor | §2: a finished band drops its text pointer index (rebuilt on the first edit); the 100M-cell export falls from 2,411 to 1,911 MiB live (texts 640 → 301, number tables 679 → 519) and a text past the u16 slots keeps its own slot |
| 2026-10-28 | minor | §2: bulk changes scan blocked arrays once for the union of the changed cells and data commands cap at 12M cells: Remove Duplicates 1M×10 91 → 12 s, Sort 3.2 s, Fill Down 1.45 s; 1M×20 refused in 1.3 s |
| 2026-10-27 | minor | §2: independent formulas recalculate on up to eight threads (leaves wave by wave; arrays/volatile/iterative sequential): 100k end-to-end 65–75 → 20–25 ms, 1M calc 517–843 → 208–277 ms; §10 next step is bounded whole-sheet work and paging |
| 2026-10-27 | minor | §2: the 100k-dependent edit gate is met single-threaded (245 → 65–75 ms; full recalculation 281–289 → 134–147 ms) by reading formulas and legacy ranges from the graph, one result slot, row-major value writes, shared row extras, cached AST byte counts and a diff-first array check; §10 next step is multi-threaded recalc |
| 2026-10-26 | minor | §10: self-spilling arrays are `#CIRC!` and a contested spill keeps its owner across F9; three seeds remain, two of them the file side of spilled arrays |
| 2026-10-26 | minor | §10: 100-seed soak; Merge Across freeze, total-row overlap, Subtotal undo depth and a `!` column name fixed; four dynamic-array conflict seeds open with replays |
| 2026-10-26 | minor | §2: whole-column references are already clipped (21.5 ms for `SUM(A:A)` over a million cells); a 100k-dependent edit measures 223–251 ms; the 159 remaining cells are Excel's stale cache, not a difference |
| 2026-10-26 | minor | §2: the stripes' report numbers and a 100M-cell measurement (2.4 GB on a unique-key export against ~490 MiB extrapolated at report density); `packed_breakdown` no longer counts two dictionaries twice |
| 2026-10-20 | minor | §11: the Japanese catalog covers the dialogs, messages and menus — 943 keys (~43%), matching the pt-BR coverage; a test keeps the two catalogs in step |
| 2026-10-17 | minor | §6: the five reports it tracked are closed and their PRs merged (#57, #133, #139, #140, #134; PRs #148, #155, #156, #157); the entry stays open for §15's oracle suite |
| 2026-10-15 | minor | §13: Solver prepares reports before committing the solution, model and report sheets together; report refusal preserves the workbook and Undo/Redo (#1050) |
| 2026-10-13 | trivial | §4: multi-level category axes landed |
| 2026-10-12 | minor | §9: next/previous sheet commands retain the current visible sheet and selection when there is no visible destination |
| 2026-10-12 | minor | rows auto-fit oversized fonts on open and whole-row/column formatting (#833) |
| 2026-10-12 | minor | Page Layout view: dragging margins on the rulers (#792) removed from the §14 remainder |
| 2026-10-12 | minor | §13: Forecast Sheet Forecast Start (hindcast) landed (#785) |
| 2026-10-12 | minor | §17: ink geometry survives XLSX saves; the icon portion of #551 remains |
| 2026-10-12 | minor | §8: values on rows (Σ Values in Rows, `dataOnRows`) laid out, moved and round-tripped |
| 2026-10-12 | minor | Desktop File > Print now discovers printers and submits PDF jobs through pinned craft-print with printer choice, copies, duplex and grayscale; browser printing and PDF export remain available. Remaining print parity includes page ranges and driver-specific tray/options UI; physical printer fidelity needs hardware validation. |
| 2026-10-11 | minor | §6: iterative calculation for circular references landed (Maximum Iterations, Maximum Change) |
| 2026-10-11 | minor | §15: trim-reference operators (`A1:.E10`, `A1.:E10`, `A1.:.E10`) parse and evaluate |
| 2026-10-11 | minor | §4: trendlines (six types, forecast, equation and R²) read, written and drawn |
| 2026-10-11 | minor | §7: password-encrypted workbooks open with their password; saving with one remains (#11) |
| 2026-10-11 | minor | §16: comment navigation, Show Comments, Show All Notes and Convert to Comments landed (#281) |
| 2026-10-11 | minor | §7: `.xls` (BIFF8) import of values, formulas and number formats (#121) |
| 2026-10-11 | minor | §3: XLSX pass-through keeps custom XML, external links, connections, query tables, printer settings, controls and OLE objects (#330, #324) |
| 2026-10-11 | minor | §13: Forecast Sheet landed (#322) |
| 2026-10-11 | minor | §15: ODDFPRICE, ODDFYIELD, ODDLPRICE, ODDLYIELD added (18 functions missing) |
| 2026-10-11 | minor | §15: PERCENTOF and EUROCONVERT added (20 functions missing) |
| 2026-10-11 | minor | §15: BAHTTEXT added (21 functions missing) |
| 2026-10-11 | minor | §15: TRIMRANGE added (21 functions missing; trim-reference operators still missing) |
| 2026-10-11 | minor | §16: Allow Edit Ranges landed (#307) |
| 2026-10-11 | minor | §8: calculated fields added |
| 2026-10-11 | minor | §8: PivotCharts added |
| 2026-10-11 | minor | §13, §15: FORECAST.ETS family landed (#314) |
| 2026-10-10 | minor | Updated for the 10-10 merges (#17, #31, #38, #62, #82, #104, #116) and the new chart resize bug (#190) |
| 2026-10-10 | minor | Noted that no gap blocks the alpha gate |
| 2026-10-10 | major | Created: 24 ranked gaps with evidence, from the full re-measure and the 60 open issues |