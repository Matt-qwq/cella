//! The chunked defined-name store (`gridcraft_engine::cmd::stash`): the workbook data GridCraft
//! keeps in hidden defined names — scripts, custom views, the Watch Window list, extensions and
//! scenarios — has to survive a real xlsx file. Excel limits a string literal to 255 characters
//! and a formula to 8,192, and the xlsx writer drops the characters XML cannot carry, so a value
//! is split over names and those characters are escaped. These tests prove the file keeps it.

use std::sync::Arc;

use gridcraft_engine::Session;
use gridcraft_engine::cmd::stash;
use gridcraft_engine::model::{DefinedName, Workbook};
use serde_json::{Value as Json, json};

/// Excel's limits: characters in one string literal, and characters in one name's formula.
const LITERAL_UNITS: usize = 255;
const FORMULA_UNITS: usize = 8_192;

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn run(s: &mut Session, cmd: &str, params: Json) -> Json {
    s.execute(cmd, params).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

/// A path in the temp directory no other test uses.
fn temp_path(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("gridcraft-stash-{tag}-{}.xlsx", std::process::id()))
}

/// The string literals of a `"a"&"b""c"` formula, quotes undoubled.
fn literals(formula: &str) -> Vec<String> {
    formula.split("\"&\"").map(|lit| lit.trim_start_matches('"').trim_end_matches('"').replace("\"\"", "\"")).collect()
}

/// Fails on a name whose formula, or one of whose string literals, Excel would refuse.
fn assert_excel_limits(wb: &Workbook) {
    for n in &wb.names {
        assert!(n.formula.encode_utf16().count() <= FORMULA_UNITS, "{} is {} characters", n.name, n.formula.chars().count());
        if stash::item_key(&n.name).is_some() {
            for lit in literals(&n.formula) {
                assert!(lit.encode_utf16().count() <= LITERAL_UNITS, "{} holds a literal of {} characters", n.name, lit.chars().count());
            }
        }
    }
}

