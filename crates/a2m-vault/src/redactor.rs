//! The redactor: every registered secret, in every form text carries it in,
//! replaced by `[redacted]`.
//!
//! A secret is matched exactly, and also where it was percent-encoded,
//! JSON-escaped, written with character references, written out in
//! hexadecimal, or base64-encoded on the way. Escapes are found by decoding
//! the text and searching what it decodes to, so every spelling of an escape
//! is covered. Base64 is found by searching for what the secret's bytes
//! encode to at each of the three positions they can fall in, also across
//! the line breaks that mail and key files put into it. A secret whose form
//! is case-insensitive is found in any letter case.
//!
//! [`Redact::redact`] sees one text at a time and keeps nothing between two
//! calls. A text that arrives in pieces goes through a [`RedactStream`],
//! which holds back the end of what it was given until it is sure no secret
//! lies across it.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fmt;
use std::sync::{Arc, PoisonError, RwLock};

use a2m_core::{REDACTED, Redact, Secret};
use aho_corasick::{AhoCorasick, Input, MatchKind};
use zeroize::Zeroizing;

use crate::hex::nibble;

/// The fewest characters an encoded form of a secret, in base64 or in
/// hexadecimal, must have to be searched for. A shorter run would match in
/// unrelated text everywhere.
const SHORTEST_ENCODED_FORM: usize = 4;
/// How many times the result is searched again after masking, for a secret
/// that only appears once another is replaced.
const ROUNDS: usize = 4;
/// The longest stretch of text that decodes to one character in any view
/// that is searched: a character outside the basic plane, JSON-escaped as
/// two `\u` escapes, with every character of those escaped again. A stream
/// treats this much at the end of what it holds as not yet settled, because
/// an escape may still be arriving.
const LONGEST_ESCAPE: usize = 72;

const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL_SAFE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// The redactor. It holds the registered secrets in memory that is wiped on
/// drop, and masks every one of them in any text.
///
/// A clone shares the same registered set, and costs one reference count.
/// It is safe to use from any thread. Registration lasts for the life of the
/// process, so that a line written after a session ends is still masked.
#[derive(Clone)]
pub struct Redactor {
    state: Arc<RwLock<State>>,
}

#[derive(Default)]
struct State {
    /// Every form that is searched for, so that a form is searched for once
    /// and registering a secret again costs a lookup: a secret is registered
    /// when the form that is the secret itself is here. The set shares the
    /// forms with `patterns`, in the same wiped memory. No digest of a
    /// secret is kept: a digest of a short secret can be searched, and
    /// would outlive the secret in memory that is never wiped.
    seen: BTreeSet<Arc<Pattern>>,
    registered: usize,
    short: usize,
    patterns: Vec<Arc<Pattern>>,
    /// Built from `patterns` by the first search after a registration, so
    /// that registering many secrets at once builds it once.
    matcher: Option<Arc<Matcher>>,
}

/// The state of the redactor's registered set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedactorHealth {
    /// How many secrets are registered.
    pub registered: usize,
    /// How many of them are shorter than [`Redactor::SHORT`] bytes. Each is
    /// masked wherever its characters appear, so it may mask unrelated text.
    pub short: usize,
    /// The size the set is expected to stay under.
    pub bound: usize,
}

impl RedactorHealth {
    /// Whether more secrets are registered than the bound. Nothing is ever
    /// dropped to stay under it; `doctor` reports it instead.
    #[must_use]
    pub const fn over_bound(&self) -> bool {
        self.registered > self.bound
    }
}

impl Redactor {
    /// The number of secrets the registered set is expected to stay under.
    pub const BOUND: usize = 4096;
    /// A secret shorter than this many bytes is counted as short in
    /// [`RedactorHealth`]. It is still registered and masked.
    pub const SHORT: usize = 8;

