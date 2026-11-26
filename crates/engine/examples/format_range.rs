//! Prices two halves of `home.formatCells` — what a rectangle of bold costs the **command**, and
//! what it costs the **file** (gaps §32).
//!
//! The 4,000,000-cell cap in `cmd/format.rs` is a style per cell, so a rectangle is O(cells) work and
//! stores a cell for each. Measured 11-26 on the development machine (release, 1M rows): 4M cells
//! **0.4 s**, 8M **0.82 s**, 16M **1.52 s**, and a pass with nothing to change is 0.18 s at 8M. The
//! second half is the one a range-level style would change: a styled **blank** is a stored cell, so
//! the save carries it — the probe saves the workbook before and after the format and reports both
//! files.
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

    // What the file carries before any format: the values, as stored cells.
    let path = std::env::temp_dir().join("cella-format-range.xlsx");
    let t = Instant::now();
    s.execute("file.save", json!({"path": path.to_string_lossy()})).expect("save");
    let plain = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    println!("plain save: {:.1} MB in {:?}", plain as f64 / 1e6, t.elapsed());

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
    // What the file carries after it: a style per cell, blank cells included.
    let t = Instant::now();
    s.execute("file.save", json!({"path": path.to_string_lossy()})).expect("save");
    let bold = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    println!(
        "bold save: {:.1} MB in {:?} (+{:.1} MB, {:.1} bytes a cell)",
        bold as f64 / 1e6,
        t.elapsed(),
        (bold.saturating_sub(plain)) as f64 / 1e6,
        (bold.saturating_sub(plain)) as f64 / (u64::from(rows) * u64::from(cols)) as f64
    );
    println!("saved to {}", path.display());
    if std::env::var_os("CELLA_KEEP").is_none() {
        let _ = std::fs::remove_file(&path);
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
