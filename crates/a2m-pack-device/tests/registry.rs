//! A registry that holds only the device pack builds, and what the pack
//! declares is what it registers.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use a2m_core::{AccessKind, Availability, FailureClass, Mode, SurfaceKind, Values};
use a2m_provider::{Exclusive, Manifest, SurfaceDescription};
use common::{Folder, registry};

#[test]
fn a_registry_with_only_the_device_pack_builds_and_its_digest_is_stable() {
    let folder = Folder::new("registry");
    let first = registry(&folder);
    let second = registry(&folder);
    assert_eq!(first.digest(), second.digest());

    let mode = Mode::new("device").unwrap();
    assert_eq!(first.access_kind(&mode), Some(AccessKind::Device));
    let defaults = first.mode_defaults(&mode).unwrap();
    // One request at a time, and one run at a time on one link.
    assert_eq!(
        (
            defaults.session_reads,
            defaults.session_writes,
            defaults.intent_window,
            defaults.exclusive
        ),
        (1, 1, 1, Exclusive::Link)
    );
    assert_eq!(
        first.driver(&mode).unwrap().choice().provider.as_str(),
        "device"
    );

    let kind = SurfaceKind::new("device_link").unwrap();
    assert!(first.surface_kind(&kind).unwrap().session);
    let set = first.compiler_set();
    assert_eq!(set.modes(), vec![&mode]);
    let verbs: Vec<&str> = set.verbs(&mode).iter().map(|verb| verb.as_str()).collect();
    assert_eq!(verbs, ["read", "submit", "update"]);
    assert_eq!(set.file_kinds().count(), 1);
    // From an identity, nothing, and the reason why; from a published map,
    // a map and its reads.
    assert_eq!(set.drafting_rules().count(), 3);
    let codes: Vec<String> = set.lint_codes().into_iter().map(|(code, _)| code).collect();
    for rule in [
        "device.writable_without_hazard",
        "device.writable_without_range",
        "device.command_hazard_none",
    ] {
        assert!(codes.iter().any(|code| code == rule), "{rule}");
    }

    let surface = SurfaceDescription {
        kind,
        access_kind: AccessKind::Device,
        address: "127.0.0.1:502".to_owned(),
        facts: Values::new(),
    };
    let discoverers = first.discoverers_for(&surface);
    let levels: Vec<(&str, a2m_core::EvidenceLevel)> = discoverers
        .iter()
        .map(|found| (found.declared().name.as_str(), found.declared().max_level))
        .collect();
    assert_eq!(
        levels,
        [
            ("device_identity", a2m_core::EvidenceLevel::Discovered),
            ("device_register_map", a2m_core::EvidenceLevel::Inferred),
        ]
    );
    let listing = first.listing();
    assert_eq!(listing.len(), 1);
    assert_eq!(listing[0].availability, Availability::Ready);
    assert!(listing[0].registered);
}

#[test]
fn every_failure_code_of_the_pack_is_declared_with_its_one_class() {
    let manifest = Manifest::parse(a2m_pack_device::MANIFEST).unwrap();
    let declared: Vec<(FailureClass, &str)> = manifest
        .failure_codes
        .iter()
        .map(|code| (code.class, code.code.as_str()))
        .collect();
    let mut own: Vec<(FailureClass, &str)> = a2m_pack_device::codes::ALL.to_vec();
    let mut listed = declared.clone();
    own.sort_by_key(|(_, code)| *code);
    listed.sort_by_key(|(_, code)| *code);
    assert_eq!(own, listed);
    // The defaults a person gets without setting anything: four frames a
    // second at most where a map states no pace, and one hundred frames a
    // run on a link.
    let default = |key: &str| {
        manifest
            .settings
            .iter()
            .find(|setting| setting.key == key)
            .unwrap_or_else(|| panic!("no setting `{key}`"))
            .default
            .as_int()
    };
    assert_eq!(default("min_gap_ms"), Some(250));
    assert_eq!(default("max_frames_per_run"), Some(100));
    // The pack reaches a network and hardware, holds a session for its
    // links, and starts no program.
    assert!(manifest.effects.devices);
    assert!(manifest.effects.processes.is_empty());
    assert!(!manifest.effects.compiled_target_programs);
    assert_eq!(manifest.effects.sessions.len(), 1);
}
