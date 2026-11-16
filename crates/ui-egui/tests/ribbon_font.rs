//! Home ▸ Font by clicking: every control in the group and the Format Cells ▸ Font tab, driven
//! through real egui pointer input, asserting on the engine's style and on what the grid paints.
//!
//! The commands themselves are proven elsewhere (`cell.set`, then `home.bold` runs and the grid
//! draws bold); what these tests own is the path from the click: the widget, the parameters it
//! sends and the state it reads back.

use egui::accesskit::{Role, Toggled};
use egui::{Event, Modifiers, PointerButton, Shape, pos2, vec2};
use egui_kittest::kittest::{NodeT, Queryable as _};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::{BorderLine, BorderStyle, Borders, Color, Fill, Style, Underline};
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::theme::Tokens;
use serde_json::{Value as Json, json};

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// A blank workbook with 1 in A1, A1 selected, drawn at 1200 × 800.
fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1]]})).unwrap();
    s.execute("selection.set", json!({"cell": "A1"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
    );
    // The machine's font catalog is scanned in the background (see fonts::start_scan); when it
    // lands, the font set is rebuilt and one pass paints nothing. Give the harness enough passes
    // that the ribbon exists no matter when that happens.
    h.run_steps(16);
    h
}

fn click_at(h: &mut Harness, p: egui::Pos2) {
    h.input_mut().events.push(Event::PointerMoved(p));
    h.input_mut().events.push(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(2);
    h.input_mut().events.push(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

/// Clicks a menu entry of a split button, retrying the open: a font set rebuilt by the background
/// catalog scan can swallow the frame the popup opens on, and the click then lands on nothing.
fn menu_item(h: &mut Harness, label: &str, item: &str) {
    for _ in 0..4 {
        click_arrow(h, label);
        h.run_steps(3);
        if let Some(node) = h.query_by_label(item) {
            let at = node.rect().center();
            click_at(h, at);
            return;
        }
    }
    panic!("the {label} menu never showed {item}");
}

/// The leftmost node with this label: a split button names both halves, and its main part is the
/// left one.
fn main_half(h: &Harness, label: &str) -> egui::Rect {
    let mut nodes: Vec<_> = h.query_all_by_label(label).collect();
    nodes.sort_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x));
    nodes.first().map(|n| n.rect()).unwrap_or_else(|| h.get_by_label(label).rect())
}

/// Clicks the middle of the named widget.
fn click(h: &mut Harness, label: &str) {
    let rect = main_half(h, label);
    click_at(h, rect.center());
}

/// Clicks the dropdown half of a split button: its main part is the labelled 25pt-wide half and
/// the arrow is the rest of the 38pt button.
fn click_arrow(h: &mut Harness, label: &str) {
    let main = main_half(h, label);
    click_at(h, pos2(main.right() + 6.5, main.center().y));
}

fn style(h: &Harness) -> Style {
    style_of(h, "A1")
}

fn style_of(h: &Harness, cell: &str) -> Style {
    let d = h.state().session.active().unwrap();
    let sh = d.wb.active().unwrap();
    d.wb.styles.get(sh.style_id(CellRef::parse(cell).unwrap())).clone()
}

/// The colours the grid paints the text of a cell in.
fn painted_text_colours(h: &Harness, cell: &str) -> Vec<egui::Color32> {
    let sh = h.state().session.active().unwrap().wb.active().unwrap().clone();
    let cell = CellRef::parse(cell).unwrap();
    let geo = h.state().grid.geo.as_ref().unwrap();
    let area = egui::Rect::from_min_max(pos2(geo.x(&sh, cell.col), geo.y(&sh, cell.row)), pos2(geo.x(&sh, cell.col + 1), geo.y(&sh, cell.row + 1)));
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Text(t) if area.contains(t.pos) => t.galley.job.sections.first().map(|s| s.format.color),
            _ => None,
        })
        .collect()
}

/// Whether the widget draws itself as pressed: a filled rectangle in the pressed token behind it.
fn paints_pressed(h: &Harness, label: &str) -> bool {
    let rect = main_half(h, label);
    let pressed = Tokens::get(&h.ctx).pressed;
    h.output().shapes.iter().any(|s| match &s.shape {
        Shape::Rect(r) => r.fill == pressed && r.rect.contains(rect.center()),
        _ => false,
    })
}

fn toggled(h: &Harness, label: &str) -> Option<Toggled> {
    let mut nodes: Vec<_> = h.query_all_by_label(label).collect();
    nodes.sort_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x));
    nodes.first().map(|n| n.accesskit_node().toggled()).unwrap_or(None)
}

