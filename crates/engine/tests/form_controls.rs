//! Form controls: insert each kind with empty params, list them, set a slider's value and see it in
//! its cell, press a button, and refuse a button's action in a workbook that came from a file until
//! its scripts are trusted. Undo and redo take back one press, and one value, as one step.

use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::display::cell_text;
use serde_json::{Value as Json, json};

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn run(s: &mut Session, id: &str, params: Json) -> Json {
    s.execute(id, params).unwrap_or_else(|e| panic!("{id} failed: {e}"))
}

fn fails(s: &mut Session, id: &str, params: Json) -> String {
    match s.execute(id, params) {
        Ok(v) => panic!("{id} should have failed, returned {v}"),
        Err(e) => e.to_string(),
    }
}

fn text(s: &Session, at: &str) -> String {
    let d = s.doc().expect("document");
    let sheet = d.wb.active().expect("sheet");
    cell_text(&d.wb, sheet, CellRef::parse(at).expect("a cell"))
}

fn controls(s: &mut Session) -> Vec<Json> {
    run(s, "control.list", json!({})).as_array().cloned().unwrap_or_default()
}

fn control_count(s: &Session) -> usize {
    s.doc().expect("doc").wb.active().map_or(0, |sh| sh.controls.len())
}

/// Each kind goes in with no parameters at all (`insert.slider` with nothing but its defaults), and
/// the three are listed with the numbers they hold.
#[test]
fn every_kind_is_inserted_with_empty_params_and_listed() {
    let mut s = session();
    let button = run(&mut s, "insert.button", json!({}));
    let slider = run(&mut s, "insert.slider", json!({}));
    let spin = run(&mut s, "insert.spinButton", json!({}));
    let id = |v: &Json| v["control"].as_u64().unwrap_or(0);
    assert!(id(&button) > 0 && id(&slider) != id(&button) && id(&spin) != id(&slider), "{button} {slider} {spin}");

    let list = controls(&mut s);
    assert_eq!(list.len(), 3, "{list:?}");
    let kinds: Vec<&str> = list.iter().filter_map(|c| c["kind"].as_str()).collect();
    assert_eq!(kinds, ["button", "slider", "spinButton"], "{list:?}");
    // A new control sits at the active cell, with its kind's own size.
    assert_eq!(list[0]["at"], "A1");
    assert_eq!((list[1]["min"].as_u64(), list[1]["max"].as_u64(), list[1]["value"].as_u64()), (Some(0), Some(100), Some(0)));
    assert_eq!(list[1]["step"].as_u64(), Some(1));
    assert_eq!(list[2]["kind"], "spinButton");
    assert_eq!(list[0]["text"], format!("Button {}", id(&button)));
    // No action, no link, until one is given.
    assert!(list[0].get("action").is_none(), "{list:?}");
    assert!(list[1].get("link").is_none(), "{list:?}");
    assert_eq!(control_count(&s), 3);
}

/// A slider's value is written to its linked cell through `cell.set`, so the cell's dependents see
/// it; the value is clamped and snapped to the step; and Undo takes the value and the cell back as
/// one step.
#[test]
fn a_value_goes_to_the_linked_cell_and_undo_takes_both_back() {
    let mut s = session();
    run(&mut s, "cell.set", json!({"cell": "B1", "input": "=C1*2"}));
    run(&mut s, "cell.set", json!({"cell": "C1", "input": "1"}));
    let v = run(&mut s, "insert.slider", json!({"at": "B3", "link": "C1", "min": 0, "max": 100, "step": 10, "value": 20}));
    let id = v["control"].as_u64().unwrap_or(0);
    assert_eq!(text(&s, "C1"), "20", "a given value goes to the linked cell");
    assert_eq!(text(&s, "B1"), "40", "and its dependents recalculate");
    assert_eq!(controls(&mut s)[0]["link"], "$C$1", "the link is stored as an absolute reference");

    // Snapped to the step, and past the maximum clamps.
    let r = run(&mut s, "control.setValue", json!({"id": id, "value": 63}));
    assert_eq!(r["value"].as_u64(), Some(60), "{r}");
    assert_eq!(text(&s, "C1"), "60");
    let r = run(&mut s, "control.setValue", json!({"id": id, "value": 900}));
    assert_eq!(r["value"].as_u64(), Some(100), "{r}");
    assert_eq!(text(&s, "C1"), "100");
    assert_eq!(controls(&mut s)[0]["value"].as_u64(), Some(100));

    // One Undo takes the last value back, on the control and in the cell.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(text(&s, "C1"), "60");
    assert_eq!(controls(&mut s)[0]["value"].as_u64(), Some(60));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(text(&s, "C1"), "100");
    assert_eq!(controls(&mut s)[0]["value"].as_u64(), Some(100));
}