    /// A redactor with nothing registered.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(State::default())),
        }
    }

    /// Registers a secret. From here on it is masked in every text, in each
    /// form text carries it in.
    ///
    /// Every secret that is not empty is registered, however short: a short
    /// secret may then mask unrelated text, which is the safer error. An
    /// empty secret has nothing to mask and is ignored. Registering a secret
    /// again changes nothing.
    ///
    /// A secret written only in hexadecimal digits, with or without hyphens
    /// or colons between them, is matched in any letter case, because
    /// systems print hexadecimal in either. Any other secret is matched as
    /// it is written; [`Redactor::register_folded`] is for one whose form
    /// is known to be case-insensitive.
    pub fn register(&self, secret: &Secret) {
        self.add(secret, false);
    }

    /// Registers a secret whose form is case-insensitive, such as a
    /// base32 key or a name a system may print in another case. It is
    /// masked as [`Redactor::register`] masks it, and also where its
    /// letters are written in any other case. Only the secret as it is
    /// written is matched in another case, not its encoded forms.
    pub fn register_folded(&self, secret: &Secret) {
        self.add(secret, true);
    }

    fn add(&self, secret: &Secret, folded: bool) {
        let value = secret.expose().as_bytes();
        if value.is_empty() {
            return;
        }
        let itself = Pattern::new(Kind::Itself, value.to_vec());
        let fold = (folded || hexadecimal(value))
            .then(|| Pattern::new(Kind::Folded, value.to_ascii_lowercase()));
        {
            let state = self.state.read().unwrap_or_else(PoisonError::into_inner);
            let known = state.seen.contains(&itself)
                && fold.as_ref().is_none_or(|fold| state.seen.contains(fold));
            if known {
                return;
            }
        }
        let mut state = self.state.write().unwrap_or_else(PoisonError::into_inner);
        let mut new = fold.into_iter().collect::<Vec<_>>();
        if !state.seen.contains(&itself) {
            state.registered += 1;
            if value.len() < Self::SHORT {
                state.short += 1;
            }
            new.push(itself);
            new.extend(encoded_forms(value));
        }
        for form in new {
            let form = Arc::new(form);
            // A form of this secret may equal a form registered before.
            if state.seen.insert(Arc::clone(&form)) {
                state.patterns.push(form);
                state.matcher = None;
            }
        }
    }

    /// A stream over this redactor's registered set, for a text that
    /// arrives in pieces: a body read in chunks, a program's output, a log
    /// that is followed. A secret that lies across two pieces is masked,
    /// which redacting each piece on its own cannot do.
    #[must_use]
    pub fn stream(&self) -> RedactStream {
        RedactStream {
            redactor: self.clone(),
            held: String::new(),
        }
    }

    /// The registered set against its bound.
    #[must_use]
    pub fn health(&self) -> RedactorHealth {
        let state = self.state.read().unwrap_or_else(PoisonError::into_inner);
        RedactorHealth {
            registered: state.registered,
            short: state.short,
            bound: Self::BOUND,
        }
    }

    /// The matcher over everything registered, or `None` when nothing is.
    fn matcher(&self) -> Option<Arc<Matcher>> {
        {
            let state = self.state.read().unwrap_or_else(PoisonError::into_inner);
            if state.patterns.is_empty() {
                return None;
            }
            if let Some(matcher) = &state.matcher {
                return Some(Arc::clone(matcher));
            }
        }
        let mut state = self.state.write().unwrap_or_else(PoisonError::into_inner);
        if let Some(matcher) = &state.matcher {
            return Some(Arc::clone(matcher));
        }
        let matcher = Arc::new(Matcher::new(state.patterns.clone()));
        state.matcher = Some(Arc::clone(&matcher));
        Some(matcher)
    }
}

impl Default for Redactor {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Redactor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Redactor")
            .field("health", &self.health())
            .finish()
    }
}

impl Redact for Redactor {
    fn redact(&self, text: &str) -> String {
        let Some(matcher) = self.matcher() else {
            return text.to_owned();
        };
        let mut current = Cow::Borrowed(text);
        for _ in 0..ROUNDS {
            let spans = matcher.spans(&current, false);
            if spans.is_empty() {
                return current.into_owned();
            }
            current = Cow::Owned(mask(&current, spans));
        }
        if matcher.spans(&current, true).is_empty() {
            current.into_owned()
        } else {
            // Masking keeps uncovering a secret. Give nothing of the text.
            REDACTED.to_owned()
        }
    }

    fn contains_secret(&self, text: &str) -> bool {
        self.matcher()
            .is_some_and(|matcher| !matcher.spans(text, true).is_empty())
    }
}

/// A text that arrives in pieces, redacted as it goes.
///
/// Each piece is given to [`RedactStream::push`], which returns what can be
/// let out so far, and [`RedactStream::finish`] returns the rest. Joined in
/// order, what they return is the whole text with every registered secret
/// masked, wherever the pieces were divided.
///
/// The stream holds back the end of what it was given: as much text as the
/// longest registered form could still begin in, and a little more for an
/// escape that may still be arriving. So what `push` returns runs behind
/// what it was given by about the length of the longest secret, and a
/// reader that needs every byte at once cannot have it redacted.
pub struct RedactStream {
    redactor: Redactor,
    /// The end of the text so far, not yet let out.
    held: String,
}

