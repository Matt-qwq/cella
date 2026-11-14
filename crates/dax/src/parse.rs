//! The language: text to tokens to an Expr tree.
//!
//! The subset is small on purpose, and the parser is the whole definition of it:
//!
//! | shape | example |
//! | --- | --- |
//! | number, text | 1, 2.5, "West" |
//! | column | Sales[Amount] |
//! | aggregate over one column | SUM(Sales[Amount]), DISTINCTCOUNT(Region[Name]) |
//! | call | DIVIDE(a, b), CALCULATE(expr, filter, ...), BLANK() |
//! | arithmetic | + - * /, unary minus, parentheses |
//! | filter (only inside CALCULATE) | Region[Name] = "West" |
//! | measure | Total -- a bare name resolves to that measure's formula |
//!
//! Everything here fails with a DaxError::Parse that carries the 0-based character offset it
//! stopped at, and every recursion is bounded by MAX_DEPTH.

use crate::{DaxError, MAX_DEPTH, MAX_FORMULA_LEN};

/// The two sides of an arithmetic operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
}

/// A parsed formula.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Expr {
    Number(f64),
    Text(String),
    /// Table[Column]: meaningful only as the single argument of an aggregate.
    Column {
        table: String,
        column: String,
        at: usize,
    },
    /// A bare name: a measure, resolved both when the formula is checked and when it is evaluated.
    Measure {
        name: String,
        at: usize,
    },
    Call {
        name: String,
        args: Vec<Expr>,
        at: usize,
    },
    Neg(Box<Expr>),
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// Table[Column] = <expr>: only ever an argument of CALCULATE.
    Filter {
        column: Box<Expr>,
        value: Box<Expr>,
        at: usize,
    },
}

impl Expr {
    /// The character offset this expression starts at, for an error that can point at it.
    pub(crate) fn at(&self) -> usize {
        match self {
            Expr::Column { at, .. } | Expr::Measure { at, .. } | Expr::Call { at, .. } | Expr::Filter { at, .. } => *at,
            Expr::Neg(inner) => inner.at(),
            Expr::Binary { left, .. } => left.at(),
            Expr::Number(_) | Expr::Text(_) => 0,
        }
    }
}

/// The aggregates the subset knows, each over exactly one column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Agg {
    Sum,
    Average,
    Count,
    CountA,
    Min,
    Max,
    DistinctCount,
}

/// The aggregate a name stands for, or None for a name that is not one.
pub(crate) fn aggregate(name: &str) -> Option<Agg> {
    let n = name.trim();
    if n.eq_ignore_ascii_case("SUM") {
        Some(Agg::Sum)
    } else if n.eq_ignore_ascii_case("AVERAGE") {
        Some(Agg::Average)
    } else if n.eq_ignore_ascii_case("COUNT") {
        Some(Agg::Count)
    } else if n.eq_ignore_ascii_case("COUNTA") {
        Some(Agg::CountA)
    } else if n.eq_ignore_ascii_case("MIN") {
        Some(Agg::Min)
    } else if n.eq_ignore_ascii_case("MAX") {
        Some(Agg::Max)
    } else if n.eq_ignore_ascii_case("DISTINCTCOUNT") {
        Some(Agg::DistinctCount)
    } else {
        None
    }
}

/// How an expression reads in an error message: "SUM needs one column ... but got a formula".
pub(crate) fn describe(e: &Expr) -> String {
    match e {
        Expr::Number(n) => format!("the number {n}"),
        Expr::Text(s) => format!("the text \"{s}\""),
        Expr::Column { table, column, .. } => format!("the column {table}[{column}]"),
        Expr::Measure { name, .. } => format!("the measure \"{name}\""),
        Expr::Call { name, .. } if name.eq_ignore_ascii_case("BLANK") => "BLANK()".to_string(),
        Expr::Call { name, .. } => format!("{name}(...)"),
        Expr::Neg(_) => "a signed value".to_string(),
        Expr::Binary { .. } => "a formula".to_string(),
        Expr::Filter { .. } => "a filter".to_string(),
    }
}

// --------------------------------------------------------------------------------- the tokens

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Number(f64),
    Text(String),
    Ident(String),
    /// [Name]: the column half of a reference, kept raw so a name may hold spaces.
    Column(String),
    LParen,
    RParen,
    Comma,
    Plus,
    Minus,
    Star,
    Slash,
    Equal,
}

