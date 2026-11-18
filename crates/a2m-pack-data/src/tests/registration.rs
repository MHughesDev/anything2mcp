//! The manifest and the registration: a registry that holds only this pack.

use std::sync::Arc;

use a2m_core::{Archetype, Availability, NoSecrets, Values};
use a2m_provider::{Manifest, Pack, SurfaceDescription};

use super::{Folder, mode, registry_of};
use crate::names::{DATABASE, STATEMENT, codes};

#[test]
fn a_registry_with_only_this_pack_builds_and_its_digest_is_stable() {
    let folder = Folder::new("registry");
    let pack = crate::pack();
    let first = registry_of(&pack, &folder, Arc::new(NoSecrets));
    let second = registry_of(&crate::pack(), &folder, Arc::new(NoSecrets));
    assert_eq!(first.digest(), second.digest());

    assert_eq!(first.archetype(&mode()), Some(Archetype::Request));
    let handle = first.driver(&mode()).unwrap();
    assert_eq!(handle.choice().provider.as_str(), "data");
    let entry = first.surface_kind(&DATABASE).unwrap();
    assert!(entry.session);
    let set = first.compiler_set();
    assert_eq!(set.modes(), vec![&mode()]);
    #[cfg(not(feature = "postgres"))]
    assert_eq!(
        set.file_kinds().collect::<Vec<_>>(),
        vec![&(STATEMENT, DATABASE)]
    );
    #[cfg(feature = "postgres")]
    {
        use crate::names::DATABASE_SERVER;
        assert_eq!(
            set.file_kinds().collect::<Vec<_>>(),
            vec![&(STATEMENT, DATABASE), &(STATEMENT, DATABASE_SERVER)]
        );
        assert!(first.surface_kind(&DATABASE_SERVER).unwrap().session);
        let server = SurfaceDescription {
            kind: DATABASE_SERVER,
            archetype: Archetype::Request,
            address: "127.0.0.1".to_owned(),
            facts: Values::new(),
        };
        assert_eq!(first.discoverers_for(&server).len(), 1);
    }
    assert_eq!(set.verbs(&mode()).len(), 6);
    assert_eq!(set.drafting_rules().count(), 2);
    let surface = SurfaceDescription {
        kind: DATABASE,
        archetype: Archetype::Request,
        address: "example.db".to_owned(),
        facts: Values::new(),
    };
    assert_eq!(first.discoverers_for(&surface).len(), 1);
    let listing = first.listing();
    assert_eq!(listing.len(), 1);
    assert_eq!(listing[0].availability, Availability::Ready);
    assert!(listing[0].registered);
}

#[test]
fn the_manifest_declares_every_code_the_driver_returns_and_no_network() {
    let manifest = Manifest::parse(crate::pack().manifest()).unwrap();
    for (class, code) in codes::ALL {
        assert!(
            manifest
                .failure_codes
                .iter()
                .any(|declared| declared.class == *class && declared.code.as_str() == *code),
            "{class}/{code} is not in the manifest"
        );
    }
    assert_eq!(
        manifest.provider.version.to_string(),
        crate::names::PACK_VERSION
    );
    assert_eq!(manifest.provider.name.as_str(), crate::names::PACK);
    // Without the server store the pack asks for no network; with it, only
    // the surfaces a target declares.
    #[cfg(not(feature = "postgres"))]
    assert_eq!(manifest.effects.network, a2m_provider::NetworkEffect::None);
    #[cfg(feature = "postgres")]
    {
        assert_eq!(
            manifest.effects.network,
            a2m_provider::NetworkEffect::Surfaces
        );
        for (class, code) in codes::SERVER {
            assert!(
                manifest
                    .failure_codes
                    .iter()
                    .any(|declared| declared.class == *class && declared.code.as_str() == *code),
                "{class}/{code} is not in the manifest"
            );
        }
    }
    assert!(manifest.effects.processes.is_empty());
}

/// The server build's manifest is the embedded one with each change made in
/// exactly the place it is made, and nothing lost.
#[cfg(feature = "postgres")]
#[test]
fn the_server_manifest_is_the_embedded_one_with_its_additions() {
    let base = include_str!("../../anything2mcp-provider.toml");
    assert_eq!(base.matches("surface_kinds = [\"database\"]").count(), 3);
    assert_eq!(base.matches("network = \"none\"").count(), 1);
    assert_eq!(base.matches("sessions = [\"database\"]").count(), 1);
    let server = crate::SERVER_MANIFEST.as_str();
    assert_eq!(
        server
            .matches("surface_kinds = [\"database\", \"database_server\"]")
            .count(),
        3
    );
    let embedded = Manifest::parse(base).unwrap();
    let built = Manifest::parse(server).unwrap();
    assert_eq!(built.modes.len(), embedded.modes.len());
    assert_eq!(built.surface_kinds.len(), 2);
    assert_eq!(built.failure_codes.len(), embedded.failure_codes.len() + 5);
}

#[test]
fn commit_is_the_one_kind_that_saves_in_the_lowering_and_in_the_manifest() {
    use a2m_files::{Lowering, StepCommits};

    use crate::names::COMMIT;

    let lowering = crate::lower::SqlLowering::new(mode());
    for kind in lowering.step_kinds() {
        let stated = lowering.commits(kind);
        if *kind == COMMIT {
            assert_eq!(stated, StepCommits::Always);
        } else {
            assert_eq!(stated, StepCommits::Never, "{kind}");
        }
    }
    // The manifest lists the same kind, so the handle refuses a `commit`
    // sent as a step that commits nothing.
    let manifest = Manifest::parse(crate::pack().manifest()).unwrap();
    assert_eq!(manifest.modes[0].commit_steps, vec![COMMIT]);
}
