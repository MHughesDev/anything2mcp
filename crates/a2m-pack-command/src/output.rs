//! From what a program printed to typed values, by the one form its command
//! declares. Anything the form does not cover is a failure, never a guess.

use a2m_core::{Decimal, Value, Values};

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
        } => match json::parse(text).map_err(|why| format!("the output is {why}"))? {
            Json::List(items) => items
                .iter()
                .map(|item| record(fields, item))
                .collect::<Result<Vec<_>, _>>()
                .map(Parsed::Rows),
            document => record(fields, &document).map(Parsed::Record),
        },
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use a2m_core::{Decimal, Value};

    use super::{Parsed, parse};
    use crate::model::{JsonField, JsonKind, Matching, Output};

    fn json(lines: bool) -> Output {
        let field = |kind, required| JsonField { kind, required };
        Output::Json {
            lines,
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
