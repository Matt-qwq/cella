//! The dependency graph: single-cell and interval precedents, and the Intervals index that
//! answers which dependents cover a cell without walking every range.

use std::collections::BTreeSet;
use std::hash::{BuildHasherDefault, Hash, Hasher};

use serde::Serialize;

use cella_core::{CellRef, RangeRef};

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
        let mut bytes = cella_model::hash_index_bytes(self.one.capacity(), size_of::<(K, V)>())
            .saturating_add(cella_model::hash_index_bytes(self.many.capacity(), size_of::<(K, Vec<V>)>()));
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
        cella_model::hash_index_bytes(self.one.capacity(), 0).saturating_add(cella_model::hash_index_bytes(self.many.capacity(), 0))
    }
}

impl<K: Copy + Eq + Hash, V: Copy + Eq> Deps<K, V> {
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
        self.for_each_of(c, &mut |k| out.push(k), to_key);
    }

    /// Calls `f` for every dependent of `c`, `to_key` turning a stored dependent into its [`Key`].
    ///
    /// A table that holds nothing is not probed: on an ordinary sheet a precedent cell has one
    /// dependent and no list, so the walk asks one table and hashes the cell once.
    fn for_each_of(&self, c: K, f: &mut impl FnMut(Key), to_key: impl Fn(V) -> Key) {
        if !self.one.is_empty()
            && let Some(v) = self.one.get(&c)
        {
            f(to_key(*v));
        }
        if !self.many.is_empty()
            && let Some(v) = self.many.get(&c)
        {
            for x in v {
                f(to_key(*x));
            }
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

/// Cells a dense one-dependent table may cover for each dependent it holds before the hash table
/// is the smaller of the two: at eight bytes a cell, two cells a dependent is sixteen bytes
/// against the eighteen a `CellRef -> CellRef` hash entry takes ([cella_model::hash_index_bytes]).
const DENSE_ONE_FILL: usize = 2;

/// The cells a dense table may cover however few dependents it holds, so a sheet that is only
/// starting to fill up is not refused for being small: 32 KB of table.
const DENSE_ONE_MIN_CELLS: usize = 4_096;

/// The most cells one sheet's dense table may cover: eight bytes a cell, 128 MB a sheet, past
/// which the sheet keeps the hash table however close together its dependents are. It is a ceiling
/// on one runaway box, not the memory promise — [`dense_budget`] is (two cells a dependent), and
/// the box is only grown to a cell the budget can pay for.
const DENSE_ONE_MAX_CELLS: usize = 16 << 20;

/// One-dependent entries a sheet holds before its index is worth a lookup by arithmetic, and the
/// first count [`SheetDeps::promote`] tries at (it retries at twice as many).
const DENSE_ONE_PROMOTE: usize = 1_024;

/// The cells a dense table may cover when it holds `entries` dependents.
fn dense_budget(entries: usize) -> usize {
    // A sheet starting to fill up gets the floor; a huge one the cap. (The two are consts, so
    // `clamp` cannot be handed them the wrong way round.)
    entries.saturating_mul(DENSE_ONE_FILL).clamp(DENSE_ONE_MIN_CELLS, DENSE_ONE_MAX_CELLS)
}

/// The box of a sheet a dense table covers: `rows` by `cols` cells from `(row, col)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DenseBox {
    row: u32,
    col: u32,
    rows: u32,
    cols: u32,
}

impl DenseBox {
    /// The cells the box covers (a box is only ever built inside [`DENSE_ONE_MAX_CELLS`]).
    fn cells(self) -> usize {
        (self.rows as usize).saturating_mul(self.cols as usize)
    }

    /// Where `c` sits in the box, when the box covers it — the whole lookup: two subtractions, two
    /// compares and a multiply-add.
    fn index(self, c: CellRef) -> Option<usize> {
        let row = c.row.checked_sub(self.row)? as usize;
        let col = c.col.checked_sub(self.col)? as usize;
        if row >= self.rows as usize || col >= self.cols as usize {
            return None;
        }
        row.checked_mul(self.cols as usize)?.checked_add(col)
    }

    /// The cell at slot `i` — the inverse of [`DenseBox::index`] (never `None` for an `i` the
    /// table holds: the two are written to agree).
    fn cell(self, i: usize) -> Option<CellRef> {
        let cols = self.cols as usize;
        if cols == 0 {
            return None;
        }
        let row = u32::try_from(i / cols).ok()?.checked_add(self.row)?;
        let col = u32::try_from(i % cols).ok()?.checked_add(self.col)?;
        Some(CellRef::new(row, col))
    }

    /// The box that covers both itself and `c`: the sides `c` is outside of stretch to it, the
    /// others keep their size.
    fn covering(self, c: CellRef) -> Option<(u32, u32, u32, u32)> {
        let row_end = self.row.checked_add(self.rows)?;
        let col_end = self.col.checked_add(self.cols)?;
        let (row, rows) = if c.row < self.row {
            (c.row, row_end.checked_sub(c.row)?)
        } else if c.row >= row_end {
            (self.row, c.row.checked_sub(self.row)?.checked_add(1)?)
        } else {
            (self.row, self.rows)
        };
        let (col, cols) = if c.col < self.col {
            (c.col, col_end.checked_sub(c.col)?)
        } else if c.col >= col_end {
            (self.col, c.col.checked_sub(self.col)?.checked_add(1)?)
        } else {
            (self.col, self.cols)
        };
        Some((row, col, rows, cols))
    }
}

/// A dependent cell in a dense slot: row plus one in the high word, column in the low one, so a
/// slot of `0` says "no dependent" and no other value does. The row a sheet cannot hold
/// (`u32::MAX`) is refused rather than packed to zero.
fn pack(c: CellRef) -> Option<u64> {
    Some(u64::from(c.row.checked_add(1)?) << 32 | u64::from(c.col))
}

/// The cell [`pack`] stored. Only `pack` writes a slot, and it writes no `0`; a row of
/// `u32::MAX` cannot be stored either, which is the one row `saturating_sub` answers for.
fn unpack(v: u64) -> CellRef {
    CellRef::new(((v >> 32) as u32).saturating_sub(1), v as u32)
}

/// Why a dense one-dependent table was not built ([DenseOne::build]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refused {
    /// The dependents do not fill the box around them closely enough ([DENSE_ONE_FILL]).
    Sparse,
    /// Their box is past the cells one sheet's dense table may cover, or the allocator would not
    /// hold it: the sheet keeps the hash table for good.
    TooBig,
}

/// The precedents of a sheet that exactly one formula reads, as one cell of a box instead of a
/// hash entry.
///
/// This is the index a workbook of ordinary formulas is: the reference workbook puts one
/// precedent a row in it, and every probe of the edit closure is a cold cache line of a 34 MB hash
/// table. The box answers the same question without hashing — the precedent's offset in its own
/// box, a load — and lays the slots out in row-major order, which is the order a column of
/// dependents walks them in.
///
/// The entry is eight bytes (the dependent cell packed, [`pack`]) against the eighteen a hash
/// entry of the same pair takes, and the box is only taken when it holds at most
/// [`DENSE_ONE_FILL`] cells a dependent — so the index is smaller than the table it replaces, and
/// the precedents a box does not round up ([`SheetDeps::one`]) stay in the hash table beside it.
#[derive(Clone, Debug)]
struct DenseOne {
    /// The box the slots cover, and where its first cell is.
    area: DenseBox,
    /// `area.cells()` slots in row-major order: `0` when no formula reads the cell there, else the
    /// dependent cell packed.
    slots: Vec<u64>,
    /// The cells this box may cover before it is copied: the row and column counts may stretch
    /// until they reach it, and only then is the table rebuilt ([DenseOne::grown]). It is what
    /// makes growth geometric — a run of precedents down a column copies the box once a doubling
    /// of the run instead of once a row, which is the whole of the last attempt's O(n²) build.
    capacity: usize,
    /// How many slots hold a dependent. Kept as the slots are written, because a delete leaves the
    /// box its size and this count is what says whether it is still worth having.
    len: usize,
}

