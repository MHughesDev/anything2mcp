//! Values: what a field holds after it is read and before it is compared.
//!
//! A value came from a file, a caller, or a system. It is never a float and
//! never a secret. Its type is described by a [`ValueType`], which the
//! expression checker and the compiled target share.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use crate::canonical::write_string;
use crate::decimal::Decimal;
use crate::digest::Digest;
use crate::failure::{Failure, FailureClass, FailureCode};
use crate::finding::Timestamp;

/// Why something is not a value this crate can hold.
///
/// It maps to the failure class `input`, code `value_out_of_range`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidValue {
    /// The text is not a plain decimal: an optional minus sign, digits, and
    /// optionally a point and more digits.
    DecimalMalformed,
    /// The number has more digits than a decimal holds.
    DecimalOutOfRange,
    /// The text is not a date written as `YYYY-MM-DD`, or the day does not
    /// exist.
    DateMalformed,
    /// The date is outside the years 1 to 9999.
    DateOutOfRange,
    /// A record key has no parts.
    KeyEmpty,
    /// A part of a record key is absent, a list, or a record. A key part is
    /// one plain value.
    KeyPartNotPlain,
}

impl fmt::Display for InvalidValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::DecimalMalformed => {
                "a decimal is an optional minus sign, digits, and optionally a point and more digits; this text is not"
            }
            Self::DecimalOutOfRange => {
                "the number has more digits than a decimal holds, which is 28 significant digits"
            }
            Self::DateMalformed => {
                "a date is written as a four-digit year, a month, and a day, such as 2026-11-17, and must be a day that exists"
            }
            Self::DateOutOfRange => "a date lies in the years 1 to 9999; this one does not",
            Self::KeyEmpty => "a record key has at least one part; this one has none",
            Self::KeyPartNotPlain => {
                "each part of a record key is one plain value; an absent value, a list, or a record cannot be a part"
            }
        })
    }
}

impl std::error::Error for InvalidValue {}

impl From<InvalidValue> for Failure {
    fn from(error: InvalidValue) -> Self {
        let next_step = match error {
            InvalidValue::DecimalMalformed | InvalidValue::DecimalOutOfRange => {
                "Write the number with digits and at most one point, such as 12.50."
            }
            InvalidValue::DateMalformed | InvalidValue::DateOutOfRange => {
                "Write the date as year, month, and day, such as 2026-11-17."
            }
            InvalidValue::KeyEmpty | InvalidValue::KeyPartNotPlain => {
                "Give every identity input of the operation one plain value."
            }
        };
        Self::new(
            FailureClass::Input,
            FailureCode::VALUE_OUT_OF_RANGE,
            error.to_string(),
            next_step,
        )
    }
}

/// A calendar day, with no time and no zone.
///
/// Which day a moment falls on is a zone rule's question, and zone rules
/// belong to field types and the expression library, not to this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

impl Date {
    /// Checks a year, a month, and a day and makes a date of them.
    ///
    /// # Errors
    ///
    /// [`InvalidValue::DateOutOfRange`] when the year is not from 1 to 9999,
    /// and [`InvalidValue::DateMalformed`] when the day does not exist.
    pub fn new(year: i32, month: u8, day: u8) -> Result<Self, InvalidValue> {
        let year = u16::try_from(year)
            .ok()
            .filter(|year| (1..=9999).contains(year))
            .ok_or(InvalidValue::DateOutOfRange)?;
        if month == 0 || month > 12 || day == 0 || day > days_in(year, month) {
            return Err(InvalidValue::DateMalformed);
        }
        Ok(Self { year, month, day })
    }

    /// The year, from 1 to 9999.
    #[must_use]
    pub const fn year(self) -> i32 {
        self.year as i32
    }

    /// The month, from 1 to 12.
    #[must_use]
    pub const fn month(self) -> u8 {
        self.month
    }

    /// The day of the month, from 1.
    #[must_use]
    pub const fn day(self) -> u8 {
        self.day
    }

