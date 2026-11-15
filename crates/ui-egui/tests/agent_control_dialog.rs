//! Automate > Agent Control reads Chinese and says what it is: where an agent connects, not a chat
//! box. GridCraft has no in-app assistant on purpose (decision 11-14).

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::i18n::Language;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// "listening" gives the app a control-channel receiver, which is what the desktop app holds once
/// --control started the server. Nothing is read from it; the dialog only asks whether it exists.
fn harness(language: Language, listening: bool) -> Harness {
    let keep = std::sync::mpsc::channel();
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
            app.ui.language = language;
            app.control_port = Some(7979);
            if listening {
                app.control_rx = Some(keep.1);
            }
            app.open_dialog("agents", serde_json::json!({}));
            app
        },
    );
    h.run_steps(4);
    h
}

#[test]
fn the_agent_dialog_says_where_to_connect_and_what_to_send() {
    let h = harness(Language::En, true);
    for label in [
        "Listening on 127.0.0.1:7979.",
        "MCP (this window)",
        "gridcraft-cli mcp --connect 7979",
        "MCP (headless)",
        "CLI, one command",
        "A control-channel request and its answer:",
    ] {
        assert!(h.query_by_label(label).is_some(), "the dialog should show {label}");
    }
}

#[test]
fn a_window_without_a_control_channel_says_how_to_start_one() {
    let h = harness(Language::En, false);
    assert!(h.query_by_label("Not listening. Start GridCraft with --control 7979 to let agents in.").is_some(), "the hint");
}

#[test]
fn the_agent_dialog_speaks_chinese() {
    let h = harness(Language::Zh, true);
    for label in ["正在 127.0.0.1:7979 上监听。", "MCP（本窗口）", "MCP（无窗口）", "CLI，单条命令", "一条控制通道请求及其应答："]
    {
        assert!(h.query_by_label(label).is_some(), "the agent dialog should show {label}");
    }
    // The five copy buttons are the only repeated label in the dialog.
    assert_eq!(h.query_all_by_label("复制").count(), 5, "every copy button reads Chinese");
}
