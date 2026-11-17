//! Formula text conversion between the model (canonical text, no `=`) and files (`_xlfn.`
//! prefixes for newer functions).

use gridcraft_core::CellRef;
use gridcraft_formula::{Expr, ShapeTable};
use gridcraft_model::Formula;

/// Formula text for a file, for the formula of the cell at `at`.
///
/// The rewrite below is only about what a file spells differently from the model's canonical
/// text, and a formula that uses none of it — most of them — is written as its own text. That
/// answer is kept **on the shape** when the cell has one (`Shape::file_rewrite`, ORed with this
/// module's `_xlfn.` table by the reader), so a save of a million shape cells resolves no tree at
/// all; a cell that carries its own tree asks it, one allocation-free walk.
pub fn file_text(f: &Formula, at: CellRef) -> std::borrow::Cow<'_, str> {
    // The model's text is the canonical print of the tree (both constructors print), so it is
    // exactly what printing it again would produce — and borrowing it saves the copy and the
    // allocation a save of a million-formula sheet made a million times.
    if let Some(shape) = f.shape() {
        if !shape.file_rewrite {
            return std::borrow::Cow::Borrowed(f.text.as_str());
        }
        return match f.expr_at(at) {
            Some(e) => std::borrow::Cow::Owned(expr_to_file(e)),
            None => std::borrow::Cow::Borrowed(f.text.as_str()),
        };
    }
    // Text alone (a JSON document, or a formula nothing has parsed yet): `expr_at` parses it at
    // the cell, and this walk decides the spelling. It is the fallback the shape design kept
    // (SHAPE-SHARING.md §5.3): an `_xlfn.` formula read back from such a document is spelled
    // right, at the price of one parse a save.
    match f.expr_at(at) {
        Some(e) if needs_file_rewrite(&e) => std::borrow::Cow::Owned(expr_to_file(e)),
        _ => std::borrow::Cow::Borrowed(f.text.as_str()),
    }
}

/// [`file_text`] as a `String`, for the callers that need one.
#[cfg(test)]
pub(crate) fn to_file(f: &Formula) -> String {
    file_text(f, CellRef::new(0, 0)).into_owned()
}

/// The rewrite itself, for the test that holds the fast path to it.
#[cfg(test)]
pub(crate) fn expr_to_file_for_test(e: Expr) -> String {
    expr_to_file(e)
}

/// The scan, for the test that holds it to the rewrite.
#[cfg(test)]
pub(crate) fn needs_file_rewrite_for_test(e: &Expr) -> bool {
    needs_file_rewrite(e)
}

/// Whether a file writes `e` differently from the canonical text: a `_xlfn.` function name, a
/// `LET`/`LAMBDA` parameter prefix, the `#`, `@` or trim-reference operator, or a structured
/// reference (all spelled out in [`expr_to_file`]). Nothing else is rewritten, so a formula that
/// holds none of them keeps its own text.
///
/// The answer belongs to the formula's **shape** (`Shape::file_rewrite`), not to the cell: a reader
/// or an editor ORs it in where the shape is interned, so a save of a million formulas never walks
/// a tree to decide how to spell one. Public because the engine interns shapes too (a typed
/// formula), and only this crate holds the `_xlfn.` table.
pub fn needs_file_rewrite(e: &Expr) -> bool {
    let mut need = false;
    e.walk(&mut |x| match x {
        // `LET`/`LAMBDA` declare parameters its calls reference, so the scope has to be built.
        Expr::Call(name, _) if name == "LET" || name == "LAMBDA" || crate::tables::file_function_name(name).is_some() => need = true,
        Expr::Unary(gridcraft_formula::UnOp::Spill | gridcraft_formula::UnOp::At | gridcraft_formula::UnOp::Trim(_), _) => need = true,
        // Only the `[@Col]` shorthand prints differently: every other structured reference is
        // already written the long way (see `printer::struct_ref_file`).
        Expr::Struct(s) if s.specifiers.len() == 1 && s.specifiers.first() == Some(&gridcraft_formula::StructItem::ThisRow) => need = true,
        _ => {}
    });
    need
}

