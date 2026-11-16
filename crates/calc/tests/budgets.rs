//! Budgets that keep a hostile formula from taking the process down.
//!
//! The evaluator meters every formula: a nesting cap, a per-cell work budget, a fuel charge per
//! LAMBDA invocation, a cap on the calls in progress and a budget on the native stack it may use.
//! A formula that runs past one of them gets an error value and the recalculation carries on with
//! the other cells; what it must never do is recurse until the stack overflows, because that
//! aborts the process (it cannot be caught) and the user loses the workbook.
//!
//! Every case runs on a thread with 1 MiB of stack, which is what Windows gives the main thread,
//! where commands and recalculation run: a guard proved on a roomy test thread proves nothing
//! about that one.

use gridcraft_calc::Calc;
use gridcraft_calc::eval::{Evaluator, Host, NameTarget};
use gridcraft_core::{CellError, CellRef, Value};
use gridcraft_model::{Cell, DefinedName, Formula, Workbook};

/// Runs `f` on a 1 MiB stack, the main thread's stack on Windows. A formula that overflows it
/// aborts the whole process, so the join is the assertion that nothing did.
fn on_a_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let handle = std::thread::Builder::new().stack_size(1 << 20).spawn(f);
    match handle {
        Ok(h) => match h.join() {
            Ok(v) => v,
            Err(_) => panic!("a formula panicked below the stack guard"),
        },
        Err(e) => panic!("could not spawn the small-stack thread: {e}"),
    }
}

/// One sheet, calculated after every edit.
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
            Cell::value(gridcraft_core::parse::parse_input(input, self.wb.date_system).value)
        };
        let at = CellRef::parse(at).expect("cell address");
        if let Some(sh) = self.wb.sheet_mut(0) {
            sh.cells.set(at, cell);
        }
        self.calc.cells_changed(&mut self.wb, &[(0, at)]);
    }
    /// Defines workbook-scoped names and rebuilds the graph, as an edit of the Name Manager does.
    fn define(&mut self, names: &[(&str, &str)]) {
        for (name, formula) in names {
            self.wb.names.push(DefinedName { name: (*name).into(), scope: None, formula: (*formula).into(), comment: String::new(), hidden: false });
        }
        self.calc.rebuild(&self.wb);
    }
    fn get(&self, at: &str) -> Value {
        let at = CellRef::parse(at).expect("cell address");
        self.wb.sheet(0).map(|s| s.value(at)).unwrap_or_default()
    }
    fn num(&self, at: &str) -> f64 {
        self.get(at).as_f64().unwrap_or_else(|| panic!("{at} = {:?}", self.get(at)))
    }
}

/// A self-referential LAMBDA used to recurse until the stack ran out, taking the process with it.
/// Both the ordinary recursion (`f` calling itself by name) and the call-through-argument shape
/// used by combinators now stop at a limit and answer `#NUM!`: the nesting cap, the call cap or
/// the stack budget, whichever comes first.
#[test]
fn a_self_referential_lambda_errors_instead_of_overflowing() {
    let (by_name, through_arguments, counting_down) = on_a_small_stack(|| {
        let mut t = T::new();
        // A name calling itself: LET cannot bind a name inside its own value, so this is the shape
        // a user reaches for (and the one Excel rejects with #NUM! too).
        t.define(&[("LOOP", "LAMBDA(n,LOOP(n))")]);
        t.set("A1", "=LOOP(1)");
        // The fixed-point combinator: no name, one LAMBDA applied to itself.
        t.set("A2", "=LET(f,LAMBDA(g,x,g(g,x)),f(f,1))");
        // A recursion that does terminate but goes as deep as is allowed to (131 steps is what
        // the nesting cap leaves room for) still computes.
        t.set("A3", "=LET(F,LAMBDA(me,n,IF(n<=0,0,1+me(me,n-1))),F(F,131))");
        (t.get("A1"), t.get("A2"), t.num("A3"))
    });
    assert_eq!(by_name, Value::Error(CellError::Num));
    assert_eq!(through_arguments, Value::Error(CellError::Num));
    assert_eq!(counting_down, 131.0);
}

