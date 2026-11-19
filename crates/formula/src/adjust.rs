//! Reference adjustment: copying formulas, inserting/deleting rows and columns, moving cells,
//! renaming and deleting sheets.

use cella_core::{CellError, MAX_COLS, MAX_ROWS, RangeRef};

use crate::ast::{Anchor, Expr, RefKind, Reference, SheetSel, clamp_anchor};

/// Copy/fill: relative parts move by `(dr, dc)`. References pushed off the sheet become `#REF!`.
pub fn shift_relative(e: Expr, dr: i64, dc: i64) -> Expr {
    e.map(&mut |x| match x {
        Expr::Ref(r) => match shift_ref(r, dr, dc) {
            Some(r) => Expr::Ref(r),
            None => Expr::Error(CellError::Ref),
        },
        other => other,
    })
}

/// Copy/fill from a tree that must stay where it is: the same result as
/// `shift_relative(e.clone(), dr, dc)` in one walk, so a fill of a million formulas copies each
/// tree once instead of twice. Follows the left spine in a loop like `Expr::map`, so an operator
/// chain as long as Excel allows does not recurse.
pub fn shift_relative_copy(e: &Expr, dr: i64, dc: i64) -> Expr {
    copy_and_shift(e, dr, dc)
}

fn copy_and_shift(e: &Expr, dr: i64, dc: i64) -> Expr {
    let mut spine: Vec<(crate::ast::BinOp, &Expr)> = Vec::new();
    let mut node = e;
    while let Expr::Binary(op, a, b) = node {
        spine.push((*op, b));
        node = a;
    }
    let mut acc = copy_node(node, dr, dc);
    while let Some((op, b)) = spine.pop() {
        acc = Expr::Binary(op, Box::new(acc), Box::new(copy_and_shift(b, dr, dc)));
    }
    acc
}

fn copy_node(e: &Expr, dr: i64, dc: i64) -> Expr {
    let child = |x: &Expr| Box::new(copy_and_shift(x, dr, dc));
    match e {
        Expr::Number(n) => Expr::Number(*n),
        Expr::Text(t) => Expr::Text(std::sync::Arc::clone(t)),
        Expr::Bool(b) => Expr::Bool(*b),
        Expr::Error(x) => Expr::Error(*x),
        Expr::Array(rows) => Expr::Array(rows.iter().map(|r| r.iter().map(|x| copy_and_shift(x, dr, dc)).collect()).collect()),
        Expr::Ref(r) => match shift_ref(r.clone(), dr, dc) {
            Some(r) => Expr::Ref(r),
            None => Expr::Error(CellError::Ref),
        },
        Expr::Name(n) => Expr::Name(n.clone()),
        Expr::Struct(s) => Expr::Struct(s.clone()),
        Expr::Unary(op, x) => Expr::Unary(*op, child(x)),
        Expr::Binary(op, a, b) => Expr::Binary(*op, child(a), child(b)),
        Expr::Call(n, args) => Expr::Call(n.clone(), args.iter().map(|x| copy_and_shift(x, dr, dc)).collect()),
        Expr::Invoke(c, args) => Expr::Invoke(child(c), args.iter().map(|x| copy_and_shift(x, dr, dc)).collect()),
        Expr::Missing => Expr::Missing,
        Expr::Paren(x) => Expr::Paren(child(x)),
    }
}

