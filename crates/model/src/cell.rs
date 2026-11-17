//! Cells.

use std::sync::Arc;

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_formula::adjust::shift_relative_copy;
use gridcraft_formula::{Expr, Shape, ShapeTable};
use serde::{Deserialize, Serialize};

use crate::style::StyleId;

/// A parsed formula. `text` is the canonical formula text without the leading `=`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Formula {
    pub text: String,
    /// The cell's own cached tree: the absolute expression text parses to. A formula created at a
    /// position (`new_at`, `from_expr_at`) keeps a **shape** instead — one tree shared by every
    /// cell the formula was filled into — and this stays `None` for it. Both fields exist for the
    /// length of the shape-sharing round (S2/S3 of `plan/SHAPE-SHARING.md`); S7 removes this one.
    #[serde(skip)]
    pub expr: Option<Arc<Expr>>,
    /// The interned shape of the formula: its tree with every relative reference stored as an
    /// offset from the cell, so it serves every cell the formula was filled into. Read it through
    /// [`Formula::expr_at`], which resolves it at the cell's position.
    #[serde(skip)]
    pub shape: Option<Arc<Shape>>,
    /// Bytes the parsed expression tree holds, counted when it is parsed. The undo budget reads
    /// this instead of walking the tree of every formula in every band an edit touched.
    #[serde(skip)]
    pub expr_bytes: u32,
    /// Legacy array formula (Ctrl+Shift+Enter) covering this range; the formula lives in the
    /// top-left cell.
    pub array: Option<RangeRef>,
    /// Read from a file that marks this formula as a dynamic-array formula (cell metadata `cm`).
    /// The writer keeps the mark, so a formula Excel evaluates as an array is never saved as a
    /// legacy formula (which Excel evaluates with implicit intersection). Formulas entered or
    /// rewritten in GridCraft start without it, and the writer decides from their shape.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dynamic: bool,
}

