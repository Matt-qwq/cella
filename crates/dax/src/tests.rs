//! The language's own tests: every rule the crate promises, on a model built here in code.
//!
//! The fixture is the smallest one that shows what a measure is for: a fact table with a blank in
//! it, a lookup table with the names a person groups by, and the relationship between them.

use gridcraft_core::Value;
use gridcraft_model::{DataModel, Measure, ModelTable, Relationship};

use crate::{Context, DaxError, MAX_FORMULA_LEN, evaluate, validate};

fn table(name: &str, columns: &[&str], rows: Vec<Vec<Value>>) -> ModelTable {
    ModelTable { name: name.to_string(), columns: columns.iter().map(|c| c.to_string()).collect(), rows, source_query: None }
}

fn rel(from_table: &str, from_column: &str, to_table: &str, to_column: &str) -> Relationship {
    Relationship {
        from_table: from_table.to_string(),
        from_column: from_column.to_string(),
        to_table: to_table.to_string(),
        to_column: to_column.to_string(),
    }
}

fn n(v: f64) -> Value {
    Value::number(v)
}

fn t(s: &str) -> Value {
    Value::text(s)
}

/// Sales(RegionId, Amount) with one blank amount, Region(Id, Name) with two regions, and the
/// relationship that lets a filter on a region reach the sales.
fn model() -> DataModel {
    DataModel {
        tables: vec![
            table(
                "Sales",
                &["RegionId", "Amount", "Note"],
                vec![vec![n(1.0), n(100.0), t("first")], vec![n(2.0), n(50.0), t("second")], vec![n(1.0), Value::Empty, t("third")]],
            ),
            table("Region", &["Id", "Name"], vec![vec![n(1.0), t("West")], vec![n(2.0), t("East")]]),
        ],
        relationships: vec![rel("Sales", "RegionId", "Region", "Id")],
        measures: Vec::new(),
    }
}

/// Sales -> Region -> Country: the two-hop shape that proves the walk follows a chain.
fn chain_model() -> DataModel {
    DataModel {
        tables: vec![
            table("Sales", &["RegionId", "Amount"], vec![vec![n(1.0), n(100.0)], vec![n(2.0), n(50.0)]]),
            table("Region", &["Id", "Name", "CountryId"], vec![vec![n(1.0), t("West"), n(10.0)], vec![n(2.0), t("East"), n(20.0)]]),
            table("Country", &["Id", "Name"], vec![vec![n(10.0), t("North")], vec![n(20.0), t("South")]]),
        ],
        relationships: vec![rel("Sales", "RegionId", "Region", "Id"), rel("Region", "CountryId", "Country", "Id")],
        measures: Vec::new(),
    }
}

fn with_measures(mut model: DataModel, measures: &[(&str, &str)]) -> DataModel {
    for (name, formula) in measures {
        model.measures.push(Measure { name: (*name).to_string(), table: "Sales".to_string(), formula: (*formula).to_string(), format: None });
    }
    model
}

fn value(model: &DataModel, formula: &str, ctx: &Context) -> Value {
    match evaluate(model, formula, ctx) {
        Ok(v) => v,
        Err(e) => panic!("{formula}: {e}"),
    }
}

fn error(model: &DataModel, formula: &str, ctx: &Context) -> DaxError {
    match evaluate(model, formula, ctx) {
        Ok(v) => panic!("{formula} should have been refused, but answered {v:?}"),
        Err(e) => e,
    }
}

// ------------------------------------------------------------------------ aggregates over a table

#[test]
fn sum_over_a_table() {
    let m = model();
    let all = Context::new();
    // Three rows, one of them blank: the blank adds nothing and is not an error.
    assert_eq!(value(&m, "SUM(Sales[Amount])", &all), n(150.0));
    // Function, table and column names ignore case, the way a person types them.
    assert_eq!(value(&m, "sum(sales[amount])", &all), n(150.0));
    // Arithmetic around an aggregate, in the usual precedence.
    assert_eq!(value(&m, "SUM(Sales[Amount]) * 2 + 100", &all), n(400.0));
    assert_eq!(value(&m, "(SUM(Sales[Amount]) + 50) / 2", &all), n(100.0));
    assert_eq!(value(&m, "-SUM(Sales[Amount])", &all), n(-150.0));
}

