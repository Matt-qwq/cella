//! Implicit-intersection analysis, without evaluating the workbook.
//!
//! Two directions, both file-format policy rather than a second evaluator: unknown results are
//! never calculated.
//!
//! **Writing** ([`needs_array`]): a range is harmless in SUM, but not in SQRT or arithmetic. Keep
//! the result's shape separate from affected operations inside it: SUM(range * range) returns a
//! scalar yet needs array evaluation. Legacy array-aware contexts such as SUMPRODUCT already
//! preserve those operations.
//!
//! **Reading** ([`apply_legacy`]): Excel evaluates a cell formula saved without dynamic-array
//! metadata (no `cm` attribute) the legacy way: a reference that may cover several cells, used
//! where one value is expected, is intersected with the formula's row or column, and a
//! multi-valued result shows its top-left value. Dynamic-array Excel displays such a formula with
//! the implicit intersection operator `@` inserted where that happens (Microsoft support,
//! "Implicit intersection operator: @"). The reader does the same, so the dynamic-array evaluator
//! computes the legacy result. A range is still harmless in SUM: `SUM(A1:A3*B1:B3)` intersects
//! both operands, while legacy array-aware parameters such as SUMPRODUCT's evaluate what is inside
//! as arrays and intersect nothing, except IF's condition and IFERROR's and IFNA's arguments
//! ([`Param::Single`]).
//!
//! The writer's own decision about which formulas a file marks as dynamic-array ones is
//! [`needs_array`]; upstream's rewrite of a legacy formula back to its file text (`export`) is not
//! ported yet.

use std::collections::HashMap;
use std::sync::Arc;

use gridcraft_core::CellRef;
use gridcraft_formula::{BinOp, Expr, RefKind, ShapeTable, SheetSel, StructItem, UnOp};
use gridcraft_model::{Formula, Workbook};

#[derive(Clone, Copy, Default)]
struct Shape {
    array: bool,
    affected: bool,
}

impl Shape {
    fn array() -> Self {
        Self { array: true, affected: false }
    }
}

pub(crate) fn needs_array(wb: &Workbook, sheet: usize, at: CellRef, formula: &Formula) -> bool {
    let Some(expr) = formula.expr_arc() else { return false };
    // The budget covers every node of a formula as long as Excel allows (8,192 characters make
    // at most about 8,200 nodes), so only names that expand a lot fall back to array evaluation.
    let mut scan = Scan { wb, at, remaining: 16_384, locals: Vec::new() };
    let shape = scan.expr(&expr, sheet, false, 0);
    // An explicit `@` is written as `_xlfn.SINGLE`, which only dynamic-array formulas carry.
    let mut at = false;
    expr.walk(&mut |e| at |= matches!(e, Expr::Unary(UnOp::At, _)));
    shape.array || shape.affected || at
}

struct Scan<'a> {
    wb: &'a Workbook,
    at: CellRef,
    remaining: usize,
    locals: Vec<(String, Shape)>,
}

