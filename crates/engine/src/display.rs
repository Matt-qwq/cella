//! How cells look as text: number formats applied, for the grid, clipboard, CSV and agents.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Mutex;

use cella_core::{CellRef, RangeRef, Value};
use cella_model::{Sheet, Workbook};
use cella_numfmt::{Formatted, NumberFormat, format_value};

/// Parsed number formats are cached by code.
fn parsed(code: &str) -> NumberFormat {
    static CACHE: std::sync::LazyLock<Mutex<HashMap<String, NumberFormat>>> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
    let lock = || CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(f) = lock().get(code) {
        return f.clone();
    }
    // Parse outside the lock, so a slow code never blocks the other threads' lookups.
    let f = NumberFormat::parse(code);
    let mut m = lock();
    if m.len() > 4096 {
        m.clear();
    }
    m.insert(code.to_string(), f.clone());
    f
}

pub fn number_format(code: &str) -> NumberFormat {
    parsed(code)
}

/// Formats a value with a format code.
pub fn format(v: &Value, code: &str, wb: &Workbook) -> Formatted {
    format_value(v, &parsed(code), wb.date_system)
}

/// Whether typing `t` back into a General cell would not give the text `t` again (it would parse
/// as a number, boolean, error, date or formula, or lose a leading apostrophe). Such text is shown
/// for editing with a leading `'`, as Excel does for text carrying a quote prefix, so that
/// committing the editor unchanged keeps the cell as text.
fn text_needs_quote_prefix(t: &str, sys: cella_core::date::DateSystem) -> bool {
    if t.is_empty() {
        return false;
    }
    let mut chars = t.chars();
    let (first, second) = (chars.next(), chars.next());
    if matches!(first, Some('=' | '\'')) {
        return true;
    }
    if matches!(first, Some('+' | '-')) && second.is_some_and(|c| c.is_ascii_alphabetic() || c == '(') {
        return true;
    }
    !matches!(cella_core::parse::parse_input(t, sys).value, Value::Text(ref x) if x.as_ref() == t)
}

/// What the cell editor and the formula bar start from. Same as `Cell::input_text`, except that a
/// constant number in a date or time format is shown as a date (as the grid shows it), not as its
/// serial. When the formatted text would not read back as the same date (for example
/// `dddd, mmmm d`), an ISO `yyyy-mm-dd` (with `hh:mm:ss` for a time of day; just `hh:mm:ss`
/// for a time alone) is used instead, and if even that would not read back, the serial.
pub fn edit_text(wb: &Workbook, sheet: &Sheet, c: CellRef) -> String {
    let raw = sheet.input_text(c);
    let Some(cell) = sheet.cell(c) else { return raw };
    if let Value::Text(t) = &cell.value
        && cell.formula.is_none()
        && wb.styles.get(cell.style).num_fmt.as_str() != "@"
        && text_needs_quote_prefix(t, wb.date_system)
    {
        return format!("'{raw}");
    }
    let Value::Number(n) = &cell.value else { return raw };
    let code = wb.styles.get(cell.style).num_fmt.as_str();
    if cell.formula.is_some() || !parsed(code).is_date() {
        return raw;
    }
    let reads_back = |s: &str| matches!(cella_core::parse::parse_input(s, wb.date_system).value, Value::Number(m) if (m - n).abs() < 1e-9);
    let shown = format(&cell.value, code, wb).text;
    if reads_back(&shown) {
        return shown;
    }
    let Some(dt) = cella_core::date::datetime_from_serial(wb.date_system, *n) else { return raw };
    let time = format!("{:02}:{:02}:{:02}", dt.hour, dt.minute, dt.second);
    let date = format!("{:04}-{:02}-{:02}", dt.year, dt.month, dt.day);
    let iso = if *n < 1.0 {
        time
    } else if n.fract() == 0.0 {
        date
    } else {
        format!("{date} {time}")
    };
    if reads_back(&iso) { iso } else { raw }
}

/// Whether `c` has a formula that must not be shown: its cell is formatted Hidden on a
/// protected sheet. A spilled cell follows its anchor.
pub fn formula_hidden(wb: &Workbook, sheet: &Sheet, c: CellRef) -> bool {
    if !sheet.is_protected() {
        return false;
    }
    let at = match sheet.cell(c) {
        Some(x) if x.formula.is_some() => c,
        _ => match sheet.spill_ranges.iter().find(|(_, r)| r.contains(c)) {
            Some((anchor, _)) => *anchor,
            None => return false,
        },
    };
    wb.styles.get(sheet.style_id(at)).protection.hidden
}

