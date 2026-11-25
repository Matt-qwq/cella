//! Cell entry, selection, undo/redo, clipboard, clear, fill, find & replace.

use std::borrow::Cow;

use cella_core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value};
use cella_model::{Cell, Formula, Sheet, StyleId, Workbook};
use serde_json::{Value as Json, json};

use super::*;
use crate::selection::{current_region, jump};
use crate::{Clipboard, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!("cell.set", "Enter Cell", [], None, "{cell?: \"B2\", input: \"text, number or =formula\", sheet?, array?: bool}", has_doc, cell_set),
        cmd!(
            "range.setValues",
            "Set Values",
            [],
            None,
            "{range?: \"A1\", values: [[...]] (rows of numbers/strings/bools/null; strings starting with = are formulas), raw?: false (write every string as text, with no number, date or formula parsing)}",
            has_doc,
            range_set_values
        ),
        cmd!(
            "range.fill",
            "Fill Range With Input",
            [],
            Some("Ctrl+Enter"),
            "{range?, input}: enters the same input in every selected cell (relative formulas adjust)",
            has_doc,
            range_fill
        ),
        cmd!(noundo "selection.set", "Select", [], None, "{range: \"A1:B2,D4\" | cell, active?: \"A1\", sheet?}", has_doc, selection_set),
        cmd!(noundo "selection.move", "Move Selection", [], None, "{dr, dc, extend?: bool, jump?: bool (Ctrl+arrow), page?: bool}", has_doc, selection_move),
        cmd!(noundo "selection.next", "Next Cell in Selection", [], None, "{forward?: true, byRow?: false} (Enter/Tab inside a selection)", has_doc, selection_next),
        cmd!(
            noundo "selection.nextCorner",
            "Next Corner of Selection",
            [],
            Some("Ctrl+."),
            "{} (the active cell moves clockwise to the next corner of the selected area, which stays selected; Ctrl+.)",
            has_doc,
            selection_next_corner
        ),
        cmd!(noundo "edit.selectAll", "Select All", ["Edit"], Some("Cmd+A"), "{} (current region first, then the whole sheet)", has_doc, select_all),
        cmd!(noundo "selection.currentRegion", "Select Current Region", [], Some("Ctrl+Shift+8"), "{}", has_doc, select_region),
        cmd!(noundo "selection.row", "Select Entire Row", [], Some("Shift+Space"), "{}", has_doc, select_row),
        cmd!(noundo "selection.column", "Select Entire Column", [], Some("Ctrl+Space"), "{}", has_doc, select_col),
        cmd!(noundo "edit.goTo", "Go To…", ["Home", "Editing", "Find & Select"], Some("Ctrl+G"), "{reference: \"B5\" | \"Sheet2!A1:C3\" | name}", has_doc, go_to),
        cmd!(noundo "edit.goToSpecial", "Go To Special…", ["Home", "Editing", "Find & Select"], None, "{kind: blanks|constants|formulas|comments|lastCell|currentRegion|visible|conditionalFormats|dataValidation|errors|numbers|text}", has_doc, go_to_special),
        cmd!(noundo "edit.undo", "Undo", ["Edit"], Some("Cmd+Z"), "{steps?: 1}", can_undo, undo),
        cmd!(noundo "edit.redo", "Redo", ["Edit"], Some("Cmd+Y"), "{steps?: 1}", can_redo, redo),
        cmd!(noundo "edit.copy", "Copy", ["Home", "Clipboard"], Some("Cmd+C"), "{range?, html?: bool} → {range, text, html?} (html: a styled HTML table, only when requested and small enough)", has_doc, copy),
        cmd!(noundo "edit.cut", "Cut", ["Home", "Clipboard"], Some("Cmd+X"), "{range?, html?: bool} → {range, text, html?} (html: a styled HTML table, only when requested and small enough)", has_doc, cut),
        cmd!(
            "edit.paste",
            "Paste",
            ["Home", "Clipboard"],
            Some("Cmd+V"),
            "{at?: \"C3\", text?: \"tab-separated text from the system clipboard\", html?: \"the clipboard's HTML flavour; cells the source recorded as numbers (x:num, sdval) stay numbers\"}",
            has_doc,
            paste
        ),
        cmd!(
            "range.move",
            "Drag and Drop",
            [],
            None,
            "{range?, to: \"C3\", overwrite?: bool} (dragging the selection's border: a cut and paste that adjusts the references that pointed at the moved cells and asks before replacing data unless overwrite)",
            has_doc,
            range_move
        ),
        cmd!(
            "edit.pasteSpecial",
            "Paste Special…",
            ["Home", "Clipboard"],
            Some("Ctrl+Cmd+V"),
            "{what: all|formulas|values|formats|comments|validation|allExceptBorders|columnWidths|formulasAndNumberFormats|valuesAndNumberFormats, operation?: none|add|subtract|multiply|divide, skipBlanks?, transpose?, link?}",
            has_clipboard,
            paste_special
        ),
        cmd!(noundo "edit.clearClipboard", "Cancel Copy", [], Some("Escape"), "{}", has_doc, clear_clipboard),
        cmd!("edit.clearAll", "Clear All", ["Home", "Editing", "Clear"], None, "{range?}", has_doc, |s, p| clear(s, p, "all")),
        cmd!("edit.clearFormats", "Clear Formats", ["Home", "Editing", "Clear"], None, "{range?}", has_doc, |s, p| clear(s, p, "formats")),
        cmd!("edit.clearContents", "Clear Contents", ["Home", "Editing", "Clear"], Some("Delete"), "{range?}", has_doc, |s, p| clear(
            s, p, "contents"
        )),
        cmd!("edit.clearComments", "Clear Comments and Notes", ["Home", "Editing", "Clear"], None, "{range?}", has_doc, |s, p| clear(
            s, p, "comments"
        )),
        cmd!("edit.clearHyperlinks", "Clear Hyperlinks", ["Home", "Editing", "Clear"], None, "{range?}", has_doc, |s, p| clear(s, p, "hyperlinks")),
        cmd!("edit.fillDown", "Fill Down", ["Home", "Editing", "Fill"], Some("Cmd+D"), "{range?}", has_doc, |s, p| fill_dir(s, p, 1, 0)),
        cmd!("edit.fillRight", "Fill Right", ["Home", "Editing", "Fill"], Some("Cmd+R"), "{range?}", has_doc, |s, p| fill_dir(s, p, 0, 1)),
        cmd!("edit.fillUp", "Fill Up", ["Home", "Editing", "Fill"], None, "{range?}", has_doc, |s, p| fill_dir(s, p, -1, 0)),
        cmd!("edit.fillLeft", "Fill Left", ["Home", "Editing", "Fill"], None, "{range?}", has_doc, |s, p| fill_dir(s, p, 0, -1)),
        cmd!(
            "edit.autoFill",
            "AutoFill",
            [],
            None,
            "{source: \"A1:A2\", target: \"A1:A10\", mode?: series|alternate|copy|formats|values|extend}: the fill-handle drag (alternate = Ctrl+drag: a lone number counts up, any other series copies; values = Fill Without Formatting: the series, targets keep their formats; extend = Fill Series: the series, and a lone number counts up)",
            has_doc,
            auto_fill
        ),
        cmd!(
            "edit.fillSeries",
            "Series…",
            ["Home", "Editing", "Fill"],
            None,
            "{range?, direction?: columns|rows, type?: linear|growth|date|autofill, step?: 1, stop?, dateUnit?: day|weekday|month|year}",
            has_doc,
            fill_series
        ),
        cmd!("edit.flashFill", "Flash Fill", ["Home", "Editing", "Fill"], Some("Ctrl+E"), "{range?}", has_doc, flash_fill),
        cmd!(noundo "edit.find", "Find…", ["Home", "Editing", "Find & Select"], Some("Cmd+F"), "{what, matchCase?, wholeCell?, lookIn?: formulas|values, byColumns?, all?: bool, within?: sheet|workbook}", has_doc, find),
        cmd!(
            "edit.replace",
            "Replace…",
            ["Home", "Editing", "Find & Select"],
            Some("Ctrl+H"),
            "{what, with, matchCase?, wholeCell?, all?: true, within?: sheet|workbook}",
            has_doc,
            replace
        ),
        cmd!(
            "edit.formatPainter",
            "Format Painter",
            ["Home", "Clipboard"],
            None,
            "{sticky?: bool} — first call picks up the selection's formats; with {apply: \"C3:D4\"} pastes them",
            has_doc,
            format_painter
        ),
        cmd!(noundo "edit.beginEdit", "Edit Cell", [], Some("F2"), "{}", has_doc, begin_edit),
    ]
}

// ---------------------------------------------------------------- entry

/// What typing `input` into a cell produces (constant or formula, plus an automatic number
/// format). Inherits the destination's cell, row or column style, including its number
/// format, before parsing. Errors when the formula can't be parsed.
pub(crate) fn input_to_cell(
    input: &str,
    sheet: usize,
    at: CellRef,
    wb: &mut cella_model::Workbook,
    shapes: &mut cella_formula::ShapeTable,
) -> Result<Option<Cell>> {
    let style = wb.sheet(sheet).map(|sh| sh.style_id(at)).unwrap_or_default();
    if input.is_empty() {
        let c = Cell { value: Value::Empty, formula: None, style };
        return Ok(if c.is_blank() { None } else { Some(c) });
    }
    let fmt_is_text = wb.styles.get(style).num_fmt.as_str() == "@";
    if fmt_is_text {
        return Ok(Some(Cell { value: Value::text(input), formula: None, style }));
    }
    let is_formula = input.starts_with('=')
        || (input.len() > 1
            && (input.starts_with('+') || input.starts_with('-'))
            && cella_core::parse::parse_number_text(input).is_none()
            && input.chars().nth(1).is_some_and(|c| c.is_ascii_alphabetic() || c == '('));
    if is_formula {
        let body = input.strip_prefix('=').unwrap_or(input);
        let expr = wb.name_call_case(parse_typed_formula(body)?);
        check_arg_counts(&expr)?;
        let rewrite = cella_xlsx::needs_file_rewrite(&expr);
        let mut cell = Cell::formula(Formula::from_expr_at(expr, at, shapes, rewrite));
        cell.style = style;
        // Formulas whose result is a date/time get a format from functions like TODAY().
        if wb.styles.get(style).num_fmt.as_str() == "General" {
            let f = cell.formula.as_ref().map(|f| f.text_at(at).unwrap_or_default().to_ascii_uppercase()).unwrap_or_default();
            let auto = if f.starts_with("TODAY(")
                || f.starts_with("DATE(")
                || f.starts_with("EDATE(")
                || f.starts_with("EOMONTH(")
                || f.starts_with("WORKDAY(")
            {
                Some("m/d/yyyy")
            } else if f.starts_with("NOW(") {
                Some("m/d/yyyy h:mm")
            } else if f.starts_with("TIME(") {
                Some("h:mm AM/PM")
            } else {
                None
            };
            if let Some(code) = auto {
                cell.style = wb.styles.derive(style, |s| s.num_fmt = cella_model::NumFmt::new(code));
            }
        }
        return Ok(Some(cell));
    }
    let mut parsed = cella_core::parse::parse_input(input, wb.date_system);
    // Like Excel's automatic percent entry: a bare number of 1 or more typed into a cell already
    // formatted as a percentage is a percent (3 -> 3%); smaller numbers are taken as fractions.
    if parsed.format.is_none()
        && !input.contains('%')
        && wb.styles.get(style).num_fmt.as_str().ends_with('%')
        && let Value::Number(n) = parsed.value
        && n.abs() >= 1.0
    {
        parsed.value = Value::Number(n / 100.0);
    }
    let mut style = style;
    if let Some(code) = parsed.format {
        let cur = wb.styles.get(style).num_fmt.as_str().to_string();
        if cur == "General" {
            style = wb.styles.derive(style, |s| s.num_fmt = cella_model::NumFmt::new(code));
        }
    }
    // Wrap text automatically when the input has line breaks (Alt+Enter).
    if input.contains('\n') && !wb.styles.get(style).align.wrap {
        style = wb.styles.derive(style, |s| s.align.wrap = true);
    }
    Ok(Some(Cell { value: parsed.value, formula: None, style }))
}

/// Parses a formula as it is typed (without the `=`). Excel closes missing parentheses for you,
/// and refuses a formula that still doesn't parse.
pub(crate) fn parse_typed_formula(body: &str) -> Result<cella_formula::Expr> {
    // Excel closes missing parentheses for you.
    let parsed = cella_formula::parse_closing_parens(body, cella_formula::parse);
    parsed.map_err(|e| EngineError::Other(format!("There's a problem with this formula: {e}")))
}

/// Like Excel, a formula that calls a built-in function with too few or too many arguments is
/// refused when it's entered, rather than stored to show `#VALUE!`. Functions the registry doesn't
/// hold (special forms like LET, LAMBDA, OFFSET, INDIRECT, SUBTOTAL), defined LAMBDA names and
/// unknown functions are skipped.
pub(crate) fn check_arg_counts(expr: &cella_formula::Expr) -> Result<()> {
    let mut bad = None;
    expr.walk(&mut |e| {
        if bad.is_none()
            && let cella_formula::Expr::Call(name, args) = e
            && let Some(spec) = cella_functions::lookup(name)
            && !spec.accepts(args.len())
        {
            let few = args.len() < spec.min_args;
            bad = Some(format!("{} has too {} arguments. Use {}", spec.name, if few { "few" } else { "many" }, spec.signature));
        }
    });
    match bad {
        Some(msg) => Err(EngineError::Other(msg)),
        None => Ok(()),
    }
}

fn cell_set(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let at = match cell_param_on(s, p, "cell", sheet)? {
        Some(c) => c,
        None => s.doc()?.selection.active,
    };
    // Both spellings take a string, number or boolean, as typed (#697).
    let input = p.get("input").or_else(|| p.get("value")).map(json_to_input).ok_or_else(|| bad("cell.set", "missing `input` (or `value`)"))?;
    let array = bool_param(p, "array").unwrap_or(false);
    let d = s.doc()?;
    if let Some(sh) = d.wb.sheet(sheet)
        && sh.is_protected()
        && d.wb.styles.get(sh.style_id(at)).protection.locked
        && let Some(m) = locked_cell_error(sh, at, &d.unlocked_ranges)
    {
        return Err(EngineError::Other(m));
    }
    let active_sheet = s.doc()?.wb.active_sheet;
    edit(s, |cx| {
        if array {
            // Legacy Ctrl+Shift+Enter array formula over the selection, when the selection holds
            // the cell; a cell elsewhere (or on another sheet) gets a one-cell array.
            let selected = cx.sel.current();
            let range = if sheet == active_sheet && selected.contains(at) { selected } else { RangeRef::cell(at) };
            // A table header can't hold an array's value, and a table resizes under it.
            if !range.is_single() && cx.wb.sheet(sheet).is_some_and(|sh| sh.tables.iter().any(|t| t.range.intersects(&range))) {
                return Err(EngineError::Other("Multi-cell array formulas aren't allowed in tables.".into()));
            }
            // The array owns every cell of its range and its result is fitted to the range, but the
            // evaluator lays out at most `MAX_CELLS`. A larger range would be `#NUM!` and the cells
            // its range still covers would keep stale values that the save loses: the file says the
            // array owns them, so reading it back drops them (`sheet_read`'s legacy-array rule).
            // Excel refuses the entry too; an error here keeps every cell as it was.
            if range.count() > cella_calc::eval::MAX_CELLS {
                return Err(EngineError::Other(format!(
                    "An array formula can't cover more than {} cells. Select a smaller range.",
                    cella_calc::eval::MAX_CELLS
                )));
            }
            let mut shapes = cella_formula::ShapeTable::default();
            let mut cell = input_to_cell(&input, sheet, range.start, &mut cx.wb, &mut shapes)?.unwrap_or_default();
            if let Some(f) = cell.formula.as_mut() {
                std::sync::Arc::make_mut(f).array = Some(Box::new(range));
            }
            let sh = cx.sheet_mut(sheet)?;
            // The array takes the whole range: other cells in it lose their contents. Every cell
            // that loses something is a change the recalc must see — a formula reading part of the
            // range (say `=MAX(A3:E10)-MIN(A3:E10)`) otherwise keeps its stale value while a full
            // recalc, which walks the whole sheet, gives the right one. Touching only the anchor was
            // exactly that bug (found by the command-sequence fuzzer, seed 295).
            let mut cleared: Vec<CellRef> = Vec::new();
            for c in range.iter().skip(1).take(1_000_000) {
                if let Some(old) = sh.cells.get(c).filter(|x| !x.value.is_empty() || x.formula.is_some()) {
                    let style = old.style;
                    sh.set_cell(c, Cell { style, ..Cell::default() });
                    cleared.push(c);
                }
            }
            sh.set_cell(range.start, cell);
            cx.touch(sheet, range.start);
            for c in cleared {
                cx.touch(sheet, c);
            }
            return Ok(Json::Null);
        }
        let mut shapes = cella_formula::ShapeTable::default();
        let cell = input_to_cell(&input, sheet, at, &mut cx.wb, &mut shapes)?;
        let sh = cx.sheet_mut(sheet)?;
        match cell {
            Some(c) => sh.set_cell(at, c),
            None => {
                sh.remove_cell(at);
            }
        }
        cx.touch(sheet, at);
        Ok(Json::Null)
    })?;
    let v = s.doc()?.wb.sheet(sheet).map(|sh| sh.value(at)).unwrap_or_default();
    Ok(json!({"cell": at.a1(), "value": v.display()}))
}

