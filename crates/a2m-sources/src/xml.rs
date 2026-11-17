//! A small reader of the XML a workbook's parts are written in: elements,
//! attributes, text, and the five named characters. It reads no document
//! type and expands no entity a document declares, so a part cannot make it
//! read anything else or grow without bound; a part that tries is refused.

use std::borrow::Cow;

use a2m_core::Failure;

use crate::error::{encoding, malformed};

/// One thing in a part, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Event<'a> {
    /// An element opens. `closed` is true when it closes at once.
    Open {
        /// The element's name without a prefix.
        name: &'a str,
        /// The attributes, as written between the name and the end.
        attributes: Attributes<'a>,
        /// True for an element with no content.
        closed: bool,
    },
    /// An element closes.
    Close {
        /// The element's name without a prefix.
        name: &'a str,
    },
    /// Text between elements.
    Text {
        /// The text, with its named characters read.
        text: Cow<'a, str>,
        /// Where the text lies in the part, as written.
        start: usize,
        /// One past its last byte.
        end: usize,
    },
}

/// The attributes of one element, read when asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Attributes<'a> {
    raw: &'a str,
}

impl<'a> Attributes<'a> {
    /// The attributes written in `raw`, after checking that every one is a
    /// name, an equals sign, and a quoted value whose characters the format
    /// defines, and that none is named twice. An attribute that is written
    /// any other way is never passed over: a reader that did would take the
    /// attributes after it as absent, and read their defaults.
    fn checked(raw: &'a str) -> Result<Self, &'static str> {
        const BROKEN: &str = "a tag's attributes are not each a name and a quoted value";
        let mut names: Vec<&str> = Vec::new();
        let mut rest = raw.trim_start();
        while !rest.is_empty() {
            let equals = rest.find('=').ok_or(BROKEN)?;
            let name = rest.get(..equals).ok_or(BROKEN)?.trim_end();
            if name.is_empty()
                || name
                    .contains(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>'))
            {
                return Err(BROKEN);
            }
            if names.contains(&name) {
                return Err("a tag names one attribute twice");
            }
            names.push(name);
            let after = rest.get(equals + 1..).ok_or(BROKEN)?.trim_start();
            let quote = after
                .chars()
                .next()
                .filter(|c| matches!(c, '"' | '\''))
                .ok_or(BROKEN)?;
            let value_end = after.get(1..).ok_or(BROKEN)?.find(quote).ok_or(BROKEN)?;
            let value = after.get(1..=value_end).ok_or(BROKEN)?;
            if value.contains('<') {
                return Err(BROKEN);
            }
            unescape(value)
                .map_err(|()| "an attribute names a character the format does not define")?;
            let following = after.get(value_end + 2..).ok_or(BROKEN)?;
            if !following.is_empty() && !following.starts_with(|c: char| c.is_whitespace()) {
                return Err(BROKEN);
            }
            rest = following.trim_start();
        }
        Ok(Self { raw })
    }

    /// The value of the attribute with exactly that name, prefix included.
    /// Nothing only when the tag does not carry the attribute: the list was
    /// checked when the tag was read.
    pub(crate) fn get(&self, name: &str) -> Option<Cow<'a, str>> {
        let mut rest = self.raw;
        loop {
            rest = rest.trim_start();
            let equals = rest.find('=')?;
            let found = rest.get(..equals)?.trim_end();
            let after = rest.get(equals + 1..)?.trim_start();
            let quote = after.chars().next().filter(|c| matches!(c, '"' | '\''))?;
            let value_end = after.get(1..)?.find(quote)?;
            let value = after.get(1..=value_end)?;
            if found == name {
                return unescape(value).ok();
            }
            rest = after.get(value_end + 2..)?;
        }
    }

    /// Whether an attribute is written as true: `1` or `true`. One that is
    /// absent, or written `0` or `false`, is not.
    ///
    /// # Errors
    ///
    /// `source_malformed` for any other value: what is neither true nor
    /// false is never read as false.
    pub(crate) fn flag(&self, name: &str) -> Result<bool, Failure> {
        match self.get(name).as_deref() {
            Some("1" | "true") => Ok(true),
            None | Some("0" | "false") => Ok(false),
            Some(_) => Err(malformed(format!(
                "the attribute `{name}` of a part of the package is neither true nor false"
            ))),
        }
    }
}

fn bad(what: &str, at: usize) -> Failure {
    malformed(format!(
        "{what}, at byte {} of a part of the package",
        at + 1
    ))
}

/// Reads the named and numbered characters of a run of text.
fn unescape(text: &str) -> Result<Cow<'_, str>, ()> {
    if !text.contains('&') {
        return Ok(Cow::Borrowed(text));
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(rest.get(..at).ok_or(())?);
        let after = rest.get(at + 1..).ok_or(())?;
        let end = after.find(';').ok_or(())?;
        let name = after.get(..end).ok_or(())?;
        let character = match name {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let code = if let Some(hex) = name.strip_prefix("#x") {
                    u32::from_str_radix(hex, 16).map_err(|_| ())?
                } else if let Some(decimal) = name.strip_prefix('#') {
                    decimal.parse().map_err(|_| ())?
                } else {
                    // A name the document would have to declare: never read.
                    return Err(());
                };
                char::from_u32(code).ok_or(())?
            }
        };
        out.push(character);
        rest = after.get(end + 1..).ok_or(())?;
    }
    out.push_str(rest);
    Ok(Cow::Owned(out))
}

