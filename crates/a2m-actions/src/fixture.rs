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
    TestPlan, Timestamp, ToolRuns, Value, Values,
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

/// The letters synthetic text is made of, less those of the original.
const LETTERS: &str = "abcdefghijklmnopqrstuvwxyz";

/// The digits synthetic text is made of where an original holds nearly
/// every letter.
const DIGITS: &str = "0123456789";

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

/// What scrubbing reads of a value's field type: what it is built on, and
/// its declared codes for a choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypeRule {
    base: FieldBase,
    codes: Vec<String>,
}

impl TypeRule {
    fn of(ty: &FieldTypePlan) -> Self {
        Self {
            base: ty.base,
            codes: ty.codes.iter().map(|code| code.code.clone()).collect(),
        }
    }
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

/// Puts synthetic values in place of observed ones, by fixed rules:
///
/// - `null` and `true`/`false` are kept;
/// - text, of a field type built on text or of no field type, by text of
///   the same length class (empty; up to 4, 8, 16, 32, 64 characters;
///   128 above) made of the letters that are not in the original, or the
///   digits where fewer than two letters are left, so that no character of
///   the original is kept; the same original gives the same synthetic text
///   everywhere in the fixture and the test's input, and two originals
///   never give the same one, so that a key stays one key;
/// - a choice, by the first declared code that is not the original;
/// - a whole number, by `10^(d-1) + k` of the same sign and number of
///   digits `d`, for the `k`-th distinct number of that many digits, and
///   never the original;
/// - an exact number, by `1` and zeros of the same number of whole digits
///   and decimal places (`2` and zeros where that is the original);
/// - a date, a moment, and a length of time, by fixed ones
///   (2000-01-01, 2000-01-01T00:00:00Z, one second);
/// - a list, by its first [`LIST_ITEMS_KEPT`] items, each scrubbed by the
///   list's field type, which is its items'; a record, field by field, its
///   names kept.
///
/// Text of a field type built on a number, a date, or a time has no rule,
/// and stops the action.
pub(crate) struct Scrubber<'r> {
    redact: &'r dyn Redact,
    texts: BTreeMap<String, String>,
    used: BTreeSet<String>,
    ints: BTreeMap<i64, i64>,
    /// How many values each rule scrubbed, by rule.
    pub(crate) counts: BTreeMap<&'static str, u32>,
    /// Every original text, for the check that none is in what is written.
    pub(crate) originals: BTreeSet<String>,
}

impl<'r> Scrubber<'r> {
    pub(crate) fn new(redact: &'r dyn Redact) -> Self {
        Self {
            redact,
            texts: BTreeMap::new(),
            used: BTreeSet::new(),
            ints: BTreeMap::new(),
            counts: BTreeMap::new(),
            originals: BTreeSet::new(),
        }
    }

    fn count(&mut self, rule: &'static str) {
        *self.counts.entry(rule).or_insert(0) += 1;
    }

    /// The values of a record, each by the field type of its name, where
    /// `types` names one.
    pub(crate) fn values(
        &mut self,
        values: &Values,
        types: &dyn Fn(&str) -> Option<TypeRule>,
    ) -> Result<Values, a2m_core::Failure> {
        let mut out = Values::new();
        for (name, value) in values.iter() {
            let ty = types(name);
            out.insert(name.to_owned(), self.value(value, ty.as_ref())?);
        }
        Ok(out)
    }

