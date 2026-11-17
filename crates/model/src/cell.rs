//! Cells.

use std::sync::Arc;

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_formula::adjust::shift_relative_copy;
use gridcraft_formula::{Expr, Shape, ShapeTable};
use serde::{Deserialize, Serialize};

use crate::style::StyleId;

/// A parsed formula. `text` is the canonical formula text without the leading `=`.
///
/// The text is the formula, and the tree is a **cache of (text, position)**: a formula created at
/// a cell (`new_at`, `from_expr_at`) keeps the [`Shape`] its cell shares with every cell of the
/// same fill, and a formula that has no shape yet (one read back from a JSON document, one built
/// by [`Formula::new`]) parses its text when something needs the tree (`expr_at`). There is no
/// per-cell tree to keep: that was the second representation the shape work removed (S7 of
/// `plan/SHAPE-SHARING.md`), and it cost a 96-byte-per-formula allocation class of its own.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Formula {
    pub text: String,
    /// The interned shape of the formula: its tree with every relative reference stored as an
    /// offset from the cell, so it serves every cell the formula was filled into. Read it through
    /// [`Formula::expr_at`], which resolves it at the cell's position.
    #[serde(skip)]
    pub shape: Option<Arc<Shape>>,
    /// Legacy array formula (Ctrl+Shift+Enter) covering this range; the formula lives in the
    /// top-left cell.
    ///
    /// Boxed: a legacy array is a fact about a handful of cells, and the 20-byte range inline cost
    /// every formula a size class of its own. `Option<Box<_>>` is transparent to serde, so the
    /// JSON and XLSX spellings are unchanged.
    pub array: Option<Box<RangeRef>>,
    /// Read from a file that marks this formula as a dynamic-array formula (cell metadata `cm`).
    /// The writer keeps the mark, so a formula Excel evaluates as an array is never saved as a
    /// legacy formula (which Excel evaluates with implicit intersection). Formulas entered or
    /// rewritten in GridCraft start without it, and the writer decides from their shape.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dynamic: bool,
}

