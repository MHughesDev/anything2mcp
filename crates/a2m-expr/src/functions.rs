//! What each host function does, over values.
//!
//! Every function is pure: the same arguments give the same answer on every
//! machine. A function that cannot do what it was asked stops with a fault
//! that names the kind of problem and never carries the value.

use std::str::FromStr;

use a2m_core::{Decimal, Millis, Rounding, Value};

use crate::dates::{self, DateSystem, Format, Offset, Shape};
use crate::error::Fault;
use crate::library::FnId;
use crate::measure::{self, FEET_INCHES};
use crate::numbers::{self, DecimalMark};
use crate::text;

/// The name of the case rule that compares text as written.
pub(crate) const CASE_SENSITIVE: &str = "sensitive";
/// The name of the case rule that ignores case.
pub(crate) const CASE_INSENSITIVE: &str = "insensitive";
/// The whitespace rules: collapse runs, trim the ends, or leave as written.
pub(crate) const WHITESPACE_RULES: [&str; 3] = ["collapse", "trim", "exact"];

const ABSENT: Fault = Fault::absent("a value the function needs is absent");
const WRONG_KIND: Fault = Fault::unparsable("a value is not of the kind the function reads");
const BAD_RULE: Fault =
    Fault::unloadable("a rule compiled into a call is not one this engine knows");

fn want_text(value: &Value) -> Result<&str, Fault> {
    match value {
        Value::Text(text) => Ok(text),
        Value::Null => Err(ABSENT),
        _ => Err(WRONG_KIND),
    }
}

fn want_decimal(value: &Value) -> Result<&Decimal, Fault> {
    match value {
        Value::Decimal(number) => Ok(number),
        Value::Null => Err(ABSENT),
        _ => Err(WRONG_KIND),
    }
}

fn rule_text(value: &Value) -> Result<&str, Fault> {
    value.as_text().ok_or(BAD_RULE)
}

pub(crate) fn places(value: &Value) -> Result<u32, Fault> {
    value
        .as_int()
        .and_then(|places| u32::try_from(places).ok())
        .filter(|places| *places <= Decimal::MAX_PLACES)
        .ok_or(BAD_RULE)
}

fn rounding(value: &Value) -> Result<Rounding, Fault> {
    Rounding::from_str(rule_text(value)?).map_err(|_| BAD_RULE)
}

/// The decimal mark at `index`, or a point when the argument was left out.
fn mark(args: &[&Value], index: usize) -> Result<DecimalMark, Fault> {
    match args.get(index) {
        None => Ok(DecimalMark::Point),
        Some(value) => DecimalMark::from_name(rule_text(value)?).ok_or(BAD_RULE),
    }
}

fn format(value: &Value, shape: Shape) -> Result<Format, Fault> {
    Format::new(rule_text(value)?, shape).map_err(|_| BAD_RULE)
}

fn zone(value: &Value) -> Result<Offset, Fault> {
    Offset::new(rule_text(value)?).map_err(|_| BAD_RULE)
}

/// Whether `code` has the shape of a currency code: three capital letters.
pub(crate) fn is_currency_code(code: &str) -> bool {
    code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_uppercase())
}

/// The form in which two labels are compared: form C, with whitespace collapsed.
fn label_form(label: &str) -> String {
    text::collapse_space(&text::nfc(label))
}

/// The stored code for `given`, which may be a code or a label.
fn code_for(given: &str, codes: &Value) -> Result<String, Fault> {
    const NO_SUCH_CHOICE: Fault = Fault::unparsable("the text is not one of the field's choices");
    let codes = codes.as_record().ok_or(BAD_RULE)?;
    if codes.contains(given) {
        return Ok(given.to_owned());
    }
    let wanted = label_form(given);
    let mut found = None;
    for (code, label) in codes.iter() {
        if label_form(rule_text(label)?) == wanted {
            if found.is_some() {
                return Err(Fault::unparsable("the text is the label of two choices"));
            }
            found = Some(code);
        }
    }
    found.map(str::to_owned).ok_or(NO_SUCH_CHOICE)
}

fn bool_from_display(given: &str, true_text: &str, false_text: &str) -> Result<bool, Fault> {
    let form = |label: &str| text::lower(&label_form(label));
    let given = form(given);
    if given == form(true_text) {
        Ok(true)
    } else if given == form(false_text) {
        Ok(false)
    } else {
        Err(Fault::unparsable(
            "the text is neither of the field's two labels",
        ))
    }
}

