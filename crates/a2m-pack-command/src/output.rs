//! From what a program printed to typed values, by the one form its command
//! declares. Anything the form does not cover is a failure, never a guess.

use a2m_core::{Decimal, Redact, Value, Values};

use crate::json::{self, Json};
use crate::model::{JsonField, JsonKind, Matching, Output};
use crate::pattern;

/// What a command's standard output held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    /// One record.
    Record(Values),
    /// Rows, in the order printed.
    Rows(Vec<Values>),
}

/// Why output does not parse: a sentence that quotes nothing a program
/// printed.
pub type Unparsed = String;

fn lines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    // A last line that ends with a line end leaves one empty piece.
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
}

/// The part of a document the records are in, by the keys declared to
/// reach it. Whatever else the document holds is not read.
fn within<'a>(document: &'a Json, at: &[String]) -> Result<&'a Json, Unparsed> {
    let mut here = document;
    for key in at {
        let Json::Object(found) = here else {
            return Err(
                "the document is not an object where the path to its records goes through a key"
                    .to_owned(),
            );
        };
        here = found
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
            .ok_or_else(|| "the document lacks a key of the path to its records".to_owned())?;
    }
    Ok(here)
}

fn field_value(name: &str, declared: JsonField, found: &Json) -> Result<Value, Unparsed> {
    let wrong = || format!("the field `{name}` is not of its declared kind");
    Ok(match (declared.kind, found) {
        (_, Json::Null) if !declared.required => Value::Null,
        (JsonKind::Text, Json::Text(text)) => Value::Text(text.clone()),
        (JsonKind::Bool, Json::Bool(flag)) => Value::Bool(*flag),
        (JsonKind::Int, Json::Number(number)) => {
            Value::Int(number.parse::<i64>().map_err(|_| wrong())?)
        }
        (JsonKind::Decimal, Json::Number(number)) => {
            Value::Decimal(Decimal::parse(number).map_err(|_| wrong())?)
        }
        _ => return Err(wrong()),
    })
}

fn record(
    fields: &std::collections::BTreeMap<String, JsonField>,
    document: &Json,
) -> Result<Values, Unparsed> {
    let Json::Object(found) = document else {
        return Err("a document is not an object".to_owned());
    };
    if found.iter().any(|(key, _)| !fields.contains_key(key)) {
        return Err("a document holds a field the command does not declare".to_owned());
    }
    let mut values = Values::new();
    for (name, declared) in fields {
        match found.iter().find(|(key, _)| key == name) {
            Some((_, value)) => {
                values.insert(name.clone(), field_value(name, *declared, value)?);
            }
            None if declared.required => {
                return Err(format!("a document lacks the field `{name}`"));
            }
            None => {
                values.insert(name.clone(), Value::Null);
            }
        }
    }
    Ok(values)
}

fn captures(compiled: &regex::Regex, text: &str) -> Option<Values> {
    let found = compiled.captures(text)?;
    let mut values = Values::new();
    for name in compiled.capture_names().flatten() {
        let value = found
            .name(name)
            .map_or(Value::Null, |group| Value::from(group.as_str()));
        values.insert(name, value);
    }
    Some(values)
}

