use super::Dialog;

use serde_json::{Value as Json, json};

use crate::SheetApp;

/// View ▸ Macros: the workbook's scripts, and the step editor of the one selected — each step is
/// a row with its command id, its params as JSON text and a ✕, plus a row that adds one. Every
/// change runs the engine's own command (`automate.editStep`, `automate.removeStep`,
/// `automate.appendStep`, `automate.renameScript`) and the steps are read back from
/// `automate.scriptSteps` each frame, so the list is what the workbook holds, not what the dialog
/// remembers; a command that refuses shows in the message bar.
pub(super) fn macros(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language;
    ui.label(crate::widgets::tr(ui, "Automation in this workbook. Cella runs its own scripts; a file's VBA project is preserved but not executed."));
    ui.add_space(6.0);
    let items: Vec<Json> = d.result.as_ref().and_then(Json::as_array).cloned().unwrap_or_default();
    if items.is_empty() {
        ui.weak(crate::widgets::tr(ui, "This workbook has no scripts or macros."));
        return;
    }
    // Scripts that came with a file stay disabled until the user trusts the workbook (#2178).
    if items.iter().any(|it| it.get("trusted").and_then(Json::as_bool) == Some(false)) {
        ui.horizontal(|ui| {
            ui.weak(crate::widgets::tr(ui, "Scripts in this workbook came from a file and are disabled."));
            if ui
                .button(crate::widgets::tr(ui, "Trust Scripts"))
                .on_hover_text(crate::widgets::tr(ui, "Let this workbook's scripts run for the rest of this session"))
                .clicked()
            {
                let _ = app.run("automate.trustScripts", json!({"trusted": true}));
                d.result = app.session.run("view.macros", json!({})).ok();
            }
        });
        ui.add_space(6.0);
    }
    // (name, is a preserved VBA project, step count).
    let scripts: Vec<(String, bool, u64)> = items
        .iter()
        .filter_map(|it| {
            Some((
                it.get("name").and_then(Json::as_str)?.to_string(),
                it.get("kind").and_then(Json::as_str) == Some("vba"),
                it.get("steps").and_then(Json::as_u64).unwrap_or(0),
            ))
        })
        .collect();
    let first = scripts.iter().find(|(_, vba, _)| !*vba).map(|(n, _, _)| n.clone());
    let selected = d.values.get("script").and_then(Json::as_str).map(str::to_string).filter(|n| scripts.iter().any(|(s, _, _)| s == n)).or(first);
    let mut pick: Option<String> = None;
    egui::ScrollArea::vertical().max_height(160.0).id_salt("macros_scripts").show(ui, |ui| {
        for (name, vba, steps) in &scripts {
            ui.horizontal(|ui| {
                if *vba {
                    ui.label(egui::RichText::new(crate::widgets::tr(ui, name)).strong());
                    ui.weak(crate::widgets::tr(ui, "— VBA project (preserved, not run)"));
                } else {
                    let shown = selected.as_deref() == Some(name.as_str());
                    if ui.selectable_label(shown, egui::RichText::new(crate::widgets::tr(ui, name)).strong()).clicked() {
                        pick = Some(name.clone());
                    }
                    let count = if *steps == 1 {
                        crate::widgets::tr(ui, "— 1 step").to_string()
                    } else {
                        crate::widgets::tr(ui, "— {steps} steps").replace("{steps}", &steps.to_string())
                    };
                    ui.weak(count);
                }
            });
        }
    });
    if let Some(p) = &pick {
        d.values.insert("script".into(), json!(p));
    }
    let selected = pick.or(selected);
    let Some(script) = selected else { return };
    ui.add_space(4.0);
    ui.separator();
    // The name the steps belong to: typed over, it is committed when the field loses focus.
    let mut renamed: Option<String> = None;
    ui.horizontal(|ui| {
        ui.label(lang.tr("Name"));
        if let Some(to) = crate::widgets::committed_text(ui, egui::Id::new("macros_script_name"), &script, Some(200.0)) {
            let to = to.trim().to_string();
            if to.is_empty() || to == script {
                return;
            }
            match app.run("automate.renameScript", json!({"name": script, "to": to})) {
                Ok(_) => renamed = Some(to),
                Err(e) => app.message = Some((d.title.clone(), crate::clean_error(&e))),
            }
        }
    });
    if let Some(to) = &renamed {
        d.values.insert("script".into(), json!(to));
        d.result = app.session.run("view.macros", json!({})).ok();
    }
    let script = renamed.unwrap_or(script);
    let steps: Vec<Json> =
        app.session.run("automate.scriptSteps", json!({"name": script})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
    ui.add_space(4.0);
    ui.label(egui::RichText::new(lang.tr("Steps")).strong());
    let mut remove: Option<usize> = None;
    let mut apply: Option<(usize, String)> = None;
    egui::ScrollArea::vertical().max_height(240.0).id_salt("macros_steps").show(ui, |ui| {
        if steps.is_empty() {
            ui.weak(lang.tr("This script has no steps."));
        }
        for (i, st) in steps.iter().enumerate() {
            let id = st.get("command").and_then(Json::as_str).unwrap_or("");
            let params = st.get("params").cloned().unwrap_or(json!({}));
            ui.horizontal(|ui| {
                ui.monospace(format!("{}. {id}", i + 1));
                // The params are JSON text: honest, and no form generator that would have to know
                // every command's parameters.
                let field = egui::Id::new(("macros_step_params", script.as_str(), i));
                if let Some(text) = crate::widgets::committed_text(ui, field, &params.to_string(), Some(250.0)) {
                    apply = Some((i, text));
                }
                if ui.button("✕").on_hover_text(lang.tr("Remove step")).clicked() {
                    remove = Some(i);
                }
            });
        }
    });
    if let Some((index, text)) = apply {
        match serde_json::from_str::<Json>(text.trim()) {
            // Text that says what the step already holds is not an edit: leaving the field must not
            // add an undo step that changes nothing.
            Ok(params) if steps.get(index).and_then(|st| st.get("params")) != Some(&params) => {
                if let Err(e) = app.run("automate.editStep", json!({"name": script, "index": index, "params": params})) {
                    app.message = Some((d.title.clone(), crate::clean_error(&e)));
                }
            }
            Ok(_) => {}
            // Not JSON at all: the step's params stay as they were and the message says why.
            Err(e) => app.message = Some((d.title.clone(), lang.tr("Params must be JSON: {error}").replace("{error}", &e.to_string()))),
        }
    }
    if let Some(index) = remove
        && let Err(e) = app.run("automate.removeStep", json!({"name": script, "index": index}))
    {
        app.message = Some((d.title.clone(), crate::clean_error(&e)));
    }
    ui.add_space(4.0);
    let mut add = false;
    ui.horizontal(|ui| {
        let mut draft = d.values.get("newStep").and_then(Json::as_str).unwrap_or("").to_string();
        let field = ui.add(egui::TextEdit::singleline(&mut draft).hint_text(lang.tr("command id, e.g. cell.set")).desired_width(220.0));
        if field.changed() {
            d.values.insert("newStep".into(), json!(draft.clone()));
        }
        let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.button(lang.tr("Add Step")).on_hover_text(lang.tr("Add a step at the end of this script")).clicked() || entered {
            add = true;
        }
    });
    if add {
        let command = d.values.get("newStep").and_then(Json::as_str).unwrap_or("").trim().to_string();
        match app.run("automate.appendStep", json!({"name": script, "command": command})) {
            Ok(_) => {
                d.values.insert("newStep".into(), json!(""));
            }
            Err(e) => app.message = Some((d.title.clone(), crate::clean_error(&e))),
        }
    }
}
