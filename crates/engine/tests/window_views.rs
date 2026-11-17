//! One view per window. View ▸ New Window puts a second window on the *same* workbook — no copy of
//! it, no second undo history, one save — but each window shows the document its own way: its own
//! active sheet, its own selection, and the selection it left each sheet with. The active window's
//! view is the one every command reads, so a command run after Switch Windows acts where that window
//! is looking, while a document-wide command reaches the whole workbook whichever window is active.

use cella_engine::core::{CellRef, Value};
use cella_engine::{Session, WindowId};
use serde_json::{Value as Json, json};

/// A workbook with two sheets: Sheet1 and Sheet2.
fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    run(&mut s, "home.insertSheet", json!({}));
    s
}

fn run(s: &mut Session, cmd: &str, params: Json) -> Json {
    s.execute(cmd, params).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn new_window(s: &mut Session) -> WindowId {
    WindowId(run(s, "view.newWindow", json!({}))["window"].as_u64().expect("the new window's id"))
}

/// What the document is showing: its active sheet and its selection.
fn view(s: &Session) -> (usize, String) {
    let d = s.doc().expect("a document");
    (d.wb.active_sheet, d.selection.a1())
}

/// What a window keeps: its own view, whether or not it is the live one.
fn window_view(s: &Session, window: WindowId) -> (usize, String, String) {
    let v = s.window_view(window).expect("the window is open");
    (v.sheet, v.sheet_name.clone(), v.selection.a1())
}

/// A cell's text on a sheet by index.
fn cell(s: &Session, sheet: usize, at: &str) -> String {
    let d = s.doc().expect("a document");
    let sh = d.wb.sheets.get(sheet).expect("the sheet");
    sh.value(CellRef::parse(at).expect("a cell address")).display()
}

#[test]
fn each_window_keeps_its_own_active_sheet_and_selection() {
    let mut s = session();
    // Window 1 looks at Sheet1, B2:C3.
    run(&mut s, "sheet.activate", json!({"sheet": 0}));
    run(&mut s, "selection.set", json!({"range": "B2:C3"}));
    let first = s.active_window().expect("a document opens with a window");
    let second = new_window(&mut s);
    assert_ne!(first, second);

    // The new window opens on what the document shows, then goes its own way.
    assert_eq!(window_view(&s, second), (0, "Sheet1".into(), "B2:C3".into()), "a new window starts on the sheet and selection the document shows");
    run(&mut s, "sheet.activate", json!({"sheet": 1}));
    run(&mut s, "selection.set", json!({"range": "D4"}));
    assert_eq!(view(&s), (1, "D4".into()), "the second window is on Sheet2, D4");

    // Switching windows brings each window's own view back.
    run(&mut s, "window.activate", json!({"index": 0}));
    assert_eq!(s.active_window(), Some(first));
    assert_eq!(view(&s), (0, "B2:C3".into()), "window 1's own sheet and selection");
    assert_eq!(window_view(&s, second), (1, "Sheet2".into(), "D4".into()), "window 2 keeps its own while window 1 shows");
    run(&mut s, "window.activate", json!({"window": second.0}));
    assert_eq!(view(&s), (1, "D4".into()), "and back again, by id this time");

    // A command acts in the active window's view: typing goes to the sheet and cell that window
    // shows, and each window's typing lands in its own place on the shared workbook.
    run(&mut s, "cell.set", json!({"input": "two"}));
    run(&mut s, "window.activate", json!({"index": 0}));
    run(&mut s, "cell.set", json!({"input": "one"}));
    assert_eq!(cell(&s, 0, "B2"), "one", "window 1 typed on Sheet1 at its own selection");
    assert_eq!(cell(&s, 1, "D4"), "two", "window 2 typed on Sheet2 at its own selection");
    assert_eq!(cell(&s, 0, "D4"), "", "one workbook, but the views do not bleed into each other");
    // Window 1 moves to a selection of its own, on its own sheet.
    run(&mut s, "selection.set", json!({"range": "A1:A2"}));

    // A window swap is view state, never an edit: switching back and forth changes nothing that
    // would have to be saved, and the undo history is the shared one.
    let path = std::env::temp_dir().join(format!("cella-window-views-{}.xlsx", std::process::id()));
    run(&mut s, "file.save", json!({"path": path.to_string_lossy()}));
    for _ in 0..3 {
        run(&mut s, "window.activate", json!({"index": 1}));
        assert_eq!(view(&s), (1, "D4".into()));
        run(&mut s, "window.activate", json!({"index": 0}));
        assert_eq!(view(&s), (0, "A1:A2".into()), "each window comes back to where it was left");
    }
    assert!(!s.doc().expect("a document").is_dirty(), "switching windows is not a change to save");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(cell(&s, 0, "B2"), "", "one undo history for both windows");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn closing_a_window_keeps_the_others_view() {
    let mut s = session();
    run(&mut s, "sheet.activate", json!({"sheet": 0}));
    run(&mut s, "selection.set", json!({"range": "B2"}));
    let first = s.active_window().expect("a window");
    let second = new_window(&mut s);
    run(&mut s, "sheet.activate", json!({"sheet": 1}));
    run(&mut s, "selection.set", json!({"range": "C3"}));

    // Closing the window the document was showing hands the document to the other one, which finds
    // its own sheet and selection where it left them.
    let r = run(&mut s, "view.closeWindow", json!({"window": second.0, "force": true}));
    assert_eq!((r["closed"].as_bool(), r["document"].as_bool()), (Some(true), Some(false)), "{r}");
    assert_eq!(s.window_count(), 1);
    assert_eq!(s.documents().len(), 1, "the document stays open");
    assert_eq!(s.active_window(), Some(first), "the window left takes the focus");
    assert_eq!(view(&s), (0, "B2".into()), "and the document shows that window's view");
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "after"}));
    assert_eq!(cell(&s, 0, "A1"), "after", "commands act in the window that is left");

    // The other way round: a window that is not showing closes without moving the view.
    run(&mut s, "selection.set", json!({"range": "B2"}));
    let kept = s.active_window().expect("a window");
    let other = new_window(&mut s);
    run(&mut s, "window.activate", json!({"window": kept.0}));
    run(&mut s, "selection.set", json!({"range": "E5"}));
    let r = run(&mut s, "view.closeWindow", json!({"window": other.0}));
    assert_eq!(r["closed"], json!(true), "{r}");
    assert_eq!(s.active_window(), Some(kept));
    assert_eq!(view(&s), (0, "E5".into()), "closing another window leaves the view alone");
}

