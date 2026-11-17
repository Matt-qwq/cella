use super::show::{form_fields, ok_cancel};
use super::{Dialog, Field};

use egui::{Color32, vec2};
use serde_json::{Map, Value as Json, json};

use crate::SheetApp;

pub(super) fn goal_seek(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    let get = |k: &str| d.values.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    let (mut set, mut to, mut changing) = (get("set"), get("to"), get("changing"));
    egui::Grid::new("gs").show(ui, |ui| {
        ui.label(crate::widgets::tr(ui, "Set cell:"));
        ui.text_edit_singleline(&mut set);
        ui.end_row();
        ui.label(crate::widgets::tr(ui, "To value:"));
        ui.text_edit_singleline(&mut to);
        ui.end_row();
        ui.label(crate::widgets::tr(ui, "By changing cell:"));
        ui.text_edit_singleline(&mut changing);
        ui.end_row();
    });
    d.values.insert("set".into(), json!(set.clone()));
    d.values.insert("to".into(), json!(to.clone()));
    d.values.insert("changing".into(), json!(changing.clone()));
    let mut open = true;
    ok_cancel(ui, confirm, &mut open);
    if *confirm {
        match app.run("data.goalSeek", json!({"set": set, "to": to.parse::<f64>().unwrap_or(0.0), "changing": changing})) {
            Ok(r) => {
                app.toast = Some((
                    crate::widgets::tr(ui, "Goal Seeking found a solution: {value}").replace("{value}", &r["value"].to_string()),
                    crate::now_ms(),
                ))
            }
            Err(e) => app.message = Some(("Goal Seek".into(), crate::clean_error(&e))),
        }
    }
    if !open {
        *confirm = true;
    }
}

