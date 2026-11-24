//! Evaluation: a loaded program, the values bound to its names, and one answer.
//!
//! Evaluation is pure and bounded. It reads nothing but the program and the
//! bindings, counts every step against the library's cost limit, and checks
//! every operand's kind again, so a value of the wrong kind stops and is
//! never reinterpreted.

use std::borrow::Cow;
use std::cell::Cell;
use std::cmp::Ordering;

use a2m_core::{CompiledExpr, Decimal, Failure, FailureClass, Value, ValueType, Values};

use crate::check::{self, Checked, TypeEnv};
use crate::error::{COMPILED_EXPRESSION_UNLOADABLE, Fault, VALUE_UNPARSABLE};
use crate::functions;
use crate::library::{FnId, LibraryVersion, library};
use crate::syntax::{self, BinaryOp, Kind, MacroKind, Node, UnaryOp};

/// The values for the names a program reads. Borrowed: nothing is copied
/// that the program does not use.
#[derive(Debug, Clone, Default)]
pub struct Bindings<'a> {
    names: Vec<(&'a str, &'a Value)>,
}

impl<'a> Bindings<'a> {
    /// No bindings.
    #[must_use]
    pub const fn new() -> Self {
        Self { names: Vec::new() }
    }

    /// Binds `name` to `value`, replacing an earlier binding of the name.
    pub fn bind(&mut self, name: &'a str, value: &'a Value) {
        match self.names.iter_mut().find(|(bound, _)| *bound == name) {
            Some(slot) => slot.1 = value,
            None => self.names.push((name, value)),
        }
    }

    /// These bindings with `name` bound to `value`.
    #[must_use]
    pub fn with(mut self, name: &'a str, value: &'a Value) -> Self {
        self.bind(name, value);
        self
    }

    fn get(&self, name: &str) -> Option<&'a Value> {
        self.names
            .iter()
            .find(|(bound, _)| *bound == name)
            .map(|(_, value)| *value)
    }
}

/// The parsed form of a compiled target's expression, rebuilt when the
/// compiled target is loaded and kept for its life.
#[derive(Debug, Clone)]
pub struct Program {
    tree: Node,
    result: ValueType,
    names: Vec<String>,
    cost_limit: u64,
}

fn unloadable(what: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        COMPILED_EXPRESSION_UNLOADABLE,
        format!("A compiled target's expression cannot be loaded: {what}."),
        "Compile the target again with this engine.",
    )
}

impl Program {
    /// Reads a compiled target's expression back into a program: parses the
    /// canonical text, and checks again that it gives the type the compiled
    /// target states.
    ///
    /// # Errors
    ///
    /// `compiled_expression_unloadable` when the compiled target names a
    /// library version this engine does not carry, when the text does not
    /// parse or is not in its canonical spelling, or when it no longer checks
    /// as its stated type.
    pub fn load(expr: &CompiledExpr) -> Result<Self, Failure> {
        let Some(version) = LibraryVersion::new(expr.library) else {
            return Err(unloadable(
                "it was checked against a host-function library this engine does not carry",
            ));
        };
        let tree =
            syntax::parse(&expr.source).map_err(|_| unloadable("its text is not an expression"))?;
        if syntax::print(&tree) != expr.source {
            return Err(unloadable("its text is not in the canonical spelling"));
        }
        // The names' types are the compiler's knowledge and are not stored, so
        // every name is read as unknown; evaluation checks each value's kind.
        let (found, names, _) = check::check_tree(&tree, &TypeEnv::open(version))
            .map_err(|_| unloadable("it no longer passes the type checker"))?;
        if let Checked::Known(found) = &found
            && !check::assignable(found, &expr.result)
        {
            return Err(unloadable(
                "it no longer gives the type the compiled target states",
            ));
        }
        Ok(Self {
            tree,
            result: expr.result.clone(),
            names,
            cost_limit: library().cost_limit(),
        })
    }

    /// The type of the answer.
    #[must_use]
    pub const fn result_type(&self) -> &ValueType {
        &self.result
    }

