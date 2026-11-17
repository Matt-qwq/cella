//! Right-hand task panes: Comments, Watch Window, Selection Pane, Format Chart.

use egui::{Color32, Ui, vec2};
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::theme::{self, Tokens};

pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    // The Comments pane is engine state (`review.showComments`, so agents can drive it); the other
    // panes are UI state. One pane shows at a time: opening another one closes Comments.
    if app.grid.pane.as_deref() == Some("comments") {
        app.grid.pane = None;
        app.run_or_alert("review.showComments", json!({"on": true}));
    } else if app.grid.pane.is_some() && app.session.comments_pane {
        app.run_or_alert("review.showComments", json!({"on": false}));
    }
    let Some(pane) = app.grid.pane.clone().or_else(|| app.session.comments_pane.then(|| "comments".to_string())) else { return };
    let t = Tokens::get(ui.ctx());
    // Capped at half the window so a pane can never cover the sheet, whatever its content asks for.
    let max = (ui.available_width() / 2.0).max(300.0);
    let panel = egui::Panel::right("task_pane").default_size(300.0).max_size(max).resizable(true);
    panel.frame(egui::Frame::NONE.fill(t.ribbon).inner_margin(10)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let title = match pane.as_str() {
                "comments" => "Comments",
                "watch" => "Watch Window",
                "selection" => "Selection",
                "formatChart" => format_pane_title(app),
                "queries" => "Queries & Connections",
                _ => "",
            };
            ui.label(egui::RichText::new(crate::widgets::tr(ui, title)).font(theme::ui_bold(15.0)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::widgets::icon_button(ui, crate::icons::Icon::Close, t.text_dim, "Close", vec2(22.0, 22.0)).clicked() {
                    if pane == "comments" {
                        app.run_or_alert("review.showComments", json!({"on": false}));
                    }
                    app.grid.pane = None;
                }
            });
        });
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| match pane.as_str() {
            "comments" => comments(app, ui),
            "watch" => watch(app, ui),
            "selection" => selection(app, ui),
            "formatChart" => format_chart(app, ui),
            "queries" => queries(app, ui),
            _ => {}
        });
    });
}

/// The Format pane's title: the same pane serves a chart, a shape and a picture, and Excel names it
/// after the selected object.
fn format_pane_title(app: &SheetApp) -> &'static str {
    let kind =
        app.session.active().and_then(|d| d.wb.active()).and_then(|sh| crate::grid::selected_object(sh, app.selected_chart)).map(|(kind, _, _)| kind);
    title_of(kind)
}

/// The title for a selected object kind (`grid::selected_object`'s first field).
fn title_of(kind: Option<&str>) -> &'static str {
    match kind {
        Some("shape") => "Format Shape",
        Some("image") => "Format Picture",
        _ => "Format Chart Area",
    }
}

