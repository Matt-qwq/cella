//! One recalculation pass: the Host its evaluator reads cells through, the spill layout, the
//! indexed and cached blocks, and the bounded-stack scheduler that suspends a formula needed too
//! deep and calculates it from the top.

use std::collections::VecDeque;

use super::Graph;
use super::graph::{HashMap, HashSet, Key, PackedArea};
use crate::eval::{Evaluator, Host};
use crate::spill_index::{Cover, SpillIndex};
use gridcraft_core::{Array, CellError, CellRef, RangeRef, Value};
use gridcraft_formula::Expr;
use gridcraft_model::Workbook;

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
pub(super) fn holds_content(sheet: &gridcraft_model::Sheet, c: CellRef) -> bool {
    sheet.cells.get(c).is_some_and(|x| x.formula.is_some() || !x.value.is_empty())
}

/// The largest single block the pass keeps built and shared. A lookup table is small; a whole
/// column read as an array is not, and rebuilding it is cheaper than holding it.
const MAX_CACHED_BLOCK_CELLS: usize = 4_000_000;
/// All shared blocks together, at 24 bytes a value.
const MAX_CACHED_BLOCK_CELLS_TOTAL: usize = 4_000_000;

/// The formulas a pass evaluates, one slot each: a cell's slot, and the tree to evaluate there.
///
/// One table for what used to be asked of three — an expression map for the tree, the result map
/// for the value, and the result map again for "is it in flight" and for storing the value, five
/// hash lookups on the common path. The slots are dense and in the order the caller lists the cells
/// (row-major, which is also the order the write-back walks), so a value is a bounds-checked index
/// and evaluating a formula is one lookup.
#[derive(Clone, Debug)]
enum Source {
    /// The cell's own cached tree.
    Tree(std::sync::Arc<Expr>),
    /// The shape the cell shares with every cell of its fill, and the cell it resolves at.
    Shape(std::sync::Arc<gridcraft_formula::Shape>, CellRef),
}

#[derive(Debug, Default)]
pub(super) struct Trees {
    index: HashMap<Key, u32>,
    keys: Vec<Key>,
    trees: Vec<Source>,
}

impl Trees {
    /// Room for `cells` formulas, so a million-formula pass neither grows nor rehashes twice.
    pub(super) fn with_capacity(cells: usize) -> Self {
        Self {
            index: HashMap::with_capacity_and_hasher(cells, Default::default()),
            keys: Vec::with_capacity(cells),
            trees: Vec::with_capacity(cells),
        }
    }

    /// Adds the formula at `k` to evaluate, unless the cell already has a slot. A pass of more than
    /// four billion formulas cannot be listed (a sheet holds 17 billion cells, but a workbook that
    /// large cannot be held in memory); cells past that limit would be left to the stored values
    /// rather than clobber the slots before them.
    ///
    /// A shape node is listed by its shape and its cell, not by a tree: the tree it reads is
    /// resolved when the pass needs it ([`Trees::expr`]), so a million formulas of one fill hold
    /// one tree between them at rest.
    pub(super) fn push(&mut self, k: Key, node: &super::graph::Node) {
        let source = match (&node.expr, &node.shape) {
            (Some(tree), _) => Source::Tree(std::sync::Arc::clone(tree)),
            (None, Some(shape)) => Source::Shape(std::sync::Arc::clone(shape), k.1),
            (None, None) => return,
        };
        self.add(k, source);
    }

    /// [`Trees::push`] with the tree itself, for a pass a test builds without a graph.
    #[cfg(test)]
    pub(super) fn push_tree(&mut self, k: Key, tree: &std::sync::Arc<Expr>) {
        self.add(k, Source::Tree(std::sync::Arc::clone(tree)));
    }

    fn add(&mut self, k: Key, source: Source) {
        if self.index.contains_key(&k) {
            return;
        }
        let Ok(slot) = u32::try_from(self.trees.len()) else { return };
        self.index.insert(k, slot);
        self.keys.push(k);
        self.trees.push(source);
    }

    /// The slot of the formula at `k`, when this pass evaluates it.
    pub(super) fn slot(&self, k: Key) -> Option<u32> {
        self.index.get(&k).copied()
    }

    /// The absolute tree to evaluate in `slot`: the cell's own, or its shape resolved at the cell.
    ///
    /// A shape cell's tree is built here and dropped when the evaluation ends, never kept: the
    /// whole point of the shape is that a million cells of one fill do not hold a million trees,
    /// and what is left is one hot tree walked a million times. (S4 of `plan/SHAPE-SHARING.md`
    /// removes even this by resolving references in the evaluator, at the cell it knows.)
    pub(super) fn expr(&self, slot: u32) -> Option<std::sync::Arc<Expr>> {
        match self.trees.get(slot as usize)? {
            Source::Tree(tree) => Some(std::sync::Arc::clone(tree)),
            Source::Shape(shape, at) => shape.resolve(*at).map(std::sync::Arc::new),
        }
    }

    /// The cells, in the order their slots were added.
    pub(super) fn keys(&self) -> &[Key] {
        &self.keys
    }

