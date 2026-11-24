//! Function-call dispatch: the functions the evaluator answers itself, the aggregates a pass
//! folds a reference into, LAMBDA application, and the data-model functions.

use std::sync::Arc;

use cella_core::{Array, CellError, RangeRef, Value, compare};
use cella_formula::Expr;
use cella_functions::Arg;

use super::array::{Acc, Agg, broadcast_len, single, zip2, zip3};
use super::format::{cell_format_code, same_kind};
use super::special::{ev_shape, resized_sum_range, resizes_sum_range, trimmed};
use super::{Area, Ev, Evaluator, FnCtx, Lambda, MAX_CELLS, MAX_LAMBDA_CALLS, Pipe, err};

impl<'h> Evaluator<'h> {
    pub(super) fn call(&mut self, name: &str, args: &[Expr]) -> Ev {
        match name {
            "IF" => {
                if args.is_empty() || args.len() > 3 {
                    return err(CellError::Value);
                }
                let c = self.arg_val(args, 0).unwrap_or_default();
                if let Value::Array(arr) = &c {
                    // Array condition: element-wise.
                    let t = self.arg_val(args, 1).unwrap_or(Value::Bool(true));
                    let f = if args.len() > 2 { self.arg_val(args, 2).unwrap_or(Value::Bool(false)) } else { Value::Bool(false) };
                    return Ev::V(zip3(arr, &t, &f, |c, t, f| match c.to_bool() {
                        Ok(true) => t.clone(),
                        Ok(false) => f.clone(),
                        Err(e) => Value::Error(e),
                    }));
                }
                match c.to_bool() {
                    Ok(true) => match args.get(1) {
                        Some(Expr::Missing) => Ev::V(Value::Number(0.0)),
                        Some(e) => self.eval(e),
                        None => Ev::V(Value::Bool(true)),
                    },
                    Ok(false) => match args.get(2) {
                        Some(Expr::Missing) => Ev::V(Value::Number(0.0)),
                        Some(e) => self.eval(e),
                        None => Ev::V(Value::Bool(false)),
                    },
                    Err(e) => err(e),
                }
            }
            "IFS" => {
                if args.len() < 2 || !args.len().is_multiple_of(2) {
                    return err(CellError::Value);
                }
                for pair in args.chunks(2) {
                    let [c, v] = pair else { break };
                    let cv = self.value(c);
                    match cv.scalar().to_bool() {
                        Ok(true) => return self.eval(v),
                        Ok(false) => {}
                        Err(e) => return err(e),
                    }
                }
                err(CellError::NA)
            }
            "IFERROR" | "IFNA" => {
                if args.len() != 2 {
                    return err(CellError::Value);
                }
                let v = self.arg_ev(args, 0).unwrap_or(Ev::V(Value::Empty));
                let na_only = name == "IFNA";
                let catch = |e: CellError| if na_only { e == CellError::NA } else { true };
                match &v {
                    Ev::V(Value::Error(e)) if catch(*e) => self.eval(&args[1]),
                    Ev::V(Value::Array(a)) if a.data.iter().any(|x| x.as_error().is_some_and(catch)) => {
                        let alt = self.arg_val(args, 1).unwrap_or_default();
                        let data = a.data.iter().map(|x| if x.as_error().is_some_and(catch) { alt.scalar() } else { x.clone() }).collect();
                        Ev::V(Array::new(a.rows, a.cols, data).map(Value::from).unwrap_or(Value::Error(CellError::Value)))
                    }
                    Ev::R(_) => {
                        let val = self.deref(v.clone());
                        match val {
                            Value::Error(e) if catch(e) => self.eval(&args[1]),
                            Value::Array(ref a) if a.data.iter().any(|x| x.as_error().is_some_and(catch)) => {
                                let alt = self.arg_val(args, 1).unwrap_or_default();
                                let data = a.data.iter().map(|x| if x.as_error().is_some_and(catch) { alt.scalar() } else { x.clone() }).collect();
                                Ev::V(Array::new(a.rows, a.cols, data).map(Value::from).unwrap_or(Value::Error(CellError::Value)))
                            }
                            _ => v,
                        }
                    }
                    _ => v,
                }
            }
            "CHOOSE" => {
                if args.len() < 2 {
                    return err(CellError::Value);
                }
                let index = self.value(&args[0]);
                if let Value::Array(_) = index {
                    // An array of indexes picks element by element: `CHOOSE({1,2},A1:A3,B1:B3)`
                    // is a two-column array. The library CHOOSE lifts over its arguments.
                    let Some(spec) = cella_functions::lookup(name) else { return err(CellError::Name) };
                    let mut evaluated = vec![Arg { value: index, from_ref: false }];
                    for a in &args[1..] {
                        let value = self.value(a);
                        evaluated.push(Arg { value, from_ref: false });
                    }
                    let mut ctx = FnCtx { ev: self, criteria: None, lambda: None };
                    return cella_functions::call(spec, &evaluated, &mut ctx).pipe(Ev::V);
                }
                let i = match index.to_number() {
                    Ok(n) => n.trunc() as i64,
                    Err(e) => return err(e),
                };
                if i < 1 || i as usize >= args.len() {
                    return err(CellError::Value);
                }
                self.eval(&args[i as usize])
            }
            "SWITCH" => {
                if args.len() < 3 {
                    return err(CellError::Value);
                }
                let v = self.arg_val(args, 0).unwrap_or_default().scalar();
                if let Value::Error(e) = v {
                    return err(e);
                }
                let rest = &args[1..];
                let mut i = 0;
                while i + 1 < rest.len() {
                    let cand = self.value(&rest[i]).scalar();
                    if !cand.is_error() && compare(&v, &cand) == std::cmp::Ordering::Equal && same_kind(&v, &cand) {
                        return self.eval(&rest[i + 1]);
                    }
                    i += 2;
                }
                if rest.len() % 2 == 1 { rest.last().map_or(err(CellError::NA), |d| self.eval(d)) } else { err(CellError::NA) }
            }
            "ROW" | "COLUMN" => {
                let is_row = name == "ROW";
                if args.is_empty() || matches!(args.first(), Some(Expr::Missing)) {
                    return Ev::V(Value::number(if is_row { self.at.row + 1 } else { self.at.col + 1 } as f64));
                }
                match self.eval(&args[0]) {
                    Ev::R(a) => {
                        let Some(a) = a.first() else { return err(CellError::Ref) };
                        let r = self.trim_lines(a);
                        if is_row {
                            let rows: Vec<Value> = (r.start.row..=r.end.row).map(|x| Value::number(x as f64 + 1.0)).collect();
                            if rows.len() == 1 { Ev::V(rows.into_iter().next().unwrap_or_default()) } else { Ev::V(Array::column(rows).into()) }
                        } else {
                            let cols: Vec<Value> = (r.start.col..=r.end.col).map(|x| Value::number(x as f64 + 1.0)).collect();
                            if cols.len() == 1 { Ev::V(cols.into_iter().next().unwrap_or_default()) } else { Ev::V(Array::row(cols).into()) }
                        }
                    }
                    Ev::V(Value::Error(e)) => err(e),
                    _ => err(CellError::Value),
                }
            }
            "ROWS" | "COLUMNS" => {
                if args.len() != 1 {
                    return err(CellError::Value);
                }
                let is_rows = name == "ROWS";
                match self.eval(&args[0]) {
                    Ev::R(a) => match a.as_slice() {
                        [a] => Ev::V(Value::number(if is_rows { a.range.height() } else { a.range.width() } as f64)),
                        _ => err(CellError::Ref),
                    },
                    Ev::V(Value::Array(arr)) => Ev::V(Value::number(if is_rows { arr.rows } else { arr.cols } as f64)),
                    Ev::V(Value::Error(e)) => err(e),
                    Ev::V(_) => Ev::V(Value::Number(1.0)),
                    Ev::L(_) => err(CellError::Value),
                }
            }
            "AREAS" => match args.first().map(|a| self.eval(a)) {
                Some(Ev::R(a)) => Ev::V(Value::number(a.len() as f64)),
                Some(Ev::V(Value::Error(e))) => err(e),
                _ => err(CellError::Value),
            },
            "ISREF" => Ev::V(Value::Bool(matches!(args.first().map(|a| self.eval(a)), Some(Ev::R(_))))),
            "ISFORMULA" | "FORMULATEXT" => {
                let Some(Ev::R(a)) = args.first().map(|a| self.eval(a)) else { return err(CellError::NA) };
                let Some(a) = a.first() else { return err(CellError::NA) };
                let f = self.host.workbook().sheet(a.sheet).and_then(|s| s.cell(a.range.start)).and_then(|c| c.formula.clone());
                if name == "ISFORMULA" {
                    Ev::V(Value::Bool(f.is_some()))
                } else {
                    match f {
                        Some(f) => Ev::V(Value::text(format!("={}", f.text_at(a.range.start).unwrap_or_default()))),
                        None => err(CellError::NA),
                    }
                }
            }
            "SHEET" | "SHEETS" => {
                let wb = self.host.workbook();
                if args.is_empty() {
                    return Ev::V(Value::number(if name == "SHEET" { self.sheet + 1 } else { wb.sheets.len() } as f64));
                }
                match self.eval(&args[0]) {
                    Ev::R(a) => {
                        if name == "SHEET" {
                            a.first().map_or(err(CellError::Ref), |a| Ev::V(Value::number(a.sheet as f64 + 1.0)))
                        } else {
                            let mut s: Vec<usize> = a.iter().map(|x| x.sheet).collect();
                            s.dedup();
                            Ev::V(Value::number(s.len() as f64))
                        }
                    }
                    Ev::V(Value::Text(t)) if name == "SHEET" => {
                        let wb = self.host.workbook();
                        wb.sheet_index(&t).map_or(err(CellError::NA), |i| Ev::V(Value::number(i as f64 + 1.0)))
                    }
                    _ => err(CellError::NA),
                }
            }
            "OFFSET" => self.offset(args),
            "INDIRECT" => self.indirect(args),
            "GETPIVOTDATA" => self.getpivotdata(args),
            "INDEX" => self.index(args),
            // The two exact lookups answer from a scan of the key column when their shape allows it
            // (a literal or scalar key, one column, an exact match): the general path builds the
            // whole block into an `Array` first, which is 25-40 ms at a million rows to read the
            // first hundred — `eval/reference_fns.rs`.
            "MATCH" => self.match_exact_column(args).unwrap_or_else(|| self.builtin("MATCH", args)),
            "VLOOKUP" => self.vlookup_exact_column(args).unwrap_or_else(|| self.builtin("VLOOKUP", args)),
            "XLOOKUP" => self.xlookup(args),
            "CELL" => self.cell_info(args),
            "MEASURE" => self.measure(args),
            "TRIMRANGE" => self.trimrange(args),
            "SUBTOTAL" | "AGGREGATE" => self.subtotal(name, args),
            "TEXT" => {
                if args.len() != 2 {
                    return err(CellError::Value);
                }
                let v = self.arg_val(args, 0).unwrap_or_default();
                let f = self.arg_val(args, 1).unwrap_or_default();
                let sys = self.host.workbook().date_system;
                let one = |v: &Value, f: &Value| -> Value {
                    if let Value::Error(e) = v {
                        return Value::Error(*e);
                    }
                    let code = match f.to_text() {
                        Ok(c) => c,
                        Err(e) => return Value::Error(e),
                    };
                    match cella_numfmt::text_function(v, &code, sys) {
                        Ok(s) => Value::text(s),
                        Err(e) => Value::Error(e),
                    }
                };
                Ev::V(zip2(&v, &f, one))
            }
            "LET" => self.let_(args),
            "LAMBDA" => {
                if args.is_empty() {
                    return err(CellError::Value);
                }
                let mut params = Vec::new();
                for p in &args[..args.len() - 1] {
                    match p {
                        Expr::Name(n) => params.push(n.trim_start_matches('[').trim_end_matches(']').to_string()),
                        _ => return err(CellError::Value),
                    }
                }
                let Some(body) = args.last() else { return err(CellError::Value) };
                Ev::L(Arc::new(Lambda { params, body: body.clone(), env: self.env.clone() }))
            }
            "ISOMITTED" => {
                let r = matches!(args.first(), Some(Expr::Name(n)) if self.env.iter().rev().find(|(k, _)| cella_formula::same_name(k, n)).is_some_and(|(_, v)| matches!(v, Ev::V(Value::Empty))));
                Ev::V(Value::Bool(r))
            }
            "MAP" | "REDUCE" | "SCAN" | "BYROW" | "BYCOL" | "MAKEARRAY" => self.higher_order(name, args),
            "GROUPBY" | "PIVOTBY" => {
                // The aggregation may be a function given by name (`SUM`): pass it as its name.
                let i = if name == "GROUPBY" { 2 } else { 3 };
                match args.get(i).and_then(|a| self.function_by_name(a)) {
                    Some(f) => {
                        let mut args = args.to_vec();
                        if let Some(a) = args.get_mut(i) {
                            *a = Expr::Text(f.into());
                        }
                        self.builtin(name, &args)
                    }
                    None => self.builtin(name, args),
                }
            }
            "NOW" | "TODAY" | "RAND" | "RANDBETWEEN" | "RANDARRAY" => {
                self.host.note_volatile();
                self.builtin(name, args)
            }
            _ => self.builtin(name, args),
        }
    }