    /// The number of days from the first day of 1970 to this date; negative
    /// before it.
    #[must_use]
    pub fn to_days(self) -> i32 {
        // Days from civil, counting years from March so that the leap day is
        // the last day of its year.
        let year = i32::from(self.year) - i32::from(self.month <= 2);
        let era = year.div_euclid(400);
        let year_of_era = year.rem_euclid(400);
        let month_from_march = (i32::from(self.month) + 9) % 12;
        let day_of_year = (153 * month_from_march + 2) / 5 + i32::from(self.day) - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }

    /// The date that many days from the first day of 1970.
    ///
    /// # Errors
    ///
    /// [`InvalidValue::DateOutOfRange`] when the date is outside the years 1
    /// to 9999.
    pub fn from_days(days: i32) -> Result<Self, InvalidValue> {
        let shifted = i64::from(days) + 719_468;
        let era = shifted.div_euclid(146_097);
        let day_of_era = shifted.rem_euclid(146_097);
        let year_of_era =
            (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_from_march = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
        let month = if month_from_march < 10 {
            month_from_march + 3
        } else {
            month_from_march - 9
        };
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        let out_of_range = InvalidValue::DateOutOfRange;
        Self::new(
            i32::try_from(year).map_err(|_| out_of_range)?,
            u8::try_from(month).map_err(|_| out_of_range)?,
            u8::try_from(day).map_err(|_| out_of_range)?,
        )
        .map_err(|_| out_of_range)
    }
}

/// How many days a month has.
const fn days_in(year: u16, month: u8) -> u8 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        _ => 31,
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl FromStr for Date {
    type Err = InvalidValue;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bytes = text.as_bytes();
        let shaped = bytes.len() == 10
            && bytes[4] == b'-'
            && bytes[7] == b'-'
            && bytes
                .iter()
                .enumerate()
                .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit());
        if !shaped {
            return Err(InvalidValue::DateMalformed);
        }
        let number = |range: std::ops::Range<usize>| {
            text.get(range)
                .and_then(|part| part.parse::<u16>().ok())
                .ok_or(InvalidValue::DateMalformed)
        };
        let month = u8::try_from(number(5..7)?).map_err(|_| InvalidValue::DateMalformed)?;
        let day = u8::try_from(number(8..10)?).map_err(|_| InvalidValue::DateMalformed)?;
        Self::new(i32::from(number(0..4)?), month, day)
    }
}

impl serde::Serialize for Date {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Date {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <String as serde::Deserialize>::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// A length of time in whole milliseconds. It may be negative: the time from
/// a later moment to an earlier one.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(transparent)]
pub struct Millis(i64);

impl Millis {
    /// A length of time from its milliseconds.
    #[must_use]
    pub const fn new(millis: i64) -> Self {
        Self(millis)
    }

    /// The milliseconds.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// What a field holds once it has been read from a file, a caller, or a
/// system.
///
/// There is no float: a number with a fraction is a [`Decimal`]. There is no
/// secret: a secret is never a field's value. Two values are equal when they
/// have the same kind and the same content; a whole number never equals a
/// decimal, and nothing is converted silently. A choice is held as the text
/// of its stored code.
///
/// Written as JSON, the kinds JSON has stand as they are: nothing, true or
/// false, a whole number, text, a list. Each other kind is an object with
/// one entry that names it: `{"decimal":"12.5"}`, `{"date":"2026-11-17"}`,
/// `{"instant":1795000000000}`, `{"duration":1500}`, `{"record":{}}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum Value {
    /// No value.
    #[default]
    Null,
    /// True or false.
    Bool(bool),
    /// A whole number.
    Int(i64),
    /// Text.
    Text(String),
    /// An exact number.
    Decimal(Decimal),
    /// A moment, with its zone rule already applied.
    Instant(Timestamp),
    /// A calendar day.
    Date(Date),
    /// A length of time.
    Duration(Millis),
    /// Values in order.
    List(Vec<Value>),
    /// Named values.
    Record(Values),
}

impl Value {
    /// The name of the value's kind, for messages: `text`, `decimal`, and so
    /// on. Never the value itself.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Text(_) => "text",
            Self::Decimal(_) => "decimal",
            Self::Instant(_) => "instant",
            Self::Date(_) => "date",
            Self::Duration(_) => "duration",
            Self::List(_) => "list",
            Self::Record(_) => "record",
        }
    }

