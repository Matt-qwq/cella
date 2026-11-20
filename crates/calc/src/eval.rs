//! The formula evaluator.
//!
//! Expressions evaluate to [`Ev`]: a value, a reference (one or more areas, kept unresolved so
//! `ROW`, `OFFSET`, `INDEX`, `:` and friends can work on references) or a lambda. Cell reads go
//! through [`Host`], which lets the recalculation engine evaluate dirty precedents on demand.

use std::sync::Arc;

use cella_core::{Array, CellError, CellRef, RangeRef, Value};
use cella_formula::{BinOp, Expr, RefKind, Reference, SheetSel, StructItem, StructRef, UnOp};
use cella_functions::Ctx;
use cella_model::Workbook;

mod array;
mod call;
mod format;
mod reference_fns;
mod special;

use array::map_unary;
pub use array::{Acc, Agg, binary_values, zip2};
pub use format::r1c1_to_a1;
pub(crate) use special::MAX_NAME_DEPTH;
pub use special::{NameCache, NameTarget, Precedents, SPECIAL_FUNCTIONS, is_known_function, precedents, precedents_into, resized_sum_range, trimmed};

/// Largest array a reference may expand to.
pub const MAX_CELLS: u64 = 16_000_000;
/// References up to this many cells (one whole row) are always read at their full size.
const FULL_SIZE_CELLS: u64 = 16_384;
/// Bounded references up to this many cells (one column's worth) are read at their full size too, as
/// Excel does, so their shape lines up with the arrays next to them (`D3:AZ400*B3:B400` is 398 by
/// 49 even when the data ends at column F) and their blanks count. References that run to the edge
/// of the sheet (`A:A`, `A2:A1048576`, several whole rows) and larger blocks stop at the sheet's used
/// range, so that formulas over them stay fast.
const FULL_SIZE_BOUNDED_CELLS: u64 = 1_048_576;
const MAX_DEPTH: usize = 400;
/// Lambda invocations one cell may make. A semantically bounded formula in Excel can invoke its
/// lambda at most once per cell of an array (capped here by `MAX_CELLS`), so this covers every
/// legitimate call; hostile work that grows past one dimension (a LAMBDA that recurses or nests
/// `MAP` into `MAP`) then stops at the same bound instead of squaring.
const LAMBDA_FUEL: u64 = MAX_CELLS;
/// Work one cell's formula may do: one unit per expression evaluated, per cell read from a
/// reference, per value handed to a function, per pair of areas intersected and per cell of a block
/// an aggregate folds. Each limit on its own bounds one kind of step, but a compact formula could
/// still repeat bounded steps exponentially often (defined names that each use the one before
/// twice, a computed criterion whose database function reruns it, an aggregate over a large array
/// inside every LAMBDA call). Past this the formula is `#CALC!`. It is far above what a formula
/// that reads every cell of a full-size array a few times needs.
const EVAL_WORK: u64 = 16 * MAX_CELLS;
/// Pending operators a formula's left-nested operator chain keeps in a fixed array on the stack
/// before it spills to a `Vec`. The audit's chained fill chains three; eight covers any formula a
/// person writes, and the spill keeps a 4,096-operand chain working (see `flat_chains_as_long_as_excel_allows`).
const INLINE_CHAIN: usize = 8;
/// LAMBDA calls that one formula may have in progress at once, counted the same way on every build.
/// The next call answers `#NUM!`. A countdown of `MAX_DEPTH` levels has far fewer calls in
/// progress than this, so a formula that recursion alone limits still computes.
pub(crate) const MAX_LAMBDA_CALLS: usize = 140;
/// Native stack, in bytes, that one formula's evaluation may use below its first `eval`. Past it,
/// `eval` answers `#NUM!`. It guards the stack and leaves 128 KiB of a 1 MiB thread (Windows' main
/// stack, where commands and recalculation run) for the frames above the formula. A formula that
/// another cell's formula computes on demand shares the budget of the outermost one
/// ([`Host::stack_top`]).
const STACK_BUDGET: usize = 896 * 1024;