/// Editing the linked cell moves the control: the number the cell holds is the control's value,
/// clamped and snapped.
#[test]
fn editing_the_linked_cell_moves_the_control() {
    let mut s = session();
    run(&mut s, "insert.spinButton", json!({"at": "B3", "link": "A1", "min": 0, "max": 100, "step": 5}));
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "37"}));
    assert_eq!(controls(&mut s)[0]["value"].as_u64(), Some(35), "snapped to the step");
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "1000"}));
    assert_eq!(controls(&mut s)[0]["value"].as_u64(), Some(100), "clamped to the maximum");
}

/// A button assigned a command runs it when it is pressed, as one undo step.
#[test]
fn a_button_runs_its_command() {
    let mut s = session();
    let v = run(&mut s, "insert.button", json!({"text": "Fill", "at": "D4"}));
    let id = v["control"].as_u64().unwrap_or(0);
    run(&mut s, "control.assign", json!({"id": id, "command": "range.setValues", "params": {"range": "A1:B1", "values": [["Qty", "Price"]]}}));
    assert_eq!(text(&s, "A1"), "", "assigning runs nothing");
    let out = run(&mut s, "control.press", json!({"id": id}));
    assert_eq!(out["cells"].as_u64(), Some(2), "the command's own reply: {out}");
    assert_eq!((text(&s, "A1"), text(&s, "B1")), ("Qty".to_string(), "Price".to_string()));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(text(&s, "A1"), "", "the press is one step, taken back whole");
    assert_eq!(run(&mut s, "control.list", json!({}))[0]["action"]["type"], "command");
}

/// A button assigned a script runs it when it is pressed.
#[test]
fn a_button_runs_its_script() {
    let mut s = session();
    run(&mut s, "automate.newScript", json!({"name": "Stamp"}));
    run(&mut s, "automate.appendStep", json!({"name": "Stamp", "command": "cell.set", "params": {"cell": "B2", "input": "done"}}));
    let v = run(&mut s, "insert.button", json!({}));
    let id = v["control"].as_u64().unwrap_or(0);
    run(&mut s, "control.assign", json!({"id": id, "macro": "Stamp"}));
    run(&mut s, "control.press", json!({"id": id}));
    assert_eq!(text(&s, "B2"), "done");
}

/// A workbook that came from a file with a button on it runs nothing until its scripts are trusted
/// in this session: that is the script policy's `Ask`, refused with the reason.
#[test]
fn a_button_in_an_untrusted_workbook_runs_nothing() {
    let mut s = session();
    let v = run(&mut s, "insert.button", json!({}));
    let id = v["control"].as_u64().unwrap_or(0);
    run(&mut s, "control.assign", json!({"id": id, "command": "cell.set", "params": {"cell": "A1", "input": "ran"}}));
    // The same workbook opened from a file: automation arrived with it, so it starts untrusted.
    s.doc_mut().expect("document").scripts_trusted = false;
    let why = fails(&mut s, "control.press", json!({"id": id}));
    assert!(why.contains("trust") && why.contains("automate.trustScripts"), "{why}");
    assert_eq!(text(&s, "A1"), "", "nothing ran");
    // Trusting the workbook allows it.
    run(&mut s, "automate.trustScripts", json!({"trusted": true}));
    run(&mut s, "control.press", json!({"id": id}));
    assert_eq!(text(&s, "A1"), "ran");
}

