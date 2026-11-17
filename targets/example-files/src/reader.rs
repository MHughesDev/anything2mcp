//! The stand's own readers for delimited text and JSON.
//!
//! They exist so that a test can ask what value is stored at a place without
//! going through any parser the product uses. They are small on purpose: they
//! read, they keep every value as the text the file holds, and they refuse what
//! they do not understand.

use std::fmt;

/// Why the stand's reader could not read a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unreadable {
    /// What is wrong.
    pub reason: &'static str,
    /// The byte the reader had reached.
    pub at: usize,
}

impl fmt::Display for Unreadable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.reason, self.at)
    }
}

fn unreadable<T>(reason: &'static str, at: usize) -> Result<T, Unreadable> {
    Err(Unreadable { reason, at })
}

const BYTE_ORDER_MARK: &[u8] = &[0xEF, 0xBB, 0xBF];

/// The text of a file that must be UTF-8, without its byte-order mark.
fn text(bytes: &[u8]) -> Result<(&str, usize), Unreadable> {
    let skipped = if bytes.starts_with(BYTE_ORDER_MARK) {
        BYTE_ORDER_MARK.len()
    } else {
        0
    };
    match std::str::from_utf8(&bytes[skipped..]) {
        Ok(text) => Ok((text, skipped)),
        Err(e) => unreadable("the bytes are not UTF-8", skipped + e.valid_up_to()),
    }
}

/// Reads delimited text into rows of fields, each field as its text.
///
/// A field in quotes may hold the delimiter, a line break, and a quote written
/// twice. A line ends with CRLF, LF, or CR. A byte-order mark is skipped. The
/// header row, if the file has one, is simply the first row.
///
/// # Errors
///
/// Returns an error if the bytes are not UTF-8, a quote is never closed, or
/// something other than a delimiter or a line end follows a closing quote.
pub fn delimited_rows(bytes: &[u8], delimiter: char) -> Result<Vec<Vec<String>>, Unreadable> {
    let (text, skipped) = text(bytes)?;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let ends_field = |c: char| c == delimiter || c == '\n' || c == '\r';
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    // Each turn of the loop starts at the first character of a field.
    let mut i = 0;
    while i < chars.len() {
        let mut field = String::new();
        if chars[i].1 == '"' {
            let opened = chars[i].0;
            i += 1;
            loop {
                match chars.get(i) {
                    None => {
                        return unreadable("a quote is opened and never closed", skipped + opened);
                    }
                    Some((_, '"')) if matches!(chars.get(i + 1), Some((_, '"'))) => {
                        field.push('"');
                        i += 2;
                    }
                    Some((_, '"')) => {
                        i += 1;
                        break;
                    }
                    Some((_, c)) => {
                        field.push(*c);
                        i += 1;
                    }
                }
            }
            if let Some((at, c)) = chars.get(i)
                && !ends_field(*c)
            {
                return unreadable("text follows a closing quote", skipped + at);
            }
        } else {
            while let Some((_, c)) = chars.get(i) {
                if ends_field(*c) {
                    break;
                }
                field.push(*c);
                i += 1;
            }
        }
        row.push(field);
        match chars.get(i) {
            // The file ends without ending its last line.
            None => rows.push(std::mem::take(&mut row)),
            Some((_, c)) if *c == delimiter => {
                i += 1;
                // A delimiter at the very end still opens one more, empty, field.
                if i == chars.len() {
                    row.push(String::new());
                    rows.push(std::mem::take(&mut row));
                }
            }
            Some((_, c)) => {
                i += 1;
                if *c == '\r' && matches!(chars.get(i), Some((_, '\n'))) {
                    i += 1;
                }
                rows.push(std::mem::take(&mut row));
            }
        }
    }
    Ok(rows)
}

/// One step of a path into a JSON file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JsonStep {
    /// The value of this key in an object. A key that occurs twice gives both.
    Key(String),
    /// The element at this position in a list, counted from zero.
    Index(usize),
    /// Every element of a list that is an object whose `key` holds the text `equals`.
    Where {
        /// The key to look at in each element.
        key: String,
        /// The text its value must hold.
        equals: String,
    },
}