/// A finite formula that evaluates deeper than the nesting cap: a chain of defined names, each
/// one the next (the parser caps a single formula's own tree, so the depth has to come from names).
/// The evaluator stops at the cap with `#NUM!` instead of following the chain as deep as it goes.
#[test]
fn a_formula_nested_past_the_cap_errors() {
    // The names are `Chain` + a number: not cell addresses (`N2` is), so each one reads the next.
    let chain = |links: usize| {
        let mut names: Vec<(String, String)> = (1..=links).map(|i| (format!("Chain{i}"), format!("Chain{}", i + 1))).collect();
        names.push((format!("Chain{}", links + 1), "1".into()));
        names
    };
    let (deep, deeper) = on_a_small_stack(move || {
        let run = |links: usize| {
            let mut t = T::new();
            let names = chain(links);
            let refs: Vec<(&str, &str)> = names.iter().map(|(n, f)| (n.as_str(), f.as_str())).collect();
            t.define(&refs);
            t.set("A1", "=Chain1");
            t.get("A1")
        };
        (run(300), run(1_000))
    });
    // 300 links is inside the cap (and reads the 1 at its end); 1,000 is past it.
    assert_eq!(deep.as_f64(), Some(1.0), "{deep:?}");
    assert_eq!(deeper, Value::Error(CellError::Num));
}

/// A chain of LAMBDA calls longer than the calls one formula may have in progress. Each step of
/// this one runs through a helper that makes two calls (`W` calls `f`, which calls itself), so
/// 70 steps are 140 calls: one step more than the cap allows, and one fewer still computes.
#[test]
fn a_lambda_chain_longer_than_the_call_cap_errors() {
    let chain = |steps: usize| format!("=LET(W,LAMBDA(f,k,f(f,k)),F,LAMBDA(me,n,IF(n<=0,0,1+W(me,n-1))),F(F,{steps}))");
    let (within, past) = on_a_small_stack(move || {
        let mut t = T::new();
        t.set("A1", &chain(69));
        t.set("A2", &chain(70));
        (t.get("A1"), t.get("A2"))
    });
    assert_eq!(within.as_f64(), Some(69.0), "{within:?}");
    assert_eq!(past, Value::Error(CellError::Num));
}

/// Long but legitimate formulas still compute: the budgets are sized for the widest formula a
/// user can write (every cell of a full-size array read a few times, one LAMBDA call per value),
/// not for the smallest one that runs away.
#[test]
fn long_but_legitimate_formulas_still_compute() {
    let value = on_a_small_stack(|| {
        let mut t = T::new();
        for r in 1..=5_000 {
            t.set(&format!("A{r}"), &r.to_string());
        }
        // 200 nested IFs, half the nesting cap.
        let nested = format!("={}7{}", "IF(TRUE,".repeat(200), ")".repeat(200));
        t.set("B1", &nested);
        // 5,000 numbers read once, and again through a LAMBDA called once per value.
        t.set("B2", "=SUM(A1:A5000)");
        t.set("B3", "=SUM(MAP(A1:A5000,LAMBDA(x,x)))");
        // A flat chain as long as Excel allows (4,096 operands) and a map over the whole column.
        t.set("B4", &format!("={}", vec!["1"; 4_096].join("+")));
        t.set("B5", "=SUM(MAP(SEQUENCE(10000),LAMBDA(x,x)))");
        (t.num("B1"), t.num("B2"), t.num("B3"), t.num("B4"), t.num("B5"))
    });
    assert_eq!(value.0, 7.0);
    assert_eq!(value.1, 12_502_500.0);
    assert_eq!(value.2, 12_502_500.0);
    assert_eq!(value.3, 4_096.0);
    assert_eq!(value.4, 50_005_000.0);
}

/// A host that says the formula started at address 0, i.e. that every `eval` is already past the
/// stack budget: what a formula calculated on demand deep below the outermost one sees. The
/// evaluator must answer `#NUM!` before recursing into anything, and say so to the host (which is
/// what makes the pass calculate that formula from the top instead).
struct NoRoom {
    wb: Workbook,
    exhausted: bool,
}

impl Host for NoRoom {
    fn workbook(&self) -> &Workbook {
        &self.wb
    }
    fn cell_value(&mut self, _sheet: usize, _at: CellRef) -> Value {
        Value::Empty
    }
    fn now_serial(&self) -> f64 {
        46_302.5
    }
    fn random(&mut self) -> f64 {
        0.5
    }
    fn name_target(&mut self, _sheet: usize, _name: &str) -> NameTarget {
        NameTarget::Unresolved
    }
    fn stack_top(&self) -> Option<usize> {
        Some(0)
    }
    fn stack_exhausted(&mut self) {
        self.exhausted = true;
    }
}

