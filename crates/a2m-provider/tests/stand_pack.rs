//! The compiler's test pack, `stand`, registered whole through the provider
//! contract: its compiler side from `a2m_files::testing`, and a driver
//! beside it. The crates above this one register it the same way.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::{Arc, Mutex};

use a2m_core::{DeclaredProgram, Digest, Failure, Version};
use a2m_files::{CompileMode, testing};
use a2m_provider::{
    CompilerRegistrations, Pack, PackContext, ProgramRef, ProviderSettings, Registrar,
    RegistryBuilder, Roots,
};
use common::{Behaviour, Folder, StubDriver, host};

const MANIFEST: &str = r#"manifest_format = 1
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
rank = 40

[[surface_kinds]]
name = "stand"
archetype = "request"
session = true

[points]
driver = ["stand"]

[[file_kinds]]
name = "call"
directory = "calls/"
surface_kinds = ["stand"]

[[finding_kinds]]
name = "stand_call"
surface_kinds = ["stand"]

[[controls]]
name = "box"

[[lint_rules]]
code = "stand.path_not_rooted"
level = "error"
summary = "A call's path starts with a slash."

[[drafting_rules]]
finding_kind = "stand_call"
file_kind = "call"

[effects]
network = "surfaces"
sessions = ["stand"]
"#;

struct StandPack;

impl Pack for StandPack {
    fn manifest(&self) -> &'static str {
        MANIFEST
    }

    fn register(&self, _cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        to.driver(
            testing::mode(),
            Arc::new(StubDriver {
                reached: Arc::new(Mutex::new(Vec::new())),
                behaviour: Arc::new(Mutex::new(Behaviour::Done)),
            }),
        )?;
        to.compiler(
            CompilerRegistrations::new()
                .surface_kind(testing::surface_kind_spec())
                .control(testing::control())
                .file_kind(testing::file_kind())
                .lowering(testing::lowering())
                .lint_rule(testing::lint_rule())
                .drafting_rule(testing::drafting_rule()),
        )
    }
}

#[test]
fn the_stand_pack_registers_whole_and_compiles_the_example_to_the_same_plan() {
    let folder = Folder::new("stand-pack");
    let mut builder =
        RegistryBuilder::new(ProviderSettings::default(), host(&folder, Roots::none()));
    builder.add_pack(&StandPack);
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    assert!(registry.driver(&testing::mode()).is_ok());

    let version = Version::new(1, 0, 0);
    let compile = |set: &a2m_files::CompilerSet| {
        let sources = a2m_files::load(&testing::example_root(), set).unwrap();
        a2m_files::compile(&sources, set, CompileMode::Publish, version)
            .unwrap_or_else(|problems| panic!("{problems}"))
            .encoded
            .hash
    };
    assert_eq!(
        compile(registry.compiler_set()),
        compile(&testing::stand_set().unwrap())
    );
}

#[test]
fn a_program_a_plan_declares_becomes_one_the_supervisor_takes() {
    let declared = DeclaredProgram {
        path: "/opt/example/bin/example-tool".to_owned(),
        digest: Some(Digest::of(b"example-tool")),
    };
    let program = ProgramRef::from(&declared);
    assert_eq!(program.path.to_str(), Some("/opt/example/bin/example-tool"));
    assert_eq!(program.digest, declared.digest);
}
