//! Regressions from the 400-seed command-sequence soak's `[xlsx roundtrip]` failures: three
//! save-and-reload defects (seeds 6, 30, 34 and 60).
//!
//! Each test builds the state by hand, drives it through `io::save_bytes` and `io::open_bytes`
//! (recalculating like opening a file does, as the fuzz harness's `roundtrip_diff` does) and
//! compares the one thing that was lost. Both classes are "the file cannot hold what the model
//! says, and the save said nothing":
//!
//! - a cell under a legacy (Ctrl+Shift+Enter) array whose range is too big for the evaluator to
//!   lay out: the array still covers it, so reading the file back drops its cached value;
//! - a column width past the format's maximum or a sheet default width of zero, both of which the
//!   writer clamps or the reader ignores, so the column map came back different;
//! - a row a filter hides that also carries a height: the file has one place for both, the sheet
//!   two (the filter bitmap and the line map), and the reader moved the hidden bit into the bitmap
//!   only when the row had nothing else — so the reloaded sheet hid that row by hand.

use cella_core::CellRef;
use cella_engine::Session;
use cella_engine::calc::Calc;
use cella_engine::io;
use cella_engine::model::{Sheet, Style, StyleId, Workbook};
use serde_json::json;

fn at(address: &str) -> CellRef {
    CellRef::parse(address).unwrap_or_default()
}

/// The workbook after a save and a reload, recalculated as opening a file does.
fn reload(wb: &Workbook) -> Workbook {
    let bytes = io::save_bytes(wb, "regression.xlsx").expect("save");
    // Every save in these tests goes through the package validator.
    cella_xlsx::testing::check_package(&bytes).expect("a valid package");
    let (mut back, _) = io::open_bytes("regression.xlsx", &bytes).expect("reload");
    Calc::new().recalc_all(&mut back);
    back
}

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn workbook(s: &Session) -> Workbook {
    (*s.doc().expect("document").wb).clone()
}

fn value(wb: &Workbook, address: &str) -> String {
    wb.active().expect("sheet").value(at(address)).display()
}

/// The legacy array ranges on a sheet, as `anchor → covered range`.
fn legacy_arrays(sheet: &Sheet) -> Vec<(String, String)> {
    sheet.cells.iter().filter_map(|(c, cell)| cell.formula.as_ref().and_then(|f| f.array.as_deref()).map(|r| (c.a1(), r.a1()))).collect()
}

/// The column properties a save and reload must give back, styles by content (the file renumbers
/// them), the way the fuzz harness's `kept_lines` compares them.
fn kept_cols(sheet: &Sheet, styles: &cella_engine::model::StyleTable) -> Vec<(u32, bool, u8, bool, Option<Style>, Option<i64>)> {
    sheet
        .cols
        .iter()
        .filter_map(|(c, info)| {
            let style = info.style.filter(|s| *s != StyleId::DEFAULT).map(|s| styles.get(s).clone());
            let size = info.size.map(|px| px.round() as i64);
            let content = info.hidden || info.outline > 0 || info.collapsed || style.is_some() || size.is_some();
            content.then_some((c, info.hidden, info.outline.min(7), info.collapsed, style, size))
        })
        .collect()
}

#[test]
fn a_legacy_array_too_big_to_lay_out_is_refused_and_leaves_the_cells_it_would_cover() {
    // Seeds 6: `home.insertSheet`, `edit.selectAll`, a value at the bottom of the sheet, then a
    // Ctrl+Shift+Enter array over the selection. The selection is the whole sheet, the evaluator
    // lays out at most `MAX_CELLS` (the article was #NUM! and filled nothing), yet the array
    // covered K1048575: the file said the array owned it, so the reload dropped its cached value.
    let mut s = session();
    s.execute("home.insertSheet", json!({"name": "Data"})).expect("insert sheet");
    s.execute("cell.set", json!({"cell": "K1048575", "input": "East"})).expect("the edge cell");
    s.execute("edit.selectAll", json!({})).expect("select all");
    assert_eq!(s.doc().expect("document").selection.current().count(), 1_048_576 * 16_384, "the whole sheet is selected");

    let err = s
        .execute("cell.set", json!({"array": true, "cell": "B1", "input": "=FILTER(D1:D20,D1:D20>5)"}))
        .expect_err("an array the engine cannot fill is refused");
    let message = err.to_string();
    assert!(message.contains("array formula") && message.contains("16000000"), "{message}");

    // Nothing moved: the cell is still there and no array claims the sheet.
    let before = workbook(&s);
    assert_eq!(value(&before, "K1048575"), "East");
    assert!(legacy_arrays(before.active().expect("sheet")).is_empty(), "the refused entry left no array");

    // Before the fix the entry succeeded and this value came back empty.
    let after = reload(&before);
    assert_eq!(value(&after, "K1048575"), "East", "the cell at the bottom of the sheet survives the save");
}

