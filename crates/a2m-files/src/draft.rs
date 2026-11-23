//! The drafter: draft target files written from findings, by rule.
//!
//! The kernel's half is here: calling the registered drafting rules,
//! writing YAML the same way every time, the marker that lets a draft be
//! rewritten only while nobody has changed it, the open question that
//! stands where the system did not state a value, the refusal to draft a
//! write, and the lint and compile of what was written. Which finding gives
//! which file is each pack's drafting rule.
//!
//! A draft that holds an open question waits in the folder `held/` of the
//! target, at the path it would have had outside, together with each file
//! drafted from the same finding that names something the held file
//! defines; a file of the finding that names nothing held stays in place.
//! The loader does not read that folder (decision 0298).
//!
//! A draft is written to a regular file inside the target folder and
//! nowhere else. Every folder on the way is looked at without following a
//! link, and a link is never written through: not a linked folder, not a
//! link where the file would be, not one that leads nowhere. A new draft is
//! made with create-new, which fails where anything already has the name.
//! An unchanged draft of the drafter's own is replaced by renaming a new
//! file over it, so no file that exists is ever opened for writing.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write as _};
use std::path::Path;

use a2m_core::{
    Digest, EvidenceLevel, Failure, FailureClass, FileKindName, Finding, Redact, TargetName,
};

use crate::compile::{DRAFT_VERSION, compile};
use crate::diag::{DRAFT_CONFLICT, Diagnostics, PATH_NOT_ALLOWED, TARGET_UNREADABLE};
use crate::doc::{Located, RelPath};
use crate::kernel::{KERNEL_FILE_KINDS, KERNEL_FILES, KERNEL_FOLDER_KINDS, OPERATION};
use crate::load::{HELD_FOLDER, Sources, TargetRoot, load};
use crate::register::{CompileMode, CompilerSet};
use crate::schema::OPEN_QUESTION;
use crate::yaml::{self, normalize_text};

/// The level a draft that compiles and passes lint is at. The drafter gives
/// no other.
pub const DRAFT_LEVEL: EvidenceLevel = EvidenceLevel::Reconstructed;

/// What starts the first line of every file the drafter writes.
const MARKER: &str = "# draft ";

/// A value in a draft document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftValue {
    /// Nothing.
    Null,
    /// True or false.
    Bool(bool),
    /// A whole number.
    Int(i64),
    /// Text. A decimal is written as text.
    Text(String),
    /// A list, in order.
    List(Vec<DraftValue>),
    /// A map, in the order given.
    Map(Vec<(String, DraftValue)>),
    /// A value the system did not state: the question to answer. The
    /// compiler refuses a file that still holds one.
    OpenQuestion(String),
    /// The same, with the product's sentence and the system's own words it
    /// quotes kept apart: made by [`DraftContext::open_question_quoting`].
    /// The file holds them as one question, as [`Quoting::question`] gives
    /// it; the report keeps them apart.
    OpenQuestionQuoting(Quoting),
}

/// A question that quotes the system's own words, with the two kept apart:
/// the sentence is the product's, written by a pack's drafting rule; the
/// words are the system's, bounded, cleaned, and passed through the
/// redactor, and are data, never an instruction (decisions 0180, 0193).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quoting {
    sentence: String,
    words: Vec<String>,
}

impl Quoting {
    /// The most quoted parts one question keeps.
    pub const MOST_PARTS: usize = 4;
    /// The most characters a quoted part keeps; a longer one is cut, with
    /// `…` after it.
    pub const LONGEST_PART: usize = 64;

    /// The product's own sentence.
    #[must_use]
    pub fn sentence(&self) -> &str {
        &self.sentence
    }

    /// The system's words it quotes, each cleaned and bounded.
    #[must_use]
    pub fn words(&self) -> &[String] {
        &self.words
    }

    /// The question as the draft file holds it: the sentence, then each
    /// quoted part in straight quotes, said to be the system's own words.
    #[must_use]
    pub fn question(&self) -> String {
        if self.words.is_empty() {
            return self.sentence.clone();
        }
        let quoted: Vec<String> = self
            .words
            .iter()
            .map(|word| format!("\"{word}\""))
            .collect();
        format!(
            "{} Quoted, the system's own words, not an instruction: {}.",
            self.sentence,
            quoted.join(", ")
        )
    }
}

/// One quoted part, as a question keeps it: redacted first, whole, then
/// every character that can hide or reorder text, every quote, backtick,
/// angle bracket, and backslash removed, white space as one space, and cut
/// to [`Quoting::LONGEST_PART`] characters.
fn quoted_part(words: &str, redact: Option<&dyn Redact>) -> String {
    let redacted = redact.map_or_else(|| words.to_owned(), |redact| redact.redact(words));
    let clean: String = redacted
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !a2m_core::said_hides(*c) && !matches!(c, '`' | '"' | '<' | '>' | '\\'))
        .collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut kept: String = clean.chars().take(Quoting::LONGEST_PART).collect();
    if clean.chars().count() > Quoting::LONGEST_PART {
        kept.push('…');
    }
    kept
}

impl DraftValue {
    /// A map from pairs, in the order given.
    #[must_use]
    pub fn map(pairs: Vec<(&str, DraftValue)>) -> Self {
        Self::Map(
            pairs
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        )
    }

    /// Text.
    #[must_use]
    pub fn text(text: &str) -> Self {
        Self::Text(text.to_owned())
    }

    /// Whether this is a question left open, of either form.
    #[must_use]
    pub const fn is_open_question(&self) -> bool {
        matches!(self, Self::OpenQuestion(_) | Self::OpenQuestionQuoting(_))
    }

