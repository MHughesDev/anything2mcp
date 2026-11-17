//! Type checking: every expression is checked once, against the names its
//! place declares, before it can go into a plan.

use std::collections::{BTreeMap, BTreeSet};

use a2m_core::{Decimal, FieldOf, PlanExpr, ValueType};

use crate::error::{ExprError, Span};
use crate::functions;
use crate::library::{FnId, HostFn, HostFnName, LibraryVersion, Param, Ret, library};
use crate::measure;
use crate::syntax::{self, BinaryOp, Kind, MacroKind, Node, UnaryOp};

/// The type of an expression or of a name: one of `a2m-core`'s value types,
/// or not known. An unknown type is allowed while drafting; it cannot go into
/// a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    /// A stated type.
    Known(ValueType),
    /// Not known until the expression runs.
    Dyn,
}

use Checked::{Dyn, Known};

/// What an expression may name. The compiler builds one per place: an
/// operation's binding, a pipeline step's `with`, a source mapping, an
/// `expect`. Policy builds its own.
#[derive(Debug, Clone)]
pub struct TypeEnv {
    names: BTreeMap<String, Checked>,
    library: LibraryVersion,
    /// True only when a plan's expression is read back: every name is then
    /// taken as declared, with a type not known.
    open: bool,
}

const RESERVED: [&str; 5] = ["true", "false", "null", "in", "has"];

fn is_identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

impl TypeEnv {
    /// An environment with no names, for expressions checked against `library`.
    #[must_use]
    pub fn new(library: LibraryVersion) -> Self {
        Self {
            names: BTreeMap::new(),
            library,
            open: false,
        }
    }

    pub(crate) fn open(library: LibraryVersion) -> Self {
        Self {
            names: BTreeMap::new(),
            library,
            open: true,
        }
    }

    /// Declares a name and its type.
    ///
    /// # Errors
    ///
    /// `expression_invalid` when the name is not an identifier, is a reserved
    /// word or a host function's name, or is already declared.
    pub fn declare(&mut self, name: &str, ty: ValueType) -> Result<(), ExprError> {
        self.insert(name, Known(ty))
    }

    /// Declares a name whose type is not known yet. An expression that reads
    /// it may compile to an unknown type, which cannot go into a plan.
    ///
    /// # Errors
    ///
    /// As for [`TypeEnv::declare`].
    pub fn declare_unknown(&mut self, name: &str) -> Result<(), ExprError> {
        self.insert(name, Dyn)
    }

    fn insert(&mut self, name: &str, ty: Checked) -> Result<(), ExprError> {
        let problem = if !is_identifier(name) {
            "a name is letters, digits, and underscores, and does not start with a digit"
        } else if RESERVED.contains(&name) || library().get(name).is_some() {
            "this name is a word of the expression language or one of its functions"
        } else if self.names.contains_key(name) {
            "this name is already declared"
        } else {
            self.names.insert(name.to_owned(), ty);
            return Ok(());
        };
        Err(ExprError::invalid(problem, Span::default()))
    }

    /// The library version expressions are checked against.
    #[must_use]
    pub const fn library(&self) -> LibraryVersion {
        self.library
    }
}

/// A checked expression, in memory, for the compiler's use. Never stored as
/// it is: the plan holds [`Compiled::to_plan_expr`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compiled {
    source: String,
    result: Checked,
    names: Vec<String>,
    functions: Vec<HostFnName>,
    library: LibraryVersion,
}

impl Compiled {
    /// The canonical text: the parsed program printed back, not the author's
    /// spacing.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The type of the result. When the place stated a type, this is it.
    #[must_use]
    pub const fn result(&self) -> &Checked {
        &self.result
    }

    /// The names the expression reads, sorted.
    #[must_use]
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// The host functions it calls, sorted.
    #[must_use]
    pub fn functions(&self) -> &[HostFnName] {
        &self.functions
    }

    /// The library version it was checked against.
    #[must_use]
    pub const fn library(&self) -> LibraryVersion {
        self.library
    }