impl RedactStream {
    /// Takes the next piece and returns the text that can be let out now,
    /// redacted. It may be empty.
    pub fn push(&mut self, piece: &str) -> String {
        self.held.push_str(piece);
        let Some(matcher) = self.redactor.matcher() else {
            return std::mem::take(&mut self.held);
        };
        let cut = matcher.safe_cut(&self.held);
        let rest = self.held.split_off(cut);
        let settled = std::mem::replace(&mut self.held, rest);
        self.redactor.redact(&settled)
    }

    /// Ends the stream and returns what was held back, redacted.
    #[must_use]
    pub fn finish(self) -> String {
        self.redactor.redact(&self.held)
    }
}

impl fmt::Debug for RedactStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedactStream")
            .field("held", &self.held.len())
            .finish_non_exhaustive()
    }
}

/// What a form of a secret is, which decides where it is searched for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    /// The secret itself.
    Itself,
    /// The secret with each space written as a plus sign.
    Plus,
    /// The secret's bytes written out in hexadecimal.
    Hex,
    /// The secret's bytes in base64. The character on each side of a match
    /// holds bits of the secret too, so the mask takes them with it.
    Base64,
    /// The secret in lower case, searched for in the text in lower case.
    Folded,
}

/// One form of one secret, in wiped memory.
struct Pattern {
    kind: Kind,
    bytes: Zeroizing<Vec<u8>>,
}

impl Pattern {
    fn new(kind: Kind, bytes: Vec<u8>) -> Self {
        Self {
            kind,
            bytes: Zeroizing::new(bytes),
        }
    }
}

impl PartialEq for Pattern {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Pattern {}

impl PartialOrd for Pattern {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Pattern {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.kind, self.bytes.as_slice()).cmp(&(other.kind, other.bytes.as_slice()))
    }
}

/// Whether a secret is written only in hexadecimal digits, with or without
/// hyphens or colons between them, and holds a letter: the form systems
/// print in either letter case.
fn hexadecimal(value: &[u8]) -> bool {
    value
        .iter()
        .all(|byte| byte.is_ascii_hexdigit() || matches!(byte, b'-' | b':'))
        && value.iter().any(u8::is_ascii_alphabetic)
}

/// The encoded forms a secret is searched for in, beside itself: itself with
/// each space written as a plus sign as a form submission writes it, its
/// bytes in hexadecimal in each letter case, and its base64 encodings.
fn encoded_forms(value: &[u8]) -> Vec<Pattern> {
    let mut forms = Vec::new();
    if value.contains(&b' ') {
        let plus = value
            .iter()
            .map(|&byte| if byte == b' ' { b'+' } else { byte });
        forms.push(Pattern::new(Kind::Plus, plus.collect()));
    }
    if value.len() * 2 >= SHORTEST_ENCODED_FORM {
        let lower = Zeroizing::new(crate::hex::encode(value));
        let upper = Zeroizing::new(lower.to_ascii_uppercase());
        forms.push(Pattern::new(Kind::Hex, lower.as_bytes().to_vec()));
        if *upper != *lower {
            forms.push(Pattern::new(Kind::Hex, upper.as_bytes().to_vec()));
        }
    }
    for offset in 0..3 {
        // The secret's bits start `offset` bytes into a group of three. Only
        // the characters made wholly of its bits are the same wherever it
        // stands; those are searched for.
        let first = (offset * 8usize).div_ceil(6);
        let end = (offset + value.len()) * 8 / 6;
        if end < first + SHORTEST_ENCODED_FORM {
            continue;
        }
        let sextets: Zeroizing<Vec<u8>> =
            Zeroizing::new((first..end).map(|at| sextet(value, offset, at)).collect());
        for alphabet in [STANDARD, URL_SAFE] {
            let form = Pattern::new(
                Kind::Base64,
                sextets
                    .iter()
                    .map(|&six| alphabet[usize::from(six)])
                    .collect(),
            );
            if !forms.contains(&form) {
                forms.push(form);
            }
        }
    }
    forms
}

/// The six bits of base64 character `at`, when `value` starts `offset` bytes
/// into the encoded stream.
fn sextet(value: &[u8], offset: usize, at: usize) -> u8 {
    let bit = at * 6;
    let byte = |index: usize| {
        index
            .checked_sub(offset)
            .and_then(|index| value.get(index))
            .copied()
            .unwrap_or(0)
    };
    let pair = (u16::from(byte(bit / 8)) << 8) | u16::from(byte(bit / 8 + 1));
    ((pair >> (10 - bit % 8)) & 0x3f) as u8
}

