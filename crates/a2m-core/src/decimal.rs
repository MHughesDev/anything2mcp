//! Exact decimal numbers. No business value is ever a float.
//!
//! The arithmetic comes from an outside crate that this module wraps and no
//! other crate names, so that the text form, the equality rule, and the
//! rounding of a division are this crate's own (decision 0068).

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

use rust_decimal::{Decimal as Inner, RoundingStrategy};

use crate::value::InvalidValue;
use crate::vocabulary::vocabulary;

vocabulary! {
    /// How a decimal is rounded to a number of places. A field type names
    /// one; nothing is ever rounded without one.
    pub enum Rounding: "rounding mode" {
        /// To the nearest; a tie goes away from zero.
        HalfUp => "half_up",
        /// To the nearest; a tie goes to the even digit.
        HalfEven => "half_even",
        /// To the nearest; a tie goes toward zero.
        HalfDown => "half_down",
        /// Away from zero.
        Up => "up",
        /// Toward zero.
        Down => "down",
        /// Toward positive infinity.
        Ceiling => "ceiling",
        /// Toward negative infinity.
        Floor => "floor",
    }
}

impl Rounding {
    const fn strategy(self) -> RoundingStrategy {
        match self {
            Self::HalfUp => RoundingStrategy::MidpointAwayFromZero,
            Self::HalfEven => RoundingStrategy::MidpointNearestEven,
            Self::HalfDown => RoundingStrategy::MidpointTowardZero,
            Self::Up => RoundingStrategy::AwayFromZero,
            Self::Down => RoundingStrategy::ToZero,
            Self::Ceiling => RoundingStrategy::ToPositiveInfinity,
            Self::Floor => RoundingStrategy::ToNegativeInfinity,
        }
    }
}

/// An exact decimal number: up to 28 significant digits, of which up to 28
/// may follow the point.
///
/// Two decimals are equal when they are the same number, whatever zeros were
/// written after the point: `1.5` equals `1.50`. The text form is the number
/// with no zeros after its last digit and no exponent, so equal numbers are
/// written alike and hash alike. How many places a value is shown with is its
/// field type's concern, through [`Decimal::to_places`].
#[derive(Clone, Copy)]
pub struct Decimal(Inner);

impl Decimal {
    /// The most places a decimal can have after its point.
    pub const MAX_PLACES: u32 = 28;

    /// Zero.
    pub const ZERO: Self = Self(Inner::ZERO);

    /// The longest text accepted as a decimal.
    const LONGEST_TEXT: usize = 64;

