//! Field-type normalization: one rule for what a caller sent and what a
//! system showed, so the two are compared as typed values.

use a2m_core::{CodeLabel, CompiledExpr, Decimal, Failure, FieldBase, Rounding, Value, ValueType};

use crate::check::{Compiled, TypeEnv, compile};
use crate::dates::Shape;
use crate::error::{ExprError, Span};
use crate::eval::{Bindings, Program};
use crate::functions::{CASE_INSENSITIVE, CASE_SENSITIVE};
use crate::library::LibraryVersion;
use crate::measure::FEET_INCHES;
use crate::numbers::{DecimalMark, currency_places};
use crate::syntax::write_text;

/// The one name a normalizer reads: the raw value.
pub const NORMALIZER_INPUT: &str = "value";

/// What a field type does to whitespace before text is compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Whitespace {
    /// Every run of whitespace becomes one space, and the ends have none.
    #[default]
    Collapse,
    /// Whitespace at the ends is dropped.
    Trim,
    /// Text is compared as written.
    Exact,
}

impl Whitespace {
    const fn name(self) -> &'static str {
        match self {
            Self::Collapse => "collapse",
            Self::Trim => "trim",
            Self::Exact => "exact",
        }
    }
}

/// The rule by which one field type normalizes a value: what `types.yaml`
/// says about it, as far as comparing goes. The compiler builds one from a
/// field type and stores [`FieldRule::normalizer`] in the compiled target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldRule {
    /// Text: Unicode form C, then the whitespace rule, then the case rule.
    Text {
        /// False when case is ignored.
        case_sensitive: bool,
        /// What is done to whitespace.
        whitespace: Whitespace,
    },
    /// A whole number, shown with or without grouping.
    Integer {
        /// The decimal mark of the display, which says what may group digits.
        mark: DecimalMark,
    },
    /// An exact number with places and a rounding mode.
    Decimal {
        /// Decimal places.
        places: u32,
        /// How a value is rounded to them.
        rounding: Rounding,
        /// The decimal mark of the display.
        mark: DecimalMark,
    },
    /// An amount in one currency.
    Money {
        /// The ISO 4217 code.
        currency: String,
        /// Decimal places; the currency's own when not stated.
        places: Option<u32>,
        /// How a value is rounded to them.
        rounding: Rounding,
        /// The decimal mark of the display.
        mark: DecimalMark,
    },
    /// A quantity, stored in one unit and shown in another.
    Measurement {
        /// The unit a value is stored and compared in.
        unit: String,
        /// The unit the system shows: a unit's name, or `ft_in`.
        display_unit: String,
        /// The smallest increment, in the stored unit.
        step: Decimal,
        /// How a value is rounded to a step.
        rounding: Rounding,
        /// The decimal mark of the display.
        mark: DecimalMark,
    },
    /// A calendar day, shown in a format.
    Date {
        /// The format, such as `MM/DD/YYYY`.
        format: String,
    },
    /// A moment, shown in a format as the wall time of a zone.
    Datetime {
        /// The format, such as `YYYY-MM-DD HH:mm`.
        format: String,
        /// `UTC`, or a fixed offset such as `+02:00`.
        zone: String,
    },
    /// A time of day, shown in a format.
    Time {
        /// The format, such as `HH:mm`.
        format: String,
    },
    /// True or false, shown as one of two labels.
    Boolean {
        /// The text shown for true.
        true_text: String,
        /// The text shown for false.
        false_text: String,
    },
    /// One of a list of stored codes, shown by its label.
    Choice {
        /// The codes and labels, in declared order.
        codes: Vec<CodeLabel>,
    },
}

fn quoted(text: &str) -> String {
    let mut out = String::new();
    write_text(text, &mut out);
    out
}

