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
