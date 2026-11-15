//! The measure language: the deliberate subset of DAX GridCraft evaluates over its data model.
//!
//! A measure is a formula such as SUM(Sales[Amount]) or
//! CALCULATE(SUM(Sales[Amount]), Region[Name] = "West") defined over the model's tables
//! (gridcraft_model::DataModel) rather than over a sheet. This crate is the whole language: one
//! parser, one evaluator, no I/O and no egui, so it builds for WebAssembly and the engine, the
//! control channel and the tests all reach it the same way.
//!
//! # The grammar
//!
//! | form | example | note |
//! | --- | --- | --- |
//! | number | 1, 2.5 | -3 is unary minus |
//! | text | "West" | a doubled quote inside: "It""s" |
//! | arithmetic | + - * /, unary -, ( ) | * and / bind tighter than + and - |
//! | column | Sales[Amount] | only as the one argument of an aggregate |
//! | aggregate | SUM, AVERAGE, COUNT, COUNTA, MIN, MAX, DISTINCTCOUNT | one column each |
//! | CALCULATE | CALCULATE(expr, Table[Column] = expr, ...) | filters are ANDed onto the context |
//! | DIVIDE | DIVIDE(numerator, denominator[, alternate]) | blank when the denominator is 0 or blank |
//! | BLANK | BLANK() | the empty value |
//! | measure | Total | a bare name is another measure's formula |
//!
//! Function names, column names, table names and measure names are matched without regard to case.
//!
//! # What the semantics promise
//!
//! The rules a spreadsheet full of numbers depends on, written here because they are choices:
//!
//! * **A blank is not a zero.** A blank cell is Value::Empty, it is never counted as a number, and
//!   an aggregate over no rows is Value::Empty rather than 0. A blank operand makes arithmetic
//!   blank (DAX would read it as a zero; see below).
//! * **A value that is not a number is #VALUE!**, not a quiet zero: text or a boolean where
//!   arithmetic needs a number answers CellError::Value, which the pivot shows in that one cell
//!   instead of failing the whole measure.
//! * **A column has one type.** Text that looks numeric does not become a number, so SUM over a
//!   column of text is blank instead of a guess. COUNT counts numbers, COUNTA counts everything
//!   non-blank, and a column of "12", "13" counts 0 and 2 respectively.
//! * **Division by zero is blank, never an error**, and DIVIDE's optional third argument is the
//!   answer in that case.
//! * **A filter reaches the fact table.** A context filter on a dimension table is propagated
//!   through the model's relationships (to_table to from_table, the one side to the many side),
//!   several hops if the model has several, and it is refused with an error rather than guessed if
//!   the relationships hold a loop.
//!
//! # Where this differs from DAX on purpose
//!
//! These are the places where full DAX would coerce and this crate refuses to, or the other way
//! round. Each is a choice, not an oversight:
//!
//! * DAX reads BLANK as zero inside + - *, so BLANK() + 1 is 1. Here a blank operand keeps the
//!   result blank: inventing a zero is the quiet wrong number the rules above exist to prevent,
//!   and DIVIDE already has the alternate argument for the case where a zero really is wanted.
//! * DAX coerces text that looks numeric, so "12" + 1 is 13. Here a column has one type and text in
//!   arithmetic is #VALUE!.
//! * A cell holding an error is skipped by the aggregates (COUNT still does not count it, COUNTA
//!   still does) rather than poisoning the total of a column that holds a million good numbers.
//! * DISTINCTCOUNT does not count blanks as a category of their own, because a blank cell in a
//!   model is no value at all rather than a distinct one.
//!
//! # Caps
//!
//! A formula is at most MAX_FORMULA_LEN characters, nests at most MAX_DEPTH levels, may chain at
//! most MAX_MEASURE_DEPTH measures, and the relationship walk is bounded by the number of
//! relationships the model holds. Nothing here allocates from a number a formula names, and no
//! function reads or writes anything outside the model it was handed.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use gridcraft_core::Value;
use gridcraft_model::DataModel;

mod eval;
mod parse;
#[cfg(test)]
mod tests;

use parse::{Expr, aggregate, describe};

/// The longest formula a measure may hold. Refused before a token is read.
pub const MAX_FORMULA_LEN: usize = 4_096;
/// How deeply parentheses and calls may nest.
pub const MAX_DEPTH: usize = 32;
/// How many measures one formula may chain through before it is refused.
pub const MAX_MEASURE_DEPTH: usize = 16;

/// The functions this subset knows, in the order a help list shows them.
pub const FUNCTIONS: [&str; 10] = ["SUM", "AVERAGE", "COUNT", "COUNTA", "MIN", "MAX", "DISTINCTCOUNT", "CALCULATE", "DIVIDE", "BLANK"];