    /// Whether there is no value.
    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// The text, when the value is text.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    /// True or false, when the value is one of them.
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(flag) => Some(*flag),
            _ => None,
        }
    }

    /// The whole number, when the value is one.
    #[must_use]
    pub const fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(number) => Some(*number),
            _ => None,
        }
    }

    /// The decimal, when the value is one. A whole number is not converted.
    #[must_use]
    pub const fn as_decimal(&self) -> Option<&Decimal> {
        match self {
            Self::Decimal(number) => Some(number),
            _ => None,
        }
    }

    /// The items, when the value is a list.
    #[must_use]
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Self::List(items) => Some(items),
            _ => None,
        }
    }

    /// The named values, when the value is a record.
    #[must_use]
    pub const fn as_record(&self) -> Option<&Values> {
        match self {
            Self::Record(values) => Some(values),
            _ => None,
        }
    }

    /// Whether the value has the type `ty`. An absent value has every type
    /// here; whether a field may be absent is its record type's `required`.
    /// A choice is text.
    #[must_use]
    pub fn has_type(&self, ty: &ValueType) -> bool {
        match (self, ty) {
            (Self::Null, _)
            | (Self::Bool(_), ValueType::Bool)
            | (Self::Int(_), ValueType::Int)
            | (Self::Text(_), ValueType::Text | ValueType::Choice)
            | (Self::Decimal(_), ValueType::Decimal)
            | (Self::Instant(_), ValueType::Instant)
            | (Self::Date(_), ValueType::Date)
            | (Self::Duration(_), ValueType::Duration) => true,
            (Self::List(items), ValueType::List(item)) => {
                items.iter().all(|value| value.has_type(item))
            }
            (Self::Record(values), ValueType::Record(fields)) => {
                values
                    .iter()
                    .all(|(name, _)| fields.iter().any(|field| field.name == name))
                    && fields.iter().all(|field| match values.get(&field.name) {
                        Some(Value::Null) | None => !field.required,
                        Some(value) => value.has_type(&field.ty),
                    })
            }
            _ => false,
        }
    }

    /// Adds the value's canonical bytes to `out`: the same bytes on every
    /// machine, and the same as the compiled target's encoding gives.
    pub fn write_canonical(&self, out: &mut Vec<u8>) {
        match self {
            Self::Null => out.extend_from_slice(b"null"),
            Self::Bool(true) => out.extend_from_slice(b"true"),
            Self::Bool(false) => out.extend_from_slice(b"false"),
            Self::Int(number) => out.extend_from_slice(number.to_string().as_bytes()),
            Self::Text(text) => write_string(out, text),
            Self::Decimal(number) => {
                out.extend_from_slice(b"{\"decimal\":");
                write_string(out, &number.to_string());
                out.push(b'}');
            }
            Self::Instant(moment) => {
                out.extend_from_slice(b"{\"instant\":");
                out.extend_from_slice(moment.as_unix_millis().to_string().as_bytes());
                out.push(b'}');
            }
            Self::Date(date) => {
                out.extend_from_slice(b"{\"date\":");
                write_string(out, &date.to_string());
                out.push(b'}');
            }
            Self::Duration(length) => {
                out.extend_from_slice(b"{\"duration\":");
                out.extend_from_slice(length.get().to_string().as_bytes());
                out.push(b'}');
            }
            Self::List(items) => {
                out.push(b'[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(b',');
                    }
                    item.write_canonical(out);
                }
                out.push(b']');
            }
            Self::Record(values) => {
                out.extend_from_slice(b"{\"record\":");
                values.write_canonical(out);
                out.push(b'}');
            }
        }
    }

    /// The value's canonical bytes.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write_canonical(&mut out);
        out
    }
}

impl From<bool> for Value {
    fn from(flag: bool) -> Self {
        Self::Bool(flag)
    }
}

