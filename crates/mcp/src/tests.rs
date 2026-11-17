#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{BufRead, BufReader, Write};

use serde_json::{Value, json};

use crate::{Backend, Headless, PROTOCOL_VERSION, Remote, Server, control_addr, tool_definitions};

fn server() -> Server {
    Server::new(Box::new(Headless::new()))
}

fn rpc(s: &mut Server, id: u64, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
    let reply = s.handle_line(&line).expect("reply");
    let v: Value = serde_json::from_str(&reply).expect("reply is JSON");
    assert_eq!(v["jsonrpc"], "2.0");
    assert_eq!(v["id"], id);
    v
}

fn call(s: &mut Server, name: &str, args: Value) -> Value {
    let v = rpc(s, 99, "tools/call", json!({"name": name, "arguments": args}));
    assert!(v.get("error").is_none(), "{v}");
    v["result"].clone()
}

fn text_of(result: &Value) -> String {
    result["content"].as_array().unwrap().iter().filter(|c| c["type"] == "text").map(|c| c["text"].as_str().unwrap().to_string()).collect()
}

/// Call a tool that must succeed; returns its JSON payload.
fn ok(s: &mut Server, name: &str, args: Value) -> Value {
    let r = call(s, name, args);
    assert_eq!(r["isError"], false, "{name}: {r}");
    serde_json::from_str(&text_of(&r)).unwrap_or(Value::Null)
}

/// Call a tool that must fail; returns its message.
fn fails(s: &mut Server, name: &str, args: Value) -> String {
    let r = call(s, name, args);
    assert_eq!(r["isError"], true, "{name} should fail: {r}");
    text_of(&r)
}

fn err_code(v: &Value) -> i64 {
    v["error"]["code"].as_i64().unwrap_or_else(|| panic!("expected an error: {v}"))
}

fn tmp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cella-mcp-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

// ---------- protocol ----------