#[test]
fn count_versus_counta() {
    let m = model();
    let all = Context::new();
    // COUNT counts numbers, so the blank row is not one of them and neither is text.
    assert_eq!(value(&m, "COUNT(Sales[Amount])", &all), n(2.0));
    assert_eq!(value(&m, "COUNT(Sales[Note])", &all), n(0.0));
    // COUNTA counts everything that isn't blank: the three notes, and the two amounts.
    assert_eq!(value(&m, "COUNTA(Sales[Note])", &all), n(3.0));
    assert_eq!(value(&m, "COUNTA(Sales[Amount])", &all), n(2.0));
    // A column of numbers with no blanks in it: the two agree.
    assert_eq!(value(&m, "COUNT(Sales[RegionId])", &all), n(3.0));
    assert_eq!(value(&m, "COUNTA(Sales[RegionId])", &all), n(3.0));
}

#[test]
fn min_max_average() {
    let m = model();
    let all = Context::new();
    assert_eq!(value(&m, "MIN(Sales[Amount])", &all), n(50.0));
    assert_eq!(value(&m, "MAX(Sales[Amount])", &all), n(100.0));
    // 150 over two numbers: the blank is not a zero in the divisor either.
    assert_eq!(value(&m, "AVERAGE(Sales[Amount])", &all), n(75.0));
}

#[test]
fn distinct_count() {
    let m = model();
    let all = Context::new();
    assert_eq!(value(&m, "DISTINCTCOUNT(Sales[RegionId])", &all), n(2.0));
    // 100 and 50, and the blank counts as nothing at all.
    assert_eq!(value(&m, "DISTINCTCOUNT(Sales[Amount])", &all), n(2.0));
    assert_eq!(value(&m, "DISTINCTCOUNT(Region[Name])", &all), n(2.0));
}

// ------------------------------------------------------------------------- the filter context

#[test]
fn a_filter_context_restricts_the_fact_rows() {
    let m = model();
    let west_rows = Context::new().filter("Sales", "RegionId", n(1.0));
    assert_eq!(value(&m, "SUM(Sales[Amount])", &west_rows), n(100.0));
    assert_eq!(value(&m, "COUNT(Sales[Amount])", &west_rows), n(1.0));
    // Both West rows exist, only one of them has an amount.
    assert_eq!(value(&m, "COUNTA(Sales[Amount])", &west_rows), n(1.0));
    assert_eq!(value(&m, "COUNTA(Sales[Note])", &west_rows), n(2.0));

    // A filter that matches no row leaves an empty result, never a zero.
    let nobody = Context::new().filter("Sales", "RegionId", n(9.0));
    assert_eq!(value(&m, "SUM(Sales[Amount])", &nobody), Value::Empty);
    assert_eq!(value(&m, "MIN(Sales[Amount])", &nobody), Value::Empty);
    assert_eq!(value(&m, "COUNT(Sales[Amount])", &nobody), n(0.0));
}

#[test]
fn a_dimension_filter_reaches_the_fact_table_through_the_relationship() {
    let m = model();
    // Region[Name] = "West" -> Region rows with Id 1 -> the Sales rows whose RegionId is 1.
    let west = Context::new().filter("Region", "Name", t("west"));
    assert_eq!(value(&m, "SUM(Sales[Amount])", &west), n(100.0));
    assert_eq!(value(&m, "COUNTA(Sales[Note])", &west), n(2.0));

    let east = Context::new().filter("Region", "Name", t("East"));
    assert_eq!(value(&m, "SUM(Sales[Amount])", &east), n(50.0));

    // Filtering the key column of the dimension reaches the fact just the same.
    let by_id = Context::new().filter("Region", "Id", n(2.0));
    assert_eq!(value(&m, "SUM(Sales[Amount])", &by_id), n(50.0));

    // A dimension value that isn't in the table filters everything out.
    let nowhere = Context::new().filter("Region", "Name", t("Nowhere"));
    assert_eq!(value(&m, "SUM(Sales[Amount])", &nowhere), Value::Empty);

    // The filter does not run backwards: a filter on the fact leaves the dimension's own measure
    // unfiltered, exactly as a one-directional relationship means.
    let fact_side = Context::new().filter("Sales", "RegionId", n(2.0));
    assert_eq!(value(&m, "DISTINCTCOUNT(Region[Name])", &fact_side), n(2.0));
}