impl Scan<'_> {
    fn expr(&mut self, e: &Expr, sheet: usize, array_context: bool, depth: usize) -> Shape {
        // Names can be cyclic, and a short chain of repeated names can expand exponentially.
        // Preserve array evaluation when analysis cannot safely finish.
        if depth >= 64 || self.remaining == 0 {
            return Shape { array: true, affected: true };
        }
        self.remaining -= 1;
        match e {
            Expr::Ref(r) => Shape { array: !r.range().is_single() || matches!(r.sheet, SheetSel::Span(_, _)), affected: false },
            Expr::Array(rows) => Shape { array: rows.len() > 1 || rows.first().is_some_and(|r| r.len() > 1), affected: false },
            Expr::Name(n) => {
                if let Some((_, shape)) = self.locals.iter().rev().find(|(key, _)| key.eq_ignore_ascii_case(n)) {
                    return *shape;
                }
                let (scope, bare) = match n.rsplit_once('!') {
                    Some((s, name)) => (self.wb.sheet_index(&gridcraft_core::unquote_sheet(s)).unwrap_or(sheet), name),
                    None => (sheet, n.as_str()),
                };
                if let Some(def) = self.wb.name(bare, scope) {
                    return match gridcraft_formula::parse_name(def.formula.trim_start_matches('=')) {
                        Ok(expr) => self.expr(&expr, def.scope.unwrap_or(scope), array_context, depth + 1),
                        Err(_) => Shape::array(),
                    };
                }
                Shape { array: self.wb.table(bare).is_some(), affected: false }
            }
            Expr::Struct(r) => {
                let table = if r.table.is_empty() {
                    self.wb.sheet(sheet).and_then(|s| s.table_at(self.at))
                } else {
                    self.wb.table(&r.table).and_then(|(si, ti)| self.wb.sheet(si).and_then(|s| s.tables.get(ti)))
                };
                let one_row = r.specifiers.iter().any(|s| matches!(s, StructItem::ThisRow | StructItem::Headers | StructItem::Totals));
                let one_col = match (&r.col_start, &r.col_end) {
                    (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                    (Some(_), None) => true,
                    _ => table.is_some_and(|t| t.columns.len() == 1),
                };
                Shape { array: !(one_row && one_col), affected: false }
            }
            Expr::Paren(x) => self.expr(x, sheet, array_context, depth + 1),
            Expr::Unary(op, x) => {
                let child = self.expr(x, sheet, array_context, depth + 1);
                match op {
                    UnOp::At => Shape { array: false, affected: child.affected },
                    // A trim reference is a reference operator, like `:`.
                    UnOp::Spill | UnOp::Trim(_) => Shape { array: true, affected: child.affected },
                    _ => Shape { array: child.array, affected: child.affected || (child.array && !array_context) },
                }
            }
            Expr::Binary(op, a, b) => {
                // A flat chain such as A1+A2+… nests one level per operator on its left: follow
                // it in a loop, so a long chain is neither deep recursion nor past the depth
                // limit (which would write a plain sum as an array formula).
                let mut spine = vec![(*op, b.as_ref())];
                let mut left = a.as_ref();
                while let Expr::Binary(op, a, b) = left {
                    if self.remaining == 0 {
                        return Shape { array: true, affected: true };
                    }
                    self.remaining -= 1;
                    spine.push((*op, b.as_ref()));
                    left = a.as_ref();
                }
                let mut acc = self.expr(left, sheet, array_context, depth + 1);
                while let Some((op, b)) = spine.pop() {
                    let b = self.expr(b, sheet, array_context, depth + 1);
                    let reference = matches!(op, BinOp::Range | BinOp::Union | BinOp::Intersect);
                    let array = reference || acc.array || b.array;
                    acc = Shape { array, affected: acc.affected || b.affected || (!reference && array && !array_context) };
                }
                acc
            }
            Expr::Call(name, args) => self.call(name, args, sheet, array_context, depth + 1),
            Expr::Invoke(_, _) => Shape { array: true, affected: false },
            _ => Shape::default(),
        }
    }

    fn call(&mut self, name: &str, args: &[Expr], sheet: usize, array_context: bool, depth: usize) -> Shape {
        if name == "LET"
            && args.len() >= 3
            && let Some((last, bindings)) = args.split_last()
        {
            let saved = self.locals.len();
            let mut affected = false;
            for pair in bindings.as_chunks::<2>().0 {
                if let [Expr::Name(n), value] = pair {
                    let shape = self.expr(value, sheet, array_context, depth);
                    affected |= shape.affected;
                    self.locals.push((n.clone(), shape));
                }
            }
            let mut result = self.expr(last, sheet, array_context, depth);
            result.affected |= affected;
            self.locals.truncate(saved);
            return result;
        }
        let legacy_array =
            matches!(name, "SUMPRODUCT" | "MMULT" | "MINVERSE" | "MDETERM" | "TRANSPOSE" | "FREQUENCY" | "LINEST" | "LOGEST" | "TREND" | "GROWTH");
        let mut affected = false;
        let mut lifted = false;
        let mut shapes = Vec::with_capacity(args.len().min(256));
        if args.len() > 255 {
            return Shape { array: true, affected: true };
        }
        for (i, arg) in args.iter().enumerate() {
            let context = array_context || legacy_array || (name == "INDEX" && i == 0);
            let shape = self.expr(arg, sheet, context, depth);
            affected |= shape.affected;
            if shape.array && scalar_arg(name, i) {
                lifted = true;
                affected |= !context;
            }
            shapes.push(shape);
        }
        let first_array = shapes.first().is_some_and(|s| s.array);
        let array = match name {
            "INDIRECT" => match args.first() {
                Some(Expr::Text(text)) if !matches!(args.get(1), Some(Expr::Bool(false))) => {
                    gridcraft_formula::parse(text).map(|e| self.expr(&e, sheet, array_context, depth).array).unwrap_or(true)
                }
                _ => true,
            },
            "OFFSET" => {
                // Omitted dimensions retain the source reference's shape; unknown dimensions
                // might become multiple cells on a later calculation.
                if args.len() <= 3 { first_array } else { !args.get(3).is_some_and(one) || args.get(4).map_or(first_array, |e| !one(e)) }
            }
            "INDEX" => {
                let row = args.get(1);
                let col = args.get(2);
                let row_positive = row.is_some_and(|e| positive_index(e, shapes.get(1).copied()));
                let col_positive = col.is_some_and(|e| positive_index(e, shapes.get(2).copied()));
                !row_positive || col.is_some_and(|_| !col_positive) || (col.is_none() && !args.first().is_some_and(single_column))
            }
            "ROW" | "COLUMN" => {
                affected |= first_array && !array_context;
                first_array
            }
            "IF" | "IFERROR" | "IFNA" | "IFS" | "SWITCH" | "CHOOSE" => shapes.iter().any(|s| s.array),
            "XLOOKUP" => {
                lifted
                    || match (args.get(1), args.get(2)) {
                        (Some(Expr::Ref(lookup)), Some(Expr::Ref(result))) => {
                            if lookup.range().width() == 1 {
                                result.range().width() > 1
                            } else {
                                result.range().height() > 1
                            }
                        }
                        _ => true,
                    }
            }
            "FILTER" | "SORT" | "SORTBY" | "UNIQUE" | "SEQUENCE" | "RANDARRAY" | "VSTACK" | "HSTACK" | "TOROW" | "TOCOL" | "WRAPROWS"
            | "WRAPCOLS" | "TAKE" | "DROP" | "CHOOSEROWS" | "CHOOSECOLS" | "EXPAND" | "GROUPBY" | "PIVOTBY" | "TRANSPOSE" | "MMULT" | "MINVERSE"
            | "FREQUENCY" | "LINEST" | "LOGEST" | "TREND" | "GROWTH" | "MAP" | "SCAN" | "BYROW" | "BYCOL" | "MAKEARRAY" | "ANCHORARRAY" => true,
            _ => lifted || self.wb.name(name, sheet).is_some(),
        };
        Shape { array, affected }
    }
}

fn positive(e: &Expr) -> bool {
    matches!(e, Expr::Number(n) if *n >= 1.0)
}

fn positive_index(e: &Expr, shape: Option<Shape>) -> bool {
    // MATCH returns a one-based position or an error, never INDEX's zero "whole row" selector.
    positive(e) || (matches!(e, Expr::Call(name, _) if matches!(name.as_str(), "MATCH" | "XMATCH")) && shape.is_some_and(|s| !s.array))
}

fn one(e: &Expr) -> bool {
    matches!(e, Expr::Number(n) if *n == 1.0)
}

fn single_column(e: &Expr) -> bool {
    matches!(e, Expr::Ref(r) if r.range().width() == 1)
}