fn comments(app: &mut SheetApp, ui: &mut Ui) {
    let Some(d) = app.session.active() else { return };
    let Some(sh) = d.wb.active() else { return };
    let list: Vec<(String, cella_engine::model::Comment)> = sh.comments.iter().map(|(c, m)| (c.a1(), m.clone())).collect();
    if list.is_empty() {
        ui.label(egui::RichText::new(crate::widgets::tr(ui, "No comments on this sheet yet. Select a cell and choose New Comment.")).italics());
    }
    for (cell, c) in list {
        egui::Frame::NONE
            .fill(if c.resolved { Color32::from_gray(240) } else { Color32::WHITE })
            .corner_radius(6.0)
            .inner_margin(8.0)
            .stroke(egui::Stroke::new(1.0, Color32::from_gray(215)))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    if ui.link(egui::RichText::new(&cell).strong()).clicked() {
                        let _ = app.run("selection.set", json!({"cell": cell}));
                        app.grid.ensure_visible = true;
                    }
                    ui.label(egui::RichText::new(&c.author).color(Color32::from_gray(90)));
                    if c.resolved {
                        ui.label(egui::RichText::new(crate::widgets::tr(ui, "Resolved")).small().color(Color32::from_rgb(0x10, 0x7C, 0x41)));
                    }
                });
                ui.label(&c.text);
                for (a, txt) in &c.replies {
                    ui.label(egui::RichText::new(format!("↳ {a}: {txt}")).color(Color32::from_gray(60)));
                }
                let key = egui::Id::new(("reply", &cell));
                let mut draft: String = ui.ctx().data_mut(|m| m.get_temp(key)).unwrap_or_default();
                // Buttons first, right to left, so the reply field gets exactly the width they leave:
                // a field sized by a guess wider than the row pushes the resizable pane wider every frame.
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(crate::widgets::tr(ui, "Delete")).clicked() {
                            app.run_or_alert("review.deleteComment", json!({"cell": cell}));
                        }
                        if ui.small_button(crate::widgets::tr(ui, if c.resolved { "Reopen" } else { "Resolve" })).clicked() {
                            app.run_or_alert("review.resolveComment", json!({"cell": cell, "resolved": !c.resolved}));
                        }
                        let send = ui.small_button(crate::widgets::tr(ui, "Send")).clicked();
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut draft).hint_text(crate::widgets::tr(ui, "Reply…")).desired_width(ui.available_width()),
                        );
                        if (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) || send) && !draft.is_empty() {
                            app.run_or_alert("review.replyComment", json!({"cell": cell, "text": draft}));
                            draft.clear();
                        }
                    });
                });
                ui.ctx().data_mut(|m| m.insert_temp(key, draft));
            });
        ui.add_space(6.0);
    }
}

fn watch(app: &mut SheetApp, ui: &mut Ui) {
    if ui.button(crate::widgets::tr(ui, "Add Watch (active cell)")).clicked()
        && let Some(d) = app.session.active()
        && let Some(sh) = d.wb.active()
    {
        let r = format!("{}!{}", cella_engine::formula::quote_sheet(&sh.name), d.selection.active.a1());
        let _ = app.session.run("formulas.watchWindow", json!({"add": r}));
    }
    let list = app.session.run("formulas.watchWindow", json!({})).unwrap_or(Json::Null);
    egui::Grid::new("watch_grid").striped(true).num_columns(4).show(ui, |ui| {
        ui.strong(crate::widgets::tr(ui, "Cell"));
        ui.strong(crate::widgets::tr(ui, "Value"));
        ui.strong(crate::widgets::tr(ui, "Formula"));
        ui.label("");
        ui.end_row();
        for w in list.as_array().cloned().unwrap_or_default() {
            let cell = w["cell"].as_str().unwrap_or("").to_string();
            ui.label(&cell);
            ui.label(w["value"].as_str().unwrap_or(""));
            ui.label(w["formula"].as_str().unwrap_or(""));
            if ui.small_button("✕").clicked() {
                let _ = app.session.run("formulas.watchWindow", json!({"remove": cell}));
            }
            ui.end_row();
        }
    });
}