/// The filter context a measure is evaluated in: the equalities a pivot's rows and columns put on
/// the model, each Table[Column] = value.
///
/// A context is built by chaining filter, which ANDs: two filters on the same column keep the rows
/// that satisfy both, which is Excel's rule and the reason a blank result appears rather than one
/// of the two.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Context {
    filters: Vec<(String, String, Value)>,
    /// Column filters that keep several members at once. A PivotTable needs them because a
    /// grouped axis item stands for a bucket, not for a value: 2024 is every date in that year,
    /// and an equality cannot say that.
    sets: Vec<(String, String, Vec<Value>)>,
}

impl Context {
    /// An empty context: every row of every table is in scope.
    pub fn new() -> Context {
        Context::default()
    }

    /// The same context with one more equality, ANDed onto the ones it already holds.
    pub fn filter(mut self, table: &str, column: &str, value: Value) -> Context {
        self.filters.push((table.to_string(), column.to_string(), value));
        self
    }

    /// The same context with one more "this column is one of these" filter, ANDed onto the ones
    /// it already holds. An empty list matches nothing, which is what a bucket with no members
    /// means.
    pub fn filter_any_of(mut self, table: &str, column: &str, values: Vec<Value>) -> Context {
        self.sets.push((table.to_string(), column.to_string(), values));
        self
    }

    /// The equalities in the order they were added.
    pub fn filters(&self) -> &[(String, String, Value)] {
        &self.filters
    }

    /// The member lists in the order they were added (see Context::filter_any_of).
    pub fn sets(&self) -> &[(String, String, Vec<Value>)] {
        &self.sets
    }

    pub fn is_empty(&self) -> bool {
        self.filters.is_empty() && self.sets.is_empty()
    }

    /// How many filters the context carries, of both kinds: an equality and a member list are one
    /// filter each, not one per member.
    pub fn len(&self) -> usize {
        self.filters.len() + self.sets.len()
    }
}

/// Why a formula could not be parsed or evaluated. Every message is a sentence a dialog can show.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DaxError {
    /// The formula can't be read. at is the 0-based character offset where reading stopped.
    #[error("the formula can't be read at character {at}: {message}")]
    Parse { at: usize, message: String },
    /// A name followed by parentheses that is not one of FUNCTIONS.
    #[error("the model has no function named \"{name}\"")]
    UnknownFunction { name: String },
    /// A Table[...] whose table the model doesn't hold.
    #[error("the model has no table named \"{name}\"")]
    UnknownTable { name: String },
    /// A column the table doesn't hold.
    #[error("the table \"{table}\" has no column named \"{column}\"")]
    UnknownColumn { table: String, column: String },
    /// A bare name that is not a measure of this model.
    #[error("the model has no measure named \"{name}\"")]
    UnknownMeasure { name: String },
    /// The wrong number of arguments for the function.
    #[error("{function} takes {expected} argument(s) but got {got}")]
    BadArity { function: String, expected: usize, got: usize },
    /// A measure that refers to itself, directly or through other measures.
    #[error("the measure \"{measure}\" refers to itself, directly or through another measure")]
    Cycle { measure: String },
    /// More measures chained than MAX_MEASURE_DEPTH allows.
    #[error("the measures nest deeper than {MAX_MEASURE_DEPTH} (reached \"{measure}\")")]
    TooDeep { measure: String },
    /// An aggregate was given something that is not a column.
    #[error("{function} needs one column such as Sales[Amount], but got {got}")]
    NotANumber { function: String, got: String },
    /// A column used outside an aggregate.
    #[error("the column \"{table}[{column}]\" has to be inside SUM, AVERAGE, COUNT or another aggregate")]
    UnaggregatedColumn { table: String, column: String },
    /// A relationship graph with a loop in it (a hand-edited model; the editor refuses them).
    #[error("the relationships around the table \"{table}\" form a loop, so a filter can't reach it")]
    RelationshipLoop { table: String },
}

/// Parses and evaluates one measure formula in a filter context.
pub fn evaluate(model: &DataModel, formula: &str, ctx: &Context) -> Result<Value, DaxError> {
    let expr = checked(model, formula)?;
    let mut evaluator = eval::Evaluator::new(model, ctx);
    let mut stack: Vec<String> = Vec::new();
    evaluator.eval(&expr, &mut stack)
}

/// Parses and resolves names without evaluating anything: what "Add Measure" checks before it
/// saves the formula, so a typo is refused while the person is still looking at it.
pub fn validate(model: &DataModel, formula: &str) -> Result<(), DaxError> {
    checked(model, formula).map(|_| ())
}

