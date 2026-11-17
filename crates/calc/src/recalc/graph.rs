//! The dependency graph: single-cell and interval precedents, and the Intervals index that
//! answers which dependents cover a cell without walking every range.

use std::collections::BTreeSet;
use std::hash::{BuildHasherDefault, Hasher};

use serde::Serialize;

use gridcraft_core::{CellRef, RangeRef};
use gridcraft_formula::Expr;

use crate::eval::Area;

/// A fast, non-cryptographic hasher for cell keys (FxHash-style multiply-rotate).
#[derive(Default, Clone, Copy)]
pub struct FxHasher(u64);

impl Hasher for FxHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.write_u64(*b as u64);
        }
    }
    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    fn write_u32(&mut self, i: u32) {
        self.write_u64(i as u64);
    }
    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }
}

pub(super) type HashMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<FxHasher>>;
pub(super) type HashSet<K> = std::collections::HashSet<K, BuildHasherDefault<FxHasher>>;

pub type Key = (usize, CellRef);

/// The formulas that read one precedent cell.
///
/// A sheet of a million rows is mostly cells one formula reads (`B2` feeding `G2`) and then cells
/// two do (`=$A$1` read by a whole column of formulas). Those live inline, so linking a formula
/// into the index allocates nothing at all; the third dependent moves the list to a `Vec` that
/// grows from there. The shape this replaced — a `HashSet` per precedent cell — allocated a table
/// for every cell a fill touched, which was the largest cost of building a million-row graph.
#[derive(Clone, Debug)]
pub(super) enum Deps {
    One(Key),
    Two(Key, Key),
    Many(Vec<Key>),
}

impl Deps {
    /// The list with `k` added, unless it is already there.
    ///
    /// A formula's areas are deduplicated before they reach here, so the graph itself never adds a
    /// dependent twice: the check is for short lists (a handful of compares) and is skipped for
    /// long ones, since a cell a million formulas read has to stay O(1) to add to.
    fn added(self, k: Key) -> Deps {
        match self {
            Deps::One(a) if a == k => Deps::One(a),
            Deps::One(a) => Deps::Two(a, k),
            Deps::Two(a, b) if a == k || b == k => Deps::Two(a, b),
            Deps::Two(a, b) => Deps::Many(vec![a, b, k]),
            Deps::Many(mut v) => {
                if v.len() > 8 || !v.contains(&k) {
                    v.push(k);
                }
                Deps::Many(v)
            }
        }
    }

    /// The list without `k`, or `None` when it held nothing else.
    fn without(self, k: Key) -> Option<Deps> {
        match self {
            Deps::One(a) => (a != k).then_some(Deps::One(a)),
            Deps::Two(a, b) => {
                if a == k {
                    Some(Deps::One(b))
                } else if b == k {
                    Some(Deps::One(a))
                } else {
                    Some(Deps::Two(a, b))
                }
            }
            Deps::Many(mut v) => {
                v.retain(|x| *x != k);
                match v.len() {
                    0 => None,
                    1 => v.pop().map(Deps::One),
                    _ => Some(Deps::Many(v)),
                }
            }
        }
    }

    fn push_into(&self, out: &mut Vec<Key>) {
        match self {
            Deps::One(a) => out.push(*a),
            Deps::Two(a, b) => {
                out.push(*a);
                out.push(*b);
            }
            Deps::Many(v) => out.extend(v.iter().copied()),
        }
    }
}

/// Adds the dependent `k` to what precedent cell `c` answers with.
fn link(map: &mut HashMap<Key, Deps>, c: Key, k: Key) {
    if let Some(deps) = map.get_mut(&c) {
        let taken = std::mem::replace(deps, Deps::One(k));
        *deps = taken.added(k);
    } else {
        map.insert(c, Deps::One(k));
    }
}

/// Drops the dependent `k` from what precedent cell `c` answers with; a cell nothing reads any
/// more leaves the map, so a query never walks an empty list.
fn unlink(map: &mut HashMap<Key, Deps>, c: Key, k: Key) {
    let Some(deps) = map.get_mut(&c) else { return };
    let taken = std::mem::replace(deps, Deps::One(k));
    match taken.without(k) {
        Some(left) => *deps = left,
        None => {
            map.remove(&c);
        }
    }
}

