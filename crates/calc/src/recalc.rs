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

mod graph;
mod pass_host;

use graph::{BULK_EDIT, HashMap, HashSet, Node};
pub use graph::{FxHasher, Graph, GraphMemory, Key};
use pass_host::{PassHost, Slot, Trees, holds_content, spilled_nothing, spills_as_before};

use cella_core::{CellError, CellRef, RangeRef, Value};
use cella_formula::{Expr, UnOp};
use cella_model::{CalcMode, Workbook};
use serde::Serialize;

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
    /// The shapes the last rebuild had to make for formulas that arrived as text alone, with the
    /// cell each belongs to. A document read back from its own saved form or from a JSON document
    /// keeps a formula's text alone (`Formula::shape` is not written anywhere), so after a rebuild
    /// these are handed back to their cells: the writer, a sort, a fill and a paste then share one
    /// shape per family instead of parsing the text once each. Empty for a workbook whose cells
    /// already carry a shape or a tree.
    unshared: Vec<(Key, std::sync::Arc<cella_formula::Shape>)>,
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
    /// Runs of chain formulas the passes so far accumulated (tests check that the fast path is
    /// taken, and that a shape it does not cover stays out of it).
    #[cfg(test)]
    pub(crate) chains: u64,
    /// The formulas those runs covered.
    #[cfg(test)]
    pub(crate) chain_formulas: u64,
    /// Whether the pass may accumulate a run of chain formulas. Tests turn it off to calculate the
    /// same workbook through the general evaluator and hold the two to the same values.
    #[cfg(test)]
    pub(crate) chain_fast: bool,
    /// Stored spill ranges the pass examined deciding whether an array spills over another
    /// formula's spill: once each to index them, plus the index entries lookups examined (tests
    /// check that M blocked arrays over N stored spills cost N + M, not N × M).
    #[cfg(test)]
    block_probes: u64,
    #[cfg(test)]
    pub(crate) rebuilds: usize,
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
            #[cfg(test)]
            rebuilds: 0,
            #[cfg(test)]
            chains: 0,
            #[cfg(test)]
            chain_formulas: 0,
            #[cfg(test)]
            chain_fast: true,
        }
    }
}

/// The live heap bytes of a [`Calc`], by allocation class: what [`Calc::memory`] returns.
///
/// Every figure is a `len`/`capacity`/*`size_of`* product (see [`GraphMemory`] for the graph's own).
/// The one walk is the trees in [`CalcMemory::unshared`] that nothing else holds.
///
/// Not counted: the work a recalculation does while it runs — the pass's slots, dirty bits, spill
/// index and block cache live in the pass (`PassHost`, `Trees`), which is dropped when it ends, so
/// a [`Calc`] at rest holds none of them ([`CalcMemory::pass`] is that zero).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct CalcMemory {
    /// The dependency graph.
    pub graph: GraphMemory,
    /// The cells whose formulas have cycle errors.
    pub circular: usize,
    /// The buffer `precedents_into` fills while a formula is linked into the graph.
    pub scratch_areas: usize,
    /// The formulas the last rebuild parsed whose text their cells still held alone, and the
    /// trees it parsed: charged here only while nothing else holds the tree (a tree the cells share
    /// is charged by the store's report).
    pub unshared: usize,
    /// The resolved-name cache.
    pub names: usize,
    /// Slots and dirty bits of a pass in progress: zero at rest, because the pass owns them.
    pub pass: usize,
}