pub(crate) fn json_to_input(v: &Json) -> String {
    match v {
        Json::Null => String::new(),
        Json::Bool(b) => {
            if *b {
                "TRUE".into()
            } else {
                "FALSE".into()
            }
        }
        Json::Number(n) => n.to_string(),
        Json::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Coarse attribution for `CELLA_PROFILE` on a bulk write path: what it spends building cells
/// (dispatch, per-cell style reads, typed-input parsing, formula rewriting) against what it spends
/// writing them to the store. The clock is read at row boundaries only — never per cell — and
/// never on wasm, where it would trap. Attribution stays coarse on purpose: a row that mixes
/// stored cells with a blank writes its tail through `set_cell` inside the build segment.
struct WriteProfile {
    /// How the line the profile prints is labelled (`setValues`, `paste`).
    label: &'static str,
    build_ms: f64,
    write_ms: f64,
    last: std::time::Instant,
}

impl WriteProfile {
    #[allow(clippy::disallowed_methods)]
    fn start(label: &'static str) -> Option<Self> {
        if cfg!(target_arch = "wasm32") || std::env::var_os("CELLA_PROFILE").is_none() {
            return None;
        }
        Some(Self { label, build_ms: 0.0, write_ms: 0.0, last: std::time::Instant::now() })
    }

    /// A row begins; the next segment is building its cells.
    #[allow(clippy::disallowed_methods)]
    fn row_started(&mut self) {
        self.last = std::time::Instant::now();
    }

    /// The row's cells are built; the next segment is the store write.
    #[allow(clippy::disallowed_methods)]
    fn built(&mut self) {
        let now = std::time::Instant::now();
        self.build_ms += now.duration_since(self.last).as_secs_f64() * 1000.0;
        self.last = now;
    }

    /// The row's cells are written.
    #[allow(clippy::disallowed_methods)]
    fn written(&mut self) {
        let now = std::time::Instant::now();
        self.write_ms += now.duration_since(self.last).as_secs_f64() * 1000.0;
        self.last = now;
    }

    fn print(&self, rows: usize, cells: usize) {
        eprintln!("{}: {rows} rows ({cells} cells), dispatch {:.1} ms, store write {:.1} ms", self.label, self.build_ms, self.write_ms);
    }
}

/// One pasted cell: written straight away, or buffered into the destination row it belongs to when
/// the paste walks rows left to right (`paste_special`'s `batch`), so the row reaches the store in
/// one call. A cell whose new picture is `None` buffers nothing for the picture buffer: the absence
/// is what the row's own write says, and eight million empty entries are a buffer the size of the
/// block.
fn write_cell(
    wb: &mut cella_model::Workbook,
    sheet: usize,
    dest: CellRef,
    new: Cell,
    picture: Option<std::sync::Arc<cella_model::CellPicture>>,
    batch: bool,
    buf: &mut Vec<(u32, Cell)>,
    pics: &mut Vec<(u32, std::sync::Arc<cella_model::CellPicture>)>,
) -> Result<()> {
    if batch {
        buf.push((dest.col, new));
        if let Some(picture) = picture {
            pics.push((dest.col, picture));
        }
        return Ok(());
    }
    let sh = wb.sheet_mut(sheet).ok_or_else(|| EngineError::Other("no such sheet".into()))?;
    sh.set_cell(dest, new);
    if let Some(picture) = picture {
        sh.cell_pictures.insert(dest, picture);
    }
    Ok(())
}

fn range_set_values(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let start = target_range(s, p)?.start;
    let Some(rows) = p.get("values").and_then(Json::as_array) else { return Err(bad("range.setValues", "missing `values` (array of rows)")) };
    if rows.len() > MAX_ROWS as usize {
        return Err(bad("range.setValues", "too many rows"));
    }
    let cells: u64 = rows.iter().map(|row| row.as_array().map_or(1, |a| a.len()) as u64).sum();
    super::check_bulk(cells)?;
    // A data import writes what the source holds: "2024-03-04" stays that text instead of
    // becoming a serial number, and "=A1" stays that text instead of becoming a formula.
    let raw = bool_param(p, "raw").unwrap_or(false);
    let mut count = 0;
    // The shapes this command interns: a formula string written into a thousand cells is one
    // shape when it spells the same references (an absolute one, or a plain `=$A$1*2`), where
    // every cell used to hold its own tree.
    let mut shapes = cella_formula::ShapeTable::default();
    // A data import is mostly rows: the store builds a whole row in one pass (set_row_cells),
    // while a cell at a time pays a row lookup, a band lookup and a growth step each. A sheet
    // holding a cell picture keeps the per-cell way, because set_cell owns that rule.
    let row_batches = cells >= 64;
    let mut profile = WriteProfile::start("setValues");
    edit(s, |cx| {
        let mut batch: Vec<(u32, Cell)> = Vec::new();
        // Every cell below is built here, so its style is in hand: a bulk import that keeps the
        // destination's plain style owes no row a fit (see `Ctx::fit_plain`). A cell removed
        // (a null) or built with a wrapping/large style turns the claim off.
        cx.fit_plain = true;
        for (ri, row) in rows.iter().enumerate() {
            if let Some(prof) = profile.as_mut() {
                prof.row_started();
            }
            let cols: &[Json] = match row.as_array() {
                Some(a) => a,
                None => std::slice::from_ref(row),
            };
            batch.clear();
            let mut bulk = row_batches && cx.wb.sheet(sheet).is_some_and(|sh| sh.cell_pictures.is_empty());
            for (ci, v) in cols.iter().enumerate() {
                let Some(at) = start.offset(ri as i64, ci as i64) else { continue };
                let cell = match v {
                    Json::Number(n) => {
                        let style = cx.wb.sheet(sheet).map(|sh| sh.style_id(at)).unwrap_or_default();
                        Some(Cell { value: Value::number(n.as_f64().unwrap_or(0.0)), formula: None, style })
                    }
                    Json::Bool(b) => {
                        let style = cx.wb.sheet(sheet).map(|sh| sh.style_id(at)).unwrap_or_default();
                        Some(Cell { value: Value::Bool(*b), formula: None, style })
                    }
                    Json::String(t) if raw => {
                        let style = cx.wb.sheet(sheet).map(|sh| sh.style_id(at)).unwrap_or_default();
                        Some(Cell { value: Value::text(t.as_str()), formula: None, style })
                    }
                    // Only a typed input can be blank (an empty string) or a formula, and only a
                    // typed input needs its text built: a number's own spelling is never used.
                    _ => input_to_cell(&json_to_input(v), sheet, at, &mut cx.wb, &mut shapes)?,
                };
                if cx.fit_plain {
                    cx.fit_plain = match cell.as_ref() {
                        Some(c) => {
                            let st = cx.wb.styles.get(c.style);
                            st.font.size <= cella_model::DEFAULT_FONT_SIZE && !st.align.wrap
                        }
                        None => false,
                    };
                }
                match cell {
                    Some(c) if bulk => batch.push((at.col, c)),
                    Some(c) => cx.sheet_mut(sheet)?.set_cell(at, c),
                    None => {
                        // Flush the prefix before switching to individual writes for this row.
                        let sh = cx.sheet_mut(sheet)?;
                        if bulk && !batch.is_empty() {
                            sh.set_row_cells(at.row, &mut batch);
                        }
                        bulk = false;
                        sh.remove_cell(at);
                    }
                }
                cx.touch(sheet, at);
                count += 1;
            }
            if let Some(prof) = profile.as_mut() {
                prof.built();
            }
            if bulk && !batch.is_empty() {
                let mut batch = std::mem::take(&mut batch);
                if let Some(r) = start.row.checked_add(ri as u32) {
                    cx.sheet_mut(sheet)?.set_row_cells(r, &mut batch);
                }
            }
            if let Some(prof) = profile.as_mut() {
                prof.written();
            }
        }
        Ok(())
    })?;
    if let Some(prof) = profile {
        prof.print(rows.len(), count);
    }
    Ok(json!({"cells": count}))
}

fn range_fill(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let ranges = target_ranges(s, p)?;
    let input = str_param(p, "input").unwrap_or("").to_string();
    let origin = s.doc()?.selection.active;
    edit(s, |cx| {
        // Each cell gets what typing the input there gives: the automatic number format where
        // it's General, its own format otherwise, text where it's Text. That depends only on
        // the cell's style, so the input is parsed once per style.
        // The input as it was entered, parsed once per style: every cell of the fill then shares
        // its shape and prints its own text from it.
        let mut by_style: std::collections::HashMap<StyleId, Option<Cell>> = std::collections::HashMap::new();
        let mut shapes = cella_formula::ShapeTable::default();
        for r in &ranges {
            if r.count() > 2_000_000 {
                return Err(bad("range.fill", "range too large"));
            }
            for at in r.iter() {
                let style = cx.wb.sheet(sheet).map(|sh| sh.style_id(at)).unwrap_or_default();
                let base = match by_style.get(&style) {
                    Some(c) => c.clone(),
                    None => {
                        let mut c = input_to_cell(&input, sheet, at, &mut cx.wb, &mut shapes)?;
                        // The input is the text typed at the *active* cell, and every other cell of
                        // the selection adjusts from there — so its shape is the one interned at
                        // `origin`, not at whichever cell happened to carry this style first.
                        if let Some(formula) = c.as_mut().and_then(|cell| cell.formula.as_ref()).cloned() {
                            let rewrite = formula.shape().is_some_and(|s| s.file_rewrite);
                            // The text as it was entered at `at`, re-anchored at the origin the
                            // rest of the fill adjusts from.
                            let text = formula.text_at(at).unwrap_or_default();
                            let entered = Formula::new_at(&text, origin, &mut shapes, rewrite);
                            if let Some(cell) = c.as_mut() {
                                cell.formula = Some(std::sync::Arc::new(entered));
                            }
                        }
                        by_style.insert(style, c.clone());
                        c
                    }
                };
                let mut cell = base.unwrap_or_default();
                // One shape for the whole fill: the destination shares it and only prints its own
                // text, unless a reference of it would fall off the sheet (then it is materialised
                // with the `#REF!` Excel shows, once for that cell).
                if let Some(f) = &cell.formula
                    && let Some(moved) = f.copied_to(origin, at)
                {
                    cell.formula = Some(std::sync::Arc::new(moved));
                }
                cx.sheet_mut(sheet)?.set_cell(at, cell);
                cx.touch(sheet, at);
            }
        }
        Ok(Json::Null)
    })
}

// ---------------------------------------------------------------- selection

/// Whether the cell may be selected: a protected sheet can forbid selecting locked and/or unlocked cells.
fn cell_selectable(wb: &cella_model::Workbook, sheet: &cella_model::Sheet, c: CellRef) -> bool {
    let Some(p) = sheet.protection.as_ref() else { return true };
    if wb.styles.get(sheet.style_id(c)).protection.locked { p.select_locked } else { p.select_unlocked }
}

fn selection_set(s: &mut Session, p: &Json) -> Result<Json> {
    if p.get("sheet").is_some() {
        let i = target_any_sheet(s, p)?;
        s.doc_mut()?.wb_switch_sheet(i);
    } else if let Some(r) = str_param(p, "range").or_else(|| str_param(p, "cell"))
        && let (Some(_), _) = split_sheet(r)
    {
        let i = target_any_sheet(s, p)?;
        s.doc_mut()?.wb_switch_sheet(i);
    }
    let ranges = target_ranges(s, p)?;
    let active = cell_param(s, p, "active")?;
    let d = s.doc_mut()?;
    let active = active.or_else(|| ranges.last().map(|r| r.start)).unwrap_or_default();
    let mut sel = crate::Selection { active, anchor: ranges.last().map(|r| r.start).unwrap_or(active), ranges };
    if let Some(sh) = d.wb.active() {
        sel.expand_merges(sh);
        if !cell_selectable(&d.wb, sh, sel.active) {
            return Err(EngineError::Other("The cell you are trying to change is on a protected sheet that does not allow selecting it.".into()));
        }
    }
    d.selection = sel;
    Ok(json!({"selection": d.selection.a1(), "active": d.selection.active.a1()}))
}

impl crate::DocState {
    /// Switches the active sheet, remembering each sheet's selection.
    pub fn wb_switch_sheet(&mut self, i: usize) {
        if i >= self.wb.sheets.len() || i == self.wb.active_sheet {
            return;
        }
        let cur = self.wb.active_sheet;
        if self.sheet_selections.len() < self.wb.sheets.len() {
            self.sheet_selections.resize(self.wb.sheets.len(), crate::Selection::default());
        }
        if let Some(slot) = self.sheet_selections.get_mut(cur) {
            *slot = self.selection.clone();
        }
        // The active sheet is view state: change it without making the workbook dirty.
        let dirty = self.is_dirty();
        let wb = std::sync::Arc::make_mut(&mut self.wb);
        wb.active_sheet = i;
        if !dirty {
            self.saved = self.wb.clone();
        }
        self.selection = self.sheet_selections.get(i).cloned().unwrap_or_default();
    }
}

fn selection_move(s: &mut Session, p: &Json) -> Result<Json> {
    let dr = p.get("dr").and_then(Json::as_i64).unwrap_or(0);
    let dc = p.get("dc").and_then(Json::as_i64).unwrap_or(0);
    let extend = bool_param(p, "extend").unwrap_or(false);
    let jmp = bool_param(p, "jump").unwrap_or(false);
    let d = s.doc_mut()?;
    let Some(sheet) = d.wb.active() else { return Err(EngineError::NoDocument) };
    let sel = &mut d.selection;
    // Extension moves the far corner, not the active cell.
    let from = if extend { sel.moving_end() } else { sel.active };
    // Skip hidden rows/columns and step over merged areas.
    let mut to = if jmp {
        jump(sheet, from, dr.signum(), dc.signum())
    } else {
        let mut t = from.offset_clamped(dr, dc);
        let mut guard = 0;
        while (sheet.is_row_hidden(t.row) || sheet.is_col_hidden(t.col)) && guard < 100_000 {
            let n = t.offset_clamped(dr.signum(), dc.signum());
            if n == t {
                break;
            }
            t = n;
            guard += 1;
        }
        if !extend && let Some(m) = sheet.merge_at(from) {
            // Leaving a merged cell moves from its edge.
            if dr > 0 {
                t = CellRef::new(m.end.offset_clamped(dr, 0).row, from.col);
            } else if dc > 0 {
                t = CellRef::new(from.row, m.end.offset_clamped(0, dc).col);
            }
        }
        t
    };
    if let Some(m) = sheet.merge_at(to)
        && !extend
    {
        to = m.start;
    }
    if !extend && !cell_selectable(&d.wb, sheet, to) {
        // Step on to the next selectable cell in the same direction; stay put if there is none.
        let (sr, sc) = (dr.signum(), dc.signum());
        let mut t = to;
        let mut found = false;
        if !jmp && (sr, sc) != (0, 0) {
            for _ in 0..10_000 {
                let n = t.offset_clamped(sr, sc);
                if n == t {
                    break;
                }
                t = n;
                if !sheet.is_row_hidden(t.row) && !sheet.is_col_hidden(t.col) && cell_selectable(&d.wb, sheet, t) {
                    found = true;
                    break;
                }
            }
        }
        if !found {
            return Ok(json!({"selection": sel.a1(), "active": sel.active.a1()}));
        }
        to = t;
    }
    if extend {
        sel.extend_to(to);
        sel.expand_merges(sheet);
    } else {
        *sel = crate::Selection::at(to);
        if let Some(m) = sheet.merge_at(to) {
            sel.ranges = vec![m];
        }
    }
    Ok(json!({"selection": sel.a1(), "active": sel.active.a1()}))
}

/// Ctrl+. (Next Corner of Selection): the active cell moves to the next corner of the selected
/// area, clockwise, and becomes the anchor. The corners that are in a hidden line, that the
/// sheet's protection doesn't let you select, or that lead out of the area are skipped.
fn selection_next_corner(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc_mut()?;
    let Some(sheet) = d.wb.active() else { return Err(EngineError::NoDocument) };
    let area = d.selection.current();
    let stop = |c: CellRef| {
        Some(sheet.merge_at(c).map_or(c, |m| m.start))
            .filter(|c| area.contains(*c) && !sheet.is_row_hidden(c.row) && !sheet.is_col_hidden(c.col) && cell_selectable(&d.wb, sheet, *c))
    };
    let mut next = d.selection.clone();
    next.next_corner_by(stop);
    d.selection = next;
    Ok(json!({"active": d.selection.active.a1(), "selection": d.selection.a1()}))
}

fn selection_next(s: &mut Session, p: &Json) -> Result<Json> {
    let fwd = bool_param(p, "forward").unwrap_or(true);
    let by_row = bool_param(p, "byRow").unwrap_or(false);
    let d = s.doc_mut()?;
    d.selection.cycle(fwd, by_row);
    Ok(json!({"active": d.selection.active.a1()}))
}

fn select_all(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc_mut()?;
    let Some(sheet) = d.wb.active() else { return Err(EngineError::NoDocument) };
    let region = current_region(sheet, d.selection.active);
    let all = RangeRef::all();
    let r = if region.is_single() || d.selection.current() == region { all } else { region };
    let active = d.selection.active;
    d.selection = crate::Selection { active, anchor: r.start, ranges: vec![r] };
    Ok(json!({"selection": d.selection.a1()}))
}

fn select_region(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc_mut()?;
    let Some(sheet) = d.wb.active() else { return Err(EngineError::NoDocument) };
    let r = current_region(sheet, d.selection.active);
    let active = d.selection.active;
    d.selection = crate::Selection { active, anchor: r.start, ranges: vec![r] };
    Ok(json!({"selection": d.selection.a1()}))
}

fn select_row(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc_mut()?;
    let r = d.selection.current();
    let active = d.selection.active;
    d.selection = crate::Selection { active, anchor: CellRef::new(r.start.row, 0), ranges: vec![RangeRef::rows(r.start.row, r.end.row)] };
    Ok(json!({"selection": d.selection.a1()}))
}

fn select_col(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc_mut()?;
    let r = d.selection.current();
    let active = d.selection.active;
    d.selection = crate::Selection { active, anchor: CellRef::new(0, r.start.col), ranges: vec![RangeRef::cols(r.start.col, r.end.col)] };
    Ok(json!({"selection": d.selection.a1()}))
}

pub(super) fn go_to(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(reference) = str_param(p, "reference").or_else(|| str_param(p, "range")) else { return Err(bad("edit.goTo", "missing `reference`")) };
    // A defined name or table?
    let d = s.doc()?;
    let resolved = if RangeRef::parse(split_sheet(reference).1).is_none() {
        let sheet = d.wb.active_sheet;
        if let Some(n) = d.wb.name(reference, sheet) {
            n.formula.clone()
        } else if let Some((si, ti)) = d.wb.table(reference) {
            // As in Excel, a table's name stands for its data, without the header and totals rows.
            let t = d.wb.sheet(si).and_then(|sh| sh.tables.get(ti)).map(|t| t.data_range().unwrap_or(t.range).a1()).unwrap_or_default();
            format!("{}!{}", cella_core::quote_sheet(&d.wb.sheet(si).map(|s| s.name.clone()).unwrap_or_default()), t)
        } else {
            return Err(EngineError::Other("Reference isn't valid.".into()));
        }
    } else {
        reference.to_string()
    };
    let resolved = resolved.replace('$', "");
    selection_set(s, &json!({"range": resolved}))
}

fn go_to_special(s: &mut Session, p: &Json) -> Result<Json> {
    let kind = str_param(p, "kind").unwrap_or("blanks");
    let d = s.doc()?;
    let Some(sheet) = d.wb.active() else { return Err(EngineError::NoDocument) };
    let scope = if d.selection.is_single_cell() { sheet.used_range().unwrap_or(RangeRef::cell(d.selection.active)) } else { d.selection.bounds() };
    if kind == "lastCell" {
        let c = sheet.used_range().map(|r| r.end).unwrap_or_default();
        return selection_set(s, &json!({"cell": c.a1()}));
    }
    if kind == "currentRegion" {
        return select_region(s, &json!({}));
    }
    let mut hits: Vec<CellRef> = Vec::new();
    if scope.count() > 4_000_000 {
        return Err(bad("edit.goToSpecial", "range too large"));
    }
    for c in scope.iter() {
        let cell = sheet.cell(c);
        let v = sheet.value(c);
        let ok = match kind {
            "blanks" => v.is_empty(),
            "constants" => cell.is_some_and(|x| x.formula.is_none() && !x.value.is_empty()),
            "formulas" => cell.is_some_and(|x| x.formula.is_some()),
            "comments" | "notes" => sheet.comments.contains_key(&c),
            "errors" => v.is_error(),
            "numbers" => v.is_number(),
            "text" => v.is_text(),
            "visible" => !sheet.is_row_hidden(c.row) && !sheet.is_col_hidden(c.col),
            "conditionalFormats" => sheet.cond_formats.iter().any(|cf| cf.ranges.iter().any(|r| r.contains(c))),
            "dataValidation" => sheet.validations.iter().any(|dv| dv.ranges.iter().any(|r| r.contains(c))),
            _ => return Err(bad("edit.goToSpecial", format!("unknown kind `{kind}`"))),
        };
        if ok {
            hits.push(c);
        }
    }
    if hits.is_empty() {
        return Err(EngineError::Other("No cells were found.".into()));
    }
    let ranges = coalesce(&hits);
    let d = s.doc_mut()?;
    d.selection = crate::Selection { active: hits[0], anchor: hits[0], ranges };
    Ok(json!({"selection": d.selection.a1(), "count": hits.len()}))
}

/// Merges cells into vertical runs per column (a compact multi-area selection).
fn coalesce(cells: &[CellRef]) -> Vec<RangeRef> {
    let mut sorted = cells.to_vec();
    sorted.sort_by_key(|c| (c.col, c.row));
    let mut out: Vec<RangeRef> = Vec::new();
    for c in sorted {
        if let Some(last) = out.last_mut()
            && last.start.col == c.col
            && last.end.col == c.col
            && last.end.row + 1 == c.row
        {
            last.end.row = c.row;
            continue;
        }
        out.push(RangeRef::cell(c));
    }
    if out.len() > 2000 {
        out.truncate(2000);
    }
    out
}

// ---------------------------------------------------------------- history

fn undo(s: &mut Session, p: &Json) -> Result<Json> {
    let steps = u32_param(p, "steps").unwrap_or(1).max(1);
    let d = s.doc_mut()?;
    let mut label = String::new();
    let now = d.wb.clone();
    // The formula cells the undone steps changed, when every one of them recorded them: a restore
    // that knows them re-inserts those nodes and reads the snapshot's formula for them alone — no
    // page of a spilled snapshot is decoded to find out what changed (see `Calc::restore_cells`).
    let mut cells: Option<Vec<(usize, cella_core::CellRef)>> = Some(Vec::new());
    for _ in 0..steps {
        let Some(mut e) = d.undo.pop() else { break };
        let cur_wb = d.wb.clone();
        // Redo goes back to the undone edit's cells, not wherever the user clicked before undoing (#1855).
        let cur_selection = e.selection.clone();
        label = e.label.clone();
        // A snapshot the byte budget spilled stays paged: it becomes the live workbook as it is,
        // and the pages are read as cells in them are read. Nothing is decoded here.
        let delta = e.formula_delta.take();
        match (cells.as_mut(), delta.as_ref()) {
            (Some(all), Some(delta)) => all.extend_from_slice(delta),
            _ => cells = None,
        }
        d.wb = e.wb;
        d.selection = e.selection;
        let (dropped, evicted) = crate::push_restored(&mut d.redo, label.clone(), cur_wb, &d.wb, cur_selection, delta);
        d.history_dropped += dropped as u64;
        d.defer_release(evicted);
    }
    super::extra::keep_watch_list(&now, &mut d.wb);
    // The snapshot carries the values the undone edit replaced, so nothing is recalculated: only
    // the graph the edit changed is put back (see `Calc::restore` and `Calc::restore_cells`).
    match cells {
        Some(cells) => d.calc.restore_cells(&d.wb, &cells),
        None => d.calc.restore(&now, &d.wb),
    };
    d.revision += 1;
    // An undone step can put an older model back (a query's load, a table removed), so the SQL
    // engine's copy of the model is stale from here.
    s.model_changed();
    Ok(json!({"undone": label}))
}

fn redo(s: &mut Session, p: &Json) -> Result<Json> {
    let steps = u32_param(p, "steps").unwrap_or(1).max(1);
    let d = s.doc_mut()?;
    let mut label = String::new();
    let now = d.wb.clone();
    let mut cells: Option<Vec<(usize, cella_core::CellRef)>> = Some(Vec::new());
    for _ in 0..steps {
        let Some(mut e) = d.redo.pop() else { break };
        let cur_wb = d.wb.clone();
        let cur_selection = d.selection.clone();
        label = e.label.clone();
        let delta = e.formula_delta.take();
        match (cells.as_mut(), delta.as_ref()) {
            (Some(all), Some(delta)) => all.extend_from_slice(delta),
            _ => cells = None,
        }
        d.wb = e.wb;
        d.selection = e.selection;
        let (dropped, evicted) = crate::push_restored(&mut d.undo, label.clone(), cur_wb, &d.wb, cur_selection, delta);
        d.history_dropped += dropped as u64;
        d.defer_release(evicted);
    }
    super::extra::keep_watch_list(&now, &mut d.wb);
    match cells {
        Some(cells) => d.calc.restore_cells(&d.wb, &cells),
        None => d.calc.restore(&now, &d.wb),
    };
    d.revision += 1;
    s.model_changed();
    Ok(json!({"redone": label}))
}

// ---------------------------------------------------------------- clipboard

fn copy_impl(s: &mut Session, p: &Json, cut: bool) -> Result<Json> {
    copy_impl_with(s, p, cut, true)
}

/// [`copy_impl`] with the tab-separated text under control. A move (`range_move`) cuts and pastes in
/// one command, so the text it would put on the system clipboard is never read — and building it
/// formats every cell of the block: seconds on a thousand-column sheet.
fn copy_impl_with(s: &mut Session, p: &Json, cut: bool, with_text: bool) -> Result<Json> {
    // The clock is read only off wasm, where `Instant::now` traps (the `cfg!` is a constant, so the
    // branch does not survive compilation there), and only when `CELLA_PROFILE` asks for it.
    let probe = !cfg!(target_arch = "wasm32") && std::env::var_os("CELLA_PROFILE").is_some();
    #[allow(clippy::disallowed_methods)]
    let t_copy_start = probe.then(std::time::Instant::now);
    let multiple = || EngineError::Other("This action won't work on multiple selections.".into());
    let sheet = target_sheet(s, p)?;
    let ranges = target_ranges(s, p)?;
    if ranges.len() > 1 && cut {
        return Err(multiple());
    }
    // The selection as made (untrimmed), for Insert Copied Cells.
    let selected = ranges.first().copied().unwrap_or_default();
    let d = s.doc()?;
    let Some(sh) = d.wb.sheet(sheet) else { return Err(EngineError::NoDocument) };
    let trim = |range: RangeRef| {
        if range.is_full_cols() || range.is_full_rows() {
            let used = sh.used_range().unwrap_or(RangeRef::cell(range.start));
            range.intersection(&used.union(&RangeRef::cell(range.start))).unwrap_or(range)
        } else {
            range
        }
    };
    let (wb, range, areas) = if ranges.len() > 1 {
        let areas: Vec<RangeRef> = ranges.iter().map(|r| trim(*r)).collect();
        let (wb, block) = slide_together(&d.wb, sheet, &areas).ok_or_else(multiple)?;
        (std::sync::Arc::new(wb), block, areas)
    } else {
        (d.wb.clone(), trim(ranges.first().copied().unwrap_or_default()), Vec::new())
    };
    #[allow(clippy::disallowed_methods)]
    let t_probe = probe.then(std::time::Instant::now);
    let text = if with_text { crate::display::range_text(&wb, sheet, range) } else { String::new() };
    if let (Some(t_probe), Some(t_copy_start)) = (t_probe, t_copy_start) {
        #[allow(clippy::disallowed_methods)]
        let (text_ms, all_ms) = (t_probe.elapsed().as_secs_f64() * 1000.0, t_copy_start.elapsed().as_secs_f64() * 1000.0);
        eprintln!("copy: text {text_ms:.0} ms, whole {all_ms:.0} ms, {} cells", range.count());
    }
    // The HTML rendering is for the host clipboard (UI); it can reach 4 MB, so agents and
    // scripts get it only when they ask for it.
    let html = if p.get("html").and_then(Json::as_bool) == Some(true) { crate::io::range_html(&wb, sheet, range, &text) } else { None };
    let clip = Clipboard { wb, sheet, range, cut, doc_uid: d.uid, text: text.clone(), areas, selected };
    s.clipboard = Some(clip);
    let mut result = json!({"range": range.a1(), "text": text});
    if let Some(html) = html {
        result["html"] = Json::String(html);
    }
    Ok(result)
}

/// Excel copies a multi-area selection only when the areas line up: all cover the same rows (they
/// slide together side by side, left to right) or all cover the same columns (stacked, top to
/// bottom), without overlapping. Returns a copy of the workbook whose sheet holds the areas packed
/// into one block at the first area's place, and that block. Formulas travel as their values, as
/// in Excel. `None` when the areas don't line up.
fn slide_together(wb: &cella_model::Workbook, sheet: usize, areas: &[RangeRef]) -> Option<(cella_model::Workbook, RangeRef)> {
    let first = *areas.first()?;
    let across = areas.iter().all(|a| a.start.row == first.start.row && a.end.row == first.end.row);
    let down = areas.iter().all(|a| a.start.col == first.start.col && a.end.col == first.end.col);
    let mut sorted = areas.to_vec();
    if across {
        sorted.sort_by_key(|a| a.start.col);
    } else if down {
        sorted.sort_by_key(|a| a.start.row);
    } else {
        return None;
    }
    if sorted.windows(2).any(|w| matches!(w, [a, b] if a.intersects(b))) {
        return None;
    }
    let anchor = sorted.first()?.start;
    let (h, w) = if across {
        (first.height(), sorted.iter().map(RangeRef::width).fold(0u32, u32::saturating_add))
    } else {
        (sorted.iter().map(RangeRef::height).fold(0u32, u32::saturating_add), first.width())
    };
    let block = RangeRef::new(anchor, anchor.offset(i64::from(h) - 1, i64::from(w) - 1)?);
    let src = wb.sheet(sheet)?;
    let mut out = wb.clone();
    let dst = out.sheet_mut(sheet)?;
    dst.take_cells(block);
    dst.spill_values.retain(|a, _| !block.contains(*a));
    dst.spill_ranges.retain(|c, _| !block.contains(*c));
    dst.comments.retain(|c, _| !block.contains(*c));
    dst.merges.retain(|m| !m.intersects(&block));
    if across {
        dst.cols.remove_range(block.start.col, block.width());
    } else {
        dst.rows.remove_range(block.start.row, block.height());
    }
    let mut next = if across { anchor.col } else { anchor.row };
    for a in &sorted {
        let (dr, dc) = if across { (0, i64::from(next) - i64::from(a.start.col)) } else { (i64::from(next) - i64::from(a.start.row), 0) };
        let cells = src
            .cells
            .iter_range(*a)
            .map(|(c, _)| c)
            .chain(src.spill_ranges.values().filter(|r| r.intersects(a)).flat_map(|r| r.iter()).filter(|c| a.contains(*c)));
        for c in cells {
            let Some(to) = c.offset(dr, dc) else { continue };
            let style = src.cell(c).map(|x| x.style).unwrap_or_default();
            dst.set_cell(to, Cell { value: src.value(c), formula: None, style });
            if let Some(picture) = src.cell_pictures.get(&c) {
                dst.cell_pictures.insert(to, picture.clone());
            }
        }
        for (c, comment) in src.comments.iter().filter(|(c, _)| a.contains(**c)) {
            if let Some(to) = c.offset(dr, dc) {
                dst.comments.insert(to, comment.clone());
            }
        }
        for m in src.merges.iter().filter(|m| a.contains_range(m)) {
            if let (Some(s), Some(e)) = (m.start.offset(dr, dc), m.end.offset(dr, dc)) {
                dst.merges.push(RangeRef::new(s, e));
            }
        }
        // Column widths (side by side) or row heights and hidden rows (stacked) follow their cells.
        if across {
            for (c, info) in src.cols.range(a.start.col..=a.end.col) {
                dst.cols.insert(next.saturating_add(c.saturating_sub(a.start.col)), *info);
            }
        } else {
            for (r, info) in src.rows.range(a.start.row..=a.end.row) {
                dst.rows.insert(next.saturating_add(r.saturating_sub(a.start.row)), *info);
            }
        }
        next = next.saturating_add(if across { a.width() } else { a.height() });
    }
    Some((out, block))
}

fn copy(s: &mut Session, p: &Json) -> Result<Json> {
    copy_impl(s, p, false)
}
fn cut(s: &mut Session, p: &Json) -> Result<Json> {
    copy_impl(s, p, true)
}
fn clear_clipboard(s: &mut Session, _: &Json) -> Result<Json> {
    s.clipboard = None;
    s.format_painter = None;
    ok()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum What {
    All,
    Formulas,
    Values,
    Formats,
    Comments,
    Validation,
    AllExceptBorders,
    ColumnWidths,
    FormulasAndNumberFormats,
    ValuesAndNumberFormats,
}

fn paste(s: &mut Session, p: &Json) -> Result<Json> {
    // External text (from the system clipboard) when it differs from ours.
    if let Some(text) = str_param(p, "text")
        && s.clipboard.as_ref().is_none_or(|c| c.text.trim_end_matches(['\r', '\n']) != text.trim_end_matches(['\r', '\n']))
    {
        return paste_text(s, p, text);
    }
    if s.clipboard.is_none() {
        return Err(EngineError::Other("Nothing to paste.".into()));
    }
    paste_special(s, &json!({"what": "all", "at": p.get("at").cloned().unwrap_or(Json::Null)}))
}

fn paste_text(s: &mut Session, p: &Json, text: &str) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let at = cell_param_on(s, p, "at", sheet)?.unwrap_or(s.doc()?.selection.current().start);
    let rows: Vec<Vec<String>> = parse_tsv(text);
    let h = rows.len();
    let w = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut shapes = cella_formula::ShapeTable::default();
    // The HTML flavour, when the host passed it: the number behind each cell's shown text. Read
    // only as far as the text's own shape, so spans in the markup can't expand without bound.
    let html = str_param(p, "html").map(|html| crate::clip_html::table_cells(html, h, w)).unwrap_or_default();
    edit(s, |cx| {
        for (ri, row) in rows.iter().enumerate() {
            for (ci, v) in row.iter().enumerate() {
                let Some(c) = at.offset(ri as i64, ci as i64) else { continue };
                let mut cell = input_to_cell(v, sheet, c, &mut cx.wb, &mut shapes).unwrap_or_else(|_| Some(Cell::value(Value::text(v.as_str()))));
                if let Some(cl) = cell.as_mut() {
                    numeric_from_html(cl, v, html.get(ri).and_then(|r| r.get(ci)), &mut cx.wb);
                }
                let sh = cx.sheet_mut(sheet)?;
                match cell {
                    Some(cell) => sh.set_cell(c, cell),
                    None => {
                        sh.remove_cell(c);
                    }
                }
                cx.touch(sheet, c);
            }
        }
        if h > 0 && w > 0 {
            let end = at.offset_clamped(h as i64 - 1, w as i64 - 1);
            *cx.sel = crate::Selection { active: at, anchor: at, ranges: vec![RangeRef::new(at, end)] };
        }
        Ok(())
    })?;
    Ok(json!({"rows": h, "cols": w}))
}

/// A pasted cell that came out as text ("Rp10,000.00" isn't en-US input) becomes the number the
/// source recorded for it in its HTML, with a format that shows the same text when one can be
/// inferred and the cell has none. The HTML cell must show the same text as the tab-separated
/// field, so HTML that belongs to another copy (or a table laid out differently) changes nothing.
fn numeric_from_html(cell: &mut Cell, field: &str, html: Option<&crate::clip_html::HtmlCell>, wb: &mut cella_model::Workbook) {
    let Some(hc) = html else { return };
    let Some(n) = hc.num else { return };
    if cell.formula.is_some() || !matches!(cell.value, Value::Text(_)) || wb.styles.get(cell.style).num_fmt.as_str() == "@" {
        return;
    }
    if crate::clip_html::normalized(&hc.text) != crate::clip_html::normalized(field) {
        return;
    }
    cell.value = Value::number(n);
    if wb.styles.get(cell.style).num_fmt.as_str() == "General"
        && let Some(code) = crate::clip_html::format_for(field, n)
    {
        cell.style = wb.styles.derive(cell.style, |s| s.num_fmt = cella_model::NumFmt::new(&code));
    }
}

/// Tab-separated text with Excel-style quoting.
pub(crate) fn parse_tsv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    let mut at_start = true;
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if at_start => {
                quoted = true;
                at_start = false;
            }
            '\t' => {
                row.push(std::mem::take(&mut field));
                at_start = true;
            }
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                at_start = true;
            }
            _ => {
                field.push(c);
                at_start = false;
            }
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// The parts of `outer` that `inner` does not cover, as rectangles (at most four).
///
/// A move's overwrite check asks whether the destination holds anything the source does not, and
/// the two overlap almost entirely: walking the whole destination to answer that was 1.3 s on a
/// thousand-column sheet, where the strips that actually differ are a few rows deep.
fn rect_difference(outer: RangeRef, inner: RangeRef) -> Vec<RangeRef> {
    let (r0, r1) = (outer.start.row, outer.end.row);
    let (c0, c1) = (outer.start.col, outer.end.col);
    let ir0 = inner.start.row.max(r0);
    let ir1 = inner.end.row.min(r1);
    let ic0 = inner.start.col.max(c0);
    let ic1 = inner.end.col.min(c1);
    if ir0 > ir1 || ic0 > ic1 {
        return vec![outer];
    }
    let mut out = Vec::new();
    if r0 < ir0 {
        out.push(RangeRef::new(CellRef::new(r0, c0), CellRef::new(ir0 - 1, c1)));
    }
    if ir1 < r1 {
        out.push(RangeRef::new(CellRef::new(ir1 + 1, c0), CellRef::new(r1, c1)));
    }
    if c0 < ic0 {
        out.push(RangeRef::new(CellRef::new(ir0, c0), CellRef::new(ir1, ic0 - 1)));
    }
    if ic1 < c1 {
        out.push(RangeRef::new(CellRef::new(ir0, ic1 + 1), CellRef::new(ir1, c1)));
    }
    out
}

/// Dragging the selection's border: a cut and paste, with the references that pointed at the
/// moved cells following them. It asks before replacing data — a programmatic caller that means
/// to replace says so with `overwrite: true`.
fn range_move(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let to = cell_param_on(s, p, "to", sheet)?.ok_or_else(|| bad("range.move", "missing `to`: the cell the range is dropped on"))?;
    let kept = s.clipboard.take();
    let probe = !cfg!(target_arch = "wasm32") && std::env::var_os("CELLA_PROFILE").is_some();
    #[allow(clippy::disallowed_methods)]
    let started = probe.then(std::time::Instant::now);
    let moved = copy_impl_with(s, p, true, false).and_then(|_| {
        let now = |at: Option<std::time::Instant>| at.map(|t| t.elapsed().as_secs_f64());
        let t_copy = now(started).unwrap_or_default();
        let src = s.clipboard.as_ref().map(|c| c.range).unwrap_or_default();
        let dest = to.offset(i64::from(src.height()) - 1, i64::from(src.width()) - 1).map(|end| RangeRef::new(to, end));
        if bool_param(p, "overwrite") != Some(true)
            && let Some(dest) = dest
            && let Some(sh) = s.doc()?.wb.sheet(sheet)
            && (rect_difference(dest, src).iter().any(|r| sh.cells.iter_range(*r).any(|(_, cell)| !cell.value.is_empty() || cell.formula.is_some()))
                || sh.spill_ranges.values().flat_map(|r| r.iter()).any(|c| dest.contains(c) && !src.contains(c)))
        {
            return Err(EngineError::Other("There's already data here. Pass `overwrite: true` to replace it.".into()));
        }
        let t_check = now(started).unwrap_or_default();
        // On the range's sheet, which `paste` would leave to the active one. The selection
        // belongs to the active sheet: it follows the moved cells only when they are on it.
        let selection = s.doc()?.selection.clone();
        let pasted = paste_special(s, &json!({"what": "all", "at": to.a1(), "sheet": sheet}))?;
        if let Some(now) = now(started) {
            eprintln!("move: copy {:.0} ms, check {:.0} ms, paste {:.0} ms", t_copy * 1000.0, (t_check - t_copy) * 1000.0, (now - t_check) * 1000.0);
        }
        if sheet != s.doc()?.wb.active_sheet {
            s.doc_mut()?.selection = selection;
        }
        Ok(pasted)
    });
    if moved.is_err() {
        s.clipboard = kept;
    }
    moved
}

fn paste_special(s: &mut Session, p: &Json) -> Result<Json> {
    let probe = !cfg!(target_arch = "wasm32") && std::env::var_os("CELLA_PROFILE").is_some();
    let Some(clip) = s.clipboard.clone() else { return Err(EngineError::Other("Nothing to paste.".into())) };
    let what = match str_param(p, "what").unwrap_or("all") {
        "all" => What::All,
        "formulas" => What::Formulas,
        "values" => What::Values,
        "formats" => What::Formats,
        "comments" | "notes" => What::Comments,
        "validation" => What::Validation,
        "allExceptBorders" => What::AllExceptBorders,
        "columnWidths" => What::ColumnWidths,
        "formulasAndNumberFormats" => What::FormulasAndNumberFormats,
        "valuesAndNumberFormats" => What::ValuesAndNumberFormats,
        other => return Err(bad("edit.pasteSpecial", format!("unknown `what`: {other}"))),
    };
    let op = str_param(p, "operation").unwrap_or("none").to_string();
    let skip_blanks = bool_param(p, "skipBlanks").unwrap_or(false);
    let transpose = bool_param(p, "transpose").unwrap_or(false);
    let link = bool_param(p, "link").unwrap_or(false);
    // The shapes this paste interns, for the cells a source shape cannot serve (a move to another
    // sheet qualifies its references, and a reference can clamp at the destination).
    let mut shapes = cella_formula::ShapeTable::default();
    if link && !clip.areas.is_empty() {
        return Err(EngineError::Other("This action won't work on multiple selections.".into()));
    }
    let dest_sheet = target_sheet(s, p)?;
    let sel = s.doc()?.selection.current();
    let at = cell_param_on(s, p, "at", dest_sheet)?.unwrap_or(sel.start);
    let src = clip.range;
    let (h, w) = if transpose { (src.width(), src.height()) } else { (src.height(), src.width()) };
    // Tile the copy over a larger destination that is a multiple of its size.
    let tile = sel.height() % h == 0
        && sel.width() % w == 0
        && !sel.is_single()
        && cell_param_on(s, p, "at", dest_sheet)?.is_none()
        && !sel.is_full_cols()
        && !sel.is_full_rows()
        && sel.count() <= 4_000_000;
    let (tiles_r, tiles_c) = if tile { (sel.height() / h, sel.width() / w) } else { (1, 1) };
    let same_doc = s.doc()?.uid == clip.doc_uid;
    let cut_move = clip.cut && same_doc && what == What::All;
    let src_wb = clip.wb.clone();
    let Some(src_sheet) = src_wb.sheet(clip.sheet) else { return Err(EngineError::Other("The copied sheet no longer exists.".into())) };
    let src_sheet_name = src_sheet.name.clone();
    // A sheet with no spill has no cell whose displayed value could differ from the value the cell
    // stores, so a paste need not read the same band twice for every cell.
    let src_has_spills = !src_sheet.spill_ranges.is_empty();
    if cut_move && at.offset((tiles_r * h).saturating_sub(1) as i64, (tiles_c * w).saturating_sub(1) as i64).is_none() {
        // Check the whole footprint before the source is emptied, so a move that overflows the grid loses nothing.
        return Err(EngineError::Other("The cut area doesn't fit at this position; nothing was moved.".into()));
    }
    edit(s, |cx| {
        // Styles from another workbook must be re-interned.
        let mut style_map = std::collections::HashMap::new();
        let mut map_style = |id: StyleId, wb: &mut cella_model::Workbook| -> StyleId {
            if same_doc {
                return id;
            }
            *style_map.entry(id).or_insert_with(|| wb.styles.intern(src_wb.styles.get(id).clone()))
        };
        // The cells a move took out of the source, in row-major order — the order the paste reads
        // them in, so a cursor walks both lists together. A `BTreeMap` keyed by address cost 8M
        // insertions to build and 8M lookups to read on a thousand-column sheet.
        let mut moved_cells: Vec<(CellRef, Cell)> = Vec::new();
        let mut moved_pictures = std::collections::BTreeMap::new();
        if cut_move {
            // Remove the source first (contents and formats), and fix references to it.
            let source = cx.sheet_mut(clip.sheet)?;
            moved_pictures = source.cell_pictures.iter().filter(|(c, _)| src.contains(**c)).map(|(c, p)| (*c, p.clone())).collect();
            #[allow(clippy::disallowed_methods)]
            let cut_started = probe.then(std::time::Instant::now);
            let taken = source.take_cells(src);
            if let Some(started) = cut_started {
                #[allow(clippy::disallowed_methods)]
                let ms = started.elapsed().as_secs_f64() * 1000.0;
                eprintln!("cut: take {ms:.0} ms, {} cells", taken.len());
            }
            // Notes, hyperlinks and merged areas sit beside the cells, and a cut takes them
            // with the block: the destination is written from the clipboard snapshot taken
            // before the cut, so removing them here loses nothing and leaves the old place
            // clean instead of leaving one merged block and one note behind.
            source.comments.retain(|c, _| !src.contains(*c));
            source.hyperlinks.retain(|c, _| !src.contains(*c));
            source.merges.retain(|m| !src.contains_range(m));
            for (c, _) in &taken {
                cx.touch(clip.sheet, *c);
            }
            moved_cells = taken;
        }
        let mut profile = WriteProfile::start("paste");
        // What a pasted cell can carry beside its value, asked once for the whole paste: a sheet
        // with no comment, hyperlink or validation rule on either side has nothing to look up and
        // nothing to clear, and asking once per cell was eight million answers to a question that
        // cannot change its shape mid-paste.
        let carry_notes = !src_sheet.comments.is_empty();
        let carry_links = !src_sheet.hyperlinks.is_empty();
        let carry_rules = !src_sheet.validations.is_empty() || cx.wb.sheet(dest_sheet).is_some_and(|sh| !sh.validations.is_empty());
        let takes_pictures = !moved_pictures.is_empty() || !src_sheet.cell_pictures.is_empty();
        // `all` with no operation writes every field of a cell from the source, so the destination's
        // own cell is never read. Decided once, not once per cell.
        let fresh_paste = what == What::All && op == "none" && !link;
        // A paste without a transpose writes each destination row left to right, so the row can go
        // to the store in one call: a row the cut emptied is then built in one pass, where a cell at
        // a time inserts into the middle of the band's stripes and shifts the rows after it.
        let batch = !transpose;
        let single_tile = tiles_r == 1 && tiles_c == 1;
        let mut buf: Vec<(u32, Cell)> = Vec::with_capacity(src.width() as usize);
        let mut pics: Vec<(u32, std::sync::Arc<cella_model::CellPicture>)> = Vec::new();
        for tr in 0..tiles_r {
            for tc in 0..tiles_c {
                // The paste reads the source row by row, so the cursor starts over for each tile.
                let mut cursor = 0usize;
                for r in 0..src.height() {
                    if let Some(prof) = profile.as_mut() {
                        prof.row_started();
                    }
                    // A sheet with no pictures anywhere — the ordinary sheet — has nothing to read
                    // for a cell's old picture, and asking its map eight million times is a tenth of
                    // the build. Checked once a row: the row's own writes only land at the flush.
                    let dest_pictures = cx.wb.sheet(dest_sheet).is_some_and(|sh| !sh.cell_pictures.is_empty());
                    for c in 0..src.width() {
                        let sc = CellRef::new(src.start.row + r, src.start.col + c);
                        let (dr, dc) = if transpose { (c, r) } else { (r, c) };
                        let Some(dest) = at.offset((tr * h + dr) as i64, (tc * w + dc) as i64) else { continue };
                        let src_cell = if cut_move {
                            while moved_cells.get(cursor).is_some_and(|(at, _)| *at < sc) {
                                cursor += 1;
                            }
                            // A single-tile paste reads each moved cell exactly once, so it can take
                            // it: a clone per cell is an `Arc` increment per text, and the block is
                            // eight million cells.
                            match moved_cells.get_mut(cursor) {
                                Some((at, cell)) if *at == sc => Some(if single_tile { std::mem::take(cell) } else { cell.clone() }),
                                _ => None,
                            }
                        } else {
                            src_sheet.cell(sc)
                        };
                        // The value a paste takes is the displayed one: with no spill on the source
                        // sheet that is the stored cell's own value, so the second read of the same
                        // band — one per pasted cell — is skipped. Only a values paste and
                        // `skipBlanks` read it: `all` writes the cell's own value below.
                        let needs_val = skip_blanks || matches!(what, What::Values | What::ValuesAndNumberFormats);
                        let src_val = if needs_val {
                            if src_has_spills { src_sheet.value(sc) } else { src_cell.as_ref().map_or(Value::Empty, |cell| cell.value.clone()) }
                        } else {
                            Value::Empty
                        };
                        let src_picture = if takes_pictures {
                            if cut_move { moved_pictures.get(&sc) } else { src_sheet.cell_pictures.get(&sc) }.cloned()
                        } else {
                            None
                        };
                        if skip_blanks && src_picture.is_none() && src_val.is_empty() && src_cell.as_ref().is_none_or(|x| x.formula.is_none()) {
                            continue;
                        }
                        // The destination's own cell has to be read only when the paste keeps part
                        // of it: `all` with no operation writes every field from the source, and
                        // asking the store for eight million cells to throw them away is a third of
                        // the dispatch on a wide block.
                        let old =
                            if fresh_paste { Cell::default() } else { cx.wb.sheet(dest_sheet).and_then(|sh| sh.cell(dest)).unwrap_or_default() };
                        let old_picture =
                            if dest_pictures { cx.wb.sheet(dest_sheet).and_then(|sh| sh.cell_pictures.get(&dest)).cloned() } else { None };
                        let mut new_picture = old_picture.clone();
                        let mut new = old.clone();
                        let mut shift = |cell: &Cell| -> Option<std::sync::Arc<Formula>> {
                            let f = cell.formula.as_ref()?;
                            if cut_move {
                                // A move rewrites its references below, workbook-wide; here the cell
                                // only takes the tree it reads, qualified when it leaves its sheet.
                                let e = f.expr_at(sc)?;
                                // A moved formula still reads cells on its original sheet. Qualify
                                // those references before the workbook-wide move adjustment below.
                                let e = if dest_sheet != clip.sheet {
                                    e.map(&mut |x| match x {
                                        cella_formula::Expr::Ref(mut r) if r.sheet == cella_formula::SheetSel::Current => {
                                            r.sheet = cella_formula::SheetSel::Named(src_sheet_name.clone());
                                            cella_formula::Expr::Ref(r)
                                        }
                                        other => other,
                                    })
                                } else {
                                    e
                                };
                                let rewrite = cella_xlsx::needs_file_rewrite(&e);
                                let mut moved = Formula::from_expr_at(e, dest, &mut shapes, rewrite);
                                // The array range moves with its formula.
                                let (dr, dc) = (dest.row as i64 - sc.row as i64, dest.col as i64 - sc.col as i64);
                                moved.array =
                                    f.array.as_deref().and_then(|a| Some(Box::new(RangeRef::new(a.start.offset(dr, dc)?, a.end.offset(dr, dc)?))));
                                moved.dynamic = f.dynamic;
                                return Some(std::sync::Arc::new(moved));
                            }
                            // A copy shares the source's shape when it fits the destination: one
                            // `Arc` and one print a cell, no tree built.
                            let mut moved = f.copied_to(sc, dest)?;
                            moved.dynamic = f.dynamic;
                            Some(std::sync::Arc::new(moved))
                        };
                        if link {
                            let sheet_prefix = if dest_sheet == clip.sheet && same_doc {
                                String::new()
                            } else {
                                format!("{}!", cella_formula::quote_sheet(&src_sheet_name))
                            };
                            // A plain sheet-qualified reference: no function, so no file spelling.
                            let link_text = format!("={sheet_prefix}{}", sc.a1());
                            new.formula = Some(std::sync::Arc::new(Formula::new_at(&link_text, dest, &mut shapes, false)));
                            new.value = Value::Empty;
                            new_picture = None;
                        } else {
                            let sc_cell = src_cell.unwrap_or_default();
                            match what {
                                What::All | What::AllExceptBorders => {
                                    new_picture = src_picture.clone();
                                    new.formula = shift(&sc_cell);
                                    let style = sc_cell.style;
                                    let st = map_style(style, &mut cx.wb);
                                    new.style = if what == What::AllExceptBorders {
                                        let borders = cx.wb.styles.get(old.style).borders;
                                        cx.wb.styles.derive(st, |s| s.borders = borders)
                                    } else {
                                        st
                                    };
                                    // The value is moved, not cloned: the source cell is not read
                                    // again.
                                    new.value = if new.formula.is_some() { Value::Empty } else { sc_cell.value };
                                }
                                What::Formulas | What::FormulasAndNumberFormats => {
                                    new_picture = src_picture.clone();
                                    new.formula = shift(&sc_cell);
                                    new.value = if new.formula.is_some() { Value::Empty } else { sc_cell.value.clone() };
                                    if what == What::FormulasAndNumberFormats {
                                        let nf = src_wb.styles.get(sc_cell.style).num_fmt.clone();
                                        new.style = cx.wb.styles.derive(old.style, |s| s.num_fmt = nf);
                                    }
                                }
                                What::Values | What::ValuesAndNumberFormats => {
                                    new_picture = src_picture.clone();
                                    new.formula = None;
                                    new.value = src_val.clone();
                                    if what == What::ValuesAndNumberFormats {
                                        let nf = src_wb.styles.get(sc_cell.style).num_fmt.clone();
                                        new.style = cx.wb.styles.derive(old.style, |s| s.num_fmt = nf);
                                    }
                                }
                                What::Formats => new.style = map_style(sc_cell.style, &mut cx.wb),
                                What::Comments | What::Validation | What::ColumnWidths => {}
                            }
                            // Operations combine numbers with what is already there.
                            if op != "none"
                                && matches!(what, What::All | What::Values | What::ValuesAndNumberFormats | What::Formulas)
                                && new.formula.is_none()
                            {
                                if old_picture.is_some() || new_picture.is_some() {
                                    new_picture = None;
                                    new.value = Value::Error(cella_core::CellError::Value);
                                }
                                let a = old.value.clone();
                                let b = new.value.clone();
                                // Numbers stored as text take part (multiplying by 1 turns them into
                                // numbers); other text and errors stay as they are.
                                if let (Ok(x), Ok(y)) = (a.to_number(), b.to_number())
                                    && (a.is_number() || a.is_empty() || a.is_text())
                                    && (b.is_number() || b.is_empty())
                                {
                                    new.value = match op.as_str() {
                                        "add" => Value::number(x + y),
                                        "subtract" => Value::number(x - y),
                                        "multiply" => Value::number(x * y),
                                        "divide" => {
                                            if y == 0.0 {
                                                Value::Error(cella_core::CellError::Div0)
                                            } else {
                                                Value::number(x / y)
                                            }
                                        }
                                        _ => b.clone(),
                                    };
                                } else if (a.is_text() || a.is_error()) && (b.is_number() || b.is_empty()) {
                                    new.value = a;
                                }
                                // A formula already there is kept and combined with the pasted number.
                                if let Some(f) = &old.formula
                                    && let Ok(y) = b.to_number()
                                    && (b.is_number() || b.is_empty())
                                    && let Some(sym) = match op.as_str() {
                                        "add" => Some('+'),
                                        "subtract" => Some('-'),
                                        "multiply" => Some('*'),
                                        "divide" => Some('/'),
                                        _ => None,
                                    }
                                {
                                    let text = format!("=({}){sym}{}", f.text_at(dest).unwrap_or_default(), cella_core::number_to_text(y));
                                    new.formula = Some(std::sync::Arc::new(Formula::new(&text)));
                                    new.value = Value::Empty;
                                }
                            }
                        }
                        // The copied cell's validation rule replaces whatever rule the destination had.
                        if carry_rules && !link && matches!(what, What::All | What::AllExceptBorders | What::Validation) {
                            let rule = src_sheet.validations.iter().find(|dv| dv.ranges.iter().any(|r| r.contains(sc))).cloned();
                            let sh = cx.sheet_mut(dest_sheet)?;
                            super::data::remove_validation_cells(&mut sh.validations, &[RangeRef::new(dest, dest)]);
                            if let Some(mut dv) = rule {
                                // Relative references move with the rule, as pasted formulas do.
                                let first = dv.ranges.first().map_or(sc, |r| r.start);
                                dv.f1 = super::data::move_rule_formula(&dv.f1, first, dest);
                                dv.f2 = dv.f2.map(|f| super::data::move_rule_formula(&f, first, dest));
                                dv.ranges = vec![RangeRef::new(dest, dest)];
                                sh.validations.push(dv);
                            }
                        }
                        if let Some(prof) = profile.as_mut() {
                            prof.built();
                        }
                        match what {
                            What::Comments => {
                                if let Some(cm) = src_sheet.comments.get(&sc).cloned() {
                                    cx.sheet_mut(dest_sheet)?.comments.insert(dest, cm);
                                }
                            }
                            What::ColumnWidths => {
                                if let Some(info) = src_sheet.cols.get(&sc.col).copied() {
                                    cx.sheet_mut(dest_sheet)?.cols.insert(dest.col, info);
                                }
                            }
                            What::All => {
                                if carry_notes && let Some(cm) = src_sheet.comments.get(&sc).cloned() {
                                    cx.sheet_mut(dest_sheet)?.comments.insert(dest, cm);
                                }
                                if carry_links && let Some(h) = src_sheet.hyperlinks.get(&sc).cloned() {
                                    cx.sheet_mut(dest_sheet)?.hyperlinks.insert(dest, h);
                                }
                                write_cell(&mut cx.wb, dest_sheet, dest, new, new_picture.take(), batch, &mut buf, &mut pics)?;
                            }
                            _ => write_cell(&mut cx.wb, dest_sheet, dest, new, new_picture.take(), batch, &mut buf, &mut pics)?,
                        }
                        if let Some(picture) = new_picture {
                            cx.sheet_mut(dest_sheet)?.cell_pictures.insert(dest, picture);
                        }
                        cx.touch(dest_sheet, dest);
                        if let Some(prof) = profile.as_mut() {
                            prof.written();
                        }
                    }
                    // One store call for the destination row: a row the cut emptied is built in one
                    // pass, where a cell at a time inserts into the middle of the band's stripes and
                    // shifts the rows after it.
                    if batch && !buf.is_empty() {
                        let row = at.row + (tr * h) + r;
                        let sh = cx.sheet_mut(dest_sheet)?;
                        if !sh.cell_pictures.is_empty() {
                            for (col, _) in &buf {
                                sh.cell_pictures.remove(&CellRef::new(row, *col));
                            }
                        }
                        for (col, picture) in pics.drain(..) {
                            sh.cell_pictures.insert(CellRef::new(row, col), picture);
                        }
                        sh.set_row_cells(row, &mut buf);
                        buf.clear();
                    }
                }
            }
        }
        if let Some(prof) = profile {
            let rows = (tiles_r as usize).saturating_mul(tiles_c as usize).saturating_mul(src.height() as usize);
            prof.print(rows, rows.saturating_mul(src.width() as usize));
        }
        // Merged areas inside the copied block travel with it.
        if matches!(what, What::All | What::Formats) && !transpose {
            let merges: Vec<RangeRef> = src_sheet.merges.iter().filter(|m| src.contains_range(m)).copied().collect();
            for m in merges {
                let dr = at.row as i64 - src.start.row as i64;
                let dc = at.col as i64 - src.start.col as i64;
                if let (Some(a), Some(b)) = (m.start.offset(dr, dc), m.end.offset(dr, dc)) {
                    let nm = RangeRef::new(a, b);
                    let sh = cx.sheet_mut(dest_sheet)?;
                    sh.merges.retain(|x| !x.intersects(&nm));
                    sh.merges.push(nm);
                }
            }
        }
        if cut_move {
            // References elsewhere to the moved block now point to its new place.
            let dest_name = cx.wb.sheet(dest_sheet).map(|s| s.name.clone()).unwrap_or_default();
            let e = if dest_sheet == clip.sheet {
                cella_formula::adjust::Edit::Move { from: src, to_row: at.row, to_col: at.col }
            } else {
                cella_formula::adjust::Edit::MoveToSheet { from: src, to_sheet: dest_name, to_row: at.row, to_col: at.col }
            };
            rewrite_all_formulas(&mut cx.wb, &src_sheet_name, &e);
            // A workbook that holds no formula has nothing to re-evaluate and no spill to lay out
            // again, and the changed list was skipped above (a move says so in one flag instead):
            // asking for a full recalculation would rebuild an empty graph over eight million cells.
            cx.structural = cx.wb.sheets.iter().any(|s| s.cells.has_formulas());
        }
        let end = at.offset_clamped((tiles_r * h) as i64 - 1, (tiles_c * w) as i64 - 1);
        *cx.sel = crate::Selection { active: at, anchor: at, ranges: vec![RangeRef::new(at, end)] };
        Ok(())
    })?;
    if cut_move {
        s.clipboard = None;
    }
    Ok(json!({"pasted": s.doc()?.selection.a1()}))
}

/// One rewrite per **shape**, and a print per cell whose text changes: the rename half of S5 of
/// `plan/SHAPE-SHARING.md`. `map` rewrites a formula's tree and returns `None` when it leaves it
/// alone. A rename maps names — a sheet, a table, a defined name — and never coordinates, so its
/// answer is the same for every cell a shape serves, and one tree rewrite serves all of them.
///
/// `map` is given the index of the sheet the formula lives on, because a name can resolve
/// differently there (a sheet-scoped name hiding a workbook one) and because a rename may apply to
/// some sheets only.
///
/// A cell whose formula has no shape yet (text alone) or one that does not resolve where it sits
/// (a fill that clamped a relative part) takes the per-cell path: its text is parsed, rewritten
/// and interned at the cell, as before.
pub(crate) fn rewrite_shapes(wb: &mut cella_model::Workbook, map: impl Fn(usize, cella_formula::Expr) -> Option<cella_formula::Expr>) {
    let mut shapes = cella_formula::ShapeTable::default();
    for si in 0..wb.sheets.len() {
        let keys: Vec<(CellRef, std::sync::Arc<Formula>)> =
            wb.sheets.get(si).map(|s| s.cells.formulas().map(|(c, f, _)| (c, f)).collect()).unwrap_or_default();
        if keys.is_empty() {
            continue;
        }
        // One rewritten tree per shape, interned once. `None` is "the map left this shape alone".
        let mut planned: std::collections::HashMap<usize, Option<std::sync::Arc<cella_formula::Shape>>> = std::collections::HashMap::new();
        let Some(sheet) = wb.sheet_mut(si) else { continue };
        for (c, f) in keys {
            let shape = f.shape.clone().filter(|s| s.fits(c));
            if let Some(shape) = shape {
                let key = std::sync::Arc::as_ptr(&shape) as usize;
                let slot = planned
                    .entry(key)
                    .or_insert_with(|| map(si, shape.tree().clone()).and_then(|tree| shape.rewritten(tree)).map(|s| shapes.intern(s)));
                let Some(new) = slot.clone() else { continue };
                let Some(text) = new.print_at(c) else { continue };
                if f.text_at(c).as_deref() == Some(text.as_str()) && f.shape.as_ref().is_some_and(|old| std::sync::Arc::ptr_eq(old, &new)) {
                    continue;
                }
                if let Some(mut cell) = sheet.cells.get(c) {
                    let nf = Formula { text: None, shape: Some(new), array: f.array.clone(), dynamic: f.dynamic };
                    cell.formula = Some(std::sync::Arc::new(nf));
                    sheet.cells.set(c, cell);
                }
                continue;
            }
            // Text alone, or a shape that does not resolve here: the cell's own parse.
            let Some(tree) = f.text_tree_at(c) else { continue };
            let Some(new) = map(si, tree) else { continue };
            let text = cella_formula::print(&new);
            if let Some(mut cell) = sheet.cells.get(c) {
                let rewrite = cella_xlsx::needs_file_rewrite(&new);
                let mut nf = Formula::from_expr_at(new, c, &mut shapes, rewrite);
                nf.array = f.array.clone();
                nf.dynamic = f.dynamic;
                if f.text_at(c).as_deref() != Some(text.as_str()) {
                    cell.formula = Some(std::sync::Arc::new(nf));
                    sheet.cells.set(c, cell);
                }
            }
        }
    }
}

/// The cell a formula sat at before `e` moved it, in the same terms as the store's shift: the
/// position the cell's text was written for, which is the one its shape resolves at. `None` when
/// the position has no predecessor (the line an insert opened) or the edit is one that moves a
/// block rather than the lines a shape is anchored to.
fn cell_before(c: CellRef, e: &cella_formula::adjust::Edit) -> Option<CellRef> {
    use cella_formula::adjust::{Axis, Edit};
    // A cut re-creates each moved formula at its destination (`from_expr_at`, which prints the
    // source's tree there), so a Move has no "cell before": the text at the cell is the text the
    // adjustment must read.
    // Cells inserted or deleted inside a range: the band moves, the rest of the sheet stays.
    if let Edit::InsertCells { axis, range } | Edit::DeleteCells { axis, range } = e {
        let insert = matches!(e, Edit::InsertCells { .. });
        let line = |v: u32, lo: u32, hi: u32, count: u32| -> Option<u32> {
            if insert {
                if v > hi {
                    v.checked_sub(count)
                } else if v < lo {
                    Some(v)
                } else {
                    None
                }
            } else if v >= lo {
                v.checked_add(count)
            } else {
                Some(v)
            }
        };
        return match axis {
            Axis::Rows if c.col >= range.start.col && c.col <= range.end.col => {
                line(c.row, range.start.row, range.end.row, range.height()).map(|row| CellRef::new(row, c.col))
            }
            Axis::Cols if c.row >= range.start.row && c.row <= range.end.row => {
                line(c.col, range.start.col, range.end.col, range.width()).map(|col| CellRef::new(c.row, col))
            }
            _ => Some(c),
        };
    }
    let (axis, at, count) = match e {
        Edit::Insert { axis, at, count } | Edit::Delete { axis, at, count } => (*axis, *at, *count),
        _ => return None,
    };
    let insert = matches!(e, Edit::Insert { .. });
    let (row, col) = (c.row, c.col);
    let v = if axis == Axis::Rows { row } else { col };
    let before = if insert {
        if v >= at.checked_add(count)? {
            v - count
        } else if v < at {
            v
        } else {
            return None;
        }
    } else if v >= at {
        v.checked_add(count)?
    } else {
        v
    };
    let max = if axis == Axis::Rows { MAX_ROWS } else { MAX_COLS };
    if before >= max {
        return None;
    }
    Some(if axis == Axis::Rows { CellRef::new(before, col) } else { CellRef::new(row, before) })
}

/// What a shape says a cell becomes after `e`, when the shape level can say it: the shape rewritten
/// **once** per (shape, whether the edit moves the cell) — S5 of `plan/SHAPE-SHARING.md` — and the
/// cell's own text printed at the cell.
///
/// `None` is "only this cell can say": it holds no shape yet, its shape does not resolve where it
/// now sits, the edit moved a block rather than lines, or one of its references straddles the edit
/// ([`Shape::straddles`]) so its offset must change with the cell. The caller then parses,
/// adjusts and interns that one cell, exactly as before.
fn form_by_shape(
    f: &Formula,
    c: CellRef,
    host: &str,
    target: &str,
    e: &cella_formula::adjust::Edit,
    shapes: &mut cella_formula::ShapeTable,
    planned: &mut std::collections::HashMap<(usize, bool), Option<std::sync::Arc<cella_formula::Shape>>>,
) -> Option<(String, std::sync::Arc<cella_formula::Shape>)> {
    use cella_formula::{Adjusted, adjust::CellMove};
    let shape = f.shape.clone()?;
    let old = cell_before(c, e)?;
    if shape.straddles(old, e) {
        return None;
    }
    let moves = old != c;
    let key = (std::sync::Arc::as_ptr(&shape) as usize, moves);
    let new = {
        let slot = planned.entry(key).or_insert_with(|| match shape.adjust(host, target, e, if moves { CellMove::Moves } else { CellMove::Stays }) {
            // Nothing of the shape moves: the cells keep the `Arc` they share, which is the whole
            // win — a million formulas of a fill keep one tree between them.
            Adjusted::Unchanged => Some(std::sync::Arc::clone(&shape)),
            Adjusted::Moved(s) => Some(shapes.intern(s)),
            Adjusted::PerCell => None,
        });
        slot.clone()?
    };
    let text = new.print_at(c)?;
    Some((text, new))
}

/// Applies a reference adjustment to every cell formula and defined name in the workbook.
///
/// The tree work is **one rewrite per shape** (S5 of `plan/SHAPE-SHARING.md`): the cells an edit
/// moves hold a handful of shapes, so a million formulas cost a handful of new trees and one print
/// per cell whose text really changed.
pub(crate) fn rewrite_all_formulas(wb: &mut cella_model::Workbook, target: &str, e: &cella_formula::adjust::Edit) {
    use cella_formula::adjust::Edit;
    // Rows and columns inserted or deleted on the target sheet move its legacy array ranges too
    // (a cut moves its arrays itself).
    let moves_arrays = matches!(e, Edit::Insert { .. } | Edit::Delete { .. } | Edit::InsertCells { .. } | Edit::DeleteCells { .. });
    // Every [`Edit`] variant moves cells: rows, columns or a block inserted or deleted, or a cut
    // pasted here or on another sheet. That is what the per-cell fallback needs to know: a shape is
    // a cache of (text, position), so a formula a cut moved is renamed onto the tree its text
    // spells even when that text does not change. The shape-level path (`form_by_shape`) reads the
    // cell's movement from the edit itself.
    let moves_cells = matches!(
        e,
        Edit::Insert { .. }
            | Edit::Delete { .. }
            | Edit::InsertCells { .. }
            | Edit::DeleteCells { .. }
            | Edit::Move { .. }
            | Edit::MoveToSheet { .. }
    );
    let mut shapes = cella_formula::ShapeTable::default();
    for si in 0..wb.sheets.len() {
        let host = wb.sheets.get(si).map(|s| s.name.clone()).unwrap_or_default();
        // The store yields formulas from its side tables, not by walking every cell: a
        // 10-million-cell sheet with no formulas must not make an insert walk 10 million cells.
        let keys: Vec<(CellRef, std::sync::Arc<Formula>)> =
            wb.sheets.get(si).map(|s| s.cells.formulas().map(|(c, f, _)| (c, f)).collect()).unwrap_or_default();
        if keys.is_empty() {
            continue;
        }
        let on_target = moves_arrays && host.eq_ignore_ascii_case(target);
        // One answer per shape — and per side of the edit, because a reference the edit leaves
        // alone is a line further away from a cell that moved (S5 of `plan/SHAPE-SHARING.md`).
        let mut planned: std::collections::HashMap<(usize, bool), Option<std::sync::Arc<cella_formula::Shape>>> = std::collections::HashMap::new();
        let Some(sheet) = wb.sheet_mut(si) else { continue };
        for (c, f) in keys {
            let array: Option<RangeRef> = match f.array.as_deref() {
                Some(a) if on_target => super::sheet::map_range(*a, e),
                a => a.copied(),
            };
            if let Some((text, shape)) = form_by_shape(&f, c, &host, target, e, &mut shapes, &mut planned) {
                let same_shape = f.shape.as_ref().is_some_and(|old| std::sync::Arc::ptr_eq(old, &shape));
                if f.text_at(c).as_deref() == Some(text.as_str()) && same_shape && array == f.array.as_deref().copied() {
                    continue;
                }
                if let Some(mut cell) = sheet.cells.get(c) {
                    let nf = Formula { text: None, shape: Some(shape), array: array.map(Box::new), dynamic: f.dynamic };
                    cell.formula = Some(std::sync::Arc::new(nf));
                    sheet.cells.set(c, cell);
                }
                continue;
            }
            // Only this cell can say (see `form_by_shape`): the cell's *text* spells the tree the
            // formula holds at the position it was written for, so the parse is the tree the edit
            // must adjust — this is the per-cell path S5 kept for what it cannot answer.
            // The text as the cell held it before the edit: for a cell a cut moved, its old
            // position (the shape prints the text it was written for, not the one its new
            // position implies), and for every other cell its own.
            let before = cell_before(c, e).unwrap_or(c);
            let Some(expr) = f.text_tree_at(before) else { continue };
            let new = cella_formula::adjust::adjust(expr, &host, target, e);
            let text = cella_formula::print(&new);
            if (f.text_at(c).as_deref() != Some(text.as_str()) || array != f.array.as_deref().copied() || (moves_cells && f.shape.is_some()))
                && let Some(mut cell) = sheet.cells.get(c)
            {
                // Interned at the cell it sits at now, so `expr_at` is this tree again.
                let rewrite = cella_xlsx::needs_file_rewrite(&new);
                let mut nf = Formula::from_expr_at(new, c, &mut shapes, rewrite);
                nf.array = array.map(Box::new);
                nf.dynamic = f.dynamic;
                cell.formula = Some(std::sync::Arc::new(nf));
                sheet.cells.set(c, cell);
            }
        }
    }
    for n in wb.names.iter_mut() {
        if let Ok(expr) = cella_formula::parse_name(&n.formula) {
            // Names are workbook-level: unqualified refs don't occur; qualified ones adjust.
            let new = cella_formula::adjust::adjust(expr, "\u{0}", target, e);
            n.formula = cella_formula::print(&new);
        }
    }
}

// ---------------------------------------------------------------- clear

fn clear(s: &mut Session, p: &Json, what: &str) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let ranges = target_ranges(s, p)?;
    // Delete on checkboxes works in two passes (as in Excel): checked boxes are unchecked first,
    // and only when every box in the selection is unchecked are the boxes removed.
    let uncheck_only = what == "contents" && {
        let d = s.doc()?;
        d.wb.sheet(sheet).is_some_and(|sh| {
            ranges.iter().any(|r| {
                r.intersection(&sh.used_range().unwrap_or(RangeRef::cell(r.start))).is_some_and(|r| {
                    sh.cells.iter_range(r).any(|(c, x)| x.value == Value::Bool(true) && super::extra::is_checkbox_cell(&d.wb, sh, c))
                })
            })
        })
    };
    edit(s, |cx| {
        // Whether any formula could read a format. `CELL("format")` and `CELL("protect")` do, and
        // the recalculation learns about a style change from the changed list — so a clear that
        // skips the list may only do it in a workbook with no formula at all.
        let formula_readers = cx.wb.sheets.iter().any(|s| s.cells.has_formulas());
        for wanted in &ranges {
            let Some(used) = cx.wb.sheet(sheet).and_then(|sh| sh.used_range()) else { continue };
            let Some(r) = wanted.intersection(&used) else { continue };
            // Whether the range spans every stored column: the store's run-level answers (a whole
            // row of styles, a whole row of cells) are only exact that way.
            let spans_columns = r.start.col <= used.start.col && r.end.col >= used.end.col;
            // Clear All over the whole used area: nothing survives, so the store is replaced rather
            // than emptied a cell at a time. A take per cell shifts every row of the band's side
            // tables, which is quadratic in a row's width: the thousand-column shape took 9.13 s
            // against 14 ms for a hundred sheets of the same eight million cells.
            let mut cells: Vec<(CellRef, Cell)> = Vec::new();
            if what == "all" && r == used {
                // Nothing survives, so the recalculation is told the sheet changed shape rather
                // than handed one address per stored cell: building that list was eight million
                // entries, and it walked the store to find them.
                let sh = cx.sheet_mut(sheet)?;
                sh.cell_pictures.retain(|c, _| !r.contains(*c));
                sh.cells = cella_model::CellStore::default();
                cx.structural = true;
            } else if what == "formats" && spans_columns && !formula_readers {
                // A row's styles are run-length encoded, so one row's runs collapse to a single
                // default run in one splice — where a cell-by-cell clear writes a thousand cells
                // through `put` (1.8 s over eight million cells on a thousand-column sheet). The
                // changed list is how the recalculation hears, so this path is taken only when no
                // formula can read a format (`CELL("format")`, `CELL("protect")`); a workbook that
                // has one keeps the cell-by-cell clear.
                cx.sheet_mut(sheet)?.cells.clear_styles_rows(r.start.row, r.end.row);
            } else {
                // A cell the clear would leave exactly as it is costs nothing to leave alone, and
                // the scan is the store's flat one (no per-band vector).
                let target = (what == "formats").then_some(StyleId::DEFAULT);
                cells = cx
                    .wb
                    .sheet(sheet)
                    .map(|sh| sh.cells.iter_scan(r).filter(|(_, x)| target.is_none_or(|t| x.style != t)).collect())
                    .unwrap_or_default();
                // Report every stored cell we touch, even beyond a million coordinates in a sparse
                // selection. Removed formula anchors must also reach calc so their spills disappear.
                cx.changed.extend(cells.iter().map(|(c, _)| (sheet, *c)));
            }
            if what == "contents" {
                for (_, cell) in cells.iter_mut() {
                    let checkbox = cx.wb.styles.get(cell.style).num_fmt.as_str() == "checkbox";
                    cell.formula = None;
                    if checkbox && uncheck_only {
                        cell.value = Value::Bool(false);
                    } else {
                        cell.value = Value::Empty;
                        if checkbox {
                            cell.style = cx.wb.styles.derive(cell.style, |st| st.num_fmt = cella_model::NumFmt::new("General"));
                        }
                    }
                }
            }
            let sh = cx.sheet_mut(sheet)?;
            for (c, mut cell) in cells {
                match what {
                    "all" => {
                        sh.remove_cell(c);
                    }
                    "contents" => {
                        sh.set_cell(c, cell);
                    }
                    "formats" => {
                        cell.style = StyleId::DEFAULT;
                        sh.cells.set(c, cell);
                    }
                    _ => {}
                }
            }
            if matches!(what, "all" | "comments") {
                sh.comments.retain(|c, _| !r.contains(*c));
            }
            if matches!(what, "all" | "hyperlinks") {
                sh.hyperlinks.retain(|c, _| !r.contains(*c));
            }
            if matches!(what, "all" | "formats") {
                let mut unmerged = false;
                sh.merges.retain(|m| {
                    let keep = !r.contains_range(m);
                    unmerged |= !keep;
                    keep
                });
                if unmerged {
                    // An empty merge can block a spill without storing any cells in this range.
                    // As in Unmerge, retry blocked formula anchors that could reach the freed area.
                    let reach = RangeRef::new(CellRef::new(0, 0), r.end);
                    let blocked: Vec<_> = sh
                        .cells
                        .iter_range(reach)
                        .filter(|(_, cell)| cell.formula.is_some() && cell.value == Value::Error(cella_core::CellError::Spill))
                        .map(|(c, _)| (sheet, c))
                        .collect();
                    cx.changed.extend(blocked);
                }
            }
        }
        Ok(Json::Null)
    })
}