    pub(super) fn builtin(&mut self, name: &str, args: &[Expr]) -> Ev {
        let Some(spec) = cella_functions::lookup(name) else {
            // A defined name holding a LAMBDA, or a LET-bound lambda.
            if let Ev::L(l) = self.name(name) {
                return self.apply_lambda(&l, args);
            }
            return err(CellError::Name);
        };
        if args.len() < spec.min_args || args.len() > spec.max_args {
            return err(CellError::Value);
        }
        if let Some(kind) = Agg::of(spec.name) {
            return self.aggregate(kind, args);
        }
        if spec.name == "SUMIFS"
            && let Some(value) = self.sumifs_numeric_columns(args)
        {
            return Ev::V(value);
        }
        let mut evaluated = Vec::with_capacity(args.len());
        // SUMIF/AVERAGEIF: the size of the criteria range (argument 1), which the sum range takes.
        let mut criteria_shape = None;
        // Database functions: the criteria range (argument 2), whose formulas are computed criteria.
        let mut criteria = None;
        // GROUPBY/PIVOTBY: the LAMBDA given as their function.
        let mut lambda = None;
        let lambda_at = match name {
            "GROUPBY" => Some(2),
            "PIVOTBY" => Some(3),
            _ => None,
        };
        for (i, a) in args.iter().enumerate() {
            let mut ev = self.eval(a);
            if lambda_at == Some(i)
                && let Ev::L(l) = &ev
            {
                lambda = Some(l.clone());
                ev = Ev::V(Value::Empty);
            }
            if spec.category == cella_functions::Category::Database
                && i == 2
                && let Ev::R(areas) = &ev
                && let [area] = areas.as_slice()
            {
                criteria = Some(*area);
            }
            if resizes_sum_range(name, args.len()) {
                if i == 0 {
                    criteria_shape = ev_shape(&ev);
                } else if i == 2
                    && let (Some((h, w)), Ev::R(areas)) = (criteria_shape, &ev)
                    && let [area] = areas.as_slice()
                {
                    ev = Ev::R(vec![Area { sheet: area.sheet, range: resized_sum_range(area.range, h, w) }]);
                }
            }
            let from_ref = matches!(ev, Ev::R(_));
            let value = match ev {
                Ev::R(areas) if areas.len() > 1 => {
                    // Multi-area references (unions): flatten into one row for aggregates.
                    let mut vals = Vec::new();
                    for ar in &areas {
                        match self.area_value(ar) {
                            Value::Array(a) => vals.extend(a.data.iter().cloned()),
                            v => vals.push(v),
                        }
                    }
                    Value::from(Array::row(vals))
                }
                ev => self.deref(ev),
            };
            evaluated.push(Arg { value, from_ref });
        }
        // The function may go through every value it is given (a SUM inside each call of a LAMBDA
        // over a large array): the values count against the cell's work.
        let values = evaluated.iter().fold(0u64, |n, a| n.saturating_add(if let Value::Array(x) = &a.value { x.data.len() as u64 } else { 1 }));
        if !self.spend(values) {
            return err(CellError::Calc);
        }
        let database = criteria.is_some();
        let mut ctx = FnCtx { ev: self, criteria, lambda };
        let v = cella_functions::call(spec, &evaluated, &mut ctx);
        // A computed criterion that ran out of work counted as not met: the result would leave
        // records out, so the whole call is `#CALC!`.
        if database && self.exhausted {
            return err(CellError::Calc);
        }
        Ev::V(v)
    }

