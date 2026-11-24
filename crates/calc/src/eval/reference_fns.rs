//! The functions that work on references (OFFSET, INDIRECT, INDEX, XLOOKUP, GETPIVOTDATA, CELL)
//! and keep them unresolved for as long as they can.

use std::sync::Arc;

use cella_core::{Array, CellError, CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value};
use cella_formula::Expr;
use cella_functions::Arg;

use super::format::r1c1_to_a1;
use super::{Area, Ev, Evaluator, FnCtx, MAX_CELLS, err};

impl<'h> Evaluator<'h> {
    /// Whole rows/columns trimmed for ROW()/COLUMN() arrays.
    pub(super) fn trim_lines(&mut self, a: &Area) -> RangeRef {
        if a.range.is_full_cols() || a.range.is_full_rows() { self.trim(a) } else { a.range }
    }

    pub(super) fn offset(&mut self, args: &[Expr]) -> Ev {
        self.host.note_volatile();
        if args.len() < 3 || args.len() > 5 {
            return err(CellError::Value);
        }
        let base = match self.eval(&args[0]) {
            Ev::R(a) if a.len() == 1 => a[0],
            Ev::V(Value::Error(e)) => return err(e),
            _ => return err(CellError::Value),
        };
        let rows = match self.arg_num(args, 1, 0.0) {
            Ok(n) => n.trunc() as i64,
            Err(e) => return err(e),
        };
        let cols = match self.arg_num(args, 2, 0.0) {
            Ok(n) => n.trunc() as i64,
            Err(e) => return err(e),
        };
        let h = match self.arg_num(args, 3, base.range.height() as f64) {
            Ok(n) => n.trunc() as i64,
            Err(e) => return err(e),
        };
        let w = match self.arg_num(args, 4, base.range.width() as f64) {
            Ok(n) => n.trunc() as i64,
            Err(e) => return err(e),
        };
        if h == 0 || w == 0 {
            return err(CellError::Ref);
        }
        let r0 = (base.range.start.row as i64).saturating_add(rows);
        let c0 = (base.range.start.col as i64).saturating_add(cols);
        let (r0, r1) = if h > 0 { (r0, r0.saturating_add(h - 1)) } else { (r0.saturating_add(h + 1), r0) };
        let (c0, c1) = if w > 0 { (c0, c0.saturating_add(w - 1)) } else { (c0.saturating_add(w + 1), c0) };
        if r0 < 0 || c0 < 0 || r1 >= MAX_ROWS as i64 || c1 >= MAX_COLS as i64 {
            return err(CellError::Ref);
        }
        Ev::R(vec![Area { sheet: base.sheet, range: RangeRef::new(CellRef::new(r0 as u32, c0 as u32), CellRef::new(r1 as u32, c1 as u32)) }])
    }

    pub(super) fn indirect(&mut self, args: &[Expr]) -> Ev {
        self.host.note_volatile();
        if args.is_empty() || args.len() > 2 {
            return err(CellError::Value);
        }
        let text = match self.arg_val(args, 0).map(|v| v.scalar().to_text()) {
            Some(Ok(t)) => t,
            Some(Err(e)) => return err(e),
            None => return err(CellError::Value),
        };
        let a1 = match args.get(1) {
            None | Some(Expr::Missing) => true,
            Some(e) => {
                let v = self.value(e);
                v.scalar().to_bool().unwrap_or(true)
            }
        };
        // With a1 = FALSE, text that isn't R1C1 can still be a name, but never an A1 reference.
        let (text, a1_ok) = if a1 {
            (text, true)
        } else {
            match r1c1_to_a1(&text, self.at) {
                Ok(Some(a1_text)) => (a1_text, true),
                Ok(None) => (text, false),
                Err(e) => return err(e),
            }
        };
        match cella_formula::parse(&text) {
            Ok(Expr::Ref(_)) if !a1_ok => err(CellError::Ref),
            // The text names coordinates: an absolute tree, whatever this formula's own tree is.
            Ok(e @ (Expr::Ref(_) | Expr::Name(_) | Expr::Struct(_))) => match self.eval_absolute(&e) {
                r @ Ev::R(_) => r,
                _ => err(CellError::Ref),
            },
            _ => err(CellError::Ref),
        }
    }