// ---------------------------------------------------------------- fill

fn fill_dir(s: &mut Session, p: &Json, dr: i64, dc: i64) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let mut r = target_range(s, p)?;
    // Whole rows or columns fill only where the sheet has content (nothing on an empty sheet).
    if r.is_full_cols() || r.is_full_rows() {
        match s.doc()?.wb.sheet(sheet).and_then(|sh| content_in_lines(sh, r)) {
            Some(u) => r = u,
            None => return Ok(Json::Null),
        }
    }
    if r.count() > crate::fill::MAX_FILL_CELLS {
        return Err(bad("edit.fill", "range too large"));
    }
    // With one row selected, Fill Down copies from the row above (Excel behaviour).
    if dr == 1 && r.height() == 1 {
        r.start.row = r.start.row.saturating_sub(1);
    }
    if dc == 1 && r.width() == 1 {
        r.start.col = r.start.col.saturating_sub(1);
    }
    let src = if dr == 1 {
        RangeRef::new(r.start, CellRef::new(r.start.row, r.end.col))
    } else if dr == -1 {
        RangeRef::new(CellRef::new(r.end.row, r.start.col), r.end)
    } else if dc == 1 {
        RangeRef::new(r.start, CellRef::new(r.end.row, r.start.col))
    } else {
        RangeRef::new(CellRef::new(r.start.row, r.end.col), r.end)
    };
    edit(s, |cx| {
        crate::fill::fill(cx, sheet, src, r, crate::fill::FillMode::Copy)?;
        Ok(Json::Null)
    })
}