fn selection(app: &mut SheetApp, ui: &mut Ui) {
    let list = app.session.run("arrange.selectionPane", json!({})).unwrap_or(Json::Null);
    let items = list.as_array().cloned().unwrap_or_default();
    // PivotTable slicers and timelines live in their own sheet lists, which `arrange.selectionPane`
    // does not carry (the arrange commands stack charts, pictures and shapes only). They are read
    // from the sheet here so they can be selected and deleted from the same pane; bring forward /
    // send backward has no meaning for a widget outside that stacking order.
    let widgets: Vec<(String, u32)> = app
        .session
        .active()
        .and_then(|d| d.wb.active())
        .map(|sh| {
            sh.pivot_slicers
                .iter()
                .map(|s| (crate::widgets::tr(ui, "Slicer {}").replace("{}", &s.name), s.id))
                .chain(sh.pivot_timelines.iter().map(|t| (crate::widgets::tr(ui, "Timeline {}").replace("{}", &t.name), t.id)))
                .collect()
        })
        .unwrap_or_default();
    if items.is_empty() && widgets.is_empty() {
        ui.label(egui::RichText::new(crate::widgets::tr(ui, "There are no shapes, pictures, charts or slicers on this sheet.")).italics());
    }
    for it in items.iter().rev() {
        let kind = it["kind"].as_str().unwrap_or("").to_string();
        let id = it["id"].as_u64().unwrap_or(0) as u32;
        ui.horizontal(|ui| {
            let sel = app.selected_chart == Some(id);
            if ui.selectable_label(sel, it["name"].as_str().unwrap_or("")).clicked() {
                app.selected_chart = Some(id);
            }
            if ui.small_button("▲").on_hover_text(crate::widgets::tr(ui, "Bring Forward")).clicked() {
                app.run_or_alert("arrange.bringForward", json!({"kind": kind, "id": id}));
            }
            if ui.small_button("▼").on_hover_text(crate::widgets::tr(ui, "Send Backward")).clicked() {
                app.run_or_alert("arrange.sendBackward", json!({"kind": kind, "id": id}));
            }
            if ui.small_button("✕").on_hover_text(crate::widgets::tr(ui, "Delete")).clicked() {
                app.run_or_alert("object.delete", json!({"kind": kind, "id": id}));
            }
        });
    }
    for (name, id) in widgets {
        ui.horizontal(|ui| {
            if ui.selectable_label(app.selected_chart == Some(id), name).clicked() {
                app.selected_chart = Some(id);
            }
            if ui.small_button("✕").on_hover_text(crate::widgets::tr(ui, "Delete")).clicked() {
                app.run_or_alert("slicer.delete", json!({"slicer": id}));
                if app.selected_chart == Some(id) {
                    app.selected_chart = None;
                }
            }
        });
    }
}

fn format_chart(app: &mut SheetApp, ui: &mut Ui) {
    crate::chart_format::show(app, ui);
}

