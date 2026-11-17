//! Home ▸ Alignment, clicked as real widgets: the six alignment buttons, Wrap Text, Merge & Center
//! with its menu, and the two indent buttons. Every test asserts both halves of "the
//! button works": the engine's style/merge state and what the grid paints for it.

use cella_engine::Session;
use cella_engine::core::{CellRef, RangeRef, Value};
use cella_engine::model::{HAlign, Style, VAlign};
use cella_ui_egui::SheetApp;
use cella_ui_egui::grid::Geo;
use egui::{Rect, Shape, accesskit::Role};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// The default row height of a blank workbook, in pixels.
const DEFAULT_ROW_HEIGHT: f32 = 20.0;

fn row_height(h: &Harness, row: u32) -> f32 {
    h.state().session.active().unwrap().wb.active().unwrap().row_height(row)
}

/// A string far wider than the default column: it overflows on one line and wraps onto several.
const LONG: &str = "the quick brown fox jumps over the lazy dog";

fn harness(selection: &str) -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "abcdef"})).unwrap();
    s.execute("cell.set", json!({"cell": "B1", "input": "covered"})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "second"})).unwrap();
    s.execute("cell.set", json!({"cell": "A3", "input": LONG})).unwrap();
    // A tall first row so vertical alignment has somewhere to go.
    s.execute("home.rowHeight", json!({"rows": "1", "height": 60})).unwrap();
    s.execute("selection.set", json!({"range": selection})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
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

/// A named ribbon button, clicked through the widget's own pointer events.
fn click(h: &mut Harness, label: &str) {
    h.get_by_role_and_label(Role::Button, label).click();
    h.run_steps(3);
}

/// The main half of a split button (the two halves share the control's name; the main one is left).
fn click_split_main(h: &mut Harness, label: &str) {
    let node = h.get_all_by_role_and_label(Role::Button, label).min_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x));
    node.expect("a split button named {label}").click();
    h.run_steps(3);
}

/// The arrow half of a split button, which opens its menu.
fn click_split_arrow(h: &mut Harness, label: &str) {
    let node = h.get_all_by_role_and_label(Role::Button, label).max_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x));
    node.expect("a split arrow named {label}").click();
    h.run_steps(4);
}

/// The engine's rect for a range (the whole merge area when the range is merged).
fn cells(h: &Harness, range: &str) -> Rect {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    geo.range_rect(sheet, RangeRef::parse(range).unwrap())
}

/// How the grid paints a text in the grid body: (left, top, width, height, line count). Grid text
/// only: the formula bar above shows the active cell's input in shapes of its own.
fn painted(h: &Harness, text: &str) -> Option<(f32, f32, f32, f32, usize)> {
    let grid = h.state().grid.rect?;
    h.output().shapes.iter().find_map(|s| match &s.shape {
        Shape::Text(t) if t.galley.job.text == text && t.pos.y >= grid.top() => {
            let r = t.galley.rect.translate(t.pos.to_vec2());
            Some((r.left(), r.top(), r.width(), r.height(), t.galley.rows.len()))
        }
        _ => None,
    })
}

fn painted_anywhere(h: &Harness, text: &str) -> bool {
    h.output().shapes.iter().any(|s| matches!(&s.shape, Shape::Text(t) if t.galley.job.text == text))
}

/// Whether a toggle button reports itself pressed (Bold is "pressed", accesskit calls it toggled).
fn pressed(h: &Harness, label: &str) -> Option<bool> {
    use egui::accesskit::Toggled;
    let node = h.get_by_role_and_label(Role::Button, label);
    match node.accesskit_node().toggled() {
        Some(Toggled::True) => Some(true),
        Some(Toggled::False) => Some(false),
        _ => None,
    }
}

