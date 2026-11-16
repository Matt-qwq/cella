//! A click on a form control's button runs the command assigned to it, as one user action.

use egui::{Event, Modifiers, PointerButton, Rect, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::display::cell_text;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::grid::Geo;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// A workbook with a button at C3 that writes `hit` into A1.
fn fixture() -> (Harness, u32) {
    let mut session = Session::new();
    session.new_workbook();
    let made = session
        .execute("insert.button", json!({"at": "C3", "text": "Go", "command": "cell.set", "params": {"cell": "A1", "input": "hit"}}))
        .expect("insert.button");
    let id = made["control"].as_u64().unwrap_or(0) as u32;
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    (h, id)
}

/// The button's rect on screen.
fn button_rect(h: &Harness, id: u32) -> Rect {
    let app = h.state();
    let sh = app.session.doc().expect("document").wb.active().expect("sheet");
    let geo = Geo::new(sh, app.grid.rect.expect("grid"), app.view().scroll);
    let a = sh.controls.iter().find(|c| c.id == id).expect("the button").anchor;
    let min = geo.cell_rect(sh, CellRef::parse("C3").expect("cell")).min + vec2(a.dx, a.dy) * geo.z;
    Rect::from_min_size(min, vec2(a.width, a.height) * geo.z)
}

fn click(h: &mut Harness, at: egui::Pos2) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
}

fn cell(h: &Harness, at: &str) -> String {
    let d = h.state().session.doc().expect("document");
    cell_text(&d.wb, d.wb.active().expect("sheet"), CellRef::parse(at).expect("a cell"))
}

#[test]
fn clicking_a_button_runs_its_command() {
    let (mut h, id) = fixture();
    assert_eq!(cell(&h, "A1"), "", "nothing runs before the click");
    let r = button_rect(&h, id);
    click(&mut h, r.center());
    assert_eq!(cell(&h, "A1"), "hit", "the click ran the button's command");
    // A click on an empty cell still selects cells instead: the button took only its own clicks.
    click(&mut h, r.left_top() + vec2(-60.0, 0.0));
    assert_eq!(cell(&h, "A1"), "hit");
}