impl FieldRule {
    /// The field-type base this rule is for.
    #[must_use]
    pub const fn base(&self) -> FieldBase {
        match self {
            Self::Text { .. } => FieldBase::Text,
            Self::Integer { .. } => FieldBase::Integer,
            Self::Decimal { .. } => FieldBase::Decimal,
            Self::Money { .. } => FieldBase::Money,
            Self::Measurement { .. } => FieldBase::Measurement,
            Self::Date { .. } => FieldBase::Date,
            Self::Datetime { .. } => FieldBase::Datetime,
            Self::Time { .. } => FieldBase::Time,
            Self::Boolean { .. } => FieldBase::Boolean,
            Self::Choice { .. } => FieldBase::Choice,
        }
    }

    /// The type of a normalized value. A time of day is the time since
    /// midnight, a duration.
    #[must_use]
    pub const fn value_type(&self) -> ValueType {
        match self {
            Self::Text { .. } => ValueType::Text,
            Self::Integer { .. } => ValueType::Int,
            Self::Decimal { .. } | Self::Money { .. } | Self::Measurement { .. } => {
                ValueType::Decimal
            }
            Self::Date { .. } => ValueType::Date,
            Self::Datetime { .. } => ValueType::Instant,
            Self::Time { .. } => ValueType::Duration,
            Self::Boolean { .. } => ValueType::Bool,
            Self::Choice { .. } => ValueType::Choice,
        }
    }

    /// The normalizer as an expression over [`NORMALIZER_INPUT`]: one call
    /// of the host function for the base, with the rule written into it.
    fn source(&self) -> String {
        let value = NORMALIZER_INPUT;
        match self {
            Self::Text {
                case_sensitive,
                whitespace,
            } => {
                let case = if *case_sensitive {
                    CASE_SENSITIVE
                } else {
                    CASE_INSENSITIVE
                };
                format!(
                    "as_text({value}, {}, {})",
                    quoted(case),
                    quoted(whitespace.name())
                )
            }
            Self::Integer { mark } => format!("as_integer({value}, {})", quoted(mark.as_str())),
            Self::Decimal {
                places,
                rounding,
                mark,
            } => format!(
                "as_decimal({value}, {places}, {}, {})",
                quoted(rounding.as_str()),
                quoted(mark.as_str())
            ),
            Self::Money {
                currency,
                places,
                rounding,
                mark,
            } => format!(
                "as_money({value}, {}, {}, {}, {})",
                quoted(currency),
                places.unwrap_or_else(|| currency_places(currency)),
                quoted(rounding.as_str()),
                quoted(mark.as_str())
            ),
            Self::Measurement {
                unit,
                display_unit,
                step,
                rounding,
                mark,
            } => format!(
                "as_measurement({value}, {}, {}, decimal({}), {}, {})",
                quoted(display_unit),
                quoted(unit),
                quoted(&step.to_string()),
                quoted(rounding.as_str()),
                quoted(mark.as_str())
            ),
            Self::Date { format } => format!("as_date({value}, {})", quoted(format)),
            Self::Datetime { format, zone } => {
                format!("as_datetime({value}, {}, {})", quoted(format), quoted(zone))
            }
            Self::Time { format } => format!("as_time({value}, {})", quoted(format)),
            Self::Boolean {
                true_text,
                false_text,
            } => format!(
                "as_boolean({value}, {}, {})",
                quoted(true_text),
                quoted(false_text)
            ),
            Self::Choice { codes } => {
                let pairs: Vec<String> = codes
                    .iter()
                    .map(|pair| format!("{}: {}", quoted(&pair.code), quoted(&pair.label)))
                    .collect();
                format!("as_choice({value}, {{{}}})", pairs.join(", "))
            }
        }
    }