/// Arbitrary formula text (names, CF, validation) for a file. Unparseable text is kept.
pub fn text_to_file(text: &str) -> String {
    let body = text.strip_prefix('=').unwrap_or(text);
    match gridcraft_formula::parse(body) {
        Ok(e) => expr_to_file(e),
        Err(_) => body.to_string(),
    }
}

const PARAM_PREFIX: &str = "_xlpm.";

/// Adds Excel's `_xlpm.` prefix to LET/LAMBDA parameter names, at the declaration and at every use in scope.
fn prefix_params(e: Expr, scope: &[String]) -> Expr {
    let sub = |x: Box<Expr>, sc: &[String]| Box::new(prefix_params(*x, sc));
    match e {
        Expr::Name(n) if scope.iter().any(|s| s.eq_ignore_ascii_case(&n)) => Expr::Name(format!("{PARAM_PREFIX}{n}")),
        Expr::Unary(op, x) => Expr::Unary(op, sub(x, scope)),
        Expr::Paren(x) => Expr::Paren(sub(x, scope)),
        Expr::Binary(op, a, b) => {
            // A long chain such as A1+A2+… nests on its left: follow it in a loop, not one
            // level of recursion per operand.
            let mut spine = vec![(op, b)];
            let mut left = *a;
            while let Expr::Binary(op, a, b) = left {
                spine.push((op, b));
                left = *a;
            }
            let mut acc = prefix_params(left, scope);
            while let Some((op, b)) = spine.pop() {
                acc = Expr::Binary(op, Box::new(acc), sub(b, scope));
            }
            acc
        }
        Expr::Invoke(c, args) => Expr::Invoke(sub(c, scope), args.into_iter().map(|a| prefix_params(a, scope)).collect()),
        Expr::Call(name, args) if name == "LET" || name == "LAMBDA" => {
            let last = args.len().saturating_sub(1);
            let is_let = name == "LET";
            let mut sc = scope.to_vec();
            let mut pending: Option<String> = None;
            let mut out = Vec::with_capacity(args.len());
            for (i, a) in args.into_iter().enumerate() {
                let declares = i < last && (!is_let || i % 2 == 0);
                match a {
                    Expr::Name(n) if declares => {
                        out.push(Expr::Name(format!("{PARAM_PREFIX}{n}")));
                        if is_let {
                            pending = Some(n);
                        } else {
                            sc.push(n);
                        }
                    }
                    other => {
                        out.push(prefix_params(other, &sc));
                        // A LET name is in scope only after its value expression.
                        if let Some(n) = pending.take() {
                            sc.push(n);
                        }
                    }
                }
            }
            Expr::Call(name, out)
        }
        // A LET-bound LAMBDA called by name, e.g. `f(3)`.
        Expr::Call(name, args) if scope.iter().any(|s| s.eq_ignore_ascii_case(&name)) => {
            Expr::Call(format!("{PARAM_PREFIX}{name}"), args.into_iter().map(|a| prefix_params(a, scope)).collect())
        }
        Expr::Call(name, args) => Expr::Call(name, args.into_iter().map(|a| prefix_params(a, scope)).collect()),
        other => other,
    }
}

