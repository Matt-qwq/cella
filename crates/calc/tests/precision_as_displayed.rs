//! "Set precision as displayed" in the recalculation pass: a formula's result takes its cell's
//! number format before it is stored and before the formulas that read it are evaluated.
//!
//! The expected values were observed in Excel (COM, `Workbook.PrecisionAsDisplayed = True`) with
//! the same synthetic formulas; upstream's pass host rounds in `compute` as ours does.

use cella_calc::Calc;
use cella_core::{CellError, CellRef, Value};
use cella_model::{Cell, Formula, NumFmt, Style, Workbook};

fn c(s: &str) -> CellRef {
    match CellRef::parse(s) {
        Some(c) => c,
        None => CellRef::new(0, 0),
    }
}

struct T {
    wb: Workbook,
    calc: Calc,
}

impl T {
    fn new() -> T {
        T { wb: Workbook::new(), calc: Calc::new() }
    }
    fn set(&mut self, at: &str, input: &str) {
        let cell = if input.starts_with('=') {
            Cell::formula(Formula::new(input))
        } else {
            Cell::value(cella_core::parse::parse_input(input, self.wb.date_system).value)
        };
        if let Some(sh) = self.wb.sheet_mut(0) {
            sh.cells.set(c(at), cell);
        }
        self.calc.cells_changed(&mut self.wb, &[(0, c(at))]);
    }
    fn fmt(&mut self, at: &str, code: &str) {
        let id = self.wb.styles.intern(Style { num_fmt: NumFmt::new(code), ..Default::default() });
        if let Some(sh) = self.wb.sheet_mut(0) {
            sh.set_style(c(at), id);
        }
    }
    fn get(&self, at: &str) -> Value {
        self.wb.sheet(0).map(|s| s.value(c(at))).unwrap_or_default()
    }
    fn num(&self, at: &str) -> f64 {
        match self.get(at) {
            Value::Number(n) => n,
            other => panic!("{at} = {other:?}"),
        }
    }
    fn on(&mut self) {
        self.wb.calc.precision_as_displayed = true;
        self.calc.recalc_all(&mut self.wb);
    }
    fn off(&mut self) {
        self.wb.calc.precision_as_displayed = false;
        self.calc.recalc_all(&mut self.wb);
    }
}

/// A formula's result is rounded to its cell's format, and a formula that reads it reads the
/// rounded value (Excel: `=1/3` in a `0.00` cell stores 0.33, so `=A1*3` is 0.99).
#[test]
fn a_result_is_rounded_before_its_dependents_read_it() {
    let mut t = T::new();
    t.set("A1", "=1/3");
    t.set("B1", "=A1*3");
    t.fmt("A1", "0.00");
    assert_eq!(t.num("A1"), 1.0 / 3.0);
    assert_eq!(t.num("B1"), 1.0);
    t.on();
    assert_eq!(t.num("A1"), 0.33);
    assert_eq!(t.num("B1"), 0.99);
    // Off again: a recalculation brings back full precision (the formula re-evaluates its text).
    t.off();
    assert_eq!(t.num("A1"), 1.0 / 3.0);
    assert_eq!(t.num("B1"), 1.0);
}

/// Excel rounds to the section that displays the number: fixed decimals, percent and thousands.
#[test]
fn the_format_decides_the_digits() {
    let mut t = T::new();
    t.set("A1", "=1.23456");
    t.set("A2", "=0.123456");
    t.set("A3", "=1234.5678");
    t.set("A4", "=1.005");
    t.fmt("A1", "0.00");
    t.fmt("A2", "0.00%");
    t.fmt("A3", "#,##0");
    t.fmt("A4", "0.00");
    t.on();
    assert_eq!(t.num("A1"), 1.23);
    assert_eq!(t.num("A2"), 0.1235);
    assert_eq!(t.num("A3"), 1235.0);
    // The display's own rounding, not a truncated one.
    assert_eq!(t.num("A4"), 1.01);
}

/// General, date and text formats keep the full value: Excel rounds only what a number section
/// shows.
#[test]
fn general_and_dates_are_never_rounded() {
    let mut t = T::new();
    t.set("A1", "=1/3");
    t.set("A2", "=45000.123456");
    t.set("A3", "=1/3");
    t.fmt("A2", "h:mm:ss");
    t.fmt("A3", "0.000");
    t.on();
    assert_eq!(t.num("A1"), 1.0 / 3.0, "General keeps every digit");
    assert_eq!(t.num("A2"), 45000.123456, "a time format keeps the serial");
    assert_eq!(t.num("A3"), 0.333);
}

/// A dynamic array's elements each take the format of the cell they land in.
#[test]
fn a_spill_is_rounded_cell_by_cell() {
    let mut t = T::new();
    t.set("F1", "=SEQUENCE(3,1,0.123,0.111)");
    t.set("G1", "=SUM(F1:F3)");
    t.fmt("F1", "0.0");
    t.fmt("F2", "0.0");
    t.on();
    let sh = t.wb.sheet(0).expect("sheet");
    let at = |a: &str| sh.value(c(a));
    assert_eq!((at("F1"), at("F2"), at("F3")), (Value::Number(0.1), Value::Number(0.2), Value::Number(0.123 + 0.111 * 2.0)));
    assert_eq!(t.num("G1"), 0.1 + 0.2 + (0.123 + 0.111 * 2.0));
}

/// Only numbers change: text, logical values and errors keep their value.
#[test]
fn other_values_are_left_alone() {
    let mut t = T::new();
    t.set("H1", "=TRUE");
    t.set("H2", "=\"1.5\"");
    t.set("H3", "=1/0");
    t.fmt("H1", "0");
    t.fmt("H2", "0");
    t.on();
    assert_eq!(t.get("H1"), Value::Bool(true));
    assert_eq!(t.get("H2"), Value::from("1.5"));
    assert_eq!(t.get("H3"), Value::Error(CellError::Div0));
}
