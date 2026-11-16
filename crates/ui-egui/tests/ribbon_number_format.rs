//! Home Number group, clicked as real widgets: Accounting/Currency, Percent Style, Comma Style,
//! Increase/Decrease Decimal, the number-format dropdown's entries, and Format Cells Number tab.
//! Each test asserts the stored format code and the text the grid draws for it.

use egui::{Shape, accesskit::Role};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::Style;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(input: &str) -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": input})).unwrap();
    s.execute("selection.set", json!({"range": "A1"})).unwrap();
    // Wide enough for the widest text these assertions read (123450.00% must not turn into ####).
    s.execute("home.columnWidth", json!({"cols": "A", "width": 160})).unwrap();
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

fn code(h: &Harness) -> String {
    style(h, "A1").num_fmt.as_str().to_string()
}

/// A named ribbon button, clicked through the widget's own pointer events.
fn click(h: &mut Harness, label: &str) {
    h.get_by_role_and_label(Role::Button, label).click();
    h.run_steps(3);
}

/// The main half of a split button (the two halves share the control's name; the main one is left).
fn click_split_main(h: &mut Harness, label: &str) {
    let node = h.get_all_by_role_and_label(Role::Button, label).min_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x));
    node.expect("a split button with a main half").click();
    h.run_steps(3);
}

/// The arrow half of a split button, which opens its menu.
fn click_split_arrow(h: &mut Harness, label: &str) {
    let node = h.get_all_by_role_and_label(Role::Button, label).max_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x));
    node.expect("a split button with an arrow").click();
    h.run_steps(4);
}

/// The texts the grid paints in its body (not the formula bar above it).
fn painted(h: &Harness) -> Vec<String> {
    let Some(grid) = h.state().grid.rect else { return Vec::new() };
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Text(t) if t.pos.y >= grid.top() => Some(t.galley.job.text.clone()),
            _ => None,
        })
        .collect()
}

/// Whether the grid draws this text. Accounting and comma formats pad with space runs (the "*" fill
/// character and the "_" skip), so the comparison collapses whitespace instead of trimming only.
fn shows(h: &Harness, text: &str) -> bool {
    let flat = |t: &str| t.split_whitespace().collect::<Vec<_>>().join(" ");
    painted(h).iter().any(|t| flat(t) == text)
}

#[test]
fn percent_and_comma_style_reach_the_engine_and_are_drawn() {
    let mut h = harness("0.5");
    click(&mut h, "Percent Style");
    assert_eq!(code(&h), "0%", "Percent Style sets 0%%");
    assert!(shows(&h, "50%"), "the cell shows 50%: {:?}", painted(&h));

    let mut h = harness("1234.5");
    click(&mut h, "Comma Style");
    let comma = gridcraft_engine::cmd::format::format_code_for("Comma");
    assert_eq!(code(&h), comma, "Comma Style sets the comma format code");
    assert!(shows(&h, "1,234.50"), "the cell shows 1,234.50: {:?}", painted(&h));
}

#[test]
fn increase_and_decrease_decimal_change_the_code_and_the_drawn_text() {
    let mut h = harness("1234.5");
    click(&mut h, "Comma Style");
    assert!(shows(&h, "1,234.50"));

    click(&mut h, "Increase Decimal");
    assert!(code(&h).contains("0.000"), "one more decimal place: {}", code(&h));
    assert!(shows(&h, "1,234.500"), "the cell follows: {:?}", painted(&h));

    click(&mut h, "Decrease Decimal");
    assert!(shows(&h, "1,234.50"), "one decimal back: {:?}", painted(&h));
    click(&mut h, "Decrease Decimal");
    assert!(shows(&h, "1,234.5"), "and one more: {:?}", painted(&h));
    click(&mut h, "Decrease Decimal");
    assert!(shows(&h, "1,235"), "a zero-decimal format rounds 1,234.5 to 1,235: {:?}", painted(&h));
}

