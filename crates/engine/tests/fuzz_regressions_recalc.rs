//! Regression tests for the recalc findings of the 400-seed command-sequence soak: after an

//! incremental edit the value in memory disagreed with a full recalculation.

//!

//! Each test replays the sequence the fuzzer shrank to (or a smaller hand-built one with the same

//! mechanism), then asserts the two paths agree *and* that the value they agree on is the right

//! one.

use std::collections::BTreeSet;

use cella_core::{CellError, CellRef, Value};

use cella_engine::Session;

use cella_engine::calc::Calc;

use cella_engine::model::Workbook;

use serde_json::{Value as Json, json};

fn session(sample: &str) -> Session {
    let mut s = Session::new();

    s.execute("file.new", json!({"sample": sample})).expect("sample opens");

    s
}

fn run(s: &mut Session, id: &str, params: Json) {
    s.execute(id, params.clone()).unwrap_or_else(|e| panic!("`{id}` {params} failed: {e}"));
}

fn wb(s: &Session) -> Workbook {
    (*s.doc().expect("document").wb).clone()
}

fn value(s: &Session, at: &str) -> Value {
    s.doc().expect("document").wb.active().expect("sheet").value(CellRef::parse(at).expect("cell"))
}

fn formula(s: &Session, at: &str) -> Option<String> {
    let wb = s.doc().expect("document").wb.clone();

    let c = CellRef::parse(at).expect("cell");
    wb.active().and_then(|sh| sh.cell(c)).and_then(|c| c.formula).and_then(|f| f.text_at(c))
}

/// A full recalculation from a fresh dependency graph.
fn recalc_all(wb: &Workbook) -> Workbook {
    let mut fresh = wb.clone();

    Calc::new().recalc_all(&mut fresh);

    fresh
}

/// The first cell where the incrementally maintained values and a full recalc disagree.
fn recalc_diff(wb: &Workbook) -> Option<String> {
    let fresh = recalc_all(wb);

    for (x, y) in wb.sheets.iter().zip(&fresh.sheets) {
        let mut keys: BTreeSet<CellRef> = BTreeSet::new();

        for s in [x, y] {
            keys.extend(s.cells.iter().filter(|(_, c)| !c.value.is_empty() || c.formula.is_some()).map(|(c, _)| c));

            keys.extend(s.spill_ranges.values().flat_map(|r| r.iter()));
        }

        for c in keys {
            let (va, vb) = (x.value(c), y.value(c));

            if va != vb {
                let f = x
                    .cell(c)
                    .and_then(|c| c.formula)
                    .and_then(|f| f.text_at(c))
                    .map(|t| format!(" (formula ={t})"))
                    .or_else(|| {
                        let (anchor, _) = x.spill_ranges.iter().find(|(_, r)| r.contains(c))?;

                        let f = x.cell(*anchor).and_then(|c| c.formula)?;

                        f.text_at(*anchor).map(|t| format!(" (spilled from {} ={t})", anchor.a1()))
                    })
                    .unwrap_or_default();

                return Some(format!("{}!{c}{f}: {va:?} in memory, {vb:?} after a full recalc", x.name));
            }
        }
    }

    None
}

fn assert_recalc_agrees(s: &Session) -> Workbook {
    let wb = wb(s);

    if let Some(diff) = recalc_diff(&wb) {
        panic!("incremental recalculation disagrees with a full one: {diff}");
    }

    wb
}

/// Seed 378. The full recalc was the wrong side: the store's per-row wide table aliased values
/// once it grew past 65,535 entries, so rewriting `A23` read another cell's text back.
#[test]

fn seed_378_insert_columns_keeps_a_concatenation_with_an_empty_cell_empty() {
    let mut s = session("sales");

    run(&mut s, "selection.set", json!({"range": "D21:G28"}));

    run(&mut s, "range.fill", json!({"input": "=I23&\"-\"&H6", "range": "23:23"}));

    run(&mut s, "home.deleteColumns", json!({"cols": "I:J"}));

    run(&mut s, "data.filterBy", json!({"column": "F", "values": ["275", "2026-01-02"]}));

    run(&mut s, "home.deleteCells", json!({"range": "B2:B9", "shift": "column"}));

    run(&mut s, "home.deleteColumns", json!({"cols": "K:L"}));

    run(&mut s, "home.insertColumns", json!({"cols": "B:B"}));

    run(&mut s, "home.insertColumns", json!({"cols": "G:H"}));

    assert_recalc_agrees(&s);

    assert_eq!(formula(&s, "A23").as_deref(), Some("F25&\"-\"&E8"));

    assert_eq!(value(&s, "F25"), Value::Empty);

    assert_eq!(value(&s, "E8"), Value::Number(70150.0));

    assert_eq!(value(&s, "A23"), Value::Text("-70150".into()));
}

