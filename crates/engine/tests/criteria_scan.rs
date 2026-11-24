//! `COUNTIF` and `SUMIF` answer from a scan of the stored cells; `COUNTIFS` and `SUMIFS` take the
//! library's path, which builds the block into an array first. With one criteria pair the two
//! spellings are the same question, so the same sheet asked both ways is the differential: every
//! criterion below must answer alike, and the cell it counts must be the cell Excel counts.
//!
//! The table is the one that makes a criterion interesting: numbers, a number spelled as text, text
//! in two cases, a wildcard literal, a boolean, an error, a blank slot in the middle, and a text
//! cell holding an empty string.

use cella_core::{CellError, CellRef, Value};
use cella_engine::Session;
use serde_json::json;

fn set(s: &mut Session, cell: &str, input: &str) {
    s.execute("cell.set", json!({"cell": cell, "input": input})).expect("set");
}

fn value(s: &Session, cell: &str) -> Value {
    let (row, col) = parse(cell);
    s.doc().expect("document").wb.active().expect("sheet").value(CellRef::new(row, col))
}

fn parse(cell: &str) -> (u32, u32) {
    let mut col = 0u32;
    let mut digits = String::new();
    for c in cell.chars() {
        if c.is_ascii_alphabetic() {
            col = col * 26 + (c.to_ascii_uppercase() as u32 - 'A' as u32 + 1);
        } else {
            digits.push(c);
        }
    }
    (digits.parse::<u32>().expect("a row") - 1, col - 1)
}

/// The sheet the cases look at: A is the criteria column, B the numbers to sum, and both have a
/// hole (row 6) so "a blank" is a real slot.
fn sheet() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    let column_a = ["10", ">20", "text", "Text", "a*b", "TRUE", "#N/A", "'7", "''", "0"];
    for (i, input) in column_a.iter().enumerate() {
        let row = if i < 5 { i + 1 } else { i + 2 };
        set(&mut s, &format!("A{row}"), input);
        // One target cell is an error: a row the criterion matches is the library's `#DIV/0!`,
        // and a row it does not match must not care.
        let target = if row == 4 { "#DIV/0!".to_string() } else { format!("{row}") };
        set(&mut s, &format!("B{row}"), &target);
    }
    s
}

/// Every criterion worth asking of that column: comparisons on numbers and text, equality, the
/// wildcards, blank and not-blank, a boolean and an error, and the numeric text `7`.
const CRITERIA: &[&str] = &[
    ">5", ">=10", "<3", "<=0", "=10", "<>10", "10", "0", "7", "text", "TEXT", "a~*b", "a*", "*b", "?ext", "", "<>", "<>*", "TRUE", "#N/A", ">a", "<t",
];

#[test]
fn countif_answers_like_countifs_with_one_pair() {
    let mut s = sheet();
    for (i, crit) in CRITERIA.iter().enumerate() {
        let at = format!("D{}", i + 1);
        set(&mut s, &at, &format!("=COUNTIF(A1:A10, \"{crit}\")"));
        set(&mut s, &format!("E{}", i + 1), &format!("=COUNTIFS(A1:A10, \"{crit}\")"));
        assert_eq!(value(&s, &at), value(&s, &format!("E{}", i + 1)), "criterion `{crit}`");
    }
}

#[test]
fn sumif_answers_like_sumifs_with_one_pair() {
    let mut s = sheet();
    for (i, crit) in CRITERIA.iter().enumerate() {
        let at = format!("D{}", i + 1);
        set(&mut s, &at, &format!("=SUMIF(A1:A10, \"{crit}\", B1:B10)"));
        set(&mut s, &format!("E{}", i + 1), &format!("=SUMIFS(B1:B10, A1:A10, \"{crit}\")"));
        assert_eq!(value(&s, &at), value(&s, &format!("E{}", i + 1)), "criterion `{crit}`");
    }
}

