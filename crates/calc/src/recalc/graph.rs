//! The dependency graph: single-cell and interval precedents, and the Intervals index that
//! answers which dependents cover a cell without walking every range.

use std::collections::BTreeSet;
use std::hash::{BuildHasherDefault, Hash, Hasher};

use serde::Serialize;

use gridcraft_core::{CellRef, RangeRef};

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

/// A precedent cell a list holds this many dependents of before adding one stops checking whether
/// it is already in the list.
const LONG_LIST: usize = 8;

/// The cells one edit changes past which it is a bulk edit — a fill, a paste of a column of
/// formulas — where growing the tables ahead of it ([`Graph::reserve_more`]) pays for itself. A
/// keystroke is not a bulk edit, and a workbook that holds no formulas is not one either.
pub(super) const BULK_EDIT: usize = 1024;

/// The dependents of a set of precedent cells: `K` is a precedent key, `V` the key of one formula
/// that reads it.
///
/// Two tables, because the entry that dominates a workbook should carry nothing it does not use: a
/// cell exactly one formula reads (`B2` feeding `G2`) sits in `one` as a bare key and value — no
/// representation tag, no room for a second dependent. The second dependent moves the cell to
/// `many` and a `Vec`, allocated at two (the commonest multi length).
///
/// The shape this replaced was one table of `Key → One(Key) | Two(Key, Key) | Many(Vec<Key>)`: a
/// 40-byte value of which a one-dependent cell used the first 16, over a key of two machine words
/// and hashbrown's control byte — 57 bytes of table a slot, or 95 an entry, on the 478 MB that the
/// million-row reference workbook's precedent index was.
///
/// The key types say what a table knows: in the per-sheet table both are a `CellRef`, because a
/// formula normally reads a cell of its own sheet and the sheet is the table's; in the cross-sheet
/// table both are full `Key`s. Dropping the sheet from both sides halves the entry again.
#[derive(Clone, Debug, Default)]
pub(super) struct Deps<K, V> {
    /// The precedent cells exactly one formula reads.
    one: HashMap<K, V>,
    /// The precedent cells two or more formulas read, each with its list (never empty).
    many: HashMap<K, Vec<V>>,
}

impl<K, V> Deps<K, V> {
    fn is_empty(&self) -> bool {
        self.one.is_empty() && self.many.is_empty()
    }

    /// Heap bytes of this table and how many multi-dependent lists it holds (see
    /// [`GraphMemory`]). The lists are the one pass over entries the report makes: a `Vec`'s
    /// capacity is not derivable from the table's.
    fn memory(&self) -> (usize, usize) {
        use std::mem::size_of;
        let mut bytes = gridcraft_model::hash_index_bytes(self.one.capacity(), size_of::<(K, V)>())
            .saturating_add(gridcraft_model::hash_index_bytes(self.many.capacity(), size_of::<(K, Vec<V>)>()));
        let mut lists = 0usize;
        for v in self.many.values() {
            lists = lists.saturating_add(1);
            bytes = bytes.saturating_add(v.capacity().saturating_mul(size_of::<V>()));
        }
        (bytes, lists)
    }

    /// The slot counts of both tables, for the test that ratchets the index's size.
    #[cfg(test)]
    fn slots(&self) -> usize {
        gridcraft_model::hash_index_bytes(self.one.capacity(), 0).saturating_add(gridcraft_model::hash_index_bytes(self.many.capacity(), 0))
    }
}

impl<K: Copy + Eq + Hash, V: Copy + Eq> Deps<K, V> {
    fn reserve(&mut self, entries: usize) {
        self.one.reserve(entries);
    }

    /// The table with the dependent `k` of precedent cell `c` added, unless it is already there.
    ///
    /// A formula's areas are deduplicated before they reach here, so the graph itself never adds a
    /// dependent twice: the check is for short lists (a compare, a handful) and is skipped for long
    /// ones, since a cell a million formulas read has to stay O(1) to add to.
    fn link(&mut self, c: K, k: V) {
        if let Some(v) = self.many.get_mut(&c) {
            if v.len() > LONG_LIST || !v.contains(&k) {
                v.push(k);
            }
            return;
        }
        match self.one.get_mut(&c) {
            None => {
                self.one.insert(c, k);
            }
            Some(first) if *first == k => {}
            Some(first) => {
                // The second dependent: a two-slot list, which is what a cell two formulas read
                // costs — the same as the inline pair this replaced, and to the right table.
                let v = vec![*first, k];
                self.one.remove(&c);
                self.many.insert(c, v);
            }
        }
    }

    /// The table without the dependent `k` of precedent cell `c`; a cell nothing reads any more
    /// leaves it, so a query never walks an empty list, and a list of one goes back to `one` — so
    /// a workbook edited often keeps the compact shape.
    fn unlink(&mut self, c: K, k: V) {
        let Some(v) = self.many.get_mut(&c) else {
            if self.one.get(&c) == Some(&k) {
                self.one.remove(&c);
            }
            return;
        };
        v.retain(|x| *x != k);
        let left = v.len();
        let last = v.first().copied();
        match left {
            0 => {
                self.many.remove(&c);
            }
            1 => {
                self.many.remove(&c);
                if let Some(last) = last {
                    self.one.insert(c, last);
                }
            }
            _ => {}
        }
    }

    /// Appends the dependents of `c` to `out`, `to_key` turning a stored dependent into its
    /// [`Key`].
    fn push_into(&self, c: K, out: &mut Vec<Key>, to_key: impl Fn(V) -> Key) {
        if let Some(v) = self.one.get(&c) {
            out.push(to_key(*v));
        }
        if let Some(v) = self.many.get(&c) {
            out.extend(v.iter().map(|x| to_key(*x)));
        }
    }