/// Every pattern, and the automaton that finds them all in one pass.
struct Matcher {
    patterns: Vec<Arc<Pattern>>,
    /// `None` when the automaton could not be built; each pattern is then
    /// searched for in turn, which is slower and finds the same matches.
    automaton: Option<AhoCorasick>,
    /// The length of the longest pattern, in bytes.
    longest: usize,
    /// Whether any pattern is matched in any letter case.
    folds: bool,
}

impl Matcher {
    fn new(patterns: Vec<Arc<Pattern>>) -> Self {
        let automaton = AhoCorasick::builder()
            .match_kind(MatchKind::Standard)
            .build(patterns.iter().map(|pattern| pattern.bytes.as_slice()))
            .ok();
        Self {
            longest: patterns
                .iter()
                .map(|pattern| pattern.bytes.len())
                .max()
                .unwrap_or(0),
            folds: patterns.iter().any(|pattern| pattern.kind == Kind::Folded),
            patterns,
            automaton,
        }
    }

    /// Calls `found` with the pattern and place of every match in `bytes`,
    /// overlapping ones included, until `found` returns false.
    fn each_match(&self, bytes: &[u8], mut found: impl FnMut(&Pattern, usize, usize) -> bool) {
        let overlapping = self
            .automaton
            .as_ref()
            .and_then(|automaton| automaton.try_find_overlapping_iter(Input::new(bytes)).ok());
        if let Some(matches) = overlapping {
            for one in matches {
                let Some(pattern) = self.patterns.get(one.pattern().as_usize()) else {
                    continue;
                };
                if !found(pattern, one.start(), one.end()) {
                    return;
                }
            }
            return;
        }
        for pattern in &self.patterns {
            let length = pattern.bytes.len();
            for (start, window) in bytes.windows(length).enumerate() {
                if window == pattern.bytes.as_slice() && !found(pattern, start, start + length) {
                    return;
                }
            }
        }
    }

