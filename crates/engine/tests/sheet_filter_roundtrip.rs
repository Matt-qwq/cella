//! A sheet AutoFilter saves and reloads with its criteria, so the rows it hides stay the filter's
//! to bring back: open a filtered file and Clear Filter must show every row again.
//!
//! Found by the command-sequence fuzzer (seed 6, step 89): a filter over a table's own range is
//! written into the table part, and the reader kept it only there. The reopened sheet had no
//! AutoFilter at all — 0 criteria, the rows hidden and `data.clearFilter` with nothing to clear.

use cella_engine::Session;
use cella_engine::model::FilterCriterion;
use serde_json::json;

/// The rows the sheet does not show, by whatever mechanism.
fn hidden_rows(s: &Session) -> Vec<u32> {
    let d = s.doc().expect("document");
    let sh = d.wb.sheet(0).expect("sheet");
    (0..64u32).filter(|r| sh.is_row_hidden(*r)).collect()
}

/// Rows hidden by a line info — the ones a filter cannot bring back.
fn hand_hidden_rows(s: &Session) -> Vec<u32> {
    let d = s.doc().expect("document");
    let sh = d.wb.sheet(0).expect("sheet");
    sh.rows.iter().filter(|(_, i)| i.hidden).map(|(r, _)| r).collect()
}

fn filter_hidden_count(s: &Session) -> u32 {
    let d = s.doc().expect("document");
    d.wb.sheet(0).map_or(0, |sh| sh.filter_hidden.count())
}

fn criteria(s: &Session) -> Vec<(u32, FilterCriterion)> {
    let d = s.doc().expect("document");
    d.wb.sheet(0).and_then(|sh| sh.autofilter.clone()).map(|a| a.criteria).unwrap_or_default()
}

fn filter_range(s: &Session) -> Option<String> {
    let d = s.doc().expect("document");
    d.wb.sheet(0).and_then(|sh| sh.autofilter.as_ref().map(|a| a.range.a1()))
}

/// Everything the sheet's tables hold as their own filter criteria.
fn table_criteria(s: &Session) -> Vec<(u32, FilterCriterion)> {
    let d = s.doc().expect("document");
    let sh = d.wb.sheet(0).expect("sheet");
    sh.tables.iter().flat_map(|t| t.filter.clone()).collect()
}

fn saved(s: &Session) -> Vec<u8> {
    cella_engine::io::save_bytes(&s.doc().expect("document").wb, "filtered.xlsx").expect("it saves")
}

fn reopened(bytes: &[u8]) -> Session {
    let mut s = Session::new();
    s.execute("file.open", json!({"name": "filtered.xlsx", "base64": cella_engine::io::base64_encode(bytes)})).expect("it opens");
    s
}

/// A value list, one of whose values starts with an apostrophe, over the sales sample's own table:
/// the filter must come back as the sheet's, keep its rows in its bitmap, and clear.
#[test]
fn a_value_list_filter_survives_save_and_reload() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "sales"})).expect("the sample loads");
    s.execute("data.filterBy", json!({"column": "G", "values": ["'007", "121"]})).expect("it filters");
    let before = hidden_rows(&s);
    assert!(!before.is_empty(), "the filter hid the rows whose growth is neither value");
    assert!(hand_hidden_rows(&s).is_empty(), "a filtered row is not a hand-hidden one");

    let mut r = reopened(&saved(&s));
    let crit = criteria(&r);
    assert!(
        matches!(crit.as_slice(), [(5, FilterCriterion::Values { values, blanks: false })]
            if values == &["'007".to_string(), "121".to_string()]),
        "the criteria were dropped or moved: {crit:?}"
    );
    assert!(table_criteria(&r).is_empty(), "and they are not duplicated into the table part");
    assert_eq!(hidden_rows(&r), before, "the same rows are still hidden");
    assert!(hand_hidden_rows(&r).is_empty(), "still by the filter, not by hand");
    assert_eq!(filter_hidden_count(&r) as usize, before.len(), "all of them in the filter's bitmap");
    assert_eq!(filter_range(&r).as_deref(), Some("B4:I10"), "the filter is the sheet's, over the table's range");

    r.execute("data.clearFilter", json!({})).expect("Clear Filter");
    assert!(hidden_rows(&r).is_empty(), "clearing the filter shows every row again");
    assert!(hand_hidden_rows(&r).is_empty());

    let again = reopened(&saved(&r));
    assert!(hidden_rows(&again).is_empty(), "and they stay shown across another save");
}

/// A custom filter (two conditions, and) is the other shape the writer folds into a table.
#[test]
fn a_custom_filter_survives_save_and_reload() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "sales"})).expect("the sample loads");
    s.execute("data.filterBy", json!({"column": "G", "custom": {"op": ">", "value": "200000", "op2": "<", "value2": "400000", "and": true}}))
        .expect("it filters");
    let before = hidden_rows(&s);
    assert!(!before.is_empty(), "the filter hid the rows outside the band");

    let mut r = reopened(&saved(&s));
    let crit = criteria(&r);
    assert!(
        matches!(crit.as_slice(), [(5, FilterCriterion::Custom { a, b: Some(b), and: true })]
            if a == &(">".to_string(), "200000".to_string()) && b == &("<".to_string(), "400000".to_string())),
        "the two conditions were dropped or reordered: {crit:?}"
    );
    assert_eq!(hidden_rows(&r), before, "the same rows are still hidden");
    assert!(hand_hidden_rows(&r).is_empty(), "and by the filter, not by hand");

    r.execute("data.clearFilter", json!({})).expect("Clear Filter");
    assert!(hidden_rows(&r).is_empty(), "clearing the filter shows every row again");
}

