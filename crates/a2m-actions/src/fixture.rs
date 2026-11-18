//! `fixture.record`: an offline test's fixture recorded, scrubbed, from one
//! real read (decision 0196).
//!
//! One drafted or published read runs in the sandbox as a trial, in the
//! look-only posture. Each step that only reads tells the control plane what
//! it observed ([`a2m_engine::Control::step_observed`]); the values are held
//! in memory for that one trial. An observed answer is business data until
//! it is scrubbed: every value passes the redactor, and then each is put in
//! place of by a synthetic one of the same type and shape, by the fixed
//! rules of [`Scrubber`]. The structure is kept: names, kinds, and list
//! lengths up to [`LIST_ITEMS_KEPT`]. Nothing of an original value is kept.
//! The test's expectation is the replay's own result against the scrubbed
//! fixture; the test is run against it in memory, and only when it passes
//! are the files written, through the checked paths; then it is run once
//! more from the files written. Where a value's type gives no safe rule,
//! the action stops by name and writes nothing. Writes are not recorded.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};

use a2m_core::{
    CanonicalJson, Date, Decimal, Digest, EffectClass, FailureClass, FieldBase, FieldHasher,
    FieldTypePlan, Millis, OperationName, OperationPlan, Plan, Redact, RunState, Step, TestKind,
    TestPlan, Timestamp, ToolRuns, Value, ValueType, Values,
};
use a2m_provider::{Recorded, RecordedAnswer};
use serde_json::{Value as Json, json};

use crate::codes;
use crate::handlers::handler;
use crate::kernel::Kernel;

/// What each step of a trial a fixture is recorded from observed, in order:
/// the name a replay answers it by, and its values, or `None` for absent.
pub(crate) type Recording = Arc<Mutex<Vec<(String, Option<Values>)>>>;

/// The most step answers one recording keeps; a trial that gives more is
/// not recorded.
const ANSWERS_MOST: usize = 256;

/// How many items of a list are kept: a longer list is cut to this many,
/// each scrubbed, and the cut is said in the result.
pub(crate) const LIST_ITEMS_KEPT: usize = 3;

/// The fixed date a date is put in place of.
const FIXED_DATE: (i32, u8, u8) = (2000, 1, 1);

/// The fixed moment a moment is put in place of: 2000-01-01T00:00:00Z.
const FIXED_INSTANT_MS: i64 = 946_684_800_000;

/// The fixed length of time a length of time is put in place of: one
/// second.
const FIXED_DURATION_MS: i64 = 1_000;

/// Keeps what a step of a trial observed, when a fixture is being recorded
/// from the trial whose run digest is `run`; nothing otherwise.
pub(crate) fn observed(kernel: &Kernel, run: Digest, step: &Step, observed: Option<&Values>) {
    let recording = kernel
        .inner
        .recordings
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&run)
        .cloned();
    if let Some(recording) = recording {
        let mut kept = recording.lock().unwrap_or_else(PoisonError::into_inner);
        if kept.len() <= ANSWERS_MOST {
            kept.push((
                a2m_provider::answer_name(step).to_owned(),
                observed.cloned(),
            ));
        }
    }
}

/// Removes a recording when the action ends, however it ends.
struct Recorder<'k> {
    kernel: &'k Kernel,
    run: Digest,
    recording: Recording,
}

impl<'k> Recorder<'k> {
    fn start(kernel: &'k Kernel, run: Digest) -> Self {
        let recording: Recording = Arc::new(Mutex::new(Vec::new()));
        kernel
            .inner
            .recordings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(run, Arc::clone(&recording));
        Self {
            kernel,
            run,
            recording,
        }
    }

    fn taken(&self) -> Vec<(String, Option<Values>)> {
        std::mem::take(
            &mut *self
                .recording
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        )
    }
}

impl Drop for Recorder<'_> {
    fn drop(&mut self) {
        self.kernel
            .inner
            .recordings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.run);
    }
}

/// What scrubbing reads of a value's field type: what it is built on, its
/// declared codes for a choice, its decimal places, and whether it is free
/// text that came from the system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypeRule {
    base: FieldBase,
    codes: Vec<String>,
    places: Option<u32>,
    untrusted: bool,
}

impl TypeRule {
    fn of(ty: &FieldTypePlan) -> Self {
        Self {
            base: ty.base,
            codes: ty.codes.iter().map(|code| code.code.clone()).collect(),
            places: ty.places,
            untrusted: ty.untrusted,
        }
    }
}

/// Where a value stands, as scrubbing needs it: its field type, its
/// declared type, and whether it is a key field, whose made-up value the
/// test's input and the fixture must agree on.
#[derive(Debug, Clone, Default)]
pub(crate) struct Slot {
    rule: Option<TypeRule>,
    declared: Option<ValueType>,
    key: bool,
}

/// Why a value cannot be scrubbed: no rule of its type writes a synthetic
/// one, so nothing is written.
fn no_rule(why: &str) -> a2m_core::Failure {
    codes::input(
        codes::FIXTURE_NO_SCRUB_RULE,
        format!(
            "The read's answer holds a value that no scrubbing rule covers: {why}. Nothing was written."
        ),
        "Write this fixture by hand with synthetic values, or record a read whose values are text, numbers, dates, choices, true or false, or empty.",
    )
}

/// The one alphabet every made-up text and key is written in.
const ALPHABET: [char; 7] = ['j', 'k', 'q', 'v', 'w', 'x', 'z'];

/// The length of made-up text of a field type built on text.
const TEXT_LENGTH: usize = 8;

/// The length of made-up free text that came from the system.
const FREE_TEXT_LENGTH: usize = 24;

/// The length of made-up text of no field type.
const UNTYPED_LENGTH: usize = 12;

/// The length of a made-up key of a map, after its `key_`.
const KEY_LENGTH: usize = 6;

