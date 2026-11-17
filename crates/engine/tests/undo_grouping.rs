//! Undo grouping: a run of commands that becomes one undo step (what a macro, a Python
//! recalculation or a multi-part command needs).
//!
//! The mechanism is the engine API only (Session::grouped, group_begin / group_end /
//! group_rollback): no command id and no ribbon entry, so nothing a user sees changes. These
//! tests prove that a group is one step, that it is costed like one entry, that a rollback leaves
//! the workbook and the history exactly as they were, that Undo and Redo refuse to run in the
//! middle of one, and that ordinary commands are one step each again afterwards.

use std::sync::Arc;

use cella_engine::core::CellRef;
use cella_engine::display::cell_text;
use cella_engine::{EngineError, Result, Session, register_test_command};
use serde_json::json;

fn cell(row: u32, col: u32) -> CellRef {
    CellRef::new(row, col)
}

fn text(s: &Session, c: CellRef) -> String {
    let d = s.doc().expect("document");
    let sheet = d.wb.active().expect("sheet");
    cell_text(&d.wb, sheet, c)
}

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn set(s: &mut Session, c: CellRef, input: &str) {
    let a1 = cella_engine::core::col_to_letters(c.col) + &(c.row + 1).to_string();
    s.execute("cell.set", json!({"cell": a1, "input": input})).expect("cell.set");
}

/// (a) Two commands inside a group are one undo step: undo once restores both.
#[test]
fn two_commands_in_a_group_are_one_undo_step() {
    let mut s = session();
    let token = s.group_begin("Pair").expect("group");
    set(&mut s, cell(0, 0), "1");
    set(&mut s, cell(1, 0), "2");
    assert_eq!(s.doc().expect("document").undo.len(), 0, "a command in a group is no step of its own");
    assert!(s.group_end(token).expect("end"));
    assert_eq!(s.doc().expect("document").undo.len(), 1, "the group is one step");
    assert_eq!(s.doc().expect("document").undo[0].label, "Pair");

    s.execute("edit.undo", json!({})).expect("undo");
    assert_eq!(text(&s, cell(0, 0)), "", "A1 came back with the group");
    assert_eq!(text(&s, cell(1, 0)), "", "A2 came back with the group");
    s.execute("edit.redo", json!({})).expect("redo");
    assert_eq!(text(&s, cell(0, 0)), "1");
    assert_eq!(text(&s, cell(1, 0)), "2");
}

/// (b) A rollback discards the group's effects and leaves the history as it was.
#[test]
fn rollback_discards_the_group_and_leaves_the_history_alone() {
    let mut s = session();
    set(&mut s, cell(0, 0), "keep");
    set(&mut s, cell(1, 0), "redo me");
    s.execute("edit.undo", json!({})).expect("undo"); // one entry to undo, one to redo
    let uid = s.doc().expect("document").uid;
    let wb_before = s.doc().expect("document").wb.clone();
    let selection_before = s.doc().expect("document").selection.clone();
    let undo_before: Vec<(String, usize)> = s.doc().expect("document").undo.iter().map(|e| (e.label.clone(), e.retained_bytes)).collect();
    let redo_before: Vec<(String, usize)> = s.doc().expect("document").redo.iter().map(|e| (e.label.clone(), e.retained_bytes)).collect();
    assert!(!undo_before.is_empty() && !redo_before.is_empty());

    let token = s.group_begin("Doomed").expect("group");
    set(&mut s, cell(0, 2), "doomed");
    s.execute("home.bold", json!({"range": "C1", "on": true})).expect("bold");
    assert!(s.in_group(uid));
    s.group_rollback(token).expect("rollback");
    assert!(!s.in_group(uid), "the group is gone");

    let d = s.doc().expect("document");
    assert!(Arc::ptr_eq(&d.wb, &wb_before), "the workbook is the one the group began with");
    assert_eq!(d.selection, selection_before);
    assert_eq!(d.undo.iter().map(|e| (e.label.clone(), e.retained_bytes)).collect::<Vec<_>>(), undo_before);
    assert_eq!(d.redo.iter().map(|e| (e.label.clone(), e.retained_bytes)).collect::<Vec<_>>(), redo_before);
    assert_eq!(text(&s, cell(0, 2)), "", "the group's edit is gone");
    // The redo history the group waited on still works.
    s.execute("edit.redo", json!({})).expect("redo");
    assert_eq!(text(&s, cell(1, 0)), "redo me");
}

