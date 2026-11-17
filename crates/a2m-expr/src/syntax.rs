//! The grammar: text to a tree, and a tree back to canonical text.
//!
//! The syntax is the part of CEL the file formats use: names, field access,
//! indexing, calls, the operators, the conditional, list and record literals,
//! and the macros `has`, `all`, `exists`, `exists_one`, `map`, and `filter`.
//! Numbers with a decimal point or an exponent, unsigned numbers, and bytes
//! are refused here, with a place, so no float can enter the tree.

use crate::error::{ExprError, Span};

/// The longest source text accepted, in bytes.
pub(crate) const MAX_SOURCE_BYTES: usize = 16 * 1024;
/// The deepest nesting accepted.
const MAX_DEPTH: u32 = 64;

/// The macros that bind a name over a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MacroKind {
    All,
    Exists,
    ExistsOne,
    Map,
    Filter,
}

impl MacroKind {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Exists => "exists",
            Self::ExistsOne => "exists_one",
            Self::Map => "map",
            Self::Filter => "filter",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "all" => Some(Self::All),
            "exists" => Some(Self::Exists),
            "exists_one" => Some(Self::ExistsOne),
            "map" => Some(Self::Map),
            "filter" => Some(Self::Filter),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnaryOp {
    Not,
    Neg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaryOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    In,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl BinaryOp {
    pub(crate) const fn symbol(self) -> &'static str {
        match self {
            Self::Or => "||",
            Self::And => "&&",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::In => "in",
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Rem => "%",
        }
    }

    /// Binding strength; higher binds tighter.
    const fn level(self) -> u8 {
        match self {
            Self::Or => 2,
            Self::And => 3,
            Self::Eq | Self::Ne | Self::Lt | Self::Le | Self::Gt | Self::Ge | Self::In => 4,
            Self::Add | Self::Sub => 5,
            Self::Mul | Self::Div | Self::Rem => 6,
        }
    }
}

const LEVEL_COND: u8 = 1;
const LEVEL_UNARY: u8 = 7;
const LEVEL_MEMBER: u8 = 8;

/// One node of the tree, with its place in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Node {
    pub kind: Kind,
    pub span: Span,
    /// One for a leaf, one more than its tallest part otherwise. Bounded, so
    /// that walking a tree never needs a deep stack.
    height: u32,
}