/// Puts synthetic values in place of observed ones (decision 0200). A
/// made-up value is chosen without looking at the original, except for what
/// the replay needs:
///
/// - text, of a field type built on text or of no field type, by text of
///   one fixed alphabet and one fixed length for its kind of type (8 for
///   text, 24 for free text that came from the system, 12 for no type);
///   it is chosen by the value's place in the fixture, so two equal
///   originals at two places get two made-up texts, and the same place in
///   two answers of one step gets the same one; in a key field (an input of
///   the operation's key, or an output of that name), the same original
///   gets the same made-up text, so that the test's input and the fixture
///   agree;
/// - a choice, by its type's first declared code, whatever it was;
/// - a whole number by `1`, and an exact number by `1` with its type's
///   decimal places (none where the type states none); in a key field, the
///   `k`-th distinct whole number by `k`;
/// - `true` and `false` by place, alternating; `null` is kept;
/// - a date, a moment, and a length of time, by fixed ones
///   (2000-01-01, 2000-01-01T00:00:00Z, one second);
/// - a list, by its first [`LIST_ITEMS_KEPT`] items, each scrubbed by the
///   list's field type, which is its items' (a list's length up to that
///   number is still shown);
/// - a record: the names its declared type lists, and an answer's names
///   that are the operation's declared outputs, are structure and are kept;
///   every other name is data and is put in place of by a made-up key, the
///   same original key by the same made-up key throughout the fixture.
///
/// Text of a field type built on a number, a date, or a time has no rule,
/// and stops the action.
pub(crate) struct Scrubber<'r> {
    redact: &'r dyn Redact,
    /// The made-up text of each original text of a key field.
    keys: BTreeMap<String, String>,
    /// The made-up number of each original whole number of a key field.
    key_ints: BTreeMap<i64, i64>,
    /// The made-up key of each original key of a map.
    data_keys: BTreeMap<String, String>,
    /// The index of each place a made-up text was chosen for.
    places: BTreeMap<String, usize>,
    /// The index of each place a made-up `true` or `false` was chosen for.
    flags: BTreeMap<String, usize>,
    /// How many values each rule scrubbed, by rule.
    pub(crate) counts: BTreeMap<&'static str, u32>,
    /// Every original value of the system as text, and every original key,
    /// for the check that none is in what is written. A choice's code is
    /// the target's own vocabulary, and is not among them.
    pub(crate) originals: BTreeSet<String>,
    /// Every made-up text and key written.
    pub(crate) synthetics: BTreeSet<String>,
}

impl<'r> Scrubber<'r> {
    pub(crate) fn new(redact: &'r dyn Redact) -> Self {
        Self {
            redact,
            keys: BTreeMap::new(),
            key_ints: BTreeMap::new(),
            data_keys: BTreeMap::new(),
            places: BTreeMap::new(),
            flags: BTreeMap::new(),
            counts: BTreeMap::new(),
            originals: BTreeSet::new(),
            synthetics: BTreeSet::new(),
        }
    }

    fn count(&mut self, rule: &'static str) {
        *self.counts.entry(rule).or_insert(0) += 1;
    }

    /// Keeps an original as text, raw and as the redactor gives it.
    fn original(&mut self, text: &str) {
        self.originals.insert(text.to_owned());
        let redacted = self.redact.redact(text);
        if redacted != text {
            self.originals.insert(redacted);
        }
    }

    /// Keeps an original as a file would write it.
    fn original_as_written(&mut self, value: &Value) {
        match serde_json::to_value(value) {
            Ok(Json::String(text)) => self.original(&text),
            Ok(Json::Number(number)) => self.original(&number.to_string()),
            _ => {}
        }
    }

    /// The index of a place, the same for the same place.
    fn place(&mut self, at: &str) -> usize {
        let next = self.places.len();
        *self.places.entry(at.to_owned()).or_insert(next)
    }

    /// The values of a record at `at`: a name `slot` answers for is
    /// structure, and its value is scrubbed in that slot; every other name
    /// is data, and is put in place of by a made-up key.
    pub(crate) fn values(
        &mut self,
        values: &Values,
        at: &str,
        slot: &dyn Fn(&str) -> Option<Slot>,
    ) -> Result<Values, a2m_core::Failure> {
        let mut out = Values::new();
        for (name, value) in values.iter() {
            let (kept, slot) = match slot(name) {
                Some(slot) => (name.to_owned(), slot),
                None => (self.data_key(name), Slot::default()),
            };
            let here = format!("{at}/{kept}");
            out.insert(kept, self.value(value, &here, &slot)?);
        }
        Ok(out)
    }

    /// The made-up key of an original key of a map, the same for the same
    /// key throughout the fixture.
    fn data_key(&mut self, original: &str) -> String {
        if let Some(made) = self.data_keys.get(original) {
            return made.clone();
        }
        self.original(original);
        let made = format!("key_{}", encode(self.data_keys.len(), KEY_LENGTH));
        self.count("a map's key by a made-up key");
        self.synthetics.insert(made.clone());
        self.data_keys.insert(original.to_owned(), made.clone());
        made
    }

    fn value(&mut self, value: &Value, at: &str, slot: &Slot) -> Result<Value, a2m_core::Failure> {
        let base = slot.rule.as_ref().map(|rule| rule.base);
        Ok(match value {
            Value::Null => {
                self.count("null kept");
                Value::Null
            }
            Value::Bool(_) => {
                self.count("true or false by place, alternating");
                let next = self.flags.len();
                let index = *self.flags.entry(at.to_owned()).or_insert(next);
                Value::Bool(index.is_multiple_of(2))
            }
            Value::Text(text) => match base {
                Some(FieldBase::Choice) => {
                    // A code is one the target's own field type declares;
                    // the first stands for every one (audit finding FX-2),
                    // so the code observed is not looked for in what is
                    // written.
                    let code = slot
                        .rule
                        .as_ref()
                        .and_then(|rule| rule.codes.first().cloned())
                        .ok_or_else(|| no_rule("a choice whose type declares no code"))?;
                    self.count("choice by its type's first declared code");
                    Value::Text(code)
                }
                None | Some(FieldBase::Text) => {
                    self.original(text);
                    let length = match &slot.rule {
                        Some(rule) if rule.untrusted => FREE_TEXT_LENGTH,
                        Some(_) => TEXT_LENGTH,
                        None => UNTYPED_LENGTH,
                    };
                    let made = if slot.key {
                        let next = self.keys.len();
                        self.keys
                            .entry(text.clone())
                            .or_insert_with(|| encode(next, length))
                            .clone()
                    } else {
                        // Places are counted apart from keys: a made-up key
                        // text is never another place's text.
                        let index = self.place(at);
                        format!("{}{}", ALPHABET[ALPHABET.len() - 1], encode(index, length - 1))
                    };
                    self.count("text by made-up text of its type's length");
                    self.synthetics.insert(made.clone());
                    Value::Text(made)
                }
                Some(base) => {
                    return Err(no_rule(&format!("text of a field type built on `{base}`")));
                }
            },
            Value::Int(number) => {
                self.original(&number.to_string());
                let made = if slot.key {
                    let next = i64::try_from(self.key_ints.len()).unwrap_or(i64::MAX - 1) + 1;
                    *self.key_ints.entry(*number).or_insert(next)
                } else {
                    1
                };
                self.count("whole number by a fixed number");
                Value::Int(made)
            }
            Value::Decimal(number) => {
                self.original(&number.to_string());
                let places = slot.rule.as_ref().and_then(|rule| rule.places).unwrap_or(0);
                self.count("exact number by a fixed number of its type's places");
                Value::Decimal(fixed_decimal(places)?)
            }
            Value::Date(_) => {
                self.original_as_written(value);
                self.count("date by a fixed date");
                let (year, month, day) = FIXED_DATE;
                Value::Date(Date::new(year, month, day).map_err(|_| no_rule("a date"))?)
            }
            Value::Instant(_) => {
                self.original_as_written(value);
                self.count("moment by a fixed moment");
                Value::Instant(Timestamp::from_unix_millis(FIXED_INSTANT_MS))
            }
            Value::Duration(_) => {
                self.count("length of time by a fixed length");
                Value::Duration(Millis::new(FIXED_DURATION_MS))
            }
            Value::List(items) => {
                if items.len() > LIST_ITEMS_KEPT {
                    self.count("list cut to its first items");
                }
                // A list output's field type is its items' type: a list of
                // choices is scrubbed code by code (decision 0199).
                let item = Slot {
                    rule: slot.rule.clone(),
                    declared: match &slot.declared {
                        Some(ValueType::List(item)) => Some((**item).clone()),
                        other => other.clone(),
                    },
                    key: slot.key,
                };
                let mut out = Vec::new();
                for (position, value) in items.iter().take(LIST_ITEMS_KEPT).enumerate() {
                    out.push(self.value(value, &format!("{at}/{position}"), &item)?);
                }
                Value::List(out)
            }
            Value::Record(values) => {
                // The names the declared type lists are structure; every
                // other name is data (audit finding FX-1).
                let fields = match &slot.declared {
                    Some(ValueType::Record(fields)) => fields.clone(),
                    _ => Vec::new(),
                };
                let named = move |name: &str| {
                    fields.iter().find(|field| field.name == name).map(|field| Slot {
                        rule: None,
                        declared: Some(field.ty.clone()),
                        key: false,
                    })
                };
                Value::Record(self.values(values, at, &named)?)
            }
        })
    }
}

