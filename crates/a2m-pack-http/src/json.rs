//! JSON as this pack reads and writes it: a small reader that keeps every
//! number as the text it was written with, so that no value passes through
//! a float, and a writer that escapes text one way.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};

/// The deepest nesting the reader accepts.
const DEEPEST: usize = 64;

/// A JSON value. A number keeps its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    /// A number, as written: checked to be a JSON number.
    Number(String),
    Text(String),
    List(Vec<Json>),
    /// Entries in the order written. No name appears twice.
    Object(Vec<(String, Json)>),
}

/// Why bytes are not JSON this pack reads. It names a place, never content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JsonError {
    pub at: usize,
    pub what: &'static str,
}

/// A value of this pack's JSON, as the shared shape checks it: a number as
/// the text it was written with.
impl a2m_provider::shape::ShapeValue for Json {
    fn view(&self) -> a2m_provider::shape::ValueView<'_, Self> {
        use a2m_provider::shape::ValueView;
        match self {
            Self::Null => ValueView::Null,
            Self::Bool(_) => ValueView::Boolean,
            Self::Number(text) => ValueView::Number(text.into()),
            Self::Text(text) => ValueView::Text(text),
            Self::List(items) => ValueView::List(items),
            Self::Object(entries) => ValueView::Object(
                entries
                    .iter()
                    .map(|(name, value)| (name.as_str(), value))
                    .collect(),
            ),
        }
    }
}