#[test]
fn initialize_and_version_negotiation() {
    let mut s = server();
    let v =
        rpc(&mut s, 1, "initialize", json!({"protocolVersion": PROTOCOL_VERSION, "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}));
    let r = &v["result"];
    assert_eq!(r["protocolVersion"], PROTOCOL_VERSION);
    assert_eq!(r["serverInfo"]["name"], "cella");
    assert!(r["capabilities"]["tools"].is_object());
    assert!(r["capabilities"]["resources"].is_object());
    assert!(r["instructions"].as_str().unwrap().contains("headless"));
    // Older supported version is echoed back; unknown versions get ours.
    let v = rpc(&mut s, 2, "initialize", json!({"protocolVersion": "2024-11-05"}));
    assert_eq!(v["result"]["protocolVersion"], "2024-11-05");
    let v = rpc(&mut s, 3, "initialize", json!({"protocolVersion": "1999-01-01"}));
    assert_eq!(v["result"]["protocolVersion"], PROTOCOL_VERSION);
    assert!(!s.is_initialized());
    assert_eq!(s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#), None);
    assert!(s.is_initialized());
    assert_eq!(rpc(&mut s, 4, "ping", json!({}))["result"], json!({}));
    assert_eq!(rpc(&mut s, 5, "prompts/list", json!({}))["result"]["prompts"], json!([]));
}

/// A schema is valid JSON Schema for us when every `required` entry is one of `properties`,
/// at every level (properties, array items and `oneOf` alternatives).
fn check_schema(name: &str, schema: &Value) {
    for r in schema["required"].as_array().into_iter().flatten() {
        let key = r.as_str().unwrap_or_default();
        assert!(schema["properties"].get(key).is_some(), "{name}: required `{key}` is not in properties");
    }
    for s in schema["properties"].as_object().into_iter().flat_map(|o| o.values()) {
        check_schema(name, s);
    }
    for s in schema.get("items").into_iter().chain(schema["oneOf"].as_array().into_iter().flatten()) {
        check_schema(name, s);
    }
}

#[test]
fn tools_list_has_schemas() {
    let mut s = server();
    let v = rpc(&mut s, 1, "tools/list", json!({}));
    let tools = v["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for want in [
        "execute_command",
        "batch",
        "list_commands",
        "inspect_workbook",
        "read_range",
        "write_range",
        "set_cell",
        "get_cell",
        "evaluate_formula",
        "select",
        "format_range",
        "insert_chart",
        "create_table",
        "sort_range",
        "filter",
        "list_queries",
        "run_query",
        "preview_query",
        "list_model",
        "add_relationship",
        "add_measure",
        "evaluate_measure",
        "run_script",
        "list_scripts",
        "open_workbook",
        "save_workbook",
        "new_workbook",
        "undo",
        "redo",
        "screenshot",
        "list_functions",
    ] {
        assert!(names.contains(&want), "missing tool {want}");
    }
    for t in tools {
        assert_eq!(t["inputSchema"]["type"], "object", "{t}");
        assert!(t["description"].as_str().unwrap().len() > 20, "{t}");
        let name = t["name"].as_str().unwrap();
        assert!(name.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{name} is not snake_case");
        check_schema(name, &t["inputSchema"]);
    }
    assert_eq!(tools.len(), tool_definitions().len());
}

#[test]
fn malformed_input_never_panics() {
    let mut s = server();
    assert_eq!(s.handle_line("   "), None);
    let parse = |s: &mut Server, l: &str| -> Value { serde_json::from_str(&s.handle_line(l).unwrap()).unwrap() };
    assert_eq!(err_code(&parse(&mut s, "{not json")), -32700);
    assert_eq!(err_code(&parse(&mut s, "\"just a string\"")), -32600);
    assert_eq!(err_code(&parse(&mut s, "[]")), -32600);
    assert_eq!(err_code(&parse(&mut s, r#"{"jsonrpc":"2.0","id":1}"#)), -32600);
    assert_eq!(err_code(&parse(&mut s, r#"{"jsonrpc":"2.0","id":{"x":1},"method":"ping"}"#)), -32600);
    assert_eq!(err_code(&parse(&mut s, r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":[1,2]}"#)), -32602);
    // Unknown method.
    assert_eq!(err_code(&rpc(&mut s, 1, "nope/nothing", json!({}))), -32601);
    // Bad tools/call params.
    assert_eq!(err_code(&rpc(&mut s, 2, "tools/call", json!({}))), -32602);
    assert_eq!(err_code(&rpc(&mut s, 3, "tools/call", json!({"name": "no_such_tool"}))), -32602);
    assert_eq!(err_code(&rpc(&mut s, 4, "tools/call", json!({"name": "set_cell", "arguments": "x"}))), -32602);
    assert_eq!(err_code(&rpc(&mut s, 5, "tools/call", json!({"name": "set_cell", "arguments": {"cell": "A1"}}))), -32602);
    assert_eq!(err_code(&rpc(&mut s, 6, "tools/call", json!({"name": "set_cell", "arguments": {"cell": 5, "input": "x"}}))), -32602);
    assert_eq!(err_code(&rpc(&mut s, 7, "tools/call", json!({"name": "insert_chart", "arguments": {"range": "A1", "type": "piechart"}}))), -32602);
    assert_eq!(err_code(&rpc(&mut s, 8, "tools/call", json!({"name": "get_cell", "arguments": {"cell": "A1", "bogus": 1}}))), -32602);
    assert_eq!(err_code(&rpc(&mut s, 9, "resources/read", json!({}))), -32602);
    assert_eq!(err_code(&rpc(&mut s, 10, "resources/read", json!({"uri": "cella://nope"}))), -32002);
    // Responses and unknown notifications are ignored.
    assert_eq!(s.handle_line(r#"{"jsonrpc":"2.0","id":7,"result":{}}"#), None);
    assert_eq!(s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/whatever"}"#), None);
    // A batch still gets answers.
    let v = parse(&mut s, r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"jsonrpc":"2.0","method":"notifications/initialized"}]"#);
    assert_eq!(v.as_array().unwrap().len(), 1);
    // Tool-level errors are isError results, not protocol errors.
    assert!(fails(&mut s, "set_cell", json!({"cell": "not a cell!!", "input": "1"})).contains("set_cell"));
    assert!(fails(&mut s, "execute_command", json!({"command": "no.such.command"})).contains("unknown command"));
    fails(&mut s, "read_range", json!({"range": "ZZZZZZZZ99999999999"}));
    fails(&mut s, "format_range", json!({"range": "A1"}));
    fails(&mut s, "sort_range", json!({"keys": []}));
    fails(&mut s, "open_workbook", json!({"path": "/definitely/not/here.xlsx"}));
    fails(&mut s, "save_workbook", json!({}));
    assert!(fails(&mut s, "screenshot", json!({"path": "/tmp/x.png"})).contains("no UI in headless mode"));
}

#[test]
fn every_tool_survives_hostile_arguments() {
    let hostile = [json!({}), json!({"range": "A1:XFD1048576"}), json!({"range": "", "cell": "", "path": "", "formula": "", "command": ""})];
    for t in tool_definitions() {
        let name = t["name"].as_str().unwrap();
        for args in &hostile {
            let mut s = server();
            // Either a protocol error or a tool result; never a panic.
            let _ =
                s.handle_line(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}}).to_string());
        }
    }
}

#[test]
fn resources() {
    let mut s = server();
    let v = rpc(&mut s, 1, "resources/list", json!({}));
    let uris: Vec<&str> = v["result"]["resources"].as_array().unwrap().iter().map(|r| r["uri"].as_str().unwrap()).collect();
    assert_eq!(uris, ["cella://workbook", "cella://commands", "cella://functions"]);
    for uri in uris {
        let v = rpc(&mut s, 2, "resources/read", json!({"uri": uri}));
        let text = v["result"]["contents"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert!(!parsed.is_null(), "{uri}");
    }
    let v = rpc(&mut s, 3, "resources/read", json!({"uri": "cella://functions"}));
    assert!(v["result"]["contents"][0]["text"].as_str().unwrap().contains("VLOOKUP"));
}

#[test]
fn basic_tools() {
    let mut s = server();
    let cmds = ok(&mut s, "list_commands", json!({"search": "bold"}));
    assert!(cmds.as_array().unwrap().iter().any(|c| c["id"] == "home.bold"));
    let all = ok(&mut s, "list_commands", json!({}));
    assert!(all.as_array().unwrap().len() > 100);
    let f = ok(&mut s, "list_functions", json!({"search": "vlookup"}));
    assert!(f.as_array().unwrap().iter().any(|x| x["name"] == "VLOOKUP"));
    assert_eq!(ok(&mut s, "evaluate_formula", json!({"formula": "SUM(1,2,3)"}))["result"], json!(6.0));
    ok(&mut s, "set_cell", json!({"cell": "A1", "input": "5"}));
    // #697: a number for `input` is entered, not rejected or cleared.
    ok(&mut s, "set_cell", json!({"cell": "B1", "input": 42.5}));
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "B1"}))["value"], json!(42.5));
    ok(&mut s, "execute_command", json!({"command": "cell.set", "params": {"cell": "A2", "input": "=A1*2"}}));
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "A2"}))["value"], json!(10.0));
    assert_eq!(ok(&mut s, "evaluate_formula", json!({"formula": "=A1+A2", "cell": "C1"}))["result"], json!(15.0));
    ok(&mut s, "undo", json!({}));
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "A2"}))["value"], Value::Null);
    ok(&mut s, "redo", json!({}));
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "A2"}))["value"], json!(10.0));
    ok(&mut s, "select", json!({"range": "A1:A2"}));
    assert_eq!(ok(&mut s, "inspect_workbook", json!({}))["selection"], "A1:A2");
    ok(&mut s, "new_workbook", json!({"sample": "sales"}));
    let info = ok(&mut s, "inspect_workbook", json!({}));
    assert!(info["sheets"][0]["cells"].as_u64().unwrap() > 10, "{info}");
}

