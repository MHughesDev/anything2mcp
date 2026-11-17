//! JSON as the pack reads and writes it: message bodies, the first wire
//! protocol's lines, and the `found` text of the pack's findings.
//!
//! A number is kept as the text that was written, never as a float, so a
//! decimal in a message body reaches a field type's normalizer exactly as
//! the broker sent it (decision 0105). Reading is bounded in depth, and an
//! object that names a key twice is refused: unknown means stop.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use a2m_core::{Decimal, Value, Values};

/// The deepest a value may nest.
const DEEPEST: usize = 32;

/// One JSON value. An object's keys are in order, so the same value always
/// writes the same text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Json {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number, as the text that was written.
    Number(String),
    /// Text.
    Text(String),
    /// An array.
    List(Vec<Json>),
    /// An object.
    Object(BTreeMap<String, Json>),
}

impl Json {
    /// Text.
    pub(crate) fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    /// A whole number.
    pub(crate) fn number(number: u64) -> Self {
        Self::Number(number.to_string())
    }

    /// An object from pairs.
    pub(crate) fn object<const N: usize>(pairs: [(&str, Json); N]) -> Self {
        Self::Object(
            pairs
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .collect(),
        )
    }

    /// The value under `key` of an object.
    pub(crate) fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Object(map) => map.get(key),
            _ => None,
        }
    }

    /// The text, when the value is text.
    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    /// The number, when the value is a whole number that is not negative.
    pub(crate) fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Number(text) => text.parse().ok(),
            _ => None,
        }
    }

    /// The flag, when the value is one.
    pub(crate) fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(flag) => Some(*flag),
            _ => None,
        }
    }

    /// The items, when the value is an array.
    pub(crate) fn as_list(&self) -> Option<&[Json]> {
        match self {
            Self::List(items) => Some(items),
            _ => None,
        }
    }

    /// The entries, when the value is an object.
    pub(crate) fn as_object(&self) -> Option<&BTreeMap<String, Json>> {
        match self {
            Self::Object(map) => Some(map),
            _ => None,
        }
    }

    /// Whether the value is `null`.
    pub(crate) fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// The name of the value's JSON type, as a schema writes it. A number
    /// with no fraction and no exponent is an `integer`.
    pub(crate) fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "boolean",
            Self::Number(text) if is_whole(text) => "integer",
            Self::Number(_) => "number",
            Self::Text(_) => "string",
            Self::List(_) => "array",
            Self::Object(_) => "object",
        }
    }

    /// The value as one line: keys in order, no spaces.
    pub(crate) fn to_line(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(true) => out.push_str("true"),
            Self::Bool(false) => out.push_str("false"),
            Self::Number(text) => out.push_str(text),
            Self::Text(text) => write_text(text, out),
            Self::List(items) => {
                out.push('[');
                for (at, item) in items.iter().enumerate() {
                    if at > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Self::Object(map) => {
                out.push('{');
                for (at, (key, value)) in map.iter().enumerate() {
                    if at > 0 {
                        out.push(',');
                    }
                    write_text(key, out);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }

    /// Reads one value from text that holds nothing else.
    ///
    /// # Errors
    ///
    /// What is wrong, in a few fixed words. The text itself is never quoted.
    pub(crate) fn parse(text: &str) -> Result<Self, &'static str> {
        let mut reader = Reader {
            bytes: text.as_bytes(),
            at: 0,
        };
        reader.skip_space();
        let value = reader.value(0)?;
        reader.skip_space();
        if reader.at == reader.bytes.len() {
            Ok(value)
        } else {
            Err("text follows the value")
        }
    }

    /// The value as the engine holds values: a whole number as an integer,
    /// any other number as an exact decimal, an object as a record.
    ///
    /// # Errors
    ///
    /// When a number has an exponent or more digits than a decimal holds:
    /// the pack has no rule for it and does not round.
    pub(crate) fn to_value(&self) -> Result<Value, &'static str> {
        Ok(match self {
            Self::Null => Value::Null,
            Self::Bool(flag) => Value::Bool(*flag),
            Self::Number(text) => match text.parse::<i64>() {
                Ok(number) if is_whole(text) => Value::Int(number),
                _ => Value::Decimal(
                    Decimal::parse(text)
                        .map_err(|_| "a number is not a whole number or an exact decimal")?,
                ),
            },
            Self::Text(text) => Value::Text(text.clone()),
            Self::List(items) => Value::List(
                items
                    .iter()
                    .map(Self::to_value)
                    .collect::<Result<Vec<Value>, _>>()?,
            ),
            Self::Object(map) => Value::Record(
                map.iter()
                    .map(|(name, value)| value.to_value().map(|value| (name.clone(), value)))
                    .collect::<Result<Values, _>>()?,
            ),
        })
    }

    /// A value of the engine as JSON. A decimal and a date travel as text,
    /// because a JSON number passes through a float in most readers.
    ///
    /// # Errors
    ///
    /// For a moment or a length of time: the JSON encoding of a message has
    /// no declared form for them.
    pub(crate) fn from_value(value: &Value) -> Result<Self, &'static str> {
        Ok(match value {
            Value::Null => Self::Null,
            Value::Bool(flag) => Self::Bool(*flag),
            Value::Int(number) => Self::Number(number.to_string()),
            Value::Text(text) => Self::Text(text.clone()),
            Value::Decimal(decimal) => Self::Text(decimal.to_string()),
            Value::Date(date) => Self::Text(date.to_string()),
            Value::Instant(_) | Value::Duration(_) => {
                return Err("a moment or a length of time has no form in a JSON message");
            }
            Value::List(items) => Self::List(
                items
                    .iter()
                    .map(Self::from_value)
                    .collect::<Result<Vec<Json>, _>>()?,
            ),
            Value::Record(values) => Self::Object(
                values
                    .iter()
                    .map(|(name, value)| {
                        Self::from_value(value).map(|value| (name.to_owned(), value))
                    })
                    .collect::<Result<BTreeMap<String, Json>, _>>()?,
            ),
        })
    }
}