#[test]
fn window_list_reports_each_windows_view() {
    let mut s = session();
    run(&mut s, "sheet.activate", json!({"sheet": 0}));
    run(&mut s, "selection.set", json!({"range": "B2:C3"}));
    let first = s.active_window().expect("a window");
    let second = new_window(&mut s);
    run(&mut s, "sheet.activate", json!({"sheet": 1}));
    run(&mut s, "selection.set", json!({"range": "D4"}));
    // Back on the first window: the list has to report the second one's own view, not the active
    // window's.
    run(&mut s, "window.activate", json!({"index": 0}));

    let list = run(&mut s, "window.list", json!({}));
    let rows = list.as_array().cloned().unwrap_or_default();
    assert_eq!(rows.len(), 2, "{list}");
    assert_eq!((rows[0]["window"].as_u64(), rows[0]["index"].as_u64()), (Some(first.0), Some(0)), "{list}");
    assert_eq!((rows[0]["sheet"].as_u64(), rows[0]["selection"].as_str()), (Some(0), Some("B2:C3")), "{list}");
    assert_eq!(rows[0]["active"], json!(true), "{list}");
    assert_eq!(
        (rows[1]["window"].as_u64(), rows[1]["sheet"].as_u64(), rows[1]["selection"].as_str()),
        (Some(second.0), Some(1), Some("D4")),
        "{list}"
    );
    assert_eq!(rows[1]["active"], json!(false), "{list}");

    // View ▸ Switch Windows lists every window of every document the same way.
    let switching = run(&mut s, "view.switchWindows", json!({}));
    let rows = switching.as_array().cloned().unwrap_or_default();
    assert_eq!((rows[1]["sheet"].as_u64(), rows[1]["selection"].as_str()), (Some(1), Some("D4")), "{switching}");
}