/// The fields of one Analysis ToolPak tool's form (input and options; output is drawn apart).
fn toolpak_fields(tool: &str) -> Vec<Field> {
    use Field::*;
    let grouped = || Choice { key: "groupedBy", label: "Grouped By:", options: vec![("columns", "Columns"), ("rows", "Rows")] };
    let labels = || Check { key: "labels", label: "Labels in first row" };
    let range = || Text { key: "range", label: "Input Range:" };
    let two = || vec![Text { key: "range1", label: "Variable 1 Range:" }, Text { key: "range2", label: "Variable 2 Range:" }];
    match tool {
        "descriptive" => vec![
            range(),
            grouped(),
            labels(),
            Check { key: "summary", label: "Summary statistics" },
            Text { key: "confidence", label: "Confidence Level for Mean (%), blank for none:" },
            Text { key: "kthLargest", label: "Kth Largest (blank for none):" },
            Text { key: "kthSmallest", label: "Kth Smallest (blank for none):" },
        ],
        "histogram" => vec![
            range(),
            Text { key: "bins", label: "Bin Range (blank for automatic bins):" },
            labels(),
            Check { key: "pareto", label: "Pareto (sorted histogram)" },
            Check { key: "cumulative", label: "Cumulative Percentage" },
        ],
        "correlation" | "covariance" | "rankPercentile" => vec![range(), grouped(), labels()],
        "anovaSingle" => vec![range(), grouped(), labels(), Text { key: "alpha", label: "Alpha:" }],
        "regression" => vec![
            Text { key: "inputY", label: "Input Y Range:" },
            Text { key: "inputX", label: "Input X Range:" },
            labels(),
            Check { key: "constantZero", label: "Constant is Zero" },
            Text { key: "confidence", label: "Confidence Level (%):" },
            Check { key: "residuals", label: "Residuals" },
            Check { key: "standardizedResiduals", label: "Standardized Residuals" },
            Check { key: "normalProbability", label: "Normal Probability" },
        ],
        "movingAverage" => {
            vec![range(), labels(), Text { key: "interval", label: "Interval:" }, Check { key: "standardErrors", label: "Standard Errors" }]
        }
        "exponentialSmoothing" => {
            vec![range(), labels(), Text { key: "damping", label: "Damping factor:" }, Check { key: "standardErrors", label: "Standard Errors" }]
        }
        "fTest" => {
            let mut v = two();
            v.extend([labels(), Text { key: "alpha", label: "Alpha:" }]);
            v
        }
        "anovaTwoFactor" => vec![range(), labels(), Text { key: "alpha", label: "Alpha:" }],
        "sampling" => vec![
            range(),
            labels(),
            Choice { key: "method", label: "Method:", options: vec![("periodic", "Periodic"), ("random", "Random")] },
            Text { key: "period", label: "Period:" },
            Text { key: "sampleSize", label: "Number of Samples:" },
            Text { key: "seed", label: "Seed (random, 0 for any):" },
        ],
        "randomNumberGeneration" => vec![
            Text { key: "numberOfVariables", label: "Number of Variables:" },
            Text { key: "numberOfRandomNumbers", label: "Number of Random Numbers:" },
            Choice {
                key: "distribution",
                label: "Distribution:",
                options: vec![
                    ("uniform", "Uniform"),
                    ("normal", "Normal"),
                    ("bernoulli", "Bernoulli"),
                    ("binomial", "Binomial"),
                    ("poisson", "Poisson"),
                    ("pattern", "Pattern"),
                    ("discrete", "Discrete"),
                ],
            },
            Text { key: "seed", label: "Seed (0 for any):" },
            Text { key: "between", label: "Uniform: between" },
            Text { key: "and", label: "and" },
            Text { key: "mean", label: "Normal: mean" },
            Text { key: "standardDev", label: "standard deviation" },
            Text { key: "probability", label: "Bernoulli/Binomial: probability" },
            Text { key: "trials", label: "Binomial: number of trials" },
            Text { key: "lambda", label: "Poisson: lambda" },
            Text { key: "pattern", label: "Pattern: values separated by commas" },
            Text { key: "probabilities", label: "Discrete: probability range" },
        ],
        "fourierAnalysis" => vec![range()],
        "anovaTwoFactorRep" => {
            vec![range(), Text { key: "rowsPerSample", label: "Rows per sample:" }, labels(), Text { key: "alpha", label: "Alpha:" }]
        }
        "zTest" => {
            let mut v = two();
            v.extend([
                Text { key: "variance1", label: "Variable 1 Variance (known):" },
                Text { key: "variance2", label: "Variable 2 Variance (known):" },
                Text { key: "hypothesizedDifference", label: "Hypothesized Mean Difference:" },
                labels(),
                Text { key: "alpha", label: "Alpha:" },
            ]);
            v
        }
        _ => {
            let mut v = two();
            v.extend([
                Text { key: "hypothesizedDifference", label: "Hypothesized Mean Difference:" },
                labels(),
                Text { key: "alpha", label: "Alpha:" },
            ]);
            v
        }
    }
}

/// The starting values of a tool's form: the selection as its input, a new sheet as output.
pub(super) fn toolpak_defaults(sel: &str, tool: &str) -> Map<String, Json> {
    let mut v = json!({
        "tool": tool, "range": sel, "inputY": "", "inputX": "", "range1": sel, "range2": "", "bins": "",
        "groupedBy": "columns", "labels": false, "summary": true, "confidence": "", "kthLargest": "", "kthSmallest": "",
        "interval": "3", "damping": "0.3", "alpha": "0.05", "hypothesizedDifference": "0", "variance1": "", "variance2": "", "rowsPerSample": "",
        "method": "periodic", "period": "1", "sampleSize": "", "numberOfVariables": "1", "numberOfRandomNumbers": "1",
        "distribution": "uniform", "seed": "0", "between": "0", "and": "1", "mean": "0", "standardDev": "1",
        "probability": "0.5", "trials": "10", "lambda": "1", "pattern": "", "probabilities": "",
        "outputTo": "newSheet", "output": "", "newSheet": "",
    });
    if tool == "regression" {
        v["confidence"] = json!("95");
        v["inputY"] = json!(sel);
    }
    v.as_object().cloned().unwrap_or_default()
}

