//! Stage 1: YAML text to located documents.
//!
//! The parser is `saphyr-parser`, used as a stream of events that each carry
//! a line and a column (decision 0078). The tree, the typing of plain
//! scalars, and the refusal of duplicate keys, anchors, aliases, and tags
//! are this module's own.

use saphyr_parser::{Event, Parser, ScalarStyle, Span, StrInput};

use crate::codes::{self, kernel};
use crate::diag::{Diagnostic, Stage};
use crate::doc::{Doc, Entry, Located, Place, RelPath};

/// How deep a value may be nested.
const DEEPEST: u32 = 64;

/// The text of a file as the compiler reads and digests it: no byte-order
/// mark, and every line ending a line feed, so a file checked out with other
/// line endings is the same file. A carriage return with a line feed after
/// it and one that stands alone are both line endings.
#[must_use]
pub fn normalize_text(text: &str) -> String {
    text.strip_prefix('\u{feff}')
        .unwrap_or(text)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

/// Reads the documents of one YAML file.
///
/// # Errors
///
/// One diagnostic, at the place the file stops being YAML this compiler
/// reads: malformed text, a key given twice, an anchor, an alias, or a tag.
pub fn parse(file: &RelPath, text: &str) -> Result<Vec<Located<Doc>>, Diagnostic> {
    let text = normalize_text(text);
    let mut reader = Reader {
        parser: Parser::new_from_str(&text),
        file,
    };
    let mut docs = Vec::new();
    loop {
        let (event, span) = reader.next()?;
        match event {
            Event::StreamEnd => return Ok(docs),
            Event::StreamStart | Event::DocumentStart(_) | Event::DocumentEnd | Event::Nothing => {}
            other => {
                let doc = reader.value(other, span, 0)?;
                if doc.value != Doc::Null {
                    docs.push(doc);
                }
            }
        }
    }
}

struct Reader<'a> {
    parser: Parser<'a, StrInput<'a>>,
    file: &'a RelPath,
}

fn count(number: usize) -> u32 {
    u32::try_from(number).unwrap_or(u32::MAX)
}

impl<'a> Reader<'a> {
    fn place(&self, span: Span) -> Place {
        Place {
            file: self.file.clone(),
            line: count(span.start.line()).max(1),
            column: count(span.start.col()).saturating_add(1),
        }
    }

    fn malformed(&self, at: Place, what: &str) -> Diagnostic {
        Diagnostic::error(
            kernel(codes::YAML_MALFORMED),
            Stage::Parse,
            &at,
            format!("The file is not well-formed YAML here: {what}."),
            "Correct the YAML at this line; check the indentation, the quotes, and the brackets.",
        )
    }

    fn refused(&self, at: Place, what: &str) -> Diagnostic {
        Diagnostic::error(
            kernel(codes::YAML_FEATURE_REFUSED),
            Stage::Parse,
            &at,
            format!("Target files do not use {what}."),
            "Write the value out in full where it is used.",
        )
    }

    fn next(&mut self) -> Result<(Event<'a>, Span), Diagnostic> {
        match self.parser.next_event() {
            Some(Ok(pair)) => Ok(pair),
            Some(Err(error)) => {
                let at = Place {
                    file: self.file.clone(),
                    line: count(error.marker().line()).max(1),
                    column: count(error.marker().col()).saturating_add(1),
                };
                Err(self.malformed(at, error.info()))
            }
            None => Err(self.malformed(Place::start_of(self.file), "the file ends too soon")),
        }
    }

    fn value(
        &mut self,
        event: Event<'a>,
        span: Span,
        depth: u32,
    ) -> Result<Located<Doc>, Diagnostic> {
        let at = self.place(span);
        if depth > DEEPEST {
            return Err(self.malformed(at, "values are nested too deeply"));
        }
        match event {
            Event::Alias(_) => Err(self.refused(at, "aliases")),
            Event::Scalar(_, _, anchor, _)
            | Event::SequenceStart(anchor, _)
            | Event::MappingStart(anchor, _)
                if anchor != 0 =>
            {
                Err(self.refused(at, "anchors"))
            }
            Event::Scalar(_, _, _, Some(_))
            | Event::SequenceStart(_, Some(_))
            | Event::MappingStart(_, Some(_)) => Err(self.refused(at, "tags")),
            Event::Scalar(text, style, _, None) => {
                let value = if style == ScalarStyle::Plain {
                    plain(&text)
                } else {
                    Doc::Text(text.into_owned())
                };
                Ok(Located::new(value, at))
            }
            Event::SequenceStart(_, None) => {
                let mut items = Vec::new();
                loop {
                    let (event, span) = self.next()?;
                    if event == Event::SequenceEnd {
                        return Ok(Located::new(Doc::List(items), at));
                    }
                    items.push(self.value(event, span, depth + 1)?);
                }
            }
            Event::MappingStart(_, None) => {
                let mut entries: Vec<Entry> = Vec::new();
                loop {
                    let (event, span) = self.next()?;
                    let key_at = self.place(span);
                    let key = match event {
                        Event::MappingEnd => return Ok(Located::new(Doc::Map(entries), at)),
                        Event::Scalar(text, _, 0, None) => text.into_owned(),
                        Event::Scalar(..) => {
                            return Err(self.refused(key_at, "anchors or tags on a key"));
                        }
                        _ => return Err(self.malformed(key_at, "a key is plain text")),
                    };
                    if let Some(first) = entries.iter().find(|entry| entry.key.value == key) {
                        return Err(Diagnostic::error(
                            kernel(codes::DUPLICATE_KEY),
                            Stage::Parse,
                            &key_at,
                            format!(
                                "The key `{key}` is given twice in one map; it is first given on line {}.",
                                first.key.at.line
                            ),
                            format!("Remove one of the two `{key}` entries, or merge them."),
                        ));
                    }
                    let (event, span) = self.next()?;
                    let value = self.value(event, span, depth + 1)?;
                    entries.push(Entry {
                        key: Located::new(key, key_at),
                        value,
                    });
                }
            }
            _ => Err(self.malformed(at, "a value was expected")),
        }
    }
}