/// Defined names: relative parts move by `(dr, dc)`, wrapping around the sheet's edges. Files
/// store a name's relative references as seen from A1 (a name for "the cell to the left" defined
/// in B1 is `Sheet1!XFD1`), and a use in another cell moves them by that cell's offset from A1.
pub fn shift_relative_wrapping(e: Expr, dr: i64, dc: i64) -> Expr {
    // A relative part wraps (it does not clamp), and the result is 0..max, which an `i32` holds.
    let wrap = |v: i32, abs: bool, d: i64, max: u32| -> i32 {
        if abs {
            return v;
        }
        let n = (i64::from(v) + d).rem_euclid(i64::from(max));
        i32::try_from(n).unwrap_or(0)
    };
    let anchor = |a: Anchor| Anchor { row: wrap(a.row, a.row_abs, dr, MAX_ROWS), col: wrap(a.col, a.col_abs, dc, MAX_COLS), ..a };
    e.map(&mut |x| match x {
        Expr::Ref(r) => {
            let kind = match r.kind {
                RefKind::Cell(a) => RefKind::Cell(anchor(a)),
                RefKind::Range(a, b) => RefKind::Range(anchor(a), anchor(b)),
                RefKind::Rows(r0, a0, r1, a1) => RefKind::Rows(wrap(r0, a0, dr, MAX_ROWS), a0, wrap(r1, a1, dr, MAX_ROWS), a1),
                RefKind::Cols(c0, a0, c1, a1) => RefKind::Cols(wrap(c0, a0, dc, MAX_COLS), a0, wrap(c1, a1, dc, MAX_COLS), a1),
            };
            Expr::Ref(Reference { sheet: r.sheet, kind })
        }
        other => other,
    })
}

/// Whether the formula has a relative (no `$`) row or column anywhere.
pub fn has_relative_refs(e: &Expr) -> bool {
    e.references().iter().any(|r| match r.kind {
        RefKind::Cell(a) => !a.row_abs || !a.col_abs,
        RefKind::Range(a, b) => !a.row_abs || !a.col_abs || !b.row_abs || !b.col_abs,
        RefKind::Rows(_, a0, _, a1) | RefKind::Cols(_, a0, _, a1) => !a0 || !a1,
    })
}

fn shift_anchor(a: &Anchor, dr: i64, dc: i64) -> Option<Anchor> {
    let row = if a.row_abs { i64::from(a.row) } else { i64::from(a.row) + dr };
    let col = if a.col_abs { i64::from(a.col) } else { i64::from(a.col) + dc };
    let (row, col) = clamp_anchor(row, col)?;
    Some(Anchor { row, col, ..*a })
}

fn shift_line(v: i32, abs: bool, d: i64, max: u32) -> Option<i32> {
    if abs {
        return Some(v);
    }
    let n = i64::from(v) + d;
    if (0..i64::from(max)).contains(&n) { i32::try_from(n).ok() } else { None }
}

fn shift_ref(r: Reference, dr: i64, dc: i64) -> Option<Reference> {
    let kind = match &r.kind {
        RefKind::Cell(a) => RefKind::Cell(shift_anchor(a, dr, dc)?),
        RefKind::Range(a, b) => RefKind::Range(shift_anchor(a, dr, dc)?, shift_anchor(b, dr, dc)?),
        RefKind::Rows(r0, a0, r1, a1) => RefKind::Rows(shift_line(*r0, *a0, dr, MAX_ROWS)?, *a0, shift_line(*r1, *a1, dr, MAX_ROWS)?, *a1),
        RefKind::Cols(c0, a0, c1, a1) => RefKind::Cols(shift_line(*c0, *a0, dc, MAX_COLS)?, *a0, shift_line(*c1, *a1, dc, MAX_COLS)?, *a1),
    };
    Some(Reference { sheet: r.sheet, kind })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Rows,
    Cols,
}

/// A structural edit on one sheet.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// Insert `count` rows/cols before index `at`.
    Insert { axis: Axis, at: u32, count: u32 },
    /// Delete `count` rows/cols starting at `at`.
    Delete { axis: Axis, at: u32, count: u32 },
    /// Cut-paste of a block: references into `from` now point into the block at `to` (top-left).
    Move { from: RangeRef, to_row: u32, to_col: u32 },
    /// Insert Cells shifting down (`Rows`) or right (`Cols`): the cells of `range` and those
    /// below (right of) it move by its height (width), within its columns (rows) only.
    InsertCells { axis: Axis, range: RangeRef },
    /// Delete Cells shifting up (`Rows`) or left (`Cols`): `range` goes and the cells below
    /// (right of) it move back by its height (width), within its columns (rows) only.
    DeleteCells { axis: Axis, range: RangeRef },
    /// Cut-paste to another sheet; fully contained references follow the destination sheet too.
    MoveToSheet { from: RangeRef, to_sheet: String, to_row: u32, to_col: u32 },
}