/// `index` written in [`ALPHABET`], filled on the left with its first
/// letter to `length` characters.
fn encode(mut index: usize, length: usize) -> String {
    let mut out = vec![ALPHABET[0]; length];
    let mut at = length;
    while index > 0 && at > 0 {
        at -= 1;
        out[at] = ALPHABET[index % ALPHABET.len()];
        index /= ALPHABET.len();
    }
    out.into_iter().collect()
}

/// The fixed exact number of `places` decimal places: `1`, or `1.0…0`.
fn fixed_decimal(places: u32) -> Result<Decimal, a2m_core::Failure> {
    let places = usize::try_from(places.min(18)).unwrap_or(0);
    let text = if places == 0 {
        "1".to_owned()
    } else {
        format!("1.{}", "0".repeat(places))
    };
    text.parse::<Decimal>()
        .map_err(|_| no_rule("an exact number"))
}

/// The slot of each output of the operation, by name: an answer's names
/// that are declared outputs are structure.
fn output_slots(plan: &Plan, operation: &OperationPlan) -> BTreeMap<String, Slot> {
    let key = key_names(operation);
    operation
        .outputs
        .iter()
        .map(|output| {
            let rule = output
                .field_type
                .and_then(|index| plan.field_type(index))
                .map(TypeRule::of);
            (
                output.name.clone(),
                Slot {
                    rule,
                    declared: Some(output.ty.clone()),
                    key: key.contains(&output.name),
                },
            )
        })
        .collect()
}

/// The slot of each input of the operation, by name.
fn input_slots(plan: &Plan, operation: &OperationPlan) -> BTreeMap<String, Slot> {
    let key = key_names(operation);
    operation
        .inputs
        .iter()
        .map(|input| {
            (
                input.name.clone(),
                Slot {
                    rule: plan.field_type(input.field_type).map(TypeRule::of),
                    declared: Some(input.ty.clone()),
                    key: key.contains(&input.name),
                },
            )
        })
        .collect()
}

/// The names of the operation's key inputs.
fn key_names(operation: &OperationPlan) -> BTreeSet<String> {
    operation
        .identity
        .key
        .iter()
        .filter_map(|index| operation.inputs.get(index.get()))
        .map(|input| input.name.clone())
        .collect()
}

/// The recorded answers, scrubbed, by name in the order each name was first
/// asked: one object where every answer of the name is the same, which
/// answers every step of it, else a list, given in order (decision 0180).
/// A value's place is its answer's name and its path inside the answer, so
/// one step's answers scrubbed alike are one object.
pub(crate) fn scrubbed_answers(
    scrubber: &mut Scrubber<'_>,
    observed: &[(String, Option<Values>)],
    slots: &BTreeMap<String, Slot>,
) -> Result<Vec<(String, Json, Vec<RecordedAnswer>)>, a2m_core::Failure> {
    let mut names: Vec<&str> = Vec::new();
    for (name, _) in observed {
        if !names.contains(&name.as_str()) {
            names.push(name);
        }
    }
    let lookup = |name: &str| slots.get(name).cloned();
    let mut out = Vec::new();
    for name in names {
        let answers: Vec<&Option<Values>> = observed
            .iter()
            .filter(|(of, _)| of == name)
            .map(|(_, answer)| answer)
            .collect();
        let mut scrubbed = Vec::new();
        for answer in &answers {
            scrubbed.push(match answer {
                Some(values) => Recorded::Done(scrubber.values(values, name, &lookup)?),
                None => Recorded::Absent,
            });
        }
        let repeats = scrubbed.windows(2).all(|pair| pair[0] == pair[1]);
        let json_of = |answer: &Recorded| -> Result<Json, a2m_core::Failure> {
            match answer {
                Recorded::Done(values) => serde_json::to_value(values).map_err(|_| {
                    no_rule("an answer that cannot be written as JSON")
                }),
                Recorded::Absent => Ok(Json::Null),
            }
        };
        let (file, replayed) = if repeats {
            let one = scrubbed.first().cloned().unwrap_or(Recorded::Absent);
            (
                json_of(&one)?,
                vec![RecordedAnswer {
                    name: name.to_owned(),
                    args: None,
                    answer: one,
                    repeats: true,
                }],
            )
        } else {
            (
                Json::Array(scrubbed.iter().map(json_of).collect::<Result<_, _>>()?),
                scrubbed
                    .into_iter()
                    .map(|answer| RecordedAnswer {
                        name: name.to_owned(),
                        args: None,
                        answer,
                        repeats: false,
                    })
                    .collect(),
            )
        };
        out.push((name.to_owned(), file, replayed));
    }
    Ok(out)
}

