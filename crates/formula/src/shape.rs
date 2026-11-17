//! Formula *shapes*: one tree per formula shape, shared by every cell a formula was filled into.
//!
//! A shape is a formula's tree with every **relative** reference stored as an offset from the cell
//! the formula lives in (negative = up/left) and every **absolute** (`$`) part kept as the
//! coordinate it names. That is exactly R1C1, so 1,000 cells of `=B2*C2+D2` filled down are one
//! shape, and so are 1,000 cells of `=SUM($G$2:G2)` (one absolute part, one relative part that
//! holds the same offset in every row).
//!
//! The cell keeps its own A1 text — the text is the source of truth, and the shape is a cache of
//! (text, position) — so nothing here changes what is written to a file or shown in the formula
//! bar. See `plan/SHAPE-SHARING.md` §3.4 for the design and §4 for the invariants this module
//! exists to satisfy.
//!
//! **A cell no longer holds a tree.** `Formula` keeps its exact A1 text and an `Arc<Shape>`; the
//! evaluator, the dependency walk, the readers, the writers, fill, copy, paste and sort all read the
//! shape at the cell they are working on (`Reference::range_at`). A cell that only has text (a JSON
//! workbook, an unparseable formula) is parsed at the cell by `Formula::expr_at`, and `Calc` interns
//! that parse into one `Shape` per rebuild so a million such formulas share a tree again.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use cella_core::{CellRef, MAX_COLS, MAX_ROWS};

use crate::adjust::{Adjusted as Canonical, Axis, CellMove, Edit, adjust_canonical};
use crate::ast::{BinOp, Expr, RefKind, Reference, StructItem, UnOp, line_at};
use crate::printer::{print_at, print_r1c1};

/// A formula's shape: its tree with every relative reference stored as an offset from A1.
#[derive(Debug)]
pub struct Shape {
    /// The canonical tree: a relative part is an offset from the formula's own cell, an absolute
    /// (a `$` part) keeps the coordinate it names.
    tree: Expr,
    /// Heap cost of the tree, counted once instead of once per cell: `size_of::<Expr>()` a node,
    /// the figure the store's accounting and the undo budget read.
    pub bytes: u32,
    /// Smallest and largest offset over the *relative* parts of each axis. A cell at `at` resolves
    /// without clamping iff `at + min >= 0` and `at + max < MAX` on both axes, which is what makes
    /// [`Shape::fits`] O(1). Both are 0 for a shape with no relative part, which never clamps.
    pub min_row: i32,
    pub max_row: i32,
    pub min_col: i32,
    pub max_col: i32,
    /// Whether a file spells this formula differently from [`print`]. A property of the shape's
    /// syntax, so a save never resolves a tree to decide how to write it — see
    /// [`file_spelling_differs`] and `xlsx::fmla::file_text`.
    pub file_rewrite: bool,
}

/// What a structural edit makes of a shape (S5 of `plan/SHAPE-SHARING.md`).
#[derive(Debug)]
pub enum Adjusted {
    /// No reference the edit touches moves: every cell keeps its tree and its text.
    Unchanged,
    /// One new tree serves every cell the shape holds, and each cell's text is its print at that
    /// cell — which changes for every cell that moved, because a relative part is spelled from
    /// the cell's own position.
    Moved(Shape),
    /// No shape-level answer: the edit moves a reference differently for different cells (see
    /// [`crate::adjust::Adjusted::PerCell`]). The caller materialises per cell instead.
    PerCell,
}

impl Shape {
    /// Canonicalises the absolute tree a parser produced for the cell at `at`.
    pub fn from_expr(e: Expr, at: CellRef) -> Shape {
        Shape::from_canonical(e.map(&mut |x| match x {
            Expr::Ref(r) => Expr::Ref(r.offset_at(at)),
            other => other,
        }))
    }

    /// The shape a **canonical** tree is: `bytes`, the offset bounds and the syntax half of
    /// `file_rewrite` are facts about the tree and nothing else. The rewrites below work on
    /// canonical trees, so their answers are built here rather than canonicalised a second time.
    pub fn from_canonical(tree: Expr) -> Shape {
        let bytes = tree_bytes(&tree);
        let file_rewrite = file_spelling_differs(&tree);
        let (min_row, max_row, min_col, max_col) = bounds_of(&tree);
        Shape { tree, bytes, min_row, max_row, min_col, max_col, file_rewrite }
    }

