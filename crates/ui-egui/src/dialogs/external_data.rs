use super::Dialog;
use super::show::ok_cancel;

use serde_json::{Value as Json, json};

use crate::SheetApp;

/// Data > Get Data > From Web: a URL, and the query it becomes. One field, because the format
/// follows the address (".json" reads as JSON, anything else as CSV) and a step can change it.
pub(super) fn from_web(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    let mut url = d.values.get("url").and_then(Json::as_str).unwrap_or("").to_string();
    ui.label(crate::widgets::tr(ui, "Address:"));
    ui.add(egui::TextEdit::singleline(&mut url).desired_width(f32::INFINITY).hint_text("https://"));
    ui.label(
        egui::RichText::new(crate::widgets::tr(ui, "The address is read when the query refreshes, not when the workbook opens."))
            .small()
            .color(crate::theme::Tokens::get(ui.ctx()).text_dim),
    );
    d.values.insert("url".into(), json!(url.clone()));
    let mut open = true;
    ok_cancel(ui, confirm, &mut open);
    if *confirm {
        match app.run("data.fromWeb", json!({"url": url})) {
            Ok(r) => {
                let name = r["name"].as_str().unwrap_or("").to_string();
                app.toast = Some((crate::widgets::tr(ui, "Added the query {name}").replace("{name}", &name), crate::now_ms()))
            }
            Err(e) => app.message = Some(("From Web".into(), crate::clean_error(&e))),
        }
    }
    if !open {
        *confirm = true;
    }
}

/// Data > Get Data > From Database: a SQLite file and the statement to run against it. The
/// statement is a box of its own rather than a form field, because a SELECT is several lines and a
/// one-line box is where multi-line SQL gets truncated.
pub(super) fn from_database(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    let mut path = d.values.get("path").and_then(Json::as_str).unwrap_or("").to_string();
    let mut sql = d.values.get("sql").and_then(Json::as_str).unwrap_or("SELECT * FROM ").to_string();
    ui.horizontal(|ui| {
        ui.label(crate::widgets::tr(ui, "Database:"));
        ui.add(egui::TextEdit::singleline(&mut path).desired_width(320.0));
        if ui.button(crate::widgets::tr(ui, "Browse…")).clicked()
            && let Some(p) = app.services.pick_open.as_ref().and_then(|pick| pick())
        {
            path = p;
        }
    });
    ui.label(crate::widgets::tr(ui, "Statement (one SELECT; the file is opened read-only):"));
    ui.add(egui::TextEdit::multiline(&mut sql).desired_width(f32::INFINITY).desired_rows(6).font(egui::TextStyle::Monospace));
    d.values.insert("path".into(), json!(path.clone()));
    d.values.insert("sql".into(), json!(sql.clone()));
    let mut open = true;
    ok_cancel(ui, confirm, &mut open);
    if *confirm {
        match app.run("data.fromDatabase", json!({"path": path, "sql": sql})) {
            Ok(r) => {
                let name = r["name"].as_str().unwrap_or("").to_string();
                app.toast = Some((crate::widgets::tr(ui, "Added the query {name}").replace("{name}", &name), crate::now_ms()))
            }
            Err(e) => app.message = Some(("From Database".into(), crate::clean_error(&e))),
        }
    }
    if !open {
        *confirm = true;
    }
}
