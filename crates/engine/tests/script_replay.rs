//! N4's exit criterion: a recorded script — with one step edited afterwards — replays to a
//! workbook identical to the one the same steps write when they are run directly.
//!
//! The sequence is recorded on an empty workbook, the recording is rolled back so the workbook
//! keeps only the script (what a file would carry), one step is edited, and the file is opened
//! again and replayed. The replay is compared against the edited steps run by hand, cell by cell
//! and format by format, so a script that skipped, reordered or lost a step shows up.

use gridcraft_core::CellRef;
use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

/// The sequence the test records. Every step is undoable and rewrites the same value, so the
/// recording can be rolled back and running it twice changes nothing.
fn sequence() -> Vec<(&'static str, Json)> {
    vec![
        ("range.setValues", json!({"range": "A1", "values": [["Item", "Qty"], ["Widget", 2]]})),
        ("home.bold", json!({"range": "A1:B1"})),
        ("cell.set", json!({"cell": "B3", "input": "=SUM(B2)"})),
        ("cell.set", json!({"cell": "A4", "input": "note"})),
    ]
}

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn run(s: &mut Session, cmd: &str, params: Json) -> Json {
    s.execute(cmd, params).unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

/// Everything a script can change, as comparable lines: each sheet's used cells (display text,
/// formula, resolved format), its merges and tables, and the workbook's defined names. The script
/// stash itself is left out — that is the script, not what it wrote.
fn content(s: &Session) -> Vec<String> {
    let d = s.doc().expect("doc");
    let wb = &d.wb;
    let mut out = Vec::new();
    for sh in &wb.sheets {
        out.push(format!("sheet {:?}", sh.name));
        if let Some(u) = sh.used_range() {
            for r in u.start.row..=u.end.row {
                for c in u.start.col..=u.end.col {
                    let cr = CellRef::new(r, c);
                    let formula = sh.cell(cr).and_then(|x| x.formula).map(|f| f.text.clone());
                    out.push(format!(
                        "{}!{} = {:?} formula {:?} format {:?}",
                        sh.name,
                        cr.a1(),
                        sh.value(cr).display(),
                        formula,
                        wb.styles.get(sh.style_id(cr))
                    ));
                }
            }
        }
        for m in &sh.merges {
            out.push(format!("{} merge {}", sh.name, m.a1()));
        }
        for t in &sh.tables {
            out.push(format!("{} table {} {}", sh.name, t.name, t.range.a1()));
        }
    }
    for n in wb.names.iter().filter(|n| !n.name.starts_with("_sc_")) {
        out.push(format!("name {} {}", n.name, n.formula));
    }
    out
}

/// A sheet's cells as rows of display text, for the expected-shape assertions.
fn grid(s: &Session, sheet: &str) -> Vec<Vec<String>> {
    let d = s.doc().expect("doc");
    let sh = d.wb.sheet_index(sheet).and_then(|i| d.wb.sheet(i)).unwrap_or_else(|| panic!("no sheet {sheet}"));
    let Some(u) = sh.used_range() else { return Vec::new() };
    (u.start.row..=u.end.row).map(|r| (u.start.col..=u.end.col).map(|c| sh.value(CellRef::new(r, c)).display()).collect()).collect()
}

#[test]
fn a_recorded_script_with_an_edited_step_replays_to_the_same_workbook() {
    let mut s = session();
    let empty = content(&s);

    // 1. Record a sequence.
    run(&mut s, "automate.recordActions", json!({}));
    for (id, params) in sequence() {
        run(&mut s, id, params);
    }
    let saved = run(&mut s, "automate.recordActions", json!({"stop": true, "name": "Build"}));
    assert_eq!(saved["steps"], sequence().len(), "{saved}");

    // 2. Undo the recording whole — the edits and the script's own undo entry with them — and write
    //    the script back from the steps it saved. The workbook is empty again and keeps only the
    //    script: the state a file carries, which is what the replay has to rebuild from.
    let recorded = run(&mut s, "automate.scriptSteps", json!({"name": "Build"}));
    assert_eq!(recorded.as_array().map(Vec::len), Some(sequence().len()), "{recorded}");
    let entries = s.active().map(|d| d.undo.len()).unwrap_or(0);
    assert!(entries >= sequence().len(), "{entries} undo entries for {} steps", sequence().len());
    run(&mut s, "edit.undo", json!({"steps": entries}));
    assert_eq!(content(&s), empty, "every recorded edit is undone");
    run(&mut s, "automate.newScript", json!({"name": "Build", "commands": recorded}));
    assert_eq!(content(&s), empty, "the script is the only thing the workbook holds");

    // 3. Edit one step: the params of the `cell.set` that writes B3 (its command is kept).
    let recorded = run(&mut s, "automate.scriptSteps", json!({"name": "Build"}));
    let index = recorded
        .as_array()
        .and_then(|a| a.iter().position(|st| st["command"] == "cell.set" && st["params"]["cell"] == "B3"))
        .expect("the recorded step that writes B3");
    let edited = run(&mut s, "automate.editStep", json!({"name": "Build", "index": index, "params": {"cell": "B3", "input": "=SUM(B2)+40"}}));
    assert_eq!(edited["steps"], sequence().len());
    let steps = run(&mut s, "automate.scriptSteps", json!({"name": "Build"}));
    assert_eq!(steps.as_array().map(Vec::len), Some(sequence().len()), "{steps}");
    assert_eq!(steps[index]["params"]["input"], "=SUM(B2)+40", "{steps}");
    assert_eq!(content(&s), empty, "editing the script changes no cell");

    // 4. The script travels with the file; opening it starts the scripts disabled.
    let bytes = gridcraft_engine::io::save_bytes(&s.doc().expect("doc").wb, "book.xlsx").expect("save");
    let mut replayed = Session::new();
    run(&mut replayed, "file.open", json!({"name": "book.xlsx", "base64": gridcraft_engine::io::base64_encode(&bytes)}));
    assert!(!replayed.active().expect("doc").scripts_trusted, "a script that came with a file starts disabled");
    assert_eq!(content(&replayed), empty, "the file holds the script, not what it does");
    let ran = run(&mut replayed, "automate.runScript", json!({"name": "Build", "trusted": true}));
    assert_eq!(ran["ran"], sequence().len(), "{ran}");

    // 5. The same edited steps, run directly on a workbook of the same shape.
    let mut direct = session();
    for st in steps.as_array().cloned().unwrap_or_default() {
        let id = st["command"].as_str().unwrap_or("").to_string();
        run(&mut direct, &id, st["params"].clone());
    }

    // 6. Identical: every cell's text, formula and format, the sheet's objects and the names.
    assert_eq!(content(&replayed), content(&direct), "a script replays exactly what its steps write");
    // And it is the recorded workbook, not an empty comparison of two untouched sheets.
    assert_eq!(
        grid(&replayed, "Sheet1"),
        vec![
            vec!["Item".to_string(), "Qty".to_string()],
            vec!["Widget".to_string(), "2".to_string()],
            vec![String::new(), "42".to_string()],
            vec!["note".to_string(), String::new()],
        ]
    );
    let d = replayed.doc().expect("doc");
    let sh = d.wb.active().expect("sheet");
    assert!(d.wb.styles.get(sh.style_id(CellRef::new(0, 0))).font.bold, "the recording's formatting is replayable too");
    assert!(!d.wb.styles.get(sh.style_id(CellRef::new(1, 0))).font.bold);
}