/// Whether a number's text has no fraction and no exponent.
fn is_whole(text: &str) -> bool {
    !text.contains(['.', 'e', 'E'])
}

fn write_text(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                // Writing to a String cannot fail.
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn word(&mut self, word: &str, value: Json) -> Result<Json, &'static str> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err("a value is not JSON")
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, &'static str> {
        if depth > DEEPEST {
            return Err("a value nests too deep");
        }
        match self.peek() {
            Some(b'n') => self.word("null", Json::Null),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'"') => self.string().map(Json::Text),
            Some(b'[') => {
                self.at += 1;
                let mut items = Vec::new();
                self.skip_space();
                if self.peek() == Some(b']') {
                    self.at += 1;
                    return Ok(Json::List(items));
                }
                loop {
                    self.skip_space();
                    items.push(self.value(depth + 1)?);
                    self.skip_space();
                    match self.peek() {
                        Some(b',') => self.at += 1,
                        Some(b']') => {
                            self.at += 1;
                            return Ok(Json::List(items));
                        }
                        _ => return Err("an array is not closed"),
                    }
                }
            }
            Some(b'{') => {
                self.at += 1;
                let mut map = BTreeMap::new();
                self.skip_space();
                if self.peek() == Some(b'}') {
                    self.at += 1;
                    return Ok(Json::Object(map));
                }
                loop {
                    self.skip_space();
                    let key = self.string()?;
                    self.skip_space();
                    if self.peek() != Some(b':') {
                        return Err("an object key has no value");
                    }
                    self.at += 1;
                    self.skip_space();
                    let value = self.value(depth + 1)?;
                    if map.insert(key, value).is_some() {
                        return Err("an object names a key twice");
                    }
                    self.skip_space();
                    match self.peek() {
                        Some(b',') => self.at += 1,
                        Some(b'}') => {
                            self.at += 1;
                            return Ok(Json::Object(map));
                        }
                        _ => return Err("an object is not closed"),
                    }
                }
            }
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err("a value is not JSON"),
        }
    }

    fn digits(&mut self) -> usize {
        let from = self.at;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        self.at - from
    }

    fn number(&mut self) -> Result<Json, &'static str> {
        let from = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        let leading_zero = self.peek() == Some(b'0');
        let whole = self.digits();
        if whole == 0 || (leading_zero && whole > 1) {
            return Err("a number is malformed");
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if self.digits() == 0 {
                return Err("a number is malformed");
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if self.digits() == 0 {
                return Err("a number is malformed");
            }
        }
        std::str::from_utf8(&self.bytes[from..self.at])
            .map(|text| Json::Number(text.to_owned()))
            .map_err(|_| "a number is malformed")
    }

    fn hex4(&mut self) -> Result<u32, &'static str> {
        let digits = self
            .bytes
            .get(self.at..self.at + 4)
            .and_then(|digits| std::str::from_utf8(digits).ok())
            .ok_or("an escape is cut short")?;
        self.at += 4;
        u32::from_str_radix(digits, 16).map_err(|_| "an escape is malformed")
    }

    fn string(&mut self) -> Result<String, &'static str> {
        if self.peek() != Some(b'"') {
            return Err("a text is expected");
        }
        self.at += 1;
        let mut out = String::new();
        loop {
            let from = self.at;
            while !matches!(self.peek(), Some(b'"' | b'\\') | None)
                && self.peek().is_some_and(|byte| byte >= 0x20)
            {
                self.at += 1;
            }
            // The input is a `str`, and the scan stops only at ASCII bytes,
            // so the piece is whole characters.
            out.push_str(
                std::str::from_utf8(&self.bytes[from..self.at])
                    .map_err(|_| "a text is not UTF-8")?,
            );
            match self.peek() {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    let escape = self.peek().ok_or("an escape is cut short")?;
                    self.at += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&first) {
                                if self.bytes.get(self.at..self.at + 2) != Some(b"\\u") {
                                    return Err("an escape is half of a pair");
                                }
                                self.at += 2;
                                let second = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&second) {
                                    return Err("an escape is half of a pair");
                                }
                                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                            } else {
                                first
                            };
                            out.push(char::from_u32(code).ok_or("an escape is no character")?);
                        }
                        _ => return Err("an escape is unknown"),
                    }
                }
                _ => return Err("a text is not closed"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use a2m_core::{Decimal, Value, Values};

    use super::Json;

    #[test]
    fn a_value_reads_and_writes_back_in_key_order_with_numbers_as_written() {
        let text =
            r#" {"b":[1,2.50,-0,1e3],"a":"x\"\u00e9\ud83d\ude00\n","c":{"t":true,"n":null}} "#;
        let value = Json::parse(text).unwrap();
        assert_eq!(
            value.to_line(),
            "{\"a\":\"x\\\"é😀\\n\",\"b\":[1,2.50,-0,1e3],\"c\":{\"n\":null,\"t\":true}}"
        );
        assert_eq!(Json::parse(&value.to_line()).unwrap(), value);
    }

    #[test]
    fn what_is_not_one_json_value_is_refused() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\":1,\"a\":2}",
            "01",
            "1.",
            "\"\\x\"",
            "\"\\ud800\"",
            "nul",
            "1 2",
            "\"a\nb\"",
        ] {
            assert!(Json::parse(bad).is_err(), "{bad}");
        }
        let deep = "[".repeat(40) + &"]".repeat(40);
        assert!(Json::parse(&deep).is_err());
    }

    #[test]
    fn a_decimal_is_exact_and_a_number_with_an_exponent_has_no_rule() {
        let body = Json::parse(r#"{"count":3,"total":"12.50","rate":0.10}"#).unwrap();
        let Value::Record(values) = body.to_value().unwrap() else {
            panic!("a record");
        };
        assert_eq!(values.get("count"), Some(&Value::Int(3)));
        assert_eq!(values.get("total"), Some(&Value::from("12.50")));
        assert_eq!(
            values.get("rate"),
            Some(&Value::Decimal(Decimal::parse("0.10").unwrap()))
        );
        assert!(Json::parse("1e3").unwrap().to_value().is_err());

        // A decimal leaves as text, so no reader can pass it through a float.
        let sent = Json::from_value(&Value::Record(
            Values::new().with("total", Decimal::parse("12.5").unwrap()),
        ))
        .unwrap();
        assert_eq!(sent.to_line(), r#"{"total":"12.5"}"#);
        assert!(Json::from_value(&Value::Duration(a2m_core::Millis::new(5))).is_err());
    }
}