#[test]
fn sort_and_filter() {
    let mut s = server();
    ok(&mut s, "write_range", json!({"range": "A1", "values": [["Name", "Score"], ["b", 2], ["c", 3], ["a", 1]]}));
    ok(&mut s, "sort_range", json!({"range": "A1:B4", "keys": [{"column": "B", "order": "desc"}], "header": true}));
    let v = ok(&mut s, "read_range", json!({"range": "A2:A4"}));
    assert_eq!(v["values"], json!([["c"], ["b"], ["a"]]));
    ok(&mut s, "sort_range", json!({"range": "A1:B4", "keys": ["A"], "header": true}));
    let v = ok(&mut s, "read_range", json!({"range": "A2:A4"}));
    assert_eq!(v["values"], json!([["a"], ["b"], ["c"]]));
    ok(&mut s, "filter", json!({"range": "A1:B4", "column": "A", "values": ["a", "c"]}));
    let info = ok(&mut s, "inspect_workbook", json!({}));
    assert_eq!(info["sheets"][0]["autofilter"], "A1:B4");
}

#[test]
fn save_and_open_roundtrip() {
    let mut s = server();
    ok(&mut s, "write_range", json!({"range": "B2", "values": [[1, 2], [3, "=SUM(B2:C2)+B3"]]}));
    let path = tmp("roundtrip.xlsx");
    let p = path.to_string_lossy().to_string();
    ok(&mut s, "save_workbook", json!({"path": p}));
    ok(&mut s, "save_workbook", json!({}));
    ok(&mut s, "new_workbook", json!({}));
    ok(&mut s, "open_workbook", json!({"path": p}));
    let v = ok(&mut s, "read_range", json!({"range": "B2:C3", "formulas": true}));
    assert_eq!(v["values"], json!([[1.0, 2.0], [3.0, "=SUM(B2:C2)+B3"]]));
    let v = ok(&mut s, "read_range", json!({"range": "C3"}));
    assert_eq!(v["values"], json!([[6.0]]));
    let _ = std::fs::remove_file(path);
}

/// An agent builds a small monthly budget through MCP tool calls only.
#[test]
fn agent_builds_a_budget() {
    let mut s = server();
    rpc(&mut s, 1, "initialize", json!({"protocolVersion": PROTOCOL_VERSION, "capabilities": {}, "clientInfo": {"name": "agent", "version": "1"}}));
    s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);

    ok(
        &mut s,
        "write_range",
        json!({"range": "A1", "values": [
            ["Category", "January", "February", "March"],
            ["Rent", 1200, 1200, 1250],
            ["Groceries", 410.5, 389.25, 402],
            ["Utilities", 150, 162, 140],
            ["Transport", 90, 85, 110],
        ]}),
    );
    ok(&mut s, "set_cell", json!({"cell": "A6", "input": "Total"}));
    for col in ["B", "C", "D"] {
        ok(&mut s, "set_cell", json!({"cell": format!("{col}6"), "input": format!("=SUM({col}2:{col}5)")}));
    }
    ok(&mut s, "set_cell", json!({"cell": "E1", "input": "Quarter"}));
    ok(
        &mut s,
        "write_range",
        json!({"range": "E2", "values": [["=SUM(B2:D2)"], ["=SUM(B3:D3)"], ["=SUM(B4:D4)"], ["=SUM(B5:D5)"], ["=SUM(B6:D6)"]]}),
    );

    let fmt =
        ok(&mut s, "format_range", json!({"range": "A1:E1", "bold": true, "fillColor": "#DDEBF7", "horizontalAlign": "center", "borders": "bottom"}));
    assert_eq!(fmt["applied"].as_array().unwrap().len(), 4, "{fmt}");
    ok(&mut s, "format_range", json!({"range": "B2:E6", "numberFormat": "#,##0.00"}));
    ok(&mut s, "format_range", json!({"range": "A6:E6", "bold": true, "borders": {"preset": "top", "style": "double"}}));

    ok(&mut s, "create_table", json!({"range": "A1:E5", "style": "TableStyleLight9"}));
    ok(&mut s, "insert_chart", json!({"range": "A1:D5", "type": "column", "title": "Monthly spend", "at": "G2"}));

    // Read back computed values.
    let totals = ok(&mut s, "read_range", json!({"range": "B6:E6"}));
    let row = totals["values"][0].as_array().unwrap();
    let nums: Vec<f64> = row.iter().map(|v| v.as_f64().unwrap()).collect();
    assert!((nums[0] - 1850.5).abs() < 1e-9, "{nums:?}");
    assert!((nums[1] - 1836.25).abs() < 1e-9, "{nums:?}");
    assert!((nums[2] - 1902.0).abs() < 1e-9, "{nums:?}");
    assert!((nums[3] - 5588.75).abs() < 1e-9, "{nums:?}");
    let shown = ok(&mut s, "read_range", json!({"range": "E6", "formatted": true}));
    assert_eq!(shown["values"][0][0], "5,588.75");
    let formulas = ok(&mut s, "read_range", json!({"range": "B6", "formulas": true}));
    assert_eq!(formulas["values"][0][0], "=SUM(B2:B5)");
    let cell = ok(&mut s, "get_cell", json!({"cell": "A1"}));
    assert_eq!(cell["style"]["font"]["bold"], true, "{cell}");

    let info = ok(&mut s, "inspect_workbook", json!({}));
    let sheet = &info["sheets"][0];
    assert_eq!(sheet["tables"].as_array().unwrap().len(), 1, "{info}");
    assert_eq!(sheet["tables"][0]["range"], "A1:E5");
    assert_eq!(sheet["charts"].as_array().unwrap().len(), 1, "{info}");
    assert_eq!(sheet["charts"][0]["title"], "Monthly spend");

    // Change an input: totals follow.
    ok(&mut s, "set_cell", json!({"cell": "B2", "input": "1300"}));
    let v = ok(&mut s, "read_range", json!({"range": "E6"}));
    assert!((v["values"][0][0].as_f64().unwrap() - 5688.75).abs() < 1e-9);
    // And the whole thing evaluates without touching the sheet.
    let v = ok(&mut s, "evaluate_formula", json!({"formula": "=AVERAGE(B6:D6)"}));
    assert!((v["result"].as_f64().unwrap() - (1950.5 + 1836.25 + 1902.0) / 3.0).abs() < 1e-9);
}