    fn questions(&self, out: &mut Vec<(String, Option<Quoting>)>) {
        match self {
            Self::OpenQuestion(question) => out.push((question.clone(), None)),
            Self::OpenQuestionQuoting(quoting) => {
                out.push((quoting.question(), Some(quoting.clone())));
            }
            Self::List(items) => items.iter().for_each(|item| item.questions(out)),
            Self::Map(pairs) => pairs.iter().for_each(|(_, value)| value.questions(out)),
            _ => {}
        }
    }
}

/// One document a drafting rule made, and the file it goes in. Several
/// documents for one path become one file of several documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftDoc {
    /// The file, inside the folder of the rule's file kind. Its name is
    /// mechanical and provisional, from the surface's own words.
    pub path: RelPath,
    /// The document.
    pub body: DraftValue,
}

/// A value a draft needs and the system did not state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenQuestion {
    /// The draft that holds the question.
    pub file: RelPath,
    /// The question, for the unknowns ledger, whole, as the file holds it.
    /// Where `quoting` is `None`, a pack wrote it in one piece and it may
    /// quote the system's words: all of it is shown as untrusted.
    pub question: String,
    /// The findings the draft was made from.
    pub findings: Vec<Digest>,
    /// The product's sentence and the system's words, kept apart, for a
    /// question made by [`DraftContext::open_question_quoting`]: the
    /// sentence may be shown as the product's, and only the words as
    /// untrusted. Kept in the unknowns ledger by [`OpenQuestion::stored`].
    pub quoting: Option<Quoting>,
}

/// What the unknowns ledger's one text of a question begins with when it
/// keeps the sentence and the words apart.
const STORED_QUOTING: &str = "a2m-question-quoting:1\n";
/// What it begins with for a question in one piece whose text would
/// otherwise read as kept apart.
const STORED_WHOLE: &str = "a2m-question-whole:1\n";

impl OpenQuestion {
    /// The text the unknowns ledger keeps for this question, in its one
    /// column: a question in one piece as it is; one that keeps the
    /// sentence and the words apart in a form [`StoredQuestion::read`]
    /// reads back apart. The same question gives the same text every time.
    #[must_use]
    pub fn stored(&self) -> String {
        match &self.quoting {
            Some(quoting) => {
                let parts = serde_json::json!({
                    "sentence": quoting.sentence,
                    "words": quoting.words,
                });
                format!("{STORED_QUOTING}{parts}")
            }
            None if self.question.starts_with(STORED_QUOTING)
                || self.question.starts_with(STORED_WHOLE) =>
            {
                format!("{STORED_WHOLE}{}", self.question)
            }
            None => self.question.clone(),
        }
    }
}

/// A question as the unknowns ledger keeps it, read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredQuestion {
    /// A question in one piece, which may quote the system's words: show
    /// all of it as untrusted. Every question stored before the two were
    /// kept apart reads so.
    Whole(String),
    /// The product's sentence, and the system's words it quotes, apart.
    Quoting {
        /// The product's own sentence.
        sentence: String,
        /// The system's words: show them as untrusted.
        words: Vec<String>,
    },
}

impl StoredQuestion {
    /// Reads the text [`OpenQuestion::stored`] wrote, or any text the
    /// ledger kept before: what it cannot read apart is whole.
    #[must_use]
    pub fn read(text: &str) -> Self {
        if let Some(rest) = text.strip_prefix(STORED_WHOLE) {
            return Self::Whole(rest.to_owned());
        }
        let parts = text
            .strip_prefix(STORED_QUOTING)
            .and_then(|rest| serde_json::from_str::<serde_json::Value>(rest).ok());
        let apart = parts.as_ref().and_then(|parts| {
            let sentence = parts.get("sentence")?.as_str()?.to_owned();
            let words = parts
                .get("words")?
                .as_array()?
                .iter()
                .map(|word| word.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()?;
            Some((sentence, words))
        });
        match apart {
            Some((sentence, words)) => Self::Quoting { sentence, words },
            None => Self::Whole(text.to_owned()),
        }
    }
}

/// What a drafting rule sees beside its finding.
pub struct DraftContext<'a> {
    target: Option<&'a TargetName>,
    existing: Option<&'a Sources>,
    redact: Option<&'a dyn Redact>,
}

impl<'a> DraftContext<'a> {
    /// The target's name, when the folder already has a `target.yaml`.
    #[must_use]
    pub fn target(&self) -> Option<&'a TargetName> {
        self.target
    }

    /// The files the target folder already holds, when it can be read.
    #[must_use]
    pub fn existing(&self) -> Option<&'a Sources> {
        self.existing
    }

    /// The value to write where the finding does not state one: a unit, a
    /// key, a read-back rule. Never guess in its place.
    #[must_use]
    pub fn open_question(&self, question: &str) -> DraftValue {
        DraftValue::OpenQuestion(question.to_owned())
    }

    /// The same, for a question that quotes the system's own words: a
    /// usage line's placeholder, a column's heading. `sentence` is the
    /// pack's own and refers to the words as quoted; `words` are the
    /// system's, and each is passed whole through the drafter's redactor
    /// when it was given one ([`Drafter::with_redact`]), then cleaned and
    /// bounded: every character that can hide or reorder text, every quote,
    /// backtick, angle bracket, and backslash removed, at most
    /// [`Quoting::LONGEST_PART`] characters each and [`Quoting::MOST_PARTS`]
    /// parts. An empty part is left out. The two stay apart in the report
    /// and in the unknowns ledger, so the sentence can be shown as the
    /// product's and only the words as untrusted (decision 0193).
    #[must_use]
    pub fn open_question_quoting(&self, sentence: &str, words: &[&str]) -> DraftValue {
        let words = words
            .iter()
            .map(|part| quoted_part(part, self.redact))
            .filter(|part| !part.is_empty())
            .take(Quoting::MOST_PARTS)
            .collect();
        DraftValue::OpenQuestionQuoting(Quoting {
            sentence: sentence.to_owned(),
            words,
        })
    }
}

