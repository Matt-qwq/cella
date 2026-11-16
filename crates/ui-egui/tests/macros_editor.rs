//! View ▸ Macros: the dialog lists the workbook's scripts and, for the selected one, its steps —
//! each row with its command id, its params as JSON text and a ✕, plus a row that adds one. What
//! this proves is that the rows are the workbook's own steps (`automate.scriptSteps`) and that
//! changing one in the dialog runs the engine command behind it.

use egui::{Event, Key, Modifiers, accesskit::Role, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::control::{Outcome, handle};
use serde_json::{Value as Json, json};

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(session: Session) -> Harness {
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn control(h: &mut Harness, method: &str, params: Json) -> Json {
    let ctx = h.ctx.clone();
    match handle(h.state_mut(), &ctx, method, params) {
        Outcome::Done(v) => v,
        Outcome::Screenshot { .. } => json!(null),
    }
}

/// A workbook with one script, `Build`, holding two steps.
fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "automate.newScript",
        json!({"name": "Build", "commands": [
            {"command": "cell.set", "params": {"cell": "B2", "input": "5"}},
            {"command": "home.bold", "params": {"range": "A1:A2"}},
        ]}),
    )
    .unwrap();
    s
}

fn steps(h: &mut Harness) -> Json {
    h.state_mut().session.run("automate.scriptSteps", json!({"name": "Build"})).unwrap_or(Json::Null)
}

fn open_macros(h: &mut Harness) {
    control(h, "ui.dialog", json!({"name": "macros"}));
    h.run_steps(4);
    assert!(h.state().dialog.is_some(), "the Macros dialog opens");
}

/// The text field whose contents are `text` (a step's params line, say).
fn params_field(h: &Harness, text: &str) -> egui::Rect {
    h.get_all_by_role(Role::TextInput).find(|n| n.value().as_deref() == Some(text)).unwrap_or_else(|| panic!("no field holding {text:?}")).rect()
}

/// Clicks a text field, replaces what it holds with `text` and presses Enter.
fn retype(h: &mut Harness, field: egui::Rect, text: &str) {
    h.input_mut().events.push(Event::PointerMoved(field.center()));
    h.run_steps(1);
    h.get_all_by_role(Role::TextInput).find(|n| n.rect() == field).expect("the field").click();
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    for c in text.chars() {
        h.input_mut().events.push(Event::Text(c.to_string()));
        h.run_steps(2);
    }
    // Leave the field by clicking a label, which is what commits it. (Enter commits too, but a
    // message box opens on the same keystroke and closes on it.)
    let away = h.get_by_label("Steps").rect().center();
    h.input_mut().events.push(Event::PointerMoved(away));
    h.run_steps(1);
    h.get_by_label("Steps").click();
    h.run_steps(3);
}

#[test]
fn the_macros_dialog_shows_a_scripts_steps_and_their_params() {
    let mut h = harness(session());
    open_macros(&mut h);
    // The script is the one selected when the dialog opens, and each step is a row of its own.
    assert!(h.query_by_label("Build").is_some(), "the script is listed");
    for row in ["1. cell.set", "2. home.bold"] {
        assert!(h.query_by_label(row).is_some(), "the step row {row:?} is drawn");
    }
    // The params field of a row carries that step's params, as JSON text.
    assert!(
        h.get_all_by_role(Role::TextInput).any(|n| n.value().as_deref() == Some("{\"cell\":\"B2\",\"input\":\"5\"}")),
        "the first step's params are in a field"
    );
    assert!(
        h.get_all_by_role(Role::TextInput).any(|n| n.value().as_deref() == Some("{\"range\":\"A1:A2\"}")),
        "the second step's params are shown too"
    );
}

#[test]
fn retyping_a_steps_params_edits_the_step() {
    let mut h = harness(session());
    open_macros(&mut h);
    let params_line = params_field(&h, "{\"cell\":\"B2\",\"input\":\"5\"}");
    retype(&mut h, params_line, "{\"cell\":\"B2\",\"input\":\"9\"}");
    let now = steps(&mut h);
    assert_eq!(now[0]["params"], json!({"cell": "B2", "input": "9"}), "{now}");
    assert_eq!(now[1]["command"], "home.bold", "the other step is untouched: {now}");
    // What is not JSON is refused with a message, and the step keeps its params.
    let params_line = params_field(&h, "{\"cell\":\"B2\",\"input\":\"9\"}");
    retype(&mut h, params_line, "not json");
    assert!(h.state().message.is_some(), "a params line that isn't JSON is reported");
    let now = steps(&mut h);
    assert_eq!(now[0]["params"], json!({"cell": "B2", "input": "9"}), "{now}");
}

#[test]
fn the_cross_removes_a_step_and_the_add_row_appends_one() {
    let mut h = harness(session());
    open_macros(&mut h);
    // The ✕ of the first row drops that step.
    let cross = h.get_all_by_label("✕").next().expect("a ✕ per step").rect();
    h.input_mut().events.push(Event::PointerMoved(cross.center()));
    h.run_steps(1);
    h.get_all_by_label("✕").next().expect("a ✕ per step").click();
    h.run_steps(3);
    let now = steps(&mut h);
    assert_eq!(now, json!([{"command": "home.bold", "params": {"range": "A1:A2"}}]), "{now}");
    // The add row takes a command id and appends a step with no params.
    let add_field = h.get_all_by_role(Role::TextInput).last().expect("the add row's field").rect();
    retype(&mut h, add_field, "cell.set");
    h.get_by_label("Add Step").click();
    h.run_steps(3);
    let now = steps(&mut h);
    assert_eq!(now[1], json!({"command": "cell.set", "params": {}}), "{now}");
}