#[test]
fn a_column_wider_than_the_file_can_store_is_refused_and_the_widest_it_can_is_kept() {
    // Seeds 34 and 60: the command took up to 1800 px, the writer stores 255 characters at most
    // (1785 px), so 1786..=1800 came back 1785 — memory and file disagreed.
    let mut s = session();
    let message =
        s.execute("home.columnWidth", json!({"cols": "B:B", "width": 1786})).expect_err("the format cannot store a wider column").to_string();
    assert!(message.contains("1785"), "{message}");

    // The widest representable width, by pixels and by characters (255 is the format's maximum).
    s.execute("home.columnWidth", json!({"cols": "B:B", "width": 1785})).expect("the widest width");
    s.execute("home.columnWidth", json!({"cols": "C:C", "chars": 255})).expect("255 characters");
    // A fractional width would be rounded by the writer: the model keeps the width it can return.
    s.execute("home.columnWidth", json!({"cols": "D:D", "width": 100.6})).expect("a fractional width");

    let before = workbook(&s);
    let sheet = before.active().expect("sheet");
    assert_eq!([sheet.col_width(1), sheet.col_width(2), sheet.col_width(3)], [1785.0, 1785.0, 101.0]);

    let after = reload(&before);
    let sheet = after.active().expect("sheet");
    assert_eq!([sheet.col_width(1), sheet.col_width(2), sheet.col_width(3)], [1785.0, 1785.0, 101.0]);
    assert_eq!(kept_cols(before.active().expect("sheet"), &before.styles), kept_cols(sheet, &after.styles), "cols differ after reload");
}

#[test]
fn a_sheet_default_width_of_zero_keeps_its_columns_through_a_save() {
    // Seeds 34 and 60, as the shrinker reduced them: `home.defaultWidth {"width": 0}` and a
    // column line without a width of its own (`A:A` formatted). The reader ignored the written
    // `defaultColWidth="0"` and kept 64, so every line the writer sized from the default (styled,
    // hidden) came back with an explicit width instead of none.
    let mut s = session();
    s.execute("home.defaultWidth", json!({"width": 0.0})).expect("default width zero");
    s.execute("home.numberFormat", json!({"range": "A:A", "format": "Percentage"})).expect("a styled column line");
    s.execute("home.hideColumns", json!({"cols": "E:E"})).expect("a hidden column line");

    let before = workbook(&s);
    assert_eq!(before.active().expect("sheet").default_col_width, 0.0);

    let after = reload(&before);
    let sheet = after.active().expect("sheet");
    assert_eq!(sheet.default_col_width, 0.0, "the written default width is read back");
    assert_eq!(kept_cols(before.active().expect("sheet"), &before.styles), kept_cols(sheet, &after.styles), "cols differ after reload");
    // The styled line still has no width of its own, and the hidden one is still hidden.
    assert_eq!(sheet.cols.get(&0).map(|i| i.size), Some(None), "A has no width");
    assert_eq!(sheet.cols.get(&4).map(|i| (i.size, i.hidden)), Some((None, true)), "E is hidden, not sized");
}

