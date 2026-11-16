//! Dependency graph and recalculation.
//!
//! The graph maps every formula cell to its static precedents (resolved areas). A reverse index
//! answers "which formulas read this cell?": single-cell precedents in a hash map, ranges as row
//! intervals per column (wide ranges per sheet), each an interval index so a lookup costs
//! O(log n + matches) however many ranges share the column. After an edit, the dirty set is the
//! transitive closure of dependents; it is evaluated in topological order, with on-demand
//! evaluation as a safety net for dynamic references. Cycles produce `#CIRC!` unless iterative
//! calculation is on: then a formula read while it is being evaluated gives its previous value,
//! and the formulas that read one are evaluated again until the loop settles
//! (`PassHost::iterate_loops`).

use std::collections::VecDeque;

mod graph;
mod pass_host;

pub use graph::{FxHasher, Graph, Key};
use graph::{HashMap, HashSet, Node};
use pass_host::{PassHost, Slot, holds_content, spilled_nothing, spills_as_before};

use gridcraft_core::{CellError, CellRef, RangeRef, Value};
use gridcraft_formula::{Expr, UnOp};
use gridcraft_model::{CalcMode, Workbook};

use crate::eval::{Area, Evaluator, precedents_into};
use crate::spill_index::{SpillIndex, indexing_pays};

/// Changed cells (in one `cells_changed`) from which a sheet's spill areas are indexed.
const INDEX_SPILLS_FROM: usize = 16;

/// Whether a sheet's spill areas are indexed for a batch of `batch` changed cells of which
/// `on_sheet` are on that sheet: the batch must be a paste, and registering the areas must cost
/// less than walking them once for each of the sheet's own changed cells.
fn index_spills<'a>(batch: usize, on_sheet: usize, areas: impl ExactSizeIterator<Item = &'a RangeRef>) -> bool {
    batch > INDEX_SPILLS_FROM && areas.len() > 0 && indexing_pays(areas, on_sheet)
}

/// Calculation state kept beside a workbook.
#[derive(Clone, Debug)]
pub struct Calc {
    pub graph: Graph,
    rng: u64,
    /// Fixed "now" for deterministic tests; `None` = system clock.
    pub fixed_now: Option<f64>,
    /// Cells whose formulas have errors from cycles.
    pub circular: Vec<Key>,
    pub last_recalc_cells: usize,
    /// Formula cells the last [`Calc::restore`] re-inserted into the graph, or `None` when it
    /// could not and rebuilt instead. Diagnostics, like `last_recalc_cells`: what Undo and Redo
    /// cost on a sheet with a million formulas.
    pub last_restore_cells: Option<usize>,
    /// A parallel pass produced a spill or a cycle and was rolled back: keep passes sequential
    /// until a formula changes, rather than paying for the same failed attempt every recalculation.
    parallel_fallback: bool,
    /// Definitions of the names formulas use, resolved once per sheet and name instead of once
    /// per formula that mentions it (see [`crate::eval::NameCache`]).
    names: crate::eval::NameCache,
    /// Buffer [`precedents_into`] fills for the formula being inserted into the graph, so building
    /// the graph of a million formulas does not allocate a `Vec` per formula.
    scratch_areas: Vec<Area>,
    /// The formulas the last rebuild had to parse from their text, with the tree it parsed. A
    /// document read back from its own saved form keeps a formula's text alone (`Formula::expr` is
    /// not written to a file), so after a rebuild these are handed back to their cells: the writer,
    /// a sort, a fill and a paste then share the tree instead of parsing the text once each. Empty
    /// for a workbook whose cells already carry theirs.
    unshared: Vec<(Key, std::sync::Arc<Expr>)>,
    #[cfg(test)]
    parallel_runs: usize,
    /// Index entries the last `cells_changed` examined looking for the spill areas the edit
    /// touched (tests check that a paste does not walk every area once per cell).
    #[cfg(test)]
    seed_probes: u64,
    /// Index entries the last recalculation's pass examined reading cells and ranges a spill
    /// covers.
    #[cfg(test)]
    read_probes: u64,
    /// Stored spill ranges the pass examined deciding whether an array spills over another
    /// formula's spill: once each to index them, plus the index entries lookups examined (tests
    /// check that M blocked arrays over N stored spills cost N + M, not N × M).
    #[cfg(test)]
    block_probes: u64,
}

impl Default for Calc {
    fn default() -> Self {
        Calc {
            graph: Graph::default(),
            rng: 0x9E37_79B9_7F4A_7C15,
            fixed_now: None,
            circular: vec![],
            last_recalc_cells: 0,
            last_restore_cells: None,
            parallel_fallback: false,
            names: crate::eval::NameCache::default(),
            scratch_areas: vec![],
            unshared: vec![],
            #[cfg(test)]
            parallel_runs: 0,
            #[cfg(test)]
            seed_probes: 0,
            #[cfg(test)]
            read_probes: 0,
            #[cfg(test)]
            block_probes: 0,
        }
    }
}

/// Arrays that own their cells when two spills want the same ones: everything already spilling.
/// The reader rebuilds that state from the file (an anchor's `ref` names the range it spilled
/// into, and the values inside it are cached), so F9 after a reload keeps the same owner. A
/// blocked array has no range and is deliberately absent: it must not be laid out before the
/// owner that sits further down the sheet, or it would take the cells by row-major order.
fn spill_owners(wb: &Workbook) -> HashSet<Key> {
    let mut owners: HashSet<Key> = HashSet::default();
    for (si, sh) in wb.sheets.iter().enumerate() {
        owners.extend(sh.spill_ranges.keys().map(|c| (si, *c)));
    }
    owners
}

/// Whether the formula may return an array (and so spill). The check is deliberately
/// conservative and only used to keep such formulas out of the parallel pass; a miss falls back to
/// a sequential pass when the pass actually produces a spill.
///
/// Functions that aggregate or look up a block and return one value are listed as scalar: their
/// arguments are not walked, so `SUM(A1:A3)` stays parallel although its argument is a range.
fn may_return_array(names: &mut crate::eval::NameCache, wb: &Workbook, sheet: usize, e: &Expr) -> bool {
    may_return_array_at(names, wb, sheet, e, 0)
}

/// [`may_return_array`] at name depth `depth`. Only a name recurses (into its definition, at
/// most `MAX_NAME_DEPTH` deep): the formula tree itself is walked with an explicit stack, as a
/// long operator chain must not recurse once per operand.
fn may_return_array_at(names: &mut crate::eval::NameCache, wb: &Workbook, sheet: usize, root: &Expr, depth: usize) -> bool {
    let mut stack = crate::walk::Stack::new(root);
    while let Some(e) = stack.pop() {
        match e {
            Expr::Array(_) => return true,
            Expr::Ref(r) => {
                if !r.range().is_single() {
                    return true;
                }
            }
            Expr::Unary(UnOp::Spill | UnOp::Trim(_), _) => return true,
            Expr::Call(name, args) => {
                if is_array_function(name) {
                    return true;
                }
                if is_scalar_function(name) {
                    continue;
                }
                for a in args.iter().rev() {
                    stack.push(a);
                }
            }
            Expr::Binary(_, a, b) => {
                stack.push(b);
                stack.push(a);
            }
            Expr::Unary(_, a) | Expr::Paren(a) => stack.push(a),
            Expr::Invoke(..) => return true,
            // A name reads what its definition reads: `Rate` defined as `Sheet1!$B$1` returns
            // one value. Anything that can't be resolved statically may return an array.
            Expr::Name(n) => match names.target(wb, sheet, n) {
                crate::eval::NameTarget::Definition(def, scope) if depth < crate::eval::MAX_NAME_DEPTH => {
                    if may_return_array_at(names, wb, scope, &def, depth + 1) {
                        return true;
                    }
                }
                _ => return true,
            },
            Expr::Struct(_) => return true,
            Expr::Missing | Expr::Number(_) | Expr::Text(_) | Expr::Bool(_) | Expr::Error(_) => {}
        }
    }
    false
}

/// Functions that can return an array of their own.
fn is_array_function(name: &str) -> bool {
    matches!(
        name,
        "FILTER"
            | "SORT"
            | "SORTBY"
            | "UNIQUE"
            | "SEQUENCE"
            | "RANDARRAY"
            | "TRANSPOSE"
            | "TAKE"
            | "DROP"
            | "VSTACK"
            | "HSTACK"
            | "TOROW"
            | "TOCOL"
            | "WRAPROWS"
            | "WRAPCOLS"
            | "EXPAND"
            | "CHOOSEROWS"
            | "CHOOSECOLS"
            | "TEXTSPLIT"
            | "MAP"
            | "SCAN"
            | "MAKEARRAY"
            | "BYROW"
            | "BYCOL"
            | "MMULT"
            | "MINVERSE"
            | "MUNIT"
            | "FREQUENCY"
            | "LINEST"
            | "TREND"
            | "GROWTH"
            | "LOGEST"
            | "GROUPBY"
            | "PIVOTBY"
    )
}

/// Functions that take a block but return one value (or a reference to one): a range argument to
/// one of these does not make the formula an array formula. A function that can also lift over an
/// array (`INDEX`, `XLOOKUP`) is listed here; if it does return an array, the pass falls back to
/// the sequential path when it sees the spill.
fn is_scalar_function(name: &str) -> bool {
    matches!(
        name,
        "SUM"
            | "SUMIF"
            | "SUMIFS"
            | "SUMX2MY2"
            | "SUMX2PY2"
            | "SUMXMY2"
            | "SUMPRODUCT"
            | "COUNT"
            | "COUNTA"
            | "COUNTBLANK"
            | "COUNTIF"
            | "COUNTIFS"
            | "AVERAGE"
            | "AVERAGEIF"
            | "AVERAGEIFS"
            | "MIN"
            | "MAX"
            | "MEDIAN"
            | "MODE"
            | "MODE.SNGL"
            | "PRODUCT"
            | "STDEV"
            | "STDEVP"
            | "STDEV.S"
            | "STDEV.P"
            | "VAR"
            | "VARP"
            | "VAR.S"
            | "VAR.P"
            | "MATCH"
            | "XMATCH"
            | "LOOKUP"
            | "VLOOKUP"
            | "HLOOKUP"
            | "XLOOKUP"
            | "INDEX"
            | "IFERROR"
            | "IFNA"
            | "ROW"
            | "COLUMN"
            | "ROWS"
            | "COLUMNS"
            | "AREAS"
            | "SHEET"
            | "SHEETS"
            | "CELL"
            | "INFO"
            | "TEXTJOIN"
            | "CONCAT"
            | "CONCATENATE"
            | "TEXT"
            | "VALUE"
            | "DATE"
            | "DATEVALUE"
            | "TIME"
            | "TIMEVALUE"
            | "SUBTOTAL"
            | "AGGREGATE"
            | "AND"
            | "OR"
            | "XOR"
    )
}

/// Independent formulas a pass needs before splitting across threads: below this, starting
/// threads and building one expression map per thread costs more than the pass.
#[cfg(not(target_arch = "wasm32"))]
const PARALLEL_MIN: usize = 16_384;

/// Largest precedent block a leaf may read and still be a parallel candidate. A formula reading a
/// whole column is left to the sequential pass rather than checking a million cells for a dirty
/// precedent.
#[cfg(not(target_arch = "wasm32"))]
const PARALLEL_LEAF_CELLS: u64 = 256;

#[cfg(not(target_arch = "wasm32"))]
struct Shard {
    values: Vec<(Key, Value)>,
    clean: bool,
}

/// Writes values back without sorting: the caller guarantees row-major order.
fn write_values_ordered(wb: &mut Workbook, values: &[(Key, Value)]) {
    let mut start = 0;
    while start < values.len() {
        let si = values[start].0.0;
        let end = values[start..].partition_point(|(k, _)| k.0 == si) + start;
        if let Some(sheet) = wb.sheet_mut(si) {
            let mut batch: Vec<(CellRef, Value)> = values[start..end].iter().map(|(k, v)| (k.1, v.clone())).collect();
            sheet.cells.set_values(&mut batch);
        }
        start = end;
    }
}

