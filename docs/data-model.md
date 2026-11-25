# The data model: what it does now

Written 2026-11-26. This is the user-facing page: what the data model is for, how to use it, what it
costs. The engineering record is in [`gaps.md`](gaps.md) §60–§72 and the engine survey and spikes are
in [`sql-engine-research.md`](sql-engine-research.md).

## The problem it solves

A worksheet holds **1,048,576 rows**, and every cell of it lives in the document. A sales export of
five million rows does not fit, and a report that has to re-read a million rows every time a slicer
moves is not interactive.

The **data model** is the part of Cella that holds a data set *outside* the grid: as typed columns in
a database, where a report is a query. A model table can be bigger than a sheet, a report over it
refreshes in milliseconds, and one data set can feed many reports without a copy per view.

## How to use it

| what you want | how |
|---|---|
| A **file** in the model (a CSV or Parquet export, however big) | **Queries & Connections ▸ Add from file to model…**, or the command `data.model.loadFile {path}` |
| A **query's result** in the model (a sheet range, a CSV with steps, a URL, a folder…) | `data.query.add {source, steps, load: "model"}`. When the source is one file and the only step is promoting its header, the file itself is kept — the rows never enter the document |
| **SQL** over what is in the model | `data.model.query {sql}` — read-only, from the pane, the control channel or MCP. `SELECT region, SUM(amount) FROM Sales GROUP BY 1` |
| A **report** over the model | Insert a PivotTable onto the table (`insert.pivotTable {source: "Sales", …}`) — its rows, columns and values are the model's fields |
| **Measures** (a formula, not a column) | `data.measure.add {name, formula}` — `SUM(Sales[Amount])`, `DIVIDE(SUM(Sales[Amount]), COUNT(Sales[Amount]))`, `CALCULATE(…, Region[Name] = "West")` |

A workbook keeps the *definition*: the paths, the column names and their types. Reopening it reads
the files again; a file that has moved away is an error a report shows, never a silently empty table.

## The numbers

Measured on one machine, release build, five million rows × five columns of CSV
(`cargo run -p cella-engine --example model_sql_probe --release --features sql-engine`):

| action | time |
|---|---:|
| `data.model.loadFile` (109 MB CSV → a model table) | **0.15 s** |
| `pivot.refresh`: 768 cells, plain aggregation | **7.6 ms** |
| `pivot.refresh`: grouped by a **joined dimension** | **20.9 ms** |
| `pivot.refresh`: a **ratio measure** over two row levels (detail + sub-totals + grand total) | **29.0 ms** |
| `pivot.refresh`: a measure over a **month axis** | **58.7 ms** |
| one cell edit in a workbook whose model holds those five million rows | **0.1 ms** |

For comparison, a report over a table cached *in the document* at one million rows used to take
**640 ms**; it is 8 ms now. The engine is not the limit — the file is read at tens of thousands of
rows a millisecond, and a group-by over twenty million rows is still under twenty milliseconds.

## What a report can do on a model table

Everything below is answered by the engine, in **one query** (a measure: one `GROUPING SETS` query
that computes every level, because a formula does not compose from its own sub-totals):

- aggregates: sum, count, count numbers, average, min, max — with sub-totals and grand totals;
- **relationships**: group, filter or sort by a related table's column (`Region[Name]`), several
  hops if the model chains;
- **measures**: DAX-class formulas — aggregates, `CALCULATE` filters, `DIVIDE`, arithmetic, a
  measure built on another measure;
- **grouped axes**: a date grouped by year, quarter, month or day; a number grouped into ranges;
- **slicers and report filters**;
- **value filters** ("keep the regions whose total is above ten");
- **Show Details** (the records behind a cell), bounded by the sheet.

Two things still read the records one by one, and say so when a table is too big for that: a value
field SQL cannot compose (**product**, **standard deviation**, **variance**) and a **text column
summed**.

## The browser build has no engine

The engine is a bundled C++ database, so it is **desktop only**. The web build keeps the workbook,
the queries, the model's *definition* and the reports' cached values, and every command that needs
the engine answers "the data model's SQL engine needs the desktop app" rather than pretending. The
grid, the formulas, the file formats and everything else are unaffected.

## Seeing it

```bash
# the pane, offscreen, with the button this page talks about
cargo run -p cella-ui-egui --example snapshot -- --sample sales --pane queries out.png

# the engine's own numbers, at any size you like
ROWS=5000000 cargo run -p cella-engine --example model_sql_probe --release --features sql-engine
```

The model is also reachable headless — `cella-cli` carries the same commands, and `data.model.query`
is what an agent uses to ask the model a question.
