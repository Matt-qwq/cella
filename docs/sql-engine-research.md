# SQL engines for the data model: the research

Written 2026-11-25. The owner's direction: **use a professional SQL engine rather than writing one** —
hidden bugs in a hand-written executor are the thing to avoid. This page is the survey and the
decision it supports. Nothing here is benchmarked inside Cella yet; the numbers are expectations from
the projects' own documentation and community reports, and the spike described at the end replaces them
with ours.

## What the engine has to do

The data model's job (`gaps` §? for the discussion): hold **more rows than a sheet** (1,048,576 is the
cap) as typed columns, and answer an interactive query in **200–300 ms** at ~5 million rows:

```sql
SELECT region, month, SUM(amount) FROM sales GROUP BY 1, 2 ORDER BY 1, 2;
```

with filters from slicers (`WHERE region IN (…)`), and — eventually — joins to lookup tables. The
front end is SQL; the pivot/slicer UI generates it, and a query box / the MCP control channel can take
it verbatim.

## Cella's constraints, which decide the shortlist

| rule | what it means for the choice |
|---|---|
| **Never break wasm** (`cargo xtask wasm`) | the engine must build for `wasm32-unknown-unknown`, or the data model is desktop-only (a product decision, see below) |
| **Rust only, no handwritten JS** | a C/C++ engine is fine *if* it builds through a Rust crate (`cc`/`bindgen`), but not one that needs an Emscripten/JS side channel for wasm |
| **No external installs for users** | the engine must be *bundled* (`duckdb`'s `bundled`, `rusqlite`'s `bundled`), not "install libchdb first" |
| **Licence** | MIT / Apache-2.0 / public domain only; never GPL/AGPL (repository rule) |
| **Build time and binary size** | Cella's `cargo xtask ci` runs often; a 10-minute C++ build is a real cost |

## The candidates

