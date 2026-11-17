use super::Dialog;

use serde_json::{Value as Json, json};

use crate::SheetApp;

/// The Macros dialog: lists Cella's own recorded scripts and (if the file carries one) the
/// preserved VBA project. Mirrors Excel's Developer ▸ Macros list, but is honest that VBA is
/// preserved and not executed.
/// Review ▸ Check Accessibility: each issue where it is, a button that goes there and — for an
/// object without alternative text — a field that sets it (`object.setAltText`).
pub(super) fn accessibility(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog) {
    ui.label(crate::widgets::tr(
        ui,
        "What a screen reader reads: alternative text on charts, pictures and shapes, header rows on tables, merged cells and sheet names.",
    ));
    ui.add_space(6.0);
    let items: Vec<Json> = d.result.as_ref().and_then(Json::as_array).cloned().unwrap_or_default();
    if items.is_empty() {
        ui.weak(crate::widgets::tr(ui, "No accessibility issues found."));
        return;
    }
    let mut refresh = false;
    let mut go = None;
    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
        for issue in &items {
            let sheet = issue["sheet"].as_str().unwrap_or_default();
            let sheet_index = issue["sheetIndex"].as_u64().unwrap_or(0);
            let what = issue["object"]
                .as_str()
                .map(str::to_string)
                .or_else(|| issue["range"].as_str().map(|r| format!("{sheet}!{r}")))
                .unwrap_or_else(|| sheet.to_string());
            ui.horizontal(|ui| {
                ui.label(format!("{} — {what}", crate::widgets::tr(ui, issue["issue"].as_str().unwrap_or(""))));
                if ui.button(crate::widgets::tr(ui, "Go to")).clicked() {
                    go = Some(issue.clone());
                }
            });
            // Why the object is listed, and — for one that carries text — the name a screen reader
            // reads for it when it has no alternative text of its own.
            if let Some(reason) = issue["reason"].as_str() {
                let reason = crate::widgets::tr(ui, reason);
                match issue["accessibleName"].as_str().filter(|n| !n.is_empty()) {
                    Some(name) => ui.weak(format!("{reason} — `{name}`")),
                    None => ui.weak(reason),
                };
            }
            let kind = issue["kind"].as_str().unwrap_or("");
            if kind.is_empty() || kind == "cellPicture" {
                continue;
            }
            let key = format!("alt:{kind}:{}", issue["id"]);
            let mut text = d.values.get(&key).and_then(Json::as_str).unwrap_or("").to_string();
            ui.horizontal(|ui| {
                ui.label(crate::widgets::tr(ui, "Alt text"));
                if ui.add(egui::TextEdit::singleline(&mut text).desired_width(220.0)).changed() {
                    d.values.insert(key.clone(), json!(text));
                }
                if ui
                    .button(crate::widgets::tr(ui, "Set"))
                    .on_hover_text(crate::widgets::tr(ui, "Describe this object for a screen reader"))
                    .clicked()
                {
                    let _ = app.session.run("sheet.activate", json!({"sheet": sheet_index}));
                    if let Some(id) = issue["id"].as_u64() {
                        let _ = app.session.run("object.setAltText", json!({"kind": kind, "id": id, "text": text}));
                        refresh = true;
                    }
                }
            });
        }
    });
    if let Some(issue) = go {
        let sheet_index = issue["sheetIndex"].as_u64().unwrap_or(0);
        let _ = app.session.run("sheet.activate", json!({"sheet": sheet_index}));
        match issue["kind"].as_str() {
            Some("chart" | "image" | "shape") => {
                if let Some(id) = issue["id"].as_u64().and_then(|id| u32::try_from(id).ok()) {
                    app.selected_chart = Some(id);
                }
            }
            _ => {
                if let Some(range) = issue["range"].as_str() {
                    let _ = app.session.run("selection.set", json!({"range": range}));
                }
            }
        }
    }
    if refresh {
        d.result = app.session.run("review.checkAccessibility", json!({})).ok();
    }
}