#[test]
fn a_document_wide_command_reaches_the_workbook_whichever_window_is_active() {
    let mut s = session();
    // Window 1 looks at Sheet2 (the sheet the insert above made active), window 2 at Sheet1.
    run(&mut s, "selection.set", json!({"range": "B2"}));
    let first = s.active_window().expect("a window");
    let second = new_window(&mut s);
    run(&mut s, "sheet.activate", json!({"sheet": 0}));
    run(&mut s, "selection.set", json!({"range": "A1"}));

    // A document-wide command (a new sheet) reaches the whole workbook from either window: it goes
    // in before the sheet the active window shows, and both windows see it.
    let r = run(&mut s, "home.insertSheet", json!({"name": "Sheet3"}));
    assert_eq!(r["name"], json!("Sheet3"), "the sheet is added to the document, not to a window");
    let names: Vec<String> = s.doc().expect("a document").wb.sheets.iter().map(|s| s.name.clone()).collect();
    assert_eq!(names, vec!["Sheet1", "Sheet3", "Sheet2"], "the sheet landed after the active window's sheet");
    assert_eq!(window_view(&s, first), (1, "Sheet2".into(), "B2".into()), "the parked window's view did not move");

    // A window's view names its sheet, not just its index: Sheet2 moved from index 1 to 2 and the
    // window still shows Sheet2 where it left it.
    run(&mut s, "window.activate", json!({"window": first.0}));
    assert_eq!(view(&s), (2, "B2".into()), "Sheet2 at its new index");
    assert_eq!(window_view(&s, second), (1, "Sheet3".into(), "A1".into()), "the window that was showing keeps the sheet the insert made active");

    // And when a sheet goes away elsewhere, the window that was parked on it finds it again.
    run(&mut s, "home.deleteSheet", json!({"sheet": 0}));
    run(&mut s, "window.activate", json!({"window": second.0}));
    let (sheet, name, selection) = window_view(&s, second);
    assert_eq!(name, "Sheet3", "the window still shows the sheet it was on");
    assert_eq!(
        s.doc().expect("a document").wb.sheets.get(sheet).map(|s| s.name.clone()).unwrap_or_default(),
        "Sheet3",
        "found by name at its new index"
    );
    assert_eq!((sheet, selection.as_str()), (0, "A1"), "at the index and selection it is owed");
    assert_eq!(view(&s), (0, "A1".into()));
    assert_eq!(s.active_window(), Some(second));
}

#[test]
fn a_new_window_shares_the_undo_history_and_the_save() {
    let mut s = session();
    let path = std::env::temp_dir().join(format!("cella-window-shared-{}.xlsx", std::process::id()));
    run(&mut s, "file.save", json!({"path": path.to_string_lossy()}));
    let first = s.active_window().expect("a window");
    let second = new_window(&mut s);
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "typed in window 2"}));
    assert_eq!(cell(&s, 1, "A1"), "typed in window 2");
    run(&mut s, "window.activate", json!({"index": 0}));
    assert_eq!(cell(&s, 1, "A1"), "typed in window 2", "the same workbook, so the first window shows it too");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(cell(&s, 1, "A1"), "", "one undo history");
    assert_eq!((s.active_window(), s.window_count()), (Some(first), 2));
    // A value that is not text still lands where the window points.
    run(&mut s, "window.activate", json!({"window": second.0}));
    run(&mut s, "cell.set", json!({"cell": "B2", "input": "=1+1"}));
    assert_eq!(s.doc().expect("a document").wb.sheets[1].value(CellRef::parse("B2").expect("a cell")), Value::number(2.0));
    let _ = std::fs::remove_file(&path);
}