/// One formula of the graph.
#[derive(Clone, Debug)]
pub(super) struct Node {
    /// The node's precedent areas, as a run of [`Graph::areas`]: `areas[start..start + len]`.
    start: u32,
    len: u32,
    /// The parsed formula, shared with the cell: a pass reads it from here instead of rebuilding
    /// the cell (which copies its value and both side tables) once per formula.
    pub(super) expr: Option<std::sync::Arc<Expr>>,
    /// The `DYNAMIC`..`VISIBILITY` bits below.
    flags: u8,
}

/// The formula is evaluated every time (INDIRECT/OFFSET, a volatile function, a moving name).
const DYNAMIC: u8 = 1;
/// The formula may return an array and so spill: the parallel pass leaves it to the sequential one.
const MAY_SPILL: u8 = 2;
/// A reference is read through the sheet's used range (see [`crate::eval::trimmed`]).
const TRIM: u8 = 4;
/// The anchor of a legacy (Ctrl+Shift+Enter) array formula; its range is in [`Graph::legacy_range`].
const LEGACY: u8 = 8;
/// The formula calls a function that answers differently every time (`NOW`, `RAND`, `TODAY`…), so
/// its cached value is out of date on open and it is recalculated then.
const VOLATILE: u8 = 16;
/// The value depends on which rows are hidden (`SUBTOTAL` and `AGGREGATE` leave out the rows a
/// filter, a collapsed group or a row height of zero hides): a command that only changes
/// visibility evaluates these instead of the whole workbook.
const VISIBILITY: u8 = 32;

impl Node {
    pub(super) fn new(expr: Option<std::sync::Arc<Expr>>, dynamic: bool, may_spill: bool, trim: bool) -> Self {
        let mut flags = 0;
        if dynamic {
            flags |= DYNAMIC;
        }
        if may_spill {
            flags |= MAY_SPILL;
        }
        if trim {
            flags |= TRIM;
        }
        Node { start: 0, len: 0, expr, flags }
    }

    /// The same node, marked as reading which rows are hidden (see [`VISIBILITY`]).
    pub(super) fn with_visibility(mut self, hidden_rows: bool) -> Self {
        if hidden_rows {
            self.flags |= VISIBILITY;
        }
        self
    }

    /// The same node, marked volatile (see [`VOLATILE`]).
    pub(super) fn with_volatile(mut self, volatile: bool) -> Self {
        if volatile {
            self.flags |= VOLATILE;
        }
        self
    }

    /// Whether the formula calls a volatile function.
    pub(super) fn is_volatile(&self) -> bool {
        self.flags & VOLATILE != 0
    }
}

/// Ranges wider than this many columns go to the per-sheet wide index instead of per-column ones.
const WIDE: u32 = 64;

/// Insertions an [`Intervals`] keeps aside (scanned by every query) before merging them in.
const PENDING: usize = 32;

/// Freed areas an arena keeps before [`Graph::settle`] compacts it (and the live share it has to
/// reach: a fifth of the arena).
const AREA_GARBAGE: usize = 4096;

/// Row intervals `[r0, r1]`, each with a payload, answering "which intervals overlap rows
/// `a..=b`?" in O(log n + k).
///
/// The intervals are sorted by start with an implicit segment tree of their largest end, so a query
/// only descends into subtrees that hold a match. Removals mark entries dead (re-inserting one
/// revives it); insertions wait in a small sorted set. [`Intervals::settle`] merges them and drops
/// dead entries once either grows, so mutations cost O(log n) amortised.
#[derive(Clone, Debug)]
pub(super) struct Intervals<T> {
    /// Sorted, without duplicates.
    items: Vec<(u32, u32, T)>,
    dead: Vec<bool>,
    dead_count: usize,
    /// Segment tree over `items`: node `i` has children `2i` and `2i + 1`, leaves start at
    /// `max_end.len() / 2`. Each node holds the largest end below it (dead entries included).
    max_end: Vec<u32>,
    /// Inserted since the last merge.
    pending: BTreeSet<(u32, u32, T)>,
}

impl<T> Default for Intervals<T> {
    fn default() -> Self {
        Intervals { items: Vec::new(), dead: Vec::new(), dead_count: 0, max_end: Vec::new(), pending: BTreeSet::new() }
    }
}