fn expr_to_file(e: Expr) -> String {
    let e = prefix_params(e, &[]);
    let e = e.map(&mut |x| match x {
        Expr::Call(name, args) => match crate::tables::file_function_name(&name) {
            Some(n) => Expr::Call(n, args),
            None => Expr::Call(name, args),
        },
        // Files have no `#` operator: Excel writes `A1#` as `_xlfn.ANCHORARRAY(A1)`, and a spill
        // reference that became `#REF!` (its cell was deleted) as plain `#REF!`.
        Expr::Unary(gridcraft_formula::UnOp::Spill, r) if matches!(*r, Expr::Error(_)) => *r,
        Expr::Unary(gridcraft_formula::UnOp::Spill, r) => Expr::Call("_xlfn.ANCHORARRAY".into(), vec![*r]),
        // Nor an `@` operator: Excel writes `@x` as `_xlfn.SINGLE(x)`.
        Expr::Unary(gridcraft_formula::UnOp::At, x) => Expr::Call("_xlfn.SINGLE".into(), vec![*x]),
        // Nor the `[@Col]` shorthand: files spell it `[[#This Row],[Col]]`, or Excel repairs
        // the file and drops the table. Carried as a name, which prints verbatim.
        Expr::Struct(st) => Expr::Name(gridcraft_formula::printer::struct_ref_file(&st)),
        // Nor trim-reference operators: written as the equivalent `TRIMRANGE(range,n,n)`, which
        // every Excel with trim references reads. (Excel's own file spelling of `A1:.E10` is
        // not publicly documented; the `_xlfn._TRO_*` call forms are accepted on read.)
        Expr::Unary(gridcraft_formula::UnOp::Trim(mode), x) => {
            let code = Expr::Number(f64::from(mode.code()));
            Expr::Call("_xlfn.TRIMRANGE".into(), vec![*x, code.clone(), code])
        }
        other => other,
    });
    gridcraft_formula::print(&e)
}

/// `_xlpm.x` → `x` (any case, as the parser upper-cases function names).
fn strip_param_prefix(n: String) -> String {
    match (n.get(..PARAM_PREFIX.len()), n.get(PARAM_PREFIX.len()..)) {
        (Some(p), Some(rest)) if p.eq_ignore_ascii_case(PARAM_PREFIX) => rest.to_string(),
        _ => n,
    }
}

/// The model formula for text read from a file at `at`: the model's spelling of it, parsed once,
/// as a shape interned in `shapes`.
///
/// [`from_file`] and [`Formula::new`] each parse and print the text, so converting a cell this way
/// used to parse every formula twice — on a sheet with a million formulas that was most of the
/// read. This builds the same `Formula` (the same text, the shape already parsed) from one parse:
/// `print` is stable, so printing the stripped tree is the text `Formula::new` would have parsed
/// again.
pub fn formula_from_file_at(text: &str, at: CellRef, shapes: &mut ShapeTable) -> Formula {
    let body = text.strip_prefix('=').unwrap_or(text);
    match gridcraft_formula::parse(body) {
        Ok(e) => {
            let e = strip_param_prefixes(e);
            // A save must not resolve a tree to find this out: the `_xlfn.` table is this crate's,
            // so the reader ORs its answer into the shape before interning it.
            let file_rewrite = needs_file_rewrite(&e);
            Formula::from_expr_at(e, at, shapes, file_rewrite)
        }
        Err(_) => Formula::from_text(body),
    }
}

/// Formula text read from a file → model text (strips prefixes when it parses).
pub fn from_file(text: &str) -> String {
    let body = text.strip_prefix('=').unwrap_or(text);
    match gridcraft_formula::parse(body) {
        Ok(e) => gridcraft_formula::print(&strip_param_prefixes(e)),
        Err(_) => body.to_string(),
    }
}

fn strip_param_prefixes(e: Expr) -> Expr {
    e.map(&mut |x| match x {
        Expr::Name(n) => Expr::Name(strip_param_prefix(n)),
        Expr::Call(n, args) => Expr::Call(strip_param_prefix(n), args),
        other => other,
    })
}

