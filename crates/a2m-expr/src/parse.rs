//! The parse functions, for callers that read a source in Rust rather than
//! through an expression. Each is the same code the host function of the
//! same name runs, so a value is read one way everywhere.

use a2m_core::{Date, Decimal, Failure, FailureClass};

use crate::dates::{self, DateSystem, Format, Shape};
use crate::error::EXPRESSION_INVALID;
use crate::measure;
use crate::numbers::{self, DecimalMark};

/// Reads a displayed decimal: an optional sign, digits that may be grouped
/// in threes, and at most one decimal mark followed by digits.
///
/// # Errors
///
/// `value_unparsable` when the text is not such a number, and
/// `value_out_of_range` when it has more digits than a decimal holds.
pub fn decimal(text: &str, mark: DecimalMark) -> Result<Decimal, Failure> {
    numbers::decimal_from_display(text, mark).map_err(|fault| fault.into_failure("parse_decimal"))
}

/// Reads a displayed whole number.
///
/// # Errors
///
/// `value_unparsable` or `value_out_of_range`.
pub fn integer(text: &str, mark: DecimalMark) -> Result<i64, Failure> {
    numbers::int_from_display(text, mark).map_err(|fault| fault.into_failure("parse_int"))
}

/// Reads displayed money in `currency`, an ISO 4217 code.
///
/// # Errors
///
/// `value_unparsable`, also when the text names another currency, or
/// `value_out_of_range`.
pub fn money(text: &str, currency: &str, mark: DecimalMark) -> Result<Decimal, Failure> {
    numbers::money_from_display(text, currency, mark)
        .map_err(|fault| fault.into_failure("parse_money"))
}

/// Reads a length written in feet and inches, as inches.
///
/// # Errors
///
/// `value_unparsable` when the text is not such a length; a bare number is
/// not one.
pub fn feet_inches(text: &str) -> Result<Decimal, Failure> {
    measure::inches_from_feet_inches(text).map_err(|fault| fault.into_failure("to_inches"))
}

/// Reads a date written in `format`, such as `MM/DD/YYYY`.
///
/// # Errors
///
/// `expression_invalid` when the format is not a date format, and
/// `value_unparsable` when the text is not a date in it.
pub fn date(text: &str, format: &str) -> Result<Date, Failure> {
    let format = Format::new(format, Shape::Date).map_err(|problem| {
        Failure::new(
            FailureClass::Input,
            EXPRESSION_INVALID,
            format!("A date format is not one the library reads: {problem}."),
            "Correct the format in the field type or the mapping.",
        )
    })?;
    dates::date_from_text(text, &format).map_err(|fault| fault.into_failure("parse_date"))
}

/// The date a workbook's serial number stands for.
///
/// # Errors
///
/// `value_out_of_range` when the serial lies before the workbook's first
/// day, is the day that never was, or lies beyond the year 9999.
pub fn excel_serial(serial: &Decimal, system: DateSystem) -> Result<Date, Failure> {
    dates::date_from_serial(serial, system)
        .map_err(|fault| fault.into_failure("excel_serial_to_date"))
}
