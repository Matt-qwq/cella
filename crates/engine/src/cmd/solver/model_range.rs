//! Load/Save Model and Reset All: the model written on cells of its sheet, read back from them,
//! and removed from the sheet.

use cella_core::{CellRef, RangeRef, Value, split_sheet};
use cella_model::{Cell, DefinedName, Formula, Sheet};
use serde_json::Value as Json;

use crate::cmd::analysis::OVERWRITE_ERROR;
use crate::cmd::{cell_param_on, edit};
use crate::{EngineError, Result, Session};

use super::{
    LOAD_CMD, MAX_CONSTRAINTS, Method, Model, Rel, Rhs, SAVE_CMD, bad, bool_param, is_model_name, json, num_text, parse_model, percent, save_model,
    saved_model, str_param,
};

/// Most cells a model written by Save Model takes: the objective, the variables, the options
/// and one per constraint.
const MAX_MODEL_ROWS: usize = MAX_CONSTRAINTS + 3;

/// How many values the options row of Save Model holds at least: method, non-negative,
/// population size, mutation rate, random seed, require bounds, convergence and generations
/// without improvement.
const OPTIONS_FIELDS: usize = 8;

/// Splits "A1,B2:C3" at the commas outside quoted sheet names, so a comma inside one
/// ("'Sales, West'!A1") doesn't start another area. A doubled quote inside a name toggles the
/// quoted state twice, as split_unquoted does, so it doesn't end it.
fn split_areas(t: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut quoted = false;
    for (i, c) in t.char_indices() {
        match c {
            '\'' => quoted = !quoted,
            ',' if !quoted => {
                if let Some(part) = t.get(start..i) {
                    out.push(part);
                }
                start = i.saturating_add(1);
            }
            _ => {}
        }
    }
    if let Some(part) = t.get(start..) {
        out.push(part);
    }
    out
}

/// t split at the first pat outside quoted sheet names: 'A<=B'!C1 <= 5 splits at the second
/// < =. A doubled quote inside a name toggles the quoted state twice, so it doesn't end it.
fn split_unquoted<'a>(t: &'a str, pat: &str) -> Option<(&'a str, &'a str)> {
    let mut quoted = false;
    for (i, c) in t.char_indices() {
        if c == '\'' {
            quoted = !quoted;
        } else if !quoted && t.get(i..).is_some_and(|rest| rest.starts_with(pat)) {
            return Some((t.get(..i)?, t.get(i.saturating_add(pat.len())..)?));
        }
    }
    None
}

fn is_options_row(formula: &str) -> bool {
    formula.trim().trim_start_matches('=').trim_start().starts_with('{')
}

/// The formulas from one cell down: to the first cell without a formula, or through the options
/// row ({...}) that ends a model written by Save Model; at most MAX_MODEL_ROWS cells.
fn formulas_down(sheet: &Sheet, start: CellRef) -> Vec<(CellRef, String)> {
    let mut out = Vec::new();
    for k in 0..MAX_MODEL_ROWS as u32 {
        let Some(c) = start.row.checked_add(k).map(|row| CellRef::new(row, start.col)) else { break };
        let Some(f) = sheet.cell(c).and_then(|x| x.formula).map(|f| f.text.clone()) else { break };
        let last = is_options_row(&f);
        out.push((c, f));
        if last {
            break;
        }
    }
    out
}

/// The cells of a model written by Save Model that starts at one cell (none if the cells there
/// don't hold one).
fn saved_model_cells(sheet: &Sheet, start: CellRef) -> Vec<CellRef> {
    let old = formulas_down(sheet, start);
    let rows: Vec<String> = old.iter().map(|(_, f)| f.clone()).collect();
    if rows.last().is_some_and(|f| is_options_row(f)) && parse_model_formulas(&rows).is_ok() {
        old.into_iter().map(|(c, _)| c).collect()
    } else {
        Vec::new()
    }
}

/// How many cells the options row of Save Model (one row holding an array constant) spans.
fn options_width(formula: &str) -> u32 {
    u32::try_from(formula.split(',').count()).unwrap_or(u32::MAX)
}

