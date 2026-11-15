//! A table may not be placed over the sheet's AutoFilter: the file holds one filter there, so the
//! criteria would be dropped and the rows the filter had hidden would come back as hand-hidden
//! ones. Found by the command-sequence fuzz (seed 4, sample budget), which is why the round trip is
//! asserted here directly.

use gridcraft_engine::Session;
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn rows_hidden_by_filter(s: &Session) -> Vec<u32> {
    let d = s.doc().expect("doc");
    let sh = d.wb.sheet(0).expect("sheet");
    (0..60u32).filter(|r| sh.filter_hidden.contains(*r)).collect()
}

#[test]
fn a_table_over_the_sheets_autofilter_is_refused() {
    let mut s = session();
    s.execute("range.setValues", json!({"range": "C16", "values": [["a","b","c"],["1","2","3"],["4","5","9"]]})).unwrap();
    s.execute("selection.set", json!({"range": "C16:E18"})).unwrap();
    s.execute("data.filterBy", json!({"column": "E", "custom": {"op": ">", "value": "4"}})).unwrap();
    assert_eq!(rows_hidden_by_filter(&s), vec![16], "the filter hid the row that failed it");
    let err = s.execute("insert.table", json!({"header": true, "name": "T", "range": "D15:E18"})).expect_err("it overlaps the AutoFilter");
    assert!(err.to_string().contains("AutoFilter"), "{err}");
}

#[test]
fn a_filtered_sheets_hidden_rows_survive_save_and_reload() {
    let mut s = session();
    s.execute("range.setValues", json!({"range": "A1", "values": [["n"], ["1"], ["9"], ["2"]]})).unwrap();
    s.execute("selection.set", json!({"range": "A1:A4"})).unwrap();
    s.execute("data.filterBy", json!({"column": "A", "custom": {"op": "<", "value": "5"}})).unwrap();
    let before = rows_hidden_by_filter(&s);
    assert_eq!(before, vec![2], "the row with 9 is hidden");
    let bytes = gridcraft_xlsx::write_xlsx(&s.doc().expect("doc").wb).unwrap();
    let (wb, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
    let sh = wb.sheet(0).expect("sheet");
    let after: Vec<u32> = (0..10u32).filter(|r| sh.filter_hidden.contains(*r)).collect();
    assert_eq!(after, before, "a hidden row stays filter-hidden, not hand-hidden");
    // Nothing was left in the line map that a scroll would walk.
    assert!(sh.rows.iter().all(|(_, i)| !i.hidden), "no line info was left holding the hidden rows");
}

#[test]
fn a_table_on_the_same_cells_as_the_filter_is_the_ordinary_case() {
    let mut s = session();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Name", "Qty"], ["Red", "1"], ["Blue", "2"]]})).unwrap();
    s.execute("data.filter", json!({"range": "A1:B3"})).unwrap();
    s.execute("insert.table", json!({"range": "A1:B3"})).expect("a table over its own filter is what Excel converts");
    // And a table that sits inside the filtered range is allowed too.
    s.execute("insert.table", json!({"range": "A1:A2"})).expect_err("overlapping another table is still refused");
}

/// Moving the AutoFilter to another range: the old filter's hidden rows come back, the way they do
/// when AutoFilter is applied somewhere else in Excel.
///
/// Found by the command-sequence fuzz (seed 14, sample grades): a filterBy on one column, then a
/// filter over a different range, and after save+reload the sheet held rows the running sheet did
/// not show. The rows were left hidden by a filter that no longer existed.
#[test]
fn moving_the_filter_range_brings_the_old_filters_rows_back() {
    let mut s = session();
    s.execute("range.setValues", json!({"range": "B20", "values": [["n"], ["1"], ["9"], ["2"], ["3"]]})).unwrap();
    s.execute("selection.set", json!({"range": "B20:B24"})).unwrap();
    s.execute("data.filterBy", json!({"column": "B", "custom": {"op": "<", "value": "5"}})).unwrap();
    assert_eq!(rows_hidden_by_filter(&s), vec![21], "the row with 9 is hidden");
    // A filter somewhere else on the sheet: the old one is gone, so its rows are not hidden by
    // anything any more.
    s.execute("data.filter", json!({"range": "D18:F22"})).unwrap();
    assert!(rows_hidden_by_filter(&s).is_empty(), "the old filter's rows came back");
    let d = s.doc().expect("doc");
    let sh = d.wb.sheet(0).expect("sheet");
    assert!(sh.rows.values().all(|r| !r.hidden), "and none of them is left hand-hidden");
}

/// A filter command that names a range is about that range, not about the table the cursor is in.
///
/// Found by the 20x60 soak (seed 14, sample grades): data.filterBy on a table's column turns the
/// table's dropdown on, then data.filter over a range elsewhere toggled THAT TABLE's dropdown off
/// because it looked at the selection, and left the sheet's filter with nowhere to be written, so
/// the rows it hid came back hand-hidden after a reload.
#[test]
fn a_filter_range_outside_the_table_toggles_the_sheet_filter_not_the_tables() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "grades"})).unwrap();
    s.execute("data.filterBy", json!({"column": "G", "values": ["73%", "'007"]})).unwrap();
    assert!(!rows_hidden_by_filter(&s).is_empty(), "the column filter hid rows");
    s.execute("data.filter", json!({"range": "B20:F22"})).unwrap();
    let d = s.doc().expect("doc");
    let sh = d.wb.sheet(0).expect("sheet");
    assert!(sh.autofilter.is_none(), "the sheet filter is off again");
    assert!(rows_hidden_by_filter(&s).is_empty(), "and its rows are back");
    assert!(sh.rows.values().all(|r| !r.hidden), "nothing left hand-hidden");
    // The table's own dropdown is not the sheet filter's business.
    assert!(sh.tables.iter().any(|t| t.name == "Grades" && t.filter_button), "the table keeps its dropdown");
}
