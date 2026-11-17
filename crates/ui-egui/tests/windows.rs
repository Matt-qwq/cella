//! View ▸ New Window is a second *window* on one workbook, never a second copy of it: both
//! windows show the same document, share its undo history, and closing one leaves it open. What
//! each window shows of it — its active sheet, its selection — is the window's own, and switching
//! windows brings that view back.

use cella_engine::Session;
use cella_ui_egui::{Services, SheetApp};
use serde_json::json;

fn app() -> SheetApp {
    let mut session = Session::new();
    session.new_workbook();
    SheetApp::new(session, Services::default())
}

/// What the grid paints in `cell` of the document the app shows. Two windows on one document read
/// the same workbook, so both answer with the same text.
fn shown(app: &SheetApp, cell: &str) -> String {
    let d = app.session.active().expect("a document");
    let sh = d.wb.active().expect("a sheet");
    let at = cella_engine::core::CellRef::parse(cell).expect("a cell address");
    cella_engine::display::edit_text(&d.wb, sh, at)
}

#[test]
fn view_new_window_opens_a_second_window_on_the_same_document() {
    let mut app = app();
    let document = app.session.documents()[0].uid;
    let first = app.session.active_window().expect("a document opens with a window");
    assert_eq!(app.session.windows().len(), 1);

    app.run("view.newWindow", json!({})).expect("the second window opens");

    // Two windows, one document: nothing was copied, so both windows are the same workbook.
    assert_eq!(app.session.documents().len(), 1, "no clone of the document");
    assert_eq!(app.session.documents()[0].uid, document, "the same document, not a copy of it");
    assert_eq!(app.session.windows().len(), 2, "two windows on it");
    let second = app.session.active_window().expect("the new window is focused");
    assert_ne!(first, second);
    // The titles are the document's own, numbered from the second window on as Excel does.
    assert_eq!(app.window_label(first), "Book1");
    assert_eq!(app.window_label(second), "Book1:2");
    assert_eq!(app.session.documents()[0].display_title(), "Book1", "a document title has no window suffix");

    // A value typed in the second window is what the first window shows — one workbook — and the
    // undo history is the shared one.
    app.run("cell.set", json!({"cell": "B2", "input": "42"})).expect("the value goes in");
    app.run("window.activate", json!({"index": 0})).expect("back to the first window");
    assert_eq!(app.session.active_window(), Some(first));
    assert_eq!(shown(&app, "B2"), "42", "the first window shows what the second one typed");
    app.run("edit.undo", json!({})).expect("undo");
    assert_eq!(shown(&app, "B2"), "", "one undo history, shared by both windows");

    // Closing a window leaves the document open; the last window of a document closes it.
    app.run("view.closeWindow", json!({"window": second.0})).expect("the second window closes");
    assert_eq!(app.session.windows().len(), 1);
    assert_eq!(app.session.documents().len(), 1, "the document stays open");
    assert_eq!(app.session.active_window(), Some(first));
    app.run("view.closeWindow", json!({"force": true})).expect("the last window closes");
    assert!(app.session.documents().is_empty(), "the last window closed the document");
}

#[test]
fn the_window_list_names_both_windows_and_switches_between_them() {
    let mut app = app();
    app.run("view.newWindow", json!({})).expect("a second window");
    let listed = app.session.run("view.switchWindows", json!({})).expect("the window list");
    let list = listed.as_array().expect("a list");
    assert_eq!(list.len(), 2, "{listed}");
    assert_eq!(list[0]["title"], json!("Book1"), "{listed}");
    assert_eq!(list[1]["title"], json!("Book1"), "both are the same document");
    assert_eq!(list[1]["active"], json!(true), "the new window has the focus: {listed}");
    // Switching windows moves the focus, as View ▸ Switch Windows does.
    app.run("window.activate", json!({"index": 0})).expect("switch to the first window");
    assert_eq!(app.session.active_window(), Some(cella_engine::WindowId(list[0]["window"].as_u64().expect("an id"))));
    let listed = app.session.run("view.switchWindows", json!({})).expect("the window list");
    assert_eq!(listed[0]["active"], json!(true));
    assert_eq!(listed[1]["active"], json!(false));
    // Hide takes the window out of the way without closing the document; Unhide brings it back.
    app.run("view.hideWindow", json!({"index": 1})).expect("hide the second window");
    assert_eq!(app.session.windows().len(), 2, "a hidden window is still open");
    assert_eq!(app.session.run("view.switchWindows", json!({})).expect("the window list")[1]["hidden"], json!(true));
    app.run("view.unhideWindow", json!({"index": 1})).expect("unhide it");
    assert_eq!(app.session.run("view.switchWindows", json!({})).expect("the window list")[1]["hidden"], json!(false));
    assert_eq!(app.session.documents().len(), 1);
}