/// Gives each formula the tree the rebuild parsed for it, when its cell has none: parsing it again
/// on every later use (the writer, a sort, a fill, a paste) is what this avoids, and the tree is
/// shared through the `Arc` rather than copied. A cell that already holds one is skipped, so the
/// write costs nothing for a formula the user typed or a file reader parsed.
pub(crate) fn share_exprs(wb: &mut Workbook, mut shared: Vec<(Key, std::sync::Arc<Expr>)>) {
    shared.sort_unstable_by_key(|(k, _)| (k.0, k.1.row, k.1.col));
    for ((si, at), e) in shared {
        let Some(sheet) = wb.sheet_mut(si) else { continue };
        let Some(mut cell) = sheet.cells.get(at) else { continue };
        let Some(mut f) = cell.formula.clone() else { continue };
        if f.expr.is_some() {
            continue;
        }
        // The store still holds the `Arc`, so this copies the `Formula` (its text and the pointer
        // to the tree), not the tree.
        std::sync::Arc::make_mut(&mut f).share_expr(&e);
        cell.formula = Some(f);
        sheet.cells.set(at, cell);
    }
}

/// Writes values back in row-major order. One sheet at a time, so the sheet's cells are borrowed
/// once rather than looked up per value; the store rewrites the packed value slot in place.
fn write_values(wb: &mut Workbook, mut values: Vec<(Key, Value)>) {
    values.sort_unstable_by_key(|(k, _)| (k.0, k.1.row, k.1.col));
    write_values_ordered(wb, &values);
}

#[cfg(not(target_arch = "wasm32"))]
impl Calc {
    /// Evaluates the independent leaf formulas of `dirty` on the machine's threads and writes their
    /// values, removing them from `dirty`. Returns false, changing nothing, when the pass is not
    /// safe to split or too small to pay for it.
    ///
    /// A formula is a leaf when none of its precedent blocks holds a formula this pass is also
    /// recalculating. Leaves cannot cycle with each other, and because the whole pass is refused
    /// when any dirty formula is dynamic, may spill or is a legacy array, the shards share only
    /// stored values. Each shard scans its slice of the dirty set, evaluates its leaves with its own
    /// expression and result maps, and returns them in row order; the values are merged and written
    /// in that order.
    fn try_parallel(&mut self, wb: &mut Workbook, dirty: &mut Vec<Key>) -> bool {
        if self.parallel_fallback || dirty.len() < PARALLEL_MIN || wb.calc.iterative {
            return false;
        }
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(8);
        if threads < 2 {
            return false;
        }
        let dirty_set: HashSet<Key> = dirty.iter().copied().collect();
        // A dynamic or array formula can read or write cells the static graph does not name; leave
        // the whole pass to the sequential path if one is being recalculated. The graph keeps these
        // sets small (they are formulas that can spill or are volatile), so checking them costs a
        // handful of lookups, not a scan of every formula.
        let intersects = |set: &HashSet<Key>| {
            if set.len() <= dirty_set.len() { set.iter().any(|k| dirty_set.contains(k)) } else { dirty_set.iter().any(|k| set.contains(k)) }
        };
        if intersects(&self.graph.dynamic) || intersects(&self.graph.may_spill) || intersects(&self.graph.legacy) {
            return false;
        }
        let chunk = dirty.len().div_ceil(threads);
        let now = self.now();
        let seed = self.rng;
        let graph = &self.graph;
        let wb_ref: &Workbook = wb;
        let dirty_set = &dirty_set;
        let started = prof_now();
        let shards: Vec<std::thread::Result<Shard>> = std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for (ti, part) in dirty.chunks(chunk).enumerate() {
                handles.push(scope.spawn(move || {
                    let mut local: Vec<Key> = Vec::new();
                    for &k in part {
                        let Some(node) = graph.nodes.get(&k) else { continue };
                        let mut leaf = true;
                        for area in graph.areas_of(node) {
                            if area.range.count() > PARALLEL_LEAF_CELLS {
                                leaf = false;
                                break;
                            }
                            for c in area.range.iter() {
                                if dirty_set.contains(&(area.sheet, c)) {
                                    leaf = false;
                                    break;
                                }
                            }
                            if !leaf {
                                break;
                            }
                        }
                        if leaf && node.expr.is_some() {
                            local.push(k);
                        }
                    }
                    if local.is_empty() {
                        return Shard { values: Vec::new(), clean: true };
                    }
                    let mut exprs: HashMap<Key, std::sync::Arc<Expr>> = HashMap::with_capacity_and_hasher(local.len(), Default::default());
                    let mut legacy: HashMap<Key, RangeRef> = HashMap::default();
                    for &k in &local {
                        if let Some(node) = graph.nodes.get(&k) {
                            if let Some(e) = &node.expr {
                                exprs.insert(k, std::sync::Arc::clone(e));
                            }
                            if let Some(r) = graph.legacy_of(k) {
                                legacy.insert(k, r);
                            }
                        }
                    }
                    let mut rng = seed ^ (ti as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
                    // A shard keeps a cache of its own: the pass's is a `&mut` borrow, which the
                    // threads can't share, and a shard resolves each name once at most anyway.
                    let mut names = crate::eval::NameCache::default();
                    let mut host = PassHost::new(wb_ref, graph, &exprs, &legacy, &mut names, &mut rng, now);
                    host.iterate = false;
                    for &k in &local {
                        host.calculate(k);
                    }
                    let clean = host.spills.is_empty()
                        && host.cycles.is_empty()
                        && !host.cycle
                        && host.suspend.is_none()
                        && host.abandoned.is_empty()
                        && host.also.is_empty()
                        && host.waiting.is_empty()
                        && host.tainted.is_empty()
                        && host.members.is_empty()
                        && host.depth == 0;
                    // Keep the local order: the values then concatenate into a row-major list.
                    let values: Vec<(Key, Value)> =
                        local.iter().filter_map(|k| host.results.get(k).and_then(|s| s.value.as_ref()).map(|v| (*k, v.clone()))).collect();
                    let clean = clean && values.len() == local.len();
                    Shard { values, clean }
                }));
            }
            handles.into_iter().map(|h| h.join()).collect()
        });
        let eval_ms = prof_now() - started;
        let total: usize = shards.iter().filter_map(|r| r.as_ref().ok()).map(|s| s.values.len()).sum();
        if total < PARALLEL_MIN {
            return false;
        }
        let mut values = Vec::with_capacity(total);
        for shard in shards {
            match shard {
                Ok(shard) if shard.clean => values.extend(shard.values),
                _ => {
                    self.parallel_fallback = true;
                    return false;
                }
            }
        }
        if values.len() != total {
            self.parallel_fallback = true;
            return false;
        }
        let write_started = prof_now();
        write_values_ordered(wb, &values);
        let write_ms = prof_now() - write_started;
        // Remove the candidates from `dirty` in one linear merge: each shard's candidates are a
        // subsequence of its slice of the (row-major) dirty list, and `values` is in that order.
        let mut vi = 0;
        let mut remaining = Vec::with_capacity(dirty.len() - total);
        for part in dirty.chunks(chunk) {
            for &k in part {
                if vi < values.len() && values[vi].0 == k {
                    vi += 1;
                } else {
                    remaining.push(k);
                }
            }
        }
        *dirty = remaining;
        self.last_recalc_cells += total;
        #[cfg(test)]
        {
            self.parallel_runs += 1;
        }
        if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
            eprintln!("  parallel {total} formulas on {threads} threads: eval {eval_ms:.1} ms, write {write_ms:.1} ms");
        }
        true
    }
}

#[cfg(target_arch = "wasm32")]
impl Calc {
    fn try_parallel(&mut self, _wb: &mut Workbook, _dirty: &mut Vec<Key>) -> bool {
        false
    }
}

impl Calc {
    pub fn new() -> Self {
        Self::default()
    }

    fn now(&self) -> f64 {
        if let Some(n) = self.fixed_now {
            return n;
        }
        now_serial()
    }

    /// Rebuilds the graph from scratch.
    pub fn rebuild(&mut self, wb: &Workbook) {
        self.parallel_fallback = false;
        self.graph = Graph::default();
        self.unshared.clear();
        for (si, sheet) in wb.sheets.iter().enumerate() {
            // One pass to learn the size (moving, not cloning, what the store hands out): the
            // graph's tables then grow once instead of rehashing every entry a dozen times.
            let formulas: Vec<_> = sheet.cells.formulas().collect();
            self.graph.reserve(formulas.len());
            for (c, f, _) in formulas {
                if let Some(e) = f.expr_arc() {
                    let reads = precedents_into(&mut self.names, wb, si, &e, &mut self.scratch_areas);
                    let may_spill = may_return_array(&mut self.names, wb, si, &e);
                    let areas = std::mem::take(&mut self.scratch_areas);
                    let node = Node::new(Some(e.clone()), reads.dynamic, may_spill, reads.trim)
                        .with_volatile(reads.volatile)
                        .with_visibility(reads.hidden_rows);
                    self.graph.insert((si, c), &areas, f.array, node);
                    self.scratch_areas = areas;
                    // The tree had to be parsed anyway; keeping it for the cell saves parsing it
                    // again for every later use.
                    if f.expr.is_none() {
                        self.unshared.push(((si, c), e));
                    }
                }
            }
        }
        self.graph.settle();
    }

    /// Puts the graph right after the workbook was replaced with a snapshot of an earlier version
    /// of itself — what Undo and Redo do: `from` is the workbook being replaced, `to` the snapshot
    /// taking its place.
    ///
    /// **Nothing is evaluated**, on purpose. The snapshot holds the values the undone edit
    /// replaced, so the workbook is already right; the only thing that can be out of date is the
    /// graph. And only the formulas the two versions disagree on are re-inserted: the cells the
    /// edit wrote, not the sheet (`CellStore::formula_changes_against` walks the bands the two
    /// versions do not share). Undoing one edit on a million formulas then costs one cell instead
    /// of a rebuild of the million.
    ///
    /// Returns whether it could do that; false means it fell back to [`rebuild`](Self::rebuild)
    /// because the two versions cannot be compared cell by cell — a different set or order of
    /// sheets, rows or columns inserted or deleted, a paged store, or defined names, tables or
    /// report definitions that moved under the formulas, since those decide what a reference means
    /// and so what a formula's precedents are.
    pub fn restore(&mut self, from: &Workbook, to: &Workbook) -> bool {
        // Whatever happens below, this call evaluates nothing: the snapshot it restores was
        // calculated when it was made.
        self.last_recalc_cells = 0;
        let Some(changes) = Self::formula_changes(from, to) else {
            self.last_restore_cells = None;
            self.rebuild(to);
            return false;
        };
        self.last_restore_cells = Some(changes.len());
        self.parallel_fallback = false;
        for (si, at, formula) in changes {
            // A formula whose text does not parse has no node at all, as in `rebuild`.
            match formula.as_ref().and_then(|f| f.expr_arc().map(|e| (e, f.array))) {
                Some((e, legacy)) => {
                    let reads = precedents_into(&mut self.names, to, si, &e, &mut self.scratch_areas);
                    let may_spill = may_return_array(&mut self.names, to, si, &e);
                    let areas = std::mem::take(&mut self.scratch_areas);
                    let node =
                        Node::new(Some(e), reads.dynamic, may_spill, reads.trim).with_volatile(reads.volatile).with_visibility(reads.hidden_rows);
                    self.graph.insert((si, at), &areas, legacy, node);
                    self.scratch_areas = areas;
                }
                None => self.graph.remove((si, at)),
            }
        }
        self.graph.settle();
        true
    }

    /// The formula cells two versions of a workbook disagree on, with the sheet each is on, or
    /// `None` when they cannot be found cheaply (see [`Calc::restore`]).
    fn formula_changes(from: &Workbook, to: &Workbook) -> Option<Vec<(usize, CellRef, Option<std::sync::Arc<gridcraft_model::Formula>>)>> {
        if from.sheets.len() != to.sheets.len() || from.names != to.names {
            return None;
        }
        let mut out = Vec::new();
        for (si, (a, b)) in from.sheets.iter().zip(&to.sheets).enumerate() {
            // What a reference resolves to lives beside the cells: the sheet's name, its tables,
            // and the reports a GETPIVOTDATA reads. A formula's precedents depend on all of them.
            if a.name != b.name || a.tables != b.tables || a.pivots != b.pivots {
                return None;
            }
            if std::sync::Arc::ptr_eq(a, b) {
                continue;
            }
            for (at, formula) in b.cells.formula_changes_against(&a.cells)? {
                out.push((si, at, formula));
            }
        }
        Some(out)
    }

