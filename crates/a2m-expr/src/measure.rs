//! Measurements: a closed table of units, exact conversion, steps, and feet
//! and inches as text.
//!
//! A measurement is a decimal in a unit its field type names. Every factor in
//! the table is exact, and every conversion rounds once, on the true quotient.

use a2m_core::{Decimal, Rounding};

use crate::error::Fault;

/// The name of the display form that writes a length as feet and inches.
pub const FEET_INCHES: &str = "ft_in";

/// The units the library knows: name, kind, and how many of the kind's
/// smallest unit one of them is. Lengths count in millimetres and masses in
/// milligrams; every factor is exact by definition of the unit.
const UNITS: [(&str, u8, &str); 13] = [
    ("mm", 0, "1"),
    ("cm", 0, "10"),
    ("m", 0, "1000"),
    ("km", 0, "1000000"),
    ("in", 0, "25.4"),
    ("ft", 0, "304.8"),
    ("yd", 0, "914.4"),
    ("mi", 0, "1609344"),
    ("mg", 1, "1"),
    ("g", 1, "1000"),
    ("kg", 1, "1000000"),
    ("oz", 1, "28349.523125"),
    ("lb", 1, "453592.37"),
];

/// The names of the units the library converts between.
#[must_use]
pub fn unit_names() -> Vec<&'static str> {
    UNITS.iter().map(|(name, _, _)| *name).collect()
}

fn unit(name: &str) -> Result<(u8, Decimal), Fault> {
    const UNKNOWN: Fault = Fault::unparsable("the unit is not one the library knows");
    let (_, kind, factor) = UNITS
        .iter()
        .find(|(known, _, _)| *known == name)
        .ok_or(UNKNOWN)?;
    let factor = Decimal::parse(factor).map_err(|_| UNKNOWN)?;
    Ok((*kind, factor))
}

/// Whether a value in `from` can be converted to `to`: both are known and
/// measure the same kind of thing.
#[must_use]
pub fn convertible(from: &str, to: &str) -> bool {
    matches!((unit(from), unit(to)), (Ok((a, _)), Ok((b, _))) if a == b)
}

const TOO_LARGE: Fault = Fault::range("the measurement has more digits than a decimal holds");

fn factors(from: &str, to: &str) -> Result<(Decimal, Decimal), Fault> {
    let (from_kind, from_factor) = unit(from)?;
    let (to_kind, to_factor) = unit(to)?;
    if from_kind != to_kind {
        return Err(Fault::unparsable(
            "the two units measure different kinds of thing",
        ));
    }
    Ok((from_factor, to_factor))
}

/// `value` in `from`, as a number of `to`, rounded once to `places`.
pub(crate) fn convert(
    value: &Decimal,
    from: &str,
    to: &str,
    places: u32,
    rounding: Rounding,
) -> Result<Decimal, Fault> {
    if places > Decimal::MAX_PLACES {
        return Err(Fault::range("a decimal has at most 28 places"));
    }
    let (from_factor, to_factor) = factors(from, to)?;
    if from_factor == to_factor {
        return Ok(value.round(places, rounding));
    }
    value
        .checked_mul(&from_factor)
        .and_then(|scaled| scaled.checked_div(&to_factor, places, rounding))
        .ok_or(TOO_LARGE)
}

/// The multiple of `step` nearest to `value`, by `rounding`.
pub(crate) fn snap(value: &Decimal, step: &Decimal, rounding: Rounding) -> Result<Decimal, Fault> {
    if step.is_zero() || step.is_negative() {
        return Err(Fault::range("a step is more than zero"));
    }
    value
        .checked_div(step, 0, rounding)
        .and_then(|count| count.checked_mul(step))
        .ok_or(TOO_LARGE)
}

