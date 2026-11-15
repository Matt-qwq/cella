//! Page Setup opens on the active sheet's saved margins and print options, so OK without edits
//! keeps them (issue #524). The page-parity options it exposes (page order, black and white, draft
//! quality) reach the sheet through the same command.

use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::{Value, json};

fn margins(app: &SheetApp) -> [f32; 6] {
    app.session.active().and_then(|d| d.wb.active()).map(|s| s.print.margins).unwrap_or_default()
}

fn app_with_custom_margins() -> SheetApp {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    app.run("pageLayout.margins", json!({"left": 2.0, "right": 2.5, "top": 3.0, "bottom": 0.25, "header": 0.1, "footer": 0.2})).unwrap();
    assert_eq!(margins(&app), [2.0, 2.5, 3.0, 0.25, 0.1, 0.2]);
    app
}

#[test]
fn page_setup_ok_without_edits_keeps_custom_margins() {
    // Custom Margins / File > Page Setup open it bare; Print Titles opens it on the Sheet tab.
    for params in [json!({}), json!({"tab": "Sheet"})] {
        let mut app = app_with_custom_margins();
        app.open_dialog("pageSetup", params);
        let d = app.dialog.take().unwrap();
        let shown = json!({
            "left": 2.0, "right": 2.5, "top": 3.0, "bottom": 0.25, "header": 0.1, "footer": 0.2,
            "pageOrder": "downThenOver", "blackAndWhite": false, "draft": false,
        });
        assert_eq!(Value::Object(d.values.clone()), shown);
        app.run(d.command.unwrap(), Value::Object(d.values)).unwrap();
        assert_eq!(margins(&app), [2.0, 2.5, 3.0, 0.25, 0.1, 0.2]);
    }
}

#[test]
fn page_setup_one_edited_field_keeps_the_other_margins() {
    let mut app = app_with_custom_margins();
    app.open_dialog("pageSetup", json!({}));
    let mut d = app.dialog.take().unwrap();
    d.values.insert("left".into(), json!(1.5));
    app.run(d.command.unwrap(), Value::Object(d.values)).unwrap();
    assert_eq!(margins(&app), [1.5, 2.5, 3.0, 0.25, 0.1, 0.2]);
}

/// The three page-parity options the dialog shows land on the sheet, and the dialog reopens on
/// what they saved.
#[test]
fn page_setup_saves_page_order_black_and_white_and_draft() {
    let mut app = app_with_custom_margins();
    app.open_dialog("pageSetup", json!({}));
    let mut d = app.dialog.take().unwrap();
    d.values.insert("pageOrder".into(), json!("overThenDown"));
    d.values.insert("blackAndWhite".into(), json!(true));
    d.values.insert("draft".into(), json!(true));
    app.run(d.command.unwrap(), Value::Object(d.values)).unwrap();
    let print = app.session.active().and_then(|d| d.wb.active()).map(|s| s.print.clone()).unwrap();
    assert_eq!(print.page_order.id(), "overThenDown");
    assert!(print.black_and_white && print.draft);
    app.open_dialog("pageSetup", json!({}));
    let d = app.dialog.take().unwrap();
    assert_eq!(d.values.get("pageOrder").and_then(Value::as_str), Some("overThenDown"));
    assert_eq!(d.values.get("blackAndWhite").and_then(Value::as_bool), Some(true));
    assert_eq!(d.values.get("draft").and_then(Value::as_bool), Some(true));
}