/// Turning a table's filter buttons off takes the filter it carried with it, rows and all: a table
/// without them keeps no criteria in the file, so leaving them on the sheet saved the rows hidden
/// and nothing to clear them.
///
/// Found by the command-sequence fuzzer (seed 6, step 89), one step after a `data.filterBy` over
/// the sales sample's table: `table.headerRow` off, then the save/reload invariant reported
/// "rows differ after reload".
#[test]
fn a_table_that_stops_filtering_gives_its_rows_back() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "sales"})).expect("the sample loads");
    s.execute("data.filterBy", json!({"column": "G", "values": ["'007", "121"]})).expect("it filters");
    assert!(!hidden_rows(&s).is_empty(), "the filter hid rows");

    // The filter buttons of the table the filter lives on go.
    s.execute("table.headerRow", json!({"table": "Sales", "on": false})).expect("the buttons go");
    assert!(criteria(&s).is_empty(), "the filter is gone");
    {
        let d = s.doc().expect("document");
        let sh = d.wb.sheet(0).expect("sheet");
        assert!(sh.autofilter.is_none(), "and so is the AutoFilter over the table");
        assert!(sh.tables.iter().all(|t| !t.filter_button && t.filter.is_empty()), "the table carries no filter any more");
        assert!(sh.filter_hidden.is_empty(), "nothing is left in the filter's bitmap");
    }
    assert!(hidden_rows(&s).is_empty(), "its rows are back");
    assert!(hand_hidden_rows(&s).is_empty(), "not left hidden by hand");
    // And the file agrees: every row shows after a save and reload.
    let r = reopened(&saved(&s));
    assert!(hidden_rows(&r).is_empty(), "the reloaded sheet shows every row");
}

/// A row hidden by hand inside a filter's range is the filter's: the file stores a hidden row one
/// way only and the reader gives it back to the filter's bitmap, so hiding it in the line map made
/// a save and reload disagree about where the row went.
///
/// Found by the command-sequence fuzzer (seed 6, step 89): a filter over a selection, then
/// `home.hideRows` on rows inside it.
#[test]
fn a_row_hidden_by_hand_inside_a_filter_belongs_to_the_filter() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "sales"})).expect("the sample loads");
    s.execute("home.mergeCenter", json!({"range": "E13:E20"})).expect("the merge");
    s.execute("selection.set", json!({"range": "D18:H19"})).expect("the selection");
    s.execute("data.filterBy", json!({"column": "G", "values": ["'007", "121"]})).expect("it filters");
    s.execute("home.hideRows", json!({"rows": "15:17"})).expect("the rows are hidden");
    let hidden = hidden_rows(&s);
    assert_eq!(hidden, (13..=19).collect::<Vec<u32>>(), "the filter's rows and the hidden ones");
    assert!(hand_hidden_rows(&s).is_empty(), "all of them are hidden by the filter, not by hand");
    assert_eq!(filter_hidden_count(&s) as usize, hidden.len(), "and all of them in its bitmap");

    let r = reopened(&saved(&s));
    assert_eq!(hidden_rows(&r), hidden, "the same rows show after a reload");
    assert!(hand_hidden_rows(&r).is_empty(), "with none of them moved into the line map");

    // And unhiding them takes them out of the bitmap again, so they stay shown across a save.
    s.execute("home.unhideRows", json!({"rows": "15:17"})).expect("they come back");
    let shown: Vec<u32> = hidden_rows(&s).into_iter().filter(|r| !(14..=16).contains(r)).collect();
    assert_eq!(shown, vec![13, 17, 18, 19], "only the filter's own rows stay hidden");
    let r = reopened(&saved(&s));
    assert_eq!(hidden_rows(&r), shown, "the reloaded sheet hides the same rows");
    assert!(hand_hidden_rows(&r).is_empty());
}

/// Filtering another column after a reload extends the filter that came back instead of
/// installing a second one somewhere else.
#[test]
fn a_reloaded_filter_is_the_filter_the_next_command_edits() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "sales"})).expect("the sample loads");
    s.execute("data.filterBy", json!({"column": "G", "values": ["'007", "121"]})).expect("it filters");
    let mut r = reopened(&saved(&s));
    r.execute("data.filterBy", json!({"column": "B", "values": ["North", "South"]})).expect("it filters a second column");
    assert_eq!(criteria(&r).len(), 2, "both columns' criteria live in the same filter: {:?}", criteria(&r));
    assert!(table_criteria(&r).is_empty());
    r.execute("data.clearFilter", json!({})).expect("Clear Filter");
    assert!(hidden_rows(&r).is_empty(), "clearing it shows every row");
}