    /// The names the program reads, sorted. Each must be bound to evaluate it.
    #[must_use]
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Evaluates the program. Pure: the same program and the same bindings
    /// give the same answer, on every machine.
    ///
    /// # Errors
    ///
    /// A failure of class `input`: `value_unparsable`, `value_out_of_range`,
    /// or `value_absent` when a value cannot be used as the expression needs;
    /// `expression_cost_exceeded` when the work passes the cost limit;
    /// `compiled_expression_unloadable` when a name the program reads is not
    /// bound. The failure names the function or operator, never a value.
    pub fn evaluate(&self, bindings: &Bindings<'_>) -> Result<Value, Failure> {
        if let Some(missing) = self.names.iter().find(|name| bindings.get(name).is_none()) {
            return Err(unloadable(&format!(
                "it reads the name `{missing}`, which was not bound"
            )));
        }
        let spent = Cell::new(0);
        let env = Env {
            bindings,
            local: None,
            spent: &spent,
            limit: self.cost_limit,
        };
        let value = eval(&self.tree, &env)
            .map_err(|stop| stop.fault.into_failure(stop.place))?
            .into_owned();
        if !value.has_type(&self.result) {
            return Err(Failure::new(
                FailureClass::Input,
                VALUE_UNPARSABLE,
                format!(
                    "An expression gave a {} where its place needs a {}.",
                    value.kind(),
                    self.result.name()
                ),
                "Give the expression values of the types its names declare.",
            ));
        }
        Ok(value)
    }
}

/// Why evaluation stopped, and in which function or operator.
struct Stop {
    fault: Fault,
    place: &'static str,
}

struct Local<'e> {
    name: &'e str,
    value: &'e Value,
    next: Option<&'e Local<'e>>,
}

struct Env<'e> {
    bindings: &'e Bindings<'e>,
    local: Option<&'e Local<'e>>,
    spent: &'e Cell<u64>,
    limit: u64,
}

impl Env<'_> {
    /// Counts `units` of work, and stops when the limit is passed.
    fn tick(&self, units: u64) -> Result<(), Stop> {
        let spent = self.spent.get().saturating_add(units);
        self.spent.set(spent);
        if spent > self.limit {
            return Err(Stop {
                fault: Fault::cost(),
                place: "",
            });
        }
        Ok(())
    }

    fn tick_len(&self, len: usize) -> Result<(), Stop> {
        self.tick(u64::try_from(len).unwrap_or(u64::MAX))
    }
}

const ABSENT: Fault = Fault::absent("a value the expression needs is absent");
const WRONG_KIND: Fault = Fault::unparsable("a value is not of the kind the expression needs");
const TOO_LARGE: Fault = Fault::range("the result has more digits than its type holds");

fn stop<T>(fault: Fault, place: &'static str) -> Result<T, Stop> {
    Err(Stop { fault, place })
}

/// The fault for an operand of the wrong kind: absent, or simply wrong.
fn unusable(value: &Value) -> Fault {
    if value.is_null() { ABSENT } else { WRONG_KIND }
}

fn truth(value: &Value, place: &'static str) -> Result<bool, Stop> {
    value.as_bool().ok_or(Stop {
        fault: unusable(value),
        place,
    })
}

fn owned<'e>(value: Value) -> Result<Cow<'e, Value>, Stop> {
    Ok(Cow::Owned(value))
}

/// The field `name` of a record. An absent record has absent fields.
fn field<'e>(record: Cow<'e, Value>, name: &str) -> Result<Cow<'e, Value>, Stop> {
    match record {
        Cow::Borrowed(Value::Record(fields)) => Ok(fields
            .get(name)
            .map_or(Cow::Owned(Value::Null), Cow::Borrowed)),
        Cow::Owned(Value::Record(mut fields)) => owned(fields.remove(name).unwrap_or(Value::Null)),
        other if other.is_null() => owned(Value::Null),
        _ => stop(WRONG_KIND, "."),
    }
}