impl JsonStep {
    /// A step to the value of a key.
    #[must_use]
    pub fn key(name: &str) -> Self {
        Self::Key(name.to_owned())
    }

    /// A step to the elements whose `key` holds `equals`.
    #[must_use]
    pub fn element_where(key: &str, equals: &str) -> Self {
        Self::Where {
            key: key.to_owned(),
            equals: equals.to_owned(),
        }
    }
}

/// One value of a JSON file, with the part of the file it was read from.
#[derive(Debug)]
enum Json {
    /// A string, with its escapes resolved.
    Text(String),
    /// A number, `true`, `false`, or `null`, exactly as the file spells it.
    Plain(String),
    /// A list, with the text of the whole list.
    List(Vec<Json>, String),
    /// An object, in file order and with every repeat of a key, with its text.
    Object(Vec<(String, Json)>, String),
}

impl Json {
    /// The value as text: a string's content, anything else as the file spells it.
    fn as_text(&self) -> &str {
        match self {
            Self::Text(text) | Self::Plain(text) | Self::List(_, text) | Self::Object(_, text) => {
                text
            }
        }
    }
}

struct JsonReader<'a> {
    text: &'a str,
    bytes: &'a [u8],
    at: usize,
    skipped: usize,
}

impl JsonReader<'_> {
    fn fail<T>(&self, reason: &'static str) -> Result<T, Unreadable> {
        unreadable(reason, self.skipped + self.at)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn expect(&mut self, byte: u8, reason: &'static str) -> Result<(), Unreadable> {
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            self.fail(reason)
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, Unreadable> {
        if depth > 64 {
            return self.fail("the file is nested too deeply for the stand's reader");
        }
        self.skip_space();
        let start = self.at;
        match self.peek() {
            Some(b'"') => Ok(Json::Text(self.string()?)),
            Some(b'{') => {
                self.at += 1;
                let mut members = Vec::new();
                self.skip_space();
                if self.peek() == Some(b'}') {
                    self.at += 1;
                } else {
                    loop {
                        self.skip_space();
                        if self.peek() != Some(b'"') {
                            return self.fail("an object key must be a string");
                        }
                        let key = self.string()?;
                        self.skip_space();
                        self.expect(b':', "a colon must follow an object key")?;
                        members.push((key, self.value(depth + 1)?));
                        self.skip_space();
                        match self.peek() {
                            Some(b',') => self.at += 1,
                            Some(b'}') => {
                                self.at += 1;
                                break;
                            }
                            _ => return self.fail("an object is not closed"),
                        }
                    }
                }
                Ok(Json::Object(members, self.text[start..self.at].to_owned()))
            }
            Some(b'[') => {
                self.at += 1;
                let mut elements = Vec::new();
                self.skip_space();
                if self.peek() == Some(b']') {
                    self.at += 1;
                } else {
                    loop {
                        elements.push(self.value(depth + 1)?);
                        self.skip_space();
                        match self.peek() {
                            Some(b',') => self.at += 1,
                            Some(b']') => {
                                self.at += 1;
                                break;
                            }
                            _ => return self.fail("a list is not closed"),
                        }
                    }
                }
                Ok(Json::List(elements, self.text[start..self.at].to_owned()))
            }
            Some(_) => {
                while self
                    .peek()
                    .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
                {
                    self.at += 1;
                }
                let word = &self.text[start..self.at];
                if matches!(word, "true" | "false" | "null") || is_number(word) {
                    Ok(Json::Plain(word.to_owned()))
                } else {
                    self.at = start;
                    self.fail("this is not a JSON value")
                }
            }
            None => self.fail("the file ends where a value should be"),
        }
    }

    /// Reads a string, the opening quote being the next byte.
    fn string(&mut self) -> Result<String, Unreadable> {
        self.at += 1;
        let mut out = String::new();
        loop {
            let rest = &self.text[self.at..];
            let Some(c) = rest.chars().next() else {
                return self.fail("a string is not closed");
            };
            self.at += c.len_utf8();
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let Some(escape) = self.peek() else {
                        return self.fail("a string is not closed");
                    };
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
                            let first = self.four_hex()?;
                            let code = if (0xD800..0xDC00).contains(&first) {
                                // The first half of a pair; the second must follow.
                                if !self.text[self.at..].starts_with("\\u") {
                                    return self.fail("half of a character pair stands alone");
                                }
                                self.at += 2;
                                let second = self.four_hex()?;
                                if !(0xDC00..0xE000).contains(&second) {
                                    return self.fail("half of a character pair stands alone");
                                }
                                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                            } else {
                                first
                            };
                            match char::from_u32(code) {
                                Some(decoded) => out.push(decoded),
                                None => {
                                    return self.fail("half of a character pair stands alone");
                                }
                            }
                        }
                        _ => return self.fail("an escape the format does not have"),
                    }
                }
                c if u32::from(c) < 0x20 => {
                    return self.fail("a control character inside a string");
                }
                c => out.push(c),
            }
        }
    }

    fn four_hex(&mut self) -> Result<u32, Unreadable> {
        let digits = self.text.get(self.at..self.at + 4);
        match digits.and_then(|d| u32::from_str_radix(d, 16).ok()) {
            Some(code) if digits.is_some_and(|d| d.bytes().all(|b| b.is_ascii_hexdigit())) => {
                self.at += 4;
                Ok(code)
            }
            _ => self.fail("four hexadecimal digits must follow \\u"),
        }
    }
}