    /// Shows `visit` every view of `text` that is searched, until `visit`
    /// returns true: the text as it is, and as it decodes.
    ///
    /// Percent-encoding and JSON escapes are decoded once and twice, and
    /// each of the two inside the other where that is common: a
    /// percent-encoded value in JSON needs no decoding of the JSON, and JSON
    /// carried in a percent-encoded value does. The text, and each of the
    /// two decoded once, is also seen without the line breaks that stand
    /// inside base64. The text is seen in lower case when a form is matched
    /// in any letter case, and with its character references decoded.
    ///
    /// With `open_ended` the text is the head of a longer one: white space
    /// at its end is taken as standing inside base64 that goes on.
    fn each_view(&self, text: &str, open_ended: bool, mut visit: impl FnMut(&View<'_>) -> bool) {
        fn with_line_breaks_out(
            view: &View<'_>,
            open_ended: bool,
            visit: &mut impl FnMut(&View<'_>) -> bool,
        ) -> bool {
            visit(view)
                || view
                    .squeezed(open_ended)
                    .is_some_and(|squeezed| visit(&squeezed))
        }

        let raw = View::of(text);
        if with_line_breaks_out(&raw, open_ended, &mut visit) {
            return;
        }
        if self.folds && raw.bytes.iter().any(u8::is_ascii_uppercase) && visit(&raw.lowered()) {
            return;
        }
        if raw.bytes.contains(&b'%') {
            let percent = raw.percent_decoded();
            if with_line_breaks_out(&percent, open_ended, &mut visit) {
                return;
            }
            if percent.bytes.contains(&b'%') && visit(&percent.percent_decoded()) {
                return;
            }
            if percent.bytes.contains(&b'\\') && visit(&percent.json_unescaped()) {
                return;
            }
        }
        if raw.bytes.contains(&b'\\') {
            let json = raw.json_unescaped();
            if with_line_breaks_out(&json, open_ended, &mut visit) {
                return;
            }
            if json.bytes.contains(&b'\\') && visit(&json.json_unescaped()) {
                return;
            }
        }
        if raw.bytes.contains(&b'&')
            && let Some(references) = raw.references_decoded()
        {
            visit(&references);
        }
    }

    /// The places in `text` to mask, as byte ranges, in no order. With
    /// `first_only` the search stops at the first one.
    fn spans(&self, text: &str, first_only: bool) -> Vec<(usize, usize)> {
        let mut spans = Vec::new();
        let mut masks = None;
        self.each_view(text, false, |view| {
            self.each_match(&view.bytes, |pattern, start, end| {
                if !view.counts.counts(pattern.kind) {
                    return true;
                }
                let (start, end) = if pattern.kind == Kind::Base64 {
                    widen(&view.bytes, start, end)
                } else {
                    (start, end)
                };
                let span = view.span(start, end);
                // The mask is not a secret. A match that lies inside one is
                // left, or masked text would never come out clean.
                let masks = masks.get_or_insert_with(|| mask_starts(text));
                if inside_a_mask(masks, span) {
                    return true;
                }
                spans.push(span);
                !first_only
            });
            first_only && !spans.is_empty()
        });
        spans
    }

    /// How much of `text`, the head of a longer text that is still arriving,
    /// can be redacted and let out now: the length of a prefix that no
    /// secret lies across, whatever follows.
    ///
    /// In every view, the bytes that came from the last
    /// [`LONGEST_ESCAPE`] bytes of the text are not settled: an escape may
    /// still be arriving. A secret that is not yet whole begins within the
    /// length of the longest form before them, and one more byte, for the
    /// neighbour a base64 match takes with it. Everything before that is
    /// settled in that view. The cut is the earliest such place over all
    /// views, moved back to the start of any secret already found that
    /// lies across it or ends at it.
    fn safe_cut(&self, text: &str) -> usize {
        let settled_until = text.len().saturating_sub(LONGEST_ESCAPE);
        let mut cut = text.len();
        self.each_view(text, true, |view| {
            let settled = view.settled(settled_until);
            let from = settled.saturating_sub(self.longest.saturating_add(1));
            cut = cut.min(view.start(from));
            false
        });
        let spans = self.spans(text, false);
        loop {
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            let across = spans
                .iter()
                .filter(|(start, end)| *start < cut && cut <= *end)
                .map(|(start, _)| *start)
                .min();
            match across {
                Some(start) => cut = start,
                None => return cut,
            }
        }
    }
}

/// Takes one base64 character on each side into a match, and the padding
/// after it: the neighbours hold the secret's remaining bits.
fn widen(bytes: &[u8], mut start: usize, mut end: usize) -> (usize, usize) {
    if start > 0 && bytes.get(start - 1).is_some_and(is_base64) {
        start -= 1;
    }
    if bytes.get(end).is_some_and(is_base64) {
        end += 1;
    }
    for _ in 0..2 {
        if bytes.get(end) == Some(&b'=') {
            end += 1;
        }
    }
    (start, end)
}

/// A character of either base64 alphabet.
fn is_base64(byte: &u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'-' | b'_')
}

/// White space as it stands inside wrapped base64: a line break, and the
/// spaces and tabs a continuation line begins with.
fn is_space(byte: &u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

/// Where each mask already in `text` starts.
fn mask_starts(text: &str) -> Vec<usize> {
    text.match_indices(REDACTED).map(|(at, _)| at).collect()
}

/// Whether `span` lies wholly inside one of the masks.
fn inside_a_mask(masks: &[usize], (start, end): (usize, usize)) -> bool {
    let after = masks.partition_point(|&mask| mask <= start);
    after
        .checked_sub(1)
        .and_then(|index| masks.get(index))
        .is_some_and(|&mask| end <= mask + REDACTED.len())
}

/// `text` with each span replaced by the mask. Spans that overlap or touch
/// become one mask.
fn mask(text: &str, mut spans: Vec<(usize, usize)>) -> String {
    spans.sort_unstable();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut spans = spans.into_iter().peekable();
    while let Some((mut start, mut end)) = spans.next() {
        while let Some(&(next_start, next_end)) = spans.peek() {
            if next_start > end {
                break;
            }
            end = end.max(next_end);
            spans.next();
        }
        // A span never splits a character; were one to, the whole character
        // is masked.
        while !text.is_char_boundary(start) {
            start -= 1;
        }
        while !text.is_char_boundary(end) {
            end += 1;
        }
        let start = start.max(copied);
        if let (Some(kept), true) = (text.get(copied..start), end > copied) {
            out.push_str(kept);
            out.push_str(REDACTED);
            copied = end;
        }
    }
    out.push_str(text.get(copied..).unwrap_or_default());
    out
}

/// Which forms count as a secret when they are found in a view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Counts {
    /// Every form. A form in lower case is then found where the text itself
    /// is in lower case.
    All,
    /// Base64 forms only: the view left line breaks out, which would
    /// otherwise join unrelated words into a match.
    Base64,
    /// Forms matched in any letter case only: the view is in lower case.
    Folded,
}

impl Counts {
    fn counts(self, kind: Kind) -> bool {
        match self {
            Self::All => true,
            Self::Base64 => kind == Kind::Base64,
            Self::Folded => kind == Kind::Folded,
        }
    }
}

/// Text as it is, or as it decodes, with the place in the original that each
/// byte came from.
struct View<'a> {
    bytes: Cow<'a, [u8]>,
    /// For a view whose bytes are not the original's: where in the original
    /// each byte's source starts. `None` when each byte stands where it
    /// stood.
    starts: Option<Vec<usize>>,
    source_length: usize,
    counts: Counts,
}

