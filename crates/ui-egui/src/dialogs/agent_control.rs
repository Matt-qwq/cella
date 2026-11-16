use crate::SheetApp;

/// Automate ▸ Agent Control: how an agent drives this application.
///
/// GridCraft has no in-app assistant on purpose (decision 11-14): a model inside the window would
/// have to ship a network client, a key store and a second way to run a command, and it would be
/// absent from the web build — while an agent outside the window already reaches **every** command
/// through the same registry the menus use. This dialog is that story's front door: where to
/// connect, what to send, and what it can reach.
pub(super) fn agent_control(app: &mut SheetApp, ui: &mut egui::Ui) {
    let port = app.control_port.unwrap_or(7979);
    let listening = app.control_rx.is_some();
    let commands = gridcraft_engine::command_specs().len();
    // The dialog has a 520-point frame; a pasted command line is longer than that, so every line
    // is truncated to the width available and shows itself in full on hover. Widening the dialog to
    // fit the longest command would put it over half the sheet.
    ui.set_max_width(500.0);
    if listening {
        ui.label(egui::RichText::new(crate::widgets::tr(ui, "Listening on 127.0.0.1:{port}.").replace("{port}", &port.to_string())).strong());
    } else {
        ui.label(
            egui::RichText::new(
                crate::widgets::tr(ui, "Not listening. Start GridCraft with --control {port} to let agents in.").replace("{port}", &port.to_string()),
            )
            .color(crate::theme::Tokens::get(ui.ctx()).warning_ink),
        );
    }
    ui.add_space(6.0);
    ui.label(crate::widgets::tr(
        ui,
        "Every menu item, button and gesture is a command, so an agent drives the same engine this window does. Three ways in:",
    ));
    for (what, line) in [
        ("MCP (this window)", format!("gridcraft-cli mcp --connect {port}")),
        ("MCP (headless)", "gridcraft-cli mcp".to_string()),
        ("CLI, one command", "gridcraft-cli run --in book.xlsx --cmd 'home.bold={\"range\":\"A1:C1\"}' --out book.xlsx".to_string()),
        ("CLI, the catalogue", "gridcraft-cli commands".to_string()),
    ] {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(crate::widgets::tr(ui, what)).size(12.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(crate::widgets::tr(ui, "Copy")).clicked() {
                    ui.ctx().copy_text(line.clone());
                }
            });
        });
        ui.add(egui::Label::new(egui::RichText::new(&line).monospace().size(11.0)).truncate()).on_hover_text(&line);
    }
    ui.add_space(8.0);
    ui.label(egui::RichText::new(crate::widgets::tr(ui, "A control-channel request and its answer:")).strong());
    let request =
        "{\"id\":1,\"method\":\"engine.execute\",\"params\":{\"command\":\"cell.set\",\"params\":{\"cell\":\"B2\",\"input\":\"=SUM(A1:A9)\"}}}}"
            .to_string();
    ui.horizontal(|ui| {
        // The button is placed first in a right-to-left layout, so the truncated line gets what is
        // left: adding the line first let it run under the button.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(crate::widgets::tr(ui, "Copy")).clicked() {
                ui.ctx().copy_text(request.clone());
            }
            ui.add(egui::Label::new(egui::RichText::new(&request).monospace().size(11.0)).truncate()).on_hover_text(&request);
        });
    });
    ui.add(egui::Label::new(egui::RichText::new("{\"id\":1,\"ok\":true,\"result\":{...}}").monospace().size(11.0)).truncate());
    ui.add_space(8.0);
    ui.label(
        crate::widgets::tr(ui, "{n} commands are reachable this way, with the same parameters the menus pass. A command with empty params uses its defaults; it never opens a dialog.")
            .replace("{n}", &commands.to_string()),
    );
    ui.add_space(4.0);
    ui.add(
        egui::Label::new(
            egui::RichText::new(crate::widgets::tr(
                ui,
                "Methods: docs/control-protocol.md; tools: docs/mcp.md. To see what an agent did, the Action Journal lists every command; inspect.whatChanged answers cell by cell.",
            ))
            .size(11.0)
            .color(crate::theme::Tokens::get(ui.ctx()).text_dim),
        )
        .wrap(),
    );
}
