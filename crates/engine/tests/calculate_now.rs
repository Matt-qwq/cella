//! Calculate Now (F9) keeps the dependency graph: every edit maintains it, so the command
//! evaluates the workbook instead of deriving every formula's precedents again — on the 1M
//! reference workbook that rebuild was 0.69 s of a 1.05 s F9 (`Calc::recalc_all_keeping_graph`).
//!
//! These tests hold the shortcut to the values a from-scratch rebuild lands on, which is the same
//! ground truth the command-sequence fuzzer uses (incremental ≡ full recalc), after a sequence
//! that takes every path which touches the graph: a value edit, a formula edit, a fill, a sort
//! that moves formulas, a structural edit, an undo/redo pair and a cross-sheet formula.

use cella_core::{CellRef, Value};
use cella_engine::Session;
use serde_json::{Value as Json, json};

fn workbook() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn run(s: &mut Session, command: &str, params: Json) {
    s.execute(command, params).unwrap();
}

fn set(s: &mut Session, cell: &str, input: &str) {
    run(s, "cell.set", json!({"cell": cell, "input": input}));
}

/// A fixed window of every sheet, so two workbooks are compared at the same addresses: a used
/// range that differs by a spilled cell is the defect, not the window.
const ROWS: u32 = 40;
const COLS: u32 = 12;

fn values(wb: &cella_model::Workbook) -> Vec<(usize, CellRef, Value)> {
    let mut out = Vec::new();
    for (si, sheet) in wb.sheets.iter().enumerate() {
        for row in 0..ROWS {
            for col in 0..COLS {
                let at = CellRef::new(row, col);
                out.push((si, at, sheet.value(at)));
            }
        }
    }
    out
}

/// The same workbook recalculated by a `Calc` that has never seen it: what `formulas.calculateNow`
/// must agree with.
fn from_scratch(s: &Session) -> Vec<(usize, CellRef, Value)> {
    let mut wb = (*s.doc().expect("a document").wb).clone();
    let mut calc = cella_calc::recalc::Calc::new();
    calc.recalc_all(&mut wb);
    values(&wb)
}

fn calculate_now(s: &mut Session) -> u64 {
    let out = s.execute("formulas.calculateNow", json!({})).expect("calculateNow");
    out.get("cells").and_then(Json::as_u64).unwrap_or(0)
}

/// Values, formulas over them, a spill, a defined name, a second sheet that reads the first.
fn populate(s: &mut Session) {
    run(
        s,
        "range.setValues",
        json!({"range": "A1", "values": [
            [1, "=A1*2", 10],
            [2, "=A2*2", 20],
            [3, "=A3*2", 30],
            [4, "=A4*2", 40]
        ]}),
    );
    set(s, "D1", "=SUM(A1:A4)");
    set(s, "E1", "=SEQUENCE(2,2)");
    set(s, "F1", "=D1+A1");
    run(s, "formulas.defineName", json!({"name": "Rate", "refersTo": "=0.07"}));
    set(s, "G1", "=SUM(A1:A4)*Rate");
    run(s, "home.insertSheet", json!({"name": "Data"}));
    set(s, "A1", "=Sheet1!D1+1");
    set(s, "A2", "=SUM(Sheet1!A1:A4)");
    set(s, "B1", "=Sheet1!E1*2");
}

/// The edits a session makes before it reaches for F9, one per path that touches the graph.
fn edit_through_every_path(s: &mut Session) {
    set(s, "A1", "7");
    set(s, "B1", "=A1*5");
    run(s, "edit.autoFill", json!({"source": "B1", "target": "B1:B4"}));
    run(s, "data.sortDescending", json!({"range": "A1:C4", "column": "C"}));
    run(s, "home.insertRows", json!({"rows": "2:2"}));
    run(s, "edit.undo", json!({}));
    run(s, "edit.redo", json!({}));
    set(s, "A2", "=C2+1");
}

#[test]
fn calculate_now_lands_where_a_from_scratch_rebuild_lands() {
    let mut s = workbook();
    populate(&mut s);
    edit_through_every_path(&mut s);
    let expected = from_scratch(&s);
    let rebuilds = s.doc().expect("a document").calc.rebuilds;
    let cells = calculate_now(&mut s);
    assert!(cells > 0, "Calculate Now recalculated nothing");
    assert_eq!(
        s.doc().expect("a document").calc.rebuilds,
        rebuilds,
        "Calculate Now derived the graph again instead of keeping the one every edit maintained"
    );
    assert_eq!(values(&s.doc().expect("a document").wb), expected, "Calculate Now disagrees with a from-scratch rebuild");
}

#[test]
fn calculate_now_twice_lands_on_the_same_numbers() {
    let mut s = workbook();
    populate(&mut s);
    edit_through_every_path(&mut s);
    set(&mut s, "A3", "=D1+1");
    assert!(calculate_now(&mut s) > 0);
    let once = values(&s.doc().expect("a document").wb);
    assert!(calculate_now(&mut s) > 0);
    let twice = values(&s.doc().expect("a document").wb);
    assert_eq!(once, twice, "a second Calculate Now changed a value");
}