/// Makes a node, and refuses a tree taller than the limit.
fn node(kind: Kind, span: Span) -> Result<Node, ExprError> {
    let tallest = match &kind {
        Kind::Null | Kind::Bool(_) | Kind::Int(_) | Kind::Text(_) | Kind::Name(_) => 0,
        Kind::Field(a, _) | Kind::Unary(_, a) | Kind::Has(a, _) => a.height,
        Kind::Index(a, b) | Kind::Binary(_, a, b) | Kind::Macro(_, a, _, b) => {
            a.height.max(b.height)
        }
        Kind::Cond(a, b, c) => a.height.max(b.height).max(c.height),
        Kind::Call(_, items) | Kind::List(items) => {
            items.iter().map(|item| item.height).max().unwrap_or(0)
        }
        Kind::Method(a, _, items) => items
            .iter()
            .map(|item| item.height)
            .max()
            .unwrap_or(0)
            .max(a.height),
        Kind::Record(fields) => fields
            .iter()
            .map(|(_, value)| value.height)
            .max()
            .unwrap_or(0),
    };
    if tallest >= MAX_DEPTH {
        return Err(
            ExprError::invalid("the expression is nested too deeply", span)
                .with_fix("Split the work into steps."),
        );
    }
    Ok(Node {
        kind,
        span,
        height: tallest + 1,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Kind {
    Null,
    Bool(bool),
    Int(i64),
    Text(String),
    Name(String),
    /// `a.f`
    Field(Box<Node>, String),
    /// `a[i]`
    Index(Box<Node>, Box<Node>),
    Unary(UnaryOp, Box<Node>),
    Binary(BinaryOp, Box<Node>, Box<Node>),
    /// `c ? a : b`
    Cond(Box<Node>, Box<Node>, Box<Node>),
    /// `f(args)`: a host function.
    Call(String, Vec<Node>),
    /// `a.f(args)`: one of CEL's member functions.
    Method(Box<Node>, String, Vec<Node>),
    /// `list.all(x, body)` and its kin.
    Macro(MacroKind, Box<Node>, String, Box<Node>),
    /// `has(a.f)`
    Has(Box<Node>, String),
    List(Vec<Node>),
    /// `{"k": v}`; keys are text literals, in the order written.
    Record(Vec<(String, Node)>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Ident(String),
    Int(u64),
    Text(String),
    Sym(&'static str),
}

const SYMBOLS: [&str; 22] = [
    "==", "!=", "<=", ">=", "&&", "||", "(", ")", "[", "]", "{", "}", ".", ",", ":", "?", "+", "-",
    "*", "/", "%", "!",
];

fn span(start: usize, end: usize) -> Span {
    // The source is at most MAX_SOURCE_BYTES long, so both fit.
    Span::new(
        u32::try_from(start).unwrap_or(u32::MAX),
        u32::try_from(end).unwrap_or(u32::MAX),
    )
}

fn lex(source: &str) -> Result<Vec<(Tok, Span)>, ExprError> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if byte == b' ' || byte == b'\t' || byte == b'\n' || byte == b'\r' {
            at += 1;
            continue;
        }
        if byte.is_ascii_alphabetic() || byte == b'_' {
            let start = at;
            while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                at += 1;
            }
            let word = &source[start..at];
            if (word == "b" || word == "B" || word == "r" || word == "R")
                && at < bytes.len()
                && (bytes[at] == b'"' || bytes[at] == b'\'')
            {
                return Err(ExprError::type_error(
                    "bytes and raw text literals are not part of the expression language",
                    span(start, at + 1),
                )
                .with_fix("Write a plain text literal in double quotes."));
            }
            tokens.push((Tok::Ident(word.to_owned()), span(start, at)));
            continue;
        }
        if byte.is_ascii_digit() {
            let (token, end) = lex_number(source, at)?;
            tokens.push((token, span(at, end)));
            at = end;
            continue;
        }
        if byte == b'"' || byte == b'\'' {
            let (text, end) = lex_text(source, at)?;
            tokens.push((Tok::Text(text), span(at, end)));
            at = end;
            continue;
        }
        if byte == b'<' || byte == b'>' {
            let two = at + 1 < bytes.len() && bytes[at + 1] == b'=';
            let symbol = match (byte, two) {
                (b'<', true) => "<=",
                (b'<', false) => "<",
                (_, true) => ">=",
                (_, false) => ">",
            };
            tokens.push((Tok::Sym(symbol), span(at, at + symbol.len())));
            at += symbol.len();
            continue;
        }
        let rest = &source[at..];
        if let Some(symbol) = SYMBOLS.iter().find(|symbol| rest.starts_with(**symbol)) {
            tokens.push((Tok::Sym(symbol), span(at, at + symbol.len())));
            at += symbol.len();
            continue;
        }
        let width = rest.chars().next().map_or(1, char::len_utf8);
        let message = if byte == b'=' {
            "a single `=` is not an operator"
        } else if byte == b'&' || byte == b'|' {
            "`&` and `|` are not operators"
        } else {
            "this character cannot start any part of an expression"
        };
        let error = ExprError::invalid(message, span(at, at + width));
        return Err(match byte {
            b'=' => error.with_fix("Write `==` to compare."),
            b'&' => error.with_fix("Write `&&`."),
            b'|' => error.with_fix("Write `||`."),
            _ => error,
        });
    }
    Ok(tokens)
}

fn lex_number(source: &str, start: usize) -> Result<(Tok, usize), ExprError> {
    let bytes = source.as_bytes();
    let mut at = start;
    while at < bytes.len() && bytes[at].is_ascii_digit() {
        at += 1;
    }
    let digits = &source[start..at];
    let next = bytes.get(at).copied();
    let after = bytes.get(at + 1).copied();
    let is_double = match next {
        Some(b'.') => after.is_some_and(|byte| byte.is_ascii_digit()),
        Some(b'e' | b'E') => {
            after.is_some_and(|byte| byte.is_ascii_digit() || byte == b'+' || byte == b'-')
        }
        _ => false,
    };
    if is_double {
        // Take in the rest of the literal so the place covers all of it.
        let mut end = at + 1;
        while end < bytes.len()
            && (bytes[end].is_ascii_alphanumeric()
                || bytes[end] == b'.'
                || ((bytes[end] == b'+' || bytes[end] == b'-')
                    && matches!(bytes[end - 1], b'e' | b'E')))
        {
            end += 1;
        }
        let literal = &source[start..end];
        return Err(ExprError::type_error(
            "a number with a decimal point or an exponent is a float, and an expression has no float",
            span(start, end),
        )
        .with_fix(format!("Write decimal(\"{literal}\") for an exact decimal.")));
    }
    if next == Some(b'u') || next == Some(b'U') {
        return Err(ExprError::type_error(
            "unsigned numbers are not part of the expression language",
            span(start, at + 1),
        )
        .with_fix(format!("Write {digits}.")));
    }
    if next.is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_') {
        return Err(
            ExprError::invalid("a number is followed by a letter", span(start, at + 1))
                .with_fix("Write whole numbers in decimal digits only."),
        );
    }
    let value: u64 = digits.parse().map_err(|_| {
        ExprError::invalid("this whole number is too large", span(start, at))
            .with_fix("Write decimal(\"...\") for a number this large.")
    })?;
    Ok((Tok::Int(value), at))
}

fn lex_text(source: &str, start: usize) -> Result<(String, usize), ExprError> {
    let quote = source.as_bytes()[start];
    let mut text = String::new();
    let mut chars = source[start + 1..].char_indices();
    let offset = start + 1;
    while let Some((index, ch)) = chars.next() {
        let here = offset + index;
        if ch as u32 == u32::from(quote) {
            return Ok((text, here + 1));
        }
        if ch == '\n' || ch == '\r' {
            break;
        }
        if ch != '\\' {
            text.push(ch);
            continue;
        }
        let Some((_, escape)) = chars.next() else {
            break;
        };
        let digits = match escape {
            '\\' | '"' | '\'' => {
                text.push(escape);
                continue;
            }
            'n' => {
                text.push('\n');
                continue;
            }
            'r' => {
                text.push('\r');
                continue;
            }
            't' => {
                text.push('\t');
                continue;
            }
            'u' => 4,
            'U' => 8,
            _ => {
                return Err(ExprError::invalid(
                    "this escape is not known",
                    span(here, here + 1 + escape.len_utf8()),
                )
                .with_fix("Use \\\\, \\\", \\', \\n, \\r, \\t, \\uXXXX, or \\UXXXXXXXX."));
            }
        };
        let mut code: u32 = 0;
        for _ in 0..digits {
            let digit = chars.next().and_then(|(_, ch)| ch.to_digit(16));
            let Some(digit) = digit else {
                return Err(ExprError::invalid(
                    "a \\u escape needs four hex digits and a \\U escape eight",
                    span(here, here + 2),
                ));
            };
            code = code * 16 + digit;
        }
        let Some(ch) = char::from_u32(code) else {
            return Err(ExprError::invalid(
                "this escape is not a character",
                span(here, here + 2 + digits),
            ));
        };
        text.push(ch);
    }
    Err(
        ExprError::invalid("a text literal is not closed", span(start, start + 1))
            .with_fix("Close the text with the quote that opened it, on the same line."),
    )
}

struct Parser {
    tokens: Vec<(Tok, Span)>,
    at: usize,
    depth: u32,
    end: Span,
}

/// Parses `source` into a tree.
pub(crate) fn parse(source: &str) -> Result<Node, ExprError> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(
            ExprError::invalid("the expression is longer than 16 KiB", Span::new(0, 0))
                .with_fix("Split the work into steps, or move it into a field type."),
        );
    }
    let tokens = lex(source)?;
    let end = span(source.len(), source.len());
    let mut parser = Parser {
        tokens,
        at: 0,
        depth: 0,
        end,
    };
    if parser.tokens.is_empty() {
        return Err(ExprError::invalid("the expression is empty", end));
    }
    let tree = parser.expr()?;
    if let Some((_, extra)) = parser.tokens.get(parser.at) {
        return Err(ExprError::invalid(
            "the expression ends before this",
            *extra,
        ));
    }
    Ok(tree)
}

