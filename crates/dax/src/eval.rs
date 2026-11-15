//! The evaluator: a parsed formula, a filter context, and the rows the context leaves.
//!
//! The two halves of the work are here. The first is arithmetic over values; the second, and the
//! reason the crate exists, is turning a filter context into the set of rows an aggregate reads:
//! a filter on Region[Name] has to reach Sales through Sales[RegionId] -> Region[Id], and one
//! relationship is rarely the whole story.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gridcraft_core::{CellError, Value};
use gridcraft_model::{DataModel, Relationship};

use crate::parse::{Agg, BinOp, Expr, aggregate, describe};
use crate::{Context, DaxError, MAX_MEASURE_DEPTH};

/// The values a key column is restricted to, worked out from a dimension table once and then
/// compared against every fact row.
///
/// Values match the way the context compares them: numbers by value, text without regard to case,
/// a blank only with a blank. A number never matches text that looks like it, because a column has
/// one type and guessing is how a total quietly gains a row.
#[derive(Debug, Default)]
pub(crate) struct ValueSet {
    keys: HashSet<Key>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Key {
    Empty,
    Number(u64),
    Text(String),
    Bool(bool),
    Error(&'static str),
    /// An array cannot match a key column; the variant exists so the match is total.
    Nothing,
}

/// A number's identity: its bits, with the two zeros folded together so 0 and -0 are one value.
fn number_bits(n: f64) -> u64 {
    if n == 0.0 { 0 } else { n.to_bits() }
}

fn key_of(v: &Value) -> Key {
    match v {
        Value::Empty => Key::Empty,
        Value::Number(n) => Key::Number(number_bits(*n)),
        Value::Text(t) => Key::Text(t.to_ascii_lowercase()),
        Value::Bool(b) => Key::Bool(*b),
        Value::Error(e) => Key::Error(e.as_str()),
        Value::Array(_) => Key::Nothing,
    }
}

/// Whether two cell values are the same value for filtering purposes.
fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Array(_), _) | (_, Value::Array(_)) => false,
        _ => key_of(a) == key_of(b),
    }
}

impl ValueSet {
    fn insert(&mut self, v: &Value) {
        if let Value::Array(_) = v {
            return;
        }
        self.keys.insert(key_of(v));
    }

    fn contains(&self, v: &Value) -> bool {
        match v {
            Value::Array(_) => false,
            other => self.keys.contains(&key_of(other)),
        }
    }

    fn len(&self) -> usize {
        self.keys.len()
    }
}

/// What one operand of an arithmetic operator turned out to be.
enum Operand {
    Number(f64),
    Blank,
    Other(Value),
}

fn operand(v: &Value) -> Operand {
    match v {
        Value::Empty => Operand::Blank,
        Value::Number(n) => Operand::Number(*n),
        other => Operand::Other(other.clone()),
    }
}

/// Adds, subtracts, multiplies or divides two values.
///
/// An error operand is the answer (a broken cell shows its own error, not a fresh one). A blank
/// operand keeps the result blank: an empty total must not become a zero, because a zero is a
/// claim about the data and there is none to make. Division by zero is blank rather than an error,
/// and an overflow is #NUM! rather than an infinity.
fn arithmetic(op: BinOp, a: &Value, b: &Value) -> Value {
    if let Value::Error(_) = a {
        return a.clone();
    }
    if let Value::Error(_) = b {
        return b.clone();
    }
    let (Operand::Number(x), Operand::Number(y)) = (operand(a), operand(b)) else {
        return if a.is_empty() || b.is_empty() { Value::Empty } else { Value::Error(CellError::Value) };
    };
    if op == BinOp::Div && y == 0.0 {
        return Value::Empty;
    }
    let n = match op {
        BinOp::Add => x + y,
        BinOp::Sub => x - y,
        BinOp::Mul => x * y,
        BinOp::Div => x / y,
    };
    if n.is_finite() { Value::number(n) } else { Value::Error(CellError::Num) }
}

/// One evaluation: the model, the context, and the caches that keep the relationship walk from
/// repeating itself for every fact row.
pub(crate) struct Evaluator<'m, 'c> {
    model: &'m DataModel,
    ctx: &'c Context,
    /// Which columns the context restricts, and to what.
    keys: HashMap<(String, String), Option<Arc<ValueSet>>>,
    /// Which tables the context reaches at all: a table nothing filters restricts nothing, which
    /// is not the same as restricting it to the empty set.
    constrained: HashMap<String, bool>,
    /// The columns being worked out right now. Meeting one again is the loop a hand-edited model
    /// can hold; the editor itself refuses those.
    busy: HashSet<(String, String)>,
    /// The tables whose reach is being worked out right now, for the same reason: a loop here
    /// would otherwise be walked around again for every table it touches.
    busy_tables: HashSet<String>,
}