/// Data › Data Analysis: pick a tool, then fill in its form. OK runs `data.analysisToolPak`
/// (through the dialog's command); a report that would replace data asks first.
pub(super) fn analysis_toolpak(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    use cella_engine::cmd::analysis::TOOLS;
    if d.values.get("askOverwrite").and_then(Json::as_bool) == Some(true) {
        ui.label(crate::widgets::tr(ui, "The output range already holds data. Replace it with the report?"));
        ui.horizontal(|ui| {
            if ui.button(crate::widgets::tr(ui, "Replace")).clicked() {
                d.values.remove("askOverwrite");
                d.values.insert("overwrite".into(), json!(true));
                *confirm = true;
            }
            if ui.button(crate::widgets::tr(ui, "Cancel")).clicked() {
                d.values.remove("askOverwrite");
            }
        });
        return;
    }
    let tool = d.values.get("tool").and_then(Json::as_str).unwrap_or("").to_string();
    let Some((_, label)) = TOOLS.iter().find(|(id, _)| *id == tool) else {
        ui.label(egui::RichText::new(crate::widgets::tr(ui, "Analysis Tools")).strong());
        let pick = d.values.get("pick").and_then(Json::as_str).unwrap_or("").to_string();
        let mut chosen = false;
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            for (id, label) in TOOLS {
                let r = ui.selectable_label(pick == *id, crate::widgets::tr(ui, label));
                if r.clicked() {
                    d.values.insert("pick".into(), json!(id));
                }
                if r.double_clicked() {
                    chosen = true;
                }
            }
        });
        let mut ok = false;
        ok_cancel(ui, &mut ok, open);
        let pick = d.values.get("pick").and_then(Json::as_str).unwrap_or("").to_string();
        if (ok || chosen) && TOOLS.iter().any(|(id, _)| *id == pick) {
            let sel = app.session.active().map(|doc| doc.selection.current().a1()).unwrap_or_default();
            d.values = toolpak_defaults(&sel, &pick);
        }
        return;
    };
    d.title = label.to_string();
    d.fields = toolpak_fields(&tool);
    form_fields(ui, d);
    ui.separator();
    ui.label(egui::RichText::new(crate::widgets::tr(ui, "Output options")).strong());
    let to = d.values.get("outputTo").and_then(Json::as_str).unwrap_or("newSheet").to_string();
    for (key, choice, text_label) in [("output", "range", "Output Range:"), ("newSheet", "newSheet", "New Worksheet Ply:")] {
        ui.horizontal(|ui| {
            if ui.radio(to == choice, crate::widgets::tr(ui, text_label)).clicked() {
                d.values.insert("outputTo".into(), json!(choice));
            }
            let mut t = d.values.get(key).and_then(Json::as_str).unwrap_or("").to_string();
            if ui.add(egui::TextEdit::singleline(&mut t).desired_width(160.0)).changed() {
                d.values.insert(key.into(), json!(t));
                d.values.insert("outputTo".into(), json!(choice));
            }
        });
    }
    ok_cancel(ui, confirm, open);
}

