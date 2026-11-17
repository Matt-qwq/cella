use super::controls::{act, anchor_keytip, control_keytip, invoke, keytip, menu_items, popup_command, sep};

use egui::Ui;
use serde_json::json;

use crate::SheetApp;
use crate::icons::Icon;
use crate::keytips::keytip_table;
use crate::widgets::{big_button, small_button};

/// The Formulas tab: Insert Function, AutoSum, the function-category menus (names in the formula
/// language, descriptions in the interface language), defined names, auditing and calculation.
pub(super) fn formulas(app: &mut SheetApp, ui: &mut Ui) {
    let __k = big_button(ui, Icon::Function, "Insert\nFunction", "Insert Function (⇧F3)", false);
    control_keytip(app, ui, &__k, "dialog:insertFunction");
    if __k.clicked() {
        app.open_dialog("insertFunction", json!({}));
    }
    let s = big_button(ui, Icon::Sum, "AutoSum", "AutoSum", true);
    control_keytip(app, ui, &s, "formulas.autoSum");
    if s.clicked() {
        act(app, "formulas.autoSum", json!({}));
    }
    for (label, cat, cid) in [
        ("Financial", "Financial", "formulas.financial"),
        ("Logical", "Logical", "formulas.logical"),
        ("Text", "Text", "formulas.text"),
        ("Date &\nTime", "DateTime", "formulas.dateTime"),
        ("Lookup &\nReference", "Lookup", "formulas.lookup"),
        ("Math &\nTrig", "MathTrig", "formulas.mathTrig"),
        ("More\nFunctions", "Statistical", "formulas.moreFunctions"),
    ] {
        let r = big_button(ui, Icon::Book, label, label, true);
        control_keytip(app, ui, &r, cid);
        let forced_c = invoke(app, cid);
        egui::Popup::menu(&r).open_memory(popup_command(forced_c, r.clicked())).show(|ui| {
            ui.set_max_height(420.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                for f in cella_engine::cmd::formulas::function_list() {
                    if f["category"].as_str() == Some(cat)
                        && let Some(n) = f["name"].as_str()
                    {
                        let locale = app.ui.language.formula_locale();
                        let name =
                            app.session.active().map(|d| crate::formula_locale::completion_name(n, locale, &d.wb, d.wb.active_sheet)).unwrap_or(n);
                        let desc = crate::formula_locale::description(n, locale, app.ui.language).unwrap_or_default();
                        if ui.button(name).on_hover_text(desc).clicked() {
                            app.run_or_alert("formulas.insertFunction", json!({"name": n}));
                            ui.close();
                        }
                    }
                }
            });
        });
    }
    sep(ui);
    let __k = big_button(ui, Icon::Name, "Name\nManager", "Name Manager", false);
    control_keytip(app, ui, &__k, "dialog:nameManager");
    if __k.clicked() {
        app.open_dialog("nameManager", json!({}));
    }
    ui.vertical(|ui| {
        let __k = small_button(ui, Icon::Name, "Define Name", "Define Name", false);
        control_keytip(app, ui, &__k, "dialog:defineName");
        if __k.clicked() {
            app.open_dialog("defineName", json!({}));
        }
        let use_in = small_button(ui, Icon::Function, "Use in Formula", "Use in Formula", true);
        anchor_keytip(app, ui, &use_in, &["formulas.pasteName"]);
        egui::Popup::menu(&use_in).show(|ui| {
            let names: Vec<String> = app.session.active().map(|d| d.wb.names.iter().map(|n| n.name.clone()).collect()).unwrap_or_default();
            for n in names {
                if ui.button(&n).clicked() {
                    let cur = app.editor.as_ref().map(|e| e.text.clone()).unwrap_or_else(|| "=".into());
                    app.begin_edit(Some(format!("{cur}{n}")), false);
                }
            }
        });
        let __k = small_button(ui, Icon::Table, "Create from Selection", "Create from Selection", false);
        control_keytip(app, ui, &__k, "formulas.createFromSelection");
        if __k.clicked() {
            act(app, "formulas.createFromSelection", json!({}));
        }
    });
    sep(ui);
    ui.vertical(|ui| {
        if small_button(ui, Icon::Trace, "Trace Precedents", "Trace Precedents", false).clicked()
            && let Ok(r) = app.run("formulas.tracePrecedents", json!({}))
        {
            app.grid.trace = Some(r);
        }
        if small_button(ui, Icon::Trace, "Trace Dependents", "Trace Dependents", false).clicked()
            && let Ok(r) = app.run("formulas.traceDependents", json!({}))
        {
            app.grid.trace = Some(r);
        }
        if small_button(ui, Icon::Close, "Remove Arrows", "Remove Arrows", false).clicked() {
            app.grid.trace = None;
        }
    });
    ui.vertical(|ui| {
        let __k = small_button(ui, Icon::Fx, "Show Formulas", "Show Formulas (⌃`)", false);
        control_keytip(app, ui, &__k, "formulas.showFormulas");
        if __k.clicked() {
            act(app, "formulas.showFormulas", json!({}));
        }
        let __k = small_button(ui, Icon::Validation, "Error Checking", "Error Checking", false);
        control_keytip(app, ui, &__k, "dialog:errorChecking");
        if __k.clicked() {
            app.open_dialog("errorChecking", json!({}));
        }
        let __k = small_button(ui, Icon::Calc, "Evaluate Formula", "Evaluate Formula", false);
        control_keytip(app, ui, &__k, "dialog:evaluateFormula");
        if __k.clicked() {
            app.open_dialog("evaluateFormula", json!({}));
        }
    });
    if big_button(ui, Icon::Search, "Watch\nWindow", "Watch Window", false).clicked() {
        app.grid.pane = Some("watch".into());
    }
    sep(ui);
    let co = big_button(ui, Icon::Calc, "Calculation\nOptions", "Calculation Options", true);
    for sequence in keytip_table::group_extension(app.keytips.prefix().unwrap_or_default(), &["formulas.calculationOptions"]) {
        keytip(app, ui, sequence, co.rect.center_bottom());
    }
    egui::Popup::menu(&co).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Automatic", "formulas.calculationOptions", json!({"mode": "automatic"})),
                ("Automatic Except for Data Tables", "formulas.calculationOptions", json!({"mode": "automaticExceptTables"})),
                ("Manual", "formulas.calculationOptions", json!({"mode": "manual"})),
            ],
        );
    });
    ui.vertical(|ui| {
        let __k = small_button(ui, Icon::Calc, "Calculate Now", "Calculate Now (F9)", false);
        control_keytip(app, ui, &__k, "formulas.calculateNow");
        if __k.clicked() {
            act(app, "formulas.calculateNow", json!({}));
        }
        let __k = small_button(ui, Icon::Sheet, "Calculate Sheet", "Calculate Sheet (⇧F9)", false);
        control_keytip(app, ui, &__k, "formulas.calculateSheet");
        if __k.clicked() {
            act(app, "formulas.calculateSheet", json!({}));
        }
    });
}
