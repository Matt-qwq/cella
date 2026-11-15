use egui_kittest::kittest::{NodeT as _, Queryable as _};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;

#[test]
fn dump_agents() {
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        {
            let mut session = Session::new();
            session.new_workbook();
            let mut app = SheetApp::new(session, Default::default());
            app.control_port = Some(7979);
            app.open_dialog("agents", serde_json::json!({}));
            app
        },
    );
    h.run_steps(4);
    let mut out = Vec::new();
    for node in h.query_all_by_label_contains("") {
        let n = node.accesskit_node();
        if let Some(l) = n.label().or_else(|| n.value()) {
            out.push(l.to_string());
        }
    }
    println!("LABELS: {}", out.join(" | "));
}