/// Data › Solver: the Solver Parameters form. Solve runs `data.solver` through the dialog's
/// command; the result opens Solver Results.
pub(super) fn solver(ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let text = |d: &Dialog, k: &str| d.values.get(k).map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).unwrap_or_default();
    let edit = |ui: &mut egui::Ui, d: &mut Dialog, k: &str, width: f32| {
        let mut t = text(d, k);
        if ui.add(egui::TextEdit::singleline(&mut t).desired_width(width)).changed() {
            d.values.insert(k.into(), json!(t));
        }
    };
    ui.horizontal(|ui| {
        ui.label(crate::widgets::tr(ui, "Set Objective:"));
        edit(ui, d, "objective", 200.0);
    });
    let goal = text(d, "goal");
    ui.horizontal(|ui| {
        ui.label(crate::widgets::tr(ui, "To:"));
        for (g, label) in [("max", "Max"), ("min", "Min"), ("value", "Value Of:")] {
            if ui.radio(goal == g, crate::widgets::tr(ui, label)).clicked() {
                d.values.insert("goal".into(), json!(g));
            }
        }
        edit(ui, d, "value", 80.0);
    });
    ui.label(crate::widgets::tr(ui, "By Changing Variable Cells:"));
    edit(ui, d, "variables", 420.0);
    ui.label(crate::widgets::tr(ui, "Subject to the Constraints:"));
    let list: Vec<String> = d
        .values
        .get("constraints")
        .and_then(Json::as_array)
        .map(|a| a.iter().map(|c| c.as_str().map(str::to_string).unwrap_or_else(|| c.to_string())).collect())
        .unwrap_or_default();
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_size(vec2(420.0, 90.0));
        egui::ScrollArea::vertical().max_height(110.0).show(ui, |ui| {
            for (i, c) in list.iter().enumerate() {
                if ui.selectable_label(d.list_index == i, c).clicked() {
                    d.list_index = i;
                }
            }
        });
    });
    ui.horizontal(|ui| {
        edit(ui, d, "newLhs", 110.0);
        let op = text(d, "newOp");
        egui::ComboBox::from_id_salt("solverOp").selected_text(op.clone()).width(60.0).show_ui(ui, |ui| {
            for o in ["<=", "=", ">=", "int", "bin", "dif"] {
                if ui.selectable_label(op == o, o).clicked() {
                    d.values.insert("newOp".into(), json!(o));
                }
            }
        });
        if !matches!(op.as_str(), "int" | "bin" | "dif") {
            edit(ui, d, "newRhs", 110.0);
        }
        if ui.button(crate::widgets::tr(ui, "Add")).clicked() {
            let lhs = text(d, "newLhs");
            if !lhs.trim().is_empty() {
                let rhs = if matches!(op.as_str(), "int" | "bin" | "dif") { String::new() } else { format!(" {}", text(d, "newRhs").trim()) };
                let mut list = list.clone();
                list.push(format!("{} {op}{rhs}", lhs.trim()));
                d.values.insert("constraints".into(), json!(list));
                d.values.insert("newLhs".into(), json!(""));
                d.values.insert("newRhs".into(), json!(""));
            }
        }
        if ui.add_enabled(d.list_index < list.len(), egui::Button::new(crate::widgets::tr(ui, "Delete"))).clicked() {
            let mut list = list.clone();
            if d.list_index < list.len() {
                list.remove(d.list_index);
            }
            d.values.insert("constraints".into(), json!(list));
        }
    });
    let mut nonneg = d.values.get("nonNegative").and_then(Json::as_bool).unwrap_or(true);
    if ui.checkbox(&mut nonneg, crate::widgets::tr(ui, "Make Unconstrained Variables Non-Negative")).changed() {
        d.values.insert("nonNegative".into(), json!(nonneg));
    }
    let method = text(d, "method");
    ui.horizontal(|ui| {
        ui.label(crate::widgets::tr(ui, "Select a Solving Method:"));
        let label = |m: &str| match m {
            "simplex" => "Simplex LP",
            "evolutionary" => "Evolutionary",
            _ => "GRG Nonlinear",
        };
        egui::ComboBox::from_id_salt("solverMethod").selected_text(crate::widgets::tr(ui, label(&method))).show_ui(ui, |ui| {
            for m in ["grg", "simplex", "evolutionary"] {
                if ui.selectable_label(method == m, crate::widgets::tr(ui, label(m))).clicked() {
                    d.values.insert("method".into(), json!(m));
                }
            }
        });
    });
    ui.label(
        egui::RichText::new(crate::widgets::tr(
            ui,
            match method.as_str() {
                "simplex" => "Simplex LP: for linear models; integer and binary constraints by branch and bound.",
                "evolutionary" => "Evolutionary: a genetic search for models that aren't smooth; every variable cell needs bounds.",
                _ => "GRG Nonlinear: a local optimum of a smooth model, from the current values.",
            },
        ))
        .small(),
    );
    // Solver Options: the evolutionary search's parameters and the evaluation budget, exactly the
    // params data.solver documents.
    let method_text = text(d, "method");
    egui::CollapsingHeader::new(crate::widgets::tr(ui, "Options")).id_salt("solverOptions").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(crate::widgets::tr(ui, "Population Size:"));
            edit(ui, d, "populationSize", 70.0);
            ui.label(crate::widgets::tr(ui, "Mutation Rate:"));
            edit(ui, d, "mutationRate", 70.0);
            ui.label(crate::widgets::tr(ui, "Random Seed:"));
            edit(ui, d, "randomSeed", 70.0);
        });
        ui.horizontal(|ui| {
            ui.label(crate::widgets::tr(ui, "Convergence:"));
            edit(ui, d, "convergence", 70.0);
            ui.label(crate::widgets::tr(ui, "Generations Without Improvement:"));
            edit(ui, d, "generationsWithoutImprovement", 60.0);
        });
        ui.horizontal(|ui| {
            ui.label(crate::widgets::tr(ui, "Max Evaluations:"));
            edit(ui, d, "maxEvaluations", 90.0);
            ui.label(crate::widgets::tr(ui, "Integer Optimality (%):"));
            edit(ui, d, "integerOptimality", 60.0);
        });
        let mut bounds = d.values.get("requireBounds").and_then(Json::as_bool).unwrap_or(true);
        if ui.checkbox(&mut bounds, crate::widgets::tr(ui, "Require Bounds on Variables")).changed() {
            d.values.insert("requireBounds".into(), json!(bounds));
        }
        if method_text != "evolutionary" && method_text != "simplex" {
            ui.label(
                egui::RichText::new(crate::widgets::tr(
                    ui,
                    "The population, mutation, seed, convergence and generations are the Evolutionary method's.",
                ))
                .small(),
            );
        }
    });
    ui.add_space(8.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let solve = ui.add(
            egui::Button::new(egui::RichText::new(format!("  {}  ", crate::widgets::tr(ui, "Solve"))).color(Color32::WHITE))
                .fill(crate::theme::Tokens::get(ui.ctx()).accent),
        );
        if solve.clicked() && !text(d, "objective").trim().is_empty() {
            *confirm = true;
        }
        if ui.button(crate::widgets::tr(ui, " Close ")).clicked() {
            *open = false;
        }
    });
}

