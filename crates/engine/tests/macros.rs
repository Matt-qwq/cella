//! The macro engine under our scripts: recording, running, the trust policy and step-through
//! (M11's adoption of upstream's `macros/`).
//!
//! A recording turns the commands that run into steps of a script in the workbook's step list; a
//! run is one undo step that stops at the first failing step and rolls itself back; a step that
//! reads a file or goes to the network needs the workbook trusted, and one that would leave the
//! workbook is refused whatever the flag says; a script that runs a script is refused rather than
//! recursed.

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

fn text(s: &Session, at: &str) -> String {
    let d = s.doc().expect("document");
    let sheet = d.wb.active().expect("sheet");
    cell_text(&d.wb, sheet, CellRef::parse(at).expect("a cell"))
}

fn steps_of(s: &mut Session, name: &str) -> Vec<Json> {
    run(s, "automate.scriptSteps", json!({ "name": name })).as_array().cloned().unwrap_or_default()
}

fn ids(steps: &[Json]) -> Vec<&str> {
    steps.iter().filter_map(|st| st["command"].as_str()).collect()
}

/// A recorded run of commands becomes a script of our `{command, params}` steps, and running it
/// back over a workbook that changed puts the values where they were recorded.
#[test]
fn a_recording_runs_back_on_a_changed_workbook() {
    let mut s = session();
    run(&mut s, "macro.record", json!({}));
    run(&mut s, "range.setValues", json!({"range": "A1", "values": [["Item", "Qty"], ["Widget", 2]]}));
    run(&mut s, "home.bold", json!({"range": "A1:B1"}));
    run(&mut s, "cell.set", json!({"cell": "B3", "input": "=SUM(B2)"}));
    let saved = run(&mut s, "macro.record", json!({"stop": true, "name": "Build"}));
    assert_eq!(saved["recording"], false, "{saved}");
    assert_eq!(saved["saved"], "Build", "{saved}");

    let steps = steps_of(&mut s, "Build");
    let ids = ids(&steps);
    assert!(ids.contains(&"range.setValues") && ids.contains(&"home.bold") && ids.contains(&"cell.set"), "{ids:?}");
    assert_eq!(saved["steps"].as_u64(), Some(steps.len() as u64), "{saved}");

    // Change the workbook, then run the script back over it.
    run(&mut s, "range.setValues", json!({"range": "A1", "values": [["", ""], ["", 0]]}));
    run(&mut s, "cell.set", json!({"cell": "B3", "input": ""}));
    let ran = run(&mut s, "macro.run", json!({"name": "Build"}));
    assert_eq!(ran["ran"].as_u64(), Some(steps.len() as u64), "{ran}");
    assert_eq!(text(&s, "A2"), "Widget");
    assert_eq!(text(&s, "B3"), "2");
}

/// The recorder leaves out what a script cannot replay: its own control commands, the script
/// store, undo and redo. The selection a command ran with becomes a step, so a command that takes
/// its range from the selection replays where it did.
#[test]
fn the_recorder_filters_and_materializes_the_selection() {
    let mut s = session();
    run(&mut s, "macro.record", json!({}));
    run(&mut s, "selection.set", json!({"range": "B2:C3"}));
    run(&mut s, "home.bold", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "automate.newScript", json!({"name": "Other", "commands": []}));
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "kept"}));
    let saved = run(&mut s, "macro.record", json!({"stop": true, "name": "Filtered"}));
    assert!(saved["skipped"].as_u64().unwrap_or(0) >= 3, "undo and the automate commands are left out: {saved}");
    let steps = steps_of(&mut s, "Filtered");
    let ids = ids(&steps);
    assert!(!ids.iter().any(|id| id.starts_with("automate.")), "{ids:?}");
    assert!(!ids.contains(&"edit.undo"), "{ids:?}");
    // The bold runs on the recorded selection, which is written out before it.
    let bold = steps.iter().position(|st| st["command"] == "home.bold").expect("the bold step");
    assert_eq!(steps[bold - 1]["command"], "selection.set");
    assert_eq!(steps[bold - 1]["params"]["range"], "B2:C3");
    run(&mut s, "selection.set", json!({"range": "A1"}));
    run(&mut s, "macro.run", json!({"name": "Filtered"}));
    let d = s.doc().expect("document");
    let sheet = d.wb.active().expect("sheet");
    let bold_cell = sheet.cell(CellRef::parse("B2").expect("cell")).map(|c| c.style).unwrap_or_default();
    assert!(d.wb.styles.get(bold_cell).font.bold, "B2 is bold after the replay");
}