/// Legacy reference/array-taking parameters. Other parameters consume a single value.
/// This is file-format policy, not a second evaluator: unknown results are never calculated.
fn scalar_arg(name: &str, i: usize) -> bool {
    match name {
        "SUM" | "AVERAGE" | "AVERAGEA" | "MIN" | "MINA" | "MAX" | "MAXA" | "COUNT" | "COUNTA" | "COUNTBLANK" | "PRODUCT" | "SUMSQ" | "MEDIAN"
        | "MODE" | "MODE.SNGL" | "STDEV" | "STDEV.S" | "STDEV.P" | "STDEVP" | "VAR" | "VAR.S" | "VAR.P" | "VARP" | "AVEDEV" | "DEVSQ" | "GEOMEAN"
        | "HARMEAN" | "KURT" | "SKEW" | "SKEW.P" | "AND" | "OR" | "XOR" | "SUMPRODUCT" | "SUMX2MY2" | "SUMX2PY2" | "SUMXMY2" | "CORREL" | "COVAR"
        | "COVARIANCE.P" | "COVARIANCE.S" | "PEARSON" | "RSQ" | "SLOPE" | "INTERCEPT" | "STEYX" | "ROWS" | "COLUMNS" | "AREAS" | "ROW" | "COLUMN"
        | "TRANSPOSE" | "ISREF" | "MMULT" | "MINVERSE" | "MDETERM" | "FREQUENCY" | "CONCAT" | "VSTACK" | "HSTACK" | "ANCHORARRAY" => false,
        "SUMIF" | "AVERAGEIF" => i == 1,
        "COUNTIF" => i != 0,
        "SUMIFS" | "AVERAGEIFS" | "MINIFS" | "MAXIFS" => i > 0 && i.is_multiple_of(2),
        "COUNTIFS" => i % 2 == 1,
        "INDEX" | "LARGE" | "SMALL" | "PERCENTILE" | "PERCENTILE.INC" | "PERCENTILE.EXC" | "QUARTILE" | "QUARTILE.INC" | "QUARTILE.EXC" | "IRR"
        | "MIRR" | "RANK" | "RANK.EQ" | "RANK.AVG" => i != if name.starts_with("RANK") { 1 } else { 0 },
        "VLOOKUP" | "HLOOKUP" | "MATCH" | "XMATCH" => i != 1,
        "LOOKUP" | "NPV" | "XNPV" | "SUBTOTAL" => i == 0,
        "XLOOKUP" => i != 1 && i != 2,
        "XIRR" => i > 1,
        "OFFSET" => i != 0,
        "AGGREGATE" | "TEXTJOIN" => i < 2,
        "FILTER" | "SORTBY" => i > 1,
        "SORT" | "UNIQUE" | "TAKE" | "DROP" | "CHOOSECOLS" | "CHOOSEROWS" | "TOROW" | "TOCOL" | "WRAPROWS" | "WRAPCOLS" | "EXPAND" => i != 0,
        "LINEST" | "LOGEST" | "TREND" | "GROWTH" => i > 1,
        "FORECAST" | "FORECAST.LINEAR" => i == 0,
        "FORECAST.ETS" | "FORECAST.ETS.CONFINT" => i != 1 && i != 2,
        "FORECAST.ETS.SEASONALITY" | "FORECAST.ETS.STAT" => i > 1,
        _ => true,
    }
}

/// What an expression produces, as far as implicit intersection is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// One value, or a reference to one cell.
    Scalar,
    /// A reference that may cover several cells.
    Ref,
    /// Several values that are not a reference.
    Array,
}

impl Kind {
    fn join(self, other: Kind) -> Kind {
        match (self, other) {
            (Kind::Array, _) | (_, Kind::Array) => Kind::Array,
            (Kind::Ref, _) | (_, Kind::Ref) => Kind::Ref,
            _ => Kind::Scalar,
        }
    }
}

/// How a position consumes its operand in a legacy formula.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ctx {
    /// One value: a multi-cell reference is intersected.
    Value,
    /// A reference or array as such (SUM's arguments); nested operations still take values.
    Ref,
    /// Array evaluation of everything inside (SUMPRODUCT's arguments): nothing is intersected.
    Array,
}

/// A function parameter's legacy evaluation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Param {
    Value,
    /// One value even inside array evaluation: IF's condition and IFERROR's and IFNA's
    /// arguments. Legacy Excel evaluates them as arrays inside SUMPRODUCT only in a
    /// Ctrl+Shift+Enter formula, which this analysis never sees. A name's definition is still
    /// evaluated as an array throughout ([`Legacy::kind_of`]).
    Single,
    Ref,
    Array,
    /// The parameter is evaluated like the call itself (IF's and CHOOSE's results).
    Pass,
    /// A value read from the top-left cell of a reference, never intersected (T: Excel caches
    /// `T(H12:J12)` as T of H12 in a formula outside columns H:J). The function lifts over the
    /// cells, so the legacy result is the top-left one.
    First,
    /// An array parameter of a dynamic-array-era function: whether Excel evaluates a legacy
    /// formula there as an array is not established, so such formulas are left as they were read.
    Uncertain,
}

/// Rewrites the legacy cell formulas at `cells` (model sheet index, positions) to their
/// dynamic-array reading, with `` where Excel intersects. Runs once names and tables are loaded.
///
/// The caller lists the cells whose formula arrived without dynamic-array metadata; a dynamic
/// formula and a Ctrl+Shift+Enter array range are read as written.
pub(crate) fn apply_legacy(wb: &mut Workbook, cells: Vec<(usize, Vec<CellRef>)>) {
    // Defined names do not change during the pass: each is analysed once for all the cells.
    let mut names = NameCache::default();
    // The formulas this pass rewrites intern their shapes here; the cells keep them alive.
    let mut shapes = ShapeTable::default();
    for (si, positions) in cells {
        let Some(sheet) = wb.sheet(si) else { continue };
        let mut changed = Vec::new();
        for at in positions {
            let Some(formula) = sheet.cell(at).and_then(|c| c.formula) else { continue };
            if formula.dynamic {
                continue;
            }
            if let Some(f) = legacy(wb, si, at, &formula, &mut names, &mut shapes) {
                changed.push((at, f));
            }
        }
        if changed.is_empty() {
            continue;
        }
        let Some(sheet) = wb.sheet_mut(si) else { continue };
        for (at, f) in changed {
            if let Some(mut cell) = sheet.cells.get(at) {
                cell.formula = Some(Arc::new(f));
                sheet.cells.set(at, cell);
            }
        }
    }
}

