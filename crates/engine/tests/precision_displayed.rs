//! "Set precision as displayed" through the command layer: the option (`fullPrecision="0"` in a
//! file), numbers typed or written while it is on, and the values formulas read.
//!
//! The behaviour was observed in Excel (COM): turning the option on rounds every number constant
//! permanently and recalculates formulas; a number typed, pasted or reformatted while it is on is
//! stored as its format shows it; General keeps full precision; the option is saved with the file.

use cella_core::{CellRef, Value};
use cella_engine::Session;
use serde_json::json;

fn s() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn v(s: &Session, a: &str) -> Value {
    let at = match CellRef::parse(a) {
        Some(c) => c,
        None => CellRef::new(0, 0),
    };
    let d = match s.doc() {
        Ok(d) => d,
        Err(_) => return Value::Empty,
    };
    d.wb.active().map(|sh| sh.value(at)).unwrap_or_default()
}

fn num(s: &Session, a: &str) -> f64 {
    match v(s, a) {
        Value::Number(n) => n,
        other => panic!("{a} = {other:?}"),
    }
}

fn fmt(s: &mut Session, range: &str, code: &str) {
    s.execute("home.numberFormat", json!({"range": range, "code": code})).unwrap();
}

fn set(s: &mut Session, cell: &str, input: &str) {
    s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
}

/// (a) `=1/3` in a `0.00` cell stores 0.33, and `=A1*3` reads 0.33 (0.99). With the option off
/// the same workbook computes at full precision (1).
#[test]
fn a_formula_result_takes_its_cell_format() {
    let mut s = s();
    set(&mut s, "A1", "=1/3");
    set(&mut s, "B1", "=A1*3");
    fmt(&mut s, "A1", "0.00");
    assert_eq!(num(&s, "A1"), 1.0 / 3.0);
    assert_eq!(num(&s, "B1"), 1.0);
    let r = s.execute("formulas.calculationOptions", json!({"precisionAsDisplayed": true})).unwrap();
    assert_eq!(r["precisionAsDisplayed"], true);
    assert_eq!(num(&s, "A1"), 0.33);
    assert_eq!(num(&s, "B1"), 0.99);
    // Off again: the recalculation brings the full value back.
    s.execute("formulas.calculationOptions", json!({"precisionAsDisplayed": false})).unwrap();
    assert_eq!(num(&s, "A1"), 1.0 / 3.0);
    assert_eq!(num(&s, "B1"), 1.0);
}

/// (b) A number typed into a `0.00` cell is stored as 1.23, and a dependent reads that.
#[test]
fn a_typed_number_is_stored_as_displayed() {
    let mut s = s();
    fmt(&mut s, "A1", "0.00");
    s.execute("formulas.calculationOptions", json!({"precisionAsDisplayed": true})).unwrap();
    set(&mut s, "A1", "1.23456");
    assert_eq!(num(&s, "A1"), 1.23);
    set(&mut s, "B1", "=A1*100");
    assert_eq!(num(&s, "B1"), 123.0);
    // Pasted values take the same path.
    s.execute("range.setValues", json!({"range": "A1", "values": [[5.43219]]})).unwrap();
    assert_eq!(num(&s, "A1"), 5.43);
    assert_eq!(num(&s, "B1"), 543.0);
    // Formatting a constant while the option is on rounds it at once (observed in Excel).
    set(&mut s, "C1", "1.23456");
    fmt(&mut s, "C1", "0.0");
    assert_eq!(num(&s, "C1"), 1.2);
    // The digits are gone for good: a wider format does not bring them back (Excel warns that
    // the data loses accuracy permanently).
    fmt(&mut s, "C1", "0.0000");
    assert_eq!(num(&s, "C1"), 1.2);
}

/// (c) General is never rounded: a typed number and a formula keep every digit there.
#[test]
fn general_keeps_full_precision() {
    let mut s = s();
    s.execute("formulas.calculationOptions", json!({"precisionAsDisplayed": true})).unwrap();
    set(&mut s, "A1", "1.234567890123456");
    assert_eq!(num(&s, "A1"), 1.234567890123456);
    set(&mut s, "A2", "=1/3");
    assert_eq!(num(&s, "A2"), 1.0 / 3.0);
}

/// (d) The flag round-trips through a saved `.xlsx` and still applies.
#[test]
fn the_option_survives_saving_and_reopening() {
    let mut s = s();
    set(&mut s, "A1", "1.23456");
    fmt(&mut s, "A1", "0.00");
    set(&mut s, "A3", "=7/3");
    fmt(&mut s, "A3", "0");
    s.execute("formulas.calculationOptions", json!({"precisionAsDisplayed": true})).unwrap();
    assert_eq!((num(&s, "A1"), num(&s, "A3")), (1.23, 2.0));
    let r = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let b64 = r["base64"].as_str().expect("base64").to_string();
    s.execute("file.open", json!({"name": "precision.xlsx", "base64": b64})).unwrap();
    assert!(s.doc().expect("doc").wb.calc.precision_as_displayed, "fullPrecision=\"0\" is read back");
    assert_eq!((num(&s, "A1"), num(&s, "A3")), (1.23, 2.0));
    let r = s.execute("file.options", json!({})).unwrap();
    assert_eq!(r["calculation"]["precisionAsDisplayed"], true);
}

/// Reformatting a cell a dynamic array spills into rounds that element: the cell holds no stored
/// value, so only its style changes and the anchor has to be recalculated (observed in Excel).
#[test]
fn a_reformatted_spill_cell_is_rounded() {
    let mut s = s();
    set(&mut s, "F1", "=SEQUENCE(3,1,0.123,0.111)");
    s.execute("formulas.calculationOptions", json!({"precisionAsDisplayed": true})).unwrap();
    set(&mut s, "G1", "=SUM(F1:F3)");
    let (f2, f3) = (0.123 + 0.111, 0.123 + 0.111 * 2.0);
    assert_eq!((num(&s, "F1"), num(&s, "F2"), num(&s, "F3")), (0.123, f2, f3));
    fmt(&mut s, "F2", "0.0");
    assert_eq!((num(&s, "F1"), num(&s, "F2"), num(&s, "F3")), (0.123, 0.2, f3));
    assert_eq!(num(&s, "G1"), 0.123 + 0.2 + f3);
}

/// Omitted parameters leave the settings as they are.
#[test]
fn an_omitted_param_leaves_the_option_alone() {
    let mut s = s();
    s.execute("formulas.calculationOptions", json!({"precisionAsDisplayed": true})).unwrap();
    let r = s.execute("formulas.calculationOptions", json!({"mode": "manual"})).unwrap();
    assert_eq!(r["precisionAsDisplayed"], true);
    assert!(s.doc().expect("doc").wb.calc.precision_as_displayed);
}

/// (e) Percent and thousands formats round to the digits they show.
#[test]
fn percent_and_thousands_round_to_their_digits() {
    let mut s = s();
    set(&mut s, "A1", "=0.123456");
    fmt(&mut s, "A1", "0.00%");
    set(&mut s, "A2", "=1234.5678");
    fmt(&mut s, "A2", "#,##0");
    set(&mut s, "A3", "0.123456");
    fmt(&mut s, "A3", "0.0%");
    s.execute("formulas.calculationOptions", json!({"precisionAsDisplayed": true})).unwrap();
    assert_eq!(num(&s, "A1"), 0.1235);
    assert_eq!(num(&s, "A2"), 1235.0);
    assert_eq!(num(&s, "A3"), 0.123);
}