/// The label `tr` for a row of a read-only view.
fn row_label(ui: &mut egui::Ui, label: &str, value: impl std::fmt::Display) {
    ui.horizontal(|ui| {
        ui.label(crate::widgets::tr(ui, label));
        ui.strong(value.to_string());
    });
}

/// Review › Workbook Statistics: the counts Excel lists, in the interface language.
pub(super) fn statistics_view(ui: &mut egui::Ui, d: &Dialog) {
    let r = d.result.clone().unwrap_or(Json::Null);
    let n = |v: &str| r["workbook"][v].as_u64();
    if r.get("workbook").is_some() {
        row_label(ui, "Workbook", crate::widgets::tr(ui, "This workbook"));
        for (key, label) in
            [("sheets", "Sheets"), ("cellsWithData", "Cells with data"), ("tables", "Tables"), ("charts", "Charts"), ("names", "Names")]
        {
            if let Some(v) = n(key) {
                row_label(ui, label, v);
            }
        }
    }
    let sheet = &r["sheet"];
    if sheet.is_object() {
        ui.add_space(6.0);
        let name = sheet["name"].as_str().unwrap_or("");
        ui.label(format!("{}: {name}", crate::widgets::tr(ui, "Sheet")));
        if let Some(end) = sheet["endOfSheet"].as_str() {
            row_label(ui, "End of sheet", end);
        }
        for (key, label) in
            [("cellsWithData", "Cells with data"), ("tables", "Tables"), ("formulas", "Formulas"), ("charts", "Charts"), ("comments", "Comments")]
        {
            if let Some(v) = sheet[key].as_u64() {
                row_label(ui, label, v);
            }
        }
    }
}

/// Review › Check Accessibility: what is wrong, or that nothing is.
pub(super) fn error_checking_view(ui: &mut egui::Ui, result: Option<&Json>) {
    let r = result.cloned().unwrap_or(Json::Null);
    let circular: Vec<String> =
        r["circular"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let errors: Vec<Json> = r["errors"].as_array().cloned().unwrap_or_default();
    if circular.is_empty() && errors.is_empty() {
        ui.label(crate::widgets::tr(ui, "No problems found."));
        return;
    }
    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
        for c in &circular {
            ui.label(format!("{} {c}", crate::widgets::tr(ui, "Circular reference:")));
        }
        for e in &errors {
            let cell = e["cell"].as_str().unwrap_or("");
            let sheet = e["sheet"].as_str().unwrap_or("");
            let err = e["error"].as_str().unwrap_or("");
            ui.label(format!("{sheet}!{cell}  {err}"));
        }
    });
}

