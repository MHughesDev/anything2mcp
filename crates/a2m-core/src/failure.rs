//! Failures: a class, a stable code, a plain sentence, and a next step.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use crate::digest::InvalidDigest;
use crate::name::{InvalidName, UnknownName, check_name, is_name};
use crate::vocabulary::vocabulary;

vocabulary! {
    /// What kind of failure stopped a call (spec section 207). The class alone
    /// decides what happens next: whether to retry, stop, reconcile, or ask a
    /// person. The list is closed; a new kind of system maps its failures to
    /// these.
    pub enum FailureClass: "failure class" {
        /// The policy engine refused: no grant, over a limit, a stale approval.
        Policy => "policy",
        /// Inputs or a source file failed validation; nothing ran.
        Input => "input",
        /// The session ended and could not renew itself.
        SessionExpired => "session_expired",
        /// A bot defence or an edge refused the request.
        EdgeBlocked => "edge_blocked",
        /// The system no longer matches what the target files describe.
        Drift => "drift",
        /// Something appeared that no rule covers.
        UnknownVariant => "unknown_variant",
        /// The system refused the input with its own validation message.
        Rejected => "rejected",
        /// The value read back differs from the value intended.
        Mismatch => "mismatch",
        /// A write may or may not have happened.
        PossibleCommit => "possible_commit",
        /// A record no longer matches what the preview saw.
        Stale => "stale",
        /// A timeout or a lost connection that may pass.
        Transient => "transient",
        /// A queue is full, a record is locked, or no runner is free.
        Busy => "busy",
    }
}

/// The stable code of one failure within its class, such as `needs_login`.
///
/// Codes are not a closed list: each crate names the failures it can produce.
/// A code never changes once a release has carried it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FailureCode(Cow<'static, str>);

impl FailureCode {
    /// The longest a code may be.
    pub const LONGEST: usize = 64;

    /// A file, an input, or a name does not have the shape its schema requires.
    pub const SCHEMA_INVALID: Self = crate::codes::SCHEMA_INVALID;

    /// A finding claims more than its source, its parents, or its content allow.
    pub const FINDING_INVALID: Self = crate::codes::FINDING_INVALID;

    /// A decimal, a date, a key part, or a moment cannot be represented.
    pub const VALUE_OUT_OF_RANGE: Self = crate::codes::VALUE_OUT_OF_RANGE;

    /// A compiled target's bytes do not match the hash it was published with.
    pub const COMPILED_TARGET_HASH_MISMATCH: Self = crate::codes::COMPILED_TARGET_HASH_MISMATCH;

    /// A compiled target's format is not one this engine loads.
    pub const COMPILED_TARGET_FORMAT_UNSUPPORTED: Self =
        crate::codes::COMPILED_TARGET_FORMAT_UNSUPPORTED;

    /// A compiled target's bytes match its hash and are not a well-formed
    /// compiled target.
    pub const COMPILED_TARGET_UNREADABLE: Self = crate::codes::COMPILED_TARGET_UNREADABLE;

    /// A payload in a compiled target does not decode as the type its reader
    /// expects.
    pub const PAYLOAD_UNREADABLE: Self = crate::codes::PAYLOAD_UNREADABLE;

    /// Data holds something the compiled target's encoding cannot carry, such
    /// as a float.
    pub const PAYLOAD_NOT_ENCODABLE: Self = crate::codes::PAYLOAD_NOT_ENCODABLE;

    /// Makes a code from a name written in the source.
    ///
    /// # Panics
    ///
    /// When the name is not well formed. Used in a `const`, as it is meant to
    /// be, that is an error at compile time and never at run time.
    #[must_use]
    pub const fn fixed(name: &'static str) -> Self {
        assert!(
            is_name(name, Self::LONGEST),
            "a failure code is lowercase letters, digits, and underscores, starting with a letter"
        );
        Self(Cow::Borrowed(name))
    }