/// Whether `r`, in a formula living on sheet `host`, points into sheet `target`.
fn on_target(r: &Reference, host: &str, target: &str) -> bool {
    match &r.sheet {
        SheetSel::Current => host.eq_ignore_ascii_case(target),
        SheetSel::Named(n) => n.eq_ignore_ascii_case(target),
        SheetSel::Span(..) => false,
    }
}

/// Whether [`adjust`] would change `r`. It runs the same [`adjust_ref`] on a copy without the
/// sheet name, so no name is copied: the adjustment reads only the position, and the sheet a cut
/// to another sheet assigns is compared afterwards.
fn ref_changes(r: &Reference, edit: &Edit) -> bool {
    let probe = Reference { sheet: SheetSel::Current, kind: r.kind.clone() };
    match adjust_ref(probe, edit) {
        None => true,
        Some(p) => p.kind != r.kind || (p.sheet != SheetSel::Current && p.sheet != r.sheet),
    }
}

/// Whether [`adjust`] would change the formula `e` at all, without copying or rebuilding it.
fn changes(e: &Expr, host: &str, target: &str, edit: &Edit) -> bool {
    let mut changed = false;
    e.walk(&mut |x| {
        if !changed && let Expr::Ref(r) = x {
            changed = on_target(r, host, target) && ref_changes(r, edit);
        }
    });
    changed
}

/// Adjusts a formula living on sheet `host` after `edit` on sheet `target`.
pub fn adjust(e: Expr, host: &str, target: &str, edit: &Edit) -> Expr {
    e.map(&mut |x| match x {
        Expr::Ref(r) if on_target(&r, host, target) => match adjust_ref(r, edit) {
            Some(r) => Expr::Ref(r),
            None => Expr::Error(CellError::Ref),
        },
        other => other,
    })
}

/// [`adjust`] for a borrowed formula: `None` when the edit leaves it exactly as it is, so a
/// caller can skip unaffected formulas without copying, rebuilding or printing them.
pub fn adjust_if_changed(e: &Expr, host: &str, target: &str, edit: &Edit) -> Option<Expr> {
    changes(e, host, target, edit).then(|| adjust(e.clone(), host, target, edit))
}

/// New position of an index after an insert (`None` never) or delete (`None` = deleted).
fn map_index(v: i32, at: u32, count: u32, insert: bool, max: u32) -> Option<i32> {
    // Sheet indices are non-negative here (an absolute tree's coordinate, never a shape's offset);
    // the arithmetic runs in `i64` so an insert near the sheet's end cannot wrap.
    let v = i64::from(v);
    let (at, count, max) = (i64::from(at), i64::from(count), i64::from(max));
    let n = if insert {
        if v >= at {
            let n = v + count;
            if n >= max {
                return None;
            }
            n
        } else {
            v
        }
    } else if v < at {
        v
    } else if v >= at + count {
        v - count
    } else {
        return None;
    };
    i32::try_from(n).ok()
}

/// Adjusts a span `[lo, hi]` (inclusive) on one axis.
fn map_span(lo: i32, hi: i32, at: u32, count: u32, insert: bool, max: u32) -> Option<(i32, i32)> {
    // As in `map_index`: coordinates, arithmetic in `i64`. `(at - 1).max(0)` keeps today's
    // `at.saturating_sub(1)`: a delete at the sheet's first line leaves an empty span at row 0.
    let (lo, hi) = (i64::from(lo), i64::from(hi));
    let (at, count, max) = (i64::from(at), i64::from(count), i64::from(max));
    if insert {
        let lo2 = if lo >= at { lo + count } else { lo };
        let hi2 = if hi >= at { hi + count } else { hi };
        // Whole-sheet spans (e.g. A:A) stay whole.
        let hi2 = hi2.min(max - 1);
        if lo2 > hi2 {
            return None;
        }
        Some((i32::try_from(lo2).ok()?, i32::try_from(hi2).ok()?))
    } else {
        let end = at + count; // exclusive
        if lo >= at && hi < end {
            return None; // fully deleted
        }
        let lo2 = if lo < at {
            lo
        } else if lo >= end {
            lo - count
        } else {
            at
        };
        let hi2 = if hi < at {
            hi
        } else if hi >= end {
            hi - count
        } else {
            (at - 1).max(0)
        };
        let lo2 = i32::try_from(lo2).ok()?;
        let hi2 = i32::try_from(hi2).ok()?;
        Some((lo2, hi2.max(lo2)))
    }
}

