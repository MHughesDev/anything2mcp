//! Decision 0298: a draft with an open question waits in `held/`. The
//! drafter writes it there, with each file of its finding that names what it
//! defines, and leaves the rest of the finding in place; the loader does
//! not read the folder, so the target's files, digest, and plan do not
//! depend on it; the compiler is not changed.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::testing::{example_root, register_stand};
use crate::{
    CompileMode, CompilerSet, DRAFT_VERSION, DraftContext, DraftDoc, DraftValue, Drafter,
    DraftingRule, RelPath, TargetRoot, compile, held_files, load,
};
use a2m_core::{
    Address, Failure, FileKindName, Finding, FindingKind, ProducedBy, Source, Subject, Timestamp,
};

fn finding(found: &str) -> Finding {
    Finding::about(
        Subject::Address(Address::new("https://app.example.com", "/orders").unwrap()),
        FindingKind::new("pair_seen").unwrap(),
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
    let folder = std::env::temp_dir().join(format!("a2m-files-held-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    copy(example_root().path(), &folder);
    folder
}

/// A finding that gives two files, a call and an operation, as a pack that
/// drafts an endpoint and the read over it does. The finding states a name
/// and either `read` or `?`, which leaves the call's effect an open
/// question.
struct PairCall;
struct PairOperation;

fn parts(finding: &Finding) -> (&str, bool) {
    let mut parts = finding.found().split_whitespace();
    let name = parts.next().unwrap();
    (name, parts.next() == Some("?"))
}

impl DraftingRule for PairCall {
    fn finding_kind(&self) -> FindingKind {
        FindingKind::new("pair_seen").unwrap()
    }
    fn file_kind(&self) -> FileKindName {
        FileKindName::new("call").unwrap()
    }
    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let (name, unknown) = parts(finding);
        let effect = if unknown {
            cx.open_question("Does the call read or write?")
        } else {
            DraftValue::text("read")
        };
        Ok(vec![DraftDoc {
            path: RelPath::new(&format!("calls/{name}.yaml"))?,
            body: DraftValue::map(vec![
                ("call", DraftValue::text(name)),
                ("path", DraftValue::text("/pair")),
                ("effect", effect),
            ]),
        }])
    }
}

impl DraftingRule for PairOperation {
    fn finding_kind(&self) -> FindingKind {
        FindingKind::new("pair_seen").unwrap()
    }
    fn file_kind(&self) -> FileKindName {
        FileKindName::new("operation").unwrap()
    }
    fn draft(&self, finding: &Finding, _: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let (name, _) = parts(finding);
        Ok(vec![DraftDoc {
            path: RelPath::new(&format!("operations/{name}.yaml"))?,
            body: DraftValue::map(vec![
                ("operation", DraftValue::text(&format!("example_{name}"))),
                ("effect", DraftValue::text("read")),
                // The read names the call it reads through.
                (
                    "bindings",
                    DraftValue::List(vec![DraftValue::map(vec![(
                        "call",
                        DraftValue::text(name),
                    )])]),
                ),
            ]),
        }])
    }
}

/// A finding of the kind `graph_seen` states its files, `;` apart, each as
/// `kind:name:ref:q`: the kind (`call` or `operation`), the name it defines,
/// the name of another file it mentions (`-` for none), and `q` where the
/// file holds an open question. One rule drafts the files of one kind.
struct Graph(&'static str);

impl DraftingRule for Graph {
    fn finding_kind(&self) -> FindingKind {
        FindingKind::new("graph_seen").unwrap()
    }
    fn file_kind(&self) -> FileKindName {
        FileKindName::new(self.0).unwrap()
    }
    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let mut docs = Vec::new();
        for item in finding.found().split(';') {
            let parts: Vec<&str> = item.split(':').collect();
            if parts[0] != self.0 {
                continue;
            }
            let (name, mention, question) = (parts[1], parts[2], parts.get(3) == Some(&"q"));
            let folder = if self.0 == "call" {
                "calls"
            } else {
                "operations"
            };
            let mut pairs = vec![(self.0, DraftValue::text(name))];
            pairs.push((
                "effect",
                if question {
                    cx.open_question("Does it read or write?")
                } else {
                    DraftValue::text("read")
                },
            ));
            if self.0 == "call" {
                pairs.push(("path", DraftValue::text("/pair")));
            }
            if mention != "-" {
                pairs.push(("mentions", DraftValue::text(mention)));
            }
            docs.push(DraftDoc {
                path: RelPath::new(&format!("{folder}/{name}.yaml"))?,
                body: DraftValue::map(pairs),
            });
        }
        Ok(docs)
    }
}

fn graph(found: &str) -> Finding {
    Finding::about(
        Subject::Address(Address::new("https://app.example.com", "/graph").unwrap()),
        FindingKind::new("graph_seen").unwrap(),
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

fn set() -> CompilerSet {
    let mut builder = CompilerSet::builder();
    register_stand(&mut builder).unwrap();
    builder.drafting_rule("stand", Arc::new(PairCall)).unwrap();
    builder
        .drafting_rule("seer", Arc::new(PairOperation))
        .unwrap();
    builder
        .drafting_rule("stand", Arc::new(Graph("call")))
        .unwrap();
    builder
        .drafting_rule("seer", Arc::new(Graph("operation")))
        .unwrap();
    builder.build()
}

fn paths(list: &[RelPath]) -> Vec<&str> {
    list.iter().map(RelPath::as_str).collect()
}

/// Points 1 and 2: what one finding gave is held together, and a finding
/// with no question is written where it always was.
#[test]
fn a_finding_whose_file_holds_a_question_is_held_whole_and_one_without_is_in_place() {
    let folder = scratch("whole");
    let root = TargetRoot::new(&folder);
    let set = set();
    let report = Drafter::new(&set, &root)
        .draft(&[finding("alpha ?"), finding("beta read")])
        .unwrap();
    assert_eq!(
        paths(&report.held),
        ["held/calls/alpha.yaml", "held/operations/alpha.yaml"]
    );
    assert_eq!(
        paths(&report.written),
        ["calls/beta.yaml", "operations/beta.yaml"]
    );
    // Only the file with the question holds one, and the report names the
    // file it is in.
    assert_eq!(report.open_questions.len(), 1);
    assert_eq!(
        report.open_questions[0].file.as_str(),
        "held/calls/alpha.yaml"
    );
    for held in ["held/calls/alpha.yaml", "held/operations/alpha.yaml"] {
        assert!(folder.join(held).is_file(), "{held}");
    }
    assert!(!folder.join("calls/alpha.yaml").exists());
    assert!(!folder.join("operations/alpha.yaml").exists());
    // What is held is listed, with its question and where it stands.
    let held = held_files(&root);
    assert!(!held.cut);
    let listed = held.files;
    assert_eq!(
        listed
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["held/calls/alpha.yaml", "held/operations/alpha.yaml"]
    );
    assert_eq!(listed[0].questions.len(), 1);
    assert_eq!(listed[0].questions[0].text, "Does the call read or write?");
    assert!(listed[1].questions.is_empty());
    assert_eq!(listed[1].names, ["example_alpha"]);
    std::fs::remove_dir_all(&folder).unwrap();
}

/// Point 1, both ways: an answered question moves the files back, and a
/// question that comes up moves the drafted files out.
#[test]
fn a_finding_is_moved_to_the_holding_folder_and_back_as_its_question_comes_and_goes() {
    let folder = scratch("moved");
    let root = TargetRoot::new(&folder);
    let set = set();
    let drafter = Drafter::new(&set, &root);
    let report = drafter.draft(&[finding("alpha ?")]).unwrap();
    assert_eq!(report.held.len(), 2);
    // The system now states the value: the held copies are removed, the
    // files are written in place, and the empty holding folder is gone.
    let report = drafter.draft(&[finding("alpha read")]).unwrap();
    assert_eq!(
        paths(&report.written),
        ["calls/alpha.yaml", "operations/alpha.yaml"]
    );
    assert!(report.held.is_empty() && report.open_questions.is_empty());
    assert!(!folder.join("held").exists());
    // A question comes up for a finding that was in place: its drafted
    // files, unchanged, are moved.
    let report = drafter.draft(&[finding("alpha ?")]).unwrap();
    assert_eq!(
        paths(&report.held),
        ["held/calls/alpha.yaml", "held/operations/alpha.yaml"]
    );
    assert!(report.written.is_empty());
    assert!(!folder.join("calls/alpha.yaml").exists());
    assert!(!folder.join("operations/alpha.yaml").exists());
    assert!(folder.join("operations").is_dir());
    std::fs::remove_dir_all(&folder).unwrap();
}

/// Point 1: a file a person changed is never moved or removed.
#[test]
fn a_file_a_person_changed_is_neither_moved_nor_removed_by_holding() {
    let folder = scratch("edited");
    let root = TargetRoot::new(&folder);
    let set = set();
    let drafter = Drafter::new(&set, &root);
    // A held draft the person edited stays as it is, whichever way the
    // question goes.
    drafter.draft(&[finding("alpha ?")]).unwrap();
    let held = folder.join("held/operations/alpha.yaml");
    let edited = format!(
        "{}# answered by a person\n",
        std::fs::read_to_string(&held).unwrap()
    );
    std::fs::write(&held, &edited).unwrap();
    let report = drafter.draft(&[finding("alpha ?")]).unwrap();
    assert_eq!(paths(&report.conflicts), ["held/operations/alpha.yaml"]);
    assert_eq!(std::fs::read_to_string(&held).unwrap(), edited);
    // The question is gone, but the held copy is a person's: nothing is
    // written in place, and the conflict names the held file.
    let report = drafter.draft(&[finding("alpha read")]).unwrap();
    assert_eq!(paths(&report.conflicts), ["held/operations/alpha.yaml"]);
    assert!(report.written.is_empty() && report.held.is_empty());
    assert!(!folder.join("calls/alpha.yaml").exists());
    assert!(!folder.join("operations/alpha.yaml").exists());
    assert_eq!(std::fs::read_to_string(&held).unwrap(), edited);
    assert!(folder.join("held/calls/alpha.yaml").is_file());
    std::fs::remove_dir_all(folder.join("held")).unwrap();
    std::fs::remove_dir_all(&folder).unwrap();
}

/// A conflict in one file of a finding leaves every file of it as it is,
/// and the conflict is reported once.
#[test]
fn a_conflict_in_one_file_of_a_finding_leaves_all_its_files_alone() {
    let folder = scratch("finding-conflict");
    let root = TargetRoot::new(&folder);
    let set = set();
    let drafter = Drafter::new(&set, &root);
    // The finding's endpoint (the call) and its read (the operation), both
    // in place; a person edits the read.
    drafter.draft(&[finding("alpha read")]).unwrap();
    let read = folder.join("operations/alpha.yaml");
    let edited = format!(
        "{}# edited by a person\n",
        std::fs::read_to_string(&read).unwrap()
    );
    std::fs::write(&read, &edited).unwrap();
    let call = folder.join("calls/alpha.yaml");
    let call_before = std::fs::read_to_string(&call).unwrap();
    let before = load(&root, &set).unwrap().digest;
    // The finding gains a question: nothing of it moves or is rewritten.
    let report = drafter.draft(&[finding("alpha ?")]).unwrap();
    assert_eq!(paths(&report.conflicts), ["operations/alpha.yaml"]);
    assert!(report.written.is_empty() && report.held.is_empty());
    assert!(report.open_questions.is_empty());
    assert_eq!(std::fs::read_to_string(&call).unwrap(), call_before);
    assert_eq!(std::fs::read_to_string(&read).unwrap(), edited);
    assert!(!folder.join("held").exists());
    assert_eq!(load(&root, &set).unwrap().digest, before);
    std::fs::remove_dir_all(&folder).unwrap();
}

/// A person's file in place is not moved when its finding gets a
/// question, and nothing is held beside it.
#[test]
fn a_drafted_file_in_place_that_a_person_edited_is_not_moved() {
    let folder = scratch("edited-in-place");
    let root = TargetRoot::new(&folder);
    let set = set();
    let drafter = Drafter::new(&set, &root);
    drafter.draft(&[finding("alpha read")]).unwrap();
    let place = folder.join("calls/alpha.yaml");
    let edited = std::fs::read_to_string(&place)
        .unwrap()
        .replace("/pair", "/pair/all");
    std::fs::write(&place, &edited).unwrap();
    let report = drafter.draft(&[finding("alpha ?")]).unwrap();
    assert_eq!(paths(&report.conflicts), ["calls/alpha.yaml"]);
    assert_eq!(std::fs::read_to_string(&place).unwrap(), edited);
    assert!(folder.join("operations/alpha.yaml").is_file());
    assert!(!folder.join("held").exists());
    std::fs::remove_dir_all(&folder).unwrap();
}

/// A name that is the holding folder's in other letters is not written
/// into, and is named.
#[test]
fn an_entry_named_as_the_holding_folder_in_other_letters_is_named_and_not_written_into() {
    let folder = scratch("odd-name");
    let root = TargetRoot::new(&folder);
    let set = set();
    std::fs::create_dir(folder.join("HELD")).unwrap();
    let report = Drafter::new(&set, &root)
        .draft(&[finding("alpha ?"), finding("beta read")])
        .unwrap();
    assert!(report.held.is_empty(), "{report:?}");
    assert_eq!(report.refused.len(), 2);
    assert!(report.refused[0].1.contains("`HELD`"), "{report:?}");
    assert!(
        !folder.join("held").exists()
            || std::fs::read_dir(folder.join("HELD")).unwrap().count() == 0
    );
    // What is not held is written as it was.
    assert_eq!(
        paths(&report.written),
        ["calls/beta.yaml", "operations/beta.yaml"]
    );
    std::fs::remove_dir_all(&folder).unwrap();
}

/// A link named `held` is not the holding folder: the loader meets it as it
/// met links before.
#[cfg(unix)]
#[test]
fn a_link_named_held_is_not_skipped_by_the_loader() {
    let folder = scratch("link");
    let root = TargetRoot::new(&folder);
    let set = set();
    let elsewhere =
        std::env::temp_dir().join(format!("a2m-files-held-link-to-{}", std::process::id()));
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, folder.join("held")).unwrap();
    assert!(load(&root, &set).is_err());
    std::fs::remove_dir_all(&folder).unwrap();
    std::fs::remove_dir_all(&elsewhere).unwrap();
}

/// No pack may register a file kind in the holding folder.
#[test]
fn no_pack_registers_a_file_kind_in_the_holding_folder() {
    struct InHeld(std::sync::Arc<dyn crate::FileKind>);
    impl crate::FileKind for InHeld {
        fn name(&self) -> FileKindName {
            self.0.name()
        }
        fn surface_kinds(&self) -> &[a2m_core::SurfaceKind] {
            self.0.surface_kinds()
        }
        fn folder(&self) -> &str {
            "held"
        }
        fn schema(&self) -> &crate::JsonSchema {
            self.0.schema()
        }
        fn names(
            &self,
            doc: &crate::Located<crate::Doc>,
        ) -> Vec<crate::Located<crate::DefinedName>> {
            self.0.names(doc)
        }
        fn expressions(&self, doc: &crate::Located<crate::Doc>) -> Vec<crate::ExprSite> {
            self.0.expressions(doc)
        }
        fn compile(
            &self,
            docs: &[crate::Located<crate::Doc>],
            cx: &crate::KindContext<'_>,
        ) -> Result<crate::CompiledSection, crate::Diagnostics> {
            self.0.compile(docs, cx)
        }
    }
    let mut builder = CompilerSet::builder();
    let refused = builder
        .file_kind("stand", Arc::new(InHeld(crate::testing::file_kind())))
        .err()
        .unwrap();
    assert_eq!(refused.code().as_str(), "registration_conflict");
}

/// Point 2 of the loader: the holding folder at the top is not read, so the
/// sources, their digest, and the plan's hash are those of the target
/// without it; a folder named `held` deeper down is not special; and an
/// open question in a file outside `held/` is still the error it was.
#[test]
fn the_loader_does_not_read_the_holding_folder_and_the_compiler_is_as_it_was() {
    let folder = scratch("loader");
    let root = TargetRoot::new(&folder);
    let set = set();
    let plain = load(&root, &set).unwrap();
    let plan_without = compile(&plain, &set, CompileMode::Draft, DRAFT_VERSION)
        .unwrap()
        .encoded
        .hash;

    Drafter::new(&set, &root)
        .draft(&[finding("alpha ?")])
        .unwrap();
    // A file no kind claims, even, is not read in there.
    std::fs::write(folder.join("held/notes.txt"), "not a target file").unwrap();
    let with = load(&root, &set).unwrap();
    assert_eq!(with.digest, plain.digest);
    assert_eq!(with.files.len(), plain.files.len());
    let plan_with = compile(&with, &set, CompileMode::Draft, DRAFT_VERSION)
        .unwrap()
        .encoded
        .hash;
    assert_eq!(plan_with, plan_without);

    // `held` below the top is an ordinary folder, and is read.
    std::fs::create_dir_all(folder.join("calls/held")).unwrap();
    std::fs::write(folder.join("calls/held/x.yaml"), "call: x\n").unwrap();
    assert!(
        load(&root, &set)
            .unwrap()
            .file("calls/held/x.yaml")
            .is_some()
    );
    std::fs::remove_dir_all(folder.join("calls/held")).unwrap();

    // An open question outside `held/` fails the compile, as it always did.
    std::fs::copy(
        folder.join("held/calls/alpha.yaml"),
        folder.join("calls/alpha.yaml"),
    )
    .unwrap();
    let loaded = load(&root, &set).unwrap();
    let problems = compile(&loaded, &set, CompileMode::Draft, DRAFT_VERSION).unwrap_err();
    assert!(problems.find("kernel.open_question").is_some());
    std::fs::remove_dir_all(&folder).unwrap();
}

/// Point 2 (decision 0298, narrowed): one finding, five files, one holding a
/// question and four naming nothing of it. Only the one is held; the four
/// are in place, and the target compiles with the held one beside it.
#[test]
fn a_file_of_a_finding_that_names_nothing_held_stays_in_place_and_compiles() {
    let folder = scratch("narrow");
    let root = TargetRoot::new(&folder);
    let set = set();
    let report = Drafter::new(&set, &root)
        .draft(&[graph(
            "call:one:-:q;call:two:-;call:three:-;call:four:-;call:five:-",
        )])
        .unwrap();
    assert_eq!(paths(&report.held), ["held/calls/one.yaml"]);
    assert_eq!(
        paths(&report.written),
        [
            "calls/five.yaml",
            "calls/four.yaml",
            "calls/three.yaml",
            "calls/two.yaml"
        ]
    );
    assert!(!folder.join("calls/one.yaml").exists());
    for stayed in ["five", "four", "three", "two"] {
        assert!(folder.join(format!("calls/{stayed}.yaml")).is_file());
        assert!(!folder.join(format!("held/calls/{stayed}.yaml")).exists());
    }
    assert!(report.kept_failing.is_empty(), "{:?}", report.kept_failing);
    let loaded = load(&root, &set).unwrap();
    compile(&loaded, &set, CompileMode::Draft, DRAFT_VERSION).unwrap();
    std::fs::remove_dir_all(&folder).unwrap();
}

/// An endpoint with a question and the operation that names it are both
/// held; an unrelated operation of the same finding stays in place.
#[test]
fn an_operation_that_names_a_held_call_is_held_and_an_unrelated_one_is_not() {
    let folder = scratch("pair");
    let root = TargetRoot::new(&folder);
    let set = set();
    let report = Drafter::new(&set, &root)
        .draft(&[graph(
            "call:alpha:-:q;operation:read_alpha:alpha;operation:read_other:-",
        )])
        .unwrap();
    assert_eq!(
        paths(&report.held),
        ["held/calls/alpha.yaml", "held/operations/read_alpha.yaml"]
    );
    assert_eq!(paths(&report.written), ["operations/read_other.yaml"]);
    assert!(!folder.join("operations/read_alpha.yaml").exists());
    assert!(!folder.join("calls/alpha.yaml").exists());
    assert!(!folder.join("held/operations/read_other.yaml").exists());
    std::fs::remove_dir_all(&folder).unwrap();
}

/// A chain inside one finding: A holds a question, B names A, C names B.
/// All three are held, in whatever order the files are drafted; D names
/// nothing and stays. A file of another finding is not reached by a name.
#[test]
fn a_chain_of_names_inside_one_finding_is_held_and_the_order_does_not_matter() {
    let spec = ["call:a:-:q", "call:b:a", "call:c:b", "call:d:-"];
    let mut results = Vec::new();
    for (position, order) in [[0, 1, 2, 3], [2, 1, 3, 0], [3, 2, 1, 0], [1, 3, 0, 2]]
        .iter()
        .enumerate()
    {
        let folder = scratch(&format!("chain{position}"));
        let root = TargetRoot::new(&folder);
        let set = set();
        let found = order
            .iter()
            .map(|index| spec[*index])
            .collect::<Vec<_>>()
            .join(";");
        let report = Drafter::new(&set, &root)
            .draft(&[graph(&found), graph("call:y:a")])
            .unwrap();
        assert_eq!(
            paths(&report.held),
            [
                "held/calls/a.yaml",
                "held/calls/b.yaml",
                "held/calls/c.yaml"
            ]
        );
        assert_eq!(paths(&report.written), ["calls/d.yaml", "calls/y.yaml"]);
        results.push((
            paths(&report.held)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            paths(&report.written)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        ));
        std::fs::remove_dir_all(&folder).unwrap();
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
}