/// One run is one undo step: a script of two commands is undone once.
#[test]
fn one_run_is_one_undo_step() {
    let mut s = session();
    run(
        &mut s,
        "automate.newScript",
        json!({"name": "Fill", "commands": [
            {"command": "cell.set", "params": {"cell": "A1", "input": "1"}},
            {"command": "cell.set", "params": {"cell": "A2", "input": "2"}},
        ]}),
    );
    let before = s.doc().expect("document").undo.len();
    assert_eq!(run(&mut s, "macro.run", json!({"name": "Fill"}))["ran"], 2);
    assert_eq!(text(&s, "A1"), "1");
    assert_eq!(s.doc().expect("document").undo.len(), before + 1, "one step for the whole run");
    let label = s.doc().expect("document").undo.last().map(|e| e.label.clone()).unwrap_or_default();
    assert!(label.contains("Run Script"), "{label}");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(text(&s, "A1"), "", "one undo takes the whole run back");
    assert_eq!(text(&s, "A2"), "");
}

/// A failing step stops the run, names the step and the command, and leaves the workbook as it was
/// before the run.
#[test]
fn a_failing_step_stops_and_rolls_the_run_back() {
    let mut s = session();
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "before"}));
    run(
        &mut s,
        "automate.newScript",
        json!({"name": "Half", "commands": [
            {"command": "cell.set", "params": {"cell": "B1", "input": "one"}},
            {"command": "cell.set", "params": {"cell": "not a cell", "input": "two"}},
            {"command": "cell.set", "params": {"cell": "C1", "input": "three"}},
        ]}),
    );
    let undo = s.doc().expect("document").undo.len();
    let err = s.execute("macro.run", json!({"name": "Half"})).expect_err("the second step fails").to_string();
    assert!(err.contains("step 2 (cell.set)"), "the error names the step: {err}");
    assert!(err.contains("Nothing was changed"), "{err}");
    assert_eq!(text(&s, "B1"), "", "the first step was rolled back");
    assert_eq!(text(&s, "C1"), "", "the third step never ran");
    assert_eq!(text(&s, "A1"), "before");
    assert_eq!(s.doc().expect("document").undo.len(), undo, "a failed run leaves no undo step");
}

/// A script that runs a script is refused, not recursed — through either id, and whatever the
/// stored list says.
#[test]
fn a_script_that_runs_a_script_is_refused() {
    let mut s = session();
    for (name, step) in [
        ("Loop", json!({"command": "automate.runScript", "params": {"name": "Loop"}})),
        ("Loop2", json!({"command": "macro.run", "params": {"name": "Loop2"}})),
    ] {
        run(&mut s, "automate.newScript", json!({ "name": name, "commands": [step.clone()] }));
        let err = s.execute("macro.run", json!({"name": name})).expect_err("a nested run is refused").to_string();
        assert!(err.contains("can't run"), "{name}: {err}");
    }
    assert_eq!(s.script_depth(), 0, "nothing is left running");
    assert_eq!(s.doc().expect("document").undo.len(), 2, "only the two scripts were written");
}