/// The worksheet functions the evaluator can calculate, for reading (#2871). A Google Sheets
/// export wrapper is unwrapped only when every function inside it is one, so a formula GridCraft
/// would show as `#NAME?` keeps the wrapper and the file's cached value instead.
///
/// Only a predicate that lives for the whole program can be given (a plain `fn`, as the engine
/// passes): a read may decode rows on several threads.
#[derive(Clone, Copy, Default)]
pub struct KnownFunctions(Option<&'static (dyn Fn(&str) -> bool + Sync)>);

impl KnownFunctions {
    /// With the evaluator's answers: `known(name)` says whether it can calculate the function.
    pub fn new(known: &'static (dyn Fn(&str) -> bool + Sync)) -> KnownFunctions {
        KnownFunctions(Some(known))
    }

    /// The predicate, when the caller gave one.
    pub(crate) fn given(self) -> Option<&'static (dyn Fn(&str) -> bool + Sync)> {
        self.0
    }
}

impl std::fmt::Debug for KnownFunctions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() { "KnownFunctions(given)" } else { "KnownFunctions(none)" })
    }
}

/// The function Google Sheets' XLSX export wraps formulas in when it thinks Excel lacks one of
/// their functions.
const DUMMY_FUNCTION: &str = "__xludf.DUMMYFUNCTION";

/// Whether `text` may hold Google Sheets' export wrapper: a cheap test before parsing.
fn mentions_dummy_function(text: &str) -> bool {
    const NEEDLE: &[u8] = b"DUMMYFUNCTION";
    text.as_bytes().windows(NEEDLE.len()).any(|w| w.eq_ignore_ascii_case(NEEDLE))
}

/// The model text of `text` (a formula as read, see [`from_file`]) with its Google Sheets export
/// wrappers replaced, or `None` when nothing changes.
pub(crate) fn unwrap_google_sheets(text: &str, known: &dyn Fn(&str) -> bool) -> Option<String> {
    if !mentions_dummy_function(text) {
        return None;
    }
    let e = gridcraft_formula::parse(text.strip_prefix('=').unwrap_or(text)).ok()?;
    let unwrapped = unwrap_dummy_functions(e, known)?;
    Some(gridcraft_formula::print(&strip_param_prefixes(unwrapped)))
}

/// The original formula inside Google Sheets' export wrapper
/// `IFERROR(__xludf.DUMMYFUNCTION("<formula, quotes doubled>"),<cached value>)`, if `e` is that
/// wrapper, the text parses and every function it calls is `known`. The parser has already undone
/// the doubled quotes.
fn dummy_function_inner(e: &Expr, known: &dyn Fn(&str) -> bool) -> Option<Expr> {
    let Expr::Call(name, args) = e else { return None };
    // The parser has already dropped an `_xlfn.` prefix from IFERROR.
    if !name.eq_ignore_ascii_case("IFERROR") {
        return None;
    }
    let [Expr::Call(dummy, inner), _fallback] = args.as_slice() else { return None };
    if !dummy.eq_ignore_ascii_case(DUMMY_FUNCTION) {
        return None;
    }
    let [Expr::Text(src)] = inner.as_slice() else { return None };
    // The text came out of a formula within the length limit, so it is within it too; check
    // anyway rather than rely on that.
    if src.len() > gridcraft_formula::MAX_FORMULA_LEN && src.chars().count() > gridcraft_formula::MAX_FORMULA_LEN {
        return None;
    }
    let parsed = gridcraft_formula::parse(src).ok()?;
    // A function GridCraft can't calculate would show `#NAME?` where Excel shows the cached
    // value; a wrapper inside the text is one too, so only one level is ever unwrapped.
    let mut all_known = true;
    parsed.walk(&mut |x| {
        if let Expr::Call(n, _) = x {
            all_known &= known(n);
        }
    });
    all_known.then_some(parsed)
}