impl<T: Copy + Ord> Intervals<T> {
    pub(super) fn is_empty(&self) -> bool {
        self.items.len() == self.dead_count && self.pending.is_empty()
    }

    pub(super) fn insert(&mut self, r0: u32, r1: u32, t: T) {
        let it = (r0, r1, t);
        if let Ok(i) = self.items.binary_search(&it) {
            if let Some(d) = self.dead.get_mut(i)
                && *d
            {
                *d = false;
                self.dead_count -= 1;
            }
            return;
        }
        self.pending.insert(it);
    }

    pub(super) fn remove(&mut self, r0: u32, r1: u32, t: T) {
        let it = (r0, r1, t);
        if let Ok(i) = self.items.binary_search(&it) {
            if let Some(d) = self.dead.get_mut(i)
                && !*d
            {
                *d = true;
                self.dead_count += 1;
            }
            return;
        }
        self.pending.remove(&it);
    }

    /// Heap bytes of this index: its items, the dead flags, the segment tree and the pending
    /// insertions (see [`GraphMemory`]).
    pub(super) fn memory(&self) -> usize {
        use std::mem::size_of;
        let item = size_of::<(u32, u32, T)>();
        self.items
            .capacity()
            .saturating_mul(item)
            .saturating_add(self.dead.capacity())
            .saturating_add(self.max_end.capacity().saturating_mul(size_of::<u32>()))
            // A `BTreeSet` has no capacity: charge the nodes its `len` entries sit in.
            .saturating_add(gridcraft_model::btree_index_bytes(self.pending.len(), item))
    }
    /// Merges pending insertions and drops dead entries when there are enough of them.
    pub(super) fn settle(&mut self) {
        if self.pending.len() <= PENDING && self.dead_count <= (self.items.len() / 2).max(PENDING) {
            return;
        }
        let mut all: Vec<(u32, u32, T)> = Vec::with_capacity(self.items.len() - self.dead_count + self.pending.len());
        all.extend(self.items.iter().zip(&self.dead).filter(|(_, d)| !**d).map(|(it, _)| *it));
        all.extend(std::mem::take(&mut self.pending));
        // Two sorted runs: the stable sort merges them in linear time.
        all.sort();
        all.dedup();
        let size = all.len().next_power_of_two();
        let mut max_end = vec![0; 2 * size];
        if let Some(leaves) = max_end.get_mut(size..) {
            for (slot, it) in leaves.iter_mut().zip(&all) {
                *slot = it.1;
            }
        }
        for i in (1..size).rev() {
            let m = max_end.get(2 * i).copied().unwrap_or(0).max(max_end.get(2 * i + 1).copied().unwrap_or(0));
            if let Some(slot) = max_end.get_mut(i) {
                *slot = m;
            }
        }
        self.dead = vec![false; all.len()];
        self.dead_count = 0;
        self.items = all;
        self.max_end = max_end;
    }

    /// Calls `f` for every interval overlapping rows `a..=b` (`r0 <= b && r1 >= a`). Returns the
    /// number of tree nodes visited.
    pub(super) fn query(&self, a: u32, b: u32, f: &mut dyn FnMut(&T)) -> usize {
        for it in &self.pending {
            if it.0 <= b && it.1 >= a {
                f(&it.2);
            }
        }
        // Entries before `p` start at or before row `b`.
        let p = self.items.partition_point(|it| it.0 <= b);
        if p == 0 {
            return 0;
        }
        self.walk(1, 0, self.max_end.len() / 2, p, a, f)
    }

    fn walk(&self, node: usize, lo: usize, hi: usize, p: usize, a: u32, f: &mut dyn FnMut(&T)) -> usize {
        if lo >= p || self.max_end.get(node).is_none_or(|m| *m < a) {
            return 1;
        }
        if hi - lo <= 1 {
            if let (Some(it), Some(false)) = (self.items.get(lo), self.dead.get(lo))
                && it.1 >= a
            {
                f(&it.2);
            }
            return 1;
        }
        let mid = lo + (hi - lo) / 2;
        1 + self.walk(2 * node, lo, mid, p, a, f) + self.walk(2 * node + 1, mid, hi, p, a, f)
    }
}