#[test]
fn new_tools_name_the_command_they_wrap() {
    let wrapped: [(&str, &[&str]); 9] = [
        ("list_queries", &["data.query.list"]),
        (
            "run_query",
            &[
                "data.query.add",
                "data.query.refresh",
                "data.query.addStep",
                "data.query.setStep",
                "data.query.removeStep",
                "data.query.steps",
                "data.query.load",
            ],
        ),
        ("preview_query", &["data.query.preview"]),
        ("list_model", &["data.model.tables", "data.relationship.list", "data.measure.list"]),
        ("add_relationship", &["data.relationship.add"]),
        ("add_measure", &["data.measure.add"]),
        ("evaluate_measure", &["data.measure.evaluate"]),
        ("run_script", &["automate.runScript"]),
        ("list_scripts", &["automate.allScripts"]),
    ];
    let defs = tool_definitions();
    for (name, commands) in wrapped {
        let def = defs.iter().find(|d| d["name"] == name).unwrap_or_else(|| panic!("missing tool {name}"));
        let desc = def["description"].as_str().unwrap_or_default();
        assert!(desc.len() > 40, "{name}: {desc}");
        for id in commands {
            assert!(desc.contains(id), "{name} should name {id}: {desc}");
            assert!(cella_engine::find_command(id).is_some(), "{name} names {id}, which is not a command");
        }
    }
}

// ---------- read_range paging ----------

#[test]
fn read_range_pages_with_offset_and_limit() {
    let mut s = server();
    let rows: Vec<Value> = (1..=20).map(|i| json!([i])).collect();
    ok(&mut s, "write_range", json!({"range": "A1", "values": rows}));
    // A small limit: the window, plus the size of the range it came from.
    let v = ok(&mut s, "read_range", json!({"range": "A1:A20", "limit": 3}));
    assert_eq!(v["values"], json!([[1.0], [2.0], [3.0]]), "{v}");
    assert_eq!(v["rows"].as_u64(), Some(20));
    assert_eq!(v["columns"].as_u64(), Some(1));
    assert_eq!(v["returnedRows"].as_u64(), Some(3));
    assert_eq!((v["offset"].as_u64(), v["limit"].as_u64()), (Some(0), Some(3)));
    assert_eq!(v["truncated"], true, "{v}");
    // The window is the rows after offset, and the last page withholds nothing.
    let v = ok(&mut s, "read_range", json!({"range": "A1:A20", "offset": 18}));
    assert_eq!(v["values"], json!([[19.0], [20.0]]), "{v}");
    assert_eq!((v["returnedRows"].as_u64(), v["truncated"].clone()), (Some(2), json!(false)));
    // A range that fits under the default limit is a full read.
    let v = ok(&mut s, "read_range", json!({"range": "A1:A20"}));
    assert_eq!(v["values"].as_array().map(Vec::len), Some(20));
    assert_eq!((v["limit"].as_u64(), v["truncated"].clone()), (Some(200), json!(false)));
    // Without a range the used range is the one that gets paged.
    let v = ok(&mut s, "read_range", json!({"limit": 2}));
    assert_eq!(v["values"], json!([[1.0], [2.0]]), "{v}");
    assert_eq!((v["rows"].as_u64(), v["truncated"].clone()), (Some(20), json!(true)));
    // A limit above the max is clamped, not refused, and the answer says which limit it used.
    let v = ok(&mut s, "read_range", json!({"range": "A1:A20", "limit": 100000}));
    assert_eq!(v["limit"].as_u64(), Some(5000), "{v}");
    assert_eq!(v["values"].as_array().map(Vec::len), Some(20));
    assert_eq!(v["truncated"], false);
    // An offset past the end is an empty window, not an error.
    let v = ok(&mut s, "read_range", json!({"range": "A1:A20", "offset": 500}));
    assert_eq!(v["values"], json!([]), "{v}");
    assert_eq!(v["range"], "A1:A20");
    assert_eq!((v["rows"].as_u64(), v["returnedRows"].as_u64()), (Some(20), Some(0)));
    assert_eq!(v["truncated"], false);
    // limit 0 answers the shape of a range without reading a cell of it.
    let v = ok(&mut s, "read_range", json!({"range": "A1:A20", "limit": 0}));
    assert_eq!((v["rows"].as_u64(), v["columns"].as_u64()), (Some(20), Some(1)), "{v}");
    assert_eq!((v["values"].clone(), v["returnedRows"].as_u64(), v["truncated"].clone()), (json!([]), Some(0), json!(true)));
    // A window on another sheet keeps that sheet.
    ok(&mut s, "execute_command", json!({"command": "home.insertSheet", "params": {"name": "Two"}}));
    ok(&mut s, "write_range", json!({"range": "Two!B1:B4", "values": [[1], [2], [3], [4]]}));
    let v = ok(&mut s, "read_range", json!({"range": "Two!B1:B4", "offset": 1, "limit": 2}));
    assert_eq!(v["values"], json!([[2.0], [3.0]]), "{v}");
    assert_eq!(v["sheet"], "Two");
    assert_eq!(v["range"], "B1:B4");
    // Whole columns mean the used part of them, as they do for sheet.read.
    let v = ok(&mut s, "read_range", json!({"sheet": "Sheet1", "range": "A:A"}));
    assert_eq!((v["rows"].as_u64(), v["returnedRows"].as_u64()), (Some(20), Some(20)), "{v}");
}

