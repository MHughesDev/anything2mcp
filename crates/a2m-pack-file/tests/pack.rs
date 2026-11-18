//! Registration and compiling: a registry that holds only the file pack,
//! the example target compiled through it, and what the compiler refuses.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use a2m_core::{Archetype, Mode, SurfaceKind};
use a2m_provider::{Manifest, Roots};
use common::{compile, fixtures, registry};

#[test]
fn a_registry_with_only_the_file_pack_builds_and_its_digest_is_stable() {
    let first = registry(Roots::none());
    let second = registry(Roots::none());
    assert_eq!(first.digest(), second.digest());
    let mode = Mode::new("file").unwrap();
    assert_eq!(first.modes().collect::<Vec<_>>(), [&mode]);
    assert_eq!(first.archetype(&mode), Some(Archetype::Artifact));
    assert!(first.driver(&mode).is_ok());
    assert!(
        first
            .surface_kind(&SurfaceKind::new("file").unwrap())
            .is_some()
    );
    let set = first.compiler_set();
    assert_eq!(
        set.verbs(&mode)
            .iter()
            .map(|verb| verb.as_str())
            .collect::<Vec<_>>(),
        ["create", "delete", "list", "read", "rename", "update"]
    );
    // Documents, folders, and growing files (decision 0246).
    assert_eq!(set.file_kinds().count(), 3);
    // A document from a layout, and the reads over it; a folder and a
    // growing file, and the reads over each (decision 0255).
    assert_eq!(set.drafting_rules().count(), 6);
}

#[test]
fn every_failure_code_of_the_pack_is_declared_with_its_one_class() {
    let manifest = Manifest::parse(a2m_pack_file::MANIFEST).unwrap();
    for (class, code) in a2m_pack_file::codes::ALL {
        let declared = manifest
            .failure_codes
            .iter()
            .find(|declared| &declared.code == code)
            .unwrap_or_else(|| panic!("`{code}` is not in the manifest"));
        assert_eq!(declared.class, *class, "{code}");
    }
    // The manifest declares one more, which the expression library makes.
    assert_eq!(
        manifest.failure_codes.len(),
        a2m_pack_file::codes::ALL.len() + 1
    );
}

#[test]
fn the_fixtures_compile_twice_to_identical_plan_bytes() {
    let registry = registry(Roots::none());
    let first = compile(&fixtures(), registry.compiler_set()).unwrap();
    let second = compile(&fixtures(), registry.compiler_set()).unwrap();
    assert_eq!(first.encoded.bytes, second.encoded.bytes);
    assert_eq!(first.encoded.hash, second.encoded.hash);
    // A write saves once, confirms, and reads back by opening the file again.
    let set = first
        .plan
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == "example_stock_set")
        .unwrap();
    let binding = &set.bindings[0];
    let kinds = |steps: &[a2m_core::Step]| -> Vec<String> {
        steps.iter().map(|step| step.kind.to_string()).collect()
    };
    assert_eq!(kinds(&binding.act), ["open", "locate", "set", "save"]);
    assert_eq!(
        binding
            .act
            .iter()
            .map(|step| step.commits)
            .collect::<Vec<_>>(),
        [false, false, false, true]
    );
    assert_eq!(kinds(&binding.confirm), ["read"]);
    assert_eq!(kinds(&binding.read_back), ["open", "locate", "read"]);
}

/// A copy of the fixtures with one file changed, removed when dropped.
struct Changed(PathBuf);

impl Changed {
    fn new(changes: &[(&str, &str, &str)]) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "a2m-pack-file-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        copy(&fixtures(), &root);
        for (file, replacing, with) in changes {
            let mut path = root.clone();
            path.extend(file.split('/'));
            let text = std::fs::read_to_string(&path).unwrap();
            let changed = text.replacen(replacing, with, 1);
            assert_ne!(text, changed, "the change to {file} changed nothing");
            std::fs::write(&path, changed).unwrap();
        }
        Self(root)
    }
}

