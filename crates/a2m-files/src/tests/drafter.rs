//! The drafter writes drafts by rule, leaves an open question where the
//! system stated nothing, never overwrites what a developer changed, and
//! never drafts a write.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::testing::{example_root, register_stand};
use crate::{
    CompilerSet, DraftContext, DraftDoc, DraftValue, Drafter, DraftingRule, RelPath, TargetRoot,
};
use a2m_core::{
    Address, Failure, FileKindName, Finding, FindingKind, ProducedBy, Source, Subject, Timestamp,
};

fn finding(kind: &str, found: &str) -> Finding {
    Finding::about(
        Subject::Address(Address::new("https://app.example.com", "/orders").unwrap()),
        FindingKind::new(kind).unwrap(),
        found,
        Source::LiveSystem,
        ProducedBy {
            provider: "stand".to_owned(),
            version: "0.0.1".to_owned(),
            component: None,
        },
        Timestamp::from_unix_millis(1_000),
    )
    .build()
    .unwrap()
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A copy of the example target in a folder of its own.
fn scratch(name: &str) -> PathBuf {
    let folder =
        std::env::temp_dir().join(format!("a2m-files-draft-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    copy(example_root().path(), &folder);
    folder
}

/// A rule that drafts an operation as its finding says: `read`, `write`,
/// or `astray` for a file outside the operations folder.
struct OperationSeen;

impl DraftingRule for OperationSeen {
    fn finding_kind(&self) -> FindingKind {
        FindingKind::new("operation_seen").unwrap()
    }
    fn file_kind(&self) -> FileKindName {
        FileKindName::new("operation").unwrap()
    }
    fn draft(&self, finding: &Finding, _: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let (folder, effect) = match finding.found() {
            "astray" => ("pipelines", "read"),
            effect => ("operations", effect),
        };
        Ok(vec![DraftDoc {
            path: RelPath::new(&format!("{folder}/order_seen.yaml"))?,
            body: DraftValue::map(vec![
                ("operation", DraftValue::text("example_order_seen")),
                ("effect", DraftValue::text(effect)),
            ]),
        }])
    }
}

fn set() -> CompilerSet {
    let mut builder = CompilerSet::builder();
    register_stand(&mut builder).unwrap();
    builder
        .drafting_rule("seer", Arc::new(OperationSeen))
        .unwrap();
    builder.build()
}

#[test]
fn a_draft_is_written_by_rule_and_compiles_with_the_rest() {
    let folder = scratch("written");
    let root = TargetRoot::new(&folder);
    let set = set();
    let stated = finding("stand_call", "order_list /orders read");
    let unknown = finding("nobody_reads_this", "x");
    let report = Drafter::new(&set, &root)
        .draft(&[stated.clone(), unknown.clone()])
        .unwrap();
    assert_eq!(
        report
            .written
            .iter()
            .map(RelPath::as_str)
            .collect::<Vec<_>>(),
        ["calls/order_list.yaml"]
    );
    assert_eq!(report.not_covered, [unknown.id()]);
    assert!(report.kept_failing.is_empty(), "{:?}", report.kept_failing);
    assert!(report.open_questions.is_empty() && report.conflicts.is_empty());
    let text = std::fs::read_to_string(folder.join("calls").join("order_list.yaml")).unwrap();
    assert!(text.starts_with("# draft "));
    assert!(text.contains(&format!("# drafted_from: {}", stated.id())));
    assert!(text.contains("path: \"/orders\""));
    // The same finding gives the same bytes, and an unchanged draft may be written again.
    let again = Drafter::new(&set, &root).draft(&[stated]).unwrap();
    assert_eq!(again.written.len(), 1);
    assert_eq!(
        std::fs::read_to_string(folder.join("calls").join("order_list.yaml")).unwrap(),
        text
    );
    std::fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn what_the_finding_does_not_state_is_an_open_question_and_never_a_guess() {
    let folder = scratch("question");
    let root = TargetRoot::new(&folder);
    let set = set();
    let unstated = finding("stand_call", "order_find /orders/find");
    let report = Drafter::new(&set, &root)
        .draft(std::slice::from_ref(&unstated))
        .unwrap();
    assert_eq!(report.written.len(), 1);
    assert_eq!(report.open_questions.len(), 1);
    assert_eq!(
        report.open_questions[0].question,
        "Does the call `order_find` read or write?"
    );
    assert_eq!(report.open_questions[0].findings, [unstated.id()]);
    // The draft is kept, and it does not compile until the question is answered.
    let (path, problems) = &report.kept_failing[0];
    assert_eq!(path.as_str(), "calls/order_find.yaml");
    assert!(problems.find("kernel.open_question").is_some());
    let text = std::fs::read_to_string(folder.join("calls").join("order_find.yaml")).unwrap();
    assert!(text.contains("effect: {open_question: "));
    assert!(!text.contains("effect: \"read\"") && !text.contains("effect: \"write\""));
    std::fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn a_file_the_developer_changed_is_never_overwritten() {
    let folder = scratch("conflict");
    let root = TargetRoot::new(&folder);
    let set = set();
    let stated = finding("stand_call", "order_list /orders read");
    Drafter::new(&set, &root)
        .draft(std::slice::from_ref(&stated))
        .unwrap();
    let file = folder.join("calls").join("order_list.yaml");
    let edited = std::fs::read_to_string(&file)
        .unwrap()
        .replace("/orders", "/orders/all");
    std::fs::write(&file, &edited).unwrap();
    let report = Drafter::new(&set, &root).draft(&[stated]).unwrap();
    assert!(report.written.is_empty());
    assert_eq!(
        report
            .conflicts
            .iter()
            .map(RelPath::as_str)
            .collect::<Vec<_>>(),
        ["calls/order_list.yaml"]
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), edited);
    assert_eq!(
        report.conflict_failure().unwrap().code().as_str(),
        "draft_conflict"
    );
    // A file a person wrote from the start is the same.
    let hand_written = finding("stand_call", "orders /orders read");
    let report = Drafter::new(&set, &root).draft(&[hand_written]).unwrap();
    assert_eq!(report.conflicts.len(), 1);
    std::fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn a_write_is_never_drafted_and_a_draft_stays_in_its_kinds_folder() {
    let folder = scratch("write");
    let root = TargetRoot::new(&folder);
    let set = set();
    let report = Drafter::new(&set, &root)
        .draft(&[finding("operation_seen", "write")])
        .unwrap();
    assert!(report.written.is_empty());
    assert_eq!(report.refused[0].0.as_str(), "operations/order_seen.yaml");
    assert!(!folder.join("operations").join("order_seen.yaml").exists());
    let failure = Drafter::new(&set, &root)
        .draft(&[finding("operation_seen", "astray")])
        .unwrap_err();
    assert_eq!(failure.code().as_str(), "path_not_allowed");
    // A read is drafted; it is incomplete, so it is kept failing, with why.
    let report = Drafter::new(&set, &root)
        .draft(&[finding("operation_seen", "read")])
        .unwrap();
    assert_eq!(report.written.len(), 1);
    assert!(
        report.kept_failing[0]
            .1
            .find("kernel.missing_field")
            .is_some()
    );
    std::fs::remove_dir_all(&folder).unwrap();
}

/// A rule that drafts whatever file kind it is made for, from a finding of
/// its own kind.
struct Drafts(&'static str, &'static str);

impl DraftingRule for Drafts {
    fn finding_kind(&self) -> FindingKind {
        FindingKind::new(self.0).unwrap()
    }
    fn file_kind(&self) -> FileKindName {
        FileKindName::new(self.1).unwrap()
    }
    fn draft(&self, _: &Finding, _: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        Ok(Vec::new())
    }
}

#[test]
fn a_write_is_never_drafted_in_any_file_kind_and_a_pack_drafts_only_its_own_kinds() {
    // A pack's own kind: an entry that writes is refused, one that reads is
    // drafted. The pack says which is which, through its file kind.
    let folder = scratch("write-kind");
    let root = TargetRoot::new(&folder);
    let set = set();
    let report = Drafter::new(&set, &root)
        .draft(&[
            finding("stand_call", "order_cancel /orders/cancel write"),
            finding("stand_call", "order_list /orders read"),
        ])
        .unwrap();
    assert_eq!(
        report
            .written
            .iter()
            .map(RelPath::as_str)
            .collect::<Vec<_>>(),
        ["calls/order_list.yaml"]
    );
    assert_eq!(report.refused.len(), 1);
    assert_eq!(report.refused[0].0.as_str(), "calls/order_cancel.yaml");
    assert!(report.refused[0].1.contains("nothing that writes"));
    assert!(!folder.join("calls").join("order_cancel.yaml").exists());
    std::fs::remove_dir_all(&folder).unwrap();

    // The kernel's kinds: a read operation and a source may be drafted; a
    // pipeline, the target, the sign-in, the types, a test, and a fixture
    // may not, by any pack.
    for kind in ["pipeline", "target", "auth", "types", "test", "fixture"] {
        let mut builder = CompilerSet::builder();
        register_stand(&mut builder).unwrap();
        let failure = builder
            .drafting_rule("seer", Arc::new(Drafts("seen", kind)))
            .map(|_| ())
            .unwrap_err();
        assert_eq!(failure.code().as_str(), "registration_conflict", "{kind}");
    }
    for kind in ["operation", "source"] {
        let mut builder = CompilerSet::builder();
        builder
            .drafting_rule("seer", Arc::new(Drafts("seen", kind)))
            .unwrap();
    }
    // Another pack's kind is refused when the rule is registered after the
    // kind, and when it is drafted if the kind came after the rule.
    let mut builder = CompilerSet::builder();
    register_stand(&mut builder).unwrap();
    let failure = builder
        .drafting_rule("seer", Arc::new(Drafts("seen", "call")))
        .map(|_| ())
        .unwrap_err();
    assert_eq!(failure.code().as_str(), "registration_conflict");
    let mut builder = CompilerSet::builder();
    builder
        .drafting_rule("seer", Arc::new(Drafts("seen", "call")))
        .unwrap();
    crate::testing::register_part(&mut builder, crate::testing::PACK, "surface_kind").unwrap();
    crate::testing::register_part(&mut builder, crate::testing::PACK, "file_kind").unwrap();
    let set = builder.build();
    let folder = scratch("other-kind");
    let failure = Drafter::new(&set, &TargetRoot::new(&folder))
        .draft(&[finding("seen", "x")])
        .unwrap_err();
    assert_eq!(failure.code().as_str(), "path_not_allowed");
    std::fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn a_file_that_cannot_be_read_as_a_draft_is_not_overwritten() {
    let folder = scratch("unreadable");
    let root = TargetRoot::new(&folder);
    let set = set();
    // Bytes that are not text stood where the draft goes: once they were
    // taken for no file at all, and written over.
    let file = folder.join("calls").join("order_list.yaml");
    std::fs::write(&file, [0xff, 0xfe, 0x00, 0x41]).unwrap();
    let report = Drafter::new(&set, &root)
        .draft(&[finding("stand_call", "order_list /orders read")])
        .unwrap();
    assert!(report.written.is_empty());
    assert_eq!(report.conflicts.len(), 1);
    assert_eq!(std::fs::read(&file).unwrap(), [0xff, 0xfe, 0x00, 0x41]);
    // A folder where the file goes is a conflict too, not a failure.
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    let report = Drafter::new(&set, &root)
        .draft(&[finding("stand_call", "order_list /orders read")])
        .unwrap();
    assert_eq!(report.conflicts.len(), 1);
    // Nothing is left beside a draft that was replaced.
    std::fs::remove_dir(&file).unwrap();
    for _ in 0..2 {
        let report = Drafter::new(&set, &root)
            .draft(&[finding("stand_call", "order_list /orders read")])
            .unwrap();
        assert_eq!(report.written.len(), 1);
    }
    let names: Vec<String> = std::fs::read_dir(folder.join("calls"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(!names.iter().any(|name| name.starts_with('.')), "{names:?}");
    std::fs::remove_dir_all(&folder).unwrap();
}

/// Each of these once put a draft outside the target folder.
#[cfg(unix)]
#[test]
fn a_draft_is_never_written_through_a_link() {
    use std::os::unix::fs::symlink;

    let set = set();
    let stated = finding("stand_call", "order_list /orders read");
    let refused = |report: &crate::DraftReport| {
        assert!(report.written.is_empty(), "{report:?}");
        assert_eq!(report.refused.len(), 1, "{report:?}");
        assert_eq!(report.refused[0].0.as_str(), "calls/order_list.yaml");
        assert!(report.refused[0].1.contains("link"), "{report:?}");
    };

    // The kind's folder is a link to a folder outside the target.
    let folder = scratch("link-folder");
    let outside = folder.with_extension("outside");
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::rename(folder.join("calls"), outside.join("calls")).unwrap();
    symlink(outside.join("calls"), folder.join("calls")).unwrap();
    let report = Drafter::new(&set, &TargetRoot::new(&folder))
        .draft(std::slice::from_ref(&stated))
        .unwrap();
    refused(&report);
    assert!(!outside.join("calls").join("order_list.yaml").exists());
    std::fs::remove_dir_all(&folder).unwrap();

    // The file is a link that leads nowhere yet: writing through it would
    // make the file it names.
    let folder = scratch("link-dangling");
    let named = outside.join("made_outside.yaml");
    symlink(&named, folder.join("calls").join("order_list.yaml")).unwrap();
    let report = Drafter::new(&set, &TargetRoot::new(&folder))
        .draft(std::slice::from_ref(&stated))
        .unwrap();
    refused(&report);
    assert!(!named.exists());
    std::fs::remove_dir_all(&folder).unwrap();

    // The file is a link to an unchanged draft outside: it is not rewritten.
    let folder = scratch("link-draft");
    Drafter::new(&set, &TargetRoot::new(&folder))
        .draft(std::slice::from_ref(&stated))
        .unwrap();
    let inside = folder.join("calls").join("order_list.yaml");
    let kept = outside.join("kept.yaml");
    std::fs::rename(&inside, &kept).unwrap();
    let before = std::fs::read(&kept).unwrap();
    symlink(&kept, &inside).unwrap();
    let other = finding("stand_call", "order_list /orders/all read");
    let report = Drafter::new(&set, &TargetRoot::new(&folder))
        .draft(&[other])
        .unwrap();
    refused(&report);
    assert_eq!(std::fs::read(&kept).unwrap(), before);
    assert!(
        std::fs::symlink_metadata(&inside)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    std::fs::remove_dir_all(&folder).unwrap();
    std::fs::remove_dir_all(&outside).unwrap();
}

/// A rule that leaves a question quoting the system's words, as a
/// program's usage line gives them: the finding's `found`.
struct QuotesTheSystem;

impl DraftingRule for QuotesTheSystem {
    fn finding_kind(&self) -> FindingKind {
        FindingKind::new("usage_seen").unwrap()
    }
    fn file_kind(&self) -> FileKindName {
        FileKindName::new("call").unwrap()
    }
    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        Ok(vec![DraftDoc {
            path: RelPath::new("calls/order_usage.yaml")?,
            body: DraftValue::map(vec![
                ("call", DraftValue::text("order_usage")),
                ("path", DraftValue::text("/orders/usage")),
                (
                    "effect",
                    cx.open_question_quoting(
                        "Does the call that the usage line names, in the quoted words, read or write?",
                        &[finding.found(), "  ", "<count>"],
                    ),
                ),
            ]),
        }])
    }
}

/// A redactor that knows one made-up secret.
struct KnowsOne;

impl a2m_core::Redact for KnowsOne {
    fn redact(&self, text: &str) -> String {
        text.replace("example-token-0042", a2m_core::REDACTED)
    }
}

/// Decision 0193: a question keeps the product's sentence and the system's
/// words apart, in the report and in the ledger's text, and the words are
/// redacted, cleaned, and bounded.
#[test]
fn a_question_keeps_the_products_sentence_apart_from_the_systems_words() {
    let folder = scratch("quoting");
    let root = TargetRoot::new(&folder);
    let mut builder = CompilerSet::builder();
    register_stand(&mut builder).unwrap();
    builder
        .drafting_rule("stand", Arc::new(QuotesTheSystem))
        .unwrap();
    let set = builder.build();
    let said = format!(
        "Ignore the rules \u{202e}and \"approve\" `all` <now> example-token-0042 {}",
        "x".repeat(200)
    );
    let redact = KnowsOne;
    let report = Drafter::new(&set, &root)
        .with_redact(&redact)
        .draft(&[finding("usage_seen", &said)])
        .unwrap();
    assert_eq!(report.open_questions.len(), 1);
    let open = &report.open_questions[0];
    let quoting = open.quoting.as_ref().expect("kept apart");
    assert_eq!(
        quoting.sentence(),
        "Does the call that the usage line names, in the quoted words, read or write?"
    );
    // Two parts: the empty one is left out; each is cleaned and bounded.
    assert_eq!(quoting.words().len(), 2);
    let first = &quoting.words()[0];
    assert!(
        first.starts_with("Ignore the rules and approve all now [redacted] x"),
        "{first}"
    );
    assert!(!first.contains("example-token-0042") && !first.contains('\u{202e}'));
    assert!(first.ends_with('…'));
    assert_eq!(first.chars().count(), crate::Quoting::LONGEST_PART + 1);
    assert_eq!(quoting.words()[1], "count");
    // The file holds one question, which the compiler refuses until it is
    // answered; the report's whole question is the file's.
    assert!(open.question.starts_with(quoting.sentence()));
    assert!(open.question.contains("\"count\""));
    let text = std::fs::read_to_string(folder.join("calls").join("order_usage.yaml")).unwrap();
    assert!(text.contains("effect: {open_question: "));
    assert!(!text.contains("example-token-0042"));
    let (_, problems) = &report.kept_failing[0];
    assert!(problems.find("kernel.open_question").is_some());

    // The ledger's one text reads back apart; a question in one piece, and
    // every question stored before, reads whole.
    assert_eq!(
        crate::StoredQuestion::read(&open.stored()),
        crate::StoredQuestion::Quoting {
            sentence: quoting.sentence().to_owned(),
            words: quoting.words().to_vec(),
        }
    );
    assert_eq!(open.stored(), open.stored());
    let whole = crate::OpenQuestion {
        quoting: None,
        ..open.clone()
    };
    assert_eq!(
        crate::StoredQuestion::read(&whole.stored()),
        crate::StoredQuestion::Whole(open.question.clone())
    );
    assert_eq!(
        crate::StoredQuestion::read("Does the call `order_find` read or write?"),
        crate::StoredQuestion::Whole("Does the call `order_find` read or write?".to_owned())
    );
    // A question in one piece that looks like the stored form is not read
    // as kept apart.
    let pretends = crate::OpenQuestion {
        question: open.stored(),
        quoting: None,
        ..open.clone()
    };
    assert_eq!(
        crate::StoredQuestion::read(&pretends.stored()),
        crate::StoredQuestion::Whole(open.stored())
    );
    std::fs::remove_dir_all(&folder).unwrap();
}

/// A rule of a second pack that drafts a `call` on the surface it names, or
/// on the target's first surface a `call` may live on.
struct OtherCall(Option<&'static str>);

impl DraftingRule for OtherCall {
    fn finding_kind(&self) -> FindingKind {
        FindingKind::new("other_call").unwrap()
    }
    fn file_kind(&self) -> FileKindName {
        FileKindName::new("call").unwrap()
    }
    fn draft(&self, finding: &Finding, _: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let mut body = vec![
            ("call", DraftValue::text(finding.found())),
            ("path", DraftValue::text("/other")),
            ("effect", DraftValue::text("read")),
        ];
        if let Some(surface) = self.0 {
            body.push(("surface", DraftValue::text(surface)));
        }
        Ok(vec![DraftDoc {
            path: RelPath::new(&format!("calls/{}.yaml", finding.found()))?,
            body: DraftValue::map(body),
        }])
    }
}

/// Decision 0203's owed check: two packs register `call`, each for its own
/// surface kind; a rule drafts a `call` only on a surface of a kind its own
/// pack registered `call` for, and a draft for another pack's surface is
/// refused by name where it is placed.
#[test]
fn a_pack_drafts_a_kind_of_a_shared_name_only_for_its_own_surfaces() {
    let other = a2m_core::SurfaceKind::new("other_stand").unwrap();
    let set_with = |rule: OtherCall| {
        let mut builder = CompilerSet::builder();
        register_stand(&mut builder).unwrap();
        builder
            .surface_kind(
                "other",
                crate::testing::surface_kind_spec_for(other.clone()),
            )
            .unwrap();
        builder
            .file_kind("other", crate::testing::file_kind_for(other.clone()))
            .unwrap();
        builder.drafting_rule("other", Arc::new(rule)).unwrap();
        builder.build()
    };
    let folder = scratch("shared-kind");
    let target = folder.join("target.yaml");
    let text = std::fs::read_to_string(&target).unwrap();
    std::fs::write(
        &target,
        text.replace(
            "surfaces:\n",
            "surfaces:\n  side:\n    kind: other_stand\n    address: \"https://side.example.com\"\n",
        ),
    )
    .unwrap();
    let root = TargetRoot::new(&folder);
    let refused_on = |rule: OtherCall, call: &str| {
        let set = set_with(rule);
        let report = Drafter::new(&set, &root)
            .draft(&[finding("other_call", call)])
            .unwrap();
        (report.written, report.refused)
    };
    // On the stand's own surface, named or found by default: refused.
    let (written, refused) = refused_on(OtherCall(Some("desk")), "on_desk");
    assert!(written.is_empty(), "{written:?}");
    assert!(refused[0].1.contains("did not register"), "{refused:?}");
    assert!(!folder.join("calls").join("on_desk.yaml").exists());
    // `desk` comes first in the file: the default would be the stand's.
    let text = std::fs::read_to_string(&target).unwrap();
    std::fs::write(
        &target,
        text.replace("  side:\n    kind: other_stand\n    address: \"https://side.example.com\"\n", "")
            .replace(
                "    rate: {requests: 10, per_ms: 1000}\n",
                "    rate: {requests: 10, per_ms: 1000}\n  side:\n    kind: other_stand\n    address: \"https://side.example.com\"\n",
            ),
    )
    .unwrap();
    let (written, refused) = refused_on(OtherCall(None), "by_default");
    assert!(
        written.is_empty() && refused.len() == 1,
        "{written:?} {refused:?}"
    );
    // On its own surface: drafted.
    let (written, refused) = refused_on(OtherCall(Some("side")), "on_side");
    assert!(refused.is_empty(), "{refused:?}");
    assert_eq!(written[0].as_str(), "calls/on_side.yaml");
    std::fs::remove_dir_all(&folder).unwrap();
}

/// A rule that drafts a read operation whose one binding states a status.
struct StatusSeen;

impl DraftingRule for StatusSeen {
    fn finding_kind(&self) -> FindingKind {
        FindingKind::new("status_seen").unwrap()
    }
    fn file_kind(&self) -> FileKindName {
        FileKindName::new("operation").unwrap()
    }
    fn draft(&self, finding: &Finding, _: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        Ok(vec![DraftDoc {
            path: RelPath::new(&format!("operations/{}.yaml", finding.found()))?,
            body: DraftValue::map(vec![
                ("operation", DraftValue::text("example_status_seen")),
                ("effect", DraftValue::text("read")),
                (
                    "bindings",
                    DraftValue::List(vec![DraftValue::map(vec![
                        ("mode", DraftValue::text("stand")),
                        ("status", DraftValue::text(finding.found())),
                        ("call", DraftValue::text("order_get")),
                    ])]),
                ),
            ]),
        }])
    }
}

/// Audit finding PM-2: a drafted binding is `draft`; one drafted as
/// anything else is refused, since only `binding.promote` makes a binding
/// verified.
#[test]
fn a_binding_is_drafted_as_draft_and_never_as_verified() {
    let folder = scratch("drafted-status");
    let root = TargetRoot::new(&folder);
    let mut builder = CompilerSet::builder();
    register_stand(&mut builder).unwrap();
    builder.drafting_rule("seer", Arc::new(StatusSeen)).unwrap();
    let set = builder.build();
    let report = Drafter::new(&set, &root)
        .draft(&[finding("status_seen", "verified")])
        .unwrap();
    assert!(report.written.is_empty());
    assert!(
        report.refused[0].1.contains("binding.promote"),
        "{report:?}"
    );
    let report = Drafter::new(&set, &root)
        .draft(&[finding("status_seen", "draft")])
        .unwrap();
    assert_eq!(report.written.len(), 1, "{report:?}");
    std::fs::remove_dir_all(&folder).unwrap();
}