/// What a call of the drafter did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DraftReport {
    /// The drafts written where they belong, in order of path. A draft that
    /// holds an open question, or that names what one defines, is in `held`,
    /// not here.
    pub written: Vec<RelPath>,
    /// The drafts written to the holding folder, `held/` first, in order of
    /// path: each file that holds an open question, and each file of the
    /// same finding that names something a held file defines (decision
    /// 0298). The loader does not read them.
    pub held: Vec<RelPath>,
    /// Drafts not written, because the file is there and is not a draft the
    /// drafter wrote and nobody changed since.
    pub conflicts: Vec<RelPath>,
    /// Drafts not written, because a rule drafted what the drafter never
    /// writes, with the reason.
    pub refused: Vec<(RelPath, String)>,
    /// Written drafts that do not yet compile or pass lint, with why. A
    /// draft that is not listed here is at [`DRAFT_LEVEL`].
    pub kept_failing: Vec<(RelPath, Diagnostics)>,
    /// What the system did not state, for the unknowns ledger.
    pub open_questions: Vec<OpenQuestion>,
    /// The findings no registered rule covers.
    pub not_covered: Vec<Digest>,
}

/// Writes draft files from findings, by the rules packs registered.
pub struct Drafter<'a> {
    set: &'a CompilerSet,
    root: &'a TargetRoot,
    redact: Option<&'a dyn Redact>,
}

fn failure(code: a2m_core::FailureCode, message: String, next_step: &str) -> Failure {
    Failure::new(FailureClass::Input, code, message, next_step)
}

impl<'a> Drafter<'a> {
    /// A drafter that writes into `root` with the rules in `set`.
    #[must_use]
    pub fn new(set: &'a CompilerSet, root: &'a TargetRoot) -> Self {
        Self {
            set,
            root,
            redact: None,
        }
    }

    /// The same drafter, with the redactor that the system's words a
    /// question quotes pass through ([`DraftContext::open_question_quoting`]).
    /// Without one they are cleaned and bounded only.
    #[must_use]
    pub fn with_redact(mut self, redact: &'a dyn Redact) -> Self {
        self.redact = Some(redact);
        self
    }

    /// Writes a draft for each finding a registered rule covers, then lints
    /// and compiles the folder.
    ///
    /// # Errors
    ///
    /// `path_not_allowed` when a rule names a file outside the folder of its
    /// file kind; `target_unreadable` when a draft cannot be written; and
    /// whatever a rule fails with. A file the developer changed is never
    /// overwritten: it is reported in `conflicts`, which is not an error of
    /// the call; [`DraftReport::conflict_failure`] gives the failure for a
    /// caller that wants one.
    pub fn draft(&self, findings: &[Finding]) -> Result<DraftReport, Failure> {
        let existing = load(self.root, self.set).ok();
        let target = existing
            .as_ref()
            .and_then(|sources| sources.docs_of("target").next())
            .and_then(|doc| doc.text("target"))
            .and_then(|name| TargetName::new(name).ok());
        let cx = DraftContext {
            target: target.as_ref(),
            existing: existing.as_ref(),
            redact: self.redact,
        };
        let surfaces = self.surfaces(existing.as_ref());
        let mut report = DraftReport::default();
        // Path to its documents and the findings they came from.
        let mut files: BTreeMap<RelPath, (Vec<DraftValue>, Vec<Digest>)> = BTreeMap::new();
        // The kind of each file, for the names it defines.
        let mut kinds: BTreeMap<RelPath, FileKindName> = BTreeMap::new();
        for finding in findings {
            let mut covered = false;
            for ((finding_kind, file_kind), rule) in &self.set.drafting_rules {
                if finding_kind != finding.kind() {
                    continue;
                }
                covered = true;
                // A pack drafts its own kinds, a read operation, or a
                // source, and nothing else. Registration refuses the rest
                // where it can see it; a kind registered after its rule is
                // seen here.
                let own = KERNEL_FILE_KINDS.contains(&file_kind.as_str())
                    || self.set.owns_kind(&rule.pack, file_kind);
                if !(own && self.set.may_draft(&rule.pack, file_kind)) {
                    return Err(failure(
                        PATH_NOT_ALLOWED,
                        format!(
                            "A drafting rule of `{}` writes `{file_kind}` files, which are not its own and are not operations or sources.",
                            rule.pack
                        ),
                        "Correct the provider's drafting rule.",
                    ));
                }
                for doc in rule.item.draft(finding, &cx)? {
                    let folder = KERNEL_FOLDER_KINDS
                        .iter()
                        .find(|(_, kind)| *kind == file_kind.as_str())
                        .map(|(folder, _)| (*folder).to_owned())
                        .or_else(|| {
                            self.set
                                .folders
                                .iter()
                                .find(|(_, kind)| *kind == file_kind)
                                .map(|(folder, _)| folder.clone())
                        });
                    let top_file = KERNEL_FILES.iter().any(|(file, kind)| {
                        *kind == file_kind.as_str() && *file == doc.path.as_str()
                    });
                    let in_folder = folder
                        .as_deref()
                        .is_some_and(|folder| doc.path.folder() == Some(folder));
                    if !(top_file || in_folder) || !doc.path.file_name().ends_with(".yaml") {
                        return Err(failure(
                            PATH_NOT_ALLOWED,
                            format!(
                                "A drafting rule of `{}` for `{file_kind}` files drafted `{}`, which is not a YAML file in that kind's folder.",
                                rule.pack, doc.path
                            ),
                            "Correct the provider's drafting rule.",
                        ));
                    }
                    if let Some(why) = self.drafts_a_write(file_kind.as_str(), &doc) {
                        report.refused.push((doc.path, why.to_owned()));
                        continue;
                    }
                    if let Some(why) =
                        self.drafts_for_another_pack(&rule.pack, file_kind, &doc, &surfaces)
                    {
                        report.refused.push((doc.path, why));
                        continue;
                    }
                    kinds
                        .entry(doc.path.clone())
                        .or_insert_with(|| file_kind.clone());
                    let entry = files.entry(doc.path).or_default();
                    entry.0.push(doc.body);
                    if !entry.1.contains(&finding.id()) {
                        entry.1.push(finding.id());
                    }
                }
            }
            if !covered {
                report.not_covered.push(finding.id());
            }
        }

        // What one finding gave is held by what names what: a draft that
        // holds an open question is held, and with it each file of the same
        // finding that names something a held file defines. A file of the
        // finding that names nothing held stays in place (decision 0298).
        let groups = groups_of(&files);
        let odd_held = self.odd_held_entry();
        let mut skip: BTreeSet<RelPath> = BTreeSet::new();
        let mut held: BTreeSet<RelPath> = BTreeSet::new();
        for group in &groups {
            let in_group = self.held_within(group, &files, &kinds);
            // One file of the finding that a person changed leaves every
            // file of it as it is, and the conflict is reported once.
            let mut conflict = None;
            'group: for path in group {
                let beside = RelPath::new(&format!("{HELD_FOLDER}/{path}"))?;
                let places = if in_group.contains(path) {
                    [beside, path.clone()]
                } else {
                    [path.clone(), beside]
                };
                for place in places {
                    if matches!(self.stand(&place), Ok(Stand::Other)) {
                        conflict = Some(place);
                        break 'group;
                    }
                }
            }
            if let Some(place) = conflict {
                report.conflicts.push(place);
                skip.extend(group.iter().cloned());
                continue;
            }
            if !in_group.is_empty() {
                if let Some(entry) = &odd_held {
                    for path in &in_group {
                        report.refused.push((
                            RelPath::new(&format!("{HELD_FOLDER}/{path}"))?,
                            format!(
                                "the target folder has an entry named `{}`, which is the holding folder's name in other letters, so nothing is held in it",
                                crate::schema::shown(entry)
                            ),
                        ));
                    }
                    skip.extend(group.iter().cloned());
                    continue;
                }
                held.extend(in_group);
            }
        }