/// `e` with each Google Sheets export wrapper (see [`dummy_function_inner`]) replaced by the
/// formula it holds, for the whole formula or anywhere inside it, or `None` when none is. If the
/// result no longer parses (nesting beyond the parser's limit), nothing is replaced.
fn unwrap_dummy_functions(e: Expr, known: &dyn Fn(&str) -> bool) -> Option<Expr> {
    // The whole formula: no parentheses needed.
    if let Some(inner) = dummy_function_inner(&e, known) {
        return Some(inner);
    }
    let mut found = false;
    let rewritten = e.map(&mut |x| match dummy_function_inner(&x, known) {
        // The printer writes no parentheses of its own, so keep the inner formula's precedence
        // with explicit ones unless it is a single operand.
        Some(
            inner @ (Expr::Call(..)
            | Expr::Ref(_)
            | Expr::Name(_)
            | Expr::Number(_)
            | Expr::Text(_)
            | Expr::Bool(_)
            | Expr::Error(_)
            | Expr::Paren(_)
            | Expr::Array(_)
            | Expr::Struct(_)),
        ) => {
            found = true;
            inner
        }
        Some(inner) => {
            found = true;
            Expr::Paren(Box::new(inner))
        }
        None => x,
    });
    if !found {
        return None;
    }
    gridcraft_formula::parse(&gridcraft_formula::print(&rewritten)).ok()?;
    Some(rewritten)
}

