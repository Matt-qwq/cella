//! The straggler commands adopted from upstream: the small ids that were missing, plus the guard
//! that keeps the defined-name store (`cella_engine::cmd::stash`) whole when a caller names one
//! of its chunks in a `formulas.*` name command.
//!
//! Every id here runs with no params (or its minimal ones), does what it says, appears in
//! `app.commands` with its ribbon path, and survives undo/redo where it edits the workbook.

use cella_engine::Session;
use cella_engine::cmd::stash;
use cella_engine::core::CellRef;
use serde_json::{Value as Json, json};

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn run(s: &mut Session, cmd: &str, params: Json) -> Json {
    s.execute(cmd, params).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn text(s: &Session, at: &str) -> String {
    let d = s.doc().expect("a document");
    d.wb.active().expect("a sheet").value(CellRef::parse(at).expect("a cell")).display()
}

fn sheet(s: &Session) -> cella_engine::model::Sheet {
    s.doc().expect("a document").wb.active().expect("a sheet").clone()
}

/// The command's entry in `app.commands`, which is what a script and the ribbon read.
fn listed(s: &mut Session, id: &str) -> Json {
    let all = run(s, "app.commands", json!({}));
    all.as_array().and_then(|a| a.iter().find(|c| c["id"] == json!(id))).cloned().unwrap_or_else(|| panic!("{id} is not in app.commands"))
}

/// Four values in A1:A4, so a move has something to carry and something to leave behind.
fn blocks() -> Session {
    let mut s = session();
    run(&mut s, "range.setValues", json!({"range": "A1", "values": [[1], [2], [3], [4]]}));
    run(&mut s, "selection.set", json!({"range": "A1:A4"}));
    s
}

#[test]
fn range_move_carries_the_cells_and_the_references_that_pointed_at_them() {
    let mut s = blocks();
    run(&mut s, "cell.set", json!({"cell": "B1", "input": "=A2"}));
    let r = run(&mut s, "range.move", json!({"range": "A1:A4", "to": "D1"}));
    assert_eq!(r["pasted"], json!("D1:D4"), "the selection follows the moved cells");
    assert_eq!(text(&s, "D1"), "1");
    assert_eq!(text(&s, "D4"), "4");
    assert_eq!(text(&s, "A1"), "", "the source is emptied");
    assert_eq!(text(&s, "B1"), "2", "a reference to a moved cell follows it");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(text(&s, "A1"), "1", "one undo step puts it back");
    assert_eq!(text(&s, "B1"), "2");
    assert_eq!(text(&s, "D1"), "");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(text(&s, "D1"), "1");
    assert_eq!(text(&s, "A1"), "");
}

#[test]
fn range_move_asks_before_replacing_data_unless_told_to() {
    let mut s = blocks();
    run(&mut s, "cell.set", json!({"cell": "C2", "input": "keep me"}));
    let e = s.execute("range.move", json!({"range": "A1:A4", "to": "C1"})).expect_err("data is in the way");
    assert!(e.to_string().contains("overwrite"), "{e}");
    assert_eq!(text(&s, "A1"), "1", "a refused move moves nothing");
    assert_eq!(text(&s, "C2"), "keep me");
    run(&mut s, "range.move", json!({"range": "A1:A4", "to": "C1", "overwrite": true}));
    assert_eq!(text(&s, "C1"), "1");
    assert_eq!(text(&s, "C2"), "2", "the moved cells replaced what was there");
    // Without `to` the command says what it needs instead of guessing.
    let e = s.execute("range.move", json!({"range": "A1"})).expect_err("`to` is required");
    assert!(e.to_string().contains("to"), "{e}");
}

#[test]
fn next_corner_walks_the_corners_of_the_selection_clockwise() {
    let mut s = session();
    run(&mut s, "selection.set", json!({"range": "A1:C3"}));
    let seen: Vec<String> =
        (0..4).map(|_| run(&mut s, "selection.nextCorner", json!({}))["active"].as_str().unwrap_or_default().to_string()).collect();
    assert_eq!(seen, ["C1", "C3", "A3", "A1"], "clockwise from the top-left, then round again");
    assert_eq!(run(&mut s, "selection.nextCorner", json!({}))["selection"], json!("A1:C3"), "the area stays selected");
    // A single cell has no other corner to go to.
    run(&mut s, "selection.set", json!({"range": "B2"}));
    assert_eq!(run(&mut s, "selection.nextCorner", json!({}))["active"], json!("B2"));
}

#[test]
fn paste_name_lists_the_names_and_asks_the_ui_to_insert_one() {
    let mut s = session();
    run(&mut s, "formulas.defineName", json!({"name": "Rate", "refersTo": "=0.07"}));
    let r = run(&mut s, "formulas.pasteName", json!({}));
    assert_eq!(r["names"], json!([{"name": "Rate", "refersTo": "=0.07"}]));
    assert_eq!(text(&s, "A1"), "", "listing the names writes nothing");
    s.take_ui_requests();
    run(&mut s, "formulas.pasteName", json!({"name": "rate"}));
    assert!(format!("{:?}", s.take_ui_requests()).contains("InsertName(\"Rate\")"), "the name is pasted as listed, in its own case");
    assert_eq!(text(&s, "A1"), "", "the cell changes only when the edit is entered");
    assert!(s.execute("formulas.pasteName", json!({"name": "NoSuchName"})).is_err());
    // Paste List writes each name and what it refers to, as text.
    run(&mut s, "selection.set", json!({"range": "D2"}));
    let r = run(&mut s, "formulas.pasteName", json!({"list": true}));
    assert_eq!(r["range"], json!("D2:E2"));
    assert_eq!(text(&s, "D2"), "Rate");
    assert_eq!(text(&s, "E2"), "=0.07");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(text(&s, "D2"), "");
}

#[test]
fn trace_error_follows_the_error_back_to_where_it_starts() {
    let mut s = session();
    run(&mut s, "cell.set", json!({"cell": "B1", "input": "0"}));
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "=B1/0"}));
    run(&mut s, "cell.set", json!({"cell": "A2", "input": "=A1+1"}));
    let r = run(&mut s, "formulas.traceError", json!({"cell": "A2"}));
    assert_eq!(r["error"], json!("#DIV/0!"));
    assert_eq!(r["source"], json!({"sheet": "Sheet1", "cell": "A1"}), "the error starts in A1");
    assert_eq!(r["truncated"], json!(false));
    let arrows = r["arrows"].as_array().cloned().unwrap_or_default();
    assert!(arrows.iter().any(|a| a["kind"] == json!("error") && a["from"]["range"] == json!("A1") && a["to"]["cell"] == json!("A2")), "{arrows:?}");
    assert!(arrows.iter().any(|a| a["kind"] == json!("precedent") && a["from"]["range"] == json!("B1")), "{arrows:?}");
    // A cell that holds no error has no trace.
    run(&mut s, "cell.set", json!({"cell": "C1", "input": "1"}));
    let r = run(&mut s, "formulas.traceError", json!({"cell": "C1"}));
    assert_eq!(r["error"], Json::Null);
    assert_eq!(r["source"], Json::Null);
    assert_eq!(r["arrows"], json!([]));
}

