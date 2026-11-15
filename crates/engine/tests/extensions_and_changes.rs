//! Extensions (named command sequences) and "what changed": the two surfaces an agent needs from
//! the engine to answer for a workbook.

use gridcraft_core::{CellRef, Value};
use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn run(s: &mut Session, cmd: &str, params: Json) -> Json {
    s.execute(cmd, params).unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

#[test]
fn an_extension_is_a_named_sequence_of_commands_that_runs_as_one() {
    let mut s = session();
    run(
        &mut s,
        "ext.add",
        json!({
            "name": "Title block",
            "description": "Write and bold the first row",
            "commands": [
                {"command": "range.setValues", "params": {"range": "A1", "values": [["Region", "Amount"]]}},
                {"command": "home.bold", "params": {"range": "A1:B1"}},
            ],
        }),
    );
    let list = run(&mut s, "ext.list", json!({}));
    assert_eq!(list[0]["name"], "Title block");
    assert_eq!(list[0]["steps"], 2);
    assert_eq!(list[0]["trusted"], true, "a workbook built here is trusted");
    let r = run(&mut s, "ext.run", json!({"name": "Title block"}));
    assert_eq!(r["ran"], 2);
    let d = s.doc().expect("doc");
    let sh = d.wb.sheet(0).expect("sheet");
    assert_eq!(sh.value(CellRef::new(0, 0)), Value::text("Region"));
}

#[test]
fn an_extension_naming_a_command_that_does_not_exist_is_refused_before_it_is_kept() {
    let mut s = session();
    let err = s
        .execute("ext.add", json!({"name": "Broken", "commands": [{"command": "nothing.here", "params": {}}]}))
        .expect_err("a step naming an unknown command");
    assert!(err.to_string().contains("nothing.here"), "{err}");
    assert_eq!(run(&mut s, "ext.list", json!({})), json!([]));
}

#[test]
fn an_extension_cannot_call_another_extension_or_a_script() {
    let mut s = session();
    run(&mut s, "ext.add", json!({"name": "Recursive", "commands": [{"command": "ext.run", "params": {"name": "Recursive"}}]}));
    let err = s.execute("ext.run", json!({"name": "Recursive"})).expect_err("one level only");
    assert!(err.to_string().contains("cannot run"), "{err}");
}

#[test]
fn an_extension_that_reaches_outside_the_workbook_is_refused() {
    let mut s = session();
    // file.saveAs writes a file, which a step of an automation may not do.
    run(&mut s, "ext.add", json!({"name": "Writer", "commands": [{"command": "file.saveAs", "params": {"path": "x.xlsx"}}]}));
    let err = s.execute("ext.run", json!({"name": "Writer"})).expect_err("no file access from an extension");
    let msg = err.to_string().to_lowercase();
    assert!(msg.contains("script") || msg.contains("outside") || msg.contains("step"), "{err}");
}

#[test]
fn a_file_that_arrives_with_an_extension_starts_untrusted() {
    let mut s = session();
    run(&mut s, "ext.add", json!({"name": "Hello", "commands": [{"command": "cell.set", "params": {"cell": "A1", "input": "hi"}}]}));
    let path = std::env::temp_dir().join("gridcraft_ext_trust.xlsx");
    let p = path.to_string_lossy().to_string();
    run(&mut s, "file.saveAs", json!({"path": p}));
    let mut back = Session::new();
    run(&mut back, "file.open", json!({"path": p}));
    let list = run(&mut back, "ext.list", json!({}));
    assert_eq!(list[0]["trusted"], false, "automation from a file starts disabled");
    let err = back.execute("ext.run", json!({"name": "Hello"})).expect_err("untrusted");
    assert!(err.to_string().contains("trust"), "{err}");
    run(&mut back, "automate.trustScripts", json!({"trusted": true}));
    assert_eq!(run(&mut back, "ext.run", json!({"name": "Hello"}))["ran"], 1);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn what_changed_lists_the_edits_since_the_file_was_saved() {
    let mut s = session();
    let path = std::env::temp_dir().join("gridcraft_whatchanged.xlsx");
    let p = path.to_string_lossy().to_string();
    run(&mut s, "range.setValues", json!({"range": "A1", "values": [["keep"], ["change"]]}));
    run(&mut s, "file.saveAs", json!({"path": p}));
    assert_eq!(run(&mut s, "inspect.whatChanged", json!({}))["changed"], 0);
    run(&mut s, "cell.set", json!({"cell": "A2", "input": "changed"}));
    run(&mut s, "cell.set", json!({"cell": "B5", "input": "=1+1"}));
    let r = run(&mut s, "inspect.whatChanged", json!({}));
    assert_eq!(r["changed"], 2);
    let cells = r["cells"].as_array().cloned().unwrap_or_default();
    let a2 = cells.iter().find(|c| c["cell"] == "A2").expect("A2 in the list");
    assert_eq!(a2["before"], "change");
    assert_eq!(a2["after"], "changed");
    let b5 = cells.iter().find(|c| c["cell"] == "B5").expect("B5 in the list");
    assert_eq!(b5["afterFormula"], "1+1", "the formula text as the model stores it, without the =");
    assert_eq!(b5["before"], Json::Null);
    // Saving again resets the baseline: the file now holds the edits.
    run(&mut s, "file.save", json!({}));
    assert_eq!(run(&mut s, "inspect.whatChanged", json!({}))["changed"], 0);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn what_changed_reports_added_and_removed_sheets_by_name() {
    let mut s = session();
    let path = std::env::temp_dir().join("gridcraft_whatchanged_sheets.xlsx");
    let p = path.to_string_lossy().to_string();
    run(&mut s, "file.saveAs", json!({"path": p}));
    run(&mut s, "home.insertSheet", json!({"name": "Extra"}));
    let r = run(&mut s, "inspect.whatChanged", json!({}));
    assert_eq!(r["sheetsAdded"][0], "Extra");
    assert_eq!(r["changed"], 0, "a new empty sheet changes no cell");
    let _ = std::fs::remove_file(&path);
}