// ---------- batch ----------

#[test]
fn batch_runs_calls_in_order_and_reports_each() {
    let mut s = server();
    let v = ok(
        &mut s,
        "batch",
        json!({"calls": [
            {"tool": "set_cell", "arguments": {"cell": "A1", "input": "7"}},
            {"tool": "set_cell", "arguments": {"cell": "A2", "input": "=A1*3"}},
            {"tool": "get_cell", "arguments": {"cell": "A2"}},
            {"tool": "no_such_tool", "arguments": {}},
            {"tool": "set_cell", "arguments": {"cell": "not a cell!!", "input": "1"}},
            {"tool": "get_cell", "arguments": {"cell": "A2"}},
        ]}),
    );
    assert_eq!((v["count"].as_u64(), v["failed"].as_u64()), (Some(6), Some(2)), "{v}");
    let r = v["results"].as_array().map(Vec::as_slice).unwrap_or_default();
    assert_eq!(r.len(), 6, "{v}");
    // In order: the read sees what the writes before it did.
    assert_eq!((r[2]["ok"].clone(), r[2]["tool"].clone()), (json!(true), json!("get_cell")), "{v}");
    assert_eq!(r[2]["result"]["value"], json!(21.0), "{v}");
    // A failing call is that call's error; the rest still run.
    assert_eq!(r[3]["ok"], false);
    assert!(r[3]["error"].as_str().unwrap_or_default().contains("unknown tool"), "{v}");
    assert_eq!(r[4]["ok"], false);
    assert_eq!(r[5]["ok"], true, "{v}");
    assert_eq!(r[5]["result"]["value"], json!(21.0), "{v}");
    // A call's arguments are validated like any other tool's.
    let v = ok(&mut s, "batch", json!({"calls": [{"tool": "set_cell", "arguments": "nope"}]}));
    assert_eq!(v["failed"].as_u64(), Some(1), "{v}");
}

#[test]
fn batch_refuses_nesting_oversize_and_too_many_calls() {
    let mut s = server();
    // A batch inside a batch: refused as that call's error, and nothing runs.
    let inner = json!({"calls": [{"tool": "evaluate_formula", "arguments": {"formula": "=1+1"}}]});
    let v = ok(&mut s, "batch", json!({"calls": [{"tool": "batch", "arguments": inner}]}));
    assert_eq!(v["failed"].as_u64(), Some(1), "{v}");
    let err = v["results"][0]["error"].as_str().unwrap_or_default().to_string();
    assert!(err.contains("another batch"), "{err}");
    // Over the call cap, the whole batch is refused (not truncated).
    let many: Vec<Value> = (0..51).map(|_| json!({"tool": "get_cell", "arguments": {"cell": "A1"}})).collect();
    let msg = fails(&mut s, "batch", json!({"calls": many}));
    assert!(msg.contains("at most 50"), "{msg}");
    // An empty batch says so.
    assert!(fails(&mut s, "batch", json!({"calls": []})).contains("at least one call"));
    // A megabyte of arguments is refused before anything is run.
    let blob = json!({"calls": [{"tool": "set_cell", "arguments": {"cell": "A1", "input": "x".repeat(300 * 1024)}}]});
    let msg = fails(&mut s, "batch", blob);
    assert!(msg.contains("KB"), "{msg}");
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "A1"}))["value"], Value::Null, "a refused batch runs nothing");
}

// ---------- data platform ----------