#[test]
fn a_two_hop_relationship_chain_propagates() {
    let m = chain_model();
    // Country[Name] = "North" -> Country 10 -> Region 1 -> the Sales rows with RegionId 1.
    let north = Context::new().filter("Country", "Name", t("North"));
    assert_eq!(value(&m, "SUM(Sales[Amount])", &north), n(100.0));

    let south = Context::new().filter("Country", "Name", t("South"));
    assert_eq!(value(&m, "SUM(Sales[Amount])", &south), n(50.0));

    // Both hops at once, ANDed, and a contradictory pair leaves nothing.
    let both = Context::new().filter("Country", "Name", t("North")).filter("Region", "Name", t("West"));
    assert_eq!(value(&m, "SUM(Sales[Amount])", &both), n(100.0));
    let contradiction = Context::new().filter("Country", "Name", t("South")).filter("Region", "Name", t("West"));
    assert_eq!(value(&m, "SUM(Sales[Amount])", &contradiction), Value::Empty);
}

// ------------------------------------------------------------------------------- CALCULATE

#[test]
fn calculate_adds_filters_with_and_semantics() {
    let m = model();
    let all = Context::new();
    // A dimension filter and a fact filter at once: both hold, so the row is counted once.
    assert_eq!(value(&m, "CALCULATE(SUM(Sales[Amount]), Region[Name] = \"West\", Sales[RegionId] = 1)", &all), n(100.0));
    // The same dimension filter with the other region: nothing satisfies both.
    assert_eq!(value(&m, "CALCULATE(SUM(Sales[Amount]), Region[Name] = \"West\", Sales[RegionId] = 2)", &all), Value::Empty);
    // Two filters on the same column intersect rather than replace.
    assert_eq!(value(&m, "CALCULATE(SUM(Sales[Amount]), Sales[RegionId] = 1, Sales[RegionId] = 2)", &all), Value::Empty);
    // A parenthesised filter is the same filter.
    assert_eq!(value(&m, "CALCULATE(SUM(Sales[Amount]), (Region[Name] = \"West\"))", &all), n(100.0));
    // CALCULATE with no filter at all is legal and changes nothing.
    assert_eq!(value(&m, "CALCULATE(SUM(Sales[Amount]))", &all), n(150.0));
    // The filters reach every aggregate in the body, not just the outer one.
    assert_eq!(value(&m, "CALCULATE(SUM(Sales[Amount]) + COUNT(Sales[Amount]), Region[Name] = \"West\")", &all), n(101.0));
}

#[test]
fn calculate_keeps_the_outer_context() {
    let m = model();
    let west = Context::new().filter("Region", "Name", t("West"));
    // The outer context still applies: the inner filter adds to it.
    assert_eq!(value(&m, "CALCULATE(SUM(Sales[Amount]), Sales[RegionId] = 1)", &west), n(100.0));
    // Asking for the other region inside a West context finds nothing, because the outer filter
    // is not thrown away.
    assert_eq!(value(&m, "CALCULATE(SUM(Sales[Amount]), Sales[RegionId] = 2)", &west), Value::Empty);
    // Nested CALCULATEs build on each other.
    assert_eq!(value(&m, "CALCULATE(CALCULATE(SUM(Sales[Amount]), Sales[RegionId] = 1), Sales[Amount] = 100)", &west), n(100.0));
    // A filter's own value is read in the context CALCULATE was called in, before the filter
    // applies. Here the value is the region the outer context already fixed, so it matches.
    assert_eq!(value(&m, "CALCULATE(SUM(Sales[Amount]), Sales[RegionId] = 1, Region[Name] = \"West\")", &west), n(100.0));
}

// ---------------------------------------------------------------------------------- DIVIDE

