//! The functions the evaluator implements itself, the names they answer to, and the static
//! references a formula's tree yields.

use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_formula::{BinOp, Expr, Reference, SheetSel, StructItem};
use gridcraft_model::Workbook;

use super::{Area, Ev, FULL_SIZE_BOUNDED_CELLS, FULL_SIZE_CELLS};

/// Most expression nodes precedents visits before it gives up and marks the formula dynamic.
const MAX_PRECEDENT_NODES: usize = 1_000_000;

/// SUMIF and AVERAGEIF with a sum range: Excel sizes the sum range like the criteria range.
pub(super) fn resizes_sum_range(name: &str, args: usize) -> bool {
    args == 3 && matches!(name, "SUMIF" | "AVERAGEIF")
}

/// Height and width of a single-area reference or an array argument.
pub(super) fn ev_shape(ev: &Ev) -> Option<(u32, u32)> {
    match ev {
        Ev::R(areas) => match areas.as_slice() {
            [a] => Some((a.range.height(), a.range.width())),
            _ => None,
        },
        Ev::V(Value::Array(a)) => Some((u32::try_from(a.rows).unwrap_or(u32::MAX), u32::try_from(a.cols).unwrap_or(u32::MAX))),
        Ev::V(_) => Some((1, 1)),
        Ev::L(_) => None,
    }
}

/// The range SUMIF/AVERAGEIF add up: from the sum range's top-left cell, as many rows and columns
/// as the criteria range (`height` × `width`), cut off at the sheet's edge.
pub fn resized_sum_range(sum: RangeRef, height: u32, width: u32) -> RangeRef {
    let row = sum.start.row.saturating_add(height.saturating_sub(1)).min(MAX_ROWS - 1);
    let col = sum.start.col.saturating_add(width.saturating_sub(1)).min(MAX_COLS - 1);
    RangeRef::new(sum.start, CellRef::new(row, col))
}

/// Whether a reference is read through the sheet's used range instead of in full: a block bigger
/// than [`FULL_SIZE_CELLS`], or one bigger than [`FULL_SIZE_BOUNDED_CELLS`] that runs to the edge of
/// the sheet (`A:A`, `1:1`). Its values then end where the sheet's content does, so its result can
/// change when the used range moves even though no cell the formula names changed.
pub fn trimmed(range: RangeRef) -> bool {
    let cells = u64::from(range.height()) * u64::from(range.width());
    let to_edge = range.end.row == MAX_ROWS - 1 || range.end.col == MAX_COLS - 1;
    !(cells <= FULL_SIZE_CELLS || (!to_edge && cells <= FULL_SIZE_BOUNDED_CELLS))
}

/// Most defined-name definitions [`precedents`] follows one into another (`Rate` defined as
/// `Step`, `Step` defined as `Sheet1!$B$1`) before it treats the name as dynamic: a file can
/// define names in a ring, and following them has to end.
pub(crate) const MAX_NAME_DEPTH: usize = 8;

/// Names one sheet's cache keeps before the least recently used is dropped (a formula can
/// mention more names than any real sheet, so the cache has to be bounded).
const MAX_CACHED_NAMES: usize = 256;

/// What a name in a formula reads, resolved the way the evaluator resolves it (`Evaluator::name`).
#[derive(Clone, Debug)]
pub enum NameTarget {
    /// The name's definition, parsed, and the sheet its unqualified references mean (its own
    /// scope sheet, or the sheet using it for a workbook-level name). No reference in it is
    /// relative, so the same tree serves every cell that uses the name.
    Definition(std::sync::Arc<Expr>, usize),
    /// The same, for a definition that holds a relative reference: it is relative to A1, so it
    /// moves by the cell that uses the name and each of those needs the tree adjusted.
    Relative(std::sync::Arc<Expr>, usize),
    /// A name that is a table's name: the table's whole range (the dependency) and its data body,
    /// which is what the name stands for as a value (`None`: the table has no body).
    Table { area: Area, body: Option<RangeRef> },
    /// An unknown name or a definition that doesn't parse.
    Unresolved,
}

/// One resolved name.
#[derive(Clone, Debug)]
struct CachedName {
    /// The name as written in the formula.
    name: String,
    /// The scope of the definition the answer came from, and its text: a cached answer is only
    /// used for the same definition, so a name edited or re-scoped (a sheet-level name shadowing
    /// a workbook-level one) is resolved again.
    scope: Option<usize>,
    formula: String,
    target: NameTarget,
}