/// Parses the formula and resolves every name in it, including the formulas of the measures it
/// refers to. The parse is kept, because evaluating needs the same tree.
fn checked(model: &DataModel, formula: &str) -> Result<Expr, DaxError> {
    let expr = parse::parse(formula)?;
    let mut stack: Vec<String> = Vec::new();
    check(model, &expr, &mut stack)?;
    Ok(expr)
}

/// Resolves the names of an expression. A column is only legal as the argument of an aggregate, so
/// any other column that reaches here is an error: a measure that silently used one row of a table
/// would be a lie about what it means.
fn check(model: &DataModel, e: &Expr, stack: &mut Vec<String>) -> Result<(), DaxError> {
    match e {
        Expr::Number(_) | Expr::Text(_) => Ok(()),
        Expr::Neg(inner) => check(model, inner, stack),
        Expr::Binary { left, right, .. } => {
            check(model, left, stack)?;
            check(model, right, stack)
        }
        Expr::Column { table, column, .. } => {
            resolve_column(model, table, column)?;
            Err(DaxError::UnaggregatedColumn { table: table.clone(), column: column.clone() })
        }
        Expr::Measure { name, .. } => {
            let Some(m) = model.measure(name) else {
                return Err(DaxError::UnknownMeasure { name: name.clone() });
            };
            if stack.iter().any(|s| s.eq_ignore_ascii_case(&m.name)) {
                return Err(DaxError::Cycle { measure: m.name.clone() });
            }
            if stack.len() >= MAX_MEASURE_DEPTH {
                return Err(DaxError::TooDeep { measure: m.name.clone() });
            }
            let inner = parse::parse(&m.formula)?;
            stack.push(m.name.clone());
            let out = check(model, &inner, stack);
            stack.pop();
            out
        }
        Expr::Filter { .. } => Err(DaxError::Parse { at: e.at(), message: "an equals sign only works as a filter inside CALCULATE".to_string() }),
        Expr::Call { name, args, .. } => check_call(model, name, args, stack),
    }
}

fn check_call(model: &DataModel, name: &str, args: &[Expr], stack: &mut Vec<String>) -> Result<(), DaxError> {
    if name.eq_ignore_ascii_case("BLANK") {
        if args.is_empty() {
            return Ok(());
        }
        return Err(DaxError::BadArity { function: "BLANK".to_string(), expected: 0, got: args.len() });
    }
    if name.eq_ignore_ascii_case("CALCULATE") {
        let Some((body, filters)) = args.split_first() else {
            return Err(DaxError::BadArity { function: "CALCULATE".to_string(), expected: 1, got: 0 });
        };
        check(model, body, stack)?;
        for filter in filters {
            let Expr::Filter { column, value, .. } = filter else {
                return Err(DaxError::Parse {
                    at: filter.at(),
                    message: "each argument after the first of CALCULATE must be a filter such as Region[Name] = \"West\"".to_string(),
                });
            };
            let Expr::Column { table, column: name, .. } = column.as_ref() else {
                return Err(DaxError::Parse {
                    at: column.at(),
                    message: "the left side of a CALCULATE filter must be one column, as in Region[Name]".to_string(),
                });
            };
            resolve_column(model, table, name)?;
            check(model, value, stack)?;
        }
        return Ok(());
    }
    if name.eq_ignore_ascii_case("DIVIDE") {
        if !(2..=3).contains(&args.len()) {
            return Err(DaxError::BadArity { function: "DIVIDE".to_string(), expected: 2, got: args.len() });
        }
        for arg in args {
            check(model, arg, stack)?;
        }
        return Ok(());
    }
    if aggregate(name).is_some() {
        let Some(arg) = args.first() else {
            return Err(DaxError::BadArity { function: name.to_string(), expected: 1, got: 0 });
        };
        if args.len() != 1 {
            return Err(DaxError::BadArity { function: name.to_string(), expected: 1, got: args.len() });
        }
        let Expr::Column { table, column, .. } = arg else {
            return Err(DaxError::NotANumber { function: name.to_string(), got: describe(arg) });
        };
        return resolve_column(model, table, column);
    }
    Err(DaxError::UnknownFunction { name: name.to_string() })
}

/// Whether the table has that column, naming whichever of the two is missing.
fn resolve_column(model: &DataModel, table: &str, column: &str) -> Result<(), DaxError> {
    let Some(t) = model.table(table) else {
        return Err(DaxError::UnknownTable { name: table.to_string() });
    };
    if t.index(column).is_none() {
        return Err(DaxError::UnknownColumn { table: t.name.clone(), column: column.to_string() });
    }
    Ok(())
}
