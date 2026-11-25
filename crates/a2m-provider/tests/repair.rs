//! The repair proposer a pack registers for a finding kind it owns
//! (decision 0999): once per kind, only for a kind its manifest owns, and
//! found by the registry for that kind alone.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;

use a2m_core::{
    Address, EvidenceLevel, Failure, FailureClass, FailureCode, Finding, FindingKind, ProducedBy,
    Source, Subject, Timestamp,
};
use a2m_provider::{
    Pack, PackContext, ProviderSettings, Registrar, Registry, RegistryBuilder, RepairAsk,
    RepairFile, RepairProposal, RepairProposer, Roots,
};
use common::{Folder, host};

const MANIFEST: &str = r#"manifest_format = 1
contract = 1

[provider]
name = "mending"
version = "0.1.0"
publisher = "Example"
kind = "pack"

[[surface_kinds]]
name = "web"
archetype = "interface"

[[finding_kinds]]
name = "claim"
surface_kinds = ["web"]

[effects]
network = "surfaces"
"#;

/// The test's proposer: it names the entry it was asked about.
struct Naming;

impl RepairProposer for Naming {
    fn propose(&self, ask: &RepairAsk<'_>) -> RepairProposal {
        match ask.files.first() {
            Some(file) => RepairProposal::Proposed {
                path: file.path.clone(),
                text: format!("{}\n# {}\n", file.text, ask.entry),
                because: vec![format!("asked about `{}`", ask.entry)],
            },
            None => RepairProposal::none("no file"),
        }
    }
}

/// A pack that registers a proposer for each kind named.
struct Mending(&'static [&'static str]);

impl Pack for Mending {
    fn manifest(&self) -> &'static str {
        MANIFEST
    }

    fn register(&self, _cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        for kind in self.0 {
            to.repair_proposer(FindingKind::new(kind).unwrap(), Arc::new(Naming))?;
        }
        Ok(())
    }
}

fn registry_of(pack: &Mending) -> (Registry, Vec<a2m_provider::RegistryProblem>) {
    let folder = Folder::new("repair");
    let mut builder =
        RegistryBuilder::new(ProviderSettings::default(), host(&folder, Roots::none()));
    builder.add_pack(pack);
    builder.build()
}

#[test]
fn a_repair_proposer_is_registered_only_for_a_kind_the_pack_owns_and_once() {
    for kinds in [&["claim", "other"][..], &["claim", "claim"][..]] {
        let (_, problems) = registry_of(&Mending(Box::leak(kinds.to_vec().into_boxed_slice())));
        assert_eq!(problems.len(), 1, "{kinds:?}: {problems:?}");
        assert_eq!(
            (
                problems[0].failure.class(),
                problems[0].failure.code().as_str()
            ),
            (FailureClass::Policy, "effect_not_declared"),
            "{kinds:?}"
        );
    }
}

#[test]
fn the_registry_gives_the_proposer_of_a_kind_and_none_for_another() {
    let (registry, problems) = registry_of(&Mending(&["claim"]));
    assert!(problems.is_empty(), "{problems:?}");
    assert!(
        registry
            .repair_proposer(&FindingKind::new("screen").unwrap())
            .is_none()
    );
    let proposer = registry
        .repair_proposer(&FindingKind::new("claim").unwrap())
        .unwrap();
    let now = Finding::about(
        Subject::Address(Address::new("https://example.test", "/").unwrap()),
        FindingKind::new("claim").unwrap(),
        "said",
        Source::LiveSystem,
        ProducedBy {
            provider: "mending".to_owned(),
            version: "0.1.0".to_owned(),
            component: None,
        },
        Timestamp::from_unix_millis(1_800_000_000_000),
    )
    .level(EvidenceLevel::Discovered)
    .build()
    .unwrap();
    let files = [RepairFile {
        path: "screens/form.yaml".to_owned(),
        text: "screen: form".to_owned(),
    }];
    let code = FailureCode::new("locator_not_found").unwrap();
    let proposed = proposer.propose(&RepairAsk {
        entry: "form",
        code: &code,
        files: &files,
        before: None,
        now: &now,
    });
    assert_eq!(
        proposed,
        RepairProposal::Proposed {
            path: "screens/form.yaml".to_owned(),
            text: "screen: form\n# form\n".to_owned(),
            because: vec!["asked about `form`".to_owned()],
        }
    );
}