    /// Checks `name` and makes a code of it.
    ///
    /// # Errors
    ///
    /// [`InvalidName`] when the name is not lowercase letters, digits, and
    /// underscores starting with a letter, or is longer than
    /// [`FailureCode::LONGEST`].
    pub fn new(name: &str) -> Result<Self, InvalidName> {
        check_name("failure code", name, Self::LONGEST)?;
        Ok(Self(Cow::Owned(name.to_owned())))
    }

    /// The code as it appears in results, the ledger, and messages.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FailureCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for FailureCode {
    type Err = InvalidName;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::new(text)
    }
}

impl serde::Serialize for FailureCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for FailureCode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::new(&text).map_err(serde::de::Error::custom)
    }
}

/// One failure, as a caller sees it: a class, a stable code, a plain
/// sentence, and what to do next (technical reference, "Failure codes").
///
/// The sentence and the next step are shown to callers and written to logs.
/// They must never contain a secret; build them from names and counts, not
/// from values a system returned.
///
/// What the system itself said, such as a validation message or the name of
/// a constraint, is kept apart, in [`Failure::said`]: bounded, redacted where
/// a redactor passes over the failure, and untrusted. It is never joined into
/// the sentence or the next step, and whoever shows it marks it as the
/// system's words (spec section 502).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    class: FailureClass,
    code: FailureCode,
    message: String,
    next_step: String,
    // What most failures leave empty, behind one pointer, so that a
    // failure, which most errors of the kernel carry, is no larger for it:
    // three fields held in place would push the errors that hold a failure
    // over clippy's limit for a large error. `None` whenever every part of
    // it is empty, so that two equal failures compare equal.
    extra: Option<Box<Extra>>,
}

/// The parts of a failure that most failures leave empty.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Extra {
    said: Option<String>,
    next: Option<NextAction>,
    retry_after_ms: Option<u64>,
}

impl Extra {
    fn is_empty(&self) -> bool {
        self.said.is_none() && self.next.is_none() && self.retry_after_ms.is_none()
    }
}

/// An action a failure names as the way out, with its arguments, such as
/// `target.status` with `{"target": "example"}`: what was made where the
/// failure was made, carried as data. Whoever passes it to a caller checks
/// that the action exists and that its arguments fit it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NextAction {
    /// The action's name.
    pub action: String,
    /// Its arguments, by field name.
    pub args: serde_json::Value,
}

/// A failure as it is written and read: every part by its name, the parts
/// most failures leave empty only when present.
#[derive(serde::Serialize)]
struct FailureOut<'a> {
    class: FailureClass,
    code: &'a FailureCode,
    message: &'a str,
    next_step: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    said: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next: Option<&'a NextAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_after_ms: Option<u64>,
}

#[derive(serde::Deserialize)]
struct FailureIn {
    class: FailureClass,
    code: FailureCode,
    message: String,
    next_step: String,
    // Read back through the same bound as `with_said`: a failure that
    // arrives as data keeps no more than one made here.
    #[serde(default, deserialize_with = "said_read")]
    said: Option<String>,
    #[serde(default)]
    next: Option<NextAction>,
    #[serde(default)]
    retry_after_ms: Option<u64>,
}

impl serde::Serialize for Failure {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        FailureOut {
            class: self.class,
            code: &self.code,
            message: &self.message,
            next_step: &self.next_step,
            said: self.said(),
            next: self.next(),
            retry_after_ms: self.retry_after_ms(),
        }
        .serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for Failure {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let read = FailureIn::deserialize(deserializer)?;
        let mut failure = Self::new(read.class, read.code, read.message, read.next_step);
        failure.change(|extra| {
            extra.said = read.said;
            extra.next = read.next;
            extra.retry_after_ms = read.retry_after_ms;
        });
        Ok(failure)
    }
}

fn said_read<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    let text = <Option<String> as serde::Deserialize>::deserialize(deserializer)?;
    Ok(text.and_then(|text| kept_said(&text)))
}