fn eval<'e>(node: &'e Node, env: &Env<'e>) -> Result<Cow<'e, Value>, Stop> {
    env.tick(1)?;
    match &node.kind {
        Kind::Null => owned(Value::Null),
        Kind::Bool(flag) => owned(Value::Bool(*flag)),
        Kind::Int(number) => owned(Value::Int(*number)),
        Kind::Text(text) => owned(Value::Text(text.clone())),
        Kind::Name(name) => {
            let mut local = env.local;
            while let Some(found) = local {
                if found.name == name.as_str() {
                    return Ok(Cow::Borrowed(found.value));
                }
                local = found.next;
            }
            match env.bindings.get(name) {
                Some(value) => Ok(Cow::Borrowed(value)),
                None => stop(
                    Fault::unloadable("a name the expression reads was not bound"),
                    "",
                ),
            }
        }
        Kind::Field(record, name) => field(eval(record, env)?, name),
        Kind::Index(subject, index) => {
            let subject = eval(subject, env)?;
            let index = eval(index, env)?;
            match (&*subject, &*index) {
                (Value::Record(_), Value::Text(name)) => field(subject, name),
                (Value::List(items), Value::Int(at)) => {
                    let at = usize::try_from(*at).ok().filter(|at| *at < items.len());
                    let Some(at) = at else {
                        return stop(Fault::range("the index is beyond the list"), "[]");
                    };
                    match subject {
                        Cow::Borrowed(Value::List(items)) => Ok(Cow::Borrowed(&items[at])),
                        Cow::Owned(Value::List(mut items)) => owned(items.swap_remove(at)),
                        _ => stop(WRONG_KIND, "[]"),
                    }
                }
                (Value::Null, _) | (_, Value::Null) => stop(ABSENT, "[]"),
                _ => stop(WRONG_KIND, "[]"),
            }
        }
        Kind::Unary(op, operand) => {
            let value = eval(operand, env)?;
            match (op, &*value) {
                (UnaryOp::Not, Value::Bool(flag)) => owned(Value::Bool(!flag)),
                (UnaryOp::Neg, Value::Int(number)) => match number.checked_neg() {
                    Some(negated) => owned(Value::Int(negated)),
                    None => stop(TOO_LARGE, "-"),
                },
                (UnaryOp::Neg, Value::Decimal(number)) => owned(Value::Decimal(number.negated())),
                (UnaryOp::Not, other) => stop(unusable(other), "!"),
                (UnaryOp::Neg, other) => stop(unusable(other), "-"),
            }
        }
        Kind::Binary(op, left, right) => binary(*op, left, right, env),
        Kind::Cond(test, then, otherwise) => {
            if truth(&*eval(test, env)?, "?:")? {
                eval(then, env)
            } else {
                eval(otherwise, env)
            }
        }
        Kind::Call(name, args) => call(name, args, env),
        Kind::Method(receiver, name, args) => {
            let receiver = eval(receiver, env)?;
            if name == "size" {
                return functions::call(FnId::Size, &[&*receiver])
                    .map(Cow::Owned)
                    .map_err(|fault| Stop {
                        fault,
                        place: "size",
                    });
            }
            let (place, test): (&'static str, fn(&str, &str) -> bool) = match name.as_str() {
                "startsWith" => ("startsWith", starts_with),
                "endsWith" => ("endsWith", ends_with),
                "contains" => ("contains", contains),
                _ => {
                    return stop(
                        Fault::unloadable("a member function is not one this engine knows"),
                        "",
                    );
                }
            };
            let Some(arg) = args.first() else {
                return stop(WRONG_KIND, place);
            };
            let part = eval(arg, env)?;
            match (&*receiver, &*part) {
                (Value::Text(text), Value::Text(part)) => {
                    env.tick_len(text.len() / 64)?;
                    owned(Value::Bool(test(text, part)))
                }
                (Value::Null, _) | (_, Value::Null) => stop(ABSENT, place),
                _ => stop(WRONG_KIND, place),
            }
        }
        Kind::Macro(kind, list, variable, body) => each(*kind, list, variable, body, env),
        Kind::Has(record, name) => {
            let record = eval(record, env)?;
            match &*record {
                Value::Record(fields) => owned(Value::Bool(
                    fields.get(name).is_some_and(|value| !value.is_null()),
                )),
                Value::Null => owned(Value::Bool(false)),
                _ => stop(WRONG_KIND, "has"),
            }
        }
        Kind::List(items) => {
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                values.push(eval(item, env)?.into_owned());
            }
            owned(Value::List(values))
        }
        Kind::Record(fields) => {
            let mut values = Values::new();
            for (name, value) in fields {
                values.insert(name.clone(), eval(value, env)?.into_owned());
            }
            owned(Value::Record(values))
        }
    }
}