    /// Recalculates every formula (F9 / Ctrl+Alt+F9, and after loading).
    pub fn recalc_all(&mut self, wb: &mut Workbook) {
        self.rebuild(wb);
        share_exprs(wb, std::mem::take(&mut self.unshared));
        let prefer = spill_owners(wb);
        // Spills are laid out again from the formulas where they are now (a structural edit moves
        // or deletes the formulas, not the values they spilled).
        for i in 0..wb.sheets.len() {
            if wb.sheet(i).is_some_and(|s| !s.spill_ranges.is_empty() || !s.spill.is_empty())
                && let Some(sh) = wb.sheet_mut(i)
            {
                sh.spill.clear();
                sh.spill_ranges.clear();
            }
        }
        let all: Vec<Key> = self.graph.nodes.keys().copied().collect();
        self.run(wb, all, true, &prefer);
    }

    /// Recalculates what a freshly opened workbook needs, and nothing else.
    ///
    /// A file carries the values of its formulas, so recalculating all of them only repeats what it
    /// already says — on a large sheet that is seconds of the open. Values are still computed when
    /// the file asks for it (`fullCalcOnLoad`), when a cell has no cached value (saved by a producer
    /// that did not calculate), when the formula is volatile (a cached `NOW()` is out of date by
    /// definition), and when the value is not in the file at all: a dynamic array's spilled cells
    /// and a legacy array formula's non-anchor cells are derived on open. Their dependents come
    /// along, so nothing is left reading a value the file does not hold.
    pub fn recalc_on_open(&mut self, wb: &mut Workbook) {
        let tp = prof_now();
        self.rebuild(wb);
        share_exprs(wb, std::mem::take(&mut self.unshared));
        let rebuild_ms = prof_now() - tp;
        let ts = prof_now();
        if wb.calc.full_calc_on_load {
            self.recalc_all(wb);
            return;
        }
        // Spilled cells are derived state, never file content.
        let mut seeds: Vec<Key> = self.graph.dynamic.iter().copied().collect();
        for (si, sheet) in wb.sheets.iter().enumerate() {
            for (c, f, has_value) in sheet.cells.formulas() {
                let derived = f.array.is_some()      // a legacy array fills its range
                    || f.dynamic                     // a dynamic array's spilled cells are derived
                    || !has_value; // the producer saved no value
                // Volatility is a flag the graph build recorded while it walked the formula for its
                // precedents, so this scan reads a bit instead of walking every tree again — a
                // tenth of a second of a large open, for an answer that was already known.
                if derived || self.graph.is_volatile((si, c)) {
                    seeds.push((si, c));
                }
            }
        }
        if seeds.is_empty() {
            if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
                eprintln!("  open: rebuild {rebuild_ms:.1} ms, no stale cells ({} ms)", prof_now() - ts);
            }
            return;
        }
        let stale = self.dirty_closure(wb, &seeds);
        if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
            eprintln!("  open: rebuild {rebuild_ms:.1} ms, {} seeds, {} stale ({} ms)", seeds.len(), stale.len(), prof_now() - ts);
        }
        self.run(wb, stale, true, &HashSet::default());
    }

    /// Whether any formula on `sheet` reads a reference through the used range, so an edit that
    /// moves the used range has to evaluate it again.
    pub fn trims_used_range(&self, sheet: usize) -> bool {
        self.graph.trim.iter().any(|k| k.0 == sheet)
    }

    /// Recalculates the formulas whose value depends on which rows are hidden, and their
    /// dependents.
    ///
    /// A filter, a group collapsed or shown and a row height of zero change no cell's content:
    /// they only change what the sheet shows. `SUBTOTAL` and `AGGREGATE` read that state, so
    /// they are the formulas such an edit has to evaluate again — not the workbook.
    pub fn visibility_changed(&mut self, wb: &mut Workbook) {
        if self.graph.visibility.is_empty() || wb.calc.mode == CalcMode::Manual {
            return;
        }
        let seeds: Vec<Key> = self.graph.visibility.iter().copied().collect();
        let dirty = self.dirty_closure(wb, &seeds);
        self.run(wb, dirty, false, &HashSet::default());
    }

    /// Updates the graph for cells whose content changed and recalculates what depends on them.
    pub fn cells_changed(&mut self, wb: &mut Workbook, changed: &[Key]) {
        self.cells_changed_with_used(wb, changed, &[]);
    }

    /// [`cells_changed`](Self::cells_changed) for an edit that moved the used range of the sheets
    /// in `used_changed`: their formulas that read a reference through the used range (a whole
    /// column or row, `SUM(A:A)`, `=23:26`) are evaluated again, because the cells they read
    /// changed without any of them being named as a precedent.
    pub fn cells_changed_with_used(&mut self, wb: &mut Workbook, changed: &[Key], used_changed: &[usize]) {
        self.parallel_fallback = false;
        let mut cleared: Vec<(usize, RangeRef)> = Vec::new();
        // Whether the edit put a formula anywhere: when it did not, and the workbook has none
        // stored either, no cell can depend on the change and the pass below has nothing to plan.
        let mut formulas = false;
        for &k in changed {
            let formula = wb.sheet(k.0).and_then(|s| s.cell(k.1)).and_then(|c| c.formula.clone());
            formulas |= formula.is_some();
            let parsed = formula.as_ref().and_then(|f| f.expr_arc().map(|e| (e, f.array)));
            match parsed {
                Some((e, legacy)) => {
                    let reads = precedents_into(&mut self.names, wb, k.0, &e, &mut self.scratch_areas);
                    let may_spill = may_return_array(&mut self.names, wb, k.0, &e);
                    let areas = std::mem::take(&mut self.scratch_areas);
                    let node =
                        Node::new(Some(e), reads.dynamic, may_spill, reads.trim).with_volatile(reads.volatile).with_visibility(reads.hidden_rows);
                    self.graph.insert(k, &areas, legacy, node);
                    self.scratch_areas = areas;
                }
                None => {
                    self.graph.remove(k);
                    // A formula replaced by a constant (or cleared) takes its spilled values with it.
                    if let Some(sh) = wb.sheet_mut(k.0)
                        && let Some(r) = sh.spill_ranges.remove(&k.1)
                    {
                        for c in r.iter() {
                            sh.spill.remove(&c);
                        }
                        cleared.push((k.0, r));
                    }
                    // A formula that doesn't parse evaluates to #NAME?.
                    if let Some(sheet) = wb.sheet_mut(k.0)
                        && let Some(mut cell) = sheet.cells.get(k.1)
                        && cell.formula.is_some()
                    {
                        cell.value = Value::Error(CellError::Name);
                        sheet.cells.set(k.1, cell);
                    }
                }
            }
        }
        self.graph.settle();
        // Nothing can be recalculated: the edit stored no formula, the workbook holds none, no
        // array spills anywhere and no used range moved. Scanning a million changed cells for
        // precedents of a data import used to cost more than writing them.
        if !formulas
            && self.graph.nodes.is_empty()
            && self.graph.trim.is_empty()
            && cleared.is_empty()
            && used_changed.is_empty()
            && wb.sheets.iter().all(|s| s.spill_ranges.is_empty() && s.spill.is_empty())
        {
            self.last_recalc_cells = 0;
            return;
        }
        if wb.calc.mode == CalcMode::Manual {
            // Only the edited formulas themselves are evaluated.
            let own: Vec<Key> = changed.iter().copied().filter(|k| self.graph.nodes.contains_key(k)).collect();
            self.run(wb, own, false, &HashSet::default());
            return;
        }
        let mut seeds: Vec<Key> = changed.to_vec();
        for &si in used_changed {
            seeds.extend(self.graph.trim.iter().filter(|k| k.0 == si).copied());
        }
        // Arrays are the only reason to look past the changed cells themselves: a formula that may
        // spill, a dynamic reference, a legacy array, or an area already spilled. With none of
        // those in the workbook no cell can be blocked and no area can move, so a bulk edit of a
        // plain sheet — a data import, a column of formulas — costs its own cells instead of a
        // scan of the area they cover.
        let arrays = !self.graph.may_spill.is_empty()
            || !self.graph.dynamic.is_empty()
            || !self.graph.legacy.is_empty()
            || wb.sheets.iter().any(|s| !s.spill_ranges.is_empty() || !s.spill.is_empty());
        #[cfg(test)]
        {
            self.seed_probes = 0;
        }
        if arrays {
            let mut emptied: Vec<Key> = Vec::new();
            // Per-sheet bounds of the edit, so a bulk change scans for blocked arrays once instead of
            // once per cell: redoing a whole row of Remove Duplicates used to walk a 64×64 block for
            // each of a million changed cells.
            let mut bounds: Vec<(usize, u32, u32, u32, u32)> = Vec::new();
            for &k in changed {
                let Some(sh) = wb.sheet(k.0) else { continue };
                match bounds.iter_mut().find(|x| x.0 == k.0) {
                    Some(x) => {
                        x.1 = x.1.min(k.1.row);
                        x.2 = x.2.min(k.1.col);
                        x.3 = x.3.max(k.1.row);
                        x.4 = x.4.max(k.1.col);
                    }
                    None => bounds.push((k.0, k.1.row, k.1.col, k.1.row, k.1.col)),
                }
                if !sh.cells.has(k.1) {
                    match emptied.iter_mut().find(|x| x.0 == k.0) {
                        Some(x) => x.1 = CellRef::new(x.1.row.max(k.1.row), x.1.col.max(k.1.col)),
                        None => emptied.push(k),
                    }
                }
            }
            // The per-cell scan looks at the 64×64 block above and to the left of each changed cell.
            // For a bulk edit, scan the union of those blocks once (or keep the per-cell scan when the
            // changed cells are scattered and the union would be much larger).
            let mut per_sheet: Vec<(usize, usize)> = bounds.iter().map(|b| (b.0, 0)).collect();
            for &k in changed {
                if let Some(x) = per_sheet.iter_mut().find(|x| x.0 == k.0) {
                    x.1 += 1;
                }
            }
            let per_cell_cost = (changed.len() as u64).saturating_mul(4096);
            let union_cost: u64 = bounds.iter().map(|&(_, r0, c0, r1, c1)| (u64::from(r1 - r0) + 64).saturating_mul(u64::from(c1 - c0) + 64)).sum();
            let bulk_spill_scan = changed.len() > 64 && union_cost <= per_cell_cost;
            if bulk_spill_scan {
                for &(si, r0, c0, r1, c1) in &bounds {
                    let Some(sh) = wb.sheet(si) else { continue };
                    let area = RangeRef::new(CellRef::new(r0.saturating_sub(64), c0.saturating_sub(64)), CellRef::new(r1, c1));
                    for (c, cell) in sh.cells.iter_range(area) {
                        if spilled_nothing(&cell.value) {
                            seeds.push((si, c));
                        }
                    }
                    let body = RangeRef::new(CellRef::new(r0, c0), CellRef::new(r1, c1));
                    for (anchor, r) in &sh.spill_ranges {
                        if r.intersects(&body) {
                            seeds.push((si, *anchor));
                        }
                    }
                }
            } else {
                // Typing into (or clearing) a cell a dynamic array spills over (or would spill over)
                // re-evaluates the anchor, which may now be blocked or unblocked. With many areas on
                // the sheet they are indexed once, so the batch costs the cells it changed rather than
                // the areas times the cells.
                let mut areas = SpillIndex::default();
                let mut strategy: Vec<(usize, bool)> = Vec::new();
                let mut hits: Vec<CellRef> = Vec::new();
                for &k in changed {
                    let Some(sh) = wb.sheet(k.0) else { continue };
                    let on_sheet = per_sheet.iter().find(|x| x.0 == k.0).map_or(0, |x| x.1);
                    let indexed = match strategy.iter().find(|x| x.0 == k.0) {
                        Some(x) => x.1,
                        None => {
                            let indexed = index_spills(changed.len(), on_sheet, sh.spill_ranges.values());
                            if indexed {
                                for (anchor, r) in &sh.spill_ranges {
                                    areas.insert_rect((k.0, *anchor), *r, false);
                                }
                            }
                            strategy.push((k.0, indexed));
                            indexed
                        }
                    };
                    if indexed {
                        // Anchor order, as walking `spill_ranges` gives.
                        areas.meeting(k.0, RangeRef::new(k.1, k.1), &mut hits);
                        seeds.extend(hits.iter().filter(|a| **a != k.1).map(|a| (k.0, *a)));
                    } else {
                        for (anchor, r) in &sh.spill_ranges {
                            if *anchor != k.1 && r.contains(k.1) {
                                seeds.push((k.0, *anchor));
                            }
                        }
                    }
                    for (c, cell) in sh.cells.iter_range(RangeRef::new(CellRef::new(k.1.row.saturating_sub(64), k.1.col.saturating_sub(64)), k.1)) {
                        if c != k.1 && spilled_nothing(&cell.value) {
                            seeds.push((k.0, c));
                        }
                    }
                }
                #[cfg(test)]
                {
                    self.seed_probes = areas.probes();
                }
            }
            // An emptied cell can unblock an anchor any distance above or to the left of it.
            for (si, end) in emptied {
                for &(s, c) in self.graph.nodes.keys() {
                    if s == si && c.row <= end.row && c.col <= end.col && wb.sheet(s).is_some_and(|sh| spilled_nothing(&sh.value(c))) {
                        seeds.push((s, c));
                    }
                }
            }
            // Spill areas of changed anchors also change.
            for &k in changed {
                if let Some(r) = wb.sheet(k.0).and_then(|s| s.spill_ranges.get(&k.1)) {
                    seeds.extend(r.iter().map(|c| (k.0, c)));
                }
            }
        }
        for (si, r) in &cleared {
            seeds.extend(r.iter().map(|c| (*si, c)));
        }
        let t0 = prof_now();
        let dirty = self.dirty_closure(wb, &seeds);
        let t1 = prof_now();
        let n = dirty.len();
        self.run(wb, dirty, false, &HashSet::default());
        if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
            eprintln!("recalc: {} seeds, closure {} cells {:.1} ms, run {:.1} ms", seeds.len(), n, t1 - t0, prof_now() - t1);
        }
    }

    fn dirty_closure(&self, _wb: &Workbook, seeds: &[Key]) -> Vec<Key> {
        let mut seen: HashSet<Key> = HashSet::default();
        let mut queue: VecDeque<Key> = VecDeque::new();
        for k in seeds {
            if self.graph.nodes.contains_key(k) && seen.insert(*k) {
                queue.push_back(*k);
            }
            let mut deps = Vec::new();
            self.graph.dependents(k.0, k.1, &mut deps);
            // One seed (the cell every formula reads) can name a million dependents: reserve once
            // instead of growing the set and queue through a dozen rehashes.
            seen.reserve(deps.len());
            queue.reserve(deps.len());
            for d in deps {
                if seen.insert(d) {
                    queue.push_back(d);
                }
            }
        }
        for k in &self.graph.dynamic {
            if seen.insert(*k) {
                queue.push_back(*k);
            }
        }
        let mut deps = Vec::new();
        while let Some(k) = queue.pop_front() {
            deps.clear();
            self.graph.dependents(k.0, k.1, &mut deps);
            for d in deps.drain(..) {
                if seen.insert(d) {
                    queue.push_back(d);
                }
            }
        }
        seen.into_iter().collect()
    }

    /// Evaluates the dirty set and writes results (and spills) back.
    fn run(&mut self, wb: &mut Workbook, mut dirty: Vec<Key>, full: bool, prefer: &HashSet<Key>) {
        let mut dynamic_again = !self.graph.dynamic.is_empty();
        self.last_recalc_cells = 0;
        // Every loop the rounds met: a later round evaluates only its dependents, so the cells
        // that were re-entered are remembered for the UI and `Calc::circular`.
        let mut found_cycles: Vec<Key> = Vec::new();
        for _round in 0..8 {
            if dirty.is_empty() {
                break;
            }
            // Row-major order is a good topological guess; on-demand evaluation fixes the rest.
            dirty.sort_by_key(|(s, c)| (*s, c.row, c.col));
            // Independent leaves are the common shape of a large sheet (a column of `=A1*2`);
            // evaluate them on the machine's threads before the sequential pass takes the rest.
            // Writing them makes their dependents leaves too, so repeat while each wave is large
            // enough to pay for the threads (a layered sheet parallelises level by level).
            if _round == 0 {
                // A layered sheet can have many waves; after a handful the remaining formulas are
                // cheaper to take in one sequential pass than to rescan again and again.
                let mut waves = 0;
                while waves < 16 && self.try_parallel(wb, &mut dirty) {
                    waves += 1;
                    if dirty.is_empty() {
                        break;
                    }
                }
                if dirty.is_empty() {
                    break;
                }
            }
            // The graph already holds each formula's parsed expression (and the range of a
            // legacy array): the pass reads them from there instead of rebuilding 100k cells.
            let mut exprs: HashMap<Key, std::sync::Arc<Expr>> = HashMap::with_capacity_and_hasher(dirty.len(), Default::default());
            let mut legacy: HashMap<Key, RangeRef> = HashMap::default();
            for k in &dirty {
                if let Some(node) = self.graph.nodes.get(k) {
                    if let Some(e) = &node.expr {
                        exprs.insert(*k, std::sync::Arc::clone(e));
                    }
                    if let Some(r) = self.graph.legacy_of(*k) {
                        legacy.insert(*k, r);
                    }
                }
            }
            let now = self.now();
            let mut rng = self.rng;
            let tp = prof_now();
            let (mut results, mut spills, cycles) = {
                let mut host = PassHost::new(wb, &self.graph, &exprs, &legacy, &mut self.names, &mut rng, now);
                host.iterate = wb.calc.iterative;
                for k in &dirty {
                    host.calculate(*k);
                }
                if host.iterate && !host.members.is_empty() {
                    let passes = host.iterate_loops(&dirty, wb.calc.max_iterations, wb.calc.max_change);
                    if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
                        eprintln!("  iterative calculation: {} passes over {} formulas", passes, host.tainted.len());
                    }
                }
                #[cfg(test)]
                {
                    self.read_probes += host.spill_index.probes();
                    self.block_probes += host.block_probes();
                }
                (host.results, host.spills, host.cycles)
            };
            self.rng = rng;
            // A formula that read its own cell is circular even when the function swallowed the
            // #CIRC! the read returned (`COUNTIF` over a column that holds its own formula counts
            // the other cells and ignores the error). Excel shows 0 with a warning; we keep it
            // explicit. Without this the loop's cells hold values that depend on the order the
            // pass happened to compute them in (found by the command-sequence fuzzer).
            //
            // An array this pass laid out but then dropped (the anchor turned out to be circular)
            // leaves the cells it would have spilled over empty. Formulas that read that area —
            // a whole-column `COUNTIF` is one — read its values during the pass, so they are
            // evaluated again after the write-back.
            let mut discarded: Vec<(usize, RangeRef)> = Vec::new();
            if !wb.calc.iterative && !cycles.is_empty() {
                for k in &cycles {
                    results.insert(*k, Slot { value: Some(Value::Error(CellError::Circ)) });
                    if let Some(arr) = spills.remove(k) {
                        let end = CellRef::new(
                            k.1.row.saturating_add((arr.rows as u32).saturating_sub(1)),
                            k.1.col.saturating_add((arr.cols as u32).saturating_sub(1)),
                        );
                        discarded.push((k.0, RangeRef::new(k.1, end)));
                    }
                }
            }
            if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
                eprintln!("  eval {} formulas {:.1} ms", results.len(), prof_now() - tp);
            }
            let tw = prof_now();
            self.last_recalc_cells += results.len();
            found_cycles.extend(cycles.iter().copied());
            // Write back.
            let mut spill_changes: Vec<Key> = Vec::new();
            for (si, range) in discarded {
                self.graph.dependents_of_range(si, range, &mut spill_changes);
            }
            let mut freed: Vec<(usize, RangeRef)> = Vec::new();
            let twr = prof_now();
            // Row-major order: the results come out of a hash map, and walking bands in row order
            // is what makes the write-back touch each band once instead of jumping between 1,563
            // of them at random. Values only change, so the store rewrites the packed slot and
            // nothing else (no `Cell` built, no formula or style looked up).
            let values: Vec<(Key, Value)> = results.into_iter().filter_map(|(k, s)| s.value.map(|v| (k, v))).collect();
            write_values(wb, values);
            let two = prof_now();
            if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
                eprintln!("  write values {:.1} ms", two - twr);
            }
            // Arrays that held a spill before this pass lay out first, so a contested area keeps
            // its owner; everything else follows in row-major order.
            let any_spills = !spills.is_empty() || wb.sheets.iter().any(|s| !s.spill_ranges.is_empty() || !s.spill.is_empty());
            let ordered: Vec<Key> = if any_spills {
                dirty.iter().copied().filter(|k| prefer.contains(k)).chain(dirty.iter().copied().filter(|k| !prefer.contains(k))).collect()
            } else {
                Vec::new()
            };
            for k in &ordered {
                let new = spills.get(k);
                let Some(sheet) = wb.sheet_mut(k.0) else { continue };
                let old = sheet.spill_ranges.get(&k.1).copied();
                if old.is_none() && new.is_none() {
                    continue;
                }
                // An array that spills exactly as before changes nothing its dependents read, so
                // they aren't queued again. Otherwise two arrays that read each other's spill areas
                // queue each other in every round, up to the round limit.
                if let (Some(r), Some(arr)) = (old, new)
                    && spills_as_before(sheet, k.1, r, arr)
                {
                    continue;
                }
                if let Some(r) = old {
                    for c in r.iter() {
                        sheet.spill.remove(&c);
                    }
                    sheet.spill_ranges.remove(&k.1);
                }
                // An array that owned its cells before the pass may grow back into cells another
                // array took in an earlier round, after that array was laid out (this pass only
                // learns the new shape now). The owner gets them back: the other array follows in
                // the layout order and is blocked if it still wants them.
                if prefer.contains(k) {
                    let evicted: Vec<(CellRef, RangeRef)> = sheet
                        .spill_ranges
                        .iter()
                        .filter(|(anchor, _)| **anchor != k.1 && !prefer.contains(&(k.0, **anchor)))
                        .map(|(anchor, range)| (*anchor, *range))
                        .collect();
                    for (anchor, range) in evicted {
                        for c in range.iter() {
                            sheet.spill.remove(&c);
                        }
                        sheet.spill_ranges.remove(&anchor);
                        // It has to re-evaluate: the cells it took are gone, and it is either
                        // blocked now or spills somewhere else.
                        spill_changes.push((k.0, anchor));
                    }
                }
                let mut changed_range = old;
                if let Some(arr) = new {
                    let end = CellRef::new(k.1.row.saturating_add(arr.rows as u32 - 1), k.1.col.saturating_add(arr.cols as u32 - 1));
                    let range = RangeRef::new(k.1, end);
                    let blocked = end.row >= gridcraft_core::MAX_ROWS
                        || end.col >= gridcraft_core::MAX_COLS
                        || range.iter().any(|c| c != k.1 && (holds_content(sheet, c) || sheet.spill.contains_key(&c)))
                        || sheet.merges.iter().any(|m| m.intersects(&range))
                        || sheet.tables.iter().any(|t| t.range.contains(k.1));
                    if blocked {
                        if let Some(mut cell) = sheet.cells.get(k.1) {
                            cell.value = Value::Error(CellError::Spill);
                            sheet.cells.set(k.1, cell);
                        }
                        // Not blocked while the pass ran (it would have been #SPILL! there), so
                        // blocked by another array of this pass: the formulas that read this one,
                        // at its anchor or over the area it would have spilled into, read #SPILL!
                        // or empty cells now.
                        let mut deps = Vec::new();
                        self.graph.dependents_of_range(k.0, range, &mut deps);
                        spill_changes.extend(deps.into_iter().filter(|d| d != k));
                    } else {
                        for (i, c) in range.iter().enumerate() {
                            if c != k.1 {
                                sheet.spill.insert(c, arr.data.get(i).cloned().unwrap_or_default());
                            }
                        }
                        sheet.spill_ranges.insert(k.1, range);
                        changed_range = Some(changed_range.map_or(range, |o| o.union(&range)));
                    }
                }
                if let Some(r) = changed_range {
                    let mut deps = Vec::new();
                    self.graph.dependents_of_range(k.0, r, &mut deps);
                    // Not the anchor itself (that would read its own array), unless its old array
                    // is gone or partly freed: its new formula may have read the old values, still
                    // laid out while it was evaluated.
                    let freed = match (old, sheet.spill_ranges.get(&k.1)) {
                        (Some(o), Some(n)) => !(n.contains(o.start) && n.contains(o.end)),
                        (_, None) => true,
                        (None, Some(_)) => false,
                    };
                    spill_changes.extend(deps.into_iter().filter(|d| d != k || freed));
                    // The spill moved the sheet's used range, and a formula that reads a whole
                    // column or row (trimmed to it) reads different cells now even when the
                    // spill lies nowhere near it.
                    spill_changes.extend(self.graph.trim.iter().filter(|t| t.0 == k.0).copied());
                    // A dynamic reference (INDIRECT, OFFSET, a defined name) can hide a read of
                    // the cells the spill just gave back; the graph does not name them.
                    spill_changes.extend(self.graph.dynamic.iter().filter(|d| d.0 == k.0).copied());
                }
                if let Some(o) = old
                    && !sheet.spill_ranges.get(&k.1).is_some_and(|n| n.contains(o.start) && n.contains(o.end))
                {
                    freed.push((k.0, o));
                }
            }
            // A formula that read one of the loop's cells before they were marked #CIRC! holds a
            // value computed from what the cell happened to show then; it is evaluated again so
            // it sees the #CIRC! a full recalculation would have given it.
            let mut cycle_changes: Vec<Key> = Vec::new();
            for k in &cycles {
                self.graph.dependents(k.0, k.1, &mut cycle_changes);
            }
            // The loop's own cells are not evaluated again: they would compute a number from the
            // other cells and their dependents would read it before this pass could mark them
            // #CIRC! again. Their value is already written.
            let cyclic: HashSet<Key> = cycles.iter().copied().collect();
            spill_changes.extend(cycle_changes.into_iter().filter(|k| !cyclic.contains(k)));
            // Formulas with references only known while evaluating (INDIRECT, OFFSET) may have read
            // a spill area before it was laid out: they are evaluated once more.
            if dynamic_again && !spills.is_empty() {
                dynamic_again = false;
                spill_changes.extend(self.graph.dynamic.iter().copied());
            }
            // An area a formula no longer spills over may unblock another formula's array.
            for (si, o) in freed {
                for &(s, c) in self.graph.nodes.keys() {
                    if s == si
                        && c.row <= o.end.row
                        && c.col <= o.end.col
                        && wb.sheet(s).is_some_and(|sh| sh.value(c) == Value::Error(CellError::Spill))
                    {
                        spill_changes.push((s, c));
                    }
                }
            }
            if full {
                // Everything was already evaluated once; spill dependents need another pass.
            }
            if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
                eprintln!("  spills {:.1} ms, write-back {:.1} ms", prof_now() - two, prof_now() - tw);
            }
            spill_changes.sort_by_key(|(s, c)| (*s, c.row, c.col));
            spill_changes.dedup();
            if spill_changes.is_empty() {
                break;
            }
            let mut next = self.dirty_closure(wb, &spill_changes);
            next.retain(|k| !cyclic.contains(k));
            dirty = next;
        }
        found_cycles.sort_unstable_by_key(|(s, c)| (*s, c.row, c.col));
        found_cycles.dedup();
        self.circular = found_cycles;
    }
}