/// A workbook that arrived from a file runs nothing until it is trusted; a step that reads a file
/// needs that trust, and one that would leave the workbook (save, open, a link, undo) is refused
/// whatever the flag says.
#[test]
fn the_trust_policy_gates_the_file_system() {
    let dir = std::env::temp_dir().join(format!("gridcraft-macros-trust-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let csv = dir.join("in.csv");
    std::fs::write(&csv, b"a,b\n1,2\n").expect("the csv");
    let csv = csv.to_string_lossy().to_string();
    let leak = dir.join("leak.xlsx").to_string_lossy().to_string();

    // A workbook authored here, saved, and opened as a file that carries a script.
    let mut author = session();
    run(
        &mut author,
        "automate.newScript",
        json!({"name": "Import", "commands": [{"command": "data.getData", "params": {"path": csv.clone(), "sheet": "Data"}}]}),
    );
    let bytes = gridcraft_engine::io::save_bytes(&author.doc().expect("document").wb, "book.xlsx").expect("save");
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"name": "downloaded.xlsx", "base64": gridcraft_engine::io::base64_encode(&bytes)}));
    assert!(!s.doc().expect("document").scripts_trusted, "a workbook opened with scripts starts untrusted");
    let err = s.execute("macro.run", json!({"name": "Import"})).expect_err("untrusted").to_string();
    assert!(err.contains("trust"), "{err}");
    assert!(s.doc().expect("document").wb.sheet_index("Data").is_none(), "no step ran");

    // Trusted, the same step reads the file: the workbook's trust flag is the approval.
    run(&mut s, "automate.trustScripts", json!({}));
    assert!(s.doc().expect("document").scripts_trusted, "Trust Scripts sets the flag");
    run(&mut s, "macro.run", json!({"name": "Import"}));
    assert!(s.doc().expect("document").wb.sheet_index("Data").is_some(), "the trusted step imported the file");

    // What leaves the workbook is refused even when trusted.
    for (name, step) in [
        ("Save", json!({"command": "file.saveAs", "params": {"path": leak.clone()}})),
        ("Open", json!({"command": "file.open", "params": {"path": leak.clone()}})),
        ("Link", json!({"command": "link.open", "params": {"url": "https://example.com/"}})),
        ("Undo", json!({"command": "edit.undo", "params": {}})),
    ] {
        run(
            &mut s,
            "automate.newScript",
            json!({ "name": name, "commands": [
                {"command": "cell.set", "params": {"cell": "C1", "input": "partial"}},
                step,
            ]}),
        );
        let err = s.execute("macro.run", json!({"name": name})).expect_err("refused").to_string();
        assert!(err.contains("step 2") || err.contains("can't run"), "{name}: {err}");
        assert_eq!(text(&s, "C1"), "", "{name}: no step of a refused script runs");
    }
    assert!(!std::path::Path::new(&leak).exists(), "nothing was written");
    assert!(!s.ui_requests.iter().any(|q| matches!(q, gridcraft_engine::UiRequest::OpenUrl(_))), "no link was opened");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A recording is stored in the workbook's defined names, so it travels with the file and runs
/// after opening it.
#[test]
fn a_recording_survives_save_and_open() {
    let mut s = session();
    run(&mut s, "macro.record", json!({}));
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "saved"}));
    run(&mut s, "macro.record", json!({"stop": true, "name": "Keep"}));
    let recorded = steps_of(&mut s, "Keep");
    assert!(!recorded.is_empty());

    // The recording wrote A1; take it back so the file holds the script and not its effect.
    run(&mut s, "cell.set", json!({"cell": "A1", "input": ""}));
    let bytes = gridcraft_engine::io::save_bytes(&s.doc().expect("document").wb, "book.xlsx").expect("save");
    let (reopened, _) = gridcraft_engine::io::open_bytes("book.xlsx", &bytes).expect("open");
    let stored = gridcraft_engine::cmd::stash::get(&reopened, "scripts").expect("the scripts are in the file");
    assert!(stored.contains("Keep") && stored.contains("saved"), "{stored}");

    // The file holds the script, not what it wrote; running it writes the cells again.
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"name": "book.xlsx", "base64": gridcraft_engine::io::base64_encode(&bytes)}));
    assert_eq!(text(&s, "A1"), "", "the saved file has the script, not its effect");
    run(&mut s, "macro.run", json!({"name": "Keep", "trusted": true}));
    assert_eq!(text(&s, "A1"), "saved");
    assert!(!s.doc().expect("document").scripts_trusted, "the run did not trust the workbook");
    assert_eq!(s.doc().expect("document").undo.len(), 1, "one run, one undo step on top of opening");
}

/// The step cap is enforced where a script is written: the recorder cuts at it and the editor
/// refuses an append past it.
#[test]
fn the_step_cap_is_enforced() {
    let mut s = session();
    run(&mut s, "macro.record", json!({}));
    for i in 0..1_005u32 {
        run(&mut s, "view.zoom", json!({ "percent": 100 + (i % 5) }));
    }
    let saved = run(&mut s, "macro.record", json!({"stop": true, "name": "Many"}));
    assert_eq!(saved["steps"].as_u64(), Some(1_000), "a recording keeps the 1000 steps a script holds: {saved}");
    assert!(saved["dropped"].as_u64().unwrap_or(0) >= 5, "{saved}");
    assert_eq!(steps_of(&mut s, "Many").len(), 1_000);

    // The same cap on a list written by hand.
    let full: Vec<Json> = (0..1_000u32).map(|_| json!({"command": "view.zoom", "params": {"percent": 110}})).collect();
    run(&mut s, "automate.newScript", json!({ "name": "Full", "commands": full }));
    let err = s.execute("automate.appendStep", json!({"name": "Full", "command": "view.zoom", "params": {"percent": 120}}));
    assert!(err.is_err(), "a script holds 1000 steps");
}