/// Seed 229. The full recalc was the wrong side: the `SORT` anchor turned out circular after its
/// array had been read, and the values read from the discarded array stayed in the reader.
#[test]

fn seed_229_a_spilled_sort_that_is_circular_leaves_no_values_behind() {
    let mut s = session("budget");

    run(&mut s, "range.fill", json!({"input": "=COUNTIF(G:G,\">0\")", "range": "B8:F14"}));

    run(&mut s, "home.insertColumns", json!({"cols": "D:E"}));

    run(&mut s, "home.deleteColumns", json!({"cols": "B:B"}));

    run(&mut s, "cell.set", json!({"cell": "K16", "input": "=SORT(D7:G12)"}));

    assert_recalc_agrees(&s);

    assert_eq!(formula(&s, "F13").as_deref(), Some("COUNTIF(L:L,\">0\")"));

    // The anchor is circular, so nothing it would have spilled exists: column L holds no number.

    assert_eq!(value(&s, "K16"), Value::Error(CellError::Circ));

    let wb = wb(&s);

    assert!(wb.active().expect("sheet").spill_ranges.is_empty(), "a circular array spills nothing");

    assert_eq!(value(&s, "F13"), Value::Number(0.0));
}

/// Seed 320. The incremental pass was the wrong side: `=23:26` is read through the sheet's used
/// range, and the last edit moved that range's right edge, so the array no longer fits.
#[test]

fn seed_320_offset_into_a_whole_row_reference_spills_like_a_full_recalc_says() {
    let mut s = session("grades");

    run(
        &mut s,
        "range.setValues",
        json!({"range": "K5", "values": [[null, false], ["=IF(B14>2,\"big\",B14)", "=A23:A26"], [true, "=SUM(H11:L17)"], [153, false]]}),
    );

    run(&mut s, "edit.replace", json!({"all": true, "what": "A", "with": ""}));

    run(&mut s, "cell.set", json!({"cell": "XFB6", "input": "=OFFSET(F22,1,1)"}));

    assert_recalc_agrees(&s);

    assert_eq!(formula(&s, "L6").as_deref(), Some("23:26"));

    assert_eq!(value(&s, "L6"), Value::Error(CellError::Spill));
}

/// Seed 229's replay at the current generator (the fuzzer's 6-step shrink): the `SORT`'s spill
/// moved under a defined name, then the `SORT` turned circular. The name hides its range from the
/// graph, so nothing seeded the sum; a spill change has to evaluate the sheet's dynamic formulas
/// again.
#[test]

fn seed_229_a_defined_name_reading_a_discarded_spill_is_evaluated_again() {
    let mut s = session("budget");

    run(&mut s, "formulas.defineName", json!({"name": "Totals", "refersTo": "='Budget'!$K$10:$N$17"}));

    run(&mut s, "home.insertColumns", json!({"cols": "I:I"}));

    run(&mut s, "cell.set", json!({"cell": "F56", "input": "=SUM(Totals)"}));

    run(&mut s, "cell.set", json!({"cell": "K16", "input": "=SORT(D7:G12)"}));

    run(&mut s, "home.deleteRows", json!({"rows": "8:9"}));

    run(&mut s, "edit.replace", json!({"all": true, "what": "1", "with": ""}));

    assert_recalc_agrees(&s);

    assert_eq!(formula(&s, "F54").as_deref(), Some("SUM(Totals)"));

    assert_eq!(value(&s, "F54"), Value::Number(0.0));
}

/// Seed 229 as the soak found it: the fill wrote `COUNTIF(D:D)` into D8:D14, counting the column
/// that holds the formulas themselves. A formula that reads its own cell is circular, and it has
/// to be circular for the formulas evaluated after it in the same pass too: the `SORT` and the
/// `COUNTIF`s fed each other and the pass ended with the `SORT`'s array from the previous round.
#[test]