#[test]
fn data_platform_tools_build_a_query_and_a_model() {
    let mut s = server();
    // A query from inline CSV, loaded into the model (no cells anywhere).
    let q = ok(
        &mut s,
        "run_query",
        json!({
            "name": "Sales",
            "source": {"kind": "csv", "text": "RegionId,Amount
1,10
2,5
1,7
"},
            "steps": ["PromoteHeaders", {"ChangeType": {"columns": ["RegionId", "Amount"], "to": "Number"}}],
            "load": "model",
        }),
    );
    assert_eq!((q["name"].clone(), q["rows"].as_u64()), (json!("Sales"), Some(3)), "{q}");
    ok(
        &mut s,
        "run_query",
        json!({
            "name": "Region",
            "source": {"kind": "csv", "text": "Id,Name
1,West
2,East
"},
            "steps": ["PromoteHeaders", {"ChangeType": {"columns": ["Id"], "to": "Number"}}],
            "load": "model",
        }),
    );
    // Listing, previewing, and re-running a query the workbook already holds.
    let queries = ok(&mut s, "list_queries", json!({}));
    let names: Vec<&str> = queries.as_array().map(Vec::as_slice).unwrap_or_default().iter().filter_map(|q| q["name"].as_str()).collect();
    assert_eq!(names, ["Sales", "Region"], "{queries}");
    let p = ok(&mut s, "preview_query", json!({"name": "Sales", "limit": 2}));
    assert_eq!((p["total"].as_u64(), p["truncated"].clone()), (Some(3), json!(true)), "{p}");
    assert_eq!(p["columns"].as_array().map(Vec::len), Some(2), "{p}");
    let again = ok(&mut s, "run_query", json!({"name": "Sales"}));
    assert_eq!(again[0]["rows"].as_u64(), Some(3), "{again}");
    // The model: two tables, a relationship between them, a measure over the facts.
    ok(&mut s, "add_relationship", json!({"from": {"table": "Sales", "column": "RegionId"}, "to": {"table": "Region", "column": "Id"}}));
    ok(&mut s, "add_measure", json!({"name": "Total", "table": "Sales", "formula": "SUM(Sales[Amount])"}));
    let m = ok(&mut s, "list_model", json!({}));
    let tables: Vec<&str> = m["tables"].as_array().map(Vec::as_slice).unwrap_or_default().iter().filter_map(|t| t["name"].as_str()).collect();
    assert_eq!(tables, ["Sales", "Region"], "{m}");
    assert_eq!(m["tables"][0]["rows"].as_u64(), Some(3), "{m}");
    assert_eq!((m["relationships"][0]["from_table"].clone(), m["relationships"][0]["to_column"].clone()), (json!("Sales"), json!("Id")), "{m}");
    assert_eq!(m["measures"][0]["formula"], "SUM(Sales[Amount])", "{m}");
    // evaluate_measure answers a number, and a filter on the dimension reaches the fact table.
    let e = ok(&mut s, "evaluate_measure", json!({"name": "Total"}));
    assert_eq!(e["value"], json!(22.0), "{e}");
    let e = ok(&mut s, "evaluate_measure", json!({"name": "Total", "filters": [{"table": "Region", "column": "Name", "value": "West"}]}));
    assert_eq!(e["value"], json!(17.0), "{e}");
    // A measure that names a table the model does not have is refused, not stored.
    assert!(fails(&mut s, "add_measure", json!({"name": "Nope", "formula": "SUM(Nope[Amount])"})).contains("Nope"));
    assert_eq!(ok(&mut s, "list_model", json!({}))["measures"].as_array().map(Vec::len), Some(1), "a refused measure is not kept");
}

// ---------- automate ----------

#[test]
fn run_script_replays_a_script_and_lists_it() {
    let mut s = server();
    let script = json!({"name": "Fill", "commands": [
        {"command": "cell.set", "params": {"cell": "B1", "input": "ran"}},
        {"command": "cell.set", "params": {"cell": "B2", "input": "=LEN(B1)"}},
    ]});
    ok(&mut s, "execute_command", json!({"command": "automate.newScript", "params": script}));
    let list = ok(&mut s, "list_scripts", json!({}));
    let found = list.as_array().map(Vec::as_slice).unwrap_or_default().iter().find(|x| x["name"] == "Fill").unwrap_or_else(|| panic!("{list}"));
    assert_eq!((found["steps"].as_u64(), found["trusted"].clone()), (Some(2), json!(true)), "{list}");
    // Storing a script runs nothing; running it replays the steps in order.
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "B1"}))["value"], Value::Null);
    assert_eq!(ok(&mut s, "run_script", json!({"name": "Fill"}))["ran"].as_u64(), Some(2));
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "B2"}))["value"], json!(3.0));
    assert!(fails(&mut s, "run_script", json!({"name": "nope"})).contains("no script named \"nope\""));
    // A workbook downloaded with scripts in it is untrusted: `trusted: true` runs one once, and the
    // trust is not remembered for the next call.
    let path = tmp("scripts.xlsx");
    ok(&mut s, "save_workbook", json!({"path": path.to_string_lossy()}));
    let bytes = std::fs::read(&path).unwrap();
    ok(&mut s, "new_workbook", json!({}));
    let downloaded = json!({"command": "file.open", "params": {"name": "downloaded.xlsx", "base64": cella_engine::io::base64_encode(&bytes)}});
    ok(&mut s, "execute_command", downloaded);
    assert!(fails(&mut s, "run_script", json!({"name": "Fill"})).contains("trust"));
    assert_eq!(ok(&mut s, "run_script", json!({"name": "Fill", "trusted": true}))["ran"].as_u64(), Some(2));
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "B2"}))["value"], json!(3.0));
    assert!(fails(&mut s, "run_script", json!({"name": "Fill"})).contains("trust"), "trusted is per call");
    let _ = std::fs::remove_file(path);
}

// ---------- serve loop + remote ----------

#[test]
fn serve_loop_over_buffers() {
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "\n",
        "garbage\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"evaluate_formula","arguments":{"formula":"=2*21"}}}"#,
        "\n"
    );
    let mut out = Vec::new();
    let mut s = server();
    s.serve(input.as_bytes(), &mut out).unwrap();
    let lines: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[1]["error"]["code"], -32700);
    assert!(lines[2]["result"]["content"][0]["text"].as_str().unwrap().contains("42"));
}