    /// Try the packed single-pair SUMIFS only when inspecting its arguments cannot calculate
    /// anything. A decline leaves the original argument evaluation and budget untouched.
    fn sumifs_numeric_columns(&mut self, args: &[Expr]) -> Option<Value> {
        let [Expr::Ref(target), Expr::Ref(criteria), Expr::Text(text)] = args else { return None };
        // These leaves normally enter eval once each. Near the stack/depth guard the generic
        // path must retain its own ordering of errors and suspensions.
        if self.depth >= super::MAX_DEPTH || super::stack_position().abs_diff(self.stack_top) > super::STACK_BUDGET - 16 * 1024 {
            return None;
        }
        let criterion = cella_functions::NumericOrderingCriterion::parse(&Value::Text(text.clone()))?;
        let target_areas = self.reference_areas(target).ok()?;
        let criteria_areas = self.reference_areas(criteria).ok()?;
        let ([target], [criteria]) = (target_areas.as_slice(), criteria_areas.as_slice()) else { return None };
        let (mut target, mut criteria) = (*target, *criteria);
        if target.sheet != criteria.sheet || target.range.width() != 1 || criteria.range.width() != 1 {
            return None;
        }
        if trimmed(target.range) {
            target.range = self.trim(&target);
        }
        if trimmed(criteria.range) {
            criteria.range = self.trim(&criteria);
        }
        let rows = u64::from(target.range.height());
        if rows <= 1 || rows > MAX_CELLS || rows != u64::from(criteria.range.height()) {
            return None;
        }
        // Generic builtin: three leaf evaluations, two area reads, and both arrays plus the
        // literal charged to the function call. Decline before any scan when budget is short.
        let work = 4 * rows + 4;
        if self.work < work {
            return None;
        }
        let value = self.host.sumifs_numeric_columns(target, criteria, criterion)?;
        self.spend(work).then_some(value)
    }

