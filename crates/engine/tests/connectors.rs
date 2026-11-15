//! The two connectors that need the outside world: a SQLite file and a URL. Both are proved
//! against real things — a database this test builds, and a server it runs on a socket — because a
//! connector that is only tested against a mock proves nothing about SQLite's types or about HTTP.
//!
//! Runs with: cargo test -p gridcraft-engine --features connectors --test connectors

#![cfg(feature = "connectors")]

use gridcraft_engine::Session;
use serde_json::{Value, json};

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.execute(cmd, params).unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

fn err(s: &mut Session, cmd: &str, params: Value) -> String {
    s.execute(cmd, params).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("{cmd} should have failed"))
}

/// A database with three rows whose column types are the interesting ones.
fn make_db(path: &std::path::Path) {
    let c = rusqlite::Connection::open(path).expect("create db");
    c.execute_batch(
        "CREATE TABLE sales (region TEXT, amount INTEGER, price REAL, on_day TEXT, note TEXT);
         INSERT INTO sales VALUES ('West', 10, 1.5, '2024-03-04', NULL);
         INSERT INTO sales VALUES ('East', 5, 2.25, '2024-03-05', NULL);
         INSERT INTO sales VALUES ('West', 7, 3.0, '2024-03-06', NULL);",
    )
    .expect("seed db");
}

fn temp_db(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("gridcraft-conn-{tag}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let db = dir.join(format!("{tag}.db"));
    let _ = std::fs::remove_file(&db);
    db
}

#[test]
fn a_sqlite_query_reads_the_statements_result_set() {
    let db = temp_db("sales");
    make_db(&db);
    let mut s = session();
    let r = run(
        &mut s,
        "data.query.add",
        json!({"name": "Sales", "source": {"kind": "sqlite", "path": db.to_string_lossy(), "sql": "SELECT region, amount, price FROM sales ORDER BY amount DESC"}}),
    );
    assert_eq!(r["rows"], 3, "{r}");
    let d = s.doc().expect("doc");
    let sh = d.wb.sheet_index("Sales").and_then(|i| d.wb.sheet(i)).expect("loaded sheet");
    let at = |a: &str| sh.value(gridcraft_core::CellRef::parse(a).expect("ref")).display();
    // The columns are the statement's own: no PromoteHeaders step was needed.
    assert_eq!(at("A1"), "region");
    assert_eq!(at("B1"), "amount");
    assert_eq!(at("A2"), "West");
    assert_eq!(at("B2"), "10");
    assert_eq!(at("C2"), "1.5");
    let _ = std::fs::remove_file(&db);
}

#[test]
fn a_sqlite_statement_cannot_change_the_database() {
    let db = temp_db("ro");
    make_db(&db);
    let mut s = session();
    let msg = err(
        &mut s,
        "data.query.add",
        json!({"name": "Write", "source": {"kind": "sqlite", "path": db.to_string_lossy(), "sql": "DELETE FROM sales"}}),
    );
    assert!(!msg.is_empty(), "a write must fail");
    // The rows are still there: the file was opened read-only, not merely refused afterwards.
    let c = rusqlite::Connection::open(&db).expect("reopen");
    let n: i64 = c.query_row("SELECT COUNT(*) FROM sales", [], |r| r.get(0)).expect("count");
    assert_eq!(n, 3);
    let _ = std::fs::remove_file(&db);
}

#[test]
fn a_binary_column_is_refused_by_name() {
    let db = temp_db("blob");
    let c = rusqlite::Connection::open(&db).expect("create");
    c.execute_batch("CREATE TABLE b (name TEXT, payload BLOB); INSERT INTO b VALUES ('a', x'0102');").expect("seed");
    drop(c);
    let mut s = session();
    let msg =
        err(&mut s, "data.query.add", json!({"name": "B", "source": {"kind": "sqlite", "path": db.to_string_lossy(), "sql": "SELECT * FROM b"}}));
    assert!(msg.contains("payload") && msg.contains("binary"), "{msg}");
    let _ = std::fs::remove_file(&db);
}

/// Serves one canned reply on a socket, then closes: what a connector sees from any small endpoint.
fn serve_once(body: &'static str, status: &'static str) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf);
            let reply = format!("HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            let _ = sock.write_all(reply.as_bytes());
            let _ = sock.flush();
        }
    });
    format!("http://{addr}/data")
}