impl<'a> View<'a> {
    fn of(text: &'a str) -> Self {
        Self {
            bytes: Cow::Borrowed(text.as_bytes()),
            starts: None,
            source_length: text.len(),
            counts: Counts::All,
        }
    }

    /// How many of this view's bytes came from the original before
    /// `until`.
    fn settled(&self, until: usize) -> usize {
        match &self.starts {
            None => until.min(self.bytes.len()),
            Some(starts) => starts.partition_point(|&start| start < until),
        }
    }

    fn start(&self, at: usize) -> usize {
        match &self.starts {
            None => at,
            Some(starts) => starts.get(at).copied().unwrap_or(self.source_length),
        }
    }

    /// The range of the original that bytes `start..end` of this view came
    /// from. A byte that came from an escape takes the whole escape.
    fn span(&self, start: usize, end: usize) -> (usize, usize) {
        if self.starts.is_none() || end == 0 {
            return (start, end);
        }
        let last = self.start(end - 1);
        let mut after = end;
        while after < self.bytes.len() && self.start(after) == last {
            after += 1;
        }
        (self.start(start), self.start(after))
    }

    fn decoded(&self, bytes: Vec<u8>, starts: Vec<usize>) -> View<'static> {
        View {
            bytes: Cow::Owned(bytes),
            starts: Some(starts),
            source_length: self.source_length,
            counts: Counts::All,
        }
    }

    /// This view in lower case, in which only the forms matched in any
    /// letter case count.
    fn lowered(&self) -> View<'static> {
        View {
            bytes: Cow::Owned(self.bytes.to_ascii_lowercase()),
            starts: self.starts.clone(),
            source_length: self.source_length,
            counts: Counts::Folded,
        }
    }

    /// This view without the line breaks that stand inside base64: each run
    /// of white space that holds a line break and has a base64 character on
    /// both sides. Only base64 forms count in it. `None` when there is no
    /// such run.
    ///
    /// With `open_ended` the view is the head of a longer text, and a run of
    /// white space at its very end is left out too: a line break and more
    /// base64 may follow.
    fn squeezed(&self, open_ended: bool) -> Option<View<'static>> {
        let source = &self.bytes;
        let worth_a_look = source.iter().any(|byte| matches!(byte, b'\n' | b'\r'))
            || (open_ended && source.last().is_some_and(is_space));
        if !worth_a_look {
            return None;
        }
        let mut bytes = Vec::with_capacity(source.len());
        let mut starts = Vec::with_capacity(source.len());
        let mut left_out = false;
        let mut at = 0;
        while let Some(&byte) = source.get(at) {
            let run = source.get(at..).map_or(0, |rest| {
                rest.iter().take_while(|byte| is_space(byte)).count()
            });
            if run == 0 {
                bytes.push(byte);
                starts.push(self.start(at));
                at += 1;
                continue;
            }
            let space = source.get(at..at + run).unwrap_or_default();
            let after_base64 = at
                .checked_sub(1)
                .and_then(|before| source.get(before))
                .is_some_and(is_base64);
            let before_base64 = match source.get(at + run) {
                Some(next) => {
                    is_base64(next) && space.iter().any(|byte| matches!(byte, b'\n' | b'\r'))
                }
                None => open_ended,
            };
            if after_base64 && before_base64 {
                left_out = true;
            } else {
                for (offset, &byte) in space.iter().enumerate() {
                    bytes.push(byte);
                    starts.push(self.start(at + offset));
                }
            }
            at += run;
        }
        left_out.then(|| View {
            counts: Counts::Base64,
            ..self.decoded(bytes, starts)
        })
    }

    /// This view with every character reference of a page's source replaced
    /// by the character it stands for: the five named ones, and the numbered
    /// ones in decimal and hexadecimal. `None` when it holds none.
    fn references_decoded(&self) -> Option<View<'static>> {
        let source = &self.bytes;
        let mut bytes = Vec::with_capacity(source.len());
        let mut starts = Vec::with_capacity(source.len());
        let mut found = false;
        let mut at = 0;
        while let Some(&byte) = source.get(at) {
            let start = self.start(at);
            let reference = (byte == b'&')
                .then(|| character_reference(source.get(at + 1..)?))
                .flatten();
            match reference {
                Some((character, length)) => {
                    found = true;
                    let mut utf8 = [0u8; 4];
                    for &encoded in character.encode_utf8(&mut utf8).as_bytes() {
                        bytes.push(encoded);
                        starts.push(start);
                    }
                    at += 1 + length;
                }
                None => {
                    bytes.push(byte);
                    starts.push(start);
                    at += 1;
                }
            }
        }
        found.then(|| self.decoded(bytes, starts))
    }

    /// This view with every `%XX` replaced by the byte it stands for.
    fn percent_decoded(&self) -> View<'static> {
        let source = &self.bytes;
        let mut bytes = Vec::with_capacity(source.len());
        let mut starts = Vec::with_capacity(source.len());
        let mut at = 0;
        while let Some(&byte) = source.get(at) {
            let escaped = (byte == b'%')
                .then(|| hex_byte(source.get(at + 1..at + 3)?))
                .flatten();
            starts.push(self.start(at));
            match escaped {
                Some(decoded) => {
                    bytes.push(decoded);
                    at += 3;
                }
                None => {
                    bytes.push(byte);
                    at += 1;
                }
            }
        }
        self.decoded(bytes, starts)
    }

    /// This view with every JSON string escape replaced by what it stands
    /// for.
    fn json_unescaped(&self) -> View<'static> {
        let source = &self.bytes;
        let mut bytes = Vec::with_capacity(source.len());
        let mut starts = Vec::with_capacity(source.len());
        let mut at = 0;
        while let Some(&byte) = source.get(at) {
            let start = self.start(at);
            let escape = (byte == b'\\')
                .then(|| json_escape(source.get(at + 1..)?))
                .flatten();
            match escape {
                Some((character, length)) => {
                    let mut utf8 = [0u8; 4];
                    for &encoded in character.encode_utf8(&mut utf8).as_bytes() {
                        bytes.push(encoded);
                        starts.push(start);
                    }
                    at += 1 + length;
                }
                None => {
                    bytes.push(byte);
                    starts.push(start);
                    at += 1;
                }
            }
        }
        self.decoded(bytes, starts)
    }
}

