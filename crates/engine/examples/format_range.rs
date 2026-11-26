//! Prices `home.formatCells` over a large range — what a rectangle of bold costs, and why the
//! 4,000,000-cell cap in `cmd/format.rs` exists (gaps §32).
//!
//! Measured 11-26 on the development machine (release, 1M rows): 4M cells **0.4 s**, 8M **0.82 s**,
//! 16M **1.52 s**; a pass with nothing to change is 0.18 s at 8M, so the writes are most of it.
//!
//! ```text
//! cargo run --release -p cella-engine --example format_range -- [rows] [cols]
//! ```

#![allow(clippy::disallowed_methods)] // a native-only benchmark: wasm never runs it

use std::time::Instant;

use cella_engine::Session;
use cella_engine::core::CellRef;
use serde_json::json;

fn main() {
    let rows: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(1_000_000);
    let cols: u32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(8);
    let mut s = Session::new();
    s.new_workbook();
    let t = Instant::now();
    let chunk = 10_000u32;
    let mut r = 0u32;
    while r < rows {
        let n = chunk.min(rows - r);
        let vals: Vec<Vec<f64>> = (0..n).map(|i| vec![(r + i) as f64; cols as usize]).collect();
        s.execute("range.setValues", json!({"range": format!("A{}", r + 1), "values": vals})).expect("set");
        r += n;
    }
    println!("built {rows}×{cols} = {} cells in {:?}", u64::from(rows) * u64::from(cols), t.elapsed());
    let last_col = CellRef::new(0, cols - 1).a1().trim_end_matches(|c: char| c.is_ascii_digit()).to_string();
    let range = format!("A1:{last_col}{rows}");

    for pass in 1..=2 {
        let t = Instant::now();
        match s.execute("home.formatCells", json!({"range": range, "style": {"font": {"bold": true}}})) {
            Ok(_) => println!("pass {pass} home.formatCells {{bold}} over {range}: {:?}", t.elapsed()),
            Err(e) => {
                println!("pass {pass} refused after {:?}: {e}", t.elapsed());
                break;
            }
        }
    }
    // One row of 1,000 columns: the wide shape the shape ruler also prices.
    let t = Instant::now();
    match s.execute(
        "home.formatCells",
        json!({"range": format!("A1:{}1", CellRef::new(0, 999).a1().trim_end_matches(|c: char| c.is_ascii_digit())), "style": {"font": {"bold": false}}}),
    ) {
        Ok(_) => println!("1000-column row: {:?}", t.elapsed()),
        Err(e) => println!("1000-column row refused: {e}"),
    }
}