/// What a control can't do is refused with a message, not a crash: a button has no value, a slider
/// can't be pressed, an empty `id` names nothing, and the numbers of a slider or spin button have
/// rules.
#[test]
fn what_a_control_cannot_do_is_refused() {
    let mut s = session();
    let b = run(&mut s, "insert.button", json!({}))["control"].as_u64().unwrap_or(0);
    let sl = run(&mut s, "insert.slider", json!({}))["control"].as_u64().unwrap_or(0);
    assert!(fails(&mut s, "control.setValue", json!({"id": b, "value": 1})).contains("button has no value"));
    assert!(fails(&mut s, "control.press", json!({"id": sl})).contains("only buttons can be pressed"));
    assert!(fails(&mut s, "control.press", json!({"id": b})).contains("has no action"));
    assert!(fails(&mut s, "control.setValue", json!({"id": 9999, "value": 1})).contains("no control 9999"));
    assert!(fails(&mut s, "control.setValue", json!({"id": sl})).contains("must be a number"));
    assert!(fails(&mut s, "control.format", json!({"id": sl, "min": 50, "max": 10})).contains("greater than"));
    assert!(fails(&mut s, "insert.slider", json!({"min": 0, "max": 40_000})).contains("30000"));
    assert!(fails(&mut s, "insert.slider", json!({"link": "not a cell"})).contains("isn't a cell of this workbook"));
    assert!(fails(&mut s, "control.assign", json!({"id": b, "command": "no.suchCommand"})).contains("there is no command"));
    assert!(fails(&mut s, "insert.button", json!({"command": "link.open", "params": {"url": "https://e/x"}})).contains("web page"));
    // Nothing was left behind by the refusals.
    assert_eq!(control_count(&s), 2);
}

/// `control.format` changes what a control shows and holds, as one undo step.
#[test]
fn formatting_a_control_is_one_step() {
    let mut s = session();
    let id = run(&mut s, "insert.button", json!({}))["control"].as_u64().unwrap_or(0);
    let out = run(&mut s, "control.format", json!({"id": id, "text": "Total", "name": "Total Button"}));
    assert_eq!((out["text"].as_str(), out["name"].as_str()), (Some("Total"), Some("Total Button")));
    run(&mut s, "edit.undo", json!({}));
    let list = controls(&mut s);
    assert_ne!(list[0]["text"], "Total", "{list:?}");
}

/// A slider's link to a cell of another sheet is stored with the sheet's name, and the value goes
/// to that cell.
#[test]
fn a_link_to_another_sheet_is_named_and_written() {
    let mut s = session();
    run(&mut s, "home.insertSheet", json!({"name": "My Sheet"}));
    // The control lives on the first sheet; its link names the other one.
    run(&mut s, "sheet.activate", json!({"sheet": 0}));
    let id = run(&mut s, "insert.slider", json!({"link": "'My Sheet'!D2", "min": 0, "max": 20, "value": 7}))["control"].as_u64().unwrap_or(0);
    let list = controls(&mut s);
    assert_eq!(list[0]["link"], "'My Sheet'!$D$2", "{list:?}");
    let d = s.doc().expect("document");
    let si = d.wb.sheet_index("My Sheet").expect("sheet");
    assert_eq!(cell_text(&d.wb, d.wb.sheet(si).expect("sheet"), CellRef::parse("D2").expect("cell")), "7");
    run(&mut s, "control.setValue", json!({"id": id, "value": 12}));
    let d = s.doc().expect("document");
    let si = d.wb.sheet_index("My Sheet").expect("sheet");
    assert_eq!(cell_text(&d.wb, d.wb.sheet(si).expect("sheet"), CellRef::parse("D2").expect("cell")), "12");
}
