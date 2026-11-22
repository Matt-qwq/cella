//! Copy-on-write sparse cell storage, packed.
//!
//! Cells live in bands of 64 rows, each band behind an `Arc`, so cloning a store (an undo
//! snapshot) copies only the band pointers and editing a cell copies just its band.
//!
//! Inside a band the cells lie in four stripes — a presence bitmap over the columns a row touches,
//! then one entry per stored cell in three parallel arrays (kind, payload, style) — plus side
//! tables for the two things a cell only rarely carries (a formula and an array value). A `Cell`
//! struct costs 40 bytes; packed like this a cell costs about 11, which is the difference between
//! a 30-million-cell sheet taking 1.6 GB and taking 400 MB.
//!
//! The stripes are band-wide vectors with a slice per row, not per-row vectors. A row that owned
//! its four arrays put four allocations behind every pointer, so reading a million single-cell
//! rows chased four million of them at ~46 ns a cell against ~2 ns for the arithmetic, and the row
//! header cost 108 bytes. Walking a stripe is sequential, the header is 48 bytes, and a page of a
//! band is one copy instead of a thousand.
//!
//! Text is the one payload that does not fit in eight bytes, so a band keeps a table of the strings
//! its rows use and the payload holds an index into it. Strings that arrive as the same `Arc` (as a
//! file's shared strings do — one `Arc` shared by every cell that mentions it) are found by
//! pointer, so deduplicating them costs a hash of a pointer, not a comparison of strings.
//!
//! Numbers are handled the same way, and it pays more: a report repeats its figures — counted on
//! one of 31.7 million cells, only 6% of the numbers in a band of 64 rows are distinct. A band
//! therefore keeps the distinct numbers it saw and a cell's payload is a two-byte index into that
//! table, so the eight bytes a number costs are paid once per distinct figure instead of per cell.
//! A band that overflows the index (more than 65,535 distinct numbers in 64 rows) falls back to
//! `NUMBER_WIDE`, which keeps the bits in the row's side table.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use cella_core::{CellError, CellRef, RangeRef, Value};
use serde::{Deserialize, Serialize};

use crate::cell::{Cell, Formula};
use crate::style::StyleId;

/// How many column runs a packed walk follows before it builds cells through `iter_range`
/// instead. Two or three runs is an ordinary insert-and-delete history; past this, repeating the
/// per-row walk once per run would cost more than one `Cell` per cell.
const MAX_PACKED_COLUMN_SEGMENTS: usize = 8;

/// Rows per band: one undo snapshot shares bands this size, so an edit copies at most 64 rows.
///
/// Public because a writer that walks the JSON spelling of the store (the band key is
/// `row / BAND`) needs the same arithmetic the serializer uses.
pub const BAND: u32 = 64;

/// A value-only write this large is split across threads; below it the thread start costs more than
/// the work. Above the ceiling the clone of every touched band would double too much of the model.
#[cfg(not(target_arch = "wasm32"))]
const PARALLEL_WRITE_MIN: usize = 16_384;
#[cfg(not(target_arch = "wasm32"))]
const PARALLEL_WRITE_MAX_CELLS: usize = 4_000_000;

/// How many bands one chunk of a spill encodes before the pages are written and the bands dropped:
/// the unit of work handed to the threads, and the bound on what a spill buffers at once (a
/// thousand bands is about five megabytes of pages, however wide the sheet).
const SPILL_CHUNK: usize = 1024;
/// A spill of at least this many bands is spread over the cores; a step's handful of bands is
/// encoded on the calling thread, where starting threads would cost more than it saves.
const SPILL_PARALLEL_MIN: usize = 256;
/// Threads a spill may use: the same ceiling the parallel value write takes, and the sort of fan-out
/// a machine's cores can hold (see [`PARALLEL_WRITE_MIN`]). wasm has no threads to spread over.
#[cfg(not(target_arch = "wasm32"))]
const SPILL_MAX_THREADS: usize = 8;

/// How many bytes of decoded bands the page cache keeps by default. Enough for a viewport and a
/// scrolling burst at the export's density, without letting a full-sheet scan grow the working set
/// back to the live model.
const PAGE_CACHE_BYTES: usize = 64 * 1024 * 1024;
/// The bounds [`enable_paging`](CellStore::enable_paging) applies to `CELLA_PAGE_CACHE_MB`.
/// Only the non-wasm environment reader uses them; wasm keeps the default.
#[cfg(not(target_arch = "wasm32"))]
const PAGE_CACHE_MIN_MB: usize = 8;
#[cfg(not(target_arch = "wasm32"))]
const PAGE_CACHE_MAX_MB: usize = 512;

/// How many rewritten pages a compaction pass keeps decoded. Genuine orphans are a handful per
/// edit, and the read that follows a save is likely to want them; a bigger pass (or one that a
/// conservative column-gap test over-selected) leaves the page undecoded for the cache to load
/// once, so a rewrite cannot pin a band per page.
const PAGE_PRIME_LIMIT: usize = 8;

/// What a payload holds, one byte per stored cell, in column order within the row.
const NUMBER: u8 = 0;
const TEXT: u8 = 1;
const BOOL: u8 = 2;
const ERROR: u8 = 3;
/// No value: a styled cell, or a formula without a cached result.
const EMPTY: u8 = 4;
/// An array value, held in the row's `arrays` side table.
const ARRAY: u8 = 5;
/// A number whose band holds more distinct values than a `u16` can index; the payload points into
/// the row's `wide` table, which holds the bits.
const NUMBER_WIDE: u8 = 6;
/// The same for a text.
const TEXT_WIDE: u8 = 7;

/// A payload that fits the row's `u16` array, or one that has to go to the row's side table.
#[derive(Clone, Copy, Debug)]
enum Payload {
    Small(u16),
    Wide(u64),
}

const ERRORS: [CellError; 11] = [
    CellError::Null,
    CellError::Div0,
    CellError::Value,
    CellError::Ref,
    CellError::Name,
    CellError::Num,
    CellError::NA,
    CellError::GettingData,
    CellError::Spill,
    CellError::Calc,
    CellError::Circ,
];

fn error_code(e: CellError) -> u64 {
    match e {
        CellError::Null => 0,
        CellError::Div0 => 1,
        CellError::Value => 2,
        CellError::Ref => 3,
        CellError::Name => 4,
        CellError::Num => 5,
        CellError::NA => 6,
        CellError::GettingData => 7,
        CellError::Spill => 8,
        CellError::Calc => 9,
        CellError::Circ => 10,
    }
}

// The kinds and the styles of a band are runs of equal values over consecutive stored cells:
// `(value, length)` pairs in cell order. A report formats a row in two or three runs — a date
// column, a block of numbers, a block of text — and its cells hold kinds in runs the same way, so a
// per-cell array is mostly repetition: two bytes a cell for the style, one for the kind. An entry
// costs four bytes and covers a run, which measured on a 31.7M-cell report is 0.04 style runs and
// 0.18 kind runs a cell. The worst case, every cell its own run, costs a third more than the
// per-cell array.
//
// The runs live in the band's stripe, one slice per row (`at..at + len`), so the helpers below take
// the slice they work on. A row's slice is its own: two neighbours that happen to hold the same
// value keep two runs, because a row is what gets moved, paged and rebuilt.

/// The values of a run slice, expanded: how many cells it covers.
#[cfg(test)]
fn runs_len(runs: &[(u16, u16)]) -> usize {
    runs.iter().map(|(_, n)| *n as usize).sum()
}

/// The value of the `i`th cell of a run slice.
fn runs_at(runs: &[(u16, u16)], i: usize) -> u16 {
    runs_at_or(runs, i, 0)
}

fn runs_at_or(runs: &[(u16, u16)], i: usize, default: u16) -> u16 {
    let mut left = i;
    for (v, n) in runs {
        let n = *n as usize;
        if left < n {
            return *v;
        }
        left -= n;
    }
    default
}

/// Appends a value at the end of the run slice that starts at `base`: it lengthens the last run
/// when that run already holds the value, and never reaches into the run of the row before.
fn run_push(runs: &mut Vec<(u16, u16)>, base: usize, value: u16) {
    let extends = runs.len() > base && matches!(runs.last(), Some((last, n)) if *last == value && *n < u16::MAX);
    if extends {
        if let Some(last) = runs.last_mut() {
            last.1 += 1;
        }
    } else {
        runs.push((value, 1));
    }
}

/// Merges run `idx` with its neighbours when they hold the same value, never across the slice
/// `at..at + slice`, and returns the slice length after the runs that went away.
fn runs_merge(runs: &mut Vec<(u16, u16)>, at: usize, slice: usize, idx: usize) -> usize {
    let mut slice = slice;
    for start in [idx, idx.saturating_sub(1)] {
        let j = start;
        while j >= at && j + 1 < at.saturating_add(slice) && runs.get(j).zip(runs.get(j + 1)).is_some_and(|(a, b)| a.0 == b.0) {
            let n = runs[j + 1].1;
            runs[j].1 = runs[j].1.saturating_add(n);
            runs.remove(j + 1);
            slice = slice.saturating_sub(1);
        }
    }
    slice
}

/// Inserts `value` at cell `i` of the run slice `at..at + len`, and returns the slice's new length.
fn runs_insert(runs: &mut Vec<(u16, u16)>, at: usize, len: usize, i: usize, value: u16) -> usize {
    let mut slice = len;
    let mut pos = 0usize;
    let mut idx = at;
    while idx < at + slice {
        let (rv, rn) = match runs.get(idx) {
            Some(&run) => run,
            None => break,
        };
        let end = pos + rn as usize;
        if i > end {
            pos = end;
            idx += 1;
            continue;
        }
        // Extending a neighbouring run that already holds the value is just a longer run.
        if rv == value {
            runs[idx].1 += 1;
            return slice;
        }
        if i == pos {
            runs.insert(idx, (value, 1));
            slice += 1;
            return runs_merge(runs, at, slice, idx);
        }
        let before = (i - pos) as u16;
        runs[idx].1 = before;
        runs.insert(idx + 1, (value, 1));
        slice += 1;
        if rn > before {
            runs.insert(idx + 2, (rv, rn - before));
            slice += 1;
        }
        return runs_merge(runs, at, slice, idx + 1);
    }
    // Past the slice's last run: the new cell is the slice's last one.
    runs.insert(at + slice, (value, 1));
    slice += 1;
    if slice > 1 {
        return runs_merge(runs, at, slice, at + slice - 2);
    }
    slice
}

/// Removes cell `i` from the run slice `at..at + len`, and returns the slice's new length.
fn runs_remove(runs: &mut Vec<(u16, u16)>, at: usize, len: usize, i: usize) -> usize {
    let mut slice = len;
    let mut pos = 0usize;
    let mut idx = at;
    while idx < at + slice {
        let rn = match runs.get(idx) {
            Some(&(_, n)) => n as usize,
            None => break,
        };
        if i < pos + rn {
            runs[idx].1 -= 1;
            if runs[idx].1 == 0 {
                runs.remove(idx);
                slice -= 1;
                if idx > at && slice > 0 {
                    return runs_merge(runs, at, slice, idx - 1);
                }
            } else {
                return runs_merge(runs, at, slice, idx);
            }
            return slice;
        }
        pos += rn;
        idx += 1;
    }
    slice
}

/// Sets cell `i` of the run slice to `value`, and returns the slice's new length.
fn runs_set(runs: &mut Vec<(u16, u16)>, at: usize, len: usize, i: usize, value: u16) -> usize {
    if runs_at(runs.get(at..at.saturating_add(len)).unwrap_or(&[]), i) == value {
        return len;
    }
    let len = runs_insert(runs, at, len, i, value);
    // `runs_insert` put the new cell in its own run: drop the old one at i + 1.
    runs_remove(runs, at, len, i + 1)
}

/// Walks one row's run slice while its cells are visited in order. A per-cell search from the
/// row's first run would be quadratic in the runs a row holds, which is what a walk over a whole
/// band pays.
struct RunCursor<'a> {
    runs: &'a [(u16, u16)],
    idx: usize,
    value: u16,
    /// First cell of the next run.
    end: usize,
    default: u16,
}

impl<'a> RunCursor<'a> {
    fn new(runs: &'a [(u16, u16)], default: u16) -> Self {
        let (value, end) = runs.first().map_or((default, 0), |(v, n)| (*v, *n as usize));
        RunCursor { runs, idx: 0, value, end, default }
    }

    /// The value at cell `i`. Cells must be asked for in order (`i` never goes back).
    fn at(&mut self, i: usize) -> u16 {
        while i >= self.end {
            self.idx += 1;
            match self.runs.get(self.idx) {
                Some(&(v, n)) => {
                    self.value = v;
                    self.end += n as usize;
                }
                None => return self.default,
            }
        }
        self.value
    }
}

/// `v[at..at + n]`, empty when the range runs off the end: a stripe's offsets and its rows are
/// built together, so this only guards against a bug becoming a panic.
fn slice<T>(v: &[T], at: u32, n: u32) -> &[T] {
    v.get(at as usize..(at as usize).saturating_add(n as usize)).unwrap_or(&[])
}

fn insert_at<T>(v: &mut Vec<T>, at: usize, x: T) {
    let at = at.min(v.len());
    v.insert(at, x);
}

fn drop_at<T>(v: &mut Vec<T>, at: usize) {
    if at < v.len() {
        v.remove(at);
    }
}

fn drop_slice<T>(v: &mut Vec<T>, at: usize, n: usize) {
    let lo = at.min(v.len());
    let hi = at.saturating_add(n).min(v.len());
    v.drain(lo..hi);
}

/// What a row carries beside its cells: its formulas, and the two things almost no row has.
///
/// The formula vector is sized exactly (`shrink_to_fit` after an edit): a million rows of two
/// formulas used to hold a doubling's spare capacity — four slots for two entries, 32 MB of
/// nothing at a million rows — because `Vec` doubles rather than fitting what it holds.
#[derive(Clone, Debug, Default)]
struct RowExtras {
    /// `(column, formula)`, sorted by column.
    formulas: Vec<(u32, Arc<Formula>)>,
    /// Allocated only by a row that holds an array value or a wide payload table.
    rare: Option<Box<RareRow>>,
}

/// The parts of a row's side table that almost no row needs, kept out of line so a row of formulas
/// pays for its formulas alone.
#[derive(Clone, Debug, Default)]
struct RareRow {
    /// `(column, array value)`, sorted by column.
    arrays: Vec<(u32, Arc<cella_core::Array>)>,
    /// Payloads too wide for the `u16` array, in the order their cells reached for them. Only a
    /// band with more than 65,535 distinct numbers ever has one.
    wide: Vec<u64>,
}

impl RowExtras {
    fn arrays(&self) -> &[(u32, Arc<cella_core::Array>)] {
        self.rare.as_deref().map_or(&[], |r| r.arrays.as_slice())
    }

    fn wide(&self) -> &[u64] {
        self.rare.as_deref().map_or(&[], |r| r.wide.as_slice())
    }

    fn rare_mut(&mut self) -> &mut RareRow {
        self.rare.get_or_insert_with(Default::default)
    }

    fn is_empty(&self) -> bool {
        self.formulas.is_empty() && self.rare.as_ref().is_none_or(|r| r.arrays.is_empty() && r.wide.is_empty())
    }
}

/// One row of a band: the row number, and where that row's cells sit in the band's four stripes.
///
/// This used to own the bitmap and the three cell arrays. It is an index now, so the row header is
/// 48 bytes instead of 108 and the cells of one band follow each other in memory.
#[derive(Clone, Debug, Default)]
struct BandRow {
    /// Row within the band.
    key: u32,
    /// Column of bit 0 of the bitmap, always a multiple of 64 so the bitmap grows by whole words.
    first: u32,
    /// First bitmap word in `Band::bits`, and how many words this row spans.
    bits_at: u32,
    words: u32,
    /// First cell in `Band::payloads`, and in the `Band::kinds` and `Band::styles` run stripes.
    cells_at: u32,
    kinds_at: u32,
    styles_at: u32,
    /// How many stored cells this row holds, and how many runs each stripe gives them.
    cells: u32,
    kind_runs: u32,
    style_runs: u32,
    /// Held out of line and shared: a band copied for a value edit (an undo snapshot keeps the
    /// old one) shares the formulas and arrays of every row that did not change, instead of
    /// copying one small vector per row.
    extras: Option<Arc<RowExtras>>,
}

impl BandRow {
    fn is_empty(&self) -> bool {
        self.cells == 0 && self.extras.is_none()
    }

    fn bits<'a>(&self, band: &'a Band) -> &'a [u64] {
        slice(&band.bits, self.bits_at, self.words)
    }

    fn kind_runs<'a>(&self, band: &'a Band) -> &'a [(u16, u16)] {
        slice(&band.kinds, self.kinds_at, self.kind_runs)
    }

    fn style_runs<'a>(&self, band: &'a Band) -> &'a [(u16, u16)] {
        slice(&band.styles, self.styles_at, self.style_runs)
    }

    /// The kind of the `i`th stored cell.
    fn kind_at(&self, band: &Band, i: usize) -> u8 {
        runs_at_or(self.kind_runs(band), i, u16::from(EMPTY)) as u8
    }

    /// The style of the `i`th stored cell.
    fn style_at(&self, band: &Band, i: usize) -> u16 {
        runs_at(self.style_runs(band), i)
    }

    /// The payload of the `i`th stored cell.
    fn payload_at(&self, band: &Band, i: usize) -> u16 {
        band.payloads.get(self.cells_at as usize + i).copied().unwrap_or(0)
    }

    fn wide_at(&self, band: &Band, i: usize) -> u64 {
        let slot = self.payload_at(band, i) as usize;
        self.extras.as_ref().map_or(0, |e| e.wide().get(slot).copied().unwrap_or(0))
    }

    fn formulas(&self) -> &[(u32, Arc<Formula>)] {
        self.extras.as_ref().map_or(&[], |e| e.formulas.as_slice())
    }

    fn arrays(&self) -> &[(u32, Arc<cella_core::Array>)] {
        self.extras.as_ref().map_or(&[], |e| e.arrays())
    }

    /// Whether two versions of a row share its formula side table, which an edit that did not
    /// write the row leaves in place.
    fn same_extras(a: &BandRow, b: &BandRow) -> bool {
        match (&a.extras, &b.extras) {
            (Some(x), Some(y)) => Arc::ptr_eq(x, y),
            (None, None) => true,
            _ => false,
        }
    }

    fn drop_unused_extras(&mut self) {
        if self.extras.as_ref().is_some_and(|e| e.is_empty()) {
            self.extras = None;
        }
    }

    /// Number of stored cells before `bit` of `word`.
    fn rank(&self, band: &Band, word: usize, bit: u32) -> usize {
        let bits = self.bits(band);
        let mut n = 0usize;
        for w in bits.get(..word).unwrap_or(&[]) {
            n += w.count_ones() as usize;
        }
        let w = bits.get(word).copied().unwrap_or(0);
        n + (w & ((1u64 << bit) - 1)).count_ones() as usize
    }

    /// Number of stored cells strictly before `col`.
    fn rank_before(&self, band: &Band, col: u32) -> usize {
        let Some(slot) = col.checked_sub(self.first) else { return 0 };
        let word = (slot / 64) as usize;
        if word >= self.words as usize {
            return self.cells as usize;
        }
        self.rank(band, word, slot % 64)
    }

    /// Index into the dense arrays for `col`, when the row stores a cell there.
    fn index_of(&self, band: &Band, col: u32) -> Option<usize> {
        let slot = col.checked_sub(self.first)?;
        let word = (slot / 64) as usize;
        let bit = slot % 64;
        let w = *self.bits(band).get(word)?;
        (w & (1u64 << bit) != 0).then(|| self.rank(band, word, bit))
    }

    /// The column of the `i`th stored cell.
    fn col_at(&self, band: &Band, i: usize) -> u32 {
        let mut seen = 0usize;
        for (word, bits) in self.bits(band).iter().enumerate() {
            let count = bits.count_ones() as usize;
            if seen + count <= i {
                seen += count;
                continue;
            }
            let mut left = i - seen;
            let mut w = *bits;
            while w != 0 {
                let bit = w.trailing_zeros();
                if left == 0 {
                    return self.first + word as u32 * 64 + bit;
                }
                left -= 1;
                w &= w - 1;
            }
        }
        self.first
    }

    fn columns(&self) -> u32 {
        self.words * 64
    }

    /// First and last stored column.
    fn column_span(&self, band: &Band) -> Option<(u32, u32)> {
        let mut first = None;
        let mut last = None;
        for (i, w) in self.bits(band).iter().enumerate() {
            if *w == 0 {
                continue;
            }
            if first.is_none() {
                first = Some(self.first + i as u32 * 64 + w.trailing_zeros());
            }
            last = Some(self.first + i as u32 * 64 + 63 - w.leading_zeros());
        }
        first.zip(last)
    }

    fn formula_at(&self, col: u32) -> Option<&Arc<Formula>> {
        let formulas = self.formulas();
        formulas.binary_search_by_key(&col, |(c, _)| *c).ok().map(|i| &formulas[i].1)
    }

    fn set_formula(&mut self, col: u32, formula: Option<&Arc<Formula>>) {
        match formula {
            Some(f) => {
                let e = Arc::make_mut(self.extras.get_or_insert_with(Default::default));
                match e.formulas.binary_search_by_key(&col, |(c, _)| *c) {
                    Ok(i) => e.formulas[i].1 = f.clone(),
                    Err(i) => {
                        e.formulas.insert(i, (col, f.clone()));
                        e.formulas.shrink_to_fit();
                    }
                }
            }
            None => {
                if let Some(e) = self.extras.as_mut()
                    && let Ok(i) = e.formulas.binary_search_by_key(&col, |(c, _)| *c)
                {
                    let e = Arc::make_mut(e);
                    e.formulas.remove(i);
                    e.formulas.shrink_to_fit();
                }
                self.drop_unused_extras();
            }
        }
    }

    fn set_array(&mut self, col: u32, array: Option<&Arc<cella_core::Array>>) {
        match array {
            Some(a) => {
                let e = Arc::make_mut(self.extras.get_or_insert_with(Default::default)).rare_mut();
                match e.arrays.binary_search_by_key(&col, |(c, _)| *c) {
                    Ok(i) => e.arrays[i].1 = a.clone(),
                    Err(i) => e.arrays.insert(i, (col, a.clone())),
                }
            }
            None => {
                let held = self.extras.as_ref().is_some_and(|e| e.arrays().binary_search_by_key(&col, |(c, _)| *c).is_ok());
                if held && let Some(e) = self.extras.as_mut() {
                    let r = Arc::make_mut(e).rare_mut();
                    if let Ok(i) = r.arrays.binary_search_by_key(&col, |(c, _)| *c) {
                        r.arrays.remove(i);
                    }
                }
                self.drop_unused_extras();
            }
        }
    }
}

/// What a band remembers about the band it was copied from, so pricing a snapshot does not have to
/// walk every row of the bands an edit copied for their values alone.
///
/// A band is immutable while any other owner holds it: `Arc::make_mut` copies it, and the copy
/// starts as an exact copy of its parent. A value-only edit — what a recalculation writes — changes
/// payloads, kinds and the number table but not one row, and every row's formula/array side table
/// stays the parent's, pointer for pointer. `retained_bytes_against` reads that from the tag: when
/// the newer store's band is an untouched copy of the older store's band, the rows add nothing to
/// what the snapshot retains, and the price is the fixed part alone — no per-row walk.
///
/// `rows_touched` is set by every mutation that can add, drop or replace a row or its side table.
/// It is a plain store, not an atomic, because it is written on the edit path; identities come from
/// a process counter, so two bands that did not copy from one another never compare equal.
#[derive(Clone, Copy, Debug)]
struct BandTag {
    /// Identity of this band object. Every band constructed takes a fresh one, so a parent's id
    /// names that band alone.
    id: u64,
    /// `id` of the band this one was copied from; 0 for a band built from cells.
    parent: u64,
    /// A row was added, dropped, or had its side table replaced since this object was copied.
    rows_touched: bool,
}

impl Default for BandTag {
    /// A constructed band's identity: fresh, with no parent and no touched rows.
    fn default() -> BandTag {
        BandTag { id: next_band_id(), parent: 0, rows_touched: false }
    }
}

/// A process-unique band identity (see [`BandTag`]): 64-bit so it never repeats in a session.
fn next_band_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// The cells of one band, striped: the presence bitmaps, kinds, payloads and styles all live in one
/// vector each, a row being a slice of all four.
#[derive(Debug, Default)]
struct Band {
    /// Sorted by row: 64 rows of 48 bytes each, where a `BTreeMap`'s leaves sit three quarters full
    /// and cost some forty bytes a row for the privilege.
    rows: Vec<BandRow>,
    /// Which of the 64 row keys has a row, bit `key`. Finding a row used to be a binary search over
    /// `rows`: six probes, each into a different 48-byte header, which is the largest single cost
    /// of a cell read (measured 15 of its 60 ns). A band is at most 64 rows, so one word holds the
    /// answer and `popcount` of the bits below it is the index — kept in step with `rows` by the
    /// three places that change it, and derived after a decode.
    row_bits: u64,
    /// The presence bitmaps, 64 columns a word, one slice per row.
    bits: Vec<u64>,
    /// The kind of each stored cell, in runs; one slice per row.
    kinds: Vec<(u16, u16)>,
    /// The style of each stored cell, in runs; one slice per row.
    styles: Vec<(u16, u16)>,
    /// A `u16` per stored cell, in the same order as the kinds and styles stripes: an index into
    /// the band's number or text table, a boolean, an error code — or, for the rare wide kinds, an
    /// index into the row's `extras.wide`.
    payloads: Vec<u16>,
    /// The distinct numbers of this band, in the order they were seen; a `NUMBER` payload is an
    /// index into it.
    numbers: Vec<f64>,
    /// Open addressing over `numbers`: each slot holds an index plus one, zero meaning free.
    number_slots: Vec<u16>,
    texts: Vec<Arc<str>>,
    /// Pointer of each kept text → its slot, sorted by pointer, so one shared `Arc` is stored once
    /// per band. A band holds a handful of texts: a sorted vector beats a hash map's bookkeeping at
    /// that size, which is why the number table (hundreds of entries) is a hash and this is not.
    text_index: Vec<(usize, u32)>,
    /// The distinct styles of this band; the `styles` stripe holds indexes into it.
    style_ids: Vec<StyleId>,
    /// Who this band was copied from, for pricing without a row walk (see [`BandTag`]).
    tag: BandTag,
}

impl Clone for Band {
    /// A copy holds exactly this band, with its own identity and this band as its parent, and no
    /// row touched yet — which is what lets pricing answer from the tag (see [`BandTag`]).
    fn clone(&self) -> Band {
        Band {
            rows: self.rows.clone(),
            row_bits: self.row_bits,
            bits: self.bits.clone(),
            kinds: self.kinds.clone(),
            styles: self.styles.clone(),
            payloads: self.payloads.clone(),
            numbers: self.numbers.clone(),
            number_slots: self.number_slots.clone(),
            texts: self.texts.clone(),
            text_index: self.text_index.clone(),
            style_ids: self.style_ids.clone(),
            tag: BandTag { id: next_band_id(), parent: self.tag.id, rows_touched: false },
        }
    }
}

/// Whether two versions of one formula are the same formula: its text, its array range and the
/// file's dynamic-array mark, which is [`Formula`]'s `PartialEq` rule. The tree and the shape are
/// caches of (text, position), so a copy that was re-parsed or re-interned is the same formula.
/// Kept as the pointer fast path of that rule.
fn same_formula(a: &Arc<Formula>, b: &Arc<Formula>) -> bool {
    Arc::ptr_eq(a, b) || a == b
}

/// The bits a number is stored under. The tables keep one entry for zero, so `-0.0` — which
/// equals `0.0` — is stored as `0.0`; a compare-then-write has to use the same rule.
fn number_bits(n: f64) -> u64 {
    if n == 0.0 { 0f64.to_bits() } else { n.to_bits() }
}

/// Mixes the bits of a double into a slot index.
fn number_hash(bits: u64) -> usize {
    let mut h = bits.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h ^= h >> 31;
    h as usize
}

/// The live heap bytes of one band, by allocation class: the breakdown
/// [`Band::memory`] returns and [`Band::heap_bytes`] sums.
///
/// One allocation is counted once: a text, a row's extras table or a parsed formula tree that
/// several rows (or a page cache entry) share through `Arc` is charged its control block here, and
/// its contents by whoever holds the last reference — the `Arc::strong_count` convention the page
/// cache has always used. The band's own `Arc` control block is not counted (the store's index
/// charges the pointer that names it).
///
/// Not counted, deliberately: allocator slack beyond a `Vec`'s capacity, and the values inside an
/// `Arc<Array>` (see [`Band::memory`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct BandMemory {
    /// The `Band` struct itself and its row index (`rows`, a `BandRow` each).
    header: usize,
    /// The presence bitmaps.
    bitmaps: usize,
    /// The payload stripe.
    payloads: usize,
    /// The kind stripe.
    kinds: usize,
    /// The style stripe.
    styles: usize,
    /// The band's `StyleId` dictionary.
    style_ids: usize,
    /// The number dictionary and its open-addressing slots.
    numbers: usize,
    /// The text dictionary: its two vectors, and each text's characters or its control block.
    texts: usize,
    /// The row side tables: the `RowExtras` structs and their three vectors.
    row_extras: usize,
    /// A formula's `Formula` struct and its text.
    formula_text: usize,
    /// The parsed expression trees of the band's formulas.
    formula_exprs: usize,
    /// Array values: their struct (their values are not walked).
    arrays: usize,
    /// Payloads in a row's overflow vector.
    wide: usize,
}

impl BandMemory {
    fn total(&self) -> usize {
        self.header
            .saturating_add(self.bitmaps)
            .saturating_add(self.payloads)
            .saturating_add(self.kinds)
            .saturating_add(self.styles)
            .saturating_add(self.style_ids)
            .saturating_add(self.numbers)
            .saturating_add(self.texts)
            .saturating_add(self.row_extras)
            .saturating_add(self.formula_text)
            .saturating_add(self.formula_exprs)
            .saturating_add(self.arrays)
            .saturating_add(self.wide)
    }
}

impl Band {
    /// Heap bytes of the band's header, its stripes and its number table: everything a band owns
    /// before the rows' headers, their extras and the text contents.
    fn stripe_bytes(&self) -> usize {
        use std::mem::size_of;
        // The sharing tag is bookkeeping, not storage a snapshot is charged for: the price of a
        // band must not move because the model remembers who copied it.
        size_of::<Band>().saturating_sub(size_of::<BandTag>())
            + self.bits.capacity() * size_of::<u64>()
            + self.kinds.capacity() * size_of::<(u16, u16)>()
            + self.payloads.capacity() * size_of::<u16>()
            + self.styles.capacity() * size_of::<(u16, u16)>()
            + self.numbers.capacity() * size_of::<f64>()
            + self.number_slots.capacity() * size_of::<u16>()
    }

    /// Whether any formula of this band owns a legacy array covering more than one cell.
    fn has_array_anchor(&self) -> bool {
        self.rows.iter().any(|row| row.formulas().iter().any(|(_, f)| f.array.as_deref().is_some_and(|r| !r.is_single())))
    }

    /// What one row's formulas and array values cost beyond the row header: the side table's run
    /// of entries, the text each formula spells, and the shape it keeps **when nothing else holds
    /// it**.
    ///
    /// A formula's tree is its shape's, charged once by whoever holds the last reference — the
    /// `Arc::strong_count` rule the band's report already uses for texts. Charging it per cell
    /// would make a million-row fill's undo entry look like a gigabyte (one shape, a million
    /// holders) and drop the whole history for a snapshot that keeps almost nothing alive.
    fn extras_bytes(row: &BandRow) -> usize {
        row.formulas().len() * (4 + std::mem::size_of::<Formula>())
            + row.arrays().len() * 8
            + row
                .formulas()
                .iter()
                .map(|(_, f)| {
                    let shape = f.shape.as_ref().filter(|s| Arc::strong_count(s) == 1).map_or(0, |s| s.bytes as usize);
                    formula_text_bytes(f).saturating_add(shape)
                })
                .sum::<usize>()
    }

    /// Heap bytes of one text: its characters plus the pointer that names them.
    #[allow(dead_code)]
    fn text_bytes(text: &Arc<str>) -> usize {
        text.len().saturating_add(std::mem::size_of::<Arc<str>>())
    }
}

/// Heap bytes a formula's text costs on top of `size_of::<Formula>()`: the characters, plus the
/// `String` header the box holds. A filled formula keeps none (R34); a text-only one pays what it
/// always did (the box itself is already inside `Formula`).
fn formula_text_bytes(f: &Formula) -> usize {
    f.text.as_ref().map_or(0, |t| t.len().saturating_add(std::mem::size_of::<String>()))
}

impl Band {
    /// Whether `next` is an untouched copy of this band, so both hold the same row keys with the
    /// same formula/array side tables, pointer for pointer (see [`BandTag`]). A value-only edit
    /// copies a band this way; nothing the copy did changed a row or a side table.
    fn shares_rows_with(&self, next: &Band) -> bool {
        !next.tag.rows_touched && next.tag.parent == self.tag.id
    }

    /// What this band costs a snapshot when every row it holds is also the newer band's: the
    /// stripes, the row headers and the texts — every term of [`unshared_band_bytes`] but the
    /// rows' side tables, which a shared row does not pay.
    fn unshared_fixed_bytes(&self) -> usize {
        let mut bytes = self.stripe_bytes().saturating_add(self.rows.len().saturating_mul(std::mem::size_of::<BandRow>()));
        for text in &self.texts {
            bytes = bytes.saturating_add(Self::text_bytes(text));
        }
        bytes
    }

    /// Heap bytes this band owns, by allocation class. O(rows + texts + formulas): every figure is
    /// a len/capacity/size_of product, and only the questions that depend on sharing (is this
    /// text, extras table, formula or tree held by someone else too?) need the walk.
    ///
    /// Where a row's extras table is also held by an undo snapshot's band, both stores charge the
    /// control block and neither the contents (neither holds the last reference); an edit that
    /// rewrites a row's formulas copies that row's table, so this is a floor for the rows an
    /// untouched value edit shares. See [`CellStore::memory`] for the whole contract.
    fn memory(&self) -> BandMemory {
        use std::mem::size_of;
        let mut m = BandMemory {
            header: size_of::<Band>().saturating_add(self.rows.capacity() * size_of::<BandRow>()),
            bitmaps: self.bits.capacity() * size_of::<u64>(),
            payloads: self.payloads.capacity() * size_of::<u16>(),
            kinds: self.kinds.capacity() * size_of::<(u16, u16)>(),
            styles: self.styles.capacity() * size_of::<(u16, u16)>(),
            style_ids: self.style_ids.capacity() * size_of::<StyleId>(),
            numbers: self.numbers.capacity() * size_of::<f64>() + self.number_slots.capacity() * size_of::<u16>(),
            texts: self.texts.capacity() * size_of::<Arc<str>>() + self.text_index.capacity() * size_of::<(usize, u32)>(),
            ..BandMemory::default()
        };
        // The dictionary holds each distinct `Arc<str>` once, so a text two cells share is charged
        // once. A text another band or a snapshot also holds is charged its control block: the
        // band holding the last reference charges the characters. Per band, not per store.
        for text in &self.texts {
            m.texts = m.texts.saturating_add(if Arc::strong_count(text) > 1 { size_of::<Arc<str>>() } else { Self::text_bytes(text) });
        }
        // Formulas and arrays are collected first and charged once per pointer: a paste, or a cell
        // written twice, can put the same `Arc<Formula>` (or the same shape) behind several rows.
        // Sorted by pointer rather than scanned, so a band holding a million formula cells cannot
        // go quadratic. The keys are addresses: compared, never dereferenced.
        let mut formulas: Vec<(usize, usize)> = Vec::new();
        let mut shapes: Vec<(usize, u32)> = Vec::new();
        let mut arrays: Vec<usize> = Vec::new();
        for row in &self.rows {
            let Some(extras) = &row.extras else { continue };
            m.row_extras = m.row_extras.saturating_add(size_of::<Arc<RowExtras>>());
            if Arc::strong_count(extras) > 1 {
                // Another owner (a snapshot's band) holds the table's contents.
                continue;
            }
            m.row_extras =
                m.row_extras.saturating_add(size_of::<RowExtras>()).saturating_add(extras.formulas.capacity() * size_of::<(u32, Arc<Formula>)>());
            if let Some(rare) = &extras.rare {
                m.row_extras = m
                    .row_extras
                    .saturating_add(size_of::<RareRow>())
                    .saturating_add(rare.arrays.capacity() * size_of::<(u32, Arc<cella_core::Array>)>());
                m.wide = m.wide.saturating_add(rare.wide.capacity() * size_of::<u64>());
            }
            for (_, f) in &extras.formulas {
                formulas.push((Arc::as_ptr(f) as usize, formula_text_bytes(f)));
                if let Some(s) = &f.shape {
                    shapes.push((Arc::as_ptr(s) as usize, s.bytes));
                }
            }
            for (_, a) in extras.arrays() {
                arrays.push(Arc::as_ptr(a) as usize);
            }
        }
        formulas.sort_unstable();
        formulas.dedup();
        for (_, text) in formulas {
            m.formula_text = m.formula_text.saturating_add(size_of::<Formula>()).saturating_add(text);
        }
        // A shape is one tree for every cell of a fill, so it is charged once per pointer: the
        // cells that share it pay the pointer, not the tree. Sorted by pointer and deduplicated,
        // so a band of a million filled cells stays O(n log n). (A cell that holds text alone and
        // no shape keeps no tree at all; the graph's node for it parses on demand — see
        // `CalcMemory::unshared`.)
        shapes.sort_unstable();
        shapes.dedup();
        for (_, bytes) in shapes {
            m.formula_exprs = m.formula_exprs.saturating_add(bytes as usize);
        }
        // An array's values are not walked: the same array is reachable from the sheet's spill map
        // and from the pass that produced it, and counting its values one by one would turn a
        // report that must stay O(bands) into a per-cell walk.
        arrays.sort_unstable();
        arrays.dedup();
        m.arrays = m.arrays.saturating_add(arrays.len() * size_of::<cella_core::Array>());
        m
    }

    /// Heap bytes this band owns, for the page cache's budget: the band's whole report, so the
    /// cache's charge and the memory report cannot disagree. A text or an extras table another
    /// owner also holds is charged as its control block, not as its contents: `decode_band` builds
    /// fresh strings, while a band primed from a live one shares them, and that memory already
    /// belongs to the model (or to an undo snapshot).
    fn heap_bytes(&self) -> usize {
        self.memory().total()
    }

    fn row_index(&self, key: u32) -> Option<usize> {
        if key < BAND {
            let bit = 1u64 << key;
            // The bit says whether the row is there; the ones below it say which row it is.
            return (self.row_bits & bit != 0).then(|| (self.row_bits & (bit - 1)).count_ones() as usize);
        }
        // A row key outside the band: only a decoded page could carry one, and the sorted search
        // still answers it (as missing, or as the row it names), never as a wrong row.
        self.rows.binary_search_by_key(&key, |r| r.key).ok()
    }

    /// The row bitmap a list of rows spells. Used where a band is built from bytes or checked.
    fn rows_bits(rows: &[BandRow]) -> u64 {
        rows.iter().fold(0u64, |bits, row| if row.key < BAND { bits | (1u64 << row.key) } else { bits })
    }

    fn row(&self, key: u32) -> Option<&BandRow> {
        self.row_index(key).map(|i| &self.rows[i])
    }

    /// The index of the row's entry, adding an empty row — its slices at the end of each stripe —
    /// when it isn't there yet.
    fn row_or_insert(&mut self, key: u32) -> usize {
        // A row that is there is one popcount away (a band is at most 64 rows); the sorted
        // search is only needed to find where a new row goes, which is the cold half.
        if let Some(i) = self.row_index(key) {
            return i;
        }
        match self.rows.binary_search_by_key(&key, |r| r.key) {
            Ok(i) => i,
            Err(i) => {
                let row = BandRow {
                    key,
                    bits_at: self.bits.len() as u32,
                    cells_at: self.payloads.len() as u32,
                    kinds_at: self.kinds.len() as u32,
                    styles_at: self.styles.len() as u32,
                    ..Default::default()
                };
                if row.key < BAND {
                    self.row_bits |= 1u64 << row.key;
                }
                self.rows.insert(i, row);
                self.tag.rows_touched = true;
                i
            }
        }
    }

    /// Drops a row together with the cells it holds, keeping every stripe contiguous.
    fn clear_row_at(&mut self, at: usize) {
        if at >= self.rows.len() {
            return;
        }
        self.tag.rows_touched = true;
        let row = self.rows.remove(at);
        if row.key < BAND {
            self.row_bits &= !(1u64 << row.key);
        }
        let (bits_at, words) = (row.bits_at as usize, row.words as usize);
        let (cells_at, cells) = (row.cells_at as usize, row.cells as usize);
        let (kinds_at, kinds) = (row.kinds_at as usize, row.kind_runs as usize);
        let (styles_at, styles) = (row.styles_at as usize, row.style_runs as usize);
        drop_slice(&mut self.bits, bits_at, words);
        drop_slice(&mut self.payloads, cells_at, cells);
        drop_slice(&mut self.kinds, kinds_at, kinds);
        drop_slice(&mut self.styles, styles_at, styles);
        // Rows whose slices sat beyond what was dropped move up. Slices never overlap, and a row
        // built later may sit before one built earlier, so every remaining row is checked.
        for r in &mut self.rows {
            if r.bits_at as usize >= bits_at {
                r.bits_at = r.bits_at.saturating_sub(words as u32);
            }
            if r.cells_at as usize >= cells_at {
                r.cells_at = r.cells_at.saturating_sub(cells as u32);
            }
            if r.kinds_at as usize >= kinds_at {
                r.kinds_at = r.kinds_at.saturating_sub(kinds as u32);
            }
            if r.styles_at as usize >= styles_at {
                r.styles_at = r.styles_at.saturating_sub(styles as u32);
            }
        }
    }

    fn remove_row(&mut self, key: u32) {
        if let Some(i) = self.row_index(key) {
            self.clear_row_at(i);
        }
    }

    /// Moves the stripe offsets of every row but the one at `at` when a slice changed size.
    /// Each argument is `(where the change happened, how many entries it added or removed)`.
    fn shift_offsets(&mut self, at: usize, bits: (u32, i32), cells: (u32, i32), kinds: (u32, i32), styles: (u32, i32)) {
        for (i, r) in self.rows.iter_mut().enumerate() {
            if i == at {
                continue;
            }
            if bits.1 != 0 && r.bits_at >= bits.0 {
                r.bits_at = r.bits_at.saturating_add_signed(bits.1);
            }
            if cells.1 != 0 && r.cells_at >= cells.0 {
                r.cells_at = r.cells_at.saturating_add_signed(cells.1);
            }
            if kinds.1 != 0 && r.kinds_at >= kinds.0 {
                r.kinds_at = r.kinds_at.saturating_add_signed(kinds.1);
            }
            if styles.1 != 0 && r.styles_at >= styles.0 {
                r.styles_at = r.styles_at.saturating_add_signed(styles.1);
            }
        }
    }

    /// Grows the bitmap of the row at `at` to cover `col`, keeping `first` word-aligned, and opens
    /// the stripes up for the words that adds.
    fn cover(&mut self, at: usize, col: u32) {
        let (first, words) = (self.rows[at].first, self.rows[at].words);
        if words == 0 {
            self.rows[at].first = col / 64 * 64;
            let bits_at = self.rows[at].bits_at;
            insert_at(&mut self.bits, bits_at as usize, 0);
            self.rows[at].words = 1;
            self.shift_offsets(at, (bits_at, 1), (0, 0), (0, 0), (0, 0));
            return;
        }
        let mut left = 0usize;
        if col < first {
            left = ((first - col) as usize).div_ceil(64);
        }
        let cover = first.saturating_add(words.saturating_mul(64));
        let right = if col >= cover { ((col - cover) / 64 + 1) as usize } else { 0 };
        if left == 0 && right == 0 {
            return;
        }
        let bits_at = self.rows[at].bits_at as usize;
        let added = (left + right) as u32;
        if left > 0 {
            for _ in 0..left {
                insert_at(&mut self.bits, bits_at, 0);
            }
            self.rows[at].first = first - left as u32 * 64;
        }
        for _ in 0..right {
            let end = bits_at + left + words as usize;
            insert_at(&mut self.bits, end, 0);
        }
        self.rows[at].words = words + added;
        self.shift_offsets(at, (bits_at as u32, added as i32), (0, 0), (0, 0), (0, 0));
    }

    /// Puts a stored cell at cell index `i` of the row at `at`, opening the stripes up for it.
    fn insert_cell(&mut self, at: usize, i: usize, col: u32, kind: u8, payload: u16, style: u16) {
        let (bits_at, first) = (self.rows[at].bits_at, self.rows[at].first);
        let (cells_at, old_cells, kinds_at, old_kinds) =
            (self.rows[at].cells_at, self.rows[at].cells, self.rows[at].kinds_at, self.rows[at].kind_runs);
        let (styles_at, old_styles) = (self.rows[at].styles_at, self.rows[at].style_runs);
        let kind_runs = runs_insert(&mut self.kinds, kinds_at as usize, old_kinds as usize, i, u16::from(kind));
        let style_runs = runs_insert(&mut self.styles, styles_at as usize, old_styles as usize, i, style);
        insert_at(&mut self.payloads, cells_at as usize + i, payload);
        // `cover` has already made room for the bit.
        let slot = col.wrapping_sub(first);
        if let Some(w) = self.bits.get_mut(bits_at as usize + (slot / 64) as usize) {
            *w |= 1u64 << (slot % 64);
        }
        self.shift_offsets(
            at,
            (bits_at, 0),
            (cells_at, 1),
            (kinds_at, kind_runs as i32 - old_kinds as i32),
            (styles_at, style_runs as i32 - old_styles as i32),
        );
        let row = &mut self.rows[at];
        row.cells = old_cells + 1;
        row.kind_runs = kind_runs as u32;
        row.style_runs = style_runs as u32;
    }

    /// Drops the stored cell at index `i` of the row at `at`.
    fn remove_cell(&mut self, at: usize, i: usize, col: u32) {
        let (bits_at, first) = (self.rows[at].bits_at, self.rows[at].first);
        let (cells_at, old_cells, kinds_at, old_kinds) =
            (self.rows[at].cells_at, self.rows[at].cells, self.rows[at].kinds_at, self.rows[at].kind_runs);
        let (styles_at, old_styles) = (self.rows[at].styles_at, self.rows[at].style_runs);
        let kind_runs = runs_remove(&mut self.kinds, kinds_at as usize, old_kinds as usize, i);
        let style_runs = runs_remove(&mut self.styles, styles_at as usize, old_styles as usize, i);
        drop_at(&mut self.payloads, cells_at as usize + i);
        let slot = col.wrapping_sub(first);
        if let Some(w) = self.bits.get_mut(bits_at as usize + (slot / 64) as usize) {
            *w &= !(1u64 << (slot % 64));
        }
        self.shift_offsets(
            at,
            (bits_at, 0),
            (cells_at, -1),
            (kinds_at, kind_runs as i32 - old_kinds as i32),
            (styles_at, style_runs as i32 - old_styles as i32),
        );
        let row = &mut self.rows[at];
        row.cells = old_cells.saturating_sub(1);
        row.kind_runs = kind_runs as u32;
        row.style_runs = style_runs as u32;
    }

    /// Gives back what building the band over-allocated: the stripes, and the number index that
    /// only adding cells needs.
    fn finish(&mut self) {
        self.rows.shrink_to_fit();
        self.bits.shrink_to_fit();
        self.kinds.shrink_to_fit();
        self.styles.shrink_to_fit();
        self.payloads.shrink_to_fit();
        self.numbers.shrink_to_fit();
        self.texts.shrink_to_fit();
        self.style_ids.shrink_to_fit();
        self.drop_number_slots();
        self.drop_text_index();
    }

    /// The band's slot for `s`, adding it when new. A band that has been finished has no index;
    /// the first edit that needs one rebuilds it from the texts the band still holds.
    fn text_slot(&mut self, s: &Arc<str>) -> u32 {
        if self.text_index.is_empty() && !self.texts.is_empty() {
            self.reindex_texts();
        }
        let key = Arc::as_ptr(s) as *const u8 as usize;
        match self.text_index.binary_search_by_key(&key, |(at, _)| *at) {
            Ok(i) => self.text_index[i].1,
            Err(at) => {
                let slot = self.texts.len() as u32;
                self.texts.push(s.clone());
                self.text_index.insert(at, (key, slot));
                slot
            }
        }
    }

    /// Rebuilds the pointer index from the band's text table. The table is kept after the band is
    /// finished; the index is only needed while cells are being added.
    fn reindex_texts(&mut self) {
        self.text_index = self.texts.iter().enumerate().map(|(i, t)| (Arc::as_ptr(t) as *const u8 as usize, i as u32)).collect();
        self.text_index.sort_unstable_by_key(|(at, _)| *at);
    }

    /// Offers the text index back to the allocator, like the number index: only adding cells needs
    /// it. Dropping it matters most on a unique-key export, where a band holds hundreds of texts
    /// and every band would otherwise keep a 16-byte sorted entry per text forever.
    fn drop_text_index(&mut self) {
        self.text_index = Vec::new();
    }

    /// The band's slot for `v`, adding it when new — `None` once the band holds more numbers than
    /// a `u16` can index, which sends the cell to the row's wide table instead.
    fn number_slot(&mut self, v: f64) -> Option<u16> {
        let bits = number_bits(v);
        if self.number_slots.is_empty() {
            if self.numbers.is_empty() {
                self.number_slots = vec![0; 16];
            } else {
                self.reindex_numbers();
            }
        }
        let mask = self.number_slots.len() - 1;
        let mut i = number_hash(bits) & mask;
        loop {
            let slot = self.number_slots[i];
            if slot == 0 {
                break;
            }
            if self.numbers[(slot - 1) as usize].to_bits() == bits {
                return Some(slot - 1);
            }
            i = (i + 1) & mask;
        }
        if self.numbers.len() >= 0xFFFF {
            return None;
        }
        let index = self.numbers.len() as u16;
        self.numbers.push(f64::from_bits(bits));
        self.number_slots[i] = index + 1;
        // Three quarters full: the probe walk stays short and the table stays small, which matters
        // because a band only holds a couple of hundred distinct numbers.
        if self.numbers.len() * 4 > self.number_slots.len() * 3 {
            self.grow_numbers();
        }
        Some(index)
    }

    /// Offers the number index back to the allocator. It is only needed to find a value while
    /// cells are being added, so a band that has been built does not keep it; an edit rebuilds it
    /// from the numbers the band still holds.
    fn drop_number_slots(&mut self) {
        self.number_slots = Vec::new();
    }

    fn reindex_numbers(&mut self) {
        let len = (self.numbers.len() * 2).max(16).next_power_of_two();
        self.number_slots = vec![0; len];
        let mask = len - 1;
        for (index, v) in self.numbers.iter().enumerate() {
            let mut i = number_hash(v.to_bits()) & mask;
            while self.number_slots[i] != 0 {
                i = (i + 1) & mask;
            }
            self.number_slots[i] = index as u16 + 1;
        }
    }

    fn grow_numbers(&mut self) {
        let len = self.number_slots.len() * 2;
        self.number_slots = vec![0; len];
        let mask = len - 1;
        for (index, v) in self.numbers.iter().enumerate() {
            let mut i = number_hash(v.to_bits()) & mask;
            while self.number_slots[i] != 0 {
                i = (i + 1) & mask;
            }
            self.number_slots[i] = index as u16 + 1;
        }
    }

    fn number(&self, slot: u16) -> f64 {
        self.numbers.get(slot as usize).copied().unwrap_or(0.0)
    }

    /// Kind and payload for a value. The tables are per band, so this needs the band.
    fn payload(&mut self, value: &Value) -> (u8, Payload) {
        match value {
            Value::Text(t) => {
                let slot = self.text_slot(t);
                match u16::try_from(slot) {
                    Ok(slot) => (TEXT, Payload::Small(slot)),
                    // The table holds more texts than a `u16` can index: the payload points into
                    // the row's side table, which holds this text's slot.
                    Err(_) => (TEXT_WIDE, Payload::Wide(u64::from(slot))),
                }
            }
            Value::Number(n) => match self.number_slot(*n) {
                Some(slot) => (NUMBER, Payload::Small(slot)),
                None => (NUMBER_WIDE, Payload::Wide(number_bits(*n))),
            },
            Value::Bool(b) => (BOOL, Payload::Small(u16::from(*b))),
            Value::Error(e) => (ERROR, Payload::Small(error_code(*e) as u16)),
            Value::Array(_) => (ARRAY, Payload::Small(0)),
            Value::Empty => (EMPTY, Payload::Small(0)),
        }
    }

    fn style_slot(&mut self, s: StyleId) -> u16 {
        if let Some(i) = self.style_ids.iter().position(|x| *x == s) {
            return i as u16;
        }
        self.style_ids.push(s);
        (self.style_ids.len() - 1) as u16
    }

    fn text(&self, payload: u64) -> Arc<str> {
        self.texts.get(payload as usize).cloned().unwrap_or_else(|| Arc::from(""))
    }

    fn style(&self, slot: u16) -> StyleId {
        self.style_ids.get(slot as usize).copied().unwrap_or(StyleId::DEFAULT)
    }

    /// Puts a payload that does not fit `u16` in the row's side table and returns its index there.
    fn push_wide(&mut self, at: usize, value: u64) -> u16 {
        // The row's side table takes a value: the band's rows are not its parent's any more.
        self.tag.rows_touched = true;
        // A value write appends (it cannot tell which entry was the cell's own), so a band
        // rewritten many times — a recalculated row of formulas — walks past the 65,535 an index
        // can hold; `compact_wide` brings it back to one entry per value the row still uses.
        if self.rows[at].extras.as_ref().is_some_and(|e| e.wide().len() > usize::from(u16::MAX)) {
            self.compact_wide(at);
        }
        let extras = Arc::make_mut(self.rows[at].extras.get_or_insert_with(Default::default));
        let rare = extras.rare_mut();
        rare.wide.push(value);
        // Compaction keeps the table at or below the row's cell count, so the index always fits.
        u16::try_from(rare.wide.len() - 1).unwrap_or(u16::MAX)
    }

    /// Rebuilds the row's wide table with one entry per distinct value its cells still use, so the
    /// index a payload stores always fits a `u16`. A row holds at most
    /// [`cella_core::MAX_COLS`] cells, so the rebuilt table always fits.
    fn compact_wide(&mut self, at: usize) {
        self.tag.rows_touched = true;
        let cells = self.rows[at].cells as usize;
        let len = self.rows[at].extras.as_ref().map_or(0, |e| e.wide().len());
        if len <= usize::from(u16::MAX) {
            return;
        }
        let mut used: Vec<(usize, u64)> = Vec::new();
        for i in 0..cells {
            let kind = self.rows[at].kind_at(self, i);
            if kind != TEXT_WIDE && kind != NUMBER_WIDE {
                continue;
            }
            let slot = usize::from(self.rows[at].payload_at(self, i));
            if let Some(v) = self.rows[at].extras.as_ref().and_then(|e| e.wide().get(slot)) {
                used.push((i, *v));
            }
        }
        let mut wide: Vec<u64> = Vec::with_capacity(used.len());
        let mut seen: std::collections::HashMap<u64, u16> = std::collections::HashMap::new();
        let mut slots: Vec<(usize, u16)> = Vec::with_capacity(used.len());
        for (i, v) in used {
            let slot = match seen.get(&v) {
                Some(slot) => *slot,
                None => {
                    let Ok(slot) = u16::try_from(wide.len()) else { break };
                    wide.push(v);
                    seen.insert(v, slot);
                    slot
                }
            };
            slots.push((i, slot));
        }
        if let Some(extras) = self.rows[at].extras.as_mut() {
            Arc::make_mut(extras).rare_mut().wide = wide;
        }
        let cells_at = self.rows[at].cells_at as usize;
        for (i, slot) in slots {
            if let Some(p) = self.payloads.get_mut(cells_at + i) {
                *p = slot;
            }
        }
    }

    /// Rebuilds the `Cell` at `col` from the packed row.
    fn cell_at(&self, row: &BandRow, i: usize, col: u32) -> Cell {
        Cell { value: self.value_at(row, i, col), formula: row.formula_at(col).cloned(), style: self.style(row.style_at(self, i)) }
    }

    /// The value of the `i`th stored cell alone, without the formula `Arc` and the style a
    /// [`Cell`] also carries: a read that wants the value pays for nothing else.
    fn value_at(&self, row: &BandRow, i: usize, col: u32) -> Value {
        let payload = row.payload_at(self, i);
        match row.kind_at(self, i) {
            NUMBER => Value::Number(self.number(payload)),
            NUMBER_WIDE => Value::Number(f64::from_bits(row.wide_at(self, i))),
            TEXT => Value::Text(self.text(u64::from(payload))),
            TEXT_WIDE => Value::Text(self.text(row.wide_at(self, i))),
            BOOL => Value::Bool(payload != 0),
            ERROR => Value::Error(*ERRORS.get(payload as usize).unwrap_or(&CellError::Value)),
            ARRAY => match row.arrays().binary_search_by_key(&col, |(c, _)| *c) {
                Ok(j) => row.arrays().get(j).map_or(Value::Empty, |(_, a)| Value::Array(a.clone())),
                Err(_) => Value::Empty,
            },
            _ => Value::Empty,
        }
    }

    /// Stores `cell` at `col`; a blank cell is removed instead. Returns whether a cell was stored
    /// at that position before and whether one is stored there now, so a caller that keeps the
    /// store's length does not have to probe the band twice more to learn it. Text and style slots
    /// are resolved before the row is borrowed, because both live in the band.
    /// Stores `cell` at `col`; a blank cell is removed instead. Returns whether a cell was stored
    /// at that position before, whether one is stored there now, and whether the cell it replaced
    /// held a formula — the last one is what lets a bulk write of values tell the recalculation it
    /// neither added nor removed a formula (see `Ctx::formulas_unchanged`).
    fn put(&mut self, row_key: u32, col: u32, cell: Cell) -> (bool, bool, bool) {
        let blank = cell.is_blank();
        let (kind, payload) = if blank { (EMPTY, Payload::Small(0)) } else { self.payload(&cell.value) };
        let style = if blank { 0 } else { self.style_slot(cell.style) };
        let array = match &cell.value {
            Value::Array(a) => Some(a.clone()),
            _ => None,
        };
        let formula = cell.formula.clone();
        let at = self.row_or_insert(row_key);
        if formula.is_some() || array.is_some() || self.rows[at].extras.is_some() {
            // A side table may be added, replaced or removed below: this band's rows are no
            // longer its parent's, so pricing has to walk them.
            self.tag.rows_touched = true;
        }
        self.cover(at, col);
        let existing = self.rows[at].index_of(self, col);
        let had_formula = self.rows[at].formula_at(col).is_some();
        self.rows[at].set_formula(col, if blank { None } else { formula.as_ref() });
        self.rows[at].set_array(col, if blank { None } else { array.as_ref() });
        let payload = match payload {
            Payload::Small(p) => p,
            Payload::Wide(v) => self.push_wide(at, v),
        };
        match existing {
            Some(i) if blank => self.remove_cell(at, i, col),
            Some(i) => {
                let (kinds_at, old_kinds) = (self.rows[at].kinds_at, self.rows[at].kind_runs);
                let kind_runs = runs_set(&mut self.kinds, kinds_at as usize, old_kinds as usize, i, u16::from(kind));
                let (styles_at, old_styles) = (self.rows[at].styles_at, self.rows[at].style_runs);
                let style_runs = runs_set(&mut self.styles, styles_at as usize, old_styles as usize, i, style);
                let slot = self.rows[at].cells_at as usize + i;
                if let Some(p) = self.payloads.get_mut(slot) {
                    *p = payload;
                }
                // shift_offsets walks every other row of the band and re-points the slices that
                // start past the change. Replacing a cell whose kind and style land in the runs
                // already there moves no slice, and that is the common case: a sort replacing a
                // row cell for cell would otherwise walk 64 rows six times a row.
                if kind_runs != old_kinds as usize || style_runs != old_styles as usize {
                    self.shift_offsets(
                        at,
                        (0, 0),
                        (0, 0),
                        (kinds_at, kind_runs as i32 - old_kinds as i32),
                        (styles_at, style_runs as i32 - old_styles as i32),
                    );
                }
                self.rows[at].kind_runs = kind_runs as u32;
                self.rows[at].style_runs = style_runs as u32;
            }
            None if !blank => {
                let i = self.rows[at].rank_before(self, col);
                self.insert_cell(at, i, col, kind, payload, style);
            }
            None => {}
        }
        if self.rows[at].is_empty() {
            self.clear_row_at(at);
        }
        (existing.is_some(), !blank, had_formula)
    }

    /// Whether the cell stored at (`row_key`, `col`) already holds exactly `value`, as the packed
    /// row sees it. `None` when no cell is stored there: a value write would have nowhere to go,
    /// exactly as [`set_value`](Self::set_value) refuses it, so a compare-then-write can tell the
    /// two apart without building a `Cell` or a new `Value`.
    fn value_is(&self, row_key: u32, col: u32, value: &Value) -> Option<bool> {
        let at = self.row_index(row_key)?;
        let row = self.rows.get(at)?;
        let i = row.index_of(self, col)?;
        Some(self.value_matches(row, i, col, value))
    }

    /// Whether the `i`th stored cell of `row` holds `value`. The packed kind and payload are
    /// compared against the incoming value's own bits: no `Cell`, no rebuilt `Value` — and for a
    /// text the pointer first, so a value the band already holds costs one comparison.
    fn value_matches(&self, row: &BandRow, i: usize, col: u32, value: &Value) -> bool {
        let payload = row.payload_at(self, i);
        match (row.kind_at(self, i), value) {
            (NUMBER, Value::Number(n)) => self.number(payload).to_bits() == number_bits(*n),
            (NUMBER_WIDE, Value::Number(n)) => row.wide_at(self, i) == number_bits(*n),
            (TEXT, Value::Text(t)) => self.text_matches(u64::from(payload), t),
            (TEXT_WIDE, Value::Text(t)) => self.text_matches(row.wide_at(self, i), t),
            (BOOL, Value::Bool(b)) => (payload != 0) == *b,
            (ERROR, Value::Error(e)) => payload == error_code(*e) as u16,
            // An array value is charged once per band and rarely written back; the pointer is the
            // test, so a deep-equal copy is a miss that writes, never a wrong skip.
            (ARRAY, Value::Array(a)) => {
                row.arrays().binary_search_by_key(&col, |(c, _)| *c).is_ok_and(|j| row.arrays().get(j).is_some_and(|(_, had)| Arc::ptr_eq(had, a)))
            }
            (EMPTY, Value::Empty) => true,
            _ => false,
        }
    }

    /// Whether the text at `slot` is `text`: the same `Arc` first, then the characters.
    fn text_matches(&self, slot: u64, text: &Arc<str>) -> bool {
        self.texts.get(slot as usize).is_some_and(|stored| Arc::ptr_eq(stored, text) || stored.as_ref() == text.as_ref())
    }

    /// Replaces the value of a stored cell, keeping its formula and style. Returns false when no
    /// cell is stored at `col` (a result has nowhere to go there). Saves what [`put`](Self::put)
    /// costs for a cell that only changes value: nothing is constructed, no formula or style slot
    /// is looked up, and the row already holds every stripe entry.
    fn set_value(&mut self, row_key: u32, col: u32, value: &Value) -> bool {
        let Some(at) = self.row_index(row_key) else { return false };
        let Some(i) = self.rows[at].index_of(self, col) else { return false };
        let (kind, payload) = self.payload(value);
        let payload = match payload {
            Payload::Small(p) => p,
            Payload::Wide(v) => self.push_wide(at, v),
        };
        let slot = self.rows[at].cells_at as usize + i;
        if let Some(p) = self.payloads.get_mut(slot) {
            *p = payload;
        }
        // Most results keep their kind (`=A1*2` stays a number): the run stripe need not move.
        if self.rows[at].kind_at(self, i) != kind {
            let (kinds_at, old_kinds) = (self.rows[at].kinds_at, self.rows[at].kind_runs);
            let kind_runs = runs_set(&mut self.kinds, kinds_at as usize, old_kinds as usize, i, u16::from(kind));
            self.shift_offsets(at, (0, 0), (0, 0), (kinds_at, kind_runs as i32 - old_kinds as i32), (0, 0));
            self.rows[at].kind_runs = kind_runs as u32;
        }
        match value {
            Value::Array(a) => {
                self.tag.rows_touched = true;
                self.rows[at].set_array(col, Some(a));
            }
            _ if self.rows[at].arrays().iter().any(|(c, _)| *c == col) => {
                self.tag.rows_touched = true;
                self.rows[at].set_array(col, None);
            }
            _ => {}
        }
        true
    }

    /// Builds a whole row at once, from cells already sorted by column. The row's slices are
    /// appended to the band's stripes, so there is no growth slack to give back and no
    /// insert-per-cell shifting.
    fn put_row(&mut self, row_key: u32, cells: &mut Vec<(u32, Cell)>) {
        // A whole row is replaced: whatever the parent's row held is not this band's any more.
        self.tag.rows_touched = true;
        // One pass: the cells arrive sorted by column, so the bitmap can be sized from the first
        // and last non-blank ones and the row built as it is walked. This used to collect a
        // `Vec<(u32, u8, Payload, u16)>` first and walk that, which was one allocation and one
        // copy per cell more than the work needs.
        let first = match cells.iter().find(|(_, cell)| !cell.is_blank()).map(|(col, _)| col / 64 * 64) {
            Some(first) => first,
            None => {
                self.remove_row(row_key);
                return;
            }
        };
        let last = cells.iter().rev().find(|(_, cell)| !cell.is_blank()).map(|(col, _)| *col).unwrap_or(first);
        let words = (last - first) / 64 + 1;
        // A row that is being replaced goes first: dropping it moves the stripes' ends.
        if let Some(at) = self.row_index(row_key) {
            self.clear_row_at(at);
        }
        let (bits_at, cells_at) = (self.bits.len() as u32, self.payloads.len() as u32);
        let (kinds_at, styles_at) = (self.kinds.len() as u32, self.styles.len() as u32);
        self.bits.resize(self.bits.len() + words as usize, 0);
        let mut formulas: Vec<(u32, Arc<Formula>)> = Vec::new();
        let mut arrays: Vec<(u32, Arc<cella_core::Array>)> = Vec::new();
        let mut wide: Vec<u64> = Vec::new();
        let mut stored = 0u32;
        for (col, cell) in cells.drain(..) {
            if cell.is_blank() {
                continue;
            }
            let (kind, payload) = self.payload(&cell.value);
            let style = self.style_slot(cell.style);
            if let Some(f) = &cell.formula {
                formulas.push((col, f.clone()));
            }
            if let Value::Array(a) = &cell.value {
                arrays.push((col, a.clone()));
            }
            let slot = col.wrapping_sub(first);
            if let Some(w) = self.bits.get_mut(bits_at as usize + (slot / 64) as usize) {
                *w |= 1u64 << (slot % 64);
            }
            run_push(&mut self.kinds, kinds_at as usize, u16::from(kind));
            run_push(&mut self.styles, styles_at as usize, style);
            let payload = match payload {
                Payload::Small(p) => p,
                Payload::Wide(v) => {
                    wide.push(v);
                    (wide.len() - 1) as u16
                }
            };
            self.payloads.push(payload);
            stored += 1;
        }
        // A Vec's first push allocates four 16-byte slots for a two-entry row, and a read of a
        // million rows would keep that spare capacity alive: give it back before the row is built.
        formulas.shrink_to_fit();
        let rare = (!arrays.is_empty() || !wide.is_empty()).then(|| Box::new(RareRow { arrays, wide }));
        let extras = if formulas.is_empty() && rare.is_none() { None } else { Some(Arc::new(RowExtras { formulas, rare })) };
        let row = BandRow {
            key: row_key,
            first,
            bits_at,
            words,
            cells_at,
            kinds_at,
            styles_at,
            cells: stored,
            kind_runs: self.kinds.len() as u32 - kinds_at,
            style_runs: self.styles.len() as u32 - styles_at,
            extras,
        };
        match self.rows.binary_search_by_key(&row_key, |r| r.key) {
            Ok(i) => self.rows[i] = row,
            Err(i) => {
                if row_key < BAND {
                    self.row_bits |= 1u64 << row_key;
                }
                self.rows.insert(i, row);
            }
        }
    }

    fn take(&mut self, row_key: u32, col: u32) -> Option<Cell> {
        let at = self.row_index(row_key)?;
        let i = self.rows[at].index_of(self, col)?;
        // The row's side tables are read and then cleared below.
        self.tag.rows_touched = true;
        let cell = self.cell_at(&self.rows[at], i, col);
        self.rows[at].set_formula(col, None);
        self.rows[at].set_array(col, None);
        self.remove_cell(at, i, col);
        if self.rows[at].is_empty() {
            self.clear_row_at(at);
        }
        Some(cell)
    }

    /// True when both bands hold the same cells. Text is compared by content: two bands that came
    /// from different reads index different tables.
    fn same_as(&self, other: &Band) -> bool {
        self.rows.len() == other.rows.len()
            && self.rows.iter().map(|r| r.key).eq(other.rows.iter().map(|r| r.key))
            && self.rows.iter().all(|a| other.row(a.key).is_some_and(|b| row_same_as(self, a, other, b)))
    }
}

/// True when two stored cells hold the same value, whatever slot their band gave it.
fn same_value(a: &Band, ra: &BandRow, i: usize, b: &Band, rb: &BandRow, j: usize) -> bool {
    if a.style(ra.style_at(a, i)) != b.style(rb.style_at(b, j)) {
        return false;
    }
    let (ka, kb) = (ra.kind_at(a, i), rb.kind_at(b, j));
    let number = |band: &Band, row: &BandRow, kind: u8, i: usize| match kind {
        NUMBER => band.number(row.payload_at(band, i)).to_bits(),
        _ => row.wide_at(band, i),
    };
    let text = |band: &Band, row: &BandRow, kind: u8, i: usize| match kind {
        TEXT => u64::from(row.payload_at(band, i)),
        _ => row.wide_at(band, i),
    };
    match (ka, kb) {
        (NUMBER | NUMBER_WIDE, NUMBER | NUMBER_WIDE) => number(a, ra, ka, i) == number(b, rb, kb, j),
        (TEXT | TEXT_WIDE, TEXT | TEXT_WIDE) => a.text(text(a, ra, ka, i)) == b.text(text(b, rb, kb, j)),
        // Arrays are compared by value where their column is known; here the position is enough.
        (ARRAY, ARRAY) => true,
        _ => ka == kb && ra.payload_at(a, i) == rb.payload_at(b, j),
    }
}

fn row_same_as(a: &Band, ra: &BandRow, b: &Band, rb: &BandRow) -> bool {
    if ra.first != rb.first || ra.bits(a) != rb.bits(b) || ra.cells != rb.cells {
        return false;
    }
    for i in 0..ra.cells as usize {
        if !same_value(a, ra, i, b, rb, i) {
            return false;
        }
    }
    // A filled formula keeps no text (R34), so "the same row" compares the formulas themselves:
    // their shape (by pointer when it is the very same `Arc`, by spelling across two workbooks),
    // their array range and their dynamic mark.
    let (fa, fb) = (ra.formulas(), rb.formulas());
    fa.len() == fb.len() && fa.iter().zip(fb).all(|((ca, x), (cb, y))| ca == cb && x == y)
}

/// Bands built away from the store, by a reader decoding rows on several threads. The step that
/// packs rows — interning their numbers and texts, then the four arrays of a row — is most of what
/// reading costs (1.7 s of a 2.3 s decode on a 21M-cell sheet), and it cannot be shared out unless
/// the bands are built beside the store and spliced in afterwards.
#[derive(Debug, Default)]
pub struct BandBatch {
    bands: BTreeMap<u32, Band>,
}

impl BandBatch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.bands.is_empty()
    }

    /// Stores a whole row, exactly as [`CellStore::set_row`] would; `cells` is drained.
    pub fn set_row(&mut self, row: u32, cells: &mut Vec<(u32, Cell)>) {
        if cells.is_empty() {
            return;
        }
        cells.sort_unstable_by_key(|(c, _)| *c);
        cells.dedup_by(|a, b| {
            a.0 == b.0 && {
                b.1 = a.1.clone();
                true
            }
        });
        let band_row = row % BAND;
        let band = self.bands.entry(row / BAND).or_default();
        if band.row(band_row).is_none_or(BandRow::is_empty) {
            band.put_row(band_row, cells);
        } else {
            for (col, cell) in cells.drain(..) {
                let _ = band.put(band_row, col, cell);
            }
        }
        if band.rows.is_empty() {
            self.bands.remove(&(row / BAND));
        }
    }
}

/// One stored cell as its page holds it: no `Cell`, no `Value`, no allocation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Packed<'a> {
    Number(f64),
    Text(&'a str),
    Bool(bool),
    Error(CellError),
}

/// One logical→physical row run. Rows are stored under physical keys; a structural row edit changes
/// this map instead of moving every cell below it. `len` is in rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RowSegment {
    logical: u32,
    physical: u32,
    len: u32,
}

impl RowSegment {
    fn logical_end(self) -> u32 {
        self.logical.saturating_add(self.len)
    }

    fn physical_end(self) -> u32 {
        self.physical.saturating_add(self.len)
    }
}

/// The logical→physical row map. `identity` is the fast path used by every workbook that has never
/// had a row inserted or deleted: logical row = physical row, no lookup. The first structural edit
/// materializes one segment covering the sheet and thereafter keeps two sorted views, by logical
/// row for reads/writes and by physical row for iteration and `used_range`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct RowMap {
    identity: bool,
    segments: Vec<RowSegment>,
    by_physical: Vec<RowSegment>,
    /// Rows written into an inserted gap get fresh physical rows from here; existing rows keep
    /// their physical keys, so an insert never touches a band.
    next_physical: u32,
}

impl Default for RowMap {
    fn default() -> Self {
        RowMap { identity: true, segments: Vec::new(), by_physical: Vec::new(), next_physical: cella_core::MAX_ROWS }
    }
}

impl RowMap {
    fn is_identity(&self) -> bool {
        self.identity
    }

    /// Whether both maps put every cell at the same logical position (two versions of one store,
    /// one of them an undo snapshot: a value edit leaves the mapping alone).
    fn same_as(&self, other: &RowMap) -> bool {
        self.identity == other.identity && self.segments == other.segments
    }

    /// Heap bytes of this map: the two segment vectors, or nothing while it is the identity.
    fn memory(&self) -> usize {
        let entry = std::mem::size_of::<RowSegment>();
        self.segments.capacity().saturating_mul(entry).saturating_add(self.by_physical.capacity().saturating_mul(entry))
    }

    fn map(&self, logical: u32) -> Option<u32> {
        if logical >= cella_core::MAX_ROWS {
            return None;
        }
        if self.identity {
            return Some(logical);
        }
        let index = self.segments.partition_point(|s| s.logical <= logical).checked_sub(1)?;
        let segment = self.segments.get(index)?;
        let offset = logical.checked_sub(segment.logical)?;
        (offset < segment.len).then(|| segment.physical.saturating_add(offset))
    }

    fn inverse(&self, physical: u32) -> Option<u32> {
        if self.identity {
            return (physical < cella_core::MAX_ROWS).then_some(physical);
        }
        let index = self.by_physical.partition_point(|s| s.physical <= physical).checked_sub(1)?;
        let segment = self.by_physical.get(index)?;
        let offset = physical.checked_sub(segment.physical)?;
        (offset < segment.len).then(|| segment.logical.saturating_add(offset))
    }

    fn ensure(&mut self) {
        if self.identity {
            self.identity = false;
            self.segments = vec![RowSegment { logical: 0, physical: 0, len: cella_core::MAX_ROWS }];
            self.rebuild();
        }
    }

    /// Allocates a physical row for a logical row that has no mapping (a row inside an inserted
    /// gap, or one exposed by a delete). The fresh key is above every physical row the sheet had,
    /// so it cannot collide with an existing band.
    fn allocate(&mut self, logical: u32) -> Option<u32> {
        if let Some(physical) = self.map(logical) {
            return Some(physical);
        }
        if logical >= cella_core::MAX_ROWS {
            return None;
        }
        self.ensure();
        let physical = self.next_physical;
        self.next_physical = self.next_physical.checked_add(1)?;
        let at = self.segments.partition_point(|s| s.logical < logical);
        self.segments.insert(at, RowSegment { logical, physical, len: 1 });
        self.rebuild();
        Some(physical)
    }

    /// The segments intersecting a logical row range, clipped to it, in logical order.
    fn segments_in(&self, r0: u32, r1: u32) -> Vec<RowSegment> {
        let (r0, r1) = (r0.min(cella_core::MAX_ROWS.saturating_sub(1)), r1.min(cella_core::MAX_ROWS.saturating_sub(1)));
        if r0 > r1 {
            return Vec::new();
        }
        if self.identity {
            return vec![RowSegment { logical: r0, physical: r0, len: r1 - r0 + 1 }];
        }
        let mut out = Vec::new();
        for s in &self.segments {
            let lo = s.logical.max(r0);
            let hi = s.logical_end().min(r1.saturating_add(1));
            if lo >= hi {
                continue;
            }
            out.push(RowSegment { logical: lo, physical: s.physical.saturating_add(lo - s.logical), len: hi - lo });
        }
        out
    }

    /// Inserts (`delta > 0`) or deletes (`delta < 0`) logical rows at `from`, changing only the
    /// mapping. Rows below stay on their physical bands; the vacated/removed range is a gap or a
    /// dropped set of segments. `from` is the first source row for a delete, as in the old
    /// `shift_rows`, so the deleted destination range is `[from - count, from)`.
    fn shift(&mut self, from: u32, delta: i64) {
        if delta == 0 {
            return;
        }
        let from = from.min(cella_core::MAX_ROWS);
        let count = delta.unsigned_abs().min(u64::from(cella_core::MAX_ROWS)) as u32;
        if count == 0 {
            return;
        }
        self.ensure();
        let mut out: Vec<RowSegment> = Vec::new();
        for s in &self.segments {
            let a = s.logical;
            let b = s.logical_end().min(cella_core::MAX_ROWS);
            if a >= b {
                continue;
            }
            if delta > 0 {
                if a < from {
                    let hi = b.min(from);
                    if a < hi {
                        out.push(RowSegment { logical: a, physical: s.physical, len: hi - a });
                    }
                }
                if b > from {
                    let start = a.max(from);
                    let new_logical = start.saturating_add(count);
                    if new_logical < cella_core::MAX_ROWS {
                        let end = b.saturating_add(count).min(cella_core::MAX_ROWS);
                        if new_logical < end {
                            out.push(RowSegment { logical: new_logical, physical: s.physical.saturating_add(start - a), len: end - new_logical });
                        }
                    }
                }
            } else {
                let delete_start = from.saturating_sub(count);
                let delete_end = from;
                if a < delete_start {
                    let hi = b.min(delete_start);
                    if a < hi {
                        out.push(RowSegment { logical: a, physical: s.physical, len: hi - a });
                    }
                }
                if b > delete_end {
                    let start = a.max(delete_end);
                    let new_logical = start.saturating_sub(count);
                    if start < b {
                        out.push(RowSegment { logical: new_logical, physical: s.physical.saturating_add(start - a), len: b - start });
                    }
                }
            }
        }
        self.segments = out;
        self.rebuild();
    }

    /// Reclaims the map after cells have been removed: merges the runs the way [`rebuild`] does
    /// and returns a single run from the origin to the identity fast path when this store has no
    /// page that the shorter run would otherwise leave behind. A page can hold cells at physical
    /// rows past the run; turning the map into an identity would make them readable again, so a
    /// store with pages only resets a map that covers every row.
    ///
    /// [`rebuild`]: Self::rebuild
    fn normalize(&mut self, reset_prefix: bool) {
        if self.identity {
            return;
        }
        self.rebuild();
        let Some(segment) = self.segments.first().copied() else { return };
        if self.segments.len() != 1 || segment.logical != 0 || segment.physical != 0 || segment.len == 0 {
            return;
        }
        if !reset_prefix && segment.len < cella_core::MAX_ROWS {
            return;
        }
        self.identity = true;
        self.segments.clear();
        self.by_physical.clear();
        self.next_physical = cella_core::MAX_ROWS;
    }

    /// Merges adjacent logical/physical runs and refreshes the inverse view.
    fn rebuild(&mut self) {
        let mut merged: Vec<RowSegment> = Vec::with_capacity(self.segments.len());
        for s in self.segments.drain(..) {
            if s.len == 0 {
                continue;
            }
            if let Some(last) = merged.last_mut()
                && last.logical_end() == s.logical
                && last.physical_end() == s.physical
                && last.len.checked_add(s.len).is_some()
            {
                last.len = last.len.saturating_add(s.len);
                continue;
            }
            merged.push(s);
        }
        self.segments = merged;
        let mut by = self.segments.clone();
        by.sort_unstable_by_key(|s| (s.physical, s.logical));
        let mut inverse: Vec<RowSegment> = Vec::with_capacity(by.len());
        for s in by {
            if let Some(last) = inverse.last_mut()
                && last.physical_end() == s.physical
                && last.logical_end() == s.logical
                && last.len.checked_add(s.len).is_some()
            {
                last.len = last.len.saturating_add(s.len);
                continue;
            }
            inverse.push(s);
        }
        self.by_physical = inverse;
    }
}

/// The column sibling of [`RowMap`]. Columns need one extra rule: their physical space is the
/// worksheet's 16,384 columns plus whatever a structural edit allocated, so a deleted run's
/// physical columns go on a free list and a write into an inserted gap reuses one before extending
/// the range. `len` is in columns.
type ColSegment = RowSegment;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ColMap {
    identity: bool,
    segments: Vec<ColSegment>,
    by_physical: Vec<ColSegment>,
    next_physical: u32,
    free: Vec<u32>,
}

impl Default for ColMap {
    fn default() -> Self {
        ColMap { identity: true, segments: Vec::new(), by_physical: Vec::new(), next_physical: 0, free: Vec::new() }
    }
}

impl ColMap {
    fn is_identity(&self) -> bool {
        self.identity
    }

    /// The column sibling of [`RowMap::same_as`].
    fn same_as(&self, other: &ColMap) -> bool {
        self.identity == other.identity && self.segments == other.segments
    }

    /// Heap bytes of this map: the two segment vectors and the free list.
    fn memory(&self) -> usize {
        let entry = std::mem::size_of::<ColSegment>();
        self.segments
            .capacity()
            .saturating_mul(entry)
            .saturating_add(self.by_physical.capacity().saturating_mul(entry))
            .saturating_add(self.free.capacity().saturating_mul(std::mem::size_of::<u32>()))
    }

    /// Materializes the initial identity run for the columns the sheet actually holds. Unlike
    /// rows, a full 16,384-column identity range would make a write into the first inserted column
    /// pick column 16,384 and widen every row's bitmap; only the used width is materialized.
    fn ensure_with(&mut self, physical_max: u32) {
        if !self.identity {
            return;
        }
        self.identity = false;
        let width = physical_max.saturating_add(1).min(cella_core::MAX_COLS);
        self.next_physical = width;
        if width > 0 {
            self.segments = vec![ColSegment { logical: 0, physical: 0, len: width }];
        }
        self.rebuild();
    }

    fn map(&self, logical: u32) -> Option<u32> {
        if logical >= cella_core::MAX_COLS {
            return None;
        }
        if self.identity {
            return Some(logical);
        }
        let index = self.segments.partition_point(|s| s.logical <= logical).checked_sub(1)?;
        let segment = self.segments.get(index)?;
        let offset = logical.checked_sub(segment.logical)?;
        (offset < segment.len).then(|| segment.physical.saturating_add(offset))
    }

    fn inverse(&self, physical: u32) -> Option<u32> {
        if self.identity {
            return (physical < cella_core::MAX_COLS).then_some(physical);
        }
        let index = self.by_physical.partition_point(|s| s.physical <= physical).checked_sub(1)?;
        let segment = self.by_physical.get(index)?;
        let offset = physical.checked_sub(segment.physical)?;
        (offset < segment.len).then(|| segment.logical.saturating_add(offset))
    }

    fn allocate(&mut self, logical: u32) -> Option<u32> {
        if let Some(physical) = self.map(logical) {
            return Some(physical);
        }
        if logical >= cella_core::MAX_COLS {
            return None;
        }
        if self.identity {
            self.ensure_with(self.next_physical.min(cella_core::MAX_COLS.saturating_sub(1)));
        }
        let physical = if let Some(physical) = self.free.pop() {
            physical
        } else {
            let physical = self.next_physical;
            self.next_physical = self.next_physical.checked_add(1)?;
            physical
        };
        let at = self.segments.partition_point(|s| s.logical < logical);
        self.segments.insert(at, ColSegment { logical, physical, len: 1 });
        self.rebuild();
        Some(physical)
    }

    fn segments_in(&self, c0: u32, c1: u32) -> Vec<ColSegment> {
        let (c0, c1) = (c0.min(cella_core::MAX_COLS.saturating_sub(1)), c1.min(cella_core::MAX_COLS.saturating_sub(1)));
        if c0 > c1 {
            return Vec::new();
        }
        if self.identity {
            return vec![ColSegment { logical: c0, physical: c0, len: c1 - c0 + 1 }];
        }
        let mut out = Vec::new();
        for s in &self.segments {
            let lo = s.logical.max(c0);
            let hi = s.logical_end().min(c1.saturating_add(1));
            if lo >= hi {
                continue;
            }
            out.push(ColSegment { logical: lo, physical: s.physical.saturating_add(lo - s.logical), len: hi - lo });
        }
        out
    }

    fn shift(&mut self, from: u32, delta: i64) {
        if delta == 0 {
            return;
        }
        let from = from.min(cella_core::MAX_COLS);
        let count = delta.unsigned_abs().min(u64::from(cella_core::MAX_COLS)) as u32;
        if count == 0 {
            return;
        }
        self.ensure_with(0);
        let mut out: Vec<ColSegment> = Vec::new();
        for s in &self.segments {
            let a = s.logical;
            let b = s.logical_end().min(cella_core::MAX_COLS);
            if a >= b {
                continue;
            }
            if delta > 0 {
                if a < from {
                    let hi = b.min(from);
                    if a < hi {
                        out.push(ColSegment { logical: a, physical: s.physical, len: hi - a });
                    }
                }
                if b > from {
                    let start = a.max(from);
                    let new_logical = start.saturating_add(count);
                    if new_logical < cella_core::MAX_COLS {
                        let end = b.saturating_add(count).min(cella_core::MAX_COLS);
                        if new_logical < end {
                            out.push(ColSegment { logical: new_logical, physical: s.physical.saturating_add(start - a), len: end - new_logical });
                        }
                    }
                }
            } else {
                let delete_start = from.saturating_sub(count);
                let delete_end = from;
                if a < delete_start {
                    let hi = b.min(delete_start);
                    if a < hi {
                        out.push(ColSegment { logical: a, physical: s.physical, len: hi - a });
                    }
                }
                if b > delete_end {
                    let start = a.max(delete_end);
                    let new_logical = start.saturating_sub(count);
                    if start < b {
                        out.push(ColSegment { logical: new_logical, physical: s.physical.saturating_add(start - a), len: b - start });
                    }
                }
                // The destination columns are no longer mapped; their physical keys are reusable.
                let drop_start = a.max(delete_start);
                let drop_end = b.min(delete_end);
                for physical in drop_start.saturating_sub(a)..drop_end.saturating_sub(a) {
                    self.free.push(s.physical.saturating_add(physical));
                }
            }
        }
        self.segments = out;
        self.rebuild();
    }

    /// Reclaims the map after cells have been removed: merges the runs, drops the free keys that
    /// are mapped or past every physical column this map has handed out, and returns a single run
    /// from the origin to the identity fast path when this store has no page that the shorter run
    /// would otherwise leave behind. See [`RowMap::normalize`] for why pages bound the reset.
    fn normalize(&mut self, reset_prefix: bool) {
        if self.identity {
            self.free.clear();
            return;
        }
        self.rebuild();
        let mut free = std::mem::take(&mut self.free);
        free.sort_unstable();
        free.dedup();
        free.retain(|physical| *physical < self.next_physical && self.inverse(*physical).is_none());
        self.free = free;
        let Some(segment) = self.segments.first().copied() else { return };
        if self.segments.len() != 1 || segment.logical != 0 || segment.physical != 0 || segment.len == 0 {
            return;
        }
        if !reset_prefix && segment.len < cella_core::MAX_COLS {
            return;
        }
        self.identity = true;
        self.segments.clear();
        self.by_physical.clear();
        self.free.clear();
        self.next_physical = 0;
    }

    fn rebuild(&mut self) {
        let mut merged: Vec<ColSegment> = Vec::with_capacity(self.segments.len());
        for s in self.segments.drain(..) {
            if s.len == 0 {
                continue;
            }
            if let Some(last) = merged.last_mut()
                && last.logical_end() == s.logical
                && last.physical_end() == s.physical
            {
                last.len = last.len.saturating_add(s.len);
                continue;
            }
            merged.push(s);
        }
        self.segments = merged;
        let mut by = self.segments.clone();
        by.sort_unstable_by_key(|s| (s.physical, s.logical));
        let mut inverse: Vec<ColSegment> = Vec::with_capacity(by.len());
        for s in by {
            if let Some(last) = inverse.last_mut()
                && last.physical_end() == s.physical
                && last.logical_end() == s.logical
            {
                last.len = last.len.saturating_add(s.len);
                continue;
            }
            inverse.push(s);
        }
        self.by_physical = inverse;
    }
}

/// What one [`CellStore::compact`] pass reclaimed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Compacted {
    /// Cells dropped because no logical row or column could reach them.
    pub cells: usize,
    /// Live bands dropped because compaction emptied them.
    pub bands: usize,
    /// Paged bands rewritten (or dropped) to lose their orphans.
    pub pages: usize,
    /// Whether the store differs from before: a cell, band or page was reclaimed, or a map merged
    /// or returned to the identity fast path.
    pub changed: bool,
}

/// What a [`CellStore::page_cache_stats`] report says about the decoded bands kept for a page file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageCacheStats {
    /// The cache's byte budget, fixed when the page file was created.
    pub budget: usize,
    /// Decoded-band bytes the cache has charged now.
    pub bytes: usize,
    /// Entries in the cache's recency queue.
    pub entries: usize,
    /// Reads that found the band already decoded.
    pub hits: u64,
    /// Decodes that installed a band (and charged it to the budget).
    pub misses: u64,
    /// Entries dropped to stay within the budget.
    pub evictions: u64,
    /// Reads that found a page the file could not read or decode.
    pub corrupt: u64,
    /// Page writes the file refused or that failed; the band stays live when this happens. The
    /// first I/O failure stops further spills for the file's life (the pages already written stay
    /// readable), so this counts the moment paging gave up, not every band that then stayed live.
    pub spill_failures: u64,
    /// The largest single band charged. When this is above `budget` the cache kept one oversize
    /// band rather than evicting it only to decode it again on the next read.
    pub max_entry_bytes: usize,
}

/// Where a page file may be created and what it may be called.
///
/// The directory is tried first, then the system temp directory, and both are attempted rather
/// than probed for writability (`create_new` is the only test). A spill holds a second,
/// zlib-compressed copy of (part of) the document, so it is private (`0600` on Unix) and has no
/// life of its own: on Unix the name is removed as soon as the file is open (the handle survives),
/// on Windows it is created with `FILE_FLAG_DELETE_ON_CLOSE`, and [`CellStore`] dropping the file
/// removes whatever is left. On those platforms even a crash leaves nothing behind.
///
/// `CELLA_SPILL` overrides the policy: `temp` never tries the document's directory (a
/// sync-hostile folder, a network share), `off` never pages at all and the store stays live.
#[derive(Clone, Copy, Debug, Default)]
pub struct SpillOptions<'a> {
    /// The directory to try first — the document's folder. `None` leaves only the temp directory.
    pub dir: Option<&'a std::path::Path>,
    /// The document's stem, sanitized to `[A-Za-z0-9_-]` (at most 32 bytes, empty → `book`) and
    /// used in the file's name. No full path and no sheet name reaches the name.
    pub name: Option<&'a str>,
    /// The decoded-band cache budget in bytes; `None` reads `CELLA_PAGE_CACHE_MB`.
    pub cache_bytes: Option<usize>,
    /// The directory tried after `dir`; `None` is the system temp directory. Tests pin it so both
    /// candidates can be made unusable without touching the process-global environment.
    pub temp: Option<&'a std::path::Path>,
}

/// The live heap bytes a [`CellStore`] reaches, by allocation class: what [`CellStore::memory`]
/// reports.
///
/// # The contract
///
/// *An allocation is counted once.* Inside a band the [`Arc::strong_count`] convention decides
/// who charges a shared text, a row's extras table, a formula and its parsed tree: the owner that
/// holds the last reference charges the contents, and any other owner charges the control block.
/// Where a tree is shared by design (the dependency graph's node holds the same `Arc<Expr>`), the
/// cell charges it and the graph charges only the node that points at it.
///
/// A band this store shares with another store (an undo snapshot) *is* charged by both: each store
/// can reach it and neither owns it. A caller that reports the history beside the live workbook
/// must therefore report what the history retains *beyond* the live workbook —
/// [`retained_bytes_against`](CellStore::retained_bytes_against) is that figure — or it would count
/// the shared bands twice.
///
/// Deliberately not counted: allocator slack beyond a `Vec`'s capacity, the `Arc` control block of
/// the bands themselves, the page file on disk, the `Value`s inside an `Arc<Array>` (see
/// [`Band::memory`]), and the row/column `LineMap`s (a sheet's, not a store's).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct StoreMemory {
    /// The `BTreeMap`s that name the live bands and the pages, entry slots and node headers.
    pub band_index: usize,
    /// The `Band` structs and their row indexes.
    pub band_headers: usize,
    /// The presence bitmaps.
    pub bitmaps: usize,
    /// The payload stripe (a `u16` a stored cell).
    pub payloads: usize,
    /// The kind runs.
    pub kind_runs: usize,
    /// The style runs.
    pub style_runs: usize,
    /// The per-band `StyleId` dictionaries.
    pub style_ids: usize,
    /// The per-band number dictionaries and their slots.
    pub number_dictionaries: usize,
    /// The text dictionaries: their vectors and the characters they hold.
    pub texts: usize,
    /// The row side tables (`RowExtras`): their structs and their three vectors.
    pub row_extras: usize,
    /// Formula text, with the `Formula` struct that holds it.
    pub formula_text: usize,
    /// Parsed formula expression trees.
    pub formula_exprs: usize,
    /// Array values (their struct; their values are not walked).
    pub arrays: usize,
    /// Payloads in the row overflow vectors of a band with too many distinct numbers.
    pub wide: usize,
    /// The logical→physical row and column maps.
    pub maps: usize,
    /// The page file's handle, its decoded-band queue, and the pages' cached bands.
    pub page_cache: usize,
}

impl StoreMemory {
    pub fn total(&self) -> usize {
        [
            self.band_index,
            self.band_headers,
            self.bitmaps,
            self.payloads,
            self.kind_runs,
            self.style_runs,
            self.style_ids,
            self.number_dictionaries,
            self.texts,
            self.row_extras,
            self.formula_text,
            self.formula_exprs,
            self.arrays,
            self.wide,
            self.maps,
            self.page_cache,
        ]
        .iter()
        .fold(0usize, |sum, part| sum.saturating_add(*part))
    }

    /// Adds one band's breakdown, class by class: the only place a band's fields meet the store's.
    fn add_band(&mut self, b: &BandMemory) {
        self.band_headers = self.band_headers.saturating_add(b.header);
        self.bitmaps = self.bitmaps.saturating_add(b.bitmaps);
        self.payloads = self.payloads.saturating_add(b.payloads);
        self.kind_runs = self.kind_runs.saturating_add(b.kinds);
        self.style_runs = self.style_runs.saturating_add(b.styles);
        self.style_ids = self.style_ids.saturating_add(b.style_ids);
        self.number_dictionaries = self.number_dictionaries.saturating_add(b.numbers);
        self.texts = self.texts.saturating_add(b.texts);
        self.row_extras = self.row_extras.saturating_add(b.row_extras);
        self.formula_text = self.formula_text.saturating_add(b.formula_text);
        self.formula_exprs = self.formula_exprs.saturating_add(b.formula_exprs);
        self.arrays = self.arrays.saturating_add(b.arrays);
        self.wide = self.wide.saturating_add(b.wide);
    }
}

/// Heap bytes of a `BTreeMap` holding `len` entries of `entry` bytes each.
///
/// std's B-tree packs 11 (key, value) slots into a node and splits a node when it is full, so a
/// node holds about eight entries: the index is `len.div_ceil(8)` nodes of 11 slots plus the
/// 16-byte node header (the parent link and the two length words). The keys and values the slots
/// hold are counted where they are allocated.
pub fn btree_index_bytes(len: usize, entry: usize) -> usize {
    const SLOTS: usize = 11;
    const FILL: usize = 8;
    const HEADER: usize = 16;
    len.div_ceil(FILL).saturating_mul(SLOTS.saturating_mul(entry).saturating_add(HEADER))
}

/// Heap bytes of a hashbrown `HashMap`/`HashSet` whose `capacity()` is `capacity` entries of
/// `entry` bytes each: the table allocates eight buckets per seven entries and keeps one control
/// byte a bucket.
pub fn hash_index_bytes(capacity: usize, entry: usize) -> usize {
    capacity.div_ceil(7).saturating_mul(8).saturating_mul(entry.saturating_add(1))
}
/// The cached used range of a [`CellStore`]: the box, and whether a walk has to rebuild it.
#[derive(Clone, Copy, Debug, Default)]
struct UsedBox {
    /// The logical bounding box of the stored cells; `None` when the store holds none.
    used: Option<RangeRef>,
    /// `used` has to be walked again: a mutation removed cells, or moved them to another logical
    /// position. A reader that finds this set walks and clears it.
    stale: bool,
}

/// A `CellStore`'s packed bands, plus the count of stored cells.
#[derive(Debug, Default)]
pub struct CellStore {
    bands: BTreeMap<u32, Arc<Band>>,
    /// Bands that were spilled to a page file. A paged band is still in the store's logical range
    /// and `len`; it is decoded on the first read and then cached in its `Page`.
    pages: BTreeMap<u32, Arc<page::Page>>,
    /// The temporary file the pages live in, and the byte-budget cache of decoded bands that
    /// belongs to it. One per store (and shared by its clones and undo snapshots); dropping the
    /// last store removes the file.
    page_file: Option<Arc<page::PageFile>>,
    /// Logical→physical rows. Identity until a row insert or delete.
    row_map: RowMap,
    /// Logical→physical columns. Identity until a column insert or delete.
    col_map: ColMap,
    len: usize,
    /// The band cells were last added to. Cells arrive band by band as a file is read, so the band
    /// before it is complete and can stop carrying its number index. Not part of what a store is:
    /// two stores holding the same cells are equal whatever this says.
    building: Option<u32>,
    /// The logical bounding box of the stored cells, as [`used_range`](Self::used_range) reports
    /// it. Kept instead of recomputed: a write that stores a cell grows the box in O(1)
    /// (`grow_used`), and a mutation that can shrink it or move a stored cell to another logical
    /// position marks it stale (`mark_used_stale`), which the next reader repairs by walking it
    /// once. Walking on every call cost 8 ms on a million stored rows and ran twice per repaint.
    ///
    /// A lock, not a plain field, because a reader repairs it: `used_range` takes `&self` (the UI
    /// asks the sheet for its extent while drawing) and a repair is a mutation of derived state
    /// only. A mutator has `&mut self` and reaches it through `Mutex::get_mut`, so the write paths
    /// pay nothing for it; a reader pays one uncontended lock.
    used: Mutex<UsedBox>,
    /// Whether any stored formula owns a legacy (Ctrl+Shift+Enter) array covering more than one
    /// cell. A commit's array guard walks the store only when one does, so this is the difference
    /// between an O(1) edit and an O(sheet) one.
    ///
    /// It is one-sided on purpose: every write of such a formula raises it, and nothing but a full
    /// walk of the store (the guard's own, or `rebuild`) may lower it. Shearing it down costs a
    /// walk nobody needs; shearing it up wrongly lets an edit overwrite part of an array, which the
    /// guard exists to refuse. A store that once held an array and no longer does takes the slow
    /// path.
    array_anchors: bool,
    /// Whether a formula has ever been written into this store; see [`CellStore::has_formulas`].
    ///
    /// One-sided like `array_anchors`, and for the same reason. The walk `has_formulas` makes
    /// costs 1.3 ms over a million stored rows, and a data import pays it once per bulk command
    /// into a workbook that holds no formula at all. Every write that can put one in raises this
    /// and nothing lowers it: a store that has held a formula takes the walk, which is the only
    /// exact answer (a row insert can leave a stored formula logically unreachable, so a counter
    /// that removal keeps in step would need to see every such path).
    wrote_formula: bool,
}

impl Clone for CellStore {
    /// A copy shares the bands; where the next cell would go is not part of what a store is, so the
    /// copy starts afresh and the first row it takes settles it.
    fn clone(&self) -> Self {
        CellStore {
            bands: self.bands.clone(),
            pages: self.pages.clone(),
            page_file: self.page_file.clone(),
            row_map: self.row_map.clone(),
            col_map: self.col_map.clone(),
            len: self.len,
            building: None,
            used: Mutex::new(self.used_box()),
            array_anchors: self.array_anchors,
            wrote_formula: self.wrote_formula,
        }
    }
}

/// Whether a cell's formula owns a legacy array covering more than one cell, so that an edit of
/// the other cells it covers has to be refused. The store index of that fact; keep the two in
/// step (a store only ever raises it).
fn cell_has_array(cell: &Cell) -> bool {
    cell.formula.as_ref().and_then(|f| f.array.as_deref()).is_some_and(|r| !r.is_single())
}

impl PartialEq for CellStore {
    fn eq(&self, other: &Self) -> bool {
        if self.len != other.len {
            return false;
        }
        if self.row_map != other.row_map || self.col_map != other.col_map {
            // A structural edit changes where a cell lives, not what it is: compare the logical
            // walk, which applies each map. A saved-and-reopened sheet has an identity map, so this
            // is the path that holds it equal to the mapped sheet it came from.
            let mut mine = self.iter();
            let mut theirs = other.iter();
            loop {
                match (mine.next(), theirs.next()) {
                    (None, None) => break,
                    (Some((a_at, a)), Some((b_at, b))) if a_at == b_at && same_cell(&a, &b) => {}
                    _ => return false,
                }
            }
            return true;
        }
        self.band_count() == other.band_count()
            && self.bands.keys().chain(self.pages.keys()).eq(other.bands.keys().chain(other.pages.keys()))
            && self.bands.keys().chain(self.pages.keys()).all(|k| match (self.band_arc(*k), other.band_arc(*k)) {
                (Some(a), Some(b)) => Arc::ptr_eq(&a, &b) || a.same_as(&b),
                _ => false,
            })
    }
}

/// Where a walk over one packed row is: the column it will look at next, the last column the walk
/// was asked for, and the index of the next stored cell. Plain numbers, so a walker can own the
/// band its row lives in and still keep its place across `next()` calls.
#[derive(Clone, Copy, Debug)]
struct PackCursor {
    col: u32,
    hi: u32,
    i: usize,
}

impl PackCursor {
    /// The cursor for the physical columns `c0..=c1` of `cells`, clipped to what the row holds.
    fn new(band: &Band, cells: &BandRow, c0: u32, c1: u32) -> PackCursor {
        let (lo, hi) = match cells.column_span(band) {
            Some(_) => (c0.max(cells.first), c1.min(cells.first.saturating_add(cells.columns()).saturating_sub(1))),
            None => (1, 0),
        };
        let col = lo.max(cells.first);
        let i = cells.rank_before(band, col);
        PackCursor { col, hi, i }
    }

    /// [`PackCursor::next`] for a read that wants the value and the formula flag alone: no
    /// [`Cell`] is built, so no formula `Arc` is cloned and no style is resolved.
    fn next_value(&mut self, band: &Band, cells: &BandRow, row: u32) -> Option<(CellRef, Value, bool)> {
        while self.col <= self.hi && self.i < cells.cells as usize {
            let slot = self.col - cells.first;
            let word = (slot / 64) as usize;
            let w = *cells.bits(band).get(word)?;
            let masked = w & (!0u64 << (slot % 64));
            if masked == 0 {
                self.col = cells.first + (word as u32 + 1) * 64;
                continue;
            }
            let col = cells.first + word as u32 * 64 + masked.trailing_zeros();
            if col > self.hi {
                return None;
            }
            let value = band.value_at(cells, self.i, col);
            let formula = cells.formula_at(col).is_some();
            self.col = col + 1;
            self.i += 1;
            return Some((CellRef::new(row, col), value, formula));
        }
        None
    }

    /// The next stored cell at or after the cursor, moving past it.
    fn next(&mut self, band: &Band, cells: &BandRow, row: u32) -> Option<(CellRef, Cell)> {
        while self.col <= self.hi && self.i < cells.cells as usize {
            let slot = self.col - cells.first;
            let word = (slot / 64) as usize;
            let w = *cells.bits(band).get(word)?;
            let masked = w & (!0u64 << (slot % 64));
            if masked == 0 {
                self.col = cells.first + (word as u32 + 1) * 64;
                continue;
            }
            let col = cells.first + word as u32 * 64 + masked.trailing_zeros();
            if col > self.hi {
                return None;
            }
            let cell = band.cell_at(cells, self.i, col);
            self.col = col + 1;
            self.i += 1;
            return Some((CellRef::new(row, col), cell));
        }
        None
    }
}

/// Walks one packed row, in column order, without allocating.
struct PackedRow<'a> {
    band: &'a Band,
    row: u32,
    cells: &'a BandRow,
    at: PackCursor,
}

impl Iterator for PackedRow<'_> {
    type Item = (CellRef, Cell);
    fn next(&mut self) -> Option<(CellRef, Cell)> {
        self.at.next(self.band, self.cells, self.row)
    }
}

fn packed_row<'a>(band: &'a Band, row: u32, cells: &'a BandRow, c0: u32, c1: u32) -> PackedRow<'a> {
    PackedRow { band, row, cells, at: PackCursor::new(band, cells, c0, c1) }
}

/// Walks a row range over live and paged bands one band at a time, so a whole-column read does not
/// collect the sheet (or decode every page at once). A band's cells are gathered into a small
/// iterator before the next band is loaded.
struct CellWalk<'a> {
    store: &'a CellStore,
    segments: std::vec::IntoIter<RowSegment>,
    segment: Option<RowSegment>,
    keys: std::vec::IntoIter<u32>,
    col_segments: Vec<ColSegment>,
    cells: std::vec::IntoIter<(CellRef, Cell)>,
    r: RangeRef,
}

impl<'a> CellWalk<'a> {
    fn new(store: &'a CellStore, r: RangeRef) -> CellWalk<'a> {
        CellWalk {
            store,
            segments: store.row_map.segments_in(r.start.row, r.end.row).into_iter(),
            segment: None,
            keys: Vec::new().into_iter(),
            col_segments: store.col_map.segments_in(r.start.col, r.end.col),
            cells: Vec::new().into_iter(),
            r,
        }
    }
}

impl Iterator for CellWalk<'_> {
    type Item = (CellRef, Cell);

    fn next(&mut self) -> Option<(CellRef, Cell)> {
        loop {
            if let Some(cell) = self.cells.next() {
                return Some(cell);
            }
            if let Some(key) = self.keys.next() {
                let Some(segment) = self.segment else { continue };
                let Some(band) = self.store.band_arc(key) else { continue };
                let physical_start = segment.physical;
                let physical_end = segment.physical.saturating_add(segment.len);
                let mut cells = Vec::new();
                for row in &band.rows {
                    let physical = key * BAND + row.key;
                    if physical < physical_start || physical >= physical_end {
                        continue;
                    }
                    let logical = segment.logical.saturating_add(physical - physical_start);
                    if logical < self.r.start.row || logical > self.r.end.row {
                        continue;
                    }
                    for columns in &self.col_segments {
                        let physical_first = columns.physical;
                        let physical_last = columns.physical.saturating_add(columns.len).saturating_sub(1);
                        if physical_first > physical_last {
                            continue;
                        }
                        for (at, cell) in packed_row(&band, logical, row, physical_first, physical_last) {
                            let col = columns.logical.saturating_add(at.col - physical_first);
                            if col >= self.r.start.col && col <= self.r.end.col {
                                cells.push((CellRef::new(logical, col), cell));
                            }
                        }
                    }
                }
                self.cells = cells.into_iter();
                continue;
            }
            let segment = self.segments.next()?;
            if segment.len == 0 {
                continue;
            }
            self.keys = self.store.bands_in(segment.physical, segment.physical.saturating_add(segment.len)).into_iter();
            self.segment = Some(segment);
        }
    }
}

/// Walks a row range like [`CellWalk`], one band at a time and one packed row at a time, reading
/// paged bands **without caching them**. The band is the only page it holds: its rows are followed
/// through a [`PackCursor`], so a band with a 16,384-column row costs a cursor, not a `Vec` of its
/// cells, and a corrupt page is a skipped band, never a panic. This is the scan a save or an export
/// wants; [`CellWalk`] stays the read path, because a viewport reads its rows again.
struct ScanWalk<'a> {
    store: &'a CellStore,
    r: RangeRef,
    segments: std::vec::IntoIter<RowSegment>,
    segment: Option<RowSegment>,
    keys: std::vec::IntoIter<u32>,
    col_segments: Vec<ColSegment>,
    /// The one band being walked; paged bands are decoded into it and dropped with the next one.
    band: Option<Arc<Band>>,
    /// The key `band` sits at, for the physical row its rows carry.
    key: u32,
    /// Index into `band.rows` of the row being walked.
    row_ix: usize,
    /// Index into `col_segments` of the run being walked.
    col_ix: usize,
    /// The logical row of `band.rows[row_ix]`.
    logical: u32,
    /// The place inside the current row's packed columns; `None` while a row or run is set up.
    cursor: Option<PackCursor>,
}

impl<'a> ScanWalk<'a> {
    fn new(store: &'a CellStore, r: RangeRef) -> ScanWalk<'a> {
        ScanWalk {
            store,
            r,
            segments: store.row_map.segments_in(r.start.row, r.end.row).into_iter(),
            segment: None,
            keys: Vec::new().into_iter(),
            col_segments: store.col_map.segments_in(r.start.col, r.end.col),
            band: None,
            key: 0,
            row_ix: 0,
            col_ix: 0,
            logical: 0,
            cursor: None,
        }
    }

    /// Loads the next band that holds data in the current row run, decoding it if it is paged.
    /// False when the walk is over. A band that cannot be decoded is skipped as if it were not
    /// there, exactly as a corrupt page reads as missing on the cached path.
    fn next_band(&mut self) -> bool {
        loop {
            let key = match self.keys.next() {
                Some(key) => key,
                None => {
                    let Some(segment) = self.next_segment() else { return false };
                    self.keys = self.store.bands_in(segment.physical, segment.physical_end()).into_iter();
                    self.segment = Some(segment);
                    continue;
                }
            };
            let Some(band) = self.store.scan_band(key) else { continue };
            self.band = Some(band);
            self.key = key;
            self.row_ix = 0;
            self.col_ix = 0;
            self.logical = 0;
            self.cursor = None;
            return true;
        }
    }

    fn next_segment(&mut self) -> Option<RowSegment> {
        loop {
            let segment = self.segments.next()?;
            if segment.len > 0 {
                return Some(segment);
            }
        }
    }

    /// Moves `row_ix` to the next row of `band` that the row map puts inside the range, in band
    /// order, and sets its logical row. False when the band holds no such row any more.
    fn begin_row(&mut self, band: &Band) -> bool {
        let Some(segment) = self.segment else { return false };
        let start = segment.physical;
        let end = segment.physical_end();
        while let Some(cells) = band.rows.get(self.row_ix) {
            let physical = self.key.saturating_mul(BAND).saturating_add(cells.key);
            if physical < start || physical >= end {
                self.row_ix += 1;
                continue;
            }
            let logical = segment.logical.saturating_add(physical - start);
            if logical < self.r.start.row || logical > self.r.end.row {
                self.row_ix += 1;
                continue;
            }
            self.logical = logical;
            self.col_ix = 0;
            self.cursor = None;
            return true;
        }
        false
    }

    /// Sets the cursor to the column run at `col_ix`, skipping the empty ones. False when the row
    /// has no run left.
    fn begin_run(&mut self, band: &Band, cells: &BandRow) -> bool {
        while let Some(columns) = self.col_segments.get(self.col_ix) {
            let first = columns.physical;
            let last = columns.physical.saturating_add(columns.len).saturating_sub(1);
            if first <= last {
                self.cursor = Some(PackCursor::new(band, cells, first, last));
                return true;
            }
            self.col_ix += 1;
        }
        false
    }
}

impl Iterator for ScanWalk<'_> {
    type Item = (CellRef, Cell);

    fn next(&mut self) -> Option<(CellRef, Cell)> {
        if self.col_segments.is_empty() {
            return None;
        }
        'band: loop {
            let Some(band) = self.band.clone() else {
                if !self.next_band() {
                    return None;
                }
                continue 'band;
            };
            loop {
                if self.cursor.is_none() {
                    if self.col_ix == 0 && !self.begin_row(&band) {
                        self.band = None;
                        continue 'band;
                    }
                    let Some(cells) = band.rows.get(self.row_ix) else {
                        self.band = None;
                        continue 'band;
                    };
                    if !self.begin_run(&band, cells) {
                        self.row_ix += 1;
                        self.col_ix = 0;
                        continue;
                    }
                }
                let Some(cells) = band.rows.get(self.row_ix) else {
                    self.band = None;
                    continue 'band;
                };
                let Some(columns) = self.col_segments.get(self.col_ix).copied() else {
                    self.cursor = None;
                    continue;
                };
                let Some(cursor) = self.cursor.as_mut() else { continue };
                if let Some((at, cell)) = cursor.next(&band, cells, self.logical) {
                    let col = columns.logical.saturating_add(at.col.saturating_sub(columns.physical));
                    if col >= self.r.start.col && col <= self.r.end.col {
                        return Some((CellRef::new(self.logical, col), cell));
                    }
                    continue;
                }
                // The run is walked out: the next column run of this row, or the next row.
                self.cursor = None;
                self.col_ix += 1;
                if self.col_ix >= self.col_segments.len() {
                    self.row_ix += 1;
                    self.col_ix = 0;
                }
            }
        }
    }
}

/// Whether writing `values` into `band` would change anything it holds: the compare half of a
/// compare-then-write. Reads only, so a worker can ask before it copies the band; the first
/// difference ends the scan, so a band that does change pays one extra lookup, not a second pass.
/// A value that lands on no stored cell is not a change either: a value write has nowhere to put
/// it, exactly as [`Band::set_value`] refuses it. `at` maps a batch cell to its packed position
/// (row key, column): a mapped store's logical rows are not its physical ones.
fn band_slice_changes(band: &Band, values: &[(CellRef, Value)], at: impl Fn(&CellRef) -> Option<(u32, u32)>) -> bool {
    values.iter().any(|(c, v)| match at(c) {
        Some((row, col)) => band.value_is(row, col, v) == Some(false),
        None => false,
    })
}

/// What one value batch did to the bands it covers: how many it changed and how many it left
/// alone because every value was already theirs. `CELLA_PROFILE` prints it (one line a batch), so
/// the compare-then-write's effect on a recalculation's write phase is visible without a profiler.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct WrittenBands {
    changed: usize,
    unchanged: usize,
}

/// One run of a value batch that lands in one band: `values[start..end]` all map to band `key`,
/// and `base` is the physical row of `values[start]`. A band can be fed by several runs when the
/// row map has a gap inside it, so a plan merges them into one task.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug)]
struct BandRun {
    key: u32,
    base: u32,
    start: usize,
    end: usize,
}

impl CellStore {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The live band, or the decoded copy of its page. The `Arc` is returned so a caller can keep
    /// it alive while it walks the band after the store borrow ends.
    fn band_arc(&self, key: u32) -> Option<Arc<Band>> {
        if let Some(band) = self.bands.get(&key) {
            return Some(band.clone());
        }
        let page = self.pages.get(&key)?;
        self.load_page(key, page)
    }

    /// The band at `key` as a comparison can read it: the live band, the decoded page, or `None`
    /// when this store holds nothing there. The outer `Option` is "this store can answer": a page
    /// that cannot be decoded makes the answer unknown, not empty, and the caller says so instead
    /// of treating a band it could not read as a band with nothing in it.
    ///
    /// A page whose header says it holds no formula reads as no band at all: its rows would
    /// contribute nothing to a formula comparison, so decoding it would only cost a read.
    fn comparable_band(&self, key: u32) -> Option<Option<Arc<Band>>> {
        if let Some(band) = self.bands.get(&key) {
            return Some(Some(band.clone()));
        }
        let Some(page) = self.pages.get(&key) else { return Some(None) };
        if !page.has_formulas() {
            return Some(None);
        }
        self.band_arc(key).map(Some)
    }

    /// Whether the band at `key` holds a formula — answered from a page's header without decoding
    /// it, so a snapshot that keeps its bands as pages can be asked cheaply.
    fn band_has_formulas(&self, key: u32) -> bool {
        if let Some(band) = self.bands.get(&key) {
            return band.rows.iter().any(|row| !row.formulas().is_empty());
        }
        self.pages.get(&key).is_some_and(|page| page.has_formulas())
    }

    /// The band at `key` for a scan: a live band as it is, or a paged band decoded straight from
    /// the page file. Nothing is installed in the cache and nothing is charged to its budget, so a
    /// full-sheet scan neither evicts the working set the viewport reads nor leaves the sheet's
    /// tail resident. A page that cannot be read is missing, exactly as it is on the cached path.
    fn scan_band(&self, key: u32) -> Option<Arc<Band>> {
        if let Some(band) = self.bands.get(&key) {
            return Some(band.clone());
        }
        let page = self.pages.get(&key)?;
        let file = self.page_file.as_deref()?;
        page.decode(file)
    }

    /// Decodes a paged band through the page cache: a fresh decode pays its heap bytes into the
    /// budget, a cached band is a hit, and a page that cannot be read is counted as corrupt. Only
    /// the call that installed the band gets a byte count back from [`page::Page::load`], so a band
    /// that several stores read at once is charged once.
    fn load_page(&self, key: u32, page: &Arc<page::Page>) -> Option<Arc<Band>> {
        let file = self.page_file.as_deref()?;
        let (band, decoded) = page.load(file);
        match (&band, decoded) {
            (Some(_), Some(bytes)) => {
                file.cache().miss();
                file.cache().note(key, page, bytes);
            }
            (Some(_), None) => file.cache().hit(),
            (None, _) => file.cache().corrupt(),
        }
        band
    }

    /// The band keys that hold data in a physical row range. Unlike a range of band numbers, this
    /// only walks bands the store actually has, so iterating a small sheet does not probe 16,384
    /// empty bands (or a million empty logical rows).
    fn bands_in(&self, physical_start: u32, physical_end: u32) -> Vec<u32> {
        let first = physical_start / BAND;
        let last = physical_end.saturating_sub(1) / BAND;
        if physical_start >= physical_end {
            return Vec::new();
        }
        let mut keys: Vec<u32> = self.bands.range(first..=last).map(|(k, _)| *k).collect();
        keys.extend(self.pages.range(first..=last).map(|(k, _)| *k));
        keys.sort_unstable();
        keys.dedup();
        keys
    }

    /// The physical row a logical row lives on, or `None` when the row is a gap made by an insert
    /// or a delete.
    fn physical_row(&self, logical: u32) -> Option<u32> {
        self.row_map.map(logical)
    }

    /// The physical row for a write: an existing mapping, or a fresh row for a logical row in an
    /// inserted gap. A failed allocation (the physical key space is exhausted) refuses the write.
    fn physical_row_for_write(&mut self, logical: u32) -> Option<u32> {
        if let Some(physical) = self.row_map.map(logical) {
            return Some(physical);
        }
        self.row_map.allocate(logical)
    }

    fn physical_col(&self, logical: u32) -> Option<u32> {
        self.col_map.map(logical)
    }

    fn physical_col_for_write(&mut self, logical: u32) -> Option<u32> {
        if let Some(physical) = self.col_map.map(logical) {
            return Some(physical);
        }
        self.col_map.allocate(logical)
    }

    /// The rightmost physical column that holds a cell, used to build just the existing width of
    /// the column map instead of a full 16,384-column identity run.
    fn physical_max_col(&self) -> Option<u32> {
        let mut max = None;
        for band in self.bands.values() {
            for row in &band.rows {
                if let Some((_, last)) = row.column_span(band) {
                    max = Some(max.map_or(last, |m: u32| m.max(last)));
                }
            }
        }
        for page in self.pages.values() {
            if let Some((_, _, _, last)) = page.bounds() {
                max = Some(max.map_or(last, |m: u32| m.max(last)));
            }
        }
        max
    }

    fn ensure_col_map(&mut self) {
        if self.col_map.is_identity() {
            self.col_map.ensure_with(self.physical_max_col().unwrap_or(0));
        }
    }

    /// The logical columns covered by a physical column span, if any. Used to keep `used_range`
    /// logical after a column edit without loading pages or walking every column.
    fn logical_col_span(&self, physical_first: u32, physical_last: u32) -> Option<(u32, u32)> {
        if self.col_map.is_identity() {
            return (physical_first < cella_core::MAX_COLS).then(|| (physical_first, physical_last.min(cella_core::MAX_COLS - 1)));
        }
        let mut lo = u32::MAX;
        let mut hi = 0u32;
        for segment in &self.col_map.segments {
            let a = segment.physical.max(physical_first);
            let b = segment.physical_end().min(physical_last.saturating_add(1));
            if a >= b {
                continue;
            }
            let logical_a = segment.logical.saturating_add(a - segment.physical);
            let logical_b = segment.logical.saturating_add(b - 1 - segment.physical);
            lo = lo.min(logical_a);
            hi = hi.max(logical_b);
        }
        (lo != u32::MAX).then_some((lo, hi))
    }

    /// Bands held in memory, and bands on the page file.
    pub fn paged_band_count(&self) -> usize {
        self.pages.len()
    }

    /// What the page cache of this store's page file has done: its byte budget, the bytes it holds,
    /// and its counters. `None` when the store does not page.
    ///
    /// `bytes` is what is charged to decoded bands the cache installed; a band can outlive its
    /// charge (a caller, or an undo snapshot, holding the `Arc`), so the figure is the budget's
    /// measure, not a heap census of the process. `entries` counts the queue's entries, including
    /// the ones whose page has been dropped and whose bytes are subtracted the next time the queue
    /// is trimmed to the budget.
    pub fn page_cache_stats(&self) -> Option<PageCacheStats> {
        self.page_file.as_ref().map(|file| file.cache().stats())
    }

    /// Where this store's page file was created, or `None` when it does not page.
    ///
    /// On Unix and Windows the file has no name of its own once it is open (it is unlinked at
    /// creation, or delete-on-close), so this reports the place the spill was made in rather than
    /// a file a caller can open: a diagnostic for a memory or disk report, not an input to one.
    pub fn page_file_path(&self) -> Option<&std::path::Path> {
        self.page_file.as_deref().map(page::PageFile::path)
    }

    /// Turns paging on for this store: every completed band already in it is written to a fresh
    /// page file, and later `insert_batch` calls spill the bands they add. Called by the reader
    /// before it fills a sheet, so a large open keeps pages instead of every band.
    ///
    /// The file goes in the system temp directory; [`enable_paging_with`](Self::enable_paging_with)
    /// is the version that also says where else it may live. The decoded-band cache is created
    /// with the file and takes its budget from `CELLA_PAGE_CACHE_MB` (clamped to 8..=512 MiB,
    /// default 64 MiB). The budget belongs to the file, so clones and undo snapshots share it and a
    /// later `enable_paging` keeps it.
    ///
    /// Returns false when no spill file can be created; the store stays ordinary.
    pub fn enable_paging(&mut self) -> bool {
        self.enable_paging_with(SpillOptions::default())
    }

    /// [`enable_paging`](Self::enable_paging) with an explicit cache budget in bytes. Tests pin the
    /// budget this way: the environment is process-global and the model tests run in parallel. A
    /// store that already pages keeps the budget it was created with.
    pub fn enable_paging_with_cache(&mut self, cache_bytes: usize) -> bool {
        self.enable_paging_with(SpillOptions { cache_bytes: Some(cache_bytes), ..SpillOptions::default() })
    }

    /// [`enable_paging`](Self::enable_paging) with everything pinned: the directory the file is
    /// tried in first (the document's folder), the stem its name is built from and the cache
    /// budget. The temp directory is the fallback; see [`SpillOptions`] for the policy, the name
    /// and how the file's lifetime is bounded.
    ///
    /// Returns false when no spill file can be created, and the store stays live.
    pub fn enable_paging_with(&mut self, options: SpillOptions<'_>) -> bool {
        if self.page_file.is_some() {
            // The budget and the place belong to the file the store already shares with its clones.
            return true;
        }
        let cache_bytes = options.cache_bytes.unwrap_or_else(page::default_cache_bytes);
        let file = match options.temp {
            Some(temp) => page::PageFile::new_in(options.dir, options.name, cache_bytes, temp),
            None => page::PageFile::new(options.dir, options.name, cache_bytes),
        };
        let Some(file) = file else { return false };
        self.page_file = Some(Arc::new(file));
        self.spill_pages();
        true
    }

    /// Spills every live band that can be encoded. A band that fails to encode (or a page that
    /// cannot be written) stays live; paging never loses cells.
    pub fn spill_pages(&mut self) {
        let keys: Vec<u32> = self.bands.keys().copied().collect();
        self.spill_bands(&keys);
    }

    fn spill_band(&mut self, key: u32) {
        self.spill_bands(&[key]);
    }

    /// Encodes `keys` and moves them onto the page file, a chunk at a time. A band is independent of
    /// every other — its numbers, texts and stripes are its own — so a chunk large enough to be
    /// worth it is encoded on several threads, and the pages are written and the bands dropped on
    /// this one once the workers are done. The chunk bounds what is buffered at once, so spilling a
    /// million-row snapshot never holds the whole sheet encoded in memory, and each band leaves the
    /// live map as its page is written.
    ///
    /// A band that cannot be encoded, a worker that panics and a file that refuses a write all leave
    /// the band live: paging never loses cells.
    fn spill_bands(&mut self, keys: &[u32]) {
        let Some(file) = self.page_file.clone() else { return };
        // A file that has already failed a write is not tried again: every later band would hit
        // the same failing syscall. The bands stay live and the pages already written stay
        // readable, so a full disk or a dropped share costs memory, never cells.
        if file.has_failed() {
            return;
        }
        for chunk in keys.chunks(SPILL_CHUNK) {
            let encoded = self.encode_chunk(chunk);
            let parts: Vec<&[u8]> = encoded.iter().map(|(_, bytes, ..)| bytes.as_slice()).collect();
            let at = match file.write_packed(&parts) {
                Ok(at) => at,
                Err(failure) => {
                    file.note_failure(failure);
                    return;
                }
            };
            for ((key, _, bounds, formulas, owned), at) in encoded.into_iter().zip(at) {
                self.bands.remove(&key);
                self.pages.insert(key, Arc::new(page::Page::new(at, bounds, formulas, owned)));
            }
            if file.has_failed() {
                return;
            }
        }
    }

    /// One chunk of [`spill_bands`](Self::spill_bands): the bands encoded, with the bounds and the
    /// flags their page carries. A chunk that is large enough is spread over the cores; below that
    /// the threads cost more than they save.
    fn encode_chunk(&self, keys: &[u32]) -> Vec<page::Spilled> {
        let bands = &self.bands;
        if keys.len() < SPILL_PARALLEL_MIN {
            return page::encode_spilled(bands, keys);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(SPILL_MAX_THREADS);
            if threads > 1 {
                let parts = keys.len().div_ceil(threads);
                let encoded: Vec<std::thread::Result<Vec<page::Spilled>>> = std::thread::scope(|scope| {
                    let mut handles = Vec::new();
                    for part in keys.chunks(parts) {
                        handles.push(scope.spawn(move || page::encode_spilled(bands, part)));
                    }
                    handles.into_iter().map(|h| h.join()).collect()
                });
                let mut out = Vec::with_capacity(keys.len());
                let mut panicked = false;
                for part in encoded {
                    match part {
                        Ok(part) => out.extend(part),
                        // A worker panicked: nothing has been written for this chunk yet, so the
                        // whole chunk is encoded again on this thread.
                        Err(_) => panicked = true,
                    }
                }
                if !panicked {
                    return out;
                }
            }
        }
        page::encode_spilled(bands, keys)
    }

    /// Loads a paged band back into the live map, so a write can modify it. A corrupt page stays
    /// paged and the write is refused rather than replacing data with an empty band. The band keeps
    /// the page's bounds, so the cached used box is unchanged either way.
    fn materialize(&mut self, key: u32) -> Option<Arc<Band>> {
        if let Some(band) = self.bands.get(&key) {
            return Some(band.clone());
        }
        let page = self.pages.get(&key)?.clone();
        let band = self.load_page(key, &page)?;
        self.pages.remove(&key);
        self.bands.insert(key, band.clone());
        Some(band)
    }

    /// Compressed bytes the page file holds: the sum of the deflated page lengths, i.e. what the
    /// spill costs on disk. The decoded bands cost memory instead, and that is what
    /// [`page_cache_stats`](Self::page_cache_stats) reports.
    pub fn paged_bytes(&self) -> usize {
        self.pages.values().map(|p| p.page_len()).sum()
    }

    /// How many bands this store keeps that `newer` no longer shares: the unit a compressed history
    /// step is priced in before it is encoded (see [`spill_retained_against`](Self::spill_retained_against)),
    /// so a caller can refuse an encode it cannot afford instead of paying for it and dropping the
    /// step anyway.
    pub fn retained_band_count(&self, newer: &CellStore) -> usize {
        self.bands.iter().filter(|(key, band)| newer.bands.get(key).is_none_or(|next| !Arc::ptr_eq(next, band))).count()
    }

    /// Moves the bands this store keeps that `newer` no longer shares into a page file of its own,
    /// so a history snapshot costs its compressed pages instead of the decoded bands the edit
    /// replaced.
    ///
    /// A band `newer` still shares is left live: the workbook reads that same copy, and encoding
    /// it here would write a second while freeing nothing. A store that already pages keeps its
    /// file and this does nothing (returns 0): its pages belong to that file, and the bands it
    /// keeps are the few an edit wrote. Returns the bands it spilled; 0 means the store was left
    /// exactly as it was — nothing to spill, or no page file could be created. Paging never loses
    /// cells: a band that cannot be encoded stays live.
    ///
    /// The file keeps the default decoded-band cache ([PAGE_CACHE_BYTES]): an undo restores the
    /// snapshot *as it is*, pages and all, so the workbook it hands back reads those pages the way
    /// any paged store does — a band at a time, through the budget — and the bands it does not read
    /// never come back.
    pub fn spill_retained_against(&mut self, newer: &CellStore) -> usize {
        self.spill_retained_within(newer, usize::MAX)
    }

    /// [`spill_retained_against`](Self::spill_retained_against) with a target: spill only as many of
    /// the bands `newer` does not share as it takes to bring what this store retains to `target`
    /// bytes. The newest history step is the case this exists for — a snapshot that costs more than
    /// the whole byte budget has to give some of its bands up, and giving all of them up is a
    /// whole-sheet encode paid inside the edit that pushed it. The bands are taken in key order, so
    /// a prefix of them is a prefix of the sheet.
    ///
    /// The target is a target, not a guarantee: bands are not all the same size and a band that
    /// cannot be encoded stays live.
    pub fn spill_retained_within(&mut self, newer: &CellStore, target: usize) -> usize {
        if self.page_file.is_some() {
            return 0;
        }
        let keys: Vec<u32> =
            self.bands.iter().filter(|(key, band)| newer.bands.get(key).is_none_or(|next| !Arc::ptr_eq(next, band))).map(|(key, _)| *key).collect();
        if keys.is_empty() {
            return 0;
        }
        // The temp directory only: a snapshot's pages must not appear in the document's folder.
        let Some(file) = page::PageFile::new(None, Some("undo"), page::default_cache_bytes()) else { return 0 };
        self.page_file = Some(Arc::new(file));
        let before = self.pages.len();
        if target == usize::MAX {
            self.spill_bands(&keys);
        } else {
            // A chunk at a time, counting what each band actually gave up: what it cost the snapshot
            // less what its page now costs. Counting only the band would overstate it by the page
            // and stop well short of the target.
            let needed = self.retained_bytes_against(newer).saturating_sub(target);
            let mut freed = 0usize;
            let mut taken = 0usize;
            while taken < keys.len() && freed < needed {
                let end = taken.saturating_add(SPILL_CHUNK).min(keys.len());
                let Some(chunk) = keys.get(taken..end) else { break };
                let costs: Vec<usize> =
                    chunk.iter().map(|key| self.bands.get(key).map_or(0, |band| Self::unshared_band_bytes(band, newer.bands.get(key)))).collect();
                self.spill_bands(chunk);
                for (key, cost) in chunk.iter().zip(costs) {
                    if let Some(page) = self.pages.get(key) {
                        freed = freed.saturating_add(cost.saturating_sub(page.page_len()));
                    }
                }
                taken = end;
            }
        }
        let spilled = self.pages.len().saturating_sub(before);
        if spilled == 0 {
            // Nothing could be encoded: leave no empty file behind.
            self.page_file = None;
        }
        spilled
    }

    /// What one band costs a snapshot that keeps it: its stripes, its row headers, the side tables
    /// `newer` does not share and its texts — the terms [`retained_bytes_against`](Self::retained_bytes_against)
    /// sums, for one band.
    fn unshared_band_bytes(band: &Band, next: Option<&Arc<Band>>) -> usize {
        let mut bytes = band.unshared_fixed_bytes();
        for row in &band.rows {
            // A value edit copies the band but shares each row's extras with the newer store; those
            // formulas and arrays are live anyway, not storage the snapshot keeps.
            let shared = next.and_then(|band| band.row(row.key)).is_some_and(|other| match (&row.extras, &other.extras) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            });
            if !shared {
                bytes = bytes.saturating_add(Band::extras_bytes(row));
            }
        }
        bytes
    }

    /// Decodes every page of this store back into a live band and drops the page file. This is
    /// what the undo that restores a compressed snapshot does with it: the workbook then prints,
    /// iterates and diffs itself band by band, exactly as the one the edit replaced did.
    ///
    /// Returns the bands decoded. A page that cannot be decoded stays paged — it reads as missing
    /// there, as it does everywhere else — and the file is kept, so nothing is lost by trying. A
    /// store that does not page returns 0 without touching anything.
    pub fn expand_pages(&mut self) -> usize {
        let keys: Vec<u32> = self.pages.keys().copied().collect();
        let mut decoded = 0usize;
        for key in keys {
            if self.materialize(key).is_some() {
                decoded = decoded.saturating_add(1);
            }
        }
        if self.pages.is_empty() {
            self.page_file = None;
        }
        decoded
    }

    /// Reads one cell. The value is rebuilt rather than borrowed: cells live in packed pages, so
    /// there is no `Cell` to point at. A text cell shares its string, so the copy is a pointer
    /// bump, not a string copy.
    pub fn get(&self, c: CellRef) -> Option<Cell> {
        self.read_cell(c, Band::cell_at)
    }

    /// Reads a stored cell through `f`. A live band is borrowed without cloning its `Arc`;
    /// a paged band is read through the page cache. Returns `None` when the cell is absent.
    fn read_cell<R>(&self, c: CellRef, f: impl FnOnce(&Band, &BandRow, usize, u32) -> R) -> Option<R> {
        let physical = self.physical_row(c.row)?;
        let col = self.physical_col(c.col)?;
        let key = physical / BAND;
        if let Some(band) = self.bands.get(&key) {
            let row = band.row(physical % BAND)?;
            let i = row.index_of(band, col)?;
            return Some(f(band, row, i, col));
        }
        let band = self.band_arc(key)?;
        let row = band.row(physical % BAND)?;
        let i = row.index_of(&band, col)?;
        Some(f(&band, row, i, col))
    }

    /// The value stored at `c`, without rebuilding a `Cell` or cloning its formula. Like
    /// `get`, this borrows live bands. Returns `Value::Empty` where nothing is stored.
    pub fn value(&self, c: CellRef) -> Value {
        self.read_cell(c, Band::value_at).unwrap_or(Value::Empty)
    }

    /// The value stored at `c`, keeping "nothing stored" apart from a stored empty value (a
    /// formatted cell, or a formula whose result is empty).
    pub fn value_ref(&self, c: CellRef) -> Option<Value> {
        self.read_cell(c, Band::value_at)
    }

    /// The value stored at `c` together with whether a formula sits on it: what a display read
    /// needs to tell a cell's own (empty) answer from a spilled value under it, without cloning
    /// either the formula or the band.
    pub fn value_with_formula(&self, c: CellRef) -> Option<(Value, bool)> {
        self.read_cell(c, |band, row, i, col| (band.value_at(row, i, col), row.formula_at(col).is_some()))
    }

    /// The formula stored at `c`, without rebuilding the cell's value or style. Like `get`,
    /// this borrows live bands and uses the page cache for paged bands.
    pub fn formula(&self, c: CellRef) -> Option<Arc<Formula>> {
        self.read_cell(c, |_, row, _, col| row.formula_at(col).cloned()).flatten()
    }

    /// The style of a stored cell, without reconstructing the `Cell` (and its value and formula).
    /// A style is all a formatting or duplicate check needs; `get` rebuilds the rest for nothing.
    pub fn style(&self, c: CellRef) -> Option<StyleId> {
        let physical = self.physical_row(c.row)?;
        let col = self.physical_col(c.col)?;
        let band = self.band_arc(physical / BAND)?;
        let row = band.row(physical % BAND)?;
        let i = row.index_of(&band, col)?;
        Some(band.style(row.style_at(&band, i)))
    }

    /// True when the position holds a cell at all (a stored style counts).
    pub fn contains(&self, c: CellRef) -> bool {
        let (Some(physical), Some(col)) = (self.physical_row(c.row), self.physical_col(c.col)) else { return false };
        self.band_arc(physical / BAND).is_some_and(|b| b.row(physical % BAND).is_some_and(|r| r.index_of(&b, col).is_some()))
    }

    /// Stores a cell; a default (empty, unstyled) cell is removed instead.
    /// The band the cells being added are going into. Moving to another band gives the one that
    /// was being built back to the allocator: [`Band::finish`] drops the number and text indexes,
    /// which only adding cells needs, and shrinks the stripes. A band built cell by cell used to
    /// keep them for the life of the workbook — a fill over a million rows kept a 1,024-byte number
    /// index per band (16 MB) plus a quarter of its number table as spare capacity (16 MB more).
    fn band_for_write(&mut self, key: u32) {
        if self.building == Some(key) {
            return;
        }
        if let Some(done) = self.building.replace(key) {
            self.finish_band(done);
        }
    }

    /// Gives one finished band back to the allocator when nobody shares it. A band an undo
    /// snapshot also holds is left alone: its stripes are the snapshot's too, and shrinking them
    /// would copy the band for nothing.
    fn finish_band(&mut self, key: u32) {
        if let Some(band) = self.bands.get_mut(&key)
            && let Some(band) = Arc::get_mut(band)
        {
            band.finish();
        }
    }

    /// Stores `cell` at `c`. Returns whether the cell it replaced held a formula — what a bulk
    /// write of values reports so the recalculation can skip a probe a cell (see
    /// `Ctx::formulas_unchanged`); `false` also covers a write that stored nothing.
    pub fn set(&mut self, c: CellRef, cell: Cell) -> bool {
        let row_physical = self.physical_row(c.row);
        let col_physical = self.physical_col(c.col);
        // A blank write into an inserted row/column must not allocate storage for nothing.
        if (row_physical.is_none() || col_physical.is_none()) && cell.is_blank() {
            return false;
        }
        let Some(physical) = (match row_physical {
            Some(physical) => Some(physical),
            None => self.physical_row_for_write(c.row),
        }) else {
            return false;
        };
        let Some(col) = (match col_physical {
            Some(physical) => Some(physical),
            None => self.physical_col_for_write(c.col),
        }) else {
            return false;
        };
        let key = physical / BAND;
        let row = physical % BAND;
        if self.pages.contains_key(&key) && self.materialize(key).is_none() {
            return false;
        }
        self.band_for_write(key);
        if !self.array_anchors {
            self.array_anchors = cell_has_array(&cell);
        }
        if !self.wrote_formula {
            self.wrote_formula = cell.formula.is_some();
        }
        // `put` answers both questions the length bookkeeping asks; two `contains` probes around it
        // cost a band lookup, a row search and a rank each, which is what a paste of six million
        // cells was paying per cell.
        let (had, has, had_formula, empty) = {
            let band = Arc::make_mut(self.bands.entry(key).or_default());
            let (had, has, had_formula) = band.put(row, col, cell);
            (had, has, had_formula, band.rows.is_empty())
        };
        match (had, has) {
            (false, true) => {
                self.len += 1;
                // The only cell that can have entered the box: this one.
                self.grow_used(c.row, c.col);
            }
            (true, false) => {
                self.len -= 1;
                // The box may have been sized by the cell that just went away.
                self.mark_used_stale();
            }
            _ => {}
        }
        if empty {
            self.bands.remove(&key);
        }
        had_formula
    }

    /// Adds bands a [`BandBatch`] built off this thread. Whole bands are moved, not cells: packing
    /// a row needs its band's tables, so a reader that decodes rows on several threads builds them
    /// there and hands them over here. A band the store already holds is replaced.
    pub fn insert_batch(&mut self, batch: BandBatch) {
        let count = |band: &Band| band.rows.iter().map(|row| row.cells as usize).sum::<usize>();
        // A batch adds: each band it leaves behind grows the cached used box as it arrives. A
        // batch that replaced a cell with a blank one can also shrink the box, and only then is
        // the box walked again.
        let mut shrank = false;
        for (key, mut band) in batch.bands {
            // A batch is complete: give back what building it over-allocated.
            band.finish();
            if !self.array_anchors {
                self.array_anchors = band.has_array_anchor();
            }
            if !self.wrote_formula {
                self.wrote_formula = band.rows.iter().any(|row| !row.formulas().is_empty());
            }
            // A chunk boundary can land in a band already spilled by the previous batch: bring it
            // back so the merge can see its rows, then spill the result again.
            if self.pages.contains_key(&key) && self.materialize(key).is_none() {
                continue;
            }
            let Some(existing) = self.bands.get_mut(&key) else {
                self.len += count(&band);
                self.bands.insert(key, Arc::new(band));
                self.spill_band(key);
                self.absorb_used_band(key);
                continue;
            };
            // A band both sides hold: a chunk boundary fell inside it, since chunks are cut at row
            // boundaries and a band is 64 rows. The batch's rows point into its own number and text
            // tables, so they are rebuilt here rather than moved.
            let rows = std::mem::take(&mut band.rows);
            let target = Arc::make_mut(existing);
            for row in rows {
                let before = target.row(row.key).map_or(0, |r| r.cells as usize);
                let mut cells: Vec<(u32, Cell)> = (0..row.cells as usize)
                    .map(|i| {
                        let col = row.col_at(&band, i);
                        (col, band.cell_at(&row, i, col))
                    })
                    .collect();
                if before == 0 && target.row(row.key).is_none_or(BandRow::is_empty) {
                    target.put_row(row.key, &mut cells);
                } else {
                    for (col, cell) in cells {
                        let _ = target.put(row.key, col, cell);
                    }
                }
                let after = target.row(row.key).map_or(0, |r| r.cells as usize);
                self.len += after - before;
                shrank |= after < before;
            }
            self.spill_band(key);
            self.absorb_used_band(key);
        }
        if self.building.is_none() {
            self.building = self.bands.keys().next_back().copied();
        }
        if shrank {
            self.mark_used_stale();
        }
    }

    /// Whether any stored formula owns a legacy array covering more than one cell: the one thing
    /// that makes an edit inside the cells it covers refusable. Answering costs nothing, so a
    /// commit's array guard can ask before it walks the store. See the field for why the answer is
    /// allowed to be stale in one direction only.
    pub fn has_array_anchors(&self) -> bool {
        self.array_anchors
    }

    /// Replaces the value of a cell that is already stored, keeping its formula and style. Returns
    /// false when nothing is stored there, so no cell enters or leaves the store and the cached
    /// used box cannot change: no repair here.
    ///
    /// A recalculation only changes values, but [`set`](Self::set) rebuilds the whole `Cell` (and
    /// takes the store's bands apart) for each one; at 100k results that is most of the write-back.
    pub fn set_value(&mut self, c: CellRef, value: &Value) -> bool {
        let Some(physical) = self.physical_row(c.row) else { return false };
        let Some(col) = self.physical_col(c.col) else { return false };
        let key = physical / BAND;
        if self.pages.contains_key(&key) && self.materialize(key).is_none() {
            return false;
        }
        self.band_for_write(key);
        let Some(band) = self.bands.get(&key) else { return false };
        // Already what it would write: no cell enters or leaves, so there is nothing to copy or
        // repair. True is the answer a write that lands on a stored cell gives.
        if band.value_is(physical % BAND, col, value) == Some(true) {
            return true;
        }
        let Some(band) = self.bands.get_mut(&key) else { return false };
        Arc::make_mut(band).set_value(physical % BAND, col, value)
    }

    /// Whether writing `values` into band `key` would change anything it holds; see
    /// [`band_slice_changes`]. A band the store does not hold is unchanged: none of the values has
    /// a cell to land in.
    fn band_changes(&self, key: u32, values: &[(CellRef, Value)], at: impl Fn(&CellRef) -> Option<(u32, u32)>) -> bool {
        self.bands.get(&key).is_some_and(|band| band_slice_changes(band, values, at))
    }

    /// Replaces the values of cells that already exist — like [`set_value`](Self::set_value), the
    /// stored positions do not move, so the cached used box stays exact. `values` must be sorted in
    /// row-major order (the order a recalculation writes them in), so each band is looked up once.
    ///
    /// A batch that is not in that order is written one value at a time instead: a mapped store's
    /// logical rows are not its physical rows, so a band-grouped walk would put a value on the
    /// wrong row. The check is one pass over the batch, and it stays in release builds: measured
    /// at 0.55–0.68 ms for a million values (a percent of a recalculation's write-back), against
    /// silently writing values on the wrong rows for any caller that gets the order wrong.
    pub fn set_values(&mut self, values: &mut [(CellRef, Value)]) {
        if values.is_empty() {
            return;
        }
        // `CELLA_PROFILE` timing: one clock read per batch — a recalculation's write-back — never
        // per value, and never on wasm, where reading a clock traps (as `cmd::commit`).
        #[allow(clippy::disallowed_methods)]
        let started =
            if cfg!(target_arch = "wasm32") || std::env::var_os("CELLA_PROFILE").is_none() { None } else { Some(std::time::Instant::now()) };
        let count = values.len();
        let outcome = self.write_values(values);
        if let Some(started) = started
            && let Some(outcome) = outcome
        {
            eprintln!(
                "set values: {count} values, {} bands ({} unchanged) {:.1} ms",
                outcome.changed.saturating_add(outcome.unchanged),
                outcome.unchanged,
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
    }

    /// The write [`set_values`](Self::set_values) makes, answering what it did to the bands when the
    /// batch took a path that counts them (`None` for the per-value fallback, which is not a band
    /// walk, and for the mapped parallel one, which counts nothing).
    fn write_values(&mut self, values: &mut [(CellRef, Value)]) -> Option<WrittenBands> {
        if !values.windows(2).all(|w| (w[0].0.row, w[0].0.col) <= (w[1].0.row, w[1].0.col)) {
            for (c, v) in values.iter() {
                self.set_value(*c, v);
            }
            return None;
        }
        if !self.row_map.is_identity() || !self.col_map.is_identity() {
            // The logical batch still arrives in row order, and inside one row segment physical
            // rows are affine in the logical row, so the write streams band by band without an
            // index, a sort, or a per-value map lookup. A mapped sheet is one that has had a
            // structural edit, not the common editing case.
            #[cfg(not(target_arch = "wasm32"))]
            if values.len() >= PARALLEL_WRITE_MIN && values.len() <= PARALLEL_WRITE_MAX_CELLS {
                let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(8);
                if threads > 1 && self.set_values_mapped_parallel(values, threads) {
                    return None;
                }
            }
            return Some(self.set_values_mapped(values));
        }
        // A paged band a result lands in has to come back before either write path looks at the
        // live map. Values are sorted by row, so the touched bands are a short run of keys.
        if !self.pages.is_empty() {
            let mut keys: Vec<u32> = values.iter().map(|(c, _)| c.row / BAND).collect();
            keys.dedup();
            for key in keys {
                if self.pages.contains_key(&key) {
                    self.materialize(key);
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        if values.len() >= PARALLEL_WRITE_MIN && values.len() <= PARALLEL_WRITE_MAX_CELLS {
            let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(8);
            if threads > 1 {
                return Some(self.set_values_parallel(values, threads));
            }
        }
        Some(self.set_values_sequential(values))
    }

    /// The value write on a mapped store: walk the row map's segments in logical order and each
    /// segment's bands in physical order, taking one band borrow and one page materialize per run.
    /// Rows the map does not cover are gaps: a value there has nowhere to go, exactly as
    /// [`set_value`](Self::set_value) finds no cell, so no band is allocated for them.
    fn set_values_mapped(&mut self, values: &[(CellRef, Value)]) -> WrittenBands {
        let mut outcome = WrittenBands::default();
        let (Some(first), Some(last)) = (values.first(), values.last()) else { return outcome };
        let mut i = 0;
        for segment in self.row_map.segments_in(first.0.row, last.0.row) {
            // The values before this segment start on logical rows the map does not cover.
            while i < values.len() && values[i].0.row < segment.logical {
                i += 1;
            }
            let physical = |row: u32| segment.physical.saturating_add(row.saturating_sub(segment.logical));
            let stop = i + values[i..].partition_point(|(c, _)| c.row < segment.logical_end());
            while i < stop {
                let key = physical(values[i].0.row) / BAND;
                let run_end = i + values[i..stop].partition_point(|(c, _)| physical(c.row) / BAND == key);
                // A corrupt page skips the run it would have held, not the batch.
                if self.pages.contains_key(&key) && self.materialize(key).is_none() {
                    i = run_end;
                    continue;
                }
                let col_map = &self.col_map;
                let first_row = values[i].0.row;
                let row0 = physical(first_row) % BAND;
                // The same compare-then-write as the identity path, through the two maps.
                let changes = self.band_changes(key, &values[i..run_end], |c| col_map.map(c.col).map(|col| (row0 + (c.row - first_row), col)));
                if changes {
                    if let Some(band) = self.bands.get_mut(&key) {
                        let band = Arc::make_mut(band);
                        for (c, v) in &values[i..run_end] {
                            if let Some(col) = col_map.map(c.col) {
                                band.set_value(row0 + (c.row - first_row), col, v);
                            }
                        }
                    }
                    self.finish_band(key);
                    outcome.changed += 1;
                } else {
                    outcome.unchanged += 1;
                }
                i = run_end;
            }
            if i >= values.len() {
                break;
            }
        }
        outcome
    }

    fn set_values_sequential(&mut self, values: &[(CellRef, Value)]) -> WrittenBands {
        let mut outcome = WrittenBands::default();
        let mut i = 0;
        while i < values.len() {
            let key = values[i].0.row / BAND;
            let end = values[i..].partition_point(|(c, _)| c.row / BAND == key) + i;
            if self.pages.contains_key(&key) && self.materialize(key).is_none() {
                i = end;
                continue;
            }
            // Compare before writing: a band where every value is already what it holds (or lands
            // on no stored cell) is left alone, so the copy an undo snapshot makes the write pay
            // for is not paid for a rewrite that changes nothing.
            if self.band_changes(key, &values[i..end], |c| Some((c.row % BAND, c.col))) {
                if let Some(band) = self.bands.get_mut(&key) {
                    let band = Arc::make_mut(band);
                    for (c, v) in &values[i..end] {
                        band.set_value(c.row % BAND, c.col, v);
                    }
                }
                self.finish_band(key);
                outcome.changed += 1;
            } else {
                outcome.unchanged += 1;
            }
            i = end;
        }
        outcome
    }

    /// The value-only write on a mapped store, split over threads. The plan is the list of band
    /// runs the logical batch covers: sorted by band key and merged, so a band that two row
    /// segments feed becomes one task and its writes cannot overwrite each other. Paged bands are
    /// materialized once here, before the workers look at the live map; a corrupt page drops its
    /// task, not the batch. Returns false when the plan would be larger than the batch itself (or
    /// a worker panics): the caller then streams the batch sequentially.
    #[cfg(not(target_arch = "wasm32"))]
    fn set_values_mapped_parallel(&mut self, values: &[(CellRef, Value)], threads: usize) -> bool {
        let (Some(first), Some(last)) = (values.first(), values.last()) else { return true };
        let mut runs: Vec<BandRun> = Vec::new();
        let mut i = 0;
        for segment in self.row_map.segments_in(first.0.row, last.0.row) {
            while i < values.len() && values[i].0.row < segment.logical {
                i += 1;
            }
            let physical = |row: u32| segment.physical.saturating_add(row.saturating_sub(segment.logical));
            let stop = i + values[i..].partition_point(|(c, _)| c.row < segment.logical_end());
            while i < stop {
                let key = physical(values[i].0.row) / BAND;
                let run_end = i + values[i..stop].partition_point(|(c, _)| physical(c.row) / BAND == key);
                runs.push(BandRun { key, base: physical(values[i].0.row), start: i, end: run_end });
                // A batch that touches a band per value (a sparse sheet after a shift) would make
                // the plan as large as the batch; streaming does the same writes with no plan.
                if runs.len() > values.len() / 4 {
                    return false;
                }
                i = run_end;
            }
            if i >= values.len() {
                break;
            }
        }
        runs.sort_unstable_by_key(|run| run.key);
        let mut tasks: Vec<(u32, Vec<BandRun>)> = Vec::new();
        for run in runs {
            match tasks.last_mut() {
                Some((key, list)) if *key == run.key => list.push(run),
                _ => tasks.push((run.key, vec![run])),
            }
        }
        // A paged band must be live before a worker copies it; one that fails to decode is
        // skipped entirely, as in the sequential mapped path.
        let mut live: Vec<(u32, Vec<BandRun>)> = Vec::with_capacity(tasks.len());
        for (key, list) in tasks {
            if self.pages.contains_key(&key) && self.materialize(key).is_none() {
                continue;
            }
            live.push((key, list));
        }
        if live.is_empty() {
            return true;
        }
        let bands = &self.bands;
        let col_map = &self.col_map;
        let chunk = live.len().div_ceil(threads);
        let results: Vec<std::thread::Result<Vec<(u32, Arc<Band>)>>> = std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for part in live.chunks(chunk) {
                handles.push(scope.spawn(move || {
                    let mut out = Vec::with_capacity(part.len());
                    for (key, list) in part {
                        let Some(old) = bands.get(key) else { continue };
                        // A band every value of this task is already what it holds is left as it
                        // is: the copy exists to be written into, and there is no write to make.
                        let changes = list.iter().any(|run| {
                            let Some((first_cell, _)) = values.get(run.start) else { return false };
                            let Some(slice) = values.get(run.start..run.end) else { return false };
                            let row0 = run.base % BAND;
                            band_slice_changes(old, slice, |c| col_map.map(c.col).map(|col| (row0 + (c.row - first_cell.row), col)))
                        });
                        if !changes {
                            continue;
                        }
                        let mut band = (**old).clone();
                        for run in list {
                            let Some((first_cell, _)) = values.get(run.start) else { continue };
                            let Some(slice) = values.get(run.start..run.end) else { continue };
                            let row0 = run.base % BAND;
                            for (c, v) in slice {
                                if let Some(col) = col_map.map(c.col) {
                                    band.set_value(row0 + (c.row - first_cell.row), col, v);
                                }
                            }
                        }
                        out.push((*key, Arc::new(band)));
                    }
                    out
                }));
            }
            handles.into_iter().map(|h| h.join()).collect()
        });
        let mut new_bands = Vec::with_capacity(live.len());
        for result in results {
            match result {
                Ok(part) => new_bands.extend(part),
                // A worker panicked: nothing has reached the store yet, so the sequential path can
                // still do the whole batch.
                Err(_) => return false,
            }
        }
        // One task a band, so no insert can overwrite another run's writes.
        for (key, band) in new_bands {
            self.bands.insert(key, band);
        }
        true
    }

    /// The value-only write, split over threads when the batch is large. A band is independent of
    /// every other (its numbers, texts and stripes are its own), so each worker clones the bands it
    /// was given, applies their values and hands the new bands back; the store replaces them only
    /// when every worker finished. The old bands stay with the undo snapshot.
    #[cfg(not(target_arch = "wasm32"))]
    fn set_values_parallel(&mut self, values: &[(CellRef, Value)], threads: usize) -> WrittenBands {
        let mut groups: Vec<(u32, usize, usize)> = Vec::new();
        let mut i = 0;
        while i < values.len() {
            let key = values[i].0.row / BAND;
            let end = values[i..].partition_point(|(c, _)| c.row / BAND == key) + i;
            groups.push((key, i, end));
            i = end;
        }
        let bands = &self.bands;
        let chunk = groups.len().div_ceil(threads);
        let results: Vec<std::thread::Result<(Vec<(u32, Arc<Band>)>, usize)>> = std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for part in groups.chunks(chunk) {
                handles.push(scope.spawn(move || {
                    let mut out = Vec::with_capacity(part.len());
                    let mut unchanged = 0usize;
                    for &(key, start, end) in part {
                        let Some(slice) = values.get(start..end) else { continue };
                        // The compare half of the write: a band whose values all match is left alone,
                        // so the worker's copy — and the replacement of a band the snapshot shares —
                        // is not paid for a write that changes nothing.
                        let Some(old) = bands.get(&key) else {
                            unchanged += 1;
                            continue;
                        };
                        if !band_slice_changes(old, slice, |c| Some((c.row % BAND, c.col))) {
                            unchanged += 1;
                            continue;
                        }
                        let mut band = (**old).clone();
                        for (c, v) in slice {
                            band.set_value(c.row % BAND, c.col, v);
                        }
                        band.finish();
                        out.push((key, Arc::new(band)));
                    }
                    (out, unchanged)
                }));
            }
            handles.into_iter().map(|h| h.join()).collect()
        });
        let mut new_bands = Vec::with_capacity(groups.len());
        let mut outcome = WrittenBands::default();
        for result in results {
            match result {
                Ok((part, unchanged)) => {
                    outcome.changed = outcome.changed.saturating_add(part.len());
                    outcome.unchanged = outcome.unchanged.saturating_add(unchanged);
                    new_bands.extend(part);
                }
                // A worker panicked: nothing has reached the store yet, so the sequential path can
                // still do the whole batch.
                Err(_) => return self.set_values_sequential(values),
            }
        }
        for (key, band) in new_bands {
            self.bands.insert(key, band);
        }
        outcome
    }

    /// Stores a whole row at once: one band lookup and one row lookup instead of one per cell.
    /// Whatever the row already holds stays; the cells given win. `cells` is drained so a reader
    /// can hand in the same buffer for every row.
    pub fn set_row(&mut self, row: u32, cells: &mut Vec<(u32, Cell)>) {
        if cells.is_empty() {
            return;
        }
        // A reader hands its rows over in column order and a sort writes its lanes that way too,
        // so the sort usually has nothing to do; checking costs one compare a cell.
        if !cells.is_sorted_by_key(|(c, _)| *c) {
            cells.sort_unstable_by_key(|(c, _)| *c);
        }
        cells.dedup_by(|a, b| {
            a.0 == b.0 && {
                b.1 = a.1.clone();
                true
            }
        });
        if !self.col_map.is_identity() {
            let mut mapped = Vec::with_capacity(cells.len());
            for (col, cell) in cells.drain(..) {
                if let Some(physical) = self.physical_col(col).or_else(|| self.physical_col_for_write(col)) {
                    mapped.push((physical, cell));
                }
            }
            mapped.sort_unstable_by_key(|(c, _)| *c);
            mapped.dedup_by(|a, b| {
                a.0 == b.0 && {
                    b.1 = a.1.clone();
                    true
                }
            });
            *cells = mapped;
        }
        // A serialized band may carry a row outside the sheet grid; keep it physical so `rebuild`
        // can find and report it rather than silently losing it (#1008).
        let physical = if row >= cella_core::MAX_ROWS {
            row
        } else {
            let Some(physical) = self.physical_row_for_write(row) else { return };
            physical
        };
        let key = physical / BAND;
        let band_row = physical % BAND;
        if self.pages.contains_key(&key) && self.materialize(key).is_none() {
            return;
        }
        self.band_for_write(key);
        if !self.array_anchors {
            self.array_anchors = cells.iter().any(|(_, cell)| cell_has_array(cell));
        }
        if !self.wrote_formula {
            self.wrote_formula = cells.iter().any(|(_, cell)| cell.formula.is_some());
        }
        let band = Arc::make_mut(self.bands.entry(key).or_default());
        // A row that isn't there yet — every row while a file opens — is built in one go: the
        // stripes are extended once from the cells instead of grown cell by cell.
        let mut shrank = false;
        if band.row(band_row).is_none_or(BandRow::is_empty) {
            let before = band.row(band_row).map_or(0, |r| r.cells as usize);
            band.put_row(band_row, cells);
            let after = band.row(band_row).map_or(0, |r| r.cells as usize);
            self.len = self.len + after - before;
            shrank = after < before;
        } else {
            for (col, cell) in cells.drain(..) {
                // put() answers whether the cell was stored before and is stored after: asking it
                // instead of probing the row twice more drops two rank computations a cell, which
                // is most of what a row of the sort's write-back costs.
                let (had, has, _had_formula) = band.put(band_row, col, cell);
                match (had, has) {
                    (false, true) => self.len += 1,
                    (true, false) => {
                        self.len -= 1;
                        shrank = true;
                    }
                    _ => {}
                }
            }
        }
        // The row's stored columns, read from its own bitmap: this is the reader's per-row path,
        // so the cached used box grows one row at a time instead of by a walk at the end.
        let span = band.row(band_row).and_then(|r| r.column_span(band));
        if band.rows.is_empty() {
            self.bands.remove(&key);
        }
        if let Some((first, last)) = span {
            self.absorb_used_stored(physical, physical, first, last);
        }
        if shrank {
            self.mark_used_stale();
        }
    }

    /// Puts every cell in the band its row says and recounts the length — serialized band keys
    /// and the cached count are derived state, and the ones a file carries are not trusted. Cells
    /// outside the sheet grid are dropped; returns how many (#1008).
    pub fn rebuild(&mut self) -> usize {
        let mut out = CellStore::new();
        let mut dropped = 0;
        // Physical order, not logical: a deserialized off-grid row has no logical mapping, and
        // that is exactly what `rebuild` is here to find and drop.
        let cells: Vec<(CellRef, Cell)> = self.raw_cells();
        for (at, cell) in cells {
            if at.is_valid() {
                out.set(at, cell);
            } else {
                dropped += 1;
            }
        }
        *self = out;
        dropped
    }

    /// Every stored cell at its physical key, ignoring the row map. Used by `rebuild` to see
    /// rows a serialized file put outside the grid.
    fn raw_cells(&self) -> Vec<(CellRef, Cell)> {
        let mut keys: Vec<u32> = self.bands.keys().chain(self.pages.keys()).copied().collect();
        keys.sort_unstable();
        keys.dedup();
        let mut out = Vec::new();
        for key in keys {
            let Some(band) = self.band_arc(key) else { continue };
            for row in &band.rows {
                let at = key * BAND + row.key;
                out.extend(packed_row(&band, at, row, 0, u32::MAX));
            }
        }
        out
    }

    pub fn remove(&mut self, c: CellRef) -> Option<Cell> {
        let physical = self.physical_row(c.row)?;
        let col = self.physical_col(c.col)?;
        let key = physical / BAND;
        if self.pages.contains_key(&key) && self.materialize(key).is_none() {
            return None;
        }
        let band = self.bands.get_mut(&key)?;
        let cell = Arc::make_mut(band).take(physical % BAND, col)?;
        self.len -= 1;
        // The box may have been sized by the cell that just went away.
        self.mark_used_stale();
        if self.bands.get(&key).is_some_and(|b| b.rows.is_empty()) {
            self.bands.remove(&key);
        }
        Some(cell)
    }

    /// The cells of `r` that carry a formula, in row-major order.
    pub fn formula_cells(&self, r: RangeRef) -> impl Iterator<Item = CellRef> + '_ {
        let mut out = Vec::new();
        // A whole-column range is millions of rows over a few thousand bands: resolve a band's
        // page and `Arc` once, not once per row. Rows arrive in physical order inside a segment,
        // so the last key covers every row of its band.
        let mut current: Option<(u32, Option<Arc<Band>>)> = None;
        for segment in self.row_map.segments_in(r.start.row, r.end.row) {
            for offset in 0..segment.len {
                let logical = segment.logical.saturating_add(offset);
                let physical = segment.physical.saturating_add(offset);
                let key = physical / BAND;
                if current.as_ref().is_none_or(|(k, _)| *k != key) {
                    // A page that says it holds no formula is skipped without decoding it.
                    let band = if self.pages.get(&key).is_some_and(|page| !page.has_formulas()) { None } else { self.band_arc(key) };
                    current = Some((key, band));
                }
                let Some(band) = current.as_ref().and_then(|(_, band)| band.as_ref()) else { continue };
                let Some(row) = band.row(physical % BAND) else { continue };
                for (col, _) in row.formulas() {
                    let Some(logical_col) = self.col_map.inverse(*col) else { continue };
                    if logical_col >= r.start.col && logical_col <= r.end.col {
                        out.push(CellRef::new(logical, logical_col));
                    }
                }
            }
        }
        out.into_iter()
    }

    /// Whether the block holds a cell whose value the store cannot give: a formula (it holds the
    /// value the file cached) or an array. Cheap next to reading the block: one check a row.
    pub fn has_owned_cells(&self, r: RangeRef) -> bool {
        // Same band cache as `formula_cells`: every whole-column `SUM`/`COUNT` pays this gate
        // before folding, and resolving the band per row cost 25 ms a million rows.
        let mut current: Option<(u32, Option<Arc<Band>>)> = None;
        for segment in self.row_map.segments_in(r.start.row, r.end.row) {
            for offset in 0..segment.len {
                let physical = segment.physical.saturating_add(offset);
                let key = physical / BAND;
                if current.as_ref().is_none_or(|(k, _)| *k != key) {
                    // A page that says it holds no formula or array is not decoded at all.
                    let band = if self.pages.get(&key).is_some_and(|page| !page.has_owned()) { None } else { self.band_arc(key) };
                    current = Some((key, band));
                }
                let Some(band) = current.as_ref().and_then(|(_, band)| band.as_ref()) else { continue };
                let Some(row) = band.row(physical % BAND) else { continue };
                let owned = row.extras.as_ref().is_some_and(|e| {
                    e.formulas.iter().any(|(c, _)| self.col_map.inverse(*c).is_some_and(|col| (r.start.col..=r.end.col).contains(&col)))
                        || e.arrays().iter().any(|(c, _)| self.col_map.inverse(*c).is_some_and(|col| (r.start.col..=r.end.col).contains(&col)))
                });
                if owned {
                    return true;
                }
            }
        }
        false
    }

    /// Calls `f` on each stored cell of `r`, row-major, handing the value as the page holds it, and
    /// `None` for a cell whose value the evaluator owns (a formula, an array, or a blank one).
    ///
    /// Reading a range through `iter_range` builds a `Cell` an iteration — a `Value`, a style, and
    /// two `Arc` bumps — which costs ~77 ns a cell against ~2 ns for the numbers themselves. Summing
    /// a million-cell column spent all of its 154 ms there.
    ///
    /// Columns are walked from the column map's runs, so a column-mapped sheet stays on this path:
    /// `segments_in` hands out clipped, physical-contiguous, logically ordered runs — one when the
    /// map is identity — and a run's logical column is `logical + (physical - first)`.
    pub fn visit_packed(&self, r: RangeRef, f: &mut dyn FnMut(CellRef, Option<Packed<'_>>) -> bool) -> bool {
        self.visit_packed_impl(r, false, f)
    }

    /// [`visit_packed`](Self::visit_packed) for a block whose formulas are **settled**: a formula
    /// cell hands out the value the store holds for it instead of `None`.
    ///
    /// The caller owns the proof (a pass that owes no formula in the block a value, a store whose
    /// last calculation is current). It is what turns a whole-column `SUM` over a formula column
    /// from one `Sheet::value` a cell into one packed walk — the difference between 139 ms and
    /// 55 ms for a million rows.
    pub fn visit_packed_settled(&self, r: RangeRef, f: &mut dyn FnMut(CellRef, Option<Packed<'_>>) -> bool) -> bool {
        self.visit_packed_impl(r, true, f)
    }

    fn visit_packed_impl(&self, r: RangeRef, settled: bool, f: &mut dyn FnMut(CellRef, Option<Packed<'_>>) -> bool) -> bool {
        let col_runs = self.col_map.segments_in(r.start.col, r.end.col);
        // A sheet with many column edits would repeat the row walk once per column run; past a
        // handful of runs build each cell once instead, so the work stays bounded by the range.
        if col_runs.len() > MAX_PACKED_COLUMN_SEGMENTS {
            return !settled && self.visit_packed_by_iter(r, f);
        }
        let (r0, r1) = (r.start.row, r.end.row);
        let mut current_key = None;
        let mut current_band: Option<Arc<Band>> = None;
        for segment in self.row_map.segments_in(r0, r1) {
            for offset in 0..segment.len {
                let at_row = segment.logical.saturating_add(offset);
                let physical = segment.physical.saturating_add(offset);
                if at_row < r0 || at_row > r1 {
                    continue;
                }
                let key = physical / BAND;
                if current_key != Some(key) {
                    current_key = Some(key);
                    current_band = self.band_arc(key);
                }
                let Some(band) = current_band.as_ref() else { continue };
                let Some(row) = band.row(physical % BAND) else { continue };
                for columns in &col_runs {
                    let last = columns.physical.saturating_add(columns.len).saturating_sub(1);
                    if columns.physical > last {
                        continue;
                    }
                    if !self.visit_packed_row(band, row, at_row, columns.logical, columns.physical, last, settled, f) {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Walks the cells of one stored row that fall in the physical column run `[first, last]`,
    /// handing each out at its logical column. Returns false when `f` stopped the walk.
    #[allow(clippy::too_many_arguments)]
    fn visit_packed_row(
        &self,
        band: &Band,
        row: &BandRow,
        at_row: u32,
        logical: u32,
        first: u32,
        last: u32,
        settled: bool,
        f: &mut dyn FnMut(CellRef, Option<Packed<'_>>) -> bool,
    ) -> bool {
        // Start at the first stored column of the range and stop past its last, so a narrow read
        // of a wide row costs the cells it asks for, not the row's width.
        let lo = first.max(row.first);
        let hi = last.min(row.first.saturating_add(row.columns()).saturating_sub(1));
        if lo > hi {
            return true;
        }
        let start_slot = (lo - row.first) as usize;
        // A cell that carries a formula stores the value the file cached, which is not its value
        // now: the evaluator owns it, so hand it over as `None`.
        let formulas = row.extras.as_ref().map(|e| e.formulas.as_slice()).filter(|f| !f.is_empty());
        let mut i = row.rank_before(band, lo);
        let mut kinds = RunCursor::new(row.kind_runs(band), u16::from(EMPTY));
        'words: for (word, bits) in row.bits(band).iter().enumerate().skip(start_slot / 64) {
            let mut w = *bits;
            if word == start_slot / 64 {
                w &= !((1u64 << (start_slot % 64)) - 1);
            }
            while w != 0 {
                let bit = w.trailing_zeros();
                w &= w - 1;
                let index = i;
                i += 1;
                let col = row.first + word as u32 * 64 + bit;
                if col > hi {
                    // Past this run's last wanted column; the next column run may still be inside.
                    break 'words;
                }
                let has_formula = formulas.is_some_and(|f| f.binary_search_by_key(&col, |(c, _)| *c).is_ok());
                let payload = row.payload_at(band, index);
                let packed = if has_formula && !settled {
                    None
                } else {
                    match kinds.at(index) as u8 {
                        // An empty slot is not a stored value (no value, a style at most),
                        // and `iter_range` does not hand one out either.
                        EMPTY => continue,
                        NUMBER => Some(Packed::Number(band.number(payload))),
                        NUMBER_WIDE => Some(Packed::Number(f64::from_bits(row.wide_at(band, index)))),
                        TEXT => band.texts.get(payload as usize).map(|t| Packed::Text(t)),
                        TEXT_WIDE => band.texts.get(row.wide_at(band, index) as usize).map(|t| Packed::Text(t)),
                        BOOL => Some(Packed::Bool(payload != 0)),
                        ERROR => Some(Packed::Error(*ERRORS.get(payload as usize).unwrap_or(&CellError::Value))),
                        _ => None,
                    }
                };
                if !f(CellRef::new(at_row, logical.saturating_add(col - first)), packed) {
                    return false;
                }
            }
        }
        true
    }

    /// The packed walk built from `iter_range` cells, for a column map with more runs than
    /// [`MAX_PACKED_COLUMN_SEGMENTS`]: one `Cell` per cell, but no repeated row walk.
    fn visit_packed_by_iter(&self, r: RangeRef, f: &mut dyn FnMut(CellRef, Option<Packed<'_>>) -> bool) -> bool {
        for (at, cell) in self.iter_range(r) {
            let packed = if cell.formula.is_some() {
                None
            } else {
                match &cell.value {
                    cella_core::Value::Number(n) => Some(Packed::Number(*n)),
                    cella_core::Value::Text(t) => Some(Packed::Text(t.as_ref())),
                    cella_core::Value::Bool(b) => Some(Packed::Bool(*b)),
                    cella_core::Value::Error(e) => Some(Packed::Error(*e)),
                    cella_core::Value::Array(_) => None,
                    cella_core::Value::Empty => continue,
                }
            };
            if !f(at, packed) {
                return false;
            }
        }
        true
    }

    pub fn iter_range(&self, r: RangeRef) -> impl Iterator<Item = (CellRef, Cell)> + '_ {
        CellWalk::new(self, r)
    }

    /// Walks `r` in [`iter_range`](Self::iter_range)'s order, handing `f` each stored cell's value
    /// and whether the cell holds a formula, **without building a [`Cell`]** and without
    /// collecting a band's cells into a vector first.
    ///
    /// A read that only wants values — the recalculation's block builder, a statistics scan —
    /// otherwise pays for the formula `Arc` and the style of every cell and for one `Vec` a band:
    /// 116 ms for a million-cell column against 78 ms of value reads alone (measured 2026-11-22).
    /// Returns false when `f` asked to stop.
    pub fn visit_values(&self, r: RangeRef, f: &mut dyn FnMut(CellRef, Value, bool) -> bool) -> bool {
        let col_segments = self.col_map.segments_in(r.start.col, r.end.col);
        for segment in self.row_map.segments_in(r.start.row, r.end.row) {
            if segment.len == 0 {
                continue;
            }
            for key in self.bands_in(segment.physical, segment.physical.saturating_add(segment.len)) {
                let Some(band) = self.band_arc(key) else { continue };
                let physical_start = segment.physical;
                let physical_end = segment.physical.saturating_add(segment.len);
                for row in &band.rows {
                    let physical = key.saturating_mul(BAND).saturating_add(row.key);
                    if physical < physical_start || physical >= physical_end {
                        continue;
                    }
                    let logical = segment.logical.saturating_add(physical - physical_start);
                    if logical < r.start.row || logical > r.end.row {
                        continue;
                    }
                    for columns in &col_segments {
                        let physical_first = columns.physical;
                        let physical_last = columns.physical.saturating_add(columns.len).saturating_sub(1);
                        if physical_first > physical_last {
                            continue;
                        }
                        let mut cursor = PackCursor::new(&band, row, physical_first, physical_last);
                        while let Some((c, value, formula)) = cursor.next_value(&band, row, logical) {
                            let col = columns.logical.saturating_add(c.col.saturating_sub(physical_first));
                            if col >= r.start.col && col <= r.end.col && !f(CellRef::new(logical, col), value, formula) {
                                return false;
                            }
                        }
                    }
                }
            }
        }
        true
    }

    /// Walks `r` in logical row-major order like [`iter_range`](Self::iter_range), but decodes
    /// paged bands **without caching them**. This is the contract of the save/export scanner: one
    /// band and one row are held at a time (so however wide the sheet is the walk stays flat), the
    /// page cache's bytes and entries are exactly what they were before it ran, and a page that
    /// cannot be decoded is a skipped band rather than an error or a panic. What it yields is
    /// `iter_range`'s walk cell for cell, formula values included.
    ///
    /// [`iter`](Self::iter) stays the viewport and random-access path: its reads are expected to
    /// repeat, so they pay for the cache. A scan reads every cell once and would only evict the
    /// working set (and leave the sheet's tail resident) if it went through it.
    pub fn iter_scan(&self, r: RangeRef) -> impl Iterator<Item = (CellRef, Cell)> + '_ {
        ScanWalk::new(self, r)
    }

    /// [`iter_scan`](Self::iter_scan) as a callback: `f` sees every cell in the same order and
    /// stops the walk by returning false (the band being walked is dropped on the way out). No
    /// collection is built per band, so an exporter can write as it goes.
    pub fn visit_scan(&self, r: RangeRef, f: &mut dyn FnMut(CellRef, Cell) -> bool) -> bool {
        for (at, cell) in self.iter_scan(r) {
            if !f(at, cell) {
                return false;
            }
        }
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = (CellRef, Cell)> + '_ {
        CellWalk::new(self, RangeRef::new(CellRef::new(0, 0), CellRef::new(u32::MAX, u32::MAX)))
    }

    /// Prepares repeated reads of the same inclusive logical column range in arbitrary row order.
    /// Each call appends one entry per column into a caller-owned buffer, with `None` for holes
    /// (including unmapped columns), and keeps a styled blank as a cell. A row borrows one live
    /// band or decodes one page; neither the column map nor an output vector is built per row.
    pub fn row_reader(&self, c0: u32, c1: u32) -> impl Fn(u32, &mut Vec<Option<Cell>>) + '_ {
        let end = c1.min(cella_core::MAX_COLS.saturating_sub(1)).saturating_add(1);
        let identity = ColSegment { logical: c0, physical: c0, len: end.saturating_sub(c0) };
        let mapped = (!self.col_map.identity && c0 < end).then(|| self.col_map.segments_in(c0, end - 1));
        move |row, out| {
            if c0 >= end {
                return;
            }
            let mut next = c0;
            if let Some(physical) = self.physical_row(row) {
                let key = physical / BAND;
                let decoded;
                let band = if let Some(band) = self.bands.get(&key) {
                    Some(band.as_ref())
                } else {
                    decoded = self.band_arc(key);
                    decoded.as_deref()
                };
                if let Some(band) = band
                    && let Some(cells) = band.row(physical % BAND)
                {
                    for columns in mapped.as_deref().unwrap_or_else(|| std::slice::from_ref(&identity)) {
                        let last = columns.physical.saturating_add(columns.len - 1);
                        for (at, cell) in packed_row(band, row, cells, columns.physical, last) {
                            let logical = columns.logical + (at.col - columns.physical);
                            if logical > next {
                                out.resize_with(out.len() + (logical - next) as usize, || None);
                            }
                            out.push(Some(cell));
                            next = logical + 1;
                        }
                    }
                }
            }
            if end > next {
                out.resize_with(out.len() + (end - next) as usize, || None);
            }
        }
    }

    pub fn row(&self, row: u32, c0: u32, c1: u32) -> impl Iterator<Item = (u32, Cell)> + '_ {
        let mut out = Vec::new();
        let Some(physical) = self.physical_row(row) else { return out.into_iter() };
        let Some(band) = self.band_arc(physical / BAND) else { return out.into_iter() };
        let Some(cells) = band.row(physical % BAND) else { return out.into_iter() };
        for columns in self.col_map.segments_in(c0, c1) {
            let first = columns.physical;
            let last = columns.physical.saturating_add(columns.len).saturating_sub(1);
            if first > last {
                continue;
            }
            out.extend(packed_row(&band, row, cells, first, last).filter_map(|(at, cell)| {
                let col = columns.logical.saturating_add(at.col - first);
                (col >= c0 && col <= c1).then_some((col, cell))
            }));
        }
        out.into_iter()
    }

    pub fn used_range(&self) -> Option<RangeRef> {
        let cached = *self.used_state();
        if !cached.stale {
            return cached.used;
        }
        // Stale: a mutation removed cells or moved them. Walking is a read-only pass over the
        // bands, so the lock is not held while it runs; two readers that race here walk twice and
        // write the same box, and a mutator cannot run meanwhile (it would need `&mut self`).
        let walked = self.walk_used();
        *self.used_state() = UsedBox { used: walked, stale: false };
        walked
    }

    /// The cache, for a reader: repair and read both go through it.
    fn used_state(&self) -> MutexGuard<'_, UsedBox> {
        self.used.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The cache, for a mutator: `&mut self` already excludes every other access, so this is the
    /// plain field rather than a lock.
    fn used_mut(&mut self) -> &mut UsedBox {
        self.used.get_mut().unwrap_or_else(PoisonError::into_inner)
    }

    /// The cache as it is, for a clone: a copy shares the bands it was derived from, so it shares
    /// the box, stale flag and all.
    fn used_box(&self) -> UsedBox {
        *self.used_state()
    }

    /// The uncached walk over every band, every stored row of it and every bitmap word of that
    /// row: the definition of [`used_range`](Self::used_range). Only a mutation that can shrink
    /// the box marks the cache stale, and the next read repairs it with this.
    fn walk_used(&self) -> Option<RangeRef> {
        let mut r0 = u32::MAX;
        let mut c0 = u32::MAX;
        let mut r1 = 0u32;
        let mut c1 = 0u32;
        for (bk, band) in &self.bands {
            for cells in &band.rows {
                let Some((first, last)) = cells.column_span(band) else { continue };
                let physical = bk * BAND + cells.key;
                let Some(logical) = self.row_map.inverse(physical) else { continue };
                let Some((logical_first, logical_last)) = self.logical_col_span(first, last) else { continue };
                r0 = r0.min(logical);
                r1 = r1.max(logical);
                c0 = c0.min(logical_first);
                c1 = c1.max(logical_last);
            }
        }
        for (bk, page) in &self.pages {
            let Some((pr0, pr1, pc0, pc1)) = page.bounds() else { continue };
            for row in pr0..=pr1 {
                let Some(logical) = self.row_map.inverse(bk * BAND + row) else { continue };
                r0 = r0.min(logical);
                r1 = r1.max(logical);
            }
            if let Some((logical_first, logical_last)) = self.logical_col_span(pc0, pc1) {
                c0 = c0.min(logical_first);
                c1 = c1.max(logical_last);
            }
        }
        if c0 == u32::MAX {
            return None;
        }
        Some(RangeRef::new(CellRef::new(r0, c0), CellRef::new(r1, c1)))
    }

    /// Marks the cached box stale: a mutation removed cells, or moved stored ones to another
    /// logical position. The next `used_range` walks it, once, however many cells the mutation
    /// touched — which is what keeps a bulk clear linear instead of one walk per removed cell.
    fn mark_used_stale(&mut self) {
        self.used_mut().stale = true;
    }

    /// Grows the cached box to include one stored cell, given its logical position. Additions only
    /// ever grow a bounding box, so this cannot disagree with `walk_used`, and it is the whole
    /// cost of a write that only adds.
    fn grow_used(&mut self, row: u32, col: u32) {
        let cache = self.used_mut();
        Self::grow_used_box(&mut *cache, row, col);
    }

    /// [`grow_used`](Self::grow_used) against a box the caller already holds the lock for: a row
    /// write absorbs both ends of its span, and taking the lock twice more per row was a
    /// measurable part of a sort's write-back.
    fn grow_used_box(cache: &mut UsedBox, row: u32, col: u32) {
        if cache.stale {
            // The walk that clears the flag rebuilds the box from nothing; growing a stale one is
            // wasted work, and only a mutation that removed something writes while stale.
            return;
        }
        match &mut cache.used {
            Some(used) => {
                used.start.row = used.start.row.min(row);
                used.start.col = used.start.col.min(col);
                used.end.row = used.end.row.max(row);
                used.end.col = used.end.col.max(col);
            }
            None => cache.used = Some(RangeRef::cell(CellRef::new(row, col))),
        }
    }

    /// Grows the cached box to include the cells of one band the store now holds, live or paged.
    /// The reader absorbs each band as it arrives, so opening a sheet grows the box band by band
    /// instead of walking it once at the end.
    fn absorb_used_band(&mut self, key: u32) {
        if self.used_mut().stale {
            return;
        }
        let bounds = match (self.bands.get(&key), self.pages.get(&key)) {
            (Some(band), _) => page::band_bounds(band),
            (None, Some(page)) => page.bounds(),
            _ => None,
        };
        // A band's bounds are the row keys inside the band, not physical rows, so the band
        // number lifts them the way the walk does.
        if let Some((r0, r1, c0, c1)) = bounds {
            let base = key.saturating_mul(BAND);
            self.absorb_used_stored(base.saturating_add(r0), base.saturating_add(r1), c0, c1);
        }
    }

    /// Grows the cached box to include the stored cells of the physical row span `r0..=r1` whose
    /// columns span `c0..=c1`, mapped to logical positions. Rows and columns the maps do not cover
    /// are skipped, exactly as `walk_used` skips them.
    fn absorb_used_stored(&mut self, r0: u32, r1: u32, c0: u32, c1: u32) {
        // A band, and so a page, spans at most `BAND` physical rows. A wider span is not something
        // a store can hold; walk instead of trusting that it is bounded.
        if r1.saturating_sub(r0) >= BAND {
            self.mark_used_stale();
            return;
        }
        // The mapping comes first and the lock is taken once: a row write absorbs its span for
        // every row, and three uncontended locks a row were a measurable part of a sort's
        // write-back. A stale box skips the growth either way, so doing this before the check
        // only wastes the mapping.
        let (mut lo, mut hi) = (u32::MAX, 0u32);
        let mut row = r0;
        loop {
            if let Some(logical) = self.row_map.inverse(row) {
                lo = lo.min(logical);
                hi = hi.max(logical);
            }
            if row >= r1 {
                break;
            }
            row = row.saturating_add(1);
        }
        if lo == u32::MAX {
            return;
        }
        let Some((first, last)) = self.logical_col_span(c0, c1) else { return };
        let cache = self.used_mut();
        if cache.stale {
            return;
        }
        Self::grow_used_box(&mut *cache, lo, first);
        Self::grow_used_box(&mut *cache, hi, last);
    }

    /// Every formula in the store with the position it sits at and whether a cached value came
    /// with it. Iterates the per-row side tables rather than the cells: a sheet can hold a hundred
    /// million cells and a few thousand formulas, and scanning the cells to find them costs more
    /// than opening the file.
    pub fn formulas(&self) -> impl Iterator<Item = (CellRef, Arc<Formula>, bool)> + '_ {
        let mut keys: Vec<u32> = self.bands.keys().chain(self.pages.keys()).copied().collect();
        keys.sort_unstable();
        keys.dedup();
        let mut out = Vec::new();
        for key in keys {
            if self.pages.get(&key).is_some_and(|page| !page.has_formulas()) {
                continue;
            }
            let Some(band) = self.band_arc(key) else { continue };
            for cells in &band.rows {
                if cells.formulas().is_empty() {
                    continue;
                }
                let Some(logical) = self.row_map.inverse(key * BAND + cells.key) else { continue };
                for (col, f) in cells.formulas() {
                    let Some(logical_col) = self.col_map.inverse(*col) else { continue };
                    let has_value = cells.index_of(&band, *col).is_some_and(|i| cells.kind_at(&band, i) != EMPTY);
                    out.push((CellRef::new(logical, logical_col), f.clone(), has_value));
                }
            }
        }
        out.sort_unstable_by_key(|(c, _, _)| (c.row, c.col));
        out.into_iter()
    }

    /// Whether any logically reachable cell contains a formula, even without a cached result.
    /// Unlike `Self::formulas`, this needs neither ordering nor a copy of each formula.
    ///
    /// A store no formula was ever written into answers without the walk: see `wrote_formula`.
    pub fn has_formulas(&self) -> bool {
        if !self.wrote_formula {
            return false;
        }
        let contains = |key: u32, band: &Band| {
            band.rows.iter().any(|row| {
                let formulas = row.formulas();
                !formulas.is_empty()
                    && self.row_map.inverse(key.saturating_mul(BAND).saturating_add(row.key)).is_some()
                    && formulas.iter().any(|(col, _)| self.col_map.inverse(*col).is_some())
            })
        };
        self.bands.iter().any(|(&key, band)| contains(key, band))
            || self.pages.iter().any(|(&key, page)| {
                // A positive page flag is not enough: a row/column deletion can leave only
                // physically stored formulas with no logical inverse, and a page may be unreadable.
                page.has_formulas() && self.load_page(key, page).is_some_and(|band| contains(key, &band))
            })
    }

    /// The cells whose formula differs between this store and an older version of it, which an
    /// undo restores: `Some(formula)` for a cell that holds one here and held none (or a
    /// different one) there, `None` for a cell that held one there and holds none here.
    ///
    /// Only the bands and rows the two versions do not share are looked at — an undo snapshot
    /// shares every band and row the edit did not touch, and a spilled snapshot shares the pages
    /// the edit left alone — so a one-cell edit costs one band, not one sheet. `None` when they
    /// cannot be compared that way: a row or column insert moved the cells to other logical
    /// positions, or a page that a band would have to come from cannot be decoded. Then finding the
    /// cells that differ costs a walk of the whole store, and the caller is better off rebuilding
    /// what it wanted to update than paying for that.
    ///
    /// A band one side keeps as a page is decoded **one band at a time, when the walk reaches it**
    /// (through the page file's cache, see [`page_cache_stats`](Self::page_cache_stats)) — never
    /// all of them up front. A page whose header says it holds no formula at all is not decoded:
    /// its formula list is empty either way, and that is the whole of what this walk compares. So a
    /// snapshot of a sheet of values, spilled to pages by the history, is compared for free.
    ///
    /// Two formulas are the same when their text is: the parsed tree is a cache of the text, so a
    /// formula copied and re-parsed is not a change.
    pub fn formula_changes_against(&self, older: &CellStore) -> Option<Vec<(CellRef, Option<Arc<Formula>>)>> {
        if !self.row_map.same_as(&older.row_map) || !self.col_map.same_as(&older.col_map) {
            return None;
        }
        let mut out: Vec<(CellRef, Option<Arc<Formula>>)> = Vec::new();
        let mut keys: Vec<u32> = self.bands.keys().chain(self.pages.keys()).chain(older.bands.keys()).chain(older.pages.keys()).copied().collect();
        keys.sort_unstable();
        keys.dedup();
        for key in keys {
            // A band both versions share is skipped whole, whether they share it as a live band or
            // as a page: an edit copies the bands it writes, and a spilled snapshot keeps the rest.
            if let (Some(a), Some(b)) = (self.bands.get(&key), older.bands.get(&key))
                && Arc::ptr_eq(a, b)
            {
                continue;
            }
            if let (Some(a), Some(b)) = (self.pages.get(&key), older.pages.get(&key))
                && Arc::ptr_eq(a, b)
            {
                continue;
            }
            // Only a band that holds a formula on one of the two sides can report a change, and a
            // page answers that from its header before anything is read.
            if !self.band_has_formulas(key) && !older.band_has_formulas(key) {
                continue;
            }
            let here = self.comparable_band(key)?;
            let there = older.comparable_band(key)?;
            let empty: &[BandRow] = &[];
            let rows_here = here.as_deref().map_or(empty, |b| b.rows.as_slice());
            let rows_there = there.as_deref().map_or(empty, |b| b.rows.as_slice());
            let (mut i, mut j) = (0usize, 0usize);
            loop {
                let (a, b) = (rows_here.get(i), rows_there.get(j));
                match (a, b) {
                    (None, None) => break,
                    // The same row on both sides: only the rows whose side table was replaced
                    // (the ones the edit wrote) can hold a formula that changed.
                    (Some(a), Some(b)) if a.key == b.key => {
                        i += 1;
                        j += 1;
                        if !BandRow::same_extras(a, b) {
                            self.push_row_changes(key, Some(a), Some(b), &mut out);
                        }
                    }
                    (Some(a), Some(b)) if a.key < b.key => {
                        i += 1;
                        self.push_row_changes(key, Some(a), None, &mut out);
                    }
                    (Some(_), Some(_)) => {
                        j += 1;
                        self.push_row_changes(key, None, b, &mut out);
                    }
                    (Some(a), None) => {
                        i += 1;
                        self.push_row_changes(key, Some(a), None, &mut out);
                    }
                    (None, Some(b)) => {
                        j += 1;
                        self.push_row_changes(key, None, Some(b), &mut out);
                    }
                }
            }
        }
        Some(out)
    }

    /// `formula_changes_against` for one row of a band: merge its two formula lists (both sorted
    /// by column) and record the cells they disagree on.
    fn push_row_changes(&self, band_key: u32, here: Option<&BandRow>, there: Option<&BandRow>, out: &mut Vec<(CellRef, Option<Arc<Formula>>)>) {
        let newer = here.map_or(&[][..], |r| r.formulas());
        let older = there.map_or(&[][..], |r| r.formulas());
        if newer.is_empty() && older.is_empty() {
            return;
        }
        let Some(row_key) = here.or(there).map(|r| r.key) else { return };
        let Some(logical_row) = self.row_map.inverse(band_key.saturating_mul(BAND).saturating_add(row_key)) else { return };
        let at = |col: u32, f: Option<Arc<Formula>>, out: &mut Vec<(CellRef, Option<Arc<Formula>>)>| {
            if let Some(logical_col) = self.col_map.inverse(col) {
                out.push((CellRef::new(logical_row, logical_col), f));
            }
        };
        let (mut i, mut j) = (0usize, 0usize);
        loop {
            match (newer.get(i), older.get(j)) {
                (None, None) => break,
                (Some((ca, fa)), Some((cb, fb))) if ca == cb => {
                    if !same_formula(fa, fb) {
                        at(*ca, Some(Arc::clone(fa)), out);
                    }
                    i += 1;
                    j += 1;
                }
                (Some((ca, fa)), Some((cb, _))) if ca < cb => {
                    at(*ca, Some(Arc::clone(fa)), out);
                    i += 1;
                }
                (Some(_), Some((cb, _))) => {
                    at(*cb, None, out);
                    j += 1;
                }
                (Some((ca, fa)), None) => {
                    at(*ca, Some(Arc::clone(fa)), out);
                    i += 1;
                }
                (None, Some((cb, _))) => {
                    at(*cb, None, out);
                    j += 1;
                }
            }
        }
    }

    /// A stored cell with a value or a formula (a style alone does not count).
    pub fn has(&self, c: CellRef) -> bool {
        let (Some(physical), Some(col)) = (self.physical_row(c.row), self.physical_col(c.col)) else { return false };
        self.band_arc(physical / BAND).is_some_and(|b| b.row(physical % BAND).is_some_and(|row| row_has_value(&b, row, col)))
    }

    /// Last row (inclusive, capped at `cap`) of the maximal run of contiguous rows at or after
    /// `from.row` that store a cell in `from.col` with a non-empty value. See `Sheet::value_ref`.
    pub fn contiguous_value_run(&self, from: CellRef, cap: u32) -> Option<u32> {
        let mut last = None;
        let mut expect = from.row;
        let col = self.physical_col(from.col)?;
        while expect <= cap {
            let Some(physical) = self.physical_row(expect) else { break };
            let Some(band) = self.band_arc(physical / BAND) else { break };
            let Some(row) = band.row(physical % BAND) else { break };
            if !row.value_nonempty(&band, col) {
                return last;
            }
            last = Some(expect);
            expect += 1;
        }
        last
    }

    /// Positions whose stored cells differ between `self` and `other`, by band key and physical
    /// column. The walk skips a band both stores share (`Arc::ptr_eq`), so it is cheap on the
    /// common edit path, where only the touched bands differ.
    ///
    /// The positions are **stored/physical**: a pure remap is not a difference, because a row or
    /// column edit changes where a cell lives, not what it is, and the two stores share the bands.
    /// With different maps a reported position may even hold nothing in either store. Use
    /// [`diff_logical`](Self::diff_logical) (or [`CellStore::eq`]) when the maps may differ.
    pub fn diff(&self, other: &CellStore) -> Vec<CellRef> {
        self.diff_bands(other)
    }

    /// The stored-position walk behind [`diff`](Self::diff).
    fn diff_bands(&self, other: &CellStore) -> Vec<CellRef> {
        let mut keys: Vec<u32> = self.bands.keys().chain(self.pages.keys()).chain(other.bands.keys()).chain(other.pages.keys()).copied().collect();
        keys.sort_unstable();
        keys.dedup();
        let mut out = Vec::new();
        for k in keys {
            let (a, b) = (self.band_arc(k), other.band_arc(k));
            if let (Some(a), Some(b)) = (&a, &b)
                && Arc::ptr_eq(a, b)
            {
                continue;
            }
            let band = k * BAND;
            let mut rows: Vec<u32> = Vec::new();
            if let Some(a) = a.as_deref() {
                rows.extend(a.rows.iter().map(|r| r.key));
            }
            if let Some(b) = b.as_deref() {
                rows.extend(b.rows.iter().map(|r| r.key));
            }
            rows.sort_unstable();
            rows.dedup();
            for row in rows {
                let (ra, rb) = (a.as_deref().and_then(|x| x.row(row)), b.as_deref().and_then(|x| x.row(row)));
                let mut cols: Vec<u32> = Vec::new();
                if let (Some(a), Some(ra)) = (a.as_deref(), ra) {
                    cols.extend(packed_row(a, band + row, ra, 0, u32::MAX).map(|(c, _)| c.col));
                }
                if let (Some(b), Some(rb)) = (b.as_deref(), rb) {
                    cols.extend(packed_row(b, band + row, rb, 0, u32::MAX).map(|(c, _)| c.col));
                }
                cols.sort_unstable();
                cols.dedup();
                for col in cols {
                    let x = a.as_deref().and_then(|x| ra.and_then(|r| r.index_of(x, col).map(|i| (x, r, i))));
                    let y = b.as_deref().and_then(|y| rb.and_then(|r| r.index_of(y, col).map(|i| (y, r, i))));
                    let same = match (x, y) {
                        (None, None) => true,
                        (Some((xb, xr, xi)), Some((yb, yr, yi))) => {
                            let arrays = |row: &BandRow| row.arrays().binary_search_by_key(&col, |(c, _)| *c).ok().map(|k| row.arrays()[k].1.clone());
                            same_value(xb, xr, xi, yb, yr, yi)
                                && (xr.kind_at(xb, xi) != ARRAY || arrays(xr) == arrays(yr))
                                && xr.formula_at(col).map(|f| f.text.clone()) == yr.formula_at(col).map(|f| f.text.clone())
                        }
                        _ => false,
                    };
                    if !same {
                        let physical = band + row;
                        if let Some(logical) = self.row_map.inverse(physical).or_else(|| other.row_map.inverse(physical))
                            && let Some(logical_col) = self.col_map.inverse(col).or_else(|| other.col_map.inverse(col))
                        {
                            out.push(CellRef::new(logical, logical_col));
                        }
                    }
                }
            }
        }
        out
    }

    /// Positions whose **logical** cells differ between `self` and `other`, whichever maps the two
    /// stores place them with. Two stores with the same content under different maps report
    /// nothing; a cell only one side holds is reported at its logical position.
    ///
    /// This walks the logical cells — the smaller store's, then the larger side's extras when the
    /// lengths differ — and rebuilds a `Cell` for each comparison, so it is O(cells) and allocates
    /// the `Cell`s it looks at. At 30M cells that is seconds: a diagnostic for tests and the fuzz,
    /// not an edit-path check.
    pub fn diff_logical(&self, other: &CellStore) -> Vec<CellRef> {
        let (small, large) = if self.len <= other.len { (self, other) } else { (other, self) };
        let mut out = Vec::new();
        for (at, cell) in small.iter() {
            if large.get(at).is_none_or(|other| !same_cell(&cell, &other)) {
                out.push(at);
            }
        }
        // Only when the counts differ can a position the smaller store holds not stand for the
        // larger one's cell: the extra cells are where `get` found nothing above.
        if self.len != other.len {
            for (at, _) in large.iter() {
                if small.get(at).is_none() {
                    out.push(at);
                }
            }
        }
        out
    }

    /// Rough heap storage kept alive by this snapshot's bands that `newer` no longer shares.
    pub fn retained_bytes_against(&self, newer: &CellStore) -> usize {
        let mut bytes = 0usize;
        for (key, page) in &self.pages {
            if newer.pages.get(key).is_some_and(|next| Arc::ptr_eq(next, page)) {
                continue;
            }
            bytes += page.page_len();
        }
        for (key, band) in &self.bands {
            let next = newer.bands.get(key);
            if next.is_some_and(|b| Arc::ptr_eq(b, band)) {
                continue;
            }
            // An untouched copy of this band keeps every row and side table: what the snapshot
            // retains is the fixed part alone, which is the whole point of the tag — a
            // recalculation copies the sheet's bands for their values and pays no row walk.
            bytes = bytes.saturating_add(match next {
                Some(next) if band.shares_rows_with(next) => band.unshared_fixed_bytes(),
                _ => Self::unshared_band_bytes(band, next),
            });
        }
        bytes
    }

    /// [`retained_bytes_against`](Self::retained_bytes_against) with nothing remembered: every
    /// band `newer` does not share is walked. This is the price the cached form must reproduce
    /// exactly, and what the equivalence tests compare it against; it is not the edit-path call.
    #[doc(hidden)]
    pub fn retained_bytes_against_full(&self, newer: &CellStore) -> usize {
        let mut bytes = 0usize;
        for (key, page) in &self.pages {
            if newer.pages.get(key).is_some_and(|next| Arc::ptr_eq(next, page)) {
                continue;
            }
            bytes += page.page_len();
        }
        for (key, band) in &self.bands {
            let next = newer.bands.get(key);
            if next.is_some_and(|b| Arc::ptr_eq(b, band)) {
                continue;
            }
            bytes = bytes.saturating_add(Self::unshared_band_bytes(band, next));
        }
        bytes
    }

    /// Bytes the packing itself holds, split by what holds them. Diagnostics for the memory work,
    /// not an exact figure: allocator rounding is not visible from here.
    pub fn packed_breakdown(&self) -> [usize; 8] {
        use std::mem::size_of;
        // rows, bits, kinds, payloads, styles, texts, formulas+arrays+style ids, band overhead
        // (the struct, the number table and its slots)
        let mut out = [0usize; 8];
        for band in self.bands.values() {
            out[7] += size_of::<Band>();
            out[7] += band.numbers.capacity() * size_of::<f64>() + band.number_slots.capacity() * size_of::<u16>();
            out[6] += band.style_ids.capacity() * size_of::<StyleId>();
            out[5] += band.texts.capacity() * size_of::<Arc<str>>() + band.text_index.capacity() * size_of::<(usize, u32)>();
            out[0] += band.rows.capacity() * size_of::<BandRow>();
            out[1] += band.bits.capacity() * size_of::<u64>();
            out[2] += band.kinds.capacity() * size_of::<(u16, u16)>();
            out[3] += band.payloads.capacity() * size_of::<u16>();
            out[4] += band.styles.capacity() * size_of::<(u16, u16)>();
            for row in &band.rows {
                out[6] += row.extras.as_ref().map_or(0, |e| e.formulas.capacity() * (4 + 8) + e.arrays().len() * (4 + 8) + e.wide().len() * 8);
            }
        }
        out
    }

    /// How many bands the store holds: the unit paging will work in, and what tells a probe whether
    /// a file's cells are sparse across rows or packed into a few.
    pub fn band_count(&self) -> usize {
        self.bands.len().saturating_add(self.pages.len())
    }

    /// Bytes the packing itself holds (see [`packed_breakdown`](Self::packed_breakdown)).
    pub fn packed_bytes(&self) -> usize {
        self.packed_breakdown().iter().sum()
    }

    /// The live heap bytes this store reaches, by allocation class: one pass over the bands and
    /// one over the pages (each [`Band::memory`] is O(rows + texts + formulas) for that band), and
    /// `len`/`capacity` arithmetic for everything else. No cell is visited, so the answer is cheap
    /// enough for a UI question even on a million-row sheet.
    ///
    /// The contract — what is counted once, what a snapshot changes, and what is deliberately left
    /// out — is on [`StoreMemory`].
    pub fn memory(&self) -> StoreMemory {
        use std::mem::size_of;
        let mut m = StoreMemory {
            band_index: btree_index_bytes(self.bands.len(), size_of::<(u32, Arc<Band>)>())
                .saturating_add(btree_index_bytes(self.pages.len(), size_of::<(u32, Arc<page::Page>)>())),
            maps: self.row_map.memory().saturating_add(self.col_map.memory()),
            // The file's handle and decoded-band queue are shared with this store's clones and
            // snapshots; the store that has the file charges them, and a caller that also reports
            // the history measures its entries against this workbook (see [`StoreMemory`]).
            page_cache: self
                .page_file
                .as_ref()
                .map_or(0, |file| file.memory())
                .saturating_add(self.pages.values().map(|page| page.memory()).sum::<usize>()),
            ..StoreMemory::default()
        };
        for band in self.bands.values() {
            m.add_band(&band.memory());
        }
        m
    }

    /// Removes every cell in `r` and returns them.
    pub fn take_range(&mut self, r: RangeRef) -> Vec<(CellRef, Cell)> {
        let keys: Vec<CellRef> = self.iter_range(r).map(|(c, _)| c).collect();
        keys.into_iter().filter_map(|c| self.remove(c).map(|cell| (c, cell))).collect()
    }

    /// Reclaims cells that no logical row or column can reach, and normalizes the maps.
    ///
    /// A structural edit changes the row/column map instead of moving cells, so a shift that is
    /// not paired with a delete can leave cells at physical rows or columns whose inverse is gone.
    /// Those cells are invisible to `get`, `iter` and `used_range` but still hold storage and
    /// count in `len`. This walks the live bands and, when a gap in either map could hold one, the
    /// pages that straddle it, drops each unreachable cell and every band that empties, then
    /// merges the map runs, bounds the column free list and returns a contiguous map to the
    /// identity fast path.
    ///
    /// With both maps identity every physical row and column has an inverse, so nothing can be an
    /// orphan and the call returns at once: a clean store (and every freshly opened one) pays
    /// O(1). Pages are found through the *gaps* of the maps — the physical runs no logical row or
    /// column covers — so a row delete probes the band keys of the deleted rows instead of
    /// decoding the sheet. A page is decoded without priming its cache and is only cloned,
    /// rewritten (appended to the page file) and replaced when it actually held an orphan, so a
    /// snapshot keeps reading its own `Arc<Page>` and its old offsets. A page that cannot be
    /// decoded leaves the pass incomplete, which keeps the maps from resetting to identity.
    pub fn compact(&mut self) -> Compacted {
        if self.row_map.is_identity() && self.col_map.is_identity() {
            return Compacted::default();
        }
        let shape = self.map_shape();
        // Compaction drops cells no logical position covers and renumbers the maps, so the cached
        // box is walked again by the next read.
        self.mark_used_stale();
        let mut compacted = Compacted::default();
        let keys: Vec<u32> = self.bands.keys().copied().collect();
        for key in keys {
            if self.bands.get(&key).is_some_and(|band| band.rows.is_empty()) {
                self.bands.remove(&key);
                compacted.bands = compacted.bands.saturating_add(1);
                continue;
            }
            // Look before copying: a band with nothing to reclaim keeps sharing its `Arc` with the
            // undo snapshot, so a second pass (or a pass over a clean store) copies no band.
            let (gone, clear_rows, drop_cells) = match self.bands.get(&key) {
                Some(band) => Self::orphan_plan(band, key, &self.row_map, &self.col_map),
                None => continue,
            };
            if gone == 0 {
                continue;
            }
            let Some(band) = self.bands.get_mut(&key) else { continue };
            let band = Arc::make_mut(band);
            for row_key in clear_rows {
                band.remove_row(row_key);
            }
            for (row_key, col) in drop_cells {
                let _ = band.take(row_key, col);
            }
            compacted.cells = compacted.cells.saturating_add(gone);
            if band.rows.is_empty() {
                self.bands.remove(&key);
                compacted.bands = compacted.bands.saturating_add(1);
            }
        }
        // A page can hold a cell at a position no logical row or column covers; once every page
        // that could is checked, a single run from the origin is identity for every cell left.
        let complete = self.compact_pages(&mut compacted);
        self.len = self.len.saturating_sub(compacted.cells);
        self.row_map.normalize(complete);
        self.col_map.normalize(complete);
        compacted.changed = compacted.cells > 0 || compacted.bands > 0 || compacted.pages > 0 || self.map_shape() != shape;
        compacted
    }

    /// Whether [`compact`](Self::compact) could reclaim anything. `compact` returns at once when
    /// both maps are the identity, and this is that same test, so a caller can use it to skip a
    /// copy of the workbook.
    pub fn needs_compaction(&self) -> bool {
        !self.row_map.is_identity() || !self.col_map.is_identity()
    }

    /// The page half of [`compact`](Self::compact): visits the pages a gap in either map could
    /// leave an orphan in, rewrites the ones that hold one and drops the ones that emptied.
    /// Returns true when every candidate was read and handled (or there were none): only then is
    /// the store free of orphans and a contiguous map may return to the identity fast path.
    fn compact_pages(&mut self, compacted: &mut Compacted) -> bool {
        let mut candidates: BTreeSet<u32> = BTreeSet::new();
        for (lo, hi) in Self::unmapped_runs(self.row_map.is_identity(), &self.row_map.by_physical, self.row_map.next_physical) {
            let (first, last) = (lo / BAND, hi.saturating_sub(1) / BAND);
            candidates.extend(self.pages.range(first..=last).map(|(key, _)| *key));
        }
        if !self.col_map.is_identity() {
            // Bounds are a span over physical columns, so this can over-select a page whose cells
            // sit on both sides of a dropped column; the plan below then finds nothing and the
            // page is left as it was. Only the bounds are read here, never the page.
            let gaps = Self::unmapped_runs(false, &self.col_map.by_physical, self.col_map.next_physical);
            if !gaps.is_empty() {
                candidates.extend(
                    self.pages
                        .iter()
                        .filter(|(_, page)| {
                            page.bounds().is_some_and(|(_, _, first, last)| gaps.iter().any(|(lo, hi)| first <= hi.saturating_sub(1) && *lo <= last))
                        })
                        .map(|(key, _)| *key),
                );
            }
        }
        let mut complete = true;
        let mut rewritten = 0usize;
        for key in candidates {
            let Some(page) = self.pages.get(&key).cloned() else { continue };
            let Some(file) = self.page_file.clone() else {
                // A page without its file cannot be read; keep it and report the pass incomplete.
                complete = false;
                continue;
            };
            // Decode without priming the page cache: a pass over many candidates must not pin a
            // band per page in a queue that would never evict it.
            let Some(band) = page.decode(&file) else {
                complete = false;
                continue;
            };
            let (gone, clear_rows, drop_cells) = Self::orphan_plan(&band, key, &self.row_map, &self.col_map);
            if gone == 0 {
                continue;
            }
            let mut band = Arc::unwrap_or_clone(band);
            for row_key in clear_rows {
                band.remove_row(row_key);
            }
            for (row_key, col) in drop_cells {
                let _ = band.take(row_key, col);
            }
            if band.rows.is_empty() {
                self.pages.remove(&key);
                compacted.cells = compacted.cells.saturating_add(gone);
                compacted.pages = compacted.pages.saturating_add(1);
                continue;
            }
            let Some(bytes) = page::encode_band(&band) else {
                complete = false;
                continue;
            };
            let Some(at) = file.write(&bytes) else {
                complete = false;
                continue;
            };
            rewritten = rewritten.saturating_add(1);
            let page = Arc::new(page::Page::from_band(at, Arc::new(band)));
            // A few rewritten pages are worth keeping decoded for the read that follows a save. A
            // column-gap pass can over-select and a big pass would pin a band per page, so those
            // are left for the page cache to decode once.
            if !(self.col_map.is_identity() && rewritten <= PAGE_PRIME_LIMIT) {
                page.clear();
            } else {
                // A primed band is cache content like a decoded one: it must be charged to the
                // budget and be evictable, or a compaction pass would leak bands past the budget.
                file.cache().note(key, &page, page.cached_bytes());
            }
            self.pages.insert(key, page);
            compacted.cells = compacted.cells.saturating_add(gone);
            compacted.pages = compacted.pages.saturating_add(1);
        }
        complete
    }

    /// The physical runs `[lo, hi)` that no logical row or column covers: the holes between the
    /// mapped runs, from the origin to the watermark of physical keys the map has handed out.
    /// Nothing is stored past that watermark, so these runs bound every cell that could be an
    /// orphan.
    fn unmapped_runs(identity: bool, by_physical: &[RowSegment], next_physical: u32) -> Vec<(u32, u32)> {
        if identity {
            return Vec::new();
        }
        let mut runs = Vec::new();
        let mut at = 0u32;
        for segment in by_physical {
            if segment.physical > at {
                runs.push((at, segment.physical));
            }
            at = at.max(segment.physical.saturating_add(segment.len));
        }
        if next_physical > at {
            runs.push((at, next_physical));
        }
        runs
    }

    /// A cheap fingerprint of the two maps: `Compacted::changed` needs to know whether
    /// normalization merged runs, returned a map to identity or trimmed the column free list.
    fn map_shape(&self) -> (bool, usize, bool, usize, usize) {
        (self.row_map.is_identity(), self.row_map.segments.len(), self.col_map.is_identity(), self.col_map.segments.len(), self.col_map.free.len())
    }

    /// The rows to clear and the cells to drop of one live band whose physical row or column has
    /// no logical inverse: `(cells, rows, (row, column))`. Rows are visited by key, the cells of a
    /// mapped row by position; the grid is never walked.
    fn orphan_plan(band: &Band, key: u32, rows: &RowMap, cols: &ColMap) -> (usize, Vec<u32>, Vec<(u32, u32)>) {
        let mut removed = 0usize;
        let mut clear_rows: Vec<u32> = Vec::new();
        let mut drop_cells: Vec<(u32, u32)> = Vec::new();
        for row in &band.rows {
            let physical = key.saturating_mul(BAND).saturating_add(row.key);
            if rows.inverse(physical).is_none() {
                removed = removed.saturating_add(row.cells as usize);
                clear_rows.push(row.key);
                continue;
            }
            // An identity column map maps every column in the grid; only a cell past the last grid
            // column can be orphaned, so the per-cell walk is skipped for a row that stops short.
            let scan = !cols.is_identity() || row.column_span(band).is_some_and(|(_, last)| last >= cella_core::MAX_COLS);
            if !scan {
                continue;
            }
            for i in 0..row.cells as usize {
                let col = row.col_at(band, i);
                if cols.inverse(col).is_none() {
                    removed = removed.saturating_add(1);
                    drop_cells.push((row.key, col));
                }
            }
        }
        (removed, clear_rows, drop_cells)
    }

    /// Rows at or after `from` moved by `delta` (positive = down). This changes the row map, not
    /// the bands: an insert above a million rows is a handful of segments, not a million cell
    /// moves.
    pub fn shift_rows(&mut self, from: u32, delta: i64) {
        self.row_map.shift(from, delta);
        // Every cell at or after `from` has a new logical row.
        self.mark_used_stale();
    }

    pub fn shift_cols(&mut self, from: u32, delta: i64) {
        self.ensure_col_map();
        self.col_map.shift(from, delta);
        self.mark_used_stale();
    }

    /// Shifts cells within rows `r0..=r1` and columns `>= from` horizontally.
    pub fn shift_cols_in_rows(&mut self, r0: u32, r1: u32, from: u32, delta: i64) {
        let moved = self.take_pred(|c| (r0..=r1).contains(&c.row) && c.col >= from);
        for (c, cell) in moved {
            let nc = c.col as i64 + delta;
            if (0..cella_core::MAX_COLS as i64).contains(&nc) {
                self.set(CellRef::new(c.row, nc as u32), cell);
            }
        }
    }

    /// Shifts cells within columns `c0..=c1` and rows `>= from` vertically.
    pub fn shift_rows_in_cols(&mut self, c0: u32, c1: u32, from: u32, delta: i64) {
        let moved = self.take_pred(|c| c.row >= from && (c0..=c1).contains(&c.col));
        for (c, cell) in moved {
            let nr = c.row as i64 + delta;
            if (0..cella_core::MAX_ROWS as i64).contains(&nr) {
                self.set(CellRef::new(nr as u32, c.col), cell);
            }
        }
    }

    fn take_pred(&mut self, keep: impl Fn(CellRef) -> bool) -> Vec<(CellRef, Cell)> {
        let keys: Vec<CellRef> = self.iter().filter(|(c, _)| keep(*c)).map(|(c, _)| c).collect();
        keys.into_iter().filter_map(|c| self.remove(c).map(|cell| (c, cell))).collect()
    }
}

/// A cell's content for logical equality: the parsed formula tree and its cached byte count are
/// derived (one side may have reparsed), so compare the text and the array/dynamic flags.
fn same_cell(a: &Cell, b: &Cell) -> bool {
    if a.value != b.value || a.style != b.style {
        return false;
    }
    match (&a.formula, &b.formula) {
        (None, None) => true,
        (Some(a), Some(b)) => a.text == b.text && a.array == b.array && a.dynamic == b.dynamic,
        _ => false,
    }
}

fn row_has_value(band: &Band, row: &BandRow, col: u32) -> bool {
    match row.index_of(band, col) {
        Some(i) => row.kind_at(band, i) != EMPTY || row.formula_at(col).is_some(),
        None => false,
    }
}

impl BandRow {
    fn value_nonempty(&self, band: &Band, col: u32) -> bool {
        match self.index_of(band, col) {
            Some(i) => match self.kind_at(band, i) {
                EMPTY => false,
                TEXT => !band.text(u64::from(self.payload_at(band, i))).is_empty(),
                TEXT_WIDE => !band.text(self.wide_at(band, i)).is_empty(),
                _ => true,
            },
            None => false,
        }
    }
}

// Serialization keeps the shape the model always wrote — bands of rows of columns of `Cell` — so a
// workbook saved before the packing still loads, and one saved now stays readable by anything that
// reads the old shape.

#[derive(Serialize, Deserialize)]
struct WireBand {
    #[serde(default)]
    rows: BTreeMap<u32, BTreeMap<u32, Cell>>,
}

impl Serialize for CellStore {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut wire: BTreeMap<u32, WireBand> = BTreeMap::new();
        for (at, cell) in self.iter() {
            wire.entry(at.row / BAND).or_insert_with(|| WireBand { rows: BTreeMap::new() });
            wire.get_mut(&(at.row / BAND)).and_then(|b| b.rows.entry(at.row).or_default().insert(at.col, cell));
        }
        let mut st = s.serialize_struct("CellStore", 2)?;
        st.serialize_field("bands", &wire)?;
        st.serialize_field("len", &self.len)?;
        st.end()
    }
}

impl<'de> Deserialize<'de> for CellStore {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            #[serde(default)]
            bands: BTreeMap<u32, WireBand>,
            #[serde(default)]
            len: usize,
        }
        let wire = Wire::deserialize(d)?;
        let mut store = CellStore::new();
        // Band keys are derived state: put each row where its number says, and recount.
        for band in wire.bands.values() {
            for (row, cols) in &band.rows {
                let mut cells: Vec<(u32, Cell)> = cols.iter().map(|(col, cell)| (*col, cell.clone())).collect();
                store.set_row(*row, &mut cells);
            }
        }
        store.len = store.iter().count().max(store.len);
        let _ = wire.len;
        Ok(store)
    }
}
#[cfg(test)]
mod packed_tests {
    use super::*;
    use crate::Cell;
    use cella_core::{CellRef, RangeRef, Value};

    fn plain(n: u32) -> Cell {
        Cell::value(Value::Number(n as f64))
    }

    /// One row per width, one band boundary, holes in both directions.
    fn store() -> CellStore {
        let mut s = CellStore::new();
        for (r, c, v) in
            [(0u32, 0u32, 1u32), (0, 1, 2), (0, 70, 3), (1, 0, 4), (1, 64, 5), (5, 3, 6), (63, 70, 7), (64, 0, 8), (64, 70, 9), (200, 1, 10)]
        {
            s.set(CellRef::new(r, c), plain(v));
        }
        s
    }

    /// A dense row read through a narrow window: the kind and payload indexes must not slip.
    #[test]
    fn packed_walk_over_a_dense_row() {
        let mut s = CellStore::new();
        for c in 0..200u32 {
            s.set(CellRef::new(3, c), plain(10_000 + c));
        }
        for (c0, c1) in [(0u32, 0u32), (26, 26), (26, 30), (63, 64), (64, 70), (127, 200), (199, 199)] {
            let range = RangeRef::new(CellRef::new(3, c0), CellRef::new(3, c1));
            let want: Vec<(CellRef, f64)> = s.iter_range(range).filter_map(|(at, c)| c.value.as_f64().map(|n| (at, n))).collect();
            let mut got = Vec::new();
            s.visit_packed(range, &mut |at, packed| {
                if let Some(Packed::Number(n)) = packed {
                    got.push((at, n));
                }
                true
            });
            assert_eq!(got, want, "range {}", range.a1());
        }
    }

    /// A row built through `BandBatch` may hold a slot with a style and no value: the packed walk
    /// must skip it exactly as `iter_range` does. The report that found this had `SUM(AA74:AA81)`
    /// answer 83,551.75 instead of 43,045.08 because the slot stopped the walk midway and the
    /// caller folded the block a second time.
    #[test]
    fn packed_walk_skips_a_slot_with_no_value() {
        let mut batch = BandBatch::new();
        let mut cells = vec![(3u32, plain(1)), (4, Cell { value: Value::Empty, formula: None, style: StyleId(0) }), (5, plain(3))];
        batch.set_row(9, &mut cells);
        let mut s = CellStore::new();
        s.insert_batch(batch);
        let range = RangeRef::new(CellRef::new(9, 0), CellRef::new(9, 10));
        let want: Vec<(CellRef, f64)> = s.iter_range(range).filter_map(|(at, c)| c.value.as_f64().map(|n| (at, n))).collect();
        let mut got = Vec::new();
        let mut needs_host = false;
        s.visit_packed(range, &mut |at, packed| {
            match packed {
                Some(Packed::Number(n)) => got.push((at, n)),
                None => needs_host = true,
                other => panic!("{} came out as {other:?}", at.a1()),
            }
            true
        });
        assert_eq!(got, want);
        assert!(!needs_host, "a value-less slot must not look like a formula");
    }

    /// The packed walk must hand out exactly the cells `iter_range` does, in the same order.
    #[test]
    fn packed_walk_matches_iter_range() {
        let s = store();
        for (r0, r1, c0, c1) in [
            (0u32, 300u32, 0u32, 100u32),
            (0, 0, 0, 0),
            (0, 0, 1, 1),
            (0, 0, 2, 69),
            (0, 0, 70, 70),
            (0, 1, 0, 200),
            (0, 63, 70, 70),
            (1, 1, 64, 64),
            (63, 64, 0, 0),
            (64, 200, 0, 70),
            (201, 400, 0, 200),
            (0, 400, 71, 200),
        ] {
            let range = RangeRef::new(CellRef::new(r0, c0), CellRef::new(r1, c1));
            let want: Vec<(CellRef, f64)> = s.iter_range(range).filter_map(|(at, cell)| cell.value.as_f64().map(|n| (at, n))).collect();
            let mut got: Vec<(CellRef, f64)> = Vec::new();
            let done = s.visit_packed(range, &mut |at, packed| {
                match packed {
                    Some(Packed::Number(n)) => got.push((at, n)),
                    other => panic!("{} came out as {other:?}", at.a1()),
                }
                true
            });
            assert!(done);
            assert_eq!(got, want, "range {}", range.a1());
        }
    }

    /// The same equality on a mapped sheet: a column-mapped store walks the column map's runs, so
    /// the cells come out at their logical columns and the whole range stays row-major.
    #[test]
    fn packed_walk_matches_iter_range_on_a_column_mapped_sheet() {
        for (rows, cols) in [(0u32, 2u32), (3, 0), (3, 2)] {
            let mut s = store();
            if rows > 0 {
                s.shift_rows(3, i64::from(rows));
            }
            if cols > 0 {
                s.shift_cols(1, i64::from(cols));
            }
            let mut ranges = vec![
                // A single cell, then a range that crosses the column map's segment boundary.
                (0u32, 0u32, 0u32, 0u32),
                (0, 0, 0, 2 + cols),
                // A whole row, and a range that starts and ends inside one segment.
                (0, 0, 0, u32::MAX),
                (0, 3 + cols, 64 + cols, 64 + cols),
                // A range that spans both axes' segment boundaries, and one past the last row.
                (0, 400, 0, 200 + cols),
                (rows, rows + 200, 0, 100 + cols),
                (0, 400, 71 + cols, 200 + cols),
            ];
            // The inserted columns themselves hold nothing.
            if cols > 0 {
                ranges.push((0, 0, 1, cols));
            }
            for (r0, r1, c0, c1) in ranges {
                let range = RangeRef::new(CellRef::new(r0, c0), CellRef::new(r1, c1));
                let want: Vec<(CellRef, f64)> = s.iter_range(range).filter_map(|(at, cell)| cell.value.as_f64().map(|n| (at, n))).collect();
                let mut got: Vec<(CellRef, f64)> = Vec::new();
                let done = s.visit_packed(range, &mut |at, packed| {
                    match packed {
                        Some(Packed::Number(n)) => got.push((at, n)),
                        other => panic!("{} came out as {other:?}", at.a1()),
                    }
                    true
                });
                assert!(done);
                assert_eq!(got, want, "range {} with {rows} rows and {cols} cols inserted", range.a1());
                assert!(got.windows(2).all(|w| (w[0].0.row, w[0].0.col) < (w[1].0.row, w[1].0.col)), "range {} is not row-major", range.a1());
            }
        }
    }

    /// The value-less slot a `BandBatch` row can hold must stay invisible on a column-mapped
    /// sheet too: it is neither a number nor a formula the evaluator owns.
    #[test]
    fn packed_walk_on_a_column_map_skips_an_empty_slot() {
        let mut batch = BandBatch::new();
        let mut cells = vec![(3u32, plain(1)), (4, Cell { value: Value::Empty, formula: None, style: StyleId(0) }), (5, plain(3))];
        batch.set_row(9, &mut cells);
        let mut s = CellStore::new();
        s.insert_batch(batch);
        s.shift_cols(1, 2);
        let range = RangeRef::new(CellRef::new(9, 0), CellRef::new(9, 10));
        let want: Vec<(CellRef, f64)> = s.iter_range(range).filter_map(|(at, c)| c.value.as_f64().map(|n| (at, n))).collect();
        let mut got = Vec::new();
        let mut needs_host = false;
        s.visit_packed(range, &mut |at, packed| {
            match packed {
                Some(Packed::Number(n)) => got.push((at, n)),
                None => needs_host = true,
                other => panic!("{} came out as {other:?}", at.a1()),
            }
            true
        });
        assert_eq!(got, want);
        assert!(!needs_host, "a value-less slot must not look like a formula");
    }

    /// Past the column-run cap the walk builds cells through `iter_range` instead of repeating
    /// the row walk once per run; the answer must not change.
    #[test]
    fn packed_walk_falls_back_past_the_segment_cap() {
        let mut s = CellStore::new();
        for col in 0..40u32 {
            s.set(CellRef::new(2, col), plain(col + 1));
        }
        for at in [1u32, 5, 9, 13, 17, 21, 25, 29, 33] {
            s.shift_cols(at, 1);
        }
        assert!(s.col_map.segments_in(0, u32::MAX).len() > MAX_PACKED_COLUMN_SEGMENTS, "the fallback is the path under test");
        let range = RangeRef::new(CellRef::new(2, 0), CellRef::new(2, u32::MAX));
        let want: Vec<(CellRef, f64)> = s.iter_range(range).filter_map(|(at, cell)| cell.value.as_f64().map(|n| (at, n))).collect();
        let mut got: Vec<(CellRef, f64)> = Vec::new();
        let done = s.visit_packed(range, &mut |at, packed| {
            match packed {
                Some(Packed::Number(n)) => got.push((at, n)),
                other => panic!("{} came out as {other:?}", at.a1()),
            }
            true
        });
        assert!(done);
        assert_eq!(got, want);
    }
}

/// The page file a store can spill completed bands into, and the band codec it uses.
///
/// A page holds one finished band: the four stripes, the number table, the text table and the
/// style table. The format is deliberately small and self-checking on read; a page that does not
/// parse is reported as missing rather than trusted. The file lives beside the document it belongs
/// to (the system temp directory when that is unusable) and is never referenced by a document on
/// disk: it is unlinked as soon as it is open on Unix, delete-on-close on Windows, and removed by
/// the last store that owns it when it drops.
mod page {
    use std::collections::VecDeque;
    use std::fs::{File, OpenOptions};
    use std::io::{Read, Seek, SeekFrom, Write};

    use flate2::Compression;
    use flate2::read::ZlibDecoder;
    use flate2::write::ZlibEncoder;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};

    use cella_core::{CellError, CellRef, RangeRef, Value};

    use super::{Band, BandRow, BandTag, ERRORS, Formula, PageCacheStats, RareRow, RowExtras, StyleId, error_code};

    const MAGIC: u32 = 0x4750_4231; // "GPB1"
    const VERSION: u8 = 1;
    const MAX_PAGE: usize = 64 * 1024 * 1024;
    const MAX_ROWS: u32 = 64;
    const MAX_COLS: u32 = 16_384;
    const MAX_CELLS: usize = 1_048_576;
    const MAX_TEXT: usize = 16 * 1024 * 1024;
    const MAX_DEPTH: u32 = 8;
    /// How many names are tried per directory before paging is given up. `create_new` still
    /// refuses a collision; the loop only moves to the next name.
    const SPILL_ATTEMPTS: u32 = 16;
    /// The longest document stem the spill name carries (`[A-Za-z0-9_-]`, so bytes are chars).
    pub(super) const SPILL_STEM_MAX: usize = 32;

    #[derive(Clone, Copy, Debug)]
    pub(super) struct PageRef {
        offset: u64,
        len: u32,
    }

    /// A completed band on disk, with the range its rows cover so `used_range` does not have to
    /// load every page. `band` caches the decoded copy after the first read; a corrupt page is
    /// remembered as `None` and behaves as missing.
    #[derive(Debug)]
    enum Cached {
        Miss,
        Corrupt,
        Band(Arc<Band>),
    }

    /// One decoded band's claim on the budget: the page that installed it, under which generation,
    /// and the bytes it was charged.
    #[derive(Debug)]
    struct Entry {
        key: u32,
        generation: u64,
        page: Weak<Page>,
        bytes: usize,
    }

    #[derive(Debug, Default)]
    struct CacheState {
        /// Oldest first. An entry whose page has been dropped stays here until the budget sends it
        /// to the front, so only trimming costs a `Weak` upgrade.
        queue: VecDeque<Entry>,
        bytes: usize,
    }

    /// The decoded-band cache of one page file: a byte budget, a queue of installs, and the
    /// counters a memory report needs. There is one decoded copy per page and the pages belong to
    /// one file, so the budget belongs to the file: clones and undo snapshots share it. The budget
    /// is fixed when the file is created; a clone with a different budget is not offered.
    #[derive(Debug)]
    pub(super) struct PageCache {
        budget: usize,
        state: Mutex<CacheState>,
        hits: AtomicU64,
        misses: AtomicU64,
        evictions: AtomicU64,
        corrupt: AtomicU64,
        spill_failures: AtomicU64,
        max_entry_bytes: AtomicUsize,
    }

    impl PageCache {
        fn new(budget: usize) -> PageCache {
            PageCache {
                budget,
                state: Mutex::new(CacheState::default()),
                hits: AtomicU64::new(0),
                misses: AtomicU64::new(0),
                evictions: AtomicU64::new(0),
                corrupt: AtomicU64::new(0),
                spill_failures: AtomicU64::new(0),
                max_entry_bytes: AtomicUsize::new(0),
            }
        }

        fn state(&self) -> MutexGuard<'_, CacheState> {
            self.state.lock().unwrap_or_else(PoisonError::into_inner)
        }

        pub(super) fn hit(&self) {
            self.hits.fetch_add(1, Ordering::Relaxed);
        }

        pub(super) fn miss(&self) {
            self.misses.fetch_add(1, Ordering::Relaxed);
        }

        pub(super) fn corrupt(&self) {
            self.corrupt.fetch_add(1, Ordering::Relaxed);
        }

        fn spill_failure(&self) {
            self.spill_failures.fetch_add(1, Ordering::Relaxed);
        }

        /// Charges a band a decode just installed, then trims the queue back to the budget, oldest
        /// first. The entry just added is last, so the trim always leaves at least one band: a band
        /// larger than the whole budget stays (evicting it would only decode it again) and
        /// `max_entry_bytes` makes that visible.
        pub(super) fn note(&self, key: u32, page: &Arc<Page>, bytes: usize) {
            self.max_entry_bytes.fetch_max(bytes, Ordering::Relaxed);
            let generation = page.generation();
            let mut state = self.state();
            // A second note for the same install of the same page would charge the band twice.
            // `Page::load` hands the byte count to exactly one caller, so this cannot happen
            // today; the check is O(1) and also keeps two pages at the same key apart, since a
            // rewritten page starts its generations over.
            if state
                .queue
                .back()
                .is_some_and(|last| last.key == key && last.generation == generation && std::ptr::eq(last.page.as_ptr(), Arc::as_ptr(page)))
            {
                return;
            }
            state.bytes = state.bytes.saturating_add(bytes);
            state.queue.push_back(Entry { key, generation, page: Arc::downgrade(page), bytes });
            while state.bytes > self.budget && state.queue.len() > 1 {
                if !self.evict_oldest(&mut state) {
                    break;
                }
            }
        }

        /// Drops the oldest entry's band (when its page still exists and has not decoded again) and
        /// takes the recorded bytes off the total. A dead `Weak` means the page, and the band it
        /// held, are already gone: the bytes are still ours to subtract.
        fn evict_oldest(&self, state: &mut CacheState) -> bool {
            let Some(entry) = state.queue.pop_front() else { return false };
            if let Some(page) = entry.page.upgrade() {
                page.clear_if(entry.generation);
            }
            state.bytes = state.bytes.saturating_sub(entry.bytes);
            self.evictions.fetch_add(1, Ordering::Relaxed);
            true
        }

        /// Heap bytes of the cache: its recency queue's entries (its counters and the budget live
        /// in the struct, which the page file charges).
        pub(super) fn memory(&self) -> usize {
            let state = self.state();
            state.queue.capacity().saturating_mul(std::mem::size_of::<Entry>())
        }
        pub(super) fn stats(&self) -> PageCacheStats {
            let state = self.state();
            PageCacheStats {
                budget: self.budget,
                bytes: state.bytes,
                entries: state.queue.len(),
                hits: self.hits.load(Ordering::Relaxed),
                misses: self.misses.load(Ordering::Relaxed),
                evictions: self.evictions.load(Ordering::Relaxed),
                corrupt: self.corrupt.load(Ordering::Relaxed),
                spill_failures: self.spill_failures.load(Ordering::Relaxed),
                max_entry_bytes: self.max_entry_bytes.load(Ordering::Relaxed),
            }
        }
    }

    #[derive(Debug)]
    pub(super) struct Page {
        at: PageRef,
        bounds: Option<(u32, u32, u32, u32)>,
        formulas: bool,
        owned: bool,
        band: Mutex<Cached>,
        /// Bumped whenever a decoded band is installed or dropped, so a cache entry can tell whether
        /// the install its bytes were charged for is still the one in the page. Without it a key
        /// decoded, evicted and decoded again would take its bytes off the budget twice.
        generation: AtomicU64,
        /// The [`Band::heap_bytes`] of the band in `band`; zero when nothing decoded is held.
        cached_bytes: AtomicUsize,
    }

    impl Page {
        pub(super) fn new(at: PageRef, bounds: Option<(u32, u32, u32, u32)>, formulas: bool, owned: bool) -> Page {
            Page { at, bounds, formulas, owned, band: Mutex::new(Cached::Miss), generation: AtomicU64::new(0), cached_bytes: AtomicUsize::new(0) }
        }

        /// The page a rewritten band becomes: the bounds and flags are recomputed from the new
        /// band exactly as [`spill_band`](super::CellStore::spill_band) computes them, and the
        /// decoded copy is installed and measured so the read that follows does not decode it
        /// again, and so the cache can charge it.
        pub(super) fn from_band(at: PageRef, band: Arc<Band>) -> Page {
            let bytes = band.heap_bytes();
            let bounds = band_bounds(&band);
            let formulas = band.rows.iter().any(|r| !r.formulas().is_empty());
            let owned = band.rows.iter().any(|r| !r.formulas().is_empty() || !r.arrays().is_empty());
            Page {
                at,
                bounds,
                formulas,
                owned,
                band: Mutex::new(Cached::Band(band)),
                generation: AtomicU64::new(1),
                cached_bytes: AtomicUsize::new(bytes),
            }
        }

        /// Reads and decodes the page without touching the cached copy or the cache's queue: a
        /// compaction pass looks at pages it may leave alone, and must not pin a decoded band per
        /// page (the queue only evicts pages it was told about).
        pub(super) fn decode(&self, file: &PageFile) -> Option<Arc<Band>> {
            file.decodes.fetch_add(1, Ordering::Relaxed);
            let bytes = file.read(self.at)?;
            decode_band(&bytes).map(Arc::new)
        }

        pub(super) fn bounds(&self) -> Option<(u32, u32, u32, u32)> {
            self.bounds
        }

        pub(super) fn has_formulas(&self) -> bool {
            self.formulas
        }

        pub(super) fn has_owned(&self) -> bool {
            self.owned
        }

        pub(super) fn page_len(&self) -> usize {
            usize::try_from(self.at.len).unwrap_or(0)
        }

        /// The current install's generation; the cache records it with the charge so an eviction
        /// can tell whether the band it is looking at is the one it charged for.
        pub(super) fn generation(&self) -> u64 {
            self.generation.load(Ordering::Relaxed)
        }

        /// The bytes the installed band was charged, or 0 when nothing is decoded.
        pub(super) fn cached_bytes(&self) -> usize {
            self.cached_bytes.load(Ordering::Relaxed)
        }

        /// Heap bytes of this page: its header and the decoded band it caches (zero when nothing
        /// is decoded). The decoded band is a copy of a page and is not in the store's band map.
        pub(super) fn memory(&self) -> usize {
            std::mem::size_of::<Page>().saturating_add(self.cached_bytes())
        }

        fn lock_band(&self) -> MutexGuard<'_, Cached> {
            self.band.lock().unwrap_or_else(PoisonError::into_inner)
        }

        /// The decoded band, reading and validating the page on first use. `Some(bytes)` says this
        /// call installed the band, and is the [`Band::heap_bytes`] the cache must charge; a cached
        /// read and a corrupt page charge nothing. Exactly one caller sees the `Some`: the install
        /// and the decision happen under the page's mutex.
        pub(super) fn load(&self, file: &PageFile) -> (Option<Arc<Band>>, Option<usize>) {
            match &*self.lock_band() {
                Cached::Band(band) => return (Some(band.clone()), None),
                Cached::Corrupt => return (None, None),
                Cached::Miss => {}
            }
            let decoded = file.read(self.at).and_then(|bytes| decode_band(&bytes).map(Arc::new));
            let mut guard = self.lock_band();
            match &*guard {
                Cached::Band(band) => return (Some(band.clone()), None),
                Cached::Corrupt => return (None, None),
                Cached::Miss => {}
            }
            match decoded {
                Some(band) => {
                    let bytes = band.heap_bytes();
                    *guard = Cached::Band(band.clone());
                    self.generation.fetch_add(1, Ordering::Relaxed);
                    self.cached_bytes.store(bytes, Ordering::Relaxed);
                    (Some(band), Some(bytes))
                }
                None => {
                    *guard = Cached::Corrupt;
                    (None, None)
                }
            }
        }

        /// Drops the decoded copy; a later read decodes the page again. A corrupt page stays
        /// remembered as corrupt rather than being retried on every access. The generation moves on
        /// so a charge already queued for the dropped band cannot clear a later install.
        pub(super) fn clear(&self) {
            let mut guard = self.lock_band();
            if matches!(&*guard, Cached::Band(_)) {
                *guard = Cached::Miss;
                self.cached_bytes.store(0, Ordering::Relaxed);
                self.generation.fetch_add(1, Ordering::Relaxed);
            }
        }

        /// Drops the decoded band only when `generation` is still the install the caller charged,
        /// and returns the bytes that were charged for it. A page that decoded again (or was
        /// cleared) keeps its band: the queue entry still goes away, since its bytes no longer
        /// describe what the page holds.
        pub(super) fn clear_if(&self, generation: u64) -> Option<usize> {
            let mut guard = self.lock_band();
            if self.generation.load(Ordering::Relaxed) != generation || !matches!(&*guard, Cached::Band(_)) {
                return None;
            }
            *guard = Cached::Miss;
            self.generation.fetch_add(1, Ordering::Relaxed);
            Some(self.cached_bytes.swap(0, Ordering::Relaxed))
        }

        #[cfg(test)]
        pub(super) fn is_cached(&self) -> bool {
            matches!(&*self.lock_band(), Cached::Band(_))
        }
    }

    /// A page file pages are appended to. One handle is shared by every clone of a store and by
    /// its undo snapshots; writes and reads serialise on the mutex, which is fine for the read path
    /// a viewport exercises (one page at a time).
    #[derive(Debug)]
    pub(super) struct PageFile {
        path: PathBuf,
        file: Mutex<Option<File>>,
        /// How many pages [`Page::decode`] has read through this file. Compaction of a store with
        /// nothing to reclaim must decode none, and a test can prove it because the count belongs
        /// to this file (not to the process, where parallel tests would see each other's reads).
        decodes: AtomicU64,
        /// Set by the first write the file refuses or fails: from then on every band stays live
        /// rather than retrying the same failing syscall. The pages already written stay readable,
        /// so a full disk or a dropped share costs memory, never cells.
        spill_failed: AtomicBool,
        /// The decoded bands read through this file. It lives here because the budget has to: there
        /// is one decoded copy per page, the pages belong to one file, and so the file's owner,
        /// clones and undo snapshots all share one cache.
        cache: PageCache,
    }

    impl PageFile {
        /// A page file in `dir` (the document's folder), falling back to the system temp
        /// directory. `hint` is the document's stem, sanitized into the name. `None` means no
        /// spill file could be created anywhere, and the caller keeps the store live.
        pub(super) fn new(dir: Option<&Path>, hint: Option<&str>, cache_bytes: usize) -> Option<PageFile> {
            let temp = std::env::temp_dir();
            PageFile::new_in(dir, hint, cache_bytes, &temp)
        }

        /// [`new`](Self::new) with the fallback directory spelled out. Tests use it to make both
        /// candidates unusable, which the process-global environment cannot be asked to do without
        /// mutating it from a parallel test.
        pub(super) fn new_in(dir: Option<&Path>, hint: Option<&str>, cache_bytes: usize, fallback: &Path) -> Option<PageFile> {
            let candidates = spill_candidates(dir, fallback);
            if candidates.is_empty() {
                return None;
            }
            // A counter, not the clock: `SystemTime::now` traps on wasm. The pid (in the temp
            // fallback) separates processes; `create_new` still refuses a collision, and the loop
            // tries the next name rather than trusting an existing file.
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let base = NEXT.fetch_add(1, Ordering::Relaxed);
            let stem = spill_stem(hint);
            for (dir, in_temp) in candidates {
                let pid = if in_temp { Some(std::process::id()) } else { None };
                for attempt in 0..SPILL_ATTEMPTS {
                    let nonce = base.saturating_mul(u64::from(SPILL_ATTEMPTS)).saturating_add(u64::from(attempt));
                    let path = dir.join(spill_file_name(&stem, pid, nonce));
                    if let Some(file) = create_spill_file(&path) {
                        return Some(PageFile {
                            path,
                            file: Mutex::new(Some(file)),
                            decodes: AtomicU64::new(0),
                            spill_failed: AtomicBool::new(false),
                            cache: PageCache::new(cache_bytes),
                        });
                    }
                }
            }
            None
        }

        /// The decoded-band cache this file's pages are read through.
        pub(super) fn cache(&self) -> &PageCache {
            &self.cache
        }

        /// Where the spill was created. On Unix the name is already gone, and on Windows it goes
        /// when the last handle closes; see [`super::CellStore::page_file_path`].
        pub(super) fn path(&self) -> &Path {
            &self.path
        }

        /// Heap bytes of the file: the handle, its name, and the decoded-band cache it owns.
        pub(super) fn memory(&self) -> usize {
            std::mem::size_of::<PageFile>().saturating_add(self.path.as_os_str().len()).saturating_add(self.cache.memory())
        }

        /// Whether a write has already failed (see the field's comment).
        pub(super) fn has_failed(&self) -> bool {
            self.spill_failed.load(Ordering::Relaxed)
        }

        /// The file handle is gone, as if the volume holding the spill had been pulled: later
        /// writes fail and latch, and pages already written can no longer be read. Tests only.
        #[cfg(test)]
        pub(super) fn close_handle(&self) {
            if let Ok(mut guard) = self.file.lock() {
                guard.take();
            }
        }

        /// Pages decoded through [`Page::decode`] so far (see the field's comment).
        #[cfg(test)]
        pub(super) fn decodes(&self) -> u64 {
            self.decodes.load(Ordering::Relaxed)
        }

        /// Overwrites `len` bytes at `offset` with zeros, as a disk fault or a foreign writer would:
        /// the page can no longer be decoded. Through the file's own handle, because on Unix the
        /// name is gone as soon as the file is open. Tests only.
        #[cfg(test)]
        pub(super) fn corrupt(&self, offset: u64, len: usize) -> bool {
            let Ok(mut guard) = self.file.lock() else { return false };
            let Some(file) = guard.as_mut() else { return false };
            if file.seek(SeekFrom::Start(offset)).is_err() {
                return false;
            }
            file.write_all(&vec![0u8; len]).is_ok()
        }

        /// Appends `bytes` and returns where they landed. A page larger than the cap is refused
        /// before it is written. A refusal or a failed write is counted as a spill failure; the
        /// caller leaves the band live, so paging never loses cells. The first I/O failure also
        /// latches the file shut: every later `write` returns `None` without touching the disk.
        pub(super) fn write(&self, bytes: &[u8]) -> Option<PageRef> {
            if self.has_failed() {
                return None;
            }
            match self.append(bytes) {
                Ok(at) => Some(at),
                Err(failure) => {
                    self.note_failure(failure);
                    None
                }
            }
        }

        /// Counts a spill failure, and latches the file shut on the first I/O one: every band that
        /// could not be written stays live.
        pub(super) fn note_failure(&self, failure: SpillFailure) {
            self.cache.spill_failure();
            if failure == SpillFailure::Io {
                self.spill_failed.store(true, Ordering::Relaxed);
            }
        }

        fn append(&self, bytes: &[u8]) -> Result<PageRef, SpillFailure> {
            if bytes.is_empty() || bytes.len() > MAX_PAGE {
                return Err(SpillFailure::TooLarge);
            }
            let packed = pack(bytes).ok_or(SpillFailure::TooLarge)?;
            let mut at = self.write_packed(&[packed.as_slice()])?;
            at.pop().ok_or(SpillFailure::Io)
        }

        /// Writes pages that a caller already deflated (see [`pack`]), laid end to end in one
        /// append, and hands back where each landed. One syscall a chunk instead of one a page is
        /// what makes a whole-sheet spill affordable: a million-row snapshot is fifteen thousand
        /// pages, and a page-at-a-time append is most of a second of syscalls on its own.
        ///
        /// The parts must each be a valid page (deflated, non-empty, within the cap). A part that
        /// is not refuses the whole batch, and every band it carried stays live.
        pub(super) fn write_packed(&self, parts: &[&[u8]]) -> Result<Vec<PageRef>, SpillFailure> {
            if self.has_failed() {
                return Err(SpillFailure::Io);
            }
            let total: usize = parts.iter().map(|part| part.len()).sum();
            if parts.iter().any(|part| part.is_empty() || part.len() > MAX_PAGE) {
                return Err(SpillFailure::TooLarge);
            }
            let mut joined = Vec::with_capacity(total);
            for part in parts {
                joined.extend_from_slice(part);
            }
            let mut guard = self.file.lock().map_err(|_| SpillFailure::Io)?;
            let file = guard.as_mut().ok_or(SpillFailure::Io)?;
            let offset = file.seek(SeekFrom::End(0)).map_err(|_| SpillFailure::Io)?;
            file.write_all(&joined).map_err(|_| SpillFailure::Io)?;
            drop(guard);
            let mut at = Vec::with_capacity(parts.len());
            let mut cursor = 0u64;
            for part in parts {
                let len = u32::try_from(part.len()).map_err(|_| SpillFailure::TooLarge)?;
                at.push(PageRef { offset: offset.saturating_add(cursor), len });
                cursor = cursor.saturating_add(part.len() as u64);
            }
            Ok(at)
        }

        fn read(&self, at: PageRef) -> Option<Vec<u8>> {
            let len = usize::try_from(at.len).ok()?;
            if len == 0 || len > MAX_PAGE {
                return None;
            }
            let mut guard = self.file.lock().ok()?;
            let file = guard.as_mut()?;
            file.seek(SeekFrom::Start(at.offset)).ok()?;
            let mut packed = vec![0u8; len];
            file.read_exact(&mut packed).ok()?;
            drop(guard);
            let mut decoder = ZlibDecoder::new(packed.as_slice());
            let mut bytes = Vec::new();
            decoder.by_ref().take(MAX_PAGE as u64 + 1).read_to_end(&mut bytes).ok()?;
            (bytes.len() <= MAX_PAGE).then_some(bytes)
        }
    }

    impl Drop for PageFile {
        fn drop(&mut self) {
            if let Ok(mut guard) = self.file.lock()
                && let Some(file) = guard.take()
            {
                drop(file);
            }
            // The belt to the braces above: on Unix the name was already removed and on Windows
            // the handle's close removed it; elsewhere (or when the last handle was not ours) this
            // is what removes it.
            let _ = std::fs::remove_file(&self.path);
        }
    }

    /// A band's bytes as a page holds them: deflated with the fastest level the format uses, or
    /// `None` when the result does not fit a page. Split out of [`PageFile::append`] so a spill can
    /// deflate its pages on several threads and then write them together.
    pub(super) fn pack(bytes: &[u8]) -> Option<Vec<u8>> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(bytes).ok()?;
        let packed = encoder.finish().ok()?;
        (!packed.is_empty() && packed.len() <= MAX_PAGE).then_some(packed)
    }

    /// Why [`PageFile::append`] returned no `PageRef`. A page too large for the format stays live
    /// and the next band is still tried; an I/O failure latches the file shut.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum SpillFailure {
        TooLarge,
        Io,
    }

    /// The directories a spill is tried in, in order, each marked with whether it is the temp
    /// fallback (whose name carries the pid). Empty means paging is off.
    #[cfg(not(target_arch = "wasm32"))]
    fn spill_candidates(dir: Option<&Path>, fallback: &Path) -> Vec<(PathBuf, bool)> {
        spill_candidates_for(spill_policy(), dir, fallback)
    }

    /// [`spill_candidates`] for a policy that is spelled out instead of read from the environment.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn spill_candidates_for(policy: SpillPolicy, dir: Option<&Path>, fallback: &Path) -> Vec<(PathBuf, bool)> {
        match policy {
            SpillPolicy::Off => Vec::new(),
            SpillPolicy::Temp => vec![(fallback.to_path_buf(), true)],
            SpillPolicy::Beside => {
                let mut candidates: Vec<(PathBuf, bool)> = Vec::new();
                // A UNC folder is skipped: the page file is written and read sequentially, hundreds
                // of megabytes per session, and over SMB that is worse than a local temp directory.
                if let Some(dir) = dir
                    && !is_network_dir(dir)
                {
                    candidates.push((dir.to_path_buf(), false));
                }
                if !candidates.iter().any(|(candidate, _)| candidate == fallback) {
                    candidates.push((fallback.to_path_buf(), true));
                }
                candidates
            }
        }
    }

    /// wasm has no file system to spill to: a request to page leaves the store live.
    #[cfg(target_arch = "wasm32")]
    fn spill_candidates(_dir: Option<&Path>, _fallback: &Path) -> Vec<(PathBuf, bool)> {
        Vec::new()
    }

    /// What `CELLA_SPILL` asks for; unset (or anything unrecognized) means beside the document.
    #[cfg(not(target_arch = "wasm32"))]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum SpillPolicy {
        /// Try the document's folder, then the temp directory.
        Beside,
        /// Never try the document's folder (a sync-hostile folder, a network share).
        Temp,
        /// Never page at all.
        Off,
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn spill_policy() -> SpillPolicy {
        parse_spill_policy(std::env::var("CELLA_SPILL").ok().as_deref())
    }

    /// [`spill_policy`]'s rule, without the environment: tests pin the policy this way because the
    /// environment is process-global and the model tests run in parallel.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn parse_spill_policy(value: Option<&str>) -> SpillPolicy {
        match value.map(str::trim) {
            Some(value) if value.eq_ignore_ascii_case("temp") => SpillPolicy::Temp,
            Some(value) if value.eq_ignore_ascii_case("off") => SpillPolicy::Off,
            _ => SpillPolicy::Beside,
        }
    }

    /// A Windows UNC folder (`\\server\share\…`), whose spill would be network I/O.
    #[cfg(all(windows, not(target_arch = "wasm32")))]
    fn is_network_dir(dir: &Path) -> bool {
        use std::path::{Component, Prefix};
        matches!(
            dir.components().next(),
            Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..))
        )
    }

    #[cfg(all(not(windows), not(target_arch = "wasm32")))]
    fn is_network_dir(_dir: &Path) -> bool {
        false
    }

    /// The document stem as the spill file may use it: `[A-Za-z0-9_-]` only, at most
    /// [`SPILL_STEM_MAX`] bytes, `book` when nothing survives. A name, never a full path or a
    /// sheet name.
    pub(super) fn spill_stem(hint: Option<&str>) -> String {
        let mut stem: String =
            hint.unwrap_or_default().chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')).take(SPILL_STEM_MAX).collect();
        if stem.is_empty() {
            stem.push_str("book");
        }
        stem
    }

    /// `.cella-spill-{stem}-{nonce}.bin`, with the pid before the nonce in the temp fallback (a
    /// stale file there is conceivable, and it must not collide with another process). The leading
    /// dot hides it on Unix and `.bin` stops a shell or an Office handler from treating it as a
    /// document.
    pub(super) fn spill_file_name(stem: &str, pid: Option<u32>, nonce: u64) -> String {
        match pid {
            Some(pid) => format!(".cella-spill-{stem}-{pid}-{nonce}.bin"),
            None => format!(".cella-spill-{stem}-{nonce}.bin"),
        }
    }

    /// Opens a new spill file, private and with no life of its own: on Unix the name is removed as
    /// soon as the handle exists (the open file survives, so a crash leaves nothing behind), and on
    /// Windows the file is created delete-on-close. A platform with neither keeps the name and
    /// relies on [`PageFile`]'s `Drop`.
    #[cfg(unix)]
    fn create_spill_file(path: &Path) -> Option<File> {
        use std::os::unix::fs::OpenOptionsExt;
        // 0600: the pages are a decodable copy of the document, so nobody else may read them. The
        // process umask the plain `create` used could leave them at 0644 in a shared /tmp.
        let file = OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(path).ok()?;
        let _ = std::fs::remove_file(path);
        Some(file)
    }

    #[cfg(windows)]
    fn create_spill_file(path: &Path) -> Option<File> {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_DELETE_ON_CLOSE: the last handle closes the file away, including after an
        // abnormal end. Some filter drivers refuse the flag, so a refused create is retried
        // without it and `Drop` removes the name.
        const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x0400_0000;
        if let Ok(file) = OpenOptions::new().read(true).write(true).create_new(true).custom_flags(FILE_FLAG_DELETE_ON_CLOSE).open(path) {
            return Some(file);
        }
        OpenOptions::new().read(true).write(true).create_new(true).open(path).ok()
    }

    #[cfg(not(any(unix, windows)))]
    fn create_spill_file(path: &Path) -> Option<File> {
        OpenOptions::new().read(true).write(true).create_new(true).open(path).ok()
    }

    /// The cache budget [`enable_paging`](super::CellStore::enable_paging) creates a page file
    /// with. Read once: `CELLA_PAGE_CACHE_MB`, clamped to 8..=512 MiB, default 64 MiB. Tests
    /// pin a budget through [`enable_paging_with_cache`](super::CellStore::enable_paging_with_cache)
    /// instead, because the environment is process-global and the model tests run in parallel.
    pub(super) fn default_cache_bytes() -> usize {
        static BUDGET: OnceLock<usize> = OnceLock::new();
        *BUDGET.get_or_init(|| requested_cache_bytes().unwrap_or(super::PAGE_CACHE_BYTES))
    }

    /// `CELLA_PAGE_CACHE_MB` in bytes, or `None` when it is unset or not a number.
    #[cfg(not(target_arch = "wasm32"))]
    fn requested_cache_bytes() -> Option<usize> {
        let mb = std::env::var("CELLA_PAGE_CACHE_MB").ok()?.trim().parse::<usize>().ok()?;
        Some(cache_bytes_for_mb(mb))
    }

    /// A `CELLA_PAGE_CACHE_MB` value in bytes, clamped so a typo cannot ask for a gigabyte or
    /// for a cache too small to hold one band. The clamp happens before the multiply, so an absurd
    /// value cannot overflow the budget.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn cache_bytes_for_mb(mb: usize) -> usize {
        mb.clamp(super::PAGE_CACHE_MIN_MB, super::PAGE_CACHE_MAX_MB).saturating_mul(1024 * 1024)
    }

    /// wasm has no environment to read; the default stands.
    #[cfg(target_arch = "wasm32")]
    fn requested_cache_bytes() -> Option<usize> {
        None
    }

    fn put_u8(out: &mut Vec<u8>, v: u8) {
        out.push(v);
    }

    fn put_u16(out: &mut Vec<u8>, v: u16) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    fn put_u32(out: &mut Vec<u8>, v: u32) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    fn put_u64(out: &mut Vec<u8>, v: u64) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    fn put_len(out: &mut Vec<u8>, len: usize) {
        // Every caller has checked the length against a page cap; a truncated length would make
        // the decoder reject the page anyway, so saturate rather than truncate silently.
        put_u32(out, u32::try_from(len).unwrap_or(u32::MAX));
    }

    fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
        put_len(out, bytes.len());
        out.extend_from_slice(bytes);
    }

    fn put_runs(out: &mut Vec<u8>, runs: &[(u16, u16)]) {
        put_len(out, runs.len());
        for (a, b) in runs {
            put_u16(out, *a);
            put_u16(out, *b);
        }
    }

    fn take_u8(input: &mut &[u8]) -> Option<u8> {
        let (first, rest) = input.split_first()?;
        *input = rest;
        Some(*first)
    }

    fn take_u16(input: &mut &[u8]) -> Option<u16> {
        let bytes: [u8; 2] = input.get(..2)?.try_into().ok()?;
        *input = input.get(2..)?;
        Some(u16::from_le_bytes(bytes))
    }

    fn take_u32(input: &mut &[u8]) -> Option<u32> {
        let bytes: [u8; 4] = input.get(..4)?.try_into().ok()?;
        *input = input.get(4..)?;
        Some(u32::from_le_bytes(bytes))
    }

    fn take_u64(input: &mut &[u8]) -> Option<u64> {
        let bytes: [u8; 8] = input.get(..8)?.try_into().ok()?;
        *input = input.get(8..)?;
        Some(u64::from_le_bytes(bytes))
    }

    fn take_len(input: &mut &[u8], cap: usize) -> Option<usize> {
        let len = usize::try_from(take_u32(input)?).ok()?;
        (len <= cap).then_some(len)
    }

    fn take_bytes(input: &mut &[u8], cap: usize) -> Option<Vec<u8>> {
        let len = take_len(input, cap)?;
        let bytes = input.get(..len)?.to_vec();
        *input = input.get(len..)?;
        Some(bytes)
    }

    fn encode_value(out: &mut Vec<u8>, value: &Value, depth: u32) -> Option<()> {
        if depth > MAX_DEPTH {
            return None;
        }
        match value {
            Value::Empty => put_u8(out, 0),
            Value::Number(n) => {
                put_u8(out, 1);
                put_u64(out, if *n == 0.0 { 0.0f64.to_bits() } else { n.to_bits() });
            }
            Value::Text(t) => {
                put_u8(out, 2);
                if t.len() > MAX_TEXT {
                    return None;
                }
                put_bytes(out, t.as_bytes());
            }
            Value::Bool(b) => {
                put_u8(out, 3);
                put_u8(out, u8::from(*b));
            }
            Value::Error(e) => {
                put_u8(out, 4);
                put_u8(out, error_code(*e) as u8);
            }
            Value::Array(a) => {
                put_u8(out, 5);
                put_u32(out, u32::try_from(a.rows).ok()?);
                put_u32(out, u32::try_from(a.cols).ok()?);
                put_len(out, a.data.len());
                for v in &a.data {
                    encode_value(out, v, depth.saturating_add(1))?;
                }
            }
        }
        Some(())
    }

    fn decode_value(input: &mut &[u8], depth: u32) -> Option<Value> {
        if depth > MAX_DEPTH {
            return None;
        }
        match take_u8(input)? {
            0 => Some(Value::Empty),
            1 => Some(Value::Number(f64::from_bits(take_u64(input)?))),
            2 => {
                let bytes = take_bytes(input, MAX_TEXT)?;
                let text = std::str::from_utf8(&bytes).ok()?;
                Some(Value::Text(Arc::from(text)))
            }
            3 => Some(Value::Bool(take_u8(input)? != 0)),
            4 => {
                let code = take_u8(input)?;
                Some(Value::Error(*ERRORS.get(usize::from(code)).unwrap_or(&CellError::Value)))
            }
            5 => {
                let rows = usize::try_from(take_u32(input)?).ok()?;
                let cols = usize::try_from(take_u32(input)?).ok()?;
                let len = take_len(input, MAX_CELLS)?;
                if rows == 0 || cols == 0 || rows.checked_mul(cols)? != len {
                    return None;
                }
                let mut data = Vec::with_capacity(len);
                for _ in 0..len {
                    data.push(decode_value(input, depth.saturating_add(1))?);
                }
                Some(Value::Array(Arc::new(cella_core::Array { rows, cols, data })))
            }
            _ => None,
        }
    }

    fn encode_formula(out: &mut Vec<u8>, f: &Formula, at: CellRef) -> Option<()> {
        // The page file carries text, so a formula that prints itself is materialised here: a
        // decoded band holds text-only formulas and re-parses what it needs.
        let text = f.text_at(at)?;
        if text.len() > MAX_TEXT {
            return None;
        }
        put_bytes(out, text.as_bytes());
        match f.array.as_deref() {
            Some(a) => {
                put_u8(out, 1);
                put_u32(out, a.start.row);
                put_u32(out, a.start.col);
                put_u32(out, a.end.row);
                put_u32(out, a.end.col);
            }
            None => put_u8(out, 0),
        }
        put_u8(out, u8::from(f.dynamic));
        Some(())
    }

    fn decode_formula(input: &mut &[u8]) -> Option<Arc<Formula>> {
        let bytes = take_bytes(input, MAX_TEXT)?;
        let text = String::from_utf8(bytes).ok()?;
        let array = match take_u8(input)? {
            0 => None,
            1 => Some(Box::new(RangeRef::new(CellRef::new(take_u32(input)?, take_u32(input)?), CellRef::new(take_u32(input)?, take_u32(input)?)))),
            _ => return None,
        };
        let dynamic = take_u8(input)? != 0;
        // Reparse so a loaded formula behaves exactly like the one that was spilled (including the
        // parsed tree and the undo byte count). Most pages hold no formulas, so the cost is rare.
        let mut formula = Formula::new(&text);
        formula.array = array;
        formula.dynamic = dynamic;
        Some(Arc::new(formula))
    }

    fn encode_row(out: &mut Vec<u8>, row: &BandRow) -> Option<()> {
        put_u32(out, row.key);
        put_u32(out, row.first);
        put_u32(out, row.bits_at);
        put_u32(out, row.words);
        put_u32(out, row.cells_at);
        put_u32(out, row.kinds_at);
        put_u32(out, row.styles_at);
        put_u32(out, row.cells);
        put_u32(out, row.kind_runs);
        put_u32(out, row.style_runs);
        match &row.extras {
            None => put_u8(out, 0),
            Some(e) => {
                put_u8(out, 1);
                put_len(out, e.formulas.len());
                for (col, f) in &e.formulas {
                    put_u32(out, *col);
                    encode_formula(out, f, CellRef::new(row.key, *col))?;
                }
                put_len(out, e.arrays().len());
                for (col, a) in e.arrays() {
                    put_u32(out, *col);
                    put_u32(out, u32::try_from(a.rows).ok()?);
                    put_u32(out, u32::try_from(a.cols).ok()?);
                    put_len(out, a.data.len());
                    for v in &a.data {
                        encode_value(out, v, 0)?;
                    }
                }
                put_len(out, e.wide().len());
                for v in e.wide() {
                    put_u64(out, *v);
                }
            }
        }
        Some(())
    }

    fn decode_row(input: &mut &[u8]) -> Option<BandRow> {
        let key = take_u32(input)?;
        if key >= MAX_ROWS {
            return None;
        }
        let first = take_u32(input)?;
        if first > MAX_COLS {
            return None;
        }
        let bits_at = take_u32(input)?;
        let words = take_u32(input)?;
        if words > MAX_COLS / 64 {
            return None;
        }
        let cells_at = take_u32(input)?;
        let kinds_at = take_u32(input)?;
        let styles_at = take_u32(input)?;
        let cells = take_u32(input)?;
        if cells > MAX_COLS {
            return None;
        }
        let kind_runs = take_u32(input)?;
        let style_runs = take_u32(input)?;
        if kind_runs > cells || style_runs > cells {
            return None;
        }
        let extras = match take_u8(input)? {
            0 => None,
            1 => {
                let formula_len = take_len(input, cells as usize)?;
                let mut formulas = Vec::with_capacity(formula_len);
                for _ in 0..formula_len {
                    let col = take_u32(input)?;
                    formulas.push((col, decode_formula(input)?));
                }
                formulas.sort_unstable_by_key(|(c, _)| *c);
                formulas.dedup_by_key(|(c, _)| *c);
                let array_len = take_len(input, cells as usize)?;
                let mut arrays = Vec::with_capacity(array_len);
                for _ in 0..array_len {
                    let col = take_u32(input)?;
                    let rows = usize::try_from(take_u32(input)?).ok()?;
                    let cols = usize::try_from(take_u32(input)?).ok()?;
                    let len = take_len(input, MAX_CELLS)?;
                    if rows == 0 || cols == 0 || rows.checked_mul(cols)? != len {
                        return None;
                    }
                    let mut data = Vec::with_capacity(len);
                    for _ in 0..len {
                        data.push(decode_value(input, 0)?);
                    }
                    arrays.push((col, Arc::new(cella_core::Array { rows, cols, data })));
                }
                arrays.sort_unstable_by_key(|(c, _)| *c);
                arrays.dedup_by_key(|(c, _)| *c);
                let wide_len = take_len(input, MAX_CELLS)?;
                let mut wide = Vec::with_capacity(wide_len);
                for _ in 0..wide_len {
                    wide.push(take_u64(input)?);
                }
                Some(Arc::new(RowExtras { formulas, rare: (!arrays.is_empty() || !wide.is_empty()).then(|| Box::new(RareRow { arrays, wide })) }))
            }
            _ => return None,
        };
        Some(BandRow { key, first, bits_at, words, cells_at, kinds_at, styles_at, cells, kind_runs, style_runs, extras })
    }

    /// Encodes one finished band. The callers spill only bands with their indexes dropped, so the
    /// hash/sorted indexes are not part of the page; the decoder leaves them empty and an edit
    /// rebuilds them.
    pub(super) fn encode_band(band: &Band) -> Option<Vec<u8>> {
        if band.rows.len() > MAX_ROWS as usize {
            return None;
        }
        let mut out = Vec::with_capacity(16 * 1024);
        put_u32(&mut out, MAGIC);
        put_u8(&mut out, VERSION);
        put_len(&mut out, band.rows.len());
        for row in &band.rows {
            encode_row(&mut out, row)?;
        }
        put_len(&mut out, band.bits.len());
        for v in &band.bits {
            put_u64(&mut out, *v);
        }
        put_runs(&mut out, &band.kinds);
        put_runs(&mut out, &band.styles);
        put_len(&mut out, band.payloads.len());
        for v in &band.payloads {
            put_u16(&mut out, *v);
        }
        put_len(&mut out, band.numbers.len());
        for v in band.numbers.iter() {
            put_u64(&mut out, if *v == 0.0 { 0.0f64.to_bits() } else { v.to_bits() });
        }
        put_len(&mut out, band.texts.len());
        for t in &band.texts {
            if t.len() > MAX_TEXT {
                return None;
            }
            put_bytes(&mut out, t.as_bytes());
        }
        put_len(&mut out, band.style_ids.len());
        for id in &band.style_ids {
            put_u32(&mut out, id.0);
        }
        if out.len() > MAX_PAGE {
            return None;
        }
        Some(out)
    }

    pub(super) fn decode_band(bytes: &[u8]) -> Option<Band> {
        let mut input = bytes;
        if take_u32(&mut input)? != MAGIC || take_u8(&mut input)? != VERSION {
            return None;
        }
        let rows_len = take_len(&mut input, MAX_ROWS as usize)?;
        let mut rows = Vec::with_capacity(rows_len);
        for _ in 0..rows_len {
            rows.push(decode_row(&mut input)?);
        }
        rows.sort_unstable_by_key(|r| r.key);
        if rows.windows(2).any(|w| w[0].key == w[1].key) {
            return None;
        }
        let bits_len = take_len(&mut input, MAX_ROWS as usize * (MAX_COLS as usize / 64))?;
        let mut bits = Vec::with_capacity(bits_len);
        for _ in 0..bits_len {
            bits.push(take_u64(&mut input)?);
        }
        let kinds = decode_runs(&mut input, MAX_CELLS)?;
        let styles = decode_runs(&mut input, MAX_CELLS)?;
        let payload_len = take_len(&mut input, MAX_CELLS)?;
        let mut payloads = Vec::with_capacity(payload_len);
        for _ in 0..payload_len {
            payloads.push(take_u16(&mut input)?);
        }
        let numbers_len = take_len(&mut input, MAX_CELLS)?;
        let mut numbers = Vec::with_capacity(numbers_len);
        for _ in 0..numbers_len {
            numbers.push(f64::from_bits(take_u64(&mut input)?));
        }
        let texts_len = take_len(&mut input, MAX_CELLS)?;
        let mut texts = Vec::with_capacity(texts_len);
        for _ in 0..texts_len {
            let raw = take_bytes(&mut input, MAX_TEXT)?;
            texts.push(Arc::<str>::from(std::str::from_utf8(&raw).ok()?));
        }
        let styles_len = take_len(&mut input, MAX_CELLS)?;
        let mut style_ids = Vec::with_capacity(styles_len);
        for _ in 0..styles_len {
            style_ids.push(StyleId(take_u32(&mut input)?));
        }
        if !input.is_empty() {
            return None;
        }
        let row_bits = Band::rows_bits(&rows);
        let band = Band {
            rows,
            row_bits,
            bits,
            kinds,
            styles,
            payloads,
            numbers,
            number_slots: Vec::new(),
            texts,
            text_index: Vec::new(),
            style_ids,
            tag: BandTag::default(),
        };
        let valid = band.rows.iter().all(|r| {
            let bits_end = (r.bits_at as usize).checked_add(r.words as usize).is_some_and(|end| end <= band.bits.len());
            let cells_end = (r.cells_at as usize).checked_add(r.cells as usize).is_some_and(|end| end <= band.payloads.len());
            let kinds_end = (r.kinds_at as usize).checked_add(r.kind_runs as usize).is_some_and(|end| end <= band.kinds.len());
            let styles_end = (r.styles_at as usize).checked_add(r.style_runs as usize).is_some_and(|end| end <= band.styles.len());
            let bits = r.bits(&band).iter().map(|w| w.count_ones()).sum::<u32>();
            let kinds = r.kind_runs(&band).iter().map(|(_, n)| u32::from(*n)).sum::<u32>();
            let styles = r.style_runs(&band).iter().map(|(_, n)| u32::from(*n)).sum::<u32>();
            bits_end && cells_end && kinds_end && styles_end && bits == r.cells && kinds == r.cells && styles == r.cells
        });
        if !valid {
            return None;
        }
        Some(band)
    }

    fn decode_runs(input: &mut &[u8], cap: usize) -> Option<Vec<(u16, u16)>> {
        let len = take_len(input, cap)?;
        let mut runs = Vec::with_capacity(len);
        for _ in 0..len {
            runs.push((take_u16(input)?, take_u16(input)?));
        }
        Some(runs)
    }

    /// One band encoded for its page, with everything a [`Page`] carries: the bytes, the range its
    /// rows cover (for `used_range` without a load) and the two flags a formula comparison and a
    /// compaction read. Computed on the worker that encodes it, so the band can be dropped as soon
    /// as its page is written.
    pub(super) type Spilled = (u32, Vec<u8>, Option<(u32, u32, u32, u32)>, bool, bool);

    /// Encodes and deflates the bands of `keys` this store holds, in key order: everything a page
    /// needs, done where it can be done on a worker thread. A band that cannot be encoded is left
    /// out: its page is never written and the band stays live.
    pub(super) fn encode_spilled(bands: &std::collections::BTreeMap<u32, Arc<Band>>, keys: &[u32]) -> Vec<Spilled> {
        let mut out = Vec::with_capacity(keys.len());
        for key in keys {
            let Some(band) = bands.get(key) else { continue };
            let Some(bytes) = encode_band(band).and_then(|raw| pack(&raw)) else { continue };
            let formulas = band.rows.iter().any(|row| !row.formulas().is_empty());
            let owned = band.rows.iter().any(|row| !row.formulas().is_empty() || !row.arrays().is_empty());
            out.push((*key, bytes, band_bounds(band), formulas, owned));
        }
        out
    }

    /// The range a band's rows cover, for `used_range` without a load.
    pub(super) fn band_bounds(band: &Band) -> Option<(u32, u32, u32, u32)> {
        let (mut r0, mut r1, mut c0, mut c1) = (u32::MAX, 0u32, u32::MAX, 0u32);
        for row in &band.rows {
            let Some((first, last)) = row.column_span(band) else { continue };
            r0 = r0.min(row.key);
            r1 = r1.max(row.key);
            c0 = c0.min(first);
            c1 = c1.max(last);
        }
        (r0 != u32::MAX).then_some((r0, r1, c0, c1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cella_core::Value;
    use cella_formula::{Shape, ShapeTable};

    fn c(r: u32, col: u32) -> CellRef {
        CellRef::new(r, col)
    }

    fn plain(n: u32) -> Cell {
        Cell::value(Value::Number(f64::from(n)))
    }

    /// A formula of `text` entered at `at`, sharing `shapes` with every cell of the same fill.
    fn shaped(shapes: &mut ShapeTable, text: &str, at: CellRef) -> Formula {
        let tree = cella_formula::parse(text).expect("the test formula parses");
        Formula::from_expr_at(tree, at, shapes, false)
    }

    #[test]
    fn formula_changes_against_reports_the_edited_formulas() {
        let mut older = CellStore::new();
        for row in 0..1000u32 {
            let mut cell = plain(row);
            cell.formula = Some(Arc::new(Formula::new(format!("=A{}+1", row + 1).as_str())));
            older.set(c(row, 1), cell);
        }
        // A value edit elsewhere in the sheet: nothing a graph would have to update.
        let mut newer = older.clone();
        newer.set(c(7, 3), plain(7));
        assert_eq!(newer.formula_changes_against(&older).map(|v| v.len()), Some(0));

        // One formula rewritten, one cleared, one new — and a copy of a formula that was
        // re-parsed (same text) is not a change.
        newer.set(c(10, 1), {
            let mut cell = plain(10);
            cell.formula = Some(Arc::new(Formula::new("=A11*2")));
            cell
        });
        newer.set(c(11, 1), plain(11));
        newer.set(c(12, 2), {
            let mut cell = plain(12);
            cell.formula = Some(Arc::new(Formula::new("=B13+1")));
            cell
        });
        let mut reparsed = newer.get(c(13, 1)).unwrap_or_default();
        reparsed.formula = Some(Arc::new(Formula::new("=A14+1")));
        newer.set(c(13, 1), reparsed);
        let changes = newer.formula_changes_against(&older).expect("comparable");
        assert_eq!(changes.len(), 3, "{changes:?}");
        assert_eq!(changes.iter().map(|(at, _)| *at).collect::<Vec<_>>(), vec![c(10, 1), c(11, 1), c(12, 2)]);
        assert_eq!(changes.first().and_then(|(at, f)| f.as_ref().and_then(|f| f.text_at(*at))), Some("A11*2".to_string()));
        assert!(changes.get(1).is_some_and(|(_, f)| f.is_none()));
        assert_eq!(changes.get(2).and_then(|(at, f)| f.as_ref().and_then(|f| f.text_at(*at))), Some("B13+1".to_string()));

        // A row insert moved the cells to other logical positions: not comparable band by band.
        let mut moved = older.clone();
        moved.shift_rows(4, 1);
        assert!(moved.formula_changes_against(&older).is_none());
    }

    /// `value` and `value_ref` read exactly what `get` rebuilds — for a number, a text, a
    /// boolean, an error, a style-only cell, a formula whose value is empty, an array, and a cell
    /// nothing is stored at — and they read it without rebuilding the `Cell`.
    #[test]
    fn value_reads_what_get_rebuilds() {
        let mut s = CellStore::new();
        let array = Arc::new(cella_core::Array::row(vec![Value::Number(2.0), Value::Text(Arc::from("x"))]));
        let cells = [
            (c(0, 0), Cell::value(Value::Number(1.25))),
            (c(1, 0), Cell::value(Value::Text(Arc::from("hello")))),
            (c(2, 0), Cell::value(Value::Bool(true))),
            (c(3, 0), Cell::value(Value::Error(CellError::Div0))),
            (c(4, 0), Cell { style: StyleId(3), ..Default::default() }),
            (c(5, 0), Cell { formula: Some(Arc::new(Formula::new("A1"))), ..Default::default() }),
            (c(6, 0), Cell::value(Value::Array(array))),
            (c(65, 3), Cell::value(Value::Text(Arc::from("past the first band")))),
        ];
        for (at, cell) in cells {
            s.set(at, cell);
        }
        // Every position of two bands, so the bands' ends, the gaps and the empty rows are read too.
        for row in 0..BAND * 2 {
            for col in 0..8u32 {
                let at = c(row, col);
                let stored = s.get(at);
                assert_eq!(s.value(at), stored.clone().map(|cell| cell.value).unwrap_or(Value::Empty), "{at:?}");
                assert_eq!(s.value_ref(at), stored.map(|cell| cell.value), "{at:?}");
            }
        }
        // The formula travels alongside the value for a display read, without being cloned.
        assert_eq!(s.value_with_formula(c(5, 0)), Some((Value::Empty, true)));
        assert_eq!(s.value_with_formula(c(4, 0)), Some((Value::Empty, false)));
        assert_eq!(s.value_with_formula(c(7, 7)), None);
    }

    #[test]
    fn row_reader_matches_cell_reads_for_live_and_paged_sparse_mapped_rows() {
        let mut store = CellStore::new();
        store.set(c(0, 2), Cell { style: StyleId(3), ..Default::default() });
        store.set(c(0, 4), Cell { formula: Some(Arc::new(Formula::new("A1"))), ..Default::default() });
        store.set(c(0, 70), Cell::value(Value::Number(18.0)));
        store.set(c(0, 71), Cell { formula: Some(Arc::new(Formula::new("2+3"))), value: Value::Number(5.0), style: StyleId(4) });
        store.set(c(BAND, 2), Cell::value(Value::Text(Arc::from("later"))));
        store.set(c(BAND, 3), Cell::value(Value::Array(Arc::new(cella_core::Array::row(vec![Value::Number(4.0)])))));
        for mapped in [false, true] {
            let mut live = store.clone();
            if mapped {
                live.shift_cols(1, 2);
                // The new logical column maps above the old physical columns.
                live.set(c(0, 1), Cell::value(Value::Bool(true)));
                live.shift_cols(5, -1);
                live.shift_rows(0, 1);
            }
            let mut paged = live.clone();
            assert!(paged.enable_paging_with_cache(64 << 20));
            let pages = paged.paged_band_count();
            assert!(pages > 0);
            for source in [&live, &paged] {
                for (c0, c1) in [(0, 75), (3, 7), (4, 4), (70, 73), (90, 92)] {
                    let read = source.row_reader(c0, c1);
                    for row in [0, 1, 3, BAND, BAND + 1] {
                        let mut got = Vec::new();
                        read(row, &mut got);
                        let expected: Vec<_> = (c0..=c1).map(|col| live.get(c(row, col))).collect();
                        assert_eq!(got, expected, "row={row}, columns={c0}..={c1}, mapped={mapped}");
                        for col in c0..=c1 {
                            let at = c(row, col);
                            assert_eq!(source.formula(at), source.get(at).and_then(|cell| cell.formula), "{at:?}, mapped={mapped}");
                        }
                    }
                }
            }
            assert_eq!(paged.paged_band_count(), pages);
            assert!(paged.bands.is_empty(), "the reader must not materialize pages");
        }
    }

    /// A paged band is decoded for a value read: the value comes back as the live band gave it,
    /// and the read leaves the band on its page (nothing is installed in the live map).
    #[test]
    fn value_reads_a_paged_band() {
        let mut s = CellStore::new();
        let rows = BAND * 3;
        for row in 0..rows {
            s.set(c(row, 0), Cell::value(Value::Number(f64::from(row))));
            s.set(c(row, 1), Cell::value(Value::Text(Arc::from(format!("r{row}").as_str()))));
        }
        assert!(s.enable_paging_with_cache(64 << 20), "the store pages");
        let paged = s.paged_band_count();
        assert!(paged > 0, "the bands went to pages");
        assert!(s.bands.is_empty(), "every band is a page");
        for _ in 0..2 {
            for row in 0..rows {
                let (number, text) = (c(row, 0), c(row, 1));
                assert_eq!(s.value(number), Value::Number(f64::from(row)), "{number:?}");
                assert_eq!(s.value(text), Value::Text(Arc::from(format!("r{row}").as_str())), "{text:?}");
                assert_eq!(s.value(c(row, 2)), Value::Empty);
            }
        }
        assert!(s.bands.is_empty(), "a value read does not materialize the page");
        assert_eq!(s.paged_band_count(), paged);
    }

    #[test]
    fn has_formulas_counts_uncalculated_and_cached_formulas() {
        let mut s = CellStore::new();
        assert!(!s.has_formulas());
        s.set(c(0, 0), Cell::value(Value::Number(4.0)));
        assert!(!s.has_formulas());
        let at = c(2, 1);
        let formula = Arc::new(Formula::new("=A1+1"));
        s.set(at, Cell { formula: Some(formula.clone()), ..Default::default() });
        assert!(s.has_formulas(), "the formula need not have a cached value");
        s.set(at, Cell { value: Value::Number(5.0), formula: Some(formula), ..Default::default() });
        assert!(s.has_formulas());
        s.remove(at);
        assert!(!s.has_formulas());
    }

    /// The cache behind `has_formulas` is a one-sided "a formula was written here": every write
    /// path that can store one raises it, and the exactness the callers rely on (a deleted formula
    /// must read false) still comes from the walk.
    #[test]
    fn has_formulas_sees_every_write_path_and_stays_exact() {
        let formula = || Arc::new(Formula::new("=A1+1"));
        // The row path a reader and a sort write through.
        let mut by_row = CellStore::new();
        let mut cells = vec![(0u32, Cell::value(Value::Number(1.0)))];
        by_row.set_row(0, &mut cells);
        assert!(!by_row.has_formulas(), "values only");
        let mut cells = vec![(0u32, Cell { formula: Some(formula()), ..Default::default() })];
        by_row.set_row(BAND, &mut cells);
        assert!(by_row.has_formulas(), "a formula written by the row path is seen");
        // The batch path the file reader decodes through.
        let mut batch = BandBatch::new();
        let mut cells = vec![(0u32, Cell { formula: Some(formula()), ..Default::default() })];
        batch.set_row(0, &mut cells);
        let mut by_batch = CellStore::new();
        by_batch.insert_batch(batch);
        assert!(by_batch.has_formulas(), "a formula written by a reader batch is seen");
        // A clone keeps the answer without walking, and a deserialized store recomputes it.
        assert!(by_batch.clone().has_formulas());
        let json = serde_json::to_string(&by_batch).expect("a store serializes");
        let back: CellStore = serde_json::from_str(&json).expect("and deserializes");
        assert!(back.has_formulas());
        // A values-only store of a few bands answers false.
        let mut plain = CellStore::new();
        for row in 0..BAND * 2 {
            let mut cells = vec![(0u32, Cell::value(Value::Number(f64::from(row))))];
            plain.set_row(row, &mut cells);
        }
        assert!(!plain.has_formulas());
    }

    #[test]
    fn has_formulas_skips_value_pages_and_excludes_deleted_rows_and_columns() {
        let mut s = CellStore::new();
        s.set(c(0, 0), Cell::value(Value::Number(4.0)));
        assert!(s.enable_paging());
        assert!(!s.has_formulas());
        let cache = s.page_cache_stats().expect("paging is enabled");
        assert_eq!((cache.hits, cache.misses), (0, 0), "value-only page needs no read");

        for delete_row in [true, false] {
            let mut s = CellStore::new();
            s.set(c(0, 0), Cell::value(Value::Number(4.0)));
            s.set(c(BAND, 1), Cell { formula: Some(Arc::new(Formula::new("=A1+1"))), ..Default::default() });
            let mut paged = s.clone();
            assert!(paged.enable_paging());
            let pages = paged.paged_band_count();
            assert_eq!(pages, 2);
            assert!(paged.has_formulas());
            let cache = paged.page_cache_stats().expect("paging is enabled");
            assert_eq!(cache.misses, 1, "only the positive formula page was decoded");
            assert_eq!(paged.paged_band_count(), pages, "the query does not materialize the page");

            for store in [&mut s, &mut paged] {
                if delete_row {
                    store.shift_rows(BAND + 1, -1);
                } else {
                    store.shift_cols(2, -1);
                }
                assert!(!store.has_formulas(), "the only formula was deleted");
                assert!(store.formulas().next().is_none());
            }
            assert!(paged.pages.get(&1).is_some_and(|page| page.has_formulas()), "the deleted formula remains physically on its page");
        }
    }

    #[test]
    fn runs_match_a_plain_vector() {
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut next = |n: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % n.max(1)) as usize
        };
        let mut runs: Vec<(u16, u16)> = Vec::new();
        let mut plain: Vec<u16> = Vec::new();
        for step in 0..4_000 {
            let op = next(4);
            let value = next(3) as u16;
            // One row, so the slice is the whole stripe.
            let slice = runs.len();
            let mut did = String::new();
            match op {
                0 => {
                    run_push(&mut runs, 0, value);
                    plain.push(value);
                }
                1 => {
                    let i = next(plain.len() as u64 + 1);
                    let new = runs_insert(&mut runs, 0, slice, i, value);
                    assert_eq!(new, runs.len());
                    plain.insert(i, value);
                    did = format!("insert({i}, {value})");
                }
                2 if !plain.is_empty() => {
                    let i = next(plain.len() as u64);
                    let new = runs_remove(&mut runs, 0, slice, i);
                    assert_eq!(new, runs.len());
                    plain.remove(i);
                    did = format!("remove({i})");
                }
                3 if !plain.is_empty() => {
                    let i = next(plain.len() as u64);
                    let new = runs_set(&mut runs, 0, slice, i, value);
                    assert_eq!(new, runs.len());
                    plain[i] = value;
                    did = format!("set({i}, {value})");
                }
                _ => {}
            }
            assert_eq!(runs_len(&runs), plain.len(), "step {step}: length");
            if step % 25 == 0 {
                for (i, v) in plain.iter().enumerate() {
                    assert_eq!(runs_at(&runs, i), *v, "step {step}: at {i} of {plain:?}");
                }
            }
            assert!(runs.windows(2).all(|w| w[0].0 != w[1].0), "step {step} {did}: adjacent runs hold the same value: {runs:?} plain {plain:?}");
        }
    }

    /// The runs of two rows share one stripe: a row must never merge into its neighbour's run, and
    /// editing one row must leave the other's slice alone.
    #[test]
    fn runs_stay_inside_their_slice() {
        let mut runs: Vec<(u16, u16)> = Vec::new();
        run_push(&mut runs, 0, 7);
        run_push(&mut runs, 0, 7);
        let second = runs.len();
        run_push(&mut runs, second, 7);
        assert_eq!(runs, [(7, 2), (7, 1)], "a new row starts its own run");
        // Two rows share one stripe: the first holds [3, 3, 4], the second [3, 3, 3].
        let mut stripe: Vec<(u16, u16)> = vec![(3, 2), (4, 1), (3, 3)];
        let first = runs_set(&mut stripe, 0, 2, 1, 4);
        assert_eq!(first, 2);
        assert_eq!(stripe, [(3, 1), (4, 2), (3, 3)], "the second row's slice is untouched");
        assert_eq!(runs_len(&stripe[..2]), 3);
        assert_eq!(runs_len(&stripe[2..]), 3);
        // The second row now starts where the first one ends, and its runs merge with each other
        // but never with the run the first row left beside them.
        let second = runs_set(&mut stripe, 2, 1, 1, 4);
        assert_eq!(second, 3);
        assert_eq!(stripe, [(3, 1), (4, 2), (3, 1), (4, 1), (3, 1)]);
    }

    #[test]
    fn set_get_remove() {
        let mut s = CellStore::new();
        s.set(c(0, 0), Cell::value(1.0.into()));
        s.set(c(100, 5), Cell::value("x".into()));
        assert_eq!(s.len(), 2);
        assert_eq!(s.get(c(100, 5)).map(|x| x.value.clone()), Some(Value::from("x")));
        assert_eq!(s.used_range().unwrap().a1(), "A1:F101");
        s.set(c(0, 0), Cell::default());
        assert_eq!(s.len(), 1);
        assert!(s.remove(c(100, 5)).is_some());
        assert!(s.is_empty());
        assert!(s.used_range().is_none());
    }

    #[test]
    fn cow_snapshots() {
        let mut s = CellStore::new();
        for r in 0..1000 {
            s.set(c(r, 0), Cell::value((r as f64).into()));
        }
        let snap = s.clone();
        s.set(c(5, 0), Cell::value(99.0.into()));
        assert_eq!(snap.get(c(5, 0)).unwrap().value, Value::Number(5.0));
        assert_eq!(s.get(c(5, 0)).unwrap().value, Value::Number(99.0));
    }

    #[test]
    fn set_value_keeps_what_the_cell_is() {
        let mut s = CellStore::new();
        let styled = Cell { value: Value::Number(1.0), formula: Some(std::sync::Arc::new(Formula::new("=1+1"))), style: StyleId(3) };
        s.set(c(0, 0), styled);
        s.set(c(0, 1), Cell::value(Value::Number(2.0)));
        assert!(s.set_value(c(0, 0), &Value::Text("changed".into())));
        let cell = s.get(c(0, 0)).unwrap();
        assert_eq!(cell.value, Value::Text("changed".into()));
        assert_eq!(cell.formula.as_ref().and_then(|f| f.text_at(c(0, 0))), Some("1+1".to_string()));
        assert_eq!(cell.style, StyleId(3), "a value write keeps the cell's formatting");
        assert_eq!(s.len(), 2);
        // A cell that isn't stored gets no value (and is not created).
        assert!(!s.set_value(c(9, 9), &Value::Number(5.0)));
        assert!(s.get(c(9, 9)).is_none());
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn a_page_file_lives_only_as_long_as_its_store() {
        let (path, spilled) = {
            let mut s = CellStore::new();
            s.set(c(0, 0), Cell::value(Value::Number(1.0)));
            assert!(s.enable_paging());
            let path = s.page_file_path().expect("the store pages").to_path_buf();
            assert_eq!(path.parent(), Some(std::env::temp_dir().as_path()), "no directory given, so the temp directory");
            let name = path.file_name().and_then(|n| n.to_str()).expect("a file name");
            assert!(name.starts_with(".cella-spill-book-") && name.ends_with(".bin"), "{name}");
            // Unix removes the name as soon as the file is open, so a crash leaves nothing; the
            // handle keeps the pages readable. Windows keeps the name, delete-on-close.
            #[cfg(unix)]
            assert!(!path.exists(), "the spill has no name once it is open");
            #[cfg(windows)]
            assert!(path.exists(), "the spill exists while it is open");
            assert!(s.paged_band_count() > 0, "the band went to the file");
            assert_eq!(s.get(c(0, 0)).map(|cell| cell.value), Some(Value::Number(1.0)), "the open file still reads");
            (path, s.paged_band_count())
        };
        assert!(spilled > 0);
        assert!(!path.exists(), "dropping the store removes the spill file");
    }

    /// A spill asked for beside a document goes there, carries the document's stem (sanitized), and
    /// leaves nothing behind when the store goes.
    #[test]
    fn a_spill_beside_the_document_goes_in_its_folder() {
        let dir = std::env::temp_dir().join(format!("cella-model-spill-{}-{}", std::process::id(), page_stem_nonce()));
        std::fs::create_dir(&dir).expect("a temp directory for the test");
        let path = {
            let mut s = CellStore::new();
            for row in 0..200u32 {
                s.set(c(row, 0), Cell::value(Value::Number(f64::from(row))));
            }
            assert!(s.enable_paging_with(SpillOptions { dir: Some(&dir), name: Some("Q3 report.v2"), ..SpillOptions::default() }));
            let path = s.page_file_path().expect("the store pages").to_path_buf();
            assert_eq!(path.parent(), Some(dir.as_path()), "the document's folder was used");
            assert_eq!(path.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with(".cella-spill-Q3reportv2-")), Some(true), "{path:?}");
            assert!(s.paged_band_count() > 0, "the bands were spilled");
            assert_eq!(s.get(c(1, 0)).map(|cell| cell.value), Some(Value::Number(1.0)));
            assert_eq!(s.get(c(199, 0)).map(|cell| cell.value), Some(Value::Number(199.0)), "a paged band still reads");
            path
        };
        assert!(!path.exists(), "the spill is gone with its store");
        assert_eq!(std::fs::read_dir(&dir).expect("the test directory").count(), 0, "the folder is as it was");
        std::fs::remove_dir_all(&dir).expect("clean up the test directory");
    }

    /// A document folder that does not exist, or is a file, is not a reason to open without
    /// paging: the temp directory is tried next, and the store pages.
    #[test]
    fn an_unusable_document_folder_falls_back_to_temp() {
        let base = std::env::temp_dir().join(format!("cella-model-fallback-{}-{}", std::process::id(), page_stem_nonce()));
        std::fs::create_dir(&base).expect("a temp directory for the test");
        let not_there = base.join("missing");
        let a_file = base.join("a-file");
        std::fs::write(&a_file, b"not a directory").expect("a file where a directory would be");
        for dir in [not_there.as_path(), a_file.as_path()] {
            let mut s = CellStore::new();
            s.set(c(0, 0), Cell::value(Value::Number(7.0)));
            assert!(
                s.enable_paging_with(SpillOptions { dir: Some(dir), name: Some("book"), ..SpillOptions::default() }),
                "{dir:?} is unusable, so the temp directory answers"
            );
            let path = s.page_file_path().expect("the store pages");
            assert_eq!(path.parent(), Some(std::env::temp_dir().as_path()), "{dir:?}");
            assert!(s.paged_band_count() > 0);
        }
        std::fs::remove_dir_all(&base).expect("clean up the test directory");
    }

    /// Two unusable directories leave the store live, exactly as `CELLA_SPILL=off` does: no
    /// page file, nothing refused, every cell still there. The option pins both so the test does
    /// not touch the process-global environment.
    #[test]
    fn paging_stays_off_when_no_directory_can_hold_the_spill() {
        let base = std::env::temp_dir().join(format!("cella-model-off-{}-{}", std::process::id(), page_stem_nonce()));
        std::fs::create_dir(&base).expect("a temp directory for the test");
        let missing = base.join("missing");
        let mut s = CellStore::new();
        s.set(c(0, 0), Cell::value(Value::Number(3.0)));
        s.set(c(70, 1), Cell::value(Value::Text(Arc::from("kept"))));
        let options = SpillOptions { dir: Some(&missing), name: Some("book"), cache_bytes: None, temp: Some(&missing) };
        assert!(!s.enable_paging_with(options), "no file could be created, so paging stays off");
        assert_eq!(s.page_file_path(), None);
        assert_eq!(s.paged_band_count(), 0);
        assert_eq!(s.page_cache_stats(), None);
        assert_eq!(s.len(), 2, "the store stayed ordinary");
        assert_eq!(s.get(c(0, 0)).map(|cell| cell.value), Some(Value::Number(3.0)));
        assert_eq!(s.get(c(70, 1)).map(|cell| cell.value), Some(Value::Text(Arc::from("kept"))));
        std::fs::remove_dir_all(&base).expect("clean up the test directory");
    }

    /// `CELLA_SPILL` parsing, without touching the environment: `off` never pages, `temp`
    /// skips the document's folder, and anything else keeps the default.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_spill_policy_knob_reads_off_and_temp() {
        assert_eq!(page::parse_spill_policy(Some("off")), page::SpillPolicy::Off);
        assert_eq!(page::parse_spill_policy(Some(" OFF ")), page::SpillPolicy::Off);
        assert_eq!(page::parse_spill_policy(Some("temp")), page::SpillPolicy::Temp);
        assert_eq!(page::parse_spill_policy(Some("beside")), page::SpillPolicy::Beside);
        assert_eq!(page::parse_spill_policy(None), page::SpillPolicy::Beside, "unset keeps the default");
        let dir = std::env::temp_dir();
        assert!(page::spill_candidates_for(page::SpillPolicy::Off, Some(&dir), &dir).is_empty(), "off never creates a file");
        let document = std::path::Path::new("a-document-folder");
        assert_eq!(
            page::spill_candidates_for(page::SpillPolicy::Temp, Some(document), &dir),
            vec![(dir.clone(), true)],
            "temp skips the document's folder"
        );
        assert_eq!(
            page::spill_candidates_for(page::SpillPolicy::Beside, Some(document), &dir),
            vec![(std::path::PathBuf::from("a-document-folder"), false), (dir, true)],
            "beside tries the document's folder first"
        );
    }

    /// A Windows UNC document folder is skipped: a page file there would be hundreds of megabytes
    /// of sequential I/O over SMB. A local folder is still tried first.
    #[cfg(windows)]
    #[test]
    fn a_network_document_folder_is_skipped() {
        let temp = std::env::temp_dir();
        let unc = std::path::Path::new(r"\\server\share\reports");
        assert_eq!(page::spill_candidates_for(page::SpillPolicy::Beside, Some(unc), &temp), vec![(temp.clone(), true)], "a UNC folder is not used");
        let local = std::path::Path::new(r"C:\reports");
        assert_eq!(
            page::spill_candidates_for(page::SpillPolicy::Beside, Some(local), &temp),
            vec![(std::path::PathBuf::from(r"C:\reports"), false), (temp, true)],
            "a local folder is"
        );
    }

    /// The name carries the document's stem only: no path, no sheet name, and the sanitized stem is
    /// bounded. An empty hint falls back to `book`.
    #[test]
    fn the_spill_name_never_carries_a_path_or_a_sheet_name() {
        assert_eq!(page::spill_stem(Some("Q3 report.v2")), "Q3reportv2");
        assert_eq!(page::spill_stem(Some("/etc/passwd")), "etcpasswd");
        assert_eq!(page::spill_stem(Some("..")), "book");
        assert_eq!(page::spill_stem(None), "book");
        assert_eq!(page::spill_stem(Some(&"x".repeat(100))).len(), page::SPILL_STEM_MAX);
        let beside = page::spill_file_name("book", None, 3);
        assert_eq!(beside, ".cella-spill-book-3.bin");
        let temp = page::spill_file_name("book", Some(7), 3);
        assert_eq!(temp, ".cella-spill-book-7-3.bin");
    }

    /// The first write failure latches the file shut: later bands are not retried against the same
    /// failing file, the failure is counted once, and no cell is lost.
    #[test]
    fn a_failed_spill_stops_further_spills() {
        let mut s = CellStore::new();
        for row in 0..200u32 {
            s.set(c(row, 0), Cell::value(Value::Number(f64::from(row))));
        }
        assert!(s.enable_paging_with_cache(64 << 20));
        let paged = s.paged_band_count();
        assert!(paged > 0);
        let file = s.page_file.clone().expect("page file");
        file.close_handle();
        // A band added after the volume went away stays live instead of being lost.
        s.set(c(300, 0), Cell::value(Value::Number(300.0)));
        s.spill_pages();
        let first = s.page_cache_stats().expect("the store pages");
        assert_eq!(first.spill_failures, 1, "the failure is counted once");
        assert!(file.has_failed());
        assert!(s.bands.contains_key(&(300u32 / 64)), "the band stayed live");
        assert_eq!(s.get(c(300, 0)).map(|cell| cell.value), Some(Value::Number(300.0)));
        // A second band is not even attempted: the count does not move.
        s.set(c(400, 0), Cell::value(Value::Number(400.0)));
        s.spill_pages();
        assert_eq!(s.page_cache_stats().expect("the store pages").spill_failures, 1);
        assert_eq!(s.get(c(400, 0)).map(|cell| cell.value), Some(Value::Number(400.0)));
        assert_eq!(s.paged_band_count(), paged, "the pages already written are still listed");
    }

    /// A history snapshot spills its own bands and reads them back exactly: what a compressed step
    /// costs is its deflated pages, and the cells, formulas and styles come back as they were.
    #[test]
    fn a_snapshot_spills_its_own_bands_and_expands_back() {
        let mut older = CellStore::new();
        for row in 0..500u32 {
            older.set(c(row, 0), Cell::value(Value::Text(Arc::<str>::from(format!("r{row}").as_str()))));
            let mut cell = plain(row);
            cell.formula = Some(Arc::new(Formula::new(format!("=B{}+1", row + 1).as_str())));
            cell.style = StyleId(2);
            older.set(c(row, 1), cell);
        }
        let len = older.len();
        let mut newer = older.clone();
        // An edit that touches the first band only: that band is the snapshot's own, the rest stay
        // shared with the workbook that replaced it.
        for row in 0..64u32 {
            newer.set(c(row, 0), plain(row + 1));
        }
        let shared = older.retained_bytes_against(&newer);
        assert_eq!(older.spill_retained_against(&newer), 1, "the one band the edit rewrote");
        assert_eq!(older.paged_band_count(), 1);
        assert_eq!(older.bands.len(), 7, "the shared bands were left live");
        assert!(older.paged_bytes() > 0, "the page holds the deflated band");
        let own = older.retained_bytes_against(&newer);
        assert!(own < shared, "a compressed snapshot is charged {own}, live it was {shared}");
        // A spilled band answers exactly what it held: value, formula and style.
        assert_eq!(older.get(c(3, 0)).map(|cell| cell.value), Some(Value::Text(Arc::<str>::from("r3"))));
        let formula = older.get(c(3, 1)).and_then(|cell| cell.formula);
        assert_eq!(formula.and_then(|f| f.text_at(c(3, 1))), Some("B4+1".to_string()));
        assert_eq!(older.get(c(3, 1)).map(|cell| cell.style), Some(StyleId(2)));
        assert_eq!(older.len(), len);
        // Expanding reads every page back into a live band and drops the file, so the snapshot is
        // what it was before the spill.
        assert_eq!(older.expand_pages(), 1);
        assert_eq!((older.paged_band_count(), older.page_cache_stats()), (0, None));
        assert_eq!(older.bands.len(), 8);
        assert_eq!(older.len(), len);
        assert_eq!(older.get(c(3, 1)).and_then(|cell| cell.formula).and_then(|f| f.text_at(c(3, 1))), Some("B4+1".to_string()));
        // The decoded bands are the snapshot's own: the page keeps a row's formula table but not
        // the sharing of it, so the expanded snapshot is charged those tables too — a few kB here,
        // and never the sheet it replaced.
        let expanded = older.retained_bytes_against(&newer);
        assert!(expanded > own && expanded < shared * 5, "expanded costs {expanded}, live {shared}");
        // Nothing is paged any more, and asking again is a no-op.
        assert_eq!(older.expand_pages(), 0);
    }

    /// A store that already pages keeps its file: its pages belong to that file, and spilling its
    /// live bands into a second one would write a copy without freeing anything.
    #[test]
    fn a_store_that_already_pages_is_not_spilled_for_history() {
        let mut s = CellStore::new();
        for row in 0..200u32 {
            s.set(c(row, 0), plain(row));
        }
        assert!(s.enable_paging_with_cache(64 << 20));
        let pages = s.paged_band_count();
        assert!(pages > 0);
        assert_eq!(s.spill_retained_against(&CellStore::new()), 0);
        assert_eq!(s.paged_band_count(), pages);
        assert_eq!(s.get(c(199, 0)), Some(plain(199)));
        // Expanding is what an undo does with a snapshot: every page becomes a live band again.
        assert_eq!(s.expand_pages(), pages);
        assert_eq!((s.paged_band_count(), s.bands.len()), (0, 4));
        assert_eq!(s.get(c(199, 0)), Some(plain(199)));
    }

    /// Pricing a snapshot must not depend on what the store remembers: the tagged walk and the
    /// uncached one answer the same number for every pair a session can produce. This drives both
    /// over a deterministic random edit sequence — values, texts, formulas, arrays, row writes,
    /// removals and value-only batches (the recalculation path) — and re-prices every snapshot
    /// kept against the store as it is after every step.
    #[test]
    fn tagged_retained_pricing_equals_the_full_walk_on_random_edits() {
        // A tiny xorshift: a fixed sequence with no dependency, and the same one on every run.
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut next = |bound: u32| -> u32 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % u64::from(bound.max(1))) as u32
        };
        let mut store = CellStore::new();
        for row in 0..BAND * 4 {
            store.set(c(row, 0), plain(row));
            store.set(c(row, 1), Cell::value(Value::Text(Arc::from(format!("r{row}").as_str()))));
            if row % 3 == 0 {
                store.set(
                    c(row, 2),
                    Cell {
                        formula: Some(Arc::new(Formula::new(format!("=A{}+1", row + 1).as_str()))),
                        value: Value::Number(f64::from(row)),
                        ..Default::default()
                    },
                );
            }
            if row % 7 == 0 {
                store.set(c(row, 3), Cell { style: StyleId(2), ..Default::default() });
            }
            if row % 11 == 0 {
                store.set(c(row, 4), Cell::value(Value::Array(Arc::new(cella_core::Array::row(vec![Value::Number(1.0)])))));
            }
        }
        store.set(c(3, 5), Cell::value(Value::Bool(true)));
        let mut snapshots: Vec<(u32, CellStore)> = Vec::new();
        for step in 0..240u32 {
            if step % 20 == 0 {
                snapshots.push((step, store.clone()));
                if snapshots.len() > 6 {
                    snapshots.remove(0);
                }
            }
            let row = next(BAND * 4);
            let col = next(6);
            match next(12) {
                0 => {
                    store.set(c(row, col), plain(row + col));
                }
                1 => {
                    store.set(c(row, col), Cell::value(Value::Text(Arc::from(format!("t{step}").as_str()))));
                }
                2 => {
                    store.set(
                        c(row, 2),
                        Cell { formula: Some(Arc::new(Formula::new(format!("=A{}+{step}", row + 1).as_str()))), ..Default::default() },
                    );
                }
                3 => {
                    let _ = store.remove(c(row, col));
                }
                4 => {
                    let _ = store.set_value(c(row, 2), &Value::Number(f64::from(step)));
                }
                5 => {
                    let _ = store.set_value(c(row, 0), &Value::Number(f64::from(step)));
                }
                6 => {
                    // The recalculation shape: a value-only batch over a whole column of formulas.
                    let mut batch: Vec<(CellRef, Value)> = (0..BAND * 4).map(|r| (c(r, 2), Value::Number(f64::from(r + step)))).collect();
                    store.set_values(&mut batch);
                }
                7 => {
                    let mut cells = vec![(0u32, plain(row)), (1, Cell::value(Value::Text(Arc::from("row"))))];
                    store.set_row(row, &mut cells);
                }
                8 => {
                    store.set(c(row, 4), Cell::value(Value::Array(Arc::new(cella_core::Array::row(vec![Value::Number(f64::from(step))])))));
                }
                9 => {
                    // A stated cell rewritten with itself: the copy is the same content.
                    if let Some(cell) = store.get(c(row, col)) {
                        store.set(c(row, col), cell);
                    }
                }
                10 => {
                    store.set(c(row, 3), Cell { style: StyleId(step % 5), ..Default::default() });
                }
                _ => {
                    let _ = store.set_value(c(row, 1), &Value::Text(Arc::from(format!("v{step}").as_str())));
                }
            }
            for (at, snapshot) in &snapshots {
                assert_eq!(
                    snapshot.retained_bytes_against(&store),
                    snapshot.retained_bytes_against_full(&store),
                    "after step {step}, the snapshot taken at step {at}"
                );
            }
        }
    }

    /// A value-only edit (a recalculation) copies a band but keeps every row's formula table: the
    /// tag says so, the price is the fixed part alone, and it equals the full walk. An edit that
    /// rewrites a formula, drops a row or replaces an array must not make that claim.
    #[test]
    fn a_value_only_edit_keeps_the_rows_its_snapshot_keeps() {
        let mut store = CellStore::new();
        for row in 0..BAND * 3 {
            store.set(
                c(row, 2),
                Cell {
                    formula: Some(Arc::new(Formula::new(format!("=A{}+1", row + 1).as_str()))),
                    value: Value::Number(f64::from(row)),
                    ..Default::default()
                },
            );
            store.set(c(row, 0), plain(row));
        }
        let snapshot = store.clone();
        let mut values: Vec<(CellRef, Value)> = (0..BAND * 3).map(|row| (c(row, 2), Value::Number(f64::from(row) * 2.0))).collect();
        store.set_values(&mut values);
        for key in 0..3u32 {
            let old = snapshot.bands.get(&key).expect("a band");
            let new = store.bands.get(&key).expect("a band");
            assert!(old.shares_rows_with(new), "band {key} was copied for its values only");
        }
        assert_eq!(snapshot.retained_bytes_against(&store), snapshot.retained_bytes_against_full(&store));

        // A formula rewritten in one band: that band's rows are no longer the snapshot's.
        let before = store.clone();
        store.set(c(5, 2), Cell { formula: Some(Arc::new(Formula::new("=A6*2"))), ..Default::default() });
        let old = before.bands.get(&0).expect("a band");
        let new = store.bands.get(&0).expect("a band");
        assert!(!old.shares_rows_with(new), "a formula edit rewrote the row's side table");
        assert_eq!(before.retained_bytes_against(&store), before.retained_bytes_against_full(&store));

        // A row dropped from the store: the snapshot's rows are not the newer band's any more.
        let before = store.clone();
        for col in 0..5u32 {
            let _ = store.remove(c(5, col));
        }
        let old = before.bands.get(&0).expect("a band");
        let new = store.bands.get(&0).expect("the band still holds other rows");
        assert!(!old.shares_rows_with(new), "a removed row is not a shared row");
        assert_eq!(before.retained_bytes_against(&store), before.retained_bytes_against_full(&store));

        // Two stores built separately never shared a band, whatever their content.
        let mut twin = CellStore::new();
        for row in 0..BAND * 3 {
            twin.set(
                c(row, 2),
                Cell {
                    formula: Some(Arc::new(Formula::new(format!("=A{}+1", row + 1).as_str()))),
                    value: Value::Number(f64::from(row) * 2.0),
                    ..Default::default()
                },
            );
            twin.set(c(row, 0), plain(row));
        }
        assert!(!snapshot.bands.get(&0).expect("a band").shares_rows_with(twin.bands.get(&0).expect("a band")));
        assert_eq!(snapshot.retained_bytes_against(&twin), snapshot.retained_bytes_against_full(&twin));
    }

    /// A process-unique number for a test's temp directory name (the clock is not read: it traps on
    /// wasm, and these tests are native only).
    fn page_stem_nonce() -> u64 {
        use std::sync::atomic::AtomicU64;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    #[test]
    fn the_page_cache_evicts_to_a_byte_budget() {
        let mut s = CellStore::new();
        for r in 0..3u32 {
            s.set(c(r * 64, 0), Cell::value(Value::Number(f64::from(r))));
        }
        assert!(s.enable_paging_with_cache(1), "one byte is a one-band budget");
        assert_eq!(s.paged_band_count(), 3);
        assert!(s.get(c(0, 0)).is_some());
        let first = s.page_cache_stats().expect("the store pages");
        assert_eq!((first.hits, first.misses, first.evictions), (0, 1, 0), "the first read decodes");
        assert!(first.bytes > first.budget, "a band bigger than the budget is kept rather than re-decoded");
        assert_eq!(first.max_entry_bytes, first.bytes, "the oversize band is reported");
        assert!(s.pages.get(&0).is_some_and(|page| page.is_cached()));
        // A second band takes the total past the budget, so the first one is evicted.
        assert!(s.get(c(64, 0)).is_some());
        let second = s.page_cache_stats().expect("the store pages");
        assert_eq!((second.hits, second.misses, second.evictions), (0, 2, 1));
        assert!(!s.pages.get(&0).is_some_and(|page| page.is_cached()), "the oldest band was evicted");
        assert!(s.pages.get(&1).is_some_and(|page| page.is_cached()));
        assert_eq!(second.bytes, second.max_entry_bytes, "only the band just decoded is charged");
        // Re-reading the evicted page decodes it again and restores the same charge.
        assert!(s.get(c(0, 0)).is_some());
        let third = s.page_cache_stats().expect("the store pages");
        assert_eq!((third.hits, third.misses, third.evictions), (0, 3, 2));
        assert_eq!(third.bytes, first.bytes, "the same band is charged the same bytes again");
        assert!(!s.pages.get(&1).is_some_and(|page| page.is_cached()));
        assert!(s.pages.get(&0).is_some_and(|page| page.is_cached()));
        // A cached read is a hit and charges nothing.
        assert!(s.get(c(0, 0)).is_some());
        let fourth = s.page_cache_stats().expect("the store pages");
        assert_eq!((fourth.hits, fourth.misses, fourth.evictions), (1, 3, 2));
        assert_eq!(fourth.bytes, third.bytes);
    }

    #[test]
    fn a_store_without_paging_has_no_page_cache() {
        assert!(CellStore::new().page_cache_stats().is_none());
    }

    /// The uncached scan yields exactly the cached walk: live, paged, and mapped-and-paged, with a
    /// row as wide as the grid and a formula cell whose cached value a writer has to see.
    #[test]
    fn iter_scan_matches_iter_range() {
        let mut s = CellStore::new();
        for row in 0..200u32 {
            for col in 0..4u32 {
                s.set(c(row, col), Cell { value: Value::Number(f64::from(row * 10 + col)), formula: None, style: StyleId(col % 3) });
            }
            s.set(c(row, 5), Cell::value(Value::Text(Arc::from(format!("r{row}").as_str()))));
        }
        // One row spanning every column the grid has.
        let mut wide: Vec<(u32, Cell)> = (0..cella_core::MAX_COLS).map(|col| (col, Cell::value(Value::Number(f64::from(col))))).collect();
        s.set_row(250, &mut wide);
        s.set(c(251, 0), Cell { value: Value::Number(3.0), formula: Some(Arc::new(Formula::new("=1+2"))), style: StyleId(2) });
        let live: Vec<(CellRef, Cell)> = s.iter_range(RangeRef::all()).collect();
        assert!(live.iter().any(|(at, _)| at.col == cella_core::MAX_COLS - 1), "the wide row is in the walk");
        assert_eq!(s.iter_scan(RangeRef::all()).collect::<Vec<_>>(), live, "a live store scans like it walks");

        let mut paged = s.clone();
        assert!(paged.enable_paging_with_cache(64 << 20), "a temporary page file is created");
        assert!(paged.paged_band_count() > 0, "the bands were spilled");
        assert!(paged.bands.is_empty(), "every band is on the page file");
        assert_eq!(paged.iter_scan(RangeRef::all()).collect::<Vec<_>>(), live, "a paged store scans like a live one");

        // A mapped store applies the row and column maps the same way on both paths.
        let mut mapped = paged.clone();
        mapped.shift_rows(1, 1);
        mapped.shift_cols(1, 1);
        let walked: Vec<(CellRef, Cell)> = mapped.iter_range(RangeRef::all()).collect();
        assert_eq!(mapped.iter_scan(RangeRef::all()).collect::<Vec<_>>(), walked, "a mapped paged store scans like it walks");
        let mut live_mapped = s.clone();
        live_mapped.shift_rows(1, 1);
        live_mapped.shift_cols(1, 1);
        assert_eq!(mapped.iter_scan(RangeRef::all()).collect::<Vec<_>>(), live_mapped.iter_range(RangeRef::all()).collect::<Vec<_>>());
    }

    /// A scan decodes each paged band once and leaves the cache exactly as it found it: a save must
    /// not evict the viewport's working set or leave the sheet's tail resident.
    #[test]
    fn a_scan_decodes_each_paged_band_once_without_caching() {
        let mut s = CellStore::new();
        for row in 0..200u32 {
            s.set(c(row, 0), Cell::value(Value::Number(f64::from(row))));
            s.set(c(row, 1), Cell::value(Value::Text(Arc::from(format!("r{row}").as_str()))));
        }
        assert!(s.enable_paging_with_cache(64 << 20), "a temporary page file is created");
        let bands = s.paged_band_count();
        assert!(bands >= 4, "200 rows over 64-row bands");
        // Prime the cache the way a viewport would, so a scan that went through it would show.
        assert!(s.get(c(0, 0)).is_some());
        let before = s.page_cache_stats().expect("the store pages");
        assert_eq!((before.hits, before.misses), (0, 1));
        let decodes = s.page_file.as_ref().map(|file| file.decodes()).unwrap_or(0);

        assert_eq!(s.iter_scan(RangeRef::all()).count(), 400);

        let after = s.page_cache_stats().expect("the store pages");
        assert_eq!(s.page_file.as_ref().map(|file| file.decodes()).unwrap_or(0), decodes + bands as u64, "each band decoded exactly once");
        assert_eq!(after.bytes, before.bytes, "the scan charged nothing to the budget");
        assert_eq!(after.entries, before.entries, "the scan queued nothing");
        assert_eq!(
            (after.hits, after.misses, after.evictions),
            (before.hits, before.misses, before.evictions),
            "the scan was neither a hit nor a miss"
        );
        assert_eq!(after.corrupt, before.corrupt);
        assert!(s.pages.get(&0).is_some_and(|page| page.is_cached()), "the primed band is still cached");
    }

    /// `visit_scan` stops on the first `false` and leaves no decoded band behind; the full walk
    /// reads every cell.
    #[test]
    fn visit_scan_stops_on_false_and_drops_the_band() {
        let mut s = CellStore::new();
        for row in 0..200u32 {
            s.set(c(row, 0), Cell::value(Value::Number(f64::from(row))));
        }
        assert!(s.enable_paging_with_cache(64 << 20), "a temporary page file is created");
        assert!(s.paged_band_count() > 0);
        let mut seen: Vec<CellRef> = Vec::new();
        let completed = s.visit_scan(RangeRef::all(), &mut |at, _| {
            seen.push(at);
            seen.len() < 3
        });
        assert!(!completed, "returning false stops the walk");
        assert_eq!(seen, [c(0, 0), c(1, 0), c(2, 0)]);
        assert!(s.pages.values().all(|page| !page.is_cached()), "the stopped scan left no band in the cache");
        let stats = s.page_cache_stats().expect("the store pages");
        assert_eq!((stats.bytes, stats.entries, stats.misses), (0, 0, 0));
        let mut count = 0usize;
        assert!(s.visit_scan(RangeRef::all(), &mut |_, _| {
            count += 1;
            true
        }));
        assert_eq!(count, 200, "the full walk visits every cell");
    }

    /// A page that cannot be decoded is a skipped band: the rest of the sheet still scans, nothing
    /// panics and nothing is cached.
    #[test]
    fn a_scan_skips_a_page_it_cannot_decode() {
        let mut s = CellStore::new();
        for row in 0..200u32 {
            s.set(c(row, 0), Cell::value(Value::Number(f64::from(row))));
        }
        assert!(s.enable_paging_with_cache(64 << 20), "a page file is created");
        assert_eq!(s.paged_band_count(), 4);
        // Pages are appended in band order, so band 0's page starts at offset 0. Overwrite it with
        // bytes that are not a page, through the file's own handle (on Unix the name is gone).
        let file = s.page_file.clone().expect("page file");
        let len = s.pages.get(&0).map(|page| page.page_len()).expect("the first page");
        assert!(file.corrupt(0, len), "overwrite the first page");
        let rows: Vec<u32> = s.iter_scan(RangeRef::all()).map(|(at, _)| at.row).collect();
        assert_eq!(rows, (64..200).collect::<Vec<_>>(), "only the corrupt band's rows are missing");
        assert!(s.pages.values().all(|page| !page.is_cached()), "a skipped page is not remembered");
        assert_eq!(s.page_cache_stats().expect("the store pages").corrupt, 0, "the scan does not count cache corruption");
        // The cached path still refuses the page the same way: it is missing, not a panic.
        assert_eq!(s.get(c(0, 0)), None);
        assert!(s.get(c(64, 0)).is_some());
    }

    /// The `CELLA_PAGE_CACHE_MB` value is clamped before it becomes bytes; the environment
    /// itself is never touched by a test, since it is process-global and these run in parallel.
    #[test]
    fn the_cache_budget_knob_is_clamped_to_its_bounds() {
        assert_eq!(page::cache_bytes_for_mb(0), 8 << 20, "a tiny budget is raised to the floor");
        assert_eq!(page::cache_bytes_for_mb(64), 64 << 20);
        assert_eq!(page::cache_bytes_for_mb(usize::MAX), 512 << 20, "an absurd value saturates the cap");
    }

    /// The budget belongs to the page file, so a clone reads into the same cache and its decode is
    /// visible from the store it came from.
    #[test]
    fn a_clone_decodes_into_the_same_page_cache() {
        let mut s = CellStore::new();
        for r in 0..2u32 {
            s.set(c(r * 64, 0), Cell::value(Value::Number(f64::from(r))));
        }
        assert!(s.enable_paging_with_cache(1));
        let clone = s.clone();
        assert!(clone.get(c(0, 0)).is_some());
        let one = s.page_cache_stats().expect("the store pages");
        assert_eq!(one.misses, 1);
        assert_eq!(clone.page_cache_stats(), Some(one), "the clone reports the same cache");
        // The clone's second band evicts the band the original decoded.
        assert!(clone.get(c(64, 0)).is_some());
        let two = s.page_cache_stats().expect("the store pages");
        assert_eq!((two.misses, two.evictions), (2, 1));
        assert!(!s.pages.get(&0).is_some_and(|page| page.is_cached()), "the eviction is shared");
        assert_eq!(clone.page_cache_stats(), Some(two));
    }

    /// A snapshot that still holds a page the newer store replaced keeps the band charged: the
    /// cache's `Weak` upgrades, so the memory is real and stays on the budget.
    #[test]
    fn a_snapshot_keeps_a_replaced_pages_bytes_charged() {
        let mut s = CellStore::new();
        for row in 0..200u32 {
            s.set(c(row, 0), Cell::value(Value::Number(f64::from(row))));
        }
        assert!(s.enable_paging_with_cache(page::default_cache_bytes()));
        let snapshot = s.clone();
        assert!(s.get(c(0, 0)).is_some());
        let old = s.page_cache_stats().expect("the store pages");
        assert_eq!(old.entries, 1);
        s.shift_rows(1, -1);
        assert_eq!(s.compact().pages, 1);
        let rewritten = s.pages.get(&0).cloned().expect("the rewritten page");
        assert!(rewritten.is_cached(), "the rewrite is primed for the read that follows");
        let now = s.page_cache_stats().expect("the store pages");
        assert_eq!(now.entries, 2, "the old page's charge is still queued");
        assert_eq!(now.bytes, old.bytes.saturating_add(rewritten.cached_bytes()));
        assert!(snapshot.pages.get(&0).is_some_and(|page| page.is_cached()), "the snapshot still holds the old band");
    }

    /// [`CellStore::visit_values`] walks exactly what [`CellStore::iter_range`] yields: the same
    /// order, the same values, the same formula flag — and it stops when the callback says so.
    #[test]
    fn visit_values_walks_what_iter_range_yields() {
        let mut store = CellStore::new();
        store.set(c(0, 0), Cell::value(Value::Number(4.5)));
        store.set(c(0, 2), Cell::value(Value::Text(Arc::from("hi"))));
        store.set(c(1, 1), Cell { formula: Some(Arc::new(Formula::new("A1*2"))), value: Value::Number(9.0), style: StyleId(1) });
        store.set(c(70, 0), Cell::value(Value::Bool(true)));
        store.set(c(BAND + 3, 1), Cell::value(Value::Error(cella_core::CellError::Div0)));
        store.set(c(BAND + 4, 0), Cell { style: StyleId(2), ..Default::default() });
        let range = RangeRef::new(c(0, 0), c(BAND + 4, 2));
        let walked: Vec<(CellRef, Cell)> = store.iter_range(range).collect();
        let mut seen: Vec<(CellRef, Value, bool)> = Vec::new();
        assert!(store.visit_values(range, &mut |at, value, formula| {
            seen.push((at, value, formula));
            true
        }));
        assert_eq!(seen.len(), walked.len(), "the two walks see the same cells");
        for ((at, value, formula), (expected_at, cell)) in seen.iter().zip(&walked) {
            assert_eq!(at, expected_at, "the same order");
            assert_eq!(value, &cell.value, "{at:?}");
            assert_eq!(*formula, cell.formula.is_some(), "{at:?}");
        }
        let mut calls = 0;
        assert!(!store.visit_values(range, &mut |_, _, _| {
            calls += 1;
            calls < 2
        }));
        assert_eq!(calls, 2, "the callback stops the walk");
    }

    /// A full-sheet scan cannot grow the cache past its budget: every decoded band is charged, and
    /// the oldest ones are evicted until the total fits.
    #[test]
    fn a_full_scan_stays_within_the_byte_budget() {
        fn scan_store(bands: u32) -> CellStore {
            let mut s = CellStore::new();
            for row in 0..bands * 64 {
                s.set(c(row, 0), Cell::value(Value::Number(f64::from(row))));
                s.set(c(row, 1), Cell::value(Value::Text(Arc::from("x"))));
            }
            s
        }
        // The decoded size of one band, measured through a cache that never evicts it.
        let mut probe = scan_store(1);
        assert!(probe.enable_paging_with_cache(usize::MAX));
        assert!(probe.get(c(0, 0)).is_some());
        let one = probe.page_cache_stats().expect("the probe pages").bytes;
        assert!(one > 0);
        let mut s = scan_store(8);
        assert!(s.enable_paging_with_cache(one.saturating_mul(2)));
        let bands = s.paged_band_count();
        assert!(bands > 4);
        assert_eq!(s.iter().count(), 8 * 64 * 2);
        let stats = s.page_cache_stats().expect("the store pages");
        assert!(stats.bytes <= stats.budget, "{} bytes over a {} byte budget", stats.bytes, stats.budget);
        assert_eq!(stats.budget, one.saturating_mul(2));
        assert_eq!(stats.misses, bands as u64, "every band was decoded once");
        assert_eq!(stats.hits, 0);
        assert!(stats.evictions > 0, "the scan decoded more than the budget holds");
        assert!(stats.entries > 0);
    }

    /// A page compaction primes is cache content: charged when it is installed, and evictable when
    /// the next decode pushes the budget past it. Under a page count it was never queued.
    #[test]
    fn compaction_charges_the_pages_it_primes_and_they_evict() {
        let mut s = CellStore::new();
        for row in 0..200u32 {
            s.set(c(row, 0), Cell::value(Value::Number(f64::from(row))));
        }
        assert!(s.enable_paging_with_cache(1));
        s.shift_rows(1, -1);
        assert_eq!(s.compact().pages, 1);
        let page = s.pages.get(&0).cloned().expect("the rewritten page");
        assert!(page.is_cached(), "the rewrite is primed");
        let primed = s.page_cache_stats().expect("the store pages");
        assert_eq!((primed.entries, primed.misses, primed.evictions), (1, 0, 0), "priming is charged, not a read");
        assert_eq!(primed.bytes, page.cached_bytes());
        assert_eq!(primed.max_entry_bytes, page.cached_bytes());
        // Decoding another band is over the one-byte budget, so the primed page is cleared.
        assert!(s.get(c(64, 0)).is_some());
        assert!(!page.is_cached(), "the primed band was evictable");
        let after = s.page_cache_stats().expect("the store pages");
        assert_eq!(after.evictions, 1);
        assert!(after.bytes > 0);
    }

    #[test]
    fn a_band_round_trips_through_a_page() {
        use cella_core::Array;
        let mut batch = BandBatch::new();
        let mut cells = vec![
            (0, Cell { value: Value::Number(1.25), formula: None, style: StyleId(3) }),
            (1, Cell::formula(Formula::new("=A1+1"))),
            (2, Cell::value(Value::Text(Arc::from("hello")))),
            (3, Cell::value(Value::Bool(true))),
            (4, Cell::value(Value::Error(CellError::Div0))),
            (5, Cell::value(Value::Array(Arc::new(Array::row(vec![Value::Number(2.0), Value::Text(Arc::from("x"))]))))),
        ];
        batch.set_row(0, &mut cells);
        let mut cells = vec![(0, Cell::value(Value::Number(-0.0))), (1, Cell::value(Value::Text(Arc::from("hello"))))];
        batch.set_row(1, &mut cells);
        let mut s = CellStore::new();
        s.insert_batch(batch);
        let band = s.bands.get(&0).expect("band");
        let bytes = page::encode_band(band).expect("encode");
        let file = page::PageFile::new(None, None, page::default_cache_bytes()).expect("page file");
        let at = file.write(&bytes).expect("write");
        let page = page::Page::new(at, page::band_bounds(band), true, true);
        let loaded = page.load(&file).0.expect("decode");
        assert!(band.same_as(&loaded), "the decoded band holds the same cells");
        for row in 0..2u32 {
            for col in 0..6u32 {
                assert_eq!(
                    s.get(c(row, col)),
                    {
                        let mut other = CellStore::new();
                        other.bands.insert(0, loaded.clone());
                        other.len = s.len();
                        other.get(c(row, col))
                    },
                    "row {row} col {col}"
                );
            }
        }
        // A corrupt or truncated page is refused rather than trusted.
        let mut corrupt = bytes.clone();
        if let Some(byte) = corrupt.first_mut() {
            *byte ^= 0xff;
        }
        assert!(page::decode_band(&corrupt).is_none());
        assert!(page::decode_band(&bytes[..bytes.len().saturating_sub(1)]).is_none());
        let mut huge = bytes.clone();
        huge.truncate(12);
        if huge.len() == 12 {
            huge[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        }
        assert!(page::decode_band(&huge).is_none());
    }

    /// A finished band keeps its texts but not the pointer index that only adding cells needs;
    /// the first edit rebuilds it. On a unique-key export this index was 16 bytes for every text
    /// of every band (640 MiB on the 100M-cell export).
    /// Paging takes completed bands out of the live map and answers the same reads from the page
    /// file; an edit brings only the touched band back, and a snapshot taken before keeps its
    /// values.
    #[test]
    fn paged_bands_read_and_edit_like_live_bands() {
        let mut s = CellStore::new();
        for r in 0..200u32 {
            s.set(c(r, 0), Cell::value(Value::Number(f64::from(r))));
            s.set(c(r, 1), Cell::value(Value::Text(Arc::from(if r % 2 == 0 { "even" } else { "odd" }))));
        }
        s.set(c(3, 2), Cell::formula(Formula::new("=A4+1")));
        s.set(c(3, 2), Cell { value: Value::Number(4.0), formula: Some(Arc::new(Formula::new("=A4+1"))), style: StyleId(2) });
        let snapshot = s.clone();
        assert!(s.enable_paging(), "a temporary page file is created");
        assert!(s.paged_band_count() > 0, "the completed bands were spilled");
        assert_eq!(s.len(), snapshot.len());
        assert_eq!(s.used_range(), snapshot.used_range());
        for r in 0..200u32 {
            assert_eq!(s.get(c(r, 0)), snapshot.get(c(r, 0)), "row {r} number");
            assert_eq!(s.get(c(r, 1)), snapshot.get(c(r, 1)), "row {r} text");
            assert_eq!(s.style(c(r, 0)), snapshot.style(c(r, 0)));
            assert!(s.contains(c(r, 1)));
        }
        assert_eq!(s.iter_range(RangeRef::parse("A1:C50").expect("range")).count(), 101);
        assert_eq!(s.iter().count(), 401);
        assert_eq!(s.formulas().count(), 1);
        assert_eq!(s.diff(&snapshot), []);
        // Editing a paged band materializes just that band and keeps the snapshot untouched.
        assert!(s.set_value(c(70, 0), &Value::Number(999.0)));
        assert_eq!(s.get(c(70, 0)).map(|x| x.value), Some(Value::Number(999.0)));
        assert_eq!(snapshot.get(c(70, 0)).map(|x| x.value), Some(Value::Number(70.0)));
        s.set(c(71, 0), Cell::value(Value::Text(Arc::from("new"))));
        assert_eq!(s.get(c(71, 0)).map(|x| x.value), Some(Value::Text(Arc::from("new"))));
        assert_eq!(s.remove(c(72, 0)).map(|x| x.value), Some(Value::Number(72.0)));
        assert_eq!(s.diff(&snapshot), [c(70, 0), c(71, 0), c(72, 0)]);
        // A band split across two batches still merges after paging.
        let mut batch = BandBatch::new();
        let mut cells = vec![(4, Cell::value(Value::Number(1234.0)))];
        batch.set_row(74, &mut cells);
        s.insert_batch(batch);
        assert_eq!(s.get(c(74, 4)).map(|x| x.value), Some(Value::Number(1234.0)));
    }

    #[test]
    fn a_finished_band_drops_its_text_index_and_rebuilds_it_on_edit() {
        let alpha: Arc<str> = Arc::from("alpha");
        let beta: Arc<str> = Arc::from("beta");
        let mut batch = BandBatch::new();
        let mut cells = vec![
            (0, Cell::value(Value::Text(alpha.clone()))),
            (1, Cell::value(Value::Text(alpha.clone()))),
            (2, Cell::value(Value::Text(beta.clone()))),
        ];
        batch.set_row(0, &mut cells);
        let mut s = CellStore::new();
        s.insert_batch(batch);
        let band = s.bands.get(&0).expect("band");
        assert!(band.text_index.is_empty(), "the completed band dropped its text index");
        assert_eq!(band.texts.len(), 2, "the same Arc is stored once");
        assert_eq!(s.get(c(0, 0)).map(|x| x.value), Some(Value::Text(alpha.clone())));
        assert_eq!(s.get(c(0, 2)).map(|x| x.value), Some(Value::Text(beta.clone())));
        // The edit finds the existing text through the rebuilt index and interns a new one.
        s.set(c(1, 0), Cell::value(Value::Text(alpha.clone())));
        s.set(c(1, 1), Cell::value(Value::Text(Arc::from("gamma"))));
        let band = s.bands.get(&0).expect("band");
        assert_eq!(band.texts.len(), 3);
        assert!(!band.text_index.is_empty(), "the index is back while the band is being edited");
        assert_eq!(s.get(c(1, 0)).map(|x| x.value), Some(Value::Text(alpha)));
        assert_eq!(s.get(c(1, 1)).map(|x| x.value), Some(Value::Text(Arc::from("gamma"))));
    }

    /// A band with more texts than a `u16` can index stores the actual slot of an existing text in
    /// the row's wide table. The old code stored the last text instead, which was only right when
    /// the text was the one that had just been added.
    #[test]
    fn a_text_past_the_u16_slots_keeps_its_own_slot() {
        let mut band = Band::default();
        let mut texts: Vec<Arc<str>> = Vec::new();
        for i in 0..=65_536usize {
            let text: Arc<str> = Arc::from(format!("t{i}"));
            assert_eq!(band.text_slot(&text), i as u32, "slot {i}");
            texts.push(text);
        }
        // One more text so the band's last slot is not the one the lookup asks for.
        assert_eq!(band.text_slot(&Arc::from("extra")), 65_537);
        let found = texts.get(65_536).cloned().expect("the text at the last u16 slot");
        let (kind, payload) = band.payload(&Value::Text(found));
        assert_eq!(kind, TEXT_WIDE);
        let Payload::Wide(slot) = payload else { panic!("expected a wide payload") };
        assert_eq!(slot, 65_536);
        assert_eq!(&*band.text(slot), "t65536");
    }

    #[test]
    fn set_values_writes_in_row_order() {
        let mut s = CellStore::new();
        let mut values: Vec<(CellRef, Value)> = Vec::new();
        for r in (0..200).rev() {
            s.set(c(r, 4), Cell::formula(Formula::new("=1+1")));
            values.push((c(r, 4), Value::Number(r as f64)));
        }
        values.sort_unstable_by_key(|(c, _)| (c.row, c.col));
        s.set_values(&mut values);
        for r in 0..200 {
            assert_eq!(s.get(c(r, 4)).unwrap().value, Value::Number(r as f64));
        }
        assert_eq!(s.len(), 200);
    }

    /// The large-batch path (above `PARALLEL_WRITE_MIN`) copies bands in workers and replaces them
    /// only after every worker finished; the old snapshot must keep its values.
    #[test]
    fn parallel_value_writes_keep_the_snapshot() {
        let mut s = CellStore::new();
        for r in 0..20_000u32 {
            s.set(c(r, 9), Cell::formula(Formula::new("=$A$1*2")));
        }
        let snapshot = s.clone();
        let mut values: Vec<(CellRef, Value)> = (0..20_000u32).map(|r| (c(r, 9), Value::Number(f64::from(r)))).collect();
        s.set_values(&mut values);
        for r in 0..20_000u32 {
            assert_eq!(s.get(c(r, 9)).unwrap().value, Value::Number(f64::from(r)));
            assert_eq!(snapshot.get(c(r, 9)).unwrap().value, Value::Empty);
        }
        assert_eq!(s.len(), 20_000);
        assert_eq!(snapshot.len(), 20_000);
    }

    /// A result batch writes through the row map: the logical rows are not the physical ones, and
    /// neither `len` nor the bands move.
    #[test]
    fn set_values_writes_through_a_row_map() {
        let mut s = CellStore::new();
        for r in 0..200u32 {
            s.set(c(r, 4), Cell::formula(Formula::new("=1+1")));
        }
        let (bands, len) = (s.band_count(), s.len());
        s.shift_rows(1, 1);
        assert_eq!(s.band_count(), bands, "a row insert moves the map, not the bands");
        let mut values: Vec<(CellRef, Value)> = (0..200u32).map(|r| (c(r, 4), Value::Number(f64::from(r)))).collect();
        s.set_values(&mut values);
        for r in 0..200u32 {
            // An insert at logical row 1 leaves that row a gap; the batch's value there is dropped.
            let want = if r == 1 { None } else { Some(Value::Number(f64::from(r))) };
            assert_eq!(s.get(c(r, 4)).map(|x| x.value), want, "row {r}");
        }
        assert_eq!(s.get(c(1, 4)), None, "the inserted row stays empty");
        assert_eq!(s.len(), len);
        assert_eq!(s.band_count(), bands);
    }

    /// A value on a logical row the map does not cover has nowhere to go: a gap row must neither
    /// allocate a physical row nor create a band, exactly as `set_value` refuses it.
    #[test]
    fn set_values_skips_a_gap_row_without_allocating() {
        let mut s = CellStore::new();
        s.set(c(0, 0), Cell::formula(Formula::new("=1")));
        s.shift_rows(0, 1);
        let (next_physical, bands) = (s.row_map.next_physical, s.band_count());
        let mut values = vec![(c(0, 0), Value::Number(1.0)), (c(1, 0), Value::Number(2.0))];
        s.set_values(&mut values);
        assert_eq!(s.get(c(1, 0)).map(|x| x.value), Some(Value::Number(2.0)));
        assert_eq!(s.row_map.next_physical, next_physical, "the gap row did not allocate a physical row");
        assert_eq!(s.band_count(), bands, "and did not create a band");
        assert_eq!(s.len(), 1);
    }

    /// The differential proof: a streamed batch must leave exactly the store `set_value` per cell
    /// leaves, on a store mapped along both axes (two row segments, two column segments). The
    /// batch covers the gap row and gap column and rows/columns no cell holds.
    #[test]
    fn set_values_equals_value_at_a_time_on_a_mapped_store() {
        let mut s = CellStore::new();
        for r in 0..140u32 {
            for col in 0..8u32 {
                s.set(c(r, col), Cell::formula(Formula::new("=1+1")));
                s.set_value(c(r, col), &Value::Number(f64::from(r * 10 + col)));
            }
        }
        s.shift_rows(1, 1);
        s.shift_cols(1, 1);
        let mut slow = s.clone();
        let mut values: Vec<(CellRef, Value)> = Vec::new();
        for r in 0..=141u32 {
            for col in 0..=9u32 {
                values.push((c(r, col), Value::Number(f64::from(r * 100 + col))));
            }
        }
        let mut fast = s;
        fast.set_values(&mut values);
        for (at, v) in values.iter() {
            slow.set_value(*at, v);
        }
        assert_eq!(fast.iter().collect::<Vec<_>>(), slow.iter().collect::<Vec<_>>());
        assert_eq!(fast, slow);
    }

    /// The batch must be in row-major order; a caller that hands in another order gets the
    /// value-at-a-time path (the same result), never a value on a band-grouped wrong row.
    #[test]
    fn set_values_writes_an_unsorted_batch_like_set_value() {
        let mut s = CellStore::new();
        for r in 0..20u32 {
            s.set(c(r, 0), Cell::formula(Formula::new("=1+1")));
        }
        s.shift_rows(1, 1);
        let mut slow = s.clone();
        let mut values: Vec<(CellRef, Value)> = (0..20u32).map(|r| (c(r, 0), Value::Number(f64::from(r)))).collect();
        values.reverse();
        let mut fast = s;
        fast.set_values(&mut values);
        for (at, v) in values.iter() {
            slow.set_value(*at, v);
        }
        assert_eq!(fast.iter().collect::<Vec<_>>(), slow.iter().collect::<Vec<_>>());
    }

    /// A band fed by two row segments must stay one task in the parallel plan: two runs of the
    /// same band inserted as two bands would let one run's writes win. This batch is one band
    /// wide (64 rows by 300 columns, 19,200 values) and spans the inserted row inside band 0.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn set_values_writes_a_band_split_by_two_row_segments() {
        let mut s = CellStore::new();
        for r in 0..64u32 {
            let mut cells: Vec<(u32, Cell)> = (0..300u32).map(|col| (col, Cell::value(Value::Number(f64::from(r * 1_000 + col))))).collect();
            s.set_row(r, &mut cells);
        }
        s.shift_rows(30, 1);
        let segments = s.row_map.segments_in(0, 63);
        assert_eq!(segments.len(), 2, "the batch spans the insert point");
        assert!(segments.iter().all(|seg| seg.physical / BAND == 0), "both runs land in band 0");
        let mut values: Vec<(CellRef, Value)> = Vec::new();
        for r in 0..64u32 {
            for col in 0..300u32 {
                values.push((c(r, col), Value::Number(f64::from(r * 1_000 + col + 7))));
            }
        }
        let mut slow = s.clone();
        assert!(s.set_values_mapped_parallel(&values, 4), "the plan fits the run budget");
        for (at, v) in values.iter() {
            slow.set_value(*at, v);
        }
        assert_eq!(s.iter().collect::<Vec<_>>(), slow.iter().collect::<Vec<_>>());
        assert_eq!(s.band_count(), 1, "the two runs write one band");
    }

    /// A paged band a result lands in comes back once, through the map; the batch does not leave a
    /// second copy or an undecoded page behind.
    #[test]
    fn set_values_materializes_a_paged_band_once() {
        let mut s = CellStore::new();
        for r in 0..200u32 {
            s.set(c(r, 0), Cell::formula(Formula::new("=1+1")));
        }
        assert!(s.enable_paging());
        let paged = s.paged_band_count();
        assert_eq!(paged, 4, "200 rows over 64-row bands");
        s.shift_rows(1, 1);
        let mut values: Vec<(CellRef, Value)> = (0..200u32).map(|r| (c(r, 0), Value::Number(f64::from(r)))).collect();
        s.set_values(&mut values);
        assert_eq!(s.paged_band_count(), 0, "every touched band is live after the write");
        for r in 0..200u32 {
            let want = if r == 1 { None } else { Some(Value::Number(f64::from(r))) };
            assert_eq!(s.get(c(r, 0)).map(|x| x.value), want, "row {r}");
        }
        assert_eq!(s.get(c(1, 0)), None, "the gap row holds nothing");
    }

    /// The mapped parallel write copies bands in workers exactly as the identity one does: the
    /// snapshot keeps its values and every value lands.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn parallel_mapped_value_writes_keep_the_snapshot() {
        let mut s = CellStore::new();
        for r in 0..20_000u32 {
            s.set(c(r, 9), Cell::formula(Formula::new("=$A$1*2")));
        }
        s.shift_rows(0, 1);
        s.shift_cols(1, 1);
        let snapshot = s.clone();
        // Logical row 0 is the inserted gap; the formula cells live on rows 1..=20,000.
        let mut values: Vec<(CellRef, Value)> = vec![(c(0, 10), Value::Number(-1.0))];
        values.extend((0..20_000u32).map(|r| (c(r + 1, 10), Value::Number(f64::from(r)))));
        let mut par = s.clone();
        assert!(par.set_values_mapped_parallel(&values, 4), "the plan fits the run budget");
        s.set_values(&mut values);
        for r in 0..20_000u32 {
            let at = c(r + 1, 10);
            assert_eq!(s.get(at).map(|x| x.value), Some(Value::Number(f64::from(r))), "row {}", r + 1);
            assert_eq!(par.get(at).map(|x| x.value), Some(Value::Number(f64::from(r))), "parallel row {}", r + 1);
            assert_eq!(snapshot.get(at).map(|x| x.value), Some(Value::Empty));
        }
        assert_eq!(s.get(c(0, 10)), None, "the gap row was not written");
        assert_eq!(s.len(), 20_000);
        assert_eq!(par, s);
    }

    /// A formula that recalculates to nothing keeps its cell: the value becomes `EMPTY`, the slot,
    /// the style and the formula stay, so `len`, `used_range` and `iter` do not change.
    #[test]
    fn set_values_keeps_a_cell_whose_result_is_empty() {
        let mut s = CellStore::new();
        s.set(c(3, 2), Cell { value: Value::Number(1.0), formula: Some(Arc::new(Formula::new("=A1"))), style: StyleId(4) });
        s.shift_rows(1, 1);
        let mut values = vec![(c(4, 2), Value::Empty)];
        s.set_values(&mut values);
        let got = s.get(c(4, 2)).expect("the cell keeps its slot");
        assert_eq!(got.value, Value::Empty);
        assert!(got.formula.is_some(), "the formula stays");
        assert_eq!(got.style, StyleId(4), "so does the style");
        assert_eq!(s.len(), 1);
        assert!(s.contains(c(4, 2)));
    }

    /// A band copied for a value edit shares the formulas of its rows; changing one formula in
    /// the copy must not change the snapshot's.
    #[test]
    fn formula_extras_are_shared_until_changed() {
        let mut s = CellStore::new();
        s.set(c(0, 0), Cell::formula(Formula::new("=1+1")));
        s.set(c(0, 1), Cell::formula(Formula::new("=2+2")));
        let mut copy = s.clone();
        copy.set_value(c(0, 0), &Value::Number(7.0));
        copy.set(c(0, 1), Cell::formula(Formula::new("=5+5")));
        let text = |store: &CellStore, at| store.get(at).and_then(|x| x.formula).and_then(|f| f.text_at(at));
        assert_eq!(text(&s, c(0, 1)).as_deref(), Some("2+2"));
        assert_eq!(text(&copy, c(0, 1)).as_deref(), Some("5+5"));
        assert_eq!(text(&s, c(0, 0)).as_deref(), Some("1+1"));
    }

    #[test]
    fn diff_visits_changed_bands() {
        let mut s = CellStore::new();
        for r in 0..1000 {
            s.set(c(r, 0), Cell::value((r as f64).into()));
        }
        let snap = s.clone();
        assert!(s.diff(&snap).is_empty());
        s.set(c(5, 0), Cell::value(99.0.into()));
        s.set(c(700, 3), Cell::value("new".into()));
        s.remove(c(900, 0));
        assert_eq!(snap.diff(&s), [c(5, 0), c(700, 3), c(900, 0)]);
        // `diff_logical` walks the smaller store's cells: it names the edit and the removal. The
        // equal-sized addition at (700, 3) is not named again — the removal already pins a change
        // — but a diff is empty only when the logical content is equal, which is what the fuzz
        // needs.
        assert_eq!(snap.diff_logical(&s), [c(5, 0), c(900, 0)]);
        assert!(!snap.diff_logical(&s).is_empty());
    }

    /// Two stores holding the same logical cells under different maps: `diff` reports positions
    /// from the physical walk (some holding nothing in either store), which is why every engine
    /// caller re-reads the content; `diff_logical` reports no difference at all.
    #[test]
    fn diff_reports_only_stored_positions_when_maps_differ() {
        let mut s = CellStore::new();
        for r in 0..5u32 {
            s.set(c(r, 0), Cell::value(Value::Number(f64::from(r))));
        }
        s.shift_rows(0, 5);
        for r in 0..5u32 {
            s.set(c(r, 0), Cell::value(Value::Number(f64::from(r + 100))));
        }
        let mut t = CellStore::new();
        for r in 0..5u32 {
            t.set(c(r, 0), Cell::value(Value::Number(f64::from(r + 100))));
        }
        for r in 5..10u32 {
            t.set(c(r, 0), Cell::value(Value::Number(f64::from(r - 5))));
        }
        assert_eq!(s, t, "same logical cells, different maps");
        assert!(s.diff_logical(&t).is_empty());
        let reported = s.diff(&t);
        // The shifted rows report their old logical homes, then the band the gap rows were
        // written to; A11..A15 hold nothing in either store.
        let want: Vec<CellRef> = (5..15u32).chain(0..5u32).map(|r| c(r, 0)).collect();
        assert_eq!(reported, want);
        assert!(s.get(c(12, 0)).is_none() && t.get(c(12, 0)).is_none(), "a reported position may hold nothing at all");
        for at in reported {
            match (s.get(at), t.get(at)) {
                (Some(a), Some(b)) => assert!(same_cell(&a, &b), "{} is a real difference", at.a1()),
                (a, b) => assert_eq!(a, b, "{} holds content on one side only", at.a1()),
            }
        }
    }

    /// A pure remap is not a content difference: the bands are shared, so `diff` reports nothing
    /// while the logical content (and therefore `eq`) says otherwise.
    #[test]
    fn diff_does_not_see_a_remap() {
        let mut s = CellStore::new();
        for r in 0..5u32 {
            s.set(c(r, 0), Cell::value(Value::Number(f64::from(r))));
        }
        let mut remap = s.clone();
        remap.shift_rows(0, 1);
        assert_ne!(s, remap, "the logical content moved");
        assert!(s.diff(&remap).is_empty(), "the bands are the same, and `diff` is physical");
        assert!(!s.diff_logical(&remap).is_empty());
        // Every cell that moved is reported at the position the smaller store holds it.
        assert_eq!(s.diff_logical(&remap), (0..5u32).map(|r| c(r, 0)).collect::<Vec<_>>());
    }

    /// `diff_logical` is empty exactly when the logical content is equal (`eq`), and for a one-cell
    /// edit under two maps it names that logical cell.
    #[test]
    fn diff_logical_matches_eq() {
        let mut s = CellStore::new();
        for r in 0..5u32 {
            s.set(c(r, 0), Cell::value(Value::Number(f64::from(r))));
        }
        s.shift_rows(0, 5);
        for r in 0..5u32 {
            s.set(c(r, 0), Cell::value(Value::Number(f64::from(r + 100))));
        }
        let mut t = CellStore::new();
        for r in 0..5u32 {
            t.set(c(r, 0), Cell::value(Value::Number(f64::from(r + 100))));
        }
        for r in 5..10u32 {
            t.set(c(r, 0), Cell::value(Value::Number(f64::from(r - 5))));
        }
        for (a, b) in [(&s, &t), (&t, &s)] {
            assert_eq!(a == b, a.diff_logical(b).is_empty(), "equal content, different maps");
        }
        t.set_value(c(5, 0), &Value::Number(-1.0));
        assert_ne!(s, t);
        assert_eq!(s.diff_logical(&t), [c(5, 0)], "only the edited logical cell");
        assert_eq!(t.diff_logical(&s), [c(5, 0)]);
    }

    /// A cell only one side holds is reported, in either direction and whatever the maps say.
    #[test]
    fn diff_logical_reports_a_cell_only_in_one_store() {
        let mut s = CellStore::new();
        for r in 0..10u32 {
            s.set(c(r, 0), Cell::value(Value::Number(f64::from(r))));
        }
        s.shift_rows(0, 1);
        let mut t = s.clone();
        assert!(t.remove(c(5, 0)).is_some());
        assert_eq!(t.diff_logical(&s), [c(5, 0)]);
        assert_eq!(s.diff_logical(&t), [c(5, 0)]);
        assert_eq!(t.diff(&s), [c(5, 0)]);
    }

    #[test]
    fn range_iteration_order() {
        let mut s = CellStore::new();
        s.set(c(70, 2), Cell::value(3.0.into()));
        s.set(c(1, 1), Cell::value(1.0.into()));
        s.set(c(1, 3), Cell::value(2.0.into()));
        s.set(c(500, 0), Cell::value(4.0.into()));
        let v: Vec<f64> = s.iter_range(RangeRef::parse("A1:D100").unwrap()).map(|(_, x)| x.value.as_f64().unwrap()).collect();
        assert_eq!(v, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn a_band_that_overflows_its_number_table_still_reads_back() {
        // 64 rows of 2,000 distinct numbers is 128,000 values in one band — twice what the
        // two-byte payload can index, so the tail of it has to go through the wide table.
        let value = |row: u32, col: u32| row as f64 * 2_000.0 + col as f64 + 0.5;
        let mut s = CellStore::new();
        for row in 0..64u32 {
            let mut cells: Vec<(u32, Cell)> = (0..2_000u32).map(|col| (col, Cell::value(value(row, col).into()))).collect();
            s.set_row(row, &mut cells);
        }
        assert_eq!(s.len(), 128_000);
        // The band really did overflow: some cells index the number table, the rest are wide.
        let band = s.bands.get(&0).expect("band");
        let kinds: Vec<u8> = band.rows.iter().flat_map(|row| (0..row.cells as usize).map(|i| row.kind_at(band, i))).collect();
        assert_eq!(kinds.iter().filter(|k| **k == NUMBER).count(), 65_535, "cells the table could hold");
        assert_eq!(kinds.iter().filter(|k| **k == NUMBER_WIDE).count(), 128_000 - 65_535, "cells that fell back");
        for row in 0..64u32 {
            for col in 0..2_000u32 {
                let got = s.get(c(row, col)).map(|cell| cell.value);
                assert_eq!(got, Some(Value::Number(value(row, col))), "row {row} col {col}");
            }
        }
        // An edit into the overflowed rows keeps working: overwrite, insert a new column, remove.
        s.set(c(63, 1_999), Cell::value((-1.5).into()));
        assert_eq!(s.get(c(63, 1_999)).map(|cell| cell.value), Some(Value::Number(-1.5)));
        s.set(c(63, 2_500), Cell::value(7.25.into()));
        assert_eq!(s.get(c(63, 2_500)).map(|cell| cell.value), Some(Value::Number(7.25)));
        assert_eq!(s.remove(c(0, 1_500)).map(|cell| cell.value), Some(Value::Number(value(0, 1_500))));
        assert_eq!(s.len(), 128_000);
        // Text in the same band, past the numbers, is still found.
        s.set(c(0, 3_000), Cell::value("tail".into()));
        assert_eq!(s.get(c(0, 3_000)).map(|cell| cell.value), Some(Value::Text("tail".into())));
    }

    #[test]
    fn a_row_insert_changes_the_map_not_the_bands() {
        let mut s = CellStore::new();
        for r in 0..8u32 {
            s.set(c(r, 0), Cell::value(Value::Number(f64::from(r))));
            s.set(c(r, 1), Cell::value(Value::Text(Arc::from(format!("r{r}")))));
        }
        s.set(c(2, 2), Cell { value: Value::Number(2.0), formula: Some(Arc::new(Formula::new("=A3"))), style: StyleId(1) });
        let bands = s.band_count();
        let len = s.len();
        s.shift_rows(1, 1);
        assert_eq!(s.band_count(), bands, "an insert does not move or split a band");
        assert_eq!(s.len(), len);
        assert_eq!(s.get(c(0, 0)).map(|x| x.value), Some(Value::Number(0.0)));
        assert_eq!(s.get(c(1, 0)), None, "the inserted row is empty");
        assert_eq!(s.get(c(2, 0)).map(|x| x.value), Some(Value::Number(1.0)));
        assert_eq!(s.get(c(3, 2)).and_then(|x| x.formula).and_then(|f| f.text_at(c(3, 2))), Some("A3".to_string()));
        let mut rows: Vec<u32> = s.iter().map(|(at, _)| at.row).collect();
        rows.dedup();
        assert_eq!(rows, [0, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(s.used_range().map(|r| r.a1()), Some("A1:C9".to_string()));
        // Writing the inserted row allocates a fresh physical row without disturbing the rest.
        s.set(c(1, 0), Cell::value(Value::Number(-1.0)));
        s.set_row(1, &mut vec![(1, Cell::value(Value::Text(Arc::from("new"))))]);
        assert_eq!(s.get(c(1, 0)).map(|x| x.value), Some(Value::Number(-1.0)));
        assert_eq!(s.get(c(1, 1)).map(|x| x.value), Some(Value::Text(Arc::from("new"))));
        assert_eq!(s.get(c(2, 0)).map(|x| x.value), Some(Value::Number(1.0)));
        assert_eq!(s.iter_range(RangeRef::parse("A1:C3").expect("range")).count(), 6);
        // A delete drops the inserted row and pulls the later rows up, still without moving bands.
        s.take_range(RangeRef::rows(1, 1));
        s.shift_rows(2, -1);
        assert_eq!(s.get(c(1, 0)).map(|x| x.value), Some(Value::Number(1.0)));
        assert_eq!(s.get(c(2, 0)).map(|x| x.value), Some(Value::Number(2.0)));
        assert_eq!(s.get(c(1, 1)).map(|x| x.value), Some(Value::Text(Arc::from("r1"))));
        assert!(s.get(c(2, 2)).is_some());
        // A formula at a logical position is yielded there after the shifts.
        let formulas: Vec<CellRef> = s.formulas().map(|(at, _, _)| at).collect();
        assert_eq!(formulas, [c(2, 2)]);
        assert_eq!(s.formula_cells(RangeRef::parse("A1:C9").expect("range")).collect::<Vec<_>>(), [c(2, 2)]);
        // The serialized shape is logical: a reopened store has an identity map but the same cells.
        let bytes = serde_json::to_vec(&s).expect("serialize");
        let back: CellStore = serde_json::from_slice(&bytes).expect("deserialize");
        assert_eq!(s, back, "the row map is not part of the saved cells");
    }

    #[test]
    fn a_column_delete_then_value_write_keeps_the_formula_cell() {
        let mut s = CellStore::new();
        s.set(c(15, 8), Cell { value: Value::Empty, formula: Some(Arc::new(Formula::new("=1/0"))), style: StyleId(0) });
        for col in 10..=13 {
            s.set(c(15, col), Cell::value(Value::Number(f64::from(col))));
        }
        s.take_range(RangeRef::cols(10, 13));
        s.shift_cols(14, -4);
        assert_eq!(s.get(c(15, 8)).and_then(|x| x.formula).and_then(|f| f.text_at(c(15, 8))), Some("1/0".to_string()));
        let mut values = vec![(c(15, 8), Value::Error(cella_core::CellError::Div0))];
        s.set_values(&mut values);
        assert_eq!(s.get(c(15, 8)).map(|x| x.value), Some(Value::Error(cella_core::CellError::Div0)));
    }

    #[test]
    fn a_column_map_value_write_reaches_the_formula_cell() {
        let mut s = CellStore::new();
        s.set(c(15, 8), Cell { value: Value::Empty, formula: Some(Arc::new(Formula::new("=1/0"))), style: StyleId(0) });
        s.set(c(15, 10), Cell::value(Value::Number(1.0)));
        s.shift_cols(10, 4);
        assert_eq!(s.get(c(15, 8)).and_then(|x| x.formula).and_then(|f| f.text_at(c(15, 8))), Some("1/0".to_string()));
        assert!(s.set_value(c(15, 8), &Value::Error(cella_core::CellError::Div0)), "the mapped formula cell takes a value");
        assert_eq!(s.get(c(15, 8)).map(|x| x.value), Some(Value::Error(cella_core::CellError::Div0)));
    }

    /// The owned-cell gate answers the same on a mapped and a paged store, and a band whose page
    /// says it holds no formula or array is skipped without a decode: a whole-column `SUM` pays
    /// one band lookup per band instead of one per row.
    #[test]
    fn the_owned_cell_gate_reads_a_band_once() {
        let mut s = CellStore::new();
        for r in 0..300u32 {
            s.set(c(r, 0), Cell::value(Value::Number(f64::from(r))));
        }
        s.set(c(130, 1), Cell { value: Value::Number(3.0), formula: Some(Arc::new(Formula::new("=A1+1"))), style: StyleId(0) });
        let narrow = RangeRef::parse("A1:A300").expect("range");
        let wide = RangeRef::parse("A1:B300").expect("range");
        assert!(!s.has_owned_cells(narrow));
        assert!(s.has_owned_cells(wide));
        // Through a row map the band cache still has to find the formula, at its new logical row.
        s.shift_rows(1, 1);
        assert!(!s.has_owned_cells(narrow), "the formula moved out of column A");
        assert!(s.has_owned_cells(wide), "and is still inside the block");
        assert_eq!(s.formula_cells(wide).collect::<Vec<_>>(), [c(131, 1)]);
        // Paging keeps the flag on the pages that hold a formula; the ones without are skipped
        // without being decoded, so the gate stays a comparison per band.
        assert!(s.enable_paging());
        assert!(s.paged_band_count() > 0);
        let decodes = || s.page_file.as_ref().map_or(0, |file| file.decodes());
        let before = decodes();
        assert!(!s.has_owned_cells(RangeRef::parse("A1:A100").expect("range")), "the first bands hold no formula");
        assert_eq!(decodes(), before, "a page without a formula or array is not decoded to answer the gate");
        assert!(!s.has_owned_cells(narrow));
        assert!(s.has_owned_cells(wide));
        assert_eq!(s.formula_cells(wide).collect::<Vec<_>>(), [c(131, 1)]);
    }

    #[test]
    fn a_column_insert_changes_the_map_not_the_bands() {
        let mut s = CellStore::new();
        for col in 0..8u32 {
            s.set(c(0, col), Cell::value(Value::Number(f64::from(col))));
            s.set(c(1, col), Cell::value(Value::Text(Arc::from(format!("c{col}")))));
        }
        s.set(c(2, 2), Cell { value: Value::Number(2.0), formula: Some(Arc::new(Formula::new("=A1"))), style: StyleId(1) });
        let bands = s.band_count();
        let len = s.len();
        s.shift_cols(1, 1);
        assert_eq!(s.band_count(), bands, "a column insert does not move or split a band");
        assert_eq!(s.len(), len);
        assert_eq!(s.get(c(0, 0)).map(|x| x.value), Some(Value::Number(0.0)));
        assert_eq!(s.get(c(0, 1)), None, "the inserted column is empty");
        assert_eq!(s.get(c(0, 2)).map(|x| x.value), Some(Value::Number(1.0)));
        assert_eq!(s.get(c(1, 3)).map(|x| x.value), Some(Value::Text(Arc::from("c2"))));
        assert_eq!(s.used_range().map(|r| r.a1()), Some("A1:I3".to_string()));
        // Writing into the gap allocates a physical column; the other cells stay put.
        s.set(c(0, 1), Cell::value(Value::Number(-1.0)));
        assert_eq!(s.get(c(0, 1)).map(|x| x.value), Some(Value::Number(-1.0)));
        assert_eq!(s.get(c(0, 2)).map(|x| x.value), Some(Value::Number(1.0)));
        let mut cols: Vec<u32> = s.iter().map(|(at, _)| at.col).collect();
        cols.sort_unstable();
        cols.dedup();
        assert_eq!(cols, [0, 1, 2, 3, 4, 5, 6, 7, 8]);
        // Delete pulls the later columns left and drops the inserted one.
        s.take_range(RangeRef::cols(1, 1));
        s.shift_cols(2, -1);
        assert_eq!(s.get(c(0, 1)).map(|x| x.value), Some(Value::Number(1.0)));
        assert_eq!(s.get(c(0, 2)).map(|x| x.value), Some(Value::Number(2.0)));
        assert_eq!(s.get(c(1, 1)).map(|x| x.value), Some(Value::Text(Arc::from("c1"))));
        let formulas: Vec<CellRef> = s.formulas().map(|(at, _, _)| at).collect();
        assert_eq!(formulas, [c(2, 2)]);
        assert_eq!(s.formula_cells(RangeRef::parse("A1:I3").expect("range")).collect::<Vec<_>>(), [c(2, 2)]);
        let bytes = serde_json::to_vec(&s).expect("serialize");
        let back: CellStore = serde_json::from_slice(&bytes).expect("deserialize");
        assert_eq!(s, back, "the column map is not part of the saved cells");
    }

    #[test]
    fn shifting() {
        let mut s = CellStore::new();
        s.set(c(2, 0), Cell::value(1.0.into()));
        s.set(c(5, 0), Cell::value(2.0.into()));
        s.shift_rows(3, 2);
        assert!(s.get(c(7, 0)).is_some());
        assert!(s.get(c(2, 0)).is_some());
        s.shift_cols(0, 1);
        assert!(s.get(c(7, 1)).is_some());
    }

    /// A reader cuts its work at row boundaries, so two chunks can land in the same band: the
    /// second batch is rebuilt against the band the store already holds.
    #[test]
    fn a_band_split_across_two_batches_keeps_both_halves() {
        let mut s = CellStore::new();
        for (from, to) in [(0u32, 40u32), (40, 80)] {
            let mut batch = BandBatch::new();
            for row in from..to {
                let mut cells: Vec<(u32, Cell)> = (0..10u32).map(|c| (c, Cell::value(Value::Number((row * 100 + c) as f64)))).collect();
                cells.push((20, Cell::value(Value::Text(Arc::from("shared")))));
                batch.set_row(row, &mut cells);
            }
            s.insert_batch(batch);
        }
        assert_eq!(s.len(), 80 * 11);
        assert_eq!(s.used_range().map(|r| r.a1()), Some("A1:U80".to_string()));
        for row in 0..80u32 {
            for col in 0..10u32 {
                assert_eq!(s.get(c(row, col)).map(|x| x.value), Some(Value::Number((row * 100 + col) as f64)), "{row}:{col}");
            }
            assert_eq!(s.get(c(row, 20)).map(|x| x.value), Some(Value::Text(Arc::from("shared"))), "{row}:U");
        }
        check_stripes(&s);
    }

    /// The stripes are one vector each, so a bug shows up as a row whose slice points into the
    /// middle of another's. Every band must account for every entry, exactly once.
    fn check_stripes(s: &CellStore) {
        for (bk, band) in &s.bands {
            let ranges = |name: &str, len: usize, f: &dyn Fn(&BandRow) -> (u32, u32)| {
                let mut v: Vec<(usize, usize)> = band
                    .rows
                    .iter()
                    .map(|r| {
                        let (at, n) = f(r);
                        (at as usize, n as usize)
                    })
                    .collect();
                v.sort_unstable();
                let mut end = 0;
                for (at, n) in &v {
                    assert_eq!(*at, end, "band {bk} {name}: a stripe has a hole or an overlap before {at}");
                    end += n;
                }
                assert_eq!(end, len, "band {bk} {name}: the stripe holds {len} entries, its rows account for {end}");
            };
            ranges("bits", band.bits.len(), &|r| (r.bits_at, r.words));
            ranges("payloads", band.payloads.len(), &|r| (r.cells_at, r.cells));
            ranges("kinds", band.kinds.len(), &|r| (r.kinds_at, r.kind_runs));
            ranges("styles", band.styles.len(), &|r| (r.styles_at, r.style_runs));
            assert!(band.rows.windows(2).all(|w| w[0].key < w[1].key), "band {bk}: rows out of order");
            assert_eq!(band.row_bits, Band::rows_bits(&band.rows), "band {bk}: the row index disagrees with its rows");
            for key in 0..BAND {
                assert_eq!(
                    band.row_index(key),
                    band.rows.binary_search_by_key(&key, |r| r.key).ok(),
                    "band {bk} row {key}: the row index and the sorted rows disagree"
                );
            }
            for row in &band.rows {
                assert_eq!(runs_len(row.kind_runs(band)), row.cells as usize, "band {bk} row {}: kinds", row.key);
                assert_eq!(runs_len(row.style_runs(band)), row.cells as usize, "band {bk} row {}: styles", row.key);
                let bits: usize = row.bits(band).iter().map(|w| w.count_ones() as usize).sum();
                assert_eq!(bits, row.cells as usize, "band {bk} row {}: bitmap", row.key);
                assert!(row.kind_runs(band).windows(2).all(|w| w[0].0 != w[1].0), "band {bk} row {}: same-valued runs", row.key);
            }
        }
    }

    /// A store built and edited cell by cell must agree with a plain map, and its stripes must stay
    /// one contiguous run of slices — the layout is the thing this file is about, so the layout is
    /// what the test checks, not only what it reads back.
    #[test]
    fn stripes_hold_exactly_the_cells_after_an_edit_soup() {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = |n: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % n.max(1)) as u32
        };
        let mut s = CellStore::new();
        let mut model: BTreeMap<(u32, u32), Value> = BTreeMap::new();
        for step in 0..3_000 {
            // Rows either side of two band boundaries; columns either side of a bitmap word and of
            // the 64-column alignment, so the bitmap has to grow left and right.
            let row = [0, 1, 62, 63, 64, 65, 127, 128][next(8) as usize % 8];
            let col = [0, 1, 63, 64, 65, 127, 128, 200][next(8) as usize % 8];
            let value = Value::Number((next(5) as i64 - 1) as f64);
            match next(4) {
                0 => {
                    s.set(c(row, col), Cell::value(value.clone()));
                    model.insert((row, col), value);
                }
                1 => {
                    let gone = s.remove(c(row, col));
                    assert_eq!(gone.map(|x| x.value), model.remove(&(row, col)), "{row}:{col}");
                }
                2 => {
                    // A whole row at once, the way a file arrives.
                    let mut cells: Vec<(u32, Cell)> = (0..4).map(|i| (col + i * 64, Cell::value(Value::Number((step * 7 + i) as f64)))).collect();
                    s.set_row(row, &mut cells);
                    for i in 0..4 {
                        model.insert((row, col + i * 64), Value::Number((step * 7 + i) as f64));
                    }
                }
                _ => {
                    let text = Value::Text(Arc::from(format!("t{step}").as_str()));
                    s.set(c(row, col), Cell::value(text.clone()));
                    model.insert((row, col), text);
                }
            }
            check_stripes(&s);
        }
        let got: Vec<((u32, u32), Value)> = s.iter().map(|(at, cell)| ((at.row, at.col), cell.value)).collect();
        let want: Vec<((u32, u32), Value)> = model.iter().map(|(at, v)| (*at, v.clone())).collect();
        assert_eq!(got, want);
        assert_eq!(s.len(), model.len());
        assert_eq!(s.iter().count(), model.len());
        for ((row, col), v) in &model {
            assert_eq!(s.get(c(*row, *col)).map(|cell| cell.value), Some(v.clone()), "{row}:{col}");
        }
    }

    #[test]
    fn compact_reclaims_rows_with_no_logical_inverse() {
        let mut s = CellStore::new();
        for row in 0..10u32 {
            s.set(c(row, 0), Cell::value(Value::Number(f64::from(row))));
        }
        s.set(c(2, 1), Cell { value: Value::Text(Arc::from("kept")), formula: Some(Arc::new(Formula::new("=A3"))), style: StyleId(4) });
        let mut dynamic = Formula::new("=A1:A2*2");
        dynamic.array = Some(Box::new(RangeRef::parse("C4:D5").expect("range")));
        dynamic.dynamic = true;
        let array = Value::Array(Arc::new(cella_core::Array::row(vec![Value::Number(1.0), Value::Number(2.0)])));
        s.set(c(3, 2), Cell { value: array, formula: Some(Arc::new(dynamic)), style: StyleId(9) });
        let before = s.len();
        assert_eq!(before, 12);
        let snapshot = s.clone();
        // A direct model shift: unlike the engine's delete command, it does not clear the cells
        // first, so the row that falls off the map stays in its band.
        s.shift_rows(1, -1);
        let visible: Vec<(CellRef, Cell)> = s.iter().collect();
        let used = s.used_range();
        assert_eq!(visible.len(), 11);
        assert_eq!(s.len(), before, "the orphan is still counted before compaction");
        s.compact();
        assert_eq!(s.iter().collect::<Vec<_>>(), visible, "mapped cells keep their content");
        assert_eq!(s.used_range(), used, "the used range is unchanged");
        assert_eq!(s.len(), visible.len(), "len now counts exactly the reachable cells");
        assert_eq!(snapshot.len(), before, "the undo snapshot keeps its cells");
        assert_eq!(snapshot.iter().count(), before);
        assert!(s.len() < before);
        assert_eq!(s.get(c(9, 0)), None, "the clipped row cannot be read");
        let (len, bands, segments) = (s.len(), s.band_count(), s.row_map.segments.len());
        s.compact();
        assert_eq!((s.len(), s.band_count(), s.row_map.segments.len()), (len, bands, segments), "a second pass is a no-op");
        assert_eq!(s.iter().collect::<Vec<_>>(), visible);
    }

    #[test]
    fn compact_reclaims_columns_with_no_logical_inverse() {
        let mut s = CellStore::new();
        for col in 0..10u32 {
            s.set(c(0, col), Cell::value(Value::Number(f64::from(col))));
        }
        let before = s.len();
        s.shift_cols(1, -1);
        let visible: Vec<(CellRef, Cell)> = s.iter().collect();
        let used = s.used_range();
        assert_eq!(visible.len(), 9);
        assert_eq!(s.len(), before, "the orphan column is still counted before compaction");
        s.compact();
        assert_eq!(s.iter().collect::<Vec<_>>(), visible, "mapped cells keep their content");
        assert_eq!(s.used_range(), used, "the used range is unchanged");
        assert_eq!(s.len(), visible.len(), "len now counts exactly the reachable cells");
        assert!(s.len() < before);
        assert_eq!(s.get(c(0, 9)), None, "the clipped column cannot be read");
        assert_eq!(s.col_map.segments.len(), 1);
        assert!(
            s.col_map.free.iter().all(|physical| *physical < s.col_map.next_physical && s.col_map.inverse(*physical).is_none()),
            "every free column is reusable and none is still mapped"
        );
        let (len, bands, free, segments) = (s.len(), s.band_count(), s.col_map.free.clone(), s.col_map.segments.len());
        s.compact();
        assert_eq!(
            (s.len(), s.band_count(), s.col_map.free.clone(), s.col_map.segments.len()),
            (len, bands, free, segments),
            "a second pass is a no-op"
        );
        assert_eq!(s.iter().collect::<Vec<_>>(), visible);
    }

    #[test]
    fn compact_bounds_the_column_free_list() {
        let mut s = CellStore::new();
        for col in 0..8u32 {
            s.set(c(0, col), Cell::value(Value::Number(f64::from(col))));
        }
        s.shift_cols(1, -1);
        assert_eq!(s.col_map.free, [0]);
        // A stale key that is mapped, duplicates, and one past every physical column handed out.
        s.col_map.free.extend([0, 0, 4, 4, 9999]);
        s.compact();
        assert_eq!(s.col_map.free, [0], "only an in-range, unmapped key survives");
        assert_eq!(s.len(), 7);
        s.compact();
        assert_eq!(s.col_map.free, [0]);
    }

    #[test]
    fn compact_reclaims_a_fragmented_store_and_restores_a_contiguous_map() {
        let mut s = CellStore::new();
        for col in 0..40u32 {
            s.set(c(0, col), Cell::value(Value::Number(f64::from(col))));
        }
        let mut max_segments = s.col_map.segments.len();
        for k in [8u32, 16, 24, 32] {
            s.shift_cols(k, 1);
            max_segments = max_segments.max(s.col_map.segments.len());
        }
        assert!(max_segments > 1, "repeated inserts fragment the map into several runs");
        // A shift that is not paired with a delete leaves the cells of the dropped columns behind.
        s.shift_cols(2, -1);
        let visible: Vec<(CellRef, Cell)> = s.iter().collect();
        let before_len = s.len();
        let before_segments = s.col_map.segments.len();
        assert!(before_len > visible.len(), "the dropped column is still stored");
        s.compact();
        assert_eq!(s.iter().collect::<Vec<_>>(), visible);
        assert!(s.col_map.segments.len() <= before_segments, "compaction never adds a run");
        assert_eq!(s.len(), visible.len(), "every unreachable cell is gone");
        assert!(s.col_map.free.windows(2).all(|w| w[0] < w[1]), "the free list is deduplicated");
        assert!(
            s.col_map.free.iter().all(|physical| *physical < s.col_map.next_physical && s.col_map.inverse(*physical).is_none()),
            "the free list is bounded to reusable, unmapped columns"
        );
        let (len, bands, free, segments) = (s.len(), s.band_count(), s.col_map.free.clone(), s.col_map.segments.len());
        s.compact();
        assert_eq!(
            (s.len(), s.band_count(), s.col_map.free.clone(), s.col_map.segments.len()),
            (len, bands, free, segments),
            "a second pass is a no-op"
        );

        // A heal in the order the inserts were made leaves one contiguous run. The runs are
        // already merged by the shift; compaction is what returns that run to the fast path.
        let mut t = CellStore::new();
        for col in 0..40u32 {
            t.set(c(0, col), Cell::value(Value::Number(f64::from(col))));
        }
        for k in [8u32, 16, 24, 32] {
            t.shift_cols(k, 1);
        }
        let fragmented = t.col_map.segments.len();
        for k in [32u32, 24, 16, 8] {
            t.shift_cols(k + 1, -1);
        }
        assert_eq!(t.col_map.segments.len(), 1, "a matched delete heals the insert (had {fragmented} runs)");
        assert!(!t.col_map.is_identity());
        let visible: Vec<(CellRef, Cell)> = t.iter().collect();
        let before_segments = t.col_map.segments.len();
        t.compact();
        assert!(t.col_map.is_identity(), "a fully contiguous map returns to the fast path");
        assert_eq!(t.col_map.segments.len(), 0);
        assert!(t.col_map.segments.len() < before_segments, "the single run collapsed");
        let after: Vec<(CellRef, Cell)> = t.iter().collect();
        assert_eq!(after, visible);
        t.compact();
        assert!(t.col_map.is_identity());
        assert_eq!(t.iter().collect::<Vec<_>>(), visible);
    }

    #[test]
    fn compact_keeps_the_logical_cells_across_serialization() {
        let mut s = CellStore::new();
        for row in 0..5u32 {
            s.set(c(row, 0), Cell { value: Value::Number(f64::from(row)), formula: None, style: StyleId(row % 3) });
            s.set(c(row, 1), Cell::value(Value::Text(Arc::from(format!("r{row}").as_str()))));
        }
        s.set(c(2, 2), Cell { value: Value::Number(9.0), formula: Some(Arc::new(Formula::new("=A1+1"))), style: StyleId(7) });
        // Direct shifts leave the first physical row and column behind.
        s.shift_rows(1, -1);
        s.shift_cols(1, -1);
        s.compact();
        let visible: Vec<(CellRef, Cell)> = s.iter().collect();
        let positions: Vec<CellRef> = visible.iter().map(|(at, _)| *at).collect();
        let len = s.len();
        assert_eq!(len, visible.len());
        let bytes = serde_json::to_vec(&s).expect("serialize");
        let back: CellStore = serde_json::from_slice(&bytes).expect("deserialize");
        assert_eq!(back.len(), len);
        assert_eq!(back.iter().map(|(at, _)| at).collect::<Vec<_>>(), positions);
        assert_eq!(back, s, "the saved cells are the logical cells");
    }

    /// A paged store whose map lost a row: the page holding it is decoded, its orphan dropped and
    /// the band written back at the same key, while every mapped cell reads as before.
    #[test]
    fn compact_reclaims_paged_rows() {
        let mut s = CellStore::new();
        for row in 0..200u32 {
            s.set(c(row, 0), Cell::value(Value::Number(f64::from(row))));
        }
        s.set(c(3, 1), Cell { value: Value::Number(99.0), formula: None, style: StyleId(2) });
        assert!(s.enable_paging());
        assert_eq!(s.paged_band_count(), 4);
        let pages: Vec<u32> = s.pages.keys().copied().collect();
        // A direct model shift: unlike the engine's delete command, it does not clear the cells
        // first, so physical row 0 stays in its page with no logical row to reach it.
        s.shift_rows(1, -1);
        let visible: Vec<(CellRef, Cell)> = s.iter().collect();
        let used = s.used_range();
        assert_eq!(visible.len(), 200);
        assert_eq!(s.len(), 201, "the orphan is still counted before compaction");
        let out = s.compact();
        assert_eq!(out.cells, 1);
        assert_eq!(out.pages, 1, "the page holding the dropped row is rewritten");
        assert_eq!(out.bands, 0);
        assert!(out.changed);
        assert_eq!(s.iter().collect::<Vec<_>>(), visible, "mapped cells keep their content");
        assert_eq!(s.used_range(), used, "the used range is unchanged");
        assert_eq!(s.len(), visible.len(), "len now counts exactly the reachable cells");
        assert_eq!(s.pages.keys().copied().collect::<Vec<_>>(), pages, "the page keeps its band key");
        for (at, cell) in &visible {
            assert_eq!(s.get(*at).as_ref(), Some(cell), "{}", at.a1());
        }
        assert_eq!(visible.last().map(|(at, _)| *at), Some(c(198, 0)));
    }

    /// The rewrite installs a new `Arc<Page>`; the snapshot keeps the old one, its offsets and the
    /// cell compaction dropped.
    #[test]
    fn compact_keeps_the_old_page_for_a_snapshot() {
        let mut s = CellStore::new();
        for row in 0..200u32 {
            s.set(c(row, 0), Cell::value(Value::Number(f64::from(row))));
        }
        assert!(s.enable_paging());
        let snapshot = s.clone();
        let old_len = s.pages.get(&0).map(|page| page.page_len()).unwrap_or(0);
        assert!(old_len > 0, "page 0 holds the first band");
        s.shift_rows(1, -1);
        assert_eq!(s.compact().pages, 1);
        let new_page = s.pages.get(&0).expect("the rewritten page stays at its key");
        let old_page = snapshot.pages.get(&0).expect("the snapshot keeps its page");
        assert!(!Arc::ptr_eq(new_page, old_page), "the rewrite installs a new page");
        assert_eq!(snapshot.get(c(0, 0)).map(|x| x.value), Some(Value::Number(0.0)), "the snapshot still reads the dropped row");
        assert_eq!(s.get(c(0, 0)).map(|x| x.value), Some(Value::Number(1.0)), "the live store moved row 0 down");
        assert_eq!(snapshot.len(), 200);
        assert_eq!(snapshot.iter().count(), 200);
        assert!(snapshot.retained_bytes_against(&s) >= old_len, "the snapshot is charged for the page it no longer shares");
    }

    /// A page whose only rows lost their inverse is dropped rather than rewritten empty.
    #[test]
    fn compact_drops_a_page_that_only_held_orphans() {
        let mut s = CellStore::new();
        for row in [0u32, 1, 2, 100, 101, 102] {
            s.set(c(row, 0), Cell::value(Value::Number(f64::from(row))));
        }
        assert!(s.enable_paging());
        assert_eq!(s.paged_band_count(), 2);
        // Delete logical rows 0..63: page 0 held only those rows.
        s.shift_rows(64, -64);
        let visible: Vec<(CellRef, Cell)> = s.iter().collect();
        let used = s.used_range();
        assert_eq!(visible.len(), 3);
        assert_eq!(s.len(), 6);
        let out = s.compact();
        assert_eq!(out.cells, 3);
        assert_eq!(out.pages, 1, "the page that only held orphans is dropped");
        assert_eq!(s.paged_band_count(), 1);
        assert_eq!(s.len(), 3);
        assert_eq!(s.iter().collect::<Vec<_>>(), visible);
        assert_eq!(s.used_range(), used);
        assert_eq!(s.get(c(36, 0)).map(|x| x.value), Some(Value::Number(100.0)), "the remaining page still reads");
    }

    /// The early-out: a clean paged store (identity maps) decodes nothing, touches no page and
    /// changes nothing. This is the regression test for the O(1) return.
    #[test]
    fn compact_is_free_when_nothing_is_orphaned() {
        let mut s = CellStore::new();
        for row in 0..200u32 {
            s.set(c(row, 0), Cell::value(Value::Number(f64::from(row))));
        }
        assert!(s.enable_paging());
        assert!(!s.needs_compaction());
        let pages: Vec<(u32, Arc<page::Page>)> = s.pages.iter().map(|(key, page)| (*key, page.clone())).collect();
        let bytes = s.paged_bytes();
        let len = s.len();
        let decodes = s.page_file.as_ref().map(|file| file.decodes()).unwrap_or(0);
        assert_eq!(s.page_cache_stats().expect("the store pages").bytes, 0, "spilling decodes nothing");
        let out = s.compact();
        assert_eq!(out, Compacted::default());
        assert_eq!(s.page_file.as_ref().map(|file| file.decodes()).unwrap_or(0), decodes, "a clean paged store decodes no page");
        let stats = s.page_cache_stats().expect("the store pages");
        assert_eq!((stats.entries, stats.bytes, stats.misses, stats.evictions), (0, 0, 0, 0), "a clean pass primes and charges nothing");
        assert_eq!(s.paged_bytes(), bytes);
        assert_eq!(s.len(), len);
        for (key, page) in pages {
            assert!(s.pages.get(&key).is_some_and(|now| Arc::ptr_eq(now, &page)), "page {key} is untouched");
        }
    }

    /// A page can hold a cell at a physical column whose inverse is gone; the column gaps select
    /// it, the rewrite drops the cell and recomputes the bounds.
    #[test]
    fn compact_reclaims_paged_columns() {
        let mut s = CellStore::new();
        for row in 0..100u32 {
            for col in 0..4u32 {
                s.set(c(row, col), Cell::value(Value::Number(f64::from(row * 10 + col))));
            }
        }
        assert!(s.enable_paging());
        let pages = s.paged_band_count();
        assert_eq!(pages, 2);
        s.shift_cols(1, -1);
        let visible: Vec<(CellRef, Cell)> = s.iter().collect();
        let used = s.used_range();
        assert_eq!(visible.len(), 300);
        assert_eq!(s.len(), 400);
        let out = s.compact();
        assert_eq!(out.cells, 100);
        assert_eq!(out.pages, pages, "every page held a cell in the dropped column");
        assert_eq!(s.len(), visible.len());
        assert_eq!(s.iter().collect::<Vec<_>>(), visible);
        assert_eq!(s.used_range(), used);
        assert!(
            s.pages.values().all(|page| page.bounds().is_some_and(|(_, _, first, _)| first >= 1)),
            "the rewritten bounds no longer cross the dropped column"
        );
        assert_eq!(s.get(c(0, 0)).map(|x| x.value), Some(Value::Number(1.0)), "the rewritten page reads");
        assert_eq!(s.get(c(99, 2)).map(|x| x.value), Some(Value::Number(993.0)));
    }

    /// With every page checked, a contiguous map returns to the identity fast path even though the
    /// store is still paged — the `reset_prefix` change — and the next pass is O(1).
    #[test]
    fn compact_restores_identity_after_a_complete_page_sweep() {
        let mut s = CellStore::new();
        for row in 0..200u32 {
            s.set(c(row, 0), Cell::value(Value::Number(f64::from(row))));
        }
        // A cell in the last row of the grid: deleting that row leaves one contiguous run from the
        // origin and puts the cell at physical `MAX_ROWS - 1` in a page of its own with no inverse.
        s.set(c(cella_core::MAX_ROWS - 1, 0), Cell::value(Value::Number(7.0)));
        assert!(s.enable_paging());
        let pages = s.paged_band_count();
        assert_eq!(pages, 5);
        s.shift_rows(cella_core::MAX_ROWS, -1);
        let visible: Vec<(CellRef, Cell)> = s.iter().collect();
        assert_eq!(visible.len(), 200);
        assert_eq!(s.len(), 201);
        assert!(!s.row_map.is_identity());
        let out = s.compact();
        assert_eq!(out.cells, 1);
        assert_eq!(out.pages, 1, "the page that only held the dropped row is gone");
        assert!(out.changed);
        assert!(s.row_map.is_identity(), "a complete sweep returns a contiguous map to the fast path");
        assert!(s.col_map.is_identity());
        assert_eq!(s.paged_band_count(), pages - 1);
        assert_eq!(s.len(), visible.len());
        assert_eq!(s.iter().collect::<Vec<_>>(), visible);
        let lens: Vec<usize> = s.pages.values().map(|page| page.page_len()).collect();
        let again = s.compact();
        assert_eq!(again, Compacted::default(), "the second pass is a no-op");
        assert!(s.row_map.is_identity());
        assert_eq!(s.pages.values().map(|page| page.page_len()).collect::<Vec<_>>(), lens);
        assert_eq!(s.iter().collect::<Vec<_>>(), visible);
    }

    /// A second pass over a store whose map cannot return to identity decodes the pages a gap still
    /// selects, finds nothing and changes nothing.
    #[test]
    fn compact_is_idempotent_across_pages() {
        let mut s = CellStore::new();
        for row in 0..200u32 {
            s.set(c(row, 0), Cell::value(Value::Number(f64::from(row))));
        }
        assert!(s.enable_paging());
        s.shift_rows(1, -1);
        assert_eq!(s.compact().pages, 1);
        let pages = s.paged_band_count();
        let lens: Vec<usize> = s.pages.values().map(|page| page.page_len()).collect();
        let bytes = s.paged_bytes();
        let visible: Vec<(CellRef, Cell)> = s.iter().collect();
        let used = s.used_range();
        let len = s.len();
        let again = s.compact();
        assert_eq!(again, Compacted::default(), "nothing left to reclaim");
        assert!(!again.changed);
        assert_eq!((s.paged_band_count(), s.paged_bytes(), s.len()), (pages, bytes, len));
        assert_eq!(s.pages.values().map(|page| page.page_len()).collect::<Vec<_>>(), lens);
        assert_eq!(s.used_range(), used);
        assert_eq!(s.iter().collect::<Vec<_>>(), visible);
    }

    /// A rewritten page recomputes its formula/array flags from the new band instead of dropping
    /// them, so `formulas` and `has_owned_cells` still answer from the page.
    #[test]
    fn compact_keeps_formulas_on_a_rewritten_page() {
        let mut s = CellStore::new();
        for row in 0..200u32 {
            s.set(c(row, 0), Cell::value(Value::Number(f64::from(row))));
        }
        s.set(c(1, 1), Cell { value: Value::Number(2.0), formula: Some(Arc::new(Formula::new("=A1+1"))), style: StyleId(3) });
        assert!(s.enable_paging());
        s.shift_rows(1, -1);
        let formulas: Vec<(CellRef, Option<String>, bool)> = s.formulas().map(|(at, f, has_value)| (at, f.text_at(at), has_value)).collect();
        assert_eq!(formulas.len(), 1);
        let block = RangeRef::new(c(0, 0), c(63, 10));
        assert!(s.has_owned_cells(block));
        assert_eq!(s.compact().pages, 1);
        assert_eq!(s.formulas().map(|(at, f, has_value)| (at, f.text_at(at), has_value)).collect::<Vec<_>>(), formulas);
        assert!(s.has_owned_cells(block));
        let page = s.pages.get(&0).expect("the rewritten page");
        assert!(page.has_formulas(), "the formula flag is recomputed");
        assert!(page.has_owned(), "the owned flag is recomputed");
        assert!(s.get(c(0, 1)).and_then(|cell| cell.formula).is_some(), "the formula reads back from the rewritten page");
    }

    /// A page that cannot be read leaves the pass incomplete: the store keeps its orphan and its
    /// map, and reports no change.
    #[test]
    fn compact_keeps_the_maps_when_a_page_cannot_be_read() {
        let mut s = CellStore::new();
        for row in 0..200u32 {
            s.set(c(row, 0), Cell::value(Value::Number(f64::from(row))));
        }
        assert!(s.enable_paging());
        s.shift_rows(1, -1);
        // The page file is gone (as a corrupt or missing page would be): the candidate cannot be
        // decoded, so nothing is dropped and the shortened run must not become identity.
        s.page_file = None;
        let out = s.compact();
        assert_eq!(out, Compacted::default(), "nothing was reclaimed");
        assert_eq!(s.len(), 200, "the orphan stays counted");
        assert_eq!(s.paged_band_count(), 4);
        assert!(!s.row_map.is_identity(), "an incomplete pass does not reset the map");
        assert!(s.needs_compaction(), "the map still has a gap to check");
    }

    /// A rewritten page serializes to the same logical cells: the wire format walks `iter`, and a
    /// save after compaction must not lose or move anything.
    #[test]
    fn compact_keeps_the_logical_cells_of_a_rewritten_page_across_serialization() {
        let mut s = CellStore::new();
        for row in 0..200u32 {
            s.set(c(row, 0), Cell { value: Value::Number(f64::from(row)), formula: None, style: StyleId(row % 3) });
            s.set(c(row, 1), Cell::value(Value::Text(Arc::from(format!("r{row}").as_str()))));
        }
        s.set(c(2, 2), Cell { value: Value::Number(9.0), formula: Some(Arc::new(Formula::new("=A1+1"))), style: StyleId(7) });
        assert!(s.enable_paging());
        s.shift_rows(1, -1);
        s.shift_cols(1, -1);
        let before = s.len();
        let visible: Vec<(CellRef, Cell)> = s.iter().collect();
        let out = s.compact();
        assert_eq!(out.cells, before - visible.len());
        assert_eq!(s.len(), visible.len());
        assert_eq!(s.iter().collect::<Vec<_>>(), visible);
        let positions: Vec<CellRef> = visible.iter().map(|(at, _)| *at).collect();
        let values: Vec<Value> = visible.iter().map(|(_, cell)| cell.value.clone()).collect();
        let bytes = serde_json::to_vec(&s).expect("serialize");
        let back: CellStore = serde_json::from_slice(&bytes).expect("deserialize");
        assert_eq!(back.len(), visible.len());
        assert_eq!(back.iter().map(|(at, _)| at).collect::<Vec<_>>(), positions);
        assert_eq!(back.iter().map(|(_, cell)| cell.value).collect::<Vec<_>>(), values);
        assert_eq!(back, s, "the saved cells are the logical cells");
    }

    /// The cached used box has to be the box a walk finds, after every kind of mutation: writes,
    /// blank writes, removals, taking a range, the reader's row-at-a-time path, band batches, row
    /// and column shifts, compaction, paging and eviction. This is the proof that no mutator
    /// forgets to grow or invalidate it: a stale box would show a wrong scrollbar extent and write
    /// a wrong `<dimension>` on save.
    #[test]
    fn used_range_equals_a_walk_after_every_mutation() {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = |n: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % n.max(1)
        };
        let number = |v: u64| Cell::value(Value::Number(v as f64));
        let mut store = CellStore::new();
        let mut paged = false;
        for step in 0..1500u32 {
            let op = next(15);
            match op {
                0..=3 => {
                    let at = c(next(200) as u32, next(8) as u32);
                    store.set(at, number(u64::from(step)));
                }
                // A blank write removes the cell instead of storing one.
                4 => {
                    let _ = store.set(c(next(200) as u32, next(8) as u32), Cell::default());
                }
                5 => {
                    store.remove(c(next(200) as u32, next(8) as u32));
                }
                6 => {
                    let r0 = next(200) as u32;
                    let c0 = next(8) as u32;
                    let r1 = r0.saturating_add(next(4) as u32).min(199);
                    let c1 = c0.saturating_add(next(3) as u32).min(7);
                    store.take_range(RangeRef::new(c(r0, c0), c(r1, c1)));
                }
                7 => {
                    // The reader's path: one whole row at a time.
                    let row = next(200) as u32;
                    let mut cells: Vec<(u32, Cell)> = (0..next(6)).map(|i| (next(8) as u32, number((u64::from(row) << 8) | i))).collect();
                    cells.sort_unstable_by_key(|(col, _)| *col);
                    store.set_row(row, &mut cells);
                }
                8 => {
                    // The reader's other path: a whole band handed over at once.
                    let mut batch = BandBatch::new();
                    for _ in 0..1 + next(3) {
                        let row = next(200) as u32;
                        let mut cells: Vec<(u32, Cell)> = (0..1 + next(4)).map(|_| (next(8) as u32, number(u64::from(step)))).collect();
                        cells.sort_unstable_by_key(|(col, _)| *col);
                        batch.set_row(row, &mut cells);
                    }
                    store.insert_batch(batch);
                }
                9 => {
                    let from = next(200) as u32;
                    store.shift_rows(from, 1 + next(2) as i64);
                }
                10 => {
                    let from = next(8) as u32;
                    store.shift_cols(from, 1 + next(2) as i64);
                }
                11 => {
                    store.compact();
                }
                12 => {
                    let from = next(120) as u32;
                    let delta = 1 + next(2) as i64;
                    if next(2) == 0 {
                        store.shift_rows_in_cols(0, 7, from, delta);
                    } else {
                        store.shift_cols_in_rows(0, 199, from, delta);
                    }
                }
                13 => {
                    if paged {
                        // Eviction happens as the paged bands are read: the box is their bounds.
                        for _ in 0..3 {
                            let _ = store.get(c(next(200) as u32, next(8) as u32));
                        }
                    } else {
                        paged = store.enable_paging_with_cache(64 * 1024);
                    }
                }
                _ => {
                    store.rebuild();
                }
            }
            assert_eq!(store.used_range(), store.walk_used(), "step {step} op {op}");
        }
        assert!(!paged || store.paged_band_count() > 0 || store.used_range() == store.walk_used());
    }

    /// The box follows its edges and leaves the middle alone: a shrink only happens when the cell
    /// that went away was the box's last row, column, or only cell.
    #[test]
    fn the_used_box_shrinks_when_its_edges_go_away() {
        let mut store = CellStore::new();
        store.set(c(2, 1), plain(1));
        store.set(c(9, 4), plain(2));
        store.set(c(5, 2), plain(3));
        assert_eq!(store.used_range(), Some(RangeRef::new(c(2, 1), c(9, 4))));
        store.remove(c(5, 2));
        assert_eq!(store.used_range(), Some(RangeRef::new(c(2, 1), c(9, 4))), "the middle is not an edge");
        store.remove(c(9, 4));
        assert_eq!(store.used_range(), Some(RangeRef::new(c(2, 1), c(2, 1))));
        store.set(c(2, 1), Cell::default());
        assert_eq!(store.used_range(), None);
        assert_eq!(store.used_range(), store.walk_used());
    }

    /// A band that moves to a page and back keeps its bounds, so the cached box is the same before
    /// the spill, after it, and after a cache that holds one band evicts every band it reads.
    #[test]
    fn a_paged_store_reports_the_same_used_box_across_a_spill_and_an_eviction() {
        let mut store = CellStore::new();
        for row in 0..600u32 {
            store.set(c(row, 3), plain(row));
        }
        let before = store.used_range();
        assert_eq!(before, Some(RangeRef::new(c(0, 3), c(599, 3))));
        assert!(store.enable_paging_with_cache(4096));
        assert!(store.paged_band_count() > 0);
        assert_eq!(store.used_range(), before, "a page carries the band's bounds");
        for row in 0..600u32 {
            let _ = store.get(c(row, 3));
        }
        assert_eq!(store.used_range(), before, "evicting decoded bands does not move the box");
        assert_eq!(store.used_range(), store.walk_used());
        store.remove(c(599, 3));
        assert_eq!(store.used_range(), Some(RangeRef::new(c(0, 3), c(598, 3))));
    }

    /// A report has to be a partition: a field added later must not be counted twice, and the
    /// total must be the fields. The store here holds values, styles, texts and formulas, so every
    /// class of the report is non-empty.
    #[test]
    fn store_memory_total_is_the_sum_of_its_parts() {
        let mut s = CellStore::new();
        let mut shapes = ShapeTable::default();
        for row in 0..200u32 {
            s.set(c(row, 0), Cell::value(Value::Number(f64::from(row))));
            s.set(c(row, 1), Cell::value(Value::Text(Arc::from(format!("row {row}").as_str()))));
            s.set(c(row, 2), Cell { value: Value::Number(1.0), formula: Some(Arc::new(shaped(&mut shapes, "A1+B1", c(row, 2)))), style: StyleId(1) });
        }
        let m = s.memory();
        let sum = m.band_index
            + m.band_headers
            + m.bitmaps
            + m.payloads
            + m.kind_runs
            + m.style_runs
            + m.style_ids
            + m.number_dictionaries
            + m.texts
            + m.row_extras
            + m.formula_text
            + m.formula_exprs
            + m.arrays
            + m.wide
            + m.maps
            + m.page_cache;
        assert_eq!(m.total(), sum, "the total is the sum of the fields");
        for (name, part) in [
            ("band headers", m.band_headers),
            ("bitmaps", m.bitmaps),
            ("payloads", m.payloads),
            ("kind runs", m.kind_runs),
            ("texts", m.texts),
            ("row extras", m.row_extras),
            ("formula text", m.formula_text),
            ("formula trees", m.formula_exprs),
        ] {
            assert!(part > 0, "{name} is reported");
        }
        // A store holding 600 cells costs its packing, not 600 `Cell` structs (40 bytes each).
        assert!(m.total() > 600 * 8, "the packing is charged");
    }

    /// A thousand formulas cost a formula's text, its shape and its side-table entry. Each
    /// formula's text is the same six characters here (`$A$1+1`, one shape at every position),
    /// so the text grows by about `1000 * (size_of::<Formula>() + 6)` while the **one** shape
    /// they share is charged once: the tree per formula is what the shape work removed (S7 of
    /// `plan/SHAPE-SHARING.md`).
    #[test]
    fn a_thousand_formulas_raise_the_formula_lines() {
        let mut s = CellStore::new();
        s.set(c(0, 0), Cell::value(Value::Number(1.0)));
        let before = s.memory();
        let mut shapes = ShapeTable::default();
        for col in 1..=1000u32 {
            let at = c(0, col);
            let text = cella_formula::parse("$A$1+1").expect("parses");
            s.set(
                at,
                Cell { value: Value::Empty, formula: Some(Arc::new(Formula::from_expr_at(text, at, &mut shapes, false))), style: StyleId::DEFAULT },
            );
        }
        let after = s.memory();
        assert_eq!(shapes.len(), 1, "one shape, however many cells hold it");
        let text = after.formula_text.saturating_sub(before.formula_text);
        let trees = after.formula_exprs.saturating_sub(before.formula_exprs);
        let extras = after.row_extras.saturating_sub(before.row_extras);
        let formula = std::mem::size_of::<Formula>();
        assert!(text >= 1000 * formula, "{text} bytes of formula text for 1000 formulas");
        assert!(text <= 2 * 1000 * (formula + 8), "{text} bytes is not more than the struct and its text");
        let one_shape = Shape::from_expr(cella_formula::parse("$A$1+1").expect("parses"), c(0, 1)).bytes as usize;
        assert!(one_shape > 0, "the formula parses into a tree");
        assert_eq!(trees, one_shape, "the one shape a thousand formulas share is charged once");
        assert!(extras > 0, "the row gained a side table");
    }

    /// Two cells that hold the same `Arc<str>` share one copy: the band's text dictionary keeps one
    /// entry per pointer, so the second cell must not add the characters again.
    #[test]
    fn a_text_two_cells_share_is_charged_once() {
        let text: Arc<str> = Arc::from("a shared string");
        let mut one = CellStore::new();
        one.set(c(0, 0), Cell::value(Value::Text(Arc::clone(&text))));
        let alone = one.memory().texts;
        let mut two = CellStore::new();
        two.set(c(0, 0), Cell::value(Value::Text(Arc::clone(&text))));
        two.set(c(1, 0), Cell::value(Value::Text(Arc::clone(&text))));
        assert_eq!(two.memory().texts, alone, "the shared string is charged once");
    }

    /// Two cells of one fill share one shape: its tree is charged once, while both formula texts
    /// are. This is the accounting the undo budget reads (SHAPE-SHARING.md trap 6) — charged per
    /// cell, a million-row fill would look like a gigabyte and drop the whole history.
    #[test]
    fn a_shape_two_cells_share_is_charged_once() {
        let mut shapes = ShapeTable::default();
        let tree = cella_formula::parse("A1+1").expect("parses");
        let one = Formula::from_expr_at(tree, c(0, 0), &mut shapes, false);
        let two = one.copied_to(c(0, 0), c(1, 0)).expect("the shape fits one row down");
        assert!(two.shape().is_some_and(|s| Arc::ptr_eq(&s, &one.shape().expect("a shape"))), "the shape is shared");
        let mut s = CellStore::new();
        s.set(c(0, 0), Cell { value: Value::Number(1.0), formula: Some(Arc::new(one)), style: StyleId::DEFAULT });
        let alone = s.memory();
        s.set(c(1, 0), Cell { value: Value::Number(1.0), formula: Some(Arc::new(two)), style: StyleId::DEFAULT });
        let both = s.memory();
        assert_eq!(both.formula_exprs, alone.formula_exprs, "the shared shape is charged once");
        assert!(both.formula_text > alone.formula_text, "both formula texts are charged");
    }

    /// An empty store reports almost nothing — the index and its two maps — and never panics.
    #[test]
    fn an_empty_store_has_a_small_report() {
        let m = CellStore::new().memory();
        assert!(m.total() < 1024, "{} bytes for an empty store", m.total());
        assert_eq!(m.formula_text, 0);
        assert_eq!(m.formula_exprs, 0);
        assert_eq!(m.texts, 0);
    }

    /// A paged store charges the decoded bands it caches, and the report falls back when the cache
    /// drops them: the page file's queue is part of the store's live memory, not a guess.
    #[test]
    fn a_paged_store_reports_its_page_cache() {
        let mut s = CellStore::new();
        for row in 0..200u32 {
            s.set(c(row, 0), Cell::value(Value::Number(f64::from(row))));
        }
        let live = s.memory().total();
        assert!(s.enable_paging_with_cache(64 << 20), "a page file is created");
        assert!(s.paged_band_count() > 0);
        let spilled = s.memory();
        assert_eq!(spilled.band_headers, 0, "no live band is left to charge");
        assert!(spilled.page_cache > 0, "the page file is charged");
        assert!(s.get(c(0, 0)).is_some());
        let cached = s.memory().page_cache;
        assert!(cached > spilled.page_cache, "the decoded band is charged");
        assert!(live > 0);
    }
}

#[cfg(test)]
mod array_anchor_tests {
    use super::*;

    fn anchored(text: &str, range: &str) -> Cell {
        let mut f = Formula::new(text);
        f.array = RangeRef::parse(range).map(Box::new);
        Cell::formula(f)
    }

    /// The flag answers "does any formula own a legacy array covering more than one cell?", which
    /// is what lets the commit's array guard skip walking the store. It has to be raised by every
    /// way a cell gets in, and kept by a clone: a missed one would let an edit overwrite part of
    /// an array.
    #[test]
    fn the_array_flag_is_raised_however_a_formula_arrives() {
        let mut s = CellStore::new();
        assert!(!s.has_array_anchors(), "an empty store holds none");
        s.set(CellRef::new(0, 0), anchored("=1", "A1"));
        assert!(!s.has_array_anchors(), "a one-cell array owns no other cell");
        s.set(CellRef::new(1, 0), Cell::value(Value::Number(1.0)));
        assert!(!s.has_array_anchors(), "a value raises nothing");
        s.set(CellRef::new(2, 0), anchored("=2", "A3:A4"));
        assert!(s.has_array_anchors(), "set");
        assert!(s.clone().has_array_anchors(), "a clone shares the cells, and the answer");

        let mut s = CellStore::new();
        s.set_row(5, &mut vec![(1, anchored("=3", "B6:B7"))]);
        assert!(s.has_array_anchors(), "set_row");

        let mut s = CellStore::new();
        let mut batch = BandBatch::new();
        batch.set_row(70, &mut vec![(2, anchored("=4", "C71:D72"))]);
        s.insert_batch(batch);
        assert!(s.has_array_anchors(), "a batch, the way a file reader fills the store");

        let wire = serde_json::to_string(&s).expect("serialize");
        let back: CellStore = serde_json::from_str(&wire).expect("deserialize");
        assert!(back.has_array_anchors(), "a store read back from a file");
        assert_eq!(back, s);
    }

    /// Removing cells or writing values over them never lowers the flag: it may answer "yes" for a
    /// store whose array is gone (a sheet pays the guard's walk then), never "no" while one is
    /// there. This is what makes the answer safe to skip the guard on.
    #[test]
    fn the_array_flag_is_only_lowered_by_a_walk_of_every_cell() {
        let mut s = CellStore::new();
        s.set(CellRef::new(2, 0), anchored("=2", "A3:A4"));
        s.remove(CellRef::new(2, 0));
        assert!(s.has_array_anchors(), "removing the array leaves the answer conservative");
        s.set(CellRef::new(2, 0), Cell::value(Value::Number(7.0)));
        s.set_value(CellRef::new(2, 0), &Value::Number(8.0));
        assert!(s.has_array_anchors());
        // Rebuilding walks every cell, so it settles the answer exactly again.
        s.rebuild();
        assert!(!s.has_array_anchors());
        s.set(CellRef::new(4, 1), anchored("=2", "B5:B6"));
        s.rebuild();
        assert!(s.has_array_anchors(), "a rebuild that carries the array raises it");
    }
}
#[cfg(test)]
mod row_extras_memory {
    use super::*;
    use crate::cell::{Cell, Formula};
    use std::mem::size_of;

    /// A row of two formulas charges two formula slots, not the four a growing `Vec` keeps: at a
    /// million rows that spare capacity was 32 MB, and the reader's rows kept it until `put_row`
    /// gave it back. Both write paths are covered — a whole row at once, and cell by cell.
    #[test]
    fn a_row_of_two_formulas_charges_two_formulas_and_no_spare_capacity() {
        let mut row_at_once = CellStore::default();
        let mut cell_by_cell = CellStore::default();
        for row in 0..1_000 {
            let two = |row: u32| -> Vec<(u32, Cell)> {
                (0..2)
                    .map(|c| {
                        let mut cell = Cell::formula(Formula::new(&format!("=A{}", row + c + 1)));
                        cell.value = Value::from(1.0);
                        (c, cell)
                    })
                    .collect()
            };
            let mut cells = two(row);
            row_at_once.set_row(row, &mut cells);
            for (col, cell) in two(row) {
                cell_by_cell.set(cella_core::CellRef::new(row, col), cell);
            }
        }
        let per_row = 8 + size_of::<RowExtras>() + 2 * size_of::<(u32, Arc<Formula>)>();
        assert_eq!(row_at_once.memory().row_extras / 1_000, per_row);
        assert_eq!(cell_by_cell.memory().row_extras / 1_000, per_row);
    }
}
#[cfg(test)]
mod dictionary_audit {
    use super::*;

    /// A band that has stopped taking cells keeps neither the number index (which only adding
    /// cells needs) nor the spare capacity a growing vector carries. A fill over a million rows
    /// used to keep both per band for the life of the workbook: 1,024 bytes of index plus a
    /// quarter of the number table.
    #[test]
    fn a_finished_band_keeps_no_number_index_and_no_spare_table() {
        let mut store = CellStore::default();
        for row in 0..600u32 {
            for col in 0..6u32 {
                let value = Value::from(f64::from(row * 6 + col) + 0.5);
                store.set(cella_core::CellRef::new(row, col), crate::cell::Cell::value(value));
            }
        }
        let slacked = store.bands.values().filter(|band| band.numbers.capacity() != band.numbers.len() || !band.number_slots.is_empty()).count();
        assert_eq!(slacked, 1, "only the band still being added to keeps its index and its slack");
        let exact = store.bands.values().filter(|band| band.numbers.capacity() == band.numbers.len()).count();
        assert!(exact >= 9, "nine bands finished, one is being built: {exact}");
    }
}

#[cfg(test)]
mod rewrite_in_place_tests {
    use super::*;
    use crate::cell::Cell;

    fn c(r: u32, col: u32) -> CellRef {
        CellRef::new(r, col)
    }

    /// Overwriting a cell whose kind and style land in the runs already there moves no stripe slice —
    /// the fast path a sort's write-back takes when it rewrites a row cell for cell — and the band that
    /// comes out must be exactly the one a fresh write of the same cells builds.
    #[test]
    fn rewriting_cells_in_place_equals_building_them_again() {
        fn cells(seed: u32, row: u32) -> Vec<(u32, Cell)> {
            (0..8u32)
                .map(|col| {
                    let n = seed.wrapping_mul(31).wrapping_add(row.wrapping_mul(8)).wrapping_add(col);
                    let value = match n % 5 {
                        0 => Value::Number(f64::from(n)),
                        1 => Value::Text(Arc::from(format!("text {n}").as_str())),
                        2 => Value::Bool(n % 2 == 0),
                        3 => Value::Error(cella_core::CellError::Div0),
                        _ => Value::Empty,
                    };
                    (col, Cell { value, formula: None, style: StyleId(n % 3) })
                })
                .collect()
        }
        // Two versions of the same five rows: one built cell by cell into empty rows, one written over
        // rows that already held cells of other kinds (which is the replacement branch of put, where
        // a run layout change has to move the band's stripe offsets).
        let mut built = CellStore::new();
        let mut rewritten = CellStore::new();
        for row in 0..5u32 {
            rewritten.set_row(row, &mut cells(0, row));
        }
        for row in 0..5u32 {
            built.set_row(row, &mut cells(1, row));
            rewritten.set_row(row, &mut cells(1, row));
        }
        assert_eq!(rewritten, built);
        assert_eq!(rewritten.len(), built.len());
        assert_eq!(rewritten.used_range(), built.used_range());
        for row in 0..5u32 {
            for col in 0..8u32 {
                assert_eq!(rewritten.get(c(row, col)), built.get(c(row, col)), "cell {row},{col}");
            }
        }
        // A row that loses a cell and one that gains one: the runs and the payload stripe change
        // length, and the rows laid out after them must still be found.
        for store in [&mut rewritten, &mut built] {
            let _ = store.remove(c(2, 3));
            let _ = store.remove(c(2, 4));
            store.set(c(4, 10), Cell { value: Value::Number(99.0), formula: None, style: StyleId(2) });
        }
        assert_eq!(rewritten, built, "after a row shrinks and another grows");
        assert_eq!(rewritten.len(), built.len());
    }
}

#[cfg(test)]
mod compare_then_write {
    use super::*;
    use crate::cell::Cell;

    fn c(r: u32, col: u32) -> CellRef {
        CellRef::new(r, col)
    }

    /// A store shaped like a recalculation's: a formula and a cached value in one column, a text in
    /// another, over enough rows that a batch spans several bands.
    fn sheet(rows: u32) -> CellStore {
        let mut s = CellStore::new();
        for r in 0..rows {
            let mut cell = Cell::value(Value::Number(f64::from(r)));
            cell.formula = Some(Arc::new(Formula::new(format!("=A{}+1", r + 1).as_str())));
            s.set(c(r, 1), cell);
            s.set(c(r, 3), Cell::value(Value::Text(Arc::from(format!("t{r}").as_str()))));
        }
        s
    }

    fn batch(rows: u32, delta: f64) -> Vec<(CellRef, Value)> {
        (0..rows).map(|r| (c(r, 1), Value::Number(f64::from(r) + delta))).collect()
    }

    /// The write `set_values` makes with the compare switched off: every band in the batch is
    /// copied and every value written, whatever the band already held. The reference a skipped
    /// write must equal, cell for cell.
    fn blind(s: &mut CellStore, values: &[(CellRef, Value)]) {
        let mut i = 0;
        while i < values.len() {
            let key = values[i].0.row / BAND;
            let end = values[i..].partition_point(|(x, _)| x.row / BAND == key) + i;
            if let Some(band) = s.bands.get_mut(&key) {
                let band = Arc::make_mut(band);
                for (x, v) in &values[i..end] {
                    band.set_value(x.row % BAND, x.col, v);
                }
            }
            i = end;
        }
    }

    /// The claim the skip rests on: a batch of values the store already holds leaves it exactly
    /// what a blind write would leave — same cells, same bands, same length, same used box, no
    /// diff — while the bands stay the snapshot's. A skipped copy must never look like ownership:
    /// the next write that does change a cell still copies, and the snapshot keeps its value.
    #[test]
    fn an_unchanged_value_write_equals_a_blind_one_and_keeps_the_bands_shared() {
        let rows = 300u32; // five bands, the last one partial
        let mut skipped = sheet(rows);
        let snapshot = skipped.clone();
        let mut written = skipped.clone();
        let values = batch(rows, 0.0);
        let mut fast = values.clone();
        skipped.set_values(&mut fast);
        blind(&mut written, &values);
        assert_eq!(skipped, written, "the store a skipped write leaves is the blind one's");
        assert_eq!(skipped.len(), written.len());
        assert_eq!(skipped.band_count(), written.band_count());
        assert_eq!(skipped.used_range(), written.used_range());
        for (key, band) in &skipped.bands {
            assert!(written.bands.get(key).is_some_and(|other| band.same_as(other)), "band {key} holds the same cells");
            let shared = snapshot.bands.get(key).expect("the snapshot keeps its band");
            assert!(Arc::ptr_eq(band, shared), "a skipped write leaves band {key} shared with the snapshot");
            assert!(Arc::strong_count(band) >= 2, "band {key} still has two owners");
        }
        assert!(skipped.diff(&snapshot).is_empty(), "and it changed nothing");

        let mut changed = vec![(c(10, 1), Value::Number(999.0))];
        skipped.set_values(&mut changed);
        assert_eq!(skipped.value(c(10, 1)), Value::Number(999.0));
        assert_eq!(snapshot.value(c(10, 1)), Value::Number(10.0), "the snapshot keeps the value it had");
        let band = skipped.bands.get(&(10 / BAND)).expect("the band");
        assert!(!Arc::ptr_eq(band, snapshot.bands.get(&(10 / BAND)).expect("the snapshot's band")), "the write that changes a cell copies it");
    }

    /// A batch that changes one band and not the next: the unchanged band is not copied, the changed
    /// one is, and the store is the blind write's outside the sharing.
    #[test]
    fn a_mixed_batch_copies_only_the_band_that_changes() {
        let rows = 128u32;
        let mut skipped = sheet(rows);
        let snapshot = skipped.clone();
        let mut written = skipped.clone();
        let values: Vec<(CellRef, Value)> =
            (0..rows).map(|r| (c(r, 1), Value::Number(if r < BAND { f64::from(r) } else { f64::from(r) + 0.5 }))).collect();
        let mut fast = values.clone();
        skipped.set_values(&mut fast);
        blind(&mut written, &values);
        assert_eq!(skipped, written);
        let b0 = skipped.bands.get(&0).expect("band 0");
        let b1 = skipped.bands.get(&1).expect("band 1");
        assert!(Arc::ptr_eq(b0, snapshot.bands.get(&0).expect("the snapshot's band 0")), "the unchanged band is not copied");
        assert!(!Arc::ptr_eq(b1, snapshot.bands.get(&1).expect("the snapshot's band 1")), "the changed band is copied");
        assert_eq!(skipped.diff(&snapshot).len(), BAND as usize, "only the changed band's cells are a diff");
    }

    /// The comparison the skip rests on: the packed row read against the incoming value, with no
    /// `Cell` built — the same number bits (zero has one spelling in the store), the same text by
    /// pointer or by content, a boolean, an error, a stored empty value, and "nothing stored" kept
    /// apart from "an empty value stored".
    #[test]
    fn value_is_reads_the_packed_row() {
        let text: Arc<str> = Arc::from("hello");
        let mut s = CellStore::new();
        s.set(c(0, 0), Cell::value(Value::Number(1.5)));
        s.set(c(0, 1), Cell::value(Value::Text(text.clone())));
        s.set(c(0, 2), Cell::value(Value::Bool(true)));
        s.set(c(0, 3), Cell::value(Value::Error(CellError::Div0)));
        s.set(c(0, 4), Cell { style: StyleId(2), ..Default::default() });
        s.set(c(0, 5), Cell::value(Value::Number(0.0)));
        let band = s.bands.get(&0).expect("band 0");
        assert_eq!(band.value_is(0, 0, &Value::Number(1.5)), Some(true));
        assert_eq!(band.value_is(0, 0, &Value::Number(1.25)), Some(false));
        assert_eq!(band.value_is(0, 1, &Value::Text(Arc::from("hello"))), Some(true), "text is compared by content");
        assert_eq!(band.value_is(0, 1, &Value::Text(text.clone())), Some(true), "and by pointer");
        assert_eq!(band.value_is(0, 1, &Value::Text(Arc::from("hellp"))), Some(false));
        assert_eq!(band.value_is(0, 2, &Value::Bool(true)), Some(true));
        assert_eq!(band.value_is(0, 2, &Value::Bool(false)), Some(false));
        assert_eq!(band.value_is(0, 3, &Value::Error(CellError::Div0)), Some(true));
        assert_eq!(band.value_is(0, 3, &Value::Error(CellError::Value)), Some(false));
        assert_eq!(band.value_is(0, 4, &Value::Empty), Some(true), "a stored empty value");
        assert_eq!(band.value_is(0, 4, &Value::Number(0.0)), Some(false), "an empty value is not a stored zero");
        assert_eq!(band.value_is(0, 5, &Value::Number(0.0)), Some(true));
        assert_eq!(band.value_is(0, 5, &Value::Number(-0.0)), Some(true), "zero has one spelling in the store");
        assert_eq!(band.value_is(0, 5, &Value::Empty), Some(false), "and a stored zero is not the empty value");
        assert_eq!(band.value_is(0, 6, &Value::Number(0.0)), None, "nothing is stored there");
        assert_eq!(band.value_is(9, 0, &Value::Number(1.0)), None, "no such row in the band");
    }

    /// A mapped store (a row insert above every cell, a column insert before them) skips an
    /// unchanged batch through both maps: the batch's logical positions are not the packed ones.
    #[test]
    fn a_mapped_store_skips_an_unchanged_batch() {
        let mut s = sheet(200);
        s.shift_rows(1, 1);
        s.shift_cols(1, 1);
        let snapshot = s.clone();
        let values: Vec<(CellRef, Value)> = (0..200u32).map(|r| (c(r + 1, 2), Value::Number(f64::from(r)))).collect();
        let mut batch = values.clone();
        s.set_values(&mut batch);
        assert_eq!(s, snapshot, "an unchanged batch leaves a mapped store where it was");
        assert!(s.diff(&snapshot).is_empty());
        for (key, band) in &s.bands {
            assert!(snapshot.bands.get(key).is_some_and(|other| Arc::ptr_eq(band, other)), "band {key} skipped, not copied");
        }
        // An insert at row 1 leaves logical row 1 a gap: the batch's value there lands nowhere,
        // and row 0 keeps the value it had.
        assert_eq!(s.value(c(1, 2)), Value::Empty, "the inserted gap still holds nothing");
        assert_eq!(s.value(c(0, 2)), Value::Number(0.0));
    }

    /// The parallel value write (a batch above `PARALLEL_WRITE_MIN`) skips unchanged bands too: the
    /// workers hand none back, so the snapshot keeps sharing them; a changed batch still replaces
    /// every band it touches.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_parallel_batch_skips_unchanged_bands() {
        let rows = 20_000u32;
        let mut s = sheet(rows);
        let snapshot = s.clone();
        let mut fast = batch(rows, 0.0);
        s.set_values(&mut fast);
        assert_eq!(s, snapshot);
        for (key, band) in &s.bands {
            assert!(snapshot.bands.get(key).is_some_and(|other| Arc::ptr_eq(band, other)), "band {key} skipped");
        }
        let mut changed = batch(rows, 0.5);
        s.set_values(&mut changed);
        assert_eq!(s.value(c(rows - 1, 1)), Value::Number(f64::from(rows - 1) + 0.5));
        assert!(!Arc::ptr_eq(&s.bands[&0], &snapshot.bands[&0]), "a changed batch replaces the band");
        assert_eq!(snapshot.value(c(rows - 1, 1)), Value::Number(f64::from(rows - 1)), "the snapshot keeps its value");
    }

    /// A single value written over itself changes nothing; one that differs still writes, keeps the
    /// formula and answers true.

    #[test]
    fn set_value_skips_an_identical_value() {
        let mut s = sheet(4);
        let snapshot = s.clone();
        assert!(s.set_value(c(2, 1), &Value::Number(2.0)));
        assert_eq!(s, snapshot);
        for (key, band) in &s.bands {
            assert!(snapshot.bands.get(key).is_some_and(|other| Arc::ptr_eq(band, other)), "band {key} untouched");
        }
        assert!(s.set_value(c(2, 1), &Value::Number(2.5)));
        assert_eq!(s.value(c(2, 1)), Value::Number(2.5));
        assert!(s.get(c(2, 1)).is_some_and(|cell| cell.formula.is_some()), "the formula stays");
        assert!(!s.set_value(c(2, 6), &Value::Number(1.0)), "a value with no cell to land in is refused");
    }

    /// A cell written, overwritten and blanked keeps `len`, `contains` and the used box right:
    /// `put` answers the two questions the length bookkeeping asks.
    #[test]
    fn a_cell_written_overwritten_and_blanked_keeps_the_books() {
        let mut s = CellStore::new();
        let cells = [(0u32, 0u32), (5, 3), (63, 64), (64, 0), (100, 10)];
        for (i, (row, col)) in cells.into_iter().enumerate() {
            s.set(c(row, col), Cell::value(Value::Number(i as f64)));
            assert_eq!(s.len(), i + 1, "a new cell counts once");
            assert!(s.contains(c(row, col)));
        }
        let before = (s.len(), s.used_range());
        s.set(c(5, 3), Cell::value(Value::Number(7.0)));
        assert_eq!(s.len(), before.0, "an overwrite changes no length");
        assert_eq!(s.used_range(), before.1, "and no edge");
        s.set(c(5, 3), Cell::default());
        assert_eq!(s.len(), before.0 - 1, "a blank removes the cell");
        assert!(!s.contains(c(5, 3)));
        assert_eq!(s.used_range(), before.1, "the box is sized by the cells that remain");
        s.set(c(300, 300), Cell::default());
        assert_eq!(s.len(), before.0 - 1, "a blank on nothing adds nothing");
        let before = s.len();
        s.set(c(300, 300), Cell::value(Value::Number(1.0)));
        s.set(c(300, 300), Cell::default());
        assert_eq!(s.len(), before, "and a cell written then blanked nets out");
        assert_eq!(s.iter().count(), s.len());
    }

    /// The row index answers exactly the rows the band holds, after rows are built one at a time,
    /// in one go and dropped — a stale bitmap would read a row that is gone or miss one that is
    /// there.
    #[test]
    fn the_row_index_agrees_with_the_rows_after_every_kind_of_edit() {
        let mut s = CellStore::new();
        for r in 0..BAND {
            s.set(c(r, 0), Cell::value(Value::Number(f64::from(r))));
        }
        for r in [7u32, 40, 63] {
            assert!(s.remove(c(r, 0)).is_some());
        }
        let mut row: Vec<(u32, Cell)> = (60..64).map(|col| (col, Cell::value(Value::Number(f64::from(col))))).collect();
        s.set_row(3, &mut row);
        let mut band: Vec<(u32, Cell)> = (0..BAND).map(|r| (r, Cell::value(Value::Number(f64::from(r))))).collect();
        s.set_row(BAND + 2, &mut band);
        s.remove(c(BAND + 2, 5));
        for (key, band) in &s.bands {
            assert_eq!(band.row_bits, Band::rows_bits(&band.rows), "band {key}: the row index disagrees with its rows");
            for k in 0..BAND {
                assert_eq!(band.row_index(k), band.rows.binary_search_by_key(&k, |r| r.key).ok(), "band {key} row {k}");
            }
        }
        // The rows that were removed read as missing, and the ones around them still read.
        let band = s.bands.get(&0).expect("band 0");
        assert_eq!(band.value_is(7, 0, &Value::Number(7.0)), None);
        assert_eq!(band.value_is(40, 0, &Value::Number(40.0)), None);
        assert_eq!(band.value_is(63, 0, &Value::Number(63.0)), None);
        assert_eq!(band.value_is(3, 62, &Value::Number(62.0)), Some(true), "the row built whole");
        assert_eq!(band.value_is(0, 0, &Value::Number(0.0)), Some(true));
    }
}
