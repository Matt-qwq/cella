//! The Solver's Sensitivity report (adoption of upstream's), and Save/Load Model with Reset All.
//!
//! The LP is Hillier & Lieberman's Wyndor Glass problem - two products (doors, windows) made
//! under three plant capacities: max 3x + 5y with x <= 4, 2y <= 12 and 3x + 2y <= 18, whose
//! optimum is (2, 6) with profit 36. Its sensitivity analysis is textbook: shadow prices 0, 1.5
//! and 1 for the three plants, plant 1's hours ranging down by 2 and up without limit, plants 2
//! and 3 by 6 either way, the doors' profit over 0..7.5 and the windows' over 2 and up.

use cella_core::Value;
use cella_engine::Session;
use serde_json::{Value as Json, json};

fn run(s: &mut Session, command: &str, params: Json) -> Json {
    s.execute(command, params).unwrap()
}

/// The same length, each within tol (absolute plus a little relative slack).
fn near(got: &[f64], want: &[f64], tol: f64) -> bool {
    got.len() == want.len() && got.iter().zip(want).all(|(g, w)| (g - w).abs() <= tol * (1.0 + w.abs()))
}

fn close(got: &[f64], want: &[f64]) -> bool {
    got.len() == want.len() && got.iter().zip(want).all(|(g, w)| (g - w).abs() <= 1e-9 * (1.0 + w.abs()))
}

fn at(s: &Session, sheet: &str, cell: &str) -> Value {
    let d = s.doc().unwrap();
    let sh = d.wb.sheet_index(sheet).and_then(|i| d.wb.sheet(i)).unwrap();
    sh.value(cella_core::CellRef::parse(cell).unwrap())
}

/// The row of a report whose Cell column reads the given text.
fn row_of(s: &Session, sheet: &str, text: &str) -> u32 {
    let d = s.doc().unwrap();
    let sh = d.wb.sheet_index(sheet).and_then(|i| d.wb.sheet(i)).unwrap();
    let r = sh.used_range().unwrap();
    (r.start.row..=r.end.row)
        .find(|row| sh.value(cella_core::CellRef::new(*row, 1)).display() == text)
        .unwrap_or_else(|| panic!("no report row holds {text}"))
}

/// The numbers in cols of the row whose Cell column reads the given text.
fn report_row(s: &Session, sheet: &str, text: &str, cols: &[u32]) -> Vec<f64> {
    let r = row_of(s, sheet, text);
    cols.iter().map(|c| at(s, sheet, &format!("{}{}", cella_core::col_to_letters(*c), r + 1)).as_f64().unwrap_or(f64::NAN)).collect()
}

/// Column D on: the final value, the price or cost, the coefficient or right side, and the two
/// allowable changes.
const COLS: [u32; 5] = [3, 4, 5, 6, 7];

/// The Wyndor Glass model on Sheet1, its variable cells at zero.
fn wyndor() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    run(
        &mut s,
        "range.setValues",
        json!({"range": "A1", "values": [
            [null, "Doors", "Windows", "Used", "Available"],
            ["Batches", 0, 0, null, null],
            ["Plant 1", 1, 0, null, 4],
            ["Plant 2", 0, 2, null, 12],
            ["Plant 3", 3, 2, null, 18],
            ["Profit", 3, 5, null, null],
        ]}),
    );
    for r in 3..=6 {
        run(&mut s, "cell.set", json!({"cell": format!("D{r}"), "input": format!("=SUMPRODUCT(B{r}:C{r},$B$2:$C$2)")}));
    }
    s
}

/// data.solver params of the Wyndor Glass model.
fn wyndor_params() -> Json {
    json!({"objective": "D6", "variables": "B2:C2", "constraints": ["D3:D5 <= E3:E5"], "method": "simplex"})
}