    /// The intern key: the shape's R1C1 spelling seen from A1, which **is** the shape (§5.1 of
    /// `plan/SHAPE-SHARING.md`). Two shapes with one spelling are one shape, so a rewrite compares
    /// this to say whether it changed anything.
    pub fn spelling(&self) -> String {
        print_r1c1(&self.tree, CellRef::new(0, 0))
    }

    /// The shape a rewritten canonical tree is, or `None` when the rewrite leaves the shape as it
    /// was (one spelling, so one intern key and one text per cell).
    ///
    /// `file_rewrite` travels with the tree: a rewrite that moves names or coordinates keeps the
    /// syntax of the shape, and the `_xlfn.` half of the answer lives in `cella-xlsx`, above
    /// this crate.
    pub fn rewritten(&self, tree: Expr) -> Option<Shape> {
        let mut shape = Shape::from_canonical(tree);
        shape.file_rewrite |= self.file_rewrite;
        (shape.spelling() != self.spelling()).then_some(shape)
    }

    /// The shape a structural edit makes of this one, reused by every cell that holds it (S5 of
    /// `plan/SHAPE-SHARING.md`): one tree rewrite per shape instead of one per cell.
    ///
    /// `host` is the sheet the cells live on and `target` the sheet the edit is on — an unqualified
    /// reference belongs to its own sheet, so the same shape on two sheets can answer differently.
    /// `cell` says whether the cell taking this answer keeps its line or moves ([`CellMove`]): a
    /// shape holds cells on both sides of an insert, and a reference the edit leaves alone is one
    /// line further away from a cell that moved.
    pub fn adjust(&self, host: &str, target: &str, edit: &Edit, cell: CellMove) -> Adjusted {
        match adjust_canonical(&self.tree, host, target, edit, cell) {
            Canonical::Moved(tree) => self.rewritten(tree).map_or(Adjusted::Unchanged, Adjusted::Moved),
            Canonical::Unchanged => Adjusted::Unchanged,
            Canonical::PerCell => Adjusted::PerCell,
        }
    }

    /// Whether the lines `edit` moves a reference of this shape differently from the cell it lives
    /// in, at `old` — the cell's position **before** the edit. Then the shape-level answer
    /// ([`Shape::adjust`]) is not this cell's: the reference moved while the cell stayed, or the
    /// other way round, so the offset itself must change and the cell needs a shape of its own.
    ///
    /// O(1) from the stored offset bounds. An edit moves every coordinate at or after `at` on its
    /// axis — and swallows the band `[at, at + count)` — so a shape whose whole offset interval
    /// falls in the same class as the cell is served by one answer; one that spans the boundary
    /// answers "per cell". The test is conservative: never the other way round.
    pub fn straddles(&self, old: CellRef, edit: &Edit) -> bool {
        let (axis, at, count, insert) = match edit {
            Edit::Insert { axis, at, count } => (*axis, *at, *count, true),
            Edit::Delete { axis, at, count } => (*axis, *at, *count, false),
            // A cut moves one block's references and nothing else's: where the cell is decides.
            _ => return true,
        };
        let (x, lo, hi) = match axis {
            Axis::Rows => (i64::from(old.row), i64::from(self.min_row), i64::from(self.max_row)),
            Axis::Cols => (i64::from(old.col), i64::from(self.min_col), i64::from(self.max_col)),
        };
        let (at, count) = (i64::from(at), i64::from(count));
        // What the edit does to a coordinate: 0 below its band (it stays), 1 inside a deleted band
        // (it is gone), 2 at or past it (it moves). An insert's band is empty.
        let place = |v: i64| -> i8 {
            if v >= at && (insert || v >= at + count) {
                2
            } else if v >= at {
                1
            } else {
                0
            }
        };
        let here = place(x);
        here == 1 || place(x.saturating_add(lo)) != here || place(x.saturating_add(hi)) != here
    }

    /// The canonical tree: structure, sizes and names. Its references are offsets, so a caller that
    /// reads *positions* must use [`Shape::resolve`] (or [`Reference::range_at`]); a caller that
    /// reads *sizes* (an array formula's width and height) may read this one.
    pub fn tree(&self) -> &Expr {
        &self.tree
    }

    /// The absolute tree this cell reads: exactly what `Formula::expr()` returns today. `None`
    /// when a relative part falls off the sheet, which the caller renders `#REF!`.
    pub fn resolve(&self, at: CellRef) -> Option<Expr> {
        resolve_copy(&self.tree, at)
    }

