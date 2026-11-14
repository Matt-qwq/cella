//! The pipeline's own tests: each step's contract, the errors a person will hit, and the caps.

use gridcraft_core::{CellError, Value};
use gridcraft_model::{QueryAggregate, QueryColumnType, QueryFilterOp, QueryStep};

use crate::*;

fn t(columns: &[&str], rows: &[&[&str]]) -> Table {
    let cols: Vec<String> = columns.iter().map(|c| c.to_string()).collect();
    let rows: Vec<Vec<Value>> = rows.iter().map(|r| r.iter().map(|c| if *c == "~" { Value::Empty } else { Value::text(*c) }).collect()).collect();
    Table::new(cols, rows).unwrap()
}

fn texts(table: &Table) -> Vec<Vec<String>> {
    table.rows.iter().map(|r| r.iter().map(crate::cell_text).collect()).collect()
}

#[test]
fn rows_are_padded_to_the_header_width() {
    let table = Table::from_rows(vec![vec![Value::text("a"), Value::text("b"), Value::text("c")], vec![Value::number(1.0)]], true).unwrap();
    assert_eq!(table.columns, ["a", "b", "c"]);
    assert_eq!(table.rows, vec![vec![Value::number(1.0), Value::Empty, Value::Empty]]);
}

#[test]
fn a_headerless_import_gets_column_names() {
    let table = Table::from_rows(vec![vec![Value::number(1.0), Value::number(2.0)]], false).unwrap();
    assert_eq!(table.columns, ["Column1", "Column2"]);
    assert_eq!(table.height(), 1);
}

#[test]
fn blank_and_duplicate_headers_are_made_unique() {
    let table = Table::from_rows(
        vec![
            vec![Value::text("Amount"), Value::Empty, Value::text("amount"), Value::text("Amount")],
            vec![Value::number(1.0), Value::number(2.0), Value::number(3.0), Value::number(4.0)],
        ],
        true,
    )
    .unwrap();
    assert_eq!(table.columns, ["Amount", "Column2", "amount2", "Amount3"]);
}

#[test]
fn promote_headers_takes_the_first_row_and_numbers_the_names() {
    let mut table = t(&["Column1", "Column2"], &[&["Region", "Sales"], &["West", "10"]]);
    table = apply(table, &[QueryStep::PromoteHeaders]).unwrap();
    assert_eq!(table.columns, ["Region", "Sales"]);
    assert_eq!(texts(&table), vec![vec!["West", "10"]]);
}

#[test]
fn promote_on_an_empty_table_is_an_error_naming_the_step() {
    let err = apply(Table::empty(), &[QueryStep::PromoteHeaders]).unwrap_err();
    assert!(matches!(err, QueryError::Invalid { index: 0, step: "Promoted Headers", .. }), "{err}");
    assert!(err.to_string().starts_with("step 0 (Promoted Headers): "), "{err}");
}

#[test]
fn change_type_converts_and_marks_what_it_cannot() {
    let table = t(&["A", "B"], &[&["1", "yes"], &["2.5", "x"], &["~", "no"]]);
    let out = apply(table, &[QueryStep::ChangeType { columns: vec!["A".into()], to: QueryColumnType::Number }]).unwrap();
    assert_eq!(out.rows[0][0], Value::number(1.0));
    assert_eq!(out.rows[2][0], Value::Empty);
    let out = apply(out, &[QueryStep::ChangeType { columns: vec!["B".into()], to: QueryColumnType::Boolean }]).unwrap();
    assert_eq!(out.rows[0][1], Value::Bool(true));
    assert_eq!(out.rows[1][1], Value::Error(CellError::Value));
}

#[test]
fn change_type_with_no_columns_changes_every_column() {
    let table = t(&["A", "B"], &[&["1", "2"]]);
    let out = apply(table, &[QueryStep::ChangeType { columns: Vec::new(), to: QueryColumnType::WholeNumber }]).unwrap();
    assert_eq!(out.rows[0], vec![Value::number(1.0), Value::number(2.0)]);
}