    /// `GETPIVOTDATA(data_field, pivot_table, field1, item1, …)`: the report cell the pairs name.
    /// The pivot table argument must be a reference into a written report; the lookup itself lives
    /// in [`crate::pivot`].
    pub(super) fn getpivotdata(&mut self, args: &[Expr]) -> Ev {
        if args.len() < 2 || !(args.len() - 2).is_multiple_of(2) {
            return err(CellError::Value);
        }
        let data = match self.arg_val(args, 0).map(|v| v.scalar().to_text()) {
            Some(Ok(t)) => t,
            Some(Err(e)) => return err(e),
            None => return err(CellError::Value),
        };
        let area = match self.eval(&args[1]) {
            Ev::R(a) => a.first().copied(),
            Ev::V(Value::Error(e)) => return err(e),
            _ => None,
        };
        let Some(area) = area else { return err(CellError::Ref) };
        let mut pairs = Vec::with_capacity((args.len() - 2) / 2);
        let mut i = 2;
        while i + 1 < args.len() {
            let mut pair = [String::new(), String::new()];
            for (k, slot) in pair.iter_mut().enumerate() {
                *slot = match self.arg_val(args, i + k).map(|v| v.scalar().to_text()) {
                    Some(Ok(t)) => t,
                    Some(Err(e)) => return err(e),
                    None => return err(CellError::Value),
                };
            }
            let [field, item] = pair;
            pairs.push((field, item));
            i += 2;
        }
        Ev::V(crate::pivot::get(self.host.workbook(), area.sheet, area.range.start, &data, &pairs))
    }

    pub(super) fn index(&mut self, args: &[Expr]) -> Ev {
        if args.is_empty() || args.len() > 4 {
            return err(CellError::Value);
        }
        let src = self.eval(&args[0]);
        let mut arg = |i: usize| match args.get(i) {
            None | Some(Expr::Missing) => None,
            Some(e) => Some(self.value(e)),
        };
        let (rows, cols) = (arg(1), arg(2));
        let area_n = match self.arg_num(args, 3, 1.0) {
            Ok(n) => n.trunc() as i64,
            Err(e) => return err(e),
        };
        let two = args.len() == 2;
        let num = |v: &Option<Value>| v.as_ref().map_or(Ok(0.0), |v| v.to_number()).map(|n| n.trunc() as i64);
        // Arrays of positions pick a value for each: INDEX(A1:C10,{1;2},{1,3}) is 2×2.
        if let (Some(Value::Array(_)), _) | (_, Some(Value::Array(_))) = (&rows, &cols) {
            let as_array = |v: &Option<Value>| match v {
                Some(Value::Array(a)) => Arc::clone(a),
                Some(v) => Arc::new(Array::scalar(v.clone())),
                None => Arc::new(Array::scalar(Value::Number(0.0))),
            };
            let (ra, ca) = (as_array(&rows), as_array(&cols));
            let (h, w) = (ra.rows.max(ca.rows), ra.cols.max(ca.cols));
            if (h as u64).saturating_mul(w as u64) > MAX_CELLS {
                return err(CellError::Num);
            }
            let mut out = Vec::with_capacity(h * w);
            for r in 0..h {
                for c in 0..w {
                    let pos = (num(&Some(ra.get_broadcast(r, c))), num(&Some(ca.get_broadcast(r, c))));
                    let v = match pos {
                        (Ok(row), Ok(col)) => {
                            let one = self.index_one(src.clone(), row, col, area_n, two);
                            match self.deref(one) {
                                Value::Array(a) if a.rows * a.cols > 1 => Value::Error(CellError::Calc),
                                v => v.scalar(),
                            }
                        }
                        (Err(e), _) | (_, Err(e)) => Value::Error(e),
                    };
                    out.push(v);
                }
            }
            return Ev::V(Array::new(h, w, out).map(Value::from).unwrap_or(Value::Error(CellError::Value)));
        }
        let (row, col) = match (num(&rows), num(&cols)) {
            (Ok(r), Ok(c)) => (r, c),
            (Err(e), _) | (_, Err(e)) => return err(e),
        };
        self.index_one(src, row, col, area_n, two)
    }

