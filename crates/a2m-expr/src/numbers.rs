//! Numbers as a system shows them: reading displayed text into exact values.
//!
//! Display grouping is stripped only when it is well formed. Anything else
//! stops: a number is never guessed.

use a2m_core::Decimal;

use crate::error::Fault;

/// Which character separates the whole part of a displayed number from its
/// fraction. The other one may group digits in threes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecimalMark {
    /// `1,234.50`
    Point,
    /// `1.234,50`
    Comma,
}

impl DecimalMark {
    /// The name used in a compiled call: `"."` or `","`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Point => ".",
            Self::Comma => ",",
        }
    }

    /// The mark with that name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "." => Some(Self::Point),
            "," => Some(Self::Comma),
            _ => None,
        }
    }

    const fn mark(self) -> char {
        match self {
            Self::Point => '.',
            Self::Comma => ',',
        }
    }

    const fn grouping(self) -> char {
        match self {
            Self::Point => ',',
            Self::Comma => '.',
        }
    }
}

/// The digits of a displayed whole part, with grouping removed. Grouping is
/// one separator used throughout, with one to three digits before the first
/// and exactly three after each.
fn whole_digits(text: &str, mark: DecimalMark) -> Result<String, Fault> {
    const BAD: Fault = Fault::unparsable("the text is not a number in the field type's format");
    let is_separator = |ch: char| {
        ch == mark.grouping() || matches!(ch, ' ' | '\u{A0}' | '\u{202F}' | '\'' | '\u{2019}')
    };
    let mut separator = None;
    let mut digits = String::with_capacity(text.len());
    let mut in_group = 0usize;
    let mut groups = 0usize;
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            in_group += 1;
        } else if is_separator(ch) {
            if separator.is_some_and(|known| known != ch) {
                return Err(BAD);
            }
            separator = Some(ch);
            let well_formed = if groups == 0 {
                (1..=3).contains(&in_group)
            } else {
                in_group == 3
            };
            if !well_formed {
                return Err(BAD);
            }
            groups += 1;
            in_group = 0;
        } else {
            return Err(BAD);
        }
    }
    if digits.is_empty() || (groups > 0 && in_group != 3) {
        return Err(BAD);
    }
    Ok(digits)
}

/// Splits a sign from displayed text. A minus may be the ASCII one or U+2212.
fn signed(text: &str) -> (bool, &str) {
    if let Some(rest) = text.strip_prefix('-').or_else(|| text.strip_prefix('\u{2212}')) {
        (true, rest.trim_start())
    } else if let Some(rest) = text.strip_prefix('+') {
        (false, rest.trim_start())
    } else {
        (false, text)
    }
}

/// Reads a displayed decimal: an optional sign, digits that may be grouped in
/// threes, and at most one decimal mark followed by digits.
pub(crate) fn decimal_from_display(text: &str, mark: DecimalMark) -> Result<Decimal, Fault> {
    const BAD: Fault = Fault::unparsable("the text is not a number in the field type's format");
    let (negative, body) = signed(text.trim());
    let (whole, fraction) = match body.split_once(mark.mark()) {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (body, None),
    };
    let mut plain = String::with_capacity(body.len() + 1);
    if negative {
        plain.push('-');
    }
    plain.push_str(&whole_digits(whole, mark)?);
    if let Some(fraction) = fraction {
        if fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(BAD);
        }
        plain.push('.');
        plain.push_str(fraction);
    }
    Decimal::parse(&plain).map_err(|_| Fault::range("the number has more digits than a decimal holds"))
}

/// Reads a displayed whole number: an optional sign and digits that may be
/// grouped in threes.
pub(crate) fn int_from_display(text: &str, mark: DecimalMark) -> Result<i64, Fault> {
    let (negative, body) = signed(text.trim());
    let digits = whole_digits(body, mark)?;
    let plain = if negative {
        format!("-{digits}")
    } else {
        digits
    };
    plain
        .parse()
        .map_err(|_| Fault::range("the whole number is too large"))
}

/// The symbols that may stand beside a displayed amount of money.
fn is_currency_symbol(ch: char) -> bool {
    matches!(ch, '$' | '\u{A2}'..='\u{A5}' | '\u{E3F}' | '\u{20A0}'..='\u{20CF}')
}

/// Reads displayed money: an amount with the field's currency code, or one
/// currency symbol, at either end, and a sign before either. Another
/// currency's code stops.
pub(crate) fn money_from_display(
    text: &str,
    currency: &str,
    mark: DecimalMark,
) -> Result<Decimal, Fault> {
    let (negative, body) = signed(text.trim());
    let body = body.trim();
    let amount = if let Some(rest) = body.strip_prefix(currency) {
        rest
    } else if let Some(rest) = body.strip_suffix(currency) {
        rest
    } else if let Some(rest) = body.strip_prefix(is_currency_symbol) {
        rest
    } else if let Some(rest) = body.strip_suffix(is_currency_symbol) {
        rest
    } else {
        body
    }
    .trim();
    if amount.chars().any(char::is_alphabetic) {
        return Err(Fault::unparsable(
            "the text names a currency other than the field type's",
        ));
    }
    let value = decimal_from_display(amount, mark)?;
    if !negative {
        return Ok(value);
    }
    if amount.starts_with(['-', '+', '\u{2212}']) {
        return Err(Fault::unparsable("the text has two signs"));
    }
    Ok(value.negated())
}