/// Resolved definitions of the names formulas use, keyed by the sheet the formula is on.
///
/// Walking a graph resolves `Rate` once for every formula that mentions it — a column of 100k
/// `=Rate*A1` formulas mentions one name 100k times — and parsing a definition each time costs
/// far more than the rest of the walk. Answers are keyed by (sheet, name) because that is what
/// the resolution depends on besides the definitions themselves, and each is checked against the
/// definition found now, so a name whose scope or text changed is resolved again. A table's range
/// is not cached (tables are few, and a table only moves through a structural edit).
#[derive(Clone, Debug, Default)]
pub struct NameCache {
    by_sheet: std::collections::HashMap<usize, Vec<CachedName>>,
}

impl NameCache {
    /// Resolves the name `n` as used from `sheet`: `Sheet1!Name` is looked up on that sheet, a
    /// bare name on `sheet` (sheet scope first, then workbook scope), and a name that is a
    /// table's name stands for that table.
    pub(crate) fn target(&mut self, wb: &Workbook, sheet: usize, n: &str) -> NameTarget {
        let (scope_sheet, bare) = match n.split_once('!') {
            Some((s, b)) => match wb.sheet_index(&gridcraft_core::unquote_sheet(s)) {
                Some(i) => (i, b),
                None => return NameTarget::Unresolved,
            },
            None => (sheet, n),
        };
        let Some(def) = wb.name(bare, scope_sheet) else {
            // A table name alone means its data body. The table's whole range is the dependency:
            // it is a superset of the body (and of a header or totals row read through it), and
            // a table only moves or resizes through a structural edit, which rebuilds the graph.
            return match wb.table(bare).and_then(|(si, ti)| wb.sheet(si).and_then(|s| s.tables.get(ti)).map(|t| (si, t))) {
                Some((si, t)) => NameTarget::Table { area: Area { sheet: si, range: t.range }, body: t.data_range() },
                None => NameTarget::Unresolved,
            };
        };
        let list = self.by_sheet.entry(sheet).or_default();
        if let Some(i) = list.iter().position(|c| c.name.eq_ignore_ascii_case(n) && c.scope == def.scope && c.formula == def.formula) {
            // Most recently used first: the same name is what the next formula reads.
            let hit = list.remove(i);
            let target = hit.target.clone();
            list.insert(0, hit);
            return target;
        }
        let target = resolve_definition(&def.formula, def.scope.unwrap_or(sheet));
        let scope = def.scope;
        let formula = def.formula.clone();
        if list.len() >= MAX_CACHED_NAMES {
            list.truncate(MAX_CACHED_NAMES - 1);
        }
        list.insert(0, CachedName { name: n.to_string(), scope, formula, target: target.clone() });
        target
    }

    /// Heap bytes of the cache: the answers, the strings they keep and the trees a definition was
    /// parsed into. A tree that is still the only reference this cache has is walked node by node
    /// (a definition tree is a handful of nodes); one a formula also holds is charged its pointer.
    pub(crate) fn memory(&self) -> usize {
        use std::mem::size_of;
        let mut bytes = gridcraft_model::hash_index_bytes(self.by_sheet.capacity(), size_of::<(usize, Vec<CachedName>)>());
        for list in self.by_sheet.values() {
            bytes = bytes.saturating_add(list.capacity().saturating_mul(size_of::<CachedName>()));
            for c in list {
                bytes = bytes.saturating_add(c.name.len()).saturating_add(c.formula.len());
                for tree in [&c.target] {
                    let tree = match tree {
                        NameTarget::Definition(tree, _) | NameTarget::Relative(tree, _) => tree,
                        NameTarget::Table { .. } | NameTarget::Unresolved => continue,
                    };
                    if std::sync::Arc::strong_count(tree) > 1 {
                        bytes = bytes.saturating_add(size_of::<std::sync::Arc<Expr>>());
                        continue;
                    }
                    tree.walk(&mut |_| bytes = bytes.saturating_add(size_of::<Expr>()));
                }
            }
        }
        bytes
    }
}

/// The definition of `formula` (a name's `refersTo` text) as a target, its unqualified
/// references meaning `scope`.
fn resolve_definition(formula: &str, scope: usize) -> NameTarget {
    let Ok(expr) = gridcraft_formula::parse_name(formula) else { return NameTarget::Unresolved };
    // A relative reference in a name is relative to A1, so it moves by the offset of the cell
    // that uses the name: usable per cell, but never one fixed area.
    if gridcraft_formula::adjust::has_relative_refs(&expr) {
        return NameTarget::Relative(std::sync::Arc::new(expr), scope);
    }
    NameTarget::Definition(std::sync::Arc::new(expr), scope)
}

