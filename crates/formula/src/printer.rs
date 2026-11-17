//! [`Expr`] → formula text (without `=`), A1 or R1C1.

use std::fmt::Write;

use gridcraft_core::{CellRef, number_to_text};

use crate::ast::{Anchor, BinOp, Expr, RefKind, Reference, SheetSel, StructItem, StructRef, UnOp};

pub use gridcraft_core::{quote_sheet, sheet_needs_quotes as needs_quotes};

fn sheet_prefix(s: &SheetSel) -> String {
    match s {
        SheetSel::Current => String::new(),
        SheetSel::Named(n) => format!("{}!", quote_sheet(n)),
        SheetSel::Span(a, b) => {
            let joined = format!("{a}:{b}");
            if needs_quotes(a) || needs_quotes(b) { format!("'{}'!", joined.replace('\'', "''")) } else { format!("{joined}!") }
        }
    }
}

/// A signed coordinate as a sheet index. Only an absolute tree reaches A1 printing (invariant 4
/// of SHAPE-SHARING.md: a shape is never printed with `print`), so the fallback never runs; it
/// keeps a misuse printing a valid cell instead of a 20-digit number.
fn on_sheet(v: i32) -> u32 {
    u32::try_from(v).map_or(0, |v| v)
}

/// One coordinate of an A1 anchor: an absolute part is the coordinate it names, a relative one is
/// `base` plus its offset — which is what a **canonical** (shape) tree stores (§5.1 of
/// `plan/SHAPE-SHARING.md`). `base` is A1 for an absolute tree, so a relative part of one prints
/// its own coordinate there.
fn resolved(v: i32, abs: bool, base: u32) -> u32 {
    if abs {
        return on_sheet(v);
    }
    on_sheet(i32::try_from(i64::from(v) + i64::from(base)).unwrap_or(i32::MAX))
}

fn push_anchor_a1(s: &mut String, a: &Anchor, base: CellRef) {
    if a.col_abs {
        s.push('$');
    }
    push_col(s, resolved(a.col, a.col_abs, base.col));
    if a.row_abs {
        s.push('$');
    }
    let _ = write!(s, "{}", u64::from(resolved(a.row, a.row_abs, base.row)) + 1);
}

/// Appends [`gridcraft_core::col_to_letters`]`(col)` without allocating.
fn push_col(s: &mut String, col: u32) {
    let mut letters = [0u8; 8];
    let mut i = letters.len();
    let mut n = col as u64 + 1;
    while n > 0 && i > 0 {
        i -= 1;
        if let Some(l) = letters.get_mut(i) {
            *l = b'A' + ((n - 1) % 26) as u8;
        }
        n = (n - 1) / 26;
    }
    for &l in letters.get(i..).unwrap_or_default() {
        s.push(char::from(l));
    }
}

pub fn reference_a1(r: &Reference) -> String {
    range_a1(r, ":")
}

/// A1 text with `sep` between the two ends of a range (`:` or a trim operator).
fn range_a1(r: &Reference, sep: &str) -> String {
    let mut s = String::new();
    push_range_a1(&mut s, r, sep, CellRef::new(0, 0));
    s
}

/// Appends [`range_a1`]`(r, sep)` to `s`. `base` resolves a relative part (`None` for an absolute
/// tree, whose relative parts are the coordinates they name).
fn push_range_a1(s: &mut String, r: &Reference, sep: &str, base: CellRef) {
    match &r.sheet {
        SheetSel::Current => {}
        SheetSel::Named(n) => {
            s.push_str(&quote_sheet(n));
            s.push('!');
        }
        sheet @ SheetSel::Span(_, _) => s.push_str(&sheet_prefix(sheet)),
    }
    let dollar = |abs: bool| if abs { "$" } else { "" };
    match &r.kind {
        RefKind::Cell(a) => push_anchor_a1(s, a, base),
        RefKind::Range(a, b) => {
            push_anchor_a1(s, a, base);
            s.push_str(sep);
            push_anchor_a1(s, b, base);
        }
        RefKind::Rows(r0, a0, r1, a1) => {
            let (x, y) = (resolved(*r0, *a0, base.row), resolved(*r1, *a1, base.row));
            let _ = write!(s, "{}{}{sep}{}{}", dollar(*a0), u64::from(x) + 1, dollar(*a1), u64::from(y) + 1);
        }
        RefKind::Cols(c0, a0, c1, a1) => {
            s.push_str(dollar(*a0));
            push_col(s, resolved(*c0, *a0, base.col));
            s.push_str(sep);
            s.push_str(dollar(*a1));
            push_col(s, resolved(*c1, *a1, base.col));
        }
    }
}