    /// Calls `f` for every precedent cell and dependent the table holds.
    fn for_each(&self, mut f: impl FnMut(K, V)) {
        for (c, v) in &self.one {
            f(*c, *v);
        }
        for (c, v) in &self.many {
            for x in v {
                f(*c, *x);
            }
        }
    }
}

/// Which formulas read which single cell: the bulk of the graph, and the bulk of its memory.
///
/// A formula nearly always reads a cell of its own sheet, so that case drops the sheet from both
/// sides of the entry (the table is the sheet's): 8-byte key, 8-byte dependent, hashbrown's control
/// byte — 17 bytes a slot against the 57 of the single `Key → Deps` table this replaced. A formula
/// reading another sheet (a summary tab over a data tab) is the rare case and keeps full [`Key`]s,
/// in a table of its own so it costs the common one nothing.
#[derive(Clone, Debug, Default)]
pub(super) struct CellDeps {
    /// sheet → the precedents in it, and the formulas of that sheet that read them.
    same: HashMap<usize, Deps<CellRef, CellRef>>,
    /// precedent → the formulas of another sheet that read it.
    cross: Deps<Key, Key>,
}

impl CellDeps {
    /// Whether nothing reads anything any more (a sheet's table leaves when its last dependent
    /// does, so there is nothing to walk).
    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.same.is_empty() && self.cross.is_empty()
    }

    fn sheet_mut(&mut self, sheet: usize) -> &mut Deps<CellRef, CellRef> {
        self.same.entry(sheet).or_default()
    }

    /// Reserves room in the table of `sheet` for `entries` precedent cells: a bulk build of a sheet
    /// would otherwise grow it a dozen times, and each growth rehashes every entry already in it.
    fn reserve(&mut self, sheet: usize, entries: usize) {
        self.sheet_mut(sheet).reserve(entries);
    }

    /// Adds the dependent `k` to what precedent cell `c` answers with.
    fn link(&mut self, c: Key, k: Key) {
        if c.0 == k.0 {
            self.sheet_mut(c.0).link(c.1, k.1);
        } else {
            self.cross.link(c, k);
        }
    }

    /// Drops the dependent `k` from what precedent cell `c` answers with.
    fn unlink(&mut self, c: Key, k: Key) {
        if c.0 == k.0 {
            let sheet = c.0;
            let Some(deps) = self.same.get_mut(&sheet) else { return };
            deps.unlink(c.1, k.1);
            if deps.is_empty() {
                self.same.remove(&sheet);
            }
        } else {
            self.cross.unlink(c, k);
        }
    }

    /// The formulas of any sheet that read cell `c` of `sheet`.
    fn dependents_into(&self, sheet: usize, c: CellRef, out: &mut Vec<Key>) {
        if let Some(deps) = self.same.get(&sheet) {
            deps.push_into(c, out, |v| (sheet, v));
        }
        self.cross.push_into((sheet, c), out, |k| k);
    }

    /// Calls `f` for every precedent cell of `sheet` and a formula that reads it.
    fn for_each_in(&self, sheet: usize, mut f: impl FnMut(CellRef, Key)) {
        if let Some(deps) = self.same.get(&sheet) {
            deps.for_each(|c, v| f(c, (sheet, v)));
        }
        self.cross.for_each(|c, v| {
            if c.0 == sheet {
                f(c.1, v);
            }
        });
    }

    /// Heap bytes of the index and how many multi-dependent lists it holds (see
    /// [`GraphMemory`]).
    fn memory(&self) -> (usize, usize) {
        use std::mem::size_of;
        let mut bytes = gridcraft_model::hash_index_bytes(self.same.capacity(), size_of::<(usize, Deps<CellRef, CellRef>)>());
        let mut lists = 0usize;
        for deps in self.same.values() {
            let (part, count) = deps.memory();
            bytes = bytes.saturating_add(part);
            lists = lists.saturating_add(count);
        }
        let (part, count) = self.cross.memory();
        (bytes.saturating_add(part), lists.saturating_add(count))
    }

    /// The slot counts of every table (see [`Graph::cell_deps_slots`]).
    #[cfg(test)]
    fn slots(&self) -> usize {
        let mut slots = gridcraft_model::hash_index_bytes(self.same.capacity(), 0).saturating_add(self.cross.slots());
        for deps in self.same.values() {
            slots = slots.saturating_add(deps.slots());
        }
        slots
    }
}

/// One formula of the graph.
///
/// Two words: where its run of [`Graph::areas`] starts, and the run's length together with the
/// flags (`RunLen`), plus the shape the formula reads. The flags used to be a byte of their own
/// beside the length, which padded the node out to a fourth word — 24 bytes for a field with six
/// bits set, in a table entry of the key's 16 bytes and hashbrown's control byte: 41 bytes of
/// table a slot, which the reference workbook's two million nodes paid 172 MB for. Packed, the
/// entry is 33 bytes a slot and the same nodes cost 138. S7 of
/// `plan/SHAPE-SHARING.md` then deleted the node's own tree pointer, so a node is **16** bytes
/// (33 bytes of table a slot) — 138 MB instead of 172.
#[derive(Clone, Debug)]
pub(super) struct Node {
    /// Where the node's run of [`Graph::areas`] starts.
    start: u32,
    /// The run's length and the `DYNAMIC`..`VISIBILITY` flags below.
    run: RunLen,
    /// The shape the formula reads. A formula has no tree of its own to keep: the pass reads this
    /// one in place, resolving its relative references at the node's own cell (the graph's key), so
    /// eight bytes stand where a tree per formula stood — the reference workbook's two million
    /// formulas are a handful of trees, not 768 MB of them (`plan/SHAPE-SHARING.md` §3.4).
    pub(super) shape: Option<std::sync::Arc<gridcraft_formula::Shape>>,
}