        for (path, (docs, from)) in files {
            if skip.contains(&path) {
                continue;
            }
            let is_held = held.contains(&path);
            let text = render(&docs, &from);
            let at = if is_held {
                RelPath::new(&format!("{HELD_FOLDER}/{path}"))?
            } else {
                path.clone()
            };
            // The other place the same draft may stand: where it belongs,
            // for a held draft; the holding folder, for one that is not.
            let other = if is_held {
                path.clone()
            } else {
                RelPath::new(&format!("{HELD_FOLDER}/{path}"))?
            };
            match place(self.root.path(), &at, &text) {
                Ok(Placed::Written) => {}
                Ok(Placed::Conflict) => {
                    report.conflicts.push(at);
                    continue;
                }
                Ok(Placed::Link) => {
                    report.refused.push((
                        at,
                        "the path leads through a link; a draft is written only to a regular file inside the target folder".to_owned(),
                    ));
                    continue;
                }
                Err(_) => {
                    return Err(failure(
                        TARGET_UNREADABLE,
                        format!("The draft `{at}` could not be written into the target folder."),
                        "Check that the target folder exists and may be written by this user.",
                    ));
                }
            }
            // The draft is where it belongs now: the copy of the drafter's
            // own at the other place, unchanged since, is removed. A file a
            // person changed is not touched.
            if self.remove_untouched(&other).is_err() {
                return Err(failure(
                    TARGET_UNREADABLE,
                    format!(
                        "The earlier draft `{other}` could not be removed from the target folder."
                    ),
                    "Check that the target folder exists and may be written by this user.",
                ));
            }
            let mut questions = Vec::new();
            docs.iter().for_each(|doc| doc.questions(&mut questions));
            report
                .open_questions
                .extend(
                    questions
                        .into_iter()
                        .map(|(question, quoting)| OpenQuestion {
                            file: at.clone(),
                            question,
                            findings: from.clone(),
                            quoting,
                        }),
                );
            if is_held {
                report.held.push(at);
            } else {
                report.written.push(at);
            }
        }

        report.written.sort();
        report.held.sort();
        report.open_questions.sort_by(|a, b| a.file.cmp(&b.file));

        // Lint and compile what is there now; a draft that does not pass is
        // kept, with why.
        let problems = match load(self.root, self.set) {
            Ok(sources) => match compile(&sources, self.set, CompileMode::Draft, DRAFT_VERSION) {
                Ok(_) => Diagnostics::new(),
                Err(problems) => problems,
            },
            Err(problems) => problems,
        };
        for path in &report.written {
            let own: Diagnostics = problems
                .iter()
                .filter(|problem| &problem.at.file == path && crate::compile::is_error(problem))
                .cloned()
                .collect();
            if !own.is_empty() {
                report.kept_failing.push((path.clone(), own));
            }
        }
        Ok(report)
    }
}