    /// Reads a decimal from text: an optional minus sign, digits, and
    /// optionally a point and more digits. No exponent, no grouping, no
    /// spaces, no plus sign.
    ///
    /// # Errors
    ///
    /// [`InvalidValue::DecimalMalformed`] when the text has another shape,
    /// and [`InvalidValue::DecimalOutOfRange`] when the number has more
    /// digits than a decimal holds. A number is never rounded to make it fit.
    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        let digits = text.strip_prefix('-').unwrap_or(text);
        let (whole, fraction) = digits.split_once('.').unwrap_or((digits, "0"));
        let all_digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
        if text.len() > Self::LONGEST_TEXT || !all_digits(whole) || !all_digits(fraction) {
            return Err(InvalidValue::DecimalMalformed);
        }
        // Zeros after the last digit carry no value and must not use up digits.
        let significant = if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.')
        } else {
            text
        };
        Inner::from_str_exact(significant)
            .map(Self::tidy)
            .map_err(|_| InvalidValue::DecimalOutOfRange)
    }

    /// A whole number as a decimal.
    #[must_use]
    pub fn from_i64(number: i64) -> Self {
        Self(Inner::from(number))
    }

    /// The number `units` divided by ten to the power `places`: `(1250, 2)`
    /// is `12.50`. This is how a whole number of the smallest unit, such as
    /// cents, becomes a decimal.
    ///
    /// # Errors
    ///
    /// [`InvalidValue::DecimalOutOfRange`] when `places` is more than
    /// [`Decimal::MAX_PLACES`].
    pub fn from_units(units: i64, places: u32) -> Result<Self, InvalidValue> {
        Inner::try_from_i128_with_scale(i128::from(units), places)
            .map(Self::tidy)
            .map_err(|_| InvalidValue::DecimalOutOfRange)
    }

    /// Drops zeros after the last digit and the sign of zero, so that equal
    /// numbers are held alike.
    fn tidy(inner: Inner) -> Self {
        if inner.is_zero() {
            Self(Inner::ZERO)
        } else {
            Self(inner.normalize())
        }
    }

    /// Whether the number is zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    /// Whether the number is below zero.
    #[must_use]
    pub fn is_negative(&self) -> bool {
        !self.0.is_zero() && self.0.is_sign_negative()
    }

    /// How many digits follow the point once zeros after the last digit are
    /// dropped: 2 for `1.25`, 0 for `40`.
    #[must_use]
    pub fn places(&self) -> u32 {
        Self::tidy(self.0).0.scale()
    }

    /// The number as a whole number, when it is one and fits.
    #[must_use]
    pub fn to_i64(&self) -> Option<i64> {
        let tidy = Self::tidy(self.0).0;
        if tidy.scale() == 0 {
            i64::try_from(tidy.mantissa()).ok()
        } else {
            None
        }
    }

    /// The number without its sign.
    #[must_use]
    pub fn abs(&self) -> Self {
        Self::tidy(self.0.abs())
    }

    /// The number with its sign turned.
    #[must_use]
    pub fn negated(&self) -> Self {
        Self::tidy(-self.0)
    }

    /// The exact sum, or `None` when it has more digits than a decimal holds.
    /// A sum is never rounded to make it fit.
    #[must_use]
    pub fn checked_add(&self, other: &Self) -> Option<Self> {
        exact_add(self.0, other.0).map(Self::tidy)
    }

    /// The exact difference, or `None` when it does not fit.
    #[must_use]
    pub fn checked_sub(&self, other: &Self) -> Option<Self> {
        exact_add(self.0, -other.0).map(Self::tidy)
    }

    /// The exact product, or `None` when it does not fit. A product is never
    /// rounded to make it fit.
    #[must_use]
    pub fn checked_mul(&self, other: &Self) -> Option<Self> {
        exact_mul(self.0, other.0).map(Self::tidy)
    }

    /// The quotient, rounded to `places` by `rounding`. There is no division
    /// without both. The rounding is decided on the exact remainder, so the
    /// answer is the true quotient correctly rounded.
    ///
    /// `None` when `divisor` is zero, when `places` is more than
    /// [`Decimal::MAX_PLACES`], or when the working does not fit.
    #[must_use]
    pub fn checked_div(&self, divisor: &Self, places: u32, rounding: Rounding) -> Option<Self> {
        if divisor.is_zero() || places > Self::MAX_PLACES {
            return None;
        }
        if self.is_zero() {
            return Some(Self::ZERO);
        }
        let negative = self.is_negative() != divisor.is_negative();
        let one = Inner::try_from_i128_with_scale(1, places).ok()?;
        let step = if negative { -one } else { one };
        let width = exact_mul(divisor.0.abs(), one)?;
        let left_of = |cut: Inner| exact_add(self.0, -exact_mul(cut, divisor.0)?);
        // Whether the remainder shows the cut went past the true quotient.
        let beyond =
            |left: Inner| !left.is_zero() && (left.is_sign_negative() != self.0.is_sign_negative());

        // A first answer cut toward zero, then corrected against the exact
        // remainder, because the outside crate's own quotient is rounded at
        // its last digit.
        let mut cut = self
            .0
            .checked_div(divisor.0)?
            .round_dp_with_strategy(places, RoundingStrategy::ToZero);
        let mut left = left_of(cut)?;
        for _ in 0..2 {
            if beyond(left) {
                cut = exact_add(cut, -step)?;
            } else if left.abs() >= width {
                cut = exact_add(cut, step)?;
            } else {
                break;
            }
            left = left_of(cut)?;
        }
        if beyond(left) || left.abs() >= width {
            return None;
        }

        let inexact = !left.is_zero();
        let against_half = exact_mul(left.abs(), Inner::TWO)?.cmp(&width);
        let cut_at_places = cut.round_dp_with_strategy(places, RoundingStrategy::ToZero);
        let last_digit_odd = cut_at_places.scale() == places && cut_at_places.mantissa() % 2 != 0;
        let away = match rounding {
            Rounding::Down => false,
            Rounding::Up => inexact,
            Rounding::Ceiling => inexact && !negative,
            Rounding::Floor => inexact && negative,
            Rounding::HalfUp => against_half != Ordering::Less,
            Rounding::HalfDown => against_half == Ordering::Greater,
            Rounding::HalfEven => {
                against_half == Ordering::Greater
                    || (against_half == Ordering::Equal && last_digit_odd)
            }
        };
        let answer = if away { exact_add(cut, step)? } else { cut };
        Some(Self::tidy(answer))
    }

    /// The number rounded to `places` by `rounding`. Exact: the number is
    /// held exactly, so the rounding is decided on all of its digits.
    #[must_use]
    pub fn round(&self, places: u32, rounding: Rounding) -> Self {
        Self::tidy(self.0.round_dp_with_strategy(places, rounding.strategy()))
    }

    /// The number written with exactly `places` digits after the point,
    /// rounded by `rounding` first: `1.5` to two places is `1.50`.
    #[must_use]
    pub fn to_places(&self, places: u32, rounding: Rounding) -> String {
        let places = places.min(Self::MAX_PLACES);
        let rounded = self.round(places, rounding);
        let mut text = rounded.to_string();
        let have = rounded.places();
        if places > 0 {
            if have == 0 {
                text.push('.');
            }
            for _ in have..places {
                text.push('0');
            }
        }
        text
    }
}