impl DenseOne {
    /// The table for `entries` precedents, when the box around them is worth one cell each.
    fn build(entries: &[(CellRef, CellRef)]) -> Result<Self, Refused> {
        Self::build_within(entries, 0)
    }

    /// The table for `entries` precedents, allowed up to `allowed` cells however sparse they are
    /// — what a batch rebuild is held to when the box it would replace already covers that many
    /// cells, so taking it is never a memory regression ([`SheetDeps::trim`]).
    fn build_within(entries: &[(CellRef, CellRef)], allowed: usize) -> Result<Self, Refused> {
        let Some(first) = entries.first() else { return Err(Refused::Sparse) };
        let (mut r0, mut r1, mut c0, mut c1) = (first.0.row, first.0.row, first.0.col, first.0.col);
        for (c, _) in entries {
            r0 = r0.min(c.row);
            r1 = r1.max(c.row);
            c0 = c0.min(c.col);
            c1 = c1.max(c.col);
        }
        let area = DenseBox {
            row: r0,
            col: c0,
            rows: r1.checked_sub(r0).and_then(|n| n.checked_add(1)).ok_or(Refused::TooBig)?,
            cols: c1.checked_sub(c0).and_then(|n| n.checked_add(1)).ok_or(Refused::TooBig)?,
        };
        let cells = area.cells();
        if cells > DENSE_ONE_MAX_CELLS {
            return Err(Refused::TooBig);
        }
        // Two cells a dependent, or the cells of the box this replaces when that is more.
        if cells > dense_budget(entries.len()).max(allowed).min(DENSE_ONE_MAX_CELLS) {
            return Err(Refused::Sparse);
        }
        // `try_reserve` instead of `vec![0; n]`: a box the allocator refuses is a reason to keep
        // the hash table, not to abort a recalculation.
        let mut slots: Vec<u64> = Vec::new();
        slots.try_reserve_exact(cells).map_err(|_| Refused::TooBig)?;
        slots.resize(cells, 0);
        // A fresh box is tight: it may grow to twice its cells before it is copied again.
        let mut dense = DenseOne { area, slots, capacity: cells, len: 0 };
        for (c, k) in entries {
            dense.put(*c, *k);
        }
        if dense.len != entries.len() {
            // The box is the bounds of the very keys it holds, so every entry has a slot; a table
            // that lost one is refused rather than kept short of its entries.
            return Err(Refused::Sparse);
        }
        Ok(dense)
    }

    /// The dependent of the precedent `c`, when the box holds one.
    fn get(&self, c: CellRef) -> Option<CellRef> {
        let v = *self.slots.get(self.area.index(c)?)?;
        if v == 0 { None } else { Some(unpack(v)) }
    }

    /// Stores `k` as the dependent of `c`, answering whether the slot was empty (or `false`
    /// when the box does not cover `c` at all).
    fn put(&mut self, c: CellRef, k: CellRef) -> bool {
        let (Some(i), Some(packed)) = (self.area.index(c), pack(k)) else { return false };
        let Some(slot) = self.slots.get_mut(i) else { return false };
        let was_empty = *slot == 0;
        *slot = packed;
        if was_empty {
            self.len = self.len.saturating_add(1);
        }
        was_empty
    }

    /// Drops the dependent of `c`, when the box holds one there.
    fn clear(&mut self, c: CellRef) {
        if let Some(i) = self.area.index(c)
            && let Some(slot) = self.slots.get_mut(i)
            && *slot != 0
        {
            *slot = 0;
            self.len = self.len.saturating_sub(1);
        }
    }

    /// Every precedent the box holds with its dependent, in the box's own order.
    fn entries(&self) -> impl Iterator<Item = (CellRef, CellRef)> + '_ {
        (0..self.slots.len()).filter_map(move |i| {
            let v = *self.slots.get(i)?;
            if v == 0 {
                return None;
            }
            Some((self.area.cell(i)?, unpack(v)))
        })
    }

    /// The box grown to cover the precedent `c`, with the dependents it holds re-slotted; `None`
    /// when `c` stays in the hash table instead.
    ///
    /// The copy is bought by the headroom it leaves: the box takes what `c` needs and doubles it,
    /// so the entries that arrive after this one — the run of formulas `fill.formulas` links — land
    /// in the box and the next copy comes when the run has doubled again. That is one copy a
    /// doubling: the last attempt grew the box to the bounds of every new precedent, which copied
    /// it once a link and took a 164 ms fill to 37.96 s. A cell more than twice the budget away is
    /// refused outright and keeps the hash table's entry, and the batch at the end of the command
    /// ([SheetDeps::trim]) tries the box around all of them once.
    fn grown(&self, c: CellRef, budget: usize) -> Option<Self> {
        let (row, col, rows, cols) = self.area.covering(c)?;
        let needed = (rows as usize).checked_mul(cols as usize)?;
        // A cell more than a doubling of the budget away is not worth a box around it: it keeps the
        // hash table's entry, and the batch at the end of the command ([SheetDeps::trim]) tries the
        // box around all of them once.
        if needed > DENSE_ONE_MAX_CELLS || needed > budget.saturating_mul(2) {
            return None;
        }
        // The cells the grown box may cover: twice what `c` needs — the headroom the copy is paid
        // for — and never less than the box already allowed, nor past the sheet's ceiling.
        let target = needed.saturating_mul(2).max(self.capacity).min(DENSE_ONE_MAX_CELLS);
        // The sides the grown area needs double while the room is there, so the next `capacity`
        // cells land in this box and the one after it costs the next copy, not this one.
        let mut area = DenseBox { row, col, rows, cols };
        while let Some(doubled) = area.rows.checked_mul(2) {
            if (doubled as usize).checked_mul(area.cols as usize)? > target {
                break;
            }
            area.rows = doubled;
        }
        while let Some(doubled) = area.cols.checked_mul(2) {
            if (area.rows as usize).checked_mul(doubled as usize)? > target {
                break;
            }
            area.cols = doubled;
        }
        let cells = area.cells();
        if cells > DENSE_ONE_MAX_CELLS {
            return None;
        }
        let mut slots: Vec<u64> = Vec::new();
        slots.try_reserve_exact(cells).ok()?;
        slots.resize(cells, 0);
        let mut grown = DenseOne { area, slots, capacity: target.max(cells), len: 0 };
        for (c, k) in self.entries() {
            grown.put(c, k);
        }
        Some(grown)
    }

    /// Heap bytes of the box (see [`GraphMemory`]).
    fn bytes(&self) -> usize {
        self.slots.capacity().saturating_mul(std::mem::size_of::<u64>())
    }
}

/// The precedents of one sheet, and the formulas of that sheet that read them.
///
/// The single-dependent entry — the entry that dominates a workbook: `B2` feeding `G2` — has two
/// representations, chosen per sheet. A hash table of `CellRef -> CellRef`, which is what a
/// workbook whose precedents are scattered wants, and a dense box ([DenseOne]) of one slot a cell
/// of the box the precedents fill, which answers a probe without hashing.
///
/// A sheet starts on the hash table and moves to the box once it holds [`DENSE_ONE_PROMOTE`]
/// entries (see [`SheetDeps::promote`]), so a sheet that stays small pays nothing for a box, and
/// the precedents that later land outside the box's budget keep the hash table's entry beside it
/// — a sheet is never rebuilt because one formula reads a cell far away. [`SheetDeps::trim`]
/// gives a box back once deletes have left it much larger than the dependents it holds.
///
/// The entries two or more formulas read are the same `Vec`-a-precedent table as before, and a
/// precedent that all but one of them stop reading leaves that table for the single-dependent one
/// (see [`Deps`]).
#[derive(Clone, Debug, Default)]
pub(super) struct SheetDeps {
    /// The precedents exactly one formula reads, by hash: every one of them before the box is
    /// built, and the ones the box does not cover after.
    one: HashMap<CellRef, CellRef>,
    /// The precedents two or more formulas read, each with its list (never empty).
    many: HashMap<CellRef, Vec<CellRef>>,
    /// The one-dependent entries by arithmetic, when the box around them is worth it.
    dense: Option<DenseOne>,
    /// The `one` entry count at the last attempt to build the box, so that attempt — a pass over
    /// every entry — is paid a logarithmic number of times; `usize::MAX` once the hash table has
    /// won for good.
    tried: usize,
    /// The `one` entry count at the last time the box refused them and the batch at the end of the
    /// command ([SheetDeps::trim]) could not take them, so that pass too is paid once per doubling
    /// of the entries waiting beside the box rather than once a command.
    batched: usize,
}