/// The byte two hexadecimal characters spell.
fn hex_byte(pair: &[u8]) -> Option<u8> {
    match pair {
        [high, low] => Some((nibble(*high)? << 4) | nibble(*low)?),
        _ => None,
    }
}

/// The character a JSON escape stands for and how many bytes the escape has
/// after its backslash, or `None` when `after` does not continue an escape.
fn json_escape(after: &[u8]) -> Option<(char, usize)> {
    let simple = |character| Some((character, 1));
    match *after.first()? {
        b'"' => simple('"'),
        b'\\' => simple('\\'),
        b'/' => simple('/'),
        b'b' => simple('\u{8}'),
        b'f' => simple('\u{c}'),
        b'n' => simple('\n'),
        b'r' => simple('\r'),
        b't' => simple('\t'),
        b'u' => {
            let high = code_unit(after.get(1..5)?)?;
            if let Some(character) = char::from_u32(high) {
                return Some((character, 5));
            }
            // Half of a pair: the other half must follow as its own escape.
            if !(0xd800..0xdc00).contains(&high) || after.get(5..7)? != b"\\u" {
                return None;
            }
            let low = code_unit(after.get(7..11)?)?;
            if !(0xdc00..0xe000).contains(&low) {
                return None;
            }
            let character = char::from_u32(0x10000 + ((high - 0xd800) << 10) + (low - 0xdc00))?;
            Some((character, 11))
        }
        _ => None,
    }
}