/// Queries & Connections: the saved Get & Transform queries, their steps and a preview of what
/// they produce. Every button runs a `data.query.*` command, so the pane is a view of engine state
/// and nothing it does is unreachable from the control channel.
fn queries(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let list = app.session.run("data.query.list", json!({})).unwrap_or(Json::Null);
    let queries = list.as_array().cloned().unwrap_or_default();
    ui.horizontal(|ui| {
        if ui.button(crate::widgets::tr(ui, "Refresh All")).clicked() {
            match app.session.run("data.refreshAll", json!({})) {
                Ok(v) => {
                    let n = v["queries"].as_array().map_or(0, Vec::len);
                    app.toast = Some((crate::widgets::tr(ui, "Refreshed {}").replace("{}", &n.to_string()), crate::now_ms()));
                }
                Err(e) => app.message = Some(("Cella".into(), crate::clean_error(&e))),
            }
        }
        if ui.button(crate::widgets::tr(ui, "New query from file…")).clicked()
            && let Some(path) = app.services.pick_open.as_ref().and_then(|pick| pick())
        {
            // A query is a recipe over a file, so the pane asks for the file and keeps the recipe:
            // the Data tab's Get Data opens the same file as a workbook instead.
            match app.session.run("data.query.add", json!({"source": source_for_path(&path)})) {
                Ok(_) => app.toast = Some((crate::widgets::tr(ui, "Query added").to_string(), crate::now_ms())),
                Err(e) => app.message = Some(("Cella".into(), crate::clean_error(&e))),
            }
        }
    });
    ui.add_space(4.0);
    if queries.is_empty() {
        ui.label(egui::RichText::new(crate::widgets::tr(ui, "No queries yet. Get Data (Text/CSV) makes one.")).color(t.text_dim));
        return;
    }
    for q in &queries {
        let name = q["name"].as_str().unwrap_or("").to_string();
        let selected = app.grid.query_selected.as_deref() == Some(name.as_str());
        let kind = q["kind"].as_str().unwrap_or("");
        let path = q["path"].as_str().unwrap_or("");
        let sheet = q["sheet"].as_str().unwrap_or("");
        let steps = q["steps"].as_array().cloned().unwrap_or_default();
        let columns = q["columns"].as_array().cloned().unwrap_or_default();
        egui::Frame::NONE.fill(if selected { t.sel_fill } else { t.ribbon }).inner_margin(8).corner_radius(5.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let caret = if selected { "▾" } else { "▸" };
                let r = ui.small_button(caret);
                if r.clicked() {
                    app.grid.query_selected = if selected { None } else { Some(name.clone()) };
                }
                let title = egui::RichText::new(&name).font(theme::ui_bold(13.0));
                if ui.selectable_label(selected, title).clicked() {
                    app.grid.query_selected = if selected { None } else { Some(name.clone()) };
                }
                if ui.small_button(crate::widgets::tr(ui, "Refresh")).clicked() {
                    match app.session.run("data.query.refresh", json!({"name": name})) {
                        Ok(_) => app.toast = Some((crate::widgets::tr(ui, "Refreshed {}").replace("{}", "1"), crate::now_ms())),
                        Err(e) => app.message = Some(("Cella".into(), crate::clean_error(&e))),
                    }
                }
                if ui.small_button(crate::widgets::tr(ui, "Delete")).clicked() {
                    match app.session.run("data.query.delete", json!({"name": name})) {
                        Ok(_) => {
                            if selected {
                                app.grid.query_selected = None;
                            }
                            app.toast = Some((crate::widgets::tr(ui, "Query deleted").to_string(), crate::now_ms()));
                        }
                        Err(e) => app.message = Some(("Cella".into(), crate::clean_error(&e))),
                    }
                }
            });
            // Where the data comes from, in one line: kind, file, and the sheet it loads onto.
            let mut detail = kind.to_string();
            if !path.is_empty() {
                detail.push_str(" · ");
                detail.push_str(path);
            }
            if !sheet.is_empty() {
                detail.push_str(&format!(" → {sheet}"));
            }
            ui.label(egui::RichText::new(detail).color(t.text_dim).size(11.0));
            if selected {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(crate::widgets::tr(ui, "Steps")).strong());
                for (i, step) in steps.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("{}. {}", i + 1, crate::widgets::tr(ui, step.as_str().unwrap_or("")))).size(12.0));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::widgets::icon_button(ui, crate::icons::Icon::Close, t.text_dim, "Remove step", vec2(16.0, 16.0)).clicked() {
                                match app.session.run("data.query.removeStep", json!({"name": name, "index": i})) {
                                    Ok(_) => app.toast = Some((crate::widgets::tr(ui, "Step removed").to_string(), crate::now_ms())),
                                    Err(e) => app.message = Some(("Cella".into(), crate::clean_error(&e))),
                                }
                            }
                        });
                    });
                }
                if !columns.is_empty() {
                    ui.add_space(4.0);
                    preview(app, ui, &name);
                }
            }
        });
        ui.add_space(4.0);
    }
    model_section(app, ui);
}