fn adjust_ref(r: Reference, edit: &Edit) -> Option<Reference> {
    match edit {
        Edit::Insert { axis, at, count } | Edit::Delete { axis, at, count } => {
            let insert = matches!(edit, Edit::Insert { .. });
            let kind = match (&r.kind, axis) {
                (RefKind::Cell(a), Axis::Rows) => RefKind::Cell(Anchor { row: map_index(a.row, *at, *count, insert, MAX_ROWS)?, ..*a }),
                (RefKind::Cell(a), Axis::Cols) => RefKind::Cell(Anchor { col: map_index(a.col, *at, *count, insert, MAX_COLS)?, ..*a }),
                (RefKind::Range(a, b), Axis::Rows) => {
                    let (lo, hi) = map_span(a.row.min(b.row), a.row.max(b.row), *at, *count, insert, MAX_ROWS)?;
                    RefKind::Range(Anchor { row: lo, ..*a }, Anchor { row: hi, ..*b })
                }
                (RefKind::Range(a, b), Axis::Cols) => {
                    let (lo, hi) = map_span(a.col.min(b.col), a.col.max(b.col), *at, *count, insert, MAX_COLS)?;
                    RefKind::Range(Anchor { col: lo, ..*a }, Anchor { col: hi, ..*b })
                }
                (RefKind::Rows(r0, a0, r1, a1), Axis::Rows) => {
                    let (lo, hi) = map_span(*r0.min(r1), *r0.max(r1), *at, *count, insert, MAX_ROWS)?;
                    RefKind::Rows(lo, *a0, hi, *a1)
                }
                (RefKind::Cols(c0, a0, c1, a1), Axis::Cols) => {
                    let (lo, hi) = map_span(*c0.min(c1), *c0.max(c1), *at, *count, insert, MAX_COLS)?;
                    RefKind::Cols(lo, *a0, hi, *a1)
                }
                (k, _) => k.clone(),
            };
            Some(Reference { sheet: r.sheet, kind })
        }
        Edit::InsertCells { axis, range } | Edit::DeleteCells { axis, range } => {
            // Like inserting/deleting lines, for references that lie within the shifted
            // columns (rows); Excel leaves references reaching outside them as they are.
            let span = r.range();
            let (inside, at, count) = match axis {
                Axis::Rows => (range.start.col <= span.start.col && span.end.col <= range.end.col, range.start.row, range.height()),
                Axis::Cols => (range.start.row <= span.start.row && span.end.row <= range.end.row, range.start.col, range.width()),
            };
            if !inside {
                return Some(r);
            }
            let axis = *axis;
            let lines = if matches!(edit, Edit::InsertCells { .. }) { Edit::Insert { axis, at, count } } else { Edit::Delete { axis, at, count } };
            adjust_ref(r, &lines)
        }
        Edit::Move { from, to_row, to_col } => {
            let dr = *to_row as i64 - from.start.row as i64;
            let dc = *to_col as i64 - from.start.col as i64;
            let range = r.range();
            if !from.contains_range(&range) {
                return Some(r);
            }
            let mv = |a: &Anchor| -> Option<Anchor> {
                let (row, col) = clamp_anchor(i64::from(a.row) + dr, i64::from(a.col) + dc)?;
                Some(Anchor { row, col, ..*a })
            };
            let kind = match &r.kind {
                RefKind::Cell(a) => RefKind::Cell(mv(a)?),
                RefKind::Range(a, b) => RefKind::Range(mv(a)?, mv(b)?),
                RefKind::Rows(a, aa, b, ba) => {
                    let dr = *to_row as i64 - from.start.row as i64;
                    RefKind::Rows(shift_line(*a, false, dr, MAX_ROWS)?, *aa, shift_line(*b, false, dr, MAX_ROWS)?, *ba)
                }
                RefKind::Cols(a, aa, b, ba) => {
                    let dc = *to_col as i64 - from.start.col as i64;
                    RefKind::Cols(shift_line(*a, false, dc, MAX_COLS)?, *aa, shift_line(*b, false, dc, MAX_COLS)?, *ba)
                }
            };
            Some(Reference { sheet: r.sheet, kind })
        }
        Edit::MoveToSheet { from, to_sheet, to_row, to_col } => {
            if !from.contains_range(&r.range()) {
                return Some(r);
            }
            let mut moved = adjust_ref(r, &Edit::Move { from: *from, to_row: *to_row, to_col: *to_col })?;
            moved.sheet = SheetSel::Named(to_sheet.clone());
            Some(moved)
        }
    }
}