/// The content a whole-row/whole-column fill covers: the data in the lines being filled, not the
/// sheet's used-range box. The box is a bounding rectangle, so one cell at the bottom of another
/// column made `Fill Down` on two columns write a million rows (and, from the commit that added
/// the narrowing: "Fill Up and Fill Left ... filled up to row 1,048,576 even when the sheet had
/// data").
fn content_in_lines(sh: &crate::model::Sheet, r: RangeRef) -> Option<RangeRef> {
    let mut out: Option<RangeRef> = None;
    let mut add = |x: RangeRef| match out {
        Some(u) => out = Some(u.union(&x)),
        None => out = Some(x),
    };
    for (c, _) in sh.cells.iter_range(r) {
        add(RangeRef::cell(c));
    }
    // A spilled array's cells are content without being stored cells.
    for sr in sh.spill_ranges.values() {
        if let Some(x) = sr.intersection(&r) {
            add(x);
        }
    }
    out
}

fn auto_fill(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let src = str_param(p, "source").and_then(RangeRef::parse).map_or_else(|| target_range(s, p), Ok)?;
    let Some(target) = str_param(p, "target").and_then(RangeRef::parse) else { return Err(bad("edit.autoFill", "missing `target`")) };
    let mode = match str_param(p, "mode").unwrap_or("series") {
        "copy" => crate::fill::FillMode::Copy,
        "alternate" => crate::fill::FillMode::Alternate,
        "formats" => crate::fill::FillMode::Formats,
        "values" => crate::fill::FillMode::ValuesOnly,
        "extend" => crate::fill::FillMode::Extend,
        _ => crate::fill::FillMode::Series,
    };
    if target.count() > crate::fill::MAX_FILL_CELLS || src.count() > crate::fill::MAX_FILL_CELLS {
        return Err(bad("edit.autoFill", "range too large"));
    }
    // Excel refuses a target that doesn't hold the source in a corner. Such targets used to fill
    // every source lane across the whole target extent: billions of cells for whole rows (`2:3`)
    // into whole columns (`A:A`).
    if !crate::fill::holds_source_in_corner(src, target) {
        return Err(bad("edit.autoFill", "the target must contain the source in one of its corners"));
    }
    edit(s, |cx| {
        crate::fill::fill(cx, sheet, src, target, mode)?;
        *cx.sel = crate::Selection { active: src.start, anchor: src.start, ranges: vec![target.union(&src)] };
        Ok(Json::Null)
    })
}