#[test]
fn whole_number_truncates_and_date_reads_iso() {
    let whole = apply(t(&["A"], &[&["-2.7"], &["2.9"]]), &[QueryStep::ChangeType { columns: Vec::new(), to: QueryColumnType::WholeNumber }]).unwrap();
    assert_eq!(whole.rows[0][0], Value::number(-2.0));
    assert_eq!(whole.rows[1][0], Value::number(2.0));
    // 2024-03-04 is serial 45355, a serial already given stays, and 2024/13/01 (no month 13) is a
    // #VALUE! rather than a guess.
    let dates = apply(
        t(&["A"], &[&["2024-03-04"], &["45355"], &["2024/13/01"]]),
        &[QueryStep::ChangeType { columns: Vec::new(), to: QueryColumnType::Date }],
    )
    .unwrap();
    assert_eq!(dates.rows[0][0], Value::number(45355.0));
    assert_eq!(dates.rows[1][0], Value::number(45355.0));
    assert_eq!(dates.rows[2][0], Value::Error(CellError::Value));
}

#[test]
fn a_step_that_names_a_missing_column_names_it_in_the_error() {
    let err = apply(t(&["A"], &[&["1"]]), &[QueryStep::RemoveColumns { columns: vec!["Nope".into()] }]).unwrap_err();
    assert!(matches!(&err, QueryError::NoColumn { column, .. } if column == "Nope"), "{err}");
    assert_eq!(err.to_string(), "step 0 (Removed Columns): there is no column named \"Nope\"");
}

#[test]
fn removing_every_column_is_refused() {
    let err = apply(t(&["A", "B"], &[&["1", "2"]]), &[QueryStep::RemoveColumns { columns: vec!["a".into(), "B".into()] }]).unwrap_err();
    assert!(matches!(err, QueryError::Invalid { .. }), "{err}");
}

#[test]
fn keep_columns_reorders_as_it_keeps() {
    let table = t(&["A", "B", "C"], &[&["1", "2", "3"]]);
    let out = apply(table, &[QueryStep::KeepColumns { columns: vec!["C".into(), "A".into()] }]).unwrap();
    // The kept columns stay in the table's own order, not the order they were listed in.
    assert_eq!(out.columns, ["A", "C"]);
    assert_eq!(texts(&out), vec![vec!["1", "3"]]);
}

#[test]
fn rename_refuses_a_blank_or_a_taken_name() {
    let table = t(&["A", "B"], &[&["1", "2"]]);
    let err = apply(table.clone(), &[QueryStep::RenameColumn { from: "A".into(), to: "  ".into() }]).unwrap_err();
    assert!(matches!(err, QueryError::Invalid { .. }));
    let err = apply(table.clone(), &[QueryStep::RenameColumn { from: "A".into(), to: "b".into() }]).unwrap_err();
    assert!(matches!(err, QueryError::Invalid { .. }));
    let ok = apply(table, &[QueryStep::RenameColumn { from: "A".into(), to: "A2".into() }]).unwrap();
    assert_eq!(ok.columns, ["A2", "B"]);
}

#[test]
fn filter_compares_numbers_as_numbers_and_text_without_case() {
    let table = t(&["Region", "Sales"], &[&["West", "10"], &["east", "9"], &["West", "100"]]);
    let west = apply(table.clone(), &[QueryStep::Filter { column: "Region".into(), op: QueryFilterOp::Equals, value: Some("west".into()) }]).unwrap();
    assert_eq!(texts(&west), vec![vec!["West", "10"], vec!["West", "100"]]);
    let over = apply(table, &[QueryStep::Filter { column: "Sales".into(), op: QueryFilterOp::Greater, value: Some("9".into()) }]).unwrap();
    assert_eq!(texts(&over), vec![vec!["West", "10"], vec!["West", "100"]]);
}

#[test]
fn blank_filters_only_match_blanks() {
    let table = t(&["A"], &[&["~"], &["x"]]);
    let blanks = apply(table.clone(), &[QueryStep::Filter { column: "A".into(), op: QueryFilterOp::IsBlank, value: None }]).unwrap();
    assert_eq!(blanks.height(), 1);
    let not_equals = apply(table, &[QueryStep::Filter { column: "A".into(), op: QueryFilterOp::NotEquals, value: Some("y".into()) }]).unwrap();
    assert_eq!(not_equals.height(), 2);
}

