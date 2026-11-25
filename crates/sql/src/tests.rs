//! What the engine promises, and what it refuses.

use super::*;

#[test]
fn an_identifier_is_quoted_not_trusted() {
    assert_eq!(quote_ident("Sales"), "\"Sales\"");
    assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
}

#[test]
fn a_literal_is_quoted_not_trusted() {
    assert_eq!(quote_literal("C:\\data\\sales.parquet"), "'C:\\data\\sales.parquet'");
    assert_eq!(quote_literal("it's"), "'it''s'");
}

#[test]
fn a_column_of_whole_numbers_is_an_integer() {
    let values = [Value::number(1.0), Value::Empty, Value::number(-3.0)];
    assert_eq!(Type::infer(values.iter()), Type::Int64);
}

#[test]
fn a_column_with_a_fraction_or_a_huge_number_is_a_double() {
    assert_eq!(Type::infer([Value::number(1.5)].iter()), Type::Double);
    // 2^53 + 2 is not exactly an f64 integer any more: keep the double.
    assert_eq!(Type::infer([Value::number(9_007_199_254_740_994.0)].iter()), Type::Double);
    // A cell never holds a NaN: `Value::number` turns one into `#NUM!`, and an error in a column
    // makes the column text (the error is written as its text, `#NUM!`).
    assert_eq!(Type::infer([Value::number(f64::NAN)].iter()), Type::Text);
}

#[test]
fn a_column_of_booleans_is_a_boolean_and_a_mixed_one_is_text() {
    assert_eq!(Type::infer([Value::Bool(true), Value::Empty].iter()), Type::Bool);
    assert_eq!(Type::infer([Value::Bool(true), Value::number(1.0)].iter()), Type::Text);
    assert_eq!(Type::infer([Value::text("west")].iter()), Type::Text);
}

#[test]
fn a_column_of_nothing_is_text() {
    assert_eq!(Type::infer([Value::Empty].iter()), Type::Text);
    assert_eq!(Type::infer(Vec::<Value>::new().iter()), Type::Text);
}

#[test]
fn the_table_surface_answers_without_panicking() {
    let table = Table { columns: vec!["a".into()], rows: vec![vec![Value::number(1.0)]] };
    assert_eq!(table.width(), 1);
    assert_eq!(table.height(), 1);
    assert!(!table.is_empty());
    assert_eq!(table.cell(0, 0), &Value::number(1.0));
    // Outside the table: a blank, not an index panic.
    assert_eq!(table.cell(9, 9), &Value::Empty);
    assert!(Table::default().is_empty());
}

#[test]
fn a_statement_that_does_not_read_is_refused() {
    for sql in [
        "SELECT 1",
        "  select region from sales",
        "WITH x AS (SELECT 1) SELECT * FROM x",
        "-- a comment\nSELECT 1",
        "/* note */ SELECT 1",
        "DESCRIBE sales",
        "SHOW TABLES",
        "SUMMARIZE sales",
        "VALUES (1)",
    ] {
        assert!(is_read_only(sql), "{sql}");
    }
    for sql in [
        "DROP TABLE sales",
        "COPY (SELECT 1) TO 'x.parquet'",
        "ATTACH 'other.db' AS o",
        "INSERT INTO sales VALUES (1)",
        "CREATE TABLE t (a INTEGER)",
        "PRAGMA database_list",
        "INSTALL httpfs",
        "LOAD httpfs",
        "EXPORT DATABASE 'dir'",
        "-- get everything\nDROP TABLE sales",
        "/* nested */ /* still refused */ DELETE FROM sales",
        "",
    ] {
        assert!(!is_read_only(sql), "{sql}");
    }
}

// ---------------------------------------------------------------- without the engine

#[cfg(not(all(feature = "duckdb", not(target_arch = "wasm32"))))]
mod without_engine {
    use super::*;

    #[test]
    fn a_build_without_the_engine_says_so() {
        assert!(!available());
        assert_eq!(Engine::in_memory().unwrap_err(), SqlError::Unavailable);
    }
}

// ---------------------------------------------------------------- with the engine

#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
mod with_engine {
    use super::*;

    fn engine() -> Engine {
        let e = Engine::in_memory().unwrap();
        assert!(available());
        e
    }

    fn temp_path(extension: &str) -> std::path::PathBuf {
        // One name per process and test file kind: tests run in parallel and must not share one.
        let mut path = std::env::temp_dir();
        path.push(format!("cella-sql-test-{}.{extension}", std::process::id()));
        path
    }

    #[test]
    fn a_query_returns_numbers_text_booleans_and_blanks() {
        let table = engine().query("SELECT 42 AS n, 2.5 AS f, 'west' AS s, NULL AS z, TRUE AS b").unwrap();
        assert_eq!(table.columns, vec!["n", "f", "s", "z", "b"]);
        assert_eq!(table.rows, vec![vec![Value::number(42.0), Value::number(2.5), Value::text("west"), Value::Empty, Value::Bool(true)]]);
    }

    #[test]
    fn a_group_by_sums_the_rows_it_was_given() {
        let mut engine = engine();
        let columns = vec!["region".to_string(), "amount".to_string()];
        let rows = vec![
            vec![Value::text("West"), Value::number(1.0)],
            vec![Value::text("East"), Value::number(2.0)],
            vec![Value::text("West"), Value::number(3.0)],
        ];
        assert_eq!(engine.load_rows("sales", &columns, &rows).unwrap(), 3);
        let table = engine.query("SELECT region, SUM(amount) AS total FROM sales GROUP BY 1 ORDER BY 1").unwrap();
        assert_eq!(table.rows, vec![vec![Value::text("East"), Value::number(2.0)], vec![Value::text("West"), Value::number(4.0)]]);
    }