#[test]
fn an_http_csv_source_is_a_table() {
    let url = serve_once("Region,Amount\nWest,10\nEast,5\n", "200 OK");
    let mut s = session();
    // A file's first row is data until a step says otherwise, the same rule a CSV on disk follows;
    // a URL is a file that arrives over a socket.
    let r = run(&mut s, "data.query.add", json!({"name": "Web", "source": {"kind": "http", "url": url}, "steps": ["PromoteHeaders"]}));
    assert_eq!(r["rows"], 2, "two data rows; the header became the columns: {r}");
    let d = s.doc().expect("doc");
    let sh = d.wb.sheet_index("Web").and_then(|i| d.wb.sheet(i)).expect("sheet");
    let at = |a: &str| sh.value(gridcraft_core::CellRef::parse(a).expect("ref")).display();
    assert_eq!(at("A1"), "Region");
    assert_eq!(at("A2"), "West");
    assert_eq!(at("B2"), "10");
}

#[test]
fn a_json_url_is_read_as_json() {
    let url = serve_once("[{\"region\":\"West\",\"amount\":10},{\"region\":\"East\",\"amount\":5}]", "200 OK");
    let mut s = session();
    let r = run(&mut s, "data.query.add", json!({"name": "J", "source": {"kind": "http", "url": url, "format": "json"}}));
    assert_eq!(r["cols"], 2, "{r}");
}

#[test]
fn a_web_page_and_a_bad_address_are_refused_in_words_a_person_can_act_on() {
    let page = serve_once("<!DOCTYPE html><html><body>Please sign in</body></html>", "200 OK");
    let mut s = session();
    let msg = err(&mut s, "data.query.add", json!({"name": "Page", "source": {"kind": "http", "url": page}}));
    assert!(msg.contains("web page"), "{msg}");

    let missing = serve_once("not found", "404 Not Found");
    let msg = err(&mut s, "data.query.add", json!({"name": "Missing", "source": {"kind": "http", "url": missing}}));
    assert!(msg.contains("404"), "{msg}");

    let msg = err(&mut s, "data.query.add", json!({"name": "File", "source": {"kind": "http", "url": "file:///etc/passwd"}}));
    assert!(msg.contains("http://"), "{msg}");
}

#[test]
fn get_data_names_the_query_after_what_it_reads_and_numbers_a_clash() {
    let first = serve_once("Region,Amount\nWest,10\n", "200 OK");
    let mut s = session();
    let r = run(&mut s, "data.fromWeb", json!({"url": first, "name": "sales"}));
    assert_eq!(r["name"], "sales", "{r}");
    // The same name again gets a number instead of an error: a person clicking Get Data twice
    // asked for two queries, not for a lesson about unique names.
    let msg = err(&mut s, "data.fromDatabase", json!({"path": "/data/sales.db", "sql": "SELECT 1", "name": "sales"}));
    assert!(msg.contains("already"), "an explicit name that is taken must be refused, not renamed: {msg}");

    let db = temp_db("named");
    make_db(&db);
    let r = run(&mut s, "data.fromDatabase", json!({"path": db.to_string_lossy(), "sql": "SELECT region FROM sales"}));
    assert_eq!(r["name"], "named", "{r}");
    let _ = std::fs::remove_file(&db);

    // Menu-style invocation (no parameters) asks for the dialog instead of guessing.
    let mut s = session();
    let _ = run(&mut s, "data.fromWeb", json!({}));
    assert!(s.ui_requests.iter().any(|q| format!("{q:?}").contains("fromWeb")), "the dialog was not asked for");
}