#[test]
fn divide_returns_blank_on_zero_and_the_alternate_when_given() {
    let m = model();
    let all = Context::new();
    assert_eq!(value(&m, "DIVIDE(10, 4)", &all), n(2.5));
    // Zero is blank, not an error and not an infinity.
    assert_eq!(value(&m, "DIVIDE(10, 0)", &all), Value::Empty);
    // A blank denominator is the same case.
    assert_eq!(value(&m, "DIVIDE(1, BLANK())", &all), Value::Empty);
    // The alternate is the answer when the denominator gives nothing.
    assert_eq!(value(&m, "DIVIDE(10, 0, -1)", &all), n(-1.0));
    assert_eq!(value(&m, "DIVIDE(1, BLANK(), 0)", &all), n(0.0));
    // And a real division over the model.
    assert_eq!(value(&m, "DIVIDE(SUM(Sales[Amount]), COUNT(Sales[Amount]))", &all), n(75.0));
    assert_eq!(value(&m, "DIVIDE(SUM(Sales[Amount]), COUNT(Sales[Amount]), 0)", &all), n(75.0));
}

// --------------------------------------------------------------------------------- measures

#[test]
fn a_measure_can_reference_another_measure() {
    let m = with_measures(model(), &[("Total", "SUM(Sales[Amount])"), ("Doubled", "Total * 2"), ("Spread", "Doubled - Total")]);
    let all = Context::new();
    assert_eq!(value(&m, "Total", &all), n(150.0));
    assert_eq!(value(&m, "Doubled", &all), n(300.0));
    assert_eq!(value(&m, "Spread", &all), n(150.0));
    // A measure is evaluated in the caller's context, so the dimension filter reaches through it.
    let west = Context::new().filter("Region", "Name", t("West"));
    assert_eq!(value(&m, "Doubled", &west), n(200.0));
    // And a measure may be used inside a bigger formula.
    assert_eq!(value(&m, "Total + SUM(Sales[RegionId])", &all), n(154.0));
    // Validating a measure that refers to measures resolves them without evaluating anything.
    assert_eq!(validate(&m, "Doubled"), Ok(()));
}

#[test]
fn a_measure_cycle_is_refused() {
    let m = with_measures(model(), &[("LoopA", "LoopB"), ("LoopB", "LoopA")]);
    let all = Context::new();
    match error(&m, "LoopA", &all) {
        DaxError::Cycle { measure } => assert!(measure == "LoopA" || measure == "LoopB", "{measure}"),
        other => panic!("expected a cycle, got {other:?}"),
    }
    // The same for a measure that simply names itself.
    let itself = with_measures(model(), &[("Self", "Self + 1")]);
    assert!(matches!(error(&itself, "Self", &all), DaxError::Cycle { .. }));
    // validate refuses it too, which is what the Add Measure dialog shows.
    assert!(matches!(validate(&m, "LoopA"), Err(DaxError::Cycle { .. })));
    // A cycle deeper in a chain is still caught.
    let deep = with_measures(model(), &[("A", "B"), ("B", "C"), ("C", "A")]);
    assert!(matches!(error(&deep, "A", &all), DaxError::Cycle { .. }));
}

// ------------------------------------------------------------------------------------ errors