/// The dynamic-array reading of a legacy formula, when it differs.
fn legacy(wb: &Workbook, sheet: usize, at: CellRef, formula: &Formula, names: &mut NameCache, shapes: &mut ShapeTable) -> Option<Formula> {
    if formula.array.is_some() {
        return None;
    }
    let expr = formula.expr_at(at)?;
    let mut legacy = Legacy::new(wb, at, names);
    let out = legacy.formula(expr.clone(), sheet);
    if legacy.certain && legacy.complete {
        if legacy.inserted == 0 {
            return None;
        }
        // Text round trip: the stored expression is exactly what the text parses to.
        if let Some(out) = reads_back(out) {
            return Some(Formula::new_at(&gridcraft_formula::print(&out), at, shapes));
        }
        // The reading's `` take it past the parser's nesting limit: handled as unfinished.
    }
    // An analysis that stopped at its limits, or met semantics it does not know (the array
    // arguments of dynamic-array functions, LAMBDA), leaves the formula as it was read, with only
    // the whole-formula `` when the result may be several values: legacy Excel shows one value,
    // so the formula must not spill. When that text does not parse either, the formula stays as
    // it was read.
    let whole = matches!(&expr, Expr::Unary(UnOp::At, _));
    let multi = legacy.top != Kind::Scalar || (legacy.certain && legacy.complete);
    (multi && !whole).then(|| self::at(expr.clone())).and_then(reads_back).map(|e| Formula::new_at(&gridcraft_formula::print(&e), at, shapes))
}

/// `e` when its text (the printer writes only the parentheses the expression has) reads back as
/// the same expression, so an `` never takes parentheses the text needs with it. An `` of a
/// bare union has no such text, so its formula is left as it was read.
fn reads_back(e: Expr) -> Option<Expr> {
    gridcraft_formula::parse(&gridcraft_formula::print(&e)).is_ok_and(|p| p == e).then_some(e)
}

/// The operand of an `` without the parentheses [`at`] adds.
fn unwrap_at(e: Expr) -> Expr {
    match e {
        Expr::Paren(p) if !bare(&p) => *p,
        other => other,
    }
}

/// ``e`, parenthesized unless `e` prints as one operand. An operand already in parentheses (the
/// file's own) gets a second pair, so [`unwrap_at`] can tell them from the ones added here.
fn at(e: Expr) -> Expr {
    let e = if bare(&e) { e } else { Expr::Paren(Box::new(e)) };
    Expr::Unary(UnOp::At, Box::new(e))
}

/// Prints as one operand of ``: Excel shows a spill reference's `` without parentheses
/// (`=@F4#`, `=@F5#%`).
fn bare(e: &Expr) -> bool {
    atomic(e) || matches!(e, Expr::Unary(UnOp::Spill, r) if matches!(**r, Expr::Ref(_)))
}

fn atomic(e: &Expr) -> bool {
    matches!(e, Expr::Ref(_) | Expr::Name(_) | Expr::Struct(_) | Expr::Call(..) | Expr::Array(_))
}

/// Nesting the analysis follows: the formula parser's own limit. Functions, parentheses and
/// prefix operators count one level each here as there, so any formula that parses is analysed
/// in full unless names, or levels the parser does not count (postfix `%` and `#`), take it
/// deeper; one that is not keeps its legacy reading ([`legacy`]). Operator chains do not count
/// (see [`Legacy::binary`]).
const MAX_DEPTH: usize = 256;

/// What the analysis of one defined name leaves for a later formula that refers to it.
#[derive(Clone)]
struct NameInfo {
    kind: Kind,
    /// The definition reached a one-cell reference written as a range.
    one_cell_range: bool,
    /// The analysis covered only established legacy semantics.
    certain: bool,
    /// Nodes of the per-formula budget the analysis used.
    cost: usize,
    /// Nesting below the reference that the analysis reached.
    depth: usize,
    /// Every name the analysis read, with its result (itself last): a hit records them all, as a
    /// fresh analysis does, so a later reference to one of them in the formula costs nothing.
    closure: Vec<(NameKey, Kind, bool)>,
}

/// A defined name as [`Legacy::names`] keys it: upper-case name, scope, collapsed.
type NameKey = (String, usize, bool);

/// Defined names analysed so far, shared by the formulas of one pass (reading a workbook) so that
/// a name is parsed and analysed once, not once per formula that refers to it. Keyed like
/// [`Legacy::names`]. Only analyses that finished within their limits, did not depend on the
/// formula's own position and did not reuse a name the formula had already read (or meet a cycle)
/// are kept. A hit charges the budget and nesting the analysis used and records every name it
/// read, and is taken only when the formula can afford it, so a formula reads the same with the
/// cache as without it.
#[derive(Default)]
pub(crate) struct NameCache {
    map: HashMap<NameKey, NameInfo>,
    /// How many names were analysed (not found in the cache).
    pub(crate) analyses: usize,
}

/// Names kept at most: the cache is bounded even when a workbook defines a great many.
const MAX_CACHED_NAMES: usize = 65_536;

struct Legacy<'a> {
    wb: &'a Workbook,
    /// Name analyses shared with the other formulas of this pass.
    shared: &'a mut NameCache,
    /// The analysis read a structured reference without a table name, which depends on `at`.
    position_dependent: bool,
    /// The analysis reused a name this formula had already read, or met a cycle: its result
    /// leaves out what that name cost.
    reused: bool,
    /// The names read so far in this formula, in order, with their results.
    read: Vec<(NameKey, Kind, bool)>,
    /// The deepest nesting reached.
    deepest: usize,
    at: CellRef,
    remaining: usize,
    locals: Vec<(String, Kind)>,
    /// Kinds of the names analysed so far, by upper-case name, scope and whether one-cell ranges
    /// counted as collapsed ones, with whether the definition reached a one-cell range written as
    /// a range; `None` while a name's definition is being analysed (a cycle).
    names: HashMap<(String, usize, bool), Option<(Kind, bool)>>,
    /// How many `` were inserted.
    inserted: usize,
    /// The analysis covered only established legacy semantics.
    certain: bool,
    /// The analysis finished within its nesting and size limits.
    complete: bool,
    /// Analysing a formula that has its own `` (see [`Legacy::explicit_at`]): the writer path
    /// that sets it is not ported yet, so reading never does.
    explicit: bool,
    /// A one-cell reference written as a range (`A1:A1`, what an edit leaves of `A1:A3`) counts
    /// as one that may cover several cells, so an `` the range had before the edit is implied.
    collapsed: bool,
    /// The analysis reached a one-cell reference written as a range, which `collapsed` changes.
    one_cell_range: bool,
    /// An explicit `` is neither implied by legacy evaluation nor of a single value.
    foreign: bool,
    /// Analysing an expression whose rewrite is discarded (see [`Legacy::kind_of`]).
    discarded: bool,
    /// Analysing a union that is a function's single-value argument (see [`Legacy::expr`]).
    union_arg: bool,
    /// The kind of the whole formula, before any whole-formula ``.
    top: Kind,
}