fn measurement_from_display(
    given: &str,
    display_unit: &str,
    unit: &str,
    step: &Decimal,
    rounding: Rounding,
    mark: DecimalMark,
) -> Result<Decimal, Fault> {
    if display_unit == FEET_INCHES {
        let inches = measure::inches_from_feet_inches(given)?;
        return measure::convert_and_snap(&inches, "in", unit, step, rounding);
    }
    // The display unit's name may follow the number.
    let body = given.trim();
    let body = body.strip_suffix(display_unit).map_or(body, str::trim_end);
    let value = numbers::decimal_from_display(body, mark)?;
    measure::convert_and_snap(&value, display_unit, unit, step, rounding)
}

fn millis_of_day(value: i64) -> Value {
    Value::Duration(Millis::new(value))
}

/// The cost of a call beyond its one unit: one more for each 64 bytes of text
/// it is given.
pub(crate) fn extra_cost(args: &[&Value]) -> u64 {
    let bytes: usize = args
        .iter()
        .map(|arg| arg.as_text().map_or(0, str::len))
        .sum();
    u64::try_from(bytes / 64).unwrap_or(u64::MAX)
}

/// Calls a host function. `coalesce` and `decimal` of a literal are the
/// evaluator's own and never arrive here with anything else to do.
pub(crate) fn call(id: FnId, args: &[&Value]) -> Result<Value, Fault> {
    let arg = |index: usize| args.get(index).copied().ok_or(BAD_RULE);
    let first = arg(0)?;
    // A field type's normalizer leaves an absent value absent.
    let normalizer = matches!(
        id,
        FnId::AsText
            | FnId::AsInteger
            | FnId::AsDecimal
            | FnId::AsMoney
            | FnId::AsMeasurement
            | FnId::AsDate
            | FnId::AsDatetime
            | FnId::AsTime
            | FnId::AsBoolean
            | FnId::AsChoice
    );
    if normalizer && first.is_null() {
        return Ok(Value::Null);
    }
    Ok(match id {
        FnId::Coalesce => args
            .iter()
            .find(|value| !value.is_null())
            .map_or(Value::Null, |value| (*value).clone()),
        FnId::Size => {
            let size = match first {
                Value::Text(text) => text.chars().count(),
                Value::List(items) => items.len(),
                Value::Record(fields) => fields.len(),
                Value::Null => return Err(ABSENT),
                _ => return Err(WRONG_KIND),
            };
            Value::Int(i64::try_from(size).map_err(|_| WRONG_KIND)?)
        }
        FnId::Decimal => match first {
            Value::Int(number) => Value::Decimal(Decimal::from_i64(*number)),
            Value::Decimal(number) => Value::Decimal(*number),
            Value::Text(text) => Value::Decimal(Decimal::parse(text).map_err(|_| BAD_RULE)?),
            Value::Null => return Err(ABSENT),
            _ => return Err(WRONG_KIND),
        },
        FnId::ParseDecimal => {
            let number = numbers::decimal_from_display(want_text(first)?, mark(args, 3)?)?;
            Value::Decimal(number.round(places(arg(1)?)?, rounding(arg(2)?)?))
        }
        FnId::ParseInt => Value::Int(numbers::int_from_display(
            want_text(first)?,
            mark(args, 1)?,
        )?),
        FnId::ParseMoney => {
            let currency = rule_text(arg(1)?)?;
            let number = numbers::money_from_display(want_text(first)?, currency, mark(args, 4)?)?;
            Value::Decimal(number.round(places(arg(2)?)?, rounding(arg(3)?)?))
        }
        FnId::ParseBool => Value::Bool(bool_from_display(
            want_text(first)?,
            rule_text(arg(1)?)?,
            rule_text(arg(2)?)?,
        )?),
        FnId::RoundTo => {
            Value::Decimal(want_decimal(first)?.round(places(arg(1)?)?, rounding(arg(2)?)?))
        }
        FnId::Div => {
            let divisor = want_decimal(arg(1)?)?;
            if divisor.is_zero() {
                return Err(Fault::range("the divisor is zero"));
            }
            want_decimal(first)?
                .checked_div(divisor, places(arg(2)?)?, rounding(arg(3)?)?)
                .map(Value::Decimal)
                .ok_or(Fault::range(
                    "the quotient has more digits than a decimal holds",
                ))?
        }
        FnId::ToInches => Value::Decimal(measure::inches_from_feet_inches(want_text(first)?)?),
        FnId::ToFeetInches => Value::Text(measure::feet_inches_from_inches(
            want_decimal(first)?,
            want_decimal(arg(1)?)?,
        )?),
        FnId::ToUnit => Value::Decimal(measure::convert(
            want_decimal(first)?,
            rule_text(arg(1)?)?,
            rule_text(arg(2)?)?,
            places(arg(3)?)?,
            rounding(arg(4)?)?,
        )?),
        FnId::SnapToStep => Value::Decimal(measure::snap(
            want_decimal(first)?,
            want_decimal(arg(1)?)?,
            rounding(arg(2)?)?,
        )?),
        FnId::ParseDate => Value::Date(dates::date_from_text(
            want_text(first)?,
            &format(arg(1)?, Shape::Date)?,
        )?),
        FnId::FormatDate => match first {
            Value::Date(date) => {
                Value::Text(dates::text_from_date(*date, &format(arg(1)?, Shape::Date)?))
            }
            Value::Null => return Err(ABSENT),
            _ => return Err(WRONG_KIND),
        },
        FnId::ParseDatetime => Value::Instant(dates::instant_from_text(
            want_text(first)?,
            &format(arg(1)?, Shape::DateTime)?,
            zone(arg(2)?)?,
        )?),
        FnId::FormatDatetime => match first {
            Value::Instant(moment) => Value::Text(dates::text_from_instant(
                *moment,
                &format(arg(1)?, Shape::DateTime)?,
                zone(arg(2)?)?,
            )?),
            Value::Null => return Err(ABSENT),
            _ => return Err(WRONG_KIND),
        },
        FnId::ParseTime => millis_of_day(dates::time_from_text(
            want_text(first)?,
            &format(arg(1)?, Shape::Time)?,
        )?),
        FnId::ExcelSerialToDate => {
            let system = DateSystem::from_name(rule_text(arg(1)?)?).ok_or(BAD_RULE)?;
            let serial = match first {
                Value::Int(serial) => Decimal::from_i64(*serial),
                Value::Decimal(serial) => *serial,
                Value::Null => return Err(ABSENT),
                _ => return Err(WRONG_KIND),
            };
            Value::Date(dates::date_from_serial(&serial, system)?)
        }
        FnId::Trim => Value::Text(text::trim(want_text(first)?).to_owned()),
        FnId::Lower => Value::Text(text::lower(want_text(first)?)),
        FnId::Upper => Value::Text(text::upper(want_text(first)?)),
        FnId::Nfc => Value::Text(text::nfc(want_text(first)?)),
        FnId::CollapseSpace => Value::Text(text::collapse_space(want_text(first)?)),
        FnId::CodeFor | FnId::AsChoice => Value::Text(code_for(want_text(first)?, arg(1)?)?),
        FnId::LabelFor => {
            let codes = arg(1)?.as_record().ok_or(BAD_RULE)?;
            let label = codes.get(want_text(first)?).ok_or(Fault::unparsable(
                "the code is not one of the field's choices",
            ))?;
            Value::Text(rule_text(label)?.to_owned())
        }
        FnId::AsText => {
            let composed = text::nfc(want_text(first)?);
            let spaced = match rule_text(arg(2)?)? {
                "collapse" => text::collapse_space(&composed),
                "trim" => text::trim(&composed).to_owned(),
                "exact" => composed,
                _ => return Err(BAD_RULE),
            };
            Value::Text(match rule_text(arg(1)?)? {
                CASE_SENSITIVE => spaced,
                CASE_INSENSITIVE => text::lower(&spaced),
                _ => return Err(BAD_RULE),
            })
        }
        FnId::AsInteger => match first {
            Value::Int(number) => Value::Int(*number),
            Value::Decimal(number) => Value::Int(number.to_i64().ok_or(Fault::unparsable(
                "the number is not a whole number that fits",
            ))?),
            Value::Text(text) => Value::Int(numbers::int_from_display(text, mark(args, 1)?)?),
            _ => return Err(WRONG_KIND),
        },
        FnId::AsDecimal => {
            let number = match first {
                Value::Decimal(number) => *number,
                Value::Int(number) => Decimal::from_i64(*number),
                Value::Text(text) => numbers::decimal_from_display(text, mark(args, 3)?)?,
                _ => return Err(WRONG_KIND),
            };
            Value::Decimal(number.round(places(arg(1)?)?, rounding(arg(2)?)?))
        }
        FnId::AsMoney => {
            let number = match first {
                Value::Decimal(number) => *number,
                Value::Text(text) => {
                    numbers::money_from_display(text, rule_text(arg(1)?)?, mark(args, 4)?)?
                }
                // A whole number could be units or the smallest unit, so it stops.
                _ => return Err(WRONG_KIND),
            };
            Value::Decimal(number.round(places(arg(2)?)?, rounding(arg(3)?)?))
        }
        FnId::AsMeasurement => {
            let unit = rule_text(arg(2)?)?;
            let step = arg(3)?.as_decimal().ok_or(BAD_RULE)?;
            let rounding = rounding(arg(4)?)?;
            Value::Decimal(match first {
                Value::Decimal(number) => measure::snap(number, step, rounding)?,
                Value::Int(number) => measure::snap(&Decimal::from_i64(*number), step, rounding)?,
                Value::Text(text) => measurement_from_display(
                    text,
                    rule_text(arg(1)?)?,
                    unit,
                    step,
                    rounding,
                    mark(args, 5)?,
                )?,
                _ => return Err(WRONG_KIND),
            })
        }
        FnId::AsDate => match first {
            Value::Date(date) => Value::Date(*date),
            Value::Text(text) => {
                Value::Date(dates::date_from_text(text, &format(arg(1)?, Shape::Date)?)?)
            }
            _ => return Err(WRONG_KIND),
        },
        FnId::AsDatetime => match first {
            Value::Instant(moment) => Value::Instant(*moment),
            Value::Text(text) => Value::Instant(dates::instant_from_text(
                text,
                &format(arg(1)?, Shape::DateTime)?,
                zone(arg(2)?)?,
            )?),
            _ => return Err(WRONG_KIND),
        },
        FnId::AsTime => match first {
            Value::Duration(length) if (0..86_400_000).contains(&length.get()) => {
                Value::Duration(*length)
            }
            Value::Duration(_) => {
                return Err(Fault::range("a time of day lies within one day"));
            }
            Value::Text(text) => {
                millis_of_day(dates::time_from_text(text, &format(arg(1)?, Shape::Time)?)?)
            }
            _ => return Err(WRONG_KIND),
        },
        FnId::AsBoolean => match first {
            Value::Bool(flag) => Value::Bool(*flag),
            Value::Text(text) => Value::Bool(bool_from_display(
                text,
                rule_text(arg(1)?)?,
                rule_text(arg(2)?)?,
            )?),
            _ => return Err(WRONG_KIND),
        },
    })
}

