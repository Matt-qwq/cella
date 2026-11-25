//! Where a block read's time goes, measured in the store alone — no evaluator in the way.
//!
//! `cargo run --release -p cella-model --example read_probe -- [--rows=N] [--cols=N] [--repeat=N]`
//!
//! The function catalogue prices a formula; a formula over a block pays for (at least) one walk of
//! that block, and the walks are the store's. This probe builds the catalogue's own sheet shape and
//! times the four walks a formula's argument can take — [`CellStore::visit_values`] (what builds an
//! `Array`, 25 ns a cell from the catalogue), [`CellStore::visit_packed`] (what an aggregate folds,
//! also 25), [`CellStore::iter_range`] (the `Cell` path) and [`CellStore::has_owned_cells`] (the
//! gate a whole-column aggregate pays before it folds anything) — so "the formula costs 25 ms a
//! million cells" can be read as a per-cell price of the walk rather than of the formula.

#![allow(clippy::disallowed_methods)] // native-only probe; the clock is the measurement

use std::process::ExitCode;
use std::time::Instant;

use cella_core::{CellRef, RangeRef, Value};
use cella_model::{Cell, CellStore, FoldAcc, FoldMode, Packed};

const DEFAULT_ROWS: usize = 1_000_000;
const DEFAULT_COLS: usize = 8;
const MAX_ROWS: usize = 5_000_000;
const MAX_COLS: usize = 64;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("read_probe: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let (mut rows, mut cols, mut repeat) = (DEFAULT_ROWS, DEFAULT_COLS, 3usize);
    for arg in std::env::args().skip(1) {
        match arg.split_once('=') {
            Some(("--rows", v)) => rows = v.parse().ok().filter(|n: &usize| (3..=MAX_ROWS).contains(n)).ok_or("--rows out of range")?,
            Some(("--cols", v)) => cols = v.parse().ok().filter(|n: &usize| (1..=MAX_COLS).contains(n)).ok_or("--cols out of range")?,
            Some(("--repeat", v)) => repeat = v.parse().ok().filter(|n: &usize| (1..=15).contains(n)).ok_or("--repeat out of range")?,
            _ => return Err(format!("unknown argument `{arg}` (usage: read_probe [--rows=N] [--cols=N] [--repeat=N])")),
        }
    }
    if cfg!(debug_assertions) {
        eprintln!("read_probe: this is a debug build — the numbers are not comparable (use --release)");
    }
    let started = Instant::now();
    let store = prepare(rows, cols);
    // The same sheet with one column inserted in front: its column map is no longer the identity,
    // so every walk takes the general path. Reading logical column B there reads the physical
    // column the identity store's A reads — the same cells, one column map apart, so the pair is an
    // A/B of the walks themselves on whatever machine this is today.
    let mut mapped = store.clone();
    mapped.shift_cols(0, 1);
    eprintln!("read_probe: {rows} rows x {cols} columns in {:.1} s", started.elapsed().as_secs_f64());
    let last = rows as u32;
    let (c0, c1) = (0u32, cols as u32 - 1);
    let one = RangeRef::new(CellRef::new(0, 0), CellRef::new(last - 1, 0));
    let one_mapped = RangeRef::new(CellRef::new(0, 1), CellRef::new(last - 1, 1));
    let block = RangeRef::new(CellRef::new(0, c0), CellRef::new(last - 1, c1));
    let cells = u64::from(one.height()) * u64::from(one.width());
    let block_cells = u64::from(block.height()) * u64::from(block.width());
    println!("\n{:<44} {:>12} {:>12}", "walk", "fastest", "ns/cell");
    // What the last load of a column read costs. A number cell's payload indexes the band's number
    // table, so reading one is three dependent loads (the row's `cells_at`, the payload, the table);
    // a payload stripe of u64s would make the number itself the payload - two. The two chains run
    // over the same data and the same loop, black-boxed so the optimiser cannot fold them away, and
    // the difference is the most inline numbers could ever buy a column read (`gaps` §48).
    {
        use std::hint::black_box;
        let n = 1_000_000usize;
        // A band's shape: 64 rows a band, a few hundred distinct numbers.
        let payloads: Vec<u32> = (0..n).map(|i| (i % 4096) as u32).collect();
        let numbers: Vec<f64> = (0..4096).map(|i| i as f64).collect();
        let inline: Vec<f64> = payloads.iter().map(|p| numbers[*p as usize]).collect();
        let chain = best(5, || {
            let mut acc = 0.0f64;
            for p in black_box(&payloads) {
                acc = black_box(acc + numbers[black_box(*p) as usize]);
            }
            acc.to_bits()
        });
        let flat = best(5, || {
            let mut acc = 0.0f64;
            for v in black_box(&inline) {
                acc = black_box(acc + *v);
            }
            acc.to_bits()
        });
        let (c, f) = (chain.0 * 1e6 / n as f64, flat.0 * 1e6 / n as f64);
        println!("{:<44} {:>12} {:>12}", "chain: payload -> number table", fmt_ms(chain.0), format!("{c:.1}"));
        println!("{:<44} {:>12} {:>12}", "chain: number in the payload", fmt_ms(flat.0), format!("{f:.1}"));
        println!("{:<44} {:>12}", "one load less would be worth", format!("{:.1} ns/cell", c - f));
    }
    report(
        "visit_values  A2:A{last}",
        cells,
        best(repeat, || {
            let mut n = 0u64;
            store.visit_values(one, &mut |_, value, _| {
                if !value.is_empty() {
                    n += 1;
                }
                true
            });
            n
        }),
    );
    report(
        "visit_values  B2:B{last} mapped (general)",
        cells,
        best(repeat, || {
            let mut n = 0u64;
            mapped.visit_values(one_mapped, &mut |_, value, _| {
                if !value.is_empty() {
                    n += 1;
                }
                true
            });
            n
        }),
    );
    report(
        "visit_packed  A2:A{last}",
        cells,
        best(repeat, || {
            let mut n = 0u64;
            store.visit_packed(one, &mut |_, packed| {
                if matches!(packed, Some(Packed::Number(_))) {
                    n += 1;
                }
                true
            });
            n
        }),
    );
    report(
        "visit_packed  B2:B{last} mapped (general)",
        cells,
        best(repeat, || {
            let mut n = 0u64;
            mapped.visit_packed(one_mapped, &mut |_, packed| {
                if matches!(packed, Some(Packed::Number(_))) {
                    n += 1;
                }
                true
            });
            n
        }),
    );
    report(
        "visit_packed  A2:H{last} (block)",
        block_cells,
        best(repeat, || {
            let mut n = 0u64;
            store.visit_packed(block, &mut |_, packed| {
                if packed.is_some() {
                    n += 1;
                }
                true
            });
            n
        }),
    );
    report(
        "fold_column Sum A2:A{last}",
        cells,
        best(repeat, || {
            let mut acc = FoldAcc::default();
            store.fold_column(one, FoldMode::Sum, &mut acc);
            acc.count
        }),
    );
    report("iter_range    A2:A{last}", cells, best(repeat, || store.iter_range(one).count() as u64));
    report("iter_range    A2:H{last} (block)", block_cells, best(repeat, || store.iter_range(block).count() as u64));
    report("has_owned_cells A2:A{last}", cells, best(repeat, || u64::from(store.has_owned_cells(one))));
    report(
        "build a Vec<Value> of the block",
        block_cells,
        best(repeat, || {
            let mut out = vec![Value::Empty; block_cells as usize];
            let mut i = 0usize;
            store.visit_values(block, &mut |_, value, _| {
                if let Some(slot) = out.get_mut(i) {
                    *slot = value;
                }
                i += 1;
                true
            });
            u64::try_from(out.len()).unwrap_or(u64::MAX)
        }),
    );
    Ok(())
}