    /// Performs aggregation operations without copying its ranges: blocks are folded over the stored
    /// cells, so no array is built and there is no `MAX_CELLS` limit. Arguments are evaluated and read one
    /// after another, as the array path does, so volatile formulas (like `RAND`) see the same order.
    /// Only `SUM`, `COUNT` and `AVG` are supported currently.
    fn aggregate(&mut self, kind: Agg, args: &[Expr]) -> Ev {
        let mut acc = Acc::default();
        let mut first_error = None;
        for a in args {
            let ev = self.eval(a);
            let steps = match ev {
                Ev::R(areas) => areas.iter().map(|area| self.aggregate_area(kind, area, &mut acc)).collect(),
                ev => {
                    let value = self.deref(ev);
                    vec![self.aggregate_value(kind, value, false, &mut acc)]
                }
            };
            if first_error.is_none() {
                first_error = steps.into_iter().find_map(Result::err);
            }
        }
        first_error.unwrap_or_else(|| Ev::V(acc.finish(kind)))
    }

    /// Adds one area to `acc`; `Err` is the result the function ends with.
    fn aggregate_area(&mut self, kind: Agg, a: &Area, acc: &mut Acc) -> Result<(), Ev> {
        if !a.range.is_single() {
            // Folded without building an array, but still walked: the part inside the used range
            // counts against the cell's work, so an aggregate inside every LAMBDA call runs out.
            let range = if trimmed(a.range) { self.trim(a) } else { a.range };
            if !self.spend(u64::from(range.height()) * u64::from(range.width())) {
                return Err(err(CellError::Calc));
            }
            match self.host.visit_aggregate(a.sheet, a.range, kind, acc) {
                Some(Ok(())) => return Ok(()),
                Some(Err(e)) => return Err(err(e)),
                None => {}
            }
            let mut error = None;
            let walked = self.host.visit_range(a.sheet, a.range, &mut |v| match acc.add_cell(kind, v) {
                Ok(()) => true,
                Err(e) => {
                    error = Some(e);
                    false
                }
            });
            if walked {
                return error.map_or(Ok(()), |e| Err(err(e)));
            }
        }
        let value = self.area_value(a);
        self.aggregate_value(kind, value, true, acc)
    }