#[test]
fn sensitivity_report_gives_the_wyndor_shadow_prices_and_ranges() {
    let mut s = wyndor();
    let mut params = wyndor_params();
    params["reports"] = json!(["sensitivity"]);
    let solved = run(&mut s, "data.solver", params);
    assert_eq!(solved["status"], "optimal", "{solved}");
    assert_eq!(solved["reports"], json!(["Sensitivity Report 1"]), "{solved}");
    let rep = "Sensitivity Report 1";
    assert_eq!(at(&s, rep, "A1"), Value::text("Cella Sensitivity Report"));
    // The report's shape: Excel's tables' rows under our headers.
    assert_eq!(at(&s, rep, "B5"), Value::text("Cell"));
    // Variable cells: final value, reduced cost, objective coefficient, allowable increase and
    // decrease. The doors' profit may rise to 7.5 and fall to 0; the windows' may fall by 3 and
    // rise without limit.
    assert!(close(&report_row(&s, rep, "$B$2", &COLS), &[2.0, 0.0, 3.0, 4.5, 3.0]), "{:?}", report_row(&s, rep, "$B$2", &COLS));
    assert!(close(&report_row(&s, rep, "$C$2", &COLS), &[6.0, 0.0, 5.0, 1e30, 3.0]), "{:?}", report_row(&s, rep, "$C$2", &COLS));
    // Constraints: final value, shadow price, right side, allowable increase and decrease.
    assert!(close(&report_row(&s, rep, "$D$3", &COLS), &[2.0, 0.0, 4.0, 1e30, 2.0]), "{:?}", report_row(&s, rep, "$D$3", &COLS));
    assert!(close(&report_row(&s, rep, "$D$4", &COLS), &[12.0, 1.5, 12.0, 6.0, 6.0]), "{:?}", report_row(&s, rep, "$D$4", &COLS));
    assert!(close(&report_row(&s, rep, "$D$5", &COLS), &[18.0, 1.0, 18.0, 6.0, 6.0]), "{:?}", report_row(&s, rep, "$D$5", &COLS));
    // data.solverReport writes the same report from the solve's own result.
    let again = run(&mut s, "data.solverReport", json!({"reports": ["sensitivity"], "model": solved["model"], "values": solved["values"]}));
    assert_eq!(again["sheets"], json!(["Sensitivity Report 2"]), "{again}");
}

#[test]
fn an_integer_model_has_no_sensitivity_report_ours_refuses_where_excel_omits_it() {
    // Excel's Solver Results doesn't offer a Sensitivity report once a model has integer or
    // binary constraints; the integer solution ours reports is a branch and bound's optimum, and
    // its duals mean nothing. So the report is refused, and skipNotOptimal doesn't hide it: only
    // a solution that isn't optimal is skipped, not a model the report can't describe.
    let mut s = wyndor();
    let mut params = wyndor_params();
    params["constraints"] = json!(["D3:D5 <= E3:E5", "B2:C2 int"]);
    let solved = run(&mut s, "data.solver", params.clone());
    // The integer optimum is the LP's own vertex here: two whole batches of each (2, 6).
    assert_eq!(solved["status"], "optimal", "{solved}");
    assert_eq!((solved["values"]["B2"].as_f64(), solved["values"]["C2"].as_f64()), (Some(2.0), Some(6.0)));
    params["reports"] = json!(["sensitivity"]);
    let e = s.execute("data.solver", params.clone()).unwrap_err().to_string();
    assert!(e.contains("integer or binary"), "{e}");
    let e = s
        .execute("data.solverReport", json!({"reports": ["sensitivity"], "skipNotOptimal": true, "model": solved["model"]}))
        .unwrap_err()
        .to_string();
    assert!(e.contains("integer or binary"), "{e}");
    // The Answer report is unaffected.
    let r = run(&mut s, "data.solverReport", json!({"reports": ["answer"], "model": solved["model"], "values": solved["values"]}));
    assert_eq!(r["sheets"], json!(["Answer Report 1"]), "{r}");
}