/// A node's run length and its flags in one word: the length in the low `RUN_BITS`, the flags in
/// the byte above them.
///
/// The length counts areas in the graph's arena, so the only constructor that takes one
/// (`RunLen::new`) answers `None` past `MAX_RUN`: a run that would reach into the flags cannot be
/// built at all, and so no node can hold one. The flags are a byte, so the low eight bits of the
/// word above the run are theirs to use (`VISIBILITY` is the sixth; two are free).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct RunLen(u32);

/// The bits of `RunLen` that hold a run's length.
const RUN_BITS: u32 = 24;
/// The mask of those bits.
const RUN_MASK: u32 = (1 << RUN_BITS) - 1;
/// The longest run a node can hold: 16,777,215 areas — a third of a gigabyte of the arena, and
/// millions of times the longest precedent list a formula has.
const MAX_RUN: usize = RUN_MASK as usize;

impl RunLen {
    /// A run of `len` areas with `flags`, or `None` when the length does not fit under them.
    fn new(len: usize, flags: u8) -> Option<Self> {
        let len = u32::try_from(len).ok().filter(|n| (*n as usize) <= MAX_RUN)?;
        Some(RunLen(len | (u32::from(flags) << RUN_BITS)))
    }

    /// A node that has no areas yet, carrying `flags` (`Graph::insert` gives it its run).
    fn no_run(flags: u8) -> Self {
        RunLen(u32::from(flags) << RUN_BITS)
    }

    /// The number of areas in the run.
    fn len(self) -> usize {
        (self.0 & RUN_MASK) as usize
    }

    /// The `DYNAMIC`..`VISIBILITY` bits.
    fn flags(self) -> u8 {
        // There is nothing above the run but the flags: `new` and `no_run` are the only
        // constructors, and both shift a byte into place.
        (self.0 >> RUN_BITS) as u8
    }

    /// The same run with `flag` set.
    fn with_flag(self, flag: u8) -> Self {
        RunLen(self.0 | (u32::from(flag) << RUN_BITS))
    }
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
    pub(super) fn new(dynamic: bool, may_spill: bool, trim: bool) -> Self {
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
        Node { start: 0, run: RunLen::no_run(flags), shape: None }
    }

    /// The `DYNAMIC`..`VISIBILITY` bits (see `RunLen`).
    fn flags(&self) -> u8 {
        self.run.flags()
    }

    /// How many areas of the arena the node owns.
    fn run_len(&self) -> usize {
        self.run.len()
    }

    /// The same node, marked as reading which rows are hidden (see [`VISIBILITY`]).
    pub(super) fn with_visibility(mut self, hidden_rows: bool) -> Self {
        if hidden_rows {
            self.run = self.run.with_flag(VISIBILITY);
        }
        self
    }

    /// The same node, holding the shape its cell shares instead of a tree of its own.
    pub(super) fn with_shape(mut self, shape: Option<std::sync::Arc<gridcraft_formula::Shape>>) -> Self {
        self.shape = shape;
        self
    }

    /// Whether the node has a formula to evaluate at all: the shape it reads. (A formula whose text
    /// does not parse has no shape and so no node — `Calc::rebuild` skips it.) Read by the
    /// parallel pass, which builds no threads on wasm.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn has_formula(&self) -> bool {
        self.shape.is_some()
    }

    /// The same node, marked volatile (see [`VOLATILE`]).
    pub(super) fn with_volatile(mut self, volatile: bool) -> Self {
        if volatile {
            self.run = self.run.with_flag(VOLATILE);
        }
        self
    }

    /// Whether the formula calls a volatile function.
    pub(super) fn is_volatile(&self) -> bool {
        self.run.flags() & VOLATILE != 0
    }
}

/// Ranges wider than this many columns go to the per-sheet wide index instead of per-column ones.
const WIDE: u32 = 64;

/// Insertions an [`Intervals`] keeps aside (scanned by every query) before merging them in.
const PENDING: usize = 32;

/// Freed areas an arena keeps before [`Graph::settle`] compacts it (and the live share it has to
/// reach: a fifth of the arena).
const AREA_GARBAGE: usize = 4096;

/// The share of its length the arena keeps spare after [`Graph::trim_areas`]: a quarter, so the
/// next edit's areas fit without a reallocation and the one after that is only paid once another
/// quarter has been filled.
const ARENA_SPARE: usize = 4;

/// Spare slots below which an arena's tail is not worth the copy giving it back makes (a few tens
/// of a kilobyte).
const ARENA_TRIM_MIN: usize = 4096;

/// The share of its capacity a trim has to free to be worth the copy: an eighth. Without it a
/// command that deletes one formula would give a slot back and the next one the slot beside it —
/// a copy of the whole arena per keystroke.
const ARENA_TRIM_SHARE: usize = 8;

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
/// multi-dependent lists ([`GraphMemory::cell_deps`]): a `Vec`'s capacity is not derivable from the
/// table's own capacity, so its values are visited once — no allocation, about a nanosecond an
/// entry, which is why this is a diagnostic and not something to ask for every frame.
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
    /// The single-cell precedent → dependents index, and every multi-dependent list it holds.
    pub cell_deps: usize,
    /// How many of that index's entries are lists of two or more (what the one walk above counts).
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
/// A precedent area as the arena keeps it: an [`Area`] with the sheet as a `u32` instead of a
/// `usize`. A workbook cannot hold four billion sheets (the index is an offset into a
/// `Vec<Sheet>`), and the packed form is 20 bytes against the evaluator's 24 — 20 MB on the five
/// million areas of the reference workbook, 34 MB on the 8.4 million slots the arena's doubling
/// left. The evaluator keeps the `usize` it resolves references with: the narrowing happens here,
/// once per area inserted, and is checked (`PackedArea::fits`), never truncated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PackedArea {
    sheet: u32,
    range: RangeRef,
}