/// A decimal from a whole number and the count of its digits that follow the
/// point, once zeros after the last digit are dropped. `None` when it has
/// more digits than a decimal holds.
fn from_parts(mut mantissa: i128, mut scale: u32) -> Option<Inner> {
    while scale > 0 && mantissa % 10 == 0 {
        mantissa /= 10;
        scale -= 1;
    }
    Inner::try_from_i128_with_scale(mantissa, scale).ok()
}

/// The exact sum. The outside crate's own addition rounds when the sum does
/// not fit; this one refuses.
fn exact_add(a: Inner, b: Inner) -> Option<Inner> {
    let (a, b) = (a.normalize(), b.normalize());
    let scale = a.scale().max(b.scale());
    let lift = |x: Inner| {
        x.mantissa()
            .checked_mul(10i128.checked_pow(scale - x.scale())?)
    };
    from_parts(lift(a)?.checked_add(lift(b)?)?, scale)
}

/// The exact product. The outside crate's own multiplication rounds when the
/// product does not fit; this one refuses.
fn exact_mul(a: Inner, b: Inner) -> Option<Inner> {
    let (a, b) = (a.normalize(), b.normalize());
    from_parts(
        a.mantissa().checked_mul(b.mantissa())?,
        a.scale().checked_add(b.scale())?,
    )
}

impl PartialEq for Decimal {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for Decimal {}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Decimal {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl Hash for Decimal {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let tidy = Self::tidy(self.0).0;
        tidy.mantissa().hash(state);
        tidy.scale().hash(state);
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", Self::tidy(self.0).0)
    }
}

impl fmt::Debug for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Decimal({self})")
    }
}

impl FromStr for Decimal {
    type Err = InvalidValue;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

impl serde::Serialize for Decimal {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Decimal {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    use super::{Decimal, Rounding};
    use crate::{Failure, FailureCode, InvalidValue};

    fn d(text: &str) -> Decimal {
        Decimal::parse(text).unwrap()
    }