#[test]
fn every_error_names_what_is_wrong() {
    let m = model();
    let all = Context::new();

    // Parse: the offset is 0-based and counts characters, so it points at the real culprit.
    let e = error(&m, "1 + )", &all);
    assert!(matches!(e, DaxError::Parse { at: 4, .. }), "{e:?}");
    assert!(e.to_string().contains("character 4"), "{e}");

    // An equals sign outside a CALCULATE filter is a mistake, and it says so where it found it.
    let e = error(&m, "1 = 1", &all);
    assert!(matches!(e, DaxError::Parse { at: 2, .. }), "{e:?}");
    assert!(e.to_string().contains("CALCULATE"), "{e}");

    // An unterminated call points past the last character.
    let e = error(&m, "SUM(Sales[Amount]", &all);
    assert!(matches!(e, DaxError::Parse { at: 17, .. }), "{e:?}");

    let e = error(&m, "MEDIAN(Sales[Amount])", &all);
    assert!(matches!(e, DaxError::UnknownFunction { .. }), "{e:?}");
    assert!(e.to_string().contains("MEDIAN"), "{e}");

    let e = error(&m, "SUM(Ghost[Amount])", &all);
    assert!(matches!(e, DaxError::UnknownTable { .. }), "{e:?}");
    assert!(e.to_string().contains("Ghost"), "{e}");

    let e = error(&m, "SUM(Sales[Nope])", &all);
    assert!(matches!(e, DaxError::UnknownColumn { .. }), "{e:?}");
    assert!(e.to_string().contains("Sales") && e.to_string().contains("Nope"), "{e}");

    let e = error(&m, "Total * 2", &all);
    assert!(matches!(e, DaxError::UnknownMeasure { .. }), "{e:?}");
    assert!(e.to_string().contains("Total"), "{e}");

    let e = error(&m, "SUM(Sales[Amount], Sales[Amount])", &all);
    assert!(matches!(e, DaxError::BadArity { expected: 1, got: 2, .. }), "{e:?}");
    assert!(e.to_string().contains("SUM") && e.to_string().contains("2"), "{e}");

    let e = error(&m, "SUM(1 + 1)", &all);
    assert!(matches!(e, DaxError::NotANumber { .. }), "{e:?}");
    assert!(e.to_string().contains("column"), "{e}");

    let e = error(&m, "Sales[Amount]", &all);
    assert!(matches!(e, DaxError::UnaggregatedColumn { .. }), "{e:?}");
    assert!(e.to_string().contains("Sales[Amount]"), "{e}");

    // A measure that comes back around names the measure it caught.
    let looped = with_measures(model(), &[("LoopA", "LoopB"), ("LoopB", "LoopA")]);
    let e = error(&looped, "LoopA", &all);
    assert!(matches!(e, DaxError::Cycle { .. }), "{e:?}");
    assert!(e.to_string().contains("LoopA") || e.to_string().contains("LoopB"), "{e}");

    // More measures chained than the cap allows.
    let chain: Vec<(String, String)> = (0..20).map(|i| (format!("A{i}"), if i == 19 { "1".to_string() } else { format!("A{}", i + 1) })).collect();
    let refs: Vec<(&str, &str)> = chain.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let deep = with_measures(model(), &refs);
    let e = error(&deep, "A0", &all);
    assert!(matches!(e, DaxError::TooDeep { .. }), "{e:?}");
    assert!(e.to_string().contains("16"), "{e}");

    // A model a hand edit could hold: two relationships between the same tables, pointing both
    // ways. The walk is bounded instead of following it round.
    let mut hand_edited = model();
    hand_edited.relationships.push(rel("Region", "Id", "Sales", "RegionId"));
    let e = error(&hand_edited, "SUM(Sales[Amount])", &all);
    assert!(matches!(e, DaxError::RelationshipLoop { .. }), "{e:?}");
    assert!(e.to_string().contains("loop"), "{e}");
}

// -------------------------------------------------------------------------------------- caps

#[test]
fn a_long_formula_is_refused_before_parsing() {
    // Not even a formula: 4,097 opening parentheses. The length is checked first, so the error is
    // about the length rather than about nesting.
    let long = "(".repeat(MAX_FORMULA_LEN + 1);
    let e = error(&model(), &long, &Context::new());
    assert!(matches!(e, DaxError::Parse { at, .. } if at == MAX_FORMULA_LEN), "{e:?}");
    assert!(e.to_string().contains("4096"), "{e}");
    assert!(!e.to_string().contains("nests"), "{e}");

    // Exactly at the cap is still read; the refusal is only past it.
    let base = "SUM(Sales[Amount])";
    let at_cap = format!("{base}{}", " ".repeat(MAX_FORMULA_LEN - base.chars().count()));
    assert_eq!(at_cap.chars().count(), MAX_FORMULA_LEN);
    assert_eq!(value(&model(), &at_cap, &Context::new()), n(150.0));
}