impl From<i64> for Value {
    fn from(number: i64) -> Self {
        Self::Int(number)
    }
}

impl From<&str> for Value {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<String> for Value {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<Decimal> for Value {
    fn from(number: Decimal) -> Self {
        Self::Decimal(number)
    }
}

impl From<Date> for Value {
    fn from(date: Date) -> Self {
        Self::Date(date)
    }
}

impl From<Timestamp> for Value {
    fn from(moment: Timestamp) -> Self {
        Self::Instant(moment)
    }
}

impl From<Values> for Value {
    fn from(values: Values) -> Self {
        Self::Record(values)
    }
}

impl From<Vec<Value>> for Value {
    fn from(items: Vec<Value>) -> Self {
        Self::List(items)
    }
}

impl serde::Serialize for Value {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            Self::Null => serializer.serialize_unit(),
            Self::Bool(flag) => serializer.serialize_bool(*flag),
            Self::Int(number) => serializer.serialize_i64(*number),
            Self::Text(text) => serializer.serialize_str(text),
            Self::List(items) => serializer.collect_seq(items),
            Self::Decimal(number) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("decimal", number)?;
                map.end()
            }
            Self::Instant(moment) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("instant", moment)?;
                map.end()
            }
            Self::Date(date) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("date", date)?;
                map.end()
            }
            Self::Duration(length) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("duration", length)?;
                map.end()
            }
            Self::Record(values) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("record", values)?;
                map.end()
            }
        }
    }
}

impl<'de> serde::Deserialize<'de> for Value {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Reader;

        impl<'de> serde::de::Visitor<'de> for Reader {
            type Value = Value;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(
                    "nothing, true or false, a whole number, text, a list, or an object with one \
                     entry named decimal, instant, date, duration, or record",
                )
            }

            fn visit_unit<E: serde::de::Error>(self) -> Result<Value, E> {
                Ok(Value::Null)
            }

            fn visit_none<E: serde::de::Error>(self) -> Result<Value, E> {
                Ok(Value::Null)
            }

            fn visit_bool<E: serde::de::Error>(self, flag: bool) -> Result<Value, E> {
                Ok(Value::Bool(flag))
            }

            fn visit_i64<E: serde::de::Error>(self, number: i64) -> Result<Value, E> {
                Ok(Value::Int(number))
            }

            fn visit_u64<E: serde::de::Error>(self, number: u64) -> Result<Value, E> {
                i64::try_from(number)
                    .map(Value::Int)
                    .map_err(|_| E::custom("a whole number beyond 64 bits; write it as a decimal"))
            }

            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Value, E> {
                Err(E::custom(
                    "a float is not a value; write a number with a fraction as a decimal, in text",
                ))
            }

            fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Value, E> {
                Ok(Value::Text(text.to_owned()))
            }

            fn visit_string<E: serde::de::Error>(self, text: String) -> Result<Value, E> {
                Ok(Value::Text(text))
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Value, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                Ok(Value::List(items))
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Value, A::Error> {
                use serde::de::Error;
                let Some(kind) = map.next_key::<String>()? else {
                    return Err(A::Error::custom(
                        "an object with no entry is not a value; an empty record is {\"record\":{}}",
                    ));
                };
                let value = match kind.as_str() {
                    "decimal" => Value::Decimal(map.next_value()?),
                    "instant" => Value::Instant(map.next_value()?),
                    "date" => Value::Date(map.next_value()?),
                    "duration" => Value::Duration(map.next_value()?),
                    "record" => Value::Record(map.next_value()?),
                    _ => {
                        return Err(A::Error::custom(
                            "an object that is a value has one entry, named decimal, instant, date, duration, or record",
                        ));
                    }
                };
                if map.next_key::<String>()?.is_some() {
                    return Err(A::Error::custom(
                        "an object that is a value has exactly one entry",
                    ));
                }
                Ok(value)
            }
        }

        deserializer.deserialize_any(Reader)
    }
}