    /// INDEX for one row and column position (0 = the whole column or row).
    fn index_one(&mut self, src: Ev, row: i64, col: i64, area_n: i64, two_args: bool) -> Ev {
        if row < 0 || col < 0 {
            return err(CellError::Value);
        }
        match src {
            Ev::R(areas) => {
                let area = usize::try_from((area_n as u64).saturating_sub(1)).ok();
                let Some(a) = area.and_then(|i| areas.get(i)).copied() else { return err(CellError::Ref) };
                let r = a.range;
                // A single row or column with one index picks along it.
                let (row, col) = if two_args && r.height() == 1 { (1, row) } else { (row, col) };
                if row as u64 > r.height() as u64 || col as u64 > r.width() as u64 {
                    return err(CellError::Ref);
                }
                let (r0, r1) = if row == 0 { (r.start.row, r.end.row) } else { (r.start.row + row as u32 - 1, r.start.row + row as u32 - 1) };
                let (c0, c1) = if col == 0 { (r.start.col, r.end.col) } else { (r.start.col + col as u32 - 1, r.start.col + col as u32 - 1) };
                Ev::R(vec![Area { sheet: a.sheet, range: RangeRef::new(CellRef::new(r0, c0), CellRef::new(r1, c1)) }])
            }
            Ev::V(Value::Array(arr)) => {
                let (row, col) = if two_args && arr.rows == 1 { (1, row) } else { (row, col) };
                if row as usize > arr.rows || col as usize > arr.cols {
                    return err(CellError::Ref);
                }
                match (row, col) {
                    (0, 0) => Ev::V(Value::Array(arr)),
                    (0, c) => Ev::V(Array::column((0..arr.rows).map(|r| arr.get(r, c as usize - 1).cloned().unwrap_or_default()).collect()).into()),
                    (r, 0) => Ev::V(Array::row((0..arr.cols).map(|c| arr.get(r as usize - 1, c).cloned().unwrap_or_default()).collect()).into()),
                    (r, c) => Ev::V(arr.get(r as usize - 1, c as usize - 1).cloned().unwrap_or(Value::Error(CellError::Ref))),
                }
            }
            Ev::V(Value::Error(e)) => err(e),
            Ev::V(v) => {
                if row <= 1 && col <= 1 {
                    Ev::V(v)
                } else {
                    err(CellError::Ref)
                }
            }
            Ev::L(_) => err(CellError::Value),
        }
    }

    /// The whole-column exact lookup, as a reference to the matched row of the return column:
    /// `Some(Err(_))` when the shape applies and the answer is an error, `None` when the shape does
    /// not apply and the general path below must run. Never evaluates the block.
    fn xlookup_scalar_column(&mut self, args: &[Expr]) -> Option<Ev> {
        // Three to six arguments: `if_not_found` may be given (argument 4), and the documented
        // defaults — exact match, first to last — may be spelled out (arguments 5 and 6). A
        // report's `XLOOKUP($A2, Codes!A:A, Codes!B:B, "?" , 0, 1)` is the same lookup as the
        // three-argument form, and it used to fall to the general path, which builds both columns.
        if !(3..=6).contains(&args.len()) {
            return None;
        }
        let default_number = |e: Option<&Expr>, want: f64| match e {
            None | Some(Expr::Missing) => true,
            Some(Expr::Number(n)) => *n == want,
            _ => false,
        };
        // match_mode 0 (exact) and search_mode 1 (first to last) are the only modes the scan can
        // answer: last-to-first, a binary search or a wildcard/lower-than mode needs the block.
        if !default_number(args.get(4), 0.0) || !default_number(args.get(5), 1.0) {
            return None;
        }
        let if_not_found = args.get(3).filter(|e| !matches!(e, Expr::Missing));
        // The needle has to be a literal: a pattern (`*`, `?`, `~`) is not a key, and evaluating an
        // argument here would run arguments out of order with the general path.
        let key = match args.first()? {
            Expr::Text(t) if !t.is_empty() && !t.contains(['*', '?', '~']) => Value::Text(Arc::clone(t)),
            Expr::Number(n) => Value::Number(*n),
            _ => return None,
        };
        let (Ev::R(look), Ev::R(ret)) = (self.eval(&args[1]), self.eval(&args[2])) else { return None };
        let ([l], [r]) = (look.as_slice(), ret.as_slice()) else { return None };
        let (l, r) = (*l, *r);
        if l.sheet != r.sheet || l.range.width() != 1 || r.range.width() != 1 || l.range.height() != r.range.height() || l.range.height() <= 1 {
            return None;
        }
        let rows = u64::from(l.range.height());
        let work = 2 * rows + 4;
        if self.work < work {
            return None;
        }
        let found = self.host.first_match_in_column(l.sheet, l.range, &key)?;
        if !self.spend(work) {
            return Some(err(CellError::Calc));
        }
        Some(match found {
            Some(row) => {
                Ev::R(vec![Area { sheet: r.sheet, range: RangeRef::new(CellRef::new(row, r.range.start.col), CellRef::new(row, r.range.end.col)) }])
            }
            // A miss answers `if_not_found` only when it was given, and only now — the general path
            // evaluates that argument lazily too.
            None => match if_not_found {
                Some(e) => self.eval(e),
                None => err(CellError::NA),
            },
        })
    }

