//! A formula saved without dynamic-array metadata (no `cm`) is read the way legacy Excel evaluates
//! it: a reference that may cover several cells, used where one value is expected, is intersected
//! with the formula's row or column and shown with Excel's `@` insertion, so the dynamic-array
//! evaluator computes the legacy result instead of spilling. A dynamic-array formula (`cm`) and a
//! Ctrl+Shift+Enter array range are read as written.

use gridcraft_core::{CellRef, RangeRef};
use gridcraft_model::Workbook;

use crate::read_xlsx;

fn at(s: &str) -> CellRef {
    CellRef::parse(s).unwrap()
}

/// A cell's formula text and array range.
fn formula(wb: &Workbook, a: &str) -> (String, Option<Box<RangeRef>>) {
    let f = wb.sheet(0).unwrap().cell(at(a)).unwrap().formula.unwrap();
    (f.text.clone(), f.array.clone())
}

#[test]
fn legacy_formulas_read_with_implicit_intersection() {
    let body = r#"<sheetData>
      <row r="1"><c r="A1"><v>1</v></c></row>
      <row r="2"><c r="A2"><v>2</v></c><c r="B2"><f>TANH(A1:A3)</f><v>0.96</v></c><c r="C2" cm="1"><f t="array" ref="C2:C4">TANH(A1:A3)</f><v>0.76</v></c><c r="D2"><f t="array" ref="D2:D4">TANH(A1:A3)</f><v>0.76</v></c></row>
      <row r="3"><c r="A3"><v>3</v></c><c r="B3"><f>SUM(A1:A3)</f><v>6</v></c><c r="E3"><f>SUMPRODUCT(A1:A3*A1:A3)</f><v>14</v></c></row>
      <row r="4"><c r="B4"><f>A1:A3+Vals*2</f></c><c r="E4"><f>INDEX(A1:C3,0,1)</f></c><c r="F4"><f>IF(A1&gt;0,A1:A3,0)</f></c><c r="G4"><f>SUM(IF(A1:A3&gt;1,A1:A3))</f></c></row>
      <row r="5"><c r="A5"><v>4</v></c><c r="B5"><v>5</v></c><c r="C5"><v>6</v></c></row>
      <row r="7"><c r="B7"><f>TANH(A5:C5)</f></c><c r="C7"><f>COUNTIF(A1:A3,"&gt;1")+ROWS(A1:A3)</f></c><c r="D7"><f>ROW(A1:A3)</f></c><c r="E7"><f>T(A5:C5)</f></c></row>
    </sheetData>"#;
    let names = r#"<definedNames><definedName name="Vals">Data!$A$1:$A$3</definedName></definedNames>"#;
    let (wb, _) = read_xlsx(&super::minimal(body, &[], "", names)).unwrap();
    let cases = [
        ("B2", "TANH(@A1:A3)"),
        ("B3", "SUM(A1:A3)"),
        ("E3", "SUMPRODUCT(A1:A3*A1:A3)"),
        ("B4", "@A1:A3+@Vals*2"),
        ("E4", "@INDEX(A1:C3,0,1)"),
        ("F4", "IF(A1>0,@A1:A3,0)"),
        ("G4", "SUM(IF(@A1:A3>1,A1:A3))"),
        ("B7", "TANH(@A5:C5)"),
        ("C7", "COUNTIF(A1:A3,\">1\")+ROWS(A1:A3)"),
        ("D7", "@ROW(A1:A3)"),
        // T reads a reference's top-left cell rather than intersecting it.
        ("E7", "@T(A5:C5)"),
    ];
    for (a, text) in cases {
        assert_eq!(formula(&wb, a), (text.to_string(), None), "{a}");
        // The skip that keeps opening a two-million-formula workbook from building two million
        // trees is sound where it applies: a formula it leaves alone is read as written. (It is
        // *not* the writer's `needs_array`, which answers a different question: it says no for
        // `SUMPRODUCT(IFERROR(A1:A3/A1:A3,0))` although that reads with an `@` inside.)
        let f = wb.sheet(0).unwrap().cell(at(a)).unwrap().formula.clone().unwrap();
        if !crate::array_formula::may_need_reading_for_test(&f, at(a)) {
            assert!(!text.contains('@'), "{a} was read with an @ the skip said it could not need");
        }
    }
    // A dynamic-array formula with `cm` is read as written; so is a Ctrl+Shift+Enter array.
    assert_eq!(formula(&wb, "C2"), ("TANH(A1:A3)".to_string(), None));
    assert_eq!(formula(&wb, "D2"), ("TANH(A1:A3)".to_string(), RangeRef::parse("D2:D4").map(Box::new)));
}