    fn value(&mut self, value: &Value, ty: Option<&TypeRule>) -> Result<Value, a2m_core::Failure> {
        Ok(match value {
            Value::Null => {
                self.count("null kept");
                Value::Null
            }
            Value::Bool(flag) => {
                self.count("true or false kept");
                Value::Bool(*flag)
            }
            Value::Text(text) => {
                let text = self.redact.redact(text);
                match ty.map(|ty| ty.base) {
                    Some(FieldBase::Choice) => {
                        let codes = ty.map(|ty| ty.codes.clone()).unwrap_or_default();
                        let code = codes
                            .into_iter()
                            .find(|code| *code != text)
                            .ok_or_else(|| no_rule("a choice with no other code"))?;
                        // A code is one the target's own field type
                        // declares, never text of the system's: the codes
                        // stand for one another, so the code observed is
                        // not looked for in what is written (decision
                        // 0199), where another code may stand for it.
                        self.count("choice by another declared code");
                        Value::Text(code)
                    }
                    None | Some(FieldBase::Text) => {
                        self.count("text by synthetic text of its length class");
                        Value::Text(self.text(&text)?)
                    }
                    Some(base) => {
                        return Err(no_rule(&format!("text of a field type built on `{base}`")));
                    }
                }
            }
            Value::Int(number) => {
                self.count("whole number by a fixed number of its digits");
                Value::Int(self.int(*number))
            }
            Value::Decimal(number) => {
                self.count("exact number by a fixed number of its scale");
                Value::Decimal(decimal_of_scale(number)?)
            }
            Value::Date(_) => {
                self.count("date by a fixed date");
                let (year, month, day) = FIXED_DATE;
                Value::Date(Date::new(year, month, day).map_err(|_| no_rule("a date"))?)
            }
            Value::Instant(_) => {
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
                let mut out = Vec::new();
                for item in items.iter().take(LIST_ITEMS_KEPT) {
                    out.push(self.value(item, ty)?);
                }
                Value::List(out)
            }
            Value::Record(values) => Value::Record(self.values(values, &|_| None)?),
        })
    }

    fn text(&mut self, original: &str) -> Result<String, a2m_core::Failure> {
        if let Some(synthetic) = self.texts.get(original) {
            return Ok(synthetic.clone());
        }
        let length = original.chars().count();
        let class = match length {
            0 => 0,
            1..=4 => 4,
            5..=8 => 8,
            9..=16 => 16,
            17..=32 => 32,
            33..=64 => 64,
            _ => 128,
        };
        if class == 0 {
            self.texts.insert(String::new(), String::new());
            return Ok(String::new());
        }
        let lower = original.to_lowercase();
        let free =
            |set: &str| -> Vec<char> { set.chars().filter(|c| !lower.contains(*c)).collect() };
        let mut alphabet = free(LETTERS);
        if alphabet.len() < 2 {
            alphabet = free(DIGITS);
        }
        if alphabet.len() < 2 {
            return Err(no_rule(
                "text that holds nearly every letter and every digit",
            ));
        }
        let mut index = self.texts.len();
        let synthetic = loop {
            let candidate = encode(index, &alphabet, class);
            if !self.used.contains(&candidate) {
                break candidate;
            }
            index += 1;
        };
        self.originals.insert(original.to_owned());
        self.used.insert(synthetic.clone());
        self.texts.insert(original.to_owned(), synthetic.clone());
        Ok(synthetic)
    }

    fn int(&mut self, number: i64) -> i64 {
        if let Some(synthetic) = self.ints.get(&number) {
            return *synthetic;
        }
        let digits = number
            .unsigned_abs()
            .checked_ilog10()
            .map_or(1, |log| log + 1);
        let base = 10_i64.saturating_pow(digits.saturating_sub(1));
        let sign = if number < 0 { -1 } else { 1 };
        let mut k = self
            .ints
            .keys()
            .filter(|earlier| {
                earlier
                    .unsigned_abs()
                    .checked_ilog10()
                    .map_or(1, |log| log + 1)
                    == digits
            })
            .count();
        let synthetic = loop {
            let candidate = sign * base.saturating_add(i64::try_from(k).unwrap_or(i64::MAX));
            if candidate != number && !self.ints.values().any(|used| *used == candidate) {
                break candidate;
            }
            k += 1;
        };
        self.ints.insert(number, synthetic);
        synthetic
    }
}