#[test]
fn sort_is_stable_and_puts_blanks_last() {
    let table = t(&["k", "v"], &[&["b", "1"], &["a", "2"], &["~", "3"], &["a", "4"]]);
    let out = apply(table, &[QueryStep::Sort { keys: vec![("k".into(), false)] }]).unwrap();
    assert_eq!(texts(&out), vec![vec!["a", "2"], vec!["a", "4"], vec!["b", "1"], vec!["", "3"]]);
    let desc = apply(out, &[QueryStep::Sort { keys: vec![("k".into(), true)] }]).unwrap();
    assert_eq!(texts(&desc)[0][0], "b");
}

#[test]
fn sorting_two_keys_breaks_ties_in_order() {
    let table = t(&["k", "v"], &[&["a", "2"], &["a", "1"], &["b", "0"]]);
    let out = apply(table, &[QueryStep::Sort { keys: vec![("k".into(), false), ("v".into(), true)] }]).unwrap();
    assert_eq!(texts(&out), vec![vec!["a", "2"], vec!["a", "1"], vec!["b", "0"]]);
}

#[test]
fn duplicates_are_compared_over_the_named_columns_only() {
    let table = t(&["k", "v"], &[&["a", "1"], &["a", "2"], &["b", "3"]]);
    let out = apply(table, &[QueryStep::RemoveDuplicates { columns: vec!["k".into()] }]).unwrap();
    assert_eq!(texts(&out), vec![vec!["a", "1"], vec!["b", "3"]]);
    let all = t(&["k", "v"], &[&["a", "1"], &["a", "1"], &["a", "2"]]);
    assert_eq!(apply(all, &[QueryStep::RemoveDuplicates { columns: Vec::new() }]).unwrap().height(), 2);
}

#[test]
fn replace_keeps_the_cell_type_and_can_fill_blanks() {
    let table =
        Table::new(vec!["n".into(), "t".into()], vec![vec![Value::number(1.0), Value::text("x")], vec![Value::number(2.0), Value::Empty]]).unwrap();
    let out = apply(
        table,
        &[
            QueryStep::ReplaceValues { column: "n".into(), from: "2".into(), to: "20".into() },
            QueryStep::ReplaceValues { column: "t".into(), from: String::new(), to: "none".into() },
        ],
    )
    .unwrap();
    assert_eq!(out.rows[0][0], Value::number(1.0));
    assert_eq!(out.rows[1][0], Value::number(20.0));
    assert_eq!(texts(&out)[1][1], "none");
}

#[test]
fn fill_down_copies_the_last_value_only_downward() {
    let table = t(&["A"], &[&["~"], &["x"], &["~"], &["~"], &["y"], &["~"]]);
    let out = apply(table, &[QueryStep::FillDown { column: "A".into() }]).unwrap();
    assert_eq!(out.rows.iter().map(|r| crate::cell_text(&r[0])).collect::<Vec<_>>(), ["", "x", "x", "x", "y", "y"]);
}

#[test]
fn unpivot_makes_one_row_per_value_and_names_its_columns() {
    let table = t(&["Region", "Q1", "Q2"], &[&["West", "1", "2"], &["East", "3", "4"]]);
    let out = apply(table, &[QueryStep::Unpivot { attributes: vec!["Region".into()], values: Vec::new() }]).unwrap();
    assert_eq!(out.columns, ["Region", "Attribute", "Value"]);
    assert_eq!(texts(&out), vec![vec!["West", "Q1", "1"], vec!["West", "Q2", "2"], vec!["East", "Q1", "3"], vec!["East", "Q2", "4"],]);
}

#[test]
fn unpivot_refuses_a_column_that_is_both_kept_and_unpivoted() {
    let table = t(&["A", "B"], &[&["1", "2"]]);
    let err = apply(table, &[QueryStep::Unpivot { attributes: vec!["A".into()], values: vec!["a".into()] }]).unwrap_err();
    assert!(matches!(err, QueryError::Invalid { .. }), "{err}");
}

#[test]
fn group_by_counts_rows_and_sums_only_numbers() {
    let table = Table::new(
        vec!["Region".into(), "Amount".into()],
        vec![
            vec![Value::text("West"), Value::number(10.0)],
            vec![Value::text("West"), Value::Empty],
            vec![Value::text("East"), Value::number(1.0)],
            vec![Value::text("West"), Value::number(5.0)],
        ],
    )
    .unwrap();
    let out = apply(
        table,
        &[QueryStep::GroupBy {
            by: vec!["Region".into()],
            aggregates: vec![
                (QueryAggregate::Count, None, "Rows".into()),
                (QueryAggregate::Sum, Some("Amount".into()), "Total".into()),
                (QueryAggregate::Average, Some("Amount".into()), "Avg".into()),
            ],
        }],
    )
    .unwrap();
    assert_eq!(out.columns, ["Region", "Rows", "Total", "Avg"]);
    assert_eq!(out.rows[0], vec![Value::text("West"), Value::number(3.0), Value::number(15.0), Value::number(7.5)]);
    assert_eq!(out.rows[1], vec![Value::text("East"), Value::number(1.0), Value::number(1.0), Value::number(1.0)]);
}

