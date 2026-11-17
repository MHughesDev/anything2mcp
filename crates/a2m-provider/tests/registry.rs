//! The manifest's checks and the registry: what is refused, what is chosen,
//! and that the order of registration changes nothing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use a2m_core::{Archetype, Availability, FailureClass, Mode, Values};
use a2m_provider::{
    FoundManifest, Manifest, ProviderName, ProviderSettings, RegistryBuilder, Roots,
    SurfaceDescription, codes,
};
use common::{Folder, TestPack, Twist, host, manifest, mode};

fn build(
    packs: &[&TestPack],
    settings: ProviderSettings,
) -> (a2m_provider::Registry, Vec<a2m_provider::RegistryProblem>) {
    let folder = Folder::new("registry");
    let mut builder = RegistryBuilder::new(settings, host(&folder, Roots::none()));
    for pack in packs {
        builder.add_pack(*pack);
    }
    builder.build()
}

fn name(text: &str) -> ProviderName {
    ProviderName::new(text).unwrap()
}

#[test]
fn a_pack_registers_a_mode_a_file_kind_a_discoverer_and_an_auth_provider() {
    let pack = TestPack::new("alpha");
    let (registry, problems) = build(&[&pack], ProviderSettings::default());
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(registry.archetype(&mode()), Some(Archetype::Request));
    let defaults = registry.mode_defaults(&mode()).unwrap();
    assert_eq!(
        (
            defaults.rank,
            defaults.session_reads,
            defaults.intent_window
        ),
        (40, 4, 1)
    );
    let handle = registry.driver(&mode()).unwrap();
    assert_eq!(handle.choice().provider, name("alpha"));
    assert_eq!(registry.auth_providers().len(), 1);
    let set = registry.compiler_set();
    assert_eq!(set.modes(), vec![&mode()]);
    assert_eq!(set.file_kinds().count(), 1);
    let kind = a2m_core::SurfaceKind::new("alpha_surface").unwrap();
    let entry = registry.surface_kind(&kind).unwrap();
    assert!(entry.session);
    let surface = SurfaceDescription {
        kind,
        archetype: Archetype::Request,
        address: "https://example.test".to_owned(),
        facts: Values::new(),
    };
    assert_eq!(registry.discoverers_for(&surface).len(), 1);
    let listing = registry.listing();
    assert_eq!(listing.len(), 1);
    assert_eq!(listing[0].availability, Availability::Ready);
    assert!(listing[0].registered);
    // The pack was handed its settings, validated, with the default filled in.
    let seen = pack.seen_settings.lock().unwrap().clone().unwrap();
    assert_eq!(
        seen.get("page_size").and_then(a2m_core::Value::as_int),
        Some(50)
    );
}

#[test]
fn a_manifest_that_asks_for_an_undeclared_effect_is_refused() {
    // A point that names a mode the manifest does not declare.
    let asks =
        manifest("alpha", "request", "").replace(r#"driver = ["stand"]"#, r#"driver = ["other"]"#);
    let failure = Manifest::parse(&asks).unwrap_err();
    assert_eq!(failure.class(), FailureClass::Policy);
    assert_eq!(failure.code(), &codes::EFFECT_NOT_DECLARED);
    // A session on a surface kind that `effects.sessions` does not list.
    let asks = manifest("alpha", "request", "")
        .replace(r#"sessions = ["alpha_surface"]"#, "sessions = []");
    let failure = Manifest::parse(&asks).unwrap_err();
    assert_eq!(failure.code(), &codes::EFFECT_NOT_DECLARED);
    // And through the registry: the pack is listed with it and registers nothing.
    let pack = TestPack::with_manifest("alpha", asks);
    let (registry, problems) = build(&[&pack], ProviderSettings::default());
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].failure.code(), &codes::EFFECT_NOT_DECLARED);
    assert_eq!(registry.archetype(&mode()), None);
}