impl<'m, 'c> Evaluator<'m, 'c> {
    pub(crate) fn new(model: &'m DataModel, ctx: &'c Context) -> Evaluator<'m, 'c> {
        Evaluator { model, ctx, keys: HashMap::new(), constrained: HashMap::new(), busy: HashSet::new(), busy_tables: HashSet::new() }
    }

    pub(crate) fn eval(&mut self, e: &Expr, stack: &mut Vec<String>) -> Result<Value, DaxError> {
        match e {
            Expr::Number(n) => Ok(Value::number(*n)),
            Expr::Text(s) => Ok(Value::text(s.as_str())),
            Expr::Neg(inner) => {
                let v = self.eval(inner, stack)?;
                Ok(match operand(&v) {
                    Operand::Number(n) => Value::number(-n),
                    Operand::Blank => Value::Empty,
                    Operand::Other(v) if v.is_error() => v,
                    Operand::Other(_) => Value::Error(CellError::Value),
                })
            }
            Expr::Binary { op, left, right } => {
                let a = self.eval(left, stack)?;
                let b = self.eval(right, stack)?;
                Ok(arithmetic(*op, &a, &b))
            }
            Expr::Column { table, column, .. } => Err(DaxError::UnaggregatedColumn { table: table.clone(), column: column.clone() }),
            Expr::Filter { .. } => Err(DaxError::Parse { at: e.at(), message: "an equals sign only works as a filter inside CALCULATE".to_string() }),
            Expr::Measure { name, .. } => self.measure(name, stack),
            Expr::Call { name, args, .. } => self.call(name, args, stack),
        }
    }

    /// A bare name is that measure's formula, evaluated in this same context. The walk is bounded
    /// by MAX_MEASURE_DEPTH and refuses a measure that comes back around.
    fn measure(&mut self, name: &str, stack: &mut Vec<String>) -> Result<Value, DaxError> {
        let model = self.model;
        let Some(m) = model.measure(name) else {
            return Err(DaxError::UnknownMeasure { name: name.to_string() });
        };
        if stack.iter().any(|s| s.eq_ignore_ascii_case(&m.name)) {
            return Err(DaxError::Cycle { measure: m.name.clone() });
        }
        if stack.len() >= MAX_MEASURE_DEPTH {
            return Err(DaxError::TooDeep { measure: m.name.clone() });
        }
        let expr = crate::parse::parse(&m.formula)?;
        stack.push(m.name.clone());
        let out = self.eval(&expr, stack);
        stack.pop();
        out
    }

    fn call(&mut self, name: &str, args: &[Expr], stack: &mut Vec<String>) -> Result<Value, DaxError> {
        if name.eq_ignore_ascii_case("BLANK") {
            if !args.is_empty() {
                return Err(DaxError::BadArity { function: "BLANK".to_string(), expected: 0, got: args.len() });
            }
            return Ok(Value::Empty);
        }
        if name.eq_ignore_ascii_case("CALCULATE") {
            return self.calculate(args, stack);
        }
        if name.eq_ignore_ascii_case("DIVIDE") {
            return self.divide(args, stack);
        }
        if let Some(kind) = aggregate(name) {
            if args.len() != 1 {
                return Err(DaxError::BadArity { function: name.to_string(), expected: 1, got: args.len() });
            }
            let Some(Expr::Column { table, column, .. }) = args.first() else {
                return Err(DaxError::NotANumber { function: name.to_string(), got: args.first().map_or_else(|| "nothing".to_string(), describe) });
            };
            let table = table.clone();
            let column = column.clone();
            return self.aggregate(kind, &table, &column);
        }
        Err(DaxError::UnknownFunction { name: name.to_string() })
    }