/// `macro.debug` runs a script one step at a time inside one undo group: a paused step can be
/// undone whole, and stopping without keeping rolls everything back.
#[test]
fn step_through_runs_one_step_at_a_time() {
    let mut s = session();
    run(
        &mut s,
        "automate.newScript",
        json!({"name": "Two", "commands": [
            {"command": "cell.set", "params": {"cell": "A1", "input": "one"}},
            {"command": "cell.set", "params": {"cell": "A2", "input": "two"}},
        ]}),
    );
    let started = run(&mut s, "macro.debug", json!({"action": "start", "name": "Two"}));
    assert_eq!(started["state"], "paused", "{started}");
    assert_eq!(started["of"], 2);
    assert_eq!(text(&s, "A1"), "", "nothing ran yet");

    let one = run(&mut s, "macro.debug", json!({"action": "step"}));
    assert_eq!(text(&s, "A1"), "one");
    assert_eq!(text(&s, "A2"), "");
    assert_eq!(one["step"], 2, "{one}");
    assert_eq!(s.doc().expect("document").undo.len(), 1, "the script's own write, not the steps");

    // Stopping without keeping puts the workbook back.
    let stopped = run(&mut s, "macro.debug", json!({"action": "stop"}));
    assert_eq!(stopped["state"], "stopped", "{stopped}");
    assert_eq!(text(&s, "A1"), "", "the paused step was rolled back");
    assert_eq!(s.doc().expect("document").undo.len(), 1);

    // Continue to the end: what ran stays as one undo step.
    run(&mut s, "macro.debug", json!({"action": "start", "name": "Two"}));
    let finished = run(&mut s, "macro.debug", json!({"action": "continue"}));
    assert_eq!(finished["state"], "finished", "{finished}");
    assert_eq!(text(&s, "A2"), "two");
    assert_eq!(s.doc().expect("document").undo.len(), 2, "the session is one step");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(text(&s, "A1"), "", "one undo takes the whole session back");
    assert_eq!(text(&s, "A2"), "");

    // A failing step ends the session and rolls it back.
    run(
        &mut s,
        "automate.newScript",
        json!({"name": "Broken", "commands": [
            {"command": "cell.set", "params": {"cell": "B1", "input": "x"}},
            {"command": "cell.set", "params": {"cell": "nope", "input": "y"}},
        ]}),
    );
    run(&mut s, "macro.debug", json!({"action": "start", "name": "Broken"}));
    let err = s.execute("macro.debug", json!({"action": "continue"})).expect_err("the second step fails").to_string();
    assert!(err.contains("step 2"), "{err}");
    assert_eq!(text(&s, "B1"), "", "the session was rolled back");
    assert_eq!(run(&mut s, "macro.debug", json!({"action": "state"}))["state"], "idle");
}

/// The upstream ids are aliases of our own commands: same store, same editor, no second script
/// list.
#[test]
fn the_macro_aliases_reach_our_scripts() {
    let mut s = session();
    run(&mut s, "macro.record", json!({}));
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "alias"}));
    run(&mut s, "macro.record", json!({ "stop": true, "name": "ByAlias" }));
    let listed = run(&mut s, "macro.list", json!({}));
    assert!(listed.as_array().is_some_and(|a| a.iter().any(|x| x["name"] == "ByAlias")), "{listed}");
    let got = run(&mut s, "macro.get", json!({"name": "ByAlias"}));
    assert_eq!(got["name"], "ByAlias");
    let got_steps = got["steps"].as_array().cloned().unwrap_or_default();
    assert_eq!(ids(&got_steps), ["cell.set"]);
    assert_eq!(steps_of(&mut s, "ByAlias").len(), 1, "the same list automate.scriptSteps shows");

    run(&mut s, "macro.rename", json!({"name": "ByAlias", "to": "Renamed"}));
    run(&mut s, "cell.set", json!({"cell": "A1", "input": ""}));
    run(&mut s, "macro.run", json!({"name": "Renamed"}));
    assert_eq!(text(&s, "A1"), "alias");
    run(&mut s, "macro.delete", json!({"name": "Renamed"}));
    assert!(s.execute("macro.run", json!({"name": "Renamed"})).is_err());
    assert!(run(&mut s, "macro.list", json!({})).as_array().is_some_and(Vec::is_empty));
}