/// Named values: an operation's input, a step's arguments, what a read
/// returned.
///
/// The names are held sorted, by their bytes, whatever order they were added
/// in. So the same named values are equal, are written alike, and have the
/// same digest, whoever built them and in whatever order; nobody has to
/// remember to sort. The order a person sees is the order the compiled
/// target's schema declares, which the reader of the schema applies (decision
/// 0068).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Values(BTreeMap<String, Value>);

impl Values {
    /// No values.
    #[must_use]
    pub const fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Sets the value of `name`, and returns the value it had.
    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<Value>) -> Option<Value> {
        self.0.insert(name.into(), value.into())
    }

    /// The same values with `name` set. For building in one expression.
    #[must_use]
    pub fn with(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.0.insert(name.into(), value.into());
        self
    }

    /// The value of `name`, when it has one.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.0.get(name)
    }

    /// Takes the value of `name` out.
    pub fn remove(&mut self, name: &str) -> Option<Value> {
        self.0.remove(name)
    }

    /// Whether `name` has a value, which may be [`Value::Null`].
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    /// How many names there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are no names.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Every name and its value, sorted by name.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(name, value)| (name.as_str(), value))
    }

    /// Every name, sorted.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    /// Adds the canonical bytes to `out`.
    pub fn write_canonical(&self, out: &mut Vec<u8>) {
        out.push(b'{');
        for (index, (name, value)) in self.0.iter().enumerate() {
            if index > 0 {
                out.push(b',');
            }
            write_string(out, name);
            out.push(b':');
            value.write_canonical(out);
        }
        out.push(b'}');
    }

    /// The canonical bytes: the same for the same named values on every
    /// machine, whatever order they were added in.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write_canonical(&mut out);
        out
    }

    /// The plain digest of the canonical bytes. It names an input or a diff
    /// that is not secret. To name a value that must not be guessable, use a
    /// [`KeyedDigest`](crate::KeyedDigest) over [`Values::canonical_bytes`].
    #[must_use]
    pub fn digest(&self) -> Digest {
        Digest::of(&self.canonical_bytes())
    }
}

impl<N: Into<String>, V: Into<Value>> FromIterator<(N, V)> for Values {
    fn from_iter<I: IntoIterator<Item = (N, V)>>(iter: I) -> Self {
        Self(
            iter.into_iter()
                .map(|(name, value)| (name.into(), value.into()))
                .collect(),
        )
    }
}

impl IntoIterator for Values {
    type Item = (String, Value);
    type IntoIter = std::collections::btree_map::IntoIter<String, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// The parts that find one record again, in the order the operation declares
/// its identity inputs, parent first.
///
/// Each part is one plain value, already normalized by its field type. The
/// order is part of the key: `[order, line]` and `[line, order]` are
/// different keys.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "Vec<Value>", into = "Vec<Value>")]
pub struct RecordKey(Vec<Value>);

impl RecordKey {
    /// Checks the parts and makes a key of them.
    ///
    /// # Errors
    ///
    /// [`InvalidValue::KeyEmpty`] when there are no parts, and
    /// [`InvalidValue::KeyPartNotPlain`] when a part is absent, a list, or a
    /// record.
    pub fn new(parts: Vec<Value>) -> Result<Self, InvalidValue> {
        if parts.is_empty() {
            return Err(InvalidValue::KeyEmpty);
        }
        if parts
            .iter()
            .any(|part| matches!(part, Value::Null | Value::List(_) | Value::Record(_)))
        {
            return Err(InvalidValue::KeyPartNotPlain);
        }
        Ok(Self(parts))
    }

    /// The parts, in declared order.
    #[must_use]
    pub fn parts(&self) -> &[Value] {
        &self.0
    }

    /// The canonical bytes: what a lock is taken on and what a keyed digest
    /// of the key is made from.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = vec![b'['];
        for (index, part) in self.0.iter().enumerate() {
            if index > 0 {
                out.push(b',');
            }
            part.write_canonical(&mut out);
        }
        out.push(b']');
        out
    }
}

impl TryFrom<Vec<Value>> for RecordKey {
    type Error = InvalidValue;

    fn try_from(parts: Vec<Value>) -> Result<Self, Self::Error> {
        Self::new(parts)
    }
}