/// The live heap bytes of a [`Graph`], by allocation class: what [`Graph::memory`] returns.
///
/// Every figure is a `len`/`capacity`/*`size_of`* product. The one pass over entries is the
/// `Deps::Many` vectors ([`GraphMemory::cell_deps`]): a `Vec`'s capacity is not derivable from the
/// map's own capacity, so the values are visited once — no allocation, about a nanosecond an entry
/// (a few milliseconds on the reference workload's 1.5M precedent cells, which is why this is a
/// diagnostic and not something to ask for every frame).
///
/// Not counted: the parsed expression trees the nodes point at. The cell owns the tree (its text
/// is the source of truth and the store's report charges the tree), so charging it here would
/// count one allocation twice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct GraphMemory {
    /// The `Key → Node` table: its slots and control bytes.
    pub nodes: usize,
    /// The precedent-area arena, including the garbage [`Graph::settle`] has not packed away.
    pub areas: usize,
    /// The single-cell precedent → dependents table, and every `Deps::Many` vector it holds.
    pub cell_deps: usize,
    /// How many of that table's entries are `Deps::Many` lists (what the one walk above counts).
    pub many_lists: usize,
    /// The per-column interval index of range precedents.
    pub cols: usize,
    /// The per-sheet interval index of ranges wider than 64 columns.
    pub wide: usize,
    /// Anchor → range of a legacy (Ctrl+Shift+Enter) array formula.
    pub legacy_ranges: usize,
    /// The `dynamic`/`may_spill`/`legacy`/`trim`/`visibility` sets.
    pub flags: usize,
    /// The range indexes changed since the last [`Graph::settle`].
    pub unsettled: usize,
    /// The buffers `insert`/`remove` sort their index entries in.
    pub scratch: usize,
}

impl GraphMemory {
    pub fn total(&self) -> usize {
        [self.nodes, self.areas, self.cell_deps, self.cols, self.wide, self.legacy_ranges, self.flags, self.unsettled, self.scratch]
            .iter()
            .fold(0usize, |sum, part| sum.saturating_add(*part))
    }
}

/// Heap bytes of one hash set of `T` (see [`GraphMemory`]).
fn set_bytes<T>(set: &HashSet<T>) -> usize {
    gridcraft_model::hash_index_bytes(set.capacity(), std::mem::size_of::<T>())
}
/// The formulas that read which cells.
///
/// Precedent areas live in one arena instead of a `Vec` per node: a rebuild of a million formulas
/// then fills one vector rather than allocating a million, and a node is eight bytes smaller. A
/// replaced node's run becomes garbage that [`Graph::settle`] drops once it is a large enough share
/// of the arena.
#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub(super) nodes: HashMap<Key, Node>,
    /// Every node's precedent areas, packed: a node owns `areas[start..start + len]`.
    areas: Vec<Area>,
    /// Areas no node points at any more.
    area_garbage: usize,
    /// precedent cell → the formulas that read it
    pub(super) cell_deps: HashMap<Key, Deps>,
    /// (sheet, col) → row intervals of the ranges (up to `WIDE` columns) that cover the column
    pub(super) col_deps: HashMap<(usize, u32), Intervals<Key>>,
    /// sheet → row intervals of wider ranges, with their columns: (first, last, dependent)
    pub(super) wide_deps: HashMap<usize, Intervals<(u32, u32, Key)>>,
    /// The range of a legacy (Ctrl+Shift+Enter) array formula, for the anchor that holds it. A
    /// side table because the range is 16 bytes a formula would have to carry to say "not an
    /// array" — and almost no formula is one.
    legacy_range: HashMap<Key, RangeRef>,
    pub(super) dynamic: HashSet<Key>,
    /// Formulas whose value may be an array (and so spill), and the anchors of legacy arrays.
    /// A pass checks these against its dirty set: a dirty one means the pass cannot be split.
    pub(super) may_spill: HashSet<Key>,
    pub(super) legacy: HashSet<Key>,
    /// Formulas with a reference read through the used range. An edit that moves the used range
    /// (or a spill that does) has to evaluate them again: they read cells it didn't name.
    pub(super) trim: HashSet<Key>,
    /// Formulas whose value depends on which rows are hidden (`SUBTOTAL`, `AGGREGATE`). A filter,
    /// a group collapsed or shown and a row height of zero change no cell's content, only what
    /// they display, so they evaluate this set — and its dependents — instead of the workbook.
    pub(super) visibility: HashSet<Key>,
    /// Range indexes changed since the last [`Graph::settle`].
    pub(super) unsettled_cols: HashSet<(usize, u32)>,
    pub(super) unsettled_wide: HashSet<usize>,
    /// Buffers `insert`/`remove` sort a node's index entries in, so linking a formula into the
    /// graph allocates nothing once the graph is warm.
    scratch_cells: Vec<Key>,
    scratch_cols: Vec<(usize, u32, u32, u32)>,
    scratch_wide: Vec<(usize, RangeRef)>,
}