impl Formula {
    /// Parses `text` (with or without `=`). Unparseable formulas keep their text and evaluate to
    /// `#NAME?`.
    ///
    /// This keeps the tree on the cell because it has no position to intern a shape from: a
    /// consumer that knows the cell it is making should call [`Formula::new_at`], which shares one
    /// tree with every cell of the same shape.
    pub fn new(text: &str) -> Formula {
        let body = text.strip_prefix('=').unwrap_or(text);
        match gridcraft_formula::parse(body) {
            Ok(e) => Formula {
                text: gridcraft_formula::print(&e),
                expr_bytes: expr_bytes(&e),
                expr: Some(Arc::new(e)),
                shape: None,
                array: None,
                dynamic: false,
            },
            Err(_) => Formula::unparseable(body),
        }
    }
    /// [`Formula::new`] for a formula entered at `at`: the tree becomes a **shape** interned in
    /// `shapes`, so a fill of this cell down a million rows shares one tree.
    ///
    /// `file_rewrite` is whether a file spells this formula differently from the model's text, the
    /// answer a save reads back from the shape (see [`Formula::from_expr_at`]).
    pub fn new_at(text: &str, at: CellRef, shapes: &mut ShapeTable, file_rewrite: bool) -> Formula {
        let body = text.strip_prefix('=').unwrap_or(text);
        match gridcraft_formula::parse(body) {
            Ok(e) => Formula::from_expr_at(e, at, shapes, file_rewrite),
            Err(_) => Formula::unparseable(body),
        }
    }
    pub fn from_expr(e: Expr) -> Formula {
        Formula { text: gridcraft_formula::print(&e), expr_bytes: expr_bytes(&e), expr: Some(Arc::new(e)), shape: None, array: None, dynamic: false }
    }
    /// [`Formula::from_expr`] for a formula at `at`: the tree is canonicalised into a shape and
    /// interned, and the cell keeps its own A1 text (what a save writes).
    ///
    /// `file_rewrite` is the half of "a file spells this formula differently" that this crate
    /// cannot compute: `gridcraft-formula` knows the syntax cases (`LET`/`LAMBDA`, `#`, `@`, a trim
    /// reference, `[@Col]`) but the `_xlfn.`/`_xlws.` function table lives in `gridcraft-xlsx`,
    /// above model. It is ORed into [`Shape::file_rewrite`] **before** interning, so a save never
    /// resolves a tree to decide how to spell a formula (S3 of `plan/SHAPE-SHARING.md`) — a shape
    /// interned without it would be saved under the model's own spelling.
    pub fn from_expr_at(e: Expr, at: CellRef, shapes: &mut ShapeTable, file_rewrite: bool) -> Formula {
        let text = gridcraft_formula::print(&e);
        let mut shape = Shape::from_expr(e, at);
        shape.file_rewrite |= file_rewrite;
        Formula { text, expr: None, shape: Some(shapes.intern(shape)), expr_bytes: 0, array: None, dynamic: false }
    }
    /// The formula of text that does not parse: the text is kept as it is and there is no tree.
    fn unparseable(text: &str) -> Formula {
        Formula { text: text.to_string(), expr: None, shape: None, expr_bytes: 0, array: None, dynamic: false }
    }
    /// The absolute tree this cell evaluates **at its position** — what `expr()` means for a cell
    /// that knows where it is: the shape resolved (its relative parts are offsets), else the cell's
    /// own cached tree, else the text parsed. `None` when the text does not parse.
    ///
    /// A shape resolves at its own cell by construction; one that would clamp at `at` falls
    /// through to the text, which is the print of the clamped tree the cell already shows.
    pub fn expr_at(&self, at: CellRef) -> Option<Expr> {
        if let Some(shape) = &self.shape
            && let Some(e) = shape.resolve(at)
        {
            return Some(e);
        }
        self.expr.as_deref().cloned().or_else(|| gridcraft_formula::parse(&self.text).ok())
    }
    /// The shape this cell's formula shares with every other cell of the same fill, when it has one.
    pub fn shape(&self) -> Option<Arc<Shape>> {
        self.shape.clone()
    }
    /// The formula this cell's content becomes when the formula at `from` is filled or copied to
    /// `at`: the **same shape**, one `Arc` clone and a print, when it fits there; otherwise a tree
    /// materialised at `at` exactly as a per-cell shift makes it (a relative part that leaves the
    /// sheet clamps to `#REF!` and stays that way — Excel does not resurrect it, trap 9).
    ///
    /// `None` when the formula cannot be moved at all (its text does not parse), which the callers
    /// treat as "keep the source cell as it is", exactly as the copy-and-shift code did.
    pub fn copied_to(&self, from: CellRef, at: CellRef) -> Option<Formula> {
        Formula::copied_from(self.shape.as_ref(), &self.text, from, at)
    }
    /// [`Formula::copied_to`] from the two halves a file's shared-formula family holds — the shape
    /// its master cell interned and, for the rare family member that cannot reuse it, the text.
    pub fn copied_from(shape: Option<&Arc<Shape>>, text: &str, from: CellRef, at: CellRef) -> Option<Formula> {
        if let Some(shape) = shape
            && shape.fits(at)
            && let Some(text) = shape.print_at(at)
        {
            return Some(Formula { text, expr: None, shape: Some(Arc::clone(shape)), expr_bytes: 0, array: None, dynamic: false });
        }
        // A member the family's shape cannot serve (a reference of it leaves the sheet at this
        // cell) gets its own tree, exactly as the per-cell shift gave it: a tree, not a shape,
        // because a private shape would have to carry a file-spelling answer this crate cannot
        // compute (see `from_expr_at`).
        let e = shape.map_or_else(|| gridcraft_formula::parse(text).ok(), |s| s.resolve(from))?;
        let shifted = shift_relative_copy(&e, i64::from(at.row) - i64::from(from.row), i64::from(at.col) - i64::from(from.col));
        Some(Formula::from_expr(shifted))
    }
    /// The absolute tree the cell's **text** spells, at the position it was written for: the tree a
    /// structural rewrite (`adjust`) must shift, where [`Formula::expr_at`] is the tree at the cell
    /// the formula sits at now. Those differ only when a structural edit has just moved the cell
    /// away from the cell its shape was interned for (a row inserted above it), and then the text
    /// is the only witness of what the formula said.
    ///
    /// A shape cell parses its text here; every static use wants [`Formula::expr_at`] instead.
    pub fn expr(&self) -> Option<Expr> {
        self.expr.as_deref().cloned().or_else(|| gridcraft_formula::parse(&self.text).ok())
    }
    /// The parsed expression without copying it (re-parses after deserialization).
    pub fn expr_arc(&self) -> Option<Arc<Expr>> {
        self.expr.clone().or_else(|| gridcraft_formula::parse(&self.text).ok().map(Arc::new))
    }
    /// Keeps the tree something else parsed from this text, so the next consumer shares it instead
    /// of parsing again: a formula deserialized into a document comes back as text alone
    /// (`expr` is not written to a file). The tree already here is never replaced by a different
    /// one — a cell that has one parsed its own text — and `text` is never regenerated from the
    /// tree, so the two cannot drift apart.
    pub fn share_expr(&mut self, e: &Arc<Expr>) {
        if self.expr.is_some() || self.shape.is_some() {
            return;
        }
        self.expr = Some(Arc::clone(e));
        self.expr_bytes = expr_bytes(e);
    }
}