impl SheetDeps {
    fn is_empty(&self) -> bool {
        self.one.is_empty() && self.many.is_empty() && self.dense.as_ref().is_none_or(|d| d.len == 0)
    }

    /// Reserves room in the hash table for `entries` precedents: the box, if one is built, takes
    /// no reservation (its geometry is not known before the entries arrive).
    fn reserve(&mut self, entries: usize) {
        self.one.reserve(entries);
    }

    /// The table with the dependent `k` of precedent cell `c` added, unless it is already there
    /// (see [`Deps::link`] for why a long list stops checking).
    fn link(&mut self, c: CellRef, k: CellRef) {
        if let Some(v) = self.many.get_mut(&c) {
            if v.len() > LONG_LIST || !v.contains(&k) {
                v.push(k);
            }
            return;
        }
        if let Some(first) = self.dense.as_ref().and_then(|d| d.get(c)) {
            if first == k {
                return;
            }
            if let Some(dense) = self.dense.as_mut() {
                dense.clear(c);
            }
            self.many.insert(c, vec![first, k]);
            return;
        }
        if let Some(first) = self.one.get(&c).copied() {
            if first == k {
                return;
            }
            self.one.remove(&c);
            self.many.insert(c, vec![first, k]);
            return;
        }
        // The first dependent of this precedent: in the box when it covers the cell (or can be
        // grown to hold it), in the hash table otherwise.
        if self.place_dense(c, k) {
            return;
        }
        self.one.insert(c, k);
        self.promote();
    }

    /// The table without the dependent `k` of precedent cell `c`; a precedent nothing reads any
    /// more leaves the table it was in, so a query never walks an empty slot.
    fn unlink(&mut self, c: CellRef, k: CellRef) {
        let Some(v) = self.many.get_mut(&c) else {
            if self.dense.as_ref().and_then(|d| d.get(c)) == Some(k) {
                if let Some(dense) = self.dense.as_mut() {
                    dense.clear(c);
                }
            } else if self.one.get(&c) == Some(&k) {
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
                if let Some(last) = last
                    && !self.place_dense(c, last)
                {
                    self.one.insert(c, last);
                }
            }
            _ => {}
        }
    }

    /// Stores the single dependent `k` of the precedent `c` in the box, growing the box around it
    /// when it is outside, and answers whether it could — `false` when there is no box yet or the
    /// cell is past the cells the box may cover, which leaves the entry to the hash table.
    fn place_dense(&mut self, c: CellRef, k: CellRef) -> bool {
        let Some(dense) = self.dense.as_ref() else { return false };
        if dense.area.index(c).is_some() {
            let Some(dense) = self.dense.as_mut() else { return false };
            return dense.put(c, k);
        }
        // The cells a sheet may cover count every single-dependent entry it would hold, the ones
        // the hash table still keeps included.
        let entries = dense.len.saturating_add(self.one.len()).saturating_add(1);
        let Some(mut grown) = dense.grown(c, dense_budget(entries)) else { return false };
        count_copy();
        let placed = grown.put(c, k);
        self.dense = Some(grown);
        placed
    }

    /// Builds the dense box out of the hash table once the entries are numerous enough for it to
    /// be worth one, retried at twice the entries when the box around them was not worth having.
    fn promote(&mut self) {
        if self.dense.is_some() || force_sparse() {
            return;
        }
        let n = self.one.len();
        if n < DENSE_ONE_PROMOTE || n < self.tried.saturating_mul(2) {
            return;
        }
        self.tried = n;
        let entries: Vec<(CellRef, CellRef)> = self.one.iter().map(|(&c, &k)| (c, k)).collect();
        match DenseOne::build(&entries) {
            Ok(dense) => {
                self.dense = Some(dense);
                // The box answers every entry it took, so the hash table gives its slots back:
                // they are the memory the box was bought with.
                self.one.clear();
                self.one.shrink_to(0);
            }
            Err(Refused::Sparse) => {}
            Err(Refused::TooBig) => self.tried = usize::MAX,
        }
    }

    /// The batch at the end of a command ([Graph::settle]): takes the entries the box refused
    /// during the command into it when the box around all of them is worth having, and gives back
    /// the room deletes have left.
    ///
    /// This is the one pass over a sheet's entries a command is allowed, and only when there is
    /// something to collect — a box that is neither short of its dependents nor holding any they
    /// were refused into answers in constant time, which is the state an ordinary edit leaves.
    fn trim(&mut self) {
        let Some(dense) = self.dense.take() else { return };
        let entries = dense.len.saturating_add(self.one.len());
        // The box covers more cells than its dependents justify — deletes left it that way, or a
        // copy got ahead of the entries — so it is rebuilt tight.
        let shrunken = dense.area.cells() > dense_budget(entries);
        if self.one.is_empty() && !shrunken {
            self.dense = Some(dense);
            return;
        }
        // Entries wait beside the box: retry at twice as many, unless the box wants shrinking
        // anyway.
        if !shrunken && self.one.len() < self.batched.saturating_mul(2) {
            self.dense = Some(dense);
            return;
        }
        self.batched = self.one.len();
        let mut held: Vec<(CellRef, CellRef)> = Vec::with_capacity(entries);
        held.extend(dense.entries());
        held.extend(self.one.iter().map(|(&c, &k)| (c, k)));
        // The box it replaces is the memory it may cost: a rebuild that covers no more cells than
        // the box already did is never a regression, and it is what lets a sheet whose precedents
        // grow a column at a time keep its index (the entries are sparse while they arrive and
        // tight once they have).
        match DenseOne::build_within(&held, dense.area.cells()) {
            Ok(tight) => {
                // The box now answers everything: the hash entries it took give their slots back,
                // which is the memory the box was bought with.
                self.dense = Some(tight);
                self.one.clear();
                self.one.shrink_to(0);
                self.batched = 0;
            }
            Err(_) => {
                // The entries do not fill a box worth the cells they span (a precedent far from
                // the rest keeps the hash table's entry beside the box, see [`DenseOne::grown`]):
                // the box stays as it is rather than being given up for a few stray cells.
                self.dense = Some(dense);
            }
        }
    }

    /// Appends the dependents of `c` to `out`, as the keys of `sheet`.
    fn push_into(&self, c: CellRef, sheet: usize, out: &mut Vec<Key>) {
        self.for_each_of(c, &mut |k| out.push((sheet, k)));
    }

    /// Calls `f` for every dependent of `c`: the one the box holds, the one the hash table holds,
    /// and the list of two or more.
    ///
    /// A table that holds nothing is not probed, and the box is the only one an ordinary sheet
    /// has: the walk asks it and reaches no map at all.
    fn for_each_of(&self, c: CellRef, f: &mut impl FnMut(CellRef)) {
        if let Some(dense) = &self.dense
            && let Some(k) = dense.get(c)
        {
            f(k);
        }
        if !self.one.is_empty()
            && let Some(k) = self.one.get(&c)
        {
            f(*k);
        }
        if !self.many.is_empty()
            && let Some(v) = self.many.get(&c)
        {
            for k in v {
                f(*k);
            }
        }
    }