/// The options row is an array constant that spills width - 1 cells to the right of its anchor.
/// Refused, before anything is written, when the spill would run off the sheet or be blocked
/// (the anchor would show #SPILL!): a cell there holds content or another formula's spilled
/// values, or a merged area or a table meets the row. "ours" are the cells Save Model writes or
/// clears (a model saved at the same place before, whose options spill may be replaced); "used"
/// are the model's own cells, which the spill must not cover.
fn check_options_spill(sheet: &Sheet, anchor: CellRef, width: u32, ours: &[CellRef], used: &[CellRef]) -> Result<()> {
    if width <= 1 {
        return Ok(());
    }
    let end = anchor.col.checked_add(width - 1).filter(|c| *c < cella_core::MAX_COLS).ok_or_else(|| {
        EngineError::Other(format!(
            "The saved model's options need {width} cells to the right from {}; choose another place to save it.",
            anchor.a1()
        ))
    })?;
    let full = RangeRef::new(anchor, CellRef::new(anchor.row, end));
    let spill = RangeRef::new(CellRef::new(anchor.row, anchor.col.saturating_add(1)), CellRef::new(anchor.row, end));
    let blocked =
        |why: String| EngineError::Other(format!("The saved model's options fill {}, but {why}; choose another place to save it.", spill.a1()));
    if let Some(c) = used.iter().find(|c| spill.contains(**c)) {
        return Err(blocked(format!("{} is a cell of the model", c.a1())));
    }
    for c in spill.iter() {
        if sheet.cells.contains(c) {
            return Err(blocked(format!("{} isn't empty", c.a1())));
        }
        if sheet.spill.contains_key(&c) && sheet.spill_anchor(c).is_none_or(|a| !ours.contains(&a)) {
            return Err(blocked(format!("{} holds the values of another formula", c.a1())));
        }
    }
    if sheet.merges.iter().any(|m| m.intersects(&full)) {
        return Err(blocked("merged cells are in the way".into()));
    }
    // A dynamic array can't spill into a table.
    if sheet.tables.iter().any(|t| t.range.intersects(&full)) {
        return Err(blocked("a table is in the way".into()));
    }
    Ok(())
}

/// Refused when a cell of the model's column holds another formula's spilled values: the
/// formula written there would block that spill (the other formula would show #SPILL!). "ours"
/// are as in check_options_spill: a model saved at the same place before may be replaced.
fn check_column_spills(sheet: &Sheet, column: RangeRef, ours: &[CellRef]) -> Result<()> {
    for c in column.iter() {
        if sheet.spill.contains_key(&c) && sheet.spill_anchor(c).is_none_or(|a| !ours.contains(&a)) {
            return Err(EngineError::Other(format!(
                "The saved model would cover {}, which holds the values of another formula; choose another place to save it.",
                c.a1()
            )));
        }
    }
    Ok(())
}

/// Refused, unless overwriting, when a cell of the model's column holds a value or a formula (a
/// model saved there before included): Save Model then asks before it replaces data, as Excel
/// does. The message carries OVERWRITE_ERROR, which the dialog turns into its question.
fn check_column_empty(sheet: &Sheet, column: RangeRef, overwrite: bool) -> Result<()> {
    let taken = column.iter().find(|c| sheet.cell(*c).is_some_and(|cell| !cell.is_blank()));
    match taken {
        Some(c) if !overwrite => Err(EngineError::Other(format!(
            "The range {} {OVERWRITE_ERROR} ({} isn't empty); pass overwrite: true to replace it.",
            column.a1(),
            c.a1()
        ))),
        _ => Ok(()),
    }
}