/// (c) A group is charged the way one undo entry is: from the snapshot it began with to the
/// workbook it leaves. The pair as two steps would charge the middle snapshot too.
#[test]
fn a_group_is_charged_as_one_entry() {
    let mut s = session();
    let start = s.doc().expect("document").wb.clone();
    let token = s.group_begin("Bold pair").expect("group");
    s.execute("home.bold", json!({"range": "A1", "on": true})).expect("bold A1");
    s.execute("home.bold", json!({"range": "A2", "on": true})).expect("bold A2");
    assert!(s.group_end(token).expect("end"));
    let d = s.doc().expect("document");
    assert_eq!(d.undo.len(), 1, "one entry, not two");
    let grouped = d.undo[0].retained_bytes;

    // The same change as one command, from the same snapshot: it must cost the same.
    s.execute("edit.undo", json!({})).expect("undo");
    assert!(Arc::ptr_eq(&s.doc().expect("document").wb, &start));
    assert!(s.doc().expect("document").undo.is_empty());
    s.execute("home.bold", json!({"range": "A1:A2", "on": true})).expect("bold both");
    let single = s.doc().expect("document").undo.last().expect("entry").retained_bytes;
    assert_eq!(grouped, single, "a group costs what the same change costs as one entry");

    // Two steps would keep the middle snapshot alive as well.
    s.execute("edit.undo", json!({})).expect("undo");
    s.execute("home.bold", json!({"range": "A1", "on": true})).expect("bold A1");
    s.execute("home.bold", json!({"range": "A2", "on": true})).expect("bold A2");
    let d = s.doc().expect("document");
    assert_eq!(d.undo.len(), 2);
    let two_steps: usize = d.undo.iter().map(|e| e.retained_bytes).sum();
    assert!(two_steps > grouped, "grouping keeps no middle snapshot: {two_steps} vs {grouped}");
}

/// (d) Undo and Redo are refused while a group is open, and allowed again after it ends or is
/// rolled back.
#[test]
fn undo_and_redo_wait_for_an_open_group() {
    let mut s = session();
    set(&mut s, cell(0, 0), "1");
    set(&mut s, cell(1, 0), "2");
    s.execute("edit.undo", json!({})).expect("undo"); // so Redo has something to do
    let uid = s.doc().expect("document").uid;
    let token = s.group_begin("Open").expect("group");
    set(&mut s, cell(0, 1), "3");

    for id in ["edit.undo", "edit.redo"] {
        match s.execute(id, json!({})) {
            Err(EngineError::Disabled(refused, why)) => {
                assert_eq!(refused, id);
                assert!(why.contains("undo group"), "{id} said: {why}");
            }
            other => panic!("{id} ran while a group was open: {other:?}"),
        }
    }
    assert!(s.in_group(uid));

    assert!(s.group_end(token).expect("end"));
    s.execute("edit.undo", json!({})).expect("undo after the group ended");
    assert_eq!(text(&s, cell(0, 1)), "", "undo took the whole group back");
    s.execute("edit.redo", json!({})).expect("redo after the group ended");
    assert_eq!(text(&s, cell(0, 1)), "3");

    // And the same after a rollback.
    let token = s.group_begin("Rolled back").expect("group");
    set(&mut s, cell(0, 2), "4");
    s.group_rollback(token).expect("rollback");
    s.execute("edit.undo", json!({})).expect("undo after the rollback");
    assert_eq!(text(&s, cell(0, 2)), "");
    s.execute("edit.redo", json!({})).expect("redo after the rollback");
}

