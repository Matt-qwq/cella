//! One recalculation pass: the Host its evaluator reads cells through, the spill layout, the
//! indexed and cached blocks, and the bounded-stack scheduler that suspends a formula needed too
//! deep and calculates it from the top.

use std::collections::VecDeque;

use super::Graph;
use super::graph::{HashMap, HashSet, Key, PackedArea};
use crate::eval::{Evaluator, Host, binary_values};
use crate::spill_index::{Cover, SpillIndex};
use cella_core::{Array, CellError, CellRef, RangeRef, Value};
use cella_formula::{BinOp, Expr, RefKind, Shape, SheetSel};
use cella_model::Workbook;

/// Formulas a formula may calculate on demand one inside the other (each needing one another's
/// value before it is done). Each level takes stack, so a chain deeper than this is suspended and
/// calculated from the top instead (see [`PassHost::calculate`]): long dependency chains calculate
/// whatever order they come in, with bounded stack.
const MAX_NESTED: usize = 64;

/// Most spill areas a range read expands one by one before it walks every spill of the pass
/// instead (see `PassHost::range_values`). Both give the same cells; past this many the walk is
/// cheaper than picking the areas out first, and it can't be talked into comparing them all.
const MAX_UNORDERED_HITS: usize = 16;

/// Formula evaluations iterative calculation may spend on one recalculation (passes × formulas
/// evaluated again), however high Maximum Iterations is.
const MAX_ITERATION_WORK: usize = 10_000_000;

/// Text keys a block may hold before the pass stops indexing it (and all blocks together): an
/// index costs ~60 bytes a key.
const MAX_INDEXED_TEXT_VALUES: usize = 200_000;
const MAX_INDEXED_TEXT_VALUES_TOTAL: usize = 2_000_000;

/// Case-folded text → the first and last position holding it.
///
/// Case folding follows `compare_text` exactly (per character, `char::to_lowercase`), because a
/// key that folded differently would answer where a scan would not. Numbers, blanks and errors are
/// not indexed: a non-empty text needle can only match a text entry, so leaving them out cannot
/// lose a match.
struct TextIndex {
    positions: HashMap<String, (u32, u32)>,
}

impl TextIndex {
    fn build(values: &[Value]) -> Self {
        let mut positions: HashMap<String, (u32, u32)> = HashMap::with_capacity_and_hasher(values.len().min(1024), Default::default());
        for (i, value) in values.iter().enumerate() {
            let Value::Text(t) = value else { continue };
            if t.is_empty() {
                continue;
            }
            match positions.entry(fold(t)) {
                std::collections::hash_map::Entry::Occupied(mut e) => e.get_mut().1 = i as u32,
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert((i as u32, i as u32));
                }
            }
        }
        Self { positions }
    }

    fn position(&self, needle: &str, reverse: bool) -> Option<usize> {
        self.positions.get(&fold(needle)).map(|(first, last)| if reverse { *last as usize } else { *first as usize })
    }
}

/// The case folding `compare_text` compares with, as a key.
fn fold(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

/// Whether `c` holds content of its own. A cell kept only for its formatting (a fill, a border,
/// an empty value) does not: an array may spill over formatted cells, and a cell that was
/// formatted while a spill covered it must not turn that spill into `#SPILL!` on the next pass.
pub(super) fn holds_content(sheet: &cella_model::Sheet, c: CellRef) -> bool {
    sheet.cells.get(c).is_some_and(|x| x.formula.is_some() || !x.value.is_empty())
}

/// The largest single block the pass keeps built and shared. A lookup table is small; a whole
/// column read as an array is not, and rebuilding it is cheaper than holding it.
const MAX_CACHED_BLOCK_CELLS: usize = 4_000_000;
/// All shared blocks together, at 24 bytes a value.
const MAX_CACHED_BLOCK_CELLS_TOTAL: usize = 4_000_000;

/// The formula a pass evaluates in one slot: the shape it reads, the cell its relative parts are
/// offsets from, and the cell itself. S7 of `plan/SHAPE-SHARING.md` deleted the variant that held a
/// cell's own tree — a formula has no tree of its own any more, and the graph's node for a
/// text-only cell holds the shape its parse was interned as.
///
/// One table for what used to be asked of three — an expression map for the tree, the result map
/// for the value, and the result map again for "is it in flight" and for storing the value, five
/// hash lookups on the common path. The slots are dense and in the order the caller lists the cells
/// (row-major, which is also the order the write-back walks), so a value is a bounds-checked index
/// and evaluating a formula is one lookup.
///
/// The shape is **borrowed** from the graph's node rather than held as an `Arc`: filling a
/// million-formula pass used to be a million atomic increments (and as many decrements to drop the
/// table), for shapes the graph owns and outlives the pass with. That also merges what was a
/// `Vec<Key>` and a `Vec<Source>` written side by side — two pushes and two allocations a cell —
/// into one `Vec` of these.
///
/// The fields are ordered for the two walks over it: `shape` and `at` first, the two an evaluation
/// reads, then `key`, which the write-back reads. Thirty-two bytes a slot, so two slots share a
/// cache line and walking them in order touches each line once.
#[derive(Clone, Copy, Debug)]
struct Entry<'a> {
    /// The shape the cell shares with every cell of its fill.
    shape: &'a Shape,
    /// The cell its relative parts are offsets from: the slot's own cell.
    at: CellRef,
    /// The cell the slot evaluates.
    key: Key,
}

/// How much larger than the list a slot table may be before the hash table is kept instead: a box
/// of cells holding one listed formula in eight would cost eight writes a formula to zero-fill,
/// which is what a dirty set of a few cells far apart looks like.
const DENSE_FILL_RATIO: usize = 8;

/// The slots of a pass whose cells fill their box closely enough that one `u32` per box cell is
/// cheaper than a hash probe per lookup: a cell's slot is `(row - row0) * cols + (col - col0)`
/// within its sheet, and each sheet adds `rows * cols`.
///
/// `0` means "this pass does not evaluate the cell"; a slot is stored as its number plus one.
#[derive(Debug)]
struct DenseIndex {
    /// The first sheet, row and column of the box the cells fill.
    sheet0: usize,
    row0: u32,
    col0: u32,
    /// The rows and columns of the box, and the cells one sheet of it spans.
    rows: usize,
    cols: usize,
    sheet_stride: usize,
    slots: Vec<u32>,
}

impl DenseIndex {
    /// The box `cells` fill, when zero-filling it is worth it. `None` when they are too sparse:
    /// three cells a thousand rows apart would memset megabytes to save three probes.
    fn new(cells: &[Key]) -> Option<Self> {
        let (first_sheet, first) = *cells.first()?;
        let (mut sheet_lo, mut sheet_hi) = (first_sheet, first_sheet);
        let (mut row_lo, mut row_hi) = (first.row, first.row);
        let (mut col_lo, mut col_hi) = (first.col, first.col);
        for &(sheet, at) in cells {
            sheet_lo = sheet_lo.min(sheet);
            sheet_hi = sheet_hi.max(sheet);
            row_lo = row_lo.min(at.row);
            row_hi = row_hi.max(at.row);
            col_lo = col_lo.min(at.col);
            col_hi = col_hi.max(at.col);
        }
        let rows = row_hi.checked_sub(row_lo)?.checked_add(1)? as usize;
        let cols = col_hi.checked_sub(col_lo)?.checked_add(1)? as usize;
        let sheets = sheet_hi.checked_sub(sheet_lo)?.checked_add(1)?;
        let sheet_stride = rows.checked_mul(cols)?;
        let cells_in_box = sheets.checked_mul(sheet_stride)?;
        if cells_in_box > cells.len().checked_mul(DENSE_FILL_RATIO)? {
            return None;
        }
        // `try_reserve` instead of `vec![0; n]`: a box the allocator refuses is a reason to keep the
        // hash table, not to abort a recalculation.
        let mut slots: Vec<u32> = Vec::new();
        slots.try_reserve_exact(cells_in_box).ok()?;
        slots.resize(cells_in_box, 0);
        Some(Self { sheet0: sheet_lo, row0: row_lo, col0: col_lo, rows, cols, sheet_stride, slots })
    }

    /// Where `k` sits in the table, when it is inside the box.
    fn at(&self, k: Key) -> Option<usize> {
        let (sheet, at) = k;
        let row = at.row.checked_sub(self.row0)? as usize;
        let col = at.col.checked_sub(self.col0)? as usize;
        // Both are checked: a row past the box would otherwise land in the next sheet's rows, which
        // is why the table is only a plain rectangle for a pass of one sheet.
        if row >= self.rows || col >= self.cols {
            return None;
        }
        let i = sheet.checked_sub(self.sheet0)?.checked_mul(self.sheet_stride)?.checked_add(row.checked_mul(self.cols)?)?.checked_add(col)?;
        // A box spanning several sheets is not a rectangle: the rest of the last sheet is past the
        // table.
        (i < self.slots.len()).then_some(i)
    }
}

#[derive(Debug, Default)]
pub(super) struct Trees<'a> {
    /// Cell to slot for a pass whose cells the box does not fit, and for any key outside the box of
    /// a pass that has one. Empty for the common pass, where it is never asked.
    index: HashMap<Key, u32>,
    /// Cell to slot by arithmetic, when the cells fill their box (see [`DenseIndex`]).
    dense: Option<DenseIndex>,
    /// The slots, in the order the caller listed the cells — the order every walk over them takes.
    entries: Vec<Entry<'a>>,
}