impl<'a> Legacy<'a> {
    fn new(wb: &'a Workbook, at: CellRef, shared: &'a mut NameCache) -> Self {
        Legacy {
            wb,
            shared,
            position_dependent: false,
            reused: false,
            read: Vec::new(),
            deepest: 0,
            at,
            remaining: 16384,
            locals: Vec::new(),
            names: HashMap::new(),
            inserted: 0,
            certain: true,
            complete: true,
            explicit: false,
            collapsed: false,
            one_cell_range: false,
            foreign: false,
            discarded: false,
            union_arg: false,
            top: Kind::Scalar,
        }
    }

    /// The kind of an expression analysed only for its kind (a name's definition, INDIRECT's
    /// text), evaluated as legacy Excel evaluates a name: as an array throughout, IF's condition
    /// included. Its rewrite is discarded, so it inserts no `` into the formula.
    fn kind_of(&mut self, e: Expr, sheet: usize, depth: usize) -> Kind {
        let outer = (std::mem::replace(&mut self.discarded, true), std::mem::replace(&mut self.explicit, false), self.inserted);
        let (_, kind) = self.expr(e, sheet, Ctx::Array, depth);
        (self.discarded, self.explicit, self.inserted) = outer;
        kind
    }

    /// The whole cell formula: a multi-valued result shows its top-left value.
    fn formula(&mut self, e: Expr, sheet: usize) -> Expr {
        let e = match e {
            Expr::Unary(UnOp::At, x) if self.explicit => return self.explicit_at(*x, sheet, Ctx::Value, 0, true).0,
            e => e,
        };
        let (e, kind) = self.expr(e, sheet, Ctx::Value, 0);
        self.top = kind;
        if kind == Kind::Array && self.certain {
            self.inserted += 1;
            at(e)
        } else {
            e
        }
    }

    /// An `` already in the formula, at a position the analysis reads in `ctx`: analysed as its
    /// operand alone. The `` is the one legacy evaluation implies when the analysis puts it back
    /// (that insertion is then not counted). An `` of a single value stays; any other `` makes
    /// the formula a dynamic-array one (`foreign`).
    fn explicit_at(&mut self, x: Expr, sheet: usize, ctx: Ctx, depth: usize, top: bool) -> (Expr, Kind) {
        let added = matches!(&x, Expr::Paren(p) if !bare(p));
        let operand = unwrap_at(x);
        let nested = matches!(operand, Expr::Unary(UnOp::At, _));
        let before = self.inserted;
        // A one-cell range the `` applies to, directly or through what a name returns, may be
        // what an edit left of a larger one: when the `` is otherwise of a single value, it is
        // implied if it is with such ranges counted as several cells. The retry's analysis is
        // kept only when it implies the ``, and runs only when the first pass reached such a
        // range (otherwise it cannot differ).
        let retry = (!nested && !self.collapsed && matches!(operand, Expr::Ref(_) | Expr::Name(_))).then(|| operand.clone());
        let seen = std::mem::replace(&mut self.one_cell_range, false);
        let (mut y, mut kind) = self.expr(operand, sheet, ctx, depth + 1);
        let mut implied = !nested && self.inserted > before;
        let shrinkable = self.one_cell_range;
        self.one_cell_range |= seen;
        if let Some(operand) = retry.filter(|_| !implied && kind == Kind::Scalar && shrinkable) {
            let state = (self.remaining, self.certain, self.complete, self.foreign);
            self.collapsed = true;
            let (y2, kind2) = self.expr(operand, sheet, ctx, depth + 1);
            self.collapsed = false;
            if self.inserted > before {
                (y, kind, implied) = (y2, kind2, true);
            } else {
                (self.remaining, self.certain, self.complete, self.foreign) = state;
            }
        }
        match y {
            Expr::Unary(UnOp::At, inner) if implied => {
                self.inserted -= 1;
                (unwrap_at(*inner), Kind::Scalar)
            }
            y if top && kind == Kind::Array => {
                // The whole-formula `` the reader puts back over a multi-valued result.
                self.top = kind;
                (y, Kind::Scalar)
            }
            y => {
                if kind != Kind::Scalar {
                    self.foreign = true;
                }
                let y = if added { Expr::Paren(Box::new(y)) } else { y };
                (Expr::Unary(UnOp::At, Box::new(y)), Kind::Scalar)
            }
        }
    }

    fn expr(&mut self, e: Expr, sheet: usize, ctx: Ctx, depth: usize) -> (Expr, Kind) {
        // Names can be cyclic or nest deeply; their kinds are remembered, so repeated names
        // cost one analysis each.
        self.deepest = self.deepest.max(depth);
        if depth >= MAX_DEPTH || self.remaining == 0 {
            self.complete = false;
            return (e, Kind::Array);
        }
        self.remaining -= 1;
        // The flag reaches the union through parentheses and ``, and not its operands.
        let union_arg = std::mem::take(&mut self.union_arg);
        self.union_arg = union_arg && matches!(e, Expr::Paren(_) | Expr::Unary(UnOp::At, _));
        let (e, kind) = self.node(e, sheet, ctx, depth);
        self.union_arg = false;
        if ctx == Ctx::Value && kind == Kind::Ref {
            if matches!(e, Expr::Binary(BinOp::Union, ..)) && !union_arg {
                // A union, which only stands in parentheses, as an operand: Excel has no legacy
                // text for its `` (it shows `(F25,F26)%`), and the operator gives #VALUE! either
                // way. A function's argument is intersected, as the function would spill it.
                return (e, Kind::Scalar);
            }
            self.inserted += 1;
            (at(e), Kind::Scalar)
        } else {
            (e, kind)
        }
    }