fn run(h: &mut Harness, id: &str, params: Json) -> Json {
    h.state_mut().session.run(id, params).unwrap_or(Json::Null)
}

fn line() -> BorderLine {
    line_of(BorderStyle::Thin)
}

fn line_of(style: BorderStyle) -> BorderLine {
    BorderLine { style, color: Color::Auto }
}

#[test]
fn bold_italic_and_strikethrough_click_on_and_off_and_draw_pressed() {
    for (label, attr) in [("Bold (Ctrl+B)", "bold"), ("Italic (Ctrl+I)", "italic"), ("Strikethrough", "strike")] {
        let mut h = harness();
        assert_eq!(toggled(&h, label), Some(Toggled::False), "{label} starts unpressed");
        assert!(!paints_pressed(&h, label), "{label} starts unpainted");

        // A two-cell selection: the click formats the whole selection, not just the active cell.
        run(&mut h, "selection.set", json!({"range": "A1:B2"}));
        h.run_steps(2);
        click(&mut h, label);
        for cell in ["A1", "B2"] {
            let on = run(&mut h, "cell.get", json!({"cell": cell}))["style"]["font"][attr].clone();
            assert_eq!(on, json!(true), "clicking {label} sets the style of {cell}");
        }
        run(&mut h, "selection.set", json!({"cell": "A1"}));
        h.run_steps(2);
        let on = run(&mut h, "cell.get", json!({"cell": "A1"}))["style"]["font"][attr].clone();
        assert_eq!(on, json!(true), "clicking {label} sets the style");
        assert_eq!(toggled(&h, label), Some(Toggled::True), "{label} reports pressed");
        assert!(paints_pressed(&h, label), "{label} draws as pressed");

        click(&mut h, label);
        let off = run(&mut h, "cell.get", json!({"cell": "A1"}))["style"]["font"][attr].clone();
        assert_eq!(off, json!(false), "a second click on {label} clears it");
        assert!(!paints_pressed(&h, label), "{label} stops drawing as pressed");
    }
}

#[test]
fn underline_click_toggles_the_single_line_and_draws_pressed() {
    let mut h = harness();
    assert_eq!(style(&h).font.underline, Underline::None);
    assert!(!paints_pressed(&h, "Underline (Ctrl+U)"));

    click(&mut h, "Underline (Ctrl+U)");
    assert_eq!(style(&h).font.underline, Underline::Single, "the main part underlines the cell");
    assert_eq!(toggled(&h, "Underline (Ctrl+U)"), Some(Toggled::True));
    assert!(paints_pressed(&h, "Underline (Ctrl+U)"), "an underlined cell shows the button pressed");

    click(&mut h, "Underline (Ctrl+U)");
    assert_eq!(style(&h).font.underline, Underline::None, "and a second click takes it off");
}

#[test]
fn the_underline_dropdown_sets_single_and_double() {
    let mut h = harness();
    click_arrow(&mut h, "Underline (Ctrl+U)");
    h.get_by_label("Underline").click();
    h.run_steps(2);
    assert_eq!(style(&h).font.underline, Underline::Single, "the menu's Underline is the single line");

    click_arrow(&mut h, "Underline (Ctrl+U)");
    h.get_by_label("Double Underline").click();
    h.run_steps(2);
    assert_eq!(style(&h).font.underline, Underline::Double, "Double Underline is a double line");
}

#[test]
fn the_font_and_size_boxes_show_the_cell_and_set_what_is_picked() {
    let mut h = harness();
    let started = style(&h).font.name.clone();
    // The name box reads the selection's font.
    h.get_by_label(&started);
    h.get_by_label(&started).click();
    h.run_steps(2);
    h.get_by_label("Cambria").click();
    h.run_steps(2);
    assert_eq!(style(&h).font.name, "Cambria", "picking a font from the list sets it on the selection");
    // ... and goes on showing it, now from the cell.
    h.get_by_label("Cambria");

    // A font further down the list: the filter field narrows the rows (Georgia is past the nine
    // the popup shows without scrolling).
    h.get_by_label("Cambria").click();
    h.run_steps(2);
    h.input_mut().events.push(Event::Text("geo".into()));
    h.run_steps(2);
    h.get_by_label("Georgia").click();
    h.run_steps(2);
    assert_eq!(style(&h).font.name, "Georgia", "a filtered pick sets it too");
    h.get_by_label("Georgia");

    // The size box reads the cell's size and its arrow lists the standard sizes.
    assert!(h.get_all_by_role(Role::TextInput).any(|n| n.value().as_deref() == Some("11")), "the size box shows 11");
    h.get_by_label("Font Size").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, "16").click();
    h.run_steps(3);
    assert_eq!(style(&h).font.size, 16.0, "picking a size sets it on the selection");
    assert!(h.get_all_by_role(Role::TextInput).any(|n| n.value().as_deref() == Some("16")), "the box shows the new size");
}

