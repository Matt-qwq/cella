use super::Dialog;

use serde_json::{Value as Json, json};

use crate::SheetApp;

pub(super) fn name_manager(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, open: &mut bool) {
    let list = app.session.run("formulas.nameManager", json!({})).unwrap_or(Json::Null);
    let names = list.as_array().cloned().unwrap_or_default();
    let selected = names.get(d.list_index).cloned();
    let field = |n: &Json, k: &str| n.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    ui.horizontal(|ui| {
        if ui.button(crate::widgets::tr(ui, "New…")).clicked() {
            app.open_dialog("defineName", json!({}));
        }
        if ui.add_enabled(selected.is_some(), egui::Button::new(crate::widgets::tr(ui, "Edit…"))).clicked()
            && let Some(n) = &selected
        {
            app.open_dialog(
                "defineName",
                json!({
                    "name": field(n, "name"), "scope": field(n, "scope"), "comment": field(n, "comment"),
                    "refersTo": field(n, "refersTo"), "oldName": field(n, "name"), "oldScope": field(n, "scope"),
                }),
            );
        }
        if ui.add_enabled(selected.is_some(), egui::Button::new(crate::widgets::tr(ui, "Delete"))).clicked()
            && let Some(n) = &selected
        {
            app.run_or_alert("formulas.deleteName", json!({"name": field(n, "name"), "scope": field(n, "scope")}));
        }
    });
    ui.add_space(4.0);
    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
        egui::Grid::new("names").striped(true).num_columns(4).show(ui, |ui| {
            ui.strong(crate::widgets::tr(ui, "Name"));
            ui.strong(crate::widgets::tr(ui, "Value"));
            ui.strong(crate::widgets::tr(ui, "Refers To"));
            ui.strong(crate::widgets::tr(ui, "Scope"));
            ui.end_row();
            if names.is_empty() {
                ui.label(egui::RichText::new(crate::widgets::tr(ui, "No names defined")).italics());
                ui.end_row();
            }
            for (i, n) in names.iter().enumerate() {
                if ui.selectable_label(i == d.list_index, field(n, "name")).clicked() {
                    d.list_index = i;
                }
                ui.label(n.get("value").map(|v| v.to_string()).unwrap_or_default().chars().take(30).collect::<String>());
                ui.label(field(n, "refersTo"));
                ui.label(field(n, "scope"));
                ui.end_row();
            }
        });
    });
    ui.add_space(8.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button(crate::widgets::tr(ui, " Close ")).clicked() {
            *open = false;
        }
    });
}