#[test]
fn the_control_channel_names_the_window_it_drives() {
    // `ui.inspect` reports the window list and which window the control channel drives and
    // screenshots (`mainWindow`), and `ui.screenshot` keeps capturing that one window.
    let app = app();
    let h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    let main = h.state().session.active_window().expect("the main window");
    let inspected = cella_ui_egui::control::inspect(h.state(), &h.ctx);
    assert_eq!(inspected["mainWindow"], json!(main.0), "{inspected}");
    assert_eq!(inspected["windows"].as_array().map(Vec::len), Some(1), "{inspected}");

    let mut h = h;
    h.state_mut().run("view.newWindow", json!({})).expect("a second window");
    h.run_steps(3);
    let inspected = cella_ui_egui::control::inspect(h.state(), &h.ctx);
    assert_eq!(h.state().session.windows().len(), 2, "{inspected}");
    assert_eq!(h.state().session.documents().len(), 1, "two windows on one document");
    let windows = inspected["windows"].as_array().expect("the window list");
    assert_eq!(windows.len(), 2, "{inspected}");
    assert_eq!(inspected["mainWindow"], json!(main.0), "the control channel keeps its own window: {inspected}");
    assert_eq!(windows[1]["title"], json!("Book1:2"), "{inspected}");
    // The frame still belongs to the app's own window: its grid is what `ui.cell.click` maps onto.
    assert_eq!(h.state().painted_window(), Some(main));
}

/// What the app shows: the active sheet and the selection of the window it is on.
fn view(app: &SheetApp) -> (usize, String) {
    let d = app.session.doc().expect("a document");
    (d.wb.active_sheet, d.selection.a1())
}

/// Switching windows is switching views: each window keeps its own active sheet and selection, and
/// the one a command reads is the view of the window the user is on.
#[test]
fn switching_windows_restores_that_windows_sheet_and_selection() {
    let mut app = app();
    app.run("home.insertSheet", json!({})).expect("a second sheet");
    app.run("sheet.activate", json!({"sheet": 0})).expect("Sheet1");
    app.run("selection.set", json!({"range": "B2:C3"})).expect("a selection");
    let first = app.session.active_window().expect("a window");
    app.run("view.newWindow", json!({})).expect("a second window");
    let second = app.session.active_window().expect("the new window");
    assert_eq!((view(&app), app.session.windows().len()), ((0, "B2:C3".to_string()), 2), "the new window opens on what the document shows");
    app.run("sheet.activate", json!({"sheet": 1})).expect("Sheet2");
    app.run("selection.set", json!({"range": "D4"})).expect("and a selection of its own");
    assert_eq!(view(&app), (1, "D4".to_string()), "the second window is on Sheet2, D4");

    // Switch Windows: the view of the window switched to is the one the grid draws and commands act
    // in, and it is the view that window had, not the one it was left at.
    app.run("window.activate", json!({"index": 0})).expect("switch to the first window");
    assert_eq!((app.session.active_window(), view(&app)), (Some(first), (0, "B2:C3".to_string())));
    app.run("cell.set", json!({"input": "one"})).expect("typed in the first window");
    app.run("window.activate", json!({"window": second.0})).expect("switch to the second window");
    assert_eq!((app.session.active_window(), view(&app)), (Some(second), (1, "D4".to_string())));
    app.run("cell.set", json!({"input": "two"})).expect("typed in the second window");
    assert_eq!(shown(&app, "D4"), "two");

    // A painted frame paints every window and leaves the session on the window the user works in:
    // its own sheet and selection are what the next command sees, not the last window drawn.
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    assert_eq!(
        (h.state().session.active_window(), view(h.state())),
        (Some(second), (1, "D4".to_string())),
        "the frame ends on the window the user is in"
    );
    // The harness is a shell with one viewport: the app draws one window at a time in it. Bind it to
    // the first window, as the shell does, and paint again: it draws — and ends on — that window's
    // own view, with the other window's view kept for when it is drawn next.
    let mut h = h;
    h.state_mut().window_root = Some(first);
    h.run_steps(3);
    assert_eq!(
        (h.state().session.active_window(), view(h.state())),
        (Some(first), (0, "B2:C3".to_string())),
        "painting a window brings its own view back"
    );
    assert_eq!(
        h.state().session.window_view(second).map(|v| (v.sheet, v.selection.a1())),
        Some((1, "D4".to_string())),
        "the window that is not being drawn keeps its own view"
    );
    assert_eq!(h.state().session.documents().len(), 1, "one document, one workbook, two views");
}

/// The two windows of a new second window: the app's first one and the one View ▸ New Window
/// added (which has the focus).
fn two_windows(app: &mut SheetApp) -> (cella_engine::WindowId, cella_engine::WindowId) {
    app.run("view.newWindow", json!({})).expect("a second window");
    let ids: Vec<_> = app.session.windows().into_iter().map(|(_, w)| w.id).collect();
    assert_eq!(ids.len(), 2, "two windows on the document");
    (ids[0], ids[1])
}