    fn node(&mut self, e: Expr, sheet: usize, ctx: Ctx, depth: usize) -> (Expr, Kind) {
        let inner = |wanted: Ctx| if ctx == Ctx::Array { Ctx::Array } else { wanted };
        match e {
            Expr::Ref(r) => {
                let one_cell_range = r.range().is_single() && !matches!(r.sheet, SheetSel::Span(_, _)) && matches!(r.kind, RefKind::Range(..));
                self.one_cell_range |= one_cell_range;
                let single = r.range().is_single() && !matches!(r.sheet, SheetSel::Span(_, _)) && !(self.collapsed && one_cell_range);
                (Expr::Ref(r), if single { Kind::Scalar } else { Kind::Ref })
            }
            Expr::Array(rows) => {
                let kind = if rows.len() > 1 || rows.first().is_some_and(|r| r.len() > 1) { Kind::Array } else { Kind::Scalar };
                (Expr::Array(rows), kind)
            }
            Expr::Name(n) => {
                let kind = self.name(&n, sheet, depth);
                (Expr::Name(n), kind)
            }
            Expr::Struct(r) => {
                let table = if r.table.is_empty() {
                    self.position_dependent = true;
                    self.wb.sheet(sheet).and_then(|s| s.table_at(self.at))
                } else {
                    self.wb.table(&r.table).and_then(|(si, ti)| self.wb.sheet(si).and_then(|s| s.tables.get(ti)))
                };
                let one_row = r.specifiers.iter().any(|s| matches!(s, StructItem::ThisRow | StructItem::Headers | StructItem::Totals));
                let one_col = match (&r.col_start, &r.col_end) {
                    (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                    (Some(_), None) => true,
                    _ => table.is_some_and(|t| t.columns.len() == 1),
                };
                (Expr::Struct(r), if one_row && one_col { Kind::Scalar } else { Kind::Ref })
            }
            Expr::Paren(x) => {
                let (x, kind) = self.expr(*x, sheet, ctx, depth + 1);
                (Expr::Paren(Box::new(x)), kind)
            }
            Expr::Unary(UnOp::At, x) if self.explicit => self.explicit_at(*x, sheet, ctx, depth, false),
            Expr::Unary(UnOp::At, x) => {
                let (x, _) = self.expr(*x, sheet, inner(Ctx::Ref), depth + 1);
                (Expr::Unary(UnOp::At, Box::new(x)), Kind::Scalar)
            }
            // A trim reference is a reference operator, like `:`.
            Expr::Unary(op @ (UnOp::Spill | UnOp::Trim(_)), x) => {
                let (x, _) = self.expr(*x, sheet, inner(Ctx::Ref), depth + 1);
                (Expr::Unary(op, Box::new(x)), Kind::Ref)
            }
            Expr::Unary(op, x) => {
                let (x, kind) = self.expr(*x, sheet, inner(Ctx::Value), depth + 1);
                (Expr::Unary(op, Box::new(x)), if kind == Kind::Scalar { Kind::Scalar } else { Kind::Array })
            }
            Expr::Binary(..) => self.binary(e, sheet, ctx, depth),
            Expr::Call(name, args) => self.call(name, args, sheet, ctx, depth + 1),
            Expr::Invoke(f, args) => {
                self.certain = false;
                (Expr::Invoke(f, args), Kind::Array)
            }
            other => (other, Kind::Scalar),
        }
    }

    fn name(&mut self, n: &str, sheet: usize, depth: usize) -> Kind {
        if let Some((_, kind)) = self.locals.iter().rev().find(|(key, _)| key.eq_ignore_ascii_case(n)) {
            return *kind;
        }
        let (scope, bare) = match n.rsplit_once('!') {
            Some((s, name)) => (self.wb.sheet_index(&gridcraft_core::unquote_sheet(s)).unwrap_or(sheet), name),
            None => (sheet, n),
        };
        if let Some(def) = self.wb.name(bare, scope) {
            let key = (bare.to_ascii_uppercase(), scope, self.collapsed);
            match self.names.get(&key) {
                Some(Some((kind, one_cell_range))) => {
                    self.reused = true;
                    self.one_cell_range |= *one_cell_range;
                    return *kind;
                }
                Some(None) => {
                    // A name defined in terms of itself has no legacy value to keep.
                    self.certain = false;
                    self.reused = true;
                    return Kind::Array;
                }
                None => {}
            }
            let reached = depth.saturating_add(1);
            // Taken only when the formula can afford it and has read none of the names it read:
            // otherwise the fresh analysis below differs (cut short, or reusing those names).
            let hit = self.shared.map.get(&key).filter(|i| {
                reached + i.depth < MAX_DEPTH && i.cost <= self.remaining && i.closure.iter().all(|(k, _, _)| !self.names.contains_key(k))
            });
            if let Some(info) = hit {
                // Charged and recorded as the analysis was: the formula reads as a fresh one does.
                self.deepest = self.deepest.max(reached + info.depth);
                self.certain &= info.certain;
                self.one_cell_range |= info.one_cell_range;
                self.remaining -= info.cost;
                for (k, kind, one_cell_range) in &info.closure {
                    self.names.insert(k.clone(), Some((*kind, *one_cell_range)));
                }
                self.read.extend(info.closure.iter().cloned());
                return info.kind;
            }
            let Ok(expr) = gridcraft_formula::parse_name(def.formula.trim_start_matches('=')) else { return Kind::Array };
            self.shared.analyses += 1;
            self.names.insert(key.clone(), None);
            // A name's own formula does not see the formula's LET variables.
            let locals = std::mem::take(&mut self.locals);
            let seen = std::mem::replace(&mut self.one_cell_range, false);
            let before = self.remaining;
            let first_read = self.read.len();
            let outer = (
                std::mem::replace(&mut self.certain, true),
                std::mem::replace(&mut self.complete, true),
                std::mem::replace(&mut self.position_dependent, false),
                std::mem::replace(&mut self.reused, false),
                std::mem::replace(&mut self.deepest, 0),
            );
            let kind = self.kind_of(expr, def.scope.unwrap_or(scope), depth + 1);
            let one_cell_range = std::mem::replace(&mut self.one_cell_range, seen);
            self.read.push((key.clone(), kind, one_cell_range));
            let cacheable = self.complete && !self.position_dependent && !self.reused;
            let info = cacheable.then(|| NameInfo {
                kind,
                one_cell_range,
                certain: self.certain,
                cost: before.saturating_sub(self.remaining),
                depth: self.deepest.saturating_sub(depth + 1),
                closure: self.read[first_read..].to_vec(),
            });
            self.certain &= outer.0;
            self.complete &= outer.1;
            self.position_dependent |= outer.2;
            self.reused |= outer.3;
            self.deepest = self.deepest.max(outer.4);
            self.one_cell_range |= one_cell_range;
            self.locals = locals;
            if let Some(info) = info.filter(|_| self.shared.map.len() < MAX_CACHED_NAMES) {
                self.shared.map.insert(key.clone(), info);
            }
            self.names.insert(key, Some((kind, one_cell_range)));
            return kind;
        }
        if self.wb.table(bare).is_some() { Kind::Ref } else { Kind::Scalar }
    }

    fn call(&mut self, name: String, args: Vec<Expr>, sheet: usize, ctx: Ctx, depth: usize) -> (Expr, Kind) {
        if name == "LET" && args.len() >= 3 {
            return self.let_(args, sheet, ctx, depth);
        }
        if args.len() > 255 || matches!(name.as_str(), "LAMBDA" | "MAP" | "REDUCE" | "SCAN" | "BYROW" | "BYCOL" | "MAKEARRAY" | "ISOMITTED") {
            self.certain = false;
            return (Expr::Call(name, args), Kind::Array);
        }
        let mut lifted = false;
        let mut kinds = Vec::with_capacity(args.len());
        let mut out = Vec::with_capacity(args.len());
        for (i, arg) in args.into_iter().enumerate() {
            let param = param(&name, i);
            let arg_ctx = match param {
                Param::Value | Param::Single => Ctx::Value,
                Param::Ref | Param::First => Ctx::Ref,
                Param::Pass => ctx,
                Param::Array => Ctx::Array,
                Param::Uncertain => {
                    self.certain = false;
                    Ctx::Array
                }
            };
            let arg_ctx = if ctx == Ctx::Array && (param != Param::Single || self.discarded) { Ctx::Array } else { arg_ctx };
            // A union argument, under the file's parentheses and any `` the reader put on it.
            let mut operand = &arg;
            while let Expr::Paren(x) | Expr::Unary(UnOp::At, x) = operand {
                operand = x;
            }
            self.union_arg = arg_ctx == Ctx::Value && matches!(operand, Expr::Binary(BinOp::Union, ..));
            let (arg, kind) = self.expr(arg, sheet, arg_ctx, depth);
            // A value parameter given several values evaluates the function once per value.
            lifted |= match param {
                Param::Value | Param::Single => kind == Kind::Array || (kind == Kind::Ref && arg_ctx == Ctx::Array),
                Param::First => kind != Kind::Scalar,
                _ => false,
            };
            kinds.push(kind);
            out.push(arg);
        }
        let first = kinds.first().copied().unwrap_or(Kind::Scalar);
        let kind = match name.as_str() {
            "IF" | "CHOOSE" if !lifted => kinds.iter().skip(1).fold(Kind::Scalar, |k, x| k.join(*x)),
            "ROW" | "COLUMN" => {
                if first == Kind::Scalar {
                    Kind::Scalar
                } else if ctx == Ctx::Array {
                    Kind::Array
                } else {
                    // Outside array evaluation, ROW and COLUMN of a range give its first row or column.
                    self.inserted += 1;
                    return (at(Expr::Call(name, out)), Kind::Scalar);
                }
            }
            "INDIRECT" => match out.first() {
                Some(Expr::Text(text)) if !matches!(out.get(1), Some(Expr::Bool(false))) => match gridcraft_formula::parse(text) {
                    Ok(e) => {
                        let kind = self.kind_of(e, sheet, depth);
                        if kind == Kind::Scalar { Kind::Scalar } else { Kind::Ref }
                    }
                    Err(_) => Kind::Ref,
                },
                _ => Kind::Ref,
            },
            "OFFSET" => offset_kind(&out, first, lifted),
            "INDEX" => index_kind(&out, &kinds, first, lifted),
            "XLOOKUP" if !lifted => match (out.get(1), out.get(2)) {
                (Some(Expr::Ref(lookup)), Some(Expr::Ref(result))) => {
                    let multi = if lookup.range().width() == 1 { result.range().width() > 1 } else { result.range().height() > 1 };
                    if multi { Kind::Ref } else { Kind::Scalar }
                }
                _ => Kind::Ref,
            },
            "ANCHORARRAY" | "TRIMRANGE" => Kind::Ref,
            "FILTER" | "SORT" | "SORTBY" | "UNIQUE" | "SEQUENCE" | "RANDARRAY" | "VSTACK" | "HSTACK" | "TOROW" | "TOCOL" | "WRAPROWS"
            | "WRAPCOLS" | "TAKE" | "DROP" | "CHOOSEROWS" | "CHOOSECOLS" | "EXPAND" | "GROUPBY" | "PIVOTBY" | "TRANSPOSE" | "MMULT" | "MINVERSE"
            | "MUNIT" | "FREQUENCY" | "LINEST" | "LOGEST" | "TREND" | "GROWTH" | "MODE.MULT" | "TEXTSPLIT" | "FILTERXML" => Kind::Array,
            // Every match (mode 1) or the capture groups (mode 2); the first match by default.
            "REGEXEXTRACT" => match out.get(2) {
                None | Some(Expr::Missing) => Kind::Scalar,
                Some(Expr::Number(n)) if *n == 0.0 => Kind::Scalar,
                _ => Kind::Array,
            },
            // Google Sheets' export placeholder fails with #NAME? so an outer IFERROR keeps the
            // cached scalar.
            "__XLUDF.DUMMYFUNCTION" => Kind::Scalar,
            _ if lifted => Kind::Array,
            _ => Kind::Scalar,
        };
        (Expr::Call(name, out), kind)
    }
}

fn offset_kind(out: &[Expr], first: Kind, lifted: bool) -> Kind {
    // Omitted dimensions keep the source reference's shape; unknown dimensions might
    // become several cells on a later calculation.
    let first_multi = first != Kind::Scalar;
    let multi = if out.len() <= 3 { first_multi } else { !out.get(3).is_some_and(one) || out.get(4).map_or(first_multi, |e| !one(e)) };
    if lifted {
        Kind::Array
    } else if multi {
        Kind::Ref
    } else {
        Kind::Scalar
    }
}

fn index_kind(out: &[Expr], kinds: &[Kind], first: Kind, lifted: bool) -> Kind {
    let row = out.get(1);
    let col = out.get(2);
    let row_positive = row.is_some_and(|e| positive_index_kind(e, kinds.get(1).copied()));
    let col_positive = col.is_some_and(|e| positive_index_kind(e, kinds.get(2).copied()));
    let multi = !row_positive || col.is_some_and(|_| !col_positive) || (col.is_none() && !out.first().is_some_and(one_line));
    if multi {
        if first == Kind::Array { Kind::Array } else { Kind::Ref }
    } else if lifted {
        Kind::Array
    } else {
        Kind::Scalar
    }
}

impl Legacy<'_> {
    /// A binary operation. `A1+A2+…+A70` parses as a left-nested chain as deep as it is long, so
    /// the chain's left operands are walked iteratively, at the same depth; each operation still
    /// costs one node of the budget.
    fn binary(&mut self, e: Expr, sheet: usize, ctx: Ctx, depth: usize) -> (Expr, Kind) {
        let operand = |op: BinOp, node: Ctx| {
            if node == Ctx::Array {
                Ctx::Array
            } else if matches!(op, BinOp::Range | BinOp::Union | BinOp::Intersect) {
                Ctx::Ref
            } else {
                Ctx::Value
            }
        };
        // (operator, right operand, the operation's context), outermost first.
        let mut spine = Vec::new();
        let mut cur = e;
        let mut cur_ctx = ctx;
        while let Expr::Binary(op, a, b) = cur {
            if !spine.is_empty() {
                // The caller's `expr` counted the outermost operation.
                if self.remaining == 0 {
                    self.complete = false;
                } else {
                    self.remaining -= 1;
                }
            }
            spine.push((op, *b, cur_ctx));
            cur_ctx = operand(op, cur_ctx);
            cur = *a;
        }
        let (mut acc, mut kind) = self.expr(cur, sheet, cur_ctx, depth);
        for (i, (op, b, node_ctx)) in spine.into_iter().enumerate().rev() {
            let (b, kb) = self.expr(b, sheet, operand(op, node_ctx), depth);
            let node = Expr::Binary(op, Box::new(acc), Box::new(b));
            let k = if matches!(op, BinOp::Range | BinOp::Union | BinOp::Intersect) {
                Kind::Ref
            } else if kind == Kind::Scalar && kb == Kind::Scalar {
                Kind::Scalar
            } else {
                Kind::Array
            };
            // An inner operation is an operand: `expr` would intersect it there.
            (acc, kind) = if i > 0 && node_ctx == Ctx::Value && k == Kind::Ref {
                self.inserted += 1;
                (at(node), Kind::Scalar)
            } else {
                (node, k)
            };
        }
        (acc, kind)
    }