impl Json {
    /// The entry named `name` of an object.
    pub(crate) fn get(&self, name: &str) -> Option<&Self> {
        match self {
            Self::Object(entries) => entries
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub(crate) fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    /// Whether a number is written as a whole number.
    pub(crate) fn is_whole(&self) -> bool {
        matches!(self, Self::Number(text) if !text.contains(['.', 'e', 'E']))
    }

    /// The value as text for a comparison with a declared word: text as it
    /// stands, a number or a truth value as written.
    pub(crate) fn scalar_text(&self) -> Option<String> {
        match self {
            Self::Text(text) => Some(text.clone()),
            Self::Number(text) => Some(text.clone()),
            Self::Bool(flag) => Some(flag.to_string()),
            _ => None,
        }
    }

    /// Reads one JSON document. Trailing whitespace is allowed and nothing
    /// else.
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, JsonError> {
        Self::parse_until(bytes, None)
    }

    /// Reads one JSON document, and stops once `stop` is set: what an
    /// answer is read with, off the engine's async threads, under the
    /// step's deadline. The work is linear in the bytes (decision 0249).
    pub(crate) fn parse_until(bytes: &[u8], stop: Option<&AtomicBool>) -> Result<Self, JsonError> {
        let text = std::str::from_utf8(bytes).map_err(|error| JsonError {
            at: error.valid_up_to(),
            what: "the bytes are not UTF-8",
        })?;
        let mut reader = Reader {
            text,
            bytes: text.as_bytes(),
            at: 0,
            stop,
            values: 0,
        };
        reader.space();
        let value = reader.value(0)?;
        reader.space();
        if reader.at == reader.bytes.len() {
            Ok(value)
        } else {
            Err(reader.error("something follows the document"))
        }
    }

    /// Writes the value with no spaces.
    pub(crate) fn write(&self, out: &mut String) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
            Self::Number(text) => out.push_str(text),
            Self::Text(text) => write_text(text, out),
            Self::List(items) => {
                out.push('[');
                for (position, item) in items.iter().enumerate() {
                    if position > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Self::Object(entries) => {
                out.push('{');
                for (position, (name, value)) in entries.iter().enumerate() {
                    if position > 0 {
                        out.push(',');
                    }
                    write_text(name, out);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }

    /// The value as text.
    pub(crate) fn to_text(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }
}

/// Whether text is a JSON number with no exponent: what a body may carry
/// for an exact decimal.
pub(crate) fn is_plain_number(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, "0"));
    let all_digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    all_digits(whole) && all_digits(fraction) && (whole == "0" || !whole.starts_with('0'))
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
                out.push_str(&format!("\\u{:04x}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Reader<'a> {
    text: &'a str,
    bytes: &'a [u8],
    at: usize,
    stop: Option<&'a AtomicBool>,
    /// The values read so far: the flag is looked at every so many.
    values: u32,
}

/// How many values are read between two looks at the stop flag.
const LOOK_EVERY: u32 = 4_096;

impl Reader<'_> {
    fn error(&self, what: &'static str) -> JsonError {
        JsonError { at: self.at, what }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn word(&mut self, word: &str, value: Json) -> Result<Json, JsonError> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.error("a value was expected"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, JsonError> {
        if depth > DEEPEST {
            return Err(self.error("the document is nested too deeply"));
        }
        self.values = self.values.wrapping_add(1);
        if self.values.is_multiple_of(LOOK_EVERY)
            && self.stop.is_some_and(|stop| stop.load(Ordering::Relaxed))
        {
            return Err(self.error("the time to read it ran out"));
        }
        match self.peek() {
            Some(b'n') => self.word("null", Json::Null),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'"') => self.string().map(Json::Text),
            Some(b'[') => self.list(depth),
            Some(b'{') => self.object(depth),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.error("a value was expected")),
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.at;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        self.at - start
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        let first = self.peek();
        let whole = self.digits();
        if whole == 0 || (whole > 1 && first == Some(b'0')) {
            return Err(self.error("a number is malformed"));
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if self.digits() == 0 {
                return Err(self.error("a number is malformed"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if self.digits() == 0 {
                return Err(self.error("a number is malformed"));
            }
        }
        Ok(Json::Number(self.text[start..self.at].to_owned()))
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let digits = self
            .text
            .get(self.at..self.at + 4)
            .ok_or_else(|| self.error("an escape is cut short"))?;
        let number =
            u32::from_str_radix(digits, 16).map_err(|_| self.error("an escape is malformed"))?;
        self.at += 4;
        Ok(number)
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.at += 1;
        let mut out = String::new();
        loop {
            let start = self.at;
            while !matches!(self.peek(), None | Some(b'"' | b'\\' | 0..=0x1f)) {
                self.at += 1;
            }
            out.push_str(&self.text[start..self.at]);
            match self.peek() {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    let escape = self.peek().ok_or_else(|| self.error("text is cut short"))?;
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
                            let mut code = self.hex4()?;
                            if (0xd800..0xdc00).contains(&code) {
                                if !self.bytes[self.at..].starts_with(b"\\u") {
                                    return Err(self.error("an escape is malformed"));
                                }
                                self.at += 2;
                                let low = self.hex4()?;
                                if !(0xdc00..0xe000).contains(&low) {
                                    return Err(self.error("an escape is malformed"));
                                }
                                code = 0x10000 + ((code - 0xd800) << 10) + (low - 0xdc00);
                            }
                            out.push(
                                char::from_u32(code)
                                    .ok_or_else(|| self.error("an escape is malformed"))?,
                            );
                        }
                        _ => return Err(self.error("an escape is malformed")),
                    }
                }
                Some(_) => return Err(self.error("text holds a control character")),
                None => return Err(self.error("text is cut short")),
            }
        }
    }

    fn list(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.at += 1;
        let mut items = Vec::new();
        self.space();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Json::List(items));
        }
        loop {
            self.space();
            items.push(self.value(depth + 1)?);
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::List(items));
                }
                _ => return Err(self.error("a list is not closed")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.at += 1;
        let mut entries: Vec<(String, Json)> = Vec::new();
        // The names seen, so that a name written twice is found in time
        // that does not grow with the square of the number of names.
        let mut names: BTreeSet<String> = BTreeSet::new();
        self.space();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Json::Object(entries));
        }
        loop {
            self.space();
            if self.peek() != Some(b'"') {
                return Err(self.error("a name was expected"));
            }
            let name = self.string()?;
            if !names.insert(name.clone()) {
                return Err(self.error("an object holds one name twice"));
            }
            self.space();
            if self.peek() != Some(b':') {
                return Err(self.error("a colon was expected"));
            }
            self.at += 1;
            self.space();
            let value = self.value(depth + 1)?;
            entries.push((name, value));
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Object(entries));
                }
                _ => return Err(self.error("an object is not closed")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Json, is_plain_number};

    /// Decision 0999: the shared shape's integer, read from this pack's
    /// JSON, which keeps a number's text. Where the HTTP pack's own copy
    /// took any whole number, a number past 64 bits and `-0` are refused,
    /// as the web pack refused them.
    #[test]
    fn an_integer_past_sixty_four_bits_or_negative_zero_does_not_fit_the_shared_shape() {
        use a2m_provider::shape::{Kind, Shape};
        let integer = Shape::of(Kind::Integer);
        let fits = |text: &str| integer.check(&Json::parse(text.as_bytes()).unwrap());
        assert_eq!(fits("9223372036854775807"), Ok(()));
        assert_eq!(fits("-12"), Ok(()));
        for refused in ["9223372036854775808", "-0", "2.0"] {
            assert!(fits(refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn a_number_keeps_its_text_and_never_becomes_a_float() {
        let json =
            Json::parse(br#"{"a": 150.50, "b": 12, "c": 1e3, "d": 12345678901234567890}"#).unwrap();
        assert_eq!(json.get("a"), Some(&Json::Number("150.50".to_owned())));
        assert!(!json.get("a").unwrap().is_whole());
        assert!(json.get("b").unwrap().is_whole());
        assert!(!json.get("c").unwrap().is_whole());
        assert_eq!(
            json.get("d").unwrap().scalar_text().as_deref(),
            Some("12345678901234567890")
        );
        assert_eq!(
            json.to_text().replace(' ', ""),
            r#"{"a":150.50,"b":12,"c":1e3,"d":12345678901234567890}"#
        );
    }

    #[test]
    fn what_is_not_one_json_document_is_refused_with_a_place_and_no_content() {
        for bad in [
            &b""[..],
            b"{",
            b"[1,]",
            b"{\"a\":1,\"a\":2}",
            b"01",
            b"1.",
            b"\"\x01\"",
            b"nul",
            b"{} x",
            b"\"\\ud800\"",
            b"\xff",
        ] {
            let error = Json::parse(bad).unwrap_err();
            assert!(error.at <= bad.len(), "{error:?}");
        }
        let deep = "[".repeat(100);
        assert!(Json::parse(deep.as_bytes()).is_err());
    }

    /// An object of 200,000 names, under the ceiling of an answer, was once
    /// a comparison of every name with every name before it (decision
    /// 0249). Its work is now linear in its names, and a set flag stops it.
    #[test]
    fn many_names_are_read_in_time_that_grows_with_their_number() {
        let names: Vec<String> = (0..200_000).map(|n| format!("\"a{n}\":0")).collect();
        let body = format!("{{{}}}", names.join(","));
        let began = std::time::Instant::now();
        let json = Json::parse(body.as_bytes()).unwrap();
        assert!(began.elapsed() < std::time::Duration::from_secs(5));
        assert!(matches!(json, Json::Object(entries) if entries.len() == 200_000));
        let twice = format!("{{{},\"a7\":1}}", names.join(","));
        assert!(Json::parse(twice.as_bytes()).is_err());
        let stop = std::sync::atomic::AtomicBool::new(true);
        let list = format!("[{}]", vec!["0"; 10_000].join(","));
        assert!(Json::parse_until(list.as_bytes(), Some(&stop)).is_err());
        assert!(Json::parse(list.as_bytes()).is_ok());
    }

    #[test]
    fn text_is_written_so_that_it_reads_back() {
        let text = "quote \" slash \\ line \n tab \t bell \u{7} é \u{1f600}";
        let written = Json::Object(vec![("k\"".to_owned(), Json::Text(text.to_owned()))]).to_text();
        let back = Json::parse(written.as_bytes()).unwrap();
        assert_eq!(back.get("k\"").and_then(Json::as_text), Some(text));
        assert_eq!(
            Json::parse(br#""\u00e9 \ud83d\ude00 \/""#).unwrap(),
            Json::Text("é \u{1f600} /".to_owned())
        );
    }

    #[test]
    fn only_a_plain_number_may_be_written_as_one() {
        for good in ["0", "-1", "150.5", "0.0625"] {
            assert!(is_plain_number(good), "{good}");
        }
        for bad in ["", "-", "01", "1.", ".5", "1e3", "1,5", "NaN", "1 "] {
            assert!(!is_plain_number(bad), "{bad}");
        }
    }
}
