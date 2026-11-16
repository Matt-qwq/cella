//! The formatting toggles' state: what Bold/Italic/Underline/Strikethrough show, and that a click
//! repaints the grid in the same frame it lands in.

use egui::{
    Event, Key, Modifiers, Shape,
    accesskit::{Role, Toggled},
};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::Style;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

/// Ctrl on Windows and Linux sets both ctrl and command.
const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };
const SHIFT: Modifiers = Modifiers { alt: false, ctrl: false, shift: true, mac_cmd: false, command: false };

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(build: impl FnOnce(&mut Session)) -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "left"})).unwrap();
    s.execute("cell.set", json!({"cell": "B1", "input": "right"})).unwrap();
    build(&mut s);
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
    );
    h.run_steps(4);
    h
}

fn style(h: &Harness, a1: &str) -> Style {
    let d = h.state().session.active().unwrap();
    let sh = d.wb.active().unwrap();
    d.wb.styles.get(sh.style_id(CellRef::parse(a1).unwrap())).clone()
}

/// Whether a toggle button reports itself pressed (accesskit calls it toggled).
fn pressed(h: &Harness, label: &str) -> Option<bool> {
    let node = h.get_by_role_and_label(Role::Button, label);
    match node.accesskit_node().toggled() {
        Some(Toggled::True) => Some(true),
        Some(Toggled::False) => Some(false),
        _ => None,
    }
}

/// The font family the grid paints a cell's text in.
fn painted_family(h: &Harness, text: &str) -> Option<String> {
    let grid = h.state().grid.rect?;
    h.output().shapes.iter().find_map(|s| match &s.shape {
        Shape::Text(t) if t.galley.job.text == text && t.pos.y >= grid.top() => {
            t.galley.job.sections.first().map(|section| format!("{:?}", section.format.font_id.family))
        }
        _ => None,
    })
}

fn key(h: &mut Harness, k: Key, m: Modifiers) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: m });
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: m });
    h.run_steps(2);
}

#[test]
fn bold_applied_by_a_command_renders_the_button_pressed() {
    let h = harness(|s| {
        s.execute("home.bold", json!({"range": "A1", "on": true})).unwrap();
        s.execute("selection.set", json!({"range": "A1"})).unwrap();
    });
    assert!(style(&h, "A1").font.bold, "the command set bold on A1");
    assert_eq!(pressed(&h, "Bold (Ctrl+B)"), Some(true), "the Bold button shows the cell's bold as pressed");
    assert_eq!(pressed(&h, "Italic (Ctrl+I)"), Some(false));
    assert_eq!(pressed(&h, "Strikethrough"), Some(false));
    assert_eq!(painted_family(&h, "left"), Some("Name(\"cell-bold\")".to_string()), "the grid draws A1 in the bold family");
}

#[test]
fn clicking_bold_sets_the_style_and_repaints_before_the_next_frame() {
    let mut h = harness(|s| {
        s.execute("selection.set", json!({"range": "A1"})).unwrap();
    });
    assert_eq!(painted_family(&h, "left"), Some("Name(\"cell\")".to_string()), "A1 starts in the plain family");

    // A step runs one frame per queued pointer event: the release frame is the one the click lands
    // in. The grid is drawn after the ribbon, so it already paints the new style in that frame —
    // the button's own face is read at the top of the frame, before its click is handled, and
    // catches up on the next one (a single frame; egui requests the repaint).
    h.get_by_role_and_label(Role::Button, "Bold (Ctrl+B)").click();
    h.step();
    assert!(style(&h, "A1").font.bold, "the click reached the engine");
    assert_eq!(painted_family(&h, "left"), Some("Name(\"cell-bold\")".to_string()), "the grid paints A1 bold in the very frame of the click");
    h.step();
    assert_eq!(pressed(&h, "Bold (Ctrl+B)"), Some(true), "and the button shows pressed on the next frame");

    // And off again.
    h.get_by_role_and_label(Role::Button, "Bold (Ctrl+B)").click();
    h.step();
    assert!(!style(&h, "A1").font.bold, "a second click turns it off");
    assert_eq!(painted_family(&h, "left"), Some("Name(\"cell\")".to_string()), "the grid paints plain again in the same frame");
    h.step();
    assert_eq!(pressed(&h, "Bold (Ctrl+B)"), Some(false));
}

