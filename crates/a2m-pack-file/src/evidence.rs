//! What discovery records of a folder, a settings file, and a growing file
//! (decision 0255): names, kinds, sizes, and shapes, never a value and
//! never a content.
//!
//! A settings value is recorded by its path of keys and its shape. Where a
//! key names a secret, or the value has the form of a credential, even the
//! shape is withheld, and the finding says so: the value never reaches a
//! finding, a draft, or a message, and no read of it is drafted (rule 4).
//! A growing file is recorded by its name and the form its complete lines
//! share, where one form is plain; no line is kept.

use std::collections::BTreeSet;

use a2m_sources::Limits;
use a2m_sources::json::{JsonValue, parse_document};
use serde::{Deserialize, Serialize};

use crate::folder;
use crate::settings::{Kind, Part, Settings, Which};

/// The kind of finding that describes a surface's folder as a collection
/// of entries.
pub const FILE_FOLDER: &str = "file_folder";
/// The kind of finding that describes a growing file.
pub const FILE_LOG: &str = "file_log";

/// The most complete lines of a growing file looked at for their form.
pub(crate) const LINES_LOOKED_AT: usize = 64;
/// The most bytes of a growing file read for its form.
pub(crate) const LOG_HEAD_BYTES: u64 = 64 * 1024;

/// What an entry of a folder is, at its own name, with no link followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKindFound {
    /// A regular file.
    File,
    /// A folder.
    Folder,
    /// A link, wherever it points: never followed.
    Link,
    /// A device, a pipe, or a socket: never opened.
    Other,
}

/// One entry of a folder, as discovery saw it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryFound {
    /// Its name below the surface's folder, with `/` between folders.
    pub name: String,
    /// What it is.
    pub kind: EntryKindFound,
    /// A file's length in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// What a `file_folder` finding holds: a surface's folder, to a depth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FolderFound {
    /// The surface whose folder it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<String>,
    /// How many folders down it was looked at: 0 is the folder's own
    /// entries.
    pub depth: u8,
    /// Its entries, in the order of their names' bytes.
    pub entries: Vec<EntryFound>,
    /// The folders at that depth whose own entries were not read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cut: Vec<String>,
    /// Names the pack cannot use: not UTF-8, or a form some system would
    /// read otherwise.
    pub skipped: u64,
    /// Whether the folder held more entries than discovery records.
    pub truncated: bool,
}

impl FolderFound {
    pub(crate) fn of(surface: &str, depth: u8, survey: folder::Survey) -> Self {
        Self {
            surface: Some(surface.to_owned()),
            depth,
            entries: survey
                .entries
                .into_iter()
                .map(|entry| EntryFound {
                    kind: match entry.kind {
                        folder::Kind::File => EntryKindFound::File,
                        folder::Kind::Folder => EntryKindFound::Folder,
                        folder::Kind::Link => EntryKindFound::Link,
                        folder::Kind::Other => EntryKindFound::Other,
                    },
                    name: entry.name,
                    size: entry.size,
                })
                .collect(),
            cut: survey.cut,
            skipped: survey.skipped,
            truncated: survey.truncated,
        }
    }

    /// The deepest level at which an entry was found: 0 for the folder's
    /// own entries.
    #[must_use]
    pub fn deepest(&self) -> u8 {
        self.entries
            .iter()
            .map(|entry| u8::try_from(entry.name.matches('/').count()).unwrap_or(u8::MAX))
            .max()
            .unwrap_or(0)
    }
}

/// The form of one value of a settings file, never the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueShape {
    /// Text. In an INI file, any value that is not spelled as one of the
    /// shapes below.
    Text,
    /// A number: a property list's `integer` or `real`, or an INI value
    /// spelled as digits with an optional sign and one decimal point.
    Number,
    /// True or false: a property list's `true` or `false`, or an INI value
    /// spelled `true` or `false`.
    Boolean,
    /// Nothing: an INI key with no value, or an empty `string`.
    Empty,
    /// A property list's `array`.
    List,
    /// A property list's `dict`, whose keys are recorded beside it.
    Record,
    /// A property list's `date`.
    Date,
    /// A property list's `data`, which the pack does not read.
    Data,
    /// A form that programs read in more than one way, or a key the file
    /// holds twice: the reader does not read it as one value.
    Unread,
    /// Not recorded: see the `withheld` beside it.
    Withheld,
}