    fn let_(&mut self, args: Vec<Expr>, sheet: usize, ctx: Ctx, depth: usize) -> (Expr, Kind) {
        let saved = self.locals.len();
        let count = args.len();
        let mut out = Vec::with_capacity(count);
        let mut pending: Option<String> = None;
        let mut kind = Kind::Scalar;
        for (i, arg) in args.into_iter().enumerate() {
            if i + 1 == count {
                let (body, k) = self.expr(arg, sheet, ctx, depth);
                out.push(body);
                kind = k;
            } else if i % 2 == 0 {
                pending = match &arg {
                    Expr::Name(n) => Some(n.clone()),
                    _ => None,
                };
                out.push(arg);
            } else {
                let value_ctx = if ctx == Ctx::Array { Ctx::Array } else { Ctx::Ref };
                let (value, k) = self.expr(arg, sheet, value_ctx, depth);
                if let Some(n) = pending.take() {
                    self.locals.push((n, k));
                }
                out.push(value);
            }
        }
        self.locals.truncate(saved);
        (Expr::Call("LET".into(), out), kind)
    }
}

fn positive_index_kind(e: &Expr, kind: Option<Kind>) -> bool {
    // MATCH returns a one-based position or an error, never INDEX's zero whole-row selector.
    positive(e) || (matches!(e, Expr::Call(name, _) if matches!(name.as_str(), "MATCH" | "XMATCH")) && kind.is_some_and(|k| k != Kind::Ref))
}

