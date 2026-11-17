//! The drafter: draft target files written from findings, by rule.
//!
//! The kernel's half is here: calling the registered drafting rules,
//! writing YAML the same way every time, the marker that lets a draft be
//! rewritten only while nobody has changed it, the open question that
//! stands where the system did not state a value, the refusal to draft a
//! write, and the lint and compile of what was written. Which finding gives
//! which file is each pack's drafting rule.
//!
//! A draft is written to a regular file inside the target folder and
//! nowhere else. Every folder on the way is looked at without following a
//! link, and a link is never written through: not a linked folder, not a
//! link where the file would be, not one that leads nowhere. A new draft is
//! made with create-new, which fails where anything already has the name.
//! An unchanged draft of the drafter's own is replaced by renaming a new
//! file over it, so no file that exists is ever opened for writing.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write as _};
use std::path::Path;

use a2m_core::{Digest, EvidenceLevel, Failure, FailureClass, Finding, TargetName};

use crate::compile::{DRAFT_VERSION, compile};
use crate::diag::{DRAFT_CONFLICT, Diagnostics, PATH_NOT_ALLOWED, TARGET_UNREADABLE};
use crate::doc::RelPath;
use crate::kernel::{KERNEL_FILE_KINDS, KERNEL_FILES, KERNEL_FOLDER_KINDS, OPERATION};
use crate::load::{Sources, TargetRoot, load};
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

    fn questions(&self, out: &mut Vec<String>) {
        match self {
            Self::OpenQuestion(question) => out.push(question.clone()),
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
    /// The question, for the unknowns ledger.
    pub question: String,
    /// The findings the draft was made from.
    pub findings: Vec<Digest>,
}

/// What a drafting rule sees beside its finding.
pub struct DraftContext<'a> {
    target: Option<&'a TargetName>,
    existing: Option<&'a Sources>,
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
}

/// What a call of the drafter did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DraftReport {
    /// The drafts written, in order of path.
    pub written: Vec<RelPath>,
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
}

fn failure(code: a2m_core::FailureCode, message: String, next_step: &str) -> Failure {
    Failure::new(FailureClass::Input, code, message, next_step)
}

impl<'a> Drafter<'a> {
    /// A drafter that writes into `root` with the rules in `set`.
    #[must_use]
    pub fn new(set: &'a CompilerSet, root: &'a TargetRoot) -> Self {
        Self { set, root }
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
        };
        let mut report = DraftReport::default();
        // Path to its documents and the findings they came from.
        let mut files: BTreeMap<RelPath, (Vec<DraftValue>, Vec<Digest>)> = BTreeMap::new();
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

        for (path, (docs, from)) in files {
            let text = render(&docs, &from);
            match place(self.root.path(), &path, &text) {
                Ok(Placed::Written) => {}
                Ok(Placed::Conflict) => {
                    report.conflicts.push(path);
                    continue;
                }
                Ok(Placed::Link) => {
                    report.refused.push((
                        path,
                        "the path leads through a link; a draft is written only to a regular file inside the target folder".to_owned(),
                    ));
                    continue;
                }
                Err(_) => {
                    return Err(failure(
                        TARGET_UNREADABLE,
                        format!("The draft `{path}` could not be written into the target folder."),
                        "Check that the target folder exists and may be written by this user.",
                    ));
                }
            }
            let mut questions = Vec::new();
            docs.iter().for_each(|doc| doc.questions(&mut questions));
            report
                .open_questions
                .extend(questions.into_iter().map(|question| OpenQuestion {
                    file: path.clone(),
                    question,
                    findings: from.clone(),
                }));
            report.written.push(path);
        }

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
            return (parsed.text("effect") != Some("read")).then_some(
                "the drafter drafts read operations only; a write is written by the developer",
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