// ---------------------------------------------------------------- shape level

/// What a lines [`Edit`] makes of a **canonical** (shape) tree: the answer [`adjust`] gives at
/// shape level (S5 of `plan/SHAPE-SHARING.md`).
#[derive(Debug)]
pub enum Adjusted {
    /// No reference the edit touches moves: the shape serves every cell it holds unchanged.
    Unchanged,
    /// The tree moves, and the move is a property of the tree alone — an absolute part names a
    /// cell, and a relative one is an offset from the formula's own cell, so it keeps itself. One
    /// answer therefore serves every cell the shape lives in.
    Moved(Expr),
    /// The edit moves a reference differently depending on where the cell sits, so only the cell
    /// can say: a cut moves the block's own references, Insert/Delete Cells move the part of a
    /// reference inside their band, a delete shrinks or swallows a span it cuts through, and a
    /// span with one `$` end is reordered by the values it resolves to.
    PerCell,
}

/// Whether the cell a shape serves keeps its position through the edit. A relative reference the
/// edit does not touch — a qualified one on another sheet, a 3-D span — keeps its *coordinates*,
/// so a cell that moves spells it one line further away: the answer differs by that much, and a
/// shape holds cells on both sides of an insert ([`Edit`] moves everything from `at` on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellMove {
    Stays,
    Moves,
}

/// [`adjust`] for a **canonical** tree: its absolute parts move exactly as [`adjust`] moves them,
/// its relative parts keep their offsets (relative to the formula's cell, they move with it or stay
/// with it). Position-free, which is what lets one answer serve a million cells (S5 of
/// `plan/SHAPE-SHARING.md`); see [`Adjusted::PerCell`] for what it cannot answer.
///
/// A cell for which this is not the same as `adjust(resolve(shape, cell))` — a reference that
/// straddles the edit's boundary, so it moves while the cell does not or the other way round — is
/// [`crate::Shape::straddles`]'s question, asked per cell.
pub fn adjust_canonical(tree: &Expr, host: &str, target: &str, edit: &Edit, cell: CellMove) -> Adjusted {
    let (axis, at, count, insert) = match edit {
        Edit::Insert { axis, at, count } => (*axis, *at, *count, true),
        Edit::Delete { axis, at, count } => (*axis, *at, *count, false),
        // A cut, and the two Cells variants: where the cell is decides, always.
        _ => {
            let touches = tree.references().iter().any(|r| on_target(r, host, target));
            return if touches { Adjusted::PerCell } else { Adjusted::Unchanged };
        }
    };
    // The lines the cell itself gains (an insert) or loses (a delete): a relative reference the
    // edit does not touch keeps its coordinates, so the cell spells it that much further away.
    let carry = match (cell, insert) {
        (CellMove::Stays, _) => 0,
        (CellMove::Moves, true) => i32::try_from(count).unwrap_or(i32::MAX),
        (CellMove::Moves, false) => i32::try_from(count).map_or(i32::MIN, |c| -c),
    };
    let mut changed = false;
    let mut per_cell = false;
    let out = tree.clone().map(&mut |x| match x {
        Expr::Ref(r) if on_target(&r, host, target) => match canonical_ref(&r, axis, at, count, insert) {
            Ok(Step::Keep) => Expr::Ref(r),
            Ok(Step::Set(nr)) => {
                changed = true;
                Expr::Ref(nr)
            }
            Ok(Step::Gone) => {
                changed = true;
                Expr::Error(CellError::Ref)
            }
            Err(()) => {
                per_cell = true;
                Expr::Ref(r)
            }
        },
        // A reference the edit does not touch (one qualified to another sheet, a 3-D span) whose
        // parts are relative: they keep their coordinates and the cell does not, so every one of
        // them is `carry` lines further away.
        Expr::Ref(r) if carry != 0 => {
            let kind = match &r.kind {
                RefKind::Cell(a) => RefKind::Cell(carry_anchor(*a, axis, carry)),
                RefKind::Range(a, b) => RefKind::Range(carry_anchor(*a, axis, carry), carry_anchor(*b, axis, carry)),
                RefKind::Rows(r0, a0, r1, a1) => RefKind::Rows(carry_line(*r0, *a0, carry), *a0, carry_line(*r1, *a1, carry), *a1),
                RefKind::Cols(c0, a0, c1, a1) => RefKind::Cols(carry_line(*c0, *a0, carry), *a0, carry_line(*c1, *a1, carry), *a1),
            };
            changed = true;
            Expr::Ref(Reference { sheet: r.sheet, kind })
        }
        other => other,
    });
    if per_cell {
        return Adjusted::PerCell;
    }
    if changed { Adjusted::Moved(out) } else { Adjusted::Unchanged }
}