/// The decimal places a currency's amounts have, by its ISO 4217 code: two,
/// unless the code is one of those with none or three.
#[must_use]
pub fn currency_places(code: &str) -> u32 {
    const NONE: [&str; 16] = [
        "BIF", "CLP", "DJF", "GNF", "ISK", "JPY", "KMF", "KRW", "PYG", "RWF", "UGX", "VND", "VUV",
        "XAF", "XOF", "XPF",
    ];
    const THREE: [&str; 7] = ["BHD", "IQD", "JOD", "KWD", "LYD", "OMR", "TND"];
    if NONE.contains(&code) {
        0
    } else if THREE.contains(&code) {
        3
    } else {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{VALUE_OUT_OF_RANGE, VALUE_UNPARSABLE};

    fn d(text: &str) -> Decimal {
        Decimal::parse(text).unwrap()
    }

    #[test]
    fn displayed_decimals_are_read_exactly() {
        let point = DecimalMark::Point;
        let comma = DecimalMark::Comma;
        for (text, mark, want) in [
            ("32.5625", point, "32.5625"),
            ("1,234.50", point, "1234.5"),
            (" 1,234,567 ", point, "1234567"),
            ("1 234,5", comma, "1234.5"),
            ("1.234,50", comma, "1234.5"),
            ("1\u{A0}234\u{A0}567,25", comma, "1234567.25"),
            ("1\u{202F}234", point, "1234"),
            ("1'234.5", point, "1234.5"),
            ("-12.5", point, "-12.5"),
            ("\u{2212}12.5", point, "-12.5"),
            ("+7", point, "7"),
            ("007", point, "7"),
            ("0.10", point, "0.1"),
        ] {
            assert_eq!(decimal_from_display(text, mark).unwrap(), d(want), "{text}");
        }
    }

    #[test]
    fn a_number_is_never_guessed() {
        let point = DecimalMark::Point;
        for text in [
            "", " ", "abc", "1,23", "12,34,567", "1,2345", ",123", "1,,234", "1.", ".5", "1.2.3",
            "1,234 567", "(12.50)", "12.5-", "1e3", "12 %", "1.5x", "--1", "1,234,56",
        ] {
            let fault = decimal_from_display(text, point).unwrap_err();
            assert_eq!(fault.code, VALUE_UNPARSABLE, "{text}");
        }
        // With a comma as the mark, a point can only group.
        assert_eq!(
            decimal_from_display("1.5", DecimalMark::Comma).unwrap_err().code,
            VALUE_UNPARSABLE
        );
        let long = "1".repeat(40);
        assert_eq!(
            decimal_from_display(&long, point).unwrap_err().code,
            VALUE_OUT_OF_RANGE
        );
    }

    #[test]
    fn displayed_whole_numbers_are_read() {
        assert_eq!(int_from_display("1,234", DecimalMark::Point).unwrap(), 1234);
        assert_eq!(int_from_display("-12", DecimalMark::Point).unwrap(), -12);
        assert_eq!(
            int_from_display("1.5", DecimalMark::Point).unwrap_err().code,
            VALUE_UNPARSABLE
        );
        assert_eq!(
            int_from_display("99999999999999999999", DecimalMark::Point)
                .unwrap_err()
                .code,
            VALUE_OUT_OF_RANGE
        );
    }

    #[test]
    fn only_the_fields_own_currency_is_removed() {
        let point = DecimalMark::Point;
        for (text, currency, mark, want) in [
            ("USD 1,234.50", "USD", point, "1234.5"),
            ("1.234,50 EUR", "EUR", DecimalMark::Comma, "1234.5"),
            ("$5.00", "USD", point, "5"),
            ("5,00 \u{20AC}", "EUR", DecimalMark::Comma, "5"),
            ("12.00", "USD", point, "12"),
            ("-12.00", "USD", point, "-12"),
            ("-$5.00", "USD", point, "-5"),
            ("$-5.00", "USD", point, "-5"),
            ("\u{2212}5.00 USD", "USD", point, "-5"),
        ] {
            assert_eq!(money_from_display(text, currency, mark).unwrap(), d(want), "{text}");
        }
        for text in ["EUR 5.00", "-$-5.00", "$", "5.00 dollars", "$$5"] {
            assert_eq!(
                money_from_display(text, "USD", point).unwrap_err().code,
                VALUE_UNPARSABLE,
                "{text}"
            );
        }
    }

    #[test]
    fn a_currency_has_its_own_places() {
        assert_eq!(currency_places("USD"), 2);
        assert_eq!(currency_places("JPY"), 0);
        assert_eq!(currency_places("KWD"), 3);
    }
}
