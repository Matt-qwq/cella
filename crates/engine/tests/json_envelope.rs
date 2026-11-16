//! The JSON workbook format: the versioned envelope, the fields an older build keeps for a newer
//! one, the caps that refuse a damaged or hostile file, and the golden bytes of a small workbook.
//!
//! `GRIDCRAFT_BLESS_GOLDEN=1` rewrites `tests/golden/json_envelope_v1.json`. Only do that for a
//! deliberate format change (and bump `json_format::FORMAT_VERSION` when the change drops or
//! renames a field, so an older build keeps the old one instead of losing it).

use std::path::PathBuf;

use gridcraft_engine::Session;
use gridcraft_engine::io;
use gridcraft_engine::json_format::{self, Limits};
use gridcraft_engine::model::Workbook;
use serde_json::{Value, json};

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.execute(id, params).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn workbook(s: &Session) -> &Workbook {
    &s.doc().expect("a document").wb
}

fn save(wb: &Workbook) -> Vec<u8> {
    io::save_bytes(wb, "book.json").expect("a JSON save")
}

/// The bytes of a small open, checked in as the golden file.
fn golden(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden").join(name)
}

fn check_golden(name: &str, actual: &[u8]) {
    let path = golden(name);
    if std::env::var_os("GRIDCRAFT_BLESS_GOLDEN").is_some() {
        std::fs::write(&path, actual).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        return;
    }
    let expected = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert!(
        expected == actual,
        "{name} changed: the writer gave {} bytes, the golden file has {}. If this is deliberate, bump \
         json_format::FORMAT_VERSION and re-bless with GRIDCRAFT_BLESS_GOLDEN=1.",
        actual.len(),
        expected.len()
    );
}

/// A workbook with a bit of everything the model holds: text, a number, a formula, a style, a
/// number format, a comment, a tab colour and a second sheet.
fn small() -> Session {
    let mut s = session();
    for (cell, input) in [("A1", "Item"), ("B1", "Qty"), ("A2", "Äpfel ✓"), ("B2", "3.5"), ("A3", "Pears"), ("B3", "2"), ("B4", "=SUM(B2:B3)")] {
        run(&mut s, "cell.set", json!({"cell": cell, "input": input}));
    }
    run(&mut s, "home.bold", json!({"range": "A1:B1"}));
    run(&mut s, "home.numberFormat", json!({"range": "B2:B4", "format": "0.00"}));
    run(&mut s, "review.newComment", json!({"cell": "A2", "text": "bought twice"}));
    run(&mut s, "sheet.tabColor", json!({"color": "#FF8800"}));
    s
}

/// A workbook using the model's less common corners, for the round trip that must keep every field.
fn rich() -> Session {
    let mut s = small();
    run(&mut s, "home.mergeCells", json!({"range": "A6:B6"}));
    run(&mut s, "cell.set", json!({"cell": "A6", "input": "merged"}));
    run(&mut s, "selection.set", json!({"range": "B2"}));
    run(&mut s, "view.freezePanes", json!({}));
    run(&mut s, "insert.table", json!({"range": "A1:B3", "header": true}));
    run(&mut s, "formulas.defineName", json!({"name": "Qty", "refersTo": "=Sheet1!$B$2:$B$3"}));
    run(&mut s, "home.insertSheet", json!({"name": "Second"}));
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "=Sheet1!B4*2"}));
    s
}

#[test]
fn every_field_survives_a_round_trip() {
    let s = rich();
    let before = workbook(&s).clone();
    let (after, warnings) = io::open_bytes("book.json", &save(&before)).expect("a JSON open");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(after, before, "a load + save changed the workbook");
}

#[test]
fn unknown_fields_at_the_envelope_and_inside_a_sheet_survive_a_load_and_save() {
    let bytes = save(workbook(&small()));
    let mut doc: Value = serde_json::from_slice(&bytes).expect("our own bytes parse");
    assert_eq!(doc["formatVersion"], json!(json_format::FORMAT_VERSION));
    // A newer build's data: a key of the envelope, one of the workbook object and one of a sheet.
    doc["graftVersion"] = json!("2.0-draft");
    doc["futureTotals"] = json!({"rolled": [1, 2, 3]});
    doc["workbook"]["futureCalcFlag"] = json!(true);
    doc["workbook"]["sheets"][0]["futureSheetThing"] = json!({"kept": "as it was"});
    let touched = serde_json::to_vec_pretty(&doc).expect("re-encode");

    let kept = io::open_bytes("book.json", &touched).expect("a JSON open").0;
    let written: Value = serde_json::from_slice(&save(&kept)).expect("re-parse");

    assert_eq!(written["graftVersion"], json!("2.0-draft"));
    assert_eq!(written["futureTotals"], json!({"rolled": [1, 2, 3]}));
    assert_eq!(written["workbook"]["futureCalcFlag"], json!(true));
    assert_eq!(written["workbook"]["sheets"][0]["futureSheetThing"], json!({"kept": "as it was"}));
    // And ours are still written once, from the model.
    assert_eq!(written["formatVersion"], json!(json_format::FORMAT_VERSION));
    assert_eq!(written["workbook"]["sheets"][0]["name"], json!("Sheet1"));
}