impl Drafter<'_> {
    /// Why a drafted document is one the drafter never writes, when it is:
    /// an operation whose effect is not plainly `read`, or a document of a
    /// pack's kind that its file kind says describes a write. The document
    /// is judged as it will be read back: written out and parsed.
    fn drafts_a_write(&self, file_kind: &str, doc: &DraftDoc) -> Option<&'static str> {
        const NOT_READABLE: &str = "the draft is not one YAML document this compiler reads, so it cannot be seen to read only";
        let text = render(std::slice::from_ref(&doc.body), &[]);
        let parsed = match yaml::parse(&doc.path, &text) {
            Ok(parsed) => parsed,
            Err(_) => return Some(NOT_READABLE),
        };
        let [parsed] = parsed.as_slice() else {
            return Some(NOT_READABLE);
        };
        if file_kind == OPERATION {
            if parsed.text("effect") != Some("read") {
                return Some(
                    "the drafter drafts read operations only; a write is written by the developer",
                );
            }
            // A drafted binding is `draft`: only `binding.promote` makes one
            // verified, on its evidence (audit finding PM-2).
            let beyond_draft = parsed
                .get("bindings")
                .map(Located::items)
                .unwrap_or_default()
                .iter()
                .any(|binding| {
                    binding.get("status").is_some() && binding.text("status") != Some("draft")
                });
            return beyond_draft.then_some(
                "the drafter drafts a binding as `draft` only; a binding is made verified by `binding.promote`",
            );
        }
        self.set
            .file_kinds
            .iter()
            .any(|((name, _), kind)| name.as_str() == file_kind && kind.item.writes(parsed))
            .then_some(
                "the drafter drafts nothing that writes; an entry that writes is written by the developer",
            )
    }
}