/// The longest text of what the system said that a door shows, in
/// characters. A failure keeps the text whole; a door cuts it for display,
/// after it was redacted, with [`said_shown`].
pub const SAID_LONGEST: usize = 400;

/// The most a failure keeps of what the system said, in bytes. Above it the
/// text is dropped whole, never cut, and [`SAID_TOO_LONG`] stands in its
/// place: a prefix could hold part of a secret the redactor would match
/// only whole.
pub const SAID_CEILING: usize = 64 * 1024;

/// What stands in `said` for a text above [`SAID_CEILING`].
pub const SAID_TOO_LONG: &str =
    "(what the system said was longer than 64 KiB, and none of it was kept)";

/// What the system said, as a failure keeps it: whole, with its line
/// breaks and every other character as given, so that the redactor, which
/// matches a secret and its encodings only whole, sees it as the system
/// said it. Nothing is cut, joined, or flattened before redaction (decision
/// 0185). `None` when the text holds nothing but white space.
fn kept_said(text: &str) -> Option<String> {
    if text.trim().is_empty() {
        return None;
    }
    if text.len() > SAID_CEILING {
        return Some(SAID_TOO_LONG.to_owned());
    }
    Some(text.to_owned())
}

/// Whether a character can reorder, hide, or restyle text when it is
/// shown, so that what the system said could pass for something else: every
/// control character (C0 and C1, the terminal's escape introducers ESC and
/// CSI among them, and NEL); the bidirectional marks, embeddings, overrides,
/// and isolates; the zero-width and other invisible format characters, the
/// soft hyphen, the word joiner, and the byte order mark; the line and
/// paragraph separators; the variation selectors and the tag characters;
/// and the other characters Unicode says to show as nothing (decision
/// 0193). The list is closed and written out here, from Unicode's
/// `Default_Ignorable_Code_Point` property and its format characters, so
/// that it does not move with an outside table.
#[must_use]
pub fn said_hides(c: char) -> bool {
    if c.is_control() {
        return true;
    }
    matches!(
        c as u32,
        0x00AD
            | 0x034F
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x115F..=0x1160
            | 0x17B4..=0x17B5
            | 0x180B..=0x180F
            | 0x200B..=0x200F
            | 0x2028..=0x202E
            | 0x2060..=0x206F
            | 0x3164
            | 0xFE00..=0xFE0F
            | 0xFEFF
            | 0xFFA0
            | 0xFFF0..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1345F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0000..=0xE0FFF
    )
}

/// Writes `text` with every character [`said_hides`] names as its escape,
/// `\n`, `\u{1b}`, `\u{202e}`, never removed, except those `keep` says
/// to leave.
fn escaped(text: &str, keep: fn(char) -> bool) -> Cow<'_, str> {
    if !text.chars().any(|c| said_hides(c) && !keep(c)) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 16);
    for c in text.chars() {
        if !said_hides(c) || keep(c) {
            out.push(c);
        } else if c.is_control() {
            out.extend(c.escape_default());
        } else {
            out.extend(c.escape_unicode());
        }
    }
    Cow::Owned(out)
}

/// What the system said, with every character [`said_hides`] names written
/// as its escape, except a line feed and a tab, for a door that carries the
/// text in an encoding of its own, such as JSON, and wants nothing in it to
/// reorder, hide, or restyle what a reader is later shown. The stored
/// failure and [`said_cut`] keep the text raw; this escapes it once, at the
/// door (decision 0193). Text with nothing to escape is returned as it is.
#[must_use]
pub fn said_neutral(text: &str) -> Cow<'_, str> {
    escaped(text, |c| c == '\n' || c == '\t')
}

/// What the system said, cut for display from text that was already
/// redacted: its first `longest` characters, ending before a redaction mark
/// that the cut would split, so that no part of a mark, and nothing a mark
/// stands for, is shown as if it were text. The text stays raw: nothing in
/// it is escaped here; a door escapes it once, when it shows it, with
/// [`said_shown`], [`said_quoted`], or [`said_neutral`]. The `bool` says
/// whether it was cut.
#[must_use]
pub fn said_cut(redacted: &str, longest: usize) -> (&str, bool) {
    let Some((mut at, _)) = redacted.char_indices().nth(longest) else {
        return (redacted, false);
    };
    let mark = crate::redact::REDACTED;
    for (start, _) in redacted.match_indices(mark) {
        if start < at && at < start + mark.len() {
            at = start;
        }
    }
    (&redacted[..at], true)
}

