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
        ["create", "delete", "list", "read", "update"]
    );
    assert_eq!(set.file_kinds().count(), 1);
    assert_eq!(set.drafting_rules().count(), 1);
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
    // The manifest declares two more, which the expression library makes.
    assert_eq!(
        manifest.failure_codes.len(),
        a2m_pack_file::codes::ALL.len() + 2
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
    let cases: [Case<'_>; 9] = [
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