impl Drafter<'_> {
    /// The target's surfaces, each by its name and kind, in the order its
    /// file declares them: from the target as loaded, or, where it does not
    /// load as a whole, from its `target.yaml` alone.
    fn surfaces(&self, existing: Option<&Sources>) -> Vec<(String, String)> {
        let read = |target: &Located<crate::doc::Doc>| -> Vec<(String, String)> {
            target
                .get("surfaces")
                .map(|surfaces| {
                    surfaces
                        .entries()
                        .iter()
                        .filter_map(|entry| {
                            Some((
                                entry.key.value.clone(),
                                entry.value.text("kind")?.to_owned(),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        if let Some(target) = existing.and_then(|sources| sources.docs_of("target").next()) {
            return read(target);
        }
        let path = RelPath::target_file();
        let file = self.root.path().join(path.as_str());
        // A link is never followed out of the target.
        if !fs::symlink_metadata(&file).is_ok_and(|meta| meta.file_type().is_file()) {
            return Vec::new();
        }
        fs::read(file)
            .ok()
            .and_then(|bytes| crate::load::SourceFile::read(path, &bytes, self.set).ok())
            .and_then(|file| file.docs.first().map(read))
            .unwrap_or_default()
    }

    /// Why a drafted document of a pack's file kind is one its rule may not
    /// draft (decision 0203): it lives on a surface of a kind for which the
    /// rule's own pack did not register that file kind, so another pack's
    /// rules would compile it. The surface is found as the compiler finds
    /// it: the one the document names, else the target's first surface of a
    /// kind the file kind is registered for. A document that lives on no
    /// surface of the target is refused too.
    fn drafts_for_another_pack(
        &self,
        pack: &str,
        file_kind: &FileKindName,
        doc: &DraftDoc,
        surfaces: &[(String, String)],
    ) -> Option<String> {
        if KERNEL_FILE_KINDS.contains(&file_kind.as_str()) {
            return None;
        }
        let text = render(std::slice::from_ref(&doc.body), &[]);
        let parsed = yaml::parse(&doc.path, &text).ok()?;
        let named = parsed
            .first()
            .and_then(|doc| doc.text("surface").map(str::to_owned));
        let kinds = self.set.surface_kinds_of(file_kind);
        let surface = match &named {
            Some(named) => surfaces.iter().find(|(name, _)| name == named),
            None => surfaces
                .iter()
                .find(|(_, kind)| kinds.iter().any(|registered| registered.as_str() == kind)),
        };
        let Some((surface, kind)) = surface else {
            return Some(format!(
                "the `{file_kind}` draft lives on no surface of the target that `{pack}` registered `{file_kind}` files for"
            ));
        };
        let own = self
            .set
            .file_kinds
            .iter()
            .any(|((name, surface_kind), owner)| {
                name == file_kind && surface_kind.as_str() == kind && owner.pack == pack
            });
        (!own).then(|| {
            format!(
                "the `{file_kind}` draft lives on the surface `{surface}` of the kind `{kind}`, for which `{pack}` did not register `{file_kind}` files; another pack's rules would compile it"
            )
        })
    }
}

/// The files of the drafts, grouped: files made from one finding are in
/// one group, and groups that share a finding are one. Each group is
/// ordered by path, and the groups by their first path.
fn groups_of(files: &BTreeMap<RelPath, (Vec<DraftValue>, Vec<Digest>)>) -> Vec<Vec<RelPath>> {
    let mut groups: Vec<(BTreeSet<RelPath>, BTreeSet<Digest>)> = Vec::new();
    for (path, (_, from)) in files {
        let mut paths = BTreeSet::from([path.clone()]);
        let mut findings: BTreeSet<Digest> = from.iter().copied().collect();
        let mut kept = Vec::new();
        for (group_paths, group_findings) in groups {
            if group_findings
                .iter()
                .any(|finding| findings.contains(finding))
            {
                paths.extend(group_paths);
                findings.extend(group_findings);
            } else {
                kept.push((group_paths, group_findings));
            }
        }
        kept.push((paths, findings));
        groups = kept;
    }
    let mut groups: Vec<Vec<RelPath>> = groups
        .into_iter()
        .map(|(paths, _)| paths.into_iter().collect())
        .collect();
    groups.sort();
    groups
}

/// Every text value in a document, keys not included.
fn collect_texts(doc: &Located<crate::doc::Doc>, out: &mut BTreeSet<String>) {
    match &doc.value {
        crate::doc::Doc::Text(text) => {
            out.insert(text.clone());
        }
        crate::doc::Doc::List(items) => items.iter().for_each(|item| collect_texts(item, out)),
        crate::doc::Doc::Map(entries) => entries
            .iter()
            .for_each(|entry| collect_texts(&entry.value, out)),
        _ => {}
    }
}

/// What stands at the place of a draft.
enum Stand {
    /// Nothing, or nothing a draft could be written over.
    Absent,
    /// A draft of the drafter's own that nobody changed.
    Untouched,
    /// A link, on the way or in the file's place: never written through.
    Link,
    /// Anything else: a file a person wrote or changed, or a folder.
    Other,
}

impl Drafter<'_> {
    /// What stands at `path` inside the target folder, looked at without
    /// following a link.
    fn stand(&self, path: &RelPath) -> io::Result<Stand> {
        let parts: Vec<&str> = path.parts().collect();
        let Some((name, folders)) = parts.split_last() else {
            return Ok(Stand::Other);
        };
        let mut at = self.root.path().to_path_buf();
        for part in folders {
            at.push(part);
            match fs::symlink_metadata(&at) {
                Ok(found) if found.is_dir() => {}
                Ok(found) if found.file_type().is_symlink() => return Ok(Stand::Link),
                Ok(_) => return Ok(Stand::Other),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Ok(Stand::Absent);
                }
                Err(error) => return Err(error),
            }
        }
        at.push(name);
        match fs::symlink_metadata(&at) {
            Ok(found) if found.is_file() => {
                let untouched =
                    fs::read_to_string(&at).is_ok_and(|present| is_untouched_draft(&present));
                Ok(if untouched {
                    Stand::Untouched
                } else {
                    Stand::Other
                })
            }
            Ok(found) if found.file_type().is_symlink() => Ok(Stand::Link),
            Ok(_) => Ok(Stand::Other),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Stand::Absent),
            Err(error) => Err(error),
        }
    }

    /// The files of one finding's group that are held: each that holds an
    /// open question, and, to a fixed point, each that names, as a whole
    /// scalar value, something a held file of the group defines. The names a
    /// pack's file defines are what its file kind's `names` gives; the
    /// kernel's kinds define the value of their naming key (`operation`,
    /// `pipeline`, `template` in a sources file, `test`) and the entries
    /// under `types:`. A value that merely equals a name holds a file it
    /// need not: that is the safe direction.
    fn held_within(
        &self,
        group: &[RelPath],
        files: &BTreeMap<RelPath, (Vec<DraftValue>, Vec<Digest>)>,
        kinds: &BTreeMap<RelPath, FileKindName>,
    ) -> BTreeSet<RelPath> {
        let mut defines: BTreeMap<&RelPath, BTreeSet<String>> = BTreeMap::new();
        let mut scalars: BTreeMap<&RelPath, BTreeSet<String>> = BTreeMap::new();
        let mut held: BTreeSet<RelPath> = BTreeSet::new();
        for path in group {
            let (docs, _) = &files[path];
            let mut questions = Vec::new();
            docs.iter().for_each(|doc| doc.questions(&mut questions));
            if !questions.is_empty() {
                held.insert(path.clone());
            }
            let text = render(docs, &[]);
            let parsed = yaml::parse(path, &text).unwrap_or_default();
            let mut names = BTreeSet::new();
            let mut texts = BTreeSet::new();
            for doc in &parsed {
                collect_texts(doc, &mut texts);
                if let Some(kind) = kinds.get(path) {
                    self.defined_names(kind, doc, &mut names);
                }
            }
            defines.insert(path, names);
            scalars.insert(path, texts);
        }
        loop {
            let named: BTreeSet<&String> = held
                .iter()
                .filter_map(|path| defines.get(path))
                .flatten()
                .collect();
            let joining: Vec<RelPath> = group
                .iter()
                .filter(|path| !held.contains(*path))
                .filter(|path| scalars[path].iter().any(|text| named.contains(text)))
                .cloned()
                .collect();
            if joining.is_empty() {
                return held;
            }
            held.extend(joining);
        }
    }

    /// The names one document of a file of `kind` defines.
    fn defined_names(
        &self,
        kind: &FileKindName,
        doc: &Located<crate::doc::Doc>,
        out: &mut BTreeSet<String>,
    ) {
        if KERNEL_FILE_KINDS.contains(&kind.as_str()) {
            let key = match kind.as_str() {
                "operation" => "operation",
                "pipeline" => "pipeline",
                "source" => "template",
                "test" => "test",
                _ => "",
            };
            if let Some(name) = doc.text(key).filter(|_| !key.is_empty()) {
                out.insert(name.to_owned());
            }
            if kind.as_str() == "types"
                && let Some(types) = doc.get("types")
            {
                out.extend(types.entries().iter().map(|entry| entry.key.value.clone()));
            }
            return;
        }
        for ((registered, _), owner) in &self.set.file_kinds {
            if registered == kind {
                out.extend(
                    owner
                        .item
                        .names(doc)
                        .into_iter()
                        .map(|name| name.value.name),
                );
            }
        }
    }

    /// An entry of the target folder named as the holding folder is, in
    /// other letters (`Held`): on a file system that folds case, writing to
    /// `held/` would write into it.
    fn odd_held_entry(&self) -> Option<String> {
        fs::read_dir(self.root.path())
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .find(|name| name != HELD_FOLDER && name.eq_ignore_ascii_case(HELD_FOLDER))
    }

    /// Removes the draft at `path` when it is one the drafter wrote and
    /// nobody changed, and then the folders of the holding folder it leaves
    /// empty. Anything else stands.
    fn remove_untouched(&self, path: &RelPath) -> io::Result<()> {
        if !matches!(self.stand(path)?, Stand::Untouched) {
            return Ok(());
        }
        match fs::remove_file(path.under(self.root.path())) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        if path.folder() == Some(HELD_FOLDER) {
            let parts: Vec<&str> = path.parts().collect();
            for depth in (1..parts.len()).rev() {
                let mut folder = self.root.path().to_path_buf();
                parts[..depth].iter().for_each(|part| folder.push(part));
                // A folder that still holds anything stands.
                if fs::remove_dir(&folder).is_err() {
                    break;
                }
            }
        }
        Ok(())
    }
}

/// What became of one draft.
enum Placed {
    /// It is in its file.
    Written,
    /// Something is there that is not an unchanged draft: a file a person
    /// wrote or changed, or a folder.
    Conflict,
    /// A link stands on the way or in the file's place.
    Link,
}

/// Puts `text` in the file at `path` inside `root`, without ever following
/// a link below `root` and without opening any file that exists for
/// writing.
fn place(root: &Path, path: &RelPath, text: &str) -> io::Result<Placed> {
    fs::create_dir_all(root)?;
    let parts: Vec<&str> = path.parts().collect();
    let Some((name, folders)) = parts.split_last() else {
        return Ok(Placed::Conflict);
    };
    let mut folder = root.to_path_buf();
    for part in folders {
        folder.push(part);
        match fs::symlink_metadata(&folder) {
            Ok(found) if found.file_type().is_symlink() => return Ok(Placed::Link),
            Ok(found) if found.is_dir() => {}
            Ok(_) => return Ok(Placed::Conflict),
            Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir(&folder)?,
            Err(error) => return Err(error),
        }
    }
    let file = folder.join(name);
    match fs::symlink_metadata(&file) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match write_new(&file, text) {
                // Something took the name in between: it is not ours.
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(Placed::Conflict),
                written => written.map(|()| Placed::Written),
            }
        }
        Err(error) => Err(error),
        Ok(found) if found.file_type().is_symlink() => Ok(Placed::Link),
        Ok(found) if found.is_file() => {
            let untouched =
                || fs::read_to_string(&file).is_ok_and(|present| is_untouched_draft(&present));
            if !untouched() {
                return Ok(Placed::Conflict);
            }
            // A name that starts with a dot is passed over by the loader.
            let beside = folder.join(format!(".{name}.drafting"));
            match fs::remove_file(&beside) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                _ => {}
            }
            write_new(&beside, text)?;
            // Looked at again as late as can be: a file changed since the
            // first look is the developer's.
            let replaced = if untouched() {
                fs::rename(&beside, &file).map(|()| Placed::Written)
            } else {
                Ok(Placed::Conflict)
            };
            if !matches!(replaced, Ok(Placed::Written)) {
                let _ = fs::remove_file(&beside);
            }
            replaced
        }
        Ok(_) => Ok(Placed::Conflict),
    }
}

