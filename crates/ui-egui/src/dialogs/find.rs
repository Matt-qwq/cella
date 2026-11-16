use super::Dialog;
use super::show::ok_cancel;

use egui::Key;
use serde_json::{Value as Json, json};

use crate::SheetApp;

pub(super) fn find(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog) {
    ui.horizontal(|ui| {
        for tab in ["Find", "Replace"] {
            if ui.selectable_label(d.tab == tab, crate::widgets::tr(ui, tab)).clicked() {
                d.tab = tab.into();
            }
        }
    });
    let mut what = d.values.get("what").and_then(Json::as_str).unwrap_or("").to_string();
    let mut with = d.values.get("with").and_then(Json::as_str).unwrap_or("").to_string();
    ui.horizontal(|ui| {
        ui.label(crate::widgets::tr(ui, "Find what:"));
        let r = ui.add(egui::TextEdit::singleline(&mut what).desired_width(260.0));
        // Like Excel, the cursor starts in "Find what" - asked for once, so clicking another field still works.
        if !d.values.contains_key("focused") {
            r.request_focus();
            d.values.insert("focused".into(), json!(true));
        }
    });
    if d.tab == "Replace" {
        ui.horizontal(|ui| {
            ui.label(crate::widgets::tr(ui, "Replace with:"));
            ui.add(egui::TextEdit::singleline(&mut with).desired_width(244.0));
        });
    }
    let mut case = d.values.get("matchCase").and_then(Json::as_bool).unwrap_or(false);
    let mut whole = d.values.get("wholeCell").and_then(Json::as_bool).unwrap_or(false);
    let mut wb = d.values.get("within").and_then(Json::as_str) == Some("workbook");
    ui.horizontal(|ui| {
        ui.checkbox(&mut case, crate::widgets::tr(ui, "Match case"));
        ui.checkbox(&mut whole, crate::widgets::tr(ui, "Match entire cell contents"));
        ui.checkbox(&mut wb, crate::widgets::tr(ui, "Within workbook"));
    });
    d.values.insert("what".into(), json!(what));
    d.values.insert("with".into(), json!(with));
    d.values.insert("matchCase".into(), json!(case));
    d.values.insert("wholeCell".into(), json!(whole));
    d.values.insert("within".into(), json!(if wb { "workbook" } else { "sheet" }));
    let base = json!({"what": what, "matchCase": case, "wholeCell": whole, "within": if wb { "workbook" } else { "sheet" }});
    ui.horizontal(|ui| {
        if d.tab == "Replace" {
            if ui.button(crate::widgets::tr(ui, "Replace All")).clicked() {
                let mut p = base.clone();
                p["with"] = json!(with);
                p["all"] = json!(true);
                match app.run("edit.replace", p) {
                    Ok(r) => {
                        app.toast = Some((
                            crate::widgets::tr(ui, "All done. We made {} replacements.").replace("{}", &r["replaced"].to_string()),
                            crate::now_ms(),
                        ))
                    }
                    Err(e) => app.message = Some(("Find and Replace".into(), crate::clean_error(&e))),
                }
            }
            if ui.button(crate::widgets::tr(ui, "Replace")).clicked() {
                let mut p = base.clone();
                p["with"] = json!(with);
                p["all"] = json!(false);
                let _ = app.run("edit.replace", p);
                let _ = app.run("edit.find", base.clone());
            }
        }
        if ui.button(crate::widgets::tr(ui, "Find All")).clicked() {
            let mut p = base.clone();
            p["all"] = json!(true);
            d.result = app.run("edit.find", p).ok();
        }
        if ui.button(crate::widgets::tr(ui, "Find Next")).clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
            if let Err(e) = app.run("edit.find", base.clone()) {
                app.message = Some(("Find and Replace".into(), crate::clean_error(&e)));
            }
            app.grid.ensure_visible = true;
        }
    });
    if let Some(r) = &d.result
        && let Some(list) = r["results"].as_array()
    {
        ui.separator();
        ui.label(crate::widgets::tr(ui, "{} cell(s) found").replace("{}", &list.len().to_string()));
        egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
            for item in list {
                let label = format!(
                    "{}  {}  {}",
                    item["sheet"].as_str().unwrap_or(""),
                    item["cell"].as_str().unwrap_or(""),
                    item["value"].as_str().unwrap_or("")
                );
                if ui.selectable_label(false, label).clicked() {
                    if let Some(s) = item["sheet"].as_str() {
                        let _ = app.run("sheet.activate", json!({"sheet": s}));
                    }
                    let _ = app.run("selection.set", json!({"cell": item["cell"]}));
                    app.grid.ensure_visible = true;
                }
            }
        });
    }
}

pub(super) fn go_to(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    let names: Vec<String> = app.session.active().map(|doc| doc.wb.names.iter().map(|n| n.name.clone()).collect()).unwrap_or_default();
    for n in names {
        if ui.selectable_label(false, &n).clicked() {
            d.values.insert("reference".into(), json!(n));
        }
    }
    let mut r = d.values.get("reference").and_then(Json::as_str).unwrap_or("").to_string();
    ui.horizontal(|ui| {
        ui.label(crate::widgets::tr(ui, "Reference:"));
        ui.text_edit_singleline(&mut r).request_focus();
    });
    d.values.insert("reference".into(), json!(r.clone()));
    if ui.button(crate::widgets::tr(ui, "Special…")).clicked() {
        app.open_dialog("goToSpecial", json!({}));
        return;
    }
    let mut open = true;
    ok_cancel(ui, confirm, &mut open);
    if *confirm {
        app.run_or_alert("edit.goTo", json!({"reference": r}));
        app.grid.ensure_visible = true;
    }
    if !open {
        *confirm = true;
    }
}