    /// Adds one argument's value by calling `SUM` and `COUNT` on it, so coercion and error rules
    /// stay in one place (`AVERAGE` of an argument is its `SUM` over its `COUNT`).
    fn aggregate_value(&mut self, kind: Agg, value: Value, from_ref: bool, acc: &mut Acc) -> Result<(), Ev> {
        // A value computed inline (not read from a reference, which `aggregate_area` charged)
        // counts against the cell's work, as it does when it is handed to any other function.
        if !from_ref && !self.spend_on(&value) {
            return Err(err(CellError::Calc));
        }
        if matches!(kind, Agg::Max | Agg::Min) {
            // The function already knows how a direct argument coerces (`MAX("7")` is 7) and what
            // an array of them folds to, so a non-reference argument goes through it.
            let name = if kind == Agg::Max { "MAX" } else { "MIN" };
            let Some(f) = cella_functions::lookup(name) else { return Err(err(CellError::Name)) };
            let args = [Arg { value, from_ref }];
            match cella_functions::call(f, &args, &mut FnCtx { ev: self, criteria: None, lambda: None }) {
                Value::Number(n) => {
                    acc.sum += n;
                    acc.count = acc.count.saturating_add(1);
                    acc.note(n, kind);
                    return Ok(());
                }
                other => return Err(Ev::V(other)),
            }
        }
        let (Some(sum), Some(count)) = (cella_functions::lookup("SUM"), cella_functions::lookup("COUNT")) else {
            return Err(err(CellError::Name));
        };
        let args = [Arg { value, from_ref }];
        if kind != Agg::Count {
            match cella_functions::call(sum, &args, &mut FnCtx { ev: self, criteria: None, lambda: None }) {
                Value::Number(n) => acc.sum += n,
                other => return Err(Ev::V(other)),
            }
        }
        if kind != Agg::Sum {
            match cella_functions::call(count, &args, &mut FnCtx { ev: self, criteria: None, lambda: None }) {
                Value::Number(n) => acc.count = acc.count.saturating_add(n as u64),
                other => return Err(Ev::V(other)),
            }
        }
        Ok(())
    }

    pub fn apply_lambda(&mut self, l: &Lambda, args: &[Expr]) -> Ev {
        // Too many or too few argument positions is #VALUE!. An explicitly empty position, as in
        // `(1,)`, is `Expr::Missing` and still counts as a position (it binds as omitted). `f()`
        // parses to no arguments, which for a one-parameter lambda can also be read as one empty
        // position, so that call still binds the parameter as omitted.
        let positions_ok = args.len() == l.params.len() || (args.is_empty() && l.params.len() == 1);
        if !positions_ok {
            return err(CellError::Value);
        }
        let mut bound = Vec::with_capacity(l.params.len());
        for (i, p) in l.params.iter().enumerate() {
            let v = match args.get(i) {
                Some(Expr::Missing) | None => Ev::V(Value::Empty),
                Some(e) => self.eval(e),
            };
            bound.push((p.clone(), v));
        }
        self.call_lambda_values(l, bound)
    }

    pub(super) fn call_lambda_values(&mut self, l: &Lambda, bound: Vec<(String, Ev)>) -> Ev {
        if self.calls >= MAX_LAMBDA_CALLS {
            return err(CellError::Num);
        }
        // One charge per invocation, so a cell's total work stays bounded however the lambdas
        // nest. Charging here, at the one place a body runs, means every path into one (direct
        // application, `MAP`/`REDUCE`/`BYROW` helpers, D-function criteria) is metered.
        self.fuel = match self.fuel.checked_sub(1) {
            Some(f) => f,
            None => return err(CellError::Calc),
        };
        self.calls += 1;
        let saved = std::mem::replace(&mut self.env, l.env.clone());
        self.env.extend(bound);
        let r = self.eval(&l.body);
        self.env = saved;
        self.calls -= 1;
        r
    }

    fn let_(&mut self, args: &[Expr]) -> Ev {
        if args.len() < 3 || args.len().is_multiple_of(2) {
            return err(CellError::Value);
        }
        let base = self.env.len();
        for pair in args[..args.len() - 1].chunks(2) {
            let [Expr::Name(n), v] = pair else {
                self.env.truncate(base);
                return err(CellError::Value);
            };
            let val = self.eval(v);
            self.env.push((n.clone(), val));
        }
        let r = match args.last() {
            Some(body) => self.eval(body),
            None => err(CellError::Value),
        };
        self.env.truncate(base);
        r
    }

    /// A worksheet function given by name where a function is expected (`BYROW(A1:C3,SUM)`,
    /// `GROUPBY(A2:A9,B2:B9,SUM)`): its name, unless a LET/LAMBDA variable or a defined name has
    /// that name.
    fn function_by_name(&self, e: &Expr) -> Option<String> {
        let Expr::Name(n) = e else { return None };
        if self.env.iter().any(|(k, _)| cella_formula::same_name(k, n)) || self.host.workbook().name(n, self.sheet).is_some() {
            return None;
        }
        let u = n.to_ascii_uppercase();
        cella_functions::lookup(&u).map(|_| u)
    }

    /// The function argument of a LAMBDA helper; a function given by name takes `arity`
    /// arguments.
    fn lambda_arg(&mut self, e: Option<&Expr>, arity: usize) -> Option<Arc<Lambda>> {
        if let Some(f) = e.and_then(|x| self.function_by_name(x)) {
            let params: Vec<String> = (0..arity).map(|i| format!("_xleta.{i}")).collect();
            let body = Expr::Call(f, params.iter().map(|p| Expr::Name(p.clone())).collect());
            return Some(Arc::new(Lambda { params, body, env: Vec::new() }));
        }
        match e.map(|x| self.eval(x)) {
            Some(Ev::L(l)) => Some(l),
            _ => None,
        }
    }