/// The address of a local variable, which shows how far down the native stack this call is.
#[inline(always)]
pub(crate) fn stack_position() -> usize {
    let marker = 0u8;
    std::hint::black_box(&marker) as *const u8 as usize
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Area {
    pub sheet: usize,
    pub range: RangeRef,
}

#[derive(Clone, Debug)]
pub struct Lambda {
    pub params: Vec<String>,
    pub body: Expr,
    pub env: Vec<(String, Ev)>,
}

#[derive(Clone, Debug)]
pub enum Ev {
    V(Value),
    R(Vec<Area>),
    L(Arc<Lambda>),
}

impl From<Value> for Ev {
    fn from(v: Value) -> Self {
        Ev::V(v)
    }
}

#[allow(dead_code)]
fn unused(_: RefKind) {}

fn err(e: CellError) -> Ev {
    Ev::V(Value::Error(e))
}

/// What the evaluator needs from its surroundings.
pub trait Host {
    fn workbook(&self) -> &Workbook;
    /// The current value of a cell (evaluating it first if it is dirty).
    fn cell_value(&mut self, sheet: usize, c: CellRef) -> Value;
    fn now_serial(&self) -> f64;
    fn random(&mut self) -> f64;
    /// Called when a volatile or dynamic-reference function is used.
    fn note_volatile(&mut self) {}
    /// The spill range of the dynamic array anchored at `anchor` (`None`: it doesn't spill).
    fn spill_range(&mut self, sheet: usize, anchor: CellRef) -> Option<RangeRef> {
        self.workbook().sheet(sheet)?.spill_ranges.get(&anchor).copied()
    }
    /// Values of a block, row-major. Hosts can override with a sparse fast path.
    fn range_values(&mut self, sheet: usize, range: RangeRef) -> Vec<Value> {
        range.iter().map(|c| self.cell_value(sheet, c)).collect()
    }
    /// The sheet's used range (`Sheet::used_range`, which walks every stored row). Hosts that
    /// keep the workbook unchanged while evaluating can remember it.
    fn used_range(&mut self, sheet: usize) -> Option<RangeRef> {
        self.workbook().sheet(sheet).and_then(|s| s.used_range())
    }
    /// The definition of the name `name` as used from `sheet`, resolved the way the evaluator
    /// reads names (see [`NameCache`]): sheet-qualified, sheet scope before workbook scope, or a
    /// table's name. Hosts keep a [`NameCache`] and answer from it, so a column of formulas
    /// mentioning one name resolves and parses its definition once instead of once per cell.
    fn name_target(&mut self, sheet: usize, name: &str) -> NameTarget;
    /// The position of a text needle in a block the host has indexed (see
    /// [`cella_functions::Ctx::exact_text_position`]); `None` to scan instead.
    fn exact_text_position(&mut self, _hay: &Arc<Array>, _needle: &str, _reverse: bool) -> Option<Option<usize>> {
        None
    }
    /// The block already built, when the host can share one: a workbook that looks up the same
    /// table from eighty thousand rows reads it once a pass instead of eighty thousand times.
    /// `None` means "read it with `range_values`".
    fn range_array(&mut self, _sheet: usize, _range: RangeRef) -> Option<Arc<Array>> {
        None
    }
    /// Calls `f` on each stored value of a block, row-major (formula cells evaluated first),
    /// until `f` returns false. `false` when this host can't (callers use `range_values`).
    fn visit_range(&mut self, _sheet: usize, _range: RangeRef, _f: &mut dyn FnMut(&Value) -> bool) -> bool {
        false
    }
    /// Folds a block into an aggregate straight from the stored pages: no `Value` an iteration.
    /// `None` when the block holds a cell only the evaluator can price (a formula, an array, or the
    /// spills of one), and the caller falls back to `visit_range`.
    fn visit_aggregate(&mut self, _sheet: usize, _range: RangeRef, _kind: Agg, _acc: &mut Acc) -> Option<Result<(), CellError>> {
        None
    }
    /// Fold one numeric ordering criterion over two constant single-column blocks, or decline
    /// without evaluating cells if either block needs the general range path.
    fn sumifs_numeric_columns(&self, _target: Area, _criteria: Area, _criterion: cella_functions::NumericOrderingCriterion) -> Option<Value> {
        None
    }
    /// While a formula is calculated on demand inside another one: [`stack_position`] where the
    /// outermost one started, which the stack budget is counted from.
    fn stack_top(&self) -> Option<usize> {
        None
    }
    /// Called when a formula's evaluation runs past the stack budget.
    fn stack_exhausted(&mut self) {}
}

pub struct Evaluator<'h> {
    pub host: &'h mut dyn Host,
    pub sheet: usize,
    pub at: CellRef,
    /// The cell the tree being evaluated was written for, when that tree is a **shape's canonical
    /// tree**: its relative references are offsets from here, so one tree serves a million cells.
    /// `None` for an absolute tree — a defined name's definition, `INDIRECT`'s own parse, a tree a
    /// caller already resolved — whose references are coordinates (see `Reference::range`).
    base: Option<CellRef>,
    env: Vec<(String, Ev)>,
    depth: usize,
    /// LAMBDA calls in progress.
    calls: usize,
    /// [`stack_position`] at the outermost `eval`.
    stack_top: usize,
    /// Lambda invocations this cell still has left. Every call spends one charge, so a compact
    /// hostile formula (a LAMBDA that calls itself, nested `MAP`s over big arrays) runs out of work
    /// after the same number of calls that a legitimately wide one could make, instead of an amount
    /// of work that squares with array nesting.
    fuel: u64,
    /// Work this cell still has left (see [`EVAL_WORK`]), shared by every branch of the formula and
    /// by the formulas it computes on the way (names, computed criteria).
    work: u64,
    /// A spend found too little work left. A spend that leaves exactly nothing is fine, so this
    /// can't be told from `work` alone; a database function checks it after its computed criteria
    /// ran, whose failures it can't otherwise see.
    exhausted: bool,
}