    /// [`crate::print`] of the resolved tree: the cell's own text, i.e. invariant §4.3 of the
    /// design. Printed in one walk without building the tree — a structural edit does this once per
    /// cell it moves (S5 of `plan/SHAPE-SHARING.md`), where it used to resolve, adjust, print and
    /// intern.
    pub fn print_at(&self, at: CellRef) -> Option<String> {
        self.fits(at).then(|| print_at(&self.tree, at))
    }

    /// Whether this cell can share the shape unchanged: no reference of it clamps at `at`. Decided
    /// from the stored offset bounds, not by walking the tree.
    pub fn fits(&self, at: CellRef) -> bool {
        let (row, col) = (i64::from(at.row), i64::from(at.col));
        row + i64::from(self.min_row) >= 0
            && row + i64::from(self.max_row) < i64::from(MAX_ROWS)
            && col + i64::from(self.min_col) >= 0
            && col + i64::from(self.max_col) < i64::from(MAX_COLS)
    }
}

/// Bytes a canonical tree holds, node by node: the figure the undo budget charges, once per
/// shape instead of once per cell (S6/S7 of `plan/SHAPE-SHARING.md`).
fn tree_bytes(e: &Expr) -> u32 {
    let mut bytes = 0u32;
    e.walk(&mut |_| bytes = bytes.saturating_add(std::mem::size_of::<Expr>() as u32));
    bytes
}

/// Whether a file writes `e` differently from [`print`]: the syntax half of
/// `xlsx::fmla::needs_file_rewrite`.
///
/// The other half — the `_xlfn.` / `_xlws.` spelling of newer function names — is a file-format
/// table that lives in `cella-xlsx` (this crate is below it, see `cargo xtask layers`), so a
/// reader or writer that interns a shape ORs that answer into [`Shape::file_rewrite`] before
/// interning (S3 of `plan/SHAPE-SHARING.md`). Neither half depends on the cell's position, which
/// is why the answer belongs to the shape.
fn file_spelling_differs(e: &Expr) -> bool {
    let mut need = false;
    e.walk(&mut |x| match x {
        // `LET`/`LAMBDA` declare parameters its calls reference, so the file needs the `_xlpm.`
        // prefix built.
        Expr::Call(name, _) if name == "LET" || name == "LAMBDA" => need = true,
        // Files have no `#`, `@` or trim-reference operator: they spell them as calls.
        Expr::Unary(UnOp::Spill | UnOp::At | UnOp::Trim(_), _) => need = true,
        // Only the `[@Col]` shorthand prints differently; every other structured reference is
        // already written the long way.
        Expr::Struct(s) if s.specifiers.len() == 1 && s.specifiers.first() == Some(&StructItem::ThisRow) => {
            need = true;
        }
        _ => {}
    });
    need
}

/// The offset bounds of the relative parts of a *canonical* tree (relative parts are offsets there).
/// Both ends start at 0 — an offset of 0 clamps nowhere, so a shape with no relative part of an
/// axis never reports a clamp on it.
fn bounds_of(tree: &Expr) -> (i32, i32, i32, i32) {
    let (mut min_row, mut max_row, mut min_col, mut max_col) = (0i32, 0i32, 0i32, 0i32);
    let mut see = |v: i32, abs: bool, row: bool| {
        if abs {
            return;
        }
        let (min, max) = if row { (&mut min_row, &mut max_row) } else { (&mut min_col, &mut max_col) };
        *min = (*min).min(v);
        *max = (*max).max(v);
    };
    for r in tree.references() {
        match &r.kind {
            RefKind::Cell(a) => {
                see(a.row, a.row_abs, true);
                see(a.col, a.col_abs, false);
            }
            RefKind::Range(a, b) => {
                see(a.row, a.row_abs, true);
                see(a.col, a.col_abs, false);
                see(b.row, b.row_abs, true);
                see(b.col, b.col_abs, false);
            }
            RefKind::Rows(r0, a0, r1, a1) => {
                see(*r0, *a0, true);
                see(*r1, *a1, true);
            }
            RefKind::Cols(c0, a0, c1, a1) => {
                see(*c0, *a0, false);
                see(*c1, *a1, false);
            }
        }
    }
    (min_row, max_row, min_col, max_col)
}