fn fill_series(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let r = target_range(s, p)?;
    let rows = str_param(p, "direction").map(|d| d == "rows").unwrap_or(r.width() > r.height());
    let kind = str_param(p, "type").unwrap_or("linear").to_string();
    let step = f64_param(p, "step").unwrap_or(1.0);
    let stop = f64_param(p, "stop");
    let unit = str_param(p, "dateUnit").unwrap_or("day").to_string();
    edit(s, |cx| {
        crate::fill::series(cx, sheet, r, rows, &kind, step, stop, &unit)?;
        Ok(Json::Null)
    })
}

fn flash_fill(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let r = target_range(s, p)?;
    let n = edit(s, |cx| crate::fill::flash_fill(cx, sheet, r.start))?;
    if n == 0 {
        return Err(EngineError::Other(
            "We looked at all the data next to your selection and didn't see a pattern for filling in values for you.".into(),
        ));
    }
    Ok(json!({"filled": n}))
}

// ---------------------------------------------------------------- find & replace

/// One `what`, prepared once for a whole scan: lowered when the search ignores case, and the
/// wildcard pattern parsed once instead of once per cell. A scan calls [`hit`](Self::hit) on every
/// stored cell — eight million of them — so nothing here allocates per call.
struct Needle {
    /// `what` as given: what a case-sensitive search compares against.
    what: String,
    /// `what.to_lowercase()`: what a case-insensitive search compares against.
    lower: Option<String>,
    /// The wildcard pattern as `(char, escaped)`, wrapped in `*…*` when the whole cell is not
    /// required. `None` when `what` holds no `*` or `?`.
    pattern: Option<Vec<(char, bool)>>,
    whole: bool,
}

