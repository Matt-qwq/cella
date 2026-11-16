//! Formatting toggles behave the way Excel's do when a cell already carries a variant.

use gridcraft_engine::Session;
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "x"})).unwrap();
    s.execute("selection.set", json!({"cell": "A1"})).unwrap();
    s
}

fn underline_of(s: &Session) -> gridcraft_model::Underline {
    let d = s.doc().expect("doc");
    let sh = d.wb.active().expect("sheet");
    d.wb.styles.get(sh.style_id(d.selection.active)).font.underline
}

#[test]
fn a_double_underline_is_turned_off_by_the_underline_button() {
    let mut s = session();
    s.execute("home.underline", json!({"style": "double"})).unwrap();
    assert_eq!(underline_of(&s), gridcraft_model::Underline::Double);
    // One click, not two: Excel has no "single underline means off" state.
    let r = s.execute("home.underline", json!({})).unwrap();
    assert_eq!(r["on"], false);
    assert_eq!(underline_of(&s), gridcraft_model::Underline::None, "the button turned it off");
    // And a third click puts a single underline back on.
    let r = s.execute("home.underline", json!({})).unwrap();
    assert_eq!(r["on"], true);
    assert_eq!(underline_of(&s), gridcraft_model::Underline::Single);
}
