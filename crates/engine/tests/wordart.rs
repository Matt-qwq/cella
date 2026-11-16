//! Insert › Text › WordArt: a floating decorative-text shape, edited by its own command and kept
//! as a WordArt (not flattened into a text box) through an XLSX save and open.

use gridcraft_engine::Session;
use gridcraft_engine::cmd::wordart::{DEFAULT_TEXT, MAX_CHARS, STYLES};
use gridcraft_engine::model::{Color, Shape, ShapeKind};
use serde_json::json;

fn workbook() -> Session {
    let mut session = Session::new();
    session.new_workbook();
    session
}

fn reopen(session: &mut Session) -> Session {
    let saved = session.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.open", json!({"name": "wordart.xlsx", "base64": saved["base64"]})).unwrap();
    reopened
}

fn shapes(session: &Session) -> &[Shape] {
    &session.doc().unwrap().wb.active().unwrap().shapes
}

fn word_art(session: &Session) -> &Shape {
    shapes(session).iter().find(|s| s.kind == ShapeKind::WordArt).expect("a WordArt shape")
}

#[test]
fn insert_with_empty_params_gives_default_text_and_a_sane_size() {
    let mut s = workbook();
    s.execute("insert.wordArt", json!({})).unwrap();
    let shape = word_art(&s);
    assert_eq!(shape.text, DEFAULT_TEXT);
    // The default preset: theme accent, bold, 36 pt.
    assert_eq!(shape.format.font_size, Some(36.0));
    assert!(shape.format.bold && !shape.format.italic);
    assert_eq!(shape.format.text_color, Some(STYLES[0].fill));
    assert_eq!(shape.format.align, Some(gridcraft_engine::model::ShapeTextAlign::Center));
    assert!(shape.format.no_line, "a WordArt has no box of its own");
    // Long enough for the text at that size, and never degenerate or unbounded.
    assert!(shape.anchor.width > 100.0 && shape.anchor.width < 4000.0, "width {}", shape.anchor.width);
    assert!(shape.anchor.height > 40.0 && shape.anchor.height < 4000.0, "height {}", shape.anchor.height);
}

#[test]
fn insert_takes_a_style_a_text_and_a_place() {
    let mut s = workbook();
    s.execute("insert.wordArt", json!({"style": "Ember", "text": "Sale", "at": "C3", "width": 200, "height": 80})).unwrap();
    let shape = word_art(&s);
    assert_eq!(shape.text, "Sale");
    assert_eq!(shape.anchor.cell, gridcraft_core::CellRef::parse("C3").unwrap());
    assert_eq!((shape.anchor.width, shape.anchor.height), (200.0, 80.0));
    let ember = STYLES.iter().find(|st| st.name == "Ember").unwrap();
    assert_eq!(shape.format.font_size, Some(ember.size));
    assert_eq!(shape.format.text_shadow, ember.shadow);
    // Hostile and unknown params are refused with a clear error, never a dialog or a panic.
    assert!(s.execute("insert.wordArt", json!({"style": "no such style"})).is_err());
    assert!(s.execute("insert.wordArt", json!({"style": 99})).is_err());
    assert!(s.execute("insert.wordArt", json!({"text": ""})).is_err());
    assert!(s.execute("insert.wordArt", json!({"text": "x".repeat(MAX_CHARS + 1)})).is_err());
    // A huge font size is capped rather than trusted.
    s.execute("insert.wordArt", json!({"text": "Big", "fontSize": 100000})).unwrap();
    let big = shapes(&s).iter().rfind(|x| x.kind == ShapeKind::WordArt).unwrap();
    assert_eq!(big.format.font_size, Some(400.0));
}