/// The row properties a save and reload must give back, styles by content (the file renumbers
/// them), the way kept_cols does for columns.
#[allow(clippy::type_complexity)]
fn kept_rows(sheet: &Sheet, styles: &cella_engine::model::StyleTable) -> Vec<(u32, bool, u8, bool, bool, Option<Style>, Option<i64>)> {
    sheet
        .rows
        .iter()
        .map(|(r, info)| {
            let style = info.style.filter(|s| *s != StyleId::DEFAULT).map(|s| styles.get(s).clone());
            (r, info.hidden, info.outline.min(7), info.collapsed, info.custom, style, info.size.map(|px| px.round() as i64))
        })
        .collect()
}

#[test]
fn a_filter_hidden_row_that_also_has_a_height_comes_back_as_the_filters() {
    // Seed 30, reduced: the sales sample, a bigger font, a top-1 filter on Total — which hides rows
    // 4..9 — and a sort of I1:M7. Row 6 is the sample's tall header, so one of the rows the filter
    // hides also carries a height. The file stores that row as a single hidden="1" element with the
    // height on it; the reader moved a row into the filter bitmap only when its line info was
    // nothing but hidden, so row 6 came back as a hand-hidden line-map row and the bitmap lost it:
    // six rows hidden before the save, five after, and Clear Filter would have left row 6 hidden
    // for good.
    let mut s = session();
    s.execute("file.new", json!({"sample": "sales"})).expect("the sales sample");
    s.execute("home.increaseFontSize", json!({})).expect("a bigger font");
    s.execute("data.filterBy", json!({"column": "I", "top": {"count": 1}})).expect("a top-1 filter on Total");
    s.execute("data.sort", json!({"header": false, "keys": [{"column": "B", "order": "desc"}], "orientation": "rows", "range": "I1:M7"}))
        .expect("a sort inside the range");

    let before = workbook(&s);
    let sheet = before.active().expect("sheet");
    assert_eq!(sheet.filter_hidden.rows().collect::<Vec<u32>>(), (4..=9).collect::<Vec<u32>>(), "the filter hides rows 4..9");
    assert_eq!(sheet.rows.get(&6).map(|i| (i.hidden, i.size)), Some((false, Some(22.0))), "and row 6 is the tall header row too");

    let after = reload(&before);
    let sheet = after.active().expect("sheet");
    assert_eq!(sheet.filter_hidden.rows().collect::<Vec<u32>>(), (4..=9).collect::<Vec<u32>>(), "the same rows are the filter's after the reload");
    assert!(sheet.rows.iter().all(|(r, info)| !info.hidden || !(4..=9).contains(&r)), "no filtered row came back as a hand-hidden one");
    assert_eq!(kept_rows(before.active().expect("sheet"), &before.styles), kept_rows(sheet, &after.styles), "rows differ after reload");
}

#[test]
fn a_sheet_copied_under_its_own_filter_keeps_its_rows_through_a_save() {
    // Seed 173, as the shrinker reduced it: a sheet copied onto itself, a filter, a table, another
    // sheet, an autofill, a second table, a three-step undo, and a second filter. The copy's
    // filtered rows came back different: the copy had taken the filter's hidden-row state and the
    // undo had restored line info over it, so what the file wrote for those rows was not what the
    // sheet in memory said they were.
    let mut s = session();
    s.execute("file.new", json!({"sample": "grades"})).expect("the grades sample");
    s.execute("sheet.move", json!({"copy": true, "sheet": 0, "to": 0})).ok();
    s.execute("data.filter", json!({})).ok();
    s.execute("insert.table", json!({"header": true, "name": "Data", "range": "D2:F7"})).ok();
    s.execute("home.insertSheet", json!({"name": "Summary"})).ok();
    s.execute("edit.autoFill", json!({"mode": "alternate", "source": "I21:L21", "target": "I21:M27"})).ok();
    s.execute("insert.table", json!({"header": false, "range": "C23:C25"})).ok();
    s.execute("edit.undo", json!({"steps": 3})).ok();
    s.execute("data.filterBy", json!({"column": "G", "top": {"count": 2}})).ok();

    let before = workbook(&s);
    let after = reload(&before);
    for (x, y) in before.sheets.iter().zip(&after.sheets) {
        assert_eq!(kept_rows(x, &before.styles), kept_rows(y, &after.styles), "{}: rows differ after reload", x.name);
    }
}