impl Graph {
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The precedent areas of a node.
    pub(super) fn areas_of(&self, node: &Node) -> &[Area] {
        let start = node.start as usize;
        self.areas.get(start..start.saturating_add(node.len as usize)).unwrap_or(&[])
    }

    /// The number of areas the arena holds (live ones plus the garbage `settle` has not packed
    /// away yet) — tests check that a bulk replacement does not leave them behind.
    #[cfg(test)]
    pub(super) fn areas_len(&self) -> usize {
        self.areas.len()
    }

    /// The range of the legacy array anchored at `k`, if it is one.
    pub(super) fn legacy_of(&self, k: Key) -> Option<RangeRef> {
        self.legacy_range.get(&k).copied()
    }

    /// Whether the formula at `k` calls a volatile function — the flag the precedents walk
    /// recorded when it visited its last node, so opening a workbook never walks a formula tree to
    /// find out. A cell without a node (no formula, or one that does not parse) is not volatile.
    pub(super) fn is_volatile(&self, k: Key) -> bool {
        self.nodes.get(&k).is_some_and(Node::is_volatile)
    }

    /// Reserves room for `formulas` formulas, so a bulk build does not grow its tables a dozen
    /// times (each growth rehashes every entry already in it).
    pub(super) fn reserve(&mut self, formulas: usize) {
        self.nodes.reserve(formulas);
        let precedent_cells = formulas.saturating_mul(3).saturating_div(2);
        self.cell_deps.reserve(precedent_cells);
        self.areas.reserve(formulas.saturating_mul(2));
    }

    /// Index entries of a formula's precedents, without duplicates: single cells, column
    /// intervals and wide ranges. The buffers are reused between calls, so this allocates nothing.
    fn entries(areas: &[Area], cells: &mut Vec<Key>, cols: &mut Vec<(usize, u32, u32, u32)>, wide: &mut Vec<(usize, RangeRef)>) {
        cells.clear();
        cols.clear();
        wide.clear();
        for a in areas {
            if a.range.is_single() {
                cells.push((a.sheet, a.range.start));
            } else if a.range.width() > WIDE {
                wide.push((a.sheet, a.range));
            } else {
                for c in a.range.start.col..=a.range.end.col {
                    cols.push((a.sheet, c, a.range.start.row, a.range.end.row));
                }
            }
        }
        cells.sort_unstable();
        cells.dedup();
        cols.sort_unstable();
        cols.dedup();
        wide.sort_unstable_by_key(|(s, r)| (*s, r.start, r.end));
        wide.dedup();
    }

    /// Takes the buffers [`Graph::entries`] fills, so they are not borrowed while the indexes they
    /// describe are updated.
    fn take_scratch(&mut self) -> (Vec<Key>, Vec<(usize, u32, u32, u32)>, Vec<(usize, RangeRef)>) {
        (std::mem::take(&mut self.scratch_cells), std::mem::take(&mut self.scratch_cols), std::mem::take(&mut self.scratch_wide))
    }

    fn put_scratch(&mut self, cells: Vec<Key>, cols: Vec<(usize, u32, u32, u32)>, wide: Vec<(usize, RangeRef)>) {
        self.scratch_cells = cells;
        self.scratch_cols = cols;
        self.scratch_wide = wide;
    }