/// Checks a rule written as literal text, when the expression is compiled.
/// Gives the sentence to show when it is not one the library knows.
pub(crate) fn check_rule_text(param: crate::library::Param, literal: &str) -> Result<(), String> {
    use crate::library::Param as P;
    let listed = |names: &[&str]| names.join(", ");
    match param {
        P::Rounding => Rounding::from_str(literal)
            .map(|_| ())
            .map_err(|_| format!("a rounding mode is one of: {}", listed(Rounding::NAMES))),
        P::Mark => DecimalMark::from_name(literal)
            .map(|_| ())
            .ok_or_else(|| "a decimal mark is \".\" or \",\"".to_owned()),
        P::Unit if measure::convertible(literal, literal) => Ok(()),
        P::DisplayUnit if literal == FEET_INCHES || measure::convertible(literal, literal) => {
            Ok(())
        }
        P::Unit | P::DisplayUnit => Err(format!(
            "a unit is one of: {}",
            listed(&measure::unit_names())
        )),
        P::DateFormat => Format::new(literal, Shape::Date)
            .map(|_| ())
            .map_err(str::to_owned),
        P::TimeFormat => Format::new(literal, Shape::Time)
            .map(|_| ())
            .map_err(str::to_owned),
        P::DatetimeFormat => Format::new(literal, Shape::DateTime)
            .map(|_| ())
            .map_err(str::to_owned),
        P::Zone => Offset::new(literal).map(|_| ()).map_err(str::to_owned),
        P::DateSystem => DateSystem::from_name(literal)
            .map(|_| ())
            .ok_or_else(|| "a workbook's date system is \"1900\" or \"1904\"".to_owned()),
        P::Case if literal == CASE_SENSITIVE || literal == CASE_INSENSITIVE => Ok(()),
        P::Case => Err("a case rule is \"sensitive\" or \"insensitive\"".to_owned()),
        P::Whitespace if WHITESPACE_RULES.contains(&literal) => Ok(()),
        P::Whitespace => Err(format!(
            "a whitespace rule is one of: {}",
            listed(&WHITESPACE_RULES)
        )),
        P::Currency if is_currency_code(literal) => Ok(()),
        P::Currency => Err("a currency is a three-letter code in capitals".to_owned()),
        _ => Ok(()),
    }
}