    /// The plan's form: the canonical text, the type, and the library version.
    ///
    /// # Errors
    ///
    /// `expression_type_error` when the result's type is not known, so that
    /// nothing unknown reaches a plan.
    pub fn to_plan_expr(&self) -> Result<PlanExpr, ExprError> {
        match &self.result {
            Known(result) => Ok(PlanExpr {
                source: self.source.clone(),
                result: result.clone(),
                library: self.library.get(),
            }),
            Dyn => Err(ExprError::type_error(
                "the expression's type is not known, so it cannot go into a plan",
                Span::new(0, u32::try_from(self.source.len()).unwrap_or(u32::MAX)),
            )
            .with_fix("State the type of every name the expression reads.")),
        }
    }
}

/// Checks and compiles one expression. `expected` is the type the place
/// requires, when it has one.
///
/// # Errors
///
/// `expression_invalid` when the text does not parse or reads a name the
/// environment does not declare; `expression_type_error` when a type does not
/// fit, including any use of a float. Each carries the place and, where the
/// checker knows one, a fix.
pub fn compile(
    source: &str,
    env: &TypeEnv,
    expected: Option<&ValueType>,
) -> Result<Compiled, ExprError> {
    let tree = syntax::parse(source)?;
    let (found, names, functions) = check_tree(&tree, env)?;
    let result = match (found, expected) {
        (Known(found), Some(wanted)) => {
            if !assignable(&found, wanted) {
                return Err(ExprError::type_error(
                    format!(
                        "this place needs a {}, and the expression gives a {}",
                        wanted.name(),
                        found.name()
                    ),
                    tree.span,
                ));
            }
            Known(wanted.clone())
        }
        (found, _) => found,
    };
    Ok(Compiled {
        source: syntax::print(&tree),
        result,
        names,
        functions,
        library: env.library,
    })
}

/// Checks a parsed tree. Gives its type, the names it reads, and the host
/// functions it calls.
pub(crate) fn check_tree(
    tree: &Node,
    env: &TypeEnv,
) -> Result<(Checked, Vec<String>, Vec<HostFnName>), ExprError> {
    let mut checker = Checker {
        env,
        locals: Vec::new(),
        names: BTreeSet::new(),
        functions: BTreeSet::new(),
    };
    let found = checker.check(tree)?;
    Ok((
        found,
        checker.names.into_iter().collect(),
        checker.functions.into_iter().collect(),
    ))
}

/// Whether a value of type `found` may stand where `wanted` is required.
pub(crate) fn assignable(found: &ValueType, wanted: &ValueType) -> bool {
    match (found, wanted) {
        (ValueType::Null, _)
        | (ValueType::Text | ValueType::Choice, ValueType::Text | ValueType::Choice) => true,
        (ValueType::List(found), ValueType::List(wanted)) => assignable(found, wanted),
        (ValueType::Record(found), ValueType::Record(wanted)) => {
            found.iter().all(|field| {
                wanted
                    .iter()
                    .any(|other| other.name == field.name && assignable(&field.ty, &other.ty))
            }) && wanted
                .iter()
                .all(|other| !other.required || found.iter().any(|field| field.name == other.name))
        }
        _ => found == wanted,
    }
}

fn unify_types(a: &ValueType, b: &ValueType) -> Option<ValueType> {
    match (a, b) {
        _ if a == b => Some(a.clone()),
        (ValueType::Null, other) | (other, ValueType::Null) => Some(other.clone()),
        (ValueType::Text, ValueType::Choice) | (ValueType::Choice, ValueType::Text) => {
            Some(ValueType::Text)
        }
        (ValueType::List(a), ValueType::List(b)) => unify_types(a, b).map(ValueType::list_of),
        _ => None,
    }
}

/// The one type two operands share, when they have one.
fn unify(a: &Checked, b: &Checked) -> Option<Checked> {
    match (a, b) {
        (Dyn, _) | (_, Dyn) => Some(Dyn),
        (Known(a), Known(b)) => unify_types(a, b).map(Known),
    }
}

fn describe(ty: &Checked) -> &'static str {
    match ty {
        Dyn => "value of unknown type",
        Known(ty) => ty.name(),
    }
}

fn is(ty: &Checked, wanted: &ValueType) -> bool {
    match ty {
        Dyn => true,
        Known(found) => {
            found == wanted || (*wanted == ValueType::Text && *found == ValueType::Choice)
        }
    }
}