impl Drop for Changed {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn what_the_pack_cannot_carry_out_is_refused_with_a_file_a_line_and_a_fix() {
    let registry = registry(Roots::none());
    // Each case: the changes, as a file, what is replaced, and what replaces
    // it; then the code of the diagnostic and a word its message holds.
    type Case<'a> = (&'a [(&'a str, &'a str, &'a str)], &'a str, &'a str);
    let cases: [Case<'_>; 11] = [
        // An unknown key in a document is the kernel's schema check.
        (
            &[("documents/stock.yaml", "records:", "shape: wide\nrecords:")],
            "kernel.unknown_key",
            "shape",
        ),
        // A format the pack names and does not read.
        (
            &[("documents/settings.yaml", "format: json", "format: yaml")],
            "file.format_not_built",
            "yaml",
        ),
        // A place that is not one of the format's.
        (
            &[("documents/stock.yaml", "rows: all", "path: [stock]")],
            "file.document_invalid",
            "not a place",
        ),
        // A file name that could leave the folder.
        (
            &[(
                "documents/stock.yaml",
                "file: stock.csv",
                "file: ../outside/outside.csv",
            )],
            "file.document_invalid",
            "one name",
        ),
        // A binding that sets a field the document does not mark writable.
        (
            &[(
                "operations/stock_set.yaml",
                "fields: {on_hand: input.on_hand}",
                "fields: {description: input.item_code}",
            )],
            "file.binding_invalid",
            "writable",
        ),
        // A write on a surface that does not say where it stands.
        (
            &[(
                "target.yaml",
                "folder: prices\n    stands: alone\n",
                "folder: prices\n",
            )],
            "file.stands_undeclared",
            "stands",
        ),
        // A write to a file an application may hold open, with no signal.
        (
            &[(
                "documents/stock.yaml",
                "lock_signal: {sibling: stock.csv.lock}\n",
                "",
            )],
            "file.open_without_signal",
            "lock signal",
        ),
        // A write to a file whose document does not say whether an
        // application may hold it open. Absent is not "no".
        (
            &[("documents/price_list.yaml", "may_be_open: false\n", "")],
            "file.open_undeclared",
            "does not say",
        ),
        // Text that may begin as a formula does, declared where no
        // spreadsheet program opens the file as cells.
        (
            &[(
                "documents/settings.yaml",
                "type: cutoff, writable: true",
                "type: cutoff, writable: true, may_begin_formula: true",
            )],
            "file.document_invalid",
            "delimited text",
        ),
        // A write to a format with no patch editor.
        (
            &[
                (
                    "documents/prices.yaml",
                    "unit_price: {header: Unit price, type: rate}",
                    "unit_price: {header: Unit price, type: rate, writable: true}",
                ),
                (
                    "operations/price_set.yaml",
                    "document: price_list",
                    "document: prices",
                ),
            ],
            "file.format_read_only",
            "cannot write back",
        ),
        // A write to a file beneath an application, with no authorization.
        (
            &[(
                "target.yaml",
                "stands: alone\n  config_files:",
                "stands: {beneath: config_files}\n  config_files:",
            )],
            "file.beneath_application",
            "beneath",
        ),
    ];
    for (changes, code, says) in cases {
        let changed = Changed::new(changes);
        let problems = match compile(&changed.0, registry.compiler_set()) {
            Ok(_) => panic!("{code}: the changed target compiled"),
            Err(problems) => problems,
        };
        let found = problems
            .find(code)
            .unwrap_or_else(|| panic!("no `{code}` among {problems:?}"));
        assert!(found.message.contains(says), "{code}: {}", found.message);
        assert!(found.at.line >= 1 && !found.at.file.as_str().is_empty());
        assert!(!found.fix.is_empty(), "{code} gives no fix");
    }
}

#[test]
fn an_authorization_key_that_names_no_record_authorizes_no_write_beneath_an_application() {
    let registry = registry(Roots::none());
    // The surface stands beneath an application, one operation that writes
    // there is irreversible, and `target.yaml` holds an authorization.
    let refused = |authorization: &str| -> bool {
        let line = format!("target: example\nauthorization: {authorization}\n");
        let changed = Changed::new(&[
            (
                "target.yaml",
                "stands: alone\n  config_files:",
                "stands: {beneath: config_files}\n  config_files:",
            ),
            ("target.yaml", "target: example\n", &line),
            (
                "operations/stock_set.yaml",
                "effect: write",
                "effect: irreversible",
            ),
        ]);
        let problems = match compile(&changed.0, registry.compiler_set()) {
            Ok(_) => panic!("the other writes beneath the application compiled"),
            Err(problems) => problems,
        };
        problems.iter().any(|found| {
            found.code.as_str() == "file.beneath_application"
                && found.at.file.as_str() == "operations/stock_set.yaml"
        })
    };
    // A reference to the record lets the irreversible write through.
    assert!(!refused("owner-agreement-2026-11"));
    // A key with nothing in it is no record.
    assert!(refused("\" \""));
}

/// A rule that drafts what the pack's own rule never does: a document with
/// a field a caller may write.
struct DraftsAWrite;

impl a2m_files::DraftingRule for DraftsAWrite {
    fn finding_kind(&self) -> a2m_core::FindingKind {
        a2m_core::FindingKind::new(a2m_pack_file::FILE_LAYOUT).unwrap()
    }

    fn file_kind(&self) -> a2m_core::FileKindName {
        a2m_pack_file::DOCUMENT_KIND
    }

    fn draft(
        &self,
        _finding: &a2m_core::Finding,
        cx: &a2m_files::DraftContext<'_>,
    ) -> Result<Vec<a2m_files::DraftDoc>, a2m_core::Failure> {
        use a2m_files::DraftValue;
        let document = |file: &str,
                        writable: Option<DraftValue>|
         -> Result<a2m_files::DraftDoc, a2m_core::Failure> {
            let mut field = vec![
                ("header", DraftValue::text("price")),
                ("type", DraftValue::text("price")),
            ];
            if let Some(writable) = writable {
                field.push(("writable", writable));
            }
            Ok(a2m_files::DraftDoc {
                path: a2m_files::RelPath::new(&format!("documents/{file}.yaml"))?,
                body: DraftValue::map(vec![
                    ("surface", DraftValue::text("files")),
                    ("file", DraftValue::text("prices.csv")),
                    ("format", DraftValue::text("delimited")),
                    (
                        "records",
                        DraftValue::map(vec![(
                            "price",
                            DraftValue::map(vec![
                                ("rows", DraftValue::text("all")),
                                (
                                    "fields",
                                    DraftValue::map(vec![("price", DraftValue::map(field))]),
                                ),
                            ]),
                        )]),
                    ),
                ]),
            })
        };
        Ok(vec![
            document("tries_a_write", Some(DraftValue::Bool(true)))?,
            document("only_reads", None)?,
            // Whether a field may be written, left as a question, counts as
            // a write (decision 0161, point 10).
            document(
                "asks_whether_it_writes",
                Some(cx.open_question("May a caller write the price?")),
            )?,
        ])
    }
}

/// A rule that drafts folders: one that names its writes, one that says
/// whether an application may hold an entry open, and one that only reads.
struct DraftsAFolderThatWrites;

impl a2m_files::DraftingRule for DraftsAFolderThatWrites {
    fn finding_kind(&self) -> a2m_core::FindingKind {
        a2m_core::FindingKind::new(a2m_pack_file::FILE_FOLDER).unwrap()
    }

    fn file_kind(&self) -> a2m_core::FileKindName {
        a2m_pack_file::FOLDER_KIND
    }

    fn draft(
        &self,
        _finding: &a2m_core::Finding,
        _cx: &a2m_files::DraftContext<'_>,
    ) -> Result<Vec<a2m_files::DraftDoc>, a2m_core::Failure> {
        use a2m_files::DraftValue;
        let folder = |name: &str, extra: Option<(&'static str, DraftValue)>| {
            let mut body = vec![("surface", DraftValue::text("files"))];
            body.extend(extra);
            a2m_files::DraftDoc {
                path: a2m_files::RelPath::new(&format!("folders/{name}.yaml")).unwrap(),
                body: DraftValue::map(body),
            }
        };
        Ok(vec![
            folder(
                "names_writes",
                Some((
                    "writes",
                    DraftValue::List(vec![DraftValue::text("create")]),
                )),
            ),
            folder("says_it_may_be_open", Some(("may_be_open", DraftValue::Bool(false)))),
            folder("only_lists", None),
        ])
    }
}

/// A rule that drafts an operation that writes, and one that reads.
struct DraftsAnOperationThatWrites;

impl a2m_files::DraftingRule for DraftsAnOperationThatWrites {
    fn finding_kind(&self) -> a2m_core::FindingKind {
        a2m_core::FindingKind::new(a2m_pack_file::FILE_FOLDER).unwrap()
    }

    fn file_kind(&self) -> a2m_core::FileKindName {
        a2m_core::FileKindName::fixed("operation")
    }

    fn draft(
        &self,
        _finding: &a2m_core::Finding,
        _cx: &a2m_files::DraftContext<'_>,
    ) -> Result<Vec<a2m_files::DraftDoc>, a2m_core::Failure> {
        use a2m_files::DraftValue;
        let operation = |name: &str, effect: &str| a2m_files::DraftDoc {
            path: a2m_files::RelPath::new(&format!("operations/{name}.yaml")).unwrap(),
            body: DraftValue::map(vec![
                ("operation", DraftValue::text(&format!("example_{name}"))),
                ("effect", DraftValue::text(effect)),
                ("inputs", DraftValue::Map(Vec::new())),
                ("outputs", DraftValue::Map(Vec::new())),
                (
                    "bindings",
                    DraftValue::List(vec![DraftValue::map(vec![
                        ("mode", DraftValue::text("file")),
                        ("verb", DraftValue::text("list")),
                        ("folder", DraftValue::text("only_lists")),
                    ])]),
                ),
                ("description", DraftValue::text("A drafted operation.")),
            ]),
        };
        Ok(vec![
            operation("entry_add", "write"),
            operation("entries_list", "read"),
        ])
    }
}

/// What the drafter refused, by path, with the reason it gave.
fn refusals(report: &a2m_files::DraftReport) -> Vec<(&str, &str)> {
    report
        .refused
        .iter()
        .map(|(path, why)| (path.as_str(), why.as_str()))
        .collect()
}

/// The reason the drafter gives for a pack's file that writes.
const DESCRIBES_A_WRITE: &str =
    "the drafter drafts nothing that writes; an entry that writes is written by the developer";

#[test]
fn a_drafting_rule_that_drafts_a_folder_or_an_operation_that_writes_is_refused() {
    use a2m_core::{Coverage, Finding, ProducedBy, Source, Subject};
    let mut builder = a2m_files::CompilerSet::builder();
    builder
        .file_kind(
            a2m_pack_file::PACK,
            std::sync::Arc::new(a2m_pack_file::FolderKind::new()),
        )
        .unwrap()
        .drafting_rule(
            a2m_pack_file::PACK,
            std::sync::Arc::new(DraftsAFolderThatWrites),
        )
        .unwrap()
        .drafting_rule(
            a2m_pack_file::PACK,
            std::sync::Arc::new(DraftsAnOperationThatWrites),
        )
        .unwrap();
    let set = builder.build();
    let finding = Finding::about(
        Subject::File {
            digest: a2m_core::Digest::of(b"a folder"),
            format: "folder".to_owned(),
            part: None,
        },
        a2m_core::FindingKind::new(a2m_pack_file::FILE_FOLDER).unwrap(),
        "{}",
        Source::LiveSystem,
        ProducedBy {
            provider: "file".to_owned(),
            version: "0.0.1".to_owned(),
            component: None,
        },
        a2m_core::Timestamp::from_unix_millis(0),
    )
    .coverage(Coverage::Complete)
    .build()
    .unwrap();
    let root = std::env::temp_dir().join(format!(
        "a2m-pack-file-draft-folder-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("target.yaml"),
        "schema_version: 1\ntarget: example\nsurfaces:\n  files:\n    kind: file\n    folder: .\n",
    )
    .unwrap();
    let report = a2m_files::Drafter::new(&set, &a2m_files::TargetRoot::new(&root))
        .draft(std::slice::from_ref(&finding))
        .unwrap();
    let written: Vec<&str> = report
        .written
        .iter()
        .map(a2m_files::RelPath::as_str)
        .collect();
    let left = (
        root.join("folders").join("names_writes.yaml").exists(),
        root.join("operations").join("entry_add.yaml").exists(),
    );
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(
        refusals(&report),
        [
            ("folders/names_writes.yaml", DESCRIBES_A_WRITE),
            ("folders/says_it_may_be_open.yaml", DESCRIBES_A_WRITE),
            (
                "operations/entry_add.yaml",
                "the drafter drafts read operations only; a write is written by the developer"
            ),
        ]
    );
    assert_eq!(
        written,
        ["folders/only_lists.yaml", "operations/entries_list.yaml"]
    );
    assert_eq!(left, (false, false));
}

#[test]
fn a_drafting_rule_that_drafts_a_document_that_writes_is_refused() {
    use a2m_core::{Coverage, Finding, ProducedBy, Source, Subject};
    let mut builder = a2m_files::CompilerSet::builder();
    builder
        .file_kind(
            a2m_pack_file::PACK,
            std::sync::Arc::new(a2m_pack_file::DocumentKind::new()),
        )
        .unwrap()
        .drafting_rule(a2m_pack_file::PACK, std::sync::Arc::new(DraftsAWrite))
        .unwrap();
    let set = builder.build();
    let finding = Finding::about(
        Subject::File {
            digest: a2m_core::Digest::of(b"a file"),
            format: "delimited".to_owned(),
            part: None,
        },
        a2m_core::FindingKind::new(a2m_pack_file::FILE_LAYOUT).unwrap(),
        "{}",
        Source::LiveSystem,
        ProducedBy {
            provider: "file".to_owned(),
            version: "0.0.1".to_owned(),
            component: None,
        },
        a2m_core::Timestamp::from_unix_millis(0),
    )
    .coverage(Coverage::Complete)
    .build()
    .unwrap();
    let root = std::env::temp_dir().join(format!("a2m-pack-file-draft-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    // The drafter places a document only on a surface the target declares
    // of a kind the pack registered the file kind for (decision 0203).
    std::fs::write(
        root.join("target.yaml"),
        "schema_version: 1\ntarget: example\nsurfaces:\n  files:\n    kind: file\n    folder: .\n",
    )
    .unwrap();
    let report = a2m_files::Drafter::new(&set, &a2m_files::TargetRoot::new(&root))
        .draft(std::slice::from_ref(&finding))
        .unwrap();
    let written: Vec<&str> = report
        .written
        .iter()
        .map(a2m_files::RelPath::as_str)
        .collect();
    // Each refusal says why: a field marked writable, and a writability
    // left as a question, each describe a write.
    assert_eq!(
        refusals(&report),
        [
            ("documents/tries_a_write.yaml", DESCRIBES_A_WRITE),
            ("documents/asks_whether_it_writes.yaml", DESCRIBES_A_WRITE),
        ]
    );
    assert_eq!(written, ["documents/only_reads.yaml"]);
    let exists = root.join("documents").join("tries_a_write.yaml").exists();
    let _ = std::fs::remove_dir_all(&root);
    assert!(!exists);
}

#[test]
fn a_write_never_compiles_as_a_read_whatever_the_file_says() {
    let changed = Changed::new(&[(
        "operations/price_set.yaml",
        "effect: write",
        "effect: read\n",
    )]);
    let registry = registry(Roots::none());
    let problems = compile(&changed.0, registry.compiler_set()).expect_err("a read that writes");
    assert!(
        problems
            .iter()
            .any(|found| found.code.as_str() == "kernel.effect_lower_than_derived"),
        "{problems}"
    );
}

#[test]
fn a_delete_is_irreversible_because_nothing_restores_the_row() {
    // The fixtures declare it so, and they compile.
    let registry = registry(Roots::none());
    let plan = compile(&fixtures(), registry.compiler_set())
        .unwrap_or_else(|problems| panic!("{problems}"))
        .plan;
    for name in ["example_stock_remove", "example_warehouse_remove"] {
        let operation = plan
            .operations
            .iter()
            .find(|operation| operation.name.as_str() == name)
            .unwrap();
        assert_eq!(operation.effect, a2m_core::EffectClass::Irreversible);
        // It is still read back: the row must be gone.
        assert!(!operation.bindings[0].read_back.is_empty());
    }
    // Declared as an ordinary write, it does not compile.
    let changed = Changed::new(&[(
        "operations/stock_remove.yaml",
        "effect: irreversible",
        "effect: write",
    )]);
    let problems = compile(&changed.0, registry.compiler_set()).expect_err("a delete as a write");
    assert!(
        problems.iter().any(|found| {
            found.code.as_str() == "kernel.effect_lower_than_derived"
                && found.at.file.as_str() == "operations/stock_remove.yaml"
        }),
        "{problems}"
    );
}

#[test]
fn a_folder_a_settings_file_or_a_growing_file_the_pack_cannot_carry_out_does_not_compile() {
    let registry = registry(Roots::none());
    type Case<'a> = (&'a [(&'a str, &'a str, &'a str)], &'a str, &'a str);
    let cases: [Case<'_>; 10] = [
        // A folder write whose verb the folder does not declare.
        (
            &[(
                "folders/inbox.yaml",
                "writes: [create, update, rename, delete]",
                "writes: [create, update, delete]",
            )],
            "file.binding_invalid",
            "does not declare",
        ),
        // A folder written whose entries may be held open, or not said.
        (
            &[("folders/inbox.yaml", "may_be_open: false\n", "")],
            "file.folder_invalid",
            "no application",
        ),
        // A record of a settings file with a key of its own.
        (
            &[(
                "documents/plist_settings.yaml",
                "path: [Orders]",
                "path: [Orders]\n    key: [prefix]",
            )],
            "file.document_invalid",
            "no key of its own",
        ),
        // An INI record that names two sections.
        (
            &[(
                "documents/ini_settings.yaml",
                "path: [warehouse W1]",
                "path: [warehouse W1, more]",
            )],
            "file.document_invalid",
            "one section",
        ),
        // A value of a settings file added.
        (
            &[(
                "operations/ini_orders_set.yaml",
                "verb: update",
                "verb: create",
            )],
            "file.binding_invalid",
            "nothing can be added",
        ),
        // A record of a document renamed.
        (
            &[("operations/stock_set.yaml", "verb: update", "verb: rename")],
            "file.binding_invalid",
            "not renamed",
        ),
        // A growing file written.
        (
            &[(
                "operations/orders_log_read.yaml",
                "verb: read",
                "verb: update",
            )],
            "file.binding_invalid",
            "only read",
        ),
        // A folder write read back from something other than its entry.
        (
            &[(
                "operations/inbox_add.yaml",
                "read_back: {record: entry}",
                "read_back: {record: note}",
            )],
            "file.binding_invalid",
            "entry it wrote",
        ),
        // An INI dialect on a JSON document.
        (
            &[(
                "documents/settings.yaml",
                "format: json",
                "format: json\ndialect: {separator: \":\"}",
            )],
            "file.document_invalid",
            "dialect",
        ),
        // A growing file named with a step out of its folder.
        (
            &[(
                "logs/orders.yaml",
                "file: orders.log",
                "file: ../orders.log",
            )],
            "file.folder_invalid",
            "one name",
        ),
    ];
    for (changes, code, says) in cases {
        let changed = Changed::new(changes);
        let problems = match compile(&changed.0, registry.compiler_set()) {
            Ok(_) => panic!("{code}: the changed target compiled"),
            Err(problems) => problems,
        };
        let found = problems
            .find(code)
            .unwrap_or_else(|| panic!("no `{code}` among {problems:?}"));
        assert!(found.message.contains(says), "{code}: {}", found.message);
        assert!(!found.fix.is_empty(), "{code} gives no fix");
    }
}