/// `index` written in the digits of `alphabet`, filled on the left with its
/// first digit to `length` characters.
fn encode(mut index: usize, alphabet: &[char], length: usize) -> String {
    let mut out = vec![alphabet[0]; length];
    let mut at = length;
    while index > 0 && at > 0 {
        at -= 1;
        out[at] = alphabet[index % alphabet.len()];
        index /= alphabet.len();
    }
    out.into_iter().collect()
}

/// An exact number of the same number of whole digits and decimal places,
/// `1` and zeros, or `2` and zeros where that is the original; below one,
/// `0.0…1` of the same places, or `0.0…2`.
fn decimal_of_scale(number: &Decimal) -> Result<Decimal, a2m_core::Failure> {
    let text = number.to_string();
    let unsigned = text.trim_start_matches('-');
    let (whole, places) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let whole_digits = whole.trim_start_matches('0').len().max(1);
    let fraction_only = whole.trim_start_matches('0').is_empty() && !places.is_empty();
    let make = |first: char| {
        if fraction_only {
            // Below one: `0.0…1`, or `0.0…2`, of the same places.
            return format!("0.{}{first}", "0".repeat(places.len() - 1));
        }
        let mut out = String::new();
        out.push(first);
        out.push_str(&"0".repeat(whole_digits - 1));
        if !places.is_empty() {
            out.push('.');
            out.push_str(&"0".repeat(places.len()));
        }
        out
    };
    let mut synthetic = make('1');
    if synthetic == unsigned {
        synthetic = make('2');
    }
    synthetic
        .parse::<Decimal>()
        .map_err(|_| no_rule("an exact number"))
}

/// The field type of each output of the operation, by name.
fn output_types(plan: &Plan, operation: &OperationPlan) -> BTreeMap<String, TypeRule> {
    operation
        .outputs
        .iter()
        .filter_map(|output| {
            let ty = plan.field_type(output.field_type?)?;
            Some((output.name.clone(), TypeRule::of(ty)))
        })
        .collect()
}

/// The field type of each input of the operation, by name.
fn input_types(plan: &Plan, operation: &OperationPlan) -> BTreeMap<String, TypeRule> {
    operation
        .inputs
        .iter()
        .filter_map(|input| {
            let ty = plan.field_type(input.field_type)?;
            Some((input.name.clone(), TypeRule::of(ty)))
        })
        .collect()
}