fn seed_229_a_formula_that_reads_its_own_cell_is_circular_in_the_same_pass() {
    let mut s = session("budget");

    run(&mut s, "edit.cut", json!({"range": "C21:E22"}));

    run(&mut s, "edit.paste", json!({"at": "G5"}));

    run(&mut s, "range.fill", json!({"input": "=COUNTIF(G:G,\">0\")", "range": "B8:F14"}));

    run(&mut s, "home.deleteRows", json!({"rows": "14:14"}));

    run(&mut s, "home.mergeAcross", json!({}));

    run(&mut s, "edit.clearComments", json!({}));

    run(&mut s, "edit.undo", json!({"steps": 3}));

    run(&mut s, "home.insertRows", json!({"rows": "15:15"}));

    run(&mut s, "cell.set", json!({"cell": "L12", "input": "=INDIRECT(\"I15\")"}));

    run(&mut s, "cell.set", json!({"cell": "K16", "input": "=SORT(D7:G12)"}));

    assert_recalc_agrees(&s);

    assert_eq!(value(&s, "D8"), Value::Error(CellError::Circ));

    assert_eq!(value(&s, "K16"), Value::Number(212.8));
}

/// Seed 51. Thirty-five `=SEQUENCE(2,2)` anchors filled a block, seven row and column edits moved
/// their spilled areas around each other, and a paste then partly covered one of the areas. The
/// pass recalculated only the pasted cells, so the anchor whose area was partly replaced kept the
/// `#SPILL!` the earlier layout had left it with, while a full recalculation lays the array out.
#[test]
fn seed_51_a_paste_partly_covering_a_spilled_area_lays_the_array_out_again() {
    let mut s = session("sales");

    s.execute("range.fill", json!({"input": "=SEQUENCE(2,2)", "range": "C18:G24"})).ok();
    s.execute("home.insertColumns", json!({"cols": "E:F"})).ok();
    s.execute("home.deleteRows", json!({"rows": "2:3"})).ok();
    s.execute("home.insertCells", json!({"range": "I21:K27", "shift": "column"})).ok();
    s.execute("home.deleteColumns", json!({"cols": "D:E"})).ok();
    s.execute("selection.set", json!({"range": "F2:I4"})).ok();
    s.execute("cell.set", json!({"cell": "G12", "input": "=D11:E15"})).ok();
    s.execute("home.insertRows", json!({"count": false})).ok();
    s.execute("home.insertRows", json!({"count": 4, "rows": "18:19"})).ok();
    s.execute("home.deleteColumns", json!({"cols": "C:C"})).ok();
    s.execute("home.insertRows", json!({"count": 1, "rows": "10:10"})).ok();
    s.execute("home.insertRows", json!({"rows": "11:13"})).ok();
    s.execute("selection.set", json!({"range": "B23:E26"})).ok();
    s.execute("home.deleteRows", json!({"rows": "15:15"})).ok();
    s.execute("home.deleteRows", json!({"rows": "14:15"})).ok();
    s.execute("home.deleteRows", json!({"rows": "17:18"})).ok();
    s.execute("edit.copy", json!({"range": "H6:L13"})).ok();
    s.execute("edit.clearContents", json!({})).ok();
    s.execute("edit.paste", json!({"at": "B10"})).ok();

    assert_recalc_agrees(&s);

    assert_eq!(formula(&s, "E22").as_deref(), Some("SEQUENCE(2,2)"));

    assert_eq!(value(&s, "E22"), Value::Number(1.0));
}