/// Writes `text` to a file that must not exist yet, under any form: a link
/// in its place, even one that leads nowhere, makes this fail.
fn write_new(file: &Path, text: &str) -> io::Result<()> {
    let mut created = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(file)?;
    created.write_all(text.as_bytes())?;
    created.sync_all()
}

impl DraftReport {
    /// The failure for the drafts that were not written because the
    /// developer changed the files: class `input`, code `draft_conflict`.
    #[must_use]
    pub fn conflict_failure(&self) -> Option<Failure> {
        let first = self.conflicts.first()?;
        Some(failure(
            DRAFT_CONFLICT,
            format!(
                "{} drafts were not written, the first `{first}`: the file is there and is not an unchanged draft.",
                self.conflicts.len()
            ),
            "Keep the file as it is, or remove it and draft again.",
        ))
    }
}

/// Whether `text` is a file the drafter wrote and nobody changed: its first
/// line carries the digest of everything after it.
fn is_untouched_draft(text: &str) -> bool {
    let text = normalize_text(text);
    let Some((first, rest)) = text.split_once('\n') else {
        return false;
    };
    first
        .strip_prefix(MARKER)
        .is_some_and(|digest| digest == Digest::of(rest.as_bytes()).to_string())
}

/// The text of a draft file: the marker line, the findings it came from,
/// and the documents as YAML. The same documents give the same bytes.
fn render(docs: &[DraftValue], from: &[Digest]) -> String {
    let mut body = String::new();
    body.push_str(
        "# Drafted by anything2mcp from findings; level: reconstructed once it compiles.\n",
    );
    for finding in from {
        body.push_str("# drafted_from: ");
        body.push_str(&finding.to_string());
        body.push('\n');
    }
    for (position, doc) in docs.iter().enumerate() {
        if position > 0 {
            body.push_str("---\n");
        }
        match doc {
            DraftValue::Map(pairs) if !pairs.is_empty() => write_map(pairs, 0, &mut body),
            DraftValue::List(items) if !items.is_empty() => write_list(items, 0, &mut body),
            other => {
                write_scalar(other, &mut body);
                body.push('\n');
            }
        }
    }
    format!("{MARKER}{}\n{body}", Digest::of(body.as_bytes()))
}