    /// `XMATCH(key, column)`: the position of the first stored cell the exact match compares equal,
    /// read as a scan that stops at the hit — the same walk `MATCH(k, col, 0)` takes, with
    /// XMATCH's own argument order and its own defaults spelled out.
    pub(super) fn xmatch_exact_column(&mut self, args: &[Expr]) -> Option<Ev> {
        if !(2..=4).contains(&args.len()) {
            return None;
        }
        let default_number = |e: Option<&Expr>, want: f64| match e {
            None | Some(Expr::Missing) => true,
            Some(Expr::Number(n)) => *n == want,
            _ => false,
        };
        // match_mode 0 is the exact match; 2 is a wildcard and ±1 an approximate one, and
        // search_mode 1 is first-to-last. Anything else needs the block.
        if !default_number(args.get(2), 0.0) || !default_number(args.get(3), 1.0) {
            return None;
        }
        let key = self.exact_key(args.first()?)?;
        let block = args.get(1)?;
        let (sheet, range) = self.one_column(block)?;
        if range.height() <= 1 {
            return None;
        }
        let rows = u64::from(range.height());
        let work = rows + 4;
        if self.work < work {
            return None;
        }
        let found = self.host.first_match_in_column(sheet, range, &key)?;
        if !self.spend(work) {
            return Some(err(CellError::Calc));
        }
        Some(match found {
            Some(row) => Ev::V(Value::number(f64::from(row.saturating_sub(range.start.row).saturating_add(1)))),
            None => err(CellError::NA),
        })
    }

    /// `COUNTIF(column, criterion)`: counts the stored cells a criterion matches, without building
    /// the column into an array to test it.
    ///
    /// The library's COUNTIF takes an `Array`, so the evaluator materialised the whole block first
    /// — a million `Value`s at a million rows — and then tested them. Here the criterion (parsed
    /// once, by the same parser the library uses) is asked of the stored cells as the walk meets
    /// them; blanks are the slots the walk does not meet.
    pub(super) fn countif(&mut self, args: &[Expr]) -> Option<Ev> {
        if args.len() != 2 {
            return None;
        }
        let block = args.first()?;
        let (sheet, range) = self.one_column(block)?;
        if range.height() <= 1 {
            return None;
        }
        let criterion = match cella_functions::Criterion::parse_arg(&self.value(args.get(1)?)) {
            Ok(c) => c,
            Err(e) => return Some(err(e)),
        };
        let rows = u64::from(range.height());
        let work = rows + 4;
        if self.work < work {
            return None;
        }
        let count = self.host.countif_column(sheet, range, &criterion)?;
        if !self.spend(work) {
            return Some(err(CellError::Calc));
        }
        Some(Ev::V(Value::number(count as f64)))
    }

    /// `SUMIF(criteria column, criterion, target column)`: adds the target cells whose criteria cell
    /// matches, both columns in one walk.
    pub(super) fn sumif(&mut self, args: &[Expr]) -> Option<Ev> {
        if !(2..=3).contains(&args.len()) {
            return None;
        }
        let (criteria_sheet, criteria) = self.one_column(args.first()?)?;
        let criterion = match cella_functions::Criterion::parse_arg(&self.value(args.get(1)?)) {
            Ok(c) => c,
            Err(e) => return Some(err(e)),
        };
        // Without a sum range the range is both what is tested and what is added.
        let (target_sheet, target) = match args.get(2) {
            Some(e) => self.one_column(e)?,
            None => (criteria_sheet, criteria),
        };
        if criteria_sheet != target_sheet || criteria.height() <= 1 {
            return None;
        }
        let rows = u64::from(criteria.height());
        let work = 2 * rows + 4;
        if self.work < work {
            return None;
        }
        let sum =
            self.host.sumif_columns(Area { sheet: target_sheet, range: target }, Area { sheet: criteria_sheet, range: criteria }, &criterion)?;
        if !self.spend(work) {
            return Some(err(CellError::Calc));
        }
        Some(Ev::V(sum))
    }