/// [`Shape::resolve`] in one pass over a borrowed tree. The same discipline as
/// `adjust::shift_relative_copy`: an operator chain as long as Excel allows follows its left spine
/// in a loop, so it does not recurse once per operand. `None` as soon as a relative part falls off
/// the sheet.
fn resolve_copy(e: &Expr, at: CellRef) -> Option<Expr> {
    let mut spine: Vec<(BinOp, &Expr)> = Vec::new();
    let mut node = e;
    while let Expr::Binary(op, a, b) = node {
        spine.push((*op, b));
        node = a;
    }
    let mut acc = resolve_node(node, at)?;
    while let Some((op, b)) = spine.pop() {
        acc = Expr::Binary(op, Box::new(acc), Box::new(resolve_copy(b, at)?));
    }
    Some(acc)
}

fn resolve_node(e: &Expr, at: CellRef) -> Option<Expr> {
    let child = |x: &Expr| resolve_copy(x, at).map(Box::new);
    let out = match e {
        Expr::Number(n) => Expr::Number(*n),
        Expr::Text(t) => Expr::Text(Arc::clone(t)),
        Expr::Bool(b) => Expr::Bool(*b),
        Expr::Error(x) => Expr::Error(*x),
        Expr::Array(rows) => {
            let rows = rows.iter().map(|r| r.iter().map(|x| resolve_copy(x, at)).collect()).collect::<Option<Vec<Vec<Expr>>>>()?;
            Expr::Array(rows)
        }
        Expr::Ref(r) => Expr::Ref(resolve_ref(r, at)?),
        Expr::Name(n) => Expr::Name(n.clone()),
        Expr::Struct(s) => Expr::Struct(s.clone()),
        Expr::Unary(op, x) => Expr::Unary(*op, child(x)?),
        Expr::Binary(op, a, b) => Expr::Binary(*op, child(a)?, child(b)?),
        Expr::Call(n, args) => {
            let args = args.iter().map(|x| resolve_copy(x, at)).collect::<Option<Vec<Expr>>>()?;
            Expr::Call(n.clone(), args)
        }
        Expr::Invoke(c, args) => {
            let args = args.iter().map(|x| resolve_copy(x, at)).collect::<Option<Vec<Expr>>>()?;
            Expr::Invoke(child(c)?, args)
        }
        Expr::Missing => Expr::Missing,
        Expr::Paren(x) => Expr::Paren(child(x)?),
    };
    Some(out)
}

fn resolve_ref(r: &Reference, at: CellRef) -> Option<Reference> {
    let kind = match &r.kind {
        RefKind::Cell(a) => RefKind::Cell(a.resolved(at)?),
        RefKind::Range(a, b) => RefKind::Range(a.resolved(at)?, b.resolved(at)?),
        RefKind::Rows(r0, a0, r1, a1) => RefKind::Rows(line_at(*r0, *a0, at.row, MAX_ROWS)?, *a0, line_at(*r1, *a1, at.row, MAX_ROWS)?, *a1),
        RefKind::Cols(c0, a0, c1, a1) => RefKind::Cols(line_at(*c0, *a0, at.col, MAX_COLS)?, *a0, line_at(*c1, *a1, at.col, MAX_COLS)?, *a1),
    };
    Some(Reference { sheet: r.sheet.clone(), kind })
}

/// Where shapes are deduplicated: the intern key is the shape's R1C1 spelling seen from A1, which
/// *is* the shape (`print_r1c1(canonical, A1)` prints the canonical tree's offsets and absolute
/// parts without adding a base of its own).
///
/// One table belongs to a load or a session, **never** to a `static`: entries are [`Weak`], so a
/// shape dies with the last cell that holds it — no leak, and no sharing across documents, which
/// keeps `Arc::ptr_eq` meaningful inside one workbook.
#[derive(Default)]
pub struct ShapeTable {
    map: HashMap<String, Weak<Shape>>,
}

/// Dead entries a table may hold before a new shape prunes them: a long session that rewrites
/// formulas would otherwise keep one key string for every shape it ever made.
const PRUNE_AT: usize = 1024;

impl ShapeTable {
    /// The one `Arc` for `shape`'s R1C1 spelling, interning it when no live shape has it yet.
    pub fn intern(&mut self, shape: Shape) -> Arc<Shape> {
        let key = print_r1c1(&shape.tree, CellRef::new(0, 0));
        if let Some(live) = self.map.get(&key).and_then(Weak::upgrade) {
            return live;
        }
        if self.map.len() >= PRUNE_AT {
            self.map.retain(|_, w| w.strong_count() > 0);
        }
        let shape = Arc::new(shape);
        self.map.insert(key, Arc::downgrade(&shape));
        shape
    }