/// The data model, as the pane shows it: the tables a query loaded without a sheet, and the
/// measures defined over them. Read-only here — the relationships and formulas are commands and a
/// field list is the next round — but seeing what is in the model is what makes a measure that
/// reads it explicable.
fn model_section(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let tables = app.session.run("data.model.tables", json!({})).unwrap_or(Json::Null);
    let measures = app.session.run("data.measure.list", json!({})).unwrap_or(Json::Null);
    let tables = tables.as_array().cloned().unwrap_or_default();
    let measures = measures.as_array().cloned().unwrap_or_default();
    if tables.is_empty() && measures.is_empty() {
        return;
    }
    ui.add_space(6.0);
    ui.separator();
    ui.label(egui::RichText::new(crate::widgets::tr(ui, "Data model")).font(theme::ui_bold(13.0)));
    for table in &tables {
        let name = table["name"].as_str().unwrap_or("");
        let rows = table["rows"].as_u64().unwrap_or(0);
        let cols = table["columns"].as_array().map_or(0, Vec::len);
        ui.label(egui::RichText::new(format!("{name}  ·  {rows} × {cols}")).size(12.0));
    }
    if !measures.is_empty() {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(crate::widgets::tr(ui, "Measures")).strong());
        for m in &measures {
            let name = m["name"].as_str().unwrap_or("");
            let formula = m["formula"].as_str().unwrap_or("");
            ui.label(egui::RichText::new(format!("{name} = {formula}")).size(12.0).color(t.text_dim));
        }
    }
}

/// The query source a picked file becomes, by extension: a csv (a tab-delimited text file gets a
/// tab), a json, an xlsx sheet or a parquet file. Anything else is read as csv, which is what the
/// extension-less export of most tools is.
fn source_for_path(path: &str) -> Json {
    let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "json" => json!({"kind": "json", "path": path}),
        "xlsx" | "xlsm" => json!({"kind": "xlsx", "path": path}),
        "parquet" => json!({"kind": "parquet", "path": path}),
        "tsv" | "txt" => json!({"kind": "csv", "path": path, "delimiter": "	"}),
        _ => json!({"kind": "csv", "path": path}),
    }
}

/// The first rows of a query, as the pipeline leaves them: the editor's whole point is that a step
/// can be judged by what it does to the data, and a failing step is reported where it stops.
fn preview(app: &mut SheetApp, ui: &mut Ui, name: &str) {
    let t = Tokens::get(ui.ctx());
    let pv = match app.session.run("data.query.preview", json!({"name": name, "limit": 10})) {
        Ok(v) => v,
        Err(e) => {
            ui.label(egui::RichText::new(e).color(t.danger).size(11.0));
            return;
        }
    };
    if let Some(failed) = pv["failed"].as_object() {
        let msg = failed.get("error").and_then(Json::as_str).unwrap_or("");
        ui.label(egui::RichText::new(crate::widgets::tr(ui, msg)).color(t.danger).size(11.0));
    }
    let columns = pv["columns"].as_array().cloned().unwrap_or_default();
    let rows = pv["rows"].as_array().cloned().unwrap_or_default();
    egui::ScrollArea::horizontal().id_salt(("query_preview", name)).show(ui, |ui| {
        egui::Grid::new(("query_preview_grid", name)).striped(true).show(ui, |ui| {
            for c in &columns {
                ui.strong(c.as_str().unwrap_or(""));
            }
            ui.end_row();
            for row in &rows {
                for cell in row.as_array().cloned().unwrap_or_default() {
                    let text = match cell {
                        Json::Null => String::new(),
                        Json::String(s) => s,
                        other => other.to_string(),
                    };
                    ui.label(text);
                }
                ui.end_row();
            }
        });
    });
    let total = pv["total"].as_u64().unwrap_or(0);
    let mut footer = crate::widgets::tr(ui, "{} rows").replace("{}", &total.to_string());
    if pv["truncated"].as_bool().unwrap_or(false) {
        footer.push_str(&format!(" ({})", crate::widgets::tr(ui, "first 10 shown")));
    }
    ui.label(egui::RichText::new(footer).color(t.text_dim).size(11.0));
}

#[cfg(test)]
mod tests {
    use super::title_of;

    #[test]
    fn the_format_pane_is_titled_after_the_object() {
        assert_eq!(title_of(Some("shape")), "Format Shape");
        assert_eq!(title_of(Some("image")), "Format Picture");
        // A chart, a slicer and an empty selection all keep the chart title the pane shipped with.
        for other in [Some("chart"), Some("slicer"), Some("timeline"), None] {
            assert_eq!(title_of(other), "Format Chart Area", "{other:?}");
        }
    }
}