    /// `MATCH(key, column, 0)`: the position of the first stored cell an exact match compares equal,
    /// read as a scan that stops at the hit.
    ///
    /// The general path builds the whole column into an `Array` first — a million `Value`s, 25 ms at
    /// a million rows — and then finds the key on the first row. Excel answers it in microseconds,
    /// and so does this: the block is never materialised, and a miss costs one pass over the stored
    /// cells instead of the array plus the pass.
    ///
    /// `None` when the shape is not this one (a wildcard key, a horizontal block, an approximate
    /// match type, a block holding formulas), so the library answers as it always did.
    pub(super) fn match_exact_column(&mut self, args: &[Expr]) -> Option<Ev> {
        let [key, block, Expr::Number(match_type)] = args else { return None };
        if *match_type != 0.0 {
            return None;
        }
        let key = self.exact_key(key)?;
        let (sheet, range) = self.one_column(block)?;
        if range.height() <= 1 {
            return None;
        }
        let rows = u64::from(range.height());
        let work = rows + 4;
        if self.work < work {
            return None;
        }
        let found = self.host.first_match_in_column(sheet, range, &key)?;
        if !self.spend(work) {
            return Some(err(CellError::Calc));
        }
        Some(match found {
            Some(row) => Ev::V(Value::number(f64::from(row.saturating_sub(range.start.row).saturating_add(1)))),
            None => err(CellError::NA),
        })
    }

    /// `VLOOKUP(key, table, n, FALSE)`: a reference to the found row's `n`th table column, found by
    /// scanning the table's first column and never building the table.
    ///
    /// This is the shape a report writes ten thousand times — `VLOOKUP(A2, Codes!A:B, 2, FALSE)` —
    /// and the general path pays for the table twice over (two million `Value`s for `A:B` at a
    /// million rows, 38 ms) to find a key in the first hundred rows. Returning a *reference* is what
    /// makes it cheap and what `XLOOKUP`'s narrow path already does: the cell is read when its value
    /// is wanted, and a formula there stays a formula.
    pub(super) fn vlookup_exact_column(&mut self, args: &[Expr]) -> Option<Ev> {
        let [key, table, index, exact] = args else { return None };
        // Only an exact match — `FALSE` or `0` — and a literal column, so the answer is one cell.
        if !matches!(exact, Expr::Bool(false) | Expr::Number(0.0)) {
            return None;
        }
        let Expr::Number(index) = index else { return None };
        if *index < 1.0 || index.fract() != 0.0 || *index > f64::from(cella_core::MAX_COLS) {
            return None;
        }
        let col = *index as u32 - 1;
        let key = self.exact_key(key)?;
        let Ev::R(areas) = self.eval(table) else { return None };
        let [t] = areas.as_slice() else { return None };
        let (sheet, t) = (t.sheet, t.range);
        // The key column is the table's first one; a table narrower than the column asked for is
        // the general path's `#REF!` to answer.
        if t.width() <= col || t.height() <= 1 {
            return None;
        }
        let key_column = RangeRef::new(t.start, CellRef::new(t.end.row, t.start.col));
        let rows = u64::from(key_column.height());
        let work = rows + 4;
        if self.work < work {
            return None;
        }
        let found = self.host.first_match_in_column(sheet, key_column, &key)?;
        if !self.spend(work) {
            return Some(err(CellError::Calc));
        }
        Some(match found {
            Some(row) => {
                // The value, not a reference: the library's answer for a table cell is its value,
                // and a blank there reads the way the general path reads it.
                let at = CellRef::new(row, t.start.col.saturating_add(col));
                Ev::V(self.host.cell_value(sheet, at))
            }
            None => err(CellError::NA),
        })
    }