/// "B2:B4,D1" as absolute references, each qualified by the sheet name when given.
fn abs_text(sheet_name: Option<&str>, cells: &str) -> String {
    let q = sheet_name.map(|n| format!("{}!", crate::pivot::quote_sheet(n))).unwrap_or_default();
    split_areas(cells)
        .into_iter()
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .filter_map(|part| {
            let r = RangeRef::parse(split_sheet(part).1.trim())?;
            let abs = |c: CellRef| format!("${}${}", cella_core::col_to_letters(c.col), c.row as u64 + 1);
            Some(if r.is_single() { format!("{q}{}", abs(r.start)) } else { format!("{q}{}:{}", abs(r.start), abs(r.end)) })
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// The cells Save Model writes, top to bottom, as formula text.
fn model_formulas(m: &Model) -> Result<Vec<String>> {
    // A comma inside a quoted sheet name ("'Sales, West'!A1") doesn't start another area.
    let one_area = |t: &str| split_areas(t).into_iter().filter(|a| !a.trim().is_empty()).count() <= 1;
    let mut rows = Vec::with_capacity(m.cons.len() + 3);
    let obj = abs_text(None, &m.objective.a1());
    rows.push(match m.goal {
        super::Goal::Max => format!("=MAX({obj})"),
        super::Goal::Min => format!("=MIN({obj})"),
        super::Goal::Value(v) => format!("={obj}={}", cella_core::number_to_text(v)),
    });
    rows.push(format!("=COUNT({})", abs_text(None, &m.vars_text)));
    for c in &m.cons {
        if c.rel == Rel::Dif {
            return Err(EngineError::Other(
                "Save Model can't write an all-different constraint: it has no Excel formula form. Remove it, or keep the model in the workbook's hidden names."
                    .into(),
            ));
        }
        // A number on the right side (1,000) is no range, whatever its separators.
        if !one_area(&c.lhs_text) || (matches!(c.rhs, Rhs::Cells(_)) && !one_area(&c.rhs_text)) {
            return Err(EngineError::Other("Save Model needs each side of a constraint to be one range.".into()));
        }
        let lhs = abs_text(None, &c.lhs_text);
        rows.push(match (c.rel, &c.rhs) {
            (Rel::Int, _) => format!("=AND({lhs}=INT({lhs}))"),
            (Rel::Bin, _) => format!("=AND(({lhs}=0)+({lhs}=1))"),
            (rel, Rhs::Cells(_)) => format!("=AND({lhs}{}{})", rel.text(), abs_text(None, &c.rhs_text)),
            (rel, Rhs::Const(v)) => format!("=AND({lhs}{}{})", rel.text(), cella_core::number_to_text(*v)),
            (rel, Rhs::None) => format!("=AND({lhs}{}0)", rel.text()),
        });
    }
    let e = &m.evo;
    let n = cella_core::number_to_text;
    let eng = match m.method {
        Method::Grg => 1,
        Method::Simplex => 2,
        Method::Evolutionary => 3,
    };
    let yes = |b: bool| if b { 1 } else { 2 };
    rows.push(format!(
        "={{{eng},{},{},{},{},{},{},{},{},{}}}",
        yes(m.non_negative),
        e.population,
        n(e.mutation),
        e.seed,
        yes(e.require_bounds),
        n(e.convergence),
        e.stall,
        m.max_evals,
        n(percent(m.gap))
    ));
    Ok(rows)
}

/// NAME(inner) becomes inner, ignoring case.
fn call_args(t: &str, name: &str) -> Option<String> {
    let head = t.get(..name.len() + 1)?;
    (head.eq_ignore_ascii_case(&format!("{name}(")) && t.ends_with(')')).then(|| t.get(name.len() + 1..t.len() - 1).map(str::to_string))?
}

/// References without sheet names or dollar signs.
fn local_refs(t: &str) -> String {
    split_areas(t).into_iter().map(|p| split_sheet(p.trim()).1.replace('$', "")).collect::<Vec<_>>().join(",")
}

fn not_a_model() -> EngineError {
    EngineError::Other("The range doesn't hold a Solver model saved with Save Model.".into())
}

/// The options row of Save Model, {method, non-negative, ...}, into the model's params. Refused
/// when it holds fewer than OPTIONS_FIELDS values or a value that isn't a number: a row cut
/// short or edited by hand would otherwise load with default options in place of the saved ones.
fn read_options_row(model: &mut Json, list: &str) -> Result<()> {
    let incomplete = || {
        EngineError::Other(format!(
            "The model's options row doesn't hold the {OPTIONS_FIELDS} or more numbers Save Model writes, so the range isn't a complete model saved with Save Model."
        ))
    };
    // Every value is checked; only the first ones are kept (the row is one formula, so its
    // length is bounded by the formula's).
    let mut v: Vec<f64> = Vec::with_capacity(16);
    let mut count = 0usize;
    for t in list.split([',', ';']) {
        let x = num_text(t).ok_or_else(incomplete)?;
        count = count.saturating_add(1);
        if v.len() < 16 {
            v.push(x);
        }
    }
    if count < OPTIONS_FIELDS {
        return Err(incomplete());
    }
    let get = |k: usize| v.get(k).copied();
    model["method"] = json!(match get(0).map(|x| x as i64) {
        Some(2) => "simplex",
        Some(3) => "evolutionary",
        _ => "grg",
    });
    model["nonNegative"] = json!(get(1) != Some(2.0));
    for (k, key) in [
        (2, "populationSize"),
        (3, "mutationRate"),
        (4, "randomSeed"),
        (6, "convergence"),
        (7, "generationsWithoutImprovement"),
        (8, "maxEvaluations"),
        (9, "integerOptimality"),
    ] {
        if let Some(x) = get(k) {
            model[key] = json!(x);
        }
    }
    model["requireBounds"] = json!(get(5) != Some(2.0));
    Ok(())
}

/// One constraint row of Save Model (an AND of the relation) as a data.solver constraint.
fn read_constraint_row(t: String) -> Result<String> {
    let t = call_args(&t, "AND").unwrap_or(t);
    if let Some((x, y)) = split_unquoted(&t, "=INT(").or_else(|| split_unquoted(&t, "=int(")) {
        if y.strip_suffix(')') != Some(x) {
            return Err(not_a_model());
        }
        return Ok(format!("{} int", local_refs(x)));
    }
    if let Some(x) = t.strip_prefix('(').and_then(|x| x.strip_suffix("=1)")).and_then(|x| split_unquoted(x, "=0)+(")).filter(|(a, b)| a == b) {
        return Ok(format!("{} bin", local_refs(x.0)));
    }
    let (l, rel, r) = super::split_constraint(&t).ok_or_else(not_a_model)?;
    if rel == Rel::Dif {
        return Err(not_a_model());
    }
    let r = r.trim_matches('"');
    Ok(match r.to_ascii_lowercase().as_str() {
        "integer" => format!("{} int", local_refs(&l)),
        "binary" => format!("{} bin", local_refs(&l)),
        _ if num_text(r).is_some() => format!("{} {} {r}", local_refs(&l), rel.text()),
        _ => format!("{} {} {}", local_refs(&l), rel.text(), local_refs(r)),
    })
}

/// Reads formula text written by Save Model back into data.solver params.
fn parse_model_formulas(rows: &[String]) -> Result<Json> {
    let body = |t: &str| t.trim().trim_start_matches('=').replace(' ', "");
    let first = body(rows.first().ok_or_else(not_a_model)?);
    let (goal, objective, value) = if let Some(o) = call_args(&first, "MAX") {
        ("max", o, 0.0)
    } else if let Some(o) = call_args(&first, "MIN") {
        ("min", o, 0.0)
    } else {
        let (o, v) = split_unquoted(&first, "=").ok_or_else(not_a_model)?;
        ("value", o.to_string(), num_text(v).ok_or_else(not_a_model)?)
    };
    let variables = call_args(&body(rows.get(1).ok_or_else(not_a_model)?), "COUNT").ok_or_else(not_a_model)?;
    let mut model = json!({"objective": local_refs(&objective), "goal": goal, "value": value, "variables": local_refs(&variables)});
    let mut constraints = Vec::new();
    let rest = rows.get(2..).unwrap_or(&[]);
    for (i, row) in rest.iter().enumerate() {
        let t = body(row);
        if let Some(list) = t.strip_prefix('{').and_then(|x| x.strip_suffix('}')) {
            // The options: the last row.
            if i + 1 != rest.len() {
                return Err(not_a_model());
            }
            read_options_row(&mut model, list)?;
            break;
        }
        constraints.push(json!(read_constraint_row(t)?));
    }
    model["constraints"] = json!(constraints);
    Ok(model)
}

/// data.solverSaveModel: writes the model on cells of its sheet, in one step. Saving over a
/// longer model saved at the same cell clears the rest of it, so Load Model reads the new one.
pub(super) fn save_model_cells(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let params = match p.get("model") {
        Some(m @ Json::Object(_)) => m.clone(),
        Some(Json::Null) | None => {
            saved_model(&d.wb, d.wb.active_sheet).ok_or_else(|| EngineError::Other("This sheet has no saved Solver model.".into()))?
        }
        Some(_) => return Err(bad(SAVE_CMD, "model must be data.solver params")),
    };
    let model = parse_model(s, &params)?;
    let to = cell_param_on(s, p, "to", model.sheet)?.ok_or_else(|| bad(SAVE_CMD, "missing to (the first cell of the saved model)"))?;
    let rows = model_formulas(&model)?;
    let count = u32::try_from(rows.len()).unwrap_or(u32::MAX);
    let last = to
        .row
        .checked_add(count.saturating_sub(1))
        .filter(|r| *r < cella_core::MAX_ROWS)
        .ok_or_else(|| EngineError::Other("The saved model doesn't fit below that cell.".into()))?;
    let range = RangeRef::new(to, CellRef::new(last, to.col));
    // The model's own cells are never overwritten.
    let mut used = vec![model.objective];
    used.extend(&model.vars);
    for c in &model.cons {
        used.extend(&c.lhs);
        if let Rhs::Cells(r) = &c.rhs {
            used.extend(r);
        }
    }
    if used.iter().any(|c| range.contains(*c)) {
        return Err(EngineError::Other(format!("{} overlaps the model's cells; choose another place to save it.", range.a1())));
    }
    let sheet = model.sheet;
    let sh = s.doc()?.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    // What is left of a model saved here before; the new model's own cells stay.
    let stale: Vec<CellRef> = saved_model_cells(sh, to).into_iter().filter(|c| !range.contains(*c) && !used.contains(c)).collect();
    let ours: Vec<CellRef> = range.iter().chain(stale.iter().copied()).collect();
    let width = rows.last().map_or(1, |f| options_width(f));
    check_column_spills(sh, range, &ours)?;
    check_options_spill(sh, CellRef::new(last, to.col), width, &ours, &used)?;
    // Last, so the question is asked only when replacing the data would succeed.
    check_column_empty(sh, range, bool_param(p, "overwrite").unwrap_or(false))?;
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        let cells: Vec<CellRef> = range.iter().collect();
        for (c, f) in cells.iter().zip(&rows) {
            let mut cell = Cell::formula(Formula::new(f));
            cell.style = sh.style_id(*c);
            sh.set_cell(*c, cell);
        }
        for c in &stale {
            let style = sh.style_id(*c);
            sh.set_cell(*c, Cell { value: Value::Empty, formula: None, style });
        }
        for c in cells.into_iter().chain(stale) {
            cx.touch(sheet, c);
        }
        Ok(())
    })?;
    Ok(json!({"range": range.a1()}))
}

/// The formulas of a model range, in order. Every cell up to the last formula must hold one: a
/// cell without a formula there (a constraint overwritten by a constant, say) would shift the
/// rows below it and drop a constraint or the options. Empty cells after the last formula are
/// allowed.
fn cells_with_formulas(r: RangeRef, formula: impl Fn(CellRef) -> Option<String>) -> Result<Vec<String>> {
    let cells: Vec<(CellRef, Option<String>)> = r.iter().map(|c| (c, formula(c))).collect();
    let end = cells.iter().rposition(|(_, f)| f.is_some()).map_or(0, |i| i + 1);
    let mut rows = Vec::with_capacity(end);
    for (c, f) in cells.into_iter().take(end) {
        let f = f.ok_or_else(|| EngineError::Other(format!("{} holds no formula, so the range isn't a model saved with Save Model.", c.a1())))?;
        rows.push(f);
    }
    Ok(rows)
}

/// A model read down from one cell must end with its options row, which Save Model always
/// writes last: a cell without a formula before it (a constraint cleared, say) would otherwise
/// end the read early and load the rows above it as the whole model, with default options.
fn complete_from_cell(start: CellRef, down: &[(CellRef, String)]) -> Result<()> {
    match down.last() {
        Some((_, f)) if is_options_row(f) => Ok(()),
        _ if down.len() >= MAX_MODEL_ROWS => Err(EngineError::Other(format!(
            "No options row ends the model within {MAX_MODEL_ROWS} cells of {}, so it isn't a model saved with Save Model.",
            start.a1()
        ))),
        last => {
            let gap = last.map_or(Some(start), |(c, _)| c.row.checked_add(1).map(|row| CellRef::new(row, c.col)));
            Err(EngineError::Other(match gap {
                Some(gap) => format!(
                    "{} holds no formula before the model's options row, so {} doesn't start a complete model saved with Save Model.",
                    gap.a1(),
                    start.a1()
                ),
                None => format!("{} doesn't start a complete model saved with Save Model.", start.a1()),
            }))
        }
    }
}

/// data.solverLoadModel: makes a model written by Save Model the active sheet's model.
pub(super) fn load_model_cells(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let active = d.wb.active_sheet;
    let text = str_param(p, "from").ok_or_else(|| bad(LOAD_CMD, "missing from (the saved model's range)"))?;
    let (sh, body) = split_sheet(text);
    if sh.is_some_and(|n| d.wb.sheet_index(&n) != Some(active)) {
        return Err(EngineError::Other("Load Model reads a model saved on the active sheet.".into()));
    }
    let r = RangeRef::parse(body.trim()).ok_or_else(|| bad(LOAD_CMD, format!("not a range: {text}")))?;
    let cap = MAX_MODEL_ROWS;
    let sheet = d.wb.sheet(active).ok_or(EngineError::NoDocument)?;
    let formula = |c: CellRef| sheet.cell(c).and_then(|x| x.formula).map(|f| f.text.clone());
    let rows: Vec<String> = if r.is_single() {
        let down = formulas_down(sheet, r.start);
        complete_from_cell(r.start, &down)?;
        down.into_iter().map(|(_, f)| f).collect()
    } else {
        if r.count() > cap as u64 {
            return Err(EngineError::Other(format!("The range is too large for a saved model (at most {cap} cells).")));
        }
        let rows = cells_with_formulas(r, formula)?;
        if !rows.last().is_some_and(|f| is_options_row(f)) {
            return Err(EngineError::Other(format!(
                "{} doesn't end with the model's options row, so it isn't a complete model saved with Save Model.",
                r.a1()
            )));
        }
        rows
    };
    let params = parse_model_formulas(&rows)?;
    let model = parse_model(s, &params)?;
    if model.sheet != active {
        return Err(EngineError::Other("Load Model reads a model of the active sheet.".into()));
    }
    edit(s, |cx| {
        save_model(&mut cx.wb, &model);
        Ok(())
    })?;
    Ok(json!({"model": saved_model(&s.doc()?.wb, active)}))
}

/// Whether a defined name belongs to the saved model of a sheet.
fn is_sheet_model_name(n: &DefinedName, sheet: usize) -> bool {
    n.scope == Some(sheet) && is_model_name(&n.name)
}

/// data.solverReset: Reset All removes the active sheet's model. A model the sheet reads from
/// workbook-level names (which other sheets may read too) is left in place and masked by an
/// empty sheet-level solver_opt, so no other sheet's model changes.
pub(super) fn reset_model(s: &mut Session, _p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let active = d.wb.active_sheet;
    let removed = d.wb.names.iter().filter(|n| is_sheet_model_name(n, active)).count();
    let mut after = d.wb.names.clone();
    after.retain(|n| !is_sheet_model_name(n, active));
    let masked = {
        let mut probe = (*d.wb).clone();
        probe.names = after;
        saved_model(&probe, active).is_some()
    };
    if removed > 0 || masked {
        edit(s, |cx| {
            cx.wb.names.retain(|n| !is_sheet_model_name(n, active));
            if masked {
                cx.wb.names.push(DefinedName {
                    name: "solver_opt".into(),
                    scope: Some(active),
                    formula: "\"\"".into(),
                    comment: String::new(),
                    hidden: true,
                });
            }
            Ok(())
        })?;
    }
    Ok(json!({"removed": removed, "masked": masked}))
}
