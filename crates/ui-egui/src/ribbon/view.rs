use super::controls::{act, checkbox_on, control_keytip, invoke, keytip, menu_items, sep};

use egui::{Rect, Ui, pos2, vec2};
use serde_json::json;

use crate::SheetApp;
use crate::icons::Icon;
use crate::keytips::keytip_table;
use crate::theme::{self, Tokens};
use crate::widgets::{big_button, small_button};

pub(super) fn view(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let _ = t;
    for (icon, label, cmd) in [
        (Icon::Normal, "Normal", "view.normal"),
        (Icon::PageBreak, "Page Break\nPreview", "view.pageBreakPreview"),
        (Icon::PageLayout, "Page\nLayout", "view.pageLayout"),
    ] {
        let __k = big_button(ui, icon, label, label, false);
        control_keytip(app, ui, &__k, cmd);
        if __k.clicked() {
            act(app, cmd, json!({}));
        }
    }
    sep(ui);
    let s = app.session.active().and_then(|d| d.wb.active().map(|s| (s.show_gridlines, s.show_headings))).unwrap_or((true, true));
    let lang = app.ui.language;
    let page_layout =
        app.session.active().and_then(|d| d.wb.active().map(|s| s.view == cella_engine::model::SheetViewMode::PageLayout)).unwrap_or(false);
    // Two columns of two, as in Excel's Show group: four stacked rows overflow the ribbon body.
    ui.vertical(|ui| {
        // Rulers exist only in Page Layout view.
        let mut ruler = !app.session.hide_ruler;
        let ruler_resp = ui.add_enabled(page_layout, egui::Checkbox::new(&mut ruler, lang.tr("Ruler")));
        control_keytip(app, ui, &ruler_resp, "view.ruler");
        if ruler_resp.changed() {
            act(app, "view.ruler", json!({"on": ruler}));
        }
        let mut g = s.0;
        let resp = ui.checkbox(&mut g, lang.tr("Gridlines"));
        control_keytip(app, ui, &resp, "view.gridlines");
        if resp.changed() {
            act(app, "view.gridlines", json!({"on": g}));
        }
        // High-contrast chrome: black, white and yellow, for readers who need stronger contrast.
        let mut hc = app.ui.high_contrast;
        let resp = ui.checkbox(&mut hc, lang.tr("High Contrast")).on_hover_text(lang.tr("Stronger colours for the window; the sheet keeps its own"));
        control_keytip(app, ui, &resp, "view.highContrast");
        if resp.changed() || invoke(app, "view.highContrast") {
            act(app, "view.highContrast", json!({"on": hc}));
        }
    });
    ui.add_space(6.0);
    ui.vertical(|ui| {
        let mut fb = app.session.prefs.formula_bar;
        let resp = ui.checkbox(&mut fb, lang.tr("Formula Bar"));
        control_keytip(app, ui, &resp, "view.formulaBar");
        if resp.changed() || invoke(app, "view.formulaBar") {
            act(app, "view.formulaBar", json!({"on": checkbox_on(resp.changed(), fb)}));
        }
        let mut h = s.1;
        let resp = ui.checkbox(&mut h, lang.tr("Headings"));
        control_keytip(app, ui, &resp, "view.headings");
        if resp.changed() {
            act(app, "view.headings", json!({"on": h}));
        }
    });
    sep(ui);
    let __k = big_button(ui, Icon::Zoom, "Zoom", "Zoom…", false);
    control_keytip(app, ui, &__k, "dialog:zoom");
    if __k.clicked() {
        app.open_dialog("zoom", json!({}));
    }
    let __k = big_button(ui, Icon::Search, "100%", "Zoom to 100%", false);
    control_keytip(app, ui, &__k, "view.zoom");
    if __k.clicked() || invoke(app, "view.zoom") {
        act(app, "view.zoom", json!({"percent": 100}));
    }
    let __k = big_button(ui, Icon::Zoom, "Zoom to\nSelection", "Zoom to Selection", false);
    control_keytip(app, ui, &__k, "view.zoomToSelection");
    if __k.clicked() {
        let r = app.grid.cells_rect.unwrap_or(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 700.0)));
        act(app, "view.zoomToSelection", json!({"viewWidth": r.width(), "viewHeight": r.height()}));
    }
    sep(ui);
    let fp = big_button(ui, Icon::Freeze, "Freeze\nPanes", "Freeze Panes", true);
    for sequence in keytip_table::group_extension(
        app.keytips.prefix().unwrap_or_default(),
        &["view.freezePanes", "view.freezeTopRow", "view.freezeFirstColumn", "view.unfreezePanes"],
    ) {
        keytip(app, ui, sequence, fp.rect.center_bottom());
    }
    egui::Popup::menu(&fp).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Freeze Panes", "view.freezePanes", json!({})),
                ("Freeze Top Row", "view.freezeTopRow", json!({})),
                ("Freeze First Column", "view.freezeFirstColumn", json!({})),
                ("Unfreeze Panes", "view.unfreezePanes", json!({})),
            ],
        );
    });
    ui.vertical(|ui| {
        ui.label(lang.tr("Display theme"));
        let mode = app.ui.theme_mode();
        let label = match mode {
            "system" => "System",
            "dark" => "Dark",
            _ => "Light",
        };
        egui::ComboBox::from_id_salt("display_theme").selected_text(lang.tr(label)).width(88.0).show_ui(ui, |ui| {
            for (value, label) in [("system", "System"), ("light", "Light"), ("dark", "Dark")] {
                if ui.selectable_label(mode == value, lang.tr(label)).clicked() {
                    act(app, "view.theme", json!({"mode": value}));
                }
            }
        });
    });
    sep(ui);
    // Interface size (issue #230): scales the whole window on top of the desktop's scale.
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(lang.tr("Interface size")).font(theme::ui_font(11.5)).color(t.text_dim));
        let current = app.ui.interface_scale;
        let text = |v: u16| if v == 100 { lang.tr("Auto").to_string() } else { format!("{v}%") };
        egui::ComboBox::from_id_salt("ui_interface_size")
            .width(120.0)
            .selected_text(egui::RichText::new(text(current)).font(theme::ui_font(12.5)))
            .show_ui(ui, |ui| {
                for v in [100, 75, 90, 110, 125, 150, 175, 200] {
                    if ui.selectable_label(v == current, text(v)).clicked() {
                        act(app, "view.interfaceScale", json!({"percent": v}));
                    }
                }
            });
    });
    // Interface language: settings live in the View tab, like Excel's Options. The label follows
    // the current language so it stays readable; the choices are always shown in their own script.
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(lang.tr("Interface language")).font(theme::ui_font(11.5)).color(t.text_dim));
        egui::ComboBox::from_id_salt("ui_language").width(120.0).selected_text(egui::RichText::new(lang.name()).font(theme::ui_font(12.5))).show_ui(
            ui,
            |ui| {
                for l in crate::i18n::Language::ALL {
                    if ui.selectable_label(l == lang, l.name()).clicked() {
                        act(app, "app.language.set", json!({"language": l.code()}));
                    }
                }
            },
        );
    });
    sep(ui);
    // Window: one workbook in several windows. The engine owns the window list, so New Window is
    // one command and every other window is an OS window of its own showing the same document.
    let new = big_button(ui, Icon::Book, "New Window", "Another window on this workbook: the same document, its undo history and its save", false);
    control_keytip(app, ui, &new, "view.newWindow");
    if new.clicked() || invoke(app, "view.newWindow") {
        act(app, "view.newWindow", json!({}));
    }
    let switching = big_button(ui, Icon::SheetList, "Switch\nWindows", "Switch Windows", true);
    control_keytip(app, ui, &switching, "view.switchWindows");
    egui::Popup::menu(&switching).show(|ui| {
        let windows = app.session.windows();
        let active = app.session.active_window();
        for (i, (_, w)) in windows.iter().enumerate() {
            // Hidden windows are not in Excel's Switch Windows list either; Unhide brings them back.
            if w.hidden {
                continue;
            }
            if ui.selectable_label(active == Some(w.id), app.window_label(w.id)).clicked() {
                act(app, "window.activate", json!({"index": i}));
            }
        }
    });
    ui.vertical(|ui| {
        let hide = small_button(ui, Icon::Minus, "Hide", "Hide this window; the document stays open", false);
        control_keytip(app, ui, &hide, "view.hideWindow");
        if hide.clicked() || invoke(app, "view.hideWindow") {
            act(app, "view.hideWindow", json!({}));
        }
        let unhide = small_button(ui, Icon::Plus, "Unhide", "Show a hidden window again", false);
        control_keytip(app, ui, &unhide, "view.unhideWindow");
        if unhide.clicked() || invoke(app, "view.unhideWindow") {
            act(app, "view.unhideWindow", json!({}));
        }
        let close = small_button(ui, Icon::Close, "Close Window", "Close this window (its document closes with the last one)", false);
        control_keytip(app, ui, &close, "view.closeWindow");
        if close.clicked() || invoke(app, "view.closeWindow") {
            act(app, "view.closeWindow", json!({}));
        }
    });
}