impl Parser {
    fn peek_sym(&self, symbol: &str) -> bool {
        matches!(self.tokens.get(self.at), Some((Tok::Sym(found), _)) if *found == symbol)
    }

    fn here(&self) -> Span {
        self.tokens.get(self.at).map_or(self.end, |(_, span)| *span)
    }

    fn last(&self) -> Span {
        if self.at == 0 {
            return self.end;
        }
        self.tokens
            .get(self.at - 1)
            .map_or(self.end, |(_, span)| *span)
    }

    fn take_sym(&mut self, symbol: &str) -> bool {
        if self.peek_sym(symbol) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect_sym(&mut self, symbol: &str, message: &str) -> Result<Span, ExprError> {
        if self.peek_sym(symbol) {
            self.at += 1;
            Ok(self.last())
        } else {
            Err(ExprError::invalid(message, self.here()))
        }
    }

    fn enter(&mut self) -> Result<(), ExprError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(
                ExprError::invalid("the expression is nested too deeply", self.here())
                    .with_fix("Split the work into steps."),
            );
        }
        Ok(())
    }

    fn expr(&mut self) -> Result<Node, ExprError> {
        self.enter()?;
        let condition = self.binary(2)?;
        let whole = if self.take_sym("?") {
            let then = self.binary(2)?;
            self.expect_sym(":", "a conditional needs `:` and a second branch")?;
            let otherwise = self.expr()?;
            let span = condition.span.to(otherwise.span);
            node(
                Kind::Cond(Box::new(condition), Box::new(then), Box::new(otherwise)),
                span,
            )?
        } else {
            condition
        };
        self.depth -= 1;
        Ok(whole)
    }

    fn binary_op(&self, level: u8) -> Option<BinaryOp> {
        let op = match self.tokens.get(self.at) {
            Some((Tok::Sym(symbol), _)) => match *symbol {
                "||" => BinaryOp::Or,
                "&&" => BinaryOp::And,
                "==" => BinaryOp::Eq,
                "!=" => BinaryOp::Ne,
                "<" => BinaryOp::Lt,
                "<=" => BinaryOp::Le,
                ">" => BinaryOp::Gt,
                ">=" => BinaryOp::Ge,
                "+" => BinaryOp::Add,
                "-" => BinaryOp::Sub,
                "*" => BinaryOp::Mul,
                "/" => BinaryOp::Div,
                "%" => BinaryOp::Rem,
                _ => return None,
            },
            Some((Tok::Ident(word), _)) if word == "in" => BinaryOp::In,
            _ => return None,
        };
        (op.level() == level).then_some(op)
    }

    /// Left-associative operators at `level`, over operands one level tighter.
    fn binary(&mut self, level: u8) -> Result<Node, ExprError> {
        if level >= LEVEL_UNARY {
            return self.unary();
        }
        let mut left = self.binary(level + 1)?;
        while let Some(op) = self.binary_op(level) {
            self.at += 1;
            let right = self.binary(level + 1)?;
            let span = left.span.to(right.span);
            left = node(Kind::Binary(op, Box::new(left), Box::new(right)), span)?;
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Node, ExprError> {
        let start = self.here();
        let op = if self.take_sym("!") {
            UnaryOp::Not
        } else if self.take_sym("-") {
            UnaryOp::Neg
        } else {
            return self.member();
        };
        self.enter()?;
        // A whole number one past the largest is allowed only straight after a minus.
        let operand = if op == UnaryOp::Neg
            && matches!(self.tokens.get(self.at), Some((Tok::Int(value), _)) if *value == 1 << 63)
            && !matches!(self.tokens.get(self.at + 1), Some((Tok::Sym("." | "["), _)))
        {
            self.at += 1;
            self.depth -= 1;
            return node(Kind::Int(i64::MIN), start.to(self.last()));
        } else {
            self.unary()?
        };
        self.depth -= 1;
        let span = start.to(operand.span);
        // Minus over a whole-number literal is that number, so `-5` has one form.
        if let (UnaryOp::Neg, Kind::Int(value)) = (op, &operand.kind) {
            let negated = value.checked_neg().ok_or_else(|| {
                ExprError::invalid("this whole number is too large", span)
                    .with_fix("Write decimal(\"...\") for a number this large.")
            })?;
            return node(Kind::Int(negated), span);
        }
        node(Kind::Unary(op, Box::new(operand)), span)
    }

    fn member(&mut self) -> Result<Node, ExprError> {
        let mut subject = self.primary()?;
        loop {
            if self.take_sym(".") {
                let (name, name_span) = self.ident("a field or function name follows `.`")?;
                if self.peek_sym("(") {
                    let (args, close) = self.args()?;
                    let span = subject.span.to(close);
                    subject = method(subject, name, name_span, args, span)?;
                } else {
                    let span = subject.span.to(name_span);
                    subject = node(Kind::Field(Box::new(subject), name), span)?;
                }
            } else if self.take_sym("[") {
                let index = self.expr()?;
                let close = self.expect_sym("]", "an index needs a closing `]`")?;
                let span = subject.span.to(close);
                subject = node(Kind::Index(Box::new(subject), Box::new(index)), span)?;
            } else {
                return Ok(subject);
            }
        }
    }

    fn ident(&mut self, message: &str) -> Result<(String, Span), ExprError> {
        match self.tokens.get(self.at) {
            Some((Tok::Ident(name), span)) => {
                let found = (name.clone(), *span);
                self.at += 1;
                Ok(found)
            }
            _ => Err(ExprError::invalid(message, self.here())),
        }
    }

    /// `( a, b, ... )`, with the opening bracket next. Gives the closing bracket's place.
    fn args(&mut self) -> Result<(Vec<Node>, Span), ExprError> {
        self.expect_sym("(", "a call needs `(`")?;
        let mut args = Vec::new();
        loop {
            if self.take_sym(")") {
                return Ok((args, self.last()));
            }
            args.push(self.expr()?);
            if !self.take_sym(",") {
                let close = self.expect_sym(")", "a call needs `,` or a closing `)`")?;
                return Ok((args, close));
            }
        }
    }

    fn primary(&mut self) -> Result<Node, ExprError> {
        let start = self.here();
        let Some((token, _)) = self.tokens.get(self.at).cloned() else {
            return Err(ExprError::invalid(
                "the expression ends where a value should be",
                self.end,
            ));
        };
        self.at += 1;
        match token {
            Tok::Int(value) => {
                let value = i64::try_from(value).map_err(|_| {
                    ExprError::invalid("this whole number is too large", start)
                        .with_fix("Write decimal(\"...\") for a number this large.")
                })?;
                node(Kind::Int(value), start)
            }
            Tok::Text(text) => node(Kind::Text(text), start),
            Tok::Ident(word) => self.word(word, start),
            Tok::Sym("(") => {
                let mut inner = self.expr()?;
                let close = self.expect_sym(")", "a bracket is not closed")?;
                inner.span = start.to(close);
                Ok(inner)
            }
            Tok::Sym("[") => {
                let mut items = Vec::new();
                loop {
                    if self.take_sym("]") {
                        break;
                    }
                    items.push(self.expr()?);
                    if !self.take_sym(",") {
                        self.expect_sym("]", "a list needs `,` or a closing `]`")?;
                        break;
                    }
                }
                node(Kind::List(items), start.to(self.last()))
            }
            Tok::Sym("{") => self.record(start),
            Tok::Sym(".") => Err(ExprError::invalid("a name cannot start with `.`", start)),
            Tok::Sym(_) => Err(ExprError::invalid("a value should be here", start)),
        }
    }

    fn word(&mut self, word: String, start: Span) -> Result<Node, ExprError> {
        let kind = match word.as_str() {
            "true" => Kind::Bool(true),
            "false" => Kind::Bool(false),
            "null" => Kind::Null,
            "in" => {
                return Err(ExprError::invalid("`in` needs a value before it", start));
            }
            _ if self.peek_sym("(") => {
                let (mut args, close) = self.args()?;
                let span = start.to(close);
                if word == "has" {
                    let field = if args.len() == 1 { args.pop() } else { None };
                    return match field {
                        Some(Node {
                            kind: Kind::Field(record, name),
                            ..
                        }) => node(Kind::Has(record, name), span),
                        _ => Err(ExprError::invalid("`has` takes one field access", span)
                            .with_fix("Write has(record.field).")),
                    };
                }
                return node(Kind::Call(word, args), span);
            }
            _ => Kind::Name(word),
        };
        node(kind, start)
    }

    fn record(&mut self, start: Span) -> Result<Node, ExprError> {
        let mut fields: Vec<(String, Node)> = Vec::new();
        loop {
            if self.take_sym("}") {
                break;
            }
            let key_span = self.here();
            let key = match self.tokens.get(self.at) {
                Some((Tok::Text(key), _)) => key.clone(),
                _ => {
                    return Err(ExprError::invalid(
                        "a record's field name is a text literal",
                        key_span,
                    )
                    .with_fix("Write {\"name\": value}."));
                }
            };
            self.at += 1;
            if fields.iter().any(|(name, _)| *name == key) {
                return Err(ExprError::invalid(
                    "a record names this field twice",
                    key_span,
                ));
            }
            self.expect_sym(":", "a record's field needs `:` and a value")?;
            let value = self.expr()?;
            fields.push((key, value));
            if !self.take_sym(",") {
                self.expect_sym("}", "a record needs `,` or a closing `}`")?;
                break;
            }
        }
        node(Kind::Record(fields), start.to(self.last()))
    }
}

fn method(
    receiver: Node,
    name: String,
    name_span: Span,
    mut args: Vec<Node>,
    span: Span,
) -> Result<Node, ExprError> {
    let Some(kind) = MacroKind::from_name(&name) else {
        return node(Kind::Method(Box::new(receiver), name, args), span);
    };
    let body = if args.len() == 2 { args.pop() } else { None };
    match (args.pop(), body) {
        (
            Some(Node {
                kind: Kind::Name(variable),
                ..
            }),
            Some(body),
        ) => node(
            Kind::Macro(kind, Box::new(receiver), variable, Box::new(body)),
            span,
        ),
        _ => Err(ExprError::invalid(
            format!("`{name}` takes a name for each item and one expression"),
            name_span,
        )
        .with_fix(format!("Write list.{name}(item, expression)."))),
    }
}

/// Prints a tree as canonical text: one spelling for each tree.
pub(crate) fn print(node: &Node) -> String {
    let mut out = String::new();
    write(node, LEVEL_COND, &mut out);
    out
}

fn level(node: &Node) -> u8 {
    match &node.kind {
        Kind::Cond(..) => LEVEL_COND,
        Kind::Binary(op, ..) => op.level(),
        Kind::Unary(..) => LEVEL_UNARY,
        Kind::Int(value) if *value < 0 => LEVEL_UNARY,
        _ => LEVEL_MEMBER,
    }
}

fn write(node: &Node, least: u8, out: &mut String) {
    let bracket = level(node) < least;
    if bracket {
        out.push('(');
    }
    match &node.kind {
        Kind::Null => out.push_str("null"),
        Kind::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Kind::Int(value) => out.push_str(&value.to_string()),
        Kind::Text(text) => write_text(text, out),
        Kind::Name(name) => out.push_str(name),
        Kind::Field(record, name) => {
            write(record, LEVEL_MEMBER, out);
            out.push('.');
            out.push_str(name);
        }
        Kind::Index(list, index) => {
            write(list, LEVEL_MEMBER, out);
            out.push('[');
            write(index, LEVEL_COND, out);
            out.push(']');
        }
        Kind::Unary(op, operand) => {
            out.push(if *op == UnaryOp::Not { '!' } else { '-' });
            // `-(-x)` keeps its bracket, or it would read as one token run `--x`
            // and a negative literal would fold differently.
            let inner = if *op == UnaryOp::Neg && level(operand) == LEVEL_UNARY {
                LEVEL_MEMBER
            } else {
                LEVEL_UNARY
            };
            write(operand, inner, out);
        }
        Kind::Binary(op, left, right) => {
            write(left, op.level(), out);
            out.push(' ');
            out.push_str(op.symbol());
            out.push(' ');
            write(right, op.level() + 1, out);
        }
        Kind::Cond(condition, then, otherwise) => {
            write(condition, 2, out);
            out.push_str(" ? ");
            write(then, 2, out);
            out.push_str(" : ");
            write(otherwise, LEVEL_COND, out);
        }
        Kind::Call(name, args) => {
            out.push_str(name);
            write_args(args, out);
        }
        Kind::Method(receiver, name, args) => {
            write(receiver, LEVEL_MEMBER, out);
            out.push('.');
            out.push_str(name);
            write_args(args, out);
        }
        Kind::Macro(kind, list, variable, body) => {
            write(list, LEVEL_MEMBER, out);
            out.push('.');
            out.push_str(kind.name());
            out.push('(');
            out.push_str(variable);
            out.push_str(", ");
            write(body, LEVEL_COND, out);
            out.push(')');
        }
        Kind::Has(record, name) => {
            out.push_str("has(");
            write(record, LEVEL_MEMBER, out);
            out.push('.');
            out.push_str(name);
            out.push(')');
        }
        Kind::List(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                write(item, LEVEL_COND, out);
            }
            out.push(']');
        }
        Kind::Record(fields) => {
            out.push('{');
            for (index, (name, value)) in fields.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                write_text(name, out);
                out.push_str(": ");
                write(value, LEVEL_COND, out);
            }
            out.push('}');
        }
    }
    if bracket {
        out.push(')');
    }
}

