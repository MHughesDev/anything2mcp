//! A stored number's digits, read straight into an exact decimal. No float
//! is involved: an exponent only moves the point.

use a2m_core::Decimal;

/// Why a number's text could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberFault {
    /// The text is not a number: no digits, or something that is not one.
    NotANumber,
    /// The number has more digits than a decimal holds.
    OutOfRange,
}

/// The longest plain form an exponent may be written out to.
const LONGEST: usize = 64;

/// Reads a number as a workbook or a JSON document stores it: an optional
/// sign, digits with at most one point, and an optional exponent. The result
/// is exactly the number written; nothing is rounded.
///
/// # Errors
///
/// [`NumberFault::NotANumber`] for any other text, including the names some
/// formats give to values that are not numbers, and
/// [`NumberFault::OutOfRange`] when a decimal cannot hold the number.
pub fn exact(text: &str) -> Result<Decimal, NumberFault> {
    let (negative, rest) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let (mantissa, exponent) = match rest.find(['e', 'E']) {
        Some(at) => (&rest[..at], Some(&rest[at + 1..])),
        None => (rest, None),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
    if (whole.is_empty() && fraction.is_empty()) || !digits(whole) || !digits(fraction) {
        return Err(NumberFault::NotANumber);
    }
    let exponent: i64 = match exponent {
        None => 0,
        Some(exponent) => {
            let unsigned = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
            if unsigned.is_empty() || !digits(unsigned) {
                return Err(NumberFault::NotANumber);
            }
            // An exponent too long to read moves the point out of any range.
            exponent.parse().map_err(|_| NumberFault::OutOfRange)?
        }
    };
    // All the digits, and where the point sits among them.
    let mut all: String = format!("{whole}{fraction}");
    let mut point = i64::try_from(whole.len()).map_err(|_| NumberFault::OutOfRange)? + exponent;
    // Zeros that carry no value must not use up digits.
    let leading = all.len() - all.trim_start_matches('0').len();
    all.drain(..leading);
    point -= i64::try_from(leading).map_err(|_| NumberFault::OutOfRange)?;
    let trailing = all.trim_end_matches('0').len();
    all.truncate(trailing);
    if all.is_empty() {
        return Ok(Decimal::ZERO);
    }
    let length = i64::try_from(all.len()).map_err(|_| NumberFault::OutOfRange)?;
    let limit = i64::try_from(LONGEST).map_err(|_| NumberFault::OutOfRange)?;
    if point > limit || point < -limit || length > limit {
        return Err(NumberFault::OutOfRange);
    }
    let plain = if point <= 0 {
        let zeros = usize::try_from(-point).map_err(|_| NumberFault::OutOfRange)?;
        format!("0.{}{all}", "0".repeat(zeros))
    } else if point >= length {
        let zeros = usize::try_from(point - length).map_err(|_| NumberFault::OutOfRange)?;
        format!("{all}{}", "0".repeat(zeros))
    } else {
        let at = usize::try_from(point).map_err(|_| NumberFault::OutOfRange)?;
        format!("{}.{}", &all[..at], &all[at..])
    };
    let signed = if negative { format!("-{plain}") } else { plain };
    if signed.len() > LONGEST {
        return Err(NumberFault::OutOfRange);
    }
    Decimal::parse(&signed).map_err(|_| NumberFault::OutOfRange)
}

/// Whether a number's text is a plain whole number: digits, with an optional
/// minus sign, and nothing else. Only such a number may stand as text.
pub(crate) fn is_plain_whole(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits.len() == 1 || !digits.starts_with('0'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> String {
        exact(text).map_or_else(|fault| format!("{fault:?}"), |number| number.to_string())
    }

    #[test]
    fn a_number_is_read_exactly_with_or_without_an_exponent() {
        assert_eq!(read("2.50"), "2.5");
        assert_eq!(read("0.1"), "0.1");
        assert_eq!(read("0.30000000000000004"), "0.30000000000000004");
        assert_eq!(read("1.9e1"), "19");
        assert_eq!(read("1.0000000000000001E-2"), "0.010000000000000001");
        assert_eq!(read("12E3"), "12000");
        assert_eq!(read("-1.5e-3"), "-0.0015");
        assert_eq!(read("+7"), "7");
        assert_eq!(read(".5"), "0.5");
        assert_eq!(read("5."), "5");
        assert_eq!(read("-0"), "0");
        assert_eq!(read("0e999999"), "0");
        // A float holds neither of these; a decimal holds both.
        assert_eq!(read("9007199254740993"), "9007199254740993");
        assert_eq!(read("1234567890.0123456789"), "1234567890.0123456789");
    }

    #[test]
    fn what_is_not_a_number_or_does_not_fit_is_refused() {
        for text in [
            "", "-", ".", "e5", "1e", "1e+", "NaN", "INF", "1,5", "0x10", " 1", "1.2.3",
        ] {
            assert_eq!(read(text), "NotANumber", "{text:?}");
        }
        assert_eq!(read("1e400"), "OutOfRange");
        assert_eq!(read("1e-400"), "OutOfRange");
        assert_eq!(read("1e99999999999999999999"), "OutOfRange");
        assert_eq!(read("123456789012345678901234567890123"), "OutOfRange");
        assert!(is_plain_whole("1002"));
        assert!(is_plain_whole("-7"));
        assert!(!is_plain_whole("0040"));
        assert!(!is_plain_whole("1.0"));
        assert!(!is_plain_whole("1e3"));
    }
}