/// Moves a relative part of a canonical reference by `carry` lines (an absolute part is a
/// coordinate and stays where it names).
fn carry_anchor(a: Anchor, axis: Axis, carry: i32) -> Anchor {
    match axis {
        Axis::Rows => Anchor { row: if a.row_abs { a.row } else { a.row.saturating_sub(carry) }, ..a },
        Axis::Cols => Anchor { col: if a.col_abs { a.col } else { a.col.saturating_sub(carry) }, ..a },
    }
}

fn carry_line(v: i32, abs: bool, carry: i32) -> i32 {
    if abs { v } else { v.saturating_sub(carry) }
}

/// One reference of a canonical tree after a lines edit.
enum Step {
    /// Nothing in it moves.
    Keep,
    /// It moves — or a span is reordered, which [`adjust`] does on every edit it touches.
    Set(Reference),
    /// The edit swallows it: `#REF!`.
    Gone,
}

/// The shape-level form of [`adjust_ref`] for a lines edit. `Err(())` is [`Adjusted::PerCell`]: the
/// two ends of a span are one absolute and one relative, and which of them is the low end is the
/// cell's position, not the tree's.
fn canonical_ref(r: &Reference, axis: Axis, at: u32, count: u32, insert: bool) -> Result<Step, ()> {
    let max = if axis == Axis::Rows { MAX_ROWS } else { MAX_COLS };
    // A single coordinate of the edited axis: an absolute one is mapped as `adjust` maps it (a
    // deleted line or one pushed past the end is `#REF!`), a relative one is an offset and keeps
    // itself.
    let index = |v: i32, abs: bool| -> Result<Option<i32>, ()> {
        if !abs {
            return Ok(None);
        }
        map_index(v, at, count, insert, max).map(Some).ok_or(())
    };
    // A span of the edited axis. Both ends absolute: `adjust`'s span arithmetic decides, so a
    // whole-line span stays whole and a deleted line collapses an absolute end onto `at`. Both
    // relative: the same arithmetic on offsets, which order by their own values. Mixed: only the
    // cell can order them. `None` inside the `Ok` is a span the edit swallowed.
    let span = |a: (i32, bool), b: (i32, bool)| -> Result<Option<(i32, i32)>, ()> {
        match (a.1, b.1) {
            (true, true) => Ok(map_span(a.0.min(b.0), a.0.max(b.0), at, count, insert, max)),
            (false, false) => Ok(Some((a.0.min(b.0), a.0.max(b.0)))),
            _ => Err(()),
        }
    };
    let kind = match (&r.kind, axis) {
        (RefKind::Cell(a), Axis::Rows) => RefKind::Cell(Anchor { row: index(a.row, a.row_abs)?.unwrap_or(a.row), ..*a }),
        (RefKind::Cell(a), Axis::Cols) => RefKind::Cell(Anchor { col: index(a.col, a.col_abs)?.unwrap_or(a.col), ..*a }),
        (RefKind::Range(a, b), Axis::Rows) => match span((a.row, a.row_abs), (b.row, b.row_abs))? {
            Some((lo, hi)) => RefKind::Range(Anchor { row: lo, ..*a }, Anchor { row: hi, ..*b }),
            None => return Ok(Step::Gone),
        },
        (RefKind::Range(a, b), Axis::Cols) => match span((a.col, a.col_abs), (b.col, b.col_abs))? {
            Some((lo, hi)) => RefKind::Range(Anchor { col: lo, ..*a }, Anchor { col: hi, ..*b }),
            None => return Ok(Step::Gone),
        },
        (RefKind::Rows(r0, a0, r1, a1), Axis::Rows) => match span((*r0, *a0), (*r1, *a1))? {
            Some((lo, hi)) => RefKind::Rows(lo, *a0, hi, *a1),
            None => return Ok(Step::Gone),
        },
        (RefKind::Cols(c0, a0, c1, a1), Axis::Cols) => match span((*c0, *a0), (*c1, *a1))? {
            Some((lo, hi)) => RefKind::Cols(lo, *a0, hi, *a1),
            None => return Ok(Step::Gone),
        },
        // The axis this edit does not move: nothing of it moves.
        (k, _) => k.clone(),
    };
    let out = Reference { sheet: r.sheet.clone(), kind };
    Ok(if out == *r { Step::Keep } else { Step::Set(out) })
}

