//! The redactor: every registered secret, in every form text carries it in,
//! replaced by `[redacted]`.
//!
//! A secret is matched exactly, and also where it was percent-encoded,
//! JSON-escaped, or base64-encoded on the way. The first two are found by
//! decoding the text and searching what it decodes to, so every spelling of
//! an escape is covered. Base64 is found by searching for what the secret's
//! bytes encode to at each of the three positions they can fall in.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fmt;
use std::sync::{Arc, PoisonError, RwLock};

use a2m_core::{Digest, REDACTED, Redact, Secret};
use aho_corasick::{AhoCorasick, Input, MatchKind};
use zeroize::Zeroizing;

use crate::hex::nibble;

/// The fewest base64 characters a secret's encoded form must have to be
/// searched for. A shorter run would match in unrelated text everywhere.
const SHORTEST_BASE64_FORM: usize = 4;
/// How many times the result is searched again after masking, for a secret
/// that only appears once another is replaced.
const ROUNDS: usize = 4;

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
    /// Digests of the secrets registered, so that registering one again
    /// costs a lookup.
    known: BTreeSet<Digest>,
    short: usize,
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
    pub fn register(&self, secret: &Secret) {
        let value = secret.expose().as_bytes();
        if value.is_empty() {
            return;
        }
        let name = Digest::of(value);
        let known = self
            .state
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .known
            .contains(&name);
        if known {
            return;
        }
        let mut state = self.state.write().unwrap_or_else(PoisonError::into_inner);
        if !state.known.insert(name) {
            return;
        }
        if value.len() < Self::SHORT {
            state.short += 1;
        }
        let mut patterns = state
            .matcher
            .as_ref()
            .map(|matcher| matcher.patterns.clone())
            .unwrap_or_default();
        for form in forms(value) {
            if !patterns.iter().any(|known| known.bytes == form.bytes) {
                patterns.push(Arc::new(form));
            }
        }
        state.matcher = Some(Arc::new(Matcher::new(patterns)));
    }

    /// The registered set against its bound.
    #[must_use]
    pub fn health(&self) -> RedactorHealth {
        let state = self.state.read().unwrap_or_else(PoisonError::into_inner);
        RedactorHealth {
            registered: state.known.len(),
            short: state.short,
            bound: Self::BOUND,
        }
    }

    fn matcher(&self) -> Option<Arc<Matcher>> {
        self.state
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .matcher
            .clone()
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

/// One form of one secret, in wiped memory.
struct Pattern {
    bytes: Zeroizing<Vec<u8>>,
    /// A base64 form: the character on each side of a match holds bits of
    /// the secret too, so the mask takes them with it.
    widen: bool,
}

/// The forms a secret is searched for in: itself, itself with each space
/// written as a plus sign as a form submission writes it, and its base64
/// encodings.
fn forms(value: &[u8]) -> Vec<Pattern> {
    let mut forms = vec![Pattern {
        bytes: Zeroizing::new(value.to_vec()),
        widen: false,
    }];
    if value.contains(&b' ') {
        let plus = value.iter().map(|&byte| if byte == b' ' { b'+' } else { byte });
        forms.push(Pattern {
            bytes: Zeroizing::new(plus.collect()),
            widen: false,
        });
    }
    for offset in 0..3 {
        // The secret's bits start `offset` bytes into a group of three. Only
        // the characters made wholly of its bits are the same wherever it
        // stands; those are searched for.
        let first = (offset * 8usize).div_ceil(6);
        let end = (offset + value.len()) * 8 / 6;
        if end < first + SHORTEST_BASE64_FORM {
            continue;
        }
        let sextets: Zeroizing<Vec<u8>> =
            Zeroizing::new((first..end).map(|at| sextet(value, offset, at)).collect());
        for alphabet in [STANDARD, URL_SAFE] {
            let bytes: Zeroizing<Vec<u8>> = Zeroizing::new(
                sextets
                    .iter()
                    .map(|&six| alphabet[usize::from(six)])
                    .collect(),
            );
            if !forms.iter().any(|form| form.bytes == bytes) {
                forms.push(Pattern { bytes, widen: true });
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
}

impl Matcher {
    fn new(patterns: Vec<Arc<Pattern>>) -> Self {
        let automaton = AhoCorasick::builder()
            .match_kind(MatchKind::Standard)
            .build(patterns.iter().map(|pattern| pattern.bytes.as_slice()))
            .ok();
        Self {
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

    /// The places in `text` to mask, as byte ranges, in no order. With
    /// `first_only` the search stops at the first one.
    fn spans(&self, text: &str, first_only: bool) -> Vec<(usize, usize)> {
        let mut spans = Vec::new();
        let mut masks = None;
        let mut search = |view: &View<'_>, spans: &mut Vec<(usize, usize)>| {
            self.each_match(&view.bytes, |pattern, start, end| {
                let (start, end) = if pattern.widen {
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
        };

        let raw = View::of(text);
        if search(&raw, &mut spans) {
            return spans;
        }
        // Decoded once and twice, and each of the two inside the other where
        // that is common: a percent-encoded value in JSON needs no decoding
        // of the JSON, and JSON carried in a percent-encoded value does.
        if raw.bytes.contains(&b'%') {
            let percent = raw.percent_decoded();
            if search(&percent, &mut spans) {
                return spans;
            }
            if percent.bytes.contains(&b'%') && search(&percent.percent_decoded(), &mut spans) {
                return spans;
            }
            if percent.bytes.contains(&b'\\') && search(&percent.json_unescaped(), &mut spans) {
                return spans;
            }
        }
        if raw.bytes.contains(&b'\\') {
            let json = raw.json_unescaped();
            if search(&json, &mut spans) {
                return spans;
            }
            if json.bytes.contains(&b'\\') {
                search(&json.json_unescaped(), &mut spans);
            }
        }
        spans
    }
}

/// Takes one base64 character on each side into a match, and the padding
/// after it: the neighbours hold the secret's remaining bits.
fn widen(bytes: &[u8], mut start: usize, mut end: usize) -> (usize, usize) {
    let is_base64 =
        |byte: &u8| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'-' | b'_');
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

/// Text as it is, or as it decodes, with the place in the original that each
/// byte came from.
struct View<'a> {
    bytes: Cow<'a, [u8]>,
    /// For a decoded view: where in the original each byte's source starts.
    /// `None` for the original itself.
    starts: Option<Vec<usize>>,
    source_length: usize,
}

impl<'a> View<'a> {
    fn of(text: &'a str) -> Self {
        Self {
            bytes: Cow::Borrowed(text.as_bytes()),
            starts: None,
            source_length: text.len(),
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
        }
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

/// The number four hexadecimal characters spell.
fn code_unit(four: &[u8]) -> Option<u32> {
    four.iter()
        .try_fold(0u32, |sum, &digit| Some((sum << 4) | u32::from(nibble(digit)?)))
}