    #[test]
    fn equal_numbers_are_equal_written_alike_and_hash_alike() {
        let hash = |value: &Decimal| {
            let mut hasher = DefaultHasher::new();
            value.hash(&mut hasher);
            hasher.finish()
        };
        for (a, b) in [
            ("1.5", "1.50"),
            ("0", "-0.000"),
            ("40", "40.0"),
            ("-7.10", "-7.1"),
        ] {
            assert_eq!(d(a), d(b), "{a} {b}");
            assert_eq!(d(a).to_string(), d(b).to_string(), "{a} {b}");
            assert_eq!(hash(&d(a)), hash(&d(b)), "{a} {b}");
        }
        assert_eq!(d("1.50").to_string(), "1.5");
        assert_eq!(d("-0.0").to_string(), "0");
        assert_eq!(d("007").to_string(), "7");
    }

    #[test]
    fn comparison_is_exact_where_a_float_would_not_be() {
        // In binary floating point 0.1 + 0.2 is not 0.3.
        assert_eq!(d("0.1").checked_add(&d("0.2")).unwrap(), d("0.3"));
        assert!(d("0.3") > d("0.29999999999999999999999999"));
        assert_ne!(d("32.5625"), d("32.56"));
        assert!(d("-1") < d("0"));
        assert_eq!(
            d("79228162514264337593543950335").to_string(),
            "79228162514264337593543950335"
        );
    }

    #[test]
    fn text_that_is_not_a_plain_decimal_is_refused() {
        for bad in [
            "", "-", ".", "1.", ".5", "+1", "1e3", "1,000", " 1", "1 ", "0x10", "NaN", "1.2.3",
            "--1", "١",
        ] {
            assert_eq!(
                Decimal::parse(bad),
                Err(InvalidValue::DecimalMalformed),
                "{bad}"
            );
        }
        let failure: Failure = Decimal::parse("1e3").unwrap_err().into();
        assert_eq!(failure.code(), &FailureCode::VALUE_OUT_OF_RANGE);
    }

    #[test]
    fn a_number_that_does_not_fit_is_refused_and_never_rounded() {
        for bad in [
            "79228162514264337593543950336",
            "0.00000000000000000000000000001",
            "1.00000000000000000000000000001",
        ] {
            assert_eq!(
                Decimal::parse(bad),
                Err(InvalidValue::DecimalOutOfRange),
                "{bad}"
            );
        }
        assert_eq!(
            d("79228162514264337593543950335").checked_add(&d("1")),
            None
        );
        assert_eq!(
            d("0.0000000000000000001").checked_mul(&d("0.0000000001")),
            None
        );
        assert_eq!(
            d("0.00000000000001").checked_mul(&d("100000")).unwrap(),
            d("0.000000001")
        );
    }

    #[test]
    fn arithmetic_is_exact() {
        assert_eq!(d("1.10").checked_mul(&d("3")).unwrap(), d("3.3"));
        assert_eq!(d("5").checked_sub(&d("7.25")).unwrap(), d("-2.25"));
        assert_eq!(d("2.5").negated(), d("-2.5"));
        assert_eq!(d("-2.5").abs(), d("2.5"));
        assert_eq!(Decimal::from_units(1250, 2).unwrap(), d("12.5"));
        assert_eq!(Decimal::from_i64(-4), d("-4"));
        assert_eq!(d("40.00").to_i64(), Some(40));
        assert_eq!(d("40.5").to_i64(), None);
        assert_eq!(d("1.250").places(), 2);
        assert!(Decimal::from_units(1, 29).is_err());
    }