/// The value of a plain scalar: nothing, a truth value, a whole number, or
/// text. A number with a decimal point stays text, read exactly where a
/// decimal is wanted; there is no float.
fn plain(text: &str) -> Doc {
    match text {
        "" | "~" | "null" => return Doc::Null,
        "true" => return Doc::Bool(true),
        "false" => return Doc::Bool(false),
        _ => {}
    }
    let digits = text.strip_prefix('-').unwrap_or(text);
    let whole = !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'));
    if whole && let Ok(number) = text.parse::<i64>() {
        return Doc::Int(number);
    }
    Doc::Text(text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{normalize_text, parse};
    use crate::doc::{Doc, RelPath};

    fn file() -> RelPath {
        RelPath::new("operations/order_get.yaml").unwrap()
    }

    #[test]
    fn every_value_keeps_its_line_and_column() {
        let text = "operation: example_order_get\ninputs:\n  order_id: {type: identifier}\nlist:\n  - 7\n  - \"7\"\n  - 1.50\n";
        let docs = parse(&file(), text).unwrap();
        assert_eq!(docs.len(), 1);
        let doc = &docs[0];
        let operation = doc.get("operation").unwrap();
        assert_eq!((operation.at.line, operation.at.column), (1, 12));
        let ty = doc
            .get("inputs")
            .unwrap()
            .get("order_id")
            .unwrap()
            .get("type")
            .unwrap();
        assert_eq!((ty.at.line, ty.at.column), (3, 20));
        assert_eq!(ty.as_text(), Some("identifier"));
        let items = doc.get("list").unwrap().items();
        assert_eq!(items[0].value, Doc::Int(7));
        assert_eq!(items[1].value, Doc::Text("7".to_owned()));
        assert_eq!(items[2].value, Doc::Text("1.50".to_owned()));
        assert_eq!(items[2].at.line, 7);
    }

    #[test]
    fn other_line_endings_and_a_byte_order_mark_change_nothing() {
        let unix = "a: 1\nb:\n  - x\n  - y\ntext: >\n  one\n  two\n";
        let windows = format!("\u{feff}{}", unix.replace('\n', "\r\n"));
        assert_eq!(normalize_text(&windows), unix);
        assert_eq!(
            parse(&file(), unix).unwrap(),
            parse(&file(), &windows).unwrap()
        );
    }

    #[test]
    fn several_documents_in_one_file_are_read_in_order() {
        let docs = parse(&file(), "a: 1\n---\nb: 2\n---\n").unwrap();
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[1].at.line, 3);
    }

    #[test]
    fn malformed_yaml_a_duplicate_key_and_an_alias_are_refused_at_their_place() {
        let broken = parse(&file(), "a: [1, 2\nb: 3\n").unwrap_err();
        assert_eq!(broken.code.as_str(), "kernel.yaml_malformed");
        assert!(broken.at.line >= 2, "{broken}");

        let twice = parse(&file(), "a: 1\nb: 2\na: 3\n").unwrap_err();
        assert_eq!(twice.code.as_str(), "kernel.duplicate_key");
        assert_eq!((twice.at.line, twice.at.column), (3, 1));
        assert!(twice.fix.contains("`a`"));

        let alias = parse(&file(), "a: &x 1\nb: *x\n").unwrap_err();
        assert_eq!(alias.code.as_str(), "kernel.yaml_feature_refused");
        let tag = parse(&file(), "a: !!str 1\n").unwrap_err();
        assert_eq!(tag.code.as_str(), "kernel.yaml_feature_refused");
    }
}
