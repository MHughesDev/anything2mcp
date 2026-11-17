//! A small reader of JSON that keeps every number as its text, so that no
//! value of a program's output passes through a float.

/// One JSON value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Json {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number, as the program wrote it.
    Number(String),
    /// Text.
    Text(String),
    /// A list.
    List(Vec<Json>),
    /// An object, in written order. No key appears twice.
    Object(Vec<(String, Json)>),
}

/// The deepest a document may nest.
const DEPTH: usize = 64;

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

type Read<T> = Result<T, &'static str>;

impl Reader<'_> {
    fn space(&mut self) {
        while matches!(self.bytes.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn word(&mut self, word: &str, value: Json) -> Read<Json> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err("a word that is not `true`, `false`, or `null`")
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.at;
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
            self.at += 1;
        }
        self.at - start
    }

    fn number(&mut self) -> Read<Json> {
        let start = self.at;
        if self.bytes.get(self.at) == Some(&b'-') {
            self.at += 1;
        }
        if self.digits() == 0 {
            return Err("a number with no digits");
        }
        if self.bytes.get(self.at) == Some(&b'.') {
            self.at += 1;
            if self.digits() == 0 {
                return Err("a number with no digits after its point");
            }
        }
        if matches!(self.bytes.get(self.at), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.bytes.get(self.at), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if self.digits() == 0 {
                return Err("a number with no digits in its exponent");
            }
        }
        std::str::from_utf8(&self.bytes[start..self.at])
            .map(|text| Json::Number(text.to_owned()))
            .map_err(|_| "a number that is not text")
    }

    fn hex4(&mut self) -> Read<u32> {
        let digits = self
            .bytes
            .get(self.at..self.at + 4)
            .and_then(|four| std::str::from_utf8(four).ok())
            .ok_or("a cut escape")?;
        self.at += 4;
        u32::from_str_radix(digits, 16).map_err(|_| "an escape that is not hexadecimal")
    }

    fn text(&mut self) -> Read<String> {
        self.at += 1;
        let mut out = String::new();
        loop {
            let start = self.at;
            while !matches!(self.bytes.get(self.at), None | Some(b'"' | b'\\' | 0..=0x1f)) {
                self.at += 1;
            }
            out.push_str(
                std::str::from_utf8(&self.bytes[start..self.at])
                    .map_err(|_| "text that is not UTF-8")?,
            );
            match self.bytes.get(self.at) {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    let escape = *self.bytes.get(self.at).ok_or("a cut escape")?;
                    self.at += 1;
                    out.push(match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xd800..0xdc00).contains(&first) {
                                if self.bytes.get(self.at..self.at + 2) != Some(b"\\u") {
                                    return Err("half of a surrogate pair");
                                }
                                self.at += 2;
                                let second = self.hex4()?;
                                if !(0xdc00..0xe000).contains(&second) {
                                    return Err("half of a surrogate pair");
                                }
                                0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00)
                            } else {
                                first
                            };
                            char::from_u32(code).ok_or("an escape that is no character")?
                        }
                        _ => return Err("an escape JSON does not have"),
                    });
                }
                _ => return Err("text that is cut, or holds a control character"),
            }
        }
    }

    fn value(&mut self, depth: usize) -> Read<Json> {
        if depth > DEPTH {
            return Err("a document nested too deep");
        }
        self.space();
        match self.bytes.get(self.at) {
            Some(b'n') => self.word("null", Json::Null),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'"') => self.text().map(Json::Text),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(b'[') => {
                self.at += 1;
                let mut items = Vec::new();
                self.space();
                if self.bytes.get(self.at) == Some(&b']') {
                    self.at += 1;
                    return Ok(Json::List(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    self.space();
                    match self.bytes.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b']') => {
                            self.at += 1;
                            return Ok(Json::List(items));
                        }
                        _ => return Err("a list that is not closed"),
                    }
                }
            }
            Some(b'{') => {
                self.at += 1;
                let mut fields: Vec<(String, Json)> = Vec::new();
                self.space();
                if self.bytes.get(self.at) == Some(&b'}') {
                    self.at += 1;
                    return Ok(Json::Object(fields));
                }
                loop {
                    self.space();
                    if self.bytes.get(self.at) != Some(&b'"') {
                        return Err("an object key that is not text");
                    }
                    let key = self.text()?;
                    if fields.iter().any(|(known, _)| *known == key) {
                        return Err("an object that names a key twice");
                    }
                    self.space();
                    if self.bytes.get(self.at) != Some(&b':') {
                        return Err("an object key with no value");
                    }
                    self.at += 1;
                    let value = self.value(depth + 1)?;
                    fields.push((key, value));
                    self.space();
                    match self.bytes.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b'}') => {
                            self.at += 1;
                            return Ok(Json::Object(fields));
                        }
                        _ => return Err("an object that is not closed"),
                    }
                }
            }
            _ => Err("something that is not a JSON value"),
        }
    }
}

/// Reads one JSON document: the whole text, with nothing after it.
///
/// # Errors
///
/// What is wrong, in a few words that quote nothing of the text.
pub fn parse(text: &str) -> Result<Json, &'static str> {
    let mut reader = Reader {
        bytes: text.as_bytes(),
        at: 0,
    };
    let value = reader.value(0)?;
    reader.space();
    if reader.at == reader.bytes.len() {
        Ok(value)
    } else {
        Err("text after the document")
    }
}

#[cfg(test)]
mod tests {
    use super::{Json, parse};

    #[test]
    fn a_document_reads_with_its_numbers_kept_as_text() {
        let read = parse(r#" {"sku":"SK-1","n":12.50,"e":"\u00e9\ud83d\ude00\n","l":[1,null,true]} "#)
            .unwrap();
        let Json::Object(fields) = read else {
            panic!("not an object");
        };
        assert_eq!(fields[1], ("n".to_owned(), Json::Number("12.50".to_owned())));
        assert_eq!(fields[2].1, Json::Text("é😀\n".to_owned()));
        assert_eq!(
            fields[3].1,
            Json::List(vec![Json::Number("1".to_owned()), Json::Null, Json::Bool(true)])
        );
    }

    #[test]
    fn what_is_not_one_document_is_refused() {
        for bad in [
            "",
            "{",
            "{\"a\":1,}",
            "{\"a\":1,\"a\":2}",
            "[1 2]",
            "1 2",
            "01x",
            "\"a\nb\"",
            "\"\\ud83d\"",
            "nul",
            "1.",
            &"[".repeat(100),
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }
}