    /// The scalar an exact lookup can compare in one pass: a number, or text that holds no wildcard
    /// (`*`, `?`, `~` — those are patterns the library expands) and is not empty (`MATCH("", A:A, 0)`
    /// matches a *blank* cell, which no stored cell is). `None` for anything else.
    fn exact_key(&mut self, e: &Expr) -> Option<Value> {
        match self.value(e) {
            Value::Number(n) => Some(Value::Number(n)),
            Value::Text(t) if !t.is_empty() && !t.contains(['*', '?', '~']) => Some(Value::Text(t)),
            _ => None,
        }
    }

    /// The one-column block an exact scan needs: its sheet and range, `None` when the expression is
    /// not a plain one-column reference.
    fn one_column(&mut self, e: &Expr) -> Option<(usize, RangeRef)> {
        let Ev::R(areas) = self.eval(e) else { return None };
        let [a] = areas.as_slice() else { return None };
        (a.range.width() == 1).then_some((a.sheet, a.range))
    }

    /// XLOOKUP whose result range is a reference returns a reference, as in Excel, so that it can
    /// end a range (`SUM(XLOOKUP(a,…):XLOOKUP(b,…))`) or feed CELL. Everything else (arrays of
    /// lookup values, array arguments, errors) goes to the library function.
    pub(super) fn xlookup(&mut self, args: &[Expr]) -> Ev {
        // The narrow shape a whole-column lookup takes: a literal text needle, exactly three
        // arguments (no `if_not_found`, both defaults: exact match, first to last), one column
        // each, the same rows. Reading the column into an `Array` first is what the general path
        // below pays — 75 ms to build a million `Value`s and 45 ms to scan them, to compare the
        // first pair (2026-11-23). The stored cells answer it in one pass, case-insensitively, the
        // way an exact lookup compares text.
        if let Some(found) = self.xlookup_scalar_column(args) {
            return found;
        }
        if args.len() == 3
            && let (Ev::R(look), Ev::R(ret)) = (self.eval(&args[1]), self.eval(&args[2]))
            && let ([l], [r]) = (look.as_slice(), ret.as_slice())
            && let Some(spec) = cella_functions::lookup("XMATCH")
        {
            let key = self.value(&args[0]);
            let (l, r) = (*l, *r);
            let vertical = l.range.width() == 1 && l.range.height() > 1;
            let horizontal = l.range.height() == 1 && l.range.width() > 1;
            let same_len = if vertical { r.range.height() == l.range.height() } else { r.range.width() == l.range.width() };
            if !matches!(key, Value::Array(_)) && (vertical || horizontal) && same_len {
                let mut xargs = vec![Arg { value: key, from_ref: false }, Arg { value: self.area_value(&l), from_ref: true }];
                // An empty match_mode before a given search_mode is the default (0), not the end.
                let given = |i: usize| args.get(i).is_some_and(|e| !matches!(e, Expr::Missing));
                let last = if given(5) {
                    5
                } else if given(4) {
                    4
                } else {
                    3
                };
                for i in 4..=last {
                    match args.get(i) {
                        Some(Expr::Missing) | None => xargs.push(Arg { value: Value::Number(0.0), from_ref: false }),
                        Some(e) => {
                            let value = self.value(e);
                            xargs.push(Arg { value, from_ref: false });
                        }
                    }
                }
                let mut ctx = FnCtx { ev: self, criteria: None, lambda: None };
                return match cella_functions::call(spec, &xargs, &mut ctx) {
                    Value::Number(p) if p >= 1.0 => {
                        let k = p as u32 - 1;
                        let range = if vertical {
                            RangeRef::new(
                                CellRef::new(r.range.start.row + k, r.range.start.col),
                                CellRef::new(r.range.start.row + k, r.range.end.col),
                            )
                        } else {
                            RangeRef::new(
                                CellRef::new(r.range.start.row, r.range.start.col + k),
                                CellRef::new(r.range.end.row, r.range.start.col + k),
                            )
                        };
                        Ev::R(vec![Area { sheet: r.sheet, range }])
                    }
                    Value::Error(CellError::NA) if args.get(3).is_some_and(|e| !matches!(e, Expr::Missing)) => self.eval(&args[3]),
                    Value::Error(e) => err(e),
                    _ => err(CellError::Value),
                };
            }
        }
        self.builtin("XLOOKUP", args)
    }
}
