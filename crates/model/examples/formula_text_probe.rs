//! The R34 measurement: what printing a formula's text from its shared shape costs.
//!
//! A filled cell keeps no text (R34): the text is `Shape::print_at(at)`, the invariant shape
//! sharing pins. This probe holds 1M formulas of one fill and prices both sides — what the text
//! would cost stored, and what printing it costs.
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
    let mut expected = Vec::with_capacity(n as usize);
    for row in 0..n {
        let at = CellRef::new(row + 1, 1);
        let text = format!("=B{}*C{}+D{}", row + 2, row + 2, row + 2);
        expected.push(Formula::new(&text).text_at(at).unwrap_or_default());
        formulas.push(Formula::new_at(&text, at, &mut shapes, false));
    }
    let build = build.elapsed();

    let print = Instant::now();
    let mut bytes = 0usize;
    let mut mismatch = 0usize;
    for (i, f) in formulas.iter().enumerate() {
        let at = CellRef::new(i as u32 + 1, 1);
        match f.text_at(at) {
            Some(t) => {
                bytes += t.len();
                if t != expected[i] {
                    mismatch += 1;
                }
            }
            None => mismatch += 1,
        }
    }
    let print = print.elapsed();

    let stored: usize = formulas.iter().map(|f| f.text.as_ref().map_or(0, |t| t.len())).sum();
    println!("formulas            {n}");
    println!("size_of::<Formula>() {} B", std::mem::size_of::<Formula>());
    println!("stored text bytes   {stored}");
    println!("printed text bytes  {bytes} ({:.1} B/formula)", bytes as f64 / n as f64);
    println!("build (parse+print+intern) {build:?} ({:.0} ns/formula)", build.as_nanos() as f64 / n as f64);
    println!("text_at             {print:?} ({:.0} ns/formula)", print.as_nanos() as f64 / n as f64);
    println!("mismatches          {mismatch}");
}