/// What the check that no original is in a written byte found: whether one
/// is, and how many originals were too short to be looked for (audit
/// finding FX-3). Each file is looked at with every made-up value taken
/// out first, so an original that a made-up value happens to hold is not
/// found in it.
pub(crate) fn leak_check(scrubber: &Scrubber<'_>, files: &[(String, Vec<u8>)]) -> (bool, usize) {
    let mut synthetics: Vec<&String> = scrubber.synthetics.iter().collect();
    synthetics.sort_by_key(|text| std::cmp::Reverse(text.len()));
    let texts: Vec<String> = files
        .iter()
        .map(|(_, bytes)| {
            let mut text = String::from_utf8_lossy(bytes).into_owned();
            for synthetic in &synthetics {
                text = text.replace(synthetic.as_str(), "\u{0}");
            }
            text
        })
        .collect();
    let mut short = 0;
    let mut leaked = false;
    for original in &scrubber.originals {
        if original.chars().count() < 3 {
            short += 1;
            continue;
        }
        if scrubber.synthetics.contains(original) {
            continue;
        }
        leaked |= texts.iter().any(|text| text.contains(original.as_str()));
    }
    (leaked, short)
}

/// A name a fixture folder and a test file may take: lowercase letters,
/// digits, and underscores, as an answer's name is.
fn fixture_name(text: &str) -> Result<String, a2m_core::Failure> {
    let fine = !text.is_empty()
        && text.len() <= 64
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if fine {
        Ok(text.to_owned())
    } else {
        Err(codes::input(
            codes::SCHEMA_INVALID,
            "A fixture's name is lowercase letters, digits, and underscores, at most 64 of them.",
            "Give `name` such a name, or leave it out to take the tool's name without the target's.",
        ))
    }
}

/// The text of a recorded test file: JSON is YAML, so the input and the
/// expectation are written as JSON, which the compiler reads. A binding the
/// call named is named in the test; with none, the test runs the binding
/// that would act live.
fn test_file(
    tool: &OperationName,
    name: &str,
    inputs: &Json,
    expect: &Json,
    binding: Option<&a2m_engine::BindingPin>,
) -> String {
    let binding = binding.map_or_else(String::new, |pin| {
        format!(
            "binding: {{mode: {}, position: {}}}\n",
            pin.mode,
            pin.position + 1
        )
    });
    format!(
        "# Recorded by `fixture.record` from one read, every value scrubbed by rule.\n\
         # A person reads this file and its fixture before committing them.\n\
         test: {tool}\nkind: offline\n{binding}fixture: {name}\ninputs: {inputs}\nexpect: {expect}\n"
    )
}

const MARKER: &str = "scrubbed: true\nnote: \"Recorded by fixture.record from one read; every value was put in place of by a synthetic one, by the rules the action names. A person reads the files before committing them.\"\n";

/// What a recording leaves out of the system's answer, and what it still
/// shows: said in every result (audit findings FX-2 and FX-3).
fn limits(scrubber: &Scrubber<'_>, short: usize) -> Vec<String> {
    let mut said = vec![
        "A list's length up to 3 items is shown, a longer list is cut to 3; a field that was empty (null) is kept empty.".to_owned(),
    ];
    if scrubber
        .counts
        .contains_key("choice by its type's first declared code")
    {
        said.push("Every choice was written as its type's first declared code, whatever the system held: the recorded test cannot tell the operation's choices apart.".to_owned());
    }
    if short > 0 {
        said.push(format!(
            "{short} original values were shorter than 3 characters and could not be looked for in the files written; each was put in place of all the same."
        ));
    }
    said
}

/// One file `fixture.record` made, and what it is, so that it can be
/// removed again if the call fails after it was made.
enum Made {
    Folder(a2m_provider::CheckedPath),
    File(a2m_provider::CheckedPath, a2m_provider::FileIdentity),
}

/// Makes a file new at `path` and writes `bytes` to it: nothing that stands
/// at the name, a file or a link, is replaced or followed (audit finding
/// FX-5). The file made is then found at the checked path by its identity.
fn create_new(
    path: &a2m_provider::CheckedPath,
    bytes: &[u8],
) -> Result<a2m_provider::FileIdentity, a2m_core::Failure> {
    use std::io::Write as _;
    let there = || {
        codes::input(
            codes::SCHEMA_INVALID,
            format!("`{}` appeared while the read was recorded, and a recorded fixture is never written over anything.", path.relative()),
            "Give `name` another name, or remove the old files with `draft.delete`.",
        )
    };
    if !matches!(path.entry_kind()?, a2m_provider::EntryKind::Absent) {
        return Err(there());
    }
    // Made new or not at all: an exclusive create refuses whatever stands
    // at the name, a link among them, without following it.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path.as_path())
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                there()
            } else {
                codes::input(
                    codes::SCHEMA_INVALID,
                    format!("`{}` cannot be written.", path.relative()),
                    "Check that the target's folder may be written, and record again.",
                )
            }
        })?;
    let identity = a2m_provider::FileIdentity::of_open(&file)?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| {
            codes::input(
                codes::SCHEMA_INVALID,
                format!("`{}` cannot be written.", path.relative()),
                "Check that the disk has room, and record again.",
            )
        })?;
    if path.identity()? != identity {
        return Err(codes::input(
            codes::SCHEMA_INVALID,
            format!("`{}` is not the file this call made: the folder changed while it was written.", path.relative()),
            "Make no change to the target's folders while a fixture is recorded, and record again.",
        ));
    }
    Ok(identity)
}

/// Removes what this call made, newest first, and only what is still the
/// file it made. Gives the paths it could not remove.
fn remove_made(made: &[Made]) -> Vec<String> {
    let mut left = Vec::new();
    for made in made.iter().rev() {
        match made {
            Made::File(path, identity) => {
                let ours = path.identity().is_ok_and(|now| now == *identity);
                if !ours || path.remove_file().is_err() {
                    left.push(path.relative().to_owned());
                }
            }
            Made::Folder(path) => {
                if std::fs::remove_dir(path.as_path()).is_err() {
                    left.push(path.relative().to_owned());
                }
            }
        }
    }
    left
}

