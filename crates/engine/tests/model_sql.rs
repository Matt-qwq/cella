//! `data.model.query`: SQL over the data model, answered by the engine the model lives on.
//!
//! The model's tables are loaded into that engine the first time a query needs them and loaded
//! again whenever the model changes (a refresh, a table removed, an undo). These tests are the
//! proof that the copy is the model's answer and not a stale one: the same query is run before
//! and after a refresh, and the numbers have to follow.

use cella_engine::Session;
use serde_json::{Value, json};

const SALES: &str = "Region,Amount\nWest,10\nEast,5\nWest,7\n";

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.execute(cmd, params).unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

/// One query loaded into the model: three rows of sales.
fn model_session() -> Session {
    let mut s = session();
    load(&mut s, SALES);
    s
}

fn load(s: &mut Session, csv: &str) {
    run(
        s,
        "data.query.add",
        json!({
            "name": "Sales",
            "source": {"kind": "csv", "text": csv},
            "steps": ["PromoteHeaders", {"ChangeType": {"columns": ["Amount"], "to": "Number"}}],
            "load": "model",
        }),
    );
}

/// Whether this build has an engine at all (the browser build, and `cargo test -p cella-engine`
/// without the feature, do not).
fn has_engine() -> bool {
    cfg!(feature = "sql-engine")
}

#[test]
fn sql_over_the_model_answers_what_the_query_loaded() {
    let mut s = model_session();
    if !has_engine() {
        let error = s.execute("data.model.query", json!({"sql": "SELECT * FROM Sales"})).unwrap_err();
        assert!(error.to_string().contains("desktop app"), "{error}");
        return;
    }
    let answer = run(&mut s, "data.model.query", json!({"sql": "SELECT COUNT(*) AS n, SUM(Amount) AS total FROM Sales"}));
    assert_eq!(answer["columns"], json!(["n", "total"]));
    assert_eq!(answer["rows"], json!([[3.0, 22.0]]));
}

#[test]
fn a_group_by_over_the_model_is_the_same_answer_the_engine_gives() {
    let mut s = model_session();
    if !has_engine() {
        return;
    }
    let answer = run(&mut s, "data.model.query", json!({"sql": "SELECT Region, SUM(Amount) AS total FROM Sales GROUP BY 1 ORDER BY 1"}));
    assert_eq!(answer["rows"], json!([["East", 5.0], ["West", 17.0]]));
}

#[test]
fn a_refresh_makes_the_engines_copy_new() {
    let mut s = model_session();
    if !has_engine() {
        return;
    }
    let before = run(&mut s, "data.model.query", json!({"sql": "SELECT SUM(Amount) AS total FROM Sales"}));
    assert_eq!(before["rows"], json!([[22.0]]));

    // The same query, re-run over different rows: the engine's copy has to follow the model.
    let name = {
        let d = s.doc().expect("document");
        d.wb.queries.first().map(|q| q.name.clone()).expect("a query")
    };
    run(&mut s, "data.query.setSource", json!({"name": name, "source": {"kind": "csv", "text": "Region,Amount\nWest,100\nEast,1\n"}}));
    run(&mut s, "data.query.refresh", json!({"name": name}));

    let after = run(&mut s, "data.model.query", json!({"sql": "SELECT SUM(Amount) AS total, COUNT(*) AS n FROM Sales"}));
    assert_eq!(after["rows"], json!([[101.0, 2.0]]));
}

#[test]
fn a_table_removed_from_the_model_leaves_the_engines_copy() {
    let mut s = model_session();
    if !has_engine() {
        return;
    }
    // Proves the copy really had it.
    run(&mut s, "data.model.query", json!({"sql": "SELECT COUNT(*) FROM Sales"}));
    run(&mut s, "data.model.removeTable", json!({"table": "Sales"}));
    let error = s.execute("data.model.query", json!({"sql": "SELECT COUNT(*) FROM Sales"})).unwrap_err();
    assert!(matches!(error, cella_engine::EngineError::Other(_)), "{error:?}");
}

#[test]
fn a_statement_that_writes_is_refused_before_it_reaches_the_engine() {
    let mut s = model_session();
    let error = s.execute("data.model.query", json!({"sql": "DROP TABLE Sales"})).unwrap_err();
    assert!(error.to_string().contains("read-only"), "{error}");
    let error = s.execute("data.model.query", json!({"sql": "COPY (SELECT 1) TO 'out.parquet'"})).unwrap_err();
    assert!(error.to_string().contains("read-only"), "{error}");
}

#[test]
fn an_empty_query_or_an_empty_model_is_an_error_not_a_panic() {
    let mut s = session();
    // No model yet: the command says what is missing before it looks at the SQL.
    let error = s.execute("data.model.query", json!({"sql": "SELECT 1"})).unwrap_err();
    assert!(error.to_string().contains("no data model"), "{error}");

    let mut s = model_session();
    let error = s.execute("data.model.query", json!({"sql": "   "})).unwrap_err();
    assert!(error.to_string().contains("missing sql"), "{error}");
    // Bad SQL is the engine's message, not a panic.
    if has_engine() {
        let error = s.execute("data.model.query", json!({"sql": "SELECT * FROM nowhere"})).unwrap_err();
        assert!(!error.to_string().is_empty());
    }
}