#[test]
fn set_changes_the_text_and_the_style_with_undo_and_redo() {
    let mut s = workbook();
    let id = s.execute("insert.wordArt", json!({})).unwrap()["shape"].as_u64().unwrap() as u32;
    s.execute("wordArt.set", json!({"id": id, "text": "Revenue", "fill": "#123456", "fontSize": 54, "italic": true, "shadow": false})).unwrap();
    let shape = word_art(&s);
    assert_eq!(shape.text, "Revenue");
    assert_eq!(shape.format.text_color, Some(Color::Rgb(0x123456)));
    assert_eq!(shape.format.font_size, Some(54.0));
    assert!(shape.format.italic);
    assert_eq!(shape.format.text_shadow, None, "an explicit false clears the preset's shadow");

    s.execute("edit.undo", json!({})).unwrap();
    let shape = word_art(&s);
    assert_eq!(shape.text, DEFAULT_TEXT, "undo puts the text back");
    assert_eq!(shape.format.font_size, Some(36.0), "and the style with it");
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(word_art(&s).text, "Revenue");
    assert_eq!(word_art(&s).format.font_size, Some(54.0));

    // Another shape's id is refused by name, and a text box is not WordArt.
    assert!(s.execute("wordArt.set", json!({"id": 9999, "text": "x"})).is_err());
    s.execute("insert.textBox", json!({"text": "box"})).unwrap();
    let box_id = shapes(&s).iter().find(|x| x.kind == ShapeKind::TextBox).unwrap().id;
    let err = s.execute("wordArt.set", json!({"id": box_id, "text": "x"})).unwrap_err().to_string();
    assert!(err.contains("not WordArt"), "{err}");
    // The text box's own path edits a WordArt too (the user edits it in place).
    s.execute("shape.setText", json!({"id": id, "text": "Edited"})).unwrap();
    assert_eq!(word_art(&s).text, "Edited");
    assert!(s.execute("shape.setText", json!({"id": id, "text": ""})).is_err(), "a WordArt keeps some text");
}

#[test]
fn every_wordart_style_is_a_valid_preset() {
    for (i, style) in STYLES.iter().enumerate() {
        let mut s = workbook();
        s.execute("insert.wordArt", json!({"style": i})).unwrap();
        let shape = word_art(&s);
        assert_eq!(shape.format.text_color, Some(style.fill), "{}", style.name);
        assert_eq!(shape.format.text_outline, style.outline, "{}", style.name);
        assert_eq!(shape.format.text_shadow, style.shadow, "{}", style.name);
        assert_eq!(shape.format.font_size, Some(style.size), "{}", style.name);
        // And by name, in any case.
        let mut s = workbook();
        s.execute("insert.wordArt", json!({"style": style.name.to_lowercase()})).unwrap();
        assert_eq!(word_art(&s).format.font_size, Some(style.size));
    }
}

#[test]
fn the_commands_are_registered_with_a_ribbon_path() {
    let s = workbook();
    let commands = s.commands();
    let insert = commands.iter().find(|c| c.id == "insert.wordArt").expect("insert.wordArt is registered");
    assert_eq!(insert.label, "WordArt");
    assert_eq!(insert.menu, vec!["Insert", "Text"]);
    assert!(insert.enabled);
    let set = commands.iter().find(|c| c.id == "wordArt.set").expect("wordArt.set is registered");
    assert_eq!(set.label, "Format WordArt");
    assert!(!set.menu.is_empty(), "a format command still says where it lives");
    assert!(set.enabled);
    // Both are reachable through the same registry the control channel, CLI and MCP use.
    assert_eq!(gridcraft_engine::find_command("insert.wordArt").map(|c| c.id), Some("insert.wordArt"));
    assert_eq!(gridcraft_engine::find_command("wordArt.set").map(|c| c.id), Some("wordArt.set"));
}

#[test]
fn a_wordart_keeps_its_kind_text_and_style_through_two_xlsx_saves() {
    let mut s = workbook();
    s.execute("insert.wordArt", json!({"style": "Ink Stamp", "text": "GridCraft\nWordArt", "at": "B4"})).unwrap();
    let id = word_art(&s).id;
    s.execute("wordArt.set", json!({"id": id, "outline": "#00FF00", "outlineWidth": 2.5})).unwrap();
    let expected = word_art(&s).clone();
    let mut want = expected.text.clone();
    for pass in 0..2 {
        s = reopen(&mut s);
        let actual = word_art(&s);
        assert_eq!(actual.kind, ShapeKind::WordArt, "still a WordArt after pass {pass}");
        assert_eq!(shapes(&s).len(), 1, "and not flattened into a second shape");
        assert_eq!(actual.text, want, "the text (and its line break) comes back");
        assert_eq!(actual.format.font_size, expected.format.font_size);
        assert_eq!(actual.format.bold, expected.format.bold);
        assert_eq!(actual.format.italic, expected.format.italic);
        assert_eq!(actual.format.text_color, expected.format.text_color);
        assert_eq!(actual.format.text_shadow, expected.format.text_shadow);
        assert_eq!(actual.format.text_outline, Some((Color::Rgb(0x00FF00), 2.5)));
        assert_eq!(actual.anchor.cell, expected.anchor.cell);
        assert!((actual.anchor.width - expected.anchor.width).abs() < 0.01);
        // Still editable as a WordArt after the round trip.
        s.execute("wordArt.set", json!({"id": actual.id, "text": "Again"})).unwrap();
        assert_eq!(word_art(&s).text, "Again");
        want = "Again".into();
    }
}
