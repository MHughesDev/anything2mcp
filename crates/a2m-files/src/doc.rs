//! Located documents: a YAML value as a tree in which every value keeps the
//! file, line, and column it was written at.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use a2m_core::{Failure, FailureClass};

use crate::diag::PATH_NOT_ALLOWED;

/// A path inside a target folder: names joined by `/`, on every operating
/// system. It never starts at a root, never holds `..`, `.`, an empty name,
/// a backslash, or a colon, so it cannot name anything outside the folder.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelPath(Arc<str>);

impl RelPath {
    /// Checks `text` and makes a path of it.
    ///
    /// # Errors
    ///
    /// A failure of class `input`, code `path_not_allowed`, when the text is
    /// empty, absolute, or holds a part that could leave the folder.
    pub fn new(text: &str) -> Result<Self, Failure> {
        let fine = !text.is_empty()
            && text.split('/').all(|part| {
                !part.is_empty() && part != "." && part != ".." && !part.contains(['\\', ':', '\0'])
            });
        if fine {
            Ok(Self(Arc::from(text)))
        } else {
            Err(Failure::new(
                FailureClass::Input,
                PATH_NOT_ALLOWED,
                "A path in a target folder is names joined by `/`, with no `..`, no drive, and no leading `/`.",
                "Name the file by its path inside the target folder, such as `operations/order_get.yaml`.",
            ))
        }
    }

    /// The path of `target.yaml`, the one file every target has.
    #[must_use]
    pub fn target_file() -> Self {
        Self(Arc::from("target.yaml"))
    }

    /// The path as text, with `/` between names.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The names the path is made of, outermost first.
    pub fn parts(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }

    /// The first name, when the path has more than one: the folder a file
    /// kind is registered for.
    #[must_use]
    pub fn folder(&self) -> Option<&str> {
        self.0.split_once('/').map(|(folder, _)| folder)
    }

    /// The last name.
    #[must_use]
    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }

    /// The last name without its extension.
    #[must_use]
    pub fn stem(&self) -> &str {
        let name = self.file_name();
        name.rsplit_once('.').map_or(name, |(stem, _)| stem)
    }

    /// The path under `root` on this machine, joined name by name.
    #[must_use]
    pub fn under(&self, root: &Path) -> PathBuf {
        let mut path = root.to_path_buf();
        for part in self.parts() {
            path.push(part);
        }
        path
    }

    /// This path with one more name.
    ///
    /// # Errors
    ///
    /// As for [`RelPath::new`].
    pub fn join(&self, name: &str) -> Result<Self, Failure> {
        Self::new(&format!("{}/{name}", self.0))
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RelPath({})", self.0)
    }
}

impl serde::Serialize for RelPath {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

/// Where a value was written: a file, a line, and a column, both counted
/// from 1.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub struct Place {
    /// The file, by its path inside the target folder.
    pub file: RelPath,
    /// The line, from 1.
    pub line: u32,
    /// The column, in characters, from 1.
    pub column: u32,
}

impl Place {
    /// The first character of a file.
    #[must_use]
    pub fn start_of(file: &RelPath) -> Self {
        Self {
            file: file.clone(),
            line: 1,
            column: 1,
        }
    }
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line, self.column)
    }
}

/// A value and the place it was written at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located<T> {
    /// The value.
    pub value: T,
    /// Where it was written.
    pub at: Place,
}

impl<T> Located<T> {
    /// A value at a place.
    pub const fn new(value: T, at: Place) -> Self {
        Self { value, at }
    }
}

/// One key of a map and its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The key, as written, and where.
    pub key: Located<String>,
    /// The value, and where.
    pub value: Located<Doc>,
}

/// One YAML value. There is no float: a plain number with a decimal point is
/// kept as its text, and a field that takes a decimal reads the text exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Doc {
    /// Nothing: `null`, `~`, or an empty value.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A whole number.
    Int(i64),
    /// Text, quoted or plain.
    Text(String),
    /// A sequence, in written order.
    List(Vec<Located<Doc>>),
    /// A mapping, in written order. No key appears twice.
    Map(Vec<Entry>),
}

impl Doc {
    /// What kind of value this is, for a message.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Null => "nothing",
            Self::Bool(_) => "true or false",
            Self::Int(_) => "whole number",
            Self::Text(_) => "text",
            Self::List(_) => "list",
            Self::Map(_) => "map",
        }
    }
}

impl Located<Doc> {
    /// The value under `key`, when this is a map that has it.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Located<Doc>> {
        self.entries()
            .iter()
            .find(|entry| entry.key.value == key)
            .map(|entry| &entry.value)
    }

    /// The entries, when this is a map; none otherwise.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        match &self.value {
            Doc::Map(entries) => entries,
            _ => &[],
        }
    }

    /// The items, when this is a list; none otherwise.
    #[must_use]
    pub fn items(&self) -> &[Located<Doc>] {
        match &self.value {
            Doc::List(items) => items,
            _ => &[],
        }
    }

    /// The text, when this is text.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match &self.value {
            Doc::Text(text) => Some(text),
            _ => None,
        }
    }

    /// The whole number, when this is one.
    #[must_use]
    pub const fn as_int(&self) -> Option<i64> {
        match &self.value {
            Doc::Int(number) => Some(*number),
            _ => None,
        }
    }

    /// The truth value, when this is one.
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match &self.value {
            Doc::Bool(flag) => Some(*flag),
            _ => None,
        }
    }

    /// The text under `key`, when this is a map that has text there.
    #[must_use]
    pub fn text(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Located::as_text)
    }

    /// The value as JSON, without places: what a pack puts in a payload.
    /// Maps keep their keys; the canonical encoding sorts them.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        match &self.value {
            Doc::Null => serde_json::Value::Null,
            Doc::Bool(flag) => serde_json::Value::Bool(*flag),
            Doc::Int(number) => serde_json::Value::from(*number),
            Doc::Text(text) => serde_json::Value::String(text.clone()),
            Doc::List(items) => {
                serde_json::Value::Array(items.iter().map(Located::to_json).collect())
            }
            Doc::Map(entries) => serde_json::Value::Object(
                entries
                    .iter()
                    .map(|entry| (entry.key.value.clone(), entry.value.to_json()))
                    .collect(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RelPath;

    #[test]
    fn a_path_that_could_leave_the_folder_is_refused() {
        for bad in [
            "",
            "/etc/hosts",
            "../target.yaml",
            "operations/../../x.yaml",
            "operations//x.yaml",
            "C:/x.yaml",
            "operations\\x.yaml",
            "./target.yaml",
        ] {
            let failure = RelPath::new(bad).unwrap_err();
            assert_eq!(failure.code().as_str(), "path_not_allowed", "{bad}");
        }
        let path = RelPath::new("operations/orders/order_get.yaml").unwrap();
        assert_eq!(path.folder(), Some("operations"));
        assert_eq!(path.file_name(), "order_get.yaml");
        assert_eq!(path.stem(), "order_get");
        assert_eq!(RelPath::new("target.yaml").unwrap().folder(), None);
    }

    #[test]
    fn a_path_is_joined_to_a_root_name_by_name() {
        let root = std::env::temp_dir().join("example");
        let path = RelPath::new("operations/order_get.yaml").unwrap();
        assert_eq!(
            path.under(&root),
            root.join("operations").join("order_get.yaml")
        );
    }
}