#[test]
fn the_three_horizontal_buttons_set_the_halign_and_move_the_text() {
    let mut h = harness("A1");
    let a1 = cells(&h, "A1");
    let (x, _, _, _, _) = painted(&h, "abcdef").expect("A1 is drawn");

    // General (the default) paints like Align Left, and no horizontal toggle is pressed.
    assert_eq!(style(&h, "A1").align.h, HAlign::General);
    assert!(x - a1.left() < 4.0, "General starts at the left edge, got {x} in {a1:?}");
    assert_eq!(pressed(&h, "Align Left"), Some(false), "General is neither Left nor Right");

    click(&mut h, "Align Right");
    assert_eq!(style(&h, "A1").align.h, HAlign::Right, "Align Right must reach the engine");
    assert_eq!(pressed(&h, "Align Right"), Some(true), "Align Right must show pressed");
    let (x, _, w, _, _) = painted(&h, "abcdef").expect("A1 is drawn");
    assert!(a1.right() - (x + w) < 4.0, "right-aligned text ends at the right edge: text {:?}, cell {a1:?}", (x, w));

    click(&mut h, "Center");
    assert_eq!(style(&h, "A1").align.h, HAlign::Center, "Center must reach the engine");
    assert_eq!(pressed(&h, "Center"), Some(true), "Center must show pressed");
    let (x, _, w, _, _) = painted(&h, "abcdef").expect("A1 is drawn");
    assert!(((x + w / 2.0) - a1.center().x).abs() < 4.0, "centred text is centred in the cell: text {:?}, cell {a1:?}", (x, w));

    click(&mut h, "Align Left");
    assert_eq!(style(&h, "A1").align.h, HAlign::Left, "Align Left must reach the engine");
    assert_eq!(pressed(&h, "Align Left"), Some(true), "Align Left must show pressed");
    let (x, _, _, _, _) = painted(&h, "abcdef").expect("A1 is drawn");
    assert!(x - a1.left() < 4.0, "left-aligned text starts at the left edge, got {x}");
}

#[test]
fn the_three_vertical_buttons_set_the_valign_and_move_the_text() {
    let mut h = harness("A1");
    let a1 = cells(&h, "A1");

    click(&mut h, "Top Align");
    assert_eq!(style(&h, "A1").align.v, VAlign::Top);
    assert_eq!(pressed(&h, "Top Align"), Some(true), "Top Align must show pressed");
    let (_, y, _, _, _) = painted(&h, "abcdef").expect("A1 is drawn");
    assert!(y - a1.top() < 4.0, "top-aligned text sits at the top: {y} in {a1:?}");

    click(&mut h, "Middle Align");
    assert_eq!(style(&h, "A1").align.v, VAlign::Center);
    assert_eq!(pressed(&h, "Middle Align"), Some(true), "Middle Align must show pressed");
    let (_, y, _, gh, _) = painted(&h, "abcdef").expect("A1 is drawn");
    assert!(((y + gh / 2.0) - a1.center().y).abs() < 4.0, "middle-aligned text is centred: {y} plus {gh} in {a1:?}");

    click(&mut h, "Bottom Align");
    assert_eq!(style(&h, "A1").align.v, VAlign::Bottom);
    assert_eq!(pressed(&h, "Bottom Align"), Some(true), "Bottom Align must show pressed");
    let (_, y, _, gh, _) = painted(&h, "abcdef").expect("A1 is drawn");
    assert!(a1.bottom() - (y + gh) < 4.0, "bottom-aligned text ends at the bottom: {y} plus {gh} in {a1:?}");
}

#[test]
fn wrap_text_breaks_the_cell_text_into_several_lines_and_off_draws_one() {
    // Wrap off (a file can store text that overflows): one line, wider than its column.
    let off = {
        let h = harness("A3");
        painted(&h, LONG).expect("A3 is drawn before any click")
    };
    assert_eq!(off.4, 1, "one line with Wrap Text off");

    let mut h = harness("A3");
    click(&mut h, "Wrap Text");
    assert!(style(&h, "A3").align.wrap, "Wrap Text must reach the engine");
    assert_eq!(pressed(&h, "Wrap Text"), Some(true), "Wrap Text must show pressed");
    let wrapped = painted(&h, LONG).expect("A3 is still drawn");
    assert!(wrapped.4 > 1, "wrapped text is drawn on several lines, got {}", wrapped.4);
    assert!(wrapped.3 > off.3, "the wrapped block is taller than the one-line block: {} vs {}", wrapped.3, off.3);
    // The row itself grows with the text (Excel auto-fits a row whose height was not set by hand):
    // without that the extra lines would be clipped to the default 20px row.
    let tall = row_height(&h, 2);
    assert!(tall > DEFAULT_ROW_HEIGHT, "the row auto-fits to the wrapped text: {tall} vs {DEFAULT_ROW_HEIGHT}");

    click(&mut h, "Wrap Text");
    assert!(!style(&h, "A3").align.wrap, "a second click turns Wrap Text off again");
    assert_eq!(pressed(&h, "Wrap Text"), Some(false));
    assert_eq!(painted(&h, LONG).map(|p| p.4), Some(1), "one line again");
    assert_eq!(row_height(&h, 2), DEFAULT_ROW_HEIGHT, "and the row drops back to the default height");
}