/// Whether a definition is a LAMBDA (a name called like a function): what it reads is only
/// known when it is called, so a formula using it cannot be tied to a fixed area.
fn is_lambda_definition(e: &Expr) -> bool {
    matches!(e, Expr::Call(n, _) if n.eq_ignore_ascii_case("LAMBDA"))
}

/// What a formula reads besides its areas: the reasons it has to be evaluated again although no
/// cell it names changed.
#[derive(Clone, Copy, Debug, Default)]
pub struct Precedents {
    /// It uses INDIRECT/OFFSET, a volatile function, or a name or table whose target can move.
    pub dynamic: bool,
    /// One of its references is read through the used range (see [`trimmed`]).
    pub trim: bool,
    /// It calls a function whose answer changes on its own (`NOW`, `RAND`, `TODAY`… see
    /// [`FnSpec::volatile`](gridcraft_functions::FnSpec)).
    pub volatile: bool,
    /// It reads `SUBTOTAL` or `AGGREGATE`, whose value leaves out the rows a filter, a collapsed
    /// group or a row height of zero hides: showing or hiding rows changes it with no cell it names
    /// changing.
    pub hidden_rows: bool,
}

/// Collects the static references of an expression with their sheets resolved, and the
/// [`Precedents`] flags of what it reads.
///
/// A name that stands for cells is a precedent like a written reference: `Rate` defined as
/// `Sheet1!$B$1` makes `=Rate*A1` read A1 and B1, so an edit elsewhere does not recalculate it
/// and an edit of B1 does. `names` keeps the resolved definitions between calls.
pub fn precedents(names: &mut NameCache, wb: &Workbook, sheet: usize, e: &Expr) -> (Vec<Area>, Precedents) {
    let mut out = Vec::new();
    let reads = precedents_into(names, wb, sheet, e, None, &mut out);
    (out, reads)
}

/// The range `r` reads in a tree whose relative parts are offsets from `base`, or coordinates when
/// `base` is `None` (an absolute tree: a defined name's definition, a resolved tree).
fn resolved(r: &Reference, base: Option<CellRef>) -> Option<RangeRef> {
    match base {
        Some(base) => r.range_at(base),
        None => Some(r.range()),
    }
}

/// [`precedents`] appending to a buffer the caller reuses and emptying it first.
///
/// Building the graph asks a million formulas for their precedents; one `Vec` per formula is a
/// million allocations that say nothing the graph does not already know.
/// `base` is the cell the tree's **relative** parts are offsets from when it is a shape's
/// canonical tree, and `None` when it is absolute (what `Reference::range` spells). A shape is read
/// where it is, so building the graph of a million-cell fill walks one tree and adds a cell offset
/// per reference instead of resolving a tree per formula (S4 of `plan/SHAPE-SHARING.md`).
pub fn precedents_into(names: &mut NameCache, wb: &Workbook, sheet: usize, e: &Expr, base: Option<CellRef>, out: &mut Vec<Area>) -> Precedents {
    out.clear();
    // One budget for the whole walk, shared by the definitions it follows: past
    // `MAX_PRECEDENT_NODES` the result is conservative (the formula is marked `dynamic`, so it
    // recalculates every time) rather than a walk that never ends.
    let mut left = MAX_PRECEDENT_NODES;
    precedents_at(names, wb, sheet, e, base, 0, &mut left, out)
}