impl From<RecordKey> for Vec<Value> {
    fn from(key: RecordKey) -> Self {
        key.0
    }
}

/// The type of a value: of an input, an output, a name in an expression, or
/// an expression's result. The expression checker and the compiled target
/// share this one description. It has no float.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    /// Only the absent value.
    Null,
    /// True or false.
    Bool,
    /// A whole number.
    Int,
    /// Text.
    Text,
    /// An exact number.
    Decimal,
    /// A moment.
    Instant,
    /// A calendar day.
    Date,
    /// A length of time.
    Duration,
    /// One of a field type's stored codes, held as text.
    Choice,
    /// Values of one type, in order.
    List(Box<ValueType>),
    /// Named values, each with its type, in the order the schema declares
    /// them.
    Record(Vec<FieldOf>),
}

impl ValueType {
    /// The name of the type, without what a list or a record holds.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Text => "text",
            Self::Decimal => "decimal",
            Self::Instant => "instant",
            Self::Date => "date",
            Self::Duration => "duration",
            Self::Choice => "choice",
            Self::List(_) => "list",
            Self::Record(_) => "record",
        }
    }

    /// A list of `item`.
    #[must_use]
    pub fn list_of(item: Self) -> Self {
        Self::List(Box::new(item))
    }
}

/// One named field of a record type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldOf {
    /// The field's name.
    pub name: String,
    /// The type of its value.
    pub ty: ValueType,
    /// Whether the field must have a value.
    pub required: bool,
}

impl FieldOf {
    /// A field that must have a value.
    #[must_use]
    pub fn required(name: impl Into<String>, ty: ValueType) -> Self {
        Self {
            name: name.into(),
            ty,
            required: true,
        }
    }

    /// A field that may be absent.
    #[must_use]
    pub fn optional(name: impl Into<String>, ty: ValueType) -> Self {
        Self {
            name: name.into(),
            ty,
            required: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Date, FieldOf, InvalidValue, Millis, RecordKey, Value, ValueType, Values};
    use crate::canonical::to_bytes;
    use crate::{Decimal, Failure, FailureClass, FailureCode, Timestamp};

    fn d(text: &str) -> Value {
        Value::Decimal(Decimal::parse(text).unwrap())
    }

    /// One value of every kind, nested.
    fn every_kind() -> Values {
        Values::new()
            .with("absent", Value::Null)
            .with("flag", true)
            .with("count", -3i64)
            .with("name", "Ünïcode \"quoted\"\n")
            .with("price", d("12.50"))
            .with("seen", Timestamp::from_unix_millis(1_795_000_000_123))
            .with("due", Date::new(2026, 11, 17).unwrap())
            .with("wait", Value::Duration(Millis::new(-1_500)))
            .with("lines", vec![d("1"), Value::Null, Value::List(vec![])])
            .with(
                "inner",
                Values::new()
                    .with("code", "A1")
                    .with("empty", Values::new()),
            )
    }

    #[test]
    fn decimals_compare_exactly_and_kinds_are_never_converted() {
        assert_eq!(d("1.5"), d("1.50"));
        assert_ne!(d("32.5625"), d("32.56"));
        assert_ne!(d("0.1"), d("0.10000000000000000000000001"));
        // A whole number is not a decimal, and text is not a number.
        assert_ne!(Value::Int(1), d("1"));
        assert_ne!(Value::Text("1".to_owned()), Value::Int(1));
        assert_ne!(Value::Null, Value::Text(String::new()));
        assert_eq!(
            Values::new().with("amount", d("40.0")),
            Values::new().with("amount", d("40"))
        );
    }

    #[test]
    fn named_values_are_the_same_whatever_order_they_were_added_in() {
        let forward = Values::new()
            .with("b", 2i64)
            .with("a", 1i64)
            .with("c", d("3.0"));
        let backward: Values = [("c", d("3")), ("a", Value::Int(1)), ("b", Value::Int(2))]
            .into_iter()
            .collect();
        assert_eq!(forward, backward);
        assert_eq!(forward.canonical_bytes(), backward.canonical_bytes());
        assert_eq!(forward.digest(), backward.digest());
        assert_eq!(forward.names().collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(
            String::from_utf8(forward.canonical_bytes()).unwrap(),
            r#"{"a":1,"b":2,"c":{"decimal":"3"}}"#
        );
        assert_ne!(forward.digest(), forward.clone().with("a", 2i64).digest());
    }

    #[test]
    fn a_value_reads_back_as_what_was_written() {
        let values = every_kind();
        let json = serde_json::to_string(&values).unwrap();
        assert_eq!(serde_json::from_str::<Values>(&json).unwrap(), values);
        let single = Value::Record(values);
        let json = serde_json::to_string(&single).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&json).unwrap(), single);
    }