impl Needle {
    fn new(what: &str, case: bool, whole: bool) -> Needle {
        let lower = (!case).then(|| what.to_lowercase());
        let pattern = what.contains(['*', '?']).then(|| {
            let mut p = parse_pattern(lower.as_deref().unwrap_or(what));
            if !whole {
                p.insert(0, ('*', false));
                p.push(('*', false));
            }
            p
        });
        Needle { what: what.to_string(), lower, pattern, whole }
    }

    /// Whether `hay` matches: a substring, the whole cell, or a `*`/`?` pattern, ignoring case
    /// unless the search asked to match it.
    fn hit(&self, hay: &str) -> bool {
        if let Some(pattern) = &self.pattern {
            return match &self.lower {
                Some(_) => wildcard_chars(&hay.to_lowercase(), pattern),
                None => wildcard_chars(hay, pattern),
            };
        }
        match &self.lower {
            Some(needle) if self.whole => eq_ignore_case(hay, needle),
            Some(needle) => contains_ignore_case(hay, needle),
            None if self.whole => hay == self.what,
            None => hay.contains(self.what.as_str()),
        }
    }
}

/// `hay == needle` ignoring case. Two ASCII strings compare byte by byte; anything else lowers the
/// text, which is what ignoring case means for the rest of Unicode.
fn eq_ignore_case(hay: &str, needle: &str) -> bool {
    if hay.is_ascii() && needle.is_ascii() { hay.eq_ignore_ascii_case(needle) } else { hay.to_lowercase() == needle }
}