/// [`precedents`] at name depth `depth`: a name whose definition is static contributes that
/// definition's references (`Rate` defined as `Sheet1!$B$1` reads one cell), and only a
/// definition that can move stays dynamic.
fn precedents_at(
    names: &mut NameCache,
    wb: &Workbook,
    sheet: usize,
    e: &Expr,
    base: Option<CellRef>,
    depth: usize,
    left: &mut usize,
    out: &mut Vec<Area>,
) -> Precedents {
    let mut dynamic = false;
    let mut trim = false;
    let mut hidden_rows = false;
    // Whether the formula calls a function that answers differently every time it is evaluated.
    // Recorded here because this walk visits every node anyway — the graph keeps the flag, so a
    // workbook's formulas never have to be walked again to find out (see `recalc_on_open`).
    let mut volatile = false;
    let push = |out: &mut Vec<Area>, r: &Reference, range: RangeRef| match &r.sheet {
        SheetSel::Current => out.push(Area { sheet, range }),
        SheetSel::Named(n) => {
            if let Some(i) = wb.sheet_index(n) {
                out.push(Area { sheet: i, range });
            }
        }
        SheetSel::Span(a, b) => {
            if let (Some(i), Some(j)) = (wb.sheet_index(a), wb.sheet_index(b)) {
                for s in i.min(j)..=i.max(j) {
                    out.push(Area { sheet: s, range });
                }
            }
        }
    };
    crate::walk::walk(e, &mut |x| {
        // One node of the budget per node visited. Past it the answer is conservative: the formula
        // is marked dynamic, so it recalculates every time rather than the walk never ending.
        if *left == 0 {
            dynamic = true;
            return;
        }
        *left -= 1;
        // The registry entry of a call's function, looked up once for the two questions below ask
        // of it: is it volatile, and is it a function at all?
        let call = match x {
            Expr::Call(n, _) => Some((n, gridcraft_functions::lookup(n))),
            _ => None,
        };
        // A volatile function's cached value is out of date by definition, so a formula calling one
        // is recalculated on open even though the file carries a value for it. A measure reads the
        // data model, which is refreshed outside the cell graph (a query reload, a relationship or
        // measure edit), so its cached value can be out of date the moment the file opens.
        if let Some((n, spec)) = call
            && (spec.is_some_and(|f| f.volatile) || n.eq_ignore_ascii_case("MEASURE"))
        {
            volatile = true;
        }
        match x {
            Expr::Ref(r) => match resolved(r, base) {
                Some(range) => {
                    trim |= trimmed(range);
                    push(&mut *out, r, range);
                }
                // A relative part off the sheet: the cell shows `#REF!` there, so it reads
                // nothing — and it is recalculated every time rather than once on a guess.
                None => dynamic = true,
            },
            // The cells SUMIF/AVERAGEIF really read from the sum range (see `resized_sum_range`); when
            // the criteria range's size isn't known statically, recalculate the formula every time.
            Expr::Call(n, args) if resizes_sum_range(n, args.len()) => match (args.first(), args.get(2)) {
                (Some(Expr::Ref(c)), Some(Expr::Ref(s))) => match (resolved(c, base), resolved(s, base)) {
                    (Some(crit), Some(sum)) => {
                        let resized = resized_sum_range(sum, crit.height(), crit.width());
                        trim |= trimmed(resized);
                        push(&mut *out, s, resized);
                    }
                    _ => dynamic = true,
                },
                (_, Some(Expr::Number(_) | Expr::Text(_) | Expr::Bool(_) | Expr::Error(_) | Expr::Array(_) | Expr::Missing)) => {}
                _ => dynamic = true,
            },
            // SUBTOTAL and AGGREGATE leave out the rows a filter hides (and, from 101 up, the ones
            // hidden by hand or by a collapsed group): the sheet's visibility is one of their
            // inputs without being a cell they name.
            Expr::Call(n, _) if n.eq_ignore_ascii_case("SUBTOTAL") || n.eq_ignore_ascii_case("AGGREGATE") => hidden_rows = true,
            Expr::Call(n, args) if n.eq_ignore_ascii_case("GETPIVOTDATA") => {
                // The answer is one cell of the report, but the report's shape changes when it is
                // refreshed: the whole written area is the dependency (and the lookup is dynamic
                // because a refresh may move the report).
                dynamic = true;
                if let Some(Expr::Ref(r)) = args.get(1)
                    && let Some(at) = resolved(r, base).map(|range| range.start)
                {
                    let mut sheets: Vec<usize> = Vec::new();
                    match &r.sheet {
                        SheetSel::Current => sheets.push(sheet),
                        SheetSel::Named(n) => sheets.extend(wb.sheet_index(n)),
                        SheetSel::Span(a, b) => {
                            if let (Some(i), Some(j)) = (wb.sheet_index(a), wb.sheet_index(b)) {
                                sheets.extend(i.min(j)..=i.max(j));
                            }
                        }
                    }
                    for s in sheets {
                        if let Some(area) = crate::pivot::pivot_at(wb, s, at).and_then(|p| p.last_range) {
                            out.push(Area { sheet: s, range: area });
                        }
                    }
                }
            }
            Expr::Call(n, _)
                if matches!(n.as_str(), "INDIRECT" | "OFFSET" | "NOW" | "TODAY" | "RAND" | "RANDBETWEEN" | "RANDARRAY" | "CELL" | "INFO") =>
            {
                dynamic = true
            }
            Expr::Name(n) => match names.target(wb, sheet, n) {
                NameTarget::Definition(def, scope) if depth < MAX_NAME_DEPTH && *left > 0 && !is_lambda_definition(&def) => {
                    // A definition is written once and kept from A1: its references are coordinates.
                    let inner = precedents_at(names, wb, scope, &def, None, depth + 1, left, out);
                    dynamic |= inner.dynamic;
                    trim |= inner.trim;
                    // `Rate` defined as `NOW()` is as volatile as the call it stands for.
                    volatile |= inner.volatile;
                    hidden_rows |= inner.hidden_rows;
                }
                NameTarget::Table { area, .. } => out.push(area),
                _ => dynamic = true,
            },
            // A structured reference is a fixed area unless it depends on the cell it is written in
            // (a bare `[Column]` binds to the table around it, `[@Column]` to this row) or the
            // table can't be found. The table's whole range covers every part of it.
            Expr::Struct(s) if !s.table.is_empty() && !s.specifiers.contains(&StructItem::ThisRow) => {
                match wb.table(&s.table).and_then(|(si, ti)| wb.sheet(si).and_then(|sh| sh.tables.get(ti)).map(|t| (si, t.range))) {
                    Some((si, range)) => out.push(Area { sheet: si, range }),
                    None => dynamic = true,
                }
            }
            Expr::Struct(_) => dynamic = true,
            Expr::Call(n, _) if call.is_some_and(|(_, spec)| spec.is_none()) && !is_special(n) => dynamic = true,

            // A range ending in a function's result (`A1:INDEX(B1:B9,n)`) covers the cells between
            // the references inside it, not only those references.
            Expr::Binary(BinOp::Range, a, b) if !matches!((&**a, &**b), (Expr::Ref(_), Expr::Ref(_))) => {
                let mut inner = Vec::new();
                precedents_at(names, wb, sheet, a, base, depth, left, &mut inner);
                precedents_at(names, wb, sheet, b, base, depth, left, &mut inner);
                let mut boxes: Vec<Area> = Vec::new();
                for area in inner {
                    match boxes.iter_mut().find(|x| x.sheet == area.sheet) {
                        Some(x) => x.range = x.range.union(&area.range),
                        None => boxes.push(area),
                    }
                }
                out.extend(boxes);
            }
            _ => {}
        }
    });
    Precedents { dynamic, trim, volatile, hidden_rows }
}