#[test]
fn the_font_size_steppers_step_through_the_standard_sizes() {
    let mut h = harness();
    click(&mut h, "Increase Font Size");
    assert_eq!(style(&h).font.size, 12.0, "11 steps up to 12");
    click(&mut h, "Decrease Font Size");
    click(&mut h, "Decrease Font Size");
    assert_eq!(style(&h).font.size, 10.0, "and down past 11 to 10");
}

#[test]
fn the_colour_buttons_apply_the_last_colour_and_a_picked_one() {
    let mut h = harness();
    // The main halves apply the colour the button remembers, set by the last pick.
    click(&mut h, "Font Color");
    assert_eq!(style(&h).font.color, Color::Rgb(0xC00000), "the font colour button's default red");
    assert!(
        painted_text_colours(&h, "A1").contains(&egui::Color32::from_rgb(0xC0, 0x00, 0x00)),
        "the grid paints the text in that colour, not just the model"
    );

    click(&mut h, "Fill Color");
    assert_eq!(style(&h).fill, Fill::solid(Color::Rgb(0xFFFF00)), "the fill button's default yellow");

    // A swatch from the dropdown: the first Standard Colors swatch is #C00000.
    click_arrow(&mut h, "Fill Color");
    h.run_steps(2);
    let label = h.get_by_label("Standard Colors").rect();
    click_at(&mut h, pos2(label.left() + 8.0, label.bottom() + 9.0));
    assert_eq!(style(&h).fill, Fill::solid(Color::Rgb(0xC00000)), "picking a swatch fills the cell with it");
    assert_eq!(h.state().grid.last_fill, "#C00000", "and the button remembers it for its main half");
    click(&mut h, "Fill Color");
    assert_eq!(style(&h).fill, Fill::solid(Color::Rgb(0xC00000)), "the main half re-applies the last pick");

    // The font colour's own dropdown: the second Standard Colors swatch is #FF0000.
    click_arrow(&mut h, "Font Color");
    h.run_steps(2);
    let label = h.get_by_label("Standard Colors").rect();
    click_at(&mut h, pos2(label.left() + 26.0, label.bottom() + 9.0));
    assert_eq!(style(&h).font.color, Color::Rgb(0xFF0000), "a font colour swatch sets the text colour");
    assert_eq!(h.state().grid.last_font_color, "#FF0000", "and the button remembers it");
    click(&mut h, "Font Color");
    assert_eq!(style(&h).font.color, Color::Rgb(0xFF0000), "the main half re-applies the last pick");
}

#[test]
fn the_borders_button_applies_its_last_preset_and_the_menu_sets_each() {
    let mut h = harness();
    click(&mut h, "Borders");
    let b = style(&h).borders;
    assert_eq!(
        (b.bottom.style, b.top.style, b.left.style, b.right.style),
        (BorderStyle::Thin, BorderStyle::None, BorderStyle::None, BorderStyle::None)
    );
    assert_eq!(h.state().grid.last_border, "bottom");

    // Every preset in the dropdown, on a single cell (all four of its sides are on the edge).
    let (thin, thick, double) = (line(), line_of(BorderStyle::Thick), line_of(BorderStyle::Double));
    let one = |top: Option<BorderLine>, bottom: Option<BorderLine>, left: Option<BorderLine>, right: Option<BorderLine>| Borders {
        top: top.unwrap_or_default(),
        bottom: bottom.unwrap_or_default(),
        left: left.unwrap_or_default(),
        right: right.unwrap_or_default(),
        ..Borders::default()
    };
    for (label, want) in [
        ("Bottom Border", one(None, Some(thin), None, None)),
        ("Top Border", one(Some(thin), None, None, None)),
        ("Left Border", one(None, None, Some(thin), None)),
        ("Right Border", one(None, None, None, Some(thin))),
        ("No Border", Borders::default()),
        ("All Borders", one(Some(thin), Some(thin), Some(thin), Some(thin))),
        ("Outside Borders", one(Some(thin), Some(thin), Some(thin), Some(thin))),
        ("Thick Outside Borders", one(Some(thick), Some(thick), Some(thick), Some(thick))),
        ("Bottom Double Border", one(None, Some(double), None, None)),
        ("Thick Bottom Border", one(None, Some(thick), None, None)),
        ("Top and Bottom Border", one(Some(thin), Some(thin), None, None)),
        ("Top and Thick Bottom Border", one(Some(thin), Some(thick), None, None)),
        ("Top and Double Bottom Border", one(Some(thin), Some(double), None, None)),
    ] {
        let mut fresh = harness();
        click_arrow(&mut fresh, "Borders");
        fresh.run_steps(2);
        fresh.get_by_label(label).click();
        fresh.run_steps(2);
        assert_eq!(style(&fresh).borders, want, "{label} sets its sides");
    }

    // Inside Borders draws only the edges between cells, so it needs a range: on A1:B2 that is the
    // two cells' facing sides.
    let mut fresh = harness();
    fresh.state_mut().session.run("selection.set", json!({"range": "A1:B2"})).unwrap();
    fresh.run_steps(2);
    menu_item(&mut fresh, "Borders", "Inside Borders");
    assert_eq!(style_of(&fresh, "A1").borders, one(None, Some(thin), None, Some(thin)), "A1 keeps its inside edges");
    assert_eq!(style_of(&fresh, "B2").borders, one(Some(thin), None, Some(thin), None), "B2 keeps its inside edges");

    // The main half re-applies whatever the menu last set.
    menu_item(&mut h, "Borders", "All Borders");
    run(&mut h, "edit.clearFormats", json!({}));
    assert_eq!(style(&h).borders, Borders::default(), "cleared");
    click(&mut h, "Borders");
    assert_eq!(style(&h).borders.top.style, BorderStyle::Thin, "the main half applies the last preset from the menu");
    assert_eq!(h.state().grid.last_border, "all");
}