#[test]
fn deep_nesting_is_refused() {
    let deep = format!("{}1{}", "(".repeat(40), ")".repeat(40));
    let e = error(&model(), &deep, &Context::new());
    assert!(matches!(e, DaxError::Parse { .. }), "{e:?}");
    assert!(e.to_string().contains("32"), "{e}");

    // Nesting that stays inside the cap is read normally.
    assert_eq!(value(&model(), "((((1))))", &Context::new()), n(1.0));
    let inside = format!("{}SUM(Sales[Amount]){}", "(".repeat(20), ")".repeat(20));
    assert_eq!(value(&model(), &inside, &Context::new()), n(150.0));
}

// ------------------------------------------------------------------------ one type per column

fn text_model() -> DataModel {
    DataModel {
        tables: vec![table(
            "Texty",
            &["Label", "Amount", "Mixed"],
            vec![vec![t("a"), t("12"), t("12")], vec![t("b"), t("3.5"), t("3.5")], vec![t("c"), Value::Empty, n(4.0)]],
        )],
        relationships: Vec::new(),
        measures: Vec::new(),
    }
}

#[test]
fn text_in_a_number_column_sums_to_blank() {
    let m = text_model();
    let all = Context::new();
    // "12" and "3.5" are text: they are not added up and they do not become numbers.
    assert_eq!(value(&m, "SUM(Texty[Amount])", &all), Value::Empty);
    assert_eq!(value(&m, "AVERAGE(Texty[Amount])", &all), Value::Empty);
    assert_eq!(value(&m, "MIN(Texty[Amount])", &all), Value::Empty);
    assert_eq!(value(&m, "MAX(Texty[Amount])", &all), Value::Empty);
    // They are still there, so COUNTA sees them and COUNT does not.
    assert_eq!(value(&m, "COUNT(Texty[Amount])", &all), n(0.0));
    assert_eq!(value(&m, "COUNTA(Texty[Amount])", &all), n(2.0));
    assert_eq!(value(&m, "DISTINCTCOUNT(Texty[Amount])", &all), n(2.0));

    // A mixed column adds up only the real numbers, not the text that looks like one.
    assert_eq!(value(&m, "SUM(Texty[Mixed])", &all), n(4.0));
    assert_eq!(value(&m, "COUNT(Texty[Mixed])", &all), n(1.0));
    assert_eq!(value(&m, "COUNTA(Texty[Mixed])", &all), n(3.0));
    // Filtering by a text value matches the text, and never a number that reads the same.
    let by_text = Context::new().filter("Texty", "Mixed", t("12"));
    assert_eq!(value(&m, "COUNTA(Texty[Mixed])", &by_text), n(1.0));
    assert_eq!(value(&m, "SUM(Texty[Mixed])", &by_text), Value::Empty);
}

// ----------------------------------------------------------------------------------- validate

#[test]
fn validate_resolves_names_without_evaluating() {
    let m = with_measures(model(), &[("Total", "SUM(Sales[Amount])")]);
    assert_eq!(validate(&m, "SUM(Sales[Amount])"), Ok(()));
    assert_eq!(validate(&m, "CALCULATE(Total * 2, Region[Name] = \"West\")"), Ok(()));
    assert_eq!(validate(&m, "DIVIDE(Total, COUNT(Sales[Amount]), 0)"), Ok(()));
    assert_eq!(validate(&m, "BLANK()"), Ok(()));

    assert!(matches!(validate(&m, "SUM(Ghost[Amount])"), Err(DaxError::UnknownTable { .. })));
    assert!(matches!(validate(&m, "SUM(Sales[Nope])"), Err(DaxError::UnknownColumn { .. })));
    assert!(matches!(validate(&m, "Nothing"), Err(DaxError::UnknownMeasure { .. })));
    assert!(matches!(validate(&m, "NOPE(1)"), Err(DaxError::UnknownFunction { .. })));
    assert!(matches!(validate(&m, "Sales[Amount]"), Err(DaxError::UnaggregatedColumn { .. })));

    // A measure whose own formula is broken reports what is wrong in it.
    let broken = with_measures(model(), &[("Bad", "SUM(Sales[Nope])")]);
    assert!(matches!(validate(&broken, "Bad + 1"), Err(DaxError::UnknownColumn { .. })));
    assert!(matches!(evaluate(&broken, "Bad + 1", &Context::new()), Err(DaxError::UnknownColumn { .. })));
}