/// The text a cell shows (full precision General; the grid narrows General to fit the column).
pub fn cell_text(wb: &Workbook, sheet: &Sheet, c: CellRef) -> String {
    if sheet.show_formulas
        && let Some(f) = sheet.cell(c).and_then(|x| x.formula)
        && !formula_hidden(wb, sheet, c)
    {
        return format!("={}", f.text_at(c).unwrap_or_default());
    }
    value_text(wb, sheet, c)
}

/// A cell's value with its number format applied, independent of column width: no `#####`
/// and no General narrowing (Save As CSV/Text writes this). Formulas give their result.
pub fn value_text(wb: &Workbook, sheet: &Sheet, c: CellRef) -> String {
    let style = wb.styles.get(sheet.style_id(c));
    value_text_as(wb, &sheet.value(c), style.num_fmt.as_str())
}

/// [`value_text`] for a value and a format code given apart from a cell (print takes the code
/// from the cell's effective look, which a conditional format can override).
pub fn value_text_as(wb: &Workbook, v: &Value, code: &str) -> String {
    value_text_with(wb, v, &number_format(code))
}

/// [`value_text_as`] for a format that is already parsed. The clipboard formats a million cells
/// under a handful of styles, and parsing the code per cell — through a global lock and a string
/// hash — was most of what a copy of a wide block cost.
fn value_text_with(wb: &Workbook, v: &Value, fmt: &NumberFormat) -> String {
    let f = format_value(v, fmt, wb.date_system);
    match f.fill {
        Some((_, pos)) => {
            let mut t = f.text;
            if pos <= t.len() && t.is_char_boundary(pos) {
                t.insert(pos, ' ');
            }
            t
        }
        None => f.text,
    }
}

/// Tab-separated text of a range, as the system clipboard gets it.
///
/// Read a row at a time through the store's own row reader. Asking the sheet for a cell's text ranks
/// the packed row once for the value and once for the style, and a row of a thousand columns holds
/// 125 kind runs: 2M cells of a wide block took 0.89 s, one per-cell text at a time.
pub fn range_text(wb: &Workbook, sheet: usize, r: RangeRef) -> String {
    let Some(sh) = wb.sheet(sheet) else { return String::new() };
    let mut out = String::new();
    let r = clipboard_range(r);
    let has_spill = !sh.spill_ranges.is_empty();
    let read = sh.cells.row_reader(r.start.col, r.end.col);
    let mut row: Vec<Option<cella_model::Cell>> = Vec::with_capacity(r.width() as usize);
    let mut formats: HashMap<cella_model::StyleId, NumberFormat> = HashMap::new();
    for at in r.start.row..=r.end.row {
        if sh.is_row_hidden(at) {
            continue;
        }
        row.clear();
        read(at, &mut row);
        let mut first = true;
        for (i, slot) in row.iter().enumerate() {
            if !first {
                out.push('\t');
            }
            first = false;
            let col = r.start.col + i as u32;
            let c = CellRef::new(at, col);
            let t: Cow<'_, str> = match slot {
                // A stored cell carries its own value and style, so its text needs no lookup.
                Some(cell) => match &cell.formula {
                    Some(f) if sh.show_formulas && !formula_hidden(wb, sh, c) => Cow::Owned(format!("={}", f.text_at(c).unwrap_or_default())),
                    _ => {
                        let fmt = formats.entry(cell.style).or_insert_with(|| number_format(wb.styles.get(cell.style).num_fmt.as_str()));
                        Cow::Owned(value_text_with(wb, &cell.value, fmt))
                    }
                },
                // No stored cell: nothing to show, unless a spill reaches here.
                None if has_spill => Cow::Owned(cell_text(wb, sh, c)),
                None => Cow::Borrowed(""),
            };
            if t.contains(['\t', '\n', '"']) {
                out.push('"');
                out.push_str(&t.replace('"', "\"\""));
                out.push('"');
            } else {
                out.push_str(&t);
            }
        }
        out.push('\n');
    }
    out
}

/// The existing plain-text clipboard limit, shared with its HTML representation.
pub(crate) fn clipboard_range(r: RangeRef) -> RangeRef {
    if r.count() > 2_000_000 { RangeRef::new(r.start, r.start.offset_clamped(1999, 999)) } else { r }
}