#[test]
fn cell_set_without_input_is_error() {
    let mut s = server();
    ok(&mut s, "set_cell", json!({"cell": "B2", "input": "keep"}));
    fails(&mut s, "execute_command", json!({"command": "cell.set", "params": {"cell": "B2"}}));
    fails(&mut s, "execute_command", json!({"command": "cell.set", "params": {}}));
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "B2"}))["text"], "keep");
    // An explicit empty string still clears.
    ok(&mut s, "execute_command", json!({"command": "cell.set", "params": {"cell": "B2", "input": ""}}));
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "B2"}))["text"], "");
}

#[test]
fn remote_backend_over_tcp() {
    // A fake app: answers engine.execute by running a headless session; closes after the first
    // request to exercise the reconnect path.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let mut h = Headless::new();
        for (i, stream) in listener.incoming().take(2).enumerate() {
            let stream = stream.unwrap();
            let mut w = stream.try_clone().unwrap();
            let mut r = BufReader::new(stream);
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let req: Value = serde_json::from_str(&line).unwrap();
                let reply = match h.call(req["method"].as_str().unwrap(), req["params"].clone()) {
                    Ok(v) => json!({"id": req["id"], "ok": true, "result": v}),
                    Err(e) => json!({"id": req["id"], "ok": false, "error": e}),
                };
                w.write_all(format!("{reply}\n").as_bytes()).unwrap();
                if i == 0 {
                    break; // drop the first connection
                }
            }
        }
    });
    let addr = control_addr(&port.to_string());
    assert_eq!(addr, format!("127.0.0.1:{port}"));
    let mut s = Server::new(Box::new(Remote::connect(&addr).unwrap()));
    assert_eq!(ok(&mut s, "evaluate_formula", json!({"formula": "=1+1"}))["result"], json!(2.0));
    // Second call reconnects transparently.
    ok(&mut s, "set_cell", json!({"cell": "A1", "input": "7"}));
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "A1"}))["value"], json!(7.0));
    let msg = fails(&mut s, "execute_command", json!({"command": "nope"}));
    assert!(msg.contains("unknown command"), "{msg}");
    drop(s);
    handle.join().unwrap();
    assert!(Remote::connect("127.0.0.1:1").is_err());
}

#[test]
fn file_commands_without_a_file_fail() {
    let mut s = server();
    // `file.save` on Book1, which has no file yet, would ask for one too.
    for command in ["file.open", "file.saveAs", "file.save", "data.getData", "data.fromTextCsv"] {
        let msg = fails(&mut s, "execute_command", json!({"command": command}));
        assert!(msg.contains("missing") && msg.contains("never open dialogs"), "{command}: {msg}");
        fails(&mut s, "execute_command", json!({"command": command, "params": {}}));
    }
}

// ---------- cell references ----------

fn cmd_fails(s: &mut Server, command: &str, params: Value) -> String {
    fails(s, "execute_command", json!({"command": command, "params": params}))
}

fn a1_display(s: &mut Server) -> String {
    ok(s, "get_cell", json!({"cell": "A1"}))["text"].as_str().unwrap_or("").to_string()
}

#[test]
fn set_cell_invalid_ref_is_error() {
    let mut s = server();
    for bad in ["ZZZZ0", "B0", "A", "1", "A1:B2", "not a cell", "", "XFE1", "A1048577"] {
        fails(&mut s, "set_cell", json!({"cell": bad, "input": "x"}));
    }
    assert_eq!(a1_display(&mut s), "");
}

#[test]
fn cell_ref_boundaries_still_work() {
    let mut s = server();
    ok(&mut s, "set_cell", json!({"cell": "A1", "input": "first"}));
    ok(&mut s, "set_cell", json!({"cell": "$B$2", "input": "abs"}));
    ok(&mut s, "set_cell", json!({"cell": "XFD1048576", "input": "last"}));
    ok(&mut s, "set_cell", json!({"cell": "xfd1", "input": "lower"}));
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "B2"}))["text"], "abs");
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "XFD1048576"}))["text"], "last");
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "XFD1"}))["text"], "lower");
    assert_eq!(a1_display(&mut s), "first");
    // Ranges are still ranges for the commands that take them.
    ok(&mut s, "read_range", json!({"range": "A1:B2"}));
    ok(&mut s, "read_range", json!({"range": "$A$1:$B$2"}));
    // The active sheet's own name is an accepted prefix; an absent cell still means the selection.
    ok(&mut s, "set_cell", json!({"cell": "Sheet1!C3", "input": "named"}));
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "C3"}))["text"], "named");
    ok(&mut s, "execute_command", json!({"command": "formulas.tracePrecedents", "params": {}}));
}

#[test]
fn bad_ref_in_command_is_error() {
    let mut s = server();
    ok(&mut s, "set_cell", json!({"cell": "A1", "input": "7"}));
    for (cmd, key) in [("view.freezePanes", "cell"), ("formulas.tracePrecedents", "cell"), ("formulas.traceDependents", "cell"), ("cell.get", "cell")]
    {
        cmd_fails(&mut s, cmd, json!({ key: "B0" }));
        cmd_fails(&mut s, cmd, json!({ key: "nonsense" }));
    }
    assert_eq!(a1_display(&mut s), "7");
}