struct FnCtx<'a, 'h> {
    ev: &'a mut Evaluator<'h>,
    /// The criteria argument of a database function, when it is a reference.
    criteria: Option<Area>,
    /// The LAMBDA given as the function argument of GROUPBY/PIVOTBY.
    lambda: Option<Arc<Lambda>>,
}

impl Ctx for FnCtx<'_, '_> {
    fn exact_text_position(&mut self, hay: &Arc<Array>, needle: &str, reverse: bool) -> Option<Option<usize>> {
        self.ev.host.exact_text_position(hay, needle, reverse)
    }
    fn date_system(&self) -> cella_core::DateSystem {
        self.ev.host.workbook().date_system
    }
    fn now_serial(&self) -> f64 {
        self.ev.host.now_serial()
    }
    fn random(&mut self) -> f64 {
        self.ev.host.random()
    }
    /// Excel's "formula as criteria": the formula is written for the first record, so its relative
    /// references move down to `record`'s row, and the database's labels name its fields.
    /// Known gap: the shifted references are not dependencies of the calling cell. One that lands
    /// outside both the database and the criteria cell's own precedents (say `=G2<H2` with column H
    /// outside the database) is read here but not tracked, so editing that cell doesn't recalculate
    /// the D-function until something else does.
    fn computed_criterion(&mut self, row: usize, col: usize, db: &Array, record: usize) -> Option<bool> {
        let area = self.criteria?;
        let at = area.range.start.offset(row as i64, col as i64).filter(|c| area.range.contains(*c))?;
        // The criteria formula is written for the first record, so each record reads it from one
        // row lower. A cell holding a shape reads its **canonical tree** with the base moved down
        // — no tree is built per record.
        let formula = self.ev.host.workbook().sheet(area.sheet)?.cell(at)?.formula.clone()?;
        let shape = formula.shape();
        let shifted: Expr;
        let down = record as i64 - 1;
        // The record's row must be on the sheet for a shape's tree to be read there; past it, the
        // tree is materialised and shifted instead. A cell that holds text alone has no shape yet,
        // so `expr_at` parses it (the same tree a shape would resolve to) and it is shifted.
        let record_at = at.offset(down, 0);
        let (tree, base): (&Expr, Option<CellRef>) = match (&shape, record_at) {
            (Some(shape), Some(record_at)) => (shape.tree(), Some(record_at)),
            _ => {
                shifted = cella_formula::adjust::shift_relative(formula.expr_at(at)?, down, 0);
                (&shifted, None)
            }
        };
        let env = (0..db.cols)
            .filter_map(|c| match db.get(0, c) {
                Some(Value::Text(t)) if !t.trim().is_empty() => {
                    Some((t.trim().to_string(), Ev::V(db.get(record, c).cloned().unwrap_or(Value::Empty))))
                }
                _ => None,
            })
            .collect();
        let mut ev = Evaluator {
            host: &mut *self.ev.host,
            sheet: area.sheet,
            at,
            base,
            env,
            depth: self.ev.depth,
            calls: self.ev.calls,
            stack_top: self.ev.stack_top,
            fuel: self.ev.fuel,
            work: self.ev.work,
            exhausted: false,
        };
        let v = matches!(ev.value(tree).scalar().to_bool(), Ok(true));
        // The child's spend counts against this cell's budget.
        self.ev.fuel = ev.fuel;
        self.ev.work = ev.work;
        self.ev.exhausted |= ev.exhausted;
        Some(v)
    }
    fn has_lambda(&self) -> bool {
        self.lambda.is_some()
    }
    /// A LAMBDA must give one value per group; an array result is #CALC!.
    fn call_lambda(&mut self, arg: Value) -> Option<Value> {
        let l = self.lambda.clone()?;
        let [p] = l.params.as_slice() else { return Some(Value::Error(CellError::Value)) };
        let r = self.ev.call_lambda_values(&l, vec![(p.clone(), Ev::V(arg))]);
        Some(match self.ev.deref(r) {
            Value::Array(a) if a.data.len() != 1 => Value::Error(CellError::Calc),
            v => v.scalar(),
        })
    }
}