/// What the engine passes as the known-function predicate, for tests here: the functions these
/// tests' formulas call. The crate has no function library beside it, so the list is explicit.
#[cfg(test)]
pub(crate) fn test_known_function(n: &str) -> bool {
    const KNOWN: [&str; 15] =
        ["IF", "IFERROR", "IFNA", "MAX", "FILTER", "ROW", "CHAR", "SUM", "SORT", "RAND", "TEXT", "INDEX", "LET", "LAMBDA", "ABS"];
    KNOWN.iter().any(|s| s.eq_ignore_ascii_case(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spill_references() {
        assert_eq!(to_file(&Formula::new("SUM(Sheet1!$D$1#)+B2#")), "SUM(_xlfn.ANCHORARRAY(Sheet1!$D$1))+_xlfn.ANCHORARRAY(B2)");
        assert_eq!(text_to_file("=Sheet1!$D$1#"), "_xlfn.ANCHORARRAY(Sheet1!$D$1)");
        assert_eq!(from_file("SUM(_xlfn.ANCHORARRAY(Sheet1!$D$1))+_xlfn.ANCHORARRAY(B2)"), "SUM(Sheet1!$D$1#)+B2#");
        // Found by the command-sequence fuzzer: deleting the anchor of `H5#` left `=#REF!` in
        // the workbook but `ANCHORARRAY(#REF!)` in the file, which read back differently.
        let deleted = Formula::from_text("#REF!");
        assert_eq!(deleted.text, "#REF!");
        assert_eq!(to_file(&deleted), "#REF!");
    }

    /// `formula_from_file_at` builds what `from_file` → `Formula::new_at` built, from one parse:
    /// the same text, the shape already interned at the cell (the read path relies on both), and
    /// that shape prints the cell's own text — the master invariant of the shape work
    /// (SHAPE-SHARING.md §4.3).
    #[test]
    fn formula_from_file_matches_the_two_step_conversion() {
        let at = CellRef::new(4, 5);
        let mut shapes = ShapeTable::default();
        for text in [
            "B1*C1+D1",
            "SUM(A1:A9)",
            "=SUM( A1 : A9 )",
            "_xlfn.XLOOKUP(1,A:A,B:B)",
            "_xlfn.LET(_xlpm.x,1,_xlpm.x+1)",
            "_xlfn.SINGLE(A1:A3)*2",
            "SUM(_xlfn.ANCHORARRAY(A1))+2",
            "NOT A FORMULA(",
            "",
        ] {
            let one = formula_from_file_at(text, at, &mut shapes);
            let model_text = from_file(text);
            let rewrite = gridcraft_formula::parse(&model_text).is_ok_and(|e| needs_file_rewrite(&e));
            let two = Formula::new_at(&model_text, at, &mut shapes, rewrite);
            assert_eq!(one.text, two.text, "{text:?}");
            assert_eq!(one.shape.is_some(), two.shape.is_some(), "{text:?}");
            // The model's text is what a save writes, so it must not have changed either.
            assert_eq!(file_text(&one, at).as_ref(), file_text(&two, at).as_ref(), "{text:?}");
            if let Some(shape) = one.shape() {
                assert_eq!(shape.print_at(at).as_deref(), Some(one.text.as_str()), "{text:?}");
            }
        }
    }

    /// The answers the shape needs from this crate are on it, not computed per save: a formula the
    /// file spells differently (`_xlfn.`, `_xlpm.`, `#`, `@`) is marked by the reader, and a
    /// formula the file writes as it is borrows its own text.
    #[test]
    fn a_shape_learns_the_file_spelling_where_it_is_interned() {
        let at = CellRef::new(1, 1);
        let mut shapes = ShapeTable::default();
        for (text, rewrite) in [("XLOOKUP(1,A:A,B:B)", true), ("B1*C1+D1", false), ("LET(x,1,x+1)", true)] {
            let f = formula_from_file_at(text, at, &mut shapes);
            let shape = f.shape().expect("a parsed formula has a shape");
            assert_eq!(shape.file_rewrite, rewrite, "{text}");
            assert_eq!(file_text(&f, at).as_ref(), to_file(&Formula::new(text)).as_str(), "{text}");
        }
    }

    #[test]
    fn implicit_intersection() {
        assert_eq!(to_file(&Formula::new("SUM(@A1:A3*B1:B3)")), "SUM(_xlfn.SINGLE(A1:A3)*B1:B3)");
        assert_eq!(to_file(&Formula::new("@A1:A3*2")), "_xlfn.SINGLE(A1:A3)*2");
        assert_eq!(text_to_file("=@Sheet1!$A$1:$A$3"), "_xlfn.SINGLE(Sheet1!$A$1:$A$3)");
        assert_eq!(from_file("_xlfn.SINGLE(A1:A3)*2"), "@A1:A3*2");
        assert_eq!(from_file("SUM(_xlfn.SINGLE(A1:A3)*B1:B3)"), "SUM(@A1:A3*B1:B3)");
    }

    #[test]
    fn trim_references() {
        assert_eq!(to_file(&Formula::new("SUM(A1:.E10)+SUM(Data!A.:A)")), "SUM(_xlfn.TRIMRANGE(A1:E10,2,2))+SUM(_xlfn.TRIMRANGE(Data!A:A,1,1))");
        assert_eq!(text_to_file("=ROWS(1.:.5)"), "ROWS(_xlfn.TRIMRANGE(1:5,3,3))");
        assert_eq!(from_file("SUM(_xlfn._TRO_TRAILING(A1:E10))+_xlfn._TRO_ALL($A:$B)"), "SUM(A1:.E10)+$A.:.$B");
    }

    #[test]
    fn struct_refs_with_a_bang_in_the_column_round_trip() {
        // Found by the command-sequence fuzzer: a table column named `#SPILL!` became
        // `'Table1[[#This Row],[''#SPILL'!]]` on save, because the file form is carried as a
        // name and the name printer split it at the `!` as a sheet qualifier.
        for f in ["Table1[@['#SPILL!]]", "Table1[[#This Row],['#SPILL!]]", "Table1[Wow!]", "Table1['Wow!']", "Sales[@Units]+1"] {
            // The model keeps the shorthand form, so compare save/reopen with the canonical text.
            let canon = from_file(f);
            let file = to_file(&Formula::new(&canon));
            assert_eq!(from_file(&file), canon, "{f} -> {canon} -> {file}");
        }
        assert_eq!(to_file(&Formula::new("Table1[@['#SPILL!]]")), "Table1[[#This Row],['#SPILL!]]");
        assert_eq!(to_file(&Formula::new("Table1[Wow!]")), "Table1[Wow!]");
    }

    #[test]
    fn unbracketed_this_row_column() {
        assert_eq!(from_file("Sales[[#This Row],Units]*[[#This Row],Price]"), "Sales[@Units]*[@Price]");
    }

    #[test]
    fn this_row_long_form() {
        // `[@Col]` is only Excel's display shorthand; a file holding it is repaired by Excel,
        // which drops the table.
        assert_eq!(to_file(&Formula::new("Sales[@Units]*Sales[@[Unit Price]]")), "Sales[[#This Row],[Units]]*Sales[[#This Row],[Unit Price]]");
        assert_eq!(text_to_file("=[@Units]&COUNTA(Sales[@])"), "[[#This Row],[Units]]&COUNTA(Sales[#This Row])");
        assert_eq!(text_to_file("=SUM(Sales[Units])+[#Totals]"), "SUM(Sales[Units])+[#Totals]");
        for f in ["Sales[@Units]*[@[Unit Price]]", "Sales[@]", "[@[Unit Price]]+1"] {
            let file = to_file(&Formula::new(f));
            assert!(!file.contains("[@"), "{file}");
            assert_eq!(from_file(&file), f);
        }
    }

    #[test]
    fn prefixes() {
        assert_eq!(to_file(&Formula::new("XLOOKUP(1,A:A,B:B)+SUM(A1)")), "_xlfn.XLOOKUP(1,A:A,B:B)+SUM(A1)");
        assert_eq!(to_file(&Formula::new("FILTER(A1:A3,B1:B3)")), "_xlfn._xlws.FILTER(A1:A3,B1:B3)");
        assert_eq!(from_file("_xlfn.XLOOKUP(1,A:A,B:B)"), "XLOOKUP(1,A:A,B:B)");
        assert_eq!(text_to_file("=IFS(A1>0,1)"), "_xlfn.IFS(A1>0,1)");
        assert_eq!(to_file(&Formula::new("REGEXTEST(A1,\"x\")&ENCODEURL(B1)")), "_xlfn.REGEXTEST(A1,\"x\")&_xlfn.ENCODEURL(B1)");
        assert_eq!(to_file(&Formula::new("GROUPBY(A1:A3,B1:B3,\"SUM\")")), "_xlfn.GROUPBY(A1:A3,B1:B3,\"SUM\")");
    }

    #[test]
    fn let_lambda_params() {
        assert_eq!(to_file(&Formula::new("LET(x,1,x+1)")), "_xlfn.LET(_xlpm.x,1,_xlpm.x+1)");
        assert_eq!(to_file(&Formula::new("LET(x,A1*2,y,x+1,x*y)")), "_xlfn.LET(_xlpm.x,A1*2,_xlpm.y,_xlpm.x+1,_xlpm.x*_xlpm.y)");
        assert_eq!(to_file(&Formula::new("LAMBDA(a,b,a^2+b)(3,4)")), "_xlfn.LAMBDA(_xlpm.a,_xlpm.b,_xlpm.a^2+_xlpm.b)(3,4)");
        // A name outside the scope stays a plain defined name.
        assert_eq!(to_file(&Formula::new("LET(x,1,x)+x")), "_xlfn.LET(_xlpm.x,1,_xlpm.x)+x");
        assert_eq!(from_file("_xlfn.LET(_xlpm.x,1,_xlpm.x+1)"), "LET(x,1,x+1)");
        assert_eq!(from_file("_xlfn.LAMBDA(_xlpm.a,_xlpm.a*2)(3)"), "LAMBDA(a,a*2)(3)");
        // A LET-bound LAMBDA called by name.
        assert_eq!(to_file(&Formula::new("LET(f,LAMBDA(x,x*2),f(3))")), "_xlfn.LET(_xlpm.f,_xlfn.LAMBDA(_xlpm.x,_xlpm.x*2),_xlpm.F(3))");
        assert_eq!(from_file("_xlfn.LET(_xlpm.f,_xlfn.LAMBDA(_xlpm.x,_xlpm.x*2),_xlpm.f(3))"), "LET(f,LAMBDA(x,x*2),F(3))");
    }
}