#[test]
fn the_currency_button_applies_accounting_and_the_arrow_lists_the_symbols() {
    let mut h = harness("1234.5");
    click_split_main(&mut h, "Accounting Number Format");
    assert!(code(&h).contains("$"), "the $(USD) accounting code: {}", code(&h));
    assert!(shows(&h, "$ 1,234.50"), "the cell shows the dollar and the number: {:?}", painted(&h));

    // The arrow's menu: the Chinese (PRC) yuan entry applies its own code.
    let mut h = harness("1234.5");
    click_split_arrow(&mut h, "Accounting Number Format");
    h.get_by_role_and_label(Role::Button, "¥ Chinese (PRC)").click();
    h.run_steps(3);
    assert!(code(&h).contains("[$¥-804]"), "the yuan accounting code: {}", code(&h));
    assert!(shows(&h, "¥ 1,234.50"), "the cell shows the yuan and the number: {:?}", painted(&h));
}

/// Opens the number-format dropdown and types into its filter, so a row below the list's nine-row
/// fold (Text, More Number Formats…) is on screen and clickable, as it is for a user who scrolls.
fn open_dropdown(h: &mut Harness, filter: &str) {
    // The combo's own button is labelled with the current format's kind ("General" at first).
    h.get_by_role_and_label(Role::Button, "General").click();
    h.run_steps(6);
    h.input_mut().events.push(egui::Event::Text(filter.into()));
    h.run_steps(3);
}

/// The dropdown row whose label contains this text (rows read "<name><padding> <preview>").
fn dropdown_row<'a>(h: &'a Harness, contains: &'a str) -> egui_kittest::Node<'a> {
    h.get_all_by_label_contains(contains).find(|n| n.rect().min.y > 110.0).unwrap_or_else(|| panic!("a dropdown row containing {contains}"))
}

#[test]
fn the_number_format_dropdown_entries_apply_their_format_code() {
    // entry, format code, and the text the grid must then draw (None: a date, whose day number
    // depends on the epoch and is not the point here).
    let cases = [
        ("Currency", "\"$\"#,##0.00", Some("$1,234.50")),
        ("Percentage", "0.00%", Some("123450.00%")),
        ("Short Date", "m/d/yyyy", None),
        ("Text", "@", Some("1234.5")),
    ];
    for (entry, want, text) in cases {
        let mut h = harness("1234.5");
        open_dropdown(&mut h, entry);
        dropdown_row(&h, entry).click();
        h.run_steps(4);
        assert_eq!(code(&h), want, "clicking {entry} applies its code");
        if let Some(text) = text {
            assert!(shows(&h, text), "clicking {entry} changes what the cell shows to {text}: {:?}", painted(&h));
        }
        // The combo's face names the format's kind, which is the entry's name unless the kind
        // groups several entries ("m/d/yyyy" is the Date kind, shown as Date).
        let face = format!("{:?}", gridcraft_engine::display::number_format(want).kind());
        assert!(h.query_by_role_and_label(Role::Button, &face).is_some(), "the combo shows the kind {face} afterwards");
    }
}

#[test]
fn format_cells_number_tab_applies_the_chosen_category() {
    let mut h = harness("1234.5");
    // The dropdown's last entry opens Format Cells on its Number tab.
    open_dropdown(&mut h, "more");
    dropdown_row(&h, "More Number Formats").click();
    h.run_steps(4);
    assert_eq!(
        h.state().dialog.as_ref().map(|d| d.tab.clone()).as_deref(),
        Some("Number"),
        "the dialog opens on the Number tab; dialog now {:?}",
        h.state().dialog.as_ref().map(|d| (d.name.clone(), d.tab.clone()))
    );

    h.get_by_role_and_label(Role::Button, "Percentage").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, "   OK   ").click();
    h.run_steps(4);
    assert_eq!(code(&h), "0.00%", "OK applies the Percentage category");
    assert!(shows(&h, "123450.00%"), "the cell shows 1234.5 as 123450.00 percent: {:?}", painted(&h));
}
