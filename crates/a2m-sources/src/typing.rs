//! Turning one cell into a typed value by a field type. This is the one
//! place that says what a cell means under a field type; `read` and the file
//! pack both use it, so a value never means one thing to a source and
//! another to a file tool.
//!
//! The field type's own rules, places and rounding, units and steps, the
//! format of a date, the codes of a choice, are its normalizer's, from
//! `a2m-expr`. What is here is only which raw value a cell hands to it.

use a2m_core::{Failure, FailureCode, FieldBase, Value};
use a2m_expr::DateSystem;

use crate::definition::FieldTyper;
use crate::error::{FORMULA_WITHOUT_VALUE, Problem, SCHEMA_INVALID, VALUE_OUT_OF_RANGE};
use crate::model::{Cell, RawCell};
use crate::number::{self, NumberFault};

/// What typing a cell needs to know beyond the cell and its field type.
#[derive(Debug, Clone, Copy, Default)]
pub struct CellContext<'a> {
    /// The date system of the workbook the cell is from. Nothing for a
    /// format that has none; a number is then never read as a date.
    pub date_system: Option<DateSystem>,
    /// The texts that count as no value. Empty unless a definition lists
    /// some.
    pub missing: &'a [String],
}

type Fault = (FailureCode, String);

fn fault(code: FailureCode, message: &str) -> Fault {
    (code, message.to_owned())
}

fn from_failure(failure: &Failure) -> Fault {
    (failure.code().clone(), failure.message().to_owned())
}

fn exact(text: &str) -> Result<a2m_core::Decimal, Fault> {
    number::exact(text).map_err(|problem| match problem {
        NumberFault::NotANumber => fault(
            SCHEMA_INVALID,
            "The cell is stored as a number and its content is not one.",
        ),
        NumberFault::OutOfRange => fault(
            VALUE_OUT_OF_RANGE,
            "The number has more digits than an exact decimal holds.",
        ),
    })
}

const fn base_noun(base: FieldBase) -> &'static str {
    match base {
        FieldBase::Text => "text",
        FieldBase::Integer => "a whole number",
        FieldBase::Decimal => "a decimal",
        FieldBase::Money => "money",
        FieldBase::Measurement => "a measurement",
        FieldBase::Date => "a date",
        FieldBase::Datetime => "a date and time",
        FieldBase::Time => "a time of day",
        FieldBase::Boolean => "true or false",
        FieldBase::Choice => "a choice",
    }
}

/// The value a cell hands to a field type of base `base`, before the field
/// type's rule is applied. An absent value is `Null`.
fn raw_value(raw: &RawCell, base: FieldBase, context: &CellContext<'_>) -> Result<Value, Fault> {
    let Some(raw) = raw.settled() else {
        return Err(fault(
            FORMULA_WITHOUT_VALUE,
            "The cell holds a formula and the workbook kept no result for it.",
        ));
    };
    let serial = |text: &str| -> Result<Value, Fault> {
        let system = context.date_system.ok_or_else(|| {
            fault(
                SCHEMA_INVALID,
                "The field is a date and the value is a number, in a format that has no date system.",
            )
        })?;
        a2m_expr::parse::excel_serial(&exact(text)?, system)
            .map(Value::Date)
            .map_err(|failure| from_failure(&failure))
    };
    let unfit = |what: &str| {
        Err(fault(
            SCHEMA_INVALID,
            &format!(
                "The cell holds {what} and the field is {}.",
                base_noun(base)
            ),
        ))
    };
    match raw {
        RawCell::Empty => Ok(Value::Null),
        RawCell::Text(text) if context.missing.contains(text) => Ok(Value::Null),
        RawCell::Text(text) => Ok(Value::Text(text.clone())),
        RawCell::Number(text) => match base {
            FieldBase::Integer | FieldBase::Decimal | FieldBase::Money | FieldBase::Measurement => {
                exact(text).map(Value::Decimal)
            }
            FieldBase::Date => serial(text),
            // Only a plain whole number has one spelling as text.
            FieldBase::Text | FieldBase::Choice if number::is_plain_whole(text) => {
                Ok(Value::Text(text.clone()))
            }
            FieldBase::Text | FieldBase::Choice => {
                unfit("a number that is not a plain whole number")
            }
            FieldBase::Datetime | FieldBase::Time | FieldBase::Boolean => unfit("a number"),
        },
        RawCell::DateSerial(text) => match base {
            FieldBase::Date => serial(text),
            _ => unfit("a number formatted as a date or a time"),
        },
        RawCell::Bool(flag) => match base {
            FieldBase::Boolean => Ok(Value::Bool(*flag)),
            _ => unfit("true or false"),
        },
        RawCell::Error(_) => unfit("an error"),
        RawCell::Nested => unfit("a list or a record"),
        // `settled` never gives a formula back.
        RawCell::Formula { .. } => unfit("a formula"),
    }
}

/// The value a cell has in the record `row` an expression reads: text as
/// text, a number or a date serial as an exact decimal, true or false, and
/// `Null` for an empty cell or a text listed as missing.
pub(crate) fn row_value(raw: &RawCell, context: &CellContext<'_>) -> Result<Value, Fault> {
    let Some(raw) = raw.settled() else {
        return Err(fault(
            FORMULA_WITHOUT_VALUE,
            "The cell holds a formula and the workbook kept no result for it.",
        ));
    };
    match raw {
        RawCell::Empty => Ok(Value::Null),
        RawCell::Text(text) if context.missing.contains(text) => Ok(Value::Null),
        RawCell::Text(text) => Ok(Value::Text(text.clone())),
        RawCell::Number(text) | RawCell::DateSerial(text) => exact(text).map(Value::Decimal),
        RawCell::Bool(flag) => Ok(Value::Bool(*flag)),
        RawCell::Error(_) => Err(fault(SCHEMA_INVALID, "The cell holds an error.")),
        RawCell::Nested | RawCell::Formula { .. } => Err(fault(
            SCHEMA_INVALID,
            "The cell holds a list or a record where one value is expected.",
        )),
    }
}

/// Applies a field type's rule to a raw value. An absent value stays absent.
pub(crate) fn normalized(value: &Value, typer: &FieldTyper) -> Result<Value, Fault> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    typer
        .normalizer()
        .normalize(value)
        .map(|normalized| normalized.normalized)
        .map_err(|failure| from_failure(&failure))
}

/// Turns one cell into a typed value by a field type: the same code `read`
/// uses. A number is read from its stored digits, a date serial by the
/// workbook's own date system, and everything else by the field type's
/// normalizer. An empty cell, and a text the context lists as missing, is
/// `Null`.
///
/// # Errors
///
/// A [`Problem`] at the cell: `formula_without_value` for a formula with no
/// kept result; `schema_invalid` when the cell holds a kind of value the
/// field type cannot take; `value_unparsable` and `value_out_of_range` from
/// the field type's rule. The message never holds the value.
pub fn typed(cell: &Cell, typer: &FieldTyper, context: &CellContext<'_>) -> Result<Value, Problem> {
    raw_value(&cell.raw, typer.base(), context)
        .and_then(|value| normalized(&value, typer))
        .map_err(|(code, message)| Problem::new(code, message).at(cell.at))
}