fn write_args(args: &[Node], out: &mut String) {
    out.push('(');
    for (index, arg) in args.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        write(arg, LEVEL_COND, out);
    }
    out.push(')');
}

/// A text literal in its one spelling: double quotes, and every character that
/// cannot be seen written as an escape.
pub(crate) fn write_text(text: &str, out: &mut String) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ' ' => out.push(' '),
            _ if ch.is_control() || ch.is_whitespace() || is_unseen(ch) => {
                let code = ch as u32;
                if code <= 0xFFFF {
                    out.push_str(&format!("\\u{code:04X}"));
                } else {
                    out.push_str(&format!("\\U{code:08X}"));
                }
            }
            _ => out.push(ch),
        }
    }
    out.push('"');
}

/// Format characters that change how text reads without showing: zero-width
/// marks, direction marks, and the byte-order mark.
fn is_unseen(ch: char) -> bool {
    matches!(ch as u32, 0x00AD | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x2069 | 0xFEFF)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{EXPRESSION_INVALID, EXPRESSION_TYPE_ERROR};

    fn canon(source: &str) -> String {
        print(&parse(source).unwrap())
    }

    #[test]
    fn the_same_tree_has_one_spelling() {
        let cases = [
            ("input.name", "input.name"),
            ("  input . name  ", "input.name"),
            ("a+b*c", "a + b * c"),
            ("(a+b)*c", "(a + b) * c"),
            ("a-(b-c)", "a - (b - c)"),
            ("(a-b)-c", "a - b - c"),
            ("a?b:c?d:e", "a ? b : c ? d : e"),
            ("(a?b:c)?d:e", "(a ? b : c) ? d : e"),
            ("!(a&&b)||c", "!(a && b) || c"),
            ("-5", "-5"),
            ("- 5", "-5"),
            ("-(5)", "-5"),
            ("--5", "5"),
            ("-9223372036854775808", "-9223372036854775808"),
            ("-(-x)", "-(-x)"),
            ("a - -5", "a - -5"),
            ("'a\"b'", "\"a\\\"b\""),
            ("\"a\\u00A0b\"", "\"a\\u00A0b\""),
            ("\"caf\u{e9}\"", "\"caf\u{e9}\""),
            ("trim( input.name ,)", "trim(input.name)"),
            ("rows.all(r,r.qty>0)", "rows.all(r, r.qty > 0)"),
            ("has(input.note)", "has(input.note)"),
            ("[1,2,]", "[1, 2]"),
            ("{'a':1,\"b\":x}", "{\"a\": 1, \"b\": x}"),
            ("steps.find.result[0].id", "steps.find.result[0].id"),
            ("x in [1,2]", "x in [1, 2]"),
            ("s.startsWith('a')", "s.startsWith(\"a\")"),
        ];
        for (source, expected) in cases {
            let once = canon(source);
            assert_eq!(once, expected, "{source}");
            assert_eq!(canon(&once), once, "printing {source} twice");
            // Reading the canonical text gives the same tree, places aside.
            assert_eq!(
                print(&parse(&once).unwrap()),
                print(&parse(source).unwrap())
            );
        }
    }

    #[test]
    fn a_float_is_refused_with_its_place_and_a_fix() {
        for (source, start, end) in [
            ("1.5", 0, 3),
            ("x + 2.50", 4, 8),
            ("1e3", 0, 3),
            ("3.0e-2 ", 0, 6),
        ] {
            let error = parse(source).unwrap_err();
            assert_eq!(error.code, EXPRESSION_TYPE_ERROR, "{source}");
            assert_eq!(error.at, Span::new(start, end), "{source}");
            assert!(error.fix.unwrap().contains("decimal(\""), "{source}");
        }
        assert_eq!(parse("7u").unwrap_err().code, EXPRESSION_TYPE_ERROR);
        assert_eq!(parse("b'ab'").unwrap_err().code, EXPRESSION_TYPE_ERROR);
    }

    #[test]
    fn bad_text_is_refused_with_its_place() {
        for (source, start) in [
            ("", 0),
            ("a +", 3),
            ("a b", 2),
            ("a = b", 2),
            ("(a", 2),
            ("'abc", 0),
            ("a.all(1, x)", 2),
            ("has(a)", 0),
            ("{a: 1}", 1),
            ("9223372036854775808", 0),
            ("a # b", 2),
            ("0x1F", 0),
        ] {
            let error = parse(source).unwrap_err();
            assert_eq!(error.code, EXPRESSION_INVALID, "{source}");
            assert_eq!(error.at.start, start, "{source}");
        }
    }

    #[test]
    fn nesting_and_length_are_bounded() {
        let deep = format!("{}1{}", "(".repeat(200), ")".repeat(200));
        assert_eq!(parse(&deep).unwrap_err().code, EXPRESSION_INVALID);
        let minus = format!("{}x", "-".repeat(200));
        assert_eq!(parse(&minus).unwrap_err().code, EXPRESSION_INVALID);
        // A long chain of operators is a tall tree, though nothing nests in the text.
        let chain = format!("1{}", " + 1".repeat(200));
        assert_eq!(parse(&chain).unwrap_err().code, EXPRESSION_INVALID);
        let fields = format!("a{}", ".b".repeat(200));
        assert_eq!(parse(&fields).unwrap_err().code, EXPRESSION_INVALID);
        assert!(parse(&format!("1{}", " + 1".repeat(60))).is_ok());
        let long = format!("\"{}\"", "a".repeat(MAX_SOURCE_BYTES));
        assert_eq!(parse(&long).unwrap_err().code, EXPRESSION_INVALID);
    }
}