#[test]
fn a_file_without_an_envelope_still_loads_and_is_upgraded_on_save() {
    // Version 0: the bare workbook JSON every build before the envelope wrote.
    let bytes = save(workbook(&small()));
    let doc: Value = serde_json::from_slice(&bytes).expect("parse");
    let mut book = doc["workbook"].clone();
    // A field this build does not model, in the bare form a build without the envelope wrote.
    book["futureBareField"] = json!("kept too");
    let bare = serde_json::to_vec_pretty(&book).expect("re-encode");
    assert!(!bare.windows(13).any(|w| w == b"formatVersion"), "the fixture must have no envelope");

    let (wb, warnings) = io::open_bytes("book.json", &bare).expect("a bare JSON open");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(wb.sheets.len(), 1);
    assert_eq!(wb.sheets.first().map(|s| s.name.as_str()), Some("Sheet1"));
    assert_eq!(
        wb.sheets.first().and_then(|s| s.cell(gridcraft_engine::core::CellRef::parse("A2").expect("A2"))).map(|c| c.value.clone()),
        Some(gridcraft_engine::core::Value::Text("Äpfel ✓".into()))
    );

    // Saving it again writes version 1, keeping everything the old file held.
    let written: Value = serde_json::from_slice(&save(&wb)).expect("re-parse");
    assert_eq!(written["formatVersion"], json!(json_format::FORMAT_VERSION));
    assert_eq!(written["workbook"]["sheets"][0]["name"], doc["workbook"]["sheets"][0]["name"]);
    assert_eq!(written["workbook"]["futureBareField"], json!("kept too"), "a bare file's unknown field is kept too");
}

#[test]
fn a_truncated_or_hostile_file_errors_instead_of_panicking() {
    let good = save(workbook(&small()));
    let mut cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("truncated mid-object", good[..good.len() / 2].to_vec()),
        ("cut between tokens", good[..good.len() - 2].to_vec()),
        ("garbage", b"not json at all".to_vec()),
        ("an array", b"[]".to_vec()),
        ("null", b"null".to_vec()),
        ("a number", b"7".to_vec()),
        ("no workbook", br#"{"formatVersion":1}"#.to_vec()),
        ("a version that is not a number", br#"{"formatVersion":"one","workbook":{}}"#.to_vec()),
        ("a negative version", br#"{"formatVersion":-1,"workbook":{}}"#.to_vec()),
        ("sheets that are not a list", br#"{"formatVersion":1,"workbook":{"sheets":"nope"}}"#.to_vec()),
        ("a number too large to hold", br#"{"formatVersion":1e400,"workbook":{}}"#.to_vec()),
        ("deep nesting", "[".repeat(20_000).into_bytes()),
    ];
    for (what, bytes) in cases.drain(..) {
        let err = io::open_bytes("book.json", &bytes).expect_err(what).to_string();
        assert!(err.starts_with("not a GridCraft JSON workbook"), "{what}: {err}");
    }
}

#[test]
fn a_file_over_the_cap_is_refused_with_a_clear_message() {
    let bytes = save(workbook(&small()));
    // The production caps take our own file, so the limits below are the only thing refusing it.
    io::open_bytes("book.json", &bytes).expect("under the default caps");

    let tight = Limits { max_bytes: 1024, max_string: json_format::DEFAULT_MAX_STRING };
    let err = json_format::read_with(&bytes, &tight).expect_err("over the cap").to_string();
    assert!(err.contains("over the 1024 bytes one file may be"), "{err}");
    assert!(err.contains("GRIDCRAFT_MAX_JSON_MB"), "the message must say how to raise the cap: {err}");

    // One string far larger than the cap, in a file that is itself small enough.
    let long = json_format::write(&Workbook::new()).expect("write");
    let mut doc: Value = serde_json::from_slice(&long).expect("parse");
    doc["workbook"]["sheets"][0]["name"] = json!("x".repeat(4096));
    let one_long = serde_json::to_vec(&doc).expect("re-encode");
    let roomy = Limits { max_bytes: 1 << 20, max_string: 1024 };
    let err = json_format::read_with(&one_long, &roomy).expect_err("string over the cap").to_string();
    assert!(err.contains("over the 1024 bytes a single string may be"), "{err}");
}

#[test]
fn the_writers_output_for_a_small_workbook_is_stable() {
    check_golden("json_envelope_v1.json", &save(workbook(&small())));
}

#[test]
fn the_golden_workbook_opens_and_saves_back_unchanged() {
    let expected = std::fs::read(golden("json_envelope_v1.json")).expect("the golden file");
    let (wb, warnings) = io::open_bytes("book.json", &expected).expect("the golden file opens");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(save(&wb) == expected, "the golden workbook saved back differently");
}