/// Status-bar statistics for a selection: average, count (non-empty), numerical count, min,
/// max, sum.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct Stats {
    pub count: u64,
    pub numeric: u64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
    pub average: Option<f64>,
}

pub fn stats(sheet: &Sheet, ranges: &[RangeRef]) -> Stats {
    let mut s = Stats { min: f64::INFINITY, max: f64::NEG_INFINITY, ..Default::default() };
    let used = sheet.used_range();
    let clipped: Vec<RangeRef> = ranges.iter().filter_map(|r| used.and_then(|u| r.intersection(&u))).collect();
    // Areas only need the `seen` set when they can overlap: a cell in two areas is counted once.
    // The areas of a selection are pairwise disjoint (Ctrl-clicking a second one takes its cells
    // out of the first), and inserting every visited cell into a `HashSet` was most of the walk's
    // cost on a big selection.
    let overlapping = clipped.iter().enumerate().any(|(i, a)| clipped[i + 1..].iter().any(|b| a.intersects(b)));
    let mut seen: Option<std::collections::HashSet<CellRef>> = if overlapping { Some(Default::default()) } else { None };
    // Hidden rows are a property of the row, so the check runs once per row of the walk instead of
    // once per cell, and not at all on a sheet with no filter and no custom-height row.
    let row_meta = !sheet.filter_hidden.is_empty() || !sheet.rows.is_empty();
    let mut last_row: Option<(u32, bool)> = None;
    let mut visit = |c: CellRef, v: &Value| {
        if seen.as_mut().is_some_and(|set| !set.insert(c)) {
            return;
        }
        let hidden = match last_row {
            Some((row, hidden)) if row == c.row => hidden,
            _ => {
                let hidden = row_meta && sheet.is_row_hidden(c.row);
                last_row = Some((c.row, hidden));
                hidden
            }
        };
        if hidden || v.is_empty() {
            return;
        }
        s.count += 1;
        if let Value::Number(n) = v {
            s.numeric += 1;
            s.sum += n;
            s.min = s.min.min(*n);
            s.max = s.max.max(*n);
        }
    };
    for r in &clipped {
        for (c, cell) in sheet.cells.iter_range(*r) {
            visit(c, &cell.value);
        }
        for (c, v) in sheet.spilled_in(*r) {
            visit(c, &v);
        }
    }
    if s.numeric > 0 {
        s.average = Some(s.sum / s.numeric as f64);
    } else {
        s.min = 0.0;
        s.max = 0.0;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::stats;
    use cella_core::RangeRef;
    use serde_json::json;

    fn session(cells: &[(&str, &str)]) -> crate::Session {
        let mut s = crate::Session::default();
        s.new_workbook();
        for (cell, input) in cells {
            s.execute("cell.set", json!({"cell": cell, "input": input})).expect("cell.set");
        }
        s
    }

    fn r(a: &str) -> RangeRef {
        RangeRef::parse(a).expect("a range")
    }

    #[test]
    fn overlapping_areas_count_a_shared_cell_once() {
        // Two areas that overlap still need the `seen` set; disjoint ones skip it (the fast path).
        let s = session(&[("A1", "1"), ("A2", "2"), ("A3", "4")]);
        let d = s.doc().expect("a document");
        let sh = d.wb.sheet(0).expect("a sheet");
        let st = stats(sh, &[r("A1:A2"), r("A2:A3")]);
        assert_eq!(st.count, 3, "A2 lies in both areas and counts once");
        assert_eq!(st.sum, 7.0);
        let st = stats(sh, &[r("A1:A1"), r("A3:A3")]);
        assert_eq!(st.count, 2);
        assert_eq!(st.sum, 5.0);
        assert_eq!(st.min, 1.0);
        assert_eq!(st.max, 4.0);
        assert_eq!(st.average, Some(2.5));
    }

    #[test]
    fn a_hidden_row_is_left_out_of_the_statistics() {
        let mut s = session(&[("A1", "1"), ("A2", "2"), ("B1", "10")]);
        s.execute("home.hideRows", json!({"rows": "2"})).expect("hide row 2");
        let d = s.doc().expect("a document");
        let sh = d.wb.sheet(0).expect("a sheet");
        let st = stats(sh, &[r("A1:B2")]);
        assert_eq!(st.count, 2, "row 2 is hidden on both columns");
        assert_eq!(st.sum, 11.0);
        assert_eq!(st.min, 1.0);
    }
}