/// `hay` contains `needle`, ignoring case.
fn contains_ignore_case(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if hay.is_ascii() && needle.is_ascii() {
        let (h, n) = (hay.as_bytes(), needle.as_bytes());
        h.len() >= n.len() && h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
    } else {
        hay.to_lowercase().contains(needle)
    }
}

/// `*`/`?` wildcard match with `~` escapes.
pub(crate) fn wildcard(text: &str, pat: &str) -> bool {
    wildcard_chars(text, &parse_pattern(pat))
}

/// The pattern of [`wildcard`], parsed once: each entry is a character and whether `~` escaped it.
fn parse_pattern(pat: &str) -> Vec<(char, bool)> {
    let mut p = Vec::new();
    let mut it = pat.chars();
    while let Some(c) = it.next() {
        if c == '~' {
            if let Some(n) = it.next() {
                p.push((n, true));
            }
        } else {
            p.push((c, false));
        }
    }
    p
}

/// [`wildcard`] against a pattern that was already parsed.
fn wildcard_chars(text: &str, p: &[(char, bool)]) -> bool {
    let t: Vec<char> = text.chars().collect();
    let (mut ti, mut pi) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == ('?', false) || (p[pi].0 == t[ti] && p[pi] != ('*', false))) {
            ti += 1;
            pi += 1;
        } else if pi < p.len() && p[pi] == ('*', false) {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == ('*', false) {
        pi += 1;
    }
    pi == p.len()
}

/// Every stored cell of a sheet: the range `find` and `replace` walk.
fn whole_sheet() -> RangeRef {
    RangeRef::new(CellRef::new(0, 0), CellRef::new(MAX_ROWS - 1, MAX_COLS - 1))
}

/// The text a stored cell shows, for matching: the formula when formulas are what is searched and
/// the sheet shows them, else the value with the cell's number format applied.
///
/// A text value under a format that leaves text alone is **borrowed** rather than rebuilt. A scan
/// that rebuilds it allocates one string per cell — eight million for a full sheet, and most of what
/// `edit.find` used to spend its time on.
fn text_of<'a>(wb: &Workbook, sh: &Sheet, c: CellRef, cell: &'a Cell, values: bool) -> Cow<'a, str> {
    if !values
        && sh.show_formulas
        && let Some(f) = &cell.formula
        && !crate::display::formula_hidden(wb, sh, c)
    {
        return Cow::Owned(format!("={}", f.text_at(c).unwrap_or_default()));
    }
    let code = wb.styles.get(cell.style).num_fmt.as_str();
    // A format's fourth section is its text section (`@`); with fewer than four sections
    // `format_text` hands the text back unchanged, so the cell's own text is what it shows. Counting
    // `;` can only over-count (a quoted literal may hold one), which takes the slow path.
    if let Value::Text(t) = &cell.value
        && code.matches(';').count() < 3
    {
        return Cow::Borrowed(t);
    }
    // A stored cell with no value and no formula still displays a spilled value, which is what
    // `Sheet::value` adds.
    let v = if cell.value.is_empty() && cell.formula.is_none() { sh.value(c) } else { cell.value.clone() };
    Cow::Owned(crate::display::value_text_as(wb, &v, code))
}

/// Where a cell sits in the scan order: row-major (`row`, `col`), or `col` first when the search
/// walks columns.
fn scan_key(si: usize, c: CellRef, by_cols: bool) -> (usize, u32, u32) {
    if by_cols { (si, c.col, c.row) } else { (si, c.row, c.col) }
}

/// The first matching cell of `sh` in row-major order, inside `rows` and accepted by `keep`, with
/// the text it matched on. Stops at that cell: the caller wanted one hit, not a list.
fn first_hit(wb: &Workbook, sh: &Sheet, needle: &Needle, values: bool, rows: RangeRef, keep: impl Fn(CellRef) -> bool) -> Option<(CellRef, String)> {
    for (c, cell) in sh.cells.iter_scan(rows) {
        if !keep(c) {
            continue;
        }
        let text = text_of(wb, sh, c, &cell, values);
        if needle.hit(&text) {
            return Some((c, text.into_owned()));
        }
    }
    None
}

/// The matching cell of `sh` that comes first in column-major order — the smallest `(col, row)` that
/// `keep` accepts. A column-first scan cannot stop at the first hit it meets, so this walks the
/// sheet once and keeps the best cell rather than every match.
fn first_hit_by_column(wb: &Workbook, sh: &Sheet, needle: &Needle, values: bool, keep: impl Fn(CellRef) -> bool) -> Option<(CellRef, String)> {
    let mut best: Option<(CellRef, String)> = None;
    for (c, cell) in sh.cells.iter_scan(whole_sheet()) {
        if !keep(c) || best.as_ref().is_some_and(|(b, _)| (b.col, b.row) <= (c.col, c.row)) {
            continue;
        }
        let text = text_of(wb, sh, c, &cell, values);
        if needle.hit(&text) {
            best = Some((c, text.into_owned()));
        }
    }
    best
}

fn find(s: &mut Session, p: &Json) -> Result<Json> {
    let what = str_param(p, "what").unwrap_or("").to_string();
    if what.is_empty() {
        return Err(bad("edit.find", "missing `what`"));
    }
    let case = bool_param(p, "matchCase").unwrap_or(false);
    let whole = bool_param(p, "wholeCell").unwrap_or(false);
    let values = str_param(p, "lookIn") == Some("values");
    let by_cols = bool_param(p, "byColumns").unwrap_or(false);
    let all = bool_param(p, "all").unwrap_or(false);
    let workbook = str_param(p, "within") == Some("workbook");
    s.last_find = Some(p.clone());
    let needle = Needle::new(&what, case, whole);
    let d = s.doc()?;
    let sheets: Vec<usize> = if workbook { (0..d.wb.sheets.len()).collect() } else { vec![d.wb.active_sheet] };
    let cur = (d.wb.active_sheet, d.selection.active);
    if all {
        let mut hits: Vec<(usize, CellRef, String)> = Vec::new();
        for si in sheets.iter().copied() {
            let Some(sh) = d.wb.sheet(si) else { continue };
            let mut cells: Vec<(CellRef, String)> = Vec::new();
            for (c, cell) in sh.cells.iter_scan(whole_sheet()) {
                let text = text_of(&d.wb, sh, c, &cell, values);
                if needle.hit(&text) {
                    cells.push((c, text.into_owned()));
                }
            }
            if by_cols {
                cells.sort_by_key(|(c, _)| (c.col, c.row));
            }
            hits.extend(cells.into_iter().map(|(c, t)| (si, c, t)));
        }
        if hits.is_empty() {
            return Err(EngineError::Other(format!("We couldn't find what you were looking for: {what}")));
        }
        let list: Vec<Json> =
            hits.iter().map(|(si, c, t)| json!({"sheet": d.wb.sheet(*si).map(|s| s.name.clone()), "cell": c.a1(), "value": t})).collect();
        return Ok(json!({"count": list.len(), "results": list}));
    }
    // Find Next: the first match after the active cell, else the first one in the sheet — the wrap
    // Excel does. Only that one cell is built, so a word with a million matches costs the distance
    // to the first of them.
    let mut found: Option<(usize, CellRef, String)> = None;
    if by_cols {
        // A column-first scan reads the whole sheet either way, so one walk collects both the next
        // match and (for the wrap) the first one.
        let (mut after, mut any) = (None, None);
        for si in sheets.iter().copied() {
            let Some(sh) = d.wb.sheet(si) else { continue };
            if after.is_none() {
                after = first_hit_by_column(&d.wb, sh, &needle, values, |c| scan_key(si, c, true) > scan_key(cur.0, cur.1, true))
                    .map(|(c, t)| (si, c, t));
            }
            if any.is_none() {
                any = first_hit_by_column(&d.wb, sh, &needle, values, |_| true).map(|(c, t)| (si, c, t));
            }
            if after.is_some() {
                break;
            }
        }
        found = after.or(any);
    } else {
        // Row-major: walk the rows from the active cell down, then the rows above it.
        for si in sheets.iter().copied() {
            if si < cur.0 {
                continue;
            }
            let Some(sh) = d.wb.sheet(si) else { continue };
            let hit = if si == cur.0 {
                let rows = RangeRef::new(CellRef::new(cur.1.row, 0), CellRef::new(MAX_ROWS - 1, MAX_COLS - 1));
                first_hit(&d.wb, sh, &needle, values, rows, |c| (c.row, c.col) > (cur.1.row, cur.1.col))
            } else {
                first_hit(&d.wb, sh, &needle, values, whole_sheet(), |_| true)
            };
            if let Some((c, t)) = hit {
                found = Some((si, c, t));
                break;
            }
        }
        if found.is_none() {
            for si in sheets.iter().copied() {
                if si > cur.0 {
                    break;
                }
                let Some(sh) = d.wb.sheet(si) else { continue };
                let hit = if si == cur.0 {
                    let rows = RangeRef::new(CellRef::new(0, 0), CellRef::new(cur.1.row, MAX_COLS - 1));
                    first_hit(&d.wb, sh, &needle, values, rows, |c| (c.row, c.col) <= (cur.1.row, cur.1.col))
                } else {
                    first_hit(&d.wb, sh, &needle, values, whole_sheet(), |_| true)
                };
                if let Some((c, t)) = hit {
                    found = Some((si, c, t));
                    break;
                }
            }
        }
    }
    let Some((si, c, t)) = found else {
        return Err(EngineError::Other(format!("We couldn't find what you were looking for: {what}")));
    };
    let dd = s.doc_mut()?;
    dd.wb_switch_sheet(si);
    dd.selection = crate::Selection::at(c);
    Ok(json!({"cell": c.a1(), "sheet": si, "value": t}))
}

fn replace(s: &mut Session, p: &Json) -> Result<Json> {
    let what = str_param(p, "what").unwrap_or("").to_string();
    let with = str_param(p, "with").unwrap_or("").to_string();
    if what.is_empty() {
        return Err(bad("edit.replace", "missing `what`"));
    }
    let case = bool_param(p, "matchCase").unwrap_or(false);
    let whole = bool_param(p, "wholeCell").unwrap_or(false);
    let all = bool_param(p, "all").unwrap_or(true);
    let workbook = str_param(p, "within") == Some("workbook");
    let active = s.doc()?.selection.active;
    let needle = Needle::new(&what, case, whole);
    let n = edit(s, |cx| {
        let sheets: Vec<usize> = if workbook { (0..cx.wb.sheets.len()).collect() } else { vec![cx.wb.active_sheet] };
        let mut shapes = cella_formula::ShapeTable::default();
        let mut n = 0;
        for si in sheets {
            let cells: Vec<(CellRef, String)> =
                cx.wb.sheet(si).map(|sh| sh.cells.iter().map(|(c, _)| (c, sh.input_text(c))).collect()).unwrap_or_default();
            for (c, text) in cells {
                if !all && c != active {
                    continue;
                }
                if !needle.hit(&text) {
                    continue;
                }
                let new = if whole {
                    with.clone()
                } else if case {
                    text.replace(&what, &with)
                } else {
                    replace_ci(&text, &what, &with)
                };
                let cell = input_to_cell(&new, si, c, &mut cx.wb, &mut shapes).unwrap_or_else(|_| Some(Cell::value(Value::text(new.as_str()))));
                let sh = cx.sheet_mut(si)?;
                match cell {
                    Some(cell) => sh.set_cell(c, cell),
                    None => {
                        sh.remove_cell(c);
                    }
                }
                cx.touch(si, c);
                n += 1;
            }
        }
        Ok(n)
    })?;
    Ok(json!({"replaced": n}))
}

/// Whether case-folding `s` maps every char to exactly one char of the same UTF-8 length, so
/// byte positions in the folded text still line up with the original's character boundaries.
fn folds_in_place(s: &str) -> bool {
    s.chars().all(|c| {
        let mut folded = c.to_lowercase();
        let first = folded.next();
        folded.next().is_none() && first.is_some_and(|l| l.len_utf8() == c.len_utf8())
    })
}

fn replace_ci(text: &str, what: &str, with: &str) -> String {
    let lower = text.to_lowercase();
    let w = what.to_lowercase();
    // Slicing `text` at positions found in `lower` is only sound when folding kept every byte
    // boundary: a length check alone misses foldings that keep the total length while shifting
    // boundaries (U+212A KELVIN SIGN shrinks 3→1 bytes, U+0130 grows 2→3), where the tail used
    // to be dropped. Those fall back to the case-sensitive replace, as length changes already do.
    if lower.len() != text.len() || w.is_empty() || !folds_in_place(text) || !folds_in_place(what) {
        return text.replace(what, with);
    }
    let mut out = String::new();
    let mut i = 0;
    while let Some(pos) = lower.get(i..).and_then(|s| s.find(&w)) {
        out.push_str(text.get(i..i + pos).unwrap_or(""));
        out.push_str(with);
        i += pos + w.len();
    }
    out.push_str(text.get(i..).unwrap_or(""));
    out
}

// ---------------------------------------------------------------- format painter