/// Whether this text is a number as JSON spells one.
fn is_number(word: &str) -> bool {
    let rest = word.strip_prefix('-').unwrap_or(word);
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    let (mantissa, exponent) = match rest.find(['e', 'E']) {
        Some(at) => (&rest[..at], Some(&rest[at + 1..])),
        None => (rest, None),
    };
    let (whole, fraction) = match mantissa.find('.') {
        Some(at) => (&mantissa[..at], Some(&mantissa[at + 1..])),
        None => (mantissa, None),
    };
    let whole_ok = digits(whole) && (whole == "0" || !whole.starts_with('0'));
    let fraction_ok = fraction.is_none_or(digits);
    let exponent_ok = exponent.is_none_or(|e| digits(e.strip_prefix(['+', '-']).unwrap_or(e)));
    whole_ok && fraction_ok && exponent_ok
}

/// Reads a JSON file and returns, as text, every value the path reaches, in file
/// order. A string is returned as its content with escapes resolved; a number,
/// `true`, `false`, and `null` exactly as the file spells them; a list or an
/// object as the text the file holds for it.
///
/// More than one value comes back when a key occurs twice in an object, or when
/// several elements match a [`JsonStep::Where`]. None comes back when the path
/// leads nowhere.
///
/// # Errors
///
/// Returns an error if the bytes are not UTF-8 or not JSON.
pub fn json_values(bytes: &[u8], path: &[JsonStep]) -> Result<Vec<String>, Unreadable> {
    let (text, skipped) = text(bytes)?;
    let mut reader = JsonReader {
        text,
        bytes: text.as_bytes(),
        at: 0,
        skipped,
    };
    let root = reader.value(0)?;
    reader.skip_space();
    if reader.at != text.len() {
        return reader.fail("text follows the end of the JSON value");
    }

    let mut reached: Vec<&Json> = vec![&root];
    for step in path {
        let mut next: Vec<&Json> = Vec::new();
        for value in reached {
            match (step, value) {
                (JsonStep::Key(name), Json::Object(members, _)) => {
                    next.extend(members.iter().filter(|(k, _)| k == name).map(|(_, v)| v));
                }
                (JsonStep::Index(position), Json::List(elements, _)) => {
                    next.extend(elements.get(*position));
                }
                (JsonStep::Where { key, equals }, Json::List(elements, _)) => {
                    next.extend(elements.iter().filter(|element| {
                        match element {
                            Json::Object(members, _) => members
                                .iter()
                                .any(|(k, v)| k == key && v.as_text() == equals),
                            _ => false,
                        }
                    }));
                }
                _ => {}
            }
        }
        reached = next;
    }
    Ok(reached
        .into_iter()
        .map(|value| value.as_text().to_owned())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(text: &str) -> Vec<Vec<String>> {
        delimited_rows(text.as_bytes(), ',').unwrap()
    }

    #[test]
    fn delimited_text_is_read_field_by_field() {
        assert_eq!(rows("a,b\n1,2\n"), [["a", "b"], ["1", "2"]]);
        assert_eq!(rows("a,b\r\n1,2"), [["a", "b"], ["1", "2"]]);
        assert_eq!(rows("a,\"b,c\",\"d\"\"e\"\n"), [["a", "b,c", "d\"e"]]);
        assert_eq!(rows("a,\"line\nbreak\",c\n"), [["a", "line\nbreak", "c"]]);
        assert_eq!(rows("a,,c\n,,\n"), [["a", "", "c"], ["", "", ""]]);
        assert_eq!(rows("a,\"\"\n\"\",b\n"), [["a", ""], ["", "b"]]);
        assert_eq!(rows("a,b,"), [["a", "b", ""]]);
        assert_eq!(rows(" 7 ,x \n"), [[" 7 ", "x "]]);
        assert!(rows("").is_empty());
        assert_eq!(rows("\n"), [[""]]);
    }

    #[test]
    fn delimited_text_that_is_not_valid_is_refused() {
        assert!(delimited_rows(b"a,\"b\n", ',').is_err());
        assert!(delimited_rows(b"a,\"b\"c\n", ',').is_err());
        assert!(delimited_rows(&[b'a', 0xE9, b'\n'], ',').is_err());
    }

    #[test]
    fn a_byte_order_mark_is_not_part_of_the_first_field() {
        let rows = delimited_rows(&[0xEF, 0xBB, 0xBF, b'a', b';', b'b'], ';').unwrap();
        assert_eq!(rows, [["a", "b"]]);
    }

    fn values(text: &str, path: &[JsonStep]) -> Vec<String> {
        json_values(text.as_bytes(), path).unwrap()
    }

    #[test]
    fn json_values_come_back_as_the_file_spells_them() {
        let text = r#"{"n": 2.50, "e": 1.9e1, "z": -0, "s": "Café \/ 😀", "l": [1, {"k": "a"}], "t":true, "x": null}"#;
        assert_eq!(values(text, &[JsonStep::key("n")]), ["2.50"]);
        assert_eq!(values(text, &[JsonStep::key("e")]), ["1.9e1"]);
        assert_eq!(values(text, &[JsonStep::key("z")]), ["-0"]);
        assert_eq!(values(text, &[JsonStep::key("s")]), ["Café / \u{1F600}"]);
        assert_eq!(values(text, &[JsonStep::key("t")]), ["true"]);
        assert_eq!(values(text, &[JsonStep::key("x")]), ["null"]);
        assert_eq!(values(text, &[JsonStep::key("l")]), [r#"[1, {"k": "a"}]"#]);
        assert_eq!(
            values(
                text,
                &[JsonStep::key("l"), JsonStep::Index(1), JsonStep::key("k")]
            ),
            ["a"]
        );
        assert!(values(text, &[JsonStep::key("missing")]).is_empty());
    }

    #[test]
    fn a_key_written_twice_gives_both_values() {
        assert_eq!(
            values(r#"{"a": 1, "a": 2}"#, &[JsonStep::key("a")]),
            ["1", "2"]
        );
    }

    #[test]
    fn json_that_is_not_valid_is_refused() {
        for text in [
            "",
            "{",
            r#"{"a": 1,}"#,
            r#"{"a": 01}"#,
            r#"{"a": 1} x"#,
            r#"["a"#,
            "[1 2]",
            r#"{"a": tru}"#,
            r#""\ud83d""#,
        ] {
            assert!(json_values(text.as_bytes(), &[]).is_err(), "{text}");
        }
    }
}
