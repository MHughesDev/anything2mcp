//! A mode's rank, as its manifest states it, reaches the compiler: an
//! operation bound to the modes of two providers lists the mode of the
//! lower rank first (decision 0337, package 4.6).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::{Arc, Mutex};

use a2m_core::{Failure, Mode, Version};
use a2m_files::testing as stand;
use a2m_files::{CompileMode, CompilerSet, RelPath, TargetRoot, compile, load_files};
use a2m_provider::{CompilerRegistrations, Pack, PackContext, Registrar, RegistryBuilder, Roots};
use common::{Behaviour, Folder, StubDriver, host};

const SLOW: &str = r#"manifest_format = 1
contract = 1

[provider]
name = "stand"
version = "0.1.0"
publisher = "Example"
kind = "pack"

[[modes]]
name = "stand"
archetype = "request"
verbs = ["read", "upsert"]
steps = ["fetch", "send"]
rank = 60

[[surface_kinds]]
name = "stand"
archetype = "request"

[[file_kinds]]
name = "call"
directory = "calls/"
surface_kinds = ["stand"]

[[controls]]
name = "box"

[points]
driver = ["stand"]

[effects]
read_roots = ["project"]
network = "surfaces"
"#;

const FAST: &str = r#"manifest_format = 1
contract = 1

[provider]
name = "stand_quick"
version = "0.1.0"
publisher = "Example"
kind = "pack"

[[modes]]
name = "stand_fast"
archetype = "request"
verbs = ["read", "upsert"]
steps = ["fetch", "send"]
rank = 20

[points]
driver = ["stand_fast"]

[effects]
read_roots = ["project"]
network = "surfaces"
"#;

/// One pack with two modes over its one surface kind, as a pack that has
/// an interface mode and a faster request mode would state them.
const BOTH: &str = r#"manifest_format = 1
contract = 1

[provider]
name = "stand"
version = "0.1.0"
publisher = "Example"
kind = "pack"

[[modes]]
name = "stand"
archetype = "request"
verbs = ["read", "upsert"]
steps = ["fetch", "send"]
rank = 60

[[modes]]
name = "stand_fast"
archetype = "request"
verbs = ["read", "upsert"]
steps = ["fetch", "send"]
rank = 20

[[surface_kinds]]
name = "stand"
archetype = "request"

[[file_kinds]]
name = "call"
directory = "calls/"
surface_kinds = ["stand"]

[[controls]]
name = "box"

[points]
driver = ["stand", "stand_fast"]

[effects]
read_roots = ["project"]
network = "surfaces"
"#;

struct BothModes;

impl Pack for BothModes {
    fn manifest(&self) -> &'static str {
        BOTH
    }

    fn register(&self, _cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        for name in ["stand", "stand_fast"] {
            to.driver(
                Mode::new(name).unwrap(),
                Arc::new(StubDriver {
                    reached: Arc::new(Mutex::new(Vec::new())),
                    behaviour: Arc::new(Mutex::new(Behaviour::Done)),
                }),
            )?;
        }
        to.compiler(
            CompilerRegistrations::new()
                .surface_kind(stand::surface_kind_spec())
                .control(stand::control())
                .file_kind(stand::file_kind())
                .lowering(stand::lowering_as(Mode::new("stand").unwrap()))
                .lowering(stand::lowering_as(Mode::new("stand_fast").unwrap())),
        )
    }
}

fn both_modes_set() -> CompilerSet {
    let folder = Folder::new("ranks_both");
    let mut builder = RegistryBuilder::new(
        a2m_provider::ProviderSettings::default(),
        host(&folder, Roots::none()),
    );
    builder.add_pack(&BothModes);
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    registry.compiler_set().clone()
}

struct RankedPack {
    manifest: &'static str,
    mode: Mode,
    whole: bool,
}

impl Pack for RankedPack {
    fn manifest(&self) -> &'static str {
        self.manifest
    }

    fn register(&self, _cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        to.driver(
            self.mode.clone(),
            Arc::new(StubDriver {
                reached: Arc::new(Mutex::new(Vec::new())),
                behaviour: Arc::new(Mutex::new(Behaviour::Done)),
            }),
        )?;
        let mut registrations = CompilerRegistrations::new();
        if self.whole {
            registrations = registrations
                .surface_kind(stand::surface_kind_spec())
                .control(stand::control())
                .file_kind(stand::file_kind());
        }
        to.compiler(registrations.lowering(stand::lowering_as(self.mode.clone())))
    }
}

fn registry_set() -> CompilerSet {
    let folder = Folder::new("ranks");
    let mut builder = RegistryBuilder::new(
        a2m_provider::ProviderSettings::default(),
        host(&folder, Roots::none()),
    );
    let slow = RankedPack {
        manifest: SLOW,
        mode: Mode::new("stand").unwrap(),
        whole: true,
    };
    let fast = RankedPack {
        manifest: FAST,
        mode: Mode::new("stand_fast").unwrap(),
        whole: false,
    };
    builder.add_pack(&slow);
    builder.add_pack(&fast);
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    registry.compiler_set().clone()
}

fn example_files() -> Vec<(RelPath, Vec<u8>)> {
    let root = stand::example_root();
    let mut files = Vec::new();
    walk(root.path(), root.path(), &mut files);
    files
}

fn walk(base: &std::path::Path, at: &std::path::Path, out: &mut Vec<(RelPath, Vec<u8>)>) {
    for entry in std::fs::read_dir(at).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(base, &path, out);
        } else {
            let relative = path
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if let Ok(rel) = RelPath::new(&relative) {
                let text = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
                out.push((rel, text.replace("\r\n", "\n").into_bytes()));
            }
        }
    }
}

#[test]
fn two_manifests_each_give_their_modes_rank_to_the_compiler() {
    let set = registry_set();
    assert_eq!(set.mode_rank(&Mode::new("stand").unwrap()), 60);
    assert_eq!(set.mode_rank(&Mode::new("stand_fast").unwrap()), 20);
}

#[test]
fn a_registry_orders_an_operations_bindings_by_the_ranks_its_manifest_states() {
    let set = both_modes_set();
    assert_eq!(set.mode_rank(&Mode::new("stand").unwrap()), 60);
    assert_eq!(set.mode_rank(&Mode::new("stand_fast").unwrap()), 20);
    let mut files = example_files();
    let (path, bytes) = files
        .iter_mut()
        .find(|(known, _)| known.as_str() == "operations/order_get.yaml")
        .expect("the example's order_get file");
    let text = String::from_utf8(bytes.clone()).unwrap();
    let from = "    status: verified\n    call: order_get\n";
    assert!(text.contains(from), "{path}");
    *bytes = text
        .replacen(
            from,
            &format!("{from}  - mode: stand_fast\n    status: verified\n    call: order_get\n"),
            1,
        )
        .into_bytes();
    let sources = load_files(&TargetRoot::new("example"), &files, &set)
        .unwrap_or_else(|problems| panic!("{problems}"));
    let compiled = compile(&sources, &set, CompileMode::Publish, Version::new(1, 0, 0))
        .unwrap_or_else(|problems| panic!("{problems}"));
    let modes: Vec<String> = compiled
        .compiled_target
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == "example_order_get")
        .unwrap()
        .bindings
        .iter()
        .map(|binding| binding.mode.to_string())
        .collect();
    assert_eq!(modes, ["stand_fast", "stand"]);
}