#[test]
fn pivot_options_report_and_set_the_options_the_model_holds() {
    let mut s = session();
    let rows = json!([["Region", "Sales"], ["East", 100], ["West", 50], ["East", 25]]);
    run(&mut s, "range.setValues", json!({"range": "A1", "values": rows}));
    run(&mut s, "selection.set", json!({"range": "A1"}));
    run(&mut s, "insert.pivotTable", json!({"name": "PT"}));
    let before = run(&mut s, "pivot.options", json!({"pivot": "PT"}));
    assert_eq!(before["layout"], json!("compact"));
    assert_eq!(before["subtotals"], json!(true));
    assert_eq!(before["grandTotalsRows"], json!(true));
    let r = run(&mut s, "pivot.options", json!({"pivot": "PT", "layout": "tabular", "subtotals": "none", "showHeaders": false}));
    assert_eq!(r["layout"], json!("tabular"));
    assert_eq!(r["subtotals"], json!(false));
    assert_eq!(r["showHeaders"], json!(false));
    run(&mut s, "edit.undo", json!({}));
    let back = run(&mut s, "pivot.options", json!({"pivot": "PT"}));
    assert_eq!(back["layout"], json!("compact"), "one undo step takes the options back");
    assert_eq!(back["subtotals"], json!(true));
    assert!(s.execute("pivot.options", json!({"pivot": "PT", "layout": "diagonal"})).is_err());
}