#[test]
fn italic_underline_and_strikethrough_click_through_to_their_styles() {
    let mut h = harness(|s| {
        s.execute("selection.set", json!({"range": "A1"})).unwrap();
    });

    h.get_by_role_and_label(Role::Button, "Italic (Ctrl+I)").click();
    h.run_steps(3);
    assert!(style(&h, "A1").font.italic, "Italic reaches the engine");
    assert_eq!(pressed(&h, "Italic (Ctrl+I)"), Some(true), "Italic shows pressed");
    assert_eq!(painted_family(&h, "left"), Some("Name(\"cell-italic\")".to_string()), "the grid paints italic");

    // Underline is a split button: its main half toggles, its arrow lists the styles.
    let main = h.get_all_by_role_and_label(Role::Button, "Underline (Ctrl+U)").min_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x));
    main.expect("Underline has a main half").click();
    h.run_steps(3);
    assert_eq!(style(&h, "A1").font.underline, gridcraft_engine::model::Underline::Single, "Underline reaches the engine");

    let arrow = h.get_all_by_role_and_label(Role::Button, "Underline (Ctrl+U)").max_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x));
    arrow.expect("Underline has an arrow").click();
    h.run_steps(4);
    h.get_by_role_and_label(Role::Button, "Double Underline").click();
    h.run_steps(3);
    assert_eq!(style(&h, "A1").font.underline, gridcraft_engine::model::Underline::Double, "Double Underline reaches the engine");

    h.get_by_role_and_label(Role::Button, "Strikethrough").click();
    h.run_steps(3);
    assert!(style(&h, "A1").font.strike, "Strikethrough reaches the engine");
    assert_eq!(pressed(&h, "Strikethrough"), Some(true), "Strikethrough shows pressed");
}

#[test]
fn the_toggles_follow_the_active_cell_there_is_no_mixed_state() {
    // The ribbon reads one style: the active (anchor) cell's. Excel shows the same, so a selection
    // of a bold cell and a plain one shows whatever the anchor is, never a third "mixed" look.
    let mut h = harness(|s| {
        s.execute("home.bold", json!({"range": "A1", "on": true})).unwrap();
        s.execute("selection.set", json!({"range": "B1"})).unwrap();
    });
    assert!(!style(&h, "B1").font.bold, "B1 is the plain one");
    assert_eq!(pressed(&h, "Bold (Ctrl+B)"), Some(false), "the anchor B1 is plain, so Bold is not pressed");

    // Shift+Left extends the selection to A1:B1 with B1 still the anchor.
    key(&mut h, Key::ArrowLeft, SHIFT);
    assert_eq!(h.state().session.active().unwrap().selection.current().a1(), "A1:B1");
    assert_eq!(pressed(&h, "Bold (Ctrl+B)"), Some(false), "still the anchor's (B1, plain) bold state");

    // The same two cells with A1 as the anchor: pressed.
    let mut h = harness(|s| {
        s.execute("home.bold", json!({"range": "A1", "on": true})).unwrap();
        s.execute("selection.set", json!({"range": "A1"})).unwrap();
    });
    key(&mut h, Key::ArrowRight, SHIFT);
    assert_eq!(h.state().session.active().unwrap().selection.current().a1(), "A1:B1");
    assert_eq!(pressed(&h, "Bold (Ctrl+B)"), Some(true), "the anchor A1 is bold, so Bold is pressed");

    // Clicking Bold toggles the anchor's state and applies the result to every cell: the anchor is
    // bold, so the click turns bold off for the whole selection (Excel does the same).
    h.get_by_role_and_label(Role::Button, "Bold (Ctrl+B)").click();
    h.run_steps(3);
    assert!(!style(&h, "A1").font.bold && !style(&h, "B1").font.bold, "the click applies the toggled anchor state to the whole selection");
    assert_eq!(pressed(&h, "Bold (Ctrl+B)"), Some(false));
}

#[test]
fn ctrl_b_ctrl_i_and_ctrl_u_reach_the_same_styles() {
    let mut h = harness(|s| {
        s.execute("selection.set", json!({"range": "A1:B1"})).unwrap();
    });

    key(&mut h, Key::B, CTRL);
    assert!(style(&h, "A1").font.bold && style(&h, "B1").font.bold, "Ctrl+B bolds the selection");
    assert_eq!(pressed(&h, "Bold (Ctrl+B)"), Some(true), "and the button shows it");

    key(&mut h, Key::I, CTRL);
    assert!(style(&h, "A1").font.italic && style(&h, "B1").font.italic, "Ctrl+I italicises the selection");
    assert_eq!(pressed(&h, "Italic (Ctrl+I)"), Some(true));

    key(&mut h, Key::U, CTRL);
    assert_eq!(style(&h, "A1").font.underline, gridcraft_engine::model::Underline::Single, "Ctrl+U underlines");

    // Ctrl+B again turns it off, as the button would.
    key(&mut h, Key::B, CTRL);
    assert!(!style(&h, "A1").font.bold, "Ctrl+B toggles back off");
    assert_eq!(pressed(&h, "Bold (Ctrl+B)"), Some(false));
}

#[test]
fn the_number_format_combo_follows_the_cells_format() {
    // The state half of the Number group: the combo's face names the active cell's format kind.
    let h = harness(|s| {
        s.execute("home.percent", json!({"range": "A1"})).unwrap();
        s.execute("selection.set", json!({"range": "A1"})).unwrap();
    });
    assert!(h.query_by_role_and_label(Role::Button, "Percentage").is_some(), "the combo shows Percentage for a percent cell");
    assert!(h.query_by_role_and_label(Role::Button, "General").is_none(), "and no longer shows General");
}