    #[test]
    fn a_short_row_is_loaded_as_blanks() {
        let mut engine = engine();
        let columns = vec!["a".to_string(), "b".to_string()];
        let rows = vec![vec![Value::number(1.0)], vec![Value::number(2.0), Value::number(3.0)]];
        engine.load_rows("ragged", &columns, &rows).unwrap();
        let table = engine.query("SELECT a, b FROM ragged ORDER BY a").unwrap();
        assert_eq!(table.rows, vec![vec![Value::number(1.0), Value::Empty], vec![Value::number(2.0), Value::number(3.0)]]);
    }

    #[test]
    fn a_whole_number_column_is_an_integer_and_a_mixed_one_is_text() {
        let mut engine = engine();
        let columns = vec!["whole".to_string(), "mixed".to_string()];
        let rows = vec![vec![Value::number(1.0), Value::number(1.0)], vec![Value::number(2.0), Value::text("2")]];
        engine.load_rows("typed", &columns, &rows).unwrap();
        let table = engine.query("SELECT typeof(whole) AS w, typeof(mixed) AS m FROM typed LIMIT 1").unwrap();
        assert_eq!(table.rows, vec![vec![Value::text("BIGINT"), Value::text("VARCHAR")]]);
    }

    #[test]
    fn a_date_comes_back_as_the_serial_number_a_cell_shows() {
        // 2024-01-01 is day 45292 in the 1900 date system.
        let table = engine().query("SELECT DATE '2024-01-01' AS d").unwrap();
        assert_eq!(table.rows, vec![vec![Value::number(45292.0)]]);
    }

    #[test]
    fn a_table_name_with_a_quote_in_it_is_a_name_not_a_second_statement() {
        let mut engine = engine();
        let columns = vec!["a".to_string()];
        engine.load_rows("keep", &columns, &[vec![Value::number(1.0)]]).unwrap();
        engine.load_rows("Sales\"; DROP TABLE keep; --", &columns, &[vec![Value::number(2.0)]]).unwrap();
        let table = engine.query("SELECT COUNT(*) AS n FROM keep").unwrap();
        assert_eq!(table.rows, vec![vec![Value::number(1.0)]]);
        let hostile = engine.query("SELECT a FROM \"Sales\"\"; DROP TABLE keep; --\"").unwrap();
        assert_eq!(hostile.rows, vec![vec![Value::number(2.0)]]);
    }

    #[test]
    fn a_result_bigger_than_a_table_is_refused_while_it_is_read() {
        let engine = engine();
        let error = engine.query("SELECT * FROM range(2000000)").unwrap_err();
        assert!(matches!(error, SqlError::TooBig { .. }), "{error:?}");
    }

    #[test]
    fn bad_sql_is_an_error_not_a_panic() {
        let engine = engine();
        assert!(matches!(engine.query("SELECT FROM"), Err(SqlError::Engine(_))));
        assert!(matches!(engine.query("SELECT * FROM nowhere"), Err(SqlError::Engine(_))));
        assert!(matches!(engine.query("   "), Err(SqlError::Invalid(_))));
        assert!(matches!(engine.query(""), Err(SqlError::Invalid(_))));
    }

    #[test]
    fn a_name_or_a_path_is_checked_before_it_reaches_the_engine() {
        let mut engine = engine();
        let columns = vec!["a".to_string()];
        assert!(matches!(engine.load_rows("", &columns, &[]), Err(SqlError::Invalid(_))));
        assert!(matches!(engine.load_rows("x", &[], &[]), Err(SqlError::Invalid(_))));
        let long = "x".repeat(MAX_NAME + 1);
        assert!(matches!(engine.load_rows(&long, &columns, &[]), Err(SqlError::Invalid(_))));
        assert!(matches!(engine.load_file("t", &[], FileFormat::Parquet), Err(SqlError::Invalid(_))));
        assert!(matches!(engine.load_file("t", &[String::new()], FileFormat::Csv), Err(SqlError::Invalid(_))));
    }

    #[test]
    fn a_file_the_engine_can_read_is_loaded_as_typed_columns() {
        let mut engine = engine();
        let path = temp_path("parquet");
        let quoted = quote_literal(&path.display().to_string());
        engine.execute(&format!("COPY (SELECT 'West' AS region, 10.5 AS amount, NULL::INTEGER AS blanks) TO {quoted} (FORMAT PARQUET)")).unwrap();
        engine.load_file("from_file", &[path.display().to_string()], FileFormat::Parquet).unwrap();
        let table = engine.query("SELECT region, amount, blanks, typeof(region) AS t FROM from_file").unwrap();
        assert_eq!(table.rows, vec![vec![Value::text("West"), Value::number(10.5), Value::Empty, Value::text("VARCHAR")]]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_csv_file_loads_with_its_header_and_its_values() {
        let mut engine = engine();
        let path = temp_path("csv");
        std::fs::write(&path, "region,amount\nWest,1.5\nEast,2\n").unwrap();
        engine.load_file("csv_table", &[path.display().to_string()], FileFormat::Csv).unwrap();
        let table = engine.query("SELECT region, amount FROM csv_table ORDER BY 1").unwrap();
        assert_eq!(table.columns, vec!["region", "amount"]);
        assert_eq!(table.rows, vec![vec![Value::text("East"), Value::number(2.0)], vec![Value::text("West"), Value::number(1.5)]]);
        let _ = std::fs::remove_file(&path);
    }
}