fn starts_with(text: &str, part: &str) -> bool {
    text.starts_with(part)
}

fn ends_with(text: &str, part: &str) -> bool {
    text.ends_with(part)
}

fn contains(text: &str, part: &str) -> bool {
    text.contains(part)
}

fn each<'e>(
    kind: MacroKind,
    list: &'e Node,
    variable: &'e str,
    body: &'e Node,
    env: &Env<'e>,
) -> Result<Cow<'e, Value>, Stop> {
    let place = kind.name();
    let list = eval(list, env)?;
    let items = match &*list {
        Value::List(items) => items,
        other => return stop(unusable(other), place),
    };
    let mut kept = Vec::new();
    let mut count = 0usize;
    for item in items {
        env.tick(1)?;
        let local = Local {
            name: variable,
            value: item,
            next: env.local,
        };
        let inner = Env {
            bindings: env.bindings,
            local: Some(&local),
            spent: env.spent,
            limit: env.limit,
        };
        let out = eval(body, &inner)?;
        match kind {
            MacroKind::Map => kept.push(out.into_owned()),
            MacroKind::Filter => {
                if truth(&out, place)? {
                    kept.push(item.clone());
                }
            }
            MacroKind::All => {
                if !truth(&out, place)? {
                    return owned(Value::Bool(false));
                }
            }
            MacroKind::Exists => {
                if truth(&out, place)? {
                    return owned(Value::Bool(true));
                }
            }
            MacroKind::ExistsOne => {
                if truth(&out, place)? {
                    count += 1;
                }
            }
        }
    }
    owned(match kind {
        MacroKind::Map | MacroKind::Filter => Value::List(kept),
        MacroKind::All => Value::Bool(true),
        MacroKind::Exists => Value::Bool(false),
        MacroKind::ExistsOne => Value::Bool(count == 1),
    })
}

fn call<'e>(name: &str, args: &'e [Node], env: &Env<'e>) -> Result<Cow<'e, Value>, Stop> {
    let Some(function) = library().get(name) else {
        return stop(
            Fault::unloadable("a function is not one this engine knows"),
            "",
        );
    };
    let place = function.name().as_str();
    if function.id == FnId::Coalesce {
        // Later arguments are not evaluated once one is present.
        for arg in args {
            let value = eval(arg, env)?;
            if !value.is_null() {
                return Ok(value);
            }
        }
        return owned(Value::Null);
    }
    let mut values = Vec::with_capacity(args.len());
    for arg in args {
        values.push(eval(arg, env)?);
    }
    let borrowed: Vec<&Value> = values.iter().map(|value| &**value).collect();
    // `decimal` reads text only when it is a literal of the expression: text
    // from a value becomes a decimal only through `parse_decimal`.
    if function.id == FnId::Decimal
        && matches!(borrowed.first(), Some(Value::Text(_)))
        && !matches!(args.first().map(|arg| &arg.kind), Some(Kind::Text(_)))
    {
        return stop(WRONG_KIND, place);
    }
    env.tick(functions::extra_cost(&borrowed))?;
    functions::call(function.id, &borrowed)
        .map(Cow::Owned)
        .map_err(|fault| Stop { fault, place })
}