impl<'a> Trees<'a> {
    /// Room for `cells` formulas, so a million-formula pass neither grows nor rehashes twice.
    pub(super) fn with_capacity(cells: usize) -> Self {
        Self { index: HashMap::with_capacity_and_hasher(cells, Default::default()), dense: None, entries: Vec::with_capacity(cells) }
    }

    /// The table for the cells a pass evaluates, listed in row-major order: a dense slot table when
    /// they fill their box, the hash table when they do not (see [`DenseIndex::new`]).
    pub(super) fn for_cells(cells: &[Key]) -> Self {
        Self::build(cells, true)
    }

    /// [`Trees::for_cells`] with the dense table refused, for the test that the two agree.
    #[cfg(test)]
    pub(super) fn for_cells_sparse(cells: &[Key]) -> Self {
        Self::build(cells, false)
    }

    /// Whether the cells are looked up by arithmetic (the sparseness fallback test reads it).
    #[cfg(test)]
    pub(super) fn is_dense(&self) -> bool {
        self.dense.is_some()
    }

    fn build(cells: &[Key], dense: bool) -> Self {
        let dense = if dense { DenseIndex::new(cells) } else { None };
        // A dense pass keeps its hash table empty: the box answers every cell it listed, so the
        // capacity the hash table would have taken is not allocated at all.
        let mut trees = Self::with_capacity(if dense.is_some() { 0 } else { cells.len() });
        trees.dense = dense;
        trees
    }

    /// Adds the formula at `k` to evaluate, unless the cell already has a slot. A pass of more than
    /// four billion formulas cannot be listed (a sheet holds 17 billion cells, but a workbook that
    /// large cannot be held in memory); cells past that limit would be left to the stored values
    /// rather than clobber the slots before them.
    ///
    /// A shape node is listed by its shape and its cell, not by a tree: the tree it reads is
    /// resolved when the pass needs it ([`Trees::source`]), so a million formulas of one fill hold
    /// one tree between them at rest.
    pub(super) fn push(&mut self, k: Key, node: &'a super::graph::Node) {
        // The shape is read where the graph keeps it: the pass borrows it for as long as it lives.
        let Some(shape) = node.shape.as_deref() else { return };
        self.add(k, shape);
    }

    /// [`Trees::push`] for a shape a test built itself (a pass a test runs without a graph): the
    /// caller keeps it alive for as long as the table, which is why a test parses its formulas into
    /// a list first and pushes from there.
    #[cfg(test)]
    pub(super) fn push_shape(&mut self, k: Key, shape: &'a Shape) {
        self.add(k, shape);
    }

    fn add(&mut self, k: Key, shape: &'a Shape) {
        let Ok(slot) = u32::try_from(self.entries.len()) else { return };
        // The dense table stores `slot + 1`, so the last number it can hold is one below the hash
        // table's. A pass that large is beyond what a workbook can hold in memory anyway.
        let Some(stored) = slot.checked_add(1) else { return };
        match self.dense.as_ref().and_then(|dense| dense.at(k)) {
            Some(i) => match self.dense.as_mut().and_then(|dense| dense.slots.get_mut(i)) {
                Some(cell) if *cell == 0 => *cell = stored,
                // The cell already has a slot, or the table lost it (which the width check rules
                // out): either way this cell is not listed twice.
                _ => return,
            },
            None => {
                if self.index.contains_key(&k) {
                    return;
                }
                self.index.insert(k, slot);
            }
        }
        self.entries.push(Entry { shape, at: k.1, key: k });
    }

    /// The slot of the formula at `k`, when this pass evaluates it.
    pub(super) fn slot(&self, k: Key) -> Option<u32> {
        if let Some(slot) = self.dense.as_ref().and_then(|dense| dense.at(k).and_then(|i| dense.slots.get(i).copied())) {
            // Inside the box the table answers alone — a cell it does not list has no slot — which
            // is the hash probe this saves on every read of the pass. (A key outside the box is
            // not in the table, so it keeps looking below.)
            return slot.checked_sub(1);
        }
        self.index.get(&k).copied()
    }

    /// The tree to evaluate in `slot` and the cell its relative parts are offsets from.
    ///
    /// A shape is read **in place**: the one tree of a million-cell fill is walked where it is,
    /// and each reference adds the cell's position as it is read (`Evaluator::ref_range`), instead
    /// of being resolved into a private tree per formula per pass (S4 of
    /// `plan/SHAPE-SHARING.md`).
    pub(super) fn source(&self, slot: u32) -> Option<(&Expr, Option<CellRef>)> {
        let entry = self.entries.get(slot as usize)?;
        Some((entry.shape.tree(), Some(entry.at)))
    }

    /// The address of the shape in `slot`: one fill's cells share one shape, so two rows of a
    /// chain compare equal in one load instead of re-reading the tree (`PassHost::chain_run`).
    pub(super) fn shape_ptr(&self, slot: u32) -> Option<usize> {
        self.entries.get(slot as usize).map(|entry| std::ptr::from_ref(entry.shape) as usize)
    }

    /// The cells, in the order their slots were added.
    pub(super) fn keys(&self) -> impl Iterator<Item = Key> + '_ {
        self.entries.iter().map(|entry| entry.key)
    }

    /// Each cell with its slot, for a pass that calculates every formula of the table.
    pub(super) fn iter(&self) -> impl Iterator<Item = (u32, Key)> + '_ {
        self.entries.iter().enumerate().filter_map(|(i, entry)| u32::try_from(i).ok().map(|slot| (slot, entry.key)))
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }
}

/// One formula's state in a pass: its value once calculated, and whether an evaluation for it is in
/// progress (which is what makes reading it a circular reference).
#[derive(Clone, Debug, Default)]
pub(super) struct Slot {
    value: Option<Value>,
    in_flight: bool,
}

impl Slot {
    /// A formula being evaluated.
    pub(super) fn running() -> Self {
        Self { value: None, in_flight: true }
    }

    /// A formula whose value is known.
    pub(super) fn done(value: Value) -> Self {
        Self { value: Some(value), in_flight: false }
    }

    /// The `#CIRC!` a circular formula shows.
    pub(super) fn circular() -> Self {
        Self::done(Value::Error(CellError::Circ))
    }

    /// The value, once calculated.
    pub(super) fn value(&self) -> Option<&Value> {
        self.value.as_ref()
    }

    /// The value, taken out of the slot.
    pub(super) fn into_value(self) -> Option<Value> {
        self.value
    }
}

/// The second operand of a chain formula: the same shape in every row of a run.
#[derive(Clone, Debug, PartialEq)]
enum Rhs {
    /// One cell, at a fixed offset from the formula's own cell: `=H2+G2` filled down reads the
    /// next row of `G` in every row.
    Cell { dr: i32, dc: i32 },
    /// A literal (`=H2+2`, `=H2*TRUE`).
    Literal(Value),
}

/// A chain shape: `prev <op> rhs`, where `prev` is the cell directly above in the same column.
/// Every cell of a run reads the same one, which is what lets a pass accumulate the run instead of
/// evaluating a formula a row (see [`PassHost::chain_run`]).
#[derive(Clone, Debug, PartialEq)]
struct Chain {
    op: BinOp,
    rhs: Rhs,
}

/// The chain `e` spells, when it is one, and nothing when it is anything else: another operator,
/// a reference that is not the cell above, a cross-sheet reference, a name, a function call, an
/// array, or a `$` part anywhere in it. The test is deliberately narrow — a shape it refuses keeps
/// the general evaluator, which is always right.
fn chain_of(e: &Expr) -> Option<Chain> {
    let Expr::Binary(op, a, b) = e else { return None };
    if !matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow) {
        return None;
    }
    let Expr::Ref(prev) = a.as_ref() else { return None };
    if !matches!(prev.sheet, SheetSel::Current) {
        return None;
    }
    let RefKind::Cell(above) = &prev.kind else { return None };
    // A relative offset of (-1, 0): the cell above, in this column.
    if above.row_abs || above.col_abs || above.row != -1 || above.col != 0 {
        return None;
    }
    let rhs = match b.as_ref() {
        Expr::Number(n) => Rhs::Literal(Value::number(*n)),
        Expr::Bool(x) => Rhs::Literal(Value::Bool(*x)),
        Expr::Text(t) => Rhs::Literal(Value::Text(t.clone())),
        Expr::Ref(r) => {
            if !matches!(r.sheet, SheetSel::Current) {
                return None;
            }
            let RefKind::Cell(c) = &r.kind else { return None };
            // A `$` part reads the same cell in every row, which is a chain of its own kind: the
            // run is defined over the whole-relative fill a formula dragged down makes, and a mixed
            // shape keeps the general path.
            if c.row_abs || c.col_abs {
                return None;
            }
            Rhs::Cell { dr: c.row, dc: c.col }
        }
        _ => return None,
    };
    Some(Chain { op: *op, rhs })
}