    /// Adds a formula to the graph, replacing whatever the cell held.
    ///
    /// `areas` are the formula's precedents, `legacy` the range of a legacy array it anchors.
    pub(super) fn insert(&mut self, k: Key, areas: &[Area], legacy: Option<RangeRef>, node: Node) {
        // The arena cannot hold more than `u32::MAX` areas (96 GB of them); a run that large is
        // not a workbook, and capping keeps the arithmetic below total.
        let start = u32::try_from(self.areas.len()).unwrap_or(u32::MAX);
        let len = u32::try_from(areas.len()).unwrap_or(u32::MAX);
        self.areas.extend_from_slice(areas);
        let mut node = Node { start, len, ..node };

        if legacy.is_some() {
            node.flags |= LEGACY;
        }
        let flags = node.flags;
        // One table probe: the node this replaces (if any) is unlinked after the new one has taken
        // its place, which is what a bulk build — a fill of a million formulas — pays per formula.
        if let Some(old) = self.nodes.insert(k, node) {
            self.unlink(k, &old);
        }
        self.link_precedents(k, areas);
        if flags & DYNAMIC != 0 {
            self.dynamic.insert(k);
        }
        if flags & MAY_SPILL != 0 {
            self.may_spill.insert(k);
        }
        if flags & TRIM != 0 {
            self.trim.insert(k);
        }
        if flags & VISIBILITY != 0 {
            self.visibility.insert(k);
        }
        if let Some(r) = legacy {
            self.legacy.insert(k);
            self.legacy_range.insert(k, r);
        }
    }

    pub(super) fn remove(&mut self, k: Key) {
        if let Some(old) = self.nodes.remove(&k) {
            self.unlink(k, &old);
        }
    }

    /// Drops a replaced node's index entries and frees its areas.
    fn unlink(&mut self, k: Key, old: &Node) {
        let (mut cells, mut cols, mut wide) = self.take_scratch();
        let start = old.start as usize;
        let old_areas = self.areas.get(start..start.saturating_add(old.len as usize)).unwrap_or(&[]);
        Self::entries(old_areas, &mut cells, &mut cols, &mut wide);
        for &c in &cells {
            unlink(&mut self.cell_deps, c, k);
        }
        for &(s, c, r0, r1) in &cols {
            if let Some(v) = self.col_deps.get_mut(&(s, c)) {
                v.remove(r0, r1, k);
                self.unsettled_cols.insert((s, c));
            }
        }
        for &(s, r) in &wide {
            if let Some(v) = self.wide_deps.get_mut(&s) {
                v.remove(r.start.row, r.end.row, (r.start.col, r.end.col, k));
                self.unsettled_wide.insert(s);
            }
        }
        self.put_scratch(cells, cols, wide);
        if old.flags & DYNAMIC != 0 {
            self.dynamic.remove(&k);
        }
        if old.flags & MAY_SPILL != 0 {
            self.may_spill.remove(&k);
        }
        if old.flags & TRIM != 0 {
            self.trim.remove(&k);
        }
        if old.flags & VISIBILITY != 0 {
            self.visibility.remove(&k);
        }
        if old.flags & LEGACY != 0 {
            self.legacy.remove(&k);
            self.legacy_range.remove(&k);
        }
        self.area_garbage = self.area_garbage.saturating_add(old.len as usize);
    }

    /// Adds a formula to the index of everything it reads.
    fn link_precedents(&mut self, k: Key, areas: &[Area]) {
        let (mut cells, mut cols, mut wide) = self.take_scratch();
        Self::entries(areas, &mut cells, &mut cols, &mut wide);
        for &c in &cells {
            link(&mut self.cell_deps, c, k);
        }
        for &(s, c, r0, r1) in &cols {
            self.col_deps.entry((s, c)).or_default().insert(r0, r1, k);
            self.unsettled_cols.insert((s, c));
        }
        for &(s, r) in &wide {
            self.wide_deps.entry(s).or_default().insert(r.start.row, r.end.row, (r.start.col, r.end.col, k));
            self.unsettled_wide.insert(s);
        }
        self.put_scratch(cells, cols, wide);
    }

    /// Tidies the range indexes changed by `insert`/`remove` (queries stay correct without it,
    /// just slower), and gives the arena's garbage back once it is a large enough share of it.
    pub(super) fn settle(&mut self) {
        for key in std::mem::take(&mut self.unsettled_cols) {
            if let Some(v) = self.col_deps.get_mut(&key) {
                v.settle();
                if v.is_empty() {
                    self.col_deps.remove(&key);
                }
            }
        }
        for key in std::mem::take(&mut self.unsettled_wide) {
            if let Some(v) = self.wide_deps.get_mut(&key) {
                v.settle();
                if v.is_empty() {
                    self.wide_deps.remove(&key);
                }
            }
        }
        if self.area_garbage > AREA_GARBAGE && self.area_garbage > self.areas.len() / 5 {
            self.compact_areas();
        }
    }