fn binary<'e>(
    op: BinaryOp,
    left: &'e Node,
    right: &'e Node,
    env: &Env<'e>,
) -> Result<Cow<'e, Value>, Stop> {
    let place = op.symbol();
    let left = eval(left, env)?;
    // `&&` and `||` do not evaluate their right side when the left decides.
    if matches!(op, BinaryOp::And | BinaryOp::Or) {
        let decided = truth(&left, place)? == (op == BinaryOp::Or);
        return if decided {
            Ok(left)
        } else {
            let right = eval(right, env)?;
            truth(&right, place)?;
            Ok(right)
        };
    }
    let right = eval(right, env)?;
    let (a, b) = (&*left, &*right);
    let flag = |flag: bool| owned(Value::Bool(flag));
    match op {
        BinaryOp::Eq | BinaryOp::Ne => {
            // Values of two different kinds are never quietly unequal.
            if a.kind() != b.kind() && !a.is_null() && !b.is_null() {
                return stop(WRONG_KIND, place);
            }
            env.tick_len(shallow_size(a))?;
            flag((a == b) == (op == BinaryOp::Eq))
        }
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            let order = match (a, b) {
                (Value::Int(a), Value::Int(b)) => a.cmp(b),
                (Value::Decimal(a), Value::Decimal(b)) => a.cmp(b),
                (Value::Text(a), Value::Text(b)) => a.cmp(b),
                (Value::Date(a), Value::Date(b)) => a.cmp(b),
                (Value::Instant(a), Value::Instant(b)) => a.cmp(b),
                (Value::Duration(a), Value::Duration(b)) => a.cmp(b),
                (Value::Null, _) | (_, Value::Null) => return stop(ABSENT, place),
                _ => return stop(WRONG_KIND, place),
            };
            flag(match op {
                BinaryOp::Lt => order == Ordering::Less,
                BinaryOp::Le => order != Ordering::Greater,
                BinaryOp::Gt => order == Ordering::Greater,
                _ => order != Ordering::Less,
            })
        }
        BinaryOp::In => match (a, b) {
            (_, Value::List(items)) => {
                env.tick_len(items.len())?;
                flag(items.contains(a))
            }
            (Value::Text(name), Value::Record(fields)) => flag(fields.contains(name)),
            (_, Value::Null) => stop(ABSENT, place),
            _ => stop(WRONG_KIND, place),
        },
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
            arithmetic(op, left, right, env)
        }
        BinaryOp::And | BinaryOp::Or => stop(WRONG_KIND, place),
    }
}

fn shallow_size(value: &Value) -> usize {
    match value {
        Value::List(items) => items.len(),
        Value::Record(fields) => fields.len(),
        Value::Text(text) => text.len() / 64,
        _ => 0,
    }
}

fn arithmetic<'e>(
    op: BinaryOp,
    left: Cow<'e, Value>,
    right: Cow<'e, Value>,
    env: &Env<'e>,
) -> Result<Cow<'e, Value>, Stop> {
    let place = op.symbol();
    let int = |value: Option<i64>| match value {
        Some(value) => owned(Value::Int(value)),
        None => stop(TOO_LARGE, place),
    };
    let decimal = |value: Option<Decimal>| match value {
        Some(value) => owned(Value::Decimal(value)),
        None => stop(TOO_LARGE, place),
    };
    match (op, &*left, &*right) {
        (BinaryOp::Add, Value::Int(a), Value::Int(b)) => int(a.checked_add(*b)),
        (BinaryOp::Sub, Value::Int(a), Value::Int(b)) => int(a.checked_sub(*b)),
        (BinaryOp::Mul, Value::Int(a), Value::Int(b)) => int(a.checked_mul(*b)),
        (BinaryOp::Div | BinaryOp::Rem, Value::Int(_), Value::Int(0)) => {
            stop(Fault::range("the divisor is zero"), place)
        }
        (BinaryOp::Div, Value::Int(a), Value::Int(b)) => int(a.checked_div(*b)),
        (BinaryOp::Rem, Value::Int(a), Value::Int(b)) => int(a.checked_rem(*b)),
        (BinaryOp::Add, Value::Decimal(a), Value::Decimal(b)) => decimal(a.checked_add(b)),
        (BinaryOp::Sub, Value::Decimal(a), Value::Decimal(b)) => decimal(a.checked_sub(b)),
        (BinaryOp::Mul, Value::Decimal(a), Value::Decimal(b)) => decimal(a.checked_mul(b)),
        (BinaryOp::Add, Value::Text(a), Value::Text(b)) => {
            env.tick_len((a.len() + b.len()) / 64)?;
            let mut joined = String::with_capacity(a.len() + b.len());
            joined.push_str(a);
            joined.push_str(b);
            owned(Value::Text(joined))
        }
        (BinaryOp::Add, Value::List(a), Value::List(b)) => {
            env.tick_len(a.len() + b.len())?;
            let mut joined = Vec::with_capacity(a.len() + b.len());
            joined.extend_from_slice(a);
            joined.extend_from_slice(b);
            owned(Value::List(joined))
        }
        (_, Value::Null, _) | (_, _, Value::Null) => stop(ABSENT, place),
        _ => stop(WRONG_KIND, place),
    }
}
