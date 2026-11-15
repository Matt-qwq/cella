//! View ▸ New Window is a second *window* on one workbook, never a second copy of it: both
//! windows show the same document, share its undo history, and closing one leaves it open.

use gridcraft_engine::Session;
use gridcraft_ui_egui::{Services, SheetApp};
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
    let at = gridcraft_engine::core::CellRef::parse(cell).expect("a cell address");
    gridcraft_engine::display::edit_text(&d.wb, sh, at)
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
    assert_eq!(app.session.active_window(), Some(gridcraft_engine::WindowId(list[0]["window"].as_u64().expect("an id"))));
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
    let inspected = gridcraft_ui_egui::control::inspect(h.state(), &h.ctx);
    assert_eq!(inspected["mainWindow"], json!(main.0), "{inspected}");
    assert_eq!(inspected["windows"].as_array().map(Vec::len), Some(1), "{inspected}");

    let mut h = h;
    h.state_mut().run("view.newWindow", json!({})).expect("a second window");
    h.run_steps(3);
    let inspected = gridcraft_ui_egui::control::inspect(h.state(), &h.ctx);
    assert_eq!(h.state().session.windows().len(), 2, "{inspected}");
    assert_eq!(h.state().session.documents().len(), 1, "two windows on one document");
    let windows = inspected["windows"].as_array().expect("the window list");
    assert_eq!(windows.len(), 2, "{inspected}");
    assert_eq!(inspected["mainWindow"], json!(main.0), "the control channel keeps its own window: {inspected}");
    assert_eq!(windows[1]["title"], json!("Book1:2"), "{inspected}");
    // The frame still belongs to the app's own window: its grid is what `ui.cell.click` maps onto.
    assert_eq!(h.state().painted_window(), Some(main));
}