#[test]
fn other_sheet_prefix_is_error() {
    let mut s = server();
    ok(&mut s, "execute_command", json!({"command": "home.insertSheet", "params": {"name": "Other"}}));
    ok(&mut s, "execute_command", json!({"command": "sheet.activate", "params": {"sheet": 0}}));
    fails(&mut s, "set_cell", json!({"cell": "Nope!B2", "input": "x"}));
    // A command that cannot honour a sheet prefix must not drop it and use the active sheet.
    cmd_fails(&mut s, "formulas.tracePrecedents", json!({"cell": "Other!B2"}));
    ok(&mut s, "set_cell", json!({"cell": "Sheet1!B2", "input": "here"}));
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "B2"}))["text"], "here");
    // cell.set resolves another existing sheet itself and writes there, not to the active sheet.
    ok(&mut s, "set_cell", json!({"cell": "Other!B3", "input": "there"}));
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "B3"}))["text"], "");
    let r = ok(&mut s, "read_range", json!({"range": "Other!B3"}));
    assert!(r.to_string().contains("there"), "{r}");
}

#[test]
fn cross_sheet_cell_refs_resolve() {
    let mut s = server();
    ok(&mut s, "execute_command", json!({"command": "home.insertSheet", "params": {"name": "Other"}}));
    ok(&mut s, "execute_command", json!({"command": "home.insertSheet", "params": {"name": "My Sheet"}}));
    ok(&mut s, "execute_command", json!({"command": "sheet.activate", "params": {"sheet": 0}}));
    ok(&mut s, "set_cell", json!({"cell": "Other!B3", "input": "there"}));
    ok(&mut s, "set_cell", json!({"cell": "'My Sheet'!A1", "input": "quoted"}));
    // Reads honour the prefix.
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "Other!B3"}))["text"], "there");
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "'My Sheet'!A1"}))["text"], "quoted");
    assert_eq!(ok(&mut s, "get_cell", json!({"cell": "B3"}))["text"], "");
    // Cell-level commands that resolve their own sheet land on it.
    ok(&mut s, "execute_command", json!({"command": "review.newComment", "params": {"cell": "Other!B3", "text": "note"}}));
    let c = ok(&mut s, "get_cell", json!({"cell": "Other!B3"}));
    assert!(c.to_string().contains("note"), "{c}");
    let c = ok(&mut s, "get_cell", json!({"cell": "B3"}));
    assert!(!c.to_string().contains("note"), "{c}");
    // The active sheet may be a quoted name with spaces.
    ok(&mut s, "execute_command", json!({"command": "sheet.activate", "params": {"sheet": "My Sheet"}}));
    ok(&mut s, "execute_command", json!({"command": "view.freezePanes", "params": {"cell": "'My Sheet'!B2"}}));
    // Freeze Panes honours another sheet's prefix (or `sheet`) and leaves the active sheet alone.
    ok(&mut s, "execute_command", json!({"command": "view.freezePanes", "params": {"cell": "Other!C4"}}));
    let sheets = ok(&mut s, "inspect_workbook", json!({}))["sheets"].clone();
    assert_eq!((sheets[1]["freeze"].clone(), sheets[2]["freeze"].clone()), (json!([3, 2]), json!([1, 1])), "{sheets}");
    // A prefix naming a different sheet than the `sheet` argument is an error, not resolved in favour of either.
    cmd_fails(&mut s, "cell.get", json!({"cell": "Other!B3", "sheet": "Sheet1"}));
}

#[test]
fn text_to_columns_destination_follows_its_sheet() {
    let mut s = server();
    ok(&mut s, "execute_command", json!({"command": "home.insertSheet", "params": {"name": "Other"}}));
    ok(&mut s, "execute_command", json!({"command": "sheet.activate", "params": {"sheet": 0}}));
    ok(&mut s, "set_cell", json!({"cell": "Other!A1", "input": "a,b"}));
    ok(
        &mut s,
        "execute_command",
        json!({"command": "data.textToColumns", "params": {"range": "Other!A1", "delimiters": [","], "destination": "Other!D1"}}),
    );
    let r = ok(&mut s, "read_range", json!({"range": "Other!D1:E1"}));
    assert!(r.to_string().contains('a') && r.to_string().contains('b'), "{r}");
    // An unparsable destination and a prefix that contradicts the command's sheet are errors, not A1.
    cmd_fails(&mut s, "data.textToColumns", json!({"range": "Other!A1", "delimiters": [","], "destination": "D0"}));
    cmd_fails(&mut s, "data.textToColumns", json!({"range": "Other!A1", "delimiters": [","], "destination": "Sheet1!D1"}));
}

#[test]
fn invalid_borders_are_invalid_params() {
    let mut s = server();
    let bad = |s: &mut Server, id: u64, borders: Value| {
        let v = rpc(s, id, "tools/call", json!({"name": "format_range", "arguments": {"range": "A1", "borders": borders}}));
        assert_eq!(err_code(&v), -32602, "{v}");
    };
    bad(&mut s, 1, json!(123));
    bad(&mut s, 2, json!("bogus"));
    bad(&mut s, 3, json!({"preset": "bogus"}));
    bad(&mut s, 4, json!({"style": "thick"}));
    ok(&mut s, "format_range", json!({"range": "A1", "borders": "all"}));
    ok(&mut s, "format_range", json!({"range": "B1", "borders": {"preset": "all", "style": "thick", "color": "#123456"}}));
}

#[test]
fn invalid_sort_keys_are_invalid_params() {
    let mut s = server();
    ok(&mut s, "write_range", json!({"range": "A1", "values": [["b"], ["c"], ["a"]]}));
    for (id, keys) in [(1, json!([123])), (2, json!([{"order": "desc"}])), (3, json!([{"column": "A", "order": "sideways"}]))] {
        let v = rpc(&mut s, id, "tools/call", json!({"name": "sort_range", "arguments": {"range": "A1:A3", "keys": keys}}));
        assert_eq!(err_code(&v), -32602, "{v}");
    }
    ok(&mut s, "sort_range", json!({"range": "A1:A3", "keys": ["A", {"column": "A", "order": "desc"}]}));
}