/// Shift+F10 on a cell opens its context menu; Format Cells… opens the dialog, then its Font tab.
fn open_format_cells(h: &mut Harness) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key { key: egui::Key::F10, physical_key: None, pressed, repeat: false, modifiers: Modifiers::SHIFT });
    }
    h.run_steps(3);
    h.get_by_label("Format Cells…").click();
    h.run_steps(3);
    h.get_by_label("Font").click();
    h.run_steps(3);
}

#[test]
fn the_format_cells_font_tab_shows_the_cells_style_and_applies_what_is_ticked() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1]]})).unwrap();
    s.execute("selection.set", json!({"cell": "A1"})).unwrap();
    s.execute("home.bold", json!({"on": true})).unwrap();
    s.execute("home.fontName", json!({"name": "Georgia"})).unwrap();
    s.execute("home.fontSize", json!({"size": 14})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
    );
    h.run_steps(4);
    open_format_cells(&mut h);

    assert_eq!(toggled(&h, "Bold"), Some(Toggled::True), "a bold cell opens with Bold ticked");
    assert_eq!(toggled(&h, "Italic"), Some(Toggled::False));
    // The tab opens on the cell's own font and size.
    let font_box = h.get_by_role_and_label(Role::ComboBox, "Font");
    assert_eq!(font_box.value().as_deref(), Some("Georgia"), "the Font box shows the cell's font");
    let size_box = h.get_by_role_and_label(Role::SpinButton, "Size");
    assert_eq!(size_box.value().as_deref(), Some("14.0"), "the Size box shows the cell's size");

    // A font picked in the dialog is applied by OK.
    h.get_by_role_and_label(Role::ComboBox, "Font").click();
    h.run_steps(3);
    h.get_by_label("Cambria").click();
    h.run_steps(3);

    // Turn on italic and strikethrough, pick the single underline, and raise the size.
    h.get_by_role_and_label(Role::CheckBox, "Italic").click();
    h.get_by_role_and_label(Role::CheckBox, "Strikethrough").click();
    h.get_by_role_and_label(Role::RadioButton, "Single").click();
    h.run_steps(3);
    let track = h.get_all_by_role(Role::Slider).map(|n| n.rect()).find(|r| r.center().y < 700.0).expect("the Font tab's size slider");
    click_at(&mut h, pos2(track.left() + track.width() * 0.75, track.center().y));
    let draft: Style = serde_json::from_value(h.state().dialog.as_ref().unwrap().values["style"].clone()).unwrap();
    assert!(draft.font.size > 14.0, "clicking further along the slider grows the size: {}", draft.font.size);

    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    let got = style(&h);
    assert!(h.state().dialog.is_none(), "OK closes the dialog");
    assert_eq!((got.font.bold, got.font.italic, got.font.strike), (true, true, true), "the ticks reach the cell");
    assert_eq!(got.font.underline, Underline::Single);
    assert_eq!(got.font.name, "Cambria", "the font picked in the dialog is applied");
    assert_eq!(got.font.size, draft.font.size, "and the size the slider was left on");
}