/// A one-column or one-row reference: INDEX with a single index picks one cell of it.
fn one_line(e: &Expr) -> bool {
    matches!(e, Expr::Ref(r) if r.range().width() == 1 || r.range().height() == 1)
}

/// A parameter's legacy evaluation, by the function name and position. The function library's
/// own lifting metadata is not consulted here: names outside this table take a value, as if every
/// function were built in and took single values (upstream's default without a library).
fn param(name: &str, i: usize) -> Param {
    match name {
        "IF" if i == 0 => Param::Single,
        "IFERROR" | "IFNA" => Param::Single,
        "CHOOSE" if i == 0 => Param::Value,
        "IF" | "CHOOSE" => Param::Pass,
        "SUMPRODUCT" | "MMULT" | "MINVERSE" | "MDETERM" | "TRANSPOSE" | "FREQUENCY" | "LINEST" | "LOGEST" | "TREND" | "GROWTH" => Param::Array,
        "INDEX" if i == 0 => Param::Array,
        // LOOKUP's vectors and AGGREGATE's array form evaluate their arguments as arrays.
        "LOOKUP" if i > 0 => Param::Array,
        "AGGREGATE" if i == 2 => Param::Array,
        "ROW" | "COLUMN" | "ROWS" | "COLUMNS" | "AREAS" | "ISREF" | "ISFORMULA" | "FORMULATEXT" | "SHEET" | "SHEETS" | "TRIMRANGE"
        | "ANCHORARRAY" => Param::Ref,
        "CELL" | "GETPIVOTDATA" if i == 1 => Param::Ref,
        "T" => Param::First,
        "N" if i == 0 => Param::Value,
        "FILTER" | "SORT" | "SORTBY" | "UNIQUE" | "VSTACK" | "HSTACK" | "TOROW" | "TOCOL" | "WRAPROWS" | "WRAPCOLS" | "TAKE" | "DROP"
        | "CHOOSEROWS" | "CHOOSECOLS" | "EXPAND" | "GROUPBY" | "PIVOTBY"
            if !scalar_arg(name, i) =>
        {
            Param::Uncertain
        }
        n if scalar_arg(n, i) => Param::Value,
        _ => Param::Ref,
    }
}