/// The name without its prefix.
fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
}

/// A reader over one part.
#[derive(Debug)]
pub(crate) struct Xml<'a> {
    text: &'a str,
    at: usize,
    /// The elements that are open, outermost first, by the name each was
    /// written with.
    open: Vec<&'a str>,
}

impl<'a> Xml<'a> {
    /// A reader over a part's bytes, which must be UTF-8.
    pub(crate) fn new(bytes: &'a [u8]) -> Result<Self, Failure> {
        if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
            return Err(encoding(
                "a part of the package is UTF-16, and only UTF-8 is read",
            ));
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|_| encoding("a part of the package is not UTF-8"))?;
        let at = if text.starts_with('\u{feff}') { 3 } else { 0 };
        Ok(Self {
            text,
            at,
            open: Vec::new(),
        })
    }

    fn rest(&self) -> &'a str {
        self.text.get(self.at..).unwrap_or("")
    }

    /// Moves past `end`, which must be found, and gives what lay before it.
    fn until(&mut self, end: &str, what: &str) -> Result<&'a str, Failure> {
        let rest = self.rest();
        let found = rest.find(end).ok_or_else(|| bad(what, self.at))?;
        self.at += found + end.len();
        Ok(rest.get(..found).unwrap_or(""))
    }

    /// The next thing in the part, or nothing at its end.
    pub(crate) fn next(&mut self) -> Result<Option<Event<'a>>, Failure> {
        loop {
            let rest = self.rest();
            if rest.is_empty() {
                if !self.open.is_empty() {
                    return Err(bad("an element is not closed", self.at));
                }
                return Ok(None);
            }
            let start = self.at;
            if !rest.starts_with('<') {
                let length = rest.find('<').unwrap_or(rest.len());
                self.at += length;
                let raw = rest.get(..length).unwrap_or("");
                let text = unescape(raw).map_err(|()| {
                    bad("text names a character the format does not define", start)
                })?;
                return Ok(Some(Event::Text {
                    text,
                    start,
                    end: self.at,
                }));
            }
            if rest.starts_with("<!--") {
                self.at += 4;
                self.until("-->", "a comment is not closed")?;
            } else if rest.starts_with("<![CDATA[") {
                self.at += 9;
                let text_start = self.at;
                let text = self.until("]]>", "a literal section is not closed")?;
                return Ok(Some(Event::Text {
                    text: Cow::Borrowed(text),
                    start: text_start,
                    end: text_start + text.len(),
                }));
            } else if rest.starts_with("<?") {
                self.at += 2;
                self.until("?>", "a processing instruction is not closed")?;
            } else if rest.starts_with("<!") {
                return Err(bad(
                    "a part declares a document type, which is never read",
                    start,
                ));
            } else if rest.starts_with("</") {
                self.at += 2;
                let name = self.until(">", "a closing tag is not closed")?.trim_end();
                if self.open.pop() != Some(name) {
                    return Err(bad(
                        "a closing tag does not close the element that is open",
                        start,
                    ));
                }
                return Ok(Some(Event::Close { name: local(name) }));
            } else {
                self.at += 1;
                // A `>` may stand inside a quoted attribute value.
                let rest = self.rest();
                let mut quote: Option<char> = None;
                let mut found = None;
                for (offset, character) in rest.char_indices() {
                    match quote {
                        Some(open) if character == open => quote = None,
                        Some(_) => {}
                        None if character == '"' || character == '\'' => quote = Some(character),
                        None if character == '>' => {
                            found = Some(offset);
                            break;
                        }
                        None => {}
                    }
                }
                let end = found.ok_or_else(|| bad("a tag is not closed", start))?;
                self.at += end + 1;
                let inside = rest.get(..end).unwrap_or("");
                let (inside, closed) = match inside.strip_suffix('/') {
                    Some(inside) => (inside, true),
                    None => (inside, false),
                };
                let name_end = inside
                    .find(|c: char| c.is_ascii_whitespace())
                    .unwrap_or(inside.len());
                let name = inside.get(..name_end).unwrap_or("");
                if name.is_empty() {
                    return Err(bad("a tag has no name", start));
                }
                let attributes = Attributes::checked(inside.get(name_end..).unwrap_or(""))
                    .map_err(|problem| bad(problem, start))?;
                if !closed {
                    self.open.push(name);
                }
                return Ok(Some(Event::Open {
                    name: local(name),
                    attributes,
                    closed,
                }));
            }
        }
    }

    /// Moves past the content of the element just opened, to after its close.
    pub(crate) fn skip(&mut self) -> Result<(), Failure> {
        let mut depth = 1_u32;
        while depth > 0 {
            match self.next()? {
                Some(Event::Open { closed: false, .. }) => depth += 1,
                Some(Event::Close { .. }) => depth -= 1,
                Some(_) => {}
                None => return Err(bad("an element is not closed", self.at)),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SOURCE_MALFORMED;

    #[test]
    fn elements_attributes_and_text_are_read_with_positions() {
        let part = b"<?xml version=\"1.0\"?>\n<x:a k=\"1 &gt; 0\" r:id='rId1'><b/><!-- c --><c>t &amp; &#x41;&#66;</c><![CDATA[<raw>]]></x:a>";
        let mut xml = Xml::new(part).unwrap();
        // The line break after the declaration.
        assert!(matches!(xml.next().unwrap(), Some(Event::Text { .. })));
        let Some(Event::Open {
            name,
            attributes,
            closed,
        }) = xml.next().unwrap()
        else {
            panic!("an element opens");
        };
        assert_eq!((name, closed), ("a", false));
        assert_eq!(attributes.get("k").as_deref(), Some("1 > 0"));
        assert_eq!(attributes.get("r:id").as_deref(), Some("rId1"));
        assert_eq!(attributes.get("id"), None);
        assert!(matches!(
            xml.next().unwrap(),
            Some(Event::Open {
                name: "b",
                closed: true,
                ..
            })
        ));
        assert!(matches!(
            xml.next().unwrap(),
            Some(Event::Open { name: "c", .. })
        ));
        let Some(Event::Text { text, start, end }) = xml.next().unwrap() else {
            panic!("text");
        };
        assert_eq!(text, "t & AB");
        assert_eq!(&part[start..end], b"t &amp; &#x41;&#66;");
        assert!(matches!(
            xml.next().unwrap(),
            Some(Event::Close { name: "c" })
        ));
        let Some(Event::Text { text, .. }) = xml.next().unwrap() else {
            panic!("text");
        };
        assert_eq!(text, "<raw>");
        assert!(matches!(
            xml.next().unwrap(),
            Some(Event::Close { name: "a" })
        ));
        assert_eq!(xml.next().unwrap(), None);
    }

    #[test]
    fn a_document_type_and_a_declared_entity_are_never_read() {
        let mut xml = Xml::new(b"<!DOCTYPE a [<!ENTITY e \"x\">]><a>&e;</a>").unwrap();
        assert_eq!(xml.next().unwrap_err().code(), &SOURCE_MALFORMED);
        let mut xml = Xml::new(b"<a>&e;</a>").unwrap();
        xml.next().unwrap();
        assert_eq!(xml.next().unwrap_err().code(), &SOURCE_MALFORMED);
        let mut xml = Xml::new(b"<a><b></a").unwrap();
        xml.next().unwrap();
        assert_eq!(xml.skip().unwrap_err().code(), &SOURCE_MALFORMED);
    }

    /// Reads a part to its end, or to the first thing that breaks the format.
    fn read_all(part: &[u8]) -> Result<usize, Failure> {
        let mut xml = Xml::new(part)?;
        let mut events = 0;
        while xml.next()?.is_some() {
            events += 1;
        }
        Ok(events)
    }

    #[test]
    fn an_attribute_that_is_not_well_formed_is_refused_and_never_taken_as_absent() {
        // Each of these once read as a tag whose later attributes were
        // absent, so a hidden row was read as shown.
        for part in [
            &b"<row r=\"3\" hidden=1 spans=\"1:2\"/>"[..],
            b"<row hidden=1/>",
            b"<row r=3 hidden=\"1\"/>",
            b"<row r=\"3\" hidden=\"&yes;\"/>",
            b"<row r=\"3\"hidden=\"1\"/>",
            b"<row r=\"3\" hidden/>",
            b"<row r=\"3\" hidden=\"1/>",
            b"<row r=\"3\" r=\"4\"/>",
            b"<row =\"3\"/>",
            b"<row r=\"a<b\"/>",
        ] {
            let failure = read_all(part).unwrap_err();
            assert_eq!(
                failure.code(),
                &SOURCE_MALFORMED,
                "{}",
                String::from_utf8_lossy(part)
            );
        }
        // What the format allows is read as before.
        assert_eq!(read_all(b"<row r = '3'\n hidden=\"1\"  />").unwrap(), 1);
        assert_eq!(read_all(b"<a k=\"x &gt; y\" x:k='\"'></a>").unwrap(), 2);
    }

    #[test]
    fn a_flag_is_true_or_false_and_nothing_else() {
        let flag = |part: &'static [u8]| {
            let mut xml = Xml::new(part).unwrap();
            let Some(Event::Open { attributes, .. }) = xml.next().unwrap() else {
                panic!("an element opens");
            };
            attributes.flag("hidden")
        };
        assert!(flag(b"<row hidden=\"1\"/>").unwrap());
        assert!(flag(b"<row hidden=\"true\"/>").unwrap());
        assert!(!flag(b"<row hidden=\"0\"/>").unwrap());
        assert!(!flag(b"<row hidden=\"false\"/>").unwrap());
        assert!(!flag(b"<row/>").unwrap());
        for part in [&b"<row hidden=\"yes\"/>"[..], b"<row hidden=\"\"/>", b"<row hidden=\"TRUE\"/>"] {
            assert_eq!(flag(part).unwrap_err().code(), &SOURCE_MALFORMED);
        }
    }

    #[test]
    fn a_closing_tag_closes_the_element_that_is_open() {
        for part in [
            &b"<a><b></a></b>"[..],
            b"<a></b>",
            b"</a>",
            b"<a><b/></a></a>",
            b"<x:a></a>",
            b"<a><b>",
            b"<a>text",
        ] {
            let failure = read_all(part).unwrap_err();
            assert_eq!(
                failure.code(),
                &SOURCE_MALFORMED,
                "{}",
                String::from_utf8_lossy(part)
            );
        }
        assert_eq!(read_all(b"<x:a><b></b ><c/></x:a>").unwrap(), 5);
    }
}