struct Checker<'e> {
    env: &'e TypeEnv,
    locals: Vec<(String, Checked)>,
    names: BTreeSet<String>,
    functions: BTreeSet<HostFnName>,
}

impl Checker<'_> {
    fn check(&mut self, node: &Node) -> Result<Checked, ExprError> {
        match &node.kind {
            Kind::Null => Ok(Known(ValueType::Null)),
            Kind::Bool(_) => Ok(Known(ValueType::Bool)),
            Kind::Int(_) => Ok(Known(ValueType::Int)),
            Kind::Text(_) => Ok(Known(ValueType::Text)),
            Kind::Name(name) => self.name(name, node.span),
            Kind::Field(record, name) => {
                let record_type = self.check(record)?;
                field_of(&record_type, name, node.span)
            }
            Kind::Index(subject, index) => self.index(subject, index),
            Kind::Unary(op, operand) => {
                let found = self.check(operand)?;
                let fits = match op {
                    UnaryOp::Not => is(&found, &ValueType::Bool),
                    UnaryOp::Neg => is(&found, &ValueType::Int) || is(&found, &ValueType::Decimal),
                };
                if fits {
                    Ok(found)
                } else {
                    let needs = match op {
                        UnaryOp::Not => "`!` needs true or false",
                        UnaryOp::Neg => "`-` needs a whole number or a decimal",
                    };
                    Err(ExprError::type_error(
                        format!("{needs}; this is a {}", describe(&found)),
                        operand.span,
                    ))
                }
            }
            Kind::Binary(op, left, right) => self.binary(*op, left, right, node.span),
            Kind::Cond(test, then, otherwise) => {
                self.expect(test, &ValueType::Bool, "a condition is true or false")?;
                let then_type = self.check(then)?;
                let otherwise_type = self.check(otherwise)?;
                unify(&then_type, &otherwise_type).ok_or_else(|| {
                    ExprError::type_error(
                        format!(
                            "the two branches give different types: a {} and a {}",
                            describe(&then_type),
                            describe(&otherwise_type)
                        ),
                        otherwise.span,
                    )
                })
            }
            Kind::Call(name, args) => self.call(name, args, node.span),
            Kind::Method(receiver, name, args) => self.method(receiver, name, args, node.span),
            Kind::Macro(kind, list, variable, body) => {
                self.each(*kind, list, variable, body, node.span)
            }
            Kind::Has(record, name) => {
                let record_type = self.check(record)?;
                field_of(&record_type, name, node.span)?;
                Ok(Known(ValueType::Bool))
            }
            Kind::List(items) => {
                let mut common: Option<Checked> = None;
                for item in items {
                    let found = self.check(item)?;
                    common = Some(match &common {
                        None => found,
                        Some(common) => unify(common, &found).ok_or_else(|| {
                            ExprError::type_error(
                                format!(
                                    "a list holds values of one type; this is a {} among {}s",
                                    describe(&found),
                                    describe(common)
                                ),
                                item.span,
                            )
                        })?,
                    });
                }
                Ok(match common {
                    Some(Known(item)) if item != ValueType::Null => Known(ValueType::list_of(item)),
                    _ => Dyn,
                })
            }
            Kind::Record(fields) => {
                let mut known = Vec::with_capacity(fields.len());
                let mut all_known = true;
                for (name, value) in fields {
                    match self.check(value)? {
                        Known(ty) if ty != ValueType::Null => {
                            known.push(FieldOf::required(name.clone(), ty));
                        }
                        _ => all_known = false,
                    }
                }
                Ok(if all_known {
                    Known(ValueType::Record(known))
                } else {
                    Dyn
                })
            }
        }
    }

    fn name(&mut self, name: &str, at: Span) -> Result<Checked, ExprError> {
        if let Some((_, ty)) = self.locals.iter().rev().find(|(local, _)| local == name) {
            return Ok(ty.clone());
        }
        if let Some(ty) = self.env.names.get(name) {
            self.names.insert(name.to_owned());
            return Ok(ty.clone());
        }
        if self.env.open {
            self.names.insert(name.to_owned());
            return Ok(Dyn);
        }
        let error = ExprError::invalid(
            format!("`{name}` is not a name this expression may read"),
            at,
        );
        if library().get(name).is_some() {
            return Err(error.with_fix(format!("`{name}` is a function: write {name}(...).")));
        }
        let known: Vec<&str> = self.env.names.keys().map(String::as_str).collect();
        Err(if known.is_empty() {
            error.with_fix("No names are declared here.")
        } else {
            error.with_fix(format!("The names here are: {}.", known.join(", ")))
        })
    }

    fn expect(&mut self, node: &Node, wanted: &ValueType, needs: &str) -> Result<(), ExprError> {
        let found = self.check(node)?;
        if is(&found, wanted) {
            Ok(())
        } else {
            Err(ExprError::type_error(
                format!("{needs}; this is a {}", describe(&found)),
                node.span,
            ))
        }
    }

    fn index(&mut self, subject: &Node, index: &Node) -> Result<Checked, ExprError> {
        let subject_type = self.check(subject)?;
        let index_type = self.check(index)?;
        match &subject_type {
            Known(ValueType::List(item)) => {
                if !is(&index_type, &ValueType::Int) {
                    return Err(ExprError::type_error(
                        format!(
                            "a list is indexed by a whole number; this is a {}",
                            describe(&index_type)
                        ),
                        index.span,
                    ));
                }
                Ok(Known((**item).clone()))
            }
            Known(ValueType::Record(_)) => match &index.kind {
                Kind::Text(name) => field_of(&subject_type, name, index.span),
                _ if is(&index_type, &ValueType::Text) => Ok(Dyn),
                _ => Err(ExprError::type_error(
                    format!(
                        "a record is indexed by a field's name; this is a {}",
                        describe(&index_type)
                    ),
                    index.span,
                )),
            },
            Dyn if is(&index_type, &ValueType::Int) || is(&index_type, &ValueType::Text) => Ok(Dyn),
            _ => Err(ExprError::type_error(
                format!(
                    "only a list or a record can be indexed; this is a {}",
                    describe(&subject_type)
                ),
                subject.span,
            )),
        }
    }

    fn binary(
        &mut self,
        op: BinaryOp,
        left: &Node,
        right: &Node,
        at: Span,
    ) -> Result<Checked, ExprError> {
        let symbol = op.symbol();
        if matches!(op, BinaryOp::And | BinaryOp::Or) {
            let needs = format!("`{symbol}` joins two conditions");
            self.expect(left, &ValueType::Bool, &needs)?;
            self.expect(right, &ValueType::Bool, &needs)?;
            return Ok(Known(ValueType::Bool));
        }
        let left_type = self.check(left)?;
        let right_type = self.check(right)?;
        if op == BinaryOp::In {
            let fits = match &right_type {
                Dyn => true,
                Known(ValueType::List(item)) => {
                    unify(&left_type, &Known((**item).clone())).is_some()
                }
                Known(ValueType::Record(_)) => is(&left_type, &ValueType::Text),
                Known(_) => false,
            };
            return if fits {
                Ok(Known(ValueType::Bool))
            } else {
                Err(ExprError::type_error(
                    format!(
                        "`in` looks for a value in a list of its type, or a field's name in a record; this looks for a {} in a {}",
                        describe(&left_type),
                        describe(&right_type)
                    ),
                    at,
                ))
            };
        }
        let mismatch = || {
            let error = ExprError::type_error(
                format!(
                    "`{symbol}` needs two values of one type; these are a {} and a {}",
                    describe(&left_type),
                    describe(&right_type)
                ),
                at,
            );
            let int = Known(ValueType::Int);
            let decimal = Known(ValueType::Decimal);
            if (left_type == int && right_type == decimal)
                || (left_type == decimal && right_type == int)
            {
                error.with_fix(
                    "Write decimal(...) around the whole number: a whole number and a decimal never mix silently.",
                )
            } else {
                error
            }
        };
        let common = unify(&left_type, &right_type).ok_or_else(mismatch)?;
        if matches!(op, BinaryOp::Eq | BinaryOp::Ne) {
            return Ok(Known(ValueType::Bool));
        }
        let null = Known(ValueType::Null);
        if left_type == null || right_type == null {
            return Err(ExprError::type_error(
                format!("`{symbol}` needs two values, and `null` is the absence of one"),
                at,
            )
            .with_fix("Compare with == or !=, or give a default with coalesce."));
        }
        let Known(shared) = &common else {
            return Ok(match op {
                BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => Known(ValueType::Bool),
                _ => Dyn,
            });
        };
        let (allowed, result) = match op {
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => (
                matches!(
                    shared,
                    ValueType::Int
                        | ValueType::Decimal
                        | ValueType::Text
                        | ValueType::Date
                        | ValueType::Instant
                        | ValueType::Duration
                ),
                Known(ValueType::Bool),
            ),
            BinaryOp::Add => (
                matches!(
                    shared,
                    ValueType::Int | ValueType::Decimal | ValueType::Text | ValueType::List(_)
                ),
                common.clone(),
            ),
            BinaryOp::Sub | BinaryOp::Mul => (
                matches!(shared, ValueType::Int | ValueType::Decimal),
                common.clone(),
            ),
            _ => (*shared == ValueType::Int, common.clone()),
        };
        if allowed {
            return Ok(result);
        }
        let error = ExprError::type_error(
            format!("`{symbol}` does not work on a {}", shared.name()),
            at,
        );
        Err(if *shared == ValueType::Decimal {
            error.with_fix(
                "Write div(a, b, places, \"half_even\"): a decimal division names its places and its rounding.",
            )
        } else {
            error
        })
    }

    fn method(
        &mut self,
        receiver: &Node,
        name: &str,
        args: &[Node],
        at: Span,
    ) -> Result<Checked, ExprError> {
        let receiver_type = self.check(receiver)?;
        match name {
            "size" if args.is_empty() => {
                check_sized(&receiver_type, receiver.span)?;
                Ok(Known(ValueType::Int))
            }
            "startsWith" | "endsWith" | "contains" if args.len() == 1 => {
                let needs = format!("`{name}` works on text");
                if !is(&receiver_type, &ValueType::Text) {
                    return Err(ExprError::type_error(
                        format!("{needs}; this is a {}", describe(&receiver_type)),
                        receiver.span,
                    ));
                }
                self.expect(&args[0], &ValueType::Text, &needs)?;
                Ok(Known(ValueType::Bool))
            }
            "size" | "startsWith" | "endsWith" | "contains" => Err(ExprError::invalid(
                format!("`{name}` is given the wrong number of values"),
                at,
            )),
            _ => {
                let error =
                    ExprError::invalid(format!("there is no member function named `{name}`"), at);
                Err(if library().get(name).is_some() {
                    error.with_fix(format!(
                        "Host functions are called by name, not on a value: write {name}(value, ...)."
                    ))
                } else {
                    error.with_fix(
                        "The member functions are size, startsWith, endsWith, contains, all, exists, exists_one, map, and filter.",
                    )
                })
            }
        }
    }

    fn each(
        &mut self,
        kind: MacroKind,
        list: &Node,
        variable: &str,
        body: &Node,
        at: Span,
    ) -> Result<Checked, ExprError> {
        let list_type = self.check(list)?;
        let item = match &list_type {
            Dyn => Dyn,
            Known(ValueType::List(item)) => Known((**item).clone()),
            Known(_) => {
                return Err(ExprError::type_error(
                    format!(
                        "`{}` goes over a list; this is a {}",
                        kind.name(),
                        describe(&list_type)
                    ),
                    list.span,
                ));
            }
        };
        let taken = RESERVED.contains(&variable)
            || library().get(variable).is_some()
            || self.env.names.contains_key(variable)
            || self.locals.iter().any(|(local, _)| local == variable);
        if taken {
            return Err(ExprError::invalid(
                format!("`{variable}` already means something here"),
                at,
            )
            .with_fix("Give each item a name that nothing else has."));
        }
        self.locals.push((variable.to_owned(), item));
        let body_type = self.check(body);
        self.locals.pop();
        let body_type = body_type?;
        if kind != MacroKind::Map && !is(&body_type, &ValueType::Bool) {
            return Err(ExprError::type_error(
                format!(
                    "`{}` needs a condition for each item; this is a {}",
                    kind.name(),
                    describe(&body_type)
                ),
                body.span,
            ));
        }
        Ok(match kind {
            MacroKind::All | MacroKind::Exists | MacroKind::ExistsOne => Known(ValueType::Bool),
            MacroKind::Filter => list_type,
            MacroKind::Map => match body_type {
                Known(item) if item != ValueType::Null => Known(ValueType::list_of(item)),
                _ => Dyn,
            },
        })
    }

    fn call(&mut self, name: &str, args: &[Node], at: Span) -> Result<Checked, ExprError> {
        let Some(function) = library().get(name) else {
            return Err(
                ExprError::invalid(format!("there is no function named `{name}`"), at).with_fix(
                    "Use a function of the host-function library; anything else is a reviewed plug-in.",
                ),
            );
        };
        self.functions.insert(function.name());
        let too_many = args.len() > function.params.len() && function.id != FnId::Coalesce;
        if args.len() < function.required || too_many {
            return Err(ExprError::invalid(
                format!("`{name}` is given the wrong number of values"),
                at,
            )
            .with_fix(format!("Write {}.", function.signature())));
        }
        match function.id {
            FnId::Coalesce => {
                let mut common = Known(ValueType::Null);
                for arg in args {
                    let found = self.check(arg)?;
                    common = unify(&common, &found).ok_or_else(|| {
                        ExprError::type_error(
                            format!(
                                "`coalesce` chooses among values of one type; this is a {} among {}s",
                                describe(&found),
                                describe(&common)
                            ),
                            arg.span,
                        )
                    })?;
                }
                Ok(common)
            }
            FnId::Size => {
                let found = self.check(&args[0])?;
                check_sized(&found, args[0].span)?;
                Ok(Known(ValueType::Int))
            }
            FnId::Decimal => self.decimal(&args[0]),
            _ => self.host(function, args, at),
        }
    }

    /// `decimal(x)`: a whole number, or a decimal written as literal text.
    fn decimal(&mut self, arg: &Node) -> Result<Checked, ExprError> {
        let found = self.check(arg)?;
        if let Kind::Text(literal) = &arg.kind {
            return match Decimal::parse(literal) {
                Ok(_) => Ok(Known(ValueType::Decimal)),
                Err(problem) => Err(ExprError::type_error(problem.to_string(), arg.span)
                    .with_fix("Write the number with digits and at most one point, such as decimal(\"12.50\").")),
            };
        }
        match found {
            Dyn | Known(ValueType::Int | ValueType::Decimal) => Ok(Known(ValueType::Decimal)),
            Known(ValueType::Text | ValueType::Choice) => Err(ExprError::type_error(
                "`decimal` reads text only when it is written in the expression",
                arg.span,
            )
            .with_fix(
                "Write parse_decimal(text, places, rounding): text becomes a decimal only with the field type's places and rounding.",
            )),
            Known(other) => Err(ExprError::type_error(
                format!("`decimal` takes a whole number; this is a {}", other.name()),
                arg.span,
            )),
        }
    }

    fn host(&mut self, function: &HostFn, args: &[Node], at: Span) -> Result<Checked, ExprError> {
        let name = function.name().as_str();
        for (arg, (param_name, param)) in args.iter().zip(function.params) {
            if param.is_rule() {
                check_rule(*param, param_name, name, arg)?;
                continue;
            }
            let found = self.check(arg)?;
            let (fits, wanted) = match param {
                Param::Text => (is(&found, &ValueType::Text), "text"),
                Param::Decimal => (is(&found, &ValueType::Decimal), "a decimal"),
                Param::Number => (
                    is(&found, &ValueType::Int) || is(&found, &ValueType::Decimal),
                    "a whole number or a decimal",
                ),
                Param::Date => (is(&found, &ValueType::Date), "a date"),
                Param::Instant => (is(&found, &ValueType::Instant), "an instant"),
                _ => (true, "any value"),
            };
            if !fits {
                let error = ExprError::type_error(
                    format!(
                        "`{name}` takes {wanted} as its {param_name}; this is a {}",
                        describe(&found)
                    ),
                    arg.span,
                );
                return Err(
                    if found == Known(ValueType::Int) && *param == Param::Decimal {
                        error.with_fix("Write decimal(...) around the whole number.")
                    } else {
                        error
                    },
                );
            }
        }
        // Rules that only make sense together.
        let literal = |index: usize| match args.get(index).map(|arg| &arg.kind) {
            Some(Kind::Text(text)) => text.as_str(),
            _ => "",
        };
        let units = match function.id {
            FnId::ToUnit => Some((literal(1), literal(2))),
            FnId::AsMeasurement if literal(1) == measure::FEET_INCHES => Some(("in", literal(2))),
            FnId::AsMeasurement => Some((literal(1), literal(2))),
            _ => None,
        };
        if let Some((from, to)) = units
            && !measure::convertible(from, to)
        {
            return Err(ExprError::invalid(
                format!("`{from}` and `{to}` measure different kinds of thing"),
                at,
            ));
        }
        Ok(Known(match function.ret {
            Ret::Bool => ValueType::Bool,
            Ret::Int => ValueType::Int,
            Ret::Text | Ret::Same => ValueType::Text,
            Ret::Decimal => ValueType::Decimal,
            Ret::Date => ValueType::Date,
            Ret::Instant => ValueType::Instant,
            Ret::Duration => ValueType::Duration,
            Ret::Choice => ValueType::Choice,
        }))
    }
}