/// Why a settings value's shape is withheld.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Withheld {
    /// A key on its path names a secret: a password, a token, a key.
    KeyNamesASecret,
    /// The value has the form of a credential: key material, an
    /// authorization header's value, a signed token, a long run of letters
    /// and digits.
    ValueLooksLikeACredential,
}

/// One value of a settings file, as discovery recorded it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingFound {
    /// Its path of keys: an INI file's section, if any, and key; a property
    /// list's keys, outermost first.
    pub path: Vec<String>,
    /// Its shape.
    pub shape: ValueShape,
    /// Why its shape is withheld, when it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub withheld: Option<Withheld>,
}

/// The form every complete line of a growing file shares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineForm {
    /// Fields apart by one delimiter, the same number in every line.
    Delimited,
    /// Words of the form `key=value`, a value in double quotes allowed.
    KeyValue,
    /// One JSON record per line.
    JsonPerLine,
}

/// What a `file_log` finding holds: one file that other programs append
/// lines to, as the pack takes it from its name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogFound {
    /// The surface the file was observed on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<String>,
    /// The file's name inside its folder.
    pub file: String,
    /// How many complete lines were looked at for their form.
    pub lines_seen: u32,
    /// The form they all share, where one form is plain; nothing where none
    /// is, or more than one would fit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_form: Option<LineForm>,
    /// The delimiter of delimited lines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delimiter: Option<String>,
}

/// Words that make a key a secret where the key holds them anywhere, and
/// words that do where the key holds them as one part. The list is the one
/// the kernel's credential lint holds for target files, kept here because
/// the kernel does not export it.
const SECRET_WORDS: [&str; 15] = [
    "password",
    "passwd",
    "passphrase",
    "secret",
    "api_key",
    "apikey",
    "private_key",
    "access_token",
    "refresh_token",
    "auth_token",
    "id_token",
    "session_token",
    "bearer_token",
    "client_secret",
    "secret_key",
];
const SECRET_PARTS: [&str; 9] = [
    "token",
    "tokens",
    "cookie",
    "cookies",
    "authorization",
    "pin",
    "bearer",
    "credential",
    "credentials",
];

/// Whether a key's name says that its value is, or may be, a secret.
pub(crate) fn names_a_secret(key: &str) -> bool {
    let key = key.to_ascii_lowercase().replace(['-', ' '], "_");
    SECRET_WORDS.iter().any(|word| key.contains(word))
        || key
            .split(['_', '.'])
            .any(|part| SECRET_PARTS.contains(&part))
}

/// Whether text has the form of a credential, under any key.
pub(crate) fn looks_like_credential(text: &str) -> bool {
    let text = text.trim();
    if text.starts_with("-----BEGIN ") {
        return true;
    }
    let lower = text.to_ascii_lowercase();
    if ["bearer ", "basic ", "token ", "negotiate ", "digest "]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
    {
        return true;
    }
    if text.starts_with("eyJ") && text.matches('.').count() == 2 && text.len() >= 40 {
        return true;
    }
    // A long run with no space that mixes letters and digits: a key.
    text.len() >= 24
        && text.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'+' | b'/' | b'=' | b'.')
        })
        && text.bytes().any(|b| b.is_ascii_digit())
        && text.bytes().any(|b| b.is_ascii_alphabetic())
}