impl<'h> Evaluator<'h> {
    /// An evaluator for an **absolute** tree: every reference in it is a coordinate (a defined
    /// name's definition, a tree the caller resolved, `INDIRECT`'s own parse).
    pub fn new(host: &'h mut dyn Host, sheet: usize, at: CellRef) -> Self {
        Evaluator {
            host,
            sheet,
            at,
            base: None,
            env: Vec::new(),
            depth: 0,
            calls: 0,
            stack_top: 0,
            fuel: LAMBDA_FUEL,
            work: EVAL_WORK,
            exhausted: false,
        }
    }

    /// The same evaluator for a shape's canonical tree: its relative references are offsets from
    /// `base`, so the one tree of a million-cell fill is evaluated in place, with no per-cell copy
    /// (S4 of `plan/SHAPE-SHARING.md`).
    pub fn at_base(mut self, base: Option<CellRef>) -> Self {
        self.base = base;
        self
    }

    /// The range `r` reads in this tree: a coordinate when the tree is absolute, `base + offset`
    /// when it is a shape's canonical tree. An error is `#REF!` — a relative part that leaves the
    /// sheet, which is what a formula copied past the edge shows (a shape never does at the cell
    /// it was interned for).
    pub fn ref_range(&self, r: &Reference) -> Result<RangeRef, CellError> {
        match self.base {
            Some(base) => r.range_at(base).ok_or(CellError::Ref),
            None => Ok(r.range()),
        }
    }

    /// Evaluates `e`, whose references are coordinates whatever this formula's own tree is: what
    /// `INDIRECT`'s parsed text and a defined name's definition need (neither is a shape).
    pub(crate) fn eval_absolute(&mut self, e: &Expr) -> Ev {
        let outer = self.base.take();
        let r = self.eval(e);
        self.base = outer;
        r
    }

    /// Spends `units` of this cell's work; `false` (and nothing left) when there isn't enough.
    fn spend(&mut self, units: u64) -> bool {
        match self.work.checked_sub(units) {
            Some(w) => {
                self.work = w;
                true
            }
            None => {
                self.work = 0;
                self.exhausted = true;
                false
            }
        }
    }

    /// Charges the values of an array (one unit per value; one for anything else).
    fn spend_on(&mut self, v: &Value) -> bool {
        match v {
            Value::Array(a) => self.spend(a.data.len() as u64),
            _ => self.spend(1),
        }
    }

    /// Evaluates to a value (references are dereferenced; multi-cell → array).
    pub fn value(&mut self, e: &Expr) -> Value {
        // A single-cell reference is by far the most common leaf (`=A1*2`, `=IF(B1>0,…)`): read
        // the cell directly instead of building an area vector and dereferencing it. The result
        // is exactly what `deref(Ev::R(areas))` gives for one single-cell area.
        if let Expr::Ref(r) = e
            && let Ok(range) = self.ref_range(r)
            && range.is_single()
        {
            let sheet = match &r.sheet {
                SheetSel::Current => Some(self.sheet),
                other => match self.resolve_sheet(other) {
                    Ok(sheets) => sheets.first().copied().filter(|_| sheets.len() == 1),
                    Err(_) => None,
                },
            };
            if let Some(sheet) = sheet {
                return self.host.cell_value(sheet, range.start);
            }
        }
        let ev = self.eval(e);
        self.deref(ev)
    }