    fn higher_order(&mut self, name: &str, args: &[Expr]) -> Ev {
        let as_array = |v: Value| -> Arc<Array> {
            match v {
                Value::Array(a) => a,
                other => Arc::new(Array::scalar(other)),
            }
        };
        match name {
            "MAP" => {
                if args.len() < 2 {
                    return err(CellError::Value);
                }
                let Some(l) = self.lambda_arg(args.last(), args.len() - 1) else { return err(CellError::Value) };
                let arrays: Vec<Arc<Array>> = args[..args.len() - 1].iter().map(|a| as_array(self.value(a))).collect();
                let rows = arrays.iter().map(|a| a.rows).max().unwrap_or(1);
                let cols = arrays.iter().map(|a| a.cols).max().unwrap_or(1);
                let Some(n) = broadcast_len(rows, cols) else { return err(CellError::Num) };
                // The LAMBDA takes one parameter per array.
                if l.params.len() != arrays.len() {
                    return err(CellError::Value);
                }
                let mut out = Vec::with_capacity(n);
                for r in 0..rows {
                    for c in 0..cols {
                        let bound = l.params.iter().zip(arrays.iter()).map(|(p, a)| (p.clone(), Ev::V(a.get_broadcast(r, c)))).collect();
                        let v = self.call_lambda_values(&l, bound);
                        let Some(one) = single(self.deref(v)) else { return err(CellError::Calc) };
                        out.push(one);
                    }
                }
                Ev::V(Array::new(rows, cols, out).map(Value::from).unwrap_or(Value::Error(CellError::Calc)))
            }
            "REDUCE" | "SCAN" => {
                if args.len() != 3 {
                    return err(CellError::Value);
                }
                let mut acc = self.value(&args[0]);
                let arr = as_array(self.value(&args[1]));
                let Some(l) = self.lambda_arg(args.get(2), 2) else { return err(CellError::Value) };
                if l.params.len() != 2 {
                    return err(CellError::Value);
                }
                let mut scan = Vec::with_capacity(arr.data.len());
                for v in arr.data.iter() {
                    let bound = vec![(l.params[0].clone(), Ev::V(acc.clone())), (l.params[1].clone(), Ev::V(v.clone()))];
                    let r = self.call_lambda_values(&l, bound);
                    acc = self.deref(r);
                    if name == "SCAN" {
                        let Some(one) = single(acc.clone()) else { return err(CellError::Calc) };
                        scan.push(one);
                    }
                }
                if name == "SCAN" {
                    Ev::V(Array::new(arr.rows, arr.cols, scan).map(Value::from).unwrap_or(Value::Error(CellError::Calc)))
                } else {
                    Ev::V(acc)
                }
            }
            "BYROW" | "BYCOL" => {
                if args.len() != 2 {
                    return err(CellError::Value);
                }
                let arr = as_array(self.value(&args[0]));
                let Some(l) = self.lambda_arg(args.get(1), 1) else { return err(CellError::Value) };
                let Some(p) = l.params.first().cloned() else { return err(CellError::Value) };
                if l.params.len() != 1 {
                    return err(CellError::Value);
                }
                let by_row = name == "BYROW";
                let n = if by_row { arr.rows } else { arr.cols };
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let slice: Vec<Value> = if by_row {
                        (0..arr.cols).map(|c| arr.get(i, c).cloned().unwrap_or_default()).collect()
                    } else {
                        (0..arr.rows).map(|r| arr.get(r, i).cloned().unwrap_or_default()).collect()
                    };
                    let a = if by_row { Array::row(slice) } else { Array::column(slice) };
                    let r = self.call_lambda_values(&l, vec![(p.clone(), Ev::V(a.into()))]);
                    let Some(one) = single(self.deref(r)) else { return err(CellError::Calc) };
                    out.push(one);
                }
                let a = if by_row { Array::column(out) } else { Array::row(out) };
                Ev::V(a.into())
            }
            "MAKEARRAY" => {
                if args.len() != 3 {
                    return err(CellError::Value);
                }
                let (rows, cols) = match (self.arg_num(args, 0, 0.0), self.arg_num(args, 1, 0.0)) {
                    (Ok(r), Ok(c)) => (r.trunc(), c.trunc()),
                    (Err(e), _) | (_, Err(e)) => return err(e),
                };
                if rows < 1.0 || cols < 1.0 || rows * cols > MAX_CELLS as f64 {
                    return err(CellError::Value);
                }
                let Some(l) = self.lambda_arg(args.get(2), 2) else { return err(CellError::Value) };
                if l.params.len() != 2 {
                    return err(CellError::Value);
                }
                let (rows, cols) = (rows as usize, cols as usize);
                let mut out = Vec::with_capacity(rows * cols);
                for r in 0..rows {
                    for c in 0..cols {
                        let bound = vec![
                            (l.params[0].clone(), Ev::V(Value::number(r as f64 + 1.0))),
                            (l.params[1].clone(), Ev::V(Value::number(c as f64 + 1.0))),
                        ];
                        let v = self.call_lambda_values(&l, bound);
                        let Some(one) = single(self.deref(v)) else { return err(CellError::Calc) };
                        out.push(one);
                    }
                }
                Ev::V(Array::new(rows, cols, out).map(Value::from).unwrap_or(Value::Error(CellError::Calc)))
            }
            _ => err(CellError::Name),
        }
    }

    /// `MEASURE(name, [field, value]…)`: a data-model measure evaluated in a filter context.
    ///
    /// Cella's own function, not Excel's: Excel reaches a model's measures through a PivotTable
    /// or the CUBE family, whose first argument is a connection string this workbook does not have.
    /// A field is written `Table[Column]`, or as a bare column name when exactly one model table
    /// has it.
    fn measure(&mut self, args: &[Expr]) -> Ev {
        let name = match self.arg_val(args, 0).map(|v| v.scalar().to_text()) {
            Some(Ok(t)) => t,
            Some(Err(e)) => return err(e),
            None => return err(CellError::Value),
        };
        let mut filters: Vec<(String, Value)> = Vec::new();
        if !(args.len() - 1).is_multiple_of(2) {
            return err(CellError::Value);
        }
        let mut i = 1;
        while i + 1 < args.len() {
            let field = match self.arg_val(args, i).map(|v| v.scalar().to_text()) {
                Some(Ok(t)) => t,
                Some(Err(e)) => return err(e),
                None => return err(CellError::Value),
            };
            let value = self.arg_val(args, i + 1).map(|v| v.scalar()).unwrap_or_default();
            filters.push((field, value));
            i += 2;
        }
        // Reading the workbook ends the mutable borrows above: nothing else may touch the host
        // while a model is being read.
        self.host.note_volatile();
        let wb = self.host.workbook();
        if wb.model.tables.is_empty() {
            return err(CellError::Name);
        }
        let Some(m) = wb.model.measure(&name) else { return err(CellError::Name) };
        let mut ctx = cella_dax::Context::new();
        for (field, value) in filters {
            let Some((table, column)) = split_model_field(&wb.model, &field) else { return err(CellError::Name) };
            ctx = ctx.filter(&table, &column, value);
        }
        match cella_dax::evaluate(&wb.model, &m.formula, &ctx) {
            Ok(v) => Ev::V(v),
            Err(e) => Ev::V(Value::Error(model_error(&e))),
        }
    }

    fn cell_info(&mut self, args: &[Expr]) -> Ev {
        let kind = match self.arg_val(args, 0).map(|v| v.scalar().to_text()) {
            Some(Ok(t)) => t.to_ascii_lowercase(),
            Some(Err(e)) => return err(e),
            None => return err(CellError::Value),
        };
        let area = match args.get(1).map(|a| self.eval(a)) {
            Some(Ev::R(a)) => a.first().copied(),
            None => Some(Area { sheet: self.sheet, range: RangeRef::cell(self.at) }),
            _ => return err(CellError::Value),
        };
        let Some(a) = area else { return err(CellError::Ref) };
        let c = a.range.start;
        let wb = self.host.workbook();
        let sheet = wb.sheet(a.sheet);
        match kind.as_str() {
            "address" => Ev::V(Value::text(format!("${}${}", cella_core::col_to_letters(c.col), c.row + 1))),
            "row" => Ev::V(Value::number(c.row as f64 + 1.0)),
            "col" => Ev::V(Value::number(c.col as f64 + 1.0)),
            "contents" => Ev::V(self.host.cell_value(a.sheet, c)),
            "type" => {
                let v = self.host.cell_value(a.sheet, c);
                Ev::V(Value::text(match v {
                    Value::Empty => "b",
                    Value::Text(_) => "l",
                    _ => "v",
                }))
            }
            "width" => Ev::V(Value::number(sheet.map_or(8.43, |s| cella_model::points_to_chars(s.col_width(c.col) as f64)).round())),
            // Excel's `<folder>/[<file>]<sheet>`, which the classic sheet-name formula splits on
            // the `]`. Empty until the workbook has a file of its own, as in Excel (#1489).
            "filename" => Ev::V(Value::text(match &wb.source_path {
                Some(file) => {
                    let name = file.file_name().unwrap_or_default().to_string_lossy();
                    let on = sheet.map_or("", |s| s.name.as_str());
                    file.parent().unwrap_or(std::path::Path::new("")).join(format!("[{name}]{on}")).to_string_lossy().into_owned()
                }
                None => String::new(),
            })),
            "sheetname" => Ev::V(Value::text(sheet.map(|s| s.name.clone()).unwrap_or_default())),
            "protect" => Ev::V(Value::number(if sheet.is_some_and(|s| wb.styles.get(s.style_id(c)).protection.locked) { 1.0 } else { 0.0 })),
            "format" => {
                let code = sheet.map(|s| wb.styles.get(s.style_id(c)).num_fmt.as_str().to_string()).unwrap_or_default();
                Ev::V(Value::text(cell_format_code(&code)))
            }
            "color" | "parentheses" => Ev::V(Value::Number(0.0)),
            "prefix" => Ev::V(Value::text("")),
            _ => err(CellError::Value),
        }
    }

    fn subtotal(&mut self, name: &str, args: &[Expr]) -> Ev {
        // SUBTOTAL needs function_num and ref1; AGGREGATE needs function_num, options and ref1.
        let min_args = if name == "AGGREGATE" { 3 } else { 2 };
        if args.len() < min_args {
            return err(CellError::Value);
        }
        let code = match self.arg_num(args, 0, 0.0) {
            Ok(n) => n.trunc() as u32,
            Err(e) => return err(e),
        };
        let (func, options, rest) = if name == "AGGREGATE" {
            let opt = match self.arg_num(args, 1, 0.0) {
                Ok(n) => n.trunc() as u32,
                Err(e) => return err(e),
            };
            (code, opt, &args[2..])
        } else {
            let (f, ignore_hidden) = if code > 100 { (code - 100, true) } else { (code, false) };
            (f, if ignore_hidden { 5 } else { 0 }, &args[1..])
        };
        if !(1..=19).contains(&func) || (name == "SUBTOTAL" && func > 11) {
            return err(CellError::Value);
        }
        let skip_hidden = matches!(options, 1 | 3 | 5 | 7);
        // SUBTOTAL leaves out rows a filter hides whatever the function number (Microsoft's
        // SUBTOTAL documentation); 101-111 also leave out rows hidden by hand.
        let skip_filtered = name == "SUBTOTAL";
        let skip_errors = matches!(options, 2 | 3 | 6 | 7);
        // SUBTOTAL always leaves out nested SUBTOTAL/AGGREGATE cells (101-111 too); AGGREGATE does so for options 0-3.
        let skip_nested = name == "SUBTOTAL" || options <= 3;
        let mut values = Vec::new();
        let mut k = None;
        let refs = if name == "AGGREGATE" && func >= 14 { &rest[..1.min(rest.len())] } else { rest };
        if name == "AGGREGATE" && func >= 14 {
            k = rest.get(1).map(|e| self.value(e));
        }
        // Every cell visited (even one skipped as hidden, nested or an error) and every value
        // counts against one budget for the whole call. All arguments are evaluated (references
        // stay lazy) and charged before any cell is read: a value stored in XFD1048576 makes
        // `A:XFD` trim to the whole sheet, ~17 billion cells (#2175).
        enum Input {
            Cells(usize, RangeRef),
            Values(Value),
        }
        let mut inputs = Vec::new();
        let mut budget = MAX_CELLS;
        for e in refs {
            match self.eval(e) {
                Ev::R(areas) => {
                    for a in areas {
                        let range = self.trim(&a);
                        budget = match budget.checked_sub(range.count()) {
                            Some(left) => left,
                            None => return err(CellError::Num),
                        };
                        inputs.push(Input::Cells(a.sheet, range));
                    }
                }
                Ev::V(v) => {
                    let n = match &v {
                        Value::Array(a) => u64::try_from(a.data.len()).unwrap_or(u64::MAX),
                        _ => 1,
                    };
                    budget = match budget.checked_sub(n) {
                        Some(left) => left,
                        None => return err(CellError::Num),
                    };
                    inputs.push(Input::Values(v));
                }
                Ev::L(_) => return err(CellError::Value),
            }
        }
        for input in inputs {
            match input {
                Input::Cells(sheet, range) => {
                    for c in range.iter() {
                        let (hidden, nested) = {
                            let wb = self.host.workbook();
                            let s = wb.sheet(sheet);
                            let hidden =
                                s.is_some_and(|s| if skip_hidden { s.is_row_hidden(c.row) } else { skip_filtered && s.is_row_filtered(c.row) });
                            let nested = skip_nested
                                && s.and_then(|s| s.cell(c)).and_then(|cell| cell.formula).is_some_and(|f| {
                                    let u = f.text_at(c).unwrap_or_default().to_ascii_uppercase();
                                    u.contains("SUBTOTAL(") || u.contains("AGGREGATE(")
                                });
                            (hidden, nested)
                        };
                        if hidden || nested {
                            continue;
                        }
                        let v = self.host.cell_value(sheet, c);
                        if skip_errors && v.is_error() {
                            continue;
                        }
                        values.push(v);
                    }
                }
                Input::Values(Value::Array(a)) => values.extend(a.data.iter().filter(|v| !(skip_errors && v.is_error())).cloned()),
                Input::Values(v) => values.push(v),
            }
        }
        if let Some(k) = k {
            return Ev::V(cella_functions::aggregate_values_k(func, &values, &k));
        }
        Ev::V(cella_functions::aggregate_values(func, &values))
    }
}