#[test]
fn a_registration_beyond_the_manifest_is_refused_and_the_pack_registers_nothing() {
    let pack = TestPack::new("alpha").twisted(Twist::UndeclaredDriver);
    let (registry, problems) = build(&[&pack], ProviderSettings::default());
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].failure.class(), FailureClass::Policy);
    assert_eq!(problems[0].failure.code(), &codes::EFFECT_NOT_DECLARED);
    assert!(!problems[0].registered);
    assert_eq!(
        registry.driver(&mode()).unwrap_err().code(),
        &codes::MODE_UNKNOWN
    );
    assert_eq!(registry.compiler_set().file_kinds().count(), 0);
    let listing = registry.listing();
    assert!(!listing[0].registered);
    assert_eq!(listing[0].problems.len(), 1);
}

#[test]
fn a_mode_registered_twice_is_refused() {
    // Twice in one manifest.
    let twice = manifest(
        "alpha",
        "request",
        "\n[[modes]]\nname = \"stand\"\narchetype = \"command\"\n",
    );
    let failure = Manifest::parse(&twice).unwrap_err();
    assert_eq!(failure.class(), FailureClass::Input);
    assert_eq!(failure.code(), &codes::MODE_CONFLICT);
    // By two packs, with two archetypes: both are refused, whatever the order.
    let request = TestPack::new("alpha");
    let command = TestPack::with_manifest("beta", manifest("beta", "command", ""));
    for packs in [[&request, &command], [&command, &request]] {
        let (registry, problems) = build(&packs, ProviderSettings::default());
        let conflicts: Vec<_> = problems
            .iter()
            .filter(|problem| problem.failure.code() == &codes::MODE_CONFLICT)
            .map(|problem| problem.provider.clone().unwrap().to_string())
            .collect();
        assert_eq!(conflicts, ["alpha", "beta"]);
        assert_eq!(registry.archetype(&mode()), None);
    }
}