/// What the system said, for a door that shows it between straight quotes
/// on a line of its own, from text that was already redacted: cut as
/// [`said_cut`] cuts it, with `…` after a cut; every character
/// [`said_hides`] names escaped (`\n`, `\u{1b}`, `\u{202e}`), and every
/// quote and backslash escaped (`\"`, `\\`), each once. The door puts it
/// between its quotes as it is (decision 0193).
#[must_use]
pub fn said_quoted(redacted: &str, longest: usize) -> String {
    let (kept, cut) = said_cut(redacted, longest);
    let mut shown = String::with_capacity(kept.len() + 8);
    for c in kept.chars() {
        if said_hides(c) && !c.is_control() {
            shown.extend(c.escape_unicode());
        } else if c.is_control() || c == '"' || c == '\\' {
            shown.extend(c.escape_default());
        } else {
            shown.push(c);
        }
    }
    if cut {
        shown.push('…');
    }
    shown
}

/// What the system said, as a door shows it on a line of its own, from
/// text that was already redacted: cut as [`said_cut`] cuts it, with `…`
/// after a cut, and every character [`said_hides`] names escaped (`\n`,
/// `\u{1b}`, `\u{202e}`), a line feed and a tab too, never removed, so
/// that the text cannot move a cursor, rewrite a line, reverse its order,
/// or hide a part, and a line break stays visible. The `bool` says whether
/// it was cut.
#[must_use]
pub fn said_shown(redacted: &str, longest: usize) -> (String, bool) {
    let (kept, cut) = said_cut(redacted, longest);
    let mut shown = escaped(kept, |_| false).into_owned();
    if cut {
        shown.push('…');
    }
    (shown, cut)
}

impl Failure {
    /// Makes a failure.
    #[must_use]
    pub fn new(
        class: FailureClass,
        code: FailureCode,
        message: impl Into<String>,
        next_step: impl Into<String>,
    ) -> Self {
        Self {
            class,
            code,
            message: message.into(),
            next_step: next_step.into(),
            extra: None,
        }
    }

    /// Changes the parts most failures leave empty, and keeps `extra` at
    /// `None` when they are all empty.
    fn change(&mut self, edit: impl FnOnce(&mut Extra)) {
        let mut extra = self.extra.take().unwrap_or_default();
        edit(&mut extra);
        if !extra.is_empty() {
            self.extra = Some(extra);
        }
    }

    /// The same failure with what the system itself said: a validation
    /// message, a constraint's name. The text is kept whole and apart from
    /// the sentence, up to [`SAID_CEILING`], above which it is dropped whole
    /// for [`SAID_TOO_LONG`]; it is untrusted data, never an instruction.
    /// The redactor that passes over the failure sees all of it; a door cuts
    /// it for display only after that, with [`said_shown`]. Text that holds
    /// nothing but white space takes away any text the failure held.
    #[must_use]
    pub fn with_said(mut self, said: impl AsRef<str>) -> Self {
        let said = kept_said(said.as_ref());
        self.change(|extra| extra.said = said);
        self
    }

    /// The same failure carrying what `from` says the system said, when this
    /// one says nothing itself. For a failure that stands in for another.
    #[must_use]
    pub fn carrying_said(mut self, from: &Self) -> Self {
        if self.said().is_none() {
            let said = from.said().map(str::to_owned);
            self.change(|extra| extra.said = said);
        }
        self
    }

    /// What the system itself said, when a driver kept it. Untrusted: show
    /// it only marked as the system's words, and never act on it.
    #[must_use]
    pub fn said(&self) -> Option<&str> {
        self.extra.as_ref()?.said.as_deref()
    }