    /// Packes the live areas together, in one pass over the nodes. Runs once the garbage a
    /// replaced formula left behind outweighs a fifth of the arena, so its cost — a walk of the
    /// nodes — is paid for by the edits that filled it.
    fn compact_areas(&mut self) {
        let old = std::mem::take(&mut self.areas);
        let live = old.len().saturating_sub(self.area_garbage);
        let mut areas: Vec<Area> = Vec::with_capacity(live);
        for node in self.nodes.values_mut() {
            let start = node.start as usize;
            let run = old.get(start..start.saturating_add(node.len as usize)).unwrap_or(&[]);
            node.start = u32::try_from(areas.len()).unwrap_or(u32::MAX);
            areas.extend_from_slice(run);
        }
        self.areas = areas;
        self.area_garbage = 0;
    }

    /// Formulas that read cell `c` of `sheet`.
    pub fn dependents(&self, sheet: usize, c: CellRef, out: &mut Vec<Key>) {
        if let Some(v) = self.cell_deps.get(&(sheet, c)) {
            v.push_into(out);
        }
        if let Some(v) = self.col_deps.get(&(sheet, c.col)) {
            v.query(c.row, c.row, &mut |k| out.push(*k));
        }
        if let Some(v) = self.wide_deps.get(&sheet) {
            v.query(c.row, c.row, &mut |(c0, c1, k)| {
                if *c0 <= c.col && c.col <= *c1 {
                    out.push(*k);
                }
            });
        }
    }

    /// Formulas that read any cell of `range` (a formula can be listed more than once).
    pub fn dependents_of_range(&self, sheet: usize, range: RangeRef, out: &mut Vec<Key>) {
        if range.count() <= 4096 {
            for c in range.iter() {
                if let Some(v) = self.cell_deps.get(&(sheet, c)) {
                    v.push_into(out);
                }
            }
        } else {
            for ((s, c), v) in &self.cell_deps {
                if *s == sheet && range.contains(*c) {
                    v.push_into(out);
                }
            }
        }
        let (r0, r1) = (range.start.row, range.end.row);
        if (range.width() as usize) <= self.col_deps.len() {
            for col in range.start.col..=range.end.col {
                if let Some(v) = self.col_deps.get(&(sheet, col)) {
                    v.query(r0, r1, &mut |k| out.push(*k));
                }
            }
        } else {
            for ((s, col), v) in &self.col_deps {
                if *s == sheet && range.start.col <= *col && *col <= range.end.col {
                    v.query(r0, r1, &mut |k| out.push(*k));
                }
            }
        }
        if let Some(v) = self.wide_deps.get(&sheet) {
            v.query(r0, r1, &mut |(c0, c1, k)| {
                if *c0 <= range.end.col && range.start.col <= *c1 {
                    out.push(*k);
                }
            });
        }
    }

    /// Precedent areas of a formula cell (for Trace Precedents).
    pub fn precedents_of(&self, k: Key) -> Vec<Area> {
        self.nodes.get(&k).map(|n| self.areas_of(n).to_vec()).unwrap_or_default()
    }

