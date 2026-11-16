use super::Dialog;

/// Help ▸ About GridCraft: the app, its contributors and the AI models that helped. The credits
/// are compiled in (`crate::credits`, docs/contributors.md).
pub(super) fn about(ui: &mut egui::Ui, d: &mut Dialog) {
    ui.horizontal(|ui| {
        for tab in ["About", "Contributors", "Models"] {
            let tab_label = crate::widgets::tr(ui, tab);
            if ui.selectable_label(d.tab == tab, tab_label).clicked() {
                d.tab = tab.to_string();
            }
        }
    });
    ui.separator();
    match d.tab.as_str() {
        "Contributors" => crate::credits::contributors_ui(ui),
        "Models" => crate::credits::models_ui(ui),
        _ => {
            ui.heading("GridCraft");
            ui.label(crate::widgets::tr(ui, "Version {version}").replace("{version}", env!("CARGO_PKG_VERSION")));
            ui.add_space(4.0);
            ui.label(crate::widgets::tr(ui, "A clean-room, open-source, Rust-native spreadsheet. Part of ArtCraft."));
            ui.hyperlink_to("getartcraft.com/apps/gridcraft", "https://getartcraft.com/apps/gridcraft");
            ui.hyperlink_to(crate::widgets::tr(ui, "Join the ArtCraft Discord"), "https://discord.gg/artcraft");
        }
    }
}