/// Legacy IF does not evaluate its condition as an array inside SUMPRODUCT, nor IFERROR and IFNA
/// their arguments (that needs Ctrl+Shift+Enter): they are intersected there, while IF's results
/// stay arrays.
#[test]
fn legacy_if_and_iferror_arguments_are_intersected_inside_array_functions() {
    let body = r#"<sheetData>
      <row r="1"><c r="A1"><v>1</v></c></row>
      <row r="2"><c r="A2"><v>2</v></c><c r="D2"><f>SUMPRODUCT(IFERROR(A1:A3/A1:A3,0))</f></c><c r="E2"><f>SUMPRODUCT(IFNA(A1:A3,0),A1:A3)</f></c></row>
      <row r="3"><c r="A3"><v>3</v></c></row>
      <row r="4"><c r="D4"><f>SUMPRODUCT(IF(A1:A3&gt;1,1,0))</f></c><c r="E4"><f>SUMPRODUCT(IF(A1&gt;1,A1:A3,0))</f></c></row>
    </sheetData>"#;
    let (wb, _) = read_xlsx(&super::minimal(body, &[], "", "")).unwrap();
    let text = |a: &str| formula(&wb, a).0;
    assert_eq!(text("D2"), "SUMPRODUCT(IFERROR(@A1:A3/@A1:A3,0))");
    assert_eq!(text("E2"), "SUMPRODUCT(IFNA(@A1:A3,0),A1:A3)");
    assert_eq!(text("D4"), "SUMPRODUCT(IF(@A1:A3>1,1,0))");
    assert_eq!(text("E4"), "SUMPRODUCT(IF(A1>1,A1:A3,0))");
}

/// The modern spelling of the same formulas is left alone: a file that marks its formulas as
/// dynamic-array ones (`cm`) evaluates them as arrays, spilling where the result needs several
/// cells, so nothing is intersected.
#[test]
fn dynamic_array_formulas_are_untouched() {
    let body = r#"<sheetData>
      <row r="1"><c r="A1"><v>1</v></c></row>
      <row r="2"><c r="A2"><v>2</v></c><c r="B2" cm="1"><f>A1:A3*2</f></c><c r="C2" cm="1"><f t="array" ref="C2:C4">A1:A3*2</f></c></row>
      <row r="3"><c r="A3"><v>3</v></c></row>
    </sheetData>"#;
    let (wb, _) = read_xlsx(&super::minimal(body, &[], "", "")).unwrap();
    assert_eq!(formula(&wb, "B2"), ("A1:A3*2".to_string(), None));
    // `cm` names the range the result spills into; it is not a legacy array range.
    assert_eq!(formula(&wb, "C2"), ("A1:A3*2".to_string(), None));
}

/// The intersected reading survives saving: the formula is saved as the dynamic-array formula its
/// text now is (Excel shows the same one value), and reopens with the same text.
#[test]
fn intersected_formula_round_trips() {
    let body = r#"<sheetData><row r="1"><c r="B1"><f>A1:A3*2</f></c></row></sheetData>"#;
    let (wb, _) = read_xlsx(&super::minimal(body, &[], "", "")).unwrap();
    let bytes = crate::write_xlsx(&wb).unwrap();
    let (back, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(formula(&back, "B1").0, "@A1:A3*2");
}