/// One R1C1 part. An absolute part is a coordinate, a relative one an offset from `base` — the
/// shape's own representation, which is why `print_r1c1(canonical, A1)` is its intern key.
fn r1c1_part(letter: char, v: i32, abs: bool, base: u32) -> String {
    if abs {
        format!("{letter}{}", u64::from(on_sheet(v)) + 1)
    } else {
        let d = i64::from(v) - i64::from(base);
        if d == 0 { letter.to_string() } else { format!("{letter}[{d}]") }
    }
}

fn anchor_r1c1(a: &Anchor, at: CellRef) -> String {
    format!("{}{}", r1c1_part('R', a.row, a.row_abs, at.row), r1c1_part('C', a.col, a.col_abs, at.col))
}

pub fn reference_r1c1(r: &Reference, at: CellRef) -> String {
    range_r1c1(r, at, ":")
}

/// R1C1 text with `sep` between the two ends of a range; a trim operator never collapses
/// `R1:R1` to `R1`.
fn range_r1c1(r: &Reference, at: CellRef, sep: &str) -> String {
    let body = match &r.kind {
        RefKind::Cell(a) => anchor_r1c1(a, at),
        RefKind::Range(a, b) => format!("{}{sep}{}", anchor_r1c1(a, at), anchor_r1c1(b, at)),
        RefKind::Rows(r0, a0, r1, a1) => {
            let (x, y) = (r1c1_part('R', *r0, *a0, at.row), r1c1_part('R', *r1, *a1, at.row));
            if x == y && sep == ":" { x } else { format!("{x}{sep}{y}") }
        }
        RefKind::Cols(c0, a0, c1, a1) => {
            let (x, y) = (r1c1_part('C', *c0, *a0, at.col), r1c1_part('C', *c1, *a1, at.col));
            if x == y && sep == ":" { x } else { format!("{x}{sep}{y}") }
        }
    };
    format!("{}{}", sheet_prefix(&r.sheet), body)
}

pub fn struct_ref(s: &StructRef) -> String {
    write_struct_ref(s, true)
}

/// Structured reference as files spell it: `[@Col]` is only Excel's display shorthand, files
/// hold `[[#This Row],[Col]]` (and `[#This Row]` for `[@]`).
pub fn struct_ref_file(s: &StructRef) -> String {
    write_struct_ref(s, false)
}

/// Splits a sheet-qualified name at its last `!` outside brackets. A `!` inside brackets
/// belongs to a structured reference's column (`Table1[Done!]`, `Table1[#REF!]`).
fn sheet_split(n: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    let mut escaped = false;
    let mut at = None;
    for (i, c) in n.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\'' if depth > 0 => escaped = true,
            '[' => depth = depth.saturating_add(1),
            ']' => depth = depth.saturating_sub(1),
            '!' if depth == 0 => at = Some(i),
            _ => {}
        }
    }
    at.and_then(|i| Some((n.get(..i)?, n.get(i + 1..)?)))
}