#[test]
fn a_script_longer_than_a_formula_round_trips_through_a_saved_file() {
    let mut s = session();
    // One step whose input is 20,000 characters: the scripts store, as JSON, is far past the
    // 8,192 characters one formula holds, so it takes several names.
    let text = "the quick brown fox ".repeat(1_000);
    let steps = json!([{"command": "cell.set", "params": {"cell": "A1", "input": text}}]);
    run(&mut s, "automate.newScript", json!({"name": "Big", "commands": steps}));
    let before = stash::get(&s.doc().expect("doc").wb, "scripts").expect("the script is stored");
    assert!(before.chars().count() > FORMULA_UNITS, "{} characters", before.chars().count());
    let chunks = s.doc().expect("doc").wb.names.iter().filter(|n| n.name.starts_with("_gc_scripts_")).count();
    assert!(chunks > 1, "the value is split over {chunks} name(s)");
    assert_excel_limits(&s.doc().expect("doc").wb);

    let path = temp_path("long-script");
    run(&mut s, "file.save", json!({"path": path.display().to_string()}));
    let mut opened = Session::new();
    run(&mut opened, "file.open", json!({"path": path.display().to_string()}));
    assert_eq!(stash::get(&opened.doc().expect("doc").wb, "scripts").as_deref(), Some(before.as_str()));
    assert_eq!(run(&mut opened, "automate.scriptSteps", json!({"name": "Big"})), steps, "the reading command gives the steps back");
    assert_excel_limits(&opened.doc().expect("doc").wb);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn characters_xml_cannot_carry_survive_a_saved_file() {
    // U+0001 is not an XML character and U+FFFF is not even a character: the xlsx writer drops
    // both, so the store writes them (and U+FFFE, and the marker itself) as U+FDD0 and four hex
    // digits. The text here is long enough to need several literals, so an escape is never split.
    let text = format!("a{}\u{FDD0}b\u{FFFF}c\u{FFFE}d{}", "\u{1}".repeat(40), "\u{2}".repeat(400));
    let mut wb = Workbook::new();
    stash::set(&mut wb, "odd", &text).expect("stored");
    assert_excel_limits(&wb);
    // What is held is the marker and digits, not the characters XML cannot carry.
    let formula = &wb.names.first().expect("one name").formula;
    assert!(formula.contains(stash::ESCAPE), "{formula:?}");
    assert!(!formula.contains('\u{FFFF}') && !formula.contains('\u{1}') && !formula.contains('\u{FFFE}'), "{formula:?}");
    let bytes = gridcraft_engine::io::save_bytes(&wb, "odd.xlsx").expect("save");
    let (back, warnings) = gridcraft_engine::io::open_bytes("odd.xlsx", &bytes).expect("open");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(stash::get(&back, "odd"), Some(text), "every character comes back as it went in");

    // Through a command's JSON store too: serde_json escapes U+0001 in the JSON text it writes
    // and leaves U+FFFF as it is — the character the writer would otherwise drop.
    let odd = "x\u{1}y\u{FFFF}z";
    let steps = json!([{"command": "cell.set", "params": {"cell": "A1", "input": odd}}]);
    let mut s = session();
    run(&mut s, "automate.newScript", json!({"name": "Odd", "commands": steps}));
    let bytes = run(&mut s, "file.saveBytes", json!({"format": "xlsx"}))["base64"].as_str().expect("base64").to_string();
    let mut opened = session();
    run(&mut opened, "file.open", json!({"name": "odd.xlsx", "base64": bytes}));
    assert_eq!(run(&mut opened, "automate.scriptSteps", json!({"name": "Odd"})), steps);
}

#[test]
fn a_workbook_that_keeps_the_old_single_name_is_still_read() {
    // A workbook written before the chunked store keeps the scripts in one `_sc_scripts` name.
    // Building it here means the name is exactly what such a file carries: one literal.
    let steps = json!([{"command": "home.bold", "params": {}}]);
    let json_text = json!({"Old": steps.clone()}).to_string();
    let formula = format!("\"{}\"", json_text.replace('"', "\"\""));
    let mut s = session();
    Arc::make_mut(&mut s.doc_mut().expect("doc").wb).names.push(DefinedName {
        name: "_sc_scripts".into(),
        scope: None,
        formula,
        comment: String::new(),
        hidden: true,
    });
    let listed = run(&mut s, "automate.allScripts", json!({}));
    assert!(listed.to_string().contains("Old"), "{listed}");
    assert_eq!(run(&mut s, "automate.scriptSteps", json!({"name": "Old"})), steps, "the old name is read");

    // A write replaces the old name with the chunked ones, keeping the script it held.
    run(&mut s, "automate.newScript", json!({"name": "New", "commands": []}));
    let wb = &s.doc().expect("doc").wb;
    assert!(wb.names.iter().all(|n| n.name != "_sc_scripts"), "the single name is gone");
    assert_eq!(wb.names.iter().find(|n| n.name == "_gc_scripts_0").map(|n| n.hidden), Some(true));
    assert_eq!(stash::get(wb, "scripts").map(|t| t.contains("Old") && t.contains("New")), Some(true));
}

#[test]
fn no_store_writes_a_literal_over_255_or_a_formula_over_8192() {
    let mut s = session();
    let text = "x".repeat(20_000);
    run(&mut s, "automate.newScript", json!({"name": "Huge", "commands": [{"command": "cell.set", "params": {"cell": "A1", "input": text}}]}));
    run(&mut s, "data.scenarioAdd", json!({"name": "Long", "changing": "A1", "values": [1], "comment": "y".repeat(20_000)}));
    run(&mut s, "view.customViews", json!({"save": "v"}));
    run(&mut s, "formulas.watchWindow", json!({"add": "A1:A9"}));
    run(&mut s, "ext.add", json!({"name": "E", "description": "d", "commands": [{"command": "home.bold", "params": {}}]}));
    let keys = ["scripts", "scenarios", "custom_views", "watch", "extensions"];
    let wb = &s.doc().expect("doc").wb;
    for key in keys {
        assert!(stash::get(wb, key).is_some(), "{key} is stored");
    }
    assert!(wb.names.iter().any(|n| n.name == "_gc_scripts_1"), "the long script is split over more than one name");
    assert_excel_limits(wb);

    // The writer re-emits each name's formula into the file: the limits hold there too, and every
    // store reads back unchanged.
    let bytes = gridcraft_engine::io::save_bytes(wb, "limits.xlsx").expect("save");
    let (back, warnings) = gridcraft_engine::io::open_bytes("limits.xlsx", &bytes).expect("open");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_excel_limits(&back);
    for key in keys {
        assert_eq!(stash::get(&back, key), stash::get(wb, key), "{key} after the round trip");
    }
}