/// Whether an INI value is spelled as a number.
fn spells_a_number(text: &str) -> bool {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    let mut parts = digits.splitn(2, '.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    !whole.is_empty()
        && whole.bytes().all(|b| b.is_ascii_digit())
        && fraction.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
}

/// Each value of a settings file by its path and shape, in the order of
/// the file. An element of an array is not a place, and is recorded only
/// as the array's shape. `bytes` are the file's, to tell an array from a
/// dictionary.
pub(crate) fn settings_found(file: &Settings, bytes: &[u8]) -> Vec<SettingFound> {
    let path_of = |path: &[Part]| -> Option<Vec<String>> {
        path.iter()
            .map(|part| match part {
                Part::Key(key) => Some(key.clone()),
                Part::Index(_) => None,
            })
            .collect()
    };
    // A path the file holds twice, two spellings that differ only in ASCII
    // case included, is not one value.
    let mut seen = BTreeSet::new();
    let mut twice = BTreeSet::new();
    for setting in &file.entries {
        if let Some(path) = path_of(&setting.path) {
            let folded: Vec<String> = path.iter().map(|key| key.to_ascii_lowercase()).collect();
            if !seen.insert(folded.clone()) {
                twice.insert(folded);
            }
        }
    }
    let mut found = Vec::new();
    for setting in &file.entries {
        let Some(path) = path_of(&setting.path) else {
            continue;
        };
        if path.is_empty() {
            // The root dictionary of a property list.
            continue;
        }
        let folded: Vec<String> = path.iter().map(|key| key.to_ascii_lowercase()).collect();
        let withheld = if path.iter().any(|key| names_a_secret(key)) {
            Some(Withheld::KeyNamesASecret)
        } else if looks_like_credential(&setting.text) {
            Some(Withheld::ValueLooksLikeACredential)
        } else {
            None
        };
        let shape = if withheld.is_some() {
            ValueShape::Withheld
        } else if setting.unread.is_some() || twice.contains(&folded) {
            ValueShape::Unread
        } else {
            match setting.kind {
                Kind::Text if setting.text.is_empty() => ValueShape::Empty,
                Kind::Text if file.which == Which::Ini && spells_a_number(&setting.text) => {
                    ValueShape::Number
                }
                Kind::Text
                    if file.which == Which::Ini
                        && matches!(setting.text.as_str(), "true" | "false") =>
                {
                    ValueShape::Boolean
                }
                Kind::Text => ValueShape::Text,
                Kind::Integer | Kind::Real => ValueShape::Number,
                Kind::Bool => ValueShape::Boolean,
                Kind::Date => ValueShape::Date,
                Kind::Data => ValueShape::Data,
                Kind::Nested => {
                    let at = usize::try_from(setting.span.start).unwrap_or(usize::MAX);
                    if bytes.get(at..).is_some_and(|rest| rest.starts_with(b"<array")) {
                        ValueShape::List
                    } else {
                        ValueShape::Record
                    }
                }
            }
        };
        found.push(SettingFound {
            path,
            shape,
            withheld,
        });
    }
    found
}

/// The words of a line, apart at blanks, a run in double quotes kept whole.
fn words(line: &str) -> Option<Vec<&str>> {
    let mut words = Vec::new();
    let mut start = None;
    let mut quoted = false;
    for (at, character) in line.char_indices() {
        match character {
            '"' => quoted = !quoted,
            ' ' | '\t' if !quoted => {
                if let Some(from) = start.take() {
                    words.push(line.get(from..at)?);
                }
                continue;
            }
            _ => {}
        }
        if start.is_none() {
            start = Some(at);
        }
    }
    if quoted {
        return None;
    }
    if let Some(from) = start {
        words.push(line.get(from..)?);
    }
    Some(words)
}

/// Whether a word is `key=value`, the key a name.
fn is_key_value(word: &str) -> bool {
    let Some((key, _)) = word.split_once('=') else {
        return false;
    };
    key.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// The form the complete lines at the start of a growing file share: how
/// many were looked at, the form where exactly one fits, and the delimiter
/// of delimited lines. A line not ended is not a line; an empty line is
/// counted and holds no form. Where any line is not UTF-8, or more than one
/// form fits, none is named: rule 7.
pub(crate) fn line_form(head: &[u8], limits: &Limits) -> (u32, Option<LineForm>, Option<char>) {
    let mut complete: Vec<&[u8]> = head.split(|byte| *byte == b'\n').collect();
    // What follows the last line feed is not a line.
    complete.pop();
    complete.truncate(LINES_LOOKED_AT);
    let seen = u32::try_from(complete.len()).unwrap_or(u32::MAX);
    let mut lines = Vec::new();
    for line in complete {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Ok(line) = std::str::from_utf8(line) else {
            return (seen, None, None);
        };
        if !line.trim().is_empty() {
            lines.push(line);
        }
    }
    if lines.is_empty() {
        return (seen, None, None);
    }
    let json = lines.iter().all(|line| {
        line.trim_start().starts_with('{')
            && parse_document(line.as_bytes(), limits)
                .is_ok_and(|node| matches!(node.value, JsonValue::Record(_)))
    });
    if json {
        return (seen, Some(LineForm::JsonPerLine), None);
    }
    let delimiters: Vec<char> = ['\t', ',', '|', ';']
        .into_iter()
        .filter(|delimiter| {
            let count = lines[0].matches(*delimiter).count();
            count > 0
                && lines
                    .iter()
                    .all(|line| line.matches(*delimiter).count() == count)
        })
        .collect();
    let key_value = lines.iter().all(|line| {
        words(line).is_some_and(|words| !words.is_empty() && words.iter().all(|w| is_key_value(w)))
    });
    match (key_value, delimiters.as_slice()) {
        (true, []) => (seen, Some(LineForm::KeyValue), None),
        (false, [delimiter]) => (seen, Some(LineForm::Delimited), Some(*delimiter)),
        _ => (seen, None, None),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "a test")]

    use super::{LineForm, line_form, looks_like_credential, names_a_secret};

    fn limits() -> a2m_sources::Limits {
        a2m_sources::Limits {
            bytes: 1 << 20,
            unpacked_bytes: 1 << 20,
            rows: 1000,
            cells: 10_000,
            cell_bytes: 1024,
        }
    }

    #[test]
    fn a_line_form_is_named_only_where_one_form_alone_fits() {
        let form = |text: &str| {
            let (_, form, delimiter) = line_form(text.as_bytes(), &limits());
            (form, delimiter)
        };
        assert_eq!(
            form("{\"a\": 1, \"b\": 2}\n{\"a\": 3}\r\n{\"half"),
            (Some(LineForm::JsonPerLine), None)
        );
        assert_eq!(
            form("at=08:00 level=info msg=\"order drafted\"\nat=08:01 level=warn\n"),
            (Some(LineForm::KeyValue), None)
        );
        assert_eq!(
            form("08:00\torder\tdrafted\n08:01\torder\tsent\n"),
            (Some(LineForm::Delimited), Some('\t'))
        );
        // Plain text, two delimiters that both fit, a key=value word that
        // also holds a delimiter, and bytes that are not text: no form.
        assert_eq!(form("order A-100 drafted\norder A-100 sent\n"), (None, None));
        assert_eq!(form("a,b;c\nd,e;f\n"), (None, None));
        assert_eq!(form("a=1,b=2\nc=3,d=4\n"), (None, None));
        assert_eq!(line_form(b"a=1\n\xff\n", &limits()), (2, None, None));
        // A line not ended is not a line.
        assert_eq!(line_form(b"a=1", &limits()), (0, None, None));
    }

    #[test]
    fn a_key_naming_a_secret_and_a_value_shaped_as_one_are_told() {
        for key in ["password", "db_password", "API-KEY", "auth.token", "Credentials"] {
            assert!(names_a_secret(key), "{key}");
        }
        for key in ["prefix", "next_number", "open", "tokenizer", "spin"] {
            assert!(!names_a_secret(key), "{key}");
        }
        assert!(looks_like_credential("Bearer example-token-for-tests"));
        assert!(looks_like_credential("example0key0for0tests0only0000"));
        assert!(looks_like_credential("-----BEGIN EXAMPLE-----"));
        assert!(!looks_like_credential("A-"));
        assert!(!looks_like_credential("Rails & brackets"));
        assert!(!looks_like_credential("16:30"));
    }
}