/// Parses standard output by its declared form.
///
/// # Errors
///
/// Why the output is not of the form.
pub fn parse(text: &str, form: &Output) -> Result<Parsed, Unparsed> {
    match form {
        Output::Ignored => Ok(Parsed::Record(Values::new())),
        Output::Json {
            lines: true,
            fields,
            ..
        } => lines(text)
            .into_iter()
            .map(|line| {
                let document = json::parse(line).map_err(|why| format!("a line is {why}"))?;
                record(fields, &document)
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Parsed::Rows),
        Output::Json {
            lines: false,
            fields,
            at,
        } => {
            let document = json::parse(text).map_err(|why| format!("the output is {why}"))?;
            match within(&document, at)? {
                Json::List(items) => items
                    .iter()
                    .map(|item| record(fields, item))
                    .collect::<Result<Vec<_>, _>>()
                    .map(Parsed::Rows),
                found => record(fields, found).map(Parsed::Record),
            }
        }
        Output::Table { header, columns } => {
            let mut rows = lines(text).into_iter();
            if *header {
                let expected = columns.join("\t");
                if rows.next() != Some(expected.as_str()) {
                    return Err("the header line is not the declared one".to_owned());
                }
            }
            rows.map(|line| {
                let cells: Vec<&str> = line.split('\t').collect();
                if cells.len() != columns.len() {
                    return Err("a row does not have the declared number of columns".to_owned());
                }
                Ok(columns
                    .iter()
                    .zip(cells)
                    .map(|(column, cell)| (column.clone(), Value::from(cell)))
                    .collect())
            })
            .collect::<Result<Vec<Values>, _>>()
            .map(Parsed::Rows)
        }
        Output::Pattern { pattern, matching } => {
            let compiled =
                pattern::whole(pattern).map_err(|_| "the pattern does not compile".to_owned())?;
            let unmatched = || "a line does not match the declared pattern".to_owned();
            match matching {
                Matching::Whole => captures(&compiled, text)
                    .map(Parsed::Record)
                    .ok_or_else(|| "the output does not match the declared pattern".to_owned()),
                Matching::OneLine => match lines(text).as_slice() {
                    [line] => captures(&compiled, line)
                        .map(Parsed::Record)
                        .ok_or_else(unmatched),
                    _ => Err("the output is not one line".to_owned()),
                },
                Matching::EveryLine => lines(text)
                    .into_iter()
                    .map(|line| captures(&compiled, line).ok_or_else(unmatched))
                    .collect::<Result<Vec<_>, _>>()
                    .map(Parsed::Rows),
            }
        }
    }
}

/// Reads standard output by its declared form, so that no value holds a
/// secret: the text is redacted whole before it is read, and every value
/// again after. The second pass is for a form that decodes what it reads:
/// a JSON text written with escapes is not the secret until it is decoded.
///
/// # Errors
///
/// Why the output is not of the form.
pub fn read(redact: &dyn Redact, text: &str, form: &Output) -> Result<Parsed, Unparsed> {
    Ok(match parse(&redact.redact(text), form)? {
        Parsed::Record(values) => Parsed::Record(redact.redact_values(&values)),
        Parsed::Rows(rows) => Parsed::Rows(
            rows.iter()
                .map(|values| redact.redact_values(values))
                .collect(),
        ),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use a2m_core::{Decimal, Redact, Value};

    use super::{Parsed, parse, read};

    /// A redactor that knows one made-up secret.
    struct Planted;

    impl Redact for Planted {
        fn redact(&self, text: &str) -> String {
            text.replace("secret-for-tests-0001", a2m_core::REDACTED)
        }
    }

    #[test]
    fn a_secret_that_decoding_puts_together_is_in_no_value() {
        let form = Output::Json {
            lines: false,
            at: Vec::new(),
            fields: BTreeMap::from([(
                "note".to_owned(),
                JsonField {
                    kind: JsonKind::Text,
                    required: true,
                },
            )]),
        };
        // As printed, and with one character written as an escape.
        for printed in [
            r#"{"note":"it is secret-for-tests-0001"}"#,
            r#"{"note":"it is secret-for-tests-0001"}"#,
        ] {
            let Ok(Parsed::Record(values)) = read(&Planted, printed, &form) else {
                panic!("{printed} was not read");
            };
            assert_eq!(
                values.get("note"),
                Some(&Value::from(format!("it is {}", a2m_core::REDACTED))),
                "{printed}"
            );
        }
    }
    use crate::model::{JsonField, JsonKind, Matching, Output};

    fn json(lines: bool) -> Output {
        let field = |kind, required| JsonField { kind, required };
        Output::Json {
            lines,
            at: Vec::new(),
            fields: BTreeMap::from([
                ("sku".to_owned(), field(JsonKind::Text, true)),
                ("quantity".to_owned(), field(JsonKind::Int, true)),
                ("price".to_owned(), field(JsonKind::Decimal, false)),
                ("note".to_owned(), field(JsonKind::Text, false)),
            ]),
        }
    }

    #[test]
    fn json_gives_typed_values_and_refuses_what_is_not_declared() {
        let Parsed::Record(values) = parse(
            "{\"sku\":\"SK-1\",\"quantity\":12,\"price\":12.50,\"note\":null}\n",
            &json(false),
        )
        .unwrap() else {
            panic!("not a record");
        };
        assert_eq!(values.get("quantity"), Some(&Value::Int(12)));
        assert_eq!(
            values.get("price"),
            Some(&Value::Decimal(Decimal::parse("12.50").unwrap()))
        );
        assert_eq!(values.get("note"), Some(&Value::Null));
        // A list of documents, and one document a line, are rows.
        let listed = parse(
            "[{\"sku\":\"A\",\"quantity\":1},{\"sku\":\"B\",\"quantity\":2}]",
            &json(false),
        );
        assert!(matches!(listed, Ok(Parsed::Rows(rows)) if rows.len() == 2));
        let by_line = parse(
            "{\"sku\":\"A\",\"quantity\":1}\n{\"sku\":\"B\",\"quantity\":2}\n",
            &json(true),
        );
        assert!(matches!(by_line, Ok(Parsed::Rows(rows)) if rows.len() == 2));
        assert_eq!(parse("", &json(true)), Ok(Parsed::Rows(Vec::new())));
        for drifted in [
            "{\"sku\":\"SK-1\",\"qty\":12}",
            "{\"sku\":\"SK-1\"}",
            "{\"sku\":\"SK-1\",\"quantity\":\"12\"}",
            "{\"sku\":\"SK-1\",\"quantity\":1.5}",
            "{\"sku\":\"SK-1\",\"quantity\":12,\"extra\":1}",
            "updated",
        ] {
            assert!(parse(drifted, &json(false)).is_err(), "{drifted}");
        }
    }

    #[test]
    fn a_path_reaches_the_records_in_a_document_that_holds_more() {
        let Output::Json { lines, fields, .. } = json(false) else {
            panic!("not json");
        };
        let nested = Output::Json {
            lines,
            fields,
            at: vec!["body".to_owned(), "items".to_owned()],
        };
        let document = r#"{"header":{"count":2,"anything":[1]},"body":{"items":[{"sku":"A","quantity":1},{"sku":"B","quantity":2}]}}"#;
        assert!(matches!(parse(document, &nested), Ok(Parsed::Rows(rows)) if rows.len() == 2));
        // One record at the path is one record.
        let one = r#"{"body":{"items":{"sku":"A","quantity":1}}}"#;
        assert!(matches!(parse(one, &nested), Ok(Parsed::Record(_))));
        // A key the path needs that is missing, or that is not in an object,
        // is a failure; so is a record the fields do not hold.
        for drifted in [
            r#"{"body":{"rows":[]}}"#,
            r#"{"body":[]}"#,
            "[]",
            r#"{"body":{"items":[{"sku":"A","qty":1}]}}"#,
        ] {
            assert!(parse(drifted, &nested).is_err(), "{drifted}");
        }
        // With no path, the same document is not a record of the fields.
        assert!(parse(document, &json(false)).is_err());
    }

    #[test]
    fn a_table_and_a_pattern_give_text_by_name_and_refuse_any_other_line() {
        let table = Output::Table {
            header: true,
            columns: vec!["sku".to_owned(), "name".to_owned()],
        };
        let Parsed::Rows(rows) = parse("sku\tname\nSK-1\tHex bolt M8\r\nSK-2\t\n", &table).unwrap()
        else {
            panic!("not rows");
        };
        assert_eq!(rows[0].get("name"), Some(&Value::from("Hex bolt M8")));
        assert_eq!(rows[1].get("name"), Some(&Value::from("")));
        assert!(parse("sku\tqty\nSK-1\t2\n", &table).is_err());
        assert!(parse("sku\tname\nSK-1\n", &table).is_err());
        assert!(parse("", &table).is_err());

        let line = Output::Pattern {
            pattern: r"^updated (?<sku>\S+) quantity=(?<quantity>\d+)$".to_owned(),
            matching: Matching::OneLine,
        };
        let Parsed::Record(values) = parse("updated SK-1 quantity=12\n", &line).unwrap() else {
            panic!("not a record");
        };
        assert_eq!(values.get("quantity"), Some(&Value::from("12")));
        assert!(parse("SK-1 updated (quantity 12)\n", &line).is_err());
        assert!(parse("updated SK-1 quantity=12\nAre you sure?", &line).is_err());
        let every = Output::Pattern {
            pattern: r"^(?<key>\w+): (?<value>.*)$".to_owned(),
            matching: Matching::EveryLine,
        };
        assert!(matches!(parse("a: 1\nb: 2\n", &every), Ok(Parsed::Rows(rows)) if rows.len() == 2));
        assert!(parse("a: 1\nnoise\n", &every).is_err());
    }
}