/// `value` in `from`, as the nearest multiple of `step` in `to`. One rounding,
/// on the exact quotient, so a converted value never lands on the wrong step.
pub(crate) fn convert_and_snap(
    value: &Decimal,
    from: &str,
    to: &str,
    step: &Decimal,
    rounding: Rounding,
) -> Result<Decimal, Fault> {
    if step.is_zero() || step.is_negative() {
        return Err(Fault::range("a step is more than zero"));
    }
    let (from_factor, to_factor) = factors(from, to)?;
    let scaled = value.checked_mul(&from_factor).ok_or(TOO_LARGE)?;
    let width = to_factor.checked_mul(step).ok_or(TOO_LARGE)?;
    scaled
        .checked_div(&width, 0, rounding)
        .and_then(|count| count.checked_mul(step))
        .ok_or(TOO_LARGE)
}

const NOT_FEET_INCHES: Fault = Fault::unparsable("the text is not a length in feet and inches");

fn strip_mark<'t>(text: &'t str, marks: &[&str]) -> Option<&'t str> {
    marks
        .iter()
        .find_map(|mark| text.strip_suffix(mark))
        .map(str::trim_end)
}

fn plain_number(text: &str) -> Result<Decimal, Fault> {
    if text.starts_with('-') {
        return Err(NOT_FEET_INCHES);
    }
    Decimal::parse(text).map_err(|_| NOT_FEET_INCHES)
}

/// `n/d` as an exact decimal. A fraction with no exact decimal form stops.
fn fraction(text: &str) -> Result<Decimal, Fault> {
    let (top, bottom) = text.split_once('/').ok_or(NOT_FEET_INCHES)?;
    let whole = |part: &str| -> Result<Decimal, Fault> {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(NOT_FEET_INCHES);
        }
        Decimal::parse(part).map_err(|_| NOT_FEET_INCHES)
    };
    let (top, bottom) = (whole(top)?, whole(bottom)?);
    let value = top
        .checked_div(&bottom, 12, Rounding::Down)
        .ok_or(NOT_FEET_INCHES)?;
    if value.checked_mul(&bottom) != Some(top) {
        return Err(NOT_FEET_INCHES);
    }
    Ok(value)
}

/// Inches written as `6`, `6.5`, `1/2`, `6 1/2`, or `6-1/2`.
fn inches_part(text: &str) -> Result<Decimal, Fault> {
    if text.contains('/') {
        // One space or one dash stands between the whole inches and the fraction.
        let (whole, part) = match text.rsplit_once([' ', '-']) {
            Some((whole, part)) => (Some(whole), part),
            None => (None, text),
        };
        let part = fraction(part)?;
        return match whole {
            Some(whole) => plain_number(whole)?
                .checked_add(&part)
                .ok_or(NOT_FEET_INCHES),
            None => Ok(part),
        };
    }
    plain_number(text)
}

/// Reads a length written in feet and inches, as inches: `7' 6 1/2"`, `7'-6"`,
/// `7'`, `6 1/2"`, `7 ft 6 in`. The marks are required: a bare number could be
/// either unit, so it stops.
pub(crate) fn inches_from_feet_inches(text: &str) -> Result<Decimal, Fault> {
    const FEET: [&str; 5] = ["'", "\u{2032}", "\u{2019}", "ft.", "ft"];
    const INCHES: [&str; 5] = ["\"", "\u{2033}", "\u{201D}", "in.", "in"];
    let text = crate::text::collapse_space(text);
    let (negative, body) = match text
        .strip_prefix('-')
        .or_else(|| text.strip_prefix('\u{2212}'))
    {
        Some(rest) => (true, rest.trim_start()),
        None => (false, text.as_str()),
    };
    // The feet end at the first feet mark.
    let feet_end = FEET
        .iter()
        .filter_map(|mark| body.find(mark).map(|at| (at, mark.len())))
        .min_by_key(|(at, width)| (*at, std::cmp::Reverse(*width)));
    let (feet, rest) = match feet_end {
        Some((at, width)) => (Some(body[..at].trim_end()), body[at + width..].trim_start()),
        None => (None, body),
    };
    let rest = match feet {
        Some(_) => rest.strip_prefix('-').map_or(rest, str::trim_start),
        None => rest,
    };
    let inches = if rest.is_empty() && feet.is_some() {
        Decimal::ZERO
    } else {
        inches_part(strip_mark(rest, &INCHES).ok_or(NOT_FEET_INCHES)?)?
    };
    let total = match feet {
        Some(feet) => plain_number(feet)?
            .checked_mul(&Decimal::from_i64(12))
            .and_then(|feet| feet.checked_add(&inches))
            .ok_or(TOO_LARGE)?,
        None => inches,
    };
    Ok(if negative { total.negated() } else { total })
}