| engine | language | licence | bundled | wasm32 | SQL coverage | analytical speed (expected) | note |
|---|---|---|---|---|---|---|---|
| **DuckDB** (`duckdb` crate) | C++ | MIT | yes (`bundled`) | **no** — the Rust crate does not build for `wasm32`; `duckdb-wasm` is a separate Emscripten/JS build ([forum thread](https://users.rust-lang.org/t/duckdb-wasm-in-rust/122979/3), [duckdb-wasm docs](https://github.com/duckdb/duckdb-wasm/blob/c16b543b8703c7e2cd545d7a939ab4d4a4b4b091/docs/local_development_guide.md)) | excellent (OLAP SQL, window, CTE, even `PIVOT`) | best in class: columnar, vectorised, `read_parquet()` directly | the reference answer for this job, **if** wasm can be given up |
| **DataFusion** | Rust | Apache-2.0 | n/a | **yes, proven** — [micromegas-datafusion-wasm](https://crates.io/crates/micromegas-datafusion-wasm/0.21.0) compiles it with `wasm-bindgen` + `wasm-opt` | very good (joins, windows, Parquet) | good: Rust, vectorised, Arrow | pure Rust, no C++ build; pulls in the Arrow/Parquet stack (we already depend on `parquet`) |
| **SQLite** (`rusqlite` + `bundled`) | C | public domain | yes | **yes** — rusqlite ≥0.38 swaps to `sqlite-wasm-rs` on `wasm32-unknown-unknown` ([cratestack-rusqlite](https://github.com/cratestack/cratestack/blob/bccf7a23b090748a5ec0ab487374a1896b0e6f97/crates/cratestack-rusqlite/README.md)) | very wide (the most tested SQL engine in existence) | row-store: a 5M-row group-by is a full scan, roughly **0.2–1 s**; loading 5M rows into `:memory:` is itself seconds | the "cannot be wrong" option; the cost is speed and load time |
| **Polars + `polars-sql`** | Rust | MIT | n/a | **no upstream support** — in-browser builds need third-party forks ([terrydc/polars-wasm](https://github.com/terrydc/polars-wasm)) | `polars-sql` is a **transpiler** to Polars plans, so coverage is partial | excellent (DataFrame engine) | great engine, weaker "SQL engine" story (partial dialect) and no upstream wasm |
| **GlareDB** | Rust | MIT | n/a | unverified | good | good | younger project; check maturity before betting on it |
| **chDB** (ClickHouse embedded) | C++ | Apache-2.0 | **no** — needs `libchdb` installed ([docs](https://clickhouse.com/docs/ja/chdb/install/rust)) | no | excellent | excellent | fails "no external installs"; Rust FFI is experimental |

Not candidates: Apache Calcite (Java), Spark, QuestDB/ClickHouse *servers* (Cella has no server
process), and anything GPL/AGPL.

## What the trade really is

**DuckDB is the best engine for the job and the worst fit for one rule.** It reads Parquet directly,
has `PIVOT`/window/CTE, and is the fastest of these on a group-by at this size. The moment the data
model is DuckDB, the browser build loses the feature — unless we accept "the data model is
desktop-only", which is a product statement, not a technical detail.

**DataFusion is the best fit for the rules.** Pure Rust, Apache-2.0, in-process, wasm-proven by a
third party, and it can read the Parquet files we already write. The costs: the Arrow dependency tree
(build time, binary size) and a query engine that is good but not DuckDB-fast.

**SQLite is the risk-free floor.** It cannot be wrong (it is the most exercised SQL engine there is),
it works everywhere including wasm, and it costs almost nothing in size (~1–2 MB). It is a row store,
so the 5M-row group-by is a scan in the hundred-millisecond-to-second range, and its in-memory load of
5M rows is seconds. As a *first* version behind a stable interface it is honest; as the long-term
analytics engine it is not.

## Recommendation

1. **Put the engine behind one internal interface** — `open(path)`, `query(sql) -> batches` — so the
   choice is replaceable. Everything else in the data model (pivot, slicers, query box, MCP) talks to
   that interface, never to a specific engine.
2. **Spike two engines on the same file and the same SQL**, before any product work:
   - **DataFusion** (the wasm-safe candidate), in a feature-gated crate, so `cargo xtask wasm` is
     told whether it can stay green;
   - **DuckDB** on native only, as the ceiling — it tells us how much the wasm constraint costs.
   Both on 5M rows × 10 columns of Parquet, running the group-by above, five passes, plus a measure of
   the wasm build's size and the build time.
3. **Then decide** with numbers:
   - DataFusion within ~2× of DuckDB and wasm stays green → **DataFusion**, one engine everywhere;
   - the gap is large and the product can accept a desktop-only data model → **DuckDB** on desktop,
     and the model says so plainly in the UI;
   - both are unacceptable in build time or size → **SQLite** behind the same interface, and revisit.

## The DuckDB spike: measured (2026-11-25)

The owner accepted a **desktop-only data model**, so the wasm constraint is off the table and the
question became "is DuckDB actually this fast at our sizes". It is. Run out of the repository in a
throwaway crate (`duckdb = { version = "1", features = ["bundled"] }`, `SET threads TO 8`), on a
machine that was busy at the time:

| case (5 M rows × 10 columns) | time |
|---|---:|
| build the table and write its Parquet file | 1.0 s (36 s the first time, cold) |
| **group-by 64 × 12 out of the Parquet file** | **18.3 ms** |
| slicer (4 of 64 regions) out of the file | 20.0 ms |
| top-20 by product out of the file | 18.8 ms |
| **load 5 M rows into memory** | **0.1 s** |
| **group-by 64 × 12 in memory** | **3.8 ms** |
| slicer in memory | 4.6 ms |
| top-20 in memory | 2.9 ms |
| join to a 64-row lookup table, then group-by | 8.2 ms |
| **20 M rows, group-by in memory** | **11.6 ms** |

**A correction to that table:** the synthetic rows used `i % 64` for the region and `i % 12` for the
month, so only **192** of the 768 pairs ever occur — those timings are for a 192-cell report, not a
768-cell one. The in-tree probe below derives each key from its own divisor.

**Memory:** a process holding **5 M and 20 M rows** of this shape (25 M rows × 10 columns) was
**1.75 GB** of working set — about **70 MB per million rows** with DuckDB's compression. So 5 M rows
≈ 350 MB, 20 M ≈ 1.4 GB.

**Build cost:** the bundled C++ engine took **7 min 13 s** to compile the first time (cached after).
That is a one-off per clean checkout, and a real cost for `cargo xtask ci` when the cache is cold.

**The comparison that matters:** our own sheet path does a million-row pivot refresh in **352 ms**
(`pivot.refresh`, five passes) and a million-group `GROUPBY` in **583 ms**. DuckDB does **five million**
rows in **3.8 ms** in memory and **18 ms** straight from a Parquet file. The target was 200–300 ms at
5 M; the engine is two orders of magnitude under it.

What this does **not** say: how long it takes to get the data *into* the model from a spreadsheet, or
how a pivot's cells map onto the query result — that is the integration work, not the engine.

## The integration, measured in the tree (11-25)

The engine landed as `crates/sql` (L2): `Engine::in_memory`, `load_rows` (rows Cella already
holds), `load_file` (a Parquet/CSV file the engine reads itself), `query` (a bounded `Table` back)
and `execute`, with DuckDB behind a `duckdb` feature the desktop build turns on and the browser
build leaves off (every call there returns `Unavailable`; `available()` says so). The questions the
spike could not answer are answered now, by
`cargo run -p cella-sql --example sql_probe --release --features duckdb`:

| case | 1 M rows | 5 M rows |
|---|---:|---:|
| Cella's `Value` rows → engine (`load_rows`) | **716 ms** (716 ns a row) | **3 677 ms** (735 ns a row) |
| Parquet file → in-memory table (`load_file`) | **99 ms** (99 ns a row) | **109 ms** (22 ns a row) |
| group-by into a 768-cell report | **6.4 ms** | **14.5 ms** |
| slicer (4 of 64 regions) + group-by | 4.7 ms | 9.5 ms |
| top-20 products | 1.7 ms | 4.2 ms |

**The bridge is the slow part, and that decides the architecture.** A row of ours costs ~720 ns to
cross the engine's interface; measured outside the tree, where the two halves could be timed
separately, that is ~200 ns to turn ten `Value`s into parameters and ~375 ns for the append itself
(a tuple instead of a `Vec` a row measures the same, so it is the per-value call, not our
allocation). A file the engine parses itself costs **22–99 ns a row — seven to thirty times less**:
a model table can only grow past a few million rows if the bytes arrive as a file.

What that means for the next round:

* **A model table loaded from a file keeps its path**, and the engine reads it (`read_parquet`), so
  5 M rows are in the model in about a tenth of a second and no 4M-cell copy is stored in the
  document.
* **A sheet range still goes row by row** — 0.72 s a million rows, paid once when a person adds it
  to the model, not on every refresh.
* **A pivot or a slicer re-queries.** 5–15 ms means a drag is one query, so the report's shape
  (drill, collapse, top-N) can be answered by SQL instead of a hand-written grouping pass.

## The engine now holds the model (11-25)

`crates/engine` gained a `sql-engine` feature (on in the desktop and CLI apps, off in the browser,
where the command says so) and **`data.model.query {sql} → {columns, rows}`**: read-only SQL over
the model's tables, from the command set, the control channel and MCP. The engine keeps a copy of
every model table, filled once on first use and rebuilt whenever the model changes (a load, a
removal, an undo, another document); `cella_sql::is_read_only` refuses anything that is not a
query before it reaches the engine, so a query box cannot write a file or attach a database.

At 1M rows × 4 columns (`cargo run -p cella-engine --example model_sql_probe --release --features
sql-engine`): the 768-cell group-by is **5.4 ms** and the same report with a 4-of-64 slicer is
**3.6 ms**, against **640 ms** for our own `pivot.refresh` of that report — 118×.

**The pivot asks the engine now.** A report over a model table is aggregated by three queries (one
`GROUP BY`, `DISTINCT` per filtered field, `DESCRIBE` for the shape) and every sub-total is composed
from the leaves; labels stay in Rust, so date grouping is identical on both paths. `pivot.refresh`
at 1M rows went **640 ms → 8.0 ms**, and the last hidden cost turned out to be the workbook copy
every command makes for undo: the model's rows are `Arc`-shared now, which took **one cell edit from
109 ms to 0.1 ms**. The differential test that holds the two aggregators together is
`crates/engine/tests/pivot_sql.rs`; what is still on the scan is a related table's columns, a DAX
measure, and the functions SQL cannot compose (product, standard deviation, variance).

**A table bigger than the cache.** `data.model.loadFile` keeps only the file's path, its columns and
their types in the document, and the engine reads the file itself: **5M rows × 4 columns (20M cells,
five times the 4M the document caches) load in 0.11 s and refresh a 768-cell report in 17.6 ms**.
The engine is now the storage, not just the calculator — which is what the "70 MB a million rows"
memory reading buys.

## Where it stands (end of 11-25)

A report over a model table is answered by the engine in every shape it has: plain aggregates (one
`GROUP BY`, sub-totals composed from the leaves), a join to a related dimension (`LEFT JOIN`, one
`t0`, `t1`… per table), a measure (one `GROUPING SETS` query for every level, because a formula does
not compose from its own sub-totals), and a measure over a date-grouped axis (grouped by the date
part, the caption still built here). The same plan (`plan_source`) feeds the headers, the scan the
engine falls back to, and the SQL, so the three cannot disagree about what a column means.

Measured on a 5M-row × 5-column CSV the engine reads itself (`model_sql_probe`, release):

| case | time |
|---|---:|
| `data.model.loadFile` (109 MB CSV) | 0.15 s |
| 768-cell group-by over it | 4.5 ms |
| **`pivot.refresh`** of that report | **7.6 ms** |
| the same, grouped by a joined dimension | 20.9 ms |
| a ratio measure, two row levels | 29.0 ms |
| a ratio measure over a month axis | 58.7 ms |

The entry points are `data.model.loadFile` (a file into the model), `data.model.query` (read-only SQL
over it, for the control channel and MCP), and **`data.query.add` with `load: model` when the source
is one file whose only step names its header** — so the ordinary "import a file" flow takes the file
route too. What still reads records: a value filter (top-N by a measure), Show Details, and a
numeric-range axis; each is bounded and says so when a table is past the bound.

