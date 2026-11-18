//! A growing file, read from where the last read stopped (decision 0246).
//!
//! Other programs append lines to the file. A read takes the complete lines
//! after a position, up to a bound of lines and of bytes, and gives the new
//! position, which the caller hands back as an input to the next read: the
//! kernel keeps a step's recorded values within one run and on its resume,
//! and nothing between runs (decision 0162). A position is the offset, a
//! digest of the file's identity, and a digest of its first bytes, so a
//! file that was replaced or cut short is told as such, as a typed outcome,
//! and the read never starts over on its own. A line that is not text ends
//! the lines a read gives; the read that meets it first tells it by its
//! outcome and gives the position after it.
//!
//! The file is opened for the one read, through a handle proven to be the
//! file at the name, and closed: it is never held between reads, never
//! locked, and never written. A partial last line is never a line. Lines are
//! UTF-8; a line ends at a line feed, with a carriage return before it taken
//! off.

use std::io::{Read as _, Seek as _, SeekFrom};

use a2m_core::{Digest, Value, Values};
use a2m_provider::StepFailure;

use crate::codes::{self, stop};
use crate::model::{LOG_LINES, LOG_MORE, LOG_OUTCOME, LOG_POSITION, LogSpec};
use crate::open::unavailable;
use crate::place::{Folder, lasting_identity};

/// How many of the file's first bytes the position holds a digest of.
const HEAD: u64 = 4096;
/// The form a position is written in, first.
const VERSION: &str = "v1";

/// What a read found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The file is the one the position was taken from: the lines after it.
    Read,
    /// Another file stands at the name.
    Replaced,
    /// The file is the one the position was taken from, and it is shorter
    /// than the position, or its first bytes changed: it was cut short, and
    /// may have grown again since.
    Truncated,
    /// The first line after the position is not text: no line is given,
    /// and the position after it is, so the next read goes on past it.
    Unreadable,
}

impl Outcome {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Replaced => "replaced",
            Self::Truncated => "truncated",
            Self::Unreadable => "unreadable",
        }
    }
}

/// Where a read stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Position {
    pub offset: u64,
    pub identity: String,
    pub head: String,
}

fn short(digest: Digest) -> String {
    let text = digest.to_string();
    let hex = text.rsplit(':').next().unwrap_or(&text);
    hex.chars().take(16).collect()
}

impl Position {
    pub(crate) fn text(&self) -> String {
        format!("{VERSION}.{}.{}.{}", self.offset, self.identity, self.head)
    }

    fn parse(text: &str) -> Result<Self, StepFailure> {
        let refused = || {
            stop(
                &codes::row::VALUE_UNPARSABLE,
                "The position given is not one a read of this file gave.",
                "Give the position the last read returned, or none to read from the start.",
            )
        };
        let mut parts = text.split('.');
        let (Some(VERSION), Some(offset), Some(identity), Some(head), None) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            return Err(refused());
        };
        let hex = |part: &str| part.len() == 16 && part.bytes().all(|b| b.is_ascii_hexdigit());
        if !hex(identity) || !hex(head) {
            return Err(refused());
        }
        Ok(Self {
            offset: offset.parse().map_err(|_| refused())?,
            identity: identity.to_owned(),
            head: head.to_owned(),
        })
    }
}

fn unread() -> StepFailure {
    StepFailure::not_sent(unavailable("cannot be read"))
}

/// The digest of the file's first `length` bytes, read through `file`.
fn head_of(file: &mut std::fs::File, length: u64) -> Result<String, StepFailure> {
    file.seek(SeekFrom::Start(0)).map_err(|_| unread())?;
    let mut bytes = Vec::new();
    file.take(length.min(HEAD))
        .read_to_end(&mut bytes)
        .map_err(|_| unread())?;
    Ok(short(Digest::of(&bytes)))
}

