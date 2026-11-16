//! Formatting controls behave the way Excel's do when a cell already carries a variant, and a
//! button that names one value is a choice rather than a toggle.

use gridcraft_engine::Session;
use gridcraft_engine::model::{HAlign, Underline};
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "x"})).unwrap();
    s.execute("selection.set", json!({"cell": "A1"})).unwrap();
    s
}

fn style(s: &Session) -> gridcraft_engine::model::Style {
    let d = s.doc().expect("doc");
    let sh = d.wb.active().expect("sheet");
    d.wb.styles.get(sh.style_id(d.selection.active)).clone()
}

#[test]
fn a_double_underline_is_turned_off_by_the_underline_button() {
    let mut s = session();
    s.execute("home.underline", json!({"style": "double"})).unwrap();
    assert_eq!(style(&s).font.underline, Underline::Double);
    // One click, not two: Excel has no "single underline means off" state.
    let r = s.execute("home.underline", json!({})).unwrap();
    assert_eq!(r["on"], false);
    assert_eq!(style(&s).font.underline, Underline::None, "the button turned it off");
    // And a third click puts a single underline back on.
    let r = s.execute("home.underline", json!({})).unwrap();
    assert_eq!(r["on"], true);
    assert_eq!(style(&s).font.underline, Underline::Single);
}

#[test]
fn naming_a_style_sets_it_even_when_the_cell_has_another() {
    // The menu's "Double Underline" is a choice: it must not toggle a single line off.
    let mut s = session();
    s.execute("home.underline", json!({"style": "single"})).unwrap();
    s.execute("home.underline", json!({"style": "double"})).unwrap();
    assert_eq!(style(&s).font.underline, Underline::Double);
}

#[test]
fn clicking_the_alignment_a_cell_already_has_leaves_it_alone() {
    let mut s = session();
    s.execute("home.alignCenter", json!({})).unwrap();
    assert_eq!(style(&s).align.h, HAlign::Center);
    // The same button again: still centred (Excel's behaviour), not back to General.
    s.execute("home.alignCenter", json!({})).unwrap();
    assert_eq!(style(&s).align.h, HAlign::Center, "a second click keeps the alignment");
    // Another one still switches.
    s.execute("home.alignRight", json!({})).unwrap();
    assert_eq!(style(&s).align.h, HAlign::Right);
}