    #[test]
    fn each_rounding_mode_rounds_as_it_says() {
        // The value, then the result at one place for each mode in the
        // order half_up, half_even, half_down, up, down, ceiling, floor.
        let cases = [
            ("2.25", ["2.3", "2.2", "2.2", "2.3", "2.2", "2.3", "2.2"]),
            ("2.35", ["2.4", "2.4", "2.3", "2.4", "2.3", "2.4", "2.3"]),
            (
                "-2.25",
                ["-2.3", "-2.2", "-2.2", "-2.3", "-2.2", "-2.2", "-2.3"],
            ),
            ("2.21", ["2.2", "2.2", "2.2", "2.3", "2.2", "2.3", "2.2"]),
            ("2.2", ["2.2", "2.2", "2.2", "2.2", "2.2", "2.2", "2.2"]),
        ];
        for (value, expected) in cases {
            for (mode, want) in Rounding::ALL.iter().zip(expected) {
                assert_eq!(d(value).round(1, *mode), d(want), "{value} {mode}");
            }
        }
        assert_eq!(d("32.5625").round(2, Rounding::HalfEven), d("32.56"));
    }

    #[test]
    fn a_division_is_the_true_quotient_correctly_rounded() {
        let cases = [
            ("1", "3", 4, Rounding::HalfUp, "0.3333"),
            ("2", "3", 4, Rounding::HalfUp, "0.6667"),
            ("2", "3", 4, Rounding::Down, "0.6666"),
            ("-2", "3", 4, Rounding::Floor, "-0.6667"),
            ("-2", "3", 4, Rounding::Ceiling, "-0.6666"),
            ("1", "8", 2, Rounding::HalfEven, "0.12"),
            ("3", "8", 2, Rounding::HalfEven, "0.38"),
            ("1", "8", 2, Rounding::HalfDown, "0.12"),
            ("1", "8", 2, Rounding::HalfUp, "0.13"),
            ("1", "-8", 2, Rounding::HalfUp, "-0.13"),
            ("10", "4", 0, Rounding::HalfEven, "2"),
            ("14", "4", 0, Rounding::HalfEven, "4"),
            ("7", "7", 3, Rounding::Up, "1"),
            ("0", "7", 3, Rounding::Up, "0"),
            ("1", "3", 0, Rounding::Up, "1"),
        ];
        for (a, b, places, mode, want) in cases {
            assert_eq!(
                d(a).checked_div(&d(b), places, mode),
                Some(d(want)),
                "{a} / {b} to {places} {mode}"
            );
        }
        // A quotient a hair above a tie: only the exact remainder shows it
        // must round up under half_down.
        let hair = d("0.5000000000000000000000000001");
        assert_eq!(
            hair.checked_div(&d("1"), 0, Rounding::HalfDown),
            Some(d("1"))
        );
        assert_eq!(
            d("1.0000000000000000000000000001").checked_div(&d("2"), 0, Rounding::HalfDown),
            Some(d("1"))
        );
        assert_eq!(d("1").checked_div(&d("0"), 2, Rounding::HalfUp), None);
        assert_eq!(d("1").checked_div(&d("3"), 29, Rounding::HalfUp), None);
    }

    #[test]
    fn a_number_is_written_with_the_places_its_field_type_asks_for() {
        assert_eq!(d("1.5").to_places(2, Rounding::HalfUp), "1.50");
        assert_eq!(d("40").to_places(2, Rounding::HalfUp), "40.00");
        assert_eq!(d("32.5625").to_places(2, Rounding::HalfEven), "32.56");
        assert_eq!(d("2.5").to_places(0, Rounding::HalfUp), "3");
        assert_eq!(d("-0.004").to_places(2, Rounding::HalfUp), "0.00");
    }

    #[test]
    fn a_decimal_is_read_and_written_as_text_never_as_a_number() {
        assert_eq!(serde_json::to_string(&d("12.50")).unwrap(), "\"12.5\"");
        assert_eq!(
            serde_json::from_str::<Decimal>("\"12.50\"").unwrap(),
            d("12.5")
        );
        assert!(serde_json::from_str::<Decimal>("12.5").is_err());
        assert!(serde_json::from_str::<Decimal>("\"1e2\"").is_err());
        assert_eq!(format!("{:?}", d("1.0")), "Decimal(1)");
    }
}
