//! A compiled target as the engine holds it: checked against its hash, with
//! every expression parsed once and every field type's normalizer ready.

use std::sync::Arc;

use a2m_core::{
    CompiledInput, CompiledTarget, Digest, ExprIndex, Failure, FieldTypeIndex, Sensitivity, Value,
    Values,
};
use a2m_expr::{Bindings, Normalizer, Program, names};

use crate::codes;

/// The name a step's expression reads for the record's current values: what
/// the system held when the engine last read the record, as the driver gave
/// it, or null when the record does not exist.
pub const CURRENT: &str = names::CURRENT;

/// The name a step's expression reads for the record's key in the system:
/// `record.key`, its parts as text, or null while it has none. Never the
/// engine's own name for the record in the run.
pub const RECORD: &str = names::RECORD;

pub(crate) struct Loaded {
    pub compiled_target: Arc<CompiledTarget>,
    pub hash: Digest,
    programs: Vec<Program>,
    normalizers: Vec<Option<Normalizer>>,
}

impl Loaded {
    /// Checks the compiled target against the hash the job names, and loads
    /// it.
    pub fn load(compiled_target: Arc<CompiledTarget>, expected: &Digest) -> Result<Self, Failure> {
        let unverified = || {
            codes::fail(
                &codes::row::COMPILED_TARGET_UNVERIFIED,
                "The compiled target handed to the engine is not the compiled target the job names by its hash.",
                "Publish the target again from its stored sources.",
            )
        };
        let encoded = compiled_target.encode().map_err(|_| unverified())?;
        if encoded.hash != *expected {
            return Err(unverified());
        }
        let programs = compiled_target
            .expressions
            .iter()
            .map(Program::load)
            .collect::<Result<Vec<_>, _>>()?;
        let mut normalizers = Vec::with_capacity(compiled_target.field_types.len());
        for field_type in &compiled_target.field_types {
            let normalizer = match field_type.normalizer {
                Some(index) => {
                    let expr = compiled_target.expression(index).ok_or_else(unverified)?;
                    Some(Normalizer::load(expr)?)
                }
                None => None,
            };
            normalizers.push(normalizer);
        }
        Ok(Self {
            compiled_target,
            hash: *expected,
            programs,
            normalizers,
        })
    }

    pub fn eval(&self, index: ExprIndex, scope: &Scope) -> Result<Value, Failure> {
        let program = self.programs.get(index.get()).ok_or_else(|| {
            codes::job_invalid("The compiled target refers to an expression it does not hold.")
        })?;
        let bindings = Bindings::new()
            .with(names::INPUT, &scope.input)
            .with(names::RESULT, &scope.result)
            .with(names::ROW, &scope.row)
            .with(names::SOURCE, &scope.source)
            .with(names::STEPS, &scope.steps)
            .with(names::ENV, &scope.env)
            .with(CURRENT, &scope.current)
            .with(RECORD, &scope.record);
        program.evaluate(&bindings)
    }

    /// Whether an expression reads the name given.
    pub fn reads(&self, index: ExprIndex, name: &str) -> bool {
        self.programs
            .get(index.get())
            .is_some_and(|program| program.names().iter().any(|read| read == name))
    }

    /// Whether an expression that gives true or false holds.
    ///
    /// A condition that gives anything else, a text or a null from an
    /// answer that changed, is no rule the engine can follow: it stops,
    /// and never takes the value for false.
    pub fn holds(&self, index: ExprIndex, scope: &Scope) -> Result<bool, Failure> {
        self.eval(index, scope)?.as_bool().ok_or_else(|| {
            codes::fail(
                &codes::row::CONDITION_NOT_BOOLEAN,
                "A condition of the compiled target gave a value that is neither true nor false.",
                "Compare the condition in the target files with what the system returns.",
            )
        })
    }

    pub fn sensitivity(&self, field_type: Option<FieldTypeIndex>) -> Sensitivity {
        field_type
            .and_then(|index| self.compiled_target.field_type(index))
            .map_or(Sensitivity::Internal, |field_type| field_type.sensitivity)
    }