/// What the cell draft holds, when one is open.
fn draft(app: &SheetApp) -> Option<&str> {
    app.editor.as_ref().map(|e| e.text.as_str())
}

/// Two windows on one document share its workbook but not their interface state: a draft typed in
/// one is that window's, so switching windows shows the other window without it.
#[test]
fn a_draft_stays_in_the_window_it_was_typed_in() {
    let mut app = app();
    let (first, second) = two_windows(&mut app);
    assert_eq!(app.session.active_window(), Some(second));
    assert_eq!(app.ui_window(), Some(second), "the new window holds the interface state");

    // A formula typed into the second window's formula bar: a draft, not committed yet.
    app.begin_edit(Some("=SUM(A1:A9)".into()), true);
    assert_eq!(draft(&app), Some("=SUM(A1:A9)"));

    app.run("window.activate", json!({"window": first.0})).expect("switch to the first window");
    assert_eq!((app.session.active_window(), app.ui_window()), (Some(first), Some(first)));
    assert_eq!(draft(&app), None, "the first window has no draft of its own");
    assert_eq!(app.parked_window_with_draft(), Some(second), "the draft is parked in the second window");

    app.run("window.activate", json!({"window": second.0})).expect("back to the second window");
    assert_eq!(draft(&app), Some("=SUM(A1:A9)"), "the draft comes back where it was typed");
    assert_eq!(app.parked_window_with_draft(), None, "nothing else holds a draft");
    // The draft was never committed: the cell is still empty on the shared workbook.
    assert_eq!(shown(&app, "A1"), "");
}

/// A dialog opened in one window is that window's: the other window shows the sheet, not it.
#[test]
fn a_dialog_stays_in_the_window_it_was_opened_in() {
    let mut app = app();
    let (first, second) = two_windows(&mut app);
    app.open_dialog("zoom", json!({}));
    assert_eq!(app.dialog.as_ref().map(|d| d.name()), Some("zoom"));

    app.run("window.activate", json!({"window": first.0})).expect("switch to the first window");
    assert!(app.dialog.is_none(), "the dialog belongs to the second window");

    app.run("window.activate", json!({"window": second.0})).expect("back to the second window");
    assert_eq!(app.dialog.as_ref().map(|d| d.name()), Some("zoom"), "the dialog is back in its window");
}

/// The File tab's Backstage view is per window too, like the dialog it is drawn as.
#[test]
fn the_file_view_belongs_to_its_window() {
    let mut app = app();
    let (first, second) = two_windows(&mut app);
    app.open_backstage(cella_ui_egui::backstage::Page::Print);
    assert_eq!(app.backstage.as_ref().map(|b| b.page.id()), Some("print"));

    app.run("window.activate", json!({"window": first.0})).expect("switch to the first window");
    assert!(app.backstage.is_none(), "the first window shows the ribbon, not the File view");
    app.run("window.activate", json!({"window": second.0})).expect("back to the second window");
    assert_eq!(app.backstage.as_ref().map(|b| b.page.id()), Some("print"), "the File view is on Print, where it was left");
}

/// Closing a window closes the interface state it kept parked: its draft is not left in the app
/// for a window that no longer exists.
#[test]
fn closing_a_window_drops_its_parked_state() {
    let mut app = app();
    let (first, second) = two_windows(&mut app);
    app.begin_edit(Some("42".into()), false);
    app.open_dialog("zoom", json!({}));
    app.run("window.activate", json!({"window": first.0})).expect("switch to the first window");
    assert_eq!(app.parked_window_with_draft(), Some(second), "the second window keeps its draft");

    app.run("view.closeWindow", json!({"window": second.0})).expect("the second window closes");
    assert!(app.session.window_doc(second).is_none(), "the window is gone");
    assert_eq!(app.parked_window_with_draft(), None, "its parked draft went with it");
    assert!(app.dialog.is_none(), "and so did its dialog");
    assert_eq!((app.session.active_window(), app.ui_window()), (Some(first), Some(first)));
}

/// A painted frame ends on the window the user works in — its interface state, not the last
/// window drawn — so the next command, keystroke or control request acts in what is on screen.
#[test]
fn a_frame_ends_on_the_focused_windows_interface_state() {
    let mut app = app();
    let (first, second) = two_windows(&mut app);
    app.begin_edit(Some("draft".into()), false);
    app.open_dialog("zoom", json!({}));
    app.run("window.activate", json!({"window": first.0})).expect("switch to the first window");
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    assert_eq!(h.state().ui_window(), Some(first), "the frame ends on the window the user is in");
    assert_eq!(draft(h.state()), None, "the second window's draft is not painted with it");
    assert!(h.state().dialog.is_none(), "nor its dialog");
    assert_eq!(h.state().parked_window_with_draft(), Some(second));
}