impl Formula {
    /// Parses `text` (with or without `=`) and keeps its canonical spelling. Unparseable formulas
    /// keep their text and evaluate to `#NAME?`.
    ///
    /// The cell gets no shape because it has no position to intern one from: a consumer that knows
    /// the cell should call [`Formula::new_at`], which shares one tree with every cell of the same
    /// shape. This form is **text alone**, the state S7 of `plan/SHAPE-SHARING.md` left as the
    /// fallback — every `expr_at` parses the text until a recalculation interns a shape for it.
    pub fn new(text: &str) -> Formula {
        let body = text.strip_prefix('=').unwrap_or(text);
        match gridcraft_formula::parse(body) {
            Ok(e) => Formula::from_text(&gridcraft_formula::print(&e)),
            Err(_) => Formula::from_text(body),
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
            Err(_) => Formula::from_text(body),
        }
    }
    /// The formula a tree spells at `at`: the tree is canonicalised into a shape and interned,
    /// and the cell keeps its own A1 text (what a save writes).
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
        Formula { text, shape: Some(shapes.intern(shape)), array: None, dynamic: false }
    }
    /// A formula of `text` with no tree and no shape: the text exactly as it was read, kept whole
    /// (it may not parse at all, and one that does not is shown and saved as it stands).
    ///
    /// This is what the parser-free paths build — a JSON document carries the text of every
    /// formula and nothing else — and where the shape design's fallback begins (§5.3 of
    /// `plan/SHAPE-SHARING.md`): [`Formula::expr_at`] parses it at the cell.
    pub fn from_text(text: &str) -> Formula {
        Formula { text: text.to_string(), shape: None, array: None, dynamic: false }
    }
    /// The absolute tree this cell evaluates **at its position**, and the only way to a tree a
    /// formula keeps: the shape resolved (its relative parts are offsets), else — a cell that holds
    /// text alone, which is what a deserialised JSON workbook is — the text **parsed at the cell**,
    /// bounded by the parser's own limits exactly as before. `None` when the text does not parse.
    ///
    /// A shape resolves at its own cell by construction; one that would clamp at `at` falls
    /// through to the text, which is the print of the clamped tree the cell already shows.
    pub fn expr_at(&self, at: CellRef) -> Option<Expr> {
        if let Some(shape) = &self.shape
            && let Some(e) = shape.resolve(at)
        {
            return Some(e);
        }
        gridcraft_formula::parse(&self.text).ok()
    }
    /// The shape this cell's formula shares with every other cell of the same fill, when it has one.
    pub fn shape(&self) -> Option<Arc<Shape>> {
        self.shape.clone()
    }
    /// The formula this cell's content becomes when the formula at `from` is filled or copied to
    /// `at`: the **same shape**, one `Arc` clone and a print, when it fits there; otherwise the
    /// tree a per-cell shift makes at `at`, held as a shape that belongs to this cell alone (a
    /// relative part that leaves the sheet clamps to `#REF!` and stays that way — Excel does not
    /// resurrect it, trap 9).
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
            return Some(Formula { text, shape: Some(Arc::clone(shape)), array: None, dynamic: false });
        }
        // A cell the shape cannot serve — a relative reference of it leaves the sheet here, so the
        // destination text names `#REF!` — gets a tree of its own again, exactly as the per-cell
        // shift gave it, held as a **private** shape (no interner in reach, and nothing to share:
        // such a cell is rare and its text is unique). It still prints the text it was given
        // (`Shape::print_at` at `at` is ``text``), so the master invariant holds for it too.
        let e = shape.map_or_else(|| gridcraft_formula::parse(text).ok(), |s| s.resolve(from))?;
        let shifted = shift_relative_copy(&e, i64::from(at.row) - i64::from(from.row), i64::from(at.col) - i64::from(from.col));
        // The text is the *resolved* tree's: the canonical one stores offsets, and printing those
        // would spell a relative reference as the row and column it is an offset by.
        let text = gridcraft_formula::print(&shifted);
        let mut private = Shape::from_expr(shifted, at);
        // A shift keeps the syntax that makes a file spell a formula differently (a `LET`, a `#`,
        // an `@`, a trim reference, a structured reference) and clamping a reference to `#REF!`
        // can only drop one. The source's answer is carried over because this crate cannot compute
        // the part of it that lives in the writer: over-approximating costs one walk on a save,
        // under-approximating would write a formula under the model's own spelling.
        private.file_rewrite |= shape.is_some_and(|s| s.file_rewrite);
        Some(Formula { text, shape: Some(Arc::new(private)), array: None, dynamic: false })
    }
    /// The absolute tree the cell's **text** spells, at the position it was written for: the tree a
    /// structural rewrite (`adjust`) must shift, where [`Formula::expr_at`] is the tree at the cell
    /// the formula sits at now.
    ///
    /// S5 of `plan/SHAPE-SHARING.md` made this the **fallback** rather than the rule: a structural
    /// edit rewrites one tree per shape (`Shape::adjust`) and only the cells it cannot answer that
    /// way — a reference that straddles the edit, so it moves while the cell does not
    /// (`Shape::straddles`), or a cell whose shape does not resolve where it now sits — parse their
    /// own text here. The text is the only witness of what such a formula said, because its shape's
    /// base is stale by construction.
    pub fn text_tree(&self) -> Option<Expr> {
        gridcraft_formula::parse(&self.text).ok()
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
/// `Formula` is **48**: a text, the shape pointer, the boxed legacy range and the dynamic-array
/// mark. It was 72 before the shape work's two layout steps (S6 boxed `array`, 72 -> 56; S7
/// deleted the per-cell tree and its byte count, 56 -> 48), which drops the `Arc<Formula>`
/// allocation from a 96-byte chunk to an 80-byte one. Pinned here because the undo budget and the
/// store's accounting read it.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<Formula>() == 48);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<Cell>() == 40);

#[cfg(test)]
mod tests {
    use super::*;

    /// The range of a legacy array is boxed, and `Option<Box<_>>` is transparent to serde: the
    /// JSON a workbook is written as must be byte-identical to the one the inline range wrote
    /// (S6 of `plan/SHAPE-SHARING.md`), so no `formatVersion` bump and old files still load.
    #[test]
    fn an_array_range_is_boxed_without_changing_the_json() {
        let mut f = Formula::new("=A1:A2*2");
        f.array = gridcraft_core::RangeRef::parse("C1:D2").map(Box::new);
        let json = serde_json::to_string(&f).expect("a formula serialises");
        assert_eq!(json, r#"{"text":"A1:A2*2","array":{"start":{"row":0,"col":2},"end":{"row":1,"col":3}}}"#);
        let back: Formula = serde_json::from_str(&json).expect("a formula deserialises");
        assert_eq!(back, f, "the text, the range and the dynamic mark survive");
        assert_eq!(back.array.as_deref().copied(), gridcraft_core::RangeRef::parse("C1:D2"));
        // A formula with no array keeps the field as `null`: the shape the field always had.
        let plain = Formula::new("=A1+1");
        assert_eq!(serde_json::to_string(&plain).expect("serialises"), r#"{"text":"A1+1","array":null}"#);
    }
}

#[cfg(all(test, target_pointer_width = "64"))]
mod size_pins {
    use super::{Cell, Formula};

    #[test]
    fn formula_sizes_are_pinned() {
        println!("size_of::<Formula>() = {}", std::mem::size_of::<Formula>());
        println!("size_of::<Cell>() = {}", std::mem::size_of::<Cell>());
        assert_eq!((std::mem::size_of::<Formula>(), std::mem::size_of::<Cell>()), (48, 40));
    }
}