    /// The live heap bytes of this graph, by allocation class: the numbers a memory report shows,
    /// and the shape a later representation change has to be measured against. Everything is
    /// `len`/`capacity`/`size_of` arithmetic except the `Deps::Many` vectors, which the map cannot
    /// size for us; see [`GraphMemory`].
    pub fn memory(&self) -> GraphMemory {
        use std::mem::size_of;
        let mut m = GraphMemory {
            nodes: gridcraft_model::hash_index_bytes(self.nodes.capacity(), size_of::<(Key, Node)>()),
            areas: self.areas.capacity().saturating_mul(size_of::<Area>()),
            cell_deps: gridcraft_model::hash_index_bytes(self.cell_deps.capacity(), size_of::<(Key, Deps)>()),
            cols: gridcraft_model::hash_index_bytes(self.col_deps.capacity(), size_of::<((usize, u32), Intervals<Key>)>())
                .saturating_add(self.col_deps.values().map(Intervals::memory).sum::<usize>()),
            wide: gridcraft_model::hash_index_bytes(self.wide_deps.capacity(), size_of::<(usize, Intervals<(u32, u32, Key)>)>())
                .saturating_add(self.wide_deps.values().map(Intervals::memory).sum::<usize>()),
            legacy_ranges: gridcraft_model::hash_index_bytes(self.legacy_range.capacity(), size_of::<(Key, RangeRef)>()),
            flags: set_bytes(&self.dynamic)
                .saturating_add(set_bytes(&self.may_spill))
                .saturating_add(set_bytes(&self.legacy))
                .saturating_add(set_bytes(&self.trim))
                .saturating_add(set_bytes(&self.visibility)),
            unsettled: set_bytes(&self.unsettled_cols).saturating_add(set_bytes(&self.unsettled_wide)),
            scratch: self
                .scratch_cells
                .capacity()
                .saturating_mul(size_of::<Key>())
                .saturating_add(self.scratch_cols.capacity().saturating_mul(size_of::<(usize, u32, u32, u32)>()))
                .saturating_add(self.scratch_wide.capacity().saturating_mul(size_of::<(usize, RangeRef)>())),
            ..GraphMemory::default()
        };
        // The one entry pass: a `Deps::Many` vector's capacity is not derivable from the table.
        for deps in self.cell_deps.values() {
            if let Deps::Many(v) = deps {
                m.many_lists = m.many_lists.saturating_add(1);
                m.cell_deps = m.cell_deps.saturating_add(v.capacity().saturating_mul(size_of::<Key>()));
            }
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_core::CellRef;

    fn key(sheet: usize, row: u32, col: u32) -> Key {
        (sheet, CellRef::new(row, col))
    }

    fn single(sheet: usize, row: u32, col: u32) -> Area {
        let c = CellRef::new(row, col);
        Area { sheet, range: RangeRef::new(c, c) }
    }

    fn whole_column(sheet: usize, col: u32) -> Area {
        Area { sheet, range: RangeRef::new(CellRef::new(0, col), CellRef::new(999, col)) }
    }

    /// A report has to be a partition, so a field added later cannot be counted twice. The graph
    /// here holds single-cell and range precedents, so every class but the rare ones is non-empty.
    #[test]
    fn graph_memory_total_is_the_sum_of_its_parts() {
        let mut g = Graph::default();
        for row in 0..500u32 {
            g.insert(key(0, row, 6), &[single(0, row, 0), whole_column(0, 2)], None, Node::new(None, false, false, false));
        }
        g.settle();
        let m = g.memory();
        let sum = m.nodes + m.areas + m.cell_deps + m.cols + m.wide + m.legacy_ranges + m.flags + m.unsettled + m.scratch;
        assert_eq!(m.total(), sum, "the total is the sum of the fields");
        assert!(m.nodes > 0, "the nodes are charged");
        assert!(m.areas > 0, "the precedent arena is charged");
        assert!(m.cell_deps > 0, "single-cell precedents are indexed");
        assert!(m.cols > 0, "a range precedent is in the per-column index");
        assert_eq!(m.wide, 0, "no range is wider than 64 columns here");
    }

    /// One precedent read by a thousand formulas is the `Deps::Many` case: the list leaves the
    /// inline `One`/`Two` shapes and the report grows by its vector (the one entry pass the
    /// report makes).
    #[test]
    fn one_precedent_read_by_a_thousand_formulas_grows_the_dependents_map() {
        let mut g = Graph::default();
        g.insert(key(0, 0, 6), &[single(0, 5, 0)], None, Node::new(None, false, false, false));
        let first = g.memory();
        assert_eq!(first.many_lists, 0, "one dependent fits inline");
        for row in 1..1000u32 {
            g.insert(key(0, row, 6), &[single(0, 5, 0)], None, Node::new(None, false, false, false));
        }
        let many = g.memory();
        assert_eq!(many.many_lists, 1, "one precedent holds the list");
        let trunk = 1000 * std::mem::size_of::<Key>();
        assert!(many.cell_deps >= first.cell_deps + trunk, "the vector of 1000 keys is charged: {} vs {}", many.cell_deps, first.cell_deps);
        assert!(many.nodes > first.nodes, "the formulas' nodes are charged");
    }
}
