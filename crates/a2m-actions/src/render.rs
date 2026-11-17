//! How a value of a system is shown to a caller.
//!
//! Inside the engine a value keeps its kind: an exact number is written
//! `{"decimal":"12.5"}`, so that it can be read back as the same value. A
//! caller is shown the value itself (spec section 210): an exact number as
//! its exact decimal text, never a float; a calendar day as `2026-11-17`; a
//! moment and a length of time as whole milliseconds; a record as an
//! object; a list as an array.

use a2m_core::{Value, Values};
use serde_json::{Map, Value as Json};

/// The value as a result carries it.
pub(crate) fn plain(value: &Value) -> Json {
    match value {
        Value::Null => Json::Null,
        Value::Bool(flag) => Json::Bool(*flag),
        Value::Int(number) => Json::from(*number),
        Value::Text(text) => Json::String(text.clone()),
        Value::Decimal(number) => Json::String(number.to_string()),
        Value::Instant(moment) => Json::from(moment.as_unix_millis()),
        Value::Date(day) => Json::String(day.to_string()),
        Value::Duration(length) => Json::from(length.get()),
        Value::List(items) => Json::Array(items.iter().map(plain).collect()),
        Value::Record(fields) => plain_values(fields),
    }
}

/// Named values as a result carries them.
pub(crate) fn plain_values(values: &Values) -> Json {
    let map: Map<String, Json> = values
        .iter()
        .map(|(name, value)| (name.to_owned(), plain(value)))
        .collect();
    Json::Object(map)
}

/// The value in one line of text, as a person reads it on a page. Text is
/// shown as it is; everything else as [`plain`] writes it.
pub(crate) fn line(value: &Value) -> String {
    match value {
        Value::Null => "(empty)".to_owned(),
        Value::Text(text) => text.clone(),
        other => match plain(other) {
            Json::String(text) => text,
            json => json.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use a2m_core::{Decimal, Value, Values};
    use serde_json::json;

    use super::{line, plain, plain_values};

    #[test]
    fn an_exact_number_is_shown_as_its_exact_text_at_any_depth() {
        let mut inner = Values::new();
        inner.insert("price", Value::Decimal("0".parse::<Decimal>().unwrap()));
        let mut values = Values::new();
        values.insert("length", Value::Decimal("150.50".parse::<Decimal>().unwrap()));
        values.insert("count", Value::Int(3));
        values.insert("lines", Value::List(vec![Value::Record(inner)]));
        values.insert("day", Value::Date("2026-11-17".parse().unwrap()));
        assert_eq!(
            plain_values(&values),
            json!({
                "length": "150.5",
                "count": 3,
                "lines": [{"price": "0"}],
                "day": "2026-11-17",
            })
        );
        assert_eq!(plain(&Value::Null), json!(null));
        assert_eq!(
            line(&Value::Decimal("12.5".parse::<Decimal>().unwrap())),
            "12.5"
        );
        assert_eq!(line(&Value::Text("New rail".to_owned())), "New rail");
        assert_eq!(line(&Value::Int(3)), "3");
    }
}