/// Two formulas are the same formula when they are the same text, array range and dynamic-array
/// mark: the tree and the shape are caches of (text, position), and parsing is deterministic, so
/// a copy that was re-parsed or re-interned is the same formula. This is the rule the store's
/// `same_formula` has always compared with — deriving `PartialEq` over the cache meant two
/// spellings of one formula compared unequal, and a test that noticed was testing the cache
/// (SHAPE-SHARING.md trap 14).
impl PartialEq for Formula {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text && self.array == other.array && self.dynamic == other.dynamic
    }
}

/// Bytes an expression tree holds, node by node (the same figure [`Formula`] keeps).
fn expr_bytes(e: &Expr) -> u32 {
    let mut bytes = 0u32;
    e.walk(&mut |_| bytes = bytes.saturating_add(std::mem::size_of::<Expr>() as u32));
    bytes
}

/// A static picture stored as cell content, independent of floating drawing anchors.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CellPicture {
    /// Encoded PNG/JPEG bytes, base64 in JSON. Sheet entries share the payload through Arc.
    #[serde(with = "crate::b64")]
    pub data: Vec<u8>,
    pub mime: String,
    pub alt: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    /// Constant value, or the cached result of the formula.
    pub value: Value,
    pub formula: Option<Arc<Formula>>,
    pub style: StyleId,
}

impl Cell {
    pub fn value(v: Value) -> Cell {
        Cell { value: v, formula: None, style: StyleId::DEFAULT }
    }
    pub fn formula(f: Formula) -> Cell {
        Cell { value: Value::Empty, formula: Some(Arc::new(f)), style: StyleId::DEFAULT }
    }
    /// No value, no formula and the default style: storing it is pointless.
    pub fn is_blank(&self) -> bool {
        self.value.is_empty() && self.formula.is_none() && self.style == StyleId::DEFAULT
    }
    /// What the formula bar shows.
    pub fn input_text(&self) -> String {
        if let Some(f) = &self.formula {
            return format!("={}", f.text);
        }
        match &self.value {
            Value::Text(t) => {
                // Text that would parse as something else is shown with its apostrophe by Excel
                // only when entered that way; we show it plainly.
                t.to_string()
            }
            v => v.display(),
        }
    }
    /// What the formula bar shows when not editing: [`input_text`](Self::input_text), with a
    /// legacy array formula in braces (`{=A1:A2*2}`).
    pub fn bar_text(&self) -> String {
        match &self.formula {
            Some(f) if f.array.is_some() => format!("{{={}}}", f.text),
            _ => self.input_text(),
        }
    }
}

/// The layout the undo budget and the store's byte accounting read (SHAPE-SHARING.md §1.1, S0).
/// 64-bit numbers: `Formula` holds a `String` and `Cell` a `Value`, so a 32-bit target (wasm32)
/// has its own, smaller layout.
///
/// `Formula` is **72** while the cell carries both representations (S2/S3): the shape pointer sits
/// beside the tree pointer, 8 bytes that do not reach the allocator, which rounds
/// `Arc<Formula>` to the same 96-byte chunk either way. S7 deletes `expr` and brings it back to
/// 64, S6 boxes `array` for another 8.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<Formula>() == 72);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<Cell>() == 40);

#[cfg(all(test, target_pointer_width = "64"))]
mod size_pins {
    use super::{Cell, Formula};

    #[test]
    fn formula_sizes_are_pinned() {
        println!("size_of::<Formula>() = {}", std::mem::size_of::<Formula>());
        println!("size_of::<Cell>() = {}", std::mem::size_of::<Cell>());
        assert_eq!((std::mem::size_of::<Formula>(), std::mem::size_of::<Cell>()), (72, 40));
    }
}