#[test]
fn save_model_then_load_model_brings_the_model_back() {
    let mut s = wyndor();
    let solved = run(&mut s, "data.solver", wyndor_params());
    assert_eq!(solved["status"], "optimal", "{solved}");
    // The model goes down a column as formulas: goal, count of variables, one per constraint,
    // then the options.
    let saved = run(&mut s, "data.solverSaveModel", json!({"to": "H8"}));
    assert_eq!(saved["range"], "H8:H11", "{saved}");
    assert_eq!(at(&s, "Sheet1", "H8"), Value::Number(36.0));
    // Changing the model: a minimisation saves a different model on the sheet.
    let mut min = wyndor_params();
    min["goal"] = json!("min");
    run(&mut s, "data.solver", min);
    assert_eq!(run(&mut s, "data.solver", json!({}))["model"]["goal"], "min");
    // Load Model reads the saved one back.
    let loaded = run(&mut s, "data.solverLoadModel", json!({"from": "H8"}));
    assert_eq!(loaded["model"]["goal"], "max", "{loaded}");
    let back = run(&mut s, "data.solver", json!({}))["model"].clone();
    assert_eq!(back["goal"], "max", "{back}");
    assert_eq!(back["objective"], "D6", "{back}");
    assert_eq!(back["method"], "simplex", "{back}");
    let constraints: Vec<String> =
        back["constraints"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    assert_eq!(constraints, vec!["D3:D5 <= E3:E5".to_string()], "{back}");
    // A range that isn't a model is refused and leaves the sheet's model alone.
    let e = s.execute("data.solverLoadModel", json!({"from": "A1"})).unwrap_err().to_string();
    assert!(e.contains("options row") || e.contains("complete model"), "{e}");
    assert_eq!(run(&mut s, "data.solver", json!({}))["model"]["goal"], "max");
}

#[test]
fn reset_all_removes_the_sheets_model_and_the_saved_cells_bring_it_back() {
    let mut s = wyndor();
    run(&mut s, "data.solver", wyndor_params());
    run(&mut s, "data.solverSaveModel", json!({"to": "H8"}));
    let r = run(&mut s, "data.solverReset", json!({}));
    assert!(r["removed"].as_u64().unwrap_or(0) > 0, "{r}");
    assert_eq!(r["masked"], false, "{r}");
    assert!(run(&mut s, "data.solver", json!({}))["model"].is_null());
    // The model written on the sheet is untouched by Reset All: Load Model reads it back.
    let loaded = run(&mut s, "data.solverLoadModel", json!({"from": "H8"}));
    assert_eq!(loaded["model"]["goal"], "max", "{loaded}");
    assert_eq!(run(&mut s, "data.solver", json!({}))["model"]["objective"], "D6");
    // A model re-saved over a shorter one clears the rest, so the sheet's model is the new one.
    let mut min = wyndor_params();
    min["goal"] = json!("min");
    min["constraints"] = json!(["D3 <= E3"]);
    run(&mut s, "data.solver", min);
    // Without overwriting the range already holds data: Save Model asks first, as Excel does.
    let e = s.execute("data.solverSaveModel", json!({"to": "H8"})).unwrap_err().to_string();
    assert!(e.contains("already holds data"), "{e}");
    let saved = run(&mut s, "data.solverSaveModel", json!({"to": "H8", "overwrite": true}));
    assert_eq!(saved["range"], "H8:H11", "{saved}");
    run(&mut s, "data.solverLoadModel", json!({"from": "H8"}));
    assert_eq!(run(&mut s, "data.solver", json!({}))["model"]["goal"], "min");
}

#[test]
fn a_nonlinear_model_gets_reduced_gradients_and_lagrange_multipliers() {
    // The largest rectangle with a perimeter of at most 20: the area (P/4)^2 grows by P/8 = 2.5
    // per unit of perimeter, and both reduced gradients are 0 at the optimum (5, 5).
    let mut s = Session::new();
    s.new_workbook();
    for (c, v) in [("A1", "1"), ("A2", "1"), ("B1", "=A1*A2"), ("B2", "=2*A1+2*A2")] {
        run(&mut s, "cell.set", json!({"cell": c, "input": v}));
    }
    let solved =
        run(&mut s, "data.solver", json!({"objective": "B1", "variables": "A1:A2", "constraints": ["B2 <= 20"], "reports": ["sensitivity"]}));
    assert_eq!(solved["status"], "converged", "{solved}");
    assert_eq!(solved["reports"], json!(["Sensitivity Report 1"]), "{solved}");
    let rep = "Sensitivity Report 1";
    assert_eq!(at(&s, rep, "E5"), Value::text("Reduced Gradient"));
    // Final value and reduced gradient; final value and Lagrange multiplier.
    let two = [3u32, 4];
    let var = report_row(&s, rep, "$A$1", &two);
    assert!(near(&var, &[5.0, 0.0], 1e-3), "{var:?}");
    let con = report_row(&s, rep, "$B$2", &two);
    assert!(near(&con, &[20.0, 2.5], 1e-3), "{con:?}");
}