    fn resolve_sheet(&self, s: &SheetSel) -> Result<Vec<usize>, CellError> {
        let wb = self.host.workbook();
        match s {
            SheetSel::Current => Ok(vec![self.sheet]),
            SheetSel::Named(n) => wb.sheet_index(n).map(|i| vec![i]).ok_or(CellError::Ref),
            SheetSel::Span(a, b) => {
                let (i, j) = (wb.sheet_index(a).ok_or(CellError::Ref)?, wb.sheet_index(b).ok_or(CellError::Ref)?);
                Ok((i.min(j)..=i.max(j)).collect())
            }
        }
    }

    pub fn reference_areas(&self, r: &Reference) -> Result<Vec<Area>, CellError> {
        let range = self.ref_range(r)?;
        Ok(self.resolve_sheet(&r.sheet)?.into_iter().map(|sheet| Area { sheet, range }).collect())
    }

    /// Dereferences: one cell → its value; a block → an array; several areas → `#VALUE!`.
    ///
    /// A block keeps its full shape (blanks included) when it holds at most `FULL_SIZE_CELLS`
    /// cells, or when it holds at most `FULL_SIZE_BOUNDED_CELLS` cells (one full column's worth)
    /// and does not run to the sheet's last row or column. Every other block (larger, or a
    /// bigger one that runs to the sheet's edge, such as `A:A` or `1:1`) is trimmed to the
    /// sheet's used range before its values are read. An array over `MAX_CELLS` is `#NUM!`.
    pub fn deref(&mut self, ev: Ev) -> Value {
        match ev {
            Ev::V(v) => v,
            Ev::L(_) => Value::Error(CellError::Calc),
            Ev::R(areas) => match areas.as_slice() {
                [a] => self.area_value(a),
                _ => Value::Error(CellError::Value),
            },
        }
    }

    pub fn area_value(&mut self, a: &Area) -> Value {
        if a.range.is_single() {
            return self.host.cell_value(a.sheet, a.range.start);
        }
        // A reference keeps its size, so the array has its shape and its blanks count
        // (`=A1:A10`, `SUMPRODUCT(--(A1:A10=""))`). `A:A`, `1:1` and `A2:A1048576` run to the
        // edge of the sheet: they stand for "the rest of the column", not for a block of a chosen
        // size, so they are read through the used range instead (see `trimmed`).
        let range = if trimmed(a.range) { self.trim(a) } else { a.range };
        let (h, w) = (range.height() as usize, range.width() as usize);
        if (h as u64) * (w as u64) > MAX_CELLS {
            return Value::Error(CellError::Num);
        }
        // Reading the block is work (a formula that reads every cell of a full-size array a few
        // times is fine; one that does it once per LAMBDA call is not).
        if !self.spend((h as u64) * (w as u64)) {
            return Value::Error(CellError::Calc);
        }
        if let Some(shared) = self.host.range_array(a.sheet, range)
            && shared.rows == h
            && shared.cols == w
        {
            return Value::Array(shared);
        }
        let data = self.host.range_values(a.sheet, range);
        Array::new(h, w, data).map(Value::from).unwrap_or(Value::Error(CellError::Value))
    }

    /// Trims the bottom/right of an area to the sheet's used range (keeps the top-left so
    /// positions stay right).
    pub fn trim(&mut self, a: &Area) -> RangeRef {
        if self.host.workbook().sheet(a.sheet).is_none() {
            return a.range;
        }
        let used = self.host.used_range(a.sheet);
        let (mut r1, mut c1) = (a.range.end.row, a.range.end.col);
        match used {
            Some(u) => {
                r1 = r1.min(u.end.row.max(a.range.start.row));
                c1 = c1.min(u.end.col.max(a.range.start.col));
            }
            None => {
                r1 = a.range.start.row;
                c1 = a.range.start.col;
            }
        }
        RangeRef::new(a.range.start, CellRef::new(r1, c1))
    }