#[test]
fn outline_settings_store_the_summary_side_on_the_sheet() {
    let mut s = session();
    assert_eq!(run(&mut s, "data.outlineSettings", json!({})), json!({"summaryBelow": true, "summaryRight": true}));
    assert_eq!(run(&mut s, "data.outlineSettings", json!({"summaryBelow": false})), json!({"summaryBelow": false, "summaryRight": true}));
    assert!(!sheet(&s).summary_below);
    run(&mut s, "edit.undo", json!({}));
    assert!(sheet(&s).summary_below, "a settings change is an edit");
    assert_eq!(run(&mut s, "data.outlineSettings", json!({"summaryRight": false})), json!({"summaryBelow": true, "summaryRight": false}));
}

#[test]
fn the_view_flags_toggle_and_survive_undo() {
    let mut s = session();
    assert_eq!(run(&mut s, "view.showSheetTabs", json!({}))["on"], json!(false));
    assert!(!s.doc().expect("a document").wb.view.show_sheet_tabs);
    assert_eq!(run(&mut s, "view.showSheetTabs", json!({"on": true}))["on"], json!(true));
    assert!(s.doc().expect("a document").wb.view.show_sheet_tabs);
    assert_eq!(run(&mut s, "view.showHorizontalScrollBar", json!({}))["on"], json!(false));
    assert!(!s.doc().expect("a document").wb.view.show_horizontal_scroll);
    assert_eq!(run(&mut s, "view.showVerticalScrollBar", json!({}))["on"], json!(false));
    assert!(!s.doc().expect("a document").wb.view.show_vertical_scroll);
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc().expect("a document").wb.view.show_vertical_scroll, "a view flag is an edit");
    assert_eq!(run(&mut s, "view.showOutlineSymbols", json!({}))["on"], json!(false));
    assert!(!sheet(&s).show_outline_symbols);
    assert_eq!(run(&mut s, "view.showOutlineSymbols", json!({"on": true}))["on"], json!(true));
}

#[test]
fn window_list_names_the_documents_windows_and_feeds_window_activate() {
    let mut s = session();
    let first = run(&mut s, "window.list", json!({}));
    assert_eq!(first.as_array().map(Vec::len), Some(1), "{first}");
    assert_eq!(first[0]["index"], json!(0));
    assert_eq!(first[0]["active"], json!(true));
    assert_eq!(first[0]["hidden"], json!(false));
    assert!(first[0]["title"].is_string());
    let window = first[0]["window"].as_u64().unwrap_or_default();
    run(&mut s, "view.newWindow", json!({}));
    let two = run(&mut s, "window.list", json!({}));
    assert_eq!(two.as_array().map(Vec::len), Some(2), "{two}");
    assert_eq!(two[1]["index"], json!(1));
    let windows = two.as_array().cloned().unwrap_or_default();
    assert!(windows.iter().any(|w| w["window"] == json!(window)), "the first window is still listed");
    run(&mut s, "window.activate", json!({"index": 0}));
    let back = run(&mut s, "window.list", json!({}));
    assert_eq!(back[0]["active"], json!(true), "`index` is what window.activate takes");
}

#[test]
fn every_straggler_is_a_registered_command_with_its_ribbon_path() {
    let mut s = session();
    for (id, menu) in [
        ("range.move", Vec::new()),
        ("selection.nextCorner", Vec::new()),
        ("formulas.pasteName", vec!["Formulas", "Defined Names"]),
        ("formulas.traceError", vec!["Formulas", "Formula Auditing"]),
        ("pivot.options", vec!["PivotTable Analyze", "PivotTable"]),
        ("data.outlineSettings", vec!["Data", "Outline"]),
        ("view.showHorizontalScrollBar", Vec::new()),
        ("view.showVerticalScrollBar", Vec::new()),
        ("view.showSheetTabs", Vec::new()),
        ("view.showOutlineSymbols", Vec::new()),
        ("window.list", vec!["Window"]),
    ] {
        let c = listed(&mut s, id);
        assert_eq!(c["menu"], json!(menu), "{id}");
        assert!(!c["params"].as_str().unwrap_or_default().is_empty(), "{id} has no params doc");
        assert_eq!(c["enabled"], json!(true), "{id}");
    }
    assert_eq!(listed(&mut s, "selection.nextCorner")["shortcut"], json!("Ctrl+."));
    assert_eq!(listed(&mut s, "view.showOutlineSymbols")["shortcut"], json!("Ctrl+8"));
    assert_eq!(listed(&mut s, "formulas.pasteName")["shortcut"], json!("F3"));
}