/// Evaluate Formula: the result, then the steps that produced it.
pub(super) fn evaluate_view(ui: &mut egui::Ui, result: Option<&Json>) {
    let Some(r) = result else {
        ui.label(crate::widgets::tr(ui, "Nothing to evaluate."));
        return;
    };
    if let Some(f) = r["formula"].as_str() {
        ui.monospace(f);
    }
    let result_text = r["result"].as_str().map(str::to_string).unwrap_or_else(|| r["result"].to_string());
    row_label(ui, "Result:", result_text);
    if let Some(steps) = r["steps"].as_array().filter(|s| !s.is_empty()) {
        ui.add_space(6.0);
        egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
            for s in steps {
                let expr = s["expression"].as_str().unwrap_or("");
                let value = s["value"].as_str().map(str::to_string).unwrap_or_else(|| s["value"].to_string());
                ui.monospace(format!("{expr}  =  {value}"));
            }
        });
    }
}

/// Review › Comments: every comment on the sheet.
pub(super) fn comments_view(app: &mut SheetApp, ui: &mut egui::Ui) {
    let Some(sh) = app.session.active().and_then(|d| d.wb.active()) else { return };
    if sh.comments.is_empty() {
        ui.label(crate::widgets::tr(ui, "There are no comments on this sheet."));
        return;
    }
    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
        for (cell, m) in &sh.comments {
            ui.label(
                crate::widgets::tr(ui, "{cell} ({author}): {text}")
                    .replace("{cell}", &cell.a1())
                    .replace("{author}", &m.author)
                    .replace("{text}", &m.text),
            );
        }
    });
}

/// Solver Results: the outcome, and keeping or restoring the variable cells.
pub(super) fn solver_results(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, open: &mut bool) {
    let message = d.values.get("message").and_then(Json::as_str).unwrap_or("").to_string();
    ui.label(crate::widgets::tr(ui, &message));
    let changed = d.values.get("changed").and_then(Json::as_bool).unwrap_or(false);
    let restore = d.values.get("restore").and_then(Json::as_bool).unwrap_or(false);
    if changed {
        ui.add_space(4.0);
        if ui.radio(!restore, crate::widgets::tr(ui, "Keep Solver Solution")).clicked() {
            d.values.insert("restore".into(), json!(false));
        }
        if ui.radio(restore, crate::widgets::tr(ui, "Restore Original Values")).clicked() {
            d.values.insert("restore".into(), json!(true));
        }
    }
    // Reports describe the solution whether it is kept or not.
    let solved = d.values.get("values").and_then(Json::as_object).is_some_and(|v| !v.is_empty());
    if solved {
        ui.add_space(4.0);
        ui.label(crate::widgets::tr(ui, "Reports:"));
        for (key, label) in [("answerReport", "Answer"), ("limitsReport", "Limits")] {
            let mut on = d.values.get(key).and_then(Json::as_bool).unwrap_or(false);
            if ui.checkbox(&mut on, crate::widgets::tr(ui, label)).changed() {
                d.values.insert(key.into(), json!(on));
            }
        }
    }
    let mut confirm = false;
    ok_cancel(ui, &mut confirm, open);
    if confirm {
        // The solution (and the saved model) is one undo step.
        if changed && restore {
            app.run_or_alert("edit.undo", json!({}));
        }
        let reports: Vec<&str> = [("answerReport", "answer"), ("limitsReport", "limits")]
            .into_iter()
            .filter(|(key, _)| d.values.get(*key).and_then(Json::as_bool) == Some(true))
            .map(|(_, kind)| kind)
            .collect();
        if solved && !reports.is_empty() {
            let field = |k: &str| d.values.get(k).cloned().unwrap_or(Json::Null);
            let params = json!({"reports": reports, "model": field("model"), "values": field("values"), "original": field("original"), "message": field("message")});
            app.run_or_alert("data.solverReport", params);
        }
        *open = false;
    }
}