#[test]
fn the_stack_guard_answers_an_error_before_recursing_further() {
    let expr = gridcraft_formula::parse("1+2").expect("formula");
    let at = CellRef::parse("A1").expect("cell address");
    let mut host = NoRoom { wb: Workbook::new(), exhausted: false };
    let v = Evaluator::new(&mut host, 0, at).value(&expr);
    assert_eq!(v, Value::Error(CellError::Num));
    assert!(host.exhausted, "the host has to hear about it");
}

/// A long chain of formulas that read each other through INDIRECT, so the pass cannot order them
/// and reaches a cell from inside another one whenever it meets it first. Each cell is 300
/// evaluations deep, so this is the on-demand path at its deepest: every value still comes out
/// right, whichever order the pass happens to take.
#[test]
fn a_chain_of_dynamic_formulas_still_computes() {
    let (top, bottom) = on_a_small_stack(|| {
        // One cell's formula is 300 evaluations deep (each name reads the next).
        let mut names: Vec<(String, String)> = (1..=300).map(|i| (format!("Chain{i}"), format!("Chain{}", i + 1))).collect();
        names.push(("Chain301".to_string(), "1".to_string()));
        let mut t = T::new();
        let refs: Vec<(&str, &str)> = names.iter().map(|(n, f)| (n.as_str(), f.as_str())).collect();
        t.define(&refs);
        // INDIRECT makes the reference dynamic, so the pass cannot order these cells and reaches
        // each one from inside the one above it.
        for r in 1..=40 {
            t.set(&format!("A{r}"), &format!("=INDIRECT(\"A{}\")+Chain1", r + 1));
        }
        t.set("A41", "0");
        (t.num("A1"), t.num("A41"))
    });
    assert_eq!(top, 40.0);
    assert_eq!(bottom, 0.0);
}

/// The stack budget belongs to the outermost formula: a cell computed on demand in the middle of
/// another one starts counting from where that one started. Three cells that each recurse as deep
/// as the nesting cap allows (131 steps), one reading the next, still compute: they share the
/// 896 KiB budget rather than each starting a fresh one.
#[test]
fn formulas_computed_on_demand_share_the_stack_budget() {
    let (sum, error) = on_a_small_stack(|| {
        let countdown = |then: &str| format!("=LET(F,LAMBDA(me,n,IF(n<=0,{then},1+me(me,n-1))),F(F,131))");
        let mut t = T::new();
        for (at, then) in [("A1", "A2"), ("A2", "A3"), ("A3", "0")] {
            t.set(at, &countdown(then));
        }
        // The same with an endless recursion at the bottom: the cells that read it still compute.
        t.set("B1", &countdown("B2"));
        t.set("B2", "=LET(f,LAMBDA(g,x,g(g,x)),f(f,1))");
        t.set("B3", &countdown("0"));
        (t.num("A1"), t.get("B2"))
    });
    assert_eq!(sum, 393.0);
    assert_eq!(error, Value::Error(CellError::Num));
}

/// One cell that runs out of work is an error value like any other: the cells around it calculate,
/// and a later edit recalculates them (and it: still an error) without the pass getting stuck.
#[test]
fn the_other_cells_still_calculate_after_one_errors() {
    let (before, after) = on_a_small_stack(|| {
        let mut t = T::new();
        t.set("A1", "5");
        t.set("B1", "=A1*2");
        t.set("C1", "=LET(f,LAMBDA(g,x,g(g,x)),f(f,1))");
        t.set("D1", "=B1+1");
        t.set("E1", "=SUM(B1:D2)");
        let before = (t.num("B1"), t.get("C1"), t.num("D1"));
        t.set("A1", "7");
        let after = (t.num("B1"), t.get("C1"), t.num("D1"), t.get("E1"));
        (before, after)
    });
    assert_eq!(before, (10.0, Value::Error(CellError::Num), 11.0));
    assert_eq!(after.0, 14.0);
    assert_eq!(after.1, Value::Error(CellError::Num));
    assert_eq!(after.2, 15.0);
    // B1 (14) + D1 (15), with the error in C1 propagating as it does for any error value.
    assert_eq!(after.3, Value::Error(CellError::Num));
}