#[test]
fn no_straggler_opens_a_dialog_or_panics_with_empty_params() {
    let mut s = blocks();
    // `range.move` needs `to`: a clear error, not a dialog.
    assert!(s.execute("range.move", json!({})).is_err());
    // No PivotTable here: a clear error.
    assert!(s.execute("pivot.options", json!({})).is_err());
    for id in ["selection.nextCorner", "formulas.pasteName", "formulas.traceError", "data.outlineSettings", "window.list"] {
        s.execute(id, json!({})).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    for id in ["view.showSheetTabs", "view.showHorizontalScrollBar", "view.showVerticalScrollBar", "view.showOutlineSymbols"] {
        s.execute(id, json!({})).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    assert!(s.take_ui_requests().is_empty(), "no command asked for a dialog");
}

#[test]
fn the_name_commands_refuse_a_chunk_of_the_store() {
    let mut s = session();
    // A script stores its steps in the defined-name store: `_gc_scripts_0`, `_gc_scripts_1`, …
    let steps = json!([{"command": "cell.set", "params": {"cell": "A1", "input": "1"}}]);
    run(&mut s, "automate.newScript", json!({"name": "One", "commands": steps}));
    let held = stash::get(&s.doc().expect("a document").wb, "scripts").expect("the script is stored");
    assert!(s.doc().expect("a document").wb.names.iter().any(|n| n.name == "_gc_scripts_0"), "the store uses _gc_ names");
    // Deleting a chunk is refused, and the store still reads back what it held.
    let e = s.execute("formulas.deleteName", json!({"name": "_gc_scripts_0"})).expect_err("a store chunk is refused");
    let msg = e.to_string();
    assert!(msg.contains("store"), "the message names the store: {msg}");
    assert!(msg.contains("force: true"), "it says how to go ahead anyway: {msg}");
    assert_eq!(stash::get(&s.doc().expect("a document").wb, "scripts").as_deref(), Some(held.as_str()));
    // Defining or renaming onto it is refused too, in either the new or the old name.
    assert!(s.execute("formulas.defineName", json!({"name": "_gc_scripts_0", "refersTo": "=1"})).is_err());
    assert!(
        s.execute("formulas.defineName", json!({"name": "Mine", "refersTo": "=1", "oldName": "_gc_scripts_0"})).is_err(),
        "a rename of a chunk would leave the rest of the item behind"
    );
    assert_eq!(stash::get(&s.doc().expect("a document").wb, "scripts").as_deref(), Some(held.as_str()));
    // Create from Selection leaves such a label out instead of writing it.
    run(&mut s, "cell.set", json!({"cell": "C1", "input": "_gc_scripts_0"}));
    run(&mut s, "cell.set", json!({"cell": "C2", "input": "7"}));
    let r = run(&mut s, "formulas.createFromSelection", json!({"range": "C1:D2", "top": false, "left": true}));
    assert_eq!(r["skipped"], json!(["_gc_scripts_0"]));
    assert_eq!(stash::get(&s.doc().expect("a document").wb, "scripts").as_deref(), Some(held.as_str()));
    // `force: true` is the way through, for someone who does mean it.
    run(&mut s, "formulas.deleteName", json!({"name": "_gc_scripts_0", "force": true}));
}

#[test]
fn legacy_underscore_sc_names_are_left_alone() {
    // Older items are one `_sc_<key>` name each, and Cella's own fixtures define those through
    // `formulas.defineName`: refusing them would break them, and one name is a whole item, so
    // deleting it loses nothing of another one.
    let mut s = session();
    run(&mut s, "formulas.defineName", json!({"name": "_sc_scripts", "refersTo": "=1"}));
    run(&mut s, "formulas.defineName", json!({"name": "_sc_scripts", "refersTo": "=2"}));
    run(&mut s, "formulas.deleteName", json!({"name": "_sc_scripts"}));
    assert!(!s.doc().expect("a document").wb.names.iter().any(|n| n.name == "_sc_scripts"));
}