/// (e) A group whose commands move the selection is still one step, and that step carries the
/// selection the group began with (a selection change makes no step of its own in this engine, so
/// it rides with the group's step instead of being lost).
#[test]
fn a_selection_change_inside_a_group_rides_with_its_one_step() {
    let mut s = session();
    set(&mut s, cell(0, 0), "x");
    let before = s.doc().expect("document").selection.clone();
    let token = s.group_begin("Type elsewhere").expect("group");
    s.execute("selection.set", json!({"cell": "D4"})).expect("select");
    assert_eq!(s.doc().expect("document").selection.active, cell(3, 3));
    set(&mut s, cell(1, 1), "y");
    assert!(s.group_end(token).expect("end"));
    assert_eq!(s.doc().expect("document").undo.len(), 2, "the group is one step on top of A1's");

    s.execute("edit.undo", json!({})).expect("undo");
    assert_eq!(text(&s, cell(1, 1)), "", "the group's edit came back");
    assert_eq!(s.doc().expect("document").selection, before, "undo returns the selection the group began with");
}

/// (f) After a group, ordinary commands are one step each again.
#[test]
fn ordinary_commands_are_one_step_each_after_a_group() {
    let mut s = session();
    let token = s.group_begin("Once").expect("group");
    set(&mut s, cell(0, 0), "1");
    set(&mut s, cell(1, 0), "2");
    assert!(s.group_end(token).expect("end"));
    let after_group = s.doc().expect("document").undo.len();
    set(&mut s, cell(2, 0), "3");
    set(&mut s, cell(3, 0), "4");
    assert_eq!(s.doc().expect("document").undo.len(), after_group + 2, "one step per command again");
    s.execute("edit.undo", json!({})).expect("undo");
    assert_eq!(text(&s, cell(3, 0)), "", "only the last command came back");
    assert_eq!(text(&s, cell(2, 0)), "3");
    assert_eq!(text(&s, cell(0, 0)), "1", "the group is still there under them");
}

/// A run that fails must leave no group open and nothing behind: the error is reported, the
/// workbook is the one the run began with. A run that panics does the same before the panic goes
/// on.
#[test]
fn a_run_that_fails_or_panics_leaves_nothing_behind() {
    let mut s = session();
    set(&mut s, cell(0, 0), "keep");
    let uid = s.doc().expect("document").uid;
    let wb_before = s.doc().expect("document").wb.clone();
    let undo_before = s.doc().expect("document").undo.len();

    let err = s.grouped("Fails", |s| -> Result<()> {
        set(s, cell(1, 1), "gone");
        // A group the run leaves open goes with the outer one.
        let _inner = s.group_begin("Inner")?;
        set(s, cell(2, 2), "gone too");
        Err(EngineError::Other("boom".into()))
    });
    assert!(err.is_err());
    assert!(!s.in_group(uid), "no group is left open");
    assert!(Arc::ptr_eq(&s.doc().expect("document").wb, &wb_before));
    assert_eq!(s.doc().expect("document").undo.len(), undo_before);
    assert_eq!(text(&s, cell(1, 1)), "");

    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = s.grouped("Panics", |s| -> Result<()> {
            set(s, cell(1, 2), "gone");
            let _inner = s.group_begin("Inner")?;
            panic!("boom");
        });
    }));
    assert!(panicked.is_err(), "the panic went on");
    assert!(!s.in_group(uid), "no group is left open by a panic either");
    assert!(Arc::ptr_eq(&s.doc().expect("document").wb, &wb_before));
    assert_eq!(s.doc().expect("document").undo.len(), undo_before);
    assert_eq!(text(&s, cell(1, 2)), "");
}

/// A command that opens a group and then fails leaves it rolled back: the execute guard, not the
/// caller, owns the groups a command opened.
fn group_and_fail(s: &mut Session, _: &serde_json::Value) -> Result<serde_json::Value> {
    let _token = s.group_begin("Doomed")?;
    set(s, cell(3, 3), "gone");
    Err(EngineError::Other("boom".into()))
}

#[test]
fn a_failing_command_leaves_none_of_its_groups_open() {
    register_test_command("test.groupAndFail", group_and_fail);
    let mut s = session();
    set(&mut s, cell(0, 0), "keep");
    let uid = s.doc().expect("document").uid;
    let wb_before = s.doc().expect("document").wb.clone();
    assert!(s.execute("test.groupAndFail", json!({})).is_err());
    assert!(!s.in_group(uid));
    assert!(Arc::ptr_eq(&s.doc().expect("document").wb, &wb_before));
    assert_eq!(text(&s, cell(3, 3)), "");
}
