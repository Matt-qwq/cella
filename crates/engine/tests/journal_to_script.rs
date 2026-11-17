//! The journal is where a procedure is captured, and a script is its replayable form: this is the
//! path an agent (or a person editing by hand) is expected to take — do it once, save the steps,
//! replay or edit them.

use cella_engine::Session;
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

#[test]
fn the_steps_that_ran_become_a_script_that_runs_the_same_way() {
    let mut s = session();
    s.execute("range.setValues", json!({"range": "A1", "values": [["n"], ["1"]]})).unwrap();
    s.execute("home.bold", json!({"range": "A1"})).unwrap();
    let r = s.execute("automate.scriptFromJournal", json!({"name": "Bold the header"})).unwrap();
    assert_eq!(r["name"], "Bold the header");
    let steps = s.execute("automate.scriptSteps", json!({"name": "Bold the header"})).unwrap();
    let ids: Vec<&str> = steps.as_array().map(|a| a.iter().filter_map(|x| x["command"].as_str()).collect()).unwrap_or_default();
    // The automation command itself is not part of the script: recording one into another is how a
    // loop gets built.
    assert!(!ids.contains(&"automate.scriptFromJournal"), "{ids:?}");
    assert!(ids.contains(&"home.bold"), "{ids:?}");
    // The workbook's state, replayed on a fresh one, is the same.
    let mut fresh = session();
    fresh.execute("automate.newScript", json!({"name": "Bold the header", "commands": steps})).unwrap();
    fresh.execute("automate.runScript", json!({"name": "Bold the header", "trusted": true})).unwrap();
    let a = s.doc().expect("doc").wb.sheet(0).expect("sheet");
    let b = fresh.doc().expect("doc").wb.sheet(0).expect("sheet");
    for at in ["A1", "A2"] {
        let at = cella_core::CellRef::parse(at).expect("ref");
        assert_eq!(a.value(at), b.value(at), "cell {at:?}");
    }
}

#[test]
fn a_script_from_the_journal_can_drop_the_noise() {
    let mut s = session();
    s.execute("range.setValues", json!({"range": "A1", "values": [["x"]]})).unwrap();
    s.execute("selection.set", json!({"cell": "A1"})).unwrap();
    let r = s.execute("automate.scriptFromJournal", json!({"name": "Only the edit", "drop": ["selection.set"]})).unwrap();
    assert_eq!(r["steps"], 1, "the selection step was dropped");
}

#[test]
fn an_empty_journal_and_a_taken_name_are_refused() {
    let mut s = session();
    let err = s.execute("automate.scriptFromJournal", json!({"name": "Nothing"})).expect_err("nothing ran");
    assert!(err.to_string().contains("nothing to save"), "{err}");
    s.execute("range.setValues", json!({"range": "A1", "values": [["x"]]})).unwrap();
    s.execute("automate.scriptFromJournal", json!({"name": "Once"})).unwrap();
    let err = s.execute("automate.scriptFromJournal", json!({"name": "Once"})).expect_err("the name is taken");
    assert!(err.to_string().contains("already a script"), "{err}");
}