pub(super) struct PassHost<'a> {
    wb: &'a Workbook,
    /// The dependency graph as it stands at the start of the pass: an array's own references,
    /// for the self-overlap check.
    graph: &'a Graph,
    /// Resolved defined-name definitions, shared with the pass so that a name a column of
    /// formulas mentions is resolved and parsed once rather than once per cell (see
    /// [`crate::eval::NameCache`]).
    names: &'a mut crate::eval::NameCache,
    /// The formulas this pass evaluates: cell to slot, and the tree in each slot (see [`Trees`]).
    trees: &'a Trees<'a>,
    /// The anchors of legacy array formulas in this pass (usually none).
    legacy: &'a HashMap<Key, RangeRef>,
    /// One slot per formula in `trees`, in the same order.
    pub(super) slots: Vec<Slot>,
    /// Formulas whose slot is marked in flight. Zero for all of a row-major pass, which is when the
    /// "is it already being evaluated?" test can be skipped.
    in_flight: usize,
    /// New spills found during the pass: anchor → array.
    pub(super) spills: HashMap<Key, std::sync::Arc<Array>>,
    /// The areas of `spills`, in 16 x 16 blocks: a read of a cell a spill covers, and the areas a
    /// range read meets, are found without walking every array the pass has produced.
    pub(super) spill_index: SpillIndex,
    /// The spill areas the sheets hold at the start of the pass (what a previous pass laid out),
    /// indexed per sheet: an array may not spill over a cell one of those holds. Built the first
    /// time an array would spill over a cell that holds a spilled value, and only of the anchors
    /// this pass doesn't recalculate (those may spill elsewhere now, so they don't block). Taken
    /// out and given back like `spill_hits`, so nothing holds a borrow of it.
    stored_spills: std::cell::Cell<HashMap<usize, SpillIndex>>,
    /// Stored spill ranges the index above was built from (tests check the cost with it).
    #[cfg(test)]
    stored_visits: std::cell::Cell<u64>,
    /// Scratch buffer for [`PassHost::spill_hits`], reused across reads.
    spill_hits: Vec<CellRef>,
    /// Used range per sheet: the workbook doesn't change during a pass, and working it out walks
    /// every row, so once per sheet rather than once per range read.
    used: HashMap<usize, Option<RangeRef>>,
    /// Blocks with no formula in them, read once a pass and shared. A reporting workbook's eighty
    /// thousand lookups read the same few columns; rebuilding each block costs ~100 µs a formula.
    blocks: HashMap<(usize, RangeRef), std::sync::Arc<Array>>,
    block_cells: usize,
    /// Text-key indexes per shared block, keyed by the block's address. The block itself is held
    /// here so its address stays unique while the index exists.
    text_indexes: HashMap<usize, Option<(std::sync::Arc<Array>, std::sync::Arc<TextIndex>)>>,
    indexed_values: usize,
    rng: &'a mut u64,
    now: f64,
    pub(super) cycle: bool,
    pub(super) cycles: Vec<Key>,
    pub(super) depth: usize,
    /// A formula needed at [`MAX_NESTED`] levels deep: the evaluations in progress are abandoned
    /// and it is calculated first.
    pub(super) suspend: Option<Key>,
    /// Formulas whose evaluation was abandoned for another one, to be calculated again after it.
    /// They all need it, so reading one of them while it is calculated is a circular reference,
    /// as reading a formula in progress is.
    pub(super) waiting: HashSet<Key>,
    /// The formulas abandoned since the last suspension.
    pub(super) abandoned: Vec<Key>,
    /// Other formulas read at [`MAX_NESTED`] levels deep by the evaluations being abandoned. They
    /// are calculated along with the suspended one, rather than one per restart (a wide range
    /// read at the limit would restart the abandoned evaluations once per cell in it).
    pub(super) also: Vec<Key>,
    /// Suspensions so far (tests check that a wide read at the limit needs one, not one per cell).
    #[cfg(test)]
    pub(super) suspensions: usize,
    /// The formulas in `cycles`.
    cycle_seen: HashSet<Key>,
    /// Iterative calculation: a circular reference reads the cell's previous value instead of
    /// giving `#CIRC!`.
    pub(super) iterate: bool,
    /// Values from the previous iteration; cells not in it read their stored value.
    prev: HashMap<Key, Value>,
    /// Formulas being evaluated, innermost last (kept only when iterating).
    stack: Vec<Key>,
    /// Formulas found on a circular reference (when iterating).
    pub(super) members: HashSet<Key>,
    /// Results that read a previous value, directly or through other formulas.
    pub(super) tainted: HashSet<Key>,
    /// Whether the formula being evaluated has read a previous value.
    taint: bool,
    /// "Set precision as displayed" is on: results are rounded to their cells' formats before
    /// they are stored or read by other formulas.
    precision: Option<crate::DisplayedPrecision>,
    /// [`crate::eval::stack_position`] where the outermost formula being calculated started.
    stack_top: usize,
    /// Whether an evaluation ran past the stack budget since the innermost formula started.
    out_of_stack: bool,
    /// Runs of chain formulas this pass accumulated instead of evaluating a row (`CELLA_PROFILE`
    /// prints them), and the formulas they covered.
    pub(super) chains: u64,
    pub(super) chain_formulas: u64,
    /// Whether `chain_run` may take a run. Tests turn it off to calculate the same workbook through
    /// the general path and compare.
    #[cfg(test)]
    pub(super) chain_fast: bool,
}

impl<'a> PassHost<'a> {
    /// The anchors of this pass's spills whose area meets `range`, sorted, taken out of the
    /// scratch buffer. The caller gives it back (`self.spill_hits = hits`) once it is done with
    /// them, so a read allocates nothing.
    fn spill_hits(&mut self, sheet: usize, range: RangeRef) -> Vec<CellRef> {
        let mut hits = std::mem::take(&mut self.spill_hits);
        self.spill_index.meeting(sheet, range, &mut hits);
        hits
    }

    /// Whether a spill produced by this pass lies over `range`.
    fn spills_into(&mut self, sheet: usize, range: RangeRef) -> bool {
        let hits = self.spill_hits(sheet, range);
        let any = !hits.is_empty();
        self.spill_hits = hits;
        any
    }

    /// Records an array the pass laid out: its values, and the area it covers in the index, so a
    /// read of a cell it covers is one block lookup instead of a walk over every array.
    fn put_spill(&mut self, k: Key, a: &std::sync::Arc<Array>) {
        self.spill_index.insert(k, a.rows, a.cols);
        self.spills.insert(k, std::sync::Arc::clone(a));
    }

    /// Drops the array at `k` (blocked, circular, or recalculated by an iteration pass): its area
    /// leaves the index with it.
    fn drop_spill(&mut self, k: Key) {
        self.spill_index.remove(k);
        self.spills.remove(&k);
    }

    /// Entries this pass examined deciding whether an array spills over a stored spill: the stored
    /// ranges once each to index them, and the index entries lookups read (tests check the cost
    /// with it).
    #[cfg(test)]
    pub(super) fn block_probes(&self) -> u64 {
        let stored = self.stored_spills.take();
        let probes = stored.values().map(SpillIndex::probes).sum::<u64>();
        self.stored_spills.set(stored);
        self.stored_visits.get().saturating_add(probes)
    }

    /// The spill areas `sheet` holds at the start of the pass, in 16 x 16 blocks, without the
    /// anchors this pass recalculates: a cell one of them covers stops an array from spilling
    /// there. The workbook doesn't change during a pass, so one build a sheet answers every array.
    fn stored_spill_index(&self, sheet: usize) -> SpillIndex {
        let mut index = SpillIndex::default();
        let Some(sh) = self.wb.sheet(sheet) else { return index };
        #[cfg(test)]
        self.stored_visits.set(self.stored_visits.get().saturating_add(sh.spill_ranges.len() as u64));
        for (anchor, range) in &sh.spill_ranges {
            if self.trees.slot((sheet, *anchor)).is_none() {
                index.insert_rect((sheet, *anchor), *range, false);
            }
        }
        index
    }

    pub(super) fn new(
        wb: &'a Workbook,
        graph: &'a Graph,
        trees: &'a Trees<'a>,
        legacy: &'a HashMap<Key, RangeRef>,
        names: &'a mut crate::eval::NameCache,
        rng: &'a mut u64,
        now: f64,
    ) -> Self {
        PassHost {
            wb,
            graph,
            names,
            trees,
            legacy,
            slots: vec![Slot::default(); trees.len()],
            in_flight: 0,
            spills: HashMap::default(),
            spill_index: SpillIndex::default(),
            stored_spills: std::cell::Cell::default(),
            #[cfg(test)]
            stored_visits: std::cell::Cell::new(0),
            spill_hits: Vec::new(),
            used: HashMap::default(),
            blocks: HashMap::default(),
            block_cells: 0,
            text_indexes: HashMap::default(),
            indexed_values: 0,
            rng,
            now,
            cycle: false,
            cycles: vec![],
            depth: 0,
            suspend: None,
            waiting: HashSet::default(),
            abandoned: Vec::new(),
            also: Vec::new(),
            #[cfg(test)]
            suspensions: 0,
            cycle_seen: HashSet::default(),
            iterate: false,
            prev: HashMap::default(),
            stack: Vec::new(),
            members: HashSet::default(),
            tainted: HashSet::default(),
            taint: false,
            precision: wb.calc.precision_as_displayed.then(crate::DisplayedPrecision::new),
            stack_top: 0,
            out_of_stack: false,
            chains: 0,
            chain_formulas: 0,
            #[cfg(test)]
            chain_fast: true,
        }
    }