/// Seed 247. Two written spill formulas fought the sample's own `=SORT` over one area, and the
/// deleteCells/deleteRows/deleteColumns after them re-based all three. The pass left the `SORT`
/// anchor with the `#SPILL!` of the old layout while a full recalculation finds it circular (its
/// re-based range now reads the cells one of the new arrays spills): the two error codes disagreed.
#[test]
fn seed_247_a_rebased_fight_between_spills_agrees_with_a_full_recalc() {
    let mut s = session("budget");

    s.execute("cell.set", json!({"cell": "K12", "input": "=SORT(F16:I19)"})).ok();
    s.execute("cell.set", json!({"cell": "I17", "input": "=TRANSPOSE(H13:L18)"})).ok();
    s.execute("home.deleteCells", json!({"range": "K1:N5", "shift": "row"})).ok();
    s.execute("home.deleteRows", json!({"rows": "3:4"})).ok();
    s.execute("home.deleteColumns", json!({"cols": "H:H"})).ok();
    s.execute("cell.set", json!({"cell": "K3", "input": "=TRANSPOSE(H23:K30)"})).ok();

    assert_recalc_agrees(&s);

    assert_eq!(value(&s, "J5"), Value::Error(CellError::Circ));
}
/// Seed 44. Two arrays share one sheet, one of them with a pre-pass spill: a full pass evicted
/// **every** other spill on the sheet when an owner grew back — not only the cells it was taking
/// back — and removed the range while leaving the array in `spill_values`. The pass then flipped
/// between two answers round after round, and with the foreign-sheet rewrite bug of the same soak
/// (a row edit moved another sheet's references) it "read" sample data that was not there.
///
/// With the eviction taking only what an owner claims, and a value going with its range, the two
/// paths agree and a second pass changes nothing. The array answers **one row of zeros**, which is
/// the right answer for its input: `UNIQUE(E14:I18)` spans an empty range (measured cell by cell).
#[test]
fn seed_44_a_short_spill_agrees_with_a_full_recalc() {
    let mut s = session("grades");

    s.execute("cell.set", json!({"cell": "Y1", "input": "=UNIQUE(E10:I14)"})).ok();
    s.execute("home.insertRows", json!({"rows": "1:3"})).ok();
    s.execute("sheet.rename", json!({"name": "it's", "sheet": "Grades"})).ok();
    s.execute("home.insertSheet", json!({"before": 1, "name": "Q4 2026"})).ok();
    s.execute("home.deleteRows", json!({"rows": "1:3"})).ok();
    s.execute("home.deleteRows", json!({"rows": "4:6"})).ok();
    s.execute("sheet.move", json!({"copy": false, "sheet": "it's", "to": 1})).ok();
    s.execute("home.insertRows", json!({"rows": "8:8"})).ok();
    s.execute("cell.set", json!({"cell": "H19", "input": "=FILTER(G1:G20,G1:G20>5)"})).ok();
    s.execute("edit.fillRight", json!({"range": "E:G"})).ok();
    s.execute("cell.set", json!({"cell": "I20", "input": "=OFFSET(K15,1,1)"})).ok();
    s.execute("home.deleteRows", json!({"rows": "11:12"})).ok();
    s.execute("cell.set", json!({"cell": "L25", "input": "=INDIRECT(\"G11\")", "sheet": "Q4 2026"})).ok();

    assert_recalc_agrees(&s);

    let state = |s: &Session| {
        let wb = wb(s);
        let sh = wb.sheets.iter().find(|x| x.name == "it's").expect("the renamed sheet");
        let row: Vec<Value> = (24..29u32).map(|c| sh.value(CellRef::new(3, c))).collect();
        (sh.spill_ranges.len(), sh.spill_values.len(), sh.spill_ranges.get(&CellRef::new(3, 24)).copied().map(|r| r.a1()), row)
    };

    let before = state(&s);
    assert_eq!(before.0, before.1, "every spilled range has the array that fills it");
    assert_eq!(before.2.as_deref(), Some("Y4:AC4"), "the array spills the one row its input has");
    assert_eq!(before.3, vec![Value::Number(0.0); 5], "an empty input is one row of zeros");

    // A full pass over a consistent workbook changes nothing: the oscillation is gone.
    run(&mut s, "formulas.calculateNow", json!({}));

    assert_eq!(state(&s), before);
}
/// Seed 135. A `=SORT(C20:D25)` at `XFC76` (the last column) is moved to `XFC73` by a row delete, and
/// a reader is entered after the anchor has become circular: `=SUM('Sales (2)'!XFC73#)`, whose own cell
/// sits inside the range the anchor sorts. The anchor answers `#CIRC!` on both paths; the reader
/// answered `#REF!` from the incremental pass (a `#` on a cell that is not spilling) and `#CIRC!` from a
/// from-scratch one, because only the full pass saw the whole chain at once. The spill reference reads
/// its anchor now, so the chain is circular for every member (gaps §6).
#[test]
fn seed_135_a_spill_reader_entered_after_the_anchor_went_circular_agrees_with_a_full_recalc() {
    let mut s = session("sales");

    s.execute("sheet.move", json!({"copy": true, "sheet": "Sales", "to": 1})).ok();
    s.execute("cell.set", json!({"cell": "XFC76", "input": "=SORT(C20:D25)"})).ok();
    s.execute(
        "range.setValues",
        json!({"range": "H4", "values": [[
            null,
            "=IF(E11>5, \"big\", E11)",
            "99.35"
        ], [
            "=SUM('Sales (2)'!XFC76#)",
            "East",
            false
        ], [
            "=VLOOKUP(E2,I20:K21,2,FALSE)",
            null,
            "=INDEX(I10:K13,5,1)"
        ], [
            "92%",
            "=OFFSET(C7,1,1)",
            "$1,234.50"
        ]]}),
    )
    .ok();
    s.execute("home.deleteRows", json!({"rows": "9:11"})).ok();
    s.execute("cell.set", json!({"cell": "D18", "input": "=SUM('Sales (2)'!XFC73#)"})).ok();
    s.execute("edit.paste", json!({"at": "G4", "text": "1\t2\nx\t=A1"})).ok();

    assert_recalc_agrees(&s);

    let wb = wb(&s);
    let sh = wb.sheets.iter().find(|x| x.name == "Sales (2)").expect("the copied sheet");
    assert_eq!(sh.value(CellRef::parse("XFC73").expect("cell")), Value::Error(CellError::Circ), "the anchor sorts its own reader");
    assert_eq!(sh.value(CellRef::parse("D18").expect("cell")), Value::Error(CellError::Circ), "the reader is in the same chain");
}
/// Seed 233. `=AVERAGE(J11:M16)*2` at `J15` sits inside the range it averages, which makes it circular,
/// and `J13` in that range holds `#N/A`. The aggregate met the error first and stopped, so it never read
/// the cell that closes the cycle: memory answered `#N/A` while a full recalculation answered `#CIRC!`.
/// The aggregate walk keeps going past the first error now — the answer is still the first error, but
/// every formula in the range is read — so the cycle is found on both paths (gaps §6).
#[test]
fn seed_233_a_self_reading_range_with_an_earlier_error_agrees_with_a_full_recalc() {
    let mut s = session("grades");

    s.execute("selection.set", json!({"range": "D7:D11"})).ok();
    s.execute("home.deleteColumns", json!({"cols": "F:F"})).ok();
    s.execute("range.setValues", json!({"range": "J18", "values": [["=COUNTIF(G:G,\">0\")"]]})).ok();
    s.execute("cell.set", json!({"cell": "L6", "input": "=G14:I17"})).ok();
    s.execute("formulas.deleteName", json!({"force": true, "name": "Data"})).ok();
    s.execute("cell.set", json!({"cell": "G15", "input": "=C5&\"-\"&H19"})).ok();
    s.execute("home.fillColor", json!({"color": "#00B050", "range": "E4:I5"})).ok();
    s.execute("cell.set", json!({"cell": "A8", "input": "=IF(J6>3,\"big\",J6)"})).ok();
    s.execute("home.deleteColumns", json!({"cols": "I:I"})).ok();
    s.execute("cell.set", json!({"cell": "D16", "input": "='Grades'!D20*2"})).ok();
    s.execute("edit.clearContents", json!({})).ok();
    s.execute("cell.set", json!({"cell": "B3", "input": "2026-03-16"})).ok();
    s.execute(
        "range.setValues",
        json!({"range": "I13", "values": [
            [null, "#N/A"],
            [70, null],
            ["=FILTER(H1:H20,H1:H20>6)", "=AVERAGE(J11:M16)*2"]
        ]}),
    )
    .ok();

    assert_recalc_agrees(&s);

    let wb = wb(&s);
    let sh = wb.sheets.iter().find(|x| x.name == "Grades").expect("the sheet");
    assert_eq!(sh.value(CellRef::parse("J15").expect("cell")), Value::Error(CellError::Circ), "its own cell is in the range");
}