/// One read of the growing file. Blocking.
pub(crate) fn read(
    folder: &Folder,
    spec: &LogSpec,
    from: Option<&str>,
    max_bytes: u64,
) -> Result<Values, StepFailure> {
    let given = from.map(Position::parse).transpose()?;
    let named = folder.file(&spec.file)?;
    let (mut file, metadata) = named.open()?;
    let identity = lasting_identity(&file, &metadata)
        .map(|text| short(Digest::of(text.as_bytes())))
        .ok_or_else(|| StepFailure::not_sent(unavailable("has no identity the system gives")))?;
    let length = metadata.len();
    let offset = given.as_ref().map_or(0, |position| position.offset);
    let outcome = match &given {
        None => Outcome::Read,
        Some(position) if position.identity != identity => Outcome::Replaced,
        Some(position) if length < position.offset => Outcome::Truncated,
        // The same file, whose first bytes are not the ones it had: it was
        // cut short and written again.
        Some(position) if head_of(&mut file, position.offset)? != position.head => {
            Outcome::Truncated
        }
        Some(_) => Outcome::Read,
    };
    let unchanged = |outcome: Outcome| {
        Values::new()
            .with(LOG_LINES, Value::List(Vec::new()))
            .with(
                LOG_POSITION,
                from.map_or(Value::Null, |text| Value::Text(text.to_owned())),
            )
            .with(LOG_OUTCOME, outcome.as_str())
            .with(LOG_MORE, false)
    };
    if outcome != Outcome::Read {
        return Ok(unchanged(outcome));
    }
    let bound = u64::from(spec.max_bytes).min(max_bytes);
    file.seek(SeekFrom::Start(offset)).map_err(|_| unread())?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(bound)
        .read_to_end(&mut bytes)
        .map_err(|_| unread())?;
    let Some(last_end) = bytes.iter().rposition(|byte| *byte == b'\n') else {
        if bytes.len() as u64 == bound && offset + bound < length {
            return Err(stop(
                &codes::row::FILE_TOO_LARGE,
                "A line of the file is longer than the most bytes one read takes, so it can never be read whole.",
                "Raise `max_bytes` in the log's declaration.",
            ));
        }
        // Only a partial line follows the position: no line yet.
        let mut values = unchanged(Outcome::Read);
        let head = head_of(&mut file, offset)?;
        values.insert(
            LOG_POSITION,
            Position {
                offset,
                identity,
                head,
            }
            .text(),
        );
        return Ok(values);
    };
    let mut lines = Vec::new();
    let mut consumed = 0_usize;
    let mut cut = false;
    let mut outcome = Outcome::Read;
    for raw in bytes
        .get(..=last_end)
        .unwrap_or_default()
        .split_inclusive(|b| *b == b'\n')
    {
        if lines.len() >= spec.max_lines as usize {
            cut = true;
            break;
        }
        let content = raw.strip_suffix(b"\n").unwrap_or(raw);
        let content = content.strip_suffix(b"\r").unwrap_or(content);
        let Ok(text) = std::str::from_utf8(content) else {
            // A line that is not text ends the lines given. Where it is the
            // first, it is told by the outcome, and the position goes past
            // it; otherwise the next read begins at it.
            if lines.is_empty() {
                outcome = Outcome::Unreadable;
                consumed += raw.len();
            }
            cut = true;
            break;
        };
        lines.push(Value::Text(text.to_owned()));
        consumed += raw.len();
    }
    let next = offset + consumed as u64;
    let more = if outcome == Outcome::Unreadable {
        next < length
    } else {
        cut || (bytes.len() as u64 == bound && offset + bound < length)
    };
    let head = head_of(&mut file, next)?;
    drop(file);
    Ok(Values::new()
        .with(LOG_LINES, Value::List(lines))
        .with(
            LOG_POSITION,
            Position {
                offset: next,
                identity,
                head,
            }
            .text(),
        )
        .with(LOG_OUTCOME, outcome.as_str())
        .with(LOG_MORE, more))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_position_is_read_back_only_in_its_own_form() {
        let position = Position {
            offset: 42,
            identity: "0123456789abcdef".to_owned(),
            head: "fedcba9876543210".to_owned(),
        };
        assert_eq!(Position::parse(&position.text()).unwrap(), position);
        for text in [
            "",
            "v1.42",
            "v2.42.0123456789abcdef.fedcba9876543210",
            "v1.x.0123456789abcdef.fedcba9876543210",
            "v1.1.short.fedcba9876543210",
        ] {
            let failure = Position::parse(text).unwrap_err();
            assert_eq!(failure.failure.code(), &codes::VALUE_UNPARSABLE, "{text}");
        }
    }
}