fn pad(indent: usize, out: &mut String) {
    for _ in 0..indent {
        out.push_str("  ");
    }
}

fn write_key(key: &str, out: &mut String) {
    let plain = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
        && !key.starts_with('-')
        && !["true", "false", "null"].contains(&key)
        && !key.bytes().all(|byte| byte.is_ascii_digit());
    if plain {
        out.push_str(key);
    } else {
        write_quoted(key, out);
    }
}

fn write_quoted(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => {
                out.push_str(&format!("\\u{:04x}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_scalar(value: &DraftValue, out: &mut String) {
    match value {
        DraftValue::Null => out.push_str("null"),
        DraftValue::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        DraftValue::Int(number) => out.push_str(&number.to_string()),
        DraftValue::Text(text) => write_quoted(text, out),
        DraftValue::OpenQuestion(question) => {
            out.push('{');
            out.push_str(OPEN_QUESTION);
            out.push_str(": ");
            write_quoted(question, out);
            out.push('}');
        }
        DraftValue::OpenQuestionQuoting(quoting) => {
            out.push('{');
            out.push_str(OPEN_QUESTION);
            out.push_str(": ");
            write_quoted(&quoting.question(), out);
            out.push('}');
        }
        DraftValue::List(_) => out.push_str("[]"),
        DraftValue::Map(_) => out.push_str("{}"),
    }
}

fn write_value(value: &DraftValue, indent: usize, out: &mut String) {
    match value {
        DraftValue::Map(pairs) if !pairs.is_empty() => {
            out.push('\n');
            write_map(pairs, indent + 1, out);
        }
        DraftValue::List(items) if !items.is_empty() => {
            out.push('\n');
            write_list(items, indent + 1, out);
        }
        scalar => {
            out.push(' ');
            write_scalar(scalar, out);
            out.push('\n');
        }
    }
}

fn write_map(pairs: &[(String, DraftValue)], indent: usize, out: &mut String) {
    for (key, value) in pairs {
        pad(indent, out);
        write_key(key, out);
        out.push(':');
        write_value(value, indent, out);
    }
}

fn write_list(items: &[DraftValue], indent: usize, out: &mut String) {
    for item in items {
        pad(indent, out);
        out.push('-');
        write_value(item, indent, out);
    }
}

#[cfg(test)]
mod tests {
    use super::{DraftValue, is_untouched_draft, render};
    use crate::doc::RelPath;
    use crate::yaml::parse;
    use a2m_core::Digest;

    fn sample() -> DraftValue {
        DraftValue::map(vec![
            ("operation", DraftValue::text("example_order_get")),
            ("effect", DraftValue::text("read")),
            ("count", DraftValue::Int(3)),
            ("flag", DraftValue::Bool(true)),
            ("empty", DraftValue::Map(vec![])),
            (
                "inputs",
                DraftValue::map(vec![(
                    "order_id",
                    DraftValue::map(vec![
                        ("type", DraftValue::text("identifier")),
                        (
                            "description",
                            DraftValue::text("Order \"number\":\n  as shown"),
                        ),
                    ]),
                )]),
            ),
            (
                "list",
                DraftValue::List(vec![
                    DraftValue::text("a: b"),
                    DraftValue::map(vec![("id", DraftValue::text("x")), ("n", DraftValue::Null)]),
                    DraftValue::List(vec![DraftValue::Int(1), DraftValue::Int(2)]),
                ]),
            ),
            (
                "unit",
                DraftValue::OpenQuestion("Which unit is the length in?".to_owned()),
            ),
        ])
    }

    #[test]
    fn a_draft_is_written_the_same_way_every_time_and_reads_back_as_what_was_drafted() {
        let from = [Digest::of(b"finding")];
        let text = render(&[sample(), sample()], &from);
        assert_eq!(text, render(&[sample(), sample()], &from));
        let file = RelPath::new("operations/order_get.yaml").unwrap();
        let docs = parse(&file, &text).unwrap();
        assert_eq!(docs.len(), 2);
        let json = docs[0].to_json();
        assert_eq!(json["operation"], "example_order_get");
        assert_eq!(json["count"], 3);
        assert_eq!(json["flag"], true);
        assert_eq!(json["empty"], serde_json::json!({}));
        assert_eq!(
            json["inputs"]["order_id"]["description"],
            "Order \"number\":\n  as shown"
        );
        assert_eq!(json["list"][0], "a: b");
        assert_eq!(json["list"][1], serde_json::json!({"id": "x", "n": null}));
        assert_eq!(json["list"][2], serde_json::json!([1, 2]));
        assert_eq!(
            json["unit"]["open_question"],
            "Which unit is the length in?"
        );
        assert!(text.contains(&format!("# drafted_from: {}", from[0])));
    }

    #[test]
    fn a_draft_is_untouched_only_while_its_bytes_are_the_ones_written() {
        let text = render(&[sample()], &[]);
        assert!(is_untouched_draft(&text));
        assert!(is_untouched_draft(&text.replace('\n', "\r\n")));
        assert!(!is_untouched_draft(&text.replace("count: 3", "count: 4")));
        assert!(!is_untouched_draft("operation: example_order_get\n"));
        assert!(!is_untouched_draft(""));
    }
}