/// Current date-time as a serial (1900 system), from the system clock (browser clock on wasm).
/// UTC; the local offset isn't available without platform calls. Falls back to a fixed date only
/// when no clock is available.
pub fn now_serial() -> f64 {
    gridcraft_core::date::unix_now_ms().map_or(46302.5, gridcraft_core::date::serial_from_unix_ms)
}

/// Evaluates a formula in the context of a cell without storing it (conditional formats, data
/// validation, Evaluate Formula, Name Manager previews, the CLI `eval`).
pub fn evaluate(wb: &Workbook, sheet: usize, at: CellRef, formula: &str) -> Value {
    let body = formula.strip_prefix('=').unwrap_or(formula);
    let Ok(expr) = gridcraft_formula::parse(body) else { return Value::Error(CellError::Name) };
    evaluate_expr(wb, sheet, at, &expr)
}

pub fn evaluate_expr(wb: &Workbook, sheet: usize, at: CellRef, expr: &Expr) -> Value {
    let exprs = HashMap::default();
    let legacy = HashMap::default();
    let graph = Graph::default();
    let mut rng = 0x1234_5678_9ABC_DEF0u64;
    // One formula, so its own cache (and its names resolved once, however many of them it mentions).
    let mut names = crate::eval::NameCache::default();
    let mut host = PassHost::new(wb, &graph, &exprs, &legacy, &mut names, &mut rng, now_serial());
    let mut ev = Evaluator::new(&mut host, sheet, at);
    ev.value(expr)
}