/// Renames sheet references `old` → `new`.
pub fn rename_sheet(e: Expr, old: &str, new: &str) -> Expr {
    e.map(&mut |x| match x {
        Expr::Ref(mut r) => {
            match &mut r.sheet {
                SheetSel::Named(n) if n.eq_ignore_ascii_case(old) => *n = new.to_string(),
                SheetSel::Span(a, b) => {
                    if a.eq_ignore_ascii_case(old) {
                        *a = new.to_string();
                    }
                    if b.eq_ignore_ascii_case(old) {
                        *b = new.to_string();
                    }
                }
                _ => {}
            }
            Expr::Ref(r)
        }
        // Sheet-qualified defined name `Old!MyName` is stored as one string, with the bare sheet name as the parser
        // stores it (the printer adds the quotes).
        Expr::Name(n) => match n.split_at_checked(old.len()) {
            Some((head, rest)) if head.eq_ignore_ascii_case(old) && rest.starts_with('!') => Expr::Name(format!("{new}{rest}")),
            _ => Expr::Name(n),
        },
        other => other,
    })
}

/// References to a deleted sheet become `#REF!`. `order` lists the sheet names (before the deletion) in tab order:
/// a 3-D span that loses an end sheet shrinks to the neighbouring surviving sheet, as in Excel.
pub fn delete_sheet(e: Expr, name: &str, order: &[String]) -> Expr {
    let pos = |n: &str| order.iter().position(|o| o.eq_ignore_ascii_case(n));
    e.map(&mut |x| match x {
        Expr::Ref(r) if r.sheet_name().is_some_and(|n| n.eq_ignore_ascii_case(name)) => Expr::Error(CellError::Ref),
        Expr::Ref(mut r) => {
            if let SheetSel::Span(a, b) = &r.sheet
                && (a.eq_ignore_ascii_case(name) || b.eq_ignore_ascii_case(name))
            {
                let (Some(ia), Some(ib)) = (pos(a), pos(b)) else { return Expr::Ref(r) };
                if ia == ib {
                    return Expr::Error(CellError::Ref);
                }
                // The deleted end moves one sheet towards the other end; the span keeps its direction.
                let inward = |from: usize, to: usize| order.get(if from < to { from + 1 } else { from.saturating_sub(1) });
                let (na, nb) = if a.eq_ignore_ascii_case(name) { (inward(ia, ib), order.get(ib)) } else { (order.get(ia), inward(ib, ia)) };
                let (Some(na), Some(nb)) = (na, nb) else { return Expr::Error(CellError::Ref) };
                r.sheet = if na.eq_ignore_ascii_case(nb) { SheetSel::Named(na.clone()) } else { SheetSel::Span(na.clone(), nb.clone()) };
            }
            Expr::Ref(r)
        }
        other => other,
    })
}