    /// The same failure naming `action`, with `args`, as the way out. It is
    /// kept as data beside the next step, which still says it in words.
    /// Whoever passes a failure to a caller decides whether to show it: an
    /// action that does not exist, or arguments that do not fit it, are
    /// dropped there.
    #[must_use]
    pub fn with_next(mut self, action: impl Into<String>, args: serde_json::Value) -> Self {
        let next = NextAction {
            action: action.into(),
            args,
        };
        self.change(|extra| extra.next = Some(next));
        self
    }

    /// The action named as the way out, when the failure names one.
    #[must_use]
    pub fn next(&self) -> Option<&NextAction> {
        self.extra.as_ref()?.next.as_ref()
    }

    /// The same failure with the earliest moment worth calling again, as a
    /// wait from when the failure was made, in whole milliseconds rounded
    /// up. A failure stored and read later says the same wait: it counts
    /// from when the failure was made, not from when it is read.
    #[must_use]
    pub fn with_retry_after(mut self, wait: Duration) -> Self {
        let whole = u64::try_from(wait.as_millis()).unwrap_or(u64::MAX);
        let ms = if wait.subsec_nanos().is_multiple_of(1_000_000) {
            whole
        } else {
            whole.saturating_add(1)
        };
        self.change(|extra| extra.retry_after_ms = Some(ms));
        self
    }

    /// How long to wait, in milliseconds from when the failure was made,
    /// before the same call is worth making again, when the failure knows.
    #[must_use]
    pub fn retry_after_ms(&self) -> Option<u64> {
        self.extra.as_ref()?.retry_after_ms
    }

    /// The class, which alone decides what happens next.
    #[must_use]
    pub fn class(&self) -> FailureClass {
        self.class
    }

    /// The stable code within the class.
    #[must_use]
    pub fn code(&self) -> &FailureCode {
        &self.code
    }

    /// What went wrong, in one plain sentence.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// What the caller should do next.
    #[must_use]
    pub fn next_step(&self) -> &str {
        &self.next_step
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}: {}", self.class, self.code, self.message)
    }
}

impl std::error::Error for Failure {}

impl From<UnknownName> for Failure {
    fn from(error: UnknownName) -> Self {
        let next_step = format!("Use one of: {}.", error.known().join(", "));
        Self::new(
            FailureClass::Input,
            FailureCode::SCHEMA_INVALID,
            error.to_string(),
            next_step,
        )
    }
}

impl From<InvalidDigest> for Failure {
    fn from(error: InvalidDigest) -> Self {
        Self::new(
            FailureClass::Input,
            FailureCode::SCHEMA_INVALID,
            error.to_string(),
            "Write the digest as 64 lowercase hexadecimal characters.",
        )
    }
}