    /// A raw value under its field type's rule. A field with no normalizer
    /// is compared as it stands.
    pub fn normalize(
        &self,
        field_type: Option<FieldTypeIndex>,
        raw: &Value,
    ) -> Result<Value, Failure> {
        match field_type.and_then(|index| self.normalizers.get(index.get())) {
            Some(Some(normalizer)) => Ok(normalizer.normalize(raw)?.normalized),
            _ => Ok(raw.clone()),
        }
    }

    /// Validates the inputs of an operation or a pipeline against their
    /// declarations, and normalizes each by its field type. Every problem
    /// is reported at once, by the name of its input and never by a value.
    pub fn inputs(&self, declared: &[CompiledInput], given: &Values) -> Result<Values, Failure> {
        self.inputs_waiting(declared, given, &[])
    }

    /// As [`Loaded::inputs`], for a call some of whose inputs wait for a
    /// record written earlier in the run: an input named in `waiting` may
    /// be missing.
    pub fn inputs_waiting(
        &self,
        declared: &[CompiledInput],
        given: &Values,
        waiting: &[String],
    ) -> Result<Values, Failure> {
        let mut problems = Vec::new();
        let mut out = Values::new();
        for name in given.names() {
            if !declared.iter().any(|input| input.name == name) {
                problems.push(format!("`{name}` is not an input"));
            }
        }
        for input in declared {
            let raw = given
                .get(&input.name)
                .filter(|value| !value.is_null())
                .or(input.default.as_ref());
            let Some(raw) = raw else {
                if input.required && !waiting.contains(&input.name) {
                    problems.push(format!("`{}` is required", input.name));
                }
                continue;
            };
            match self.normalize(Some(input.field_type), raw) {
                Ok(value) if value.has_type(&input.ty) => {
                    out.insert(input.name.clone(), value);
                }
                Ok(_) => problems.push(format!("`{}` is not a {}", input.name, input.ty.name())),
                Err(failure) => problems.push(format!(
                    "`{}` cannot be read as its type ({})",
                    input.name,
                    failure.code()
                )),
            }
        }
        if problems.is_empty() {
            Ok(out)
        } else {
            Err(codes::fail(
                &codes::row::INPUT_INVALID,
                format!("The inputs were refused: {}.", problems.join("; ")),
                "Correct every input named and call the tool again.",
            ))
        }
    }
}

/// The values an expression may read, by the names the file formats give.
#[derive(Debug, Clone)]
pub(crate) struct Scope {
    pub input: Value,
    pub result: Value,
    pub row: Value,
    pub source: Value,
    pub steps: Value,
    pub env: Value,
    pub current: Value,
    pub record: Value,
}

impl Scope {
    pub fn new(env: Value) -> Self {
        Self {
            input: Value::Record(Values::new()),
            result: Value::Record(Values::new()),
            row: Value::Null,
            source: Value::Null,
            steps: Value::Record(Values::new()),
            env,
            current: Value::Null,
            record: Value::Null,
        }
    }

    pub fn with_input(&self, input: &Values) -> Self {
        let mut scope = self.clone();
        scope.input = Value::Record(input.clone());
        scope.result = Value::Record(Values::new());
        scope.current = Value::Null;
        scope.record = Value::Null;
        scope
    }

    /// The value of `record` for a record with this key in the system.
    pub fn record_of(key: Option<&a2m_core::RecordKey>) -> Value {
        let Some(key) = key else {
            return Value::Null;
        };
        let parts = key
            .parts()
            .iter()
            .map(|part| {
                Value::Text(match part {
                    Value::Text(text) => text.clone(),
                    Value::Int(number) => number.to_string(),
                    Value::Decimal(number) => number.exact_text(),
                    Value::Date(date) => date.to_string(),
                    Value::Bool(flag) => flag.to_string(),
                    Value::Instant(moment) => moment.as_unix_millis().to_string(),
                    Value::Duration(length) => length.get().to_string(),
                    other => {
                        let mut bytes = Vec::new();
                        other.write_canonical(&mut bytes);
                        String::from_utf8_lossy(&bytes).into_owned()
                    }
                })
            })
            .collect();
        Value::Record(Values::new().with("key", Value::List(parts)))
    }
}