/// Writes every file of a recording, all or none (audit finding FX-4): the
/// fixture folder is made new, each file is made new, and on any failure
/// what this call made is removed again.
fn write_all_or_none(
    at: &a2m_provider::CheckedPath,
    folder: &str,
    files: &[(String, Vec<u8>)],
    made: &mut Vec<Made>,
) -> Result<(), a2m_core::Failure> {
    let fixtures = at.join("fixtures")?;
    if !fixtures.is_dir() {
        fixtures.create_dir()?;
        made.push(Made::Folder(fixtures));
    }
    let fixture = at.join(folder)?;
    fixture.create_dir().map_err(|_| {
        codes::input(
            codes::SCHEMA_INVALID,
            format!("`{folder}` appeared while the read was recorded, and a recorded fixture is never written over one."),
            "Give `name` another name, or remove the old files with `draft.delete`.",
        )
    })?;
    made.push(Made::Folder(fixture));
    for (path, bytes) in files {
        if let Some((parent, _)) = path.rsplit_once('/')
            && parent != folder
        {
            let parent = at.join(parent)?;
            if !parent.is_dir() {
                parent.create_dir()?;
                made.push(Made::Folder(parent));
            }
        }
        let file = at.join(path)?;
        let identity = create_new(&file, bytes)?;
        made.push(Made::File(file, identity));
    }
    Ok(())
}

/// A failure after files were written, with what became of them.
fn after_written(failure: a2m_core::Failure, left: &[String]) -> a2m_core::Failure {
    let said = if left.is_empty() {
        " The files this call wrote were removed again, so the target is as it was.".to_owned()
    } else {
        format!(
            " The files this call wrote were removed again, except: {}; look at them, and remove them with `draft.delete`.",
            left.join(", ")
        )
    };
    a2m_core::Failure::new(
        failure.class(),
        failure.code().clone(),
        format!("{}{said}", failure.message()),
        failure.next_step().to_owned(),
    )
}