impl From<InvalidName> for Failure {
    fn from(error: InvalidName) -> Self {
        let next_step = error.next_step();
        Self::new(
            FailureClass::Input,
            FailureCode::SCHEMA_INVALID,
            error.to_string(),
            next_step,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Failure, FailureClass, FailureCode};
    use crate::{Mode, RunState};

    #[test]
    fn a_failure_carries_its_four_parts() {
        let failure = Failure::new(
            FailureClass::SessionExpired,
            FailureCode::new("needs_login").unwrap(),
            "The session for this target has ended.",
            "Sign in again, then resume the run.",
        );
        assert_eq!(failure.class(), FailureClass::SessionExpired);
        assert_eq!(failure.code().as_str(), "needs_login");
        assert_eq!(
            failure.to_string(),
            "session_expired/needs_login: The session for this target has ended."
        );
        assert_eq!(failure.next_step(), "Sign in again, then resume the run.");
    }

    #[test]
    fn a_failure_is_read_and_written_by_its_names() {
        let failure = Failure::new(
            FailureClass::Busy,
            FailureCode::new("record_locked").unwrap(),
            "Another run holds this record.",
            "Try again in a minute.",
        );
        let json = serde_json::to_value(&failure).unwrap();
        assert_eq!(json["class"], "busy");
        assert_eq!(json["code"], "record_locked");
        assert_eq!(serde_json::from_value::<Failure>(json).unwrap(), failure);
    }

    #[test]
    fn a_badly_formed_code_is_refused() {
        for bad in ["", "Needs_Login", "needs-login", "9lives", &"c".repeat(65)] {
            assert!(FailureCode::new(bad).is_err(), "{bad}");
        }
        assert!(serde_json::from_str::<FailureCode>("\"NOT A CODE\"").is_err());
    }

    #[test]
    fn a_fixed_code_equals_the_same_code_made_at_run_time() {
        assert_eq!(
            FailureCode::SCHEMA_INVALID,
            FailureCode::new("schema_invalid").unwrap()
        );
    }

    #[test]
    fn an_unknown_word_becomes_an_input_failure_that_lists_the_known_ones() {
        let failure: Failure = "paused".parse::<RunState>().unwrap_err().into();
        assert_eq!(failure.class(), FailureClass::Input);
        assert_eq!(failure.code(), &FailureCode::SCHEMA_INVALID);
        assert!(
            failure.message().contains("`paused` is not a run state"),
            "{failure}"
        );
        assert!(
            failure.next_step().contains("needs_approval"),
            "{}",
            failure.next_step()
        );
    }

    /// Decision 0193, finding C-2: no character of a class that can
    /// reorder, hide, or restyle shown text passes a door as itself.
    #[test]
    fn every_character_that_can_reorder_hide_or_restyle_text_is_escaped_for_a_door() {
        let classes: &[(&str, &[char])] = &[
            (
                "bidirectional marks, embeddings, and overrides",
                &[
                    '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}',
                    '\u{202e}', '\u{061c}',
                ],
            ),
            (
                "bidirectional isolates",
                &['\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}'],
            ),
            (
                "zero-width and other invisible format characters",
                &[
                    '\u{200b}', '\u{200c}', '\u{200d}', '\u{2060}', '\u{2064}', '\u{feff}',
                    '\u{00ad}', '\u{180e}', '\u{fff9}', '\u{3164}',
                ],
            ),
            (
                "line and paragraph separators",
                &['\u{2028}', '\u{2029}', '\u{0085}'],
            ),
            (
                "variation selectors and tags",
                &['\u{fe0f}', '\u{e0041}', '\u{e0100}'],
            ),
            (
                "terminal escape introducers",
                &[
                    '\u{1b}', '\u{9b}', '\u{9d}', '\u{90}', '\u{7}', '\r', '\u{8}',
                ],
            ),
        ];
        for (class, chars) in classes {
            for &c in *chars {
                assert!(super::said_hides(c), "{class}: {:?}", c);
                let text = format!("total{c}must be positive");
                // Carried raw, escaped once where it is shown.
                let (carried, cut) = super::said_cut(&text, super::SAID_LONGEST);
                assert!(!cut);
                assert_eq!(carried, text);
                let (shown, _) = super::said_shown(&text, super::SAID_LONGEST);
                assert!(!shown.contains(c), "{class}: {c:?} passed `said_shown`");
                let quoted = super::said_quoted(&text, super::SAID_LONGEST);
                assert!(!quoted.contains(c), "{class}: {c:?} passed `said_quoted`");
                assert!(
                    quoted.contains(&c.escape_unicode().to_string())
                        || quoted.contains(&c.escape_default().to_string()),
                    "{class}: {quoted}"
                );
                assert!(!super::said_neutral(&text).contains(c), "{class}");
            }
        }
        // Ordinary text, letters of every script, and emoji pass as they are.
        for plain in [
            "total must be positive",
            "Größe ≤ 12 mm",
            "عدد",
            "数量",
            "ok 👍",
        ] {
            assert!(plain.chars().all(|c| !super::said_hides(c)), "{plain}");
            assert_eq!(super::said_neutral(plain), plain);
            assert_eq!(super::said_shown(plain, super::SAID_LONGEST).0, plain);
        }
        // A door's data keeps a line feed and a tab, which its encoding
        // escapes; the line a command prints shows them escaped.
        assert_eq!(super::said_neutral("a\nb\tc\u{202e}"), "a\nb\tc\\u{202e}");
        assert_eq!(super::said_shown("a\nb\tc", 400).0, "a\\nb\\tc");
        // Quotes and backslashes, for the quoted line, once each.
        assert_eq!(
            super::said_quoted("say \"no\" \\ \u{1b}[31m\u{2066}", 400),
            "say \\\"no\\\" \\\\ \\u{1b}[31m\\u{2066}"
        );
        // The cut is made before the escape: an escape is never split.
        assert_eq!(super::said_quoted("ab\u{202e}cd", 3), "ab\\u{202e}…");
    }

    /// A failure that arrives as data is held to the bound `with_said` keeps.
    #[test]
    fn a_failure_read_as_data_keeps_no_more_of_what_the_system_said_than_one_made_here() {
        let failure = Failure::new(
            FailureClass::Rejected,
            FailureCode::new("system_refused").unwrap(),
            "The system refused the record.",
            "Correct the record and call again.",
        );
        let mut json = serde_json::to_value(&failure).unwrap();
        json["said"] = serde_json::Value::String("x".repeat(super::SAID_CEILING + 1));
        let read: Failure = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(read.said(), Some(super::SAID_TOO_LONG));
        json["said"] = serde_json::Value::String(" \n ".to_owned());
        assert_eq!(
            serde_json::from_value::<Failure>(json).unwrap().said(),
            None
        );
        let kept = failure.with_said("total must be positive");
        let json = serde_json::to_value(&kept).unwrap();
        assert_eq!(serde_json::from_value::<Failure>(json).unwrap(), kept);
    }

    #[test]
    fn a_failure_keeps_its_next_through_serde() {
        let plain = Failure::new(
            FailureClass::Busy,
            FailureCode::new("queue_full").unwrap(),
            "The queue is full.",
            "Call again in a moment.",
        );
        // Absent parts are not written, so a failure without them reads as
        // it did before they existed.
        let json = serde_json::to_value(&plain).unwrap();
        assert_eq!(
            json.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["class", "code", "message", "next_step"]
        );
        let full = plain
            .clone()
            .with_next("target.status", serde_json::json!({"target": "example"}))
            .with_retry_after(std::time::Duration::from_micros(1500))
            .with_said("queue at its limit");
        assert_eq!(full.next().unwrap().action, "target.status");
        assert_eq!(full.next().unwrap().args["target"], "example");
        // Rounded up: a wait is never shorter than the one given.
        assert_eq!(full.retry_after_ms(), Some(2));
        let json = serde_json::to_value(&full).unwrap();
        assert_eq!(json["next"]["action"], "target.status");
        assert_eq!(json["retry_after_ms"], 2);
        assert_eq!(serde_json::from_value::<Failure>(json).unwrap(), full);
        // A part set and then emptied leaves a failure equal to one never given it.
        assert_eq!(plain.clone().with_said(" "), plain);
        assert_eq!(plain.next(), None);
        assert_eq!(plain.retry_after_ms(), None);
    }

    #[test]
    fn a_failure_is_no_larger_for_the_parts_most_failures_leave_empty() {
        // Class, code, message, next step, and one pointer: the errors that
        // hold a failure stay under clippy's limit for a large error.
        assert!(
            std::mem::size_of::<Failure>() <= 11 * std::mem::size_of::<usize>(),
            "{}",
            std::mem::size_of::<Failure>()
        );
    }

    #[test]
    fn a_badly_formed_name_becomes_an_input_failure() {
        let failure: Failure = Mode::new("Page HTTP").unwrap_err().into();
        assert_eq!(failure.class(), FailureClass::Input);
        assert_eq!(failure.code(), &FailureCode::SCHEMA_INVALID);
    }
}