    /// The value the pass calculated for the formula at `k`, when it has one.
    pub(super) fn value(&self, k: Key) -> Option<&Value> {
        self.trees.slot(k).and_then(|slot| self.slot_value(slot))
    }

    /// The value in `slot`, once calculated.
    fn slot_value(&self, slot: u32) -> Option<&Value> {
        self.slots.get(slot as usize).and_then(Slot::value)
    }

    /// Puts a state in `slot`.
    fn set_slot(&mut self, slot: u32, state: Slot) {
        if let Some(s) = self.slots.get_mut(slot as usize) {
            *s = state;
        }
    }

    /// Takes the value out of `slot`, leaving it empty — the last iteration's value.
    fn take_slot(&mut self, slot: u32) -> Option<Value> {
        let state = std::mem::take(self.slots.get_mut(slot as usize)?);
        state.into_value()
    }
}

/// Whether an array at `anchor` covers `c`.
fn spill_covers(anchor: CellRef, arr: &Array, c: CellRef) -> bool {
    c.row >= anchor.row && c.col >= anchor.col && ((c.row - anchor.row) as usize) < arr.rows && ((c.col - anchor.col) as usize) < arr.cols
}

/// Whether two of the spill areas anchored at `hits` share a cell inside `range`.
fn spill_hits_overlap(index: &SpillIndex, sheet: usize, hits: &[CellRef], range: RangeRef) -> bool {
    let rects: Vec<RangeRef> = hits.iter().filter_map(|a| index.area(sheet, *a)).filter(|(_, empty)| !empty).map(|(r, _)| r).collect();
    rects.iter().enumerate().any(|(i, a)| {
        rects.iter().skip(i + 1).any(|b| {
            a.intersects(b)
                && RangeRef::new(
                    CellRef::new(a.start.row.max(b.start.row), a.start.col.max(b.start.col)),
                    CellRef::new(a.end.row.min(b.end.row), a.end.col.min(b.end.col)),
                )
                .intersects(&range)
        })
    })
}

/// Whether spilling `a` at `k` would write cells that `k`'s own formula reads. Reading a cell the
/// same formula writes is a circular reference (Excel: `#CIRC!`), not a conflict with anything
/// else: `=TRANSPOSE(A1:B6)` in A2 spills over part of its own source. A range that only touches
/// the anchor is the anchor reading itself, which the graph's cycle detection already refuses.
fn reads_own_spill(host: &PassHost<'_>, (sheet, anchor): Key, a: &Array) -> bool {
    let Some(node) = host.graph.node((sheet, anchor)) else { return false };
    let end =
        CellRef::new(anchor.row.saturating_add((a.rows as u32).saturating_sub(1)), anchor.col.saturating_add((a.cols as u32).saturating_sub(1)));
    let range = RangeRef::new(anchor, end);
    host.graph.areas_of(node).iter().any(|area| area.sheet() == sheet && area.range().intersects(&range))
}

/// How many formulas [`spill_feeds_its_source`] walks before it stops looking. A source range that
/// runs through thousands of formulas is not how an array reads its own spill back; the check has
/// to cost a little next to the pass it guards, so it gives up rather than walk a huge sheet.
const SPILL_LOOP_FORMULAS: usize = 4096;

/// Whether spilling `a` at `(sheet, anchor)` would write a cell that a formula the anchor's own
/// formula reads — directly, or through further formulas — also reads. The array's value then
/// depends on a cell it writes: a circular reference through its spill. The static graph cannot
/// see it (the spill's area is only known once the array is computed), and without the check the
/// pass alternated between two layouts — the array spilling, the reader feeding the source making
/// it `#SPILL!`, the reader firing it again — and stopped on whichever one the round limit met, so
/// the value in memory depended on how many rounds the sequence took to get there.
fn spill_feeds_its_source(host: &PassHost<'_>, (sheet, anchor): Key, a: &Array) -> bool {
    let Some(node) = host.graph.node((sheet, anchor)) else { return false };
    let end =
        CellRef::new(anchor.row.saturating_add((a.rows as u32).saturating_sub(1)), anchor.col.saturating_add((a.cols as u32).saturating_sub(1)));
    let range = RangeRef::new(anchor, end);
    let mut seen: HashSet<Key> = HashSet::default();
    let mut queue: VecDeque<PackedArea> = host.graph.areas_of(node).iter().copied().collect();
    while let Some(area) = queue.pop_front() {
        let Some(sh) = host.wb.sheet(area.sheet()) else { continue };
        for (c, cell) in sh.cells.iter_range(area.range()) {
            if cell.formula.is_none() {
                continue;
            }
            let k = (area.sheet(), c);
            // The anchor reading itself is circular whether or not it spills; `compute` refuses it
            // while it is in flight, and `reads_own_spill` handles the rest.
            if k == (sheet, anchor) || !seen.insert(k) {
                continue;
            }
            let Some(n) = host.graph.node(k) else { continue };
            if host.graph.areas_of(n).iter().any(|x| x.sheet() == sheet && x.range().intersects(&range)) {
                return true;
            }
            queue.extend(host.graph.areas_of(n).iter().copied());
            if seen.len() >= SPILL_LOOP_FORMULAS {
                return false;
            }
        }
    }
    false
}

/// Whether an array result at `k` can't spill for a reason of its own: it is in a table, or its
/// area runs off the sheet, overlaps merged cells, or holds other cells' content.
fn spill_blocked_by_cells(host: &PassHost<'_>, (sheet, anchor): Key, a: &Array) -> bool {
    let Some(sh) = host.wb.sheet(sheet) else { return false };
    let end =
        CellRef::new(anchor.row.saturating_add((a.rows as u32).saturating_sub(1)), anchor.col.saturating_add((a.cols as u32).saturating_sub(1)));
    let range = RangeRef::new(anchor, end);
    end.row >= cella_core::MAX_ROWS
        || end.col >= cella_core::MAX_COLS
        || sh.merges.iter().any(|m| m.intersects(&range))
        || sh.tables.iter().any(|t| t.range.contains(anchor))
        || sh.cells.iter_range(range).any(|(c, cell)| c != anchor && (!cell.value.is_empty() || cell.formula.is_some()))
}

/// Whether another formula's stored spill holds a cell of the array's area (unless that formula
/// is recalculated in this pass, and so may spill elsewhere now). A full recalculation clears the
/// spill maps before it starts, so this is never what a full pass answers an array with at compute
/// time; its write-back's layout order decides a contested area there.
fn spill_blocked_by_spill(host: &PassHost<'_>, (sheet, anchor): Key, a: &Array) -> bool {
    let Some(sh) = host.wb.sheet(sheet) else { return false };
    let end =
        CellRef::new(anchor.row.saturating_add((a.rows as u32).saturating_sub(1)), anchor.col.saturating_add((a.cols as u32).saturating_sub(1)));
    let range = RangeRef::new(anchor, end);
    // Only a cell that holds a spilled value can be one another formula's spill holds, so the
    // areas are looked for only when the array would cover one. Their index is built once a sheet
    // and pass: a cell inside it is a block lookup, where a walk over every stored range was one
    // containment check per cell of the array and per range the sheet holds.
    let mut over = sh.spill.range(range.start..=range.end).map(|(c, _)| *c).filter(|c| range.contains(*c)).peekable();
    if over.peek().is_none() {
        return false;
    }
    let own = sh.spill_ranges.get(&anchor);
    let mut index = host.stored_spills.take();
    let entry = index.entry(sheet).or_insert_with(|| host.stored_spill_index(sheet));
    let blocked = over.any(|c| !own.is_some_and(|o| o.contains(c)) && !matches!(entry.covering(sheet, c), Cover::None));
    host.stored_spills.set(index);
    blocked
}

/// Whether a cell's value says its formula is an array that spilled nothing: blocked (`#SPILL!`)
/// or circular through its own spill (`#CIRC!`). An edit where such an anchor would spill is the
/// only thing that can wake it: the sheet stores no spill range for it, so the edit names no
/// dependent of it.
pub(super) fn spilled_nothing(v: &Value) -> bool {
    matches!(v, Value::Error(CellError::Spill | CellError::Circ))
}

/// Whether `arr`, anchored at `anchor`, would spill exactly what the sheet already holds over
/// `range`: the same area, nothing new in the way, and the same values.
pub(super) fn spills_as_before(sheet: &cella_model::Sheet, anchor: CellRef, range: RangeRef, arr: &Array) -> bool {
    let end =
        CellRef::new(anchor.row.saturating_add((arr.rows as u32).saturating_sub(1)), anchor.col.saturating_add((arr.cols as u32).saturating_sub(1)));
    RangeRef::new(anchor, end) == range
        && !sheet.merges.iter().any(|m| m.intersects(&range))
        && !sheet.tables.iter().any(|t| t.range.contains(anchor))
        && range
            .iter()
            .enumerate()
            .all(|(i, c)| c == anchor || (!holds_content(sheet, c) && sheet.spill.get(&c) == Some(arr.data.get(i).unwrap_or(&Value::Empty))))
}

