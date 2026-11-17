//! What the control interface reports: values exactly as stored, rows, columns,
//! triggers, and the log of writes.

use std::collections::BTreeMap;

use rusqlite::types::{ToSqlOutput, ValueRef};

use crate::schema::Table;

/// One value exactly as the engine stores it, with its storage class.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Value {
    /// No value.
    Null,
    /// A whole number.
    Integer(i64),
    /// Text. Decimals, dates, and times are stored as text.
    Text(String),
    /// A floating value, which the seed never holds. It is kept as its shortest
    /// exact text, so that two of them are never compared as floats.
    Real(String),
    /// Bytes.
    Blob(Vec<u8>),
}

impl Value {
    pub(crate) fn from_ref(value: ValueRef<'_>) -> Self {
        match value {
            ValueRef::Null => Self::Null,
            ValueRef::Integer(n) => Self::Integer(n),
            ValueRef::Real(f) => Self::Real(format!("{f:?}")),
            ValueRef::Text(bytes) => Self::Text(String::from_utf8_lossy(bytes).into_owned()),
            ValueRef::Blob(bytes) => Self::Blob(bytes.to_vec()),
        }
    }
}

impl From<&str> for Value {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<i64> for Value {
    fn from(number: i64) -> Self {
        Self::Integer(number)
    }
}

impl rusqlite::ToSql for Value {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(match self {
            Self::Null => ToSqlOutput::Borrowed(ValueRef::Null),
            Self::Integer(n) => ToSqlOutput::Borrowed(ValueRef::Integer(*n)),
            Self::Text(text) => ToSqlOutput::Borrowed(ValueRef::Text(text.as_bytes())),
            Self::Real(text) => {
                let number: f64 = text
                    .parse()
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                ToSqlOutput::Borrowed(ValueRef::Real(number))
            }
            Self::Blob(bytes) => ToSqlOutput::Borrowed(ValueRef::Blob(bytes)),
        })
    }
}

/// One row exactly as stored: each column's name and its value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    values: BTreeMap<String, Value>,
}

impl Row {
    pub(crate) fn new(values: BTreeMap<String, Value>) -> Self {
        Self { values }
    }

    /// The stored value of a column, or `None` if the row has no such column.
    #[must_use]
    pub fn get(&self, column: &str) -> Option<&Value> {
        self.values.get(column)
    }

    /// The stored text of a column, or `None` if it is absent or not stored as text.
    #[must_use]
    pub fn text(&self, column: &str) -> Option<&str> {
        match self.values.get(column) {
            Some(Value::Text(text)) => Some(text),
            _ => None,
        }
    }

    /// The stored whole number of a column, or `None` if it is absent or not stored
    /// as a whole number.
    #[must_use]
    pub fn integer(&self, column: &str) -> Option<i64> {
        match self.values.get(column) {
            Some(Value::Integer(number)) => Some(*number),
            _ => None,
        }
    }

    /// Whether the row has this column and stores no value in it.
    #[must_use]
    pub fn is_null(&self, column: &str) -> bool {
        self.values.get(column) == Some(&Value::Null)
    }

    /// Every column and its value, in the order of the column names.
    pub fn values(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.values
            .iter()
            .map(|(name, value)| (name.as_str(), value))
    }
}

/// One column as the store's catalog describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    /// The column's name.
    pub name: String,
    /// The type the column was declared with, as written in the schema.
    pub declared_type: String,
    /// Whether the column refuses a null.
    pub not_null: bool,
    /// The column's place in the primary key, counted from 1, or 0 if it is not
    /// part of it.
    pub key_position: u32,
}

/// One trigger in the store's catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trigger {
    /// The trigger's name. The stand's own start with `stand_`.
    pub name: String,
    /// The table it is on.
    pub table: String,
}

/// What a write did to a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteAction {
    /// The row was inserted.
    Insert,
    /// The row was updated.
    Update,
    /// The row was deleted.
    Delete,
}

/// Who made a write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Writer {
    /// The control interface, acting as someone else.
    Stand,
    /// Any other connection: the one under test, or a test's own.
    Other,
}

/// One change to one row, as the stand's triggers saw it.
///
/// The engine shows one connection nothing of another's statements, so this is
/// what the stand can observe of them: each row a statement inserted, updated, or
/// deleted, in order. A statement that changes three rows leaves three records. A
/// read leaves none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteRecord {
    /// The record's place in the log, rising by one, from 1 after a reset.
    pub seq: i64,
    /// The table the row is in.
    pub table: Table,
    /// The row's key, one text per key column. For the table with no key, the
    /// engine's row number.
    pub key: Vec<String>,
    /// What happened to the row.
    pub action: WriteAction,
    /// Who did it.
    pub writer: Writer,
}