impl CalcMemory {
    pub fn total(&self) -> usize {
        [self.graph.total(), self.circular, self.scratch_areas, self.unshared, self.names, self.pass]
            .iter()
            .fold(0usize, |sum, part| sum.saturating_add(*part))
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
fn may_return_array(names: &mut crate::eval::NameCache, wb: &Workbook, sheet: usize, e: &Expr, base: Option<CellRef>) -> bool {
    may_return_array_at(names, wb, sheet, e, base, 0)
}

/// [`may_return_array`] at name depth `depth`. Only a name recurses (into its definition, at
/// most `MAX_NAME_DEPTH` deep): the formula tree itself is walked with an explicit stack, as a
/// long operator chain must not recurse once per operand.
fn may_return_array_at(names: &mut crate::eval::NameCache, wb: &Workbook, sheet: usize, root: &Expr, base: Option<CellRef>, depth: usize) -> bool {
    let mut stack = crate::walk::Stack::new(root);
    while let Some(e) = stack.pop() {
        match e {
            Expr::Array(_) => return true,
            // One reference's size, read where the formula lives (a shape's offsets are sizes
            // wherever it resolves). A reference that leaves the sheet reads nothing.
            Expr::Ref(r) => match base {
                Some(base) => {
                    if r.range_at(base).is_none_or(|range| !range.is_single()) {
                        return true;
                    }
                }
                None => {
                    if !r.range().is_single() {
                        return true;
                    }
                }
            },
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
                    if may_return_array_at(names, wb, scope, &def, None, depth + 1) {
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
const PARALLEL_MAX_THREADS: usize = 8;

/// Independent formulas per thread a wave needs before splitting: below this, starting the threads
/// and building one slot table per thread costs more than the pass (measured on a 1M-row sheet: some
/// two thousand formulas a thread pay for themselves).
#[cfg(not(target_arch = "wasm32"))]
const PARALLEL_MIN_PER_THREAD: usize = 2_048;

/// The machine's threads, capped, or 1 where they cannot be had (wasm).
fn parallel_threads() -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(PARALLEL_MAX_THREADS)
    }
    #[cfg(target_arch = "wasm32")]
    {
        1
    }
}

/// Independent formulas a pass needs before splitting across `threads` threads.
#[cfg(not(target_arch = "wasm32"))]
fn parallel_min(threads: usize) -> usize {
    PARALLEL_MIN_PER_THREAD.saturating_mul(threads.max(1))
}

/// Most cells the dense map below will index; past this a hash set is smaller than the box.
#[cfg(not(target_arch = "wasm32"))]
const MAX_DIRTY_BITS: usize = 1 << 27;

/// Where a pass's dirty formulas are: the leaf scan asks it whether a precedent cell is one the pass
/// is recalculating, once per cell of every candidate's precedent blocks.
///
/// A dirty set is a block — the formulas of a column, a filled range, a copied table — so one bit
/// per cell of its bounding box a sheet answers that test with two compares and a shift, where a
/// hash entry per key cost an insert to build and a cache miss per probe. On a million-formula pass
/// gathering the set cost a third of the scan it fed. A sheet whose dirty cells are spread wider
/// than `MAX_DIRTY_BITS` cells (a paste over a whole sheet) falls back to a hash set, where a probe
/// costs what it used to.
#[cfg(not(target_arch = "wasm32"))]
struct DirtyCells {
    boxes: HashMap<usize, DirtyBox>,
    fallback: Option<HashSet<Key>>,
}

#[cfg(not(target_arch = "wasm32"))]
struct DirtyBox {
    row: u32,
    col: u32,
    height: u32,
    width: u32,
    bits: Vec<u64>,
}

#[cfg(not(target_arch = "wasm32"))]
impl DirtyCells {
    fn build(dirty: &[Key]) -> Self {
        // The bounding box of each sheet's dirty cells first, so the map is sized once.
        let mut bounds: HashMap<usize, (u32, u32, u32, u32)> = HashMap::default();
        for &(sheet, c) in dirty {
            match bounds.get_mut(&sheet) {
                Some(b) => {
                    b.0 = b.0.min(c.row);
                    b.1 = b.1.max(c.row);
                    b.2 = b.2.min(c.col);
                    b.3 = b.3.max(c.col);
                }
                None => {
                    bounds.insert(sheet, (c.row, c.row, c.col, c.col));
                }
            }
        }
        let cells = bounds.values().fold(0usize, |n, (r0, r1, c0, c1)| n.saturating_add((r1 - r0 + 1) as usize * (c1 - c0 + 1) as usize));
        if cells > MAX_DIRTY_BITS {
            return Self { boxes: HashMap::default(), fallback: Some(dirty.iter().copied().collect()) };
        }
        let mut boxes: HashMap<usize, DirtyBox> = HashMap::default();
        for (&sheet, &(r0, r1, c0, c1)) in &bounds {
            let (height, width) = (r1 - r0 + 1, c1 - c0 + 1);
            let bits = vec![0u64; (height as usize * width as usize).div_ceil(64)];
            boxes.insert(sheet, DirtyBox { row: r0, col: c0, height, width, bits });
        }
        for &(sheet, c) in dirty {
            if let Some(b) = boxes.get_mut(&sheet) {
                b.set(c);
            }
        }
        Self { boxes, fallback: None }
    }

    fn contains(&self, sheet: usize, c: CellRef) -> bool {
        match &self.fallback {
            Some(cells) => cells.contains(&(sheet, c)),
            None => self.boxes.get(&sheet).is_some_and(|b| b.contains(c)),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl DirtyBox {
    fn set(&mut self, c: CellRef) {
        let (dr, dc) = (c.row - self.row, c.col - self.col);
        let i = dr as usize * self.width as usize + dc as usize;
        if let Some(w) = self.bits.get_mut(i / 64) {
            *w |= 1 << (i % 64);
        }
    }

    fn contains(&self, c: CellRef) -> bool {
        if c.row < self.row || c.col < self.col {
            return false;
        }
        let (dr, dc) = (c.row - self.row, c.col - self.col);
        if dr >= self.height || dc >= self.width {
            return false;
        }
        let i = dr as usize * self.width as usize + dc as usize;
        self.bits.get(i / 64).is_some_and(|w| w >> (i % 64) & 1 == 1)
    }

    /// Whether `c` is inside the box at all (set or not), the test that says whether [`Self::set`]
    /// would reach its bit.
    fn in_bounds(&self, c: CellRef) -> bool {
        c.row >= self.row && c.col >= self.col && c.row - self.row < self.height && c.col - self.col < self.width
    }

    /// The box of the single cell `c`: what a walk of a sheet starts from.
    fn at(c: CellRef) -> Self {
        let mut boxed = DirtyBox { row: c.row, col: c.col, height: 1, width: 1, bits: vec![0u64; 1] };
        boxed.set(c);
        boxed
    }

    /// Calls `f` for each cell the box holds set (the cells a sheet had when its box was dropped).
    fn for_each_set(&self, mut f: impl FnMut(CellRef)) {
        let width = self.width as usize;
        if width == 0 {
            return;
        }
        for (word, bits) in self.bits.iter().enumerate() {
            let mut rest = *bits;
            while rest != 0 {
                let bit = rest.trailing_zeros() as usize;
                rest &= rest - 1;
                let i = word.saturating_mul(64).saturating_add(bit);
                let (dr, dc) = (u32::try_from(i / width).unwrap_or(u32::MAX), u32::try_from(i % width).unwrap_or(u32::MAX));
                f(CellRef::new(self.row.saturating_add(dr), self.col.saturating_add(dc)));
            }
        }
    }

    /// Grows the box around `c`, keeping the cells of `sheet` in `keys` set, and answers whether
    /// it could: a box that would have to cover more than `MAX_DIRTY_BITS` cells is refused, and
    /// the caller keeps its cells another way.
    ///
    /// The sides double, because the walk this serves adds rows to a column one at a time: each
    /// growth pays for the cells already in the box and leaves room for as many again, so a walk
    /// of a million cells copies them a couple of million times rather than a billion.
    fn grow(&mut self, c: CellRef, keys: &[Key], sheet: usize) -> bool {
        let row = self.row.min(c.row);
        let col = self.col.min(c.col);
        let bottom = self.row.saturating_add(self.height.saturating_sub(1)).max(c.row);
        let right = self.col.saturating_add(self.width.saturating_sub(1)).max(c.col);
        let (need_h, need_w) = (bottom - row + 1, right - col + 1);
        let area = |h: u32, w: u32| (h as usize).saturating_mul(w as usize);
        // The side that has to grow doubles; the other keeps its size, so a walk down a column does
        // not make the box wider than the column is.
        let mut height = if need_h > self.height { need_h.max(self.height.saturating_mul(2)) } else { self.height };
        let mut width = if need_w > self.width { need_w.max(self.width.saturating_mul(2)) } else { self.width };
        if area(height, width) > MAX_DIRTY_BITS {
            // The doubling is what has to give, not the cell that is out of the box: as much of the
            // dense-map budget as the sides can use, and a hash only when not even the cells the
            // walk has visited fit in a box.
            let rows = (MAX_DIRTY_BITS / (width as usize).max(1)).max(need_h as usize);
            height = u32::try_from(rows).unwrap_or(need_h);
            let cols = (MAX_DIRTY_BITS / (height as usize).max(1)).max(need_w as usize);
            width = u32::try_from(cols).unwrap_or(need_w);
            if area(height, width) > MAX_DIRTY_BITS {
                return false;
            }
        }
        let mut grown = DirtyBox { row, col, height, width, bits: vec![0u64; area(height, width).div_ceil(64)] };
        for &(s, k) in keys {
            if s == sheet {
                grown.set(k);
            }
        }
        grown.set(c);
        *self = grown;
        true
    }
}

/// The cells a dependents walk has visited: a bit each where a sheet's cells fit a dense box (a
/// chain walks a column, where a box is the cheapest set there is), a hash set where they do not.
///
/// The walk asks this once for every dependent it finds, so it has to cost less than the hash set
/// it replaced: a compare, a shift and a mask while the cells stay in the box, where a `Key` in a
/// hash table was a hash of two words and a cache miss. The box grows by doubling (see
/// [`DirtyBox::grow`]), so the rows a chain adds below the edited cell cost a copy per doubling,
/// not per row.
#[derive(Default)]
struct SeenCells {
    /// Every cell the walk visited, in the order it visited them: the closure's answer.
    keys: Vec<Key>,
    /// Each sheet's box, or that its cells went to `loose` once a box would have grown past
    /// `MAX_DIRTY_BITS` (a paste over a whole sheet).
    #[cfg(not(target_arch = "wasm32"))]
    sheets: HashMap<usize, SheetSeen>,
    /// The visited cells no box covers — every cell of them where a box is not built at all (wasm).
    loose: HashSet<Key>,
}

#[cfg(not(target_arch = "wasm32"))]
enum SheetSeen {
    Boxed(DirtyBox),
    Loose,
}

/// What a box did with a cell.
#[cfg(not(target_arch = "wasm32"))]
enum Marked {
    /// The box did not hold it and does now.
    New,
    /// The box already held it.
    Seen,
    /// No box covers the sheet: the caller decides with `loose`.
    Loose,
}

impl SeenCells {
    /// Marks `k` visited, answering whether it was new.
    fn mark(&mut self, k: Key) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        match self.mark_boxed(k) {
            Marked::New => {
                self.keys.push(k);
                return true;
            }
            Marked::Seen => return false,
            Marked::Loose => {}
        }
        if self.loose.insert(k) {
            self.keys.push(k);
            true
        } else {
            false
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn mark_boxed(&mut self, k: Key) -> Marked {
        // Where the cell stands is answered before anything is written, because the table it asks
        // is borrowed either way; a sheet the walk has not seen yet starts a box of its own cell.
        let place = match self.sheets.get(&k.0) {
            None => Place::First,
            Some(SheetSeen::Loose) => Place::Loose,
            Some(SheetSeen::Boxed(boxed)) => {
                if boxed.contains(k.1) {
                    Place::Seen
                } else if boxed.in_bounds(k.1) {
                    Place::Inside
                } else {
                    Place::Outside
                }
            }
        };
        match place {
            Place::Seen => Marked::Seen,
            Place::Loose => Marked::Loose,
            Place::First => {
                self.sheets.insert(k.0, SheetSeen::Boxed(DirtyBox::at(k.1)));
                Marked::New
            }
            Place::Inside => match self.sheets.get_mut(&k.0) {
                Some(SheetSeen::Boxed(boxed)) => {
                    boxed.set(k.1);
                    Marked::New
                }
                // The table cannot have changed between the two lookups; a hash is still right.
                _ => Marked::Loose,
            },
            Place::Outside => {
                let grown = match self.sheets.get_mut(&k.0) {
                    Some(SheetSeen::Boxed(boxed)) => boxed.grow(k.1, &self.keys, k.0),
                    _ => false,
                };
                if grown {
                    Marked::New
                } else {
                    // Too wide to box: this sheet's cells go to the hash set from here on — the
                    // ones the box already holds included, since the walk asks for them again and
                    // a cell it marks twice is a cell it visits twice.
                    if let Some(SheetSeen::Boxed(old)) = self.sheets.insert(k.0, SheetSeen::Loose) {
                        old.for_each_set(|c| {
                            self.loose.insert((k.0, c));
                        });
                    }
                    Marked::Loose
                }
            }
        }
    }

    /// The visited cells, in the order the walk found them.
    fn into_keys(self) -> Vec<Key> {
        self.keys
    }
}

/// Where a cell stands with the box of its sheet.
#[cfg(not(target_arch = "wasm32"))]
enum Place {
    First,
    Seen,
    Inside,
    Outside,
    Loose,
}

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

/// Writes a pass's results back, moving every value out of whatever holds it: `results` arrive in
/// row-major order (sheet, then row, then column), so a run of one sheet is one band-ordered
/// batch and the store is handed exactly the batches a whole-list write would have given it.
///
/// The values are *moved*, never cloned, and `batch` is the caller's scratch, so a million-value
/// write-back allocates its temporary once per pass instead of once per sheet, and a pass that
/// streams its slots in here builds no list of its own. Returns how many values it wrote.
fn write_values<I>(wb: &mut Workbook, results: I, batch: &mut Vec<(CellRef, Value)>, max: usize) -> usize
where
    I: Iterator<Item = (Key, Value)>,
{
    // One allocation for the whole pass: a batch that grows by doubling copies 16 MB at 1M
    // results, and the buffer is handed from one flush to the next either way.
    batch.reserve(max);
    let mut written = 0;
    let mut sheet: Option<usize> = None;
    #[cfg(debug_assertions)]
    let mut last: Option<Key> = None;
    for (k, v) in results {
        #[cfg(debug_assertions)]
        {
            // The store walks each batch in band order and relies on it being sorted (it checks
            // that itself); a pass that broke the order is a bug in the pass, caught here.
            debug_assert!(last.is_none_or(|l| (l.0, l.1.row, l.1.col) <= (k.0, k.1.row, k.1.col)), "a pass's results must be in row-major order");
            last = Some(k);
        }
        if sheet != Some(k.0) {
            if let Some(si) = sheet {
                flush_values(wb, si, batch);
            }
            sheet = Some(k.0);
        }
        batch.push((k.1, v));
        written += 1;
    }
    if let Some(si) = sheet {
        flush_values(wb, si, batch);
    }
    written
}

/// Hands one sheet's batch to its store — a value whose sheet is gone is dropped, as it was when
/// the batches were built per sheet — and takes the buffer back for the next one.
fn flush_values(wb: &mut Workbook, si: usize, batch: &mut Vec<(CellRef, Value)>) {
    if let Some(sheet) = wb.sheet_mut(si) {
        sheet.cells.set_values(batch);
    }
    batch.clear();
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
    fn try_parallel(&mut self, wb: &mut Workbook, dirty: &mut Vec<Key>, batch: &mut Vec<(CellRef, Value)>) -> bool {
        let threads = parallel_threads();
        let min = parallel_min(threads);
        // Counting the dirty formulas against the wave's price first: below it no arrangement of
        // them can pay for the threads, and the scan below (which is what a wave that finds nothing
        // costs) is not run at all. A chain of formulas — each reading the one above it, the shape
        // of a running total — has a million dirty formulas and one leaf; an edit's dirty set is
        // mostly its own dependents, so that shape is the common one.
        if self.parallel_fallback || threads < 2 || dirty.len() < min || wb.calc.iterative {
            return false;
        }
        let cells = DirtyCells::build(dirty);
        // A dynamic or array formula can read or write cells the static graph does not name; leave
        // the whole pass to the sequential path if one is being recalculated. The graph keeps these
        // sets small (they are formulas that can spill or are volatile), so checking them costs a
        // handful of lookups, not a scan of every formula.
        if self.graph.dynamic.iter().any(|k| cells.contains(k.0, k.1))
            || self.graph.may_spill.iter().any(|k| cells.contains(k.0, k.1))
            || self.graph.legacy.iter().any(|k| cells.contains(k.0, k.1))
        {
            return false;
        }
        let chunk = dirty.len().div_ceil(threads);
        let now = self.now();
        let seed = self.rng;
        let graph = &self.graph;
        let wb_ref: &Workbook = wb;
        let cells = &cells;
        let started = prof_now();
        let shards: Vec<std::thread::Result<Shard>> = std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for (ti, part) in dirty.chunks(chunk).enumerate() {
                handles.push(scope.spawn(move || {
                    let mut local: Vec<Key> = Vec::new();
                    for &k in part {
                        let Some(node) = graph.node(k) else { continue };
                        let mut leaf = true;
                        for area in graph.areas_of(node) {
                            if area.range().count() > PARALLEL_LEAF_CELLS {
                                leaf = false;
                                break;
                            }
                            for c in area.range().iter() {
                                if cells.contains(area.sheet(), c) {
                                    leaf = false;
                                    break;
                                }
                            }
                            if !leaf {
                                break;
                            }
                        }
                        if leaf && node.has_formula() {
                            local.push(k);
                        }
                    }
                    if local.is_empty() {
                        return Shard { values: Vec::new(), clean: true };
                    }
                    // A shard keeps slot tables of its own: the values cannot be shared (the pass's
                    // are a `&mut` borrow its threads do not have), and its leaves only read cells
                    // outside the dirty set anyway.
                    let mut trees = Trees::for_cells(&local);
                    let mut legacy: HashMap<Key, RangeRef> = HashMap::default();
                    // The sheet's node table is found once for the run of its cells (see
                    // `Graph::nodes_of`).
                    for group in local.chunk_by(|a, b| a.0 == b.0) {
                        let Some(sheet) = group.first().map(|k| k.0) else { continue };
                        let nodes = graph.nodes_of(sheet);
                        for &k in group {
                            let Some(node) = nodes.and_then(|nodes| nodes.get(&k.1)) else { continue };
                            trees.push(k, node);
                            if let Some(r) = graph.legacy_of(k) {
                                legacy.insert(k, r);
                            }
                        }
                    }
                    let mut rng = seed ^ (ti as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
                    // A shard keeps a cache of its own: the pass's is a `&mut` borrow, which the
                    // threads can't share, and a shard resolves each name once at most anyway.
                    let mut names = crate::eval::NameCache::default();
                    let mut host = PassHost::new(wb_ref, graph, &trees, &legacy, &mut names, &mut rng, now);
                    host.iterate = false;
                    for (slot, k) in trees.iter() {
                        host.calculate_at(k, slot);
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
                    let values: Vec<(Key, Value)> = trees.keys().filter_map(|k| host.value(k).map(|v| (k, v.clone()))).collect();
                    let clean = clean && values.len() == local.len();
                    Shard { values, clean }
                }));
            }
            handles.into_iter().map(|h| h.join()).collect()
        });
        let eval_ms = prof_now() - started;
        let total: usize = shards.iter().filter_map(|r| r.as_ref().ok()).map(|s| s.values.len()).sum();
        if std::env::var_os("CELLA_PROFILE").is_some() {
            eprintln!("  wave: {} dirty, {total} leaves on {threads} threads (min {min}), {eval_ms:.1} ms", dirty.len());
        }
        if total < min {
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
        // The shards' values are moved into the store; their keys stay for the merge below.
        let max = values.len();
        write_values(wb, values.iter_mut().map(|(k, v)| (*k, std::mem::take(v))), batch, max);
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
        if std::env::var_os("CELLA_PROFILE").is_some() {
            eprintln!("  parallel {total} formulas on {threads} threads: eval {eval_ms:.1} ms, write {write_ms:.1} ms");
        }
        true
    }
}

#[cfg(target_arch = "wasm32")]
impl Calc {
    fn try_parallel(&mut self, _wb: &mut Workbook, _dirty: &mut Vec<Key>, _batch: &mut Vec<(CellRef, Value)>) -> bool {
        false
    }
}

impl Calc {
    pub fn new() -> Self {
        Self::default()
    }

    /// The shape the graph evaluates at `k`, for the test that holds a column of one `=A1*2` fill
    /// to one tree.
    #[cfg(test)]
    pub(crate) fn node_shape(&self, k: Key) -> Option<std::sync::Arc<cella_formula::Shape>> {
        self.graph.node_shape(k)
    }

    /// The live heap bytes this calculation state holds, by allocation class. Nothing here walks the
    /// graph's tables: the sizes come from capacities (see [`GraphMemory`]), so a UI can ask on a
    /// million-formula workbook. The one exception is the shapes this state alone holds in
    /// `unshared` — the trees a rebuild parsed for cells that arrived as text alone, which no cell
    /// holds a pointer to — and they are charged once per pointer, not walked
    /// ([`CalcMemory::unshared`]).
    pub fn memory(&self) -> CalcMemory {
        use std::mem::size_of;
        let mut unshared_trees = 0usize;
        let mut seen: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
        for (_, shape) in &self.unshared {
            // A shape counts its own tree (one `size_of::<Expr>()` a node, the figure the store
            // charges), so there is nothing to walk. The interner hands one `Arc` to a million
            // cells of one shape, so the pointers are deduplicated: one tree, one charge.
            if seen.insert(std::sync::Arc::as_ptr(shape) as usize) {
                unshared_trees = unshared_trees.saturating_add(shape.bytes as usize);
            }
        }
        CalcMemory {
            graph: self.graph.memory(),
            circular: self.circular.capacity().saturating_mul(size_of::<Key>()),
            scratch_areas: self.scratch_areas.capacity().saturating_mul(size_of::<Area>()),
            unshared: self
                .unshared
                .capacity()
                .saturating_mul(size_of::<(Key, std::sync::Arc<cella_formula::Shape>)>())
                .saturating_add(unshared_trees),
            names: self.names.memory(),
            pass: 0,
        }
    }

    fn now(&self) -> f64 {
        if let Some(n) = self.fixed_now {
            return n;
        }
        now_serial()
    }

    /// Rebuilds the graph from scratch.
    pub fn rebuild(&mut self, wb: &Workbook) {
        #[cfg(test)]
        {
            self.rebuilds += 1;
        }
        self.parallel_fallback = false;
        self.graph = Graph::default();
        self.unshared.clear();
        // The shapes of the formulas that arrived as text alone are interned here, so a document
        // of a million formulas of one shape ends with one — and the nodes of those cells hold the
        // one `Arc`, so a pass resolves nothing it did not resolve here.
        let mut shapes = cella_formula::ShapeTable::default();
        // The shapes this state, not a cell, holds: one entry per distinct `Arc`, for the memory
        // report (a cell that holds a shape is charged by the store).
        let mut kept: std::collections::HashSet<usize> = std::collections::HashSet::default();
        for (si, sheet) in wb.sheets.iter().enumerate() {
            // One pass to learn the size (moving, not cloning, what the store hands out): the
            // graph's tables then grow once instead of rehashing every entry a dozen times.
            let formulas: Vec<_> = sheet.cells.formulas().collect();
            self.graph.reserve(si, formulas.len());
            for (c, f, _) in formulas {
                if let Some((node, keep)) = self.node_for(wb, si, c, &f, &mut shapes) {
                    let areas = std::mem::take(&mut self.scratch_areas);
                    self.graph.insert((si, c), &areas, f.array.as_deref().copied(), node);
                    self.scratch_areas = areas;
                    // The shape a text-only cell's parse made: the node holds it, so a later use
                    // of that formula needs no parse either, and the report charges it here.
                    if let Some(shape) = keep
                        && kept.insert(std::sync::Arc::as_ptr(&shape) as usize)
                    {
                        self.unshared.push(((si, c), shape));
                    }
                }
            }
        }
        self.graph.settle();
    }

    /// The graph node of the formula at `(si, at)`: what the formula reads, its flags, and how the
    /// pass finds the tree it evaluates — the cell's own, or the shape it shares with every cell of
    /// its fill (the node then holds eight bytes where a tree stood).
    ///
    /// Also returns the shape a cell that arrived as text alone should keep: its one parse gives
    /// both the tree to walk for precedents and the shape to intern, which `shapes` deduplicates.
    /// `None` when the text does not parse — the formula has no node at all.
    fn node_for(
        &mut self,
        wb: &Workbook,
        si: usize,
        at: CellRef,
        f: &cella_model::Formula,
        shapes: &mut cella_formula::ShapeTable,
    ) -> Option<(Node, Option<std::sync::Arc<cella_formula::Shape>>)> {
        // The tree this formula reads, and the cell its relative parts are offsets from: a shape's
        // canonical tree, read where it is. Nothing is resolved here: a shape is walked once, in
        // place, for a million cells (S4). A formula that holds no shape arrived as text alone (a
        // JSON document, or `Formula::new`), and `expr_at` is the fallback that parses it at the
        // cell; interning that parse makes the million cells of one shape share one tree.
        let own = f.shape();
        let mut keep: Option<std::sync::Arc<cella_formula::Shape>> = None;
        let (tree, base): (&cella_formula::Expr, Option<CellRef>) = match &own {
            Some(shape) => (shape.tree(), Some(at)),
            None => {
                keep = Some(shapes.intern(cella_formula::Shape::from_expr(f.expr_at(at)?, at)));
                (keep.as_ref()?.tree(), Some(at))
            }
        };
        let reads = precedents_into(&mut self.names, wb, si, tree, base, &mut self.scratch_areas);
        let may_spill = may_return_array(&mut self.names, wb, si, tree, base);
        let shape = match &keep {
            Some(shape) => Some(std::sync::Arc::clone(shape)),
            None => own,
        };
        let node = Node::new(reads.dynamic, may_spill, reads.trim).with_volatile(reads.volatile).with_visibility(reads.hidden_rows).with_shape(shape);
        Some((node, keep))
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
    /// sheets, rows or columns inserted or deleted, or defined names, tables or report definitions
    /// that moved under the formulas, since those decide what a reference means and so what a
    /// formula's precedents are. A store that keeps its bands as compressed pages is compared a
    /// band at a time, decoding the ones the walk reaches (see
    /// [`CellStore::formula_changes_against`]); a caller that already knows the cells that differ
    /// passes them to [`restore_cells`](Self::restore_cells) instead, and decodes none.
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
        let mut shapes = cella_formula::ShapeTable::default();
        for (si, at, formula) in changes {
            // A formula whose text does not parse has no node at all, as in `rebuild`. The shape
            // a text-only formula's parse makes is dropped here: this path has a snapshot to look
            // at, not a workbook to write to (a rebuild is the one that keeps it).
            match formula.as_ref().and_then(|f| self.node_for(to, si, at, f, &mut shapes)) {
                Some((node, _keep)) => {
                    let legacy = formula.as_ref().and_then(|f| f.array.as_deref().copied());
                    let areas = std::mem::take(&mut self.scratch_areas);
                    self.graph.insert((si, at), &areas, legacy, node);
                    self.scratch_areas = areas;
                }
                None => self.graph.remove((si, at)),
            }
        }
        self.graph.settle();
        true
    }

    /// [`restore`](Self::restore) for a caller that already knows which cells' formulas the two
    /// versions disagree on: the undo history records them when a step is spilled to its pages,
    /// while both versions are live and can be compared band by band, so the snapshot's formula for each of
    /// them is all that has to be read here — a band at a time for a snapshot kept as compressed
    /// pages, instead of the whole sheet decoded to find out.
    ///
    /// **Nothing is evaluated**, exactly as [`restore`](Self::restore): the snapshot holds the
    /// values its own calculation produced. `cells` must be every cell whose formula differs
    /// between the two versions, in the coordinates of `to`; a caller that cannot say that (a
    /// structural edit, a defined name that moved under the formulas) restores through
    /// [`restore`](Self::restore), which finds the differences itself or rebuilds.
    pub fn restore_cells(&mut self, to: &Workbook, cells: &[(usize, CellRef)]) -> bool {
        // Whatever happens below, this call evaluates nothing.
        self.last_recalc_cells = 0;
        self.last_restore_cells = Some(cells.len());
        self.parallel_fallback = false;
        let mut shapes = cella_formula::ShapeTable::default();
        for (si, at) in cells {
            let formula = to.sheets.get(*si).and_then(|sheet| sheet.cells.formula(*at));
            // A formula whose text does not parse has no node at all, as in `rebuild`.
            match formula.as_ref().and_then(|f| self.node_for(to, *si, *at, f, &mut shapes)) {
                Some((node, _keep)) => {
                    let legacy = formula.as_ref().and_then(|f| f.array.as_deref().copied());
                    let areas = std::mem::take(&mut self.scratch_areas);
                    self.graph.insert((*si, *at), &areas, legacy, node);
                    self.scratch_areas = areas;
                }
                None => self.graph.remove((*si, *at)),
            }
        }
        self.graph.settle();
        true
    }

    /// The formula cells two versions of a workbook disagree on, with the sheet each is on, or
    /// `None` when they cannot be found cheaply (see [`Calc::restore`]).
    fn formula_changes(from: &Workbook, to: &Workbook) -> Option<Vec<(usize, CellRef, Option<std::sync::Arc<cella_model::Formula>>)>> {
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
        let all: Vec<Key> = self.graph.node_keys().collect();
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
        if wb.calc.full_calc_on_load {
            // The full pass rebuilds its own graph; there is no need to build it twice.
            self.recalc_all(wb);
            return;
        }
        let tp = prof_now();
        self.rebuild(wb);
        let rebuild_ms = prof_now() - tp;
        let ts = prof_now();
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
            if std::env::var_os("CELLA_PROFILE").is_some() {
                eprintln!("  open: rebuild {rebuild_ms:.1} ms, no stale cells ({} ms)", prof_now() - ts);
            }
            return;
        }
        let stale = self.dirty_closure(wb, &seeds);
        if std::env::var_os("CELLA_PROFILE").is_some() {
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
        self.cells_changed_impl(wb, changed, used_changed, false);
    }

    /// [`cells_changed_with_used`](Self::cells_changed_with_used) for an edit that neither added
    /// nor removed a formula at any changed cell — a sort of a block that carries values only. The
    /// graph's nodes are exactly what they were, so the per-cell formula probe and the graph update
    /// are the only work skipped: the cells are still seeds, and their dependents are still
    /// recalculated.
    pub fn cells_changed_values_only(&mut self, wb: &mut Workbook, changed: &[Key], used_changed: &[usize]) {
        self.cells_changed_impl(wb, changed, used_changed, true);
    }

    fn cells_changed_impl(&mut self, wb: &mut Workbook, changed: &[Key], used_changed: &[usize], values_only: bool) {
        self.parallel_fallback = false;
        let mut cleared: Vec<(usize, RangeRef)> = Vec::new();
        // A bulk edit — a fill, a paste of a column of formulas — inserts many nodes at once, and
        // the tables grow a dozen times as it fills, each growth rehashing or copying everything
        // already in them (`Graph::reserve_more`). Growing them once, to what the edit can add, is
        // worth it on a sheet that already holds formulas — and a workbook that holds none (a fill
        // of values) skips it without counting anything.
        if changed.len() >= BULK_EDIT && self.graph.len() * 2 >= changed.len() {
            // At most as many as the graph holds: a fill adds formulas, it does not double a
            // browser's worth of nodes on a sheet whose other cells are values. The node table
            // reserved is the sheet the edit writes to — a bulk edit is a rectangle of one sheet,
            // and that is the table its `insert`s will fill.
            let sheet = changed.first().map_or(0, |k| k.0);
            self.graph.reserve_more(sheet, changed.len().min(self.graph.len()));
        }
        // A bulk edit of a workbook that holds no formula anywhere: the graph has no node to
        // remove (it was built from the workbook before the edit), every spill map is empty, no
        // used range moved, and the new workbook's `has_formulas` walk finds nothing — so every
        // key below would read `None` and change nothing. That walk is band-level, one pass over
        // the stored rows, and it is also the one thing that catches a formula the edit itself
        // just wrote, whose node the per-key walk below would have to insert. Below the bulk
        // threshold an ordinary edit keeps the exact per-key path.
        if changed.len() >= BULK_EDIT
            && used_changed.is_empty()
            && self.graph.is_empty()
            && self.graph.trim.is_empty()
            && wb.sheets.iter().all(|s| s.spill_ranges.is_empty() && s.spill.is_empty())
            && wb.sheets.iter().all(|s| !s.cells.has_formulas())
        {
            self.last_recalc_cells = 0;
            return;
        }
        // Whether the edit put a formula anywhere: when it did not, and the workbook has none
        // stored either, no cell can depend on the change and the pass below has nothing to plan.
        let mut formulas = false;
        let mut shapes = cella_formula::ShapeTable::default();
        if !values_only {
            for &k in changed {
                let formula = wb.sheet(k.0).and_then(|s| s.cells.formula(k.1));
                formulas |= formula.is_some();
                let parsed = formula.as_ref().and_then(|f| self.node_for(wb, k.0, k.1, f, &mut shapes));
                match parsed {
                    Some((node, _keep)) => {
                        let legacy = formula.as_ref().and_then(|f| f.array.as_deref().copied());
                        let areas = std::mem::take(&mut self.scratch_areas);
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
                        // Only an unparseable formula needs its cached value replaced with #NAME?.
                        if formula.is_some()
                            && let Some(sheet) = wb.sheet_mut(k.0)
                            && let Some(mut cell) = sheet.cells.get(k.1)
                            && cell.formula.is_some()
                        {
                            cell.value = Value::Error(CellError::Name);
                            sheet.cells.set(k.1, cell);
                        }
                    }
                }
            }
        }
        self.graph.settle();
        // Nothing can be recalculated: the edit stored no formula, the workbook holds none, no
        // array spills anywhere and no used range moved. Scanning a million changed cells for
        // precedents of a data import used to cost more than writing them.
        if !formulas
            && self.graph.is_empty()
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
            let own: Vec<Key> = changed.iter().copied().filter(|k| self.graph.has_node(*k)).collect();
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
                for (s, c) in self.graph.node_keys() {
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
        // The spill a replaced or cleared formula gave up may be what was blocking another
        // formula's array, and an edit that overwrites the anchor (a paste over a spilled area's
        // corner) is one more way that owner disappears. Seed the anchors that spilled nothing
        // within reach of a freed area, as an emptied cell's are: one corner per sheet bounds the
        // scan the same way the emptied loop's does.
        let mut unblocked: Vec<(usize, CellRef)> = Vec::new();
        for &(si, r) in &cleared {
            match unblocked.iter_mut().find(|x| x.0 == si) {
                Some(x) => x.1 = CellRef::new(x.1.row.max(r.end.row), x.1.col.max(r.end.col)),
                None => unblocked.push((si, r.end)),
            }
        }
        for (si, end) in unblocked {
            for (s, c) in self.graph.node_keys() {
                if s == si && c.row <= end.row && c.col <= end.col && wb.sheet(s).is_some_and(|sh| spilled_nothing(&sh.value(c))) {
                    seeds.push((s, c));
                }
            }
        }
        let t0 = prof_now();
        let dirty = self.dirty_closure(wb, &seeds);
        let t1 = prof_now();
        let n = dirty.len();
        self.run(wb, dirty, false, &HashSet::default());
        if std::env::var_os("CELLA_PROFILE").is_some() {
            eprintln!("recalc: {} seeds, closure {} cells {:.1} ms, run {:.1} ms", seeds.len(), n, t1 - t0, prof_now() - t1);
        }
    }

    /// The cells an edit reaches: the seeds, the formulas that read them, and so on out to the last
    /// formula of the chain.
    ///
    /// One probe a cell. The seen-set is the sheet's bits ([`SeenCells`]), the queue is a vector, and
    /// the tables a workbook of ordinary formulas leaves empty — cross-sheet, whole column, wide
    /// range — are skipped once per walk instead of once per cell ([`graph::Walker`]).
    fn dirty_closure(&self, _wb: &Workbook, seeds: &[Key]) -> Vec<Key> {
        let mut seen = SeenCells::default();
        // Found but not walked yet. A seed can name a million dependents (the cell every formula
        // reads), so the front moves along the vector instead of shifting it.
        let mut queue: Vec<Key> = Vec::new();
        let mut walk = self.graph.walker();
        for &k in seeds {
            if self.graph.has_node(k) && seen.mark(k) {
                queue.push(k);
            }
            walk.dependents_of(k.0, k.1, &mut |d| {
                if seen.mark(d) {
                    queue.push(d);
                }
            });
        }
        for &k in &self.graph.dynamic {
            if seen.mark(k) {
                queue.push(k);
            }
        }
        let mut head = 0;
        while let Some(&k) = queue.get(head) {
            head += 1;
            walk.dependents_of(k.0, k.1, &mut |d| {
                if seen.mark(d) {
                    queue.push(d);
                }
            });
        }
        seen.into_keys()
    }

    /// Evaluates the dirty set and writes results (and spills) back.
    fn run(&mut self, wb: &mut Workbook, mut dirty: Vec<Key>, full: bool, prefer: &HashSet<Key>) {
        let mut dynamic_again = !self.graph.dynamic.is_empty();
        self.last_recalc_cells = 0;
        // Every loop the rounds met: a later round evaluates only its dependents, so the cells
        // that were re-entered are remembered for the UI and `Calc::circular`.
        let mut found_cycles: Vec<Key> = Vec::new();
        // The write-back's batch buffer, kept across the rounds and waves of this pass so a
        // million-value write-back allocates its temporary once (and the values are moved into it,
        // never cloned). Dropped when the pass ends, so it holds no memory between edits.
        let mut batch: Vec<(CellRef, Value)> = Vec::new();
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
                while waves < 16 && self.try_parallel(wb, &mut dirty, &mut batch) {
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
            // One dense table of the formulas to evaluate, in the row-major order `dirty` is in: the
            // pass looks a cell up once (its slot), and the write-back below walks the same order.
            // The table is indexed by the box `dirty` fills, so a lookup is arithmetic rather than a
            // hash probe (and a sparse set keeps the hash table instead).
            //
            // Its build is timed only when profiling is asked for: a clock read is not a pass's
            // business otherwise.
            let trees_started = std::env::var_os("CELLA_PROFILE").is_some().then(prof_now);
            let mut trees = Trees::for_cells(&dirty);
            let mut legacy: HashMap<Key, RangeRef> = HashMap::default();
            // The sheet's node table is found once for the run of its cells `dirty` lists
            // (`dirty` is row-major, so they arrive together): a million-formula pass of one sheet
            // asked the sheet table a million times for a table it never changed.
            for group in dirty.chunk_by(|a, b| a.0 == b.0) {
                let Some(sheet) = group.first().map(|k| k.0) else { continue };
                let nodes = self.graph.nodes_of(sheet);
                for k in group {
                    let Some(node) = nodes.and_then(|nodes| nodes.get(&k.1)) else { continue };
                    trees.push(*k, node);
                    if let Some(r) = self.graph.legacy_of(*k) {
                        legacy.insert(*k, r);
                    }
                }
            }
            if let Some(started) = trees_started {
                eprintln!("  trees {} slots {:.1} ms", trees.len(), prof_now() - started);
            }
            let now = self.now();
            let mut rng = self.rng;
            let tp = prof_now();
            #[cfg(test)]
            let chain_fast = self.chain_fast;
            let (mut slots, mut spills, cycles) = {
                let mut host = PassHost::new(wb, &self.graph, &trees, &legacy, &mut self.names, &mut rng, now);
                host.iterate = wb.calc.iterative;
                #[cfg(test)]
                {
                    host.chain_fast = chain_fast;
                }
                for (slot, k) in trees.iter() {
                    // A run of chain formulas — a column of running totals, which is most of the
                    // dirty set of an edit at the top of a large sheet — is accumulated in one
                    // loop; anything else goes through the general evaluator, cell by cell.
                    if host.chain_run(k, slot) {
                        continue;
                    }
                    host.calculate_at(k, slot);
                }
                if std::env::var_os("CELLA_PROFILE").is_some() {
                    eprintln!("  chain: {} runs, {} formulas", host.chains, host.chain_formulas);
                }
                if host.iterate && !host.members.is_empty() {
                    let passes = host.iterate_loops(&dirty, wb.calc.max_iterations, wb.calc.max_change);
                    if std::env::var_os("CELLA_PROFILE").is_some() {
                        eprintln!("  iterative calculation: {} passes over {} formulas", passes, host.tainted.len());
                    }
                }
                #[cfg(test)]
                {
                    self.read_probes += host.spill_index.probes();
                    self.block_probes += host.block_probes();
                    self.chains = self.chains.saturating_add(host.chains);
                    self.chain_formulas = self.chain_formulas.saturating_add(host.chain_formulas);
                }
                (host.slots, host.spills, host.cycles)
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
                    if let Some(state) = trees.slot(*k).and_then(|slot| slots.get_mut(slot as usize)) {
                        *state = Slot::circular();
                    }
                    if let Some(arr) = spills.remove(k) {
                        let end = CellRef::new(
                            k.1.row.saturating_add((arr.rows as u32).saturating_sub(1)),
                            k.1.col.saturating_add((arr.cols as u32).saturating_sub(1)),
                        );
                        discarded.push((k.0, RangeRef::new(k.1, end)));
                    }
                }
            }
            if std::env::var_os("CELLA_PROFILE").is_some() {
                let done = slots.iter().filter(|s| s.value().is_some()).count();
                eprintln!("  eval {done} formulas {:.1} ms", prof_now() - tp);
            }
            let tw = prof_now();
            found_cycles.extend(cycles.iter().copied());
            // Write back.
            let mut spill_changes: Vec<Key> = Vec::new();
            for (si, range) in discarded {
                self.graph.dependents_of_range(si, range, &mut spill_changes);
            }
            let mut freed: Vec<(usize, RangeRef)> = Vec::new();
            let twr = prof_now();
            // Row-major order: the slots are in `dirty`'s order, and walking bands in row order is
            // what makes the write-back touch each band once instead of jumping between 1,563 of
            // them at random. Values only change, so the store rewrites the packed slot and nothing
            // else (no `Cell` built, no formula or style looked up).
            // The slots move straight into the store: a million values first collected into a list
            // of their own was 16 MB built only to be walked again.
            let written = write_values(wb, trees.keys().zip(slots).filter_map(|(k, s)| s.into_value().map(|v| (k, v))), &mut batch, trees.len());
            self.last_recalc_cells += written;
            let two = prof_now();
            if std::env::var_os("CELLA_PROFILE").is_some() {
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
                    let blocked = end.row >= cella_core::MAX_ROWS
                        || end.col >= cella_core::MAX_COLS
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
                for (s, c) in self.graph.node_keys() {
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
            if std::env::var_os("CELLA_PROFILE").is_some() {
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
    cella_core::date::unix_now_ms().map_or(46302.5, cella_core::date::serial_from_unix_ms)
}

/// Evaluates a formula in the context of a cell without storing it (conditional formats, data
/// validation, Evaluate Formula, Name Manager previews, the CLI `eval`).
pub fn evaluate(wb: &Workbook, sheet: usize, at: CellRef, formula: &str) -> Value {
    let body = formula.strip_prefix('=').unwrap_or(formula);
    let Ok(expr) = cella_formula::parse(body) else { return Value::Error(CellError::Name) };
    evaluate_expr(wb, sheet, at, &expr)
}

pub fn evaluate_expr(wb: &Workbook, sheet: usize, at: CellRef, expr: &Expr) -> Value {
    let trees = Trees::default();
    let legacy = HashMap::default();
    let graph = Graph::default();
    let mut rng = 0x1234_5678_9ABC_DEF0u64;
    // One formula, so its own cache (and its names resolved once, however many of them it mentions).
    let mut names = crate::eval::NameCache::default();
    let mut host = PassHost::new(wb, &graph, &trees, &legacy, &mut names, &mut rng, now_serial());
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

    use cella_core::{CellError, CellRef, RangeRef, Value};
    use cella_model::{Cell, DefinedName, Formula, Workbook};

    use crate::eval::Host;

    use super::graph::{Intervals, without_dense};
    use super::{Calc, Graph, HashMap, Key, PassHost, SeenCells, Trees};

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
        wb.sheets.push(std::sync::Arc::new(cella_model::Sheet::new("Data")));
        set(&mut wb, &mut calc, "DA9", "=SUM(Data!A1:A3)+SUM(Data!A:CZ)");
        assert_eq!(num(&wb, "DA9"), 0.0);
        wb.sheet_mut(1).unwrap().cells.set(CellRef::parse("A2").unwrap(), Cell::value(Value::Number(4.0)));
        calc.cells_changed(&mut wb, &[(1, CellRef::parse("A2").unwrap())]);
        assert_eq!(num(&wb, "DA9"), 8.0);
        deps.clear();
        calc.graph.dependents(1, CellRef::parse("A2").unwrap(), &mut deps);
        assert_eq!(deps, vec![at("DA9"), at("DA9")]);
    }

    /// The closure carries every dependent of a seed once: down a column of formulas that makes the
    /// seen-set's box grow, through a diamond that reaches a formula twice, and across a sheet edge
    /// the empty tables of an ordinary workbook would hide.
    #[test]
    fn the_closure_visits_each_dependent_once() {
        let mut wb = Workbook::new();
        let at = |s: &str| CellRef::parse(s).unwrap();
        wb.sheets.push(std::sync::Arc::new(cella_model::Sheet::new("Data")));
        if let Some(sh) = wb.sheet_mut(0) {
            sh.cells.set(at("A1"), Cell::value(Value::Number(3.0)));
            sh.cells.set(at("B1"), Cell::formula(Formula::new("=A1*2")));
            sh.cells.set(at("C1"), Cell::formula(Formula::new("=A1+1")));
            // A diamond: A1 feeds B1 and C1, and D1 reads both of them.
            sh.cells.set(at("D1"), Cell::formula(Formula::new("=B1+C1")));
            sh.cells.set(at("E1"), Cell::formula(Formula::new("=A1+D1")));
            for row in 1..300u32 {
                sh.cells.set(CellRef::new(row, 4), Cell::formula(Formula::new("=$A$1")));
            }
        }
        if let Some(sh) = wb.sheet_mut(1) {
            sh.cells.set(at("A1"), Cell::formula(Formula::new("=Sheet1!D1")));
        }
        let mut calc = Calc::new();
        calc.rebuild(&wb);
        // A1 is a value, so it is no node of the graph and the closure starts at what reads it.
        let closure = calc.dirty_closure(&wb, &[(0, at("A1"))]);
        let mut sorted = closure.clone();
        sorted.sort_unstable();
        let mut once = sorted.clone();
        once.dedup();
        assert_eq!(once.len(), sorted.len(), "the walk visited a cell twice: {:?}", closure);
        let mut want: Vec<Key> = vec![(0, at("B1")), (0, at("C1")), (0, at("D1")), (0, at("E1")), (1, at("A1"))];
        want.extend((1..300u32).map(|row| (0, CellRef::new(row, 4))));
        want.sort_unstable();
        assert_eq!(once, want);
        // The same walk through the graph's own cell lookup, which is the one the engine's Trace
        // Dependents command and the write-back use: one sheet's table, then the cross-sheet one.
        let mut deps = Vec::new();
        calc.graph.dependents(0, at("A1"), &mut deps);
        deps.sort_unstable();
        for wanted in ["B1", "C1", "E1"] {
            assert!(deps.contains(&(0, at(wanted))), "{wanted} missing: {deps:?}");
        }
        assert_eq!(deps.len(), 3 + 299, "the column that reads it too");
        deps.clear();
        calc.graph.dependents(0, at("D1"), &mut deps);
        assert_eq!(deps, vec![(0, at("E1")), (1, at("A1"))], "the cross-sheet dependent is found");
    }

    /// The seen-set the closure relies on: a bit each while the cells of a sheet fit a box, the box
    /// grown by doubling as the walk leaves it, and a cell no box could hold marked in the hash set
    /// — but each cell marked once whichever set answers, or the walk visits it twice.
    #[test]
    fn the_seen_set_marks_a_cell_once() {
        let mut seen = SeenCells::default();
        // A column: the one-cell box the first mark makes is doubled a dozen times to hold it.
        for row in 0..4_000u32 {
            assert!(seen.mark((0, CellRef::new(row, 1))), "row {row} is new");
        }
        for row in (0..4_000u32).rev() {
            assert!(!seen.mark((0, CellRef::new(row, 1))), "row {row} was marked");
        }
        // Another sheet, and a cell forty columns past the box.
        assert!(seen.mark((1, CellRef::new(9, 9))));
        assert!(!seen.mark((1, CellRef::new(9, 9))));
        assert!(seen.mark((0, CellRef::new(3, 10))));
        assert!(seen.mark((0, CellRef::new(3, 40))), "a cell the box has to grow sideways for");
        assert!(!seen.mark((0, CellRef::new(3, 40))));
        // A cell a box cannot hold — the far corner of the sheet is past `MAX_DIRTY_BITS` cells
        // away — and the cells the box held before it, which the hash set then answers for.
        let far = (2, CellRef::new(cella_core::MAX_ROWS - 1, cella_core::MAX_COLS - 1));
        for row in 0..5u32 {
            assert!(seen.mark((2, CellRef::new(row, 0))));
        }
        assert!(seen.mark(far));
        assert!(!seen.mark(far));
        assert!(!seen.mark((2, CellRef::new(3, 0))), "a cell the dropned box held is still marked");
        let keys = seen.into_keys();
        let mut unique = keys.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), keys.len(), "a cell was marked twice");
        assert_eq!(keys.len(), 4_000 + 2 + 1 + 5 + 1);
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
        array.array = RangeRef::parse("A1:A2").map(Box::new);
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
        wb.sheets.push(std::sync::Arc::new(cella_model::Sheet::new("Sheet2")));
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

    /// A column of formulas each reading the one above it — a running total — has one leaf in a
    /// million, so the pass leaves it to one thread: no wave is started (and the dirty cells are
    /// counted before the leaf scan, which is all a wave that finds nothing would cost).
    #[test]
    fn a_chain_of_formulas_stays_on_one_thread() {
        let n = 17_000u32;
        let mut wb = Workbook::new();
        if let Some(sh) = wb.sheet_mut(0) {
            sh.cells.set(CellRef::new(0, 1), Cell::value(Value::Number(1.0)));
            sh.cells.set(CellRef::new(0, 9), Cell::formula(Formula::new("=B1")));
            for r in 1..n {
                sh.cells.set(CellRef::new(r, 1), Cell::value(Value::Number(1.0)));
                sh.cells.set(CellRef::new(r, 9), Cell::formula(Formula::new(&format!("=J{}+B{}", r, r + 1))));
            }
        }
        let mut calc = Calc::new();
        calc.recalc_all(&mut wb);
        assert_eq!(calc.parallel_runs, 0, "a chain of formulas has no independent wave");
        for r in 0..n {
            assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(r, 9)), Value::Number((r + 1) as f64), "row {r}");
        }
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
        // The shapes a pass reads are borrowed from the graph it runs on; here they are kept in
        // one list, which the table below borrows for as long as it lives.
        let mut formulas: Vec<(CellRef, cella_formula::Shape)> = Vec::new();
        let mut put = |wb: &mut Workbook, at: String, text: String| {
            let at = CellRef::parse(&at).unwrap();
            wb.sheet_mut(0).unwrap().cells.set(at, Cell::formula(Formula::new(&text)));
            formulas.push((at, cella_formula::Shape::from_expr(cella_formula::parse(&text[1..]).unwrap(), at)));
        };
        // A1 reads A2, ..., A64 (the 64th formula deep) reads a wide range of formulas.
        for r in 1..64 {
            put(&mut wb, format!("A{r}"), format!("=A{}+1", r + 1));
        }
        put(&mut wb, "A64".to_string(), format!("=SUM(B1:B{WIDTH})"));
        for r in 1..=WIDTH {
            put(&mut wb, format!("B{r}"), format!("={r}*2"));
        }
        let mut trees = Trees::default();
        for (at, shape) in &formulas {
            trees.push_shape((0, *at), shape);
        }
        let mut rng = 1u64;
        let graph = Graph::default();
        let legacy = HashMap::default();
        let mut names = crate::eval::NameCache::default();
        let mut host = PassHost::new(&wb, &graph, &trees, &legacy, &mut names, &mut rng, 0.0);
        host.calculate((0, CellRef::parse("A1").unwrap()));
        let total = (WIDTH * (WIDTH + 1)) as f64;
        assert_eq!(host.value((0, CellRef::parse("A1").unwrap())), Some(&Value::Number(total + 63.0)));
        assert!(host.cycles.is_empty());
        assert!(host.suspensions <= 3, "{} suspensions", host.suspensions);
    }

    #[test]
    fn sumifs_packed_numeric_columns_preserve_order_errors_and_fallback() {
        let mut wb = Workbook::new();
        let sh = wb.sheet_mut(0).unwrap();
        let put = |sh: &mut cella_model::Sheet, at: &str, value| sh.cells.set(CellRef::parse(at).unwrap(), Cell::value(value));
        for (at, value) in [
            ("A1", Value::Error(CellError::NA)),
            ("A2", Value::Number(1e16)),
            ("A3", Value::Number(1.0)),
            ("A4", Value::Number(-1e16)),
            ("A5", Value::Bool(true)),
            ("A6", Value::text("20")),
            ("A7", Value::Error(CellError::Div0)),
            ("A8", Value::Error(CellError::Ref)),
            ("B2", Value::Number(0.1 + 0.2)),
            ("B3", Value::text("9")),
            ("B4", Value::Number(1.0)),
            ("B5", Value::Number(1.0)),
            ("B6", Value::Number(1.0)),
            ("B7", Value::Number(1.0)),
            ("B8", Value::Number(2.0)),
            ("B9", Value::Number(3.0)),
            ("B10", Value::Error(CellError::NA)),
        ] {
            put(sh, at, value);
        }
        let area = |text| crate::eval::Area { sheet: 0, range: RangeRef::parse(text).unwrap() };
        let check = |wb: &Workbook, target, criteria, op| {
            let trees = Trees::default();
            let graph = Graph::default();
            let legacy = HashMap::default();
            let mut names = crate::eval::NameCache::default();
            let mut rng = 1;
            let host = PassHost::new(wb, &graph, &trees, &legacy, &mut names, &mut rng, 0.0);
            let op = cella_functions::NumericOrderingCriterion::parse(&Value::text(op)).unwrap();
            host.sumifs_numeric_columns(area(target), area(criteria), op)
        };
        assert_eq!(check(&wb, "A1:A10", "B1:B10", ">=0.3"), Some(Value::Error(CellError::Div0)));
        // Shifted ranges align their first rows. Values 1e16, 1, -1e16 sum to 0 in row order.
        assert_eq!(check(&wb, "A2:A4", "B4:B6", ">0"), Some(Value::Number(0.0)));
        // Blanks and numeric text do not match a numeric ordering condition. 0.1+0.2 is not >0.3.
        assert_eq!(check(&wb, "A1:A3", "B1:B3", ">0.3"), Some(Value::Number(0.0)));
        assert_eq!(check(&wb, "A1:A3", "B1:B3", ">=0.3"), Some(Value::Number(1e16)));
        let sh = wb.sheet_mut(0).unwrap();
        sh.cells.set(CellRef::parse("A9").unwrap(), Cell::formula(Formula::new("=10")));
        assert_eq!(check(&wb, "A1:A10", "B1:B10", ">=0.3"), None, "late formula after a matching error");
        let sh = wb.sheet_mut(0).unwrap();
        put(sh, "B2", Value::from(cella_core::Array::row(vec![Value::Number(1.0)])));
        assert_eq!(check(&wb, "A1:A3", "B1:B3", ">=0.3"), None, "array criteria require scalarization");
        let sh = wb.sheet_mut(0).unwrap();
        sh.spill.insert(CellRef::parse("B1").unwrap(), Value::Number(1.0));
        assert_eq!(check(&wb, "A1:A3", "B4:B6", ">0"), None, "existing spills decline without evaluating");
    }

    #[test]
    fn packed_sum_formula_column_preserves_prepass_and_spill_fallback() {
        use crate::eval::{Acc, Agg};

        let col = RangeRef::parse("D1:D4096").unwrap();
        let d2 = CellRef::parse("D2").unwrap();
        let d5 = CellRef::parse("D5").unwrap();
        for (input, expected) in [("=7", Some(Ok(()))), ("=1/0", Some(Err(CellError::Div0))), ("=SEQUENCE(3)", None)] {
            let mut wb = Workbook::new();
            let sh = wb.sheet_mut(0).unwrap();
            sh.cells.set(col.start, Cell::value(Value::Number(42.0)));
            sh.cells.set(d2, Cell::formula(Formula::new(input)));
            sh.cells.set(d5, Cell::formula(Formula::new("=9")));
            // A stored array is not a formula; ignore it without blocking the spill at D2:D4.
            sh.cells.set(CellRef::parse("D6").unwrap(), Cell::value(Value::from(cella_core::Array::row(vec![Value::Number(99.0)]))));
            let cached = Cell { value: Value::Number(5.0), ..Cell::formula(Formula::new("=5")) };
            sh.cells.set(CellRef::parse("D7").unwrap(), cached);

            let first = cella_formula::Shape::from_expr(cella_formula::parse(input.trim_start_matches('=')).unwrap(), d2);
            let last = cella_formula::Shape::from_expr(cella_formula::parse("9").unwrap(), d5);
            let mut trees = Trees::default();
            trees.push_shape((0, d2), &first);
            trees.push_shape((0, d5), &last);
            let graph = Graph::default();
            let legacy = HashMap::default();
            let mut names = crate::eval::NameCache::default();
            let mut rng = 1;
            let mut host = PassHost::new(&wb, &graph, &trees, &legacy, &mut names, &mut rng, 0.0);
            let mut acc = Acc::default();
            acc.add_cell(Agg::Sum, &Value::Number(1.0)).unwrap();
            assert_eq!(host.visit_aggregate(0, col, Agg::Sum, &mut acc), expected);
            assert_eq!(host.value((0, d5)), Some(&Value::Number(9.0)), "the prepass still evaluates formulas after an earlier error or spill");
            if expected == Some(Ok(())) {
                assert_eq!(acc.finish(Agg::Sum), Value::Number(64.0), "reads live and cached formulas but ignores the stored array");
            } else if expected.is_none() {
                assert_eq!(acc.finish(Agg::Sum), Value::Number(1.0), "a new spill leaves the accumulator untouched for fallback");
            }
        }
    }

    #[test]
    fn range_values_keeps_constants_and_materializes_old_and_new_spills() {
        let mut wb = Workbook::new();
        let sh = wb.sheet_mut(0).unwrap();
        let range = RangeRef::parse("D1:D6").unwrap();
        let d4 = CellRef::parse("D4").unwrap();
        let d5 = CellRef::parse("D5").unwrap();
        sh.cells.set(range.start, Cell::formula(Formula::new("=SEQUENCE(3)")));
        sh.cells.set(d4, Cell::value(Value::text("constant")));
        sh.cells.set(d5, Cell::value(Value::Error(CellError::Div0)));
        sh.spill.insert(d5, Value::Number(-1.0));
        sh.spill.insert(range.end, Value::Number(6.0));

        let shape = cella_formula::Shape::from_expr(cella_formula::parse("SEQUENCE(3)").unwrap(), range.start);
        let mut trees = Trees::default();
        trees.push_shape((0, range.start), &shape);
        let graph = Graph::default();
        let legacy = HashMap::default();
        let mut names = crate::eval::NameCache::default();
        let mut rng = 1;
        let mut host = PassHost::new(&wb, &graph, &trees, &legacy, &mut names, &mut rng, 0.0);
        assert_eq!(
            host.range_values(0, range),
            vec![
                Value::Number(1.0),
                Value::Number(2.0),
                Value::Number(3.0),
                Value::text("constant"),
                Value::Error(CellError::Div0),
                Value::Number(6.0)
            ]
        );
    }

    /// For tests of `PassHost::visit_range`, which can't be reached from outside this module: the
    /// values it hands over for `range` while the formulas in `pending` (cell, formula) have not been
    /// calculated yet, or `None` when it declines the block.
    pub(crate) fn probe_visit_range(wb: &Workbook, pending: &[(&str, &str)], range: RangeRef) -> Option<Vec<Value>> {
        // Parsed into one list first: the table points at the shapes a real pass borrows from its
        // graph, so they have to outlive it.
        let cells: Vec<(CellRef, cella_formula::Shape)> = pending
            .iter()
            .filter_map(|(at, formula)| {
                let at = CellRef::parse(at)?;
                let expr = cella_formula::parse(formula.strip_prefix('=').unwrap_or(formula)).ok()?;
                Some((at, cella_formula::Shape::from_expr(expr, at)))
            })
            .collect();
        let mut trees = Trees::default();
        for (at, shape) in &cells {
            trees.push_shape((0, *at), shape);
        }

        // A cell in `pending` is calculated the first time something reads it, which is the state
        // of a real pass partway through; `visit_range` has to cope with the formulas it reads
        // being calculated under it.
        let mut rng = 1u64;
        let graph = Graph::default();
        let legacy = HashMap::default();
        let mut names = crate::eval::NameCache::default();
        let mut host = PassHost::new(wb, &graph, &trees, &legacy, &mut names, &mut rng, 0.0);

        let mut seen = Vec::new();
        host.visit_range(0, range, &mut |v| {
            seen.push(v.clone());
            true
        })
        .then_some(seen)
    }

    /// The formula cells of `wb`, in the row-major order a pass lists them.
    fn dirty_formulas(wb: &Workbook) -> Vec<Key> {
        let mut dirty: Vec<Key> = Vec::new();
        for (si, sheet) in wb.sheets.iter().enumerate() {
            for (at, cell) in sheet.cells.iter() {
                if cell.formula.is_some() {
                    dirty.push((si, at));
                }
            }
        }
        dirty.sort_unstable_by_key(|(s, c)| (*s, c.row, c.col));
        dirty
    }

    /// One sequential pass over `dirty`, as the recalc runs it, on the slot table `dense` picks.
    /// Returns whether the table was dense, and the value of every listed cell.
    fn pass_values(wb: &Workbook, dirty: &[Key], dense: bool) -> (bool, Vec<(Key, Option<Value>)>) {
        // The shapes are built first and kept here: the table borrows them (as a pass borrows its
        // graph's).
        let shapes: Vec<(Key, cella_formula::Shape)> = dirty
            .iter()
            .filter_map(|&k| {
                let text = wb.sheet(k.0).and_then(|s| s.cells.get(k.1)).and_then(|c| c.formula.as_ref().map(|f| f.text.clone()))?;
                let expr = cella_formula::parse(text.strip_prefix('=').unwrap_or(&text)).ok()?;
                Some((k, cella_formula::Shape::from_expr(expr, k.1)))
            })
            .collect();
        let mut trees = if dense { Trees::for_cells(dirty) } else { Trees::for_cells_sparse(dirty) };
        for (k, shape) in &shapes {
            trees.push_shape(*k, shape);
        }
        let mut rng = 1u64;
        let graph = Graph::default();
        let legacy = HashMap::default();
        let mut names = crate::eval::NameCache::default();
        let mut host = PassHost::new(wb, &graph, &trees, &legacy, &mut names, &mut rng, 0.0);
        for (slot, k) in trees.iter() {
            host.calculate_at(k, slot);
        }
        (trees.is_dense(), dirty.iter().map(|k| (*k, host.value(*k).cloned())).collect())
    }

    /// The slot table a pass builds is a performance choice, not a semantic one: the same workbook
    /// and the same dirty list must calculate the same values through the dense box and the hash
    /// table, and the values must be the ones a whole recalculation gives.
    #[test]
    fn a_dense_slot_table_calculates_what_the_hash_table_does() {
        let mut wb = Workbook::new();
        {
            let sh = wb.sheet_mut(0).expect("sheet");
            for r in 0..5 {
                let row = r + 1;
                sh.cells.set(CellRef::new(r, 0), Cell::value(Value::Number(f64::from(row))));
                sh.cells.set(CellRef::new(r, 1), Cell::formula(Formula::new(&format!("=A{row}*2"))));
                sh.cells.set(CellRef::new(r, 2), Cell::formula(Formula::new(&format!("=B{row}+A{row}"))));
                sh.cells.set(CellRef::new(r, 3), Cell::formula(Formula::new(&format!("=SUM(B$1:B{row})"))));
                sh.cells.set(CellRef::new(r, 4), Cell::formula(Formula::new(&format!("=IF(C{row}>4,\"big\",C{row}*1.5)"))));
            }
            sh.cells.set(CellRef::new(0, 5), Cell::formula(Formula::new("=SUM(E1:E5)")));
            sh.cells.set(CellRef::new(1, 5), Cell::formula(Formula::new("=Z1+1")));
            sh.cells.set(CellRef::new(2, 5), Cell::formula(Formula::new("=1/0")));
        }
        let dirty = dirty_formulas(&wb);
        assert!(dirty.len() > 20, "{} formulas", dirty.len());
        let (dense, dense_values) = pass_values(&wb, &dirty, true);
        let (sparse, sparse_values) = pass_values(&wb, &dirty, false);
        assert!(dense, "a filled box must use the dense table");
        assert!(!sparse, "the sparse table is the one the sparse path builds");
        assert_eq!(dense_values.len(), dirty.len(), "every formula got a slot");
        assert_eq!(dense_values, sparse_values, "the two tables must agree cell for cell");
        let mut full = wb.clone();
        Calc::new().recalc_all(&mut full);
        for (k, v) in &dense_values {
            let expected = full.sheet(k.0).expect("sheet").value(k.1);
            assert_eq!(v.as_ref(), Some(&expected), "cell {k:?}");
        }
    }

    /// A dirty set that does not fill its box keeps the hash table: three cells far apart would
    /// otherwise zero-fill the box between them, which is the cost this table exists to avoid.
    #[test]
    fn a_sparse_dirty_set_keeps_the_hash_table() {
        let far: Vec<Key> = vec![(0, CellRef::new(0, 0)), (0, CellRef::new(200_000, 5)), (0, CellRef::new(900_000, 40))];
        assert!(!Trees::for_cells(&far).is_dense(), "three cells far apart must not zero-fill a box of the sheet");
        // A column is a box one cell wide, however tall.
        let column: Vec<Key> = (0..1_000u32).map(|r| (0, CellRef::new(r, 7))).collect();
        assert!(Trees::for_cells(&column).is_dense());
        // One listed cell in eight box cells is the threshold: ten cells on a ten-by-ten diagonal
        // are 100 box cells against 80, so the hash table.
        let diagonal: Vec<Key> = (0..10u32).map(|i| (0, CellRef::new(i, i))).collect();
        assert!(!Trees::for_cells(&diagonal).is_dense());
        // The sheet is part of the box, so a list split over two sheets pays for both, and the
        // arithmetic puts a second sheet's cells after the first sheet's rows.
        let two_sheets: Vec<Key> = (0..2usize).flat_map(|s| (0..400u32).map(move |r| (s, CellRef::new(r, 3)))).collect();
        let mut trees = Trees::for_cells(&two_sheets);
        assert!(trees.is_dense());
        let one = cella_formula::Shape::from_expr(cella_formula::parse("1").expect("a literal parses"), CellRef::new(0, 3));
        trees.push_shape(two_sheets[0], &one);
        trees.push_shape((1, CellRef::new(0, 3)), &one);
        assert_eq!(trees.slot(two_sheets[0]), Some(0));
        assert_eq!(trees.slot((1, CellRef::new(0, 3))), Some(1));
        assert_eq!(trees.slot((1, CellRef::new(400, 3))), None, "a cell past the box has no slot");
        // A row past the box would land inside the next sheet's rows of a table that only checked
        // the end of the list: a pass hopping between two sheets of the same height must not read
        // the other sheet's cell.
        assert_eq!(trees.slot((0, CellRef::new(450, 3))), None, "a row past the box must not read the next sheet");
        assert_eq!(trees.slot((0, CellRef::new(0, 4))), None, "a cell beside the box has no slot");
        assert_eq!(trees.slot((2, CellRef::new(0, 3))), None, "a sheet the box does not span has no slot");
    }

    /// A chain of `rows` formulas down column B: each reads the cell above it, and every tenth also
    /// reads the one value cell A1 — so one precedent is a list and the rest are single cells.
    fn chain(rows: u32) -> Workbook {
        let mut wb = Workbook::new();
        if let Some(sh) = wb.sheet_mut(0) {
            sh.cells.set(CellRef::new(0, 0), Cell::value(Value::Number(1.0)));
            sh.cells.set(CellRef::new(0, 1), Cell::formula(Formula::new("=A1*2")));
            for row in 1..rows {
                let text = if row % 10 == 0 { format!("=B{}+$A$1", row) } else { format!("=B{}+1", row) };
                sh.cells.set(CellRef::new(row, 1), Cell::formula(Formula::new(&text)));
            }
        }
        wb
    }

    /// The same workbook's graph with the dense box refused: the hash table the dense index has to
    /// agree with, key for key.
    fn sparse_twin(wb: &Workbook) -> Calc {
        let mut calc = Calc::new();
        without_dense(true, || calc.rebuild(wb));
        assert!((0..wb.sheets.len()).all(|sheet| !calc.graph.has_dense_index(sheet)), "the box is refused here");
        calc
    }

    /// Every precedent the two graphs answer for, and the dependents each gives it — as sets: the
    /// order of a list is the order its dependents were linked in, which a rebuild (row by row) and
    /// an edit (a cell at a time) do not share, so only the same construction path can be held to
    /// the same order (see the closure test above).
    fn assert_same_index(a: &Calc, b: &Calc) {
        assert_eq!(a.graph.index_answers(), b.graph.index_answers(), "the two indexes hold different entries");
        for ((sheet, at), _) in a.graph.index_answers() {
            let (mut boxed, mut hashed) = (Vec::new(), Vec::new());
            a.graph.dependents(sheet, at, &mut boxed);
            b.graph.dependents(sheet, at, &mut hashed);
            boxed.sort_unstable();
            hashed.sort_unstable();
            assert_eq!(boxed, hashed, "the dependents of sheet {sheet} at {at:?} differ");
        }
    }

    /// The closure an edit walks — every cell, in the order the walk reaches it — is the same with
    /// the dense box and with the hash table, and so is every value the pass writes.
    #[test]
    fn the_dense_index_and_the_hash_table_reach_the_same_cells_in_the_same_order() {
        let seeds: Vec<Key> = vec![(0, CellRef::new(0, 1))];
        let mut wb = chain(2_000);
        let mut dense = Calc::new();
        dense.rebuild(&wb);
        assert!(dense.graph.has_dense_index(0), "2,000 precedents fill a box");
        let dense_dirty = dense.dirty_closure(&wb, &seeds);
        assert!(dense_dirty.len() > 1_900, "the whole chain is behind B1: {}", dense_dirty.len());

        let mut sparse = sparse_twin(&wb);
        let sparse_dirty = sparse.dirty_closure(&wb, &seeds);
        assert_eq!(dense_dirty, sparse_dirty, "the same cells, in the same order");
        assert_same_index(&dense, &sparse);

        // The values the pass writes have to agree cell for cell after the same edit.
        let mut sparse_wb = wb.clone();
        let touched: Vec<Key> = vec![(0, CellRef::new(0, 0))];
        for book in [&mut wb, &mut sparse_wb] {
            if let Some(sh) = book.sheet_mut(0) {
                sh.cells.set(CellRef::new(0, 0), Cell::value(Value::Number(7.0)));
            }
        }
        dense.cells_changed(&mut wb, &touched);
        without_dense(true, || sparse.cells_changed(&mut sparse_wb, &touched));
        for row in 0..2_000u32 {
            let at = CellRef::new(row, 1);
            let (a, b) = (wb.sheet(0).map(|s| s.value(at)), sparse_wb.sheet(0).map(|s| s.value(at)));
            assert_eq!(a, b, "the value of B{} differs", row + 1);
        }
        assert_same_index(&dense, &sparse);
    }

    /// A structural edit is a rebuild, and an undo relinks cells one at a time; both leave the dense
    /// box holding exactly the entries the hash table of the same workbook holds.
    #[test]
    fn a_structural_edit_and_a_restore_keep_the_dense_index_equal_to_the_hash_one() {
        let mut wb = chain(1_500);
        let mut calc = Calc::new();
        calc.rebuild(&wb);
        assert!(calc.graph.has_dense_index(0), "1,500 precedents fill a box");
        assert_same_index(&calc, &sparse_twin(&wb));

        // insertRows: every cell from row 500 down moves four rows down, formulas and all.
        if let Some(sh) = wb.sheet_mut(0) {
            sh.cells.shift_rows(500, 4);
        }
        calc.rebuild(&wb);
        assert!(calc.graph.has_dense_index(0), "the moved precedents still fill a box");
        assert_same_index(&calc, &sparse_twin(&wb));

        // insertCols, then formulas re-entered and others replaced by values: the incremental path
        // a command takes.
        if let Some(sh) = wb.sheet_mut(0) {
            sh.cells.shift_cols(0, 1);
        }
        calc.rebuild(&wb);
        assert_same_index(&calc, &sparse_twin(&wb));
        let mut edited: Vec<Key> = Vec::new();
        if let Some(sh) = wb.sheet_mut(0) {
            for row in 1..400u32 {
                sh.cells.set(CellRef::new(row, 2), Cell::formula(Formula::new(&format!("=C{}+A1", row))));
                edited.push((0, CellRef::new(row, 2)));
            }
            for row in 300..400u32 {
                sh.cells.set(CellRef::new(row, 2), Cell::value(Value::Number(2.0)));
            }
        }
        calc.cells_changed(&mut wb, &edited);
        assert!(calc.graph.has_dense_index(0), "the re-entered formulas are linked into the box");
        assert_same_index(&calc, &sparse_twin(&wb));

        // Undo: the graph is put right from the cells the two versions disagree on.
        let snapshot = wb.clone();
        if let Some(sh) = wb.sheet_mut(0) {
            sh.cells.set(CellRef::new(1, 2), Cell::formula(Formula::new("=C1+2")));
        }
        calc.cells_changed(&mut wb, &[(0, CellRef::new(1, 2))]);
        let edited_wb = std::mem::replace(&mut wb, snapshot);
        assert!(calc.restore(&edited_wb, &wb), "an edit is restored from the cell it changed");
        assert_same_index(&calc, &sparse_twin(&wb));
    }
}