    pub fn eval(&mut self, e: &Expr) -> Ev {
        // The native stack this evaluation has used below its first `eval`: past the budget, an
        // error, because the alternative is an overflow, which aborts the process and cannot be
        // caught (the user loses the workbook). A formula calculated on demand inside another
        // one is counted from the outermost one's first `eval` (`Host::stack_top`).
        let here = stack_position();
        if self.depth == 0 {
            self.stack_top = self.host.stack_top().unwrap_or(here);
        }
        if self.stack_top.abs_diff(here) > STACK_BUDGET {
            self.host.stack_exhausted();
            return err(CellError::Num);
        }
        if !self.spend(1) {
            return err(CellError::Calc);
        }
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            return err(CellError::Num);
        }
        let r = self.eval_inner(e);
        self.depth -= 1;
        r
    }

    fn eval_inner(&mut self, e: &Expr) -> Ev {
        match e {
            Expr::Number(n) => Ev::V(Value::number(*n)),
            Expr::Text(t) => Ev::V(Value::Text(t.clone())),
            Expr::Bool(b) => Ev::V(Value::Bool(*b)),
            Expr::Error(x) => err(*x),
            Expr::Missing => Ev::V(Value::Empty),
            Expr::Paren(x) => self.eval(x),
            Expr::Array(rows) => {
                let h = rows.len();
                let w = rows.first().map_or(0, Vec::len);
                let mut data = Vec::with_capacity(h * w);
                for row in rows {
                    for el in row {
                        let v = self.value(el);
                        data.push(v);
                    }
                }
                Ev::V(Array::new(h, w, data).map(Value::from).unwrap_or(Value::Error(CellError::Value)))
            }
            Expr::Ref(r) => match self.reference_areas(r) {
                Ok(a) => Ev::R(a),
                Err(x) => err(x),
            },
            Expr::Name(n) => self.name(n),
            Expr::Struct(s) => self.structured(s),
            Expr::Unary(op, x) => self.unary(*op, x),
            Expr::Binary(op, a, b) => self.binary(*op, a, b),
            Expr::Call(name, args) => self.call(name, args),
            Expr::Invoke(callee, args) => {
                let f = self.eval(callee);
                match f {
                    Ev::L(l) => self.apply_lambda(&l, args),
                    Ev::V(Value::Error(x)) => err(x),
                    _ => err(CellError::Value),
                }
            }
        }
    }

    fn name(&mut self, n: &str) -> Ev {
        if let Some((_, v)) = self.env.iter().rev().find(|(k, _)| cella_formula::same_name(k, n)) {
            return v.clone();
        }
        // The host resolves the name (and parses its definition) once per sheet, not once per
        // cell: a column of 100k `=Rate*A1` reads one cached tree 100k times. The answer is
        // owned, so nothing of the cache is borrowed while the definition is evaluated — it may
        // mention another name, which resolves the same way.
        match self.host.name_target(self.sheet, n) {
            NameTarget::Definition(expr, scope) => self.name_value(&expr, scope),
            NameTarget::Relative(expr, scope) => {
                // Relative references in a name are relative to A1: used here, they move by this
                // cell's offset from it. The adjustment builds a tree, so the cached one stays.
                let shifted = cella_formula::adjust::shift_relative_wrapping((*expr).clone(), self.at.row as i64, self.at.col as i64);
                self.name_value(&shifted, scope)
            }
            // A table name alone means its data body.
            NameTarget::Table { area, body } => match body {
                Some(range) => Ev::R(vec![Area { sheet: area.sheet, range }]),
                None => err(CellError::Name),
            },
            NameTarget::Unresolved => err(CellError::Name),
        }
    }

    /// Evaluates a name's definition, whose unqualified references mean the scope sheet.
    fn name_value(&mut self, expr: &Expr, scope: usize) -> Ev {
        let saved = self.sheet;
        self.sheet = scope;
        // A definition is written once and kept from A1: its references are coordinates, not
        // offsets from the cell using the name.
        let r = self.eval_absolute(expr);
        self.sheet = saved;
        r
    }

    fn structured(&mut self, s: &StructRef) -> Ev {
        let wb = self.host.workbook();
        let found = if s.table.is_empty() {
            wb.sheet(self.sheet).and_then(|sh| sh.tables.iter().position(|t| t.range.contains(self.at)).map(|ti| (self.sheet, ti)))
        } else {
            wb.table(&s.table)
        };
        let Some((si, ti)) = found else { return err(CellError::Ref) };
        let Some(t) = wb.sheet(si).and_then(|sh| sh.tables.get(ti)) else { return err(CellError::Ref) };
        let mut r0 = t.range.start.row;
        let mut r1 = t.range.end.row;
        let data = t.data_range();
        let items = &s.specifiers;
        if items.is_empty() || items == &[StructItem::Data] {
            match data {
                Some(d) => {
                    r0 = d.start.row;
                    r1 = d.end.row;
                }
                None => return err(CellError::Ref),
            }
        } else if items.contains(&StructItem::ThisRow) {
            if self.at.row < t.range.start.row || self.at.row > t.range.end.row {
                return err(CellError::Value);
            }
            r0 = self.at.row;
            r1 = self.at.row;
        } else if !items.contains(&StructItem::All) {
            let has = |i: StructItem| items.contains(&i);
            let hdr = t.header_row.then_some(t.range.start.row);
            let tot = t.totals_row.then_some(t.range.end.row);
            let mut lo = u32::MAX;
            let mut hi = 0;
            let mut add = |a: u32, b: u32| {
                lo = lo.min(a);
                hi = hi.max(b);
            };
            if has(StructItem::Headers) {
                match hdr {
                    Some(h) => add(h, h),
                    None => return err(CellError::Ref),
                }
            }
            if has(StructItem::Data)
                && let Some(d) = data
            {
                add(d.start.row, d.end.row);
            }
            if has(StructItem::Totals) {
                match tot {
                    Some(h) => add(h, h),
                    None => return err(CellError::Ref),
                }
            }
            if lo > hi {
                return err(CellError::Ref);
            }
            r0 = lo;
            r1 = hi;
        }
        let (mut c0, mut c1) = (t.range.start.col, t.range.end.col);
        if let Some(a) = &s.col_start {
            let Some(i) = t.column_index(a) else { return err(CellError::Ref) };
            c0 = t.range.start.col + i as u32;
            c1 = c0;
            if let Some(b) = &s.col_end {
                let Some(j) = t.column_index(b) else { return err(CellError::Ref) };
                let cj = t.range.start.col + j as u32;
                c0 = c0.min(cj);
                c1 = c1.max(cj);
            }
        }
        Ev::R(vec![Area { sheet: si, range: RangeRef::new(CellRef::new(r0, c0), CellRef::new(r1, c1)) }])
    }

    fn unary(&mut self, op: UnOp, x: &Expr) -> Ev {
        if let UnOp::Trim(mode) = op {
            return self.trim_ref(mode, x);
        }
        if op == UnOp::Spill {
            return match self.eval(x) {
                Ev::R(areas) => match areas.as_slice() {
                    [a] if a.range.is_single() => match self.host.spill_range(a.sheet, a.range.start) {
                        Some(range) => Ev::R(vec![Area { sheet: a.sheet, range }]),
                        None => err(CellError::Ref),
                    },
                    _ => err(CellError::Ref),
                },
                Ev::V(Value::Error(e)) => err(e),
                _ => err(CellError::Ref),
            };
        }
        if op == UnOp::At {
            let ev = self.eval(x);
            return match ev {
                Ev::R(areas) => match areas.as_slice() {
                    [a] => match self.implicit_intersect(a) {
                        Some(c) => Ev::V(self.host.cell_value(a.sheet, c)),
                        None => err(CellError::Value),
                    },
                    _ => err(CellError::Value),
                },
                Ev::V(v) => Ev::V(v.scalar()),
                l => l,
            };
        }
        let v = self.value(x);
        let sys = self.host.workbook().date_system;
        Ev::V(map_unary(
            &v,
            &|n| match op {
                UnOp::Neg => Value::number(-n),
                UnOp::Plus => Value::number(n),
                UnOp::Percent => Value::number(n / 100.0),
                UnOp::At | UnOp::Spill | UnOp::Trim(_) => Value::number(n),
            },
            op == UnOp::Plus,
            sys,
        ))
    }

    fn implicit_intersect(&self, a: &Area) -> Option<CellRef> {
        let r = a.range;
        if r.is_single() {
            return Some(r.start);
        }
        if r.width() == 1 && r.start.row <= self.at.row && self.at.row <= r.end.row {
            return Some(CellRef::new(self.at.row, r.start.col));
        }
        if r.height() == 1 && r.start.col <= self.at.col && self.at.col <= r.end.col {
            return Some(CellRef::new(r.start.row, self.at.col));
        }
        None
    }

    fn binary(&mut self, op: BinOp, a: &Expr, b: &Expr) -> Ev {
        match op {
            BinOp::Range => {
                let (x, y) = (self.eval(a), self.eval(b));
                let (Ev::R(xa), Ev::R(ya)) = (&x, &y) else {
                    if let Ev::V(Value::Error(e)) = x {
                        return err(e);
                    }
                    if let Ev::V(Value::Error(e)) = y {
                        return err(e);
                    }
                    return err(CellError::Value);
                };
                let all: Vec<&Area> = xa.iter().chain(ya.iter()).collect();
                let Some(first) = all.first() else { return err(CellError::Ref) };
                if all.iter().any(|ar| ar.sheet != first.sheet) {
                    return err(CellError::Value);
                }
                let mut r = first.range;
                for ar in &all {
                    r = r.union(&ar.range);
                }
                Ev::R(vec![Area { sheet: first.sheet, range: r }])
            }
            BinOp::Union => {
                let (x, y) = (self.eval(a), self.eval(b));
                match (x, y) {
                    (Ev::R(mut xa), Ev::R(ya)) => {
                        // A union reused through LET doubles at each step: its areas count.
                        if !self.spend((xa.len() as u64).saturating_add(ya.len() as u64)) {
                            return err(CellError::Calc);
                        }
                        xa.extend(ya);
                        Ev::R(xa)
                    }
                    (Ev::V(Value::Error(e)), _) | (_, Ev::V(Value::Error(e))) => err(e),
                    _ => err(CellError::Value),
                }
            }
            BinOp::Intersect => {
                let (x, y) = (self.eval(a), self.eval(b));
                match (x, y) {
                    (Ev::R(xa), Ev::R(ya)) => {
                        // Every pair is looked at, and each may be kept: charged before the loop,
                        // so unions squared through LET can't build an enormous list.
                        if !self.spend((xa.len() as u64).saturating_mul(ya.len() as u64)) {
                            return err(CellError::Calc);
                        }
                        let mut out = Vec::new();
                        for p in &xa {
                            for q in &ya {
                                if p.sheet == q.sheet
                                    && let Some(i) = p.range.intersection(&q.range)
                                {
                                    out.push(Area { sheet: p.sheet, range: i });
                                }
                            }
                        }
                        if out.is_empty() { err(CellError::Null) } else { Ev::R(out) }
                    }
                    (Ev::V(Value::Error(e)), _) | (_, Ev::V(Value::Error(e))) => err(e),
                    _ => err(CellError::Value),
                }
            }
            _ => {
                // The common shape is one operator over two terms: no chain to fold, so no work
                // list to allocate.
                let chained = matches!(a, Expr::Binary(o, _, _) if !matches!(o, BinOp::Range | BinOp::Union | BinOp::Intersect));
                if !chained {
                    let (x, y) = (self.value(a), self.value(b));
                    return Ev::V(binary_values(op, &x, &y, self.host.workbook().date_system));
                }
                // A long chain such as A1+A2+…+A500 nests to the left: fold it in a loop instead
                // of one level of recursion per operator (past the depth limit it was #CALC!).
                // A formula chains a handful of operators at most, so the pending list lives in a
                // fixed array on the stack; only a chain longer than `INLINE_CHAIN` spills to the
                // heap. The fold order is the same either way: deepest operator first.
                let mut inline: [(BinOp, &Expr); INLINE_CHAIN] = [(op, b); INLINE_CHAIN];
                let mut held = 1;
                let mut spilled: Vec<(BinOp, &Expr)> = Vec::new();
                let mut left = a;
                while let Expr::Binary(o, l, r) = left
                    && !matches!(o, BinOp::Range | BinOp::Union | BinOp::Intersect)
                {
                    match inline.get_mut(held) {
                        Some(slot) => {
                            *slot = (*o, r.as_ref());
                            held += 1;
                        }
                        None => spilled.push((*o, r.as_ref())),
                    }
                    left = l.as_ref();
                }
                let sys = self.host.workbook().date_system;
                let mut acc = self.value(left);
                while let Some((o, r)) = spilled.pop() {
                    let y = self.value(r);
                    acc = binary_values(o, &acc, &y, sys);
                }
                while held > 0 {
                    held -= 1;
                    let Some(&(o, r)) = inline.get(held) else { break };
                    let y = self.value(r);
                    acc = binary_values(o, &acc, &y, sys);
                }
                Ev::V(acc)
            }
        }
    }

    // ------------------------------------------------------------------ calls

    fn arg_ev(&mut self, args: &[Expr], i: usize) -> Option<Ev> {
        args.get(i).map(|e| self.eval(e))
    }
    fn arg_val(&mut self, args: &[Expr], i: usize) -> Option<Value> {
        args.get(i).map(|e| self.value(e))
    }
    fn arg_num(&mut self, args: &[Expr], i: usize, default: f64) -> Result<f64, CellError> {
        match args.get(i) {
            None | Some(Expr::Missing) => Ok(default),
            Some(e) => {
                let v = self.value(e);
                v.scalar().to_number()
            }
        }
    }
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}