/// Writes inches as feet and inches to the nearest `step`, such as
/// `7' 6 1/2"`. A fraction of an inch is written over 2, 4, 8, 16, 32, or 64
/// when it is one, and as a decimal otherwise.
pub(crate) fn feet_inches_from_inches(inches: &Decimal, step: &Decimal) -> Result<String, Fault> {
    let snapped = snap(inches, step, Rounding::HalfEven)?;
    let size = snapped.abs();
    let twelve = Decimal::from_i64(12);
    let feet = size
        .checked_div(&twelve, 0, Rounding::Down)
        .ok_or(TOO_LARGE)?;
    let left = feet
        .checked_mul(&twelve)
        .and_then(|whole| size.checked_sub(&whole))
        .ok_or(TOO_LARGE)?;
    let whole = left.round(0, Rounding::Down);
    let part = left.checked_sub(&whole).ok_or(TOO_LARGE)?;
    let inches_text = if part.is_zero() {
        whole.to_string()
    } else {
        let over = [2i64, 4, 8, 16, 32, 64].into_iter().find_map(|bottom| {
            let top = part.checked_mul(&Decimal::from_i64(bottom))?.to_i64()?;
            Some(format!("{top}/{bottom}"))
        });
        match over {
            Some(over) if whole.is_zero() => over,
            Some(over) => format!("{whole} {over}"),
            None => left.to_string(),
        }
    };
    let sign = if snapped.is_negative() { "-" } else { "" };
    Ok(if feet.is_zero() {
        format!("{sign}{inches_text}\"")
    } else {
        format!("{sign}{feet}' {inches_text}\"")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{VALUE_OUT_OF_RANGE, VALUE_UNPARSABLE};

    fn d(text: &str) -> Decimal {
        Decimal::parse(text).unwrap()
    }

    #[test]
    fn feet_and_inches_are_read_as_exact_inches() {
        for (text, want) in [
            ("7' 6 1/2\"", "90.5"),
            ("7'6\"", "90"),
            ("7'-6 1/2\"", "90.5"),
            ("7'", "84"),
            ("6\"", "6"),
            ("6 1/2\"", "6.5"),
            ("6-1/2\"", "6.5"),
            ("1/16\"", "0.0625"),
            ("7' 6.5\"", "90.5"),
            ("7 ft 6 in", "90"),
            ("7ft. 6 3/4in.", "90.75"),
            ("7\u{2032} 6\u{2033}", "90"),
            ("-1' 3\"", "-15"),
            ("0' 0\"", "0"),
            (" 12' 0 15/16\" ", "144.9375"),
        ] {
            assert_eq!(inches_from_feet_inches(text).unwrap(), d(want), "{text}");
        }
    }

    #[test]
    fn a_length_is_never_guessed() {
        for text in [
            "",
            "90",
            "7 6",
            "7' 6",
            "7' x\"",
            "1/3\"",
            "7' 6 1/0\"",
            "seven feet",
            "7'' 6\"",
            "7' -6 -1/2\"",
            "6\" 7'",
            "7' 6\" 3",
        ] {
            let fault = inches_from_feet_inches(text).unwrap_err();
            assert_eq!(fault.code, VALUE_UNPARSABLE, "{text}");
        }
    }

    #[test]
    fn inches_are_written_as_feet_and_inches_and_read_back() {
        let sixteenth = d("0.0625");
        for (inches, want) in [
            ("90.5", "7' 6 1/2\""),
            ("84", "7' 0\""),
            ("6", "6\""),
            ("0.0625", "1/16\""),
            ("144.9375", "12' 15/16\""),
            ("-15", "-1' 3\""),
            ("0", "0\""),
            // 90.53 is nearer to 90 1/2 than to 90 9/16.
            ("90.53", "7' 6 1/2\""),
        ] {
            let text = feet_inches_from_inches(&d(inches), &sixteenth).unwrap();
            assert_eq!(text, want, "{inches}");
            let back = inches_from_feet_inches(&text).unwrap();
            assert_eq!(
                back,
                snap(&d(inches), &sixteenth, Rounding::HalfEven).unwrap()
            );
        }
        // A step with no fraction over a power of two is written as a decimal.
        assert_eq!(
            feet_inches_from_inches(&d("90.3"), &d("0.1")).unwrap(),
            "7' 6.3\""
        );
        assert_eq!(
            feet_inches_from_inches(&d("1"), &d("0")).unwrap_err().code,
            VALUE_OUT_OF_RANGE
        );
    }

    #[test]
    fn units_convert_exactly_and_round_once() {
        let even = Rounding::HalfEven;
        assert_eq!(
            convert(&d("90.5"), "in", "mm", 4, even).unwrap(),
            d("2298.7")
        );
        assert_eq!(
            convert(&d("2298.7"), "mm", "in", 4, even).unwrap(),
            d("90.5")
        );
        assert_eq!(convert(&d("1"), "mm", "in", 6, even).unwrap(), d("0.03937"));
        assert_eq!(convert(&d("1"), "mi", "ft", 0, even).unwrap(), d("5280"));
        assert_eq!(convert(&d("1"), "lb", "oz", 0, even).unwrap(), d("16"));
        assert_eq!(convert(&d("1"), "kg", "lb", 4, even).unwrap(), d("2.2046"));
        assert_eq!(convert(&d("2.345"), "m", "m", 2, even).unwrap(), d("2.34"));
        for (from, to) in [("in", "kg"), ("in", "furlong"), ("", "mm")] {
            assert_eq!(
                convert(&d("1"), from, to, 2, even).unwrap_err().code,
                VALUE_UNPARSABLE
            );
            assert!(!convertible(from, to));
        }
        assert!(convertible("mm", "ft"));
        assert_eq!(
            convert(&d("1"), "mm", "in", 29, even).unwrap_err().code,
            VALUE_OUT_OF_RANGE
        );
    }

    #[test]
    fn a_value_snaps_to_the_nearest_step() {
        let even = Rounding::HalfEven;
        let sixteenth = d("0.0625");
        assert_eq!(snap(&d("32.56"), &sixteenth, even).unwrap(), d("32.5625"));
        assert_eq!(snap(&d("32.5625"), &sixteenth, even).unwrap(), d("32.5625"));
        // A tie between two steps goes to the even multiple.
        assert_eq!(snap(&d("0.03125"), &sixteenth, even).unwrap(), d("0"));
        assert_eq!(snap(&d("0.09375"), &sixteenth, even).unwrap(), d("0.125"));
        assert_eq!(snap(&d("-32.56"), &sixteenth, even).unwrap(), d("-32.5625"));
        // 827.1 mm is 32.5629... in: one rounding, to the nearest sixteenth.
        assert_eq!(
            convert_and_snap(&d("827.1"), "mm", "in", &sixteenth, even).unwrap(),
            d("32.5625")
        );
        assert_eq!(
            snap(&d("1"), &d("-1"), even).unwrap_err().code,
            VALUE_OUT_OF_RANGE
        );
    }
}