    /// Calls `f` for every precedent cell and dependent the table holds.
    fn for_each(&self, mut f: impl FnMut(CellRef, CellRef)) {
        if let Some(dense) = &self.dense {
            for (c, k) in dense.entries() {
                f(c, k);
            }
        }
        for (c, k) in &self.one {
            f(*c, *k);
        }
        for (c, v) in &self.many {
            for k in v {
                f(*c, *k);
            }
        }
    }

    /// Heap bytes of both tables and how many multi-dependent lists they hold (see
    /// [`GraphMemory`]).
    fn memory(&self) -> (usize, usize) {
        use std::mem::size_of;
        let mut bytes = cella_model::hash_index_bytes(self.one.capacity(), size_of::<(CellRef, CellRef)>())
            .saturating_add(cella_model::hash_index_bytes(self.many.capacity(), size_of::<(CellRef, Vec<CellRef>)>()));
        if let Some(dense) = &self.dense {
            bytes = bytes.saturating_add(dense.bytes());
        }
        let mut lists = 0usize;
        for v in self.many.values() {
            lists = lists.saturating_add(1);
            bytes = bytes.saturating_add(v.capacity().saturating_mul(size_of::<CellRef>()));
        }
        (bytes, lists)
    }

    /// The slot counts of the two hash tables, for the tests that hold the index's size.
    #[cfg(test)]
    fn slots(&self) -> usize {
        cella_model::hash_index_bytes(self.one.capacity(), 0).saturating_add(cella_model::hash_index_bytes(self.many.capacity(), 0))
    }

    /// Every precedent with a dependent it answers, as a walk finds it — the pairs the dense box
    /// has to hold key for key (see [`Graph::index_answers`]).
    #[cfg(test)]
    fn answers(&self) -> Vec<(CellRef, CellRef)> {
        let mut out = Vec::new();
        self.for_each(|c, k| out.push((c, k)));
        out.sort_unstable();
        out
    }

    /// Whether the box is the sheet's index (the test that it was built at all).
    #[cfg(test)]
    fn is_dense(&self) -> bool {
        self.dense.is_some()
    }

    /// The cells the box covers, and the slots that hold a dependent.
    #[cfg(test)]
    fn dense_cells(&self) -> (usize, usize) {
        self.dense.as_ref().map_or((0, 0), |d| (d.slots.len(), d.len))
    }
}

#[cfg(test)]
thread_local! {
    /// Whether the dense one-dependent table is refused, so a test can hold the box and the hash
    /// table to the same answers (see [`without_dense`]).
    static NO_DENSE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// How many times a box was copied whole, so a test can hold growth to one copy a doubling
    /// (the O(n²) build the first attempt died of — see [`SheetDeps::place_dense`]).
    static DENSE_COPIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Counts a copy of a box (see [`DENSE_COPIES`]).
#[cfg(test)]
fn count_copy() {
    DENSE_COPIES.with(|n| n.set(n.get().saturating_add(1)));
}

/// Counts nothing: the product builds the boxes and only a test counts them.
#[cfg(not(test))]
fn count_copy() {}

/// The boxes copied on this thread since [`reset_copies`].
#[cfg(test)]
pub(super) fn dense_copies() -> usize {
    DENSE_COPIES.with(std::cell::Cell::get)
}

/// Starts counting the boxes copied on this thread from zero.
#[cfg(test)]
pub(super) fn reset_copies() {
    DENSE_COPIES.with(|n| n.set(0));
}

/// Runs `f` with the dense one-dependent table refused (`on`) — or back to being built — for this
/// thread: the tests that the two representations answer the same dependents.
#[cfg(test)]
pub(super) fn without_dense<T>(on: bool, f: impl FnOnce() -> T) -> T {
    let previous = NO_DENSE.replace(on);
    let out = f();
    NO_DENSE.set(previous);
    out
}

/// Whether the dense table is refused for this thread (see [`without_dense`]).
#[cfg(test)]
fn force_sparse() -> bool {
    NO_DENSE.get()
}

/// Whether the dense table is refused: the product builds it, and only a test turns it off.
#[cfg(not(test))]
fn force_sparse() -> bool {
    false
}

/// Which formulas read which single cell: the bulk of the graph, and the bulk of its memory.
///
/// A formula nearly always reads a cell of its own sheet, so that case drops the sheet from both
/// sides of the entry (the table is the sheet's): 8-byte key, 8-byte dependent, hashbrown's control
/// byte — 17 bytes a slot against the 57 of the single `Key → Deps` table this replaced — and, once
/// a sheet holds enough of them, the dense box of [`SheetDeps`] instead of the hash table. A
/// formula reading another sheet (a summary tab over a data tab) is the rare case and keeps full
/// [`Key`]s, in a table of its own so it costs the common one nothing.
#[derive(Clone, Debug, Default)]
pub(super) struct CellDeps {
    /// sheet → the precedents in it, and the formulas of that sheet that read them.
    same: HashMap<usize, SheetDeps>,
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

    fn sheet_mut(&mut self, sheet: usize) -> &mut SheetDeps {
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
            deps.push_into(c, sheet, out);
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
        let mut bytes = cella_model::hash_index_bytes(self.same.capacity(), size_of::<(usize, SheetDeps)>());
        let mut lists = 0usize;
        for deps in self.same.values() {
            let (part, count) = deps.memory();
            bytes = bytes.saturating_add(part);
            lists = lists.saturating_add(count);
        }
        let (part, count) = self.cross.memory();
        (bytes.saturating_add(part), lists.saturating_add(count))
    }

    /// Gives back a sheet's box once deletes have left it much larger than the dependents it holds
    /// (see [`SheetDeps::trim`]).
    fn trim(&mut self) {
        for deps in self.same.values_mut() {
            deps.trim();
        }
        self.same.retain(|_, deps| !deps.is_empty());
    }

    /// Every precedent the single-cell index answers for with the dependent it answers, in one
    /// order: the pairs the dense box and the hash table have to hold identically (see
    /// [`SheetDeps::answers`]).
    #[cfg(test)]
    fn answers(&self) -> Vec<(Key, Key)> {
        let mut out = Vec::new();
        for (&sheet, deps) in &self.same {
            for (c, k) in deps.answers() {
                out.push(((sheet, c), (sheet, k)));
            }
        }
        self.cross.for_each(|c, k| out.push((c, k)));
        out.sort_unstable();
        out
    }

    /// The slot counts of every table (see [`Graph::cell_deps_slots`]).
    #[cfg(test)]
    fn slots(&self) -> usize {
        let mut slots = cella_model::hash_index_bytes(self.same.capacity(), 0).saturating_add(self.cross.slots());
        for deps in self.same.values() {
            slots = slots.saturating_add(deps.slots());
        }
        slots
    }
}

/// The tables a dependents walk answers from, with the ones a workbook leaves empty set aside.
///
/// A cell's dependents are in its sheet's table. The three other tables answer only for a
/// reference another sheet reads, a whole-column reference and a range wider than the `WIDE`
/// columns cutoff, and a workbook of ordinary formulas has none of them — yet the walk probed all
/// four per cell, so a chain of a million formulas paid five hash lookups a cell for one answer.
/// This asks the three once, remembers the sheet's table across the cells of that sheet, and skips
/// a table that holds nothing at all (`Deps::for_each_of`).
pub(super) struct Walker<'a> {
    same: &'a HashMap<usize, SheetDeps>,
    cross: &'a Deps<Key, Key>,
    col_deps: &'a HashMap<(usize, u32), Intervals<Key>>,
    wide_deps: &'a HashMap<usize, Intervals<(u32, u32, Key)>>,
    has_cross: bool,
    has_cols: bool,
    has_wide: bool,
    /// The sheet whose table the last lookup asked for, and what it found. The graph does not change
    /// under a walk, so a run of cells of one sheet — a column of formulas — pays for it once.
    cached: Option<(usize, Option<&'a SheetDeps>)>,
}

impl<'a> Walker<'a> {
    fn of(graph: &'a Graph) -> Self {
        Self {
            same: &graph.cell_deps.same,
            cross: &graph.cell_deps.cross,
            col_deps: &graph.col_deps,
            wide_deps: &graph.wide_deps,
            has_cross: !graph.cell_deps.cross.is_empty(),
            has_cols: !graph.col_deps.is_empty(),
            has_wide: !graph.wide_deps.is_empty(),
            cached: None,
        }
    }

    /// The table of `sheet`, or the one the last lookup found when it is the same sheet.
    fn sheet(&mut self, sheet: usize) -> Option<&'a SheetDeps> {
        match self.cached {
            Some((asked, found)) if asked == sheet => found,
            _ => {
                let found = self.same.get(&sheet);
                self.cached = Some((sheet, found));
                found
            }
        }
    }

    /// Calls `f` for every formula that reads the cell `c` of `sheet`.
    pub(super) fn dependents_of(&mut self, sheet: usize, c: CellRef, f: &mut impl FnMut(Key)) {
        if let Some(deps) = self.sheet(sheet) {
            deps.for_each_of(c, &mut |k| f((sheet, k)));
        }
        if self.has_cross {
            self.cross.for_each_of((sheet, c), f, |k| k);
        }
        if self.has_cols
            && let Some(v) = self.col_deps.get(&(sheet, c.col))
        {
            v.query(c.row, c.row, &mut |k| f(*k));
        }
        if self.has_wide
            && let Some(v) = self.wide_deps.get(&sheet)
        {
            v.query(c.row, c.row, &mut |(c0, c1, k)| {
                if *c0 <= c.col && c.col <= *c1 {
                    f(*k);
                }
            });
        }
    }
}