handler!(
    /// `fixture.record`: runs one drafted or published read as a trial in
    /// the sandbox, scrubs what each step observed into a fixture, and
    /// writes the fixture and an offline test whose expectation is the
    /// replay's own result (decision 0196). Reads only.
    fixture_record,
    |kernel, call| {
        let target = a2m_core::TargetName::new(call.required("target")?)?;
        let tool_name = OperationName::new(call.required("tool")?)?;
        let project_folder = call.project_folder()?.to_path_buf();
        let compiled = crate::handlers::build::compile_in(
            kernel,
            project_folder.clone(),
            &target,
            a2m_files::CompileMode::Draft,
            a2m_files::DRAFT_VERSION,
        )
        .await?
        .map_err(a2m_core::Failure::from)?;
        let plan = Arc::new(compiled.plan);
        let Some(tool) = plan.tool(&tool_name).cloned() else {
            return Err(codes::input(
                codes::NOT_FOUND,
                format!("The drafts of `{target}` hold no tool named `{tool_name}`."),
                "Call `draft.lint` to list the drafted tools.",
            ));
        };
        let operation = match tool.runs {
            ToolRuns::Operation(index) => plan.operation(index).cloned(),
            ToolRuns::Pipeline(_) => None,
        };
        let Some(operation) = operation.filter(|operation| operation.effect == EffectClass::Read)
        else {
            return Err(codes::input(
                codes::ACTION_NOT_BUILT,
                format!("`{tool_name}` is not a read: a fixture is recorded from a read alone."),
                "Record the fixture of a read; a write's fixture is written by hand, with its request.",
            ));
        };
        let name = match call.text("name") {
            Some(given) => fixture_name(given)?,
            None => fixture_name(
                tool_name
                    .as_str()
                    .strip_prefix(&format!("{target}_"))
                    .unwrap_or(tool_name.as_str()),
            )?,
        };
        let input: Values = match call.input.get("input") {
            Some(given) => serde_json::from_value(given.clone()).map_err(|_| {
                codes::input(
                    codes::SCHEMA_INVALID,
                    "The tool's input holds a value that cannot be taken.",
                    "Use text, whole numbers, true or false, lists, and maps; write an exact number as text.",
                )
            })?,
            None => Values::new(),
        };
        let environment = plan
            .default_environment()
            .map(|environment| environment.name.clone())
            .ok_or_else(|| {
                codes::input(
                    codes::NOT_FOUND,
                    "The drafted target declares no environment.",
                    "Declare an environment in the target's files.",
                )
            })?;
        // Nothing is written over: a fixture or a test of this name already
        // there is refused before the read runs. Each file is made new when
        // it is written, too, so one that appears meanwhile is never
        // replaced (audit finding FX-5).
        let (folder, test_path) = (format!("fixtures/{name}"), format!("tests/{name}.yaml"));
        {
            let (project_folder, target, folder, test_path) = (
                project_folder.clone(),
                target.clone(),
                folder.clone(),
                test_path.clone(),
            );
            crate::handlers::build::off_thread(move || {
                let at = crate::handlers::build::target_folder(&project_folder, &target)?;
                if at.join(&folder)?.exists() || at.join(&test_path)?.exists() {
                    return Err(codes::input(
                        codes::SCHEMA_INVALID,
                        format!("The target already has `{folder}` or `{test_path}`, and a recorded fixture is never written over one."),
                        "Give `name` another name, or remove the old files with `draft.delete`.",
                    ));
                }
                Ok(())
            })
            .await?;
        }

        // The trial, recorded. Its digest names this call alone, so that
        // it is a run of its own (ledger rule 6) and the recording is its.
        let mut nonce = [0_u8; 16];
        kernel.inner.random.fill(&mut nonce)?;
        let mut hasher = FieldHasher::new();
        hasher.field("purpose", b"anything2mcp fixture record 1");
        hasher.field("input", input.digest().to_string().as_bytes());
        hasher.field("nonce", &nonce);
        let digest = hasher.finish();
        let found = crate::tools::FoundTool {
            target: target.clone(),
            plan_hash: compiled.encoded.hash,
            plan: Arc::clone(&plan),
            tool: tool.clone(),
        };
        // The binding recorded is the one named, else the one that would
        // act live; the trial, the test, and the recorded test name it
        // (audit finding AC-6).
        let binding = crate::handlers::tools::named_binding(&plan, &tool, &call)?;
        let named = call.text("mode").is_some() || call.int("position").is_some();
        kernel.keep_draft(&target, compiled.encoded.hash, Arc::clone(&plan));
        let recorder = Recorder::start(kernel, digest);
        let run = crate::handlers::tools::submit_trial_kept(
            kernel,
            &call,
            &found,
            &input,
            environment,
            (digest, binding.clone()),
            a2m_ledger::RetentionClass::Short,
        )
        .await?;
        let row = kernel
            .wait_for_rest(run, crate::handlers::tools::wait_of(&call))
            .await?;
        let observed = recorder.taken();
        drop(recorder);
        let active = matches!(
            row.state,
            RunState::Queued | RunState::Validating | RunState::Running
        );
        if active {
            // Not at rest within the wait: it is cancelled, so that it does
            // not go on reading the system for nobody (audit finding FX-6).
            let _ = kernel.sched()?.cancel(run).await;
        }
        if row.state != RunState::Completed {
            return Err(codes::input(
                codes::FIXTURE_READ_FAILED,
                format!(
                    "The read did not complete in the sandbox (`{}`{}){}, so nothing was recorded.",
                    row.state,
                    row.failure
                        .as_ref()
                        .map(|(class, code)| format!(", {class}/{code}"))
                        .unwrap_or_default(),
                    if active { " within the wait, and was cancelled" } else { "" }
                ),
                format!(
                    "Call `run.get` with `{run}` to see why; correct the draft or the input, and record again."
                ),
            ));
        }
        if observed.is_empty() || observed.len() > ANSWERS_MOST {
            return Err(codes::input(
                codes::FIXTURE_READ_FAILED,
                "The read gave no step answer that can be recorded, or more than a fixture keeps.",
                "Write this fixture by hand.",
            ));
        }

        // Scrub: the input first, so that a key keeps one synthetic value
        // in the input and in every answer of a key field.
        let redact = kernel.redact();
        let mut scrubber = Scrubber::new(redact.as_ref());
        let input_slots = input_slots(&plan, &operation);
        let inputs = scrubber.values(&input, "input", &|name| {
            Some(input_slots.get(name).cloned().unwrap_or_default())
        })?;
        let answers = scrubbed_answers(&mut scrubber, &observed, &output_slots(&plan, &operation))?;

        // The test, run against the scrubbed fixture in memory: once for
        // its result, which becomes its expectation, and once to pass.
        let tool_index = plan
            .tools
            .iter()
            .position(|kept| kept.name == tool.name)
            .and_then(|at| u32::try_from(at).ok())
            .map(a2m_core::ToolIndex::new)
            .ok_or_else(|| {
                codes::input(
                    codes::NOT_FOUND,
                    "The tool is not in the plan.",
                    "Report this.",
                )
            })?;
        let replayed: Vec<RecordedAnswer> = answers
            .iter()
            .flat_map(|(_, _, replayed)| replayed.iter().cloned())
            .collect();
        let world = crate::offline::OfflineWorld {
            workspace: call.caller.principal.workspace,
            principal: call.caller.principal.principal.id,
            vault: kernel.inner.vault.clone(),
        };
        let mut test = TestPlan {
            name: name.clone(),
            tool: tool_index,
            kind: TestKind::Offline,
            fixture: Some(folder.clone()),
            inputs: inputs.clone(),
            expect: CanonicalJson::encode(&json!({}))?,
            restore: false,
        };
        let engine = kernel.engine().await?;
        let (runs_plan, hash) = (Arc::clone(&plan), compiled.encoded.hash);
        let first = test.clone();
        let (first_binding, second_binding) = (binding.clone(), binding.clone());
        let (runs_replayed, runs_world) = (replayed.clone(), world.clone());
        let runtime = tokio::runtime::Handle::current();
        let interrupted = || {
            codes::busy(
                codes::ENGINE_DEGRADED,
                "The replay of the recorded fixture was interrupted.",
                "Call `fixture.record` again.",
            )
        };
        let engine_again = Arc::clone(&engine);
        let (_, got) = tokio::task::spawn_blocking(move || {
            runtime.block_on(crate::offline::run_test_with_result(
                &engine,
                &runs_plan,
                hash,
                &runs_world,
                (&first, first_binding),
                Some(Ok(crate::offline::Fixture::recorded(runs_replayed))),
            ))
        })
        .await
        .map_err(|_| interrupted())?;
        if !got.is_object() {
            return Err(codes::input(
                codes::FIXTURE_NOT_REPLAYED,
                "The read does not give a result against its scrubbed fixture, so nothing was written: a value's synthetic stand-in is one its field type refuses.",
                "Write this fixture by hand with synthetic values its field types take.",
            ));
        }
        test.expect = CanonicalJson::encode(&got)?;
        let (runs_plan, second, runs_world) = (Arc::clone(&plan), test.clone(), world.clone());
        let runtime = tokio::runtime::Handle::current();
        let (outcome, _) = tokio::task::spawn_blocking(move || {
            runtime.block_on(crate::offline::run_test_with_result(
                &engine_again,
                &runs_plan,
                hash,
                &runs_world,
                (&second, second_binding),
                Some(Ok(crate::offline::Fixture::recorded(replayed))),
            ))
        })
        .await
        .map_err(|_| interrupted())?;
        if outcome.state != "passed" {
            return Err(codes::input(
                codes::FIXTURE_NOT_REPLAYED,
                "The recorded test does not pass against its scrubbed fixture, so nothing was written.",
                "Write this fixture by hand with synthetic values its field types take.",
            ));
        }

        // The files, and the check that no original value or key is in any
        // byte.
        let inputs_json = crate::render::plain_values(&inputs);
        let unencodable = || no_rule("an answer that cannot be written as JSON");
        let mut files: Vec<(String, Vec<u8>)> =
            vec![(format!("{folder}/fixture.yaml"), MARKER.as_bytes().to_vec())];
        for (answer, file, _) in &answers {
            let mut bytes = serde_json::to_vec_pretty(file).map_err(|_| unencodable())?;
            bytes.push(b'\n');
            files.push((format!("{folder}/{answer}.json"), bytes));
        }
        files.push((
            test_path.clone(),
            test_file(
                &tool.name,
                &name,
                &inputs_json,
                &got,
                binding.as_ref().filter(|_| named),
            )
            .into_bytes(),
        ));
        let (leaked, short) = leak_check(&scrubber, &files);
        if leaked {
            return Err(codes::input(
                codes::FIXTURE_NOT_REPLAYED,
                "A value or a key the read observed would still be in a file written, so nothing was written.",
                "Write this fixture by hand with synthetic values.",
            ));
        }
        let written: Vec<String> = files.iter().map(|(path, _)| path.clone()).collect();
        let made: Arc<Mutex<Vec<Made>>> = Arc::new(Mutex::new(Vec::new()));
        {
            let (project_folder, target, folder, made) = (
                project_folder.clone(),
                target.clone(),
                folder.clone(),
                Arc::clone(&made),
            );
            crate::handlers::build::off_thread(move || {
                let at = crate::handlers::build::target_folder(&project_folder, &target)?;
                let mut made = made.lock().unwrap_or_else(PoisonError::into_inner);
                let written = write_all_or_none(&at, &folder, &files, &mut made);
                if let Err(failure) = written {
                    let left = remove_made(&made);
                    made.clear();
                    return Err(after_written(failure, &left));
                }
                Ok(())
            })
            .await?;
        }

        // Run once more from the files written, as `test.run` would. Any
        // failure from here on removes the files this call wrote.
        let checked = async {
            let compiled = crate::handlers::build::compile_in(
                kernel,
                project_folder.clone(),
                &target,
                a2m_files::CompileMode::Draft,
                a2m_files::DRAFT_VERSION,
            )
            .await?
            .map_err(a2m_core::Failure::from)?;
            let gates = crate::offline::run_gates(
                kernel,
                project_folder.clone(),
                &target,
                &compiled,
                world,
                None,
            )
            .await?;
            let state = gates.gates[1]["tests"]
                .as_array()
                .and_then(|tests| tests.iter().find(|shown| shown["test"] == name.as_str()))
                .map(|shown| shown["state"].clone())
                .unwrap_or(Json::Null);
            if state != "passed" {
                return Err(Failure::new(
                    FailureClass::Input,
                    codes::FIXTURE_NOT_REPLAYED,
                    "The recorded test does not pass when it is read back from its files.",
                    "Call `fixture.record` again; where it fails the same way, write the fixture by hand.",
                ));
            }
            Ok(())
        }
        .await;
        if let Err(failure) = checked {
            let made = Arc::clone(&made);
            let left = crate::handlers::build::off_thread(move || {
                let made = made.lock().unwrap_or_else(PoisonError::into_inner);
                Ok(remove_made(&made))
            })
            .await?;
            return Err(after_written(failure, &left));
        }
        let rules: Vec<Json> = scrubber
            .counts
            .iter()
            .map(|(rule, count)| json!({"rule": rule, "values": count}))
            .collect();
        Ok(crate::dispatch::Output::ok(json!({
            "target": target.as_str(),
            "tool": tool.name.as_str(),
            "fixture": folder,
            "test": test_path,
            "files": written,
            "scrubbed": rules,
            "test_state": "passed",
            "binding": binding.as_ref().map(a2m_engine::BindingPin::to_json),
            "trial": {"run_id": run.to_string(), "retention": "short"},
            "limits": limits(&scrubber, short),
            "review": "Every value and every key of a map the read observed was put in place of by a made-up one, by the rules named, chosen without looking at the original except where the replay needs it. A person reads the files before committing them.",
        }))
        .then(crate::dispatch::Next::new(
            "test.run",
            json!({"target": target.as_str()}),
            "to run every offline test of the target",
        )))
    }
);