#[allow(clippy::disallowed_methods)] // the clock is read only off wasm
fn prof_now() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        0.0
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::time::{Duration, Instant};

    use gridcraft_core::{CellError, CellRef, RangeRef, Value};
    use gridcraft_model::{Cell, DefinedName, Formula, Workbook};

    use crate::eval::Host;

    use super::graph::Intervals;
    use super::{Calc, Graph, HashMap, Key, PassHost};

    /// xorshift64 for repeatable pseudo-random cases.
    fn rng(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    fn found(iv: &Intervals<u32>, a: u32, b: u32) -> Vec<u32> {
        let mut v = Vec::new();
        iv.query(a, b, &mut |t| v.push(*t));
        v.sort_unstable();
        v
    }

    #[test]
    fn intervals_match_a_linear_scan() {
        let mut iv: Intervals<u32> = Intervals::default();
        let mut model: Vec<(u32, u32, u32)> = Vec::new();
        let mut s = 0x9E37_79B9_7F4A_7C15;
        for step in 0..20_000u32 {
            let roll = rng(&mut s) % 10;
            if roll < 6 || model.is_empty() {
                let r0 = (rng(&mut s) % 500) as u32;
                let r1 = r0 + (rng(&mut s) % 40) as u32;
                if !model.contains(&(r0, r1, step)) {
                    iv.insert(r0, r1, step);
                    model.push((r0, r1, step));
                }
            } else {
                let i = (rng(&mut s) % model.len() as u64) as usize;
                let (r0, r1, t) = model.swap_remove(i);
                iv.remove(r0, r1, t);
                if roll == 9 {
                    // Removed and put back before the next merge.
                    iv.insert(r0, r1, t);
                    model.push((r0, r1, t));
                }
            }
            if rng(&mut s).is_multiple_of(50) {
                iv.settle();
            }
            if step.is_multiple_of(97) {
                let a = (rng(&mut s) % 560) as u32;
                let b = a + (rng(&mut s) % 30) as u32;
                let mut want: Vec<u32> = model.iter().filter(|(r0, r1, _)| *r0 <= b && *r1 >= a).map(|(_, _, t)| *t).collect();
                want.sort_unstable();
                assert_eq!(found(&iv, a, b), want, "rows {a}..={b} at step {step}");
            }
        }
        iv.settle();
        for row in 0..560 {
            let mut want: Vec<u32> = model.iter().filter(|(r0, r1, _)| *r0 <= row && row <= *r1).map(|(_, _, t)| *t).collect();
            want.sort_unstable();
            assert_eq!(found(&iv, row, row), want, "row {row}");
        }
        for (r0, r1, t) in std::mem::take(&mut model) {
            iv.remove(r0, r1, t);
        }
        iv.settle();
        assert!(iv.is_empty());
    }

    #[test]
    fn interval_queries_cost_log_n_plus_matches() {
        // 40k one-row ranges (like =SUM(E1:J1) down a column) and 1k ranges over everything:
        // a point query visits O(log n) tree nodes per match, not every range in the column.
        let n = 40_000u32;
        let mut iv: Intervals<u32> = Intervals::default();
        for r in 0..n {
            iv.insert(r, r, r);
        }
        for t in 0..1_000 {
            iv.insert(0, n, n + t);
        }
        iv.settle();
        let mut worst = 0;
        for row in (0..n).step_by(7) {
            let mut k = 0;
            let visited = iv.query(row, row, &mut |_| k += 1);
            assert_eq!(k, 1_001);
            worst = worst.max(visited);
        }
        // 1,001 matches in a tree of depth 16: well under 4 nodes per match plus the path.
        assert!(worst < 4 * 1_001 + 64, "visited {worst} nodes");
        let mut k = 0;
        let visited = iv.query(n + 1, n + 5, &mut |_| k += 1);
        assert_eq!(k, 0);
        assert!(visited < 64, "visited {visited} nodes for no match");
    }

    /// `rows` rows of six numbers (E:J) and `=SUM(E{r}:J{r})` in K.
    fn sum_rows(rows: u32) -> Workbook {
        let mut wb = Workbook::new();
        if let Some(sh) = wb.sheet_mut(0) {
            for r in 0..rows {
                for c in 4..10 {
                    sh.cells.set(CellRef::new(r, c), Cell::value(Value::Number(f64::from(r % 97 + c))));
                }
                sh.cells.set(CellRef::new(r, 10), Cell::formula(Formula::new(&format!("=SUM(E{0}:J{0})", r + 1))));
            }
        }
        wb
    }

    /// Opening (full recalc), changing every value of a column, and re-entering every formula.
    #[allow(clippy::disallowed_methods)] // a native-only timing test: the clock is read off wasm
    fn open_and_bulk_edit(rows: u32) -> Duration {
        let mut wb = sum_rows(rows);
        let mut calc = Calc::new();
        let t = Instant::now();
        calc.recalc_all(&mut wb);
        let values: Vec<Key> = (0..rows).map(|r| (0, CellRef::new(r, 4))).collect();
        if let Some(sh) = wb.sheet_mut(0) {
            for (_, c) in &values {
                sh.cells.set(*c, Cell::value(Value::Number(1.0)));
            }
        }
        calc.cells_changed(&mut wb, &values);
        let formulas: Vec<Key> = (0..rows).map(|r| (0, CellRef::new(r, 10))).collect();
        calc.cells_changed(&mut wb, &formulas);
        let elapsed = t.elapsed();
        let last = wb.sheet(0).map(|s| s.value(CellRef::new(rows - 1, 10)));
        let r = rows - 1;
        assert_eq!(last, Some(Value::Number(f64::from(1 + (5..10).map(|c| r % 97 + c).sum::<u32>()))));
        elapsed
    }

    #[test]
    fn range_formulas_recalc_and_bulk_edit_correctly() {
        // The values behind the timing test below, checked on every run.
        open_and_bulk_edit(500);
    }

    #[test]
    #[ignore = "timing-sensitive; run with --ignored in release"]
    #[allow(clippy::disallowed_methods)]
    fn recalc_with_range_formulas_scales_linearly() {
        // Each row's SUM reads a range in the same columns. Recalculating on open used to walk
        // every row per range read (the used range), and finding a cell's dependents scanned
        // every range in its column: 4x the rows took 16x the time. Linear is 4x; allow 8x.
        let best = |rows| (0..3).map(|_| open_and_bulk_edit(rows)).min().unwrap_or_default();
        let small = best(5_000);
        let large = best(20_000);
        assert!(large < small * 8 + Duration::from_millis(100), "5k rows {small:?}, 20k rows {large:?}");
    }

    /// Index entries the seed scan of a scattered paste over `areas` wide spill areas examines.
    /// Each pasted cell lands inside one area, three hundred rows from the next, so the batch is
    /// not scanned as one block and every cell has an anchor to find.
    fn paste_over_spills(areas: u32) -> u64 {
        let mut wb = Workbook::new();
        for i in 0..areas {
            wb.sheet_mut(0).unwrap().cells.set(CellRef::new(i * 300, 0), Cell::formula(Formula::new("=SEQUENCE(2,60)")));
        }
        let mut calc = Calc::new();
        calc.recalc_all(&mut wb);
        assert_eq!(wb.sheet(0).unwrap().spill_ranges.len(), areas as usize);
        // Onto the second row of each area, in its twentieth column.
        let changed: Vec<Key> = (0..areas).map(|i| (0, CellRef::new(i * 300 + 1, 20))).collect();
        for k in &changed {
            wb.sheet_mut(0).unwrap().cells.set(k.1, Cell::value(Value::Number(1.0)));
        }
        calc.cells_changed(&mut wb, &changed);
        // The pasted cell blocks each array: #SPILL! where it spilled, as a paste over an array gives.
        assert_eq!(wb.sheet(0).unwrap().spill_ranges.len(), 0, "the paste onto the areas did not block them");
        assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(0, 0)), Value::Error(CellError::Spill));
        calc.seed_probes
    }

    #[test]
    fn a_paste_over_many_spill_areas_costs_the_cells_it_changed() {
        // Typing into (or clearing) a cell an array spills over re-evaluates the anchor, so the
        // edit used to test every spilled area on the sheet for every changed cell: 300 cells over
        // 300 areas was 90,000 containment checks, and four times the areas times four times the
        // cells was sixteen times the work. The block index makes it a lookup per cell.
        let small = paste_over_spills(75);
        let large = paste_over_spills(300);
        // One area met per cell: 75 and 300 entries, where the walk was 5,625 and 90,000.
        assert!(small > 0 && small <= 2 * 75, "{small} index entries for 75 cells over 75 areas");
        assert!(large > 0 && large <= 2 * 300, "{large} index entries for 300 cells over 300 areas");
        // Four times the areas and four times the cells: the walk was sixteen times the work, the
        // index is the cells (plus the block each one shares with the area it lands on).
        assert!(large <= 6 * small, "{small} entries for 75 areas, {large} for 300");
    }

    /// Entries a pass examines over `ranges` stored spill areas, each with a new array just below
    /// and to the left of it that would spill over one of its cells: the arrays are blocked
    /// (`#SPILL!`), so each one asks which stored area holds the cell it would cover.
    fn blocked_over_stored_spills(ranges: u32) -> u64 {
        let mut wb = Workbook::new();
        for i in 0..ranges {
            put_formula(&mut wb, 0, &format!("C{}", i * 6 + 1), "=SEQUENCE(2,2)");
        }
        let mut calc = Calc::new();
        calc.recalc_all(&mut wb);
        assert_eq!(wb.sheet(0).unwrap().spill_ranges.len(), ranges as usize);
        // One row down and one column left of each area's anchor, so an array covers the area's
        // bottom-left cell without covering its anchor (which would block it as content), and no
        // edit lands inside an area (which would recalculate that area).
        let mut keys: Vec<Key> = Vec::new();
        for i in 0..ranges {
            let anchor = CellRef::new(i * 6 + 1, 1);
            wb.sheet_mut(0).unwrap().cells.set(anchor, Cell::formula(Formula::new("=SEQUENCE(2,2)")));
            keys.push((0, anchor));
        }
        calc.block_probes = 0;
        calc.cells_changed(&mut wb, &keys);
        let sh = wb.sheet(0).unwrap();
        for &(_, c) in &keys {
            assert_eq!(sh.value(c), Value::Error(CellError::Spill), "{c:?} did not spill, over a stored area");
        }
        calc.block_probes
    }

    #[test]
    fn arrays_blocked_by_stored_areas_cost_the_areas_and_the_arrays() {
        // An array that would spill over a cell another formula's spill holds is #SPILL!. Asking
        // the sheet's stored ranges about every cell of the array walked all of them per array:
        // 400 areas with 400 arrays was 160,000 containment checks, and sixteen times each was 256
        // times the work. The areas are indexed once a pass, so the work is their number plus the
        // number of cells that ask.
        let counts = [100u32, 400, 1_600];
        let probes: Vec<u64> = counts.iter().copied().map(blocked_over_stored_spills).collect();
        for (&n, &p) in counts.iter().zip(&probes) {
            // The build walks the sheet's ranges once, and each array examines the entries of the
            // one block its cell falls in (about three areas at this spacing), not all of them.
            assert!(p > 0, "no array was blocked through the index among {n} areas");
            assert!(p <= 8 * 2 * u64::from(n), "{p} entries for {n} areas and {n} arrays");
        }
        eprintln!("stored spill areas / arrays -> entries examined: {counts:?} -> {probes:?}");
        // Sixteen times the areas and sixteen times the arrays: linear, where the walk was
        // quadratic (16 x 16 = 256 times the work).
        assert!(probes[2] <= 20 * probes[0], "{probes:?}");
        assert!(probes[1] <= 6 * probes[0], "{probes:?}");
    }

    #[test]
    fn a_range_read_meets_the_spills_that_reach_it() {
        // Arrays lay out first (row-major), then formulas reading a small range over them: every
        // one of those reads used to walk every array the pass had produced.
        let areas = 300u32;
        let mut wb = Workbook::new();
        for i in 0..areas {
            put_formula(&mut wb, 0, &format!("A{}", i * 3 + 1), "=SEQUENCE(2,2)");
            let row = i * 3 + 2;
            put_formula(&mut wb, 0, &format!("D{row}"), &format!("=SUM(A{row}:C{row})"));
        }
        let mut calc = Calc::new();
        calc.recalc_all(&mut wb);
        assert_eq!(wb.sheet(0).unwrap().spill_ranges.len(), areas as usize);
        // The second row of each 2 x 2 array, summed across A:C: 3 + 4 + blank.
        for i in [0u32, 150, areas - 1] {
            assert_eq!(cell_value(&wb, 0, &format!("D{}", i * 3 + 2)), Value::Number(7.0));
        }
        // The blocks the reads touch hold about six areas each, not three hundred: a walk per
        // read would be 90,000 entries.
        assert!(calc.read_probes > 0, "the reads did not go through the index");
        assert!(calc.read_probes <= 8 * u64::from(areas), "{} index entries for {areas} reads", calc.read_probes);
    }

    /// Two workbooks of the same shape: three arrays, one of them blocked by a filled cell, and a
    /// formula reading a spilled cell.
    fn paste_book() -> Workbook {
        let mut wb = Workbook::new();
        for i in 0..3u32 {
            put_formula(&mut wb, 0, &format!("A{}", i * 3 + 1), "=SEQUENCE(2)");
        }
        put_value(&mut wb, 0, "A5", 9.0);
        put_formula(&mut wb, 0, "C1", "=A2");
        let mut calc = Calc::new();
        calc.recalc_all(&mut wb);
        wb
    }

    #[test]
    fn an_indexed_paste_lands_like_the_per_cell_walk() {
        // The same twenty edits as one batch (past the walk threshold, so the areas are indexed)
        // and one at a time (the walk) leave the same cells, the same spilled areas and the same
        // #SPILL!.
        let edits: Vec<(CellRef, Option<f64>)> = vec![
            (CellRef::new(1, 0), Some(1.0)), // onto an array's second row: blocks it
            (CellRef::new(2, 0), Some(2.0)),
            (CellRef::new(3, 0), Some(3.0)),
            (CellRef::new(4, 0), Some(4.0)), // the filled cell that blocks the second array
            (CellRef::new(7, 0), Some(5.0)),
            (CellRef::new(8, 0), None), // clearing a covered cell unblocks the array
            (CellRef::new(9, 0), Some(6.0)),
            (CellRef::new(11, 0), None),
            (CellRef::new(13, 0), Some(7.0)),
            (CellRef::new(14, 0), Some(8.0)),
            (CellRef::new(20, 0), Some(9.0)),
            (CellRef::new(21, 1), Some(10.0)),
            (CellRef::new(22, 1), Some(11.0)),
            (CellRef::new(23, 1), Some(12.0)),
            (CellRef::new(24, 1), Some(13.0)),
            (CellRef::new(25, 1), Some(14.0)),
            (CellRef::new(26, 1), Some(15.0)),
            (CellRef::new(30, 2), Some(16.0)),
            (CellRef::new(31, 2), None),
            (CellRef::new(32, 2), Some(17.0)),
        ];
        let write = |wb: &mut Workbook, c: CellRef, v: Option<f64>| match v {
            Some(v) => wb.sheet_mut(0).unwrap().cells.set(c, Cell::value(Value::Number(v))),
            None => {
                wb.sheet_mut(0).unwrap().cells.remove(c);
            }
        };
        let mut batch = paste_book();
        let mut calc = Calc::new();
        for &(c, v) in &edits {
            write(&mut batch, c, v);
        }
        let keys: Vec<Key> = edits.iter().map(|(c, _)| (0, *c)).collect();
        calc.cells_changed(&mut batch, &keys);
        assert!(calc.seed_probes > 0, "the batch did not use the index");
        let mut single = paste_book();
        let mut calc = Calc::new();
        for &(c, v) in &edits {
            write(&mut single, c, v);
            calc.cells_changed(&mut single, &[(0, c)]);
        }
        let area = RangeRef::new(CellRef::new(0, 0), CellRef::new(40, 3));
        let (a, b) = (batch.sheet(0).unwrap(), single.sheet(0).unwrap());
        for (c, cell) in a.cells.iter_range(area) {
            let other = b.cells.get(c).map(|x| x.value.clone());
            assert_eq!(Some(cell.value.clone()), other, "{c:?}");
        }
        for (c, _) in b.cells.iter_range(area) {
            assert!(a.cells.get(c).is_some(), "{c:?} only in the per-cell walk");
        }
        assert_eq!(a.spill_ranges, b.spill_ranges);
        assert_eq!(a.spill, b.spill);
        assert_eq!(a.value(CellRef::new(0, 2)), b.value(CellRef::new(0, 2)));
    }

    #[test]
    fn range_dependents_after_edits() {
        // Formulas that read ranges, wide ranges and whole columns, found again after edits.
        let mut wb = Workbook::new();
        let mut calc = Calc::new();
        let set = |wb: &mut Workbook, calc: &mut Calc, at: &str, input: &str| {
            let c = CellRef::parse(at).unwrap();
            let cell = if input.starts_with('=') { Cell::formula(Formula::new(input)) } else { Cell::value(Value::Number(input.parse().unwrap())) };
            wb.sheet_mut(0).unwrap().cells.set(c, cell);
            calc.cells_changed(wb, &[(0, c)]);
        };
        let num = |wb: &Workbook, at: &str| {
            let v = wb.sheet(0).unwrap().value(CellRef::parse(at).unwrap());
            v.as_f64().unwrap_or_else(|| panic!("{at} = {v:?}"))
        };
        set(&mut wb, &mut calc, "A1", "1");
        set(&mut wb, &mut calc, "B2", "2");
        set(&mut wb, &mut calc, "CZ3", "3");
        set(&mut wb, &mut calc, "DA5", "=SUM(A1:B2)");
        set(&mut wb, &mut calc, "DA6", "=SUM(A1:CZ3)");
        set(&mut wb, &mut calc, "DA7", "=SUM(B:B)");
        set(&mut wb, &mut calc, "DA8", "=SUM(2:2)");
        assert_eq!((num(&wb, "DA5"), num(&wb, "DA6"), num(&wb, "DA7"), num(&wb, "DA8")), (3.0, 6.0, 2.0, 2.0));
        set(&mut wb, &mut calc, "B2", "20");
        assert_eq!((num(&wb, "DA5"), num(&wb, "DA6"), num(&wb, "DA7"), num(&wb, "DA8")), (21.0, 24.0, 20.0, 20.0));
        // Re-entering a formula with another range drops the old one.
        set(&mut wb, &mut calc, "DA5", "=SUM(A1:A2)");
        set(&mut wb, &mut calc, "B2", "5");
        assert_eq!(num(&wb, "DA5"), 1.0);
        let mut deps = Vec::new();
        calc.graph.dependents(0, CellRef::parse("B2").unwrap(), &mut deps);
        deps.sort_unstable();
        let at = |s: &str| (0, CellRef::parse(s).unwrap());
        assert_eq!(deps, vec![at("DA6"), at("DA7"), at("DA8")]);
        deps.clear();
        calc.graph.dependents_of_range(0, RangeRef::new(CellRef::parse("A1").unwrap(), CellRef::parse("A9").unwrap()), &mut deps);
        deps.sort_unstable();
        deps.dedup();
        assert_eq!(deps, vec![at("DA5"), at("DA6"), at("DA8")]);
        set(&mut wb, &mut calc, "CZ3", "30");
        assert_eq!(num(&wb, "DA6"), 36.0);
        // Ranges on another sheet.
        wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
        set(&mut wb, &mut calc, "DA9", "=SUM(Data!A1:A3)+SUM(Data!A:CZ)");
        assert_eq!(num(&wb, "DA9"), 0.0);
        wb.sheet_mut(1).unwrap().cells.set(CellRef::parse("A2").unwrap(), Cell::value(Value::Number(4.0)));
        calc.cells_changed(&mut wb, &[(1, CellRef::parse("A2").unwrap())]);
        assert_eq!(num(&wb, "DA9"), 8.0);
        deps.clear();
        calc.graph.dependents(1, CellRef::parse("A2").unwrap(), &mut deps);
        assert_eq!(deps, vec![at("DA9"), at("DA9")]);
    }

    #[test]
    fn compacting_the_area_arena_keeps_every_precedent() {
        // A replaced formula's areas become garbage that `settle` packs away once it outweighs a
        // fifth of the arena. Every formula's precedents have to survive the move.
        let mut wb = Workbook::new();
        let mut calc = Calc::new();
        let rows = 5_000u32;
        for r in 0..rows {
            wb.sheet_mut(0).unwrap().cells.set(CellRef::new(r, 0), Cell::value(Value::Number(f64::from(r))));
            wb.sheet_mut(0).unwrap().cells.set(CellRef::new(r, 1), Cell::formula(Formula::new(&format!("=SUM(A{0}:B{0})", r + 1))));
        }
        let all: Vec<Key> = (0..rows).map(|r| (0, CellRef::new(r, 1))).collect();
        calc.cells_changed(&mut wb, &all);
        // Every formula replaced by one that reads another cell: the old runs are now garbage.
        for r in 0..rows {
            wb.sheet_mut(0).unwrap().cells.set(CellRef::new(r, 1), Cell::formula(Formula::new(&format!("=A{0}*2", r + 1))));
        }
        calc.cells_changed(&mut wb, &all);
        assert_eq!(calc.graph.len(), rows as usize);
        // One area per formula: the runs of the replaced ones were given back, not kept.
        assert_eq!(calc.graph.areas_len(), rows as usize, "the arena kept the replaced formulas' areas");
        for r in 0..rows {
            let k = (0, CellRef::new(r, 1));
            let one = crate::eval::Area { sheet: 0, range: RangeRef::new(CellRef::new(r, 0), CellRef::new(r, 0)) };
            assert_eq!(calc.graph.precedents_of(k), vec![one], "row {r} lost its precedent");
        }
        let mut deps = Vec::new();
        calc.graph.dependents(0, CellRef::new(3, 0), &mut deps);
        assert_eq!(deps, vec![(0, CellRef::new(3, 1))]);
        assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(3, 1)), Value::Number(6.0));
        // The cell the replaced formula used to read is not a precedent any more.
        deps.clear();
        calc.graph.dependents(0, CellRef::new(3, 1), &mut deps);
        assert!(deps.is_empty(), "{deps:?}");
    }

    #[test]
    fn replacing_an_array_formula_drops_its_range() {
        // A legacy (Ctrl+Shift+Enter) array's range is a side table of the graph, so replacing the
        // formula has to take it out: left behind, the plain formula that replaced it was still
        // read as an array anchor that wrote the cells below it.
        let mut wb = Workbook::new();
        let mut array = Formula::new("=1");
        array.array = RangeRef::parse("A1:A2");
        wb.sheet_mut(0).unwrap().cells.set(CellRef::new(0, 0), Cell::formula(array));
        let mut calc = Calc::new();
        calc.rebuild(&wb);
        let at = (0, CellRef::new(0, 0));
        assert_eq!(calc.graph.legacy_of(at), RangeRef::parse("A1:A2"));
        wb.sheet_mut(0).unwrap().cells.set(CellRef::new(0, 0), Cell::formula(Formula::new("=2")));
        calc.cells_changed(&mut wb, &[at]);
        assert_eq!(calc.graph.legacy_of(at), None, "the graph still holds the array's range");
        assert!(!calc.graph.legacy.contains(&at), "the graph still counts the cell as an array anchor");
    }

    /// `n` rows of `text` in column J, with A1 = 1 and B1 = 2 as inputs. Above the parallel
    /// threshold, every row is an independent leaf.
    fn leaf_book(n: u32, text: &str) -> Workbook {
        let mut wb = Workbook::new();
        if let Some(sh) = wb.sheet_mut(0) {
            sh.cells.set(CellRef::new(0, 0), Cell::value(Value::Number(1.0)));
            sh.cells.set(CellRef::new(0, 1), Cell::value(Value::Number(2.0)));
            for r in 0..n {
                sh.cells.set(CellRef::new(r, 9), Cell::formula(Formula::new(text)));
            }
        }
        wb
    }

    fn can_parallel() -> bool {
        std::thread::available_parallelism().is_ok_and(|n| n.get() > 1)
    }

    fn column_j(wb: &Workbook, n: u32) -> Vec<Value> {
        (0..n).map(|r| wb.sheet(0).unwrap().value(CellRef::new(r, 9))).collect()
    }

    /// A defined name, as a file spells its `refersTo` (no `=`): `scope` is the sheet it is
    /// defined on, `None` for the workbook.
    fn define_name(wb: &mut Workbook, name: &str, scope: Option<usize>, refers: &str) {
        wb.names.push(DefinedName { name: name.into(), scope, formula: refers.into(), comment: String::new(), hidden: false });
    }

    fn put_formula(wb: &mut Workbook, sheet: usize, at: &str, text: &str) {
        let c = CellRef::parse(at).unwrap();
        wb.sheet_mut(sheet).unwrap().cells.set(c, Cell::formula(Formula::new(text)));
    }

    fn put_value(wb: &mut Workbook, sheet: usize, at: &str, v: f64) {
        let c = CellRef::parse(at).unwrap();
        wb.sheet_mut(sheet).unwrap().cells.set(c, Cell::value(Value::Number(v)));
    }

    fn cell_value(wb: &Workbook, sheet: usize, at: &str) -> Value {
        wb.sheet(sheet).unwrap().value(CellRef::parse(at).unwrap())
    }

    /// A two-sheet workbook: Sheet1 and Sheet2.
    fn two_sheets() -> Workbook {
        let mut wb = Workbook::new();
        wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Sheet2")));
        wb
    }

    /// A cached definition is only valid for the text and scope it was resolved from: a name
    /// edited through the model is read with its new definition by the formulas already using
    /// it, and a name moved to a sheet is no longer the workbook's.
    #[test]
    fn an_edited_name_is_read_with_its_new_definition() {
        let mut wb = two_sheets();
        put_value(&mut wb, 0, "B1", 2.0);
        put_value(&mut wb, 0, "C1", 3.0);
        put_formula(&mut wb, 0, "D1", "=Rate*10");
        put_formula(&mut wb, 1, "A1", "=Rate*10");
        define_name(&mut wb, "Rate", None, "Sheet1!$B$1");
        let mut calc = Calc::new();
        calc.recalc_all(&mut wb);
        assert_eq!(cell_value(&wb, 0, "D1").as_f64(), Some(20.0));
        assert_eq!(cell_value(&wb, 1, "A1").as_f64(), Some(20.0));

        // The definition text changes: the next pass reads the new one.
        wb.names[0].formula = "Sheet1!$C$1".into();
        calc.recalc_all(&mut wb);
        assert_eq!(cell_value(&wb, 0, "D1").as_f64(), Some(30.0));
        assert_eq!(cell_value(&wb, 1, "A1").as_f64(), Some(30.0));

        // Its scope too: now Sheet1's own name, so Sheet2 no longer sees it.
        wb.names[0].formula = "Sheet1!$B$1".into();
        wb.names[0].scope = Some(0);
        calc.recalc_all(&mut wb);
        assert_eq!(cell_value(&wb, 0, "D1").as_f64(), Some(20.0));
        assert_eq!(cell_value(&wb, 1, "A1"), Value::Error(CellError::Name));
    }

    /// One name, two sheets, two definitions: a sheet-level name shadows the workbook-level one
    /// on its own sheet only, and both are resolved in the same pass.
    #[test]
    fn a_sheet_level_name_shadows_the_workbook_one_per_sheet() {
        let mut wb = two_sheets();
        put_value(&mut wb, 0, "B1", 2.0);
        put_value(&mut wb, 0, "C1", 5.0);
        define_name(&mut wb, "Rate", None, "Sheet1!$B$1");
        define_name(&mut wb, "Rate", Some(1), "Sheet1!$C$1");
        put_formula(&mut wb, 0, "A1", "=Rate*10");
        put_formula(&mut wb, 1, "A1", "=Rate*10");
        let mut calc = Calc::new();
        calc.recalc_all(&mut wb);
        assert_eq!(cell_value(&wb, 0, "A1").as_f64(), Some(20.0));
        assert_eq!(cell_value(&wb, 1, "A1").as_f64(), Some(50.0));

        // Swap the two definitions: each sheet follows its own.
        wb.names[0].formula = "Sheet1!$C$1".into();
        wb.names[1].formula = "Sheet1!$B$1".into();
        calc.recalc_all(&mut wb);
        assert_eq!(cell_value(&wb, 0, "A1").as_f64(), Some(50.0));
        assert_eq!(cell_value(&wb, 1, "A1").as_f64(), Some(20.0));
    }

    /// A workbook-level name whose definition has no sheet qualifier means the sheet using it:
    /// Sheet1 and Sheet2 each read their own B1, so one name has two answers in one pass.
    #[test]
    fn an_unqualified_workbook_name_means_the_using_sheet() {
        let mut wb = two_sheets();
        put_value(&mut wb, 0, "B1", 2.0);
        put_value(&mut wb, 1, "B1", 7.0);
        define_name(&mut wb, "Rate", None, "$B$1");
        put_formula(&mut wb, 0, "A1", "=Rate*10");
        put_formula(&mut wb, 1, "A1", "=Rate*10");
        let mut calc = Calc::new();
        calc.recalc_all(&mut wb);
        assert_eq!(cell_value(&wb, 0, "A1").as_f64(), Some(20.0));
        assert_eq!(cell_value(&wb, 1, "A1").as_f64(), Some(70.0));
        // And after an edit of either cell the same holds.
        put_value(&mut wb, 1, "B1", 3.0);
        calc.cells_changed(&mut wb, &[(1, CellRef::parse("B1").unwrap())]);
        assert_eq!(cell_value(&wb, 0, "A1").as_f64(), Some(20.0));
        assert_eq!(cell_value(&wb, 1, "A1").as_f64(), Some(30.0));
    }

    /// `Sheet1!Rate` reads the name as it is defined for Sheet1, whatever the using sheet's own
    /// names say: the two spellings are cached apart.
    #[test]
    fn a_sheet_qualified_name_reads_the_other_sheet_s_definition() {
        let mut wb = two_sheets();
        put_value(&mut wb, 0, "B1", 2.0);
        put_value(&mut wb, 0, "C1", 5.0);
        define_name(&mut wb, "Rate", None, "Sheet1!$B$1");
        define_name(&mut wb, "Rate", Some(1), "Sheet1!$C$1");
        put_formula(&mut wb, 1, "A1", "=Rate*10");
        put_formula(&mut wb, 1, "A2", "=Sheet1!Rate*10");
        put_formula(&mut wb, 0, "A1", "=Rate*10");
        let mut calc = Calc::new();
        calc.recalc_all(&mut wb);
        assert_eq!(cell_value(&wb, 1, "A1").as_f64(), Some(50.0));
        assert_eq!(cell_value(&wb, 1, "A2").as_f64(), Some(20.0));
        assert_eq!(cell_value(&wb, 0, "A1").as_f64(), Some(20.0));
    }

    /// A name holding a relative reference is relative to A1, so each cell using it reads the
    /// cell at its own offset: `Step` defined as `B1` reads one column right of the user, as
    /// the same reference written inline does.
    #[test]
    fn a_relative_name_shifts_with_the_cell_that_uses_it() {
        let mut wb = Workbook::new();
        define_name(&mut wb, "Step", None, "B1");
        for r in 10..=20 {
            put_value(&mut wb, 0, &format!("B{r}"), r as f64);
            put_formula(&mut wb, 0, &format!("A{r}"), "=Step");
            put_formula(&mut wb, 0, &format!("C{r}"), &format!("=B{r}"));
        }
        let mut calc = Calc::new();
        calc.recalc_all(&mut wb);
        for r in 10..=20 {
            assert_eq!(cell_value(&wb, 0, &format!("A{r}")), cell_value(&wb, 0, &format!("C{r}")), "row {r}");
            assert_eq!(cell_value(&wb, 0, &format!("A{r}")).as_f64(), Some(r as f64));
        }
    }

    /// Names resolve to names: `Rate` defined as `Step`, `Step` as a cell. A ring is bounded and
    /// shows an error rather than never ending.
    #[test]
    fn a_chained_name_evaluates_and_a_ring_ends() {
        let mut wb = Workbook::new();
        put_value(&mut wb, 0, "B1", 2.0);
        define_name(&mut wb, "Rate", None, "Step");
        define_name(&mut wb, "Step", None, "Sheet1!$B$1");
        define_name(&mut wb, "Loop1", None, "Loop2");
        define_name(&mut wb, "Loop2", None, "Loop1");
        put_formula(&mut wb, 0, "A1", "=Rate*10");
        put_formula(&mut wb, 0, "A2", "=Loop1");
        let mut calc = Calc::new();
        calc.recalc_all(&mut wb);
        assert_eq!(cell_value(&wb, 0, "A1").as_f64(), Some(20.0));
        // The ring runs into the evaluator's nesting limit: `#NUM!`, as a formula nested too deep
        // shows (it used to be `#CALC!`).
        assert_eq!(cell_value(&wb, 0, "A2"), Value::Error(CellError::Num));
    }

    /// An unknown name, and one whose definition doesn't parse, are #NAME? — the same error the
    /// evaluator gave when it parsed each definition itself.
    #[test]
    fn an_unknown_name_and_a_definition_that_does_not_parse_are_name_errors() {
        let mut wb = Workbook::new();
        define_name(&mut wb, "Bad", None, "1+");
        put_formula(&mut wb, 0, "A1", "=Nope*2");
        put_formula(&mut wb, 0, "A2", "=Bad*2");
        put_formula(&mut wb, 0, "A3", "=Bad");
        put_formula(&mut wb, 0, "A4", "=Sheet9!Nope");
        let mut calc = Calc::new();
        calc.recalc_all(&mut wb);
        for at in ["A1", "A2", "A3", "A4"] {
            assert_eq!(cell_value(&wb, 0, at), Value::Error(CellError::Name), "{at}");
        }
    }

    #[test]
    fn independent_leaves_recalc_on_threads() {
        let n = 17_000u32;
        let mut wb = leaf_book(n, "=$A$1*2");
        let mut calc = Calc::new();
        calc.recalc_all(&mut wb);
        if can_parallel() {
            assert!(calc.parallel_runs > 0, "the parallel path did not run");
        }
        assert!(column_j(&wb, n).iter().all(|v| *v == Value::Number(2.0)));
        wb.sheet_mut(0).unwrap().cells.set(CellRef::new(0, 0), Cell::value(Value::Number(10.0)));
        calc.cells_changed(&mut wb, &[(0, CellRef::new(0, 0))]);
        if can_parallel() {
            assert!(calc.parallel_runs > 1, "the edit did not run the parallel path");
        }
        assert!(column_j(&wb, n).iter().all(|v| *v == Value::Number(20.0)));
    }

    /// A column of `=Rate*` formulas is as parallel as the same column written with the
    /// reference the name stands for: a static name is a precedent like any other, not one of
    /// the formulas that keep the whole pass sequential.
    #[test]
    fn a_static_name_does_not_keep_the_pass_sequential() {
        let n = 17_000u32;
        let mut named = leaf_book(n, "=Rate*2");
        named.names.push(DefinedName { name: "Rate".into(), scope: None, formula: "Sheet1!$A$1".into(), comment: String::new(), hidden: false });
        let mut plain = leaf_book(n, "=$A$1*2");
        let mut calc = Calc::new();
        calc.recalc_all(&mut named);
        if can_parallel() {
            assert!(calc.parallel_runs > 0, "the parallel path did not run for a static name");
        }
        calc.recalc_all(&mut plain);
        assert_eq!(column_j(&named, n), column_j(&plain, n));
        // An edit of the cell the name names: the same formulas on threads, the same output.
        named.sheet_mut(0).unwrap().cells.set(CellRef::new(0, 0), Cell::value(Value::Number(10.0)));
        plain.sheet_mut(0).unwrap().cells.set(CellRef::new(0, 0), Cell::value(Value::Number(10.0)));
        calc.parallel_runs = 0;
        calc.cells_changed(&mut named, &[(0, CellRef::new(0, 0))]);
        if can_parallel() {
            assert!(calc.parallel_runs > 0, "the edit did not run the parallel path for a static name");
        }
        calc.cells_changed(&mut plain, &[(0, CellRef::new(0, 0))]);
        assert_eq!(column_j(&named, n), column_j(&plain, n));
        assert!(column_j(&named, n).iter().all(|v| *v == Value::Number(20.0)));
    }

    #[test]
    fn independent_range_leaves_recalc_on_threads() {
        let n = 17_000u32;
        let mut wb = leaf_book(n, "=SUM($A$1:$B$1)");
        let mut calc = Calc::new();
        calc.recalc_all(&mut wb);
        if can_parallel() {
            assert!(calc.parallel_runs > 0, "the parallel path did not run");
        }
        assert!(column_j(&wb, n).iter().all(|v| *v == Value::Number(3.0)));
        wb.sheet_mut(0).unwrap().cells.set(CellRef::new(0, 0), Cell::value(Value::Number(4.0)));
        calc.cells_changed(&mut wb, &[(0, CellRef::new(0, 0))]);
        assert!(column_j(&wb, n).iter().all(|v| *v == Value::Number(6.0)));
    }

    /// Writing the first wave makes the next layer independent: the pass repeats, so a layered
    /// sheet parallelises level by level rather than falling back to one sequential pass.
    #[test]
    fn a_second_wave_parallelises_after_the_first() {
        let n = 17_000u32;
        let mut wb = leaf_book(n, "=$A$1*2");
        if let Some(sh) = wb.sheet_mut(0) {
            for r in 0..n {
                sh.cells.set(CellRef::new(r, 10), Cell::formula(Formula::new(&format!("=J{}+1", r + 1))));
            }
        }
        let mut calc = Calc::new();
        calc.recalc_all(&mut wb);
        if can_parallel() {
            assert!(calc.parallel_runs >= 2, "the second wave did not run on threads");
        }
        for r in 0..n {
            assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(r, 9)), Value::Number(2.0));
            assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(r, 10)), Value::Number(3.0));
        }
    }

    /// Clearing a blocked array's cell along with a bulk change (Remove Duplicates, a paste, a
    /// whole-row clear) still finds the anchor: the blocked-anchor scan covers the union of the
    /// changed cells' 64×64 blocks once.
    #[test]
    fn a_bulk_change_unblocks_a_blocked_array() {
        let mut wb = Workbook::new();
        {
            let sh = wb.sheet_mut(0).expect("sheet");
            sh.cells.set(CellRef::new(0, 0), Cell::value(Value::Number(1.0)));
            sh.cells.set(CellRef::new(1, 0), Cell::value(Value::Number(2.0)));
            sh.cells.set(CellRef::new(0, 10), Cell::formula(Formula::new("=A1:A2")));
            sh.cells.set(CellRef::new(1, 10), Cell::value(Value::Number(3.0)));
            for r in 0..1_000 {
                sh.cells.set(CellRef::new(r, 1), Cell::value(Value::Number(4.0)));
            }
        }
        let mut calc = Calc::new();
        calc.recalc_all(&mut wb);
        assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(0, 10)), Value::Error(CellError::Spill));
        let mut changed = vec![(0, CellRef::new(1, 10))];
        wb.sheet_mut(0).unwrap().cells.remove(CellRef::new(1, 10));
        for r in 0..1_000 {
            wb.sheet_mut(0).unwrap().cells.remove(CellRef::new(r, 1));
            changed.push((0, CellRef::new(r, 1)));
        }
        calc.cells_changed(&mut wb, &changed);
        assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(0, 10)), Value::Number(1.0));
        assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(1, 10)), Value::Number(2.0));
    }

    #[test]
    fn a_dynamic_formula_keeps_the_pass_sequential() {
        let n = 17_000u32;
        let mut wb = leaf_book(n, "=$A$1*2");
        wb.sheet_mut(0).unwrap().cells.set(CellRef::new(0, 10), Cell::formula(Formula::new("=RAND()")));
        let mut calc = Calc::new();
        calc.recalc_all(&mut wb);
        assert_eq!(calc.parallel_runs, 0, "a volatile formula must keep the pass sequential");
        assert!(column_j(&wb, n).iter().all(|v| *v == Value::Number(2.0)));
        assert!(matches!(wb.sheet(0).unwrap().value(CellRef::new(0, 10)), Value::Number(_)));
    }

    #[test]
    fn an_array_formula_keeps_the_pass_sequential() {
        let n = 17_000u32;
        let mut wb = leaf_book(n, "=$A$1*2");
        wb.sheet_mut(0).unwrap().cells.set(CellRef::new(1, 0), Cell::value(Value::Number(2.0)));
        wb.sheet_mut(0).unwrap().cells.set(CellRef::new(0, 10), Cell::formula(Formula::new("=A1:A2")));
        let mut calc = Calc::new();
        calc.recalc_all(&mut wb);
        assert_eq!(calc.parallel_runs, 0, "an array formula must keep the pass sequential");
        assert!(column_j(&wb, n).iter().all(|v| *v == Value::Number(2.0)));
        assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(0, 10)), Value::Number(1.0));
        assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(1, 10)), Value::Number(2.0));
    }

    /// A formula at the nesting limit reading thousands of formulas not calculated yet suspends the
    /// evaluations in progress once, not once per formula read.
    #[test]
    fn wide_read_at_the_nesting_limit_suspends_once() {
        const WIDTH: u32 = 3000;
        let mut wb = Workbook::new();
        let mut exprs = HashMap::default();
        let mut put = |wb: &mut Workbook, at: String, text: String| {
            let at = CellRef::parse(&at).unwrap();
            wb.sheet_mut(0).unwrap().cells.set(at, Cell::formula(Formula::new(&text)));
            exprs.insert((0, at), std::sync::Arc::new(gridcraft_formula::parse(&text[1..]).unwrap()));
        };
        // A1 reads A2, ..., A64 (the 64th formula deep) reads a wide range of formulas.
        for r in 1..64 {
            put(&mut wb, format!("A{r}"), format!("=A{}+1", r + 1));
        }
        put(&mut wb, "A64".to_string(), format!("=SUM(B1:B{WIDTH})"));
        for r in 1..=WIDTH {
            put(&mut wb, format!("B{r}"), format!("={r}*2"));
        }
        let mut rng = 1u64;
        let graph = Graph::default();
        let legacy = HashMap::default();
        let mut names = crate::eval::NameCache::default();
        let mut host = PassHost::new(&wb, &graph, &exprs, &legacy, &mut names, &mut rng, 0.0);
        host.calculate((0, CellRef::parse("A1").unwrap()));
        let total = (WIDTH * (WIDTH + 1)) as f64;
        assert_eq!(host.results.get(&(0, CellRef::parse("A1").unwrap())).and_then(|s| s.value.as_ref()), Some(&Value::Number(total + 63.0)));
        assert!(host.cycles.is_empty());
        assert!(host.suspensions <= 3, "{} suspensions", host.suspensions);
    }

    /// For tests of `PassHost::visit_range`, which can't be reached from outside this module: the
    /// values it hands over for `range` while the formulas in `pending` (cell, formula) have not been
    /// calculated yet, or `None` when it declines the block.
    pub(crate) fn probe_visit_range(wb: &Workbook, pending: &[(&str, &str)], range: RangeRef) -> Option<Vec<Value>> {
        let mut exprs = HashMap::default();
        for (at, formula) in pending {
            let expr = gridcraft_formula::parse(formula.strip_prefix('=').unwrap_or(formula)).ok()?;
            exprs.insert((0, CellRef::parse(at)?), std::sync::Arc::new(expr));
        }

        // A cell in `pending` is calculated the first time something reads it, which is the state
        // of a real pass partway through; `visit_range` has to cope with the formulas it reads
        // being calculated under it.
        let mut rng = 1u64;
        let graph = Graph::default();
        let legacy = HashMap::default();
        let mut names = crate::eval::NameCache::default();
        let mut host = PassHost::new(wb, &graph, &exprs, &legacy, &mut names, &mut rng, 0.0);

        let mut seen = Vec::new();
        host.visit_range(0, range, &mut |v| {
            seen.push(v.clone());
            true
        })
        .then_some(seen)
    }
}