/// `SUMIF(range, criterion)` — no sum range, so the range is both what is tested and what is
/// added: the one-column shape, and `SUMIFS(range, range, criterion)` asks the same question.
#[test]
fn sumif_over_one_column_answers_like_sumifs() {
    let mut s = sheet();
    for (i, crit) in CRITERIA.iter().enumerate() {
        let at = format!("D{}", i + 1);
        set(&mut s, &at, &format!("=SUMIF(A1:A10, \"{crit}\")"));
        set(&mut s, &format!("E{}", i + 1), &format!("=SUMIFS(A1:A10, A1:A10, \"{crit}\")"));
        assert_eq!(value(&s, &at), value(&s, &format!("E{}", i + 1)), "criterion `{crit}`");
    }
    // `>5` matches the number 10 in A1 and nothing else (the text 7 is not a number here).
    set(&mut s, "D1", "=SUMIF(A1:A10, \">5\")");
    assert_eq!(value(&s, "D1"), Value::Number(10.0));
}

/// The answers themselves on a column whose content is known by reading it: a number, a text, a
/// blank slot, another number and a text that spells a number.
#[test]
fn countif_counts_what_the_sheet_holds() {
    let mut s = Session::new();
    s.new_workbook();
    set(&mut s, "A1", "10");
    set(&mut s, "A2", "text");
    set(&mut s, "A4", "7");
    set(&mut s, "A5", "'7");
    let count = |s: &mut Session, crit: &str| -> Value {
        set(s, "D1", &format!("=COUNTIF(A1:A5, \"{crit}\")"));
        value(s, "D1")
    };
    assert_eq!(count(&mut s, ">5"), Value::Number(2.0), "10 and 7; the text 7 is not a number here");
    assert_eq!(count(&mut s, "7"), Value::Number(2.0), "the number 7 and the text 7");
    assert_eq!(count(&mut s, ""), Value::Number(1.0), "the blank slot only");
    assert_eq!(count(&mut s, "<>"), Value::Number(4.0), "everything but the blank");
    assert_eq!(count(&mut s, "text"), Value::Number(1.0));
    assert_eq!(count(&mut s, "*"), Value::Number(2.0), "any text cell: the word and the text 7");
}

/// An error in the criteria column is a value a criterion can name: `#N/A` counts the error cell and
/// nothing else.
#[test]
fn an_error_cell_counts_only_when_the_criterion_names_it() {
    let mut s = sheet();
    set(&mut s, "D1", "=COUNTIF(A1:A10, \"#N/A\")");
    set(&mut s, "D2", "=COUNTIFS(A1:A10, \"#N/A\")");
    assert_eq!(value(&s, "D1"), value(&s, "D2"));
    set(&mut s, "D3", "=SUMIF(A1:A10, \"#N/A\", B1:B10)");
    set(&mut s, "D4", "=SUMIFS(B1:B10, A1:A10, \"#N/A\")");
    assert_eq!(value(&s, "D3"), value(&s, "D4"));
}

/// A formula in the counted column is a cell only the general path can price: the scan declines and
/// the answer is the library's — which is what the second spelling gives anyway.
#[test]
fn a_formula_in_the_column_still_answers() {
    let mut s = sheet();
    set(&mut s, "A11", "=SUM(1,2)");
    set(&mut s, "D1", "=COUNTIF(A1:A11, \">2\")");
    set(&mut s, "D2", "=COUNTIFS(A1:A11, \">2\")");
    assert_eq!(value(&s, "D1"), value(&s, "D2"));
    set(&mut s, "D3", "=SUMIF(A1:A11, \">2\", B1:B11)");
    set(&mut s, "D4", "=SUMIFS(B1:B11, A1:A11, \">2\")");
    assert_eq!(value(&s, "D3"), value(&s, "D4"));
}

/// A criterion over 255 UTF-16 units is `#VALUE!` for the single-criterion functions; the scan
/// parses with the same parser, so it is `#VALUE!` there too rather than a different answer.
#[test]
fn an_overlong_criterion_is_still_an_error() {
    let mut s = sheet();
    let long = "a".repeat(260);
    set(&mut s, "D1", &format!("=COUNTIF(A1:A10, \"{long}\")"));
    assert_eq!(value(&s, "D1"), Value::Error(CellError::Value));
    set(&mut s, "D2", &format!("=SUMIF(A1:A10, \"{long}\", B1:B10)"));
    assert_eq!(value(&s, "D2"), Value::Error(CellError::Value));
}