impl PassHost<'_> {
    /// Whether every formula this pass still owes a value for, among the cells of `range`, has
    /// one — what a block read asks before it treats a range full of formulas as constants.
    ///
    /// The walk takes the smaller side: a range with fewer cells than the pass has formulas is
    /// probed cell by cell (arithmetic in the dense table), and a pass with fewer formulas walks
    /// its own entries. Asking the *store* which of a million cells hold formulas instead built a
    /// `Cell` a cell — 90 ms for a whole-column read, and a pass whose three aggregates each read
    /// one paid it three times.
    fn settled_in(&self, sheet: usize, range: RangeRef) -> bool {
        let cells = u64::from(range.height()) * u64::from(range.width());
        if cells <= self.trees.len() as u64 {
            for row in range.start.row..=range.end.row {
                for col in range.start.col..=range.end.col {
                    if let Some(slot) = self.trees.slot((sheet, CellRef::new(row, col)))
                        && self.slot_value(slot).is_none()
                    {
                        return false;
                    }
                }
            }
            return true;
        }
        self.trees.iter().all(|(slot, key)| key.0 != sheet || !range.contains(key.1) || self.slot_value(slot).is_some())
    }
    /// Accumulates the run of chain formulas that starts at `k` (whose slot is `slot`), when `k`
    /// is one, and says whether it calculated `k`.
    ///
    /// A run is the rows below `k` in its column that this pass also evaluates and that read the
    /// same [`Chain`] — the shape an edit at the top of a large sheet mostly dirties (`H2=H1+G2`,
    /// `H3=H2+G3`, … a million rows of one formula). One `arith` step a row replaces a whole
    /// evaluation: no `Evaluator`, no slot marked in flight, no walk of the formula's tree, and the
    /// second operand is one read.
    ///
    /// The semantics are the general path's by construction, not by a second implementation of it:
    /// the values go through [`binary_values`], the kernel `Expr::Binary` itself evaluates with, and
    /// every operand is read through [`Host::cell_value`], the read the evaluator makes. So a blank
    /// is 0, a boolean or text is coerced, an error propagates, a division by zero is `#DIV/0!`, an
    /// overflow is `#NUM!` and a reference that leaves the sheet is `#REF!` — cell for cell the
    /// same value, and "Set precision as displayed" rounds each one as it lands.
    ///
    /// A row the pass has already calculated keeps its value, and an operand still to be calculated
    /// is calculated on demand here, exactly as the general path would when it reached the row.
    pub(super) fn chain_run(&mut self, k: Key, slot: u32) -> bool {
        #[cfg(test)]
        if !self.chain_fast {
            return false;
        }
        // Iterative calculation watches which formulas read a previous value and stops when they
        // settle; a run keeps none of that bookkeeping, so a workbook that iterates keeps the
        // general path. A suspension already unwinding is `calculate`'s business too.
        if self.iterate || self.suspend.is_some() || self.slot_value(slot).is_some() {
            return false;
        }
        let trees = self.trees;
        let (sheet, base) = k;
        let Some((expr, Some(at))) = trees.source(slot) else { return false };
        if at != base {
            return false;
        }
        let Some(chain) = chain_of(expr) else { return false };
        let shape = trees.shape_ptr(slot);
        // The cell above the run's first row is what its `prev` reference reads; past the top of
        // the sheet the reference is `#REF!`, which the first row then carries down the column.
        let mut acc = match base.offset(-1, 0) {
            Some(above) => self.cell_value(sheet, above),
            None => Value::Error(CellError::Ref),
        };
        if self.suspend.is_some() {
            return false;
        }
        let date_system = self.wb.date_system;
        let mut formulas = 0u64;
        let mut abandoned: Option<u32> = None;
        let mut step = 0u32;
        while let Some(cell) = base.offset(i64::from(step), 0) {
            let here = (sheet, cell);
            let Some(slot) = trees.slot(here) else { break };
            // The row must read the same chain: one fill's cells share one shape, so a pointer
            // comparison settles every row of it, and a shape of its own is read before it ends
            // the run on it.
            let own = trees.shape_ptr(slot);
            if step > 0 && own != shape {
                let Some((row_expr, Some(_))) = trees.source(slot) else { break };
                if chain_of(row_expr).as_ref() != Some(&chain) {
                    break;
                }
            }
            let next = step.saturating_add(1);
            if let Some(v) = self.slot_value(slot) {
                acc = v.clone();
                step = next;
                continue;
            }
            // A formula found circular is `#CIRC!` whatever its operands say; the rows from it
            // down are left to the general path, which is what the walk reaches next.
            if !self.cycle_seen.is_empty() && self.cycle_seen.contains(&here) {
                break;
            }
            let rhs = match &chain.rhs {
                Rhs::Literal(v) => v.clone(),
                Rhs::Cell { dr, dc } => match cell.offset(i64::from(*dr), i64::from(*dc)) {
                    Some(target) => self.cell_value(sheet, target),
                    None => Value::Error(CellError::Ref),
                },
            };
            // An operand read deeper than the stack or the nesting limit allows abandons the
            // evaluations in progress: the values this run has written are given back below.
            if self.suspend.is_some() {
                abandoned = Some(step);
                break;
            }
            let mut v = binary_values(chain.op, &acc, &rhs, date_system);
            if let Some(p) = self.precision.as_mut() {
                v = p.round_value(self.wb, sheet, cell, v);
            }
            self.set_slot(slot, Slot::done(v.clone()));
            acc = v;
            formulas = formulas.saturating_add(1);
            step = next;
        }
        if let Some(abandoned) = abandoned {
            // The run is abandoned: every cell it calculated is left to the pass again, which
            // recalculates `k` here (`calculate_at`) and the rest when its walk reaches them.
            for step in 0..=abandoned {
                if let Some(cell) = base.offset(i64::from(step), 0)
                    && let Some(slot) = trees.slot((sheet, cell))
                {
                    self.set_slot(slot, Slot::default());
                }
            }
            return false;
        }
        self.chains = self.chains.saturating_add(1);
        self.chain_formulas = self.chain_formulas.saturating_add(formulas);
        true
    }

    /// Calculates `k` (and the formulas it reads) with bounded stack: when [`Self::compute`] would
    /// go deeper than [`MAX_NESTED`], it suspends what is in progress, the formula it needed is
    /// calculated from here, then the suspended ones again (reading the results kept so far).
    pub(super) fn calculate(&mut self, k: Key) {
        let Some(slot) = self.trees.slot(k) else { return };
        self.calculate_at(k, slot);
    }

    /// Calculates the formula in `slot`, whose cell is `k`: the caller walks the pass's slots, so
    /// the cell's slot is known and is not looked up again.
    pub(super) fn calculate_at(&mut self, k: Key, slot: u32) {
        // The formula the caller asked for is evaluated with its slot, and the formulas it needs
        // past the depth limit are frames of their own, each with the ones abandoned to calculate
        // the next. That first formula is *not* a frame: it is kept here and its held formulas in
        // `held`, so a formula that never suspends — every ordinary chain — allocates nothing where
        // a one-element `Vec` was a malloc per cell.
        let mut work: Vec<(Key, Vec<Key>)> = Vec::new();
        let mut held: Vec<Key> = Vec::new();
        let mut top = k;
        let mut top_slot = Some(slot);
        loop {
            match top_slot.take() {
                Some(slot) => {
                    self.compute_at(top, slot);
                }
                None => {
                    self.compute(top);
                }
            }
            match self.suspend.take() {
                Some(next) => {
                    #[cfg(test)]
                    {
                        self.suspensions += 1;
                    }
                    let abandoned = std::mem::take(&mut self.abandoned);
                    self.waiting.extend(abandoned.iter().copied());
                    match work.last_mut() {
                        Some(entry) => entry.1 = abandoned,
                        None => held = abandoned,
                    }
                    // The other formulas read at the limit are calculated after `next`, before the
                    // held evaluations restart. Each has nothing held of its own, so the held ones
                    // are released only when the last of them is done.
                    let mut seen: HashSet<Key> = HashSet::default();
                    seen.insert(next);
                    for s in std::mem::take(&mut self.also) {
                        if seen.insert(s) {
                            work.push((s, Vec::new()));
                        }
                    }
                    work.push((next, Vec::new()));
                    top = next;
                }
                None => {
                    // This frame is done: release what it held and go back to the one below it —
                    // the first formula again once the last frame is gone.
                    if work.is_empty() {
                        return;
                    }
                    work.pop();
                    match work.last_mut() {
                        Some(entry) => {
                            for a in entry.1.drain(..) {
                                self.waiting.remove(&a);
                            }
                        }
                        None => {
                            for a in held.drain(..) {
                                self.waiting.remove(&a);
                            }
                        }
                    }
                    top = work.last().map_or(k, |entry| entry.0);
                }
            }
        }
    }

    fn compute(&mut self, k: Key) -> Value {
        let Some(slot) = self.trees.slot(k) else {
            return self.wb.sheet(k.0).map(|s| s.value(k.1)).unwrap_or_default();
        };
        self.compute_at(k, slot)
    }

    /// Calculates the formula at `k`, whose slot is `slot`: one lookup for the tree, the value it
    /// may already have and the "is it in flight" mark, where the pass used to ask three tables.
    fn compute_at(&mut self, k: Key, slot: u32) -> Value {
        let trees = self.trees;
        let current = self.slots.get(slot as usize).cloned().unwrap_or_default();
        if let Some(v) = current.value {
            if self.iterate && self.tainted.contains(&k) {
                self.taint = true;
            }
            return v;
        }
        // A slot marked in flight is a formula being evaluated: reading it is a circular reference.
        let running = self.in_flight > 0 && current.in_flight;
        let Some((expr, base)) = trees.source(slot) else {
            return self.wb.sheet(k.0).map(|s| s.value(k.1)).unwrap_or_default();
        };
        if self.suspend.is_some() {
            // Unwinding to `calculate`: the value is thrown away. A formula read at the limit is
            // calculated with the suspended one, so the evaluations are not restarted for each.
            if self.depth >= MAX_NESTED && !running && !self.waiting.contains(&k) {
                self.also.push(k);
            }
            return Value::Error(CellError::Calc);
        }
        if running || (!self.waiting.is_empty() && self.waiting.contains(&k)) {
            self.cycle = true;
            if self.iterate {
                return self.circular_read(k);
            }
            // Once each, though a suspended evaluation meets it again.
            if self.cycle_seen.insert(k) {
                self.cycles.push(k);
            }
            return Value::Error(CellError::Circ);
        }
        if self.depth >= MAX_NESTED {
            self.suspend = Some(k);
            return Value::Error(CellError::Calc);
        }
        if self.depth == 0 {
            self.stack_top = crate::eval::stack_position();
        }
        self.set_slot(slot, Slot::running());
        self.in_flight += 1;
        self.depth += 1;
        let outer_taint = std::mem::replace(&mut self.taint, false);
        if self.iterate {
            self.stack.push(k);
        }
        let outer_out_of_stack = std::mem::replace(&mut self.out_of_stack, false);
        let v = {
            let mut ev = Evaluator::new(self, k.0, k.1).at_base(base);
            ev.value(expr)
        };
        let out_of_stack = std::mem::replace(&mut self.out_of_stack, outer_out_of_stack);
        if out_of_stack && self.depth > 1 && self.suspend.is_none() {
            // It ran out of the stack left below the formulas that read it: calculate it from the
            // top instead, as a formula needed at `MAX_NESTED` levels deep is.
            self.suspend = Some(k);
            self.depth -= 1;
            self.set_slot(slot, Slot::default());
            self.in_flight = self.in_flight.saturating_sub(1);
            if self.iterate {
                self.stack.pop();
            }
            self.taint = outer_taint;
            return Value::Error(CellError::Calc);
        }
        if self.suspend.is_some() {
            // Abandoned: `calculate` evaluates this formula again once the one it needs is done.
            self.depth -= 1;
            self.set_slot(slot, Slot::default());
            self.in_flight = self.in_flight.saturating_sub(1);
            self.abandoned.push(k);
            if self.iterate {
                self.stack.pop();
            }
            self.taint = outer_taint;
            return Value::Error(CellError::Calc);
        }
        // A legacy array formula fills the range it was entered in.
        let legacy = if self.legacy.is_empty() { None } else { self.legacy.get(&k).copied() };
        let v = match legacy {
            Some(range) => fit_to_range(v, range),
            None => v,
        };
        self.depth -= 1;
        if self.iterate {
            self.stack.pop();
            if self.taint {
                self.tainted.insert(k);
            } else {
                self.tainted.remove(&k);
            }
        }
        self.taint |= outer_taint;
        // A blank cell shows as 0 in a formula's result, inside an array as well (`=A1:A3`,
        // FILTER or SORT of a range with blanks). GROUPBY and PIVOTBY lay out blank cells
        // of their own, which stay blank.
        let keep_blanks = matches!(expr, Expr::Call(n, _) if matches!(n.as_str(), "GROUPBY" | "PIVOTBY"));
        let v = match v {
            Value::Array(a) => {
                let a = if keep_blanks || !a.data.iter().any(|x| matches!(x, Value::Empty)) {
                    a
                } else {
                    let data = a.data.iter().map(|x| if matches!(x, Value::Empty) { Value::Number(0.0) } else { x.clone() }).collect();
                    Array::new(a.rows, a.cols, data).map(std::sync::Arc::new).unwrap_or(a)
                };
                // "Set precision as displayed": each element takes the format of the cell it
                // lands in.
                let a = match self.precision.as_mut() {
                    Some(p) if a.data.iter().any(|x| matches!(x, Value::Number(_))) => {
                        let cols = a.cols.max(1);
                        let data = a
                            .data
                            .iter()
                            .enumerate()
                            .map(|(i, x)| {
                                let at = CellRef::new(k.1.row.saturating_add((i / cols) as u32), k.1.col.saturating_add((i % cols) as u32));
                                p.round_value(self.wb, k.0, at, x.clone())
                            })
                            .collect();
                        Array::new(a.rows, a.cols, data).map(std::sync::Arc::new).unwrap_or(a)
                    }
                    _ => a,
                };
                // A dynamic-array formula anchors a spill range even while its result is one
                // value -- a FILTER matching one row, a SORT or UNIQUE over one item -- so `C1#`
                // is the 1 x 1 range and `ROWS(C1#)` is 1, as in Excel (#1491). A formula that
                // returns a scalar never reaches this arm, so it does not become an anchor.
                self.put_spill(k, &a);
                a.data.first().cloned().unwrap_or_default()
            }
            Value::Empty => Value::Number(0.0),
            v @ Value::Number(_) => match self.precision.as_mut() {
                Some(p) => p.round_value(self.wb, k.0, k.1, v),
                None => v,
            },
            v => v,
        };
        // An array that can't spill is #SPILL! for the formulas evaluated after it in this pass too.
        // One that spills over its own source is circular: no layout, `#CIRC!`.
        // Nothing can be spilled when the pass has laid none out: the lookup is skipped, and so is
        // the "did it read its own cell?" one below.
        let v = if self.spills.is_empty() {
            v
        } else {
            match self.spills.get(&k) {
                Some(a) if reads_own_spill(self, k, a) => {
                    self.drop_spill(k);
                    if self.cycle_seen.insert(k) {
                        self.cycles.push(k);
                    }
                    Value::Error(CellError::Circ)
                }
                // The area's own obstacles first: they hold whatever this pass does.
                Some(a) if spill_blocked_by_cells(self, k, a) => {
                    self.drop_spill(k);
                    Value::Error(CellError::Spill)
                }
                // A spill that feeds its own source is circular before a rival's stored area is
                // consulted: a full pass computes every array with the sheets' spill maps empty,
                // so this check is what answers there, and a later round of an incremental run
                // must not turn the same array #SPILL! on the strength of a spill only that
                // earlier round stored (found by the command-sequence fuzzer: a spill fight
                // re-based by deleteCells/deleteRows/deleteColumns, where the incremental state
                // said #SPILL! and a full recalculation said #CIRC!).
                Some(a) if spill_feeds_its_source(self, k, a) => {
                    self.drop_spill(k);
                    if self.cycle_seen.insert(k) {
                        self.cycles.push(k);
                    }
                    Value::Error(CellError::Circ)
                }
                Some(a) if spill_blocked_by_spill(self, k, a) => {
                    self.drop_spill(k);
                    Value::Error(CellError::Spill)
                }
                _ => v,
            }
        };
        // A formula that read its own cell — directly, or through a range read that came back to
        // it — is circular now, not only when the pass ends: a formula evaluated after it reads
        // the #CIRC! a full recalculation gives it, instead of a number computed from whatever the
        // cell showed mid-pass. Two formulas that feed each other that way otherwise swap values
        // every round until the round limit cuts the pass off in one of the two states.
        let v = if !self.iterate && !self.cycle_seen.is_empty() && self.cycle_seen.contains(&k) {
            self.drop_spill(k);
            Value::Error(CellError::Circ)
        } else {
            v
        };
        self.set_slot(slot, Slot::done(v.clone()));
        self.in_flight = self.in_flight.saturating_sub(1);
        v
    }

    /// A formula's previous value for iterative calculation: from the last iteration, else the
    /// value it shows. Blank, and the `#CIRC!` of a loop calculated with iteration off, are 0.
    fn previous(&self, k: Key) -> Value {
        let v = match self.prev.get(&k) {
            Some(v) => v.clone(),
            None => self.wb.sheet(k.0).map(|s| s.value(k.1)).unwrap_or_default(),
        };
        if matches!(v, Value::Empty | Value::Error(CellError::Circ)) { Value::Number(0.0) } else { v }
    }

    /// Reading `k` while it is being evaluated, with iterative calculation on: its previous value.
    /// The formulas around the loop are the ones whose changes decide when iteration stops.
    fn circular_read(&mut self, k: Key) -> Value {
        self.taint = true;
        match self.stack.iter().rposition(|s| *s == k) {
            Some(i) => self.members.extend(self.stack.get(i..).unwrap_or_default().iter().copied()),
            // Too deep to follow: the formula itself is watched.
            None => {
                self.members.insert(k);
            }
        }
        self.previous(k)
    }

    /// Iterative calculation, after a first pass over `order` that met circular references. The
    /// formulas that read a previous value (the loops and what depends on them) are evaluated
    /// again in the same order, each pass starting from the last one's values and reading what it
    /// has already produced (Gauss-Seidel), until no formula on a loop changes by `max_change` or
    /// more, or after `max_iterations` passes in all. Returns the number of passes.
    pub(super) fn iterate_loops(&mut self, order: &[Key], max_iterations: u32, max_change: f64) -> u32 {
        let again: Vec<Key> = order.iter().copied().filter(|k| self.tainted.contains(k)).collect();
        let set: HashSet<Key> = again.iter().copied().collect();
        // Bounds the work whatever Maximum Iterations says.
        let max_passes = u32::try_from(MAX_ITERATION_WORK / again.len().max(1)).unwrap_or(u32::MAX).max(1);
        let mut passes = 1u32;
        while passes < max_iterations.min(max_passes) && !self.converged(max_change) {
            let mut previous: HashMap<Key, Value> = HashMap::default();
            for k in &again {
                if let Some(slot) = self.trees.slot(*k)
                    && let Some(v) = self.take_slot(slot)
                {
                    previous.insert(*k, v);
                }
            }
            self.prev = previous;
            let gone: Vec<Key> = self.spills.keys().filter(|a| set.contains(a)).copied().collect();
            for a in gone {
                self.drop_spill(a);
            }
            for k in &again {
                self.calculate(*k);
            }
            passes += 1;
        }
        passes
    }

    /// Whether every formula on a loop changed by less than `max_change` in the last pass (text,
    /// logical and error values: not at all).
    fn converged(&self, max_change: f64) -> bool {
        self.members.iter().all(|k| {
            let Some(new) = self.value(*k) else { return true };
            match (new, &self.previous(*k)) {
                (Value::Number(a), Value::Number(b)) => {
                    let d = (a - b).abs();
                    d < max_change || d == 0.0
                }
                (new, old) => new == old,
            }
        })
    }
}