impl Tok {
    fn describe(&self) -> String {
        match self {
            Tok::Number(n) => format!("the number {n}"),
            Tok::Text(s) => format!("the text \"{s}\""),
            Tok::Ident(name) => format!("\"{name}\""),
            Tok::Column(name) => format!("\"[{name}]\" (a column needs its table)"),
            Tok::LParen => "\"(\"".to_string(),
            Tok::RParen => "\")\"".to_string(),
            Tok::Comma => "\",\"".to_string(),
            Tok::Plus => "\"+\"".to_string(),
            Tok::Minus => "\"-\"".to_string(),
            Tok::Star => "\"*\"".to_string(),
            Tok::Slash => "\"/\"".to_string(),
            Tok::Equal => "\"=\"".to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Token {
    tok: Tok,
    /// 0-based character offset in the formula.
    at: usize,
}

/// The one character that opens and closes a text value. Kept named so the lexer never has to
/// spell a bare quote inside another one.
const QUOTE: char = '"';

fn digit(c: &char) -> bool {
    c.is_ascii_digit()
}

/// Splits the formula into tokens. Offsets count characters, not bytes, so a formula holding
/// non-ASCII text still reports a position a person can point at.
fn lex(chars: &[char]) -> Result<Vec<Token>, DaxError> {
    let mut out: Vec<Token> = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        let Some(&c) = chars.get(i) else { break };
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let at = i;
        let tok = match c {
            '(' => {
                i += 1;
                Tok::LParen
            }
            ')' => {
                i += 1;
                Tok::RParen
            }
            ',' => {
                i += 1;
                Tok::Comma
            }
            '+' => {
                i += 1;
                Tok::Plus
            }
            '-' => {
                i += 1;
                Tok::Minus
            }
            '*' => {
                i += 1;
                Tok::Star
            }
            '/' => {
                i += 1;
                Tok::Slash
            }
            '=' => {
                i += 1;
                Tok::Equal
            }
            _ if c == QUOTE => {
                let mut text = String::new();
                let mut j = i + 1;
                loop {
                    match chars.get(j) {
                        None => {
                            return Err(DaxError::Parse { at, message: "a text value needs a closing quote".to_string() });
                        }
                        Some(&next) if next == QUOTE && chars.get(j + 1) != Some(&QUOTE) => {
                            j += 1;
                            break;
                        }
                        Some(&next) if next == QUOTE => {
                            text.push(QUOTE);
                            j += 2;
                        }
                        Some(other) => {
                            text.push(*other);
                            j += 1;
                        }
                    }
                }
                i = j;
                Tok::Text(text)
            }
            '[' => {
                let mut name = String::new();
                let mut j = i + 1;
                loop {
                    match chars.get(j) {
                        None => {
                            return Err(DaxError::Parse { at, message: "a column name needs a closing bracket".to_string() });
                        }
                        Some(']') => {
                            j += 1;
                            break;
                        }
                        Some(other) => {
                            name.push(*other);
                            j += 1;
                        }
                    }
                }
                let name = name.trim().to_string();
                if name.is_empty() {
                    return Err(DaxError::Parse { at, message: "the brackets are empty; a column is written as Sales[Amount]".to_string() });
                }
                i = j;
                Tok::Column(name)
            }
            _ if digit(&c) => {
                let mut j = i;
                while chars.get(j).is_some_and(digit) {
                    j += 1;
                }
                if chars.get(j) == Some(&'.') && chars.get(j + 1).is_some_and(digit) {
                    j += 1;
                    while chars.get(j).is_some_and(digit) {
                        j += 1;
                    }
                }
                let text: String = chars.get(i..j).unwrap_or_default().iter().collect();
                let n = text.parse::<f64>().map_err(|_| DaxError::Parse { at, message: format!("the number \"{text}\" can't be read") })?;
                if !n.is_finite() {
                    return Err(DaxError::Parse { at, message: format!("the number \"{text}\" is too large") });
                }
                i = j;
                Tok::Number(n)
            }
            _ if c.is_alphabetic() || c == '_' => {
                let mut j = i;
                while chars.get(j).is_some_and(|c| c.is_alphanumeric() || *c == '_') {
                    j += 1;
                }
                let text: String = chars.get(i..j).unwrap_or_default().iter().collect();
                i = j;
                Tok::Ident(text)
            }
            other => {
                return Err(DaxError::Parse { at, message: format!("the character \"{other}\" has no meaning here") });
            }
        };
        out.push(Token { tok, at });
    }
    Ok(out)
}

// -------------------------------------------------------------------------------- the parser

/// Parses one measure formula. Length is refused before anything is read; nesting is refused at
/// MAX_DEPTH levels.
pub(crate) fn parse(src: &str) -> Result<Expr, DaxError> {
    let chars: Vec<char> = src.chars().collect();
    if chars.len() > MAX_FORMULA_LEN {
        return Err(DaxError::Parse { at: MAX_FORMULA_LEN, message: format!("the formula is longer than {MAX_FORMULA_LEN} characters") });
    }
    if chars.is_empty() {
        return Err(DaxError::Parse { at: 0, message: "the formula is empty".to_string() });
    }
    let tokens = lex(&chars)?;
    let mut p = Parser { tokens, src_len: chars.len(), pos: 0 };
    let expr = p.expr(0, false)?;
    if let Some(token) = p.tokens.get(p.pos) {
        return Err(DaxError::Parse {
            at: token.at,
            message: format!("the formula continues with {} after a complete expression", token.tok.describe()),
        });
    }
    Ok(expr)
}

struct Parser {
    tokens: Vec<Token>,
    src_len: usize,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn peek_tok(&self) -> Option<&Tok> {
        self.peek().map(|t| &t.tok)
    }

    fn is(&self, t: &Tok) -> bool {
        self.peek_tok() == Some(t)
    }

    /// The offset of what comes next (the formula's length at the end).
    fn at(&self) -> usize {
        self.peek().map_or(self.src_len, |t| t.at)
    }

    fn bump(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn expect(&mut self, want: &Tok, what: &str) -> Result<(), DaxError> {
        match self.bump() {
            Some(token) if &token.tok == want => Ok(()),
            Some(token) => Err(DaxError::Parse { at: token.at, message: format!("expected {what} but found {}", token.tok.describe()) }),
            None => Err(DaxError::Parse { at: self.src_len, message: format!("expected {what} but the formula ends") }),
        }
    }

    fn guard(&self, depth: usize) -> Result<(), DaxError> {
        if depth > MAX_DEPTH {
            return Err(DaxError::Parse { at: self.at(), message: format!("the formula nests more than {MAX_DEPTH} levels deep") });
        }
        Ok(())
    }

    /// An expression. An equality is only accepted where filters is true, which is inside CALCULATE
    /// (or in parentheses directly there).
    fn expr(&mut self, depth: usize, filters: bool) -> Result<Expr, DaxError> {
        self.guard(depth)?;
        let start = self.at();
        let left = self.additive(depth, filters)?;
        if self.is(&Tok::Equal) {
            if !filters {
                return Err(DaxError::Parse { at: self.at(), message: "an equals sign only works as a filter inside CALCULATE".to_string() });
            }
            self.bump();
            let value = self.additive(depth, false)?;
            return Ok(Expr::Filter { column: Box::new(left), value: Box::new(value), at: start });
        }
        Ok(left)
    }

    fn additive(&mut self, depth: usize, filters: bool) -> Result<Expr, DaxError> {
        let mut left = self.multiplicative(depth, filters)?;
        loop {
            let op = match self.peek_tok() {
                Some(Tok::Plus) => BinOp::Add,
                Some(Tok::Minus) => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let right = self.multiplicative(depth, filters)?;
            left = Expr::Binary { op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn multiplicative(&mut self, depth: usize, filters: bool) -> Result<Expr, DaxError> {
        let mut left = self.unary(depth, filters)?;
        loop {
            let op = match self.peek_tok() {
                Some(Tok::Star) => BinOp::Mul,
                Some(Tok::Slash) => BinOp::Div,
                _ => break,
            };
            self.bump();
            let right = self.unary(depth, filters)?;
            left = Expr::Binary { op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn unary(&mut self, depth: usize, filters: bool) -> Result<Expr, DaxError> {
        self.guard(depth)?;
        match self.peek_tok() {
            Some(Tok::Minus) => {
                self.bump();
                Ok(Expr::Neg(Box::new(self.unary(depth + 1, filters)?)))
            }
            Some(Tok::Plus) => {
                self.bump();
                self.unary(depth + 1, filters)
            }
            _ => self.primary(depth, filters),
        }
    }

    fn primary(&mut self, depth: usize, filters: bool) -> Result<Expr, DaxError> {
        self.guard(depth)?;
        let Some(token) = self.bump() else {
            return Err(DaxError::Parse { at: self.src_len, message: "the formula ends where a value should be".to_string() });
        };
        match token.tok {
            Tok::Number(n) => Ok(Expr::Number(n)),
            Tok::Text(s) => Ok(Expr::Text(s)),
            Tok::LParen => {
                let inner = self.expr(depth + 1, filters)?;
                self.expect(&Tok::RParen, "a closing parenthesis")?;
                Ok(inner)
            }
            Tok::Ident(name) => {
                if let Some(Tok::Column(column)) = self.peek_tok() {
                    let column = column.clone();
                    self.bump();
                    return Ok(Expr::Column { table: name, column, at: token.at });
                }
                if self.is(&Tok::LParen) {
                    self.bump();
                    // Only CALCULATE takes filters; everywhere else an equals sign is a mistake.
                    let allow_filters = name.eq_ignore_ascii_case("CALCULATE");
                    let mut args: Vec<Expr> = Vec::new();
                    if !self.is(&Tok::RParen) {
                        loop {
                            args.push(self.expr(depth + 1, allow_filters)?);
                            if self.is(&Tok::Comma) {
                                self.bump();
                                continue;
                            }
                            break;
                        }
                    }
                    self.expect(&Tok::RParen, "a closing parenthesis")?;
                    return Ok(Expr::Call { name, args, at: token.at });
                }
                Ok(Expr::Measure { name, at: token.at })
            }
            Tok::Column(column) => {
                Err(DaxError::Parse { at: token.at, message: format!("the column \"[{column}]\" needs its table, as in Sales[{column}]") })
            }
            other => Err(DaxError::Parse { at: token.at, message: format!("expected a value but found {}", other.describe()) }),
        }
    }
}
