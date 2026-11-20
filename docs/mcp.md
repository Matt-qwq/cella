# Cella MCP server

`cella-cli mcp` exposes Cella to AI agents over the [Model Context Protocol](https://modelcontextprotocol.io):
newline-delimited JSON-RPC 2.0 on stdin/stdout (protocol `2025-06-18`; `2025-03-26` and `2024-11-05` clients are
answered in their own version). Logs go to stderr.

Two backends:

| Mode | Command | What it drives |
|---|---|---|
| Headless (default) | `cella-cli mcp [--in book.xlsx \| --sample budget]` | An in-process engine session — no window. |
| Remote | `cella-cli mcp --connect 7979` | A running app started with `cella --control 7979` (TCP JSON lines on `127.0.0.1`). |

## Claude Code

```sh
claude mcp add cella -- cella-cli mcp
# or drive the desktop app you are looking at:
claude mcp add cella-app -- cella-cli mcp --connect 7979
```

JSON config (`.mcp.json`, Claude Desktop, other clients):

```json
{
  "mcpServers": {
    "cella": {
      "command": "cella-cli",
      "args": ["mcp"]
    }
  }
}
```

Use `"args": ["mcp", "--connect", "7979"]` for the desktop app, or `["mcp", "--in", "/path/book.xlsx"]` to start
with a file open.

## Tools

| Tool | Arguments | Does |
|---|---|---|
| `execute_command` | `command`, `params?` | Runs **any** engine command (`cell.set`, `home.bold`, `data.removeDuplicates`, …). |
| `batch` | `calls` | Up to 50 other tools in one request, in order — one answer per call, one failing call never stops the rest. |
| `list_commands` | `search?` | Command ids, labels, ribbon paths, shortcuts and parameter docs. |
| `inspect_workbook` | — | Sheets, used ranges, tables, charts, names, selection, history. |
| `read_range` | `range?`, `sheet?`, `formulas?`, `formatted?` | Rows of values (default: the used range). |
| `write_range` | `range`, `values` (2-D) | `range.setValues`; strings starting with `=` are formulas. |
| `set_cell` / `get_cell` | `cell`, `input` / `cell` | Enter one cell / read value, formula, display text, style. |
| `evaluate_formula` | `formula`, `cell?` | Evaluates without changing the sheet. A formula that couldn't be entered in a cell (it doesn't parse, or a function gets the wrong number of arguments) is an error saying why. |
| `select` | `range`, `active?` | Sets the selection. |
| `format_range` | `range`, `bold?`, `italic?`, `underline?`, `fontName?`, `fontSize?`, `fontColor?`, `fillColor?`, `numberFormat?`, `horizontalAlign?`, `wrapText?`, `borders?` | Composes the `home.*` formatting commands. |
| `insert_chart` | `range`, `type`, `title?`, `subtype?`, `at?` | `insert.chart`. |
| `create_table` | `range`, `style?`, `header?`, `name?` | `insert.table`. |
| `sort_range` | `keys`, `range?`, `header?` | `data.sort`; keys are `"B"`, `"B desc"` or `{column, order}`. |
| `filter` | `column`, `range?`, `values?`, `custom?`, `top?`, `blanks?`, `clear?` | `data.filterBy` (turns AutoFilter on when needed). |
| `list_queries` | — | Every Get & Transform query (`data.query.list`): name, source kind and path, the sheet it loads to, its columns and its steps. |
| `run_query` | `name`, `source?`, `steps?`, `load?` | Define and run a query pipeline (`data.query.add`): a csv/json/xlsx/parquet/folder/range/table source, steps such as PromoteHeaders or ChangeType, loaded to a sheet (or `load: "model"` into the data model); answers `{name, sheet, rows, cols}`. Without `source`, re-runs the named query (`data.query.refresh`). |
| `preview_query` | `name`, `step?`, `at?`, `limit?` | Runs a pipeline without writing anything and shows the top of the result (`data.query.preview`): `{columns, rows, total, truncated, failed?}`; `step` with `at` answers what a step still being composed would do. |
| `list_model` | — | The workbook's data model in one answer: `{tables, relationships, measures}` — what `data.model.tables`, `data.relationship.list` and `data.measure.list` report separately. |
| `add_relationship` | `from`, `to` | Relates two model tables many-to-one (`data.relationship.add`): each row of `from` (the many side, the facts) matches one row of `to` (the one side, a lookup) on equal column values; filters flow from `to` to `from`. |
| `add_measure` | `name`, `formula`, `table?`, `format?` | Defines a measure over the model (`data.measure.add`): DAX (`SUM`, `CALCULATE`, `DIVIDE`, …, columns as `Table[Column]`), validated before it is kept — a missing table or column is refused. |
| `evaluate_measure` | `name`, `filters?` | Answers a measure without putting it on a sheet (`data.measure.evaluate`) → `{value, display}`; `filters` narrow the model first and reach the facts through the relationships. |
| `run_script` | `name`, `trusted?` | Replays a script stored in the workbook (`automate.runScript`): its steps in order, answers `{ran: n}`. A script from a file stays disabled until the workbook is trusted — `trusted: true` runs it this once; a step that reaches outside the workbook (file, path, link) is refused before any step runs. |
| `list_scripts` | — | Scripts stored in the workbook (`automate.allScripts`): name, step count, whether they may run now; a carried-through VBA project is listed as "(VBA)" and never executed. |
| `open_workbook` / `save_workbook` / `new_workbook` | `path` / `path?` / `sample?` | Files (`.xlsx`, `.csv`, `.tsv`, `.json`, `.html`). |
| `undo` / `redo` | `steps?` | History. |
| `screenshot` | `path` | PNG of the window (Remote only). |
| `list_functions` | `search?`, `category?` | Worksheet functions with signatures. |

Results are one text block holding pretty JSON; failures set `isError: true` with a readable message. Unknown tools,
missing required arguments and wrong argument types are JSON-RPC `-32602` errors; malformed JSON is `-32700`; unknown
methods `-32601`.

`batch` answers once per call — `{tool, ok, result}` or `{tool, ok, error}` — in the order the calls were sent, so a
write and its read-back cost one round trip instead of two. One failing call is its own error and never stops the rest.
A list of more than 50 calls, an `arguments` blob over 256 KB of JSON, and a batch inside a batch are all refused
outright (nothing runs) — send several batches instead.

## Resources

- `cella://workbook` — `document.inspect` of the active workbook.
- `cella://commands` — every command with its parameter docs.
- `cella://functions` — every worksheet function.

`prompts/list` returns an empty list.

## Example session

```text
→ {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}
→ {"jsonrpc":"2.0","method":"notifications/initialized"}
→ {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"write_range","arguments":{"range":"A1","values":[["Item","Cost"],["Rent",1200],["Food",400],["Total","=SUM(B2:B3)"]]}}}
→ {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"read_range","arguments":{"range":"B4"}}}
← {"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"{\n  \"range\": \"B4\", ... \"values\": [[1600.0]]\n}"}],"isError":false}}
```

## Embedding

`cella-mcp` is a library: `Server::new(Box::new(Headless::new()))` or
`Server::new(Box::new(Remote::connect(&control_addr("7979"))?))`, then `Server::serve(stdin, stdout)` or
`Server::handle_line(line) -> Option<String>`. Implement the `Backend` trait (`call(method, params)`) to drive another
host.
