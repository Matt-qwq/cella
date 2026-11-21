//! The R34 measurement: what printing a formula's text from its shared shape costs.
//!
//! The text a cell shows is a pure function of the shape and the cell (`print_at`), which is the
//! invariant the shape-sharing work pins. This probe prices the two sides of removing the per-cell
//! copy: how many bytes the text costs today, and what re-printing it would cost.
//!
//! `cargo run --release -p cella-model --example formula_text_probe`

#![allow(clippy::disallowed_methods)] // a native-only probe: the clock is the measurement

use std::time::Instant;

use cella_core::CellRef;
use cella_formula::ShapeTable;
use cella_model::Formula;

fn main() {
    let n: u32 = 1_000_000;
    let mut shapes = ShapeTable::default();
    let build = Instant::now();
    let mut formulas = Vec::with_capacity(n as usize);
    for row in 0..n {
        let at = CellRef::new(row + 1, 1);
        let text = format!("=B{}*C{}+D{}", row + 2, row + 2, row + 2);
        formulas.push(Formula::new_at(&text, at, &mut shapes, false));
    }
    let build = build.elapsed();

    let print = Instant::now();
    let mut bytes = 0usize;
    let mut mismatch = 0usize;
    let mut no_shape = 0usize;
    for (i, f) in formulas.iter().enumerate() {
        let at = CellRef::new(i as u32 + 1, 1);
        match f.shape.as_ref().and_then(|s| s.print_at(at)) {
            Some(t) => {
                bytes += t.len();
                if t != f.text {
                    mismatch += 1;
                }
            }
            None => no_shape += 1,
        }
    }
    let print = print.elapsed();

    println!("formulas            {n}");
    println!("size_of::<Formula>() {} B", std::mem::size_of::<Formula>());
    println!("text bytes          {bytes} ({:.1} B/formula)", bytes as f64 / n as f64);
    println!("build (parse+print+intern) {build:?} ({:.0} ns/formula)", build.as_nanos() as f64 / n as f64);
    println!("print_at            {print:?} ({:.0} ns/formula)", print.as_nanos() as f64 / n as f64);
    println!("mismatches          {mismatch}, no shape {no_shape}");
}