/// The character a character reference stands for and how many bytes the
/// reference has after its ampersand, or `None` when `after` does not
/// continue one. The five named references every page's source uses are
/// known; any other name is left as it is.
fn character_reference(after: &[u8]) -> Option<(char, usize)> {
    // The longest reference to a character there is: `#x10FFFF`.
    let length = after.iter().take(9).position(|&byte| byte == b';')?;
    let character = match after.get(..length)? {
        b"amp" => '&',
        b"lt" => '<',
        b"gt" => '>',
        b"quot" => '"',
        b"apos" => '\'',
        [b'#', b'x' | b'X', digits @ ..] if !digits.is_empty() => {
            char::from_u32(digits.iter().try_fold(0u32, |sum, &digit| {
                Some((sum << 4) | u32::from(nibble(digit)?))
            })?)?
        }
        [b'#', digits @ ..] if !digits.is_empty() => {
            char::from_u32(digits.iter().try_fold(0u32, |sum, &digit| {
                let value = u32::from(digit.checked_sub(b'0').filter(|value| *value < 10)?);
                sum.checked_mul(10)?.checked_add(value)
            })?)?
        }
        _ => return None,
    };
    Some((character, length + 1))
}

/// The number four hexadecimal characters spell.
fn code_unit(four: &[u8]) -> Option<u32> {
    four.iter().try_fold(0u32, |sum, &digit| {
        Some((sum << 4) | u32::from(nibble(digit)?))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The secret itself and its encoded forms.
    fn forms(value: &[u8]) -> Vec<Pattern> {
        let mut forms = vec![Pattern::new(Kind::Itself, value.to_vec())];
        forms.extend(encoded_forms(value));
        forms
    }

    fn patterns(secrets: &[&str]) -> Vec<Arc<Pattern>> {
        secrets
            .iter()
            .flat_map(|secret| forms(secret.as_bytes()))
            .map(Arc::new)
            .collect()
    }

    #[test]
    fn the_search_without_the_automaton_finds_the_same_places() {
        let secrets = ["abcd-1234", "1234-wxyz", "p w\"d", "k"];
        let with = Matcher::new(patterns(&secrets));
        assert!(with.automaton.is_some());
        let without = Matcher {
            automaton: None,
            ..Matcher::new(patterns(&secrets))
        };
        for text in [
            "<abcd-1234-wxyz> k",
            "p+w%22d and p w\\\"d and cCB3ImQ=",
            "YWJjZC0xMjM0 xYWJjZC0xMjM0 xxYWJjZC0xMjM0",
            "nothing here",
        ] {
            let mut expected = with.spans(text, false);
            let mut found = without.spans(text, false);
            expected.sort_unstable();
            found.sort_unstable();
            assert_eq!(found, expected, "{text}");
            assert_eq!(mask(text, found), mask(text, expected));
        }
        assert!(!with.spans("<abcd-1234>", true).is_empty());
    }

    #[test]
    fn a_base64_form_is_only_searched_for_when_it_is_long_enough() {
        let count = |secret: &str, kind: Kind| {
            forms(secret.as_bytes())
                .iter()
                .filter(|form| form.kind == kind)
                .count()
        };
        // One or two bytes: no base64 form. Three: the aligned encoding.
        // Four or more: all three alignments.
        assert_eq!(count("k", Kind::Base64), 0);
        assert_eq!(count("kk", Kind::Base64), 0);
        assert_eq!(count("abc", Kind::Base64), 1);
        assert_eq!(count("abcd", Kind::Base64), 3);
        // Bytes that encode to a plus or a slash add the URL-safe spellings.
        assert!(count("\u{3ff}\u{3ff}\u{3ff}\u{3ff}", Kind::Base64) > 3);
        // A space adds the form-submission spelling.
        assert_eq!(count("ab d", Kind::Plus), 1);
        assert_eq!(count("abcd", Kind::Plus), 0);
        // One byte: no hexadecimal form. From two: one, and a second when
        // the two letter cases differ.
        assert_eq!(count("k", Kind::Hex), 0);
        assert_eq!(count("ab", Kind::Hex), 1);
        assert_eq!(count("kk", Kind::Hex), 2);
        assert_eq!(forms(b"k").len(), 1);
    }

    #[test]
    fn an_escape_is_masked_whole() {
        let text = "x=%41%42%43&y";
        let view = View::of(text).percent_decoded();
        assert_eq!(view.bytes.as_ref(), b"x=ABC&y");
        assert_eq!(view.span(2, 5), (2, 11));
        assert_eq!(view.span(3, 4), (5, 8));

        let text = "\\ud83d\\ude00!";
        let view = View::of(text).json_unescaped();
        assert_eq!(view.bytes.as_ref(), "😀!".as_bytes());
        assert_eq!(view.span(0, 4), (0, 12));
        assert_eq!(view.span(1, 2), (0, 12));
        assert_eq!(view.span(4, 5), (12, 13));
    }
}