#[test]
fn group_by_refuses_a_result_name_it_already_has_and_an_aggregate_without_a_column() {
    let table = t(&["Region", "Amount"], &[&["West", "10"]]);
    let err = apply(
        table.clone(),
        &[QueryStep::GroupBy { by: vec!["Region".into()], aggregates: vec![(QueryAggregate::Sum, Some("Amount".into()), "region".into())] }],
    )
    .unwrap_err();
    assert!(matches!(err, QueryError::Invalid { .. }), "{err}");
    let err =
        apply(table, &[QueryStep::GroupBy { by: vec!["Region".into()], aggregates: vec![(QueryAggregate::Sum, None, "Total".into())] }]).unwrap_err();
    assert!(matches!(err, QueryError::Invalid { .. }), "{err}");
}

#[test]
fn a_group_of_blanks_averages_to_blank_not_zero() {
    let table = t(&["k", "v"], &[&["a", "~"], &["a", "~"]]);
    let out =
        apply(table, &[QueryStep::GroupBy { by: vec!["k".into()], aggregates: vec![(QueryAggregate::Average, Some("v".into()), "avg".into())] }])
            .unwrap();
    assert_eq!(out.rows[0][1], Value::Empty);
}

#[test]
fn a_pipeline_reports_the_index_of_the_step_that_failed() {
    let table = t(&["Column1"], &[&["A"], &["1"]]);
    let steps = vec![
        QueryStep::PromoteHeaders,
        QueryStep::ChangeType { columns: vec!["A".into()], to: QueryColumnType::Number },
        QueryStep::RenameColumn { from: "A".into(), to: "B".into() },
        QueryStep::Filter { column: "A".into(), op: QueryFilterOp::Equals, value: Some("1".into()) },
    ];
    let err = apply(table, &steps).unwrap_err();
    assert!(matches!(err, QueryError::NoColumn { index: 3, .. }), "{err}");
    assert!(err.to_string().starts_with("step 3 (Filtered Rows):"), "{err}");
}

#[test]
fn the_table_cap_is_checked_from_the_declared_size() {
    // One row of 16,385 columns is over Excel's width, without building a million rows to prove it.
    let columns: Vec<String> = (0..MAX_COLUMNS + 1).map(|i| format!("c{i}")).collect();
    let err = Table::new(columns, vec![Vec::new()]).unwrap_err();
    assert!(matches!(err, QueryError::TooLarge { limit, .. } if limit == MAX_CELLS), "{err}");
    // 4M cells is the budget: 3,000 × 1,500 is refused, 2,000 × 2,000 is exactly the budget.
    let wide = (0..3_000).map(|i| format!("c{i}")).collect();
    assert!(Table::new(wide, vec![Vec::new(); 1_500]).is_err());
    let ok = (0..2_000).map(|i| format!("c{i}")).collect();
    assert!(Table::new(ok, vec![Vec::new(); 2_000]).is_ok());
}

#[test]
fn a_step_that_grows_past_the_cap_is_refused_not_allocated() {
    // Unpivot grows rows by the number of value columns; 4 columns over 1,048,576 rows is over
    // both the row cap and the cell budget, and the error comes from the check, not from a panic.
    let columns = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let mut table = Table::new(columns, Vec::new()).unwrap();
    table.rows = vec![vec![Value::number(1.0); 3]; MAX_ROWS];
    let err = apply(table, &[QueryStep::Unpivot { attributes: vec!["a".into()], values: Vec::new() }]).unwrap_err();
    assert!(matches!(err, QueryError::TooLarge { .. }), "{err}");
}

#[test]
fn a_query_with_no_steps_is_the_source_unchanged() {
    let table = t(&["A"], &[&["1"]]);
    assert_eq!(apply(table.clone(), &[]).unwrap(), table);
}
