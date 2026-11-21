# Gaps

> **Last reviewed:** 2026-11-20 · **Last updated:** 2026-11-20 · **Change:** round 27 (11-21): three sign-tested knives landed (§2) — the closure's second-dependent box (−18.4 % on the 2M-dirty edit), band-copy-tag pricing (−7.2 %), and the store's compare-before-write plus one-bit row find (−20.1 %/−21.4 %). Earlier: round 26 (11-20): the evaluator is refuted by measurement (§2) — bytecode built and equivalence-proven, sign-tested a no-win (the per-cell cost is kernel-bound, not traversal-bound); the chain edit's missing time was undo bookkeeping, and an evicted step now releases on later cheap commands (built-case edit −30 %, 8/8, p=0.0039; the gate's aggregate 443 ms). Earlier: round 25 (11-20): the 256 × 200 release soak's three findings are **fixed** with red-first regressions — two incremental-vs-full spill divergences in calc, and §3's suppressed-filter hidden rows, the same root cause as the seed-97 case, closed to its design question; the perf gate is locked at twice the 11-20 quiet pass and grew `mem_drift` and `save.repeat` (22 → 25 scenarios, all green); and the unmeasured workloads have numbers (performance.md). Earlier: round 23 closed the memory and performance line (§2): a 1M × 6 workbook is **0.884 GB** live / 0.683 GB reopened (2.90 GB before the round), the row side tables (144 → 72 MB), the band's number index (66 → 33 MB) and the journal (42 → 15 MB) taking the last steps; **measured against Excel 16 and WPS 12.1** on one 70 MB file (open 3.3 vs 8.0 / 10.7 s, recalc 0.75 vs 0.31 / 15.3 s, idle 246 vs 35 / 288 MB) with **OpenGL the Windows default** (183 MB lighter at the same 144 Hz); two recalculation changes took the 1M chain 0.85 → 0.75 s; and the soak found one reference bug (§6: a formula moved by Remove Duplicates kept its stale shape), fixed with its test. Earlier: the memory round (§2), three waves: byte accounting per part; the dependency graph compacted twice (dependents 478.2 → 142.6 MB, nodes 172 → 104.9, areas 201 → 125); the save build parallelised; formula **shape sharing** (2M formulas → 3 shapes, the per-cell tree deleted, `Formula` 48 B, `Node` 16 B); a **structural edit rewritten once per shape** (insert a row above 2M formulas 8.29 → 3.97 s, delete 8.61 → 3.59 s, rename a sheet 8.58 → 4.86 s); and an **undo that restores a paged store**, gated so it never costs the edit (a sort of a paged store used to take minutes and now takes the sheet back into memory). A 1M × 6 workbook is **2.90 → 0.971 GB** live (reopened 0.77 GB) and no scenario is slower: save 3.7 → 2.05 s, open 5.5 → 4.02 s, fill formulas 2.8 → 1.77 s, recalc 2.3 → 1.80 s, rebuild 1.7 → 1.12 s. Earlier: minor (11-14: the per-cell half of the performance round — `MODE` 2.66 s → 15 ms, `VLOOKUP` 2.9–4.9× and text `MATCH` 7.2×, wildcard `COUNTIF` 2.0×, grid paint 1.60 → 1.07 ms/frame, all in §2; and §3's filtered-sheet save bugs were found by a 150-seed soak. 11-13: the performance round (§2) and three save-fidelity bugs in filtered sheets found by the command-sequence soak (§3), one case left recorded there with a six-step repro; round 18: a real-world corpus run — **3,084 workbooks opened, 635 save/reopen with 0 mismatches, 16 files identical in Excel 16** — so #1 is met for beta; the pinned corpus remains for CI. Round 17: `.xls` **embedded charts, autoshapes and text boxes** are read — group kind and variants, series ranges, cached names, title, legend and the `msosptChart` anchor — and **verified in Excel 16** (five chart kinds; five shapes with their text, fill and line); §7 loses its charts and shapes items and is met for beta, leaving OLE objects and chart sheets as full scope. **RC4 is not an ECMA-376 Standard cipher**: [MS-OFFCRYPTO] 2.3.4.5 requires AES and Excel refuses a hand-made RC4 `.xlsx` at 40 and 128 bits, so "RC4 write" is really encrypted `.xls` output and waits for a `.xls` writer. No score change) · **Target:** Microsoft Excel (Microsoft 365); earlier: the accessibility check names every object without alternative text (WordArt and equations included) |

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

## Revision history

| Date | Change | Summary |
|---|---|---|
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