    /// Each cell with its slot, for a pass that calculates every formula of the table.
    pub(super) fn iter(&self) -> impl Iterator<Item = (u32, Key)> + '_ {
        self.keys.iter().enumerate().filter_map(|(i, k)| u32::try_from(i).ok().map(|slot| (slot, *k)))
    }

    pub(super) fn len(&self) -> usize {
        self.trees.len()
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
    trees: &'a Trees,
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
        trees: &'a Trees,
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
    let Some(node) = host.graph.nodes.get(&(sheet, anchor)) else { return false };
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
    let Some(node) = host.graph.nodes.get(&(sheet, anchor)) else { return false };
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
            let Some(n) = host.graph.nodes.get(&k) else { continue };
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

/// Whether an array result at `k` can't spill: it is in a table, or its area runs off the sheet,
/// overlaps merged cells, holds other cells, or another formula's spill (unless that formula is
/// recalculated in this pass, and so may spill elsewhere now).
fn spill_blocked(host: &PassHost<'_>, (sheet, anchor): Key, a: &Array) -> bool {
    let Some(sh) = host.wb.sheet(sheet) else { return false };
    let end =
        CellRef::new(anchor.row.saturating_add((a.rows as u32).saturating_sub(1)), anchor.col.saturating_add((a.cols as u32).saturating_sub(1)));
    let range = RangeRef::new(anchor, end);
    if end.row >= gridcraft_core::MAX_ROWS
        || end.col >= gridcraft_core::MAX_COLS
        || sh.merges.iter().any(|m| m.intersects(&range))
        || sh.tables.iter().any(|t| t.range.contains(anchor))
        || sh.cells.iter_range(range).any(|(c, cell)| c != anchor && (!cell.value.is_empty() || cell.formula.is_some()))
    {
        return true;
    }
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
pub(super) fn spills_as_before(sheet: &gridcraft_model::Sheet, anchor: CellRef, range: RangeRef, arr: &Array) -> bool {
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
        // Formulas to calculate, innermost last, each with the ones abandoned to calculate the
        // next.
        let mut work: Vec<(Key, Vec<Key>)> = vec![(k, Vec::new())];
        let mut first = Some(slot);
        while let Some(&(top, _)) = work.last() {
            match first.take() {
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
                    let held = std::mem::take(&mut self.abandoned);
                    self.waiting.extend(held.iter().copied());
                    if let Some(entry) = work.last_mut() {
                        entry.1 = held;
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
                }
                None => {
                    work.pop();
                    if let Some(entry) = work.last_mut() {
                        for a in entry.1.drain(..) {
                            self.waiting.remove(&a);
                        }
                    }
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
        let Some(expr) = trees.expr(slot) else {
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
            let mut ev = Evaluator::new(self, k.0, k.1);
            ev.value(&expr)
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
        let keep_blanks = matches!(&*expr, Expr::Call(n, _) if matches!(n.as_str(), "GROUPBY" | "PIVOTBY"));
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
                Some(a) if spill_blocked(self, k, a) => {
                    self.drop_spill(k);
                    Value::Error(CellError::Spill)
                }
                Some(a) if spill_feeds_its_source(self, k, a) => {
                    self.drop_spill(k);
                    if self.cycle_seen.insert(k) {
                        self.cycles.push(k);
                    }
                    Value::Error(CellError::Circ)
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
        let blocked = end.row >= gridcraft_core::MAX_ROWS
            || end.col >= gridcraft_core::MAX_COLS
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
        let stable = !self.iterate
            && sh.cells.formula_cells(range).all(|c| match self.trees.slot((sheet, c)) {
                Some(slot) => self.slot_value(slot).is_some(),
                None => true,
            });
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
        for (c, cell) in sh.cells.iter_range(range) {
            let i = (c.row - range.start.row) as usize * w + (c.col - range.start.col) as usize;
            if cell.formula.is_some() {
                formulas.push((i, c));
            } else if let Some(slot) = out.get_mut(i) {
                *slot = cell.value.clone();
            }
        }
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
    ) -> Option<Result<(), gridcraft_core::CellError>> {
        let wb = self.wb;
        let sh = wb.sheet(sheet)?;
        // Spilled values sit between the stored cells, and a spill may hold a number: leave such
        // blocks to `visit_range`, which knows about them.
        if self.spill_index.has_sheet(sheet) || sh.spill.range(range.start..=range.end).any(|(c, _)| range.contains(*c)) {
            return None;
        }
        // Decide before folding anything: a block with a formula in it is priced by `visit_range`,
        // which evaluates those first, and folding half of it here would count the rest twice.
        if sh.cells.has_owned_cells(range) {
            return None;
        }
        let mut needs_host = false;
        let mut error = None;
        let done = sh.cells.visit_packed(range, &mut |_, packed| {
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
        if needs_host {
            return None;
        }
        // A stop with no error is the closure's only other exit, so `!done` here means `error`.
        Some(match error {
            Some(e) => Err(e),
            None => {
                let _ = done;
                Ok(())
            }
        })
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