/// A legacy (Ctrl+Shift+Enter) array formula's result fitted to the range it was entered in, as
/// Excel does: a single row or column repeats across the range, a single value fills it, cells
/// past the result show `#N/A`, and a result larger than the range is cut off.
fn fit_to_range(v: Value, range: RangeRef) -> Value {
    if range.count() > crate::eval::MAX_CELLS {
        return Value::Error(CellError::Num);
    }
    let (h, w) = (range.height() as usize, range.width() as usize);
    let Value::Array(a) = v else {
        return if h * w == 1 { v } else { Array::new(h, w, vec![v; h * w]).map(Value::from).unwrap_or(Value::Error(CellError::Value)) };
    };
    if h * w == 1 {
        return a.data.first().cloned().unwrap_or_default();
    }
    let mut data = Vec::with_capacity(h * w);
    for r in 0..h {
        for c in 0..w {
            let (r, c) = (if a.rows == 1 { 0 } else { r }, if a.cols == 1 { 0 } else { c });
            data.push(a.get(r, c).cloned().unwrap_or(Value::Error(CellError::NA)));
        }
    }
    Array::new(h, w, data).map(Value::from).unwrap_or(Value::Error(CellError::Value))
}

impl Host for PassHost<'_> {
    fn workbook(&self) -> &Workbook {
        self.wb
    }
    fn stack_top(&self) -> Option<usize> {
        (self.depth > 1).then_some(self.stack_top)
    }
    fn stack_exhausted(&mut self) {
        self.out_of_stack = true;
    }
    fn name_target(&mut self, sheet: usize, name: &str) -> crate::eval::NameTarget {
        self.names.target(self.wb, sheet, name)
    }
    fn spill_range(&mut self, sheet: usize, anchor: CellRef) -> Option<RangeRef> {
        let k = (sheet, anchor);
        if self.trees.slot(k).is_none() {
            return self.wb.sheet(sheet)?.spill_ranges.get(&anchor).copied();
        }
        // Recalculated in this pass: its new array, unless something blocks it (#SPILL!).
        self.compute(k);
        let arr = self.spills.get(&k)?;
        let end = CellRef::new(
            anchor.row.saturating_add((arr.rows as u32).saturating_sub(1)),
            anchor.col.saturating_add((arr.cols as u32).saturating_sub(1)),
        );
        let range = RangeRef::new(anchor, end);
        let sh = self.wb.sheet(sheet)?;
        let blocked = end.row >= cella_core::MAX_ROWS
            || end.col >= cella_core::MAX_COLS
            || sh.cells.iter_range(range).any(|(c, cell)| c != anchor && (cell.formula.is_some() || !cell.value.is_empty()))
            || sh.merges.iter().any(|m| m.intersects(&range));
        (!blocked).then_some(range)
    }
    fn cell_value(&mut self, sheet: usize, c: CellRef) -> Value {
        let k = (sheet, c);
        if let Some(slot) = self.trees.slot(k) {
            return self.compute_at(k, slot);
        }
        // A cell covered by a spill computed in this pass. Where several overlap, the first in
        // `spills`' iteration order counts, as it always has. A pass that has laid no array out (the
        // common one) skips the index: no cell can be covered.
        let covering = if self.spills.is_empty() {
            None
        } else {
            match self.spill_index.covering(sheet, c) {
                Cover::None => None,
                Cover::One(a) => self.spills.get_key_value(&(sheet, a)),
                Cover::Many => self.spills.iter().find(|(a, arr)| a.0 == sheet && spill_covers(a.1, arr, c)),
            }
        };
        if let Some((anchor, arr)) = covering {
            let (dr, dc) = ((c.row - anchor.1.row) as usize, (c.col - anchor.1.col) as usize);
            if self.iterate && self.tainted.contains(anchor) {
                self.taint = true;
            }
            return arr.get(dr, dc).cloned().unwrap_or_default();
        }
        self.wb.sheet(sheet).map(|s| s.value(c)).unwrap_or_default()
    }
    fn exact_text_position(&mut self, hay: &std::sync::Arc<Array>, needle: &str, reverse: bool) -> Option<Option<usize>> {
        let key = std::sync::Arc::as_ptr(hay) as *const () as usize;
        if let Some(slot) = self.text_indexes.get(&key) {
            return slot.as_ref().map(|(_, index)| index.position(needle, reverse));
        }
        if hay.data.len() > MAX_INDEXED_TEXT_VALUES || self.indexed_values.saturating_add(hay.data.len()) > MAX_INDEXED_TEXT_VALUES_TOTAL {
            self.text_indexes.insert(key, None);
            return None;
        }
        let index = std::sync::Arc::new(TextIndex::build(&hay.data));
        self.indexed_values += hay.data.len();
        self.text_indexes.insert(key, Some((std::sync::Arc::clone(hay), std::sync::Arc::clone(&index))));
        Some(index.position(needle, reverse))
    }

    fn range_array(&mut self, sheet: usize, range: RangeRef) -> Option<std::sync::Arc<Array>> {
        // A block can only be reused while nothing that writes can change it: no formula cell in
        // it still to be worked out, no spill inside it, and no spill this pass has already
        // produced over it.
        if self.spills_into(sheet, range) {
            return None;
        }
        if let Some(hit) = self.blocks.get(&(sheet, range)) {
            return Some(std::sync::Arc::clone(hit));
        }
        let (h, w) = (range.height() as usize, range.width() as usize);
        let cells = h.checked_mul(w)?;
        if cells > MAX_CACHED_BLOCK_CELLS || self.block_cells.saturating_add(cells) > MAX_CACHED_BLOCK_CELLS_TOTAL {
            return None;
        }
        let sh = self.wb.sheet(sheet)?;
        if sh.spill.range(range.start..=range.end).next().is_some() {
            return None;
        }
        // A formula cell in the block usually rules the block out — its value is being worked
        // out. Once it has been, the pass never evaluates it again (`results` is authoritative),
        // so the block is as fixed as a block of constants, and the next formula that reads it
        // gets the same array. That is the shape of a reporting sheet: eighty thousand lookups
        // into one small table whose own few formulas were worked out on the first read.
        // A formula cell is settled when this pass has already worked it out, or when the pass is
        // not working it out at all (it keeps the value the file or the last calculation gave it).
        let stable = !self.iterate && self.settled_in(sheet, range);
        if !stable {
            return None;
        }
        let data = self.range_values(sheet, range);
        let array = std::sync::Arc::new(Array::new(h, w, data)?);
        self.block_cells += cells;
        self.blocks.insert((sheet, range), std::sync::Arc::clone(&array));
        Some(array)
    }
    fn range_values(&mut self, sheet: usize, range: RangeRef) -> Vec<Value> {
        let (h, w) = (range.height() as usize, range.width() as usize);
        let mut out = vec![Value::Empty; h * w];
        let Some(sh) = self.wb.sheet(sheet) else { return out };
        // Sparse walk: only stored cells; formula cells that are (or were) dirty go through
        // `compute`, constants are copied straight from the store.
        let mut formulas: Vec<(usize, CellRef)> = Vec::new();
        // A value-only walk: the block builder wants the value and the formula flag, not a `Cell`
        // with its formula `Arc` and style — 50 ms against 113 ms for a million-cell column.
        sh.cells.visit_values(range, &mut |c, value, has_formula| {
            let i = (c.row - range.start.row) as usize * w + (c.col - range.start.col) as usize;
            if has_formula {
                formulas.push((i, c));
            } else if let Some(slot) = out.get_mut(i) {
                *slot = value;
            }
            true
        });
        for (c, v) in sh.spill.range(range.start..=range.end) {
            if range.contains(*c) {
                let i = (c.row - range.start.row) as usize * w + (c.col - range.start.col) as usize;
                if let Some(slot) = out.get_mut(i)
                    && slot.is_empty()
                {
                    *slot = v.clone();
                }
            }
        }
        for (i, c) in formulas {
            let v = self.cell_value(sheet, c);
            if let Some(slot) = out.get_mut(i) {
                *slot = v;
            }
        }
        if !self.spills.is_empty() {
            // The spills that reach into the range are picked out from the block index, so a read
            // only looks at the part of each that lands in it: a sheet may hold many arrays, and a
            // whole-column read meets few of them.
            let hits = self.spill_hits(sheet, range);
            // Iterating: an array spilled by a formula that read a previous value.
            if self.iterate && hits.iter().any(|a| self.tainted.contains(&(sheet, *a))) {
                self.taint = true;
            }
            // Where areas overlap inside the range, the last in `spills`' iteration order wins, as
            // it always has; otherwise the order doesn't matter. Past `MAX_UNORDERED_HITS` areas
            // or one overlap, the whole set is walked in that order instead.
            let ordered = hits.len() > MAX_UNORDERED_HITS || spill_hits_overlap(&self.spill_index, sheet, &hits, range);
            let mut new: Vec<(usize, Value)> = Vec::new();
            let mut put = |anchor: CellRef, arr: &Array| {
                if arr.rows == 0 || arr.cols == 0 {
                    return;
                }
                let cells = RangeRef::new(
                    anchor,
                    CellRef::new(
                        anchor.row.saturating_add((arr.rows as u32).saturating_sub(1)),
                        anchor.col.saturating_add((arr.cols as u32).saturating_sub(1)),
                    ),
                );
                let (r0, c0) = (range.start.row.max(anchor.row) - anchor.row, range.start.col.max(anchor.col) - anchor.col);
                let (r1, c1) = (range.end.row.min(cells.end.row) - anchor.row, range.end.col.min(cells.end.col) - anchor.col);
                for r in r0..=r1 {
                    for cc in c0..=c1 {
                        let c = CellRef::new(anchor.row.saturating_add(r), anchor.col.saturating_add(cc));
                        if c != anchor && range.contains(c) {
                            new.push((
                                (c.row - range.start.row) as usize * w + (c.col - range.start.col) as usize,
                                arr.get(r as usize, cc as usize).cloned().unwrap_or_default(),
                            ));
                        }
                    }
                }
            };
            if ordered {
                let wanted: HashSet<CellRef> = hits.iter().copied().collect();
                for (a, arr) in &self.spills {
                    if a.0 == sheet && wanted.contains(&a.1) {
                        put(a.1, arr);
                    }
                }
            } else {
                for a in &hits {
                    if let Some(arr) = self.spills.get(&(sheet, *a)) {
                        put(*a, arr);
                    }
                }
            }
            for (i, v) in new {
                if let Some(slot) = out.get_mut(i) {
                    *slot = v;
                }
            }
            self.spill_hits = hits;
        }
        out
    }
    fn visit_aggregate(
        &mut self,
        sheet: usize,
        range: RangeRef,
        kind: crate::eval::Agg,
        acc: &mut crate::eval::Acc,
    ) -> Option<Result<(), cella_core::CellError>> {
        let wb = self.wb;
        let sh = wb.sheet(sheet)?;
        // Spilled values sit between the stored cells, and a spill may hold a number: leave such
        // blocks to `visit_range`, which knows about them.
        if self.spill_index.has_sheet(sheet) || sh.spill.range(range.start..=range.end).any(|(c, _)| range.contains(*c)) {
            return None;
        }
        let owned = sh.cells.has_owned_cells(range);
        // A settled block of one column is the shape of a whole-column SUM over formulas: the
        // formulas hold the values the last calculation left, this pass owes none of them, so the
        // fold reads them packed — no prepass, no `Sheet::value` a cell (139 ms → 55 ms at 1M).
        let settled_owned = owned
            && kind == crate::eval::Agg::Sum
            && range.width() == 1
            && range.height() >= 4096
            && !self.iterate
            && self.settled_in(sheet, range);
        if settled_owned {
            let mut needs_host = false;
            let mut error = None;
            sh.cells.visit_packed_settled(range, &mut |_, packed| {
                let Some(packed) = packed else {
                    needs_host = true;
                    return false;
                };
                match acc.add_packed(kind, packed) {
                    Ok(()) => true,
                    Err(e) => {
                        error = Some(e);
                        false
                    }
                }
            });
            return (!needs_host).then_some(error.map_or(Ok(()), Err));
        }
        if owned {
            // On a large formula column, avoid rebuilding its cells twice or staging formula
            // coordinates. Keep the small/multi-column and iterative cases on the old path.
            if kind != crate::eval::Agg::Sum || range.width() != 1 || range.height() < 4096 || self.iterate {
                return None;
            }
            sh.cells.visit_packed(range, &mut |at, packed| {
                if packed.is_none() {
                    let key = (sheet, at);
                    // Only dirty formulas can have evaluation side effects. A stored array or a
                    // clean formula (neither has a slot) needs no read on this first walk.
                    if let Some(slot) = self.trees.slot(key) {
                        let _ = self.compute_at(key, slot);
                    }
                }
                true
            });
            // Preserve visit_range's prepass order and do not touch Acc if it laid out a spill.
            if self.spill_index.has_sheet(sheet) {
                return None;
            }
        }
        let mut needs_host = false;
        let mut error = None;
        sh.cells.visit_packed(range, &mut |at, packed| {
            let result = match packed {
                Some(v) => acc.add_packed(kind, v),
                // Clean formulas read their cached workbook value; stored arrays and empty values
                // are ignored by add_cell, as in the original fold. The spill check is now final.
                None if owned => acc.add_cell(kind, &self.cell_value(sheet, at)),
                None => {
                    needs_host = true;
                    return false;
                }
            };
            match result {
                Ok(()) => true,
                Err(e) => {
                    error = Some(e);
                    false
                }
            }
        });
        (!needs_host).then_some(error.map_or(Ok(()), Err))
    }
    fn sumifs_numeric_columns(
        &self,
        target: crate::eval::Area,
        criteria: crate::eval::Area,
        criterion: cella_functions::NumericOrderingCriterion,
    ) -> Option<Value> {
        use cella_model::Packed;

        let sh = self.wb.sheet(target.sheet)?;
        if self.iterate || self.spill_index.has_sheet(target.sheet) || !sh.spill.is_empty() {
            return None;
        }
        let (target, criteria) = (target.range, criteria.range);
        let rows = criteria.height() as usize;
        let mut matches = vec![0u64; rows.div_ceil(64)];
        let complete = sh.cells.visit_packed(criteria, &mut |at, packed| match packed {
            Some(Packed::Number(n)) if criterion.matches(n) => {
                let i = (at.row - criteria.start.row) as usize;
                let Some(word) = matches.get_mut(i / 64) else { return false };
                *word |= 1u64 << (i % 64);
                true
            }
            Some(_) => true,
            // A formula or array may compute a matching value or lay out a spill.
            None => false,
        });
        if !complete {
            return None;
        }
        // Both walks are in logical row order. Keep scanning after the first matching error:
        // a later formula/array still forces fallback so it is evaluated as in range_values.
        let mut sum = -0.0f64;
        let mut error = None;
        let complete = sh.cells.visit_packed(target, &mut |at, packed| {
            let Some(packed) = packed else { return false };
            let i = (at.row - target.start.row) as usize;
            let Some(word) = matches.get(i / 64) else { return false };
            if error.is_none() && word & (1u64 << (i % 64)) != 0 {
                match packed {
                    Packed::Number(n) => sum += n,
                    Packed::Error(e) => error = Some(e),
                    _ => {}
                }
            }
            true
        });
        complete.then(|| error.map_or_else(|| Value::number(sum), Value::Error))
    }

    fn visit_range(&mut self, sheet: usize, range: RangeRef, f: &mut dyn FnMut(&Value) -> bool) -> bool {
        let wb = self.wb;
        let Some(sh) = wb.sheet(sheet) else { return false };
        // Formula cells first, in row-major order like `range_values`: reading one calculates it
        // and the pass remembers the value, so the walk below gets the same value again without
        // this method holding any.
        for (c, cell) in sh.cells.iter_range(range) {
            if cell.formula.is_some() {
                let _ = self.cell_value(sheet, c);
            }
        }
        // Spilled values sit between the stored cells (and the formulas just calculated may have
        // produced some): leave such blocks to `range_values`.
        if self.spill_index.has_sheet(sheet) || sh.spill.range(range.start..=range.end).any(|(c, _)| range.contains(*c)) {
            return false;
        }
        for (c, cell) in sh.cells.iter_range(range) {
            let go = if cell.formula.is_some() { f(&self.cell_value(sheet, c)) } else { f(&cell.value) };
            if !go {
                break;
            }
        }
        true
    }
    fn used_range(&mut self, sheet: usize) -> Option<RangeRef> {
        let wb = self.wb;
        *self.used.entry(sheet).or_insert_with(|| wb.sheet(sheet).and_then(|s| s.used_range()))
    }
    fn now_serial(&self) -> f64 {
        self.now
    }
    fn random(&mut self) -> f64 {
        // xorshift64*
        let mut x = *self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        *self.rng = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
}