/// Splits `Table[Column]` (or a bare column name that exactly one table has) into its two names.
fn split_model_field(model: &cella_model::DataModel, field: &str) -> Option<(String, String)> {
    let text = field.trim();
    if let Some((table, rest)) = text.split_once('[')
        && let Some(column) = rest.strip_suffix(']')
    {
        let t = model.table(table.trim())?;
        if t.index(column.trim()).is_some() {
            return Some((t.name.clone(), column.trim().to_string()));
        }
        return None;
    }
    let mut found: Option<(String, String)> = None;
    for t in &model.tables {
        if t.index(text).is_some() {
            if found.is_some() {
                return None;
            }
            found = Some((t.name.clone(), text.to_string()));
        }
    }
    found
}

/// The worksheet error a measure's failure becomes: an unknown name is `#NAME?`, a formula that
/// cannot be computed is `#VALUE!`, and a measure that recurses too deep is `#NUM!`.
fn model_error(e: &cella_dax::DaxError) -> CellError {
    use cella_dax::DaxError;
    match e {
        DaxError::UnknownMeasure { .. } | DaxError::UnknownTable { .. } | DaxError::UnknownColumn { .. } => CellError::Name,
        DaxError::Cycle { .. } | DaxError::TooDeep { .. } => CellError::Num,
        _ => CellError::Value,
    }
}
