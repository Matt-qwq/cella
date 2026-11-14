//! Queries & Connections reads Chinese: the pane's title, its buttons, the query steps (which are
//! engine labels the pane translates) and the preview's footer. A step whose label was never added
//! to the catalog would show English here.

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::i18n::Language;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(session: Session, language: Language) -> Harness {
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        {
            let mut app = SheetApp::new(session, Default::default());
            app.ui.language = language;
            app.grid.pane = Some("queries".into());
            app
        },
    );
    h.run_steps(4);
    h
}

/// A CSV query with two steps, expanded, so the pane draws the steps and the preview.
fn query_session() -> Session {
    let mut session = Session::new();
    session.new_workbook();
    session
        .execute(
            "data.query.add",
            json!({
                "name": "Sales",
                "source": {"kind": "csv", "text": "Region,Amount\nWest,10\nEast,5\n"},
                "steps": ["PromoteHeaders", {"ChangeType": {"columns": ["Amount"], "to": "Number"}}],
            }),
        )
        .unwrap();
    session
}

fn expanded_harness(language: Language) -> Harness {
    let mut h = harness(query_session(), language);
    // The pane starts collapsed; open the query the way a click does.
    h.state_mut().grid.query_selected = Some("Sales".into());
    h.run_steps(3);
    h
}

#[test]
fn the_queries_pane_speaks_chinese() {
    let h = expanded_harness(Language::Zh);
    for label in ["查询和连接", "全部刷新", "刷新", "删除", "步骤", "1. 提升的标题", "2. 更改的类型"] {
        assert!(h.query_by_label(label).is_some(), "the queries pane should show {label}");
    }
    // And the query's own name is data, so it stays exactly as it was typed.
    assert!(h.query_all_by_label("Sales").next().is_some(), "the query name is data");
}

#[test]
fn the_queries_pane_reads_english_on_an_english_screen() {
    let h = expanded_harness(Language::En);
    for label in ["Queries & Connections", "Refresh All", "Steps", "1. Promoted Headers", "2. Changed Type"] {
        assert!(h.query_by_label(label).is_some(), "the queries pane should show {label}");
    }
}

#[test]
fn an_empty_pane_says_how_to_make_a_query() {
    let mut session = Session::new();
    session.new_workbook();
    let h = harness(session, Language::Zh);
    assert!(h.query_by_label("还没有查询。用“获取数据（文本/CSV）”新建一个。").is_some(), "the empty pane explains itself");
}

/// A workbook whose data model holds one table and one measure.
fn model_session() -> Session {
    let mut session = Session::new();
    session.new_workbook();
    session
        .execute(
            "data.query.add",
            json!({
                "name": "Sales",
                "source": {"kind": "csv", "text": "Region,Amount\nWest,10\nEast,5\n"},
                "steps": ["PromoteHeaders"],
                "load": "model",
            }),
        )
        .unwrap();
    session.execute("data.measure.add", json!({"name": "Total", "formula": "SUM(Sales[Amount])"})).unwrap();
    session
}

#[test]
fn the_pane_shows_what_is_in_the_data_model() {
    let h = harness(model_session(), Language::Zh);
    for label in ["数据模型", "度量值"] {
        assert!(h.query_by_label(label).is_some(), "the model section should show {label}");
    }
    // The measure reads as its formula, which is data: the names stay as they were written.
    assert!(h.query_all_by_label_contains("Total = SUM(Sales[Amount])").next().is_some(), "the measure line");
    // And the model table is listed with its shape.
    assert!(h.query_all_by_label_contains("Sales  ·  2 × 2").next().is_some(), "the table line");
}