/// The fastest of `repeat` runs of `f`, in milliseconds, with the answer's magnitude printed after
/// it so a walk that did nothing can't look fast.
fn best(repeat: usize, mut f: impl FnMut() -> u64) -> (f64, u64) {
    let mut best = f64::INFINITY;
    let mut answer = 0u64;
    for _ in 0..repeat {
        let t0 = Instant::now();
        answer = f();
        best = best.min(t0.elapsed().as_secs_f64() * 1000.0);
    }
    (best, answer)
}

fn report(label: &str, cells: u64, (ms, answer): (f64, u64)) {
    let per = if cells == 0 { 0.0 } else { ms * 1e6 / cells as f64 };
    println!("{label:<44} {:>12} {:>9.1}  ({answer})", fmt_ms(ms), per);
}

fn fmt_ms(ms: f64) -> String {
    if ms >= 1_000.0 {
        format!("{:.2} s", ms / 1_000.0)
    } else if ms >= 10.0 {
        format!("{ms:.0} ms")
    } else if ms >= 1.0 {
        format!("{ms:.1} ms")
    } else {
        format!("{:.0} us", ms * 1_000.0)
    }
}

/// The catalogue's sheet shape: a number, a quantity, a price, an item name, a region, a date, a
/// key and a value, repeated to fill `cols` columns.
fn prepare(rows: usize, cols: usize) -> CellStore {
    let mut store = CellStore::new();
    let mut row_cells: Vec<(u32, Cell)> = Vec::with_capacity(cols);
    for row in 0..rows {
        row_cells.clear();
        for col in 0..cols {
            let id = row + 1;
            let cell = match col % 8 {
                3 => Cell::value(Value::text(format!("Item {id:06}"))),
                4 => Cell::value(Value::text(["North", "South", "East", "West"][id % 4])),
                6 => Cell::value(Value::text(format!("K{id:06}"))),
                1 => Cell::value(Value::Number((id % 1_000 + 1) as f64)),
                2 => Cell::value(Value::Number((id % 97) as f64 + 0.5)),
                5 => Cell::value(Value::Number((id % 400) as f64)),
                7 => Cell::value(Value::Number((id % 1_000) as f64)),
                _ => Cell::value(Value::Number(id as f64)),
            };
            row_cells.push((col as u32, cell));
        }
        store.set_row(row as u32, &mut row_cells);
    }
    store
}