fn is_special(n: &str) -> bool {
    matches!(
        n,
        "IF" | "IFS"
            | "IFERROR"
            | "IFNA"
            | "CHOOSE"
            | "SWITCH"
            | "ROW"
            | "COLUMN"
            | "ROWS"
            | "COLUMNS"
            | "AREAS"
            | "ISREF"
            | "ISFORMULA"
            | "FORMULATEXT"
            | "SHEET"
            | "SHEETS"
            | "OFFSET"
            | "INDIRECT"
            | "GETPIVOTDATA"
            | "INDEX"
            | "CELL"
            | "MEASURE"
            | "TRIMRANGE"
            | "SUBTOTAL"
            | "AGGREGATE"
            | "TEXT"
            | "LET"
            | "LAMBDA"
            | "ISOMITTED"
            | "MAP"
            | "REDUCE"
            | "SCAN"
            | "BYROW"
            | "BYCOL"
            | "MAKEARRAY"
    )
}

/// Names of functions the evaluator implements itself (merged into the function list for UI).
pub const SPECIAL_FUNCTIONS: &[&str] = &[
    "IF",
    "IFS",
    "IFERROR",
    "IFNA",
    "CHOOSE",
    "SWITCH",
    "ROW",
    "COLUMN",
    "ROWS",
    "COLUMNS",
    "AREAS",
    "ISREF",
    "ISFORMULA",
    "FORMULATEXT",
    "SHEET",
    "SHEETS",
    "OFFSET",
    "INDIRECT",
    "GETPIVOTDATA",
    "INDEX",
    "CELL",
    "MEASURE",
    "TRIMRANGE",
    "SUBTOTAL",
    "AGGREGATE",
    "TEXT",
    "LET",
    "LAMBDA",
    "ISOMITTED",
    "MAP",
    "REDUCE",
    "SCAN",
    "BYROW",
    "BYCOL",
    "MAKEARRAY",
];

pub fn is_known_function(n: &str) -> bool {
    is_special(&n.to_ascii_uppercase()) || gridcraft_functions::lookup(n).is_some()
}