/// One formula of the graph.
///
/// Two words: where its run of [`Graph::areas`] starts, and the run's length together with the
/// flags (`RunLen`), plus the shape the formula reads. The flags used to be a byte of their own
/// beside the length, which padded the node out to a fourth word — 24 bytes for a field with six
/// bits set, in a table entry of the key's 16 bytes and hashbrown's control byte: 41 bytes of
/// table a slot, which the reference workbook's two million nodes paid 172 MB for. Packed, the
/// entry was 33 bytes a slot and the same nodes cost 138. S7 of `plan/SHAPE-SHARING.md` then
/// deleted the node's own tree pointer, so a node is **16** bytes — and the table holds it beside
/// an 8-byte cell instead of a 16-byte `(sheet, cell)` key ([`NodeTable`]): 25 bytes of table a
/// slot, 105 MB for those two million nodes.
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
    pub(super) shape: Option<std::sync::Arc<cella_formula::Shape>>,
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
    pub(super) fn with_shape(mut self, shape: Option<std::sync::Arc<cella_formula::Shape>>) -> Self {
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

/// The formula nodes, one `CellRef → Node` table a sheet.
///
/// A formula normally sits on the sheet the graph is reporting on, so the table that holds them
/// drops the sheet from its key — the same split, and for the same reason, as [`CellDeps`]: an
/// 8-byte key, a 16-byte node and hashbrown's control byte, **25 bytes a slot**, against the 33 of
/// the single `(sheet, cell) → Node` table this replaced (a 16-byte key padded to two words). The
/// reference workbook's two million formulas paid 138 MB for that table and pay 105 for this one.
///
/// The outer map costs one entry a sheet that holds formulas: a sheet of values takes no table,
/// and a sheet whose last formula leaves gives its table back ([`NodeTable::remove`]), so the
/// shape follows the workbook rather than the sheets it once had.
#[derive(Clone, Debug, Default)]
pub(super) struct NodeTable {
    /// sheet → its formulas, by cell.
    sheets: HashMap<usize, HashMap<CellRef, Node>>,
}

impl NodeTable {
    /// The node of the formula at `k`.
    fn get(&self, k: Key) -> Option<&Node> {
        self.sheets.get(&k.0)?.get(&k.1)
    }

    /// The formulas of `sheet`, by cell — for a pass that lists a run of one sheet's cells and
    /// would otherwise pay the sheet's own probe once a cell (see [`Graph::nodes_of`]).
    fn sheet(&self, sheet: usize) -> Option<&HashMap<CellRef, Node>> {
        self.sheets.get(&sheet)
    }

    /// Whether a formula is stored at `k`.
    fn contains_key(&self, k: Key) -> bool {
        self.sheets.get(&k.0).is_some_and(|m| m.contains_key(&k.1))
    }

    /// Stores `node` at `k`, answering the node it replaced.
    fn insert(&mut self, k: Key, node: Node) -> Option<Node> {
        self.sheets.entry(k.0).or_default().insert(k.1, node)
    }

    /// Drops the formula at `k`, answering its node; the sheet's table leaves with its last
    /// formula, so a workbook edited down to values holds no node table at all.
    fn remove(&mut self, k: Key) -> Option<Node> {
        let sheet = self.sheets.get_mut(&k.0)?;
        let old = sheet.remove(&k.1);
        if sheet.is_empty() {
            self.sheets.remove(&k.0);
        }
        old
    }

    fn is_empty(&self) -> bool {
        self.sheets.is_empty()
    }

    fn len(&self) -> usize {
        self.sheets.values().map(|m| m.len()).sum()
    }

    /// Reserves room in the table of `sheet` for `formulas` more formulas (see
    /// [`Graph::reserve`]): a bulk build would otherwise grow it a dozen times, and each growth
    /// rehashes every entry already in it. A sheet with nothing to reserve takes no table.
    fn reserve(&mut self, sheet: usize, formulas: usize) {
        if formulas == 0 {
            return;
        }
        self.sheets.entry(sheet).or_default().reserve(formulas);
    }

    /// Every formula the table holds, as its `(sheet, cell)` key.
    fn keys(&self) -> impl Iterator<Item = Key> + '_ {
        self.sheets.iter().flat_map(|(sheet, m)| m.keys().map(move |c| (*sheet, *c)))
    }

    /// Every node, for the one pass that rewrites what they point at in the arena
    /// ([`Graph::compact_areas`]).
    fn values_mut(&mut self) -> impl Iterator<Item = &mut Node> + '_ {
        self.sheets.values_mut().flat_map(|m| m.values_mut())
    }

    /// Drops the tables of sheets that hold no formula ([`Graph::settle`]): a reserve for formulas
    /// that never arrived — a sheet whose text does not parse, say — leaves one behind.
    fn drop_empty(&mut self) {
        self.sheets.retain(|_, m| !m.is_empty());
        if self.sheets.is_empty() {
            // The outer map is a sheet's entry and nothing else: a graph of values holds no
            // allocation here at all, not a map waiting for the next formula.
            self.sheets.shrink_to(0);
        }
    }

    /// Heap bytes of the outer map and of every sheet's table (see [`GraphMemory`]).
    fn memory(&self) -> usize {
        use std::mem::size_of;
        let mut bytes = cella_model::hash_index_bytes(self.sheets.capacity(), size_of::<(usize, HashMap<CellRef, Node>)>());
        for m in self.sheets.values() {
            bytes = bytes.saturating_add(cella_model::hash_index_bytes(m.capacity(), size_of::<(CellRef, Node)>()));
        }
        bytes
    }

    /// The slot counts of the outer map and of every sheet's table (see [`Graph::node_slots`]).
    #[cfg(test)]
    fn slots(&self) -> usize {
        let mut slots = cella_model::hash_index_bytes(self.sheets.capacity(), 0);
        for m in self.sheets.values() {
            slots = slots.saturating_add(cella_model::hash_index_bytes(m.capacity(), 0));
        }
        slots
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
            .saturating_add(cella_model::btree_index_bytes(self.pending.len(), item))
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
    /// The node table: the per-sheet outer map, and each sheet's `CellRef → Node` slots and
    /// control bytes.
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
    cella_model::hash_index_bytes(set.capacity(), std::mem::size_of::<T>())
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
    nodes: NodeTable,
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

    /// The node of the formula at `k` — the sheet's table, its cell's entry.
    pub(super) fn node(&self, k: Key) -> Option<&Node> {
        self.nodes.get(k)
    }

    /// The formulas of `sheet`, by cell.
    ///
    /// A pass lists its cells in row-major order, so the cells of one sheet arrive together: asking
    /// the sheet's table once for the run replaces the sheet-table probe `node` makes a cell, which
    /// on a million-formula pass is a million probes for one answer.
    pub(super) fn nodes_of(&self, sheet: usize) -> Option<&HashMap<CellRef, Node>> {
        self.nodes.sheet(sheet)
    }

    /// Whether the graph holds a formula at `k`.
    pub(super) fn has_node(&self, k: Key) -> bool {
        self.nodes.contains_key(k)
    }

    /// Every formula the graph holds, as its `(sheet, cell)` key (a rebuild's or a full
    /// recalculation's work list).
    pub(super) fn node_keys(&self) -> impl Iterator<Item = Key> + '_ {
        self.nodes.keys()
    }

    /// The slot counts of the node table (see [`NodeTable::slots`]): the test that ratchets the
    /// table's size divides its bytes by them.
    #[cfg(test)]
    pub(super) fn node_slots(&self) -> usize {
        self.nodes.slots()
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

    /// Every precedent the single-cell index answers for with its dependent, in one order — what
    /// the dense box and the hash table have to agree on key for key.
    #[cfg(test)]
    pub(super) fn index_answers(&self) -> Vec<(Key, Key)> {
        self.cell_deps.answers()
    }

    /// Whether `sheet`'s single-dependent entries are in a dense box (the tests that one is built
    /// where it is worth it, and not where it is not).
    #[cfg(test)]
    pub(super) fn has_dense_index(&self, sheet: usize) -> bool {
        self.cell_deps.same.get(&sheet).is_some_and(SheetDeps::is_dense)
    }

    /// The cells `sheet`'s box covers, and how many of them hold a dependent.
    #[cfg(test)]
    pub(super) fn dense_index_cells(&self, sheet: usize) -> (usize, usize) {
        self.cell_deps.same.get(&sheet).map_or((0, 0), SheetDeps::dense_cells)
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
        // A workbook of plain formulas has no array at all, and a pass asks this for every formula
        // it evaluates: the side table being empty answers that without hashing a cell.
        if self.legacy_range.is_empty() {
            return None;
        }
        self.legacy_range.get(&k).copied()
    }

    /// Whether the formula at `k` calls a volatile function — the flag the precedents walk
    /// recorded when it visited its last node, so opening a workbook never walks a formula tree to
    /// find out. A cell without a node (no formula, or one that does not parse) is not volatile.
    pub(super) fn is_volatile(&self, k: Key) -> bool {
        self.nodes.get(k).is_some_and(Node::is_volatile)
    }

    /// Reserves room for `formulas` formulas of `sheet`, so a bulk build does not grow its tables
    /// a dozen times (each growth rehashes every entry already in it). The node and precedent tables
    /// reserved are the sheet's own — where its formulas and their same-sheet references land; a
    /// formula reading another sheet's cells goes to the cross table, which grows as it fills.
    pub(super) fn reserve(&mut self, sheet: usize, formulas: usize) {
        self.reserve_more(sheet, formulas);
        let precedent_cells = formulas.saturating_mul(3).saturating_div(2);
        self.cell_deps.reserve(sheet, precedent_cells);
    }

    /// Reserves room for `formulas` more formulas of `sheet` and their precedents, before a bulk
    /// insert (a fill, a paste of a column of formulas) — the part of [`Graph::reserve`] that is
    /// not one sheet's precedent table. The node table and the arena would otherwise grow a dozen
    /// times during the fill, each growth rehashing or copying everything already in them.
    pub(super) fn reserve_more(&mut self, sheet: usize, formulas: usize) {
        self.nodes.reserve(sheet, formulas);
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
        if let Some(old) = self.nodes.remove(k) {
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
    pub(super) fn node_shape(&self, k: Key) -> Option<std::sync::Arc<cella_formula::Shape>> {
        self.nodes.get(k).and_then(|n| n.shape.clone())
    }

    /// Every command ends here (a rebuild, a restore, an edit's commit), which is the one point
    /// the graph is quiescent: a fill through `insert` alone must not have the arena reallocated
    /// under it, so nothing below runs while one is in progress.
    pub(super) fn settle(&mut self) {
        // A reserve that no formula followed up on leaves a table nothing is in (the sheet a
        // rebuild reserved for, whose text did not parse): it gives it back here, once.
        self.nodes.drop_empty();
        self.cell_deps.trim();
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
        self.walker().dependents_of(sheet, c, &mut |k| out.push(k));
    }

    /// The tables a dependents walk answers from (see [`Walker`]): reused across the cells of a
    /// walk, so what a workbook leaves empty is checked once instead of once per cell.
    pub(super) fn walker(&self) -> Walker<'_> {
        Walker::of(self)
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
        self.nodes.get(k).map(|n| self.areas_of(n).iter().map(|a| a.to_area()).collect()).unwrap_or_default()
    }

    /// The live heap bytes of this graph, by allocation class: the numbers a memory report shows,
    /// and the shape a later representation change has to be measured against. Everything is
    /// `len`/`capacity`/`size_of` arithmetic except the multi-dependent lists, which the table
    /// cannot size for us; see [`GraphMemory`].
    pub fn memory(&self) -> GraphMemory {
        use std::mem::size_of;
        let (cell_deps, many_lists) = self.cell_deps.memory();
        GraphMemory {
            nodes: self.nodes.memory(),
            areas: self.areas.capacity().saturating_mul(size_of::<PackedArea>()),
            cell_deps,
            many_lists,
            cols: cella_model::hash_index_bytes(self.col_deps.capacity(), size_of::<((usize, u32), Intervals<Key>)>())
                .saturating_add(self.col_deps.values().map(Intervals::memory).sum::<usize>()),
            wide: cella_model::hash_index_bytes(self.wide_deps.capacity(), size_of::<(usize, Intervals<(u32, u32, Key)>)>())
                .saturating_add(self.wide_deps.values().map(Intervals::memory).sum::<usize>()),
            legacy_ranges: cella_model::hash_index_bytes(self.legacy_range.capacity(), size_of::<(Key, RangeRef)>()),
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
    use cella_core::CellRef;

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
        // The hash table is what this measures; the box a dense index builds instead is
        // `the_dense_index_is_no_larger_than_the_hash_table_it_replaces`.
        let mut g = Graph::default();
        without_dense(true, || {
            for row in 0..5_000u32 {
                g.insert(key(0, row, 6), &[single(0, row, 1), single(0, row, 2), single(0, row, 3)], None, Node::new(false, false, false));
            }
        });
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

    /// The same edits on two graphs, the second with the dense box refused: one set of entries, and
    /// one answer a precedent.
    fn two_ways(ops: &[&dyn Fn(&mut Graph)]) -> (Graph, Graph) {
        let mut dense = Graph::default();
        let mut sparse = Graph::default();
        for op in ops {
            op(&mut dense);
            without_dense(true, || op(&mut sparse));
        }
        dense.settle();
        without_dense(true, || sparse.settle());
        (dense, sparse)
    }

    /// The pairs two indexes hold, and the dependents each answers for every precedent of either:
    /// the assertion that a box and a hash table are the same index.
    fn assert_same_index(dense: &Graph, sparse: &Graph) {
        assert_eq!(dense.index_answers(), sparse.index_answers(), "the two indexes hold different entries");
        assert_eq!(dense.memory().many_lists, sparse.memory().many_lists, "the multi-dependent lists differ");
        let mut held = dense.index_answers();
        held.extend(sparse.index_answers());
        for (sheet, at) in held.into_iter().map(|(precedent, _)| precedent) {
            let (mut boxed, mut hashed) = (Vec::new(), Vec::new());
            dense.dependents(sheet, at, &mut boxed);
            sparse.dependents(sheet, at, &mut hashed);
            assert_eq!(boxed, hashed, "the dependents of sheet {sheet} at {at:?} differ");
        }
    }

    /// A sheet's single-cell precedents are a box where they fill their bounding box; the same
    /// edits on a graph whose box is refused are the same index in a hash table. The two have to
    /// hold the same entries and answer the same dependents, before and after formulas are
    /// replaced and dropped.
    #[test]
    fn the_dense_box_and_the_hash_table_answer_the_same_dependents() {
        let fill = |g: &mut Graph| {
            for row in 0..3_000u32 {
                let mut areas = vec![single(0, row, 0), single(0, row, 1)];
                // Every tenth formula also reads one cell every formula reads: a list.
                if row % 10 == 0 {
                    areas.push(single(0, 0, 5));
                }
                g.insert(key(0, row, 6), &areas, None, Node::new(false, false, false));
            }
            // A formula of another sheet (the cross table) and one that reads a column.
            g.insert(key(0, 0, 8), &[single(1, 2, 2)], None, Node::new(false, false, false));
            g.insert(key(0, 1, 8), &[whole_column(0, 3)], None, Node::new(false, false, false));
        };
        // A formula re-entered with different precedents: its entries leave and new ones arrive.
        let replace = |g: &mut Graph| {
            for row in (0..3_000u32).step_by(3) {
                g.insert(key(0, row, 6), &[single(0, row, 0), single(0, row, 2)], None, Node::new(false, false, false));
            }
        };
        // …and formulas cleared, which unlinks what they read.
        let drop_some = |g: &mut Graph| {
            for row in (0..3_000u32).step_by(7) {
                g.remove(key(0, row, 6));
            }
            g.remove(key(0, 0, 8));
        };
        let (dense, sparse) = two_ways(&[&fill]);
        assert!(dense.has_dense_index(0), "3,000 precedents fill a box");
        assert!(!sparse.has_dense_index(0), "the box is refused here");
        assert_same_index(&dense, &sparse);

        let (dense, sparse) = two_ways(&[&fill, &replace, &drop_some]);
        assert!(dense.has_dense_index(0), "a replaced formula is linked into the box again");
        assert_same_index(&dense, &sparse);
    }

    /// Precedents a thousand rows apart are not worth a box — one over them would memset a megabyte
    /// to answer a thousand probes — and a box past the cells one sheet may cover is refused for
    /// good: both keep the hash table.
    #[test]
    fn precedents_far_apart_keep_the_hash_table() {
        let mut g = Graph::default();
        for i in 0..1_200u32 {
            let row = i.saturating_mul(1_000);
            g.insert(key(0, row, 6), &[single(0, row, 1)], None, Node::new(false, false, false));
        }
        g.settle();
        assert_eq!(g.memory().many_lists, 0, "every entry is a single dependent");
        assert!(!g.has_dense_index(0), "1.2 million cells for 1,200 dependents is not a box worth having");
        assert_eq!(g.dense_index_cells(0), (0, 0), "and no box was allocated at all");

        // The cells one box may cover are capped (sixteen million, `DENSE_ONE_MAX_CELLS`), and a
        // sheet that meets the cap keeps the table even after it fills up around the cells that
        // made it too big.
        let mut capped = Graph::default();
        for i in 0..1_800u32 {
            let row = i.saturating_mul(10_000);
            capped.insert(key(0, row, 6), &[single(0, row, 1)], None, Node::new(false, false, false));
        }
        capped.settle();
        assert!(!capped.has_dense_index(0), "eighteen million cells is past the sixteen a box may cover");
        for row in 0..2_000u32 {
            capped.insert(key(0, row, 8), &[single(0, row, 7)], None, Node::new(false, false, false));
        }
        capped.settle();
        assert!(!capped.has_dense_index(0), "the cap's refusal is permanent for the sheet");
    }

    /// The box is the smaller index: sixteen bytes a dependent (two cells of eight, the fill ratio)
    /// against the twenty a `CellRef -> CellRef` hash entry takes.
    #[test]
    fn the_dense_index_is_no_larger_than_the_hash_table_it_replaces() {
        let fill = |g: &mut Graph| {
            for row in 0..20_000u32 {
                g.insert(key(0, row, 6), &[single(0, row, 1), single(0, row, 2)], None, Node::new(false, false, false));
            }
        };
        let (dense, sparse) = two_ways(&[&fill]);
        assert!(dense.has_dense_index(0), "40,000 precedents fill a box");
        let (cells, held) = dense.dense_index_cells(0);
        assert_eq!(held, 40_000, "two precedents a row");
        assert!(cells <= held.saturating_mul(DENSE_ONE_FILL), "{cells} cells for {held} dependents");
        let (boxed, hashed) = (dense.memory().cell_deps, sparse.memory().cell_deps);
        assert!(boxed < hashed, "the box costs {boxed} bytes where the hash table costs {hashed}");
    }

    /// The shape `fill.formulas` builds — a hundred thousand single precedents down one column —
    /// copies the box once a doubling of the run, never once a link. The copy count is the hard
    /// assertion (it is what makes the build O(n) instead of the O(n²) of the first attempt, which
    /// took 100k rows from 164 ms to 37.96 s); the wall clock is the loose one that says the whole
    /// build is small.
    #[test]
    #[allow(clippy::disallowed_methods)] // the clock is read only off wasm, as in `prof_now`
    fn a_hundred_thousand_linked_precedents_copy_the_box_once_a_doubling() {
        reset_copies();
        let start = std::time::Instant::now();
        let mut g = Graph::default();
        for row in 0..100_000u32 {
            g.insert(key(0, row, 6), &[single(0, row, 1)], None, Node::new(false, false, false));
        }
        g.settle();
        let elapsed = start.elapsed();
        let copies = dense_copies();
        assert!(g.has_dense_index(0), "100,000 precedents fill a box");
        let (cells, held) = g.dense_index_cells(0);
        assert_eq!(held, 100_000, "every precedent is in the box");
        assert!(cells <= held.saturating_mul(DENSE_ONE_FILL), "{cells} cells for {held} dependents");
        assert!(copies <= 64, "{copies} copies of the box for 100,000 precedents: growth is not geometric");
        assert!(elapsed < std::time::Duration::from_secs(5), "100,000 links took {elapsed:?}");
        // The same graph on the hash table holds the same entries.
        let (dense, sparse) = two_ways(&[&|g: &mut Graph| {
            for row in 0..2_000u32 {
                g.insert(key(0, row, 6), &[single(0, row, 1)], None, Node::new(false, false, false));
            }
            // …with rows arriving out of order, which grows the box both ways.
            for row in (2_000..4_000u32).rev() {
                g.insert(key(0, row, 6), &[single(0, row, 1)], None, Node::new(false, false, false));
            }
        }]);
        assert_same_index(&dense, &sparse);
    }

    /// An edit that adds and drops single precedents in a boxed sheet leaves the box holding
    /// exactly the entries the hash table of the same workbook holds — inserted out of order,
    /// inside the box, beside it and past the cells it may cover.
    #[test]
    fn inserts_and_deletes_after_the_box_leave_it_equal_to_the_hash_table() {
        let fill = |g: &mut Graph| {
            for row in 0..2_000u32 {
                g.insert(key(0, row, 6), &[single(0, row, 1)], None, Node::new(false, false, false));
            }
        };
        let edits = |g: &mut Graph| {
            // New precedents below the box (it grows), and above it (it moves).
            for row in 2_000..3_000u32 {
                g.insert(key(0, row, 6), &[single(0, row, 1)], None, Node::new(false, false, false));
            }
            for row in 0..200u32 {
                g.insert(key(0, row, 8), &[single(0, row, 9)], None, Node::new(false, false, false));
            }
            // A precedent a million rows away: past the cells the box may cover for its entries.
            g.insert(key(0, 1_000_000, 6), &[single(0, 1_000_000, 1)], None, Node::new(false, false, false));
            // Formulas dropped, one in three of them.
            for row in (0..2_000u32).step_by(3) {
                g.remove(key(0, row, 6));
            }
            // A second formula reading a precedent that had one: it leaves the box for a list.
            g.insert(key(0, 1, 7), &[single(0, 1, 1)], None, Node::new(false, false, false));
            // …and dropped again, which puts the precedent back in the box.
            g.remove(key(0, 1, 7));
        };
        let (dense, sparse) = two_ways(&[&fill, &edits]);
        assert!(dense.has_dense_index(0), "the box survives the edit");
        assert_same_index(&dense, &sparse);
    }

    /// A precedent far outside the box is linked into the hash table beside it rather than copied
    /// into an enormous box, and the box stays the sheet's index for everything else.
    #[test]
    fn a_precedent_far_from_the_box_keeps_the_hash_table_beside_it() {
        let mut g = Graph::default();
        for row in 0..4_000u32 {
            g.insert(key(0, row, 6), &[single(0, row, 1)], None, Node::new(false, false, false));
        }
        g.settle();
        assert!(g.has_dense_index(0), "4,000 precedents fill a box");
        let (cells, _) = g.dense_index_cells(0);
        reset_copies();
        g.insert(key(0, 900_000, 6), &[single(0, 900_000, 1)], None, Node::new(false, false, false));
        assert_eq!(dense_copies(), 0, "a cell 900,000 rows away is not worth copying the box for");
        assert!(g.has_dense_index(0), "the box is kept");
        g.settle();
        assert!(g.has_dense_index(0), "the batch refused the box around both and kept the one it had");
        assert_eq!(g.dense_index_cells(0).0, cells, "the box did not move");
        let mut out = Vec::new();
        g.dependents(0, CellRef::new(900_000, 1), &mut out);
        assert_eq!(out, vec![key(0, 900_000, 6)], "the far precedent is answered from the hash table");
        out.clear();
        g.dependents(0, CellRef::new(1_234, 1), &mut out);
        assert_eq!(out, vec![key(0, 1_234, 6)], "the boxed ones are answered from the box");
    }

    /// Deletes give the box's room back on the next `settle` — what every command ends with —
    /// instead of leaving a box sized for the formulas the sheet had.
    #[test]
    fn a_box_gives_its_room_back_once_deletes_leave_it_mostly_empty() {
        let mut g = Graph::default();
        for row in 0..20_000u32 {
            g.insert(key(0, row, 6), &[single(0, row, 1)], None, Node::new(false, false, false));
        }
        g.settle();
        assert!(g.has_dense_index(0), "20,000 precedents fill a box");
        let before = g.memory().cell_deps;
        for row in 10..20_000u32 {
            g.remove(key(0, row, 6));
        }
        g.settle();
        let after = g.memory().cell_deps;
        assert!(after.saturating_mul(8) < before, "the box shrank: {after} bytes against {before}");
        // The ten dependents left are still answered, and the cleared precedents are not.
        let mut out = Vec::new();
        for row in 0..10u32 {
            out.clear();
            g.dependents(0, CellRef::new(row, 1), &mut out);
            assert_eq!(out, vec![key(0, row, 6)], "row {row}");
        }
        out.clear();
        g.dependents(0, CellRef::new(5_000, 1), &mut out);
        assert!(out.is_empty(), "a cleared precedent answers nobody: {out:?}");
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

    /// The allocations the memory rounds shrank, in the arithmetic the memory report makes: a
    /// node's flags moved into its run's word, an area's sheet packs into a `u32` (24 to 20), S7
    /// deleted the node's own tree pointer (24 to 16 — the reference workbook's two million nodes,
    /// 172 MB to 138), and GR3 dropped the sheet from the table's key (33 to 25 bytes a slot —
    /// 138 MB to 105).
    #[test]
    fn a_node_is_two_words_and_a_packed_area_five() {
        assert_eq!(std::mem::size_of::<Node>(), 16, "a `u32` start, the run/flag word, the shape");
        assert_eq!(std::mem::size_of::<(CellRef, Node)>() + 1, 25, "the node table's bytes a slot");
        assert_eq!(std::mem::size_of::<(Key, Node)>() + 1, 33, "the key the per-sheet table dropped");
        assert_eq!(std::mem::size_of::<PackedArea>(), 20, "a `u32` sheet and a 16-byte range");
        assert_eq!(std::mem::size_of::<Area>(), 24, "the evaluator's form, which the arena does not pay for");
    }

    /// The node table is the per-sheet `CellRef → Node` one, and a formula costs it 25 bytes a
    /// slot — the ratchet, so a later change cannot quietly re-widen the key back to a `Key` and
    /// buy the 8 bytes a formula back: 10,000 formulas over 16,384 slots (the doubling's tail) come
    /// out at 25 bytes a slot, not 33.
    ///
    /// The table doubles, so a slot count and a formula count are not the same number: 10,000
    /// formulas fill 16,384 slots, and the report charges what is allocated (41 bytes a formula
    /// here, 52 at the reference workbook's two million). The 26 is the entry's own arithmetic —
    /// 8 + 16 + 1 — with a byte of slack.
    #[test]
    fn the_node_table_costs_at_most_twenty_six_bytes_a_slot() {
        let mut g = Graph::default();
        for row in 0..10_000u32 {
            g.insert(key(0, row, 6), &[single(0, row, 1)], None, Node::new(false, false, false));
        }
        let m = g.memory();
        let slots = g.node_slots();
        assert_eq!(slots, 16_392, "16,384 slots for 10,000 formulas, 8 for the sheet that holds them");
        assert!(m.nodes <= slots * 26, "the node table costs {} bytes for {slots} slots", m.nodes);
        assert!(m.nodes >= slots * 25, "…and no less than the entry: {} bytes for {slots} slots", m.nodes);
        // The table this replaced: the same slots, a 16-byte key padded around the 16-byte node.
        let replaced = slots.saturating_mul(33);
        assert!(m.nodes < replaced, "the shape it replaced cost {replaced} bytes here, this one {}", m.nodes);
    }

    /// A sheet's table is the sheet's: a formula on one sheet does not put a table on another, a
    /// sheet that loses its last formula gives its table back, and a sheet that holds none is never
    /// charged for one — the outer map is one entry a sheet that has formulas, not one a sheet.
    #[test]
    fn a_sheet_of_values_takes_no_node_table() {
        let mut g = Graph::default();
        g.insert(key(0, 0, 6), &[single(0, 0, 1)], None, Node::new(false, false, false));
        g.insert(key(1, 0, 6), &[single(1, 0, 1)], None, Node::new(false, false, false));
        assert_eq!(g.nodes.sheets.len(), 2, "one table a sheet with a formula");
        assert_eq!(g.len(), 2);
        assert!(g.has_node(key(1, 0, 6)));
        assert!(!g.has_node(key(2, 0, 6)), "a sheet the graph never saw");
        assert_eq!(g.node_keys().count(), 2);
        let two = g.memory().nodes;
        g.remove(key(1, 0, 6));
        assert_eq!(g.nodes.sheets.len(), 1, "the sheet's table left with its last formula");
        assert!(g.memory().nodes < two, "and so did its bytes");
        assert_eq!(g.node_keys().collect::<Vec<_>>(), vec![key(0, 0, 6)]);
        g.remove(key(0, 0, 6));
        assert!(g.is_empty());
        assert_eq!(g.nodes.sheets.len(), 0, "the last table leaves too");
    }

    /// A reserve is a reservation, not a table: a sheet reserved for formulas that never arrive —
    /// a rebuild of a sheet whose text does not parse — holds none once the graph settles.
    #[test]
    fn a_reserve_with_no_formulas_leaves_no_table() {
        let mut g = Graph::default();
        g.reserve(0, 5_000);
        assert!(g.memory().nodes > 0, "the room is taken");
        g.settle();
        assert_eq!(g.nodes.sheets.len(), 0, "and given back");
        assert_eq!(g.memory().nodes, 0);
        g.reserve(0, 0);
        assert_eq!(g.nodes.sheets.len(), 0, "reserving nothing takes no table");
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
            let stored = g.node(k);
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
            let Some(node) = g.node(key(0, row, 6)) else { continue };
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