fn check_sized(found: &Checked, at: Span) -> Result<(), ExprError> {
    match found {
        Dyn
        | Known(ValueType::Text | ValueType::Choice | ValueType::List(_) | ValueType::Record(_)) => {
            Ok(())
        }
        Known(other) => Err(ExprError::type_error(
            format!(
                "`size` counts text, a list, or a record; this is a {}",
                other.name()
            ),
            at,
        )),
    }
}

/// The type of field `name` of a record of type `record`.
fn field_of(record: &Checked, name: &str, at: Span) -> Result<Checked, ExprError> {
    match record {
        Dyn => Ok(Dyn),
        Known(ValueType::Record(fields)) => {
            if let Some(field) = fields.iter().find(|field| field.name == name) {
                return Ok(Known(field.ty.clone()));
            }
            let error = ExprError::invalid(format!("the record has no field `{name}`"), at);
            let known: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
            Err(if known.is_empty() {
                error.with_fix("The record declares no fields.")
            } else {
                error.with_fix(format!("Its fields are: {}.", known.join(", ")))
            })
        }
        Known(other) => Err(ExprError::type_error(
            format!("only a record has fields; this is a {}", other.name()),
            at,
        )),
    }
}

/// Checks an argument that is a rule: it is written as a literal, and it is
/// one the library knows.
fn check_rule(param: Param, param_name: &str, function: &str, arg: &Node) -> Result<(), ExprError> {
    let not_literal = || {
        ExprError::invalid(
            format!("the {param_name} of `{function}` is a rule, written in the expression itself"),
            arg.span,
        )
        .with_fix("Write the rule as a literal; it comes from the field type, not from a value.")
    };
    match (param, &arg.kind) {
        (Param::Places, Kind::Int(places)) => {
            if (0..=i64::from(Decimal::MAX_PLACES)).contains(places) {
                Ok(())
            } else {
                Err(ExprError::invalid(
                    "a decimal has from 0 to 28 places",
                    arg.span,
                ))
            }
        }
        (Param::Codes, Kind::Record(fields)) => {
            let mut labels = BTreeSet::new();
            for (_, label) in fields {
                let Kind::Text(label_text) = &label.kind else {
                    return Err(ExprError::invalid(
                        "a code list maps each stored code to its label, both as literal text",
                        label.span,
                    ));
                };
                if !labels.insert(crate::text::collapse_space(&crate::text::nfc(label_text))) {
                    return Err(ExprError::invalid(
                        "two codes of a code list have the same label",
                        label.span,
                    ));
                }
            }
            if fields.is_empty() {
                return Err(ExprError::invalid(
                    "a code list has at least one code",
                    arg.span,
                ));
            }
            Ok(())
        }
        (Param::Places | Param::Codes, _) => Err(not_literal()),
        (_, Kind::Text(literal)) => functions::check_rule_text(param, literal)
            .map_err(|problem| ExprError::invalid(problem, arg.span)),
        _ => Err(not_literal()),
    }
}