#[test]
fn a_badly_formed_name_or_file_is_refused_with_the_line() {
    let bad_mode =
        manifest("alpha", "request", "").replace(r#"name = "stand""#, r#"name = "Stand Mode""#);
    let failure = Manifest::parse(&bad_mode).unwrap_err();
    assert_eq!(
        (failure.class(), failure.code()),
        (FailureClass::Input, &codes::MANIFEST_INVALID)
    );
    assert!(
        failure.next_step().contains("line 11"),
        "{}",
        failure.next_step()
    );
    let bad_provider = manifest("Alpha", "request", "");
    assert_eq!(
        Manifest::parse(&bad_provider).unwrap_err().code(),
        &codes::MANIFEST_INVALID
    );
    let unknown_key = manifest("alpha", "request", "\n[surprise]\nkey = 1\n");
    assert_eq!(
        Manifest::parse(&unknown_key).unwrap_err().code(),
        &codes::MANIFEST_INVALID
    );
    let contract = manifest("alpha", "request", "").replace("contract = 1", "contract = 7");
    assert_eq!(
        Manifest::parse(&contract).unwrap_err().code(),
        &codes::CONTRACT_UNSUPPORTED
    );
    let program = manifest("alpha", "request", "").replace(
        "[effects]",
        "[effects]\nprocesses = [{ path = \"../outside/tool\" }]",
    );
    assert_eq!(
        Manifest::parse(&program).unwrap_err().code(),
        &codes::PATH_NOT_ALLOWED
    );
    // A pack has no folder of its own: a relative program path would be
    // looked for in the engine's working folder.
    let relative = manifest("alpha", "request", "").replace(
        "[effects]",
        "[effects]\nprocesses = [{ path = \"bin/tool\" }]",
    );
    assert_eq!(
        Manifest::parse(&relative).unwrap_err().code(),
        &codes::PATH_NOT_ALLOWED
    );
    let plugin = relative.replace(r#"kind = "pack""#, r#"kind = "plugin""#);
    assert_ne!(
        Manifest::parse(&plugin).unwrap_err().code(),
        &codes::PATH_NOT_ALLOWED,
        "a plug-in may name a program inside its own folder"
    );
    // A code that states the kernel's own bookkeeping is not a provider's
    // to declare, whatever class it gives it.
    for code in codes::KERNEL_ONLY {
        let kernels = manifest(
            "alpha",
            "request",
            &format!("\n[[failure_codes]]\nclass = \"busy\"\ncode = \"{code}\"\n"),
        );
        assert_eq!(
            Manifest::parse(&kernels).unwrap_err().code(),
            &codes::MANIFEST_INVALID,
            "{code}"
        );
    }
    // A step kind that always commits is one of the mode's step kinds.
    let commits = manifest("alpha", "request", "").replace(
        "steps = [\"fetch\", \"send\"]",
        "steps = [\"fetch\", \"send\"]\ncommit_steps = [\"save\"]",
    );
    assert_eq!(
        Manifest::parse(&commits).unwrap_err().code(),
        &codes::MANIFEST_INVALID
    );
    // A step kind whose absence is authoritative is one of the mode's
    // step kinds, and never one that always commits.
    for absence in [
        "absence_steps = [\"look\"]",
        "commit_steps = [\"send\"]\nabsence_steps = [\"send\"]",
    ] {
        let declares = manifest("alpha", "request", "").replace(
            "steps = [\"fetch\", \"send\"]",
            &format!("steps = [\"fetch\", \"send\"]\n{absence}"),
        );
        assert_eq!(
            Manifest::parse(&declares).unwrap_err().code(),
            &codes::MANIFEST_INVALID,
            "{absence}"
        );
    }
    let level = manifest("alpha", "request", "")
        .replace(r#"max_level = "discovered""#, r#"max_level = "approved""#);
    assert_eq!(
        Manifest::parse(&level).unwrap_err().code(),
        &codes::MANIFEST_INVALID
    );
}

#[test]
fn the_digest_is_the_same_in_any_order_and_changes_with_a_registration() {
    let alpha = TestPack::new("alpha");
    let beta = TestPack::with_manifest(
        "beta",
        manifest("beta", "request", "")
            .replace(r#"name = "stand""#, r#"name = "other""#)
            .replace(r#"driver = ["stand"]"#, "driver = []"),
    )
    .twisted(Twist::NoDriver);
    let gamma = TestPack::with_manifest(
        "gamma",
        manifest("gamma", "request", "")
            .replace(r#"name = "stand""#, r#"name = "third""#)
            .replace(r#"driver = ["stand"]"#, "driver = []"),
    )
    .twisted(Twist::NoDriver);
    let orders: [[&TestPack; 3]; 6] = [
        [&alpha, &beta, &gamma],
        [&alpha, &gamma, &beta],
        [&beta, &alpha, &gamma],
        [&beta, &gamma, &alpha],
        [&gamma, &alpha, &beta],
        [&gamma, &beta, &alpha],
    ];
    let mut digests = Vec::new();
    for order in orders {
        let (registry, problems) = build(&order, ProviderSettings::default());
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(
            registry.driver(&mode()).unwrap().choice().provider,
            name("alpha")
        );
        digests.push(registry.digest());
    }
    assert!(digests.windows(2).all(|pair| pair[0] == pair[1]));

    // One registration less: another digest.
    let (fewer, _) = build(&[&alpha, &beta], ProviderSettings::default());
    assert_ne!(fewer.digest(), digests[0]);
    // The same packs with one setting changed: another digest.
    let mut settings = ProviderSettings::default();
    settings
        .packs
        .insert(name("alpha"), Values::new().with("page_size", 10_i64));
    let (changed, problems) = build(&[&alpha, &beta, &gamma], settings);
    assert!(problems.is_empty(), "{problems:?}");
    assert_ne!(changed.digest(), digests[0]);
    // A pack that registers one object less: another digest.
    let alpha_without = TestPack::new("alpha").twisted(Twist::NoDriver);
    let (without, _) = build(
        &[&alpha_without, &beta, &gamma],
        ProviderSettings::default(),
    );
    assert_ne!(without.digest(), digests[0]);
}

#[test]
fn two_ready_providers_and_no_setting_are_ambiguous_and_a_setting_names_one() {
    let alpha = TestPack::new("alpha");
    // Two packs that each drive `stand`; only the first brings its lowering.
    let second = TestPack::with_manifest("beta", manifest("beta", "request", ""));
    struct DriverOnly<'a>(&'a TestPack);
    impl a2m_provider::Pack for DriverOnly<'_> {
        fn manifest(&self) -> &'static str {
            self.0.manifest
        }
        fn register(
            &self,
            _cx: &a2m_provider::PackContext,
            to: &mut a2m_provider::Registrar<'_>,
        ) -> Result<(), a2m_core::Failure> {
            to.driver(
                mode(),
                std::sync::Arc::new(common::StubDriver {
                    reached: self.0.reached.clone(),
                    behaviour: self.0.behaviour.clone(),
                }),
            )
        }
    }
    let folder = Folder::new("ambiguous");
    let build_two = |settings: ProviderSettings| {
        let mut builder = RegistryBuilder::new(settings, host(&folder, Roots::none()));
        builder.add_pack(&alpha);
        builder.add_pack(&DriverOnly(&second));
        builder.build()
    };
    let (registry, problems) = build_two(ProviderSettings::default());
    assert!(problems.is_empty(), "{problems:?}");
    let failure = registry.driver(&mode()).unwrap_err();
    assert_eq!(
        (failure.class(), failure.code()),
        (FailureClass::Input, &codes::PROVIDER_AMBIGUOUS)
    );
    assert!(
        failure.message().contains("alpha (ready)") && failure.message().contains("beta (ready)")
    );

    let mut settings = ProviderSettings::default();
    settings.mode_providers.insert(mode(), name("beta"));
    let (registry, _) = build_two(settings);
    let handle = registry.driver(&mode()).unwrap();
    assert_eq!(handle.choice().provider, name("beta"));
    assert_eq!(
        handle.choice().reason,
        a2m_provider::ChoiceReason::NamedBySetting
    );
}

#[test]
fn a_provider_that_is_not_ready_is_never_replaced_by_another() {
    // The pack needs a program that is not on this machine.
    let missing = manifest(
        "alpha",
        "request",
        "\n[[requirements]]\nname = \"example-runtime\"\ndetect = { paths = [\"/nowhere/example-runtime\", \"C:/nowhere/example-runtime.exe\"] }\nmissing = \"Install Example Runtime.\"\n",
    );
    let pack = TestPack::with_manifest("alpha", missing);
    let (registry, problems) = build(&[&pack], ProviderSettings::default());
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(
        registry.listing()[0].availability,
        Availability::ComponentMissing
    );
    let failure = registry.driver(&mode()).unwrap_err();
    assert_eq!(
        (failure.class(), failure.code()),
        (FailureClass::Busy, &codes::PROVIDER_UNAVAILABLE)
    );
    assert_eq!(failure.next_step(), "Install Example Runtime.");
}

#[test]
fn a_disabled_pack_and_a_plug_in_are_listed_and_register_nothing() {
    let pack = TestPack::new("alpha");
    let mut settings = ProviderSettings::default();
    settings.disabled.insert(name("alpha"));
    let folder = Folder::new("listing");
    let mut builder = RegistryBuilder::new(settings, host(&folder, Roots::none()));
    builder.add_pack(&pack);
    let plugin = manifest("example_sheet", "artifact", "")
        .replace(r#"kind = "pack""#, r#"kind = "plugin""#)
        .replace(
            "[[modes]]",
            "[run]\nprogram = \"bin/example-sheet\"\nprotocol = [1]\n\n[[modes]]",
        );
    builder.add_plugin(&FoundManifest {
        text: plugin,
        folder: folder.path().to_path_buf(),
    });
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    let listing = registry.listing();
    assert_eq!(listing.len(), 2);
    assert!(
        listing
            .iter()
            .all(|one| one.availability == Availability::NotEnabled && !one.registered)
    );
    assert!(
        pack.seen_settings.lock().unwrap().is_none(),
        "a disabled pack is never asked to register"
    );
    assert_eq!(
        registry.driver(&mode()).unwrap_err().code(),
        &codes::MODE_UNKNOWN
    );
}

#[test]
fn a_kind_has_one_owner_and_a_reader_of_a_missing_owner_stays_registered() {
    // Two packs own one file kind for one surface kind.
    let alpha = TestPack::new("alpha");
    let rival = TestPack::with_manifest(
        "beta",
        manifest("beta", "request", "")
            .replace(r#"name = "stand""#, r#"name = "other""#)
            .replace(r#"driver = ["stand"]"#, "driver = []")
            .replace(
                "[[finding_kinds]]",
                "[[file_kinds]]\nname = \"alpha_call\"\ndirectory = \"alpha_calls/\"\nsurface_kinds = [\"alpha_surface\"]\n\n[[finding_kinds]]",
            ),
    )
    .twisted(Twist::NoDriver);
    let (_, problems) = build(&[&alpha, &rival], ProviderSettings::default());
    let conflicts: Vec<_> = problems
        .iter()
        .filter(|problem| problem.failure.code() == &codes::KIND_OWNER_CONFLICT)
        .collect();
    assert_eq!(conflicts.len(), 2);

    // A pack reads a kind whose owner is disabled: listed, and still registered.
    let reader = TestPack::with_manifest(
        "beta",
        manifest("beta", "request", "")
            .replace(r#"name = "stand""#, r#"name = "other""#)
            .replace(r#"driver = ["stand"]"#, "driver = []")
            .replace(
                "[points]",
                "[[reads]]\nkind = \"alpha_call\"\nof = \"file_kinds\"\nsurface_kind = \"alpha_surface\"\n\n[points]",
            ),
    )
    .twisted(Twist::NoDriver);
    let mut settings = ProviderSettings::default();
    settings.disabled.insert(name("alpha"));
    let (registry, problems) = build(&[&alpha, &reader], settings);
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].failure.code(), &codes::KIND_OWNER_MISSING);
    assert!(problems[0].registered);
    assert!(problems[0].failure.next_step().contains("alpha"));
    assert_eq!(
        registry.archetype(&Mode::new("other").unwrap()),
        Some(Archetype::Request)
    );
    // With the owner enabled there is no problem.
    let (_, problems) = build(&[&alpha, &reader], ProviderSettings::default());
    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn a_pack_setting_that_does_not_fit_its_schema_is_refused() {
    let pack = TestPack::new("alpha");
    for wrong in [
        Values::new().with("page_size", 9000_i64),
        Values::new().with("page_size", "many"),
        Values::new().with("surprise", true),
    ] {
        let mut settings = ProviderSettings::default();
        settings.packs.insert(name("alpha"), wrong);
        let (registry, problems) = build(&[&pack], settings);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].failure.code(), &codes::PACK_SETTINGS_INVALID);
        assert_eq!(
            registry.listing()[0].availability,
            Availability::Misconfigured
        );
    }
}

#[test]
fn a_pack_whose_manifest_is_refused_is_named_with_its_problem_and_registers_nothing() {
    // A code of the contract declared under another class.
    let refused = format!(
        "{}\n[[failure_codes]]\nclass = \"transient\"\ncode = \"manifest_invalid\"\n",
        manifest("alpha", "request", "")
    );
    let pack = TestPack::with_manifest("alpha", refused);
    let (registry, problems) = build(&[&pack], ProviderSettings::default());
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].provider, Some(name("alpha")));
    assert_eq!(problems[0].failure.code(), &codes::MANIFEST_INVALID);
    assert!(
        problems[0].failure.message().contains("manifest_invalid"),
        "{:?}",
        problems[0].failure
    );
    assert!(registry.listing().is_empty());
    let named = registry.refused();
    assert_eq!(named.len(), 1);
    assert_eq!(named[0].name.as_deref(), Some("alpha"));
    assert_eq!(named[0].version.as_deref(), Some("0.1.0"));
    assert_eq!(named[0].problem, problems[0].failure);
    assert_eq!(registry.archetype(&mode()), None);
}