    #[test]
    fn the_canonical_bytes_are_what_the_compiled_targets_encoding_gives() {
        let values = every_kind();
        assert_eq!(values.canonical_bytes(), to_bytes(&values).unwrap());
        let single = Value::Record(values);
        assert_eq!(single.canonical_bytes(), to_bytes(&single).unwrap());
    }

    #[test]
    fn a_float_or_an_untagged_object_is_not_a_value() {
        for bad in [
            "1.5",
            "1e3",
            "[1, 2.0]",
            "{}",
            r#"{"amount": 3}"#,
            r#"{"decimal": "1.5", "date": "2026-11-17"}"#,
            r#"{"decimal": 1.5}"#,
            r#"{"decimal": "1e3"}"#,
            r#"{"date": "2026-02-30"}"#,
            r#"{"record": {"price": 9.99}}"#,
            "18446744073709551615",
        ] {
            assert!(serde_json::from_str::<Value>(bad).is_err(), "{bad}");
        }
        assert_eq!(serde_json::from_str::<Value>("7").unwrap(), Value::Int(7));
        assert_eq!(serde_json::from_str::<Value>("null").unwrap(), Value::Null);
    }

    #[test]
    fn a_date_is_a_day_that_exists() {
        let date: Date = "2028-02-29".parse().unwrap();
        assert_eq!((date.year(), date.month(), date.day()), (2028, 2, 29));
        assert_eq!(date.to_string(), "2028-02-29");
        assert!(Date::new(2026, 11, 17).unwrap() < Date::new(2026, 12, 1).unwrap());
        for bad in [
            "2026-02-29",
            "2026-13-01",
            "2026-00-10",
            "2026-11-00",
            "2026-11-31",
            "1900-02-29",
            "26-11-17",
            "2026-1-7",
            "2026/11/17",
            "2026-11-17T00:00",
            "",
            "２０２６-11-17",
        ] {
            assert_eq!(
                bad.parse::<Date>(),
                Err(InvalidValue::DateMalformed),
                "{bad}"
            );
        }
        assert_eq!(
            "0000-01-01".parse::<Date>(),
            Err(InvalidValue::DateOutOfRange)
        );
        assert_eq!(Date::new(10_000, 1, 1), Err(InvalidValue::DateOutOfRange));
        let failure: Failure = InvalidValue::DateMalformed.into();
        assert_eq!(failure.class(), FailureClass::Input);
        assert_eq!(failure.code(), &FailureCode::VALUE_OUT_OF_RANGE);
    }

    #[test]
    fn a_date_counts_days_from_1970_and_back() {
        let known = [
            ("1970-01-01", 0),
            ("1969-12-31", -1),
            ("2000-02-29", 11_016),
            ("2000-03-01", 11_017),
            ("2026-11-17", 20_774),
            ("1899-12-30", -25_569),
            ("0001-01-01", -719_162),
            ("9999-12-31", 2_932_896),
        ];
        for (text, days) in known {
            let date: Date = text.parse().unwrap();
            assert_eq!(date.to_days(), days, "{text}");
            assert_eq!(Date::from_days(days).unwrap(), date, "{text}");
        }
        // Every day of eight years, across a century that is not a leap year.
        let start = "1897-01-01".parse::<Date>().unwrap().to_days();
        let mut previous = Date::from_days(start - 1).unwrap();
        for days in start..start + 8 * 366 {
            let date = Date::from_days(days).unwrap();
            assert_eq!(date.to_days(), days);
            assert!(date > previous);
            previous = date;
        }
        assert_eq!(
            Date::from_days(2_932_897),
            Err(InvalidValue::DateOutOfRange)
        );
        assert_eq!(Date::from_days(-719_163), Err(InvalidValue::DateOutOfRange));
        assert_eq!(Date::from_days(i32::MIN), Err(InvalidValue::DateOutOfRange));
    }

