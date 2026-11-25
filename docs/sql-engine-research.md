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

## Spike checklist (1–2 days)

- [ ] `crates/…-spike` (or a `--features datafusion` gate): create a 5M × 10 Parquet file, load, run
      `SELECT region, month, SUM(amount) … GROUP BY`, five passes.
- [ ] Same file, same SQL, DuckDB (native, `bundled`).
- [ ] Record: query ms, load ms, RSS delta, crate tree size, `cargo build` time, `cargo xtask wasm`
      outcome and `.wasm` size.
- [ ] Write the numbers into this page and pick the engine.

Sources: [SQLite vs DuckDB vs Arrow discussion](https://hn.svelte.dev/item/29010103),
[columnar DB survey](https://github.com/alopex-db/docs/blob/main/tech/columnar-db-research.md),
[BenchBox platform comparison](https://benchbox.dev/docs/guides/platform-comparison.html).