use a2m_core::Failure;

#[cfg(test)]
mod tests {
    use super::*;

    fn text_rule(untrusted: bool) -> TypeRule {
        TypeRule {
            base: FieldBase::Text,
            codes: Vec::new(),
            places: None,
            untrusted,
        }
    }

    fn texts(values: &Values) -> Vec<(String, String)> {
        values
            .iter()
            .filter_map(|(name, value)| match value {
                Value::Text(text) => Some((name.to_owned(), text.clone())),
                _ => None,
            })
            .collect()
    }

    /// Audit finding FX-3: a made-up value is chosen without looking at the
    /// original. Text is of one alphabet and of its type's one length,
    /// whatever the original's letters and length; equal originals at two
    /// places are two texts, but a key field keeps one; numbers are fixed,
    /// with no sign or size kept; booleans alternate by place.
    #[test]
    fn a_made_up_value_says_nothing_of_the_original_but_what_the_replay_needs() {
        let redact = a2m_core::NoSecrets;
        let mut scrubber = Scrubber::new(&redact);
        let key = Slot {
            rule: Some(text_rule(false)),
            declared: Some(ValueType::Text),
            key: true,
        };
        let typed = |name: &str| match name {
            "order_id" => Some(key.clone()),
            "first" | "second" => Some(Slot {
                rule: Some(text_rule(false)),
                declared: Some(ValueType::Text),
                key: false,
            }),
            "note" => Some(Slot {
                rule: Some(text_rule(true)),
                declared: Some(ValueType::Text),
                key: false,
            }),
            "balance" | "count" | "open" | "shut" | "items" => Some(Slot::default()),
            _ => None,
        };
        let observed = Values::new()
            .with("order_id", "A-100")
            .with("first", "Same")
            .with("second", "Same")
            .with("note", "A rather long note of the system's own")
            .with("count", Value::Int(-4_711_000))
            .with("balance", Value::Decimal("-1830.25".parse().unwrap()))
            .with("open", Value::Bool(true))
            .with("shut", Value::Bool(true))
            .with("items", Value::List(vec![Value::Text("A-100".to_owned()); 5]));
        let scrubbed = scrubber.values(&observed, "answer", &typed).unwrap();
        let input = scrubber
            .values(&Values::new().with("order_id", "A-100"), "input", &typed)
            .unwrap();
        for (name, text) in texts(&scrubbed) {
            assert!(text.chars().all(|c| ALPHABET.contains(&c)), "{name}: {text}");
        }
        let get = |name: &str| scrubbed.get(name).cloned().unwrap();
        assert_eq!(get("order_id"), input.get("order_id").cloned().unwrap());
        assert_eq!(texts(&Values::new().with("x", get("first")))[0].1.len(), TEXT_LENGTH);
        assert_ne!(get("first"), get("second"));
        match get("note") {
            Value::Text(text) => assert_eq!(text.chars().count(), FREE_TEXT_LENGTH),
            other => panic!("{other:?}"),
        }
        assert_eq!(get("count"), Value::Int(1));
        assert_eq!(get("balance"), Value::Decimal("1".parse().unwrap()));
        assert_ne!(get("open"), get("shut"));
        // A list keeps three items, each a place of its own.
        match get("items") {
            Value::List(items) => {
                assert_eq!(items.len(), LIST_ITEMS_KEPT);
                assert_ne!(items[0], items[1]);
            }
            other => panic!("{other:?}"),
        }
        // Text of a field type built on a number has no rule.
        let money = Slot {
            rule: Some(TypeRule {
                base: FieldBase::Money,
                codes: Vec::new(),
                places: Some(2),
                untrusted: false,
            }),
            declared: Some(ValueType::Text),
            key: false,
        };
        let refused = scrubber
            .values(&Values::new().with("total", "12.50"), "answer", &|_| {
                Some(money.clone())
            })
            .unwrap_err();
        assert_eq!(refused.code(), &codes::FIXTURE_NO_SCRUB_RULE);
    }

