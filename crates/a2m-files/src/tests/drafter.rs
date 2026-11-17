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