impl PackedArea {
    /// Whether the area's sheet fits the packed form.
    fn fits(a: &Area) -> bool {
        u32::try_from(a.sheet).is_ok()
    }

    /// The packed form of `a`, or `None` when its sheet does not fit (see [`PackedArea::fits`]).
    fn new(a: &Area) -> Option<Self> {
        Some(PackedArea { sheet: u32::try_from(a.sheet).ok()?, range: a.range })
    }

    /// The sheet, as the workbook's sheet index.
    pub(super) fn sheet(self) -> usize {
        self.sheet as usize
    }

    /// The cell range on that sheet.
    pub(super) fn range(self) -> RangeRef {
        self.range
    }

    /// The same area in the evaluator's form.
    pub(super) fn to_area(self) -> Area {
        Area { sheet: self.sheet(), range: self.range }
    }
}

/// The formulas that read which cells.
///
/// Precedent areas live in one arena instead of a `Vec` per node: a rebuild of a million formulas
/// then fills one vector rather than allocating a million, and a node is eight bytes smaller. A
/// replaced node's run becomes garbage that [`Graph::settle`] drops once it is a large enough share
/// of the arena, and the spare capacity the vector's doubling leaves behind goes back with it —
/// a tail that was 81 MB of the reference workbook.
#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub(super) nodes: HashMap<Key, Node>,
    /// Every node's precedent areas, packed: a node owns `areas[start..start + len]`, its `run`.
    areas: Vec<PackedArea>,
    /// Areas no node points at any more.
    area_garbage: usize,
    /// precedent cell → the formulas that read it
    pub(super) cell_deps: CellDeps,
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

    /// The precedent areas of a node, as the arena keeps them (see [`PackedArea`]).
    pub(super) fn areas_of(&self, node: &Node) -> &[PackedArea] {
        let start = node.start as usize;
        self.areas.get(start..start.saturating_add(node.run_len())).unwrap_or(&[])
    }

    /// The slot counts of the single-cell precedent index (see [`CellDeps::slots`]): the test
    /// that ratchets the index's size divides its bytes by them.
    #[cfg(test)]
    pub(super) fn cell_deps_slots(&self) -> usize {
        self.cell_deps.slots()
    }

    /// The number of areas the arena holds (live ones plus the garbage `settle` has not packed
    /// away yet) — tests check that a bulk replacement does not leave them behind.
    #[cfg(test)]
    pub(super) fn areas_len(&self) -> usize {
        self.areas.len()
    }

    /// The arena's capacity, in areas — the test that checks a fill leaves no doubling tail. A
    /// `Vec`'s capacity is the only thing that says whether an insert will copy it.
    #[cfg(test)]
    pub(super) fn areas_capacity(&self) -> usize {
        self.areas.capacity()
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

    /// Reserves room for `formulas` formulas of `sheet`, so a bulk build does not grow its tables
    /// a dozen times (each growth rehashes every entry already in it). The precedent table reserved
    /// is the sheet's own, where its formulas' same-sheet references land; a formula reading another
    /// sheet's cells goes to the cross table, which grows as it fills.
    pub(super) fn reserve(&mut self, sheet: usize, formulas: usize) {
        self.reserve_more(formulas);
        let precedent_cells = formulas.saturating_mul(3).saturating_div(2);
        self.cell_deps.reserve(sheet, precedent_cells);
    }

    /// Reserves room for `formulas` more formulas and their precedents, before a bulk insert
    /// (a fill, a paste of a column of formulas) — the part of [`Graph::reserve`] that does not
    /// belong to one sheet. The nodes and the arena would otherwise grow a dozen times during the
    /// fill, each growth rehashing or copying everything already in them.
    pub(super) fn reserve_more(&mut self, formulas: usize) {
        self.nodes.reserve(formulas);
        self.areas.reserve(formulas.saturating_mul(2));
    }

    /// Index entries of a formula's precedents, without duplicates: single cells, column
    /// intervals and wide ranges. The buffers are reused between calls, so this allocates nothing.
    fn entries(areas: &[PackedArea], cells: &mut Vec<Key>, cols: &mut Vec<(usize, u32, u32, u32)>, wide: &mut Vec<(usize, RangeRef)>) {
        cells.clear();
        cols.clear();
        wide.clear();
        for a in areas {
            let sheet = a.sheet();
            if a.range.is_single() {
                cells.push((sheet, a.range.start));
            } else if a.range.width() > WIDE {
                wide.push((sheet, a.range));
            } else {
                for c in a.range.start.col..=a.range.end.col {
                    cols.push((sheet, c, a.range.start.row, a.range.end.row));
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
        let mut flags = node.flags();
        if legacy.is_some() {
            flags |= LEGACY;
        }
        // A node records its precedents as a run of the arena whose length shares a word with the
        // flags, and an area packs its sheet into a `u32`: past those limits (16,777,215
        // precedents, four billion sheets) a formula cannot be recorded, and `Graph::place`
        // refuses it rather than truncating it. What it gets instead is what the precedent walk
        // gives a formula that runs past its own budget — no precedents, and `DYNAMIC`, so every
        // pass evaluates it and an unlinked precedent cannot leave a stale value behind.
        let placed = self.place(areas, flags);
        let (node, flags) = match placed {
            Some((start, run)) => (Node { start, run, shape: node.shape }, flags),
            None => (Node { start: 0, run: RunLen::no_run(flags | DYNAMIC), shape: node.shape }, flags | DYNAMIC),
        };
        // One table probe: the node this replaces (if any) is unlinked after the new one has taken
        // its place, which is what a bulk build — a fill of a million formulas — pays per formula.
        if let Some(old) = self.nodes.insert(k, node) {
            self.unlink(k, &old);
        }
        if let Some((start, run)) = placed {
            self.link_run(k, start, run.len());
        }
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

    /// Appends `areas` to the arena and answers the run they became — `None`, having appended
    /// nothing, when they do not fit a node (see [`Graph::insert`]).
    fn place(&mut self, areas: &[Area], flags: u8) -> Option<(u32, RunLen)> {
        // Everything is checked before anything is appended: half a run in the arena and half a
        // node would leave precedents recorded and out of the index.
        if !areas.iter().all(PackedArea::fits) {
            return None;
        }
        let start = u32::try_from(self.areas.len()).ok()?;
        let run = RunLen::new(areas.len(), flags)?;
        self.areas.extend(areas.iter().filter_map(PackedArea::new));
        Some((start, run))
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
        let old_areas = self.areas.get(start..start.saturating_add(old.run_len())).unwrap_or(&[]);
        Self::entries(old_areas, &mut cells, &mut cols, &mut wide);
        for &c in &cells {
            self.cell_deps.unlink(c, k);
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
        if old.flags() & DYNAMIC != 0 {
            self.dynamic.remove(&k);
        }
        if old.flags() & MAY_SPILL != 0 {
            self.may_spill.remove(&k);
        }
        if old.flags() & TRIM != 0 {
            self.trim.remove(&k);
        }
        if old.flags() & VISIBILITY != 0 {
            self.visibility.remove(&k);
        }
        if old.flags() & LEGACY != 0 {
            self.legacy.remove(&k);
            self.legacy_range.remove(&k);
        }
        self.area_garbage = self.area_garbage.saturating_add(old.run_len());
    }

    /// Adds the run at `areas[start..start + len]` to the index of everything it reads.
    fn link_run(&mut self, k: Key, start: u32, len: usize) {
        let (mut cells, mut cols, mut wide) = self.take_scratch();
        let areas = self.areas.get(start as usize..(start as usize).saturating_add(len)).unwrap_or(&[]);
        Self::entries(areas, &mut cells, &mut cols, &mut wide);
        for &c in &cells {
            self.cell_deps.link(c, k);
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
    /// just slower), and gives the arena's garbage and spare capacity back once they are a large
    /// enough share of it.
    ///
    /// The shape the node at `k` evaluates, for the test that holds a fill to one tree.
    ///
    /// `cfg(test)`: the pass reads a node's shape through [`crate::recalc::pass_host::Trees`], and
    /// nothing in the product asks the graph for one.
    #[cfg(test)]
    pub(super) fn node_shape(&self, k: Key) -> Option<std::sync::Arc<gridcraft_formula::Shape>> {
        self.nodes.get(&k).and_then(|n| n.shape.clone())
    }

    /// Every command ends here (a rebuild, a restore, an edit's commit), which is the one point
    /// the graph is quiescent: a fill through `insert` alone must not have the arena reallocated
    /// under it, so nothing below runs while one is in progress.
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
        self.trim_areas();
    }

    /// Gives back the spare capacity a fill's doubling left behind: the reference workbook's five
    /// million areas sat in an arena grown to 8,388,608 slots, of which 3.39 million — 81 MB —
    /// were never used. A `Vec` doubles, so a fill ends somewhere between half full and full.
    ///
    /// Called from [`Graph::settle`], which every command ends with, so it runs when the graph is
    /// quiescent and never inside a fill. The quarter it keeps spare ([`ARENA_SPARE`]) is what
    /// makes that safe: the next formula's areas fit without a reallocation, and the one after
    /// that is only paid once another quarter of the arena has been filled — so a trim costs one
    /// copy per quarter of growth, not one per insert.
    fn trim_areas(&mut self) {
        let (len, cap) = (self.areas.len(), self.areas.capacity());
        if len == 0 {
            self.areas.shrink_to(0);
            return;
        }
        let target = len.saturating_add((len / ARENA_SPARE).max(ARENA_TRIM_MIN));
        // Only when the copy buys an eighth of the capacity: an arena that has grown into its tail
        // and one a delete left a few slots too big are both cheaper to leave alone than to copy,
        // and this is the difference between trimming after a fill and thrashing on every edit.
        if cap < target.saturating_add(target / ARENA_TRIM_SHARE) {
            return;
        }
        self.areas.shrink_to(target);
    }

    /// Packes the live areas together, in one pass over the nodes. Runs once the garbage a
    /// replaced formula left behind outweighs a fifth of the arena, so its cost — a walk of the
    /// nodes — is paid for by the edits that filled it.
    fn compact_areas(&mut self) {
        let old = std::mem::take(&mut self.areas);
        let live = old.len().saturating_sub(self.area_garbage);
        // A quarter spare, so the edit that follows does not reallocate what this has just copied
        // (see `trim_areas`) — a compaction that came back to an exactly-sized arena made the
        // next single formula copy the whole thing.
        let mut areas: Vec<PackedArea> = Vec::with_capacity(live.saturating_add(live / ARENA_SPARE));
        for node in self.nodes.values_mut() {
            let start = node.start as usize;
            let run = old.get(start..start.saturating_add(node.run_len())).unwrap_or(&[]);
            node.start = u32::try_from(areas.len()).unwrap_or(u32::MAX);
            areas.extend_from_slice(run);
        }
        self.areas = areas;
        self.area_garbage = 0;
    }

    /// Formulas that read cell `c` of `sheet`.
    pub fn dependents(&self, sheet: usize, c: CellRef, out: &mut Vec<Key>) {
        self.cell_deps.dependents_into(sheet, c, out);
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
                self.cell_deps.dependents_into(sheet, c, out);
            }
        } else {
            // A range too big to probe cell by cell: walk the sheet's precedents instead.
            self.cell_deps.for_each_in(sheet, |c, k| {
                if range.contains(c) {
                    out.push(k);
                }
            });
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
        self.nodes.get(&k).map(|n| self.areas_of(n).iter().map(|a| a.to_area()).collect()).unwrap_or_default()
    }

    /// The live heap bytes of this graph, by allocation class: the numbers a memory report shows,
    /// and the shape a later representation change has to be measured against. Everything is
    /// `len`/`capacity`/`size_of` arithmetic except the multi-dependent lists, which the table
    /// cannot size for us; see [`GraphMemory`].
    pub fn memory(&self) -> GraphMemory {
        use std::mem::size_of;
        let (cell_deps, many_lists) = self.cell_deps.memory();
        GraphMemory {
            nodes: gridcraft_model::hash_index_bytes(self.nodes.capacity(), size_of::<(Key, Node)>()),
            areas: self.areas.capacity().saturating_mul(size_of::<PackedArea>()),
            cell_deps,
            many_lists,
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
        }
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
            g.insert(key(0, row, 6), &[single(0, row, 0), whole_column(0, 2)], None, Node::new(false, false, false));
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

    /// One precedent read by a thousand formulas is the multi-dependent case: the cell leaves the
    /// one-to-one table and the report grows by its list (the one entry pass the report makes).
    #[test]
    fn one_precedent_read_by_a_thousand_formulas_grows_the_dependents_map() {
        let mut g = Graph::default();
        g.insert(key(0, 0, 6), &[single(0, 5, 0)], None, Node::new(false, false, false));
        let first = g.memory();
        assert_eq!(first.many_lists, 0, "one dependent is a one-to-one entry");
        for row in 1..1000u32 {
            g.insert(key(0, row, 6), &[single(0, 5, 0)], None, Node::new(false, false, false));
        }
        let many = g.memory();
        assert_eq!(many.many_lists, 1, "one precedent holds the list");
        let trunk = 1000 * std::mem::size_of::<CellRef>();
        assert!(many.cell_deps >= first.cell_deps + trunk, "the list of 1000 dependents is charged: {} vs {}", many.cell_deps, first.cell_deps);
        assert!(many.nodes > first.nodes, "the formulas' nodes are charged");
    }

    /// The reference workload's shape — a column of formulas each reading its own single-cell
    /// precedents, so every entry a one-to-one pair — is the shape the precedent index has to be
    /// cheap in. It cost 57 bytes of table a slot (a 16-byte key, the 40-byte `One | Two | Many`
    /// value, hashbrown's control byte) and costs 17 now: a ratchet, so a later change cannot
    /// silently buy the entry back.
    #[test]
    fn the_precedent_index_is_narrower_than_the_shape_it_replaced() {
        let mut g = Graph::default();
        for row in 0..5_000u32 {
            g.insert(key(0, row, 6), &[single(0, row, 1), single(0, row, 2), single(0, row, 3)], None, Node::new(false, false, false));
        }
        let m = g.memory();
        assert_eq!(m.many_lists, 0, "every entry is a single dependent");
        let slots = g.cell_deps_slots();
        // What one slot of the table this replaced cost: the 16-byte `Key`, the 40-byte `Deps`
        // (`One(Key)`, `Two(Key, Key)` and the tag around them) and the control byte.
        let replaced = slots.saturating_mul(16 + 40 + 1);
        assert!(
            m.cell_deps.saturating_mul(100) <= replaced.saturating_mul(35),
            "the index costs {} bytes for {slots} slots; the shape it replaced cost {replaced}",
            m.cell_deps
        );
        // …and it really is the narrow entry: an 8-byte key, an 8-byte dependent, a control byte.
        assert_eq!(std::mem::size_of::<(CellRef, CellRef)>() + 1, 17);
    }

    /// A precedent with one, two, three and ten thousand dependents answers exactly those keys;
    /// a dependent added twice is one entry while the list is short, and a long list stops
    /// checking (the O(1) add a cell a million formulas read needs — see `Deps::link`).
    #[test]
    fn a_precedent_answers_the_dependents_it_has() {
        let mut g = Graph::default();
        let mut out = Vec::new();
        for n in [1u32, 2, 3, 10_000] {
            for row in 0..n {
                g.insert(key(0, row, 6), &[single(0, 0, 0)], None, Node::new(false, false, false));
            }
            out.clear();
            g.dependents(0, CellRef::new(0, 0), &mut out);
            out.sort_unstable();
            let want: Vec<Key> = (0..n).map(|row| key(0, row, 6)).collect();
            assert_eq!(out, want, "{n} dependents");
            assert_eq!(g.memory().many_lists, usize::from(n > 1), "{n} dependents is a list from two on");
        }
        // A list walked back down: removing one of ten thousand leaves it a list…
        g.remove(key(0, 2, 6));
        out.clear();
        g.dependents(0, CellRef::new(0, 0), &mut out);
        assert_eq!(out.len(), 9_999, "one dependent dropped");
        // …and a list of one returns to the one-to-one table, so a workbook edited often keeps
        // the compact shape.
        let mut small = Graph::default();
        for row in 0..3u32 {
            small.insert(key(0, row, 6), &[single(0, 0, 0)], None, Node::new(false, false, false));
        }
        assert_eq!(small.memory().many_lists, 1, "three dependents are a list");
        small.remove(key(0, 2, 6));
        assert_eq!(small.memory().many_lists, 1, "two are still a list");
        small.remove(key(0, 1, 6));
        assert_eq!(small.memory().many_lists, 0, "one is a one-to-one entry again");
        out.clear();
        small.dependents(0, CellRef::new(0, 0), &mut out);
        assert_eq!(out, vec![key(0, 0, 6)]);
        small.remove(key(0, 0, 6));
        assert!(small.cell_deps.is_empty(), "the sheet's table leaves with its last dependent");
    }

    /// A precedent read by a formula of its own sheet and by one of another sheet answers both,
    /// through a cell lookup and through the walk a range too big to probe cell by cell uses, and
    /// the index holds nothing once the formulas are gone.
    #[test]
    fn a_precedent_read_from_two_sheets_answers_both() {
        let mut g = Graph::default();
        g.insert(key(0, 1, 6), &[single(0, 0, 0)], None, Node::new(false, false, false));
        g.insert(key(1, 1, 6), &[single(0, 0, 0)], None, Node::new(false, false, false));
        let both = vec![key(0, 1, 6), key(1, 1, 6)];
        let mut out = Vec::new();
        g.dependents(0, CellRef::new(0, 0), &mut out);
        out.sort_unstable();
        assert_eq!(out, both);
        let small = RangeRef::new(CellRef::new(0, 0), CellRef::new(9, 9));
        out.clear();
        g.dependents_of_range(0, small, &mut out);
        out.sort_unstable();
        out.dedup();
        assert_eq!(out, both, "a small range probes the cell");
        // 201 rows by 41 columns is past the 4096 cells a range query probes one at a time.
        let wide = RangeRef::new(CellRef::new(0, 0), CellRef::new(200, 40));
        assert!(wide.count() > 4096, "the walk this checks");
        out.clear();
        g.dependents_of_range(0, wide, &mut out);
        out.sort_unstable();
        out.dedup();
        assert_eq!(out, both, "a wide range walks the sheet");
        out.clear();
        g.dependents_of_range(0, RangeRef::new(CellRef::new(300, 0), CellRef::new(400, 40)), &mut out);
        assert!(out.is_empty(), "a range that does not cover the precedent names nobody: {out:?}");
        for k in [key(0, 1, 6), key(1, 1, 6)] {
            g.remove(k);
        }
        out.clear();
        g.dependents(0, CellRef::new(0, 0), &mut out);
        assert!(out.is_empty(), "{out:?}");
        assert!(g.cell_deps.is_empty(), "a sheet's table leaves with its last dependent");
        assert_eq!(g.memory().many_lists, 0);
    }

    /// The dedup a short list does and a long one skips, directly on the index (a formula's areas
    /// are deduplicated before they reach the graph, so `link` is the only place that sees a
    /// repeat).
    #[test]
    fn a_short_list_holds_a_dependent_once_and_a_long_one_stops_checking() {
        let c = key(0, 0, 0);
        let mut out = Vec::new();
        let mut short = CellDeps::default();
        short.link(c, key(0, 1, 6));
        short.link(c, key(0, 1, 6));
        short.link(c, key(0, 1, 6));
        short.dependents_into(0, CellRef::new(0, 0), &mut out);
        assert_eq!(out, vec![key(0, 1, 6)], "a short list holds a dependent once");

        let mut long = CellDeps::default();
        for col in 0..LONG_LIST as u32 + 1 {
            long.link(c, key(0, col, 7));
        }
        long.link(c, key(0, 0, 7));
        out.clear();
        long.dependents_into(0, CellRef::new(0, 0), &mut out);
        assert_eq!(out.len(), LONG_LIST + 2, "a long list takes the repeat: {out:?}");
        out.retain(|k| *k == key(0, 0, 7));
        assert_eq!(out.len(), 2, "the long list stopped checking");
    }

    /// The three allocations the memory rounds shrank, in the arithmetic the memory report makes: a
    /// node's flags moved into its run's word, an area's sheet packs into a `u32` (24 to 20), and
    /// S7 deleted the node's own tree pointer (24 to 16 — the reference workbook's two million
    /// nodes, 172 MB to 138).
    #[test]
    fn a_node_is_two_words_and_a_packed_area_five() {
        assert_eq!(std::mem::size_of::<Node>(), 16, "a `u32` start, the run/flag word, the shape");
        assert_eq!(std::mem::size_of::<(Key, Node)>() + 1, 33, "the node table's bytes a slot");
        assert_eq!(std::mem::size_of::<PackedArea>(), 20, "a `u32` sheet and a 16-byte range");
        assert_eq!(std::mem::size_of::<Area>(), 24, "the evaluator's form, which the arena does not pay for");
    }

    /// A run's length shares a word with the flags, so the only constructor that takes one has to
    /// refuse a length that would reach into them: there is no other way to build a `RunLen`, and
    /// so no way for a node to hold an over-long run (see `Node`).
    #[test]
    fn a_run_length_cannot_reach_into_the_flags() {
        assert_eq!(MAX_RUN, 16_777_215, "the 24 bits under the flags");
        let longest = RunLen::new(MAX_RUN, u8::MAX);
        assert!(longest.is_some(), "the longest run fits, with every flag set");
        let Some(longest) = longest else { return };
        assert_eq!(longest.len(), MAX_RUN, "it is stored exactly");
        assert_eq!(longest.flags(), u8::MAX, "and every flags bit is still there");
        assert!(RunLen::new(MAX_RUN + 1, 0).is_none(), "one area more is refused");
        assert!(RunLen::new(usize::MAX, 0).is_none(), "and a length that is not a `u32` at all");
        for flag in [DYNAMIC, MAY_SPILL, TRIM, LEGACY, VOLATILE, VISIBILITY] {
            let Some(run) = RunLen::new(MAX_RUN, flag) else { continue };
            assert_eq!(run.flags(), flag, "flag {flag} survives the longest run");
            assert_eq!(run.len(), MAX_RUN);
            assert_eq!(run.with_flag(flag).flags(), flag, "setting a bit twice is setting it once");
        }
    }

    /// Every combination of the six bits a node can carry, stored and read back: the flags, the
    /// run beside them, the sets each bit feeds, and the node's last effect on the graph when it
    /// leaves. The two spare bits of the flag byte are in `a_run_length_cannot_reach_into_the_flags`.
    #[test]
    fn every_combination_of_node_flags_survives_the_packed_word() {
        let areas = [single(0, 3, 0), single(0, 3, 1), whole_column(0, 2)];
        let legacy = RangeRef::new(CellRef::new(0, 0), CellRef::new(1, 0));
        for bits in 0u8..64 {
            let (dynamic, may_spill, trim) = (bits & DYNAMIC != 0, bits & MAY_SPILL != 0, bits & TRIM != 0);
            let (is_legacy, volatile, hidden) = (bits & LEGACY != 0, bits & VOLATILE != 0, bits & VISIBILITY != 0);
            let k = key(0, 7, 6);
            let mut g = Graph::default();
            let node = Node::new(dynamic, may_spill, trim).with_volatile(volatile).with_visibility(hidden);
            g.insert(k, &areas, is_legacy.then_some(legacy), node);
            let stored = g.nodes.get(&k);
            assert!(stored.is_some(), "bits {bits:06b}: the node is stored");
            let Some(stored) = stored else { continue };
            assert_eq!(stored.flags(), bits, "bits {bits:06b}: the flags read back");
            assert_eq!(stored.run_len(), areas.len(), "bits {bits:06b}: the run keeps its length");
            assert_eq!(g.areas_of(stored).len(), areas.len(), "bits {bits:06b}: and the arena its areas");
            assert_eq!(g.areas_of(stored)[2].range(), areas[2].range);
            assert_eq!(g.is_volatile(k), volatile, "bits {bits:06b}: volatile");
            assert_eq!(g.dynamic.contains(&k), dynamic, "bits {bits:06b}: dynamic");
            assert_eq!(g.may_spill.contains(&k), may_spill, "bits {bits:06b}: may spilling");
            assert_eq!(g.trim.contains(&k), trim, "bits {bits:06b}: trim");
            assert_eq!(g.visibility.contains(&k), hidden, "bits {bits:06b}: visibility");
            assert_eq!(g.legacy_range.get(&k).copied(), is_legacy.then_some(legacy), "bits {bits:06b}: legacy");
            g.remove(k);
            assert!(g.dynamic.is_empty() && g.may_spill.is_empty() && g.trim.is_empty(), "bits {bits:06b}: the sets are empty again");
            assert!(g.visibility.is_empty() && g.legacy_range.is_empty(), "bits {bits:06b}: every set is empty again");
        }
    }

    /// A compaction rewrites every node's run: the length it keeps, the flags beside it and the
    /// areas it points at all have to come through unchanged.
    #[test]
    fn compacting_the_arena_keeps_each_run_and_its_flags() {
        let mut g = Graph::default();
        for row in 0..8_000u32 {
            let node = Node::new(false, false, true).with_volatile(row % 2 == 0);
            g.insert(key(0, row, 6), &[single(0, row, 0), single(0, row, 1)], None, node);
        }
        // Replace every third formula with a wider run: a fifth of the arena is garbage now, which
        // is what `settle` compacts.
        for row in (0..8_000u32).step_by(3) {
            let node = Node::new(false, false, true);
            g.insert(key(0, row, 6), &[single(0, row, 0), single(0, row, 1), single(0, row, 2)], None, node);
        }
        let before = g.areas_len();
        g.settle();
        assert!(g.areas_len() < before, "the replaced runs are packed away: {} of {before}", g.areas_len());
        for row in 0..8_000u32 {
            let Some(node) = g.nodes.get(&key(0, row, 6)) else { continue };
            let replaced = row % 3 == 0;
            assert_eq!(node.run_len(), if replaced { 3 } else { 2 }, "row {row}'s run");
            assert!(node.flags() & TRIM != 0, "row {row} keeps TRIM");
            assert_eq!(node.is_volatile(), !replaced && row % 2 == 0, "row {row}'s VOLATILE");
            assert_eq!(g.areas_of(node)[0].range(), single(0, row, 0).range, "row {row} reads its own row");
        }
    }

    /// `Vec` doubles, so a fill ends with somewhere between a quarter and a half of the arena's
    /// capacity unused — 3.39 million slots, 81 MB, on the reference workbook's five million
    /// areas. `settle` (which every command ends with) gives that tail back and keeps a quarter,
    /// so the edit that follows does not reallocate what the trim has just copied.
    #[test]
    fn a_fill_leaves_the_arena_a_quarter_spare_and_no_more() {
        let mut g = Graph::default();
        let fill = |g: &mut Graph, rows: std::ops::Range<u32>| {
            for row in rows {
                g.insert(key(0, row, 6), &[single(0, row, 0), single(0, row, 1), single(0, row, 2)], None, Node::new(false, false, false));
            }
        };
        fill(&mut g, 0..10_000);
        g.settle();
        let live = g.areas_len();
        assert_eq!(live, 30_000, "three areas a formula");
        assert!(g.areas_capacity() * 4 <= live * 5, "{} slots for {live} areas", g.areas_capacity());

        // Two thousand more rows cross the capacity the first ten thousand grew the arena to, so
        // the tail is the doubling's (65,536 slots for 36,000 areas) and the trim has to undo it.
        fill(&mut g, 10_000..12_000);
        g.settle();
        let live = g.areas_len();
        assert_eq!(live, 36_000, "three areas a formula");
        assert!(g.areas_capacity() * 4 <= live * 5, "{} slots for {live} areas", g.areas_capacity());
        assert!(g.areas_capacity() < 65_536, "the doubling's tail is gone, not kept: {} slots", g.areas_capacity());

        // The quarter kept spare is what makes trimming safe on every command: the next edits fit
        // without a reallocation (a trim that left the arena exactly sized would copy it per
        // formula, which is the thrash this avoids).
        let cap = g.areas_capacity();
        fill(&mut g, 12_000..12_200);
        for row in 12_000..12_200u32 {
            g.remove(key(0, row, 6));
            g.settle();
            assert_eq!(g.areas_capacity(), cap, "row {row}'s edit reallocated the arena");
        }
    }
}
