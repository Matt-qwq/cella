//! A narrow window: the ribbon's groups that do not fit stay reachable through the » button,
//! which opens the whole tab, wrapped, in a popup — instead of hiding commands behind an
//! invisible horizontal scroll.

use cella_engine::Session;
use cella_ui_egui::SheetApp;
use egui::{Event, Modifiers, PointerButton, vec2};
use egui_kittest::kittest::Queryable as _;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(width: f32) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(width, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h.state_mut().ui.ribbon_tab = "Formulas".into();
    h.run_steps(4);
    h
}

fn click(h: &mut Harness, at: egui::Pos2) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
}

#[test]
fn a_narrow_window_offers_the_whole_tab_in_a_popup() {
    let mut h = harness(640.0);
    assert!(h.query_by_label("All commands on this tab").is_none(), "the popup starts closed");
    let at = h.get_by_label("More commands on this tab").rect().center();
    click(&mut h, at);
    h.run_steps(4);
    assert!(h.query_by_label("All commands on this tab").is_some(), "» opens the wrapped tab");
}

#[test]
fn a_wide_window_keeps_the_whole_tab_in_one_row() {
    let h = harness(1600.0);
    assert!(h.query_by_label("More commands on this tab").is_none(), "nothing overflows at 1600 px");
}