    /// Audit finding FX-2: a choice is its type's first declared code,
    /// whatever it was, so a fixture says nothing of which it was.
    #[test]
    fn every_choice_is_its_types_first_code_whatever_it_was() {
        let redact = a2m_core::NoSecrets;
        let mut scrubber = Scrubber::new(&redact);
        let choice = Slot {
            rule: Some(TypeRule {
                base: FieldBase::Choice,
                codes: vec!["active".to_owned(), "blocked".to_owned()],
                places: None,
                untrusted: false,
            }),
            declared: Some(ValueType::Choice),
            key: false,
        };
        for original in ["active", "blocked"] {
            let chosen = scrubber
                .values(&Values::new().with("status", original), "answer", &|_| {
                    Some(choice.clone())
                })
                .unwrap();
            assert_eq!(chosen.get("status"), Some(&Value::Text("active".to_owned())));
        }
    }

    /// Audit finding FX-1: a map keyed by data has its keys put in place of,
    /// the same key by the same made-up key throughout, and the check
    /// that nothing original is written looks at keys too.
    #[test]
    fn a_maps_keys_are_data_and_never_reach_a_file() {
        let redact = a2m_core::NoSecrets;
        let mut scrubber = Scrubber::new(&redact);
        let balances = |first: &str| {
            Value::Record(
                Values::new()
                    .with(first, Value::Decimal("1830.20".parse().unwrap()))
                    .with("cust-5120@example.com", Value::Decimal("12.00".parse().unwrap())),
            )
        };
        let declared = |name: &str| {
            (name == "balances").then(|| Slot {
                rule: None,
                declared: Some(ValueType::Text),
                key: false,
            })
        };
        let one = scrubber
            .values(
                &Values::new()
                    .with("balances", balances("cust-4711@example.com"))
                    .with("cust-9@example.com", "undeclared"),
                "answer",
                &declared,
            )
            .unwrap();
        let two = scrubber
            .values(
                &Values::new().with("balances", balances("cust-4711@example.com")),
                "answer",
                &declared,
            )
            .unwrap();
        assert_eq!(one.get("balances"), two.get("balances"));
        let written = serde_json::to_vec(&one).unwrap();
        let text = String::from_utf8(written.clone()).unwrap();
        assert!(text.contains("\"balances\""), "{text}");
        assert!(!text.contains("example.com"), "{text}");
        assert!(scrubber.originals.contains("cust-4711@example.com"));
        let (leaked, _) = leak_check(&scrubber, &[("a.json".to_owned(), written)]);
        assert!(!leaked);
        // A key left as it was is found by the check.
        let (leaked, _) = leak_check(
            &scrubber,
            &[("a.json".to_owned(), b"{\"cust-4711@example.com\": 1}".to_vec())],
        );
        assert!(leaked);
    }

    /// The final check: a short original cannot be looked for and is
    /// counted, never a reason to fail; an original a made-up value happens
    /// to hold is not found in it.
    #[test]
    fn the_leak_check_counts_what_it_cannot_look_for_and_reads_past_made_up_values() {
        let redact = a2m_core::NoSecrets;
        let mut scrubber = Scrubber::new(&redact);
        scrubber
            .values(&Values::new().with("a", "ab").with("b", "zjj"), "answer", &|_| {
                Some(Slot::default())
            })
            .unwrap();
        let made: Vec<String> = scrubber.synthetics.iter().cloned().collect();
        let bytes = made.join(" ").into_bytes();
        let (leaked, short) = leak_check(&scrubber, &[("a.json".to_owned(), bytes)]);
        assert!(!leaked);
        assert_eq!(short, 1);
    }

    /// Audit findings FX-4 and FX-5: every file is made new at the moment it
    /// is written, and a failure after the first file was written removes
    /// what this call made; what stood there before is left as it was.
    #[test]
    fn a_recording_writes_all_its_files_or_none_and_never_over_one_that_appeared() {
        let folder = std::env::temp_dir().join(format!("a2m-record-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("tests")).unwrap();
        // A person wrote the test file while the read was being recorded.
        std::fs::write(folder.join("tests").join("one.yaml"), "kept: true\n").unwrap();
        let mut roots = a2m_provider::Roots::none();
        let name = a2m_provider::RootName::new("target").unwrap();
        roots
            .add(name.clone(), &folder, a2m_provider::Access::Write)
            .unwrap();
        let at = roots.folder(&name, a2m_provider::Access::Write).unwrap();
        let files = vec![
            ("fixtures/one/fixture.yaml".to_owned(), MARKER.as_bytes().to_vec()),
            ("fixtures/one/order_get.json".to_owned(), b"{}\n".to_vec()),
            ("tests/one.yaml".to_owned(), b"test: made\n".to_vec()),
        ];
        let mut made = Vec::new();
        let refused = write_all_or_none(&at, "fixtures/one", &files, &mut made).unwrap_err();
        assert!(refused.message().contains("appeared"), "{}", refused.message());
        assert!(remove_made(&made).is_empty());
        assert!(!folder.join("fixtures").exists());
        assert_eq!(
            std::fs::read_to_string(folder.join("tests").join("one.yaml")).unwrap(),
            "kept: true\n"
        );
        // A link at a file's name is never followed or replaced.
        #[cfg(unix)]
        {
            std::fs::remove_file(folder.join("tests").join("one.yaml")).unwrap();
            let elsewhere = folder.join("elsewhere.yaml");
            std::fs::write(&elsewhere, "outside\n").unwrap();
            std::os::unix::fs::symlink(&elsewhere, folder.join("tests").join("one.yaml")).unwrap();
            let mut made = Vec::new();
            assert!(write_all_or_none(&at, "fixtures/one", &files, &mut made).is_err());
            assert!(remove_made(&made).is_empty());
            assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), "outside\n");
            std::fs::remove_file(folder.join("tests").join("one.yaml")).unwrap();
        }
        // With nothing in the way, all are written.
        #[cfg(not(unix))]
        std::fs::remove_file(folder.join("tests").join("one.yaml")).unwrap();
        let mut made = Vec::new();
        write_all_or_none(&at, "fixtures/one", &files, &mut made).unwrap();
        assert_eq!(
            std::fs::read_to_string(folder.join("tests").join("one.yaml")).unwrap(),
            "test: made\n"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }
}