    /// CALCULATE: the body is evaluated in the context these filters build on top of the one it was
    /// called in, so the outer context stays and the new filters are ANDed onto it.
    fn calculate(&mut self, args: &[Expr], stack: &mut Vec<String>) -> Result<Value, DaxError> {
        let Some((body, filters)) = args.split_first() else {
            return Err(DaxError::BadArity { function: "CALCULATE".to_string(), expected: 1, got: 0 });
        };
        let mut inner = self.ctx.clone();
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
            // A filter's own value is read in the context this CALCULATE was called in, before the
            // filter applies.
            let v = self.eval(value, stack)?;
            inner = inner.filter(table, name, v);
        }
        let model = self.model;
        let mut nested = Evaluator::new(model, &inner);
        nested.eval(body, stack)
    }

    /// DIVIDE: blank when the denominator is zero or blank, the alternate when one was given, and
    /// never an error for a denominator that happens to be empty.
    fn divide(&mut self, args: &[Expr], stack: &mut Vec<String>) -> Result<Value, DaxError> {
        let (Some(numerator), Some(denominator)) = (args.first(), args.get(1)) else {
            return Err(DaxError::BadArity { function: "DIVIDE".to_string(), expected: 2, got: args.len() });
        };
        if args.len() > 3 {
            // Named with the larger of the two legal counts, so "takes 3 but got 4" reads right.
            return Err(DaxError::BadArity { function: "DIVIDE".to_string(), expected: 3, got: args.len() });
        }
        let alternate = args.get(2);
        let n = self.eval(numerator, stack)?;
        let d = self.eval(denominator, stack)?;
        let zero_or_blank = match operand(&d) {
            Operand::Blank => true,
            Operand::Number(y) => y == 0.0,
            Operand::Other(_) => false,
        };
        if zero_or_blank {
            return match alternate {
                Some(alt) => self.eval(alt, stack),
                None => Ok(Value::Empty),
            };
        }
        Ok(arithmetic(BinOp::Div, &n, &d))
    }

    /// An aggregate over one column, over exactly the rows this context leaves.
    ///
    /// A blank cell is not a number: SUM, AVERAGE, MIN and MAX skip it and answer blank when there
    /// was nothing left to add up. COUNT counts numbers, COUNTA counts everything non-blank, and
    /// DISTINCTCOUNT counts distinct non-blank values. A cell holding an error is non-blank but not
    /// a number, so it is counted by COUNTA and skipped by the rest rather than poisoning a total.
    fn aggregate(&mut self, kind: Agg, table: &str, column: &str) -> Result<Value, DaxError> {
        let model = self.model;
        let Some(t) = model.table(table) else {
            return Err(DaxError::UnknownTable { name: table.to_string() });
        };
        let Some(ci) = t.index(column) else {
            return Err(DaxError::UnknownColumn { table: t.name.clone(), column: column.to_string() });
        };
        let name = t.name.clone();
        let mut numbers = 0usize;
        let mut non_blank = 0usize;
        let mut sum = 0.0f64;
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        let mut distinct = ValueSet::default();
        for row in &t.rows {
            if !self.row_ok(&name, row, 0)? {
                continue;
            }
            // A short row has no cell to add up, which is the same as a blank one.
            let Some(v) = row.get(ci) else { continue };
            if v.is_empty() {
                continue;
            }
            non_blank += 1;
            if let Value::Number(n) = v
                && n.is_finite()
            {
                numbers += 1;
                sum += *n;
                if *n < min {
                    min = *n;
                }
                if *n > max {
                    max = *n;
                }
            }
            if kind == Agg::DistinctCount {
                distinct.insert(v);
            }
        }
        let count = numbers as f64;
        Ok(match kind {
            Agg::Sum => {
                if numbers == 0 {
                    Value::Empty
                } else if sum.is_finite() {
                    Value::number(sum)
                } else {
                    Value::Error(CellError::Num)
                }
            }
            Agg::Average => {
                if numbers == 0 {
                    Value::Empty
                } else if (sum / count).is_finite() {
                    Value::number(sum / count)
                } else {
                    Value::Error(CellError::Num)
                }
            }
            Agg::Count => Value::number(count),
            Agg::CountA => Value::number(non_blank as f64),
            Agg::Min => {
                if numbers == 0 {
                    Value::Empty
                } else {
                    Value::number(min)
                }
            }
            Agg::Max => {
                if numbers == 0 {
                    Value::Empty
                } else {
                    Value::number(max)
                }
            }
            Agg::DistinctCount => Value::number(distinct.len() as f64),
        })
    }

    /// Whether one row of a table survives the context.
    fn row_ok(&mut self, table: &str, row: &[Value], depth: usize) -> Result<bool, DaxError> {
        let ctx = self.ctx;
        let model = self.model;
        // The filters written against this table itself.
        for (f_table, f_column, f_value) in ctx.filters() {
            if !f_table.eq_ignore_ascii_case(table) {
                continue;
            }
            let Some(t) = model.table(table) else {
                return Err(DaxError::UnknownTable { name: table.to_string() });
            };
            let Some(ci) = t.index(f_column) else {
                return Err(DaxError::UnknownColumn { table: t.name.clone(), column: f_column.clone() });
            };
            let ok = match row.get(ci) {
                Some(v) => same_value(v, f_value),
                None => f_value.is_empty(),
            };
            if !ok {
                return Ok(false);
            }
        }
        // The member lists written against this table: a row survives when its value is one of
        // them. An empty list keeps nothing, which is what a bucket with no members means.
        for (f_table, f_column, f_values) in ctx.sets() {
            if !f_table.eq_ignore_ascii_case(table) {
                continue;
            }
            let Some(t) = model.table(table) else {
                return Err(DaxError::UnknownTable { name: table.to_string() });
            };
            let Some(ci) = t.index(f_column) else {
                return Err(DaxError::UnknownColumn { table: t.name.clone(), column: f_column.clone() });
            };
            let hit = match row.get(ci) {
                Some(v) => f_values.iter().any(|f| same_value(v, f)),
                None => f_values.iter().any(Value::is_empty),
            };
            if !hit {
                return Ok(false);
            }
        }
        // The filters that reach this table from a lookup table, several hops away included.
        let incoming: Vec<Relationship> = model.incoming(table).cloned().collect();
        for r in &incoming {
            // One step along the walk, in the same hop: the depth below counts hops, not calls.
            let Some(keys) = self.keys_for(&r.to_table, &r.to_column, depth)? else {
                continue;
            };
            let Some(t) = model.table(table) else {
                return Err(DaxError::UnknownTable { name: table.to_string() });
            };
            let Some(ci) = t.index(&r.from_column) else {
                return Err(DaxError::UnknownColumn { table: t.name.clone(), column: r.from_column.clone() });
            };
            let ok = match row.get(ci) {
                Some(v) => keys.contains(v),
                None => keys.contains(&Value::Empty),
            };
            if !ok {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The values of one column the context leaves, or None when nothing restricts that table: a
    /// table no filter reaches must not restrict the tables it relates to, or a fact row whose key
    /// is missing from its lookup would silently disappear from every total.
    fn keys_for(&mut self, table: &str, column: &str, depth: usize) -> Result<Option<Arc<ValueSet>>, DaxError> {
        // A walk that has taken more hops than the model has relationships has gone round: the
        // editor refuses such a model, but a hand-edited file could hold one.
        if depth > self.model.relationships.len() {
            return Err(DaxError::RelationshipLoop { table: table.to_string() });
        }
        let key = (table.to_ascii_lowercase(), column.to_ascii_lowercase());
        if let Some(done) = self.keys.get(&key) {
            return Ok(done.clone());
        }
        if self.busy.contains(&key) {
            return Err(DaxError::RelationshipLoop { table: table.to_string() });
        }
        if !self.constrained(table, depth)? {
            self.keys.insert(key, None);
            return Ok(None);
        }
        let model = self.model;
        let Some(t) = model.table(table) else {
            return Err(DaxError::UnknownTable { name: table.to_string() });
        };
        let Some(ci) = t.index(column) else {
            return Err(DaxError::UnknownColumn { table: t.name.clone(), column: column.to_string() });
        };
        let name = t.name.clone();
        self.busy.insert(key.clone());
        let mut set = ValueSet::default();
        for row in &t.rows {
            if self.row_ok(&name, row, depth)?
                && let Some(v) = row.get(ci)
            {
                set.insert(v);
            }
        }
        self.busy.remove(&key);
        let out = Arc::new(set);
        self.keys.insert(key, Some(out.clone()));
        Ok(Some(out))
    }

    /// Whether the context touches a table at all, directly or through a relationship.
    fn constrained(&mut self, table: &str, depth: usize) -> Result<bool, DaxError> {
        let lower = table.to_ascii_lowercase();
        if let Some(done) = self.constrained.get(&lower) {
            return Ok(*done);
        }
        if !self.busy_tables.insert(lower.clone()) {
            return Err(DaxError::RelationshipLoop { table: table.to_string() });
        }
        let ctx = self.ctx;
        let model = self.model;
        let mut yes =
            ctx.filters().iter().any(|(t, _, _)| t.eq_ignore_ascii_case(table)) || ctx.sets().iter().any(|(t, _, _)| t.eq_ignore_ascii_case(table));
        if !yes {
            let incoming: Vec<Relationship> = model.incoming(table).cloned().collect();
            for r in &incoming {
                if self.keys_for(&r.to_table, &r.to_column, depth + 1)?.is_some() {
                    yes = true;
                    break;
                }
            }
        }
        self.busy_tables.remove(&lower);
        self.constrained.insert(lower, yes);
        Ok(yes)
    }
}