fn write_struct_ref(s: &StructRef, at_shorthand: bool) -> String {
    let esc = |c: &str| c.replace('\'', "''").replace('[', "'[").replace(']', "']").replace('#', "'#");
    let item = |i: &StructItem| match i {
        StructItem::All => "#All",
        StructItem::Data => "#Data",
        StructItem::Headers => "#Headers",
        StructItem::Totals => "#Totals",
        StructItem::ThisRow => "#This Row",
    };
    let this_row = at_shorthand && s.specifiers == [StructItem::ThisRow];
    let mut out = s.table.clone();
    let simple_col = |c: &str| !c.chars().any(|ch| matches!(ch, ' ' | '[' | ']' | '#' | '\'' | ',' | ':'));
    match (&s.col_start, &s.col_end) {
        (None, None) if s.specifiers.is_empty() => out.push_str("[]"),
        (None, None) if this_row => out.push_str("[@]"),
        (None, None) if s.specifiers.len() == 1 => {
            let _ = write!(out, "[{}]", s.specifiers.first().map(item).unwrap_or(""));
        }
        (Some(c), None) if s.specifiers.is_empty() => {
            // Names with spaces at their edges, or starting with `@` (this-row shorthand), use the double-bracket form.
            if c.trim() == c && !c.starts_with('@') {
                let _ = write!(out, "[{}]", esc(c));
            } else {
                let _ = write!(out, "[[{}]]", esc(c));
            }
        }
        (Some(c), None) if this_row => {
            if simple_col(c) {
                let _ = write!(out, "[@{}]", esc(c));
            } else {
                let _ = write!(out, "[@[{}]]", esc(c));
            }
        }
        _ => {
            let mut parts: Vec<String> = s.specifiers.iter().map(|i| format!("[{}]", item(i))).collect();
            match (&s.col_start, &s.col_end) {
                (Some(a), Some(b)) => parts.push(format!("[{}]:[{}]", esc(a), esc(b))),
                (Some(a), None) => parts.push(format!("[{}]", esc(a))),
                _ => {}
            }
            let _ = write!(out, "[{}]", parts.join(","));
        }
    }
    out
}

fn number(n: f64) -> String {
    let text = number_to_text(n);
    // 15 significant digits can round past f64::MAX, which no longer parses.
    if text.parse::<f64>().is_ok_and(f64::is_finite) {
        return text;
    }
    let exact = format!("{:E}", n);
    match exact.split_once('E') {
        Some((m, e)) if !e.starts_with('-') => format!("{m}E+{e}"),
        _ => exact,
    }
}

/// Prints an expression in A1 notation.
pub fn print(e: &Expr) -> String {
    let mut s = String::new();
    write_expr(&mut s, e, Mode::A1);
    s
}

/// Prints an expression in R1C1 notation relative to `at`.
pub fn print_r1c1(e: &Expr, at: CellRef) -> String {
    let mut s = String::new();
    write_expr(&mut s, e, Mode::R1c1(at));
    s
}

/// Prints a **canonical** (shape) tree as the A1 text the cell at `at` spells: a relative part is
/// printed as `at` plus its offset, an absolute one as the coordinate it names. This is
/// `print(resolve(tree, at))` in one walk and no tree, which is what a structural edit costs per
/// cell it moves (S5 of `plan/SHAPE-SHARING.md`).
///
/// The caller checks that the shape fits at `at` ([crate::Shape::fits]): one that does not would
/// clamp a reference to `#REF!`, and a clamp mid-print is not a text.
pub fn print_at(tree: &Expr, at: CellRef) -> String {
    let mut s = String::new();
    write_expr(&mut s, tree, Mode::At(at));
    s
}

/// How the printer reads a reference: as the absolute tree it is, as the R1C1 spelling a shape is
/// keyed by, or as the A1 text a shape spells at a cell.
#[derive(Clone, Copy)]
enum Mode {
    A1,
    R1c1(CellRef),
    At(CellRef),
}

impl Mode {
    /// The cell whose position a relative part is read from (A1 for an absolute tree).
    fn base(self) -> CellRef {
        match self {
            Mode::A1 => CellRef::new(0, 0),
            Mode::R1c1(at) | Mode::At(at) => at,
        }
    }
}