fn format_painter(s: &mut Session, p: &Json) -> Result<Json> {
    if let Some(dest) = str_param(p, "apply").and_then(RangeRef::parse) {
        let Some((wb, sheet, src, sticky)) = s.format_painter.clone() else { return Err(EngineError::Other("Pick up a format first.".into())) };
        let dest_sheet = s.doc()?.wb.active_sheet;
        let same = std::sync::Arc::ptr_eq(&wb, &s.doc()?.wb) || s.doc()?.wb.styles == wb.styles;
        edit(s, |cx| {
            let Some(src_sheet) = wb.sheet(sheet) else { return Ok(()) };
            let (h, w) = (src.height(), src.width());
            let dest = if dest.is_single() { RangeRef::new(dest.start, dest.start.offset_clamped(h as i64 - 1, w as i64 - 1)) } else { dest };
            // Whole rows or columns take a line format, as the ribbon's formatting commands give
            // them, instead of a style on each of their cells (a column alone has a million). The
            // line format comes from the first cell of the source pattern across the line; cells
            // already stored follow the pattern. Row heights and column widths stay as they are.
            let rows = dest.is_full_rows() && !dest.is_full_cols();
            let cols = dest.is_full_cols() && !dest.is_full_rows();
            let src_full = (rows && src.is_full_rows()) || (cols && src.is_full_cols());
            if rows || cols {
                let (first, src_first, n) = if rows { (dest.start.row, src.start.row, h) } else { (dest.start.col, src.start.col, w) };
                let lines = if rows { dest.start.row..=dest.end.row } else { dest.start.col..=dest.end.col };
                for line in lines {
                    let sl = src_first + (line - first) % n;
                    // The source cell for position `k` along the line.
                    let at = |k: u32| if rows { CellRef::new(sl, src.start.col + k % w) } else { CellRef::new(src.start.row + k % h, sl) };
                    let line_style = if src_full {
                        if rows { src_sheet.rows.get(&sl) } else { src_sheet.cols.get(&sl) }.and_then(|i| i.style)
                    } else {
                        Some(src_sheet.style_id(at(0)))
                    };
                    let line_style = line_style.map(|st| if same { st } else { cx.wb.styles.intern(wb.styles.get(st).clone()) });
                    let (dest_line, src_line) = if rows {
                        (RangeRef::rows(line, line), RangeRef::rows(sl, sl))
                    } else {
                        (RangeRef::cols(line, line), RangeRef::cols(sl, sl))
                    };
                    let mut cells: Vec<CellRef> =
                        cx.wb.sheet(dest_sheet).map(|sh| sh.cells.iter_range(dest_line).map(|(c, _)| c).collect()).unwrap_or_default();
                    if src_full {
                        cells.extend(
                            src_sheet
                                .cells
                                .iter_range(src_line)
                                .map(|(c, _)| if rows { CellRef::new(line, c.col) } else { CellRef::new(c.row, line) }),
                        );
                    }
                    if cols {
                        // A row format outranks a column format: the column's cells in formatted
                        // rows need a style of their own.
                        cells.extend(
                            cx.wb
                                .sheet(dest_sheet)
                                .into_iter()
                                .flat_map(|sh| sh.rows.iter().filter(|(_, i)| i.style.is_some()).map(|(r, _)| CellRef::new(r, line))),
                        );
                    }
                    let sh = cx.sheet_mut(dest_sheet)?;
                    (if rows { &mut sh.rows } else { &mut sh.cols }).entry(line).or_default().style = line_style;
                    for c in cells {
                        let st = src_sheet.style_id(at(if rows { c.col } else { c.row }));
                        let st = if same { st } else { cx.wb.styles.intern(wb.styles.get(st).clone()) };
                        cx.sheet_mut(dest_sheet)?.set_style(c, st);
                        cx.touch(dest_sheet, c);
                    }
                }
                *cx.sel = crate::Selection::range(dest);
                return Ok(());
            }
            for c in dest.iter().take(2_000_000) {
                let sc = CellRef::new(src.start.row + (c.row - dest.start.row) % h, src.start.col + (c.col - dest.start.col) % w);
                let st = src_sheet.style_id(sc);
                let st = if same { st } else { cx.wb.styles.intern(wb.styles.get(st).clone()) };
                cx.sheet_mut(dest_sheet)?.set_style(c, st);
                cx.touch(dest_sheet, c);
            }
            *cx.sel = crate::Selection::range(dest);
            Ok(())
        })?;
        if !sticky {
            s.format_painter = None;
        }
        return ok();
    }
    let sticky = bool_param(p, "sticky").unwrap_or(false);
    let d = s.doc()?;
    s.format_painter = Some((d.wb.clone(), d.wb.active_sheet, d.selection.current(), sticky));
    ok()
}

fn begin_edit(s: &mut Session, _: &Json) -> Result<Json> {
    s.ui_requests.push(crate::UiRequest::EditCell(None));
    ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A formula typed in the engine learns how a file spells it *when its shape is interned*: the
    /// `_xlfn.` table lives one crate above the model, so a shape interned without it would be saved
    /// under the model's own spelling — `XLOOKUP(...)` where Excel reads `_xlfn.XLOOKUP(...)` and
    /// repairs the file (SHAPE-SHARING.md §5.2 S3).
    #[test]
    fn a_typed_formula_knows_how_a_file_spells_it() {
        let mut s = Session::new();
        s.new_workbook();
        for (cell, input, rewrite) in
            [("A1", "=XLOOKUP(1,B:B,C:C)", true), ("A2", "=LET(x,1,x+1)", true), ("A3", "=B3*C3+D3", false), ("A4", "=SUM(B4:B9)", false)]
        {
            s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
            let at = CellRef::parse(cell).unwrap();
            let sh = s.doc().unwrap().wb.active().unwrap();
            let f = sh.cells.get(at).and_then(|c| c.formula).expect("the formula is stored");
            // The syntax half the model can see, ORed with the file-format half only this crate can.
            let shape = f.shape().expect("a typed formula interns a shape");
            assert_eq!(shape.file_rewrite, rewrite, "{input}");
            assert_eq!(shape.print_at(at), f.text_at(at), "{input}");
        }
    }

    #[test]
    fn cell_set_takes_numbers_for_input_and_value() {
        // #697: a JSON number for `input` used to clear the cell instead of entering it.
        let mut s = Session::new();
        s.new_workbook();
        s.execute("cell.set", json!({"cell": "A1", "input": 123.5})).unwrap();
        s.execute("cell.set", json!({"cell": "A2", "value": 77})).unwrap();
        s.execute("cell.set", json!({"cell": "A3", "input": true})).unwrap();
        s.execute("cell.set", json!({"cell": "A4", "input": "=A1+A2"})).unwrap();
        let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap();
        assert_eq!(get(&mut s, "A1")["value"], json!(123.5));
        assert_eq!(get(&mut s, "A2")["value"], json!(77.0));
        assert_eq!(get(&mut s, "A3")["value"], json!(true));
        assert_eq!(get(&mut s, "A4")["value"], json!(200.5));
        assert!(s.execute("cell.set", json!({"cell": "A5"})).is_err());
    }

    #[test]
    fn legacy_array_entry() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "B2", "values": [[1], [2]]})).unwrap();
        s.execute("cell.set", json!({"cell": "C4", "input": "old"})).unwrap();
        s.execute("selection.set", json!({"range": "C2:C4"})).unwrap();
        s.execute("cell.set", json!({"cell": "C2", "input": "=B2:B3*2", "array": true})).unwrap();
        let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap();
        assert_eq!(get(&mut s, "C3")["value"], json!(4.0));
        // Past the result Excel shows #N/A; the old content of the range is replaced.
        assert_eq!(get(&mut s, "C4")["text"], "#N/A");
        let bar = s.doc().unwrap().wb.active().unwrap().cell(CellRef::parse("C2").unwrap()).map(|c| c.bar_text_at(CellRef::parse("C2").unwrap()));
        assert_eq!(bar.as_deref(), Some("{=B2:B3*2}"));
    }

    #[test]
    fn an_array_entered_outside_the_selection_covers_only_its_cell() {
        // Found by the command-sequence fuzzer: the array took the selection's range even with
        // `cell` elsewhere (or on another sheet), so its formula sat outside its own range and the
        // selected cells were wiped.
        let mut s = Session::new();
        s.new_workbook();
        s.execute("cell.set", json!({"cell": "K20", "input": "keep"})).unwrap();
        s.execute("selection.set", json!({"range": "K20:K22"})).unwrap();
        s.execute("cell.set", json!({"cell": "H5", "input": "=1+1", "array": true})).unwrap();
        let sh = s.doc().unwrap().wb.active().unwrap();
        let f = sh.cell(CellRef::parse("H5").unwrap()).and_then(|c| c.formula.clone()).unwrap();
        assert_eq!(f.array.as_deref().copied(), RangeRef::parse("H5"));
        assert_eq!(sh.value(CellRef::parse("K20").unwrap()), Value::text("keep"));
    }

    #[test]
    fn wildcards() {
        assert!(wildcard("hello", "h*o"));
        assert!(wildcard("hello", "h?llo"));
        assert!(!wildcard("hello", "h?lo"));
        assert!(wildcard("a*b", "a~*b"));
        assert!(!wildcard("axb", "a~*b"));
        assert!(wildcard("", "*"));
    }

    #[test]
    fn tsv() {
        assert_eq!(parse_tsv("a\tb\n1\t2\n"), vec![vec!["a", "b"], vec!["1", "2"]]);
        assert_eq!(parse_tsv("\"x\ty\"\tz"), vec![vec!["x\ty", "z"]]);
        assert_eq!(parse_tsv("\"he said \"\"hi\"\"\""), vec![vec!["he said \"hi\""]]);
    }

    #[test]
    fn paste_special_operations_keep_text_errors_and_formulas() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute(
            "range.setValues",
            json!({"range": "A1", "values": [["'123", "Total", "=NA()", "=B3*2", 5], [1, null, null, null, null], [null, 4, null, null, null]]}),
        )
        .unwrap();
        let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap();
        assert_eq!(get(&mut s, "A1")["value"], "123");
        s.execute("edit.copy", json!({"range": "A2"})).unwrap();
        s.execute("selection.set", json!({"range": "A1:E1"})).unwrap();
        s.execute("edit.pasteSpecial", json!({"what": "values", "operation": "multiply"})).unwrap();
        assert_eq!(get(&mut s, "A1")["value"], json!(123.0));
        assert_eq!(get(&mut s, "B1")["value"], "Total");
        assert_eq!(get(&mut s, "C1")["value"]["error"], "#N/A");
        assert_eq!(get(&mut s, "D1")["formula"], "=(B3*2)*1");
        assert_eq!(get(&mut s, "D1")["value"], json!(8.0));
        assert_eq!(get(&mut s, "E1")["value"], json!(5.0));
    }

    #[test]
    fn cut_paste_that_overflows_the_grid_moves_nothing() {
        // #997: the source was emptied first and the cells that fell off the grid were skipped.
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[1], [2]]})).unwrap();
        let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap();
        s.execute("edit.cut", json!({"range": "A1:A2"})).unwrap();
        // Transposed, the 2 rows become 2 columns and the second one is past XFD.
        assert!(s.execute("edit.pasteSpecial", json!({"what": "all", "transpose": true, "at": "XFD1"})).is_err());
        assert_eq!(get(&mut s, "A1")["value"], json!(1.0));
        assert_eq!(get(&mut s, "A2")["value"], json!(2.0));
        // Not transposed, the second row would be past the last row.
        assert!(s.execute("edit.pasteSpecial", json!({"what": "all", "at": "A1048576"})).is_err());
        assert_eq!(get(&mut s, "A1")["value"], json!(1.0));
        assert_eq!(get(&mut s, "A2")["value"], json!(2.0));
        // Not transposed it still fits in column XFD.
        s.execute("edit.pasteSpecial", json!({"what": "all", "at": "XFD1"})).unwrap();
        assert_eq!(get(&mut s, "XFD1")["value"], json!(1.0));
        assert_eq!(get(&mut s, "XFD2")["value"], json!(2.0));
    }

    #[test]
    fn replace_case_insensitive() {
        assert_eq!(replace_ci("Apple apple APPLE", "apple", "pear"), "pear pear pear");
    }

    /// Moving a block that spans every column the sheet stores takes the rows whole — and leaves
    /// behind exactly what the rows did not hold.
    #[test]
    fn a_move_of_whole_rows_leaves_nothing_behind() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1:C3", "values": [[1, 2, 3], [4, 5, 6], [7, 8, 9]]})).unwrap();
        let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap()["value"].clone();
        s.execute("range.move", json!({"range": "A1:C2", "to": "E5"})).unwrap();
        for a in ["A1", "B1", "C1", "A2", "B2", "C2"] {
            assert_eq!(get(&mut s, a), Json::Null, "{a} was moved away");
        }
        assert_eq!(get(&mut s, "E5"), json!(1.0));
        assert_eq!(get(&mut s, "G6"), json!(6.0));
        assert_eq!(get(&mut s, "A3"), json!(7.0), "the row below the block stays");
        assert_eq!(get(&mut s, "C3"), json!(9.0));
    }

    /// A move onto cells the source does not cover still asks — and the cells it asks about are the
    /// ones outside the overlap, not the whole destination.
    #[test]
    fn a_move_onto_data_asks_before_replacing_it() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1:C2", "values": [[1, 2, 3], [4, 5, 6]]})).unwrap();
        s.execute("cell.set", json!({"cell": "A3", "input": "busy"})).unwrap();
        assert!(s.execute("range.move", json!({"range": "A1:C2", "to": "A3"})).is_err(), "no overlap at all");
        // B2:D3 covers B2:C2 of the source; D2 and row 3 are the cells that differ.
        s.execute("cell.set", json!({"cell": "D2", "input": "busy"})).unwrap();
        assert!(s.execute("range.move", json!({"range": "A1:C2", "to": "B2"})).is_err(), "a cell beside the overlap");
        s.execute("cell.set", json!({"cell": "D2", "input": ""})).unwrap();
        s.execute("range.move", json!({"range": "A1:C2", "to": "B2", "overwrite": true})).unwrap();
        let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap()["value"].clone();
        assert_eq!(get(&mut s, "B2"), json!(1.0));
        assert_eq!(get(&mut s, "D3"), json!(6.0));
        assert_eq!(get(&mut s, "A3"), json!("busy"), "the row outside the move stays");
    }

    /// A three-by-three sheet with three `x` cells, laid out so row order and column order disagree:
    /// row-major finds B1, A2, B3; column-major finds A2, B1, B3.
    fn three_x() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1:C3", "values": [["a", "x", "b"], ["x", "c", "d"], ["e", "x", "f"]]})).unwrap();
        s
    }

    /// Find Next answers with the first match after the active cell, wraps to the top when there is
    /// none, and walks rows first — or columns first when `byColumns` asks for it.
    #[test]
    fn find_walks_in_scan_order_and_wraps() {
        let mut s = three_x();
        let at = |s: &mut Session, p: Json| s.execute("edit.find", p).unwrap()["cell"].as_str().unwrap_or_default().to_string();
        let cursor = |s: &mut Session, a: &str| s.execute("selection.set", json!({"range": a})).unwrap();
        for (from, want) in [("A1", "B1"), ("B1", "A2"), ("C3", "B1"), ("B3", "B1")] {
            cursor(&mut s, from);
            assert_eq!(at(&mut s, json!({"what": "x"})), want, "row-major from {from}");
        }
        for (from, want) in [("A1", "A2"), ("B1", "B3"), ("C1", "A2")] {
            cursor(&mut s, from);
            assert_eq!(at(&mut s, json!({"what": "x", "byColumns": true})), want, "column-major from {from}");
        }
    }

    /// `all` still collects every match, in scan order.
    #[test]
    fn find_all_collects_every_match_in_scan_order() {
        let mut s = three_x();
        let cells = |s: &mut Session, by_cols: bool| -> Vec<String> {
            let r = s.execute("edit.find", json!({"what": "x", "all": true, "byColumns": by_cols})).unwrap();
            assert_eq!(r["count"], 3);
            r["results"].as_array().map(|a| a.iter().filter_map(|x| x["cell"].as_str().map(str::to_string)).collect()).unwrap_or_default()
        };
        assert_eq!(cells(&mut s, false), ["B1", "A2", "B3"]);
        assert_eq!(cells(&mut s, true), ["A2", "B1", "B3"]);
    }

    /// Within a workbook the search starts at the active sheet and wraps round the sheets.
    #[test]
    fn find_wraps_round_the_workbook() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["here", "here"]]})).unwrap();
        s.execute("home.insertSheet", json!({"name": "Second"})).unwrap();
        s.execute("range.setValues", json!({"range": "A1", "values": [["here", "here"]]})).unwrap();
        let p = json!({"what": "here", "within": "workbook"});
        // The new sheet is the active one, so its B1 (after the active cell A1) is the first hit.
        assert_eq!(s.execute("edit.find", p.clone()).unwrap()["sheet"], json!(1), "the active sheet first");
        assert_eq!(s.execute("edit.find", p).unwrap()["sheet"], json!(0), "then the sheets before it");
    }

    /// What a cell is matched on is what it shows: a number format's literal counts, and so does a
    /// text section. The fast path that borrows a text value's own text must not skip either.
    #[test]
    fn find_matches_the_text_a_format_shows() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("cell.set", json!({"cell": "A1", "input": 5})).unwrap();
        s.execute("home.numberFormat", json!({"range": "A1", "code": "0\" units\""})).unwrap();
        s.execute("cell.set", json!({"cell": "A2", "input": "abc"})).unwrap();
        s.execute("home.numberFormat", json!({"range": "A2", "code": "0;0;0;\"text:\"@"})).unwrap();
        let find = |s: &mut Session, what: &str| -> String {
            s.execute("edit.find", json!({"what": what})).unwrap()["value"].as_str().unwrap_or_default().to_string()
        };
        assert_eq!(find(&mut s, "units"), "5 units");
        assert_eq!(find(&mut s, "text:abc"), "text:abc");
        assert_eq!(find(&mut s, "abc"), "text:abc", "a text section still holds the text");
    }

    #[test]
    fn replace_case_insensitive_keeps_text_when_folding_moves_boundaries() {
        // U+212A KELVIN SIGN folds to `k` (3→1 bytes) while U+0130 folds to `i` plus a
        // combining dot (2→3 bytes): the folded text is the same length but its byte positions
        // no longer line up with the original, and the old slicing dropped the tail.
        let mut s = Session::new();
        s.new_workbook();
        let text = "\u{212A}\u{0130}\u{0130}";
        s.execute("cell.set", json!({"cell": "A1", "input": text})).unwrap();
        s.execute("edit.replace", json!({"what": "k", "with": "x"})).unwrap();
        let sh = s.doc().unwrap().wb.active().unwrap();
        assert_eq!(sh.value(CellRef::new(0, 0)), Value::text(text));
    }
}