#[test]
fn merge_and_center_paints_one_centred_cell_and_unmerge_brings_the_covered_cell_back() {
    let mut h = harness("A1:B1");
    click_split_main(&mut h, "Merge & Center");
    let merged: Vec<String> = {
        let d = h.state().session.active().unwrap();
        let sheet = d.wb.active().unwrap();
        sheet.merges.iter().map(|m| m.a1()).collect()
    };
    assert_eq!(merged, vec!["A1:B1".to_string()], "Merge & Center merges the selection");

    // One cell: the anchor's text, centred over the whole merged area.
    let area = cells(&h, "A1:B1");
    let (x, _, w, _, _) = painted(&h, "abcdef").expect("the merged cell is drawn");
    assert!(((x + w / 2.0) - area.center().x).abs() < 6.0, "merged text is centred over A1:B1: text {:?}, area {area:?}", (x, w));
    // Merging keeps only the anchor's value (as Excel warns), so B1's text is gone, not painted.
    assert!(!painted_anywhere(&h, "covered"), "the covered cell's text is not painted separately");
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse("B1").unwrap()), Value::Empty);

    // Unmerge through the split button's menu: the text is one cell's again, centred in A1 only.
    click_split_arrow(&mut h, "Merge & Center");
    h.get_by_role_and_label(Role::Button, "Unmerge Cells").click();
    h.run_steps(3);
    assert!(h.state().session.active().unwrap().wb.active().unwrap().merges.is_empty(), "Unmerge Cells empties the merge list");
    let a1 = cells(&h, "A1");
    let (x, _, w, _, _) = painted(&h, "abcdef").expect("A1 is drawn");
    assert!(((x + w / 2.0) - a1.center().x).abs() < 6.0, "unmerged text is centred in A1 alone: text {:?}, cell {a1:?}", (x, w));
    assert!(((x + w / 2.0) - area.center().x).abs() > 6.0, "and no longer centred over A1:B1");
}

#[test]
fn the_merge_menu_merges_across_and_merges_cells() {
    let mut h = harness("A1:B2");
    click_split_arrow(&mut h, "Merge & Center");
    h.get_by_role_and_label(Role::Button, "Merge Across").click();
    h.run_steps(3);
    let mut across: Vec<String> = h.state().session.active().unwrap().wb.active().unwrap().merges.iter().map(|m| m.a1()).collect();
    across.sort();
    assert_eq!(across, vec!["A1:B1".to_string(), "A2:B2".to_string()], "Merge Across merges each row separately");

    let mut h = harness("A1:B2");
    click_split_arrow(&mut h, "Merge & Center");
    h.get_by_role_and_label(Role::Button, "Merge Cells").click();
    h.run_steps(3);
    let block: Vec<String> = h.state().session.active().unwrap().wb.active().unwrap().merges.iter().map(|m| m.a1()).collect();
    assert_eq!(block, vec!["A1:B2".to_string()], "Merge Cells merges the block");
}

#[test]
fn the_indent_buttons_move_the_text_nine_pixels_a_step_and_stop_at_zero() {
    let mut h = harness("A1");
    let start = painted(&h, "abcdef").expect("A1 is drawn").0;
    assert_eq!(style(&h, "A1").align.indent, 0);

    click(&mut h, "Increase Indent");
    assert_eq!(style(&h, "A1").align.indent, 1, "Increase Indent must reach the engine");
    let one = painted(&h, "abcdef").expect("A1 is drawn").0;
    assert!(one > start, "indented text starts further in: {one} vs {start}");

    click(&mut h, "Increase Indent");
    assert_eq!(style(&h, "A1").align.indent, 2);
    let two = painted(&h, "abcdef").expect("A1 is drawn").0;
    assert!(two > one, "each step moves the text in: {two} vs {one}");
    assert!(((two - one) - (one - start)).abs() < 1.0, "the steps are uniform: {} vs {}", two - one, one - start);

    click(&mut h, "Decrease Indent");
    click(&mut h, "Decrease Indent");
    assert_eq!(style(&h, "A1").align.indent, 0, "Decrease Indent returns to the margin");
    assert!((painted(&h, "abcdef").expect("A1 is drawn").0 - start).abs() < 0.01);

    // Zero is the floor: a further decrease neither panics nor goes below it.
    click(&mut h, "Decrease Indent");
    assert_eq!(style(&h, "A1").align.indent, 0, "indent never goes negative");
}