fn write_expr(s: &mut String, e: &Expr, mode: Mode) {
    match e {
        Expr::Number(n) => s.push_str(&number(*n)),
        Expr::Text(t) => {
            s.push('"');
            s.push_str(&t.replace('"', "\"\""));
            s.push('"');
        }
        Expr::Bool(b) => s.push_str(if *b { "TRUE" } else { "FALSE" }),
        Expr::Error(err) => s.push_str(err.as_str()),
        Expr::Array(rows) => {
            s.push('{');
            for (ri, row) in rows.iter().enumerate() {
                if ri > 0 {
                    s.push(';');
                }
                for (ci, el) in row.iter().enumerate() {
                    if ci > 0 {
                        s.push(',');
                    }
                    write_expr(s, el, mode);
                }
            }
            s.push('}');
        }
        Expr::Ref(r) => match mode {
            Mode::R1c1(at) => s.push_str(&reference_r1c1(r, at)),
            Mode::A1 | Mode::At(_) => push_range_a1(s, r, ":", mode.base()),
        },
        Expr::Name(n) => match sheet_split(n) {
            // A sheet-qualified name is stored with the bare sheet name (`My Sheet!Total`): quote it so it parses back.
            Some((sheet, name)) => {
                s.push_str(&quote_sheet(sheet));
                s.push('!');
                s.push_str(name);
            }
            _ => s.push_str(n),
        },
        Expr::Struct(st) => s.push_str(&struct_ref(st)),
        Expr::Unary(op, x) => match op {
            UnOp::Neg => {
                s.push('-');
                write_expr(s, x, mode);
            }
            UnOp::Plus => {
                s.push('+');
                write_expr(s, x, mode);
            }
            UnOp::At => {
                s.push('@');
                write_expr(s, x, mode);
            }
            UnOp::Percent => {
                write_expr(s, x, mode);
                s.push('%');
            }
            UnOp::Spill => {
                write_expr(s, x, mode);
                // A reference that became #REF! (its sheet or cell was deleted) stays `#REF!`.
                if !matches!(**x, Expr::Error(_)) {
                    s.push('#');
                }
            }
            UnOp::Trim(trim) => match &**x {
                Expr::Ref(r) if !matches!(r.kind, RefKind::Cell(_)) => match mode {
                    Mode::R1c1(at) => s.push_str(&range_r1c1(r, at, trim.symbol())),
                    Mode::A1 | Mode::At(_) => push_range_a1(s, r, trim.symbol(), mode.base()),
                },
                Expr::Binary(BinOp::Range, a, b) => {
                    write_expr(s, a, mode);
                    s.push_str(trim.symbol());
                    write_expr(s, b, mode);
                }
                // A reference that became #REF! stays `#REF!`.
                Expr::Error(_) => write_expr(s, x, mode),
                // No operator form (e.g. a range shrunk to one cell): the equivalent call.
                other => {
                    s.push_str("TRIMRANGE(");
                    write_expr(s, other, mode);
                    let _ = write!(s, ",{0},{0})", trim.code());
                }
            },
        },
        Expr::Binary(..) => {
            // Walk the left spine iteratively: a flat chain such as `1+1+1+...` parses into a
            // tree one level deeper per operator, and recursing on it could exhaust the stack.
            let mut rest: Vec<(&BinOp, &Expr)> = Vec::new();
            let mut first = e;
            while let Expr::Binary(op, a, b) = first {
                rest.push((op, b));
                first = a;
            }
            write_expr(s, first, mode);
            for (op, right) in rest.into_iter().rev() {
                s.push_str(op.symbol());
                write_expr(s, right, mode);
            }
        }
        Expr::Call(name, args) => {
            s.push_str(name);
            s.push('(');
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                write_expr(s, a, mode);
            }
            s.push(')');
        }
        Expr::Invoke(c, args) => {
            write_expr(s, c, mode);
            s.push('(');
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                write_expr(s, a, mode);
            }
            s.push(')');
        }
        Expr::Missing => {}
        Expr::Paren(x) => {
            s.push('(');
            write_expr(s, x, mode);
            s.push(')');
        }
    }
}