    /// Compiles the rule into its normalizer. The compiler stores
    /// `to_compiled_expr()` of the result with the field type.
    ///
    /// # Errors
    ///
    /// `expression_invalid` when the rule names something the library does
    /// not know: a unit, a format, a zone, a rounding of more than 28 places,
    /// a code list with a code or a label twice, or a step that is not more
    /// than zero.
    pub fn normalizer(&self) -> Result<Compiled, ExprError> {
        if let Self::Measurement { step, .. } = self
            && (step.is_zero() || step.is_negative())
        {
            return Err(ExprError::invalid(
                "a measurement's step is more than zero",
                Span::default(),
            ));
        }
        if let Self::Boolean {
            true_text,
            false_text,
        } = self
            && crate::text::fold(&crate::text::collapse_space(true_text))
                == crate::text::fold(&crate::text::collapse_space(false_text))
        {
            return Err(ExprError::invalid(
                "the labels for true and false are the same",
                Span::default(),
            ));
        }
        let mut env = TypeEnv::new(LibraryVersion::CURRENT);
        env.declare_unknown(NORMALIZER_INPUT)?;
        compile(&self.source(), &env, Some(&self.value_type()))
    }
}

/// A raw value and what its field type's rule makes of it. Both are kept, so
/// a mismatch report shows exactly what the system did to the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized {
    /// The value as it was sent or shown.
    pub raw: Value,
    /// The value after the field type's rule.
    pub normalized: Value,
}

impl Normalized {
    /// Whether two values are the same under their field type's rule.
    #[must_use]
    pub fn agrees_with(&self, other: &Self) -> bool {
        self.normalized == other.normalized
    }
}

/// Field-type normalization, as the compiled target stores it: the loaded
/// program of the expression the compiler built for the field type.
#[derive(Debug, Clone)]
pub struct Normalizer {
    program: Program,
}

impl Normalizer {
    /// Loads a field type's normalizer from the compiled target.
    ///
    /// # Errors
    ///
    /// `compiled_expression_unloadable`, as for [`Program::load`], and when
    /// the expression reads a name other than [`NORMALIZER_INPUT`].
    pub fn load(expr: &CompiledExpr) -> Result<Self, Failure> {
        let program = Program::load(expr)?;
        if program.names().iter().any(|name| name != NORMALIZER_INPUT) {
            return Err(Failure::new(
                a2m_core::FailureClass::Input,
                crate::error::COMPILED_EXPRESSION_UNLOADABLE,
                "A compiled target's expression cannot be loaded: a normalizer reads only the raw value.",
                "Compile the target again with this engine.",
            ));
        }
        Ok(Self { program })
    }

    /// The type of a normalized value.
    #[must_use]
    pub const fn value_type(&self) -> &ValueType {
        self.program.result_type()
    }

    /// Normalizes a raw value: one a caller sent, already typed, or one a
    /// system showed, as text. An absent value stays absent.
    ///
    /// # Errors
    ///
    /// `value_unparsable` when the value cannot be read by the field type's
    /// rule, and `value_out_of_range` when it cannot be represented.
    pub fn normalize(&self, raw: &Value) -> Result<Normalized, Failure> {
        let bindings = Bindings::new().with(NORMALIZER_INPUT, raw);
        let normalized = self.program.evaluate(&bindings)?;
        Ok(Normalized {
            raw: raw.clone(),
            normalized,
        })
    }
}

/// Checks a date, time, or date-and-time format without compiling anything.
/// For the compiler's lint of a field type's `format`.
///
/// # Errors
///
/// `expression_invalid`, with the sentence that says what the format lacks.
pub fn check_format(format: &str, base: FieldBase) -> Result<(), ExprError> {
    let shape = match base {
        FieldBase::Date => Shape::Date,
        FieldBase::Datetime => Shape::DateTime,
        FieldBase::Time => Shape::Time,
        _ => {
            return Err(ExprError::invalid(
                "only a date, a time, or a date and time has a format of this kind",
                Span::default(),
            ));
        }
    };
    crate::dates::Format::new(format, shape)
        .map(|_| ())
        .map_err(|problem| ExprError::invalid(problem, Span::default()))
}

/// The name of the display form that writes a length as feet and inches,
/// for a measurement's `display_unit`.
pub const DISPLAY_FEET_INCHES: &str = FEET_INCHES;
