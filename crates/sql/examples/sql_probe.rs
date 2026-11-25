//! The ruler for the data model's engine, in the tree: how long it takes Cella's own rows to
//! reach DuckDB, how long the pivot and slicer queries take once they are there, and how much
//! faster the same data is when the engine reads the file itself.
//!
//! ```text
//! cargo run -p cella-sql --example sql_probe --release --features duckdb
//! ```
//!
//! With the feature off it says so and exits: a build without the engine has nothing to time.

#![allow(clippy::disallowed_methods)] // native-only probe; the clock is the measurement

use std::sync::Arc;
use std::time::Instant;

use cella_core::Value;
use cella_sql::{Engine, FileFormat, quote_literal};

/// The shape of a sales table: keys to group by, a couple of numbers to add up, one text column
/// (a slicer groups by the region's *name*, which is what makes the string path worth timing).
#[derive(Clone, Copy)]
struct Row {
    region: u16,
    month: u16,
    product: u16,
    channel: u16,
    customer: u32,
    amount: f64,
    qty: f64,
    cost: f64,
    discount: f64,
    day: u16,
}

/// Keys must be independent of each other: `(i % 64, i % 12)` only ever makes 192 pairs, and a
/// report that was meant to be 768 cells would then be measured as a third of the work.
fn build(rows: usize) -> Vec<Row> {
    (0..rows)
        .map(|i| Row {
            region: (i % 64) as u16,
            month: ((i / 64) % 12) as u16,
            product: ((i / 768) % 1000) as u16,
            channel: ((i / 768_000) % 7) as u16,
            customer: ((i / 5376) % 100_000) as u32,
            amount: (i % 9973) as f64 * 0.01 + 1.0,
            qty: (i % 25 + 1) as f64,
            cost: (i % 7919) as f64 * 0.01,
            discount: (i % 31) as f64 * 0.01,
            day: ((i / 64) % 730) as u16,
        })
        .collect()
}

fn values(rows: &[Row], regions: &[Arc<str>]) -> Vec<Vec<Value>> {
    rows.iter()
        .map(|r| {
            vec![
                Value::Text(regions[r.region as usize].clone()),
                Value::number(f64::from(r.month)),
                Value::number(f64::from(r.product)),
                Value::number(f64::from(r.channel)),
                Value::number(f64::from(r.customer)),
                Value::number(r.amount),
                Value::number(r.qty),
                Value::number(r.cost),
                Value::number(r.discount),
                Value::number(f64::from(r.day)),
            ]
        })
        .collect()
}

const COLUMNS: [&str; 10] = ["region", "month", "product", "channel", "customer", "amount", "qty", "cost", "discount", "day"];

fn seconds(d: std::time::Duration) -> f64 {
    d.as_secs_f64()
}

/// Best of five: the machine is shared, and the fastest run is the one that got its cores.
fn best(engine: &Engine, sql: &str) -> (f64, usize) {
    let mut fastest = f64::MAX;
    let mut rows = 0;
    for _ in 0..5 {
        let t = Instant::now();
        let table = engine.query(sql).expect("query");
        fastest = fastest.min(seconds(t.elapsed()));
        rows = table.rows.len();
    }
    (fastest, rows)
}

fn main() {
    if !cella_sql::available() {
        println!("this build has no SQL engine (build with --features duckdb on a desktop target)");
        return;
    }
    let sizes: Vec<usize> = match std::env::var("ROWS") {
        Ok(text) => text.split(',').filter_map(|n| n.trim().parse().ok()).collect(),
        Err(_) => vec![1_000_000, 5_000_000],
    };
    for rows in sizes {
        println!("\n=== {:.1} M rows x 10 columns ===", rows as f64 / 1e6);
        let t = Instant::now();
        let raw = build(rows);
        let regions: Vec<Arc<str>> = (0..64).map(|i| Arc::from(format!("Region {i}").as_str())).collect();
        let data = values(&raw, &regions);
        drop(raw);
        println!("built {} Value rows in {:.2} s", data.len(), seconds(t.elapsed()));

        let mut engine = Engine::in_memory().expect("engine");
        let t = Instant::now();
        let loaded = engine.load_rows("sales", &COLUMNS.map(String::from), &data).expect("load");
        let load = seconds(t.elapsed());
        println!("rows -> engine: {:.0} ms ({:.0} ns a row, {loaded} rows)", load * 1e3, load * 1e9 / rows as f64);
        drop(data);

        let queries = [
            ("group-by region x month", "SELECT region, month, SUM(amount), COUNT(*) FROM sales GROUP BY 1, 2 ORDER BY 1, 2"),
            (
                "slicer: 4 of 64 regions, then group-by",
                "SELECT region, month, SUM(amount), COUNT(*) FROM sales \
                 WHERE region IN ('Region 1','Region 7','Region 23','Region 40') GROUP BY 1, 2 ORDER BY 1, 2",
            ),
            ("top-20 products", "SELECT product, SUM(amount) FROM sales GROUP BY 1 ORDER BY 2 DESC LIMIT 20"),
        ];
        for (label, sql) in queries {
            let (ms, out) = best(&engine, sql);
            println!("  {label}: {:.1} ms ({out} rows)", ms * 1e3);
        }

        // The file route: what a model table built from a file pays, and what it does not.
        let path = std::env::temp_dir().join(format!("cella-sql-probe-{rows}.parquet"));
        let quoted = quote_literal(&path.display().to_string());
        let t = Instant::now();
        engine.execute(&format!("COPY sales TO {quoted} (FORMAT PARQUET, COMPRESSION ZSTD)")).expect("write parquet");
        println!("  the engine wrote its own Parquet copy in {:.0} ms", seconds(t.elapsed()) * 1e3);

        let mut file_engine = Engine::in_memory().expect("engine");
        let t = Instant::now();
        file_engine.load_file("sales", &[path.display().to_string()], FileFormat::Parquet).expect("load file");
        let from_file = seconds(t.elapsed());
        println!("  Parquet file -> table: {:.0} ms ({:.0} ns a row)", from_file * 1e3, from_file * 1e9 / rows as f64);
        let (ms, out) = best(&file_engine, "SELECT region, month, SUM(amount), COUNT(*) FROM sales GROUP BY 1, 2 ORDER BY 1, 2");
        println!("  group-by on the table read from the file: {:.1} ms ({out} rows)", ms * 1e3);
        let _ = std::fs::remove_file(&path);
    }
}