    #[test]
    fn a_record_key_is_plain_parts_in_declared_order() {
        let key = RecordKey::new(vec![Value::from("SO-1001"), Value::Int(3)]).unwrap();
        let turned = RecordKey::new(vec![Value::Int(3), Value::from("SO-1001")]).unwrap();
        assert_ne!(key, turned);
        assert_ne!(key.canonical_bytes(), turned.canonical_bytes());
        assert_eq!(
            String::from_utf8(key.canonical_bytes()).unwrap(),
            r#"["SO-1001",3]"#
        );
        assert_eq!(
            RecordKey::new(vec![d("7.0")]).unwrap().canonical_bytes(),
            RecordKey::new(vec![d("7")]).unwrap().canonical_bytes()
        );
        let json = serde_json::to_string(&key).unwrap();
        assert_eq!(serde_json::from_str::<RecordKey>(&json).unwrap(), key);

        assert_eq!(RecordKey::new(vec![]), Err(InvalidValue::KeyEmpty));
        for bad in [
            Value::Null,
            Value::List(vec![]),
            Value::Record(Values::new()),
        ] {
            assert_eq!(
                RecordKey::new(vec![Value::Int(1), bad]),
                Err(InvalidValue::KeyPartNotPlain)
            );
        }
        assert!(serde_json::from_str::<RecordKey>("[]").is_err());
        assert!(serde_json::from_str::<RecordKey>("[null]").is_err());
        let failure: Failure = InvalidValue::KeyEmpty.into();
        assert_eq!(failure.code(), &FailureCode::VALUE_OUT_OF_RANGE);
    }

    #[test]
    fn a_value_is_checked_against_a_type() {
        let order = ValueType::Record(vec![
            FieldOf::required("id", ValueType::Text),
            FieldOf::required("total", ValueType::Decimal),
            FieldOf::optional("status", ValueType::Choice),
            FieldOf::optional("lines", ValueType::list_of(ValueType::Int)),
        ]);
        let good = Values::new()
            .with("id", "SO-1")
            .with("total", d("9.5"))
            .with("status", "open");
        assert!(Value::Record(good.clone()).has_type(&order));
        assert!(Value::Record(good.clone().with("lines", vec![Value::Int(1)])).has_type(&order));

        let missing = Values::new().with("id", "SO-1");
        let wrong_kind = good.clone().with("total", 9i64);
        let undeclared = good.clone().with("extra", 1i64);
        let wrong_item = good.clone().with("lines", vec![Value::from("one")]);
        let absent_required = good.with("id", Value::Null);
        for bad in [missing, wrong_kind, undeclared, wrong_item, absent_required] {
            assert!(!Value::Record(bad).has_type(&order));
        }
        assert!(!Value::Int(1).has_type(&ValueType::Decimal));
        assert!(Value::Null.has_type(&ValueType::Date));
    }

    #[test]
    fn a_type_reads_back_as_what_was_written() {
        let ty = ValueType::Record(vec![
            FieldOf::required("id", ValueType::Text),
            FieldOf::optional("lines", ValueType::list_of(ValueType::Decimal)),
        ]);
        let json = serde_json::to_string(&ty).unwrap();
        assert_eq!(
            json,
            r#"{"record":[{"name":"id","ty":"text","required":true},{"name":"lines","ty":{"list":"decimal"},"required":false}]}"#
        );
        assert_eq!(serde_json::from_str::<ValueType>(&json).unwrap(), ty);
        assert!(serde_json::from_str::<ValueType>("\"double\"").is_err());
        assert_eq!(ty.name(), "record");
    }
}