/// The recorded answers, scrubbed, by name in the order each name was first
/// asked: one object where every answer of the name is the same, which
/// answers every step of it, else a list, given in order (decision 0180).
pub(crate) fn scrubbed_answers(
    scrubber: &mut Scrubber<'_>,
    observed: &[(String, Option<Values>)],
    types: &BTreeMap<String, TypeRule>,
) -> Result<Vec<(String, Json, Vec<RecordedAnswer>)>, a2m_core::Failure> {
    let mut names: Vec<&str> = Vec::new();
    for (name, _) in observed {
        if !names.contains(&name.as_str()) {
            names.push(name);
        }
    }
    let lookup = |name: &str| types.get(name).cloned();
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
                Some(values) => Recorded::Done(scrubber.values(values, &lookup)?),
                None => Recorded::Absent,
            });
        }
        let repeats = scrubbed.windows(2).all(|pair| pair[0] == pair[1]);
        let json_of = |answer: &Recorded| match answer {
            Recorded::Done(values) => serde_json::to_value(values).unwrap_or(Json::Null),
            Recorded::Absent => Json::Null,
        };
        let (file, replayed) = if repeats {
            let one = scrubbed.first().cloned().unwrap_or(Recorded::Absent);
            (
                json_of(&one),
                vec![RecordedAnswer {
                    name: name.to_owned(),
                    args: None,
                    answer: one,
                    repeats: true,
                }],
            )
        } else {
            (
                Json::Array(scrubbed.iter().map(json_of).collect()),
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
/// expectation are written as JSON, which the compiler reads.
fn test_file(tool: &OperationName, name: &str, inputs: &Json, expect: &Json) -> String {
    format!(
        "# Recorded by `fixture.record` from one read, every value scrubbed by rule.\n\
         # A person reads this file and its fixture before committing them.\n\
         test: {tool}\nkind: offline\nfixture: {name}\ninputs: {inputs}\nexpect: {expect}\n"
    )
}

const MARKER: &str = "scrubbed: true\nnote: \"Recorded by fixture.record from one read; every value was put in place of by a synthetic one, by the rules the action names. A person reads the files before committing them.\"\n";

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
        // there is refused before the read runs.
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
        kernel.keep_draft(&target, compiled.encoded.hash, Arc::clone(&plan));
        let recorder = Recorder::start(kernel, digest);
        let run = crate::handlers::tools::submit_trial_as(
            kernel,
            &call,
            &found,
            &input,
            environment,
            digest,
            binding.clone(),
        )
        .await?;
        let row = kernel
            .wait_for_rest(run, crate::handlers::tools::wait_of(&call))
            .await?;
        let observed = recorder.taken();
        drop(recorder);
        if row.state != RunState::Completed {
            return Err(codes::input(
                codes::FIXTURE_READ_FAILED,
                format!(
                    "The read did not complete in the sandbox (`{}`{}), so nothing was recorded.",
                    row.state,
                    row.failure
                        .as_ref()
                        .map(|(class, code)| format!(", {class}/{code}"))
                        .unwrap_or_default()
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
        // in the input and in every answer.
        let redact = kernel.redact();
        let mut scrubber = Scrubber::new(redact.as_ref());
        let inputs_typed = input_types(&plan, &operation);
        let inputs = scrubber.values(&input, &|name| inputs_typed.get(name).cloned())?;
        let answers = scrubbed_answers(&mut scrubber, &observed, &output_types(&plan, &operation))?;

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

        // The files, and the check that no original text is in any byte.
        let inputs_json = crate::render::plain_values(&inputs);
        let mut files: Vec<(String, Vec<u8>)> =
            vec![(format!("{folder}/fixture.yaml"), MARKER.as_bytes().to_vec())];
        for (answer, file, _) in &answers {
            let mut bytes = serde_json::to_vec_pretty(file).unwrap_or_default();
            bytes.push(b'\n');
            files.push((format!("{folder}/{answer}.json"), bytes));
        }
        files.push((
            test_path.clone(),
            test_file(&tool.name, &name, &inputs_json, &got).into_bytes(),
        ));
        let leaked = scrubber
            .originals
            .iter()
            .filter(|original| original.chars().count() >= 3)
            .any(|original| {
                files
                    .iter()
                    .any(|(_, bytes)| String::from_utf8_lossy(bytes).contains(original.as_str()))
            });
        if leaked {
            return Err(codes::input(
                codes::FIXTURE_NOT_REPLAYED,
                "A value the read observed would still be in a file written, so nothing was written.",
                "Write this fixture by hand with synthetic values.",
            ));
        }
        let written: Vec<String> = files.iter().map(|(path, _)| path.clone()).collect();
        {
            let (project_folder, target, folder) =
                (project_folder.clone(), target.clone(), folder.clone());
            crate::handlers::build::off_thread(move || {
                let at = crate::handlers::build::target_folder(&project_folder, &target)?;
                at.join(&folder)?.create_dir_all()?;
                for (path, bytes) in &files {
                    let file = at.join(path)?;
                    if let Some(parent) = path.rsplit_once('/').map(|(parent, _)| parent) {
                        at.join(parent)?.create_dir_all()?;
                    }
                    file.write(bytes)?;
                }
                Ok(())
            })
            .await?;
        }

        // Run once more from the files written, as `test.run` would.
        let compiled = crate::handlers::build::compile_in(
            kernel,
            project_folder.clone(),
            &target,
            a2m_files::CompileMode::Draft,
            a2m_files::DRAFT_VERSION,
        )
        .await?
        .map_err(a2m_core::Failure::from)?;
        let gates =
            crate::offline::run_gates(kernel, project_folder, &target, &compiled, world, None).await?;
        let state = gates.gates[1]["tests"]
            .as_array()
            .and_then(|tests| tests.iter().find(|shown| shown["test"] == name.as_str()))
            .map(|shown| shown["state"].clone())
            .unwrap_or(Json::Null);
        if state != "passed" {
            return Err(Failure::new(
                FailureClass::Input,
                codes::FIXTURE_NOT_REPLAYED,
                "The recorded test was written and does not pass when it is read back from its files; the files are left for a person to look at.",
                "Call `test.run` to see what the test found, and correct or remove the files.",
            ));
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
            "review": "Every value the read observed was put in place of by a synthetic one, by the rules named. A person reads the files before committing them.",
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

    /// The fixed rules: nothing of an original text is kept, the same
    /// original gives the same synthetic value and two never give one,
    /// numbers keep their scale, and a type with no rule stops.
    #[test]
    fn every_observed_value_is_put_in_place_of_by_a_synthetic_one_of_its_shape() {
        let redact = a2m_core::NoSecrets;
        let mut scrubber = Scrubber::new(&redact);
        let observed = Values::new()
            .with("order_id", "A-100")
            .with("customer", "Example Customer One")
            .with("other", "A-101")
            .with("lines", Value::Int(57))
            .with("open", Value::Bool(true))
            .with("note", Value::Null)
            .with(
                "items",
                Value::List(vec![Value::Text("A-100".to_owned()); 5]),
            );
        let scrubbed = scrubber.values(&observed, &|_| None).unwrap();
        let text = |name: &str| match scrubbed.get(name) {
            Some(Value::Text(text)) => text.clone(),
            other => panic!("{other:?}"),
        };
        let id = text("order_id");
        assert_eq!(id.chars().count(), 8);
        assert!(!id.chars().any(|c| "a-100".contains(c)), "{id}");
        assert_ne!(text("other"), id);
        assert_eq!(text("customer").chars().count(), 32);
        assert_eq!(scrubbed.get("lines"), Some(&Value::Int(10)));
        assert_eq!(scrubbed.get("open"), Some(&Value::Bool(true)));
        assert_eq!(scrubbed.get("note"), Some(&Value::Null));
        // A list keeps its first items, and a key the same synthetic value.
        assert_eq!(
            scrubbed.get("items"),
            Some(&Value::List(vec![Value::Text(id.clone()); LIST_ITEMS_KEPT]))
        );
        let scaled = |text: &str| decimal_of_scale(&text.parse().unwrap()).unwrap();
        assert_eq!(scaled("125.25"), "100.00".parse().unwrap());
        assert_eq!(scaled("100.00"), "200".parse().unwrap());
        assert_eq!(scaled("0.75"), "0.01".parse().unwrap());
        // Text of a field type built on a number has no rule.
        let money = TypeRule {
            base: FieldBase::Money,
            codes: Vec::new(),
        };
        let refused = scrubber
            .values(&Values::new().with("total", "12.50"), &|_| {
                Some(money.clone())
            })
            .unwrap_err();
        assert_eq!(refused.code(), &codes::FIXTURE_NO_SCRUB_RULE);
        // A choice by another of its codes.
        let choice = TypeRule {
            base: FieldBase::Choice,
            codes: vec!["draft".to_owned(), "submitted".to_owned()],
        };
        let chosen = scrubber
            .values(&Values::new().with("status", "draft"), &|_| {
                Some(choice.clone())
            })
            .unwrap();
        assert_eq!(
            chosen.get("status"),
            Some(&Value::Text("submitted".to_owned()))
        );
    }
}