    /// Live shapes held by the table: the measurement S3 of `plan/SHAPE-SHARING.md` reports.
    pub fn len(&self) -> usize {
        self.map.values().filter(|w| w.strong_count() > 0).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adjust::shift_relative_copy;
    use crate::parse;
    use crate::printer::print;

    /// One of everything the syntax can hold: absolute and relative cells, mixed `$` parts, whole
    /// rows and columns, 3-D spans, structured references, names, array constants, trim references
    /// and the sheet's own corners.
    const CORPUS: &[&str] = &[
        "1+2*3",
        "B2*C2+D2",
        "SUM($G$2:G2)",
        "SUM(A1:B2)",
        "SUM($A$1:B$2,C3)",
        "IF(A1>0,\"yes\",\"no\")",
        "-2^2",
        "A1%",
        "Sheet2!A1+'My Sheet'!$B$3",
        "SUM(Sheet1:Sheet3!A1)",
        "SUM(Sheet1:Sheet3!$A$1)",
        "SUM(A:A)",
        "SUM(3:5)",
        "SUM($A1)",
        "SUM(A$1)",
        "B:$C",
        "$3:4",
        "1:$1048576",
        "$A:XFD",
        "{1,2;3,4}",
        "{-1,\"a\",TRUE,#N/A}",
        "VLOOKUP(A1,Table1[#All],2,FALSE)",
        "Table1[Sales]",
        "Table1[@Sales]",
        "Table1[[#Headers],[A]:[C]]",
        "\"a\"\"b\"&C1",
        "SUM((A1:A3,C1:C3))",
        "A1:C3 B2:B5",
        "LAMBDA(x,x+1)(2)",
        "LET(x,1,x+1)",
        "F(1,,3)",
        "@A1:A10",
        "A1#",
        "Sheet1!$D$1#",
        "A1<>B1",
        "#REF!+1",
        "MyName*2",
        "Sheet1!MyName",
        "SUM(A1:.B3)+SUM(A.:.B)",
        "XFD1048576",
        "$A$1",
        "A1",
    ];

    fn parsed(src: &str) -> Expr {
        parse(src).unwrap_or_else(|e| panic!("{src}: {e}"))
    }

    /// Positions a formula is entered at or read at: corners, edges, the middle and the sheet's own
    /// last cell.
    fn positions() -> Vec<CellRef> {
        let mut v = Vec::new();
        for &(r, c) in
            &[(0u32, 0u32), (0, 1), (1, 0), (1, 1), (1, 6), (1, 7), (5, 3), (1023, 64), (MAX_ROWS - 2, MAX_COLS - 2), (MAX_ROWS - 1, MAX_COLS - 1)]
        {
            v.push(CellRef::new(r, c));
        }
        v
    }

    /// Positions around every edge, where a relative part clamps.
    fn edge_positions() -> Vec<CellRef> {
        let rows = [0, 1, 2, 3, MAX_ROWS - 3, MAX_ROWS - 2, MAX_ROWS - 1];
        let cols = [0, 1, 2, 3, MAX_COLS - 3, MAX_COLS - 2, MAX_COLS - 1];
        let mut v = Vec::new();
        for &r in &rows {
            for &c in &cols {
                v.push(CellRef::new(r, c));
            }
        }
        v
    }

    /// A parsed tree resolves back to itself at the cell it was parsed for, and fits there: the
    /// canonicalisation is exactly invertible for a tree that came off the sheet.
    #[test]
    fn a_shape_resolves_to_the_tree_it_was_made_from() {
        for src in CORPUS {
            let e = parsed(src);
            for at in positions() {
                let sh = Shape::from_expr(e.clone(), at);
                assert_eq!(sh.resolve(at), Some(e.clone()), "{src} at {at:?}");
                assert!(sh.fits(at), "{src} at {at:?}");
            }
        }
    }

    /// `print_at` is `print(resolve(at))` — invariant §4.3 of the design: the cell's text is the
    /// print of its tree at its position, and nothing here changes it.
    #[test]
    fn a_shape_prints_the_text_of_every_cell_it_serves() {
        for src in CORPUS {
            let e = parsed(src);
            for made_at in positions() {
                let sh = Shape::from_expr(e.clone(), made_at);
                for at in positions() {
                    let Some(tree) = sh.resolve(at) else { continue };
                    assert_eq!(sh.print_at(at), Some(print(&tree)), "{src} made at {made_at:?}, read at {at:?}");
                    // At the cell it was entered in, the shape prints exactly the text it came from.
                    if at == made_at {
                        assert_eq!(sh.print_at(at), Some(print(&e)), "{src} at {at:?}");
                    }
                }
            }
        }
    }

    /// The intern key is the shape: the canonical tree printed as R1C1 from A1 is the tree printed
    /// as R1C1 at the cell it was entered in. (R1C1 *is* the shape representation — see §3.1.)
    #[test]
    fn the_r1c1_spelling_of_a_shape_is_its_position_spelling() {
        for src in CORPUS {
            let e = parsed(src);
            for made_at in positions() {
                let sh = Shape::from_expr(e.clone(), made_at);
                assert_eq!(print_r1c1(sh.tree(), CellRef::new(0, 0)), print_r1c1(&e, made_at), "{src} made at {made_at:?}");
            }
        }
    }

    /// The reference workbook's G family: 1,000 rows of `=B2*C2+D2` filled down intern to one
    /// `Arc<Shape>`, and so do 1,000 rows of the H family `=SUM($G$2:G2)` — which is one shape in
    /// R1C1 even though every row has a different absolute tree.
    #[test]
    fn a_fill_of_a_thousand_rows_interns_one_shape() {
        let cases = [
            ("B2*C2+D2", CellRef::new(1, 6), (1u32, 6u32), "RC[-5]*RC[-4]+RC[-3]"),
            ("SUM($G$2:G2)", CellRef::new(1, 7), (1, 7), "SUM(R2C7:RC[-1])"),
        ];
        for (src, at, (row, col), r1c1) in cases {
            let first_cell = parsed(src);
            let mut table = ShapeTable::default();
            let mut first: Option<Arc<Shape>> = None;
            for dr in 0..1_000u32 {
                let cell = CellRef::new(row + dr, col);
                // What a fill does today: the same tree moved down by dr.
                let tree = shift_relative_copy(&first_cell, i64::from(dr), 0);
                let shape = table.intern(Shape::from_expr(tree, cell));
                match &first {
                    None => first = Some(shape),
                    Some(a) => assert!(Arc::ptr_eq(a, &shape), "{src} at row {dr}"),
                }
            }
            let Some(first) = first else { panic!("{src}: no shapes") };
            assert_eq!(table.len(), 1, "{src}");
            assert_eq!(print_r1c1(first.tree(), CellRef::new(0, 0)), r1c1, "{src}");
            assert!(first.print_at(at).is_some(), "{src}");
        }
    }

    /// The O(1) `fits` is the brute-force answer: resolving every reference and asking whether it is
    /// on the sheet. Checked over every edge of the sheet, with mixed `$A1`/`A$1` forms and whole
    /// rows and columns among the corpus.
    #[test]
    fn fits_agrees_with_resolving_every_edge() {
        for src in CORPUS {
            let e = parsed(src);
            for made_at in positions() {
                let sh = Shape::from_expr(e.clone(), made_at);
                for at in edge_positions() {
                    assert_eq!(sh.fits(at), sh.resolve(at).is_some(), "{src} made at {made_at:?}, read at {at:?}");
                }
            }
        }
    }

    /// A relative part that would fall off the sheet makes the shape not fit, and the copy that
    /// lands there is a `#REF!` cell — the same outcome today's `shift_relative` gives.
    #[test]
    fn a_relative_part_that_leaves_the_sheet_does_not_fit() {
        let sh = Shape::from_expr(parsed("A1+$A$1"), CellRef::new(2, 2));
        assert!(sh.fits(CellRef::new(2, 2)));
        assert!(!sh.fits(CellRef::new(1, 2)), "one row up puts the relative part off the sheet");
        assert!(!sh.fits(CellRef::new(2, 1)));
        assert!(sh.fits(CellRef::new(MAX_ROWS - 1, MAX_COLS - 1)));
        assert_eq!(sh.resolve(CellRef::new(1, 2)), None);
        // An absolute part clamps nothing: a shape of absolute references only always fits.
        let abs = Shape::from_expr(parsed("$A$1+$XFD$1048576"), CellRef::new(1, 1));
        for at in [CellRef::new(1, 1), CellRef::new(0, 0), CellRef::new(MAX_ROWS - 1, MAX_COLS - 1)] {
            assert!(abs.fits(at), "{at:?}");
        }
    }

    /// Names, structured references and 3-D spans carry no coordinates: canonicalisation must leave
    /// them exactly as they are (trap 8 of the design — `@` means "the formula's row", and a sheet
    /// name is not a position).
    #[test]
    fn names_structs_and_spans_are_not_coordinates() {
        for src in ["MyName*2", "Sheet1!MyName", "Table1[@Sales]", "SUM(Sales[Units])", "SUM(Sheet1:Sheet3!A1)"] {
            let e = parsed(src);
            for at in positions() {
                let sh = Shape::from_expr(e.clone(), at);
                assert_eq!(sh.resolve(at), Some(e.clone()), "{src} at {at:?}");
            }
        }
        let sh = Shape::from_expr(parsed("SUM(Sheet1:Sheet3!A1)"), CellRef::new(3, 3));
        assert!(sh.tree().references().iter().any(|r| matches!(r.sheet, crate::ast::SheetSel::Span(..))), "the 3-D span survives canonicalisation");
    }

    /// A legacy array formula's tree canonicalises and resolves like any other: `array` and
    /// `dynamic` stay per-cell facts on `Formula`, which nothing here touches. The sizes a writer
    /// asks the tree for are translation-invariant.
    #[test]
    fn an_array_formula_tree_canonicalises_like_any_other() {
        for src in ["{1,2;3,4}", "SUM(A1:A3*B1:B3)", "A1:A3*2"] {
            let e = parsed(src);
            for at in positions() {
                let sh = Shape::from_expr(e.clone(), at);
                assert_eq!(sh.resolve(at), Some(e.clone()), "{src} at {at:?}");
            }
        }
        let sh = Shape::from_expr(parsed("A1:A3*2"), CellRef::new(4, 4));
        let RefKind::Range(a, b) = &sh.tree().references()[0].kind else { panic!("expected a range") };
        assert_eq!((b.row - a.row, b.col - a.col), (2, 0));
    }

    /// The positions a cell can be moved by the edits below, before and after: the sheet's corners,
    /// the lines a fill lives on, and the edges.
    fn moved_by(at: CellRef, edit: &Edit) -> Option<CellRef> {
        let (axis, from, count, insert) = match edit {
            Edit::Insert { axis, at: a, count } => (*axis, *a, *count, true),
            Edit::Delete { axis, at: a, count } => (*axis, *a, *count, false),
            _ => return None,
        };
        let max = if axis == Axis::Rows { MAX_ROWS } else { MAX_COLS };
        let main = if axis == Axis::Rows { at.row } else { at.col };
        let after = if insert {
            if main >= from { main.checked_add(count)? } else { main }
        } else if main < from {
            main
        } else if main >= from.checked_add(count)? {
            main - count
        } else {
            return None;
        };
        if after >= max {
            return None;
        }
        Some(if axis == Axis::Rows { CellRef::new(after, at.col) } else { CellRef::new(at.row, after) })
    }

    /// The edits a structural command makes, at the sheet's start, its middle and its edges.
    fn line_edits() -> Vec<Edit> {
        let mut v = Vec::new();
        for axis in [Axis::Rows, Axis::Cols] {
            let max = if axis == Axis::Rows { MAX_ROWS } else { MAX_COLS };
            for at in [0, 1, 2, 3, 5, max - 2, max - 1] {
                for count in [1, 2, 5] {
                    v.push(Edit::Insert { axis, at, count });
                    v.push(Edit::Delete { axis, at, count });
                }
            }
        }
        v
    }

    /// A cell's position after moving it and back, for the cells a shape serves.
    fn served_cells() -> Vec<CellRef> {
        let mut v = Vec::new();
        for &r in &[0u32, 1, 2, 3, 4, 5, 6, 7, 1023, MAX_ROWS - 3, MAX_ROWS - 2, MAX_ROWS - 1] {
            for &c in &[0u32, 1, 2, 6, 7, MAX_COLS - 1] {
                v.push(CellRef::new(r, c));
            }
        }
        v
    }

    /// **The property S5 needs** (§4.11 of `plan/SHAPE-SHARING.md`): the shape-level answer is the
    /// per-cell answer for every cell `Shape::straddles` lets it serve — the text the cell spells
    /// is `print(adjust(resolve(shape, old)))`, and the shape the cell keeps is that tree
    /// canonicalised at its new position. Where the gate says "per cell", only that cell's own
    /// materialisation is right, and the engine makes it.
    #[test]
    fn the_shape_level_adjust_is_the_per_cell_adjust() {
        let edits = line_edits();
        let cells = served_cells();
        let mut checked = 0usize;
        for src in CORPUS {
            let e = parsed(src);
            for made_at in positions() {
                let sh = Shape::from_expr(e.clone(), made_at);
                for (host, target) in [("Sheet1", "Sheet1"), ("Other", "Sheet1"), ("Sheet1", "Other")] {
                    for edit in &edits {
                        for old in &cells {
                            let Some(old_tree) = sh.resolve(*old) else { continue };
                            // A cell moves only when the edit is on its own sheet.
                            let new = if host == target {
                                let Some(new) = moved_by(*old, edit) else { continue };
                                new
                            } else {
                                *old
                            };
                            if sh.straddles(*old, edit) {
                                continue;
                            }
                            let cell = if new == *old { CellMove::Stays } else { CellMove::Moves };
                            let level = sh.adjust(host, target, edit, cell);
                            let want = crate::adjust::adjust(old_tree, host, target, edit);
                            let at_new = match &level {
                                Adjusted::Unchanged => &sh,
                                Adjusted::Moved(s) => s,
                                Adjusted::PerCell => continue,
                            };
                            // A shape that would clamp at the cell it moved to is the per-cell
                            // path's case too: it materialises the `#REF!` the cell then spells.
                            if !at_new.fits(new) {
                                continue;
                            }
                            assert_eq!(
                                at_new.print_at(new),
                                Some(print(&want)),
                                "{src} made at {made_at:?} on {host}/{target} after {edit:?}, cell {old:?}"
                            );
                            assert_eq!(
                                at_new.spelling(),
                                Shape::from_expr(want, new).spelling(),
                                "{src} made at {made_at:?} on {host}/{target} after {edit:?}, cell {old:?}: the shape"
                            );
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert!(checked > 20_000, "{checked} cases checked");
    }

    /// The shape counts its tree once (a node is `size_of::<Expr>()`), which the undo budget and
    /// the store's accounting read.
    #[test]
    fn a_shape_counts_its_tree_once() {
        let node = std::mem::size_of::<Expr>() as u32;
        // Add(Mul(B2, C2), D2): five nodes, four of them children in their parent's box.
        assert_eq!(Shape::from_expr(parsed("B2*C2+D2"), CellRef::new(1, 6)).bytes, 5 * node);
        // Call("SUM", [Ref]): two.
        assert_eq!(Shape::from_expr(parsed("SUM($G$2:G2)"), CellRef::new(1, 7)).bytes, 2 * node);
    }

    /// Whether a file spells the formula differently is a syntax property of the shape, decided once.
    #[test]
    fn a_file_rewrite_is_a_property_of_the_shape() {
        let sh = |src: &str| Shape::from_expr(parsed(src), CellRef::new(1, 1)).file_rewrite;
        assert!(!sh("B2*C2+D2"));
        assert!(!sh("SUM(Table1[Sales])"));
        assert!(!sh("Table1[[#Headers],[A]:[C]]"));
        assert!(sh("LET(x,1,x+1)"));
        assert!(sh("LAMBDA(x,x+1)(2)"));
        assert!(sh("SUM(A1#)"));
        assert!(sh("@A1:A3"));
        assert!(sh("SUM(A1:.B3)"));
        assert!(sh("Table1[@Sales]"));
    }

    /// Shapes live in a table of weak entries, so a shape dies with the last cell that holds it: no
    /// leak, no sharing across documents, and `Arc::ptr_eq` keeps meaning "the same shape".
    #[test]
    fn a_shape_dies_with_the_last_cell_that_holds_it() {
        let mut table = ShapeTable::default();
        assert!(table.is_empty());
        let one = table.intern(Shape::from_expr(parsed("B2*C2+D2"), CellRef::new(1, 6)));
        let again = table.intern(Shape::from_expr(parsed("B2*C2+D2"), CellRef::new(1, 6)));
        assert!(Arc::ptr_eq(&one, &again));
        assert_eq!(table.len(), 1);
        // A different shape is its own entry.
        let other = table.intern(Shape::from_expr(parsed("B2*C2+D3"), CellRef::new(1, 6)));
        assert!(!Arc::ptr_eq(&one, &other));
        assert_eq!(table.len(), 2);
        drop((one, again, other));
        assert_eq!(table.len(), 0);
        assert!(table.is_empty());
    }
}
