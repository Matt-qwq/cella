//! `XLOOKUP("text", A:A, B:B)` over two single-column references answers what the general path
//! answers.
//!
//! The fast path reads the stored cells of the lookup column and the one matching cell of the
//! return column; the general path builds both blocks first. A needle held in a cell takes the
//! general path (the fast path only takes a literal), so the same lookup written both ways on the
//! same sheet is the differential: every case below must answer alike, errors included.

use cella_core::{CellRef, Value};
use cella_engine::Session;
use serde_json::json;

fn workbook() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn set(s: &mut Session, cell: &str, input: &str) {
    s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
}

fn value(s: &Session, cell: &str) -> Value {
    let (row, col) = parse(cell);
    s.doc().unwrap().wb.active().unwrap().value(CellRef::new(row, col))
}

fn parse(cell: &str) -> (u32, u32) {
    let mut letters = 0usize;
    let mut col = 0u32;
    let mut digits = String::new();
    for c in cell.chars() {
        if c.is_ascii_alphabetic() {
            letters += 1;
            col = col * 26 + (c.to_ascii_uppercase() as u32 - 'A' as u32 + 1);
        } else {
            digits.push(c);
        }
    }
    assert!(letters > 0 && !digits.is_empty(), "a cell address, not {cell}");
    (digits.parse::<u32>().unwrap().saturating_sub(1), col.saturating_sub(1))
}

/// The sheet the cases look up into: texts in A (mixed case, a blank, a literal wildcard), values in
/// B (with a blank), and a formula of its own in A9.
fn sheet() -> Session {
    let mut s = workbook();
    for (i, text) in ["Alpha", "beta", "ALPHA", "blankrow", "x*y", "Gamma", "7", "Delta"].iter().enumerate() {
        set(&mut s, &format!("A{}", i + 1), text);
    }
    set(&mut s, "A9", "=\"Hel\"&\"lo\"");
    set(&mut s, "A10", "7");
    // B4 is left empty: a lookup that lands there answers 0, not a blank.
    for (i, n) in [10, 20, 30, 50, 60, 70, 80, 90].iter().enumerate() {
        let row = if i < 3 { i + 1 } else { i + 2 };
        set(&mut s, &format!("B{row}"), &n.to_string());
    }
    s
}

/// Both spellings of the same lookup: the literal needle (fast path) and the needle in Z1 (general
/// path). Returns the two answers.
fn both(s: &mut Session, needle: &str) -> (Value, Value) {
    set(s, "Z1", needle);
    set(s, "C1", &format!("=XLOOKUP(\"{needle}\",A:A,B:B)"));
    set(s, "C2", "=XLOOKUP(Z1,A:A,B:B)");
    (value(s, "C1"), value(s, "C2"))
}

#[test]
fn a_text_needle_answers_like_the_general_path() {
    let mut s = sheet();
    // Every text in the column, in the case it is stored and in another case: the exact search
    // compares text case-insensitively.
    for (needle, expect) in [("Alpha", 10.0), ("alpha", 10.0), ("ALPHA", 10.0), ("beta", 20.0), ("Gamma", 60.0), ("Delta", 80.0), ("x*y", 50.0)] {
        let (fast, slow) = both(&mut s, needle);
        assert_eq!(fast, slow, "needle {needle}");
        assert_eq!(fast, Value::Number(expect), "needle {needle}");
    }
}

#[test]
fn a_missing_needle_is_na_either_way() {
    let mut s = sheet();
    let (fast, slow) = both(&mut s, "nowhere");
    assert_eq!(fast, slow);
    assert_eq!(fast, Value::Error(cella_core::CellError::NA));
}

/// A needle that matches a row whose return cell is blank reads as 0, not as a blank.
#[test]
fn a_blank_return_cell_reads_as_zero() {
    let mut s = sheet();
    // Row 4 (A4 = blankrow) has no B4.
    let (fast, slow) = both(&mut s, "blankrow");
    assert_eq!(fast, slow);
    assert_eq!(fast, Value::Number(0.0));
}

/// A formula in the lookup column is a cell only the general path can price: the fast path declines
/// and the answer is still the general path's.
#[test]
fn a_formula_in_the_lookup_column_still_answers() {
    let mut s = sheet();
    let (fast, slow) = both(&mut s, "Hello");
    assert_eq!(fast, slow);
    assert_eq!(fast, Value::Number(90.0));
}

/// A number needle is not a text search: the fast path declines, and the answer matches the general
/// path's (A10 holds the text "7", A7 the text "7" too — a number needle matches neither).
#[test]
fn a_number_needle_answers_like_the_general_path() {
    let mut s = sheet();
    set(&mut s, "B11", "70");
    set(&mut s, "C1", "=XLOOKUP(70,A:A,B:B)");
    set(&mut s, "Z1", "70");
    set(&mut s, "C2", "=XLOOKUP(Z1,A:A,B:B)");
    set(&mut s, "Z2", "70");
    set(&mut s, "C3", "=XLOOKUP(VALUE(Z2),A:A,B:B)");
    assert_eq!(value(&s, "C1"), value(&s, "C3"));
}

/// A wildcard needle keeps its wildcard meaning on both paths.
/// Exact mode reads a wildcard literally (wildcards are what `match_mode` 2 asks for), so the
/// fast path declines the needle and both spellings answer `#N/A`; with `match_mode` 2 the needle
/// is a pattern and the fast path declines again — four arguments is not the shape it takes.
#[test]
fn a_wildcard_needle_answers_like_the_general_path() {
    let mut s = sheet();
    let (fast, slow) = both(&mut s, "*a");
    assert_eq!(fast, slow);
    assert_eq!(fast, Value::Error(cella_core::CellError::NA));

    set(&mut s, "Z2", "2");
    set(&mut s, "C1", "=XLOOKUP(\"*a\",A:A,B:B,,\"2\")");
    set(&mut s, "C2", "=XLOOKUP(Z1,A:A,B:B,,Z2)");
    assert_eq!(value(&s, "C1"), value(&s, "C2"));
    assert_eq!(value(&s, "C1"), Value::Number(10.0), "the pattern matches Alpha");
}

/// The shape the fast path exists for: a column of a few thousand texts. The answer is the general
/// path's, and the lookup is over more rows than the block would cache.
#[test]
fn a_long_column_answers_like_the_general_path() {
    let mut s = workbook();
    for i in 0..5_000u32 {
        set(&mut s, &format!("A{}", i + 1), &format!("Item {i}"));
        set(&mut s, &format!("B{}", i + 1), &format!("{}", i * 2));
    }
    for (needle, expect) in [("Item 0", Some(0.0)), ("Item 4321", Some(8642.0)), ("Item 4999", Some(9998.0)), ("Item 5000", None)] {
        let (fast, slow) = both(&mut s, needle);
        assert_eq!(fast, slow, "needle {needle}");
        match expect {
            Some(n) => assert_eq!(fast, Value::Number(n), "needle {needle}"),
            None => assert_eq!(fast, Value::Error(cella_core::CellError::NA), "needle {needle}"),
        }
    }
}
