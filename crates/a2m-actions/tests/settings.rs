//! The settings loader: eight layers, what each may set, and where every
//! value came from (spec section 226).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "this file holds only tests; the helpers the tests share do not carry the test attribute"
)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use a2m_actions::settings::{Layer, SCHEMA, find};
use a2m_actions::{Settings, Sources, settings::SourceFile};
use a2m_core::{FailureClass, Value};

fn file(name: &str, text: &str) -> Option<SourceFile> {
    Some(SourceFile::new(PathBuf::from(name), text))
}

#[test]
fn an_empty_file_is_a_complete_configuration() {
    let none = Settings::load(&Sources::none()).unwrap();
    let empty = Settings::load(&Sources {
        installation: file("installation.toml", ""),
        user: file("user.toml", "\n# nothing\n"),
        project: file("anything2mcp.toml", ""),
        ..Sources::none()
    })
    .unwrap();
    assert_eq!(none.digest(), empty.digest());
    // The Solo profile, the standard preset, writes approved by a person
    // every run, nothing sent anywhere.
    assert_eq!(none.text("profile.name"), Some("solo"));
    assert_eq!(none.text("profile.preset"), Some("standard"));
    assert_eq!(none.text("policy.approve_writes"), Some("person_each_run"));
    assert_eq!(none.text("telemetry.export"), Some(""));
    assert_eq!(none.flag("telemetry.crash_reports"), Some(false));
    assert_eq!(none.flag("updates.check"), Some(false));
    assert_eq!(none.text("server.listen"), Some(""));
    assert_eq!(none.text("authoring.scout.run"), Some("on_add"));
    // Every key the schema declares has a value, and a layer.
    for setting in SCHEMA.iter().filter(|setting| !setting.is_pattern()) {
        let entry = none.entry(setting.key).unwrap();
        assert_eq!(entry.layer, Layer::Default, "{}", setting.key);
    }
    assert!(
        none.presets()
            .iter()
            .any(|preset| preset.name == "standard" && preset.chosen)
    );
}

#[test]
fn a_higher_layer_wins_key_by_key_and_config_show_names_the_layer_of_every_value() {
    let sources = Sources {
        installation: file(
            "installation.toml",
            "[logging]\nlevel = \"warn\"\n\n[evidence]\ntrace_retention_days = 90\n",
        ),
        user: file(
            "user.toml",
            "[logging]\nlevel = \"debug\"\nformat = \"json\"\n",
        ),
        project: file(
            "anything2mcp.toml",
            "[authoring.scout]\nrun = \"on_request\"\n",
        ),
        environment: BTreeMap::from([
            (
                "A2M_EVIDENCE__TRACE_RETENTION_DAYS".to_owned(),
                "7".to_owned(),
            ),
            ("A2M_HOME".to_owned(), "not a setting".to_owned()),
            ("PATH".to_owned(), "not ours".to_owned()),
        ]),
        ..Sources::none()
    }
    .with_flag("logging.level", "error");
    let settings = Settings::load(&sources).unwrap();
    assert_eq!(settings.text("logging.level"), Some("error"));
    assert_eq!(settings.int("evidence.trace_retention_days"), Some(7));

    let level = settings.entry("logging.level").unwrap();
    assert_eq!(level.layer, Layer::Flag);
    assert_eq!(level.origin, "the flag `--logging.level`");
    let lower: Vec<(Layer, &str)> = level
        .overridden
        .iter()
        .map(|lower| (lower.layer, lower.origin.as_str()))
        .collect();
    assert_eq!(
        lower,
        [
            (Layer::Default, "the built-in default"),
            (Layer::Installation, "`installation.toml`, line 2"),
            (Layer::User, "`user.toml`, line 2"),
        ]
    );
    assert_eq!(
        settings
            .entry("evidence.trace_retention_days")
            .unwrap()
            .origin,
        "the variable `A2M_EVIDENCE__TRACE_RETENTION_DAYS`"
    );

    // `config.show` prints every key with its value and its layer.
    let shown = settings.show(None).unwrap();
    let rows = shown["settings"].as_array().unwrap();
    assert_eq!(rows.len(), settings.entries().count());
    for row in rows {
        assert!(
            row["layer"].is_string() && row["origin"].is_string(),
            "{row}"
        );
    }
    let run = rows
        .iter()
        .find(|row| row["key"] == "authoring.scout.run")
        .unwrap();
    assert_eq!(run["layer"], "project");
    assert_eq!(run["value"], "on_request");
    // With a key, that key's documentation.
    let one = settings.show(Some("policy.max_records_per_run")).unwrap();
    assert!(
        one["settings"][0]["doc"]
            .as_str()
            .unwrap()
            .contains("most records")
    );
    assert_eq!(one["settings"][0]["protected"], true);
    let unknown = settings
        .show(Some("policy.max_record_per_run"))
        .unwrap_err();
    assert!(unknown.next_step().contains("policy.max_records_per_run"));
}

#[test]
fn an_unknown_key_is_refused_with_its_file_its_line_and_the_nearest_keys() {
    let failure = Settings::load(&Sources {
        user: file(
            "user.toml",
            "[logging]\nlevel = \"info\"\n\n[policy]\nmax_record_per_run = 5\n",
        ),
        ..Sources::none()
    })
    .unwrap_err();
    assert_eq!(failure.class(), FailureClass::Input);
    assert_eq!(failure.code().as_str(), "settings_invalid");
    assert!(
        failure.message().contains("`user.toml`, line 5"),
        "{failure}"
    );
    assert!(failure.next_step().contains("policy.max_records_per_run"));

    let wrong_type = Settings::load(&Sources {
        user: file("user.toml", "[policy]\nmax_records_per_run = \"many\"\n"),
        ..Sources::none()
    })
    .unwrap_err();
    assert!(wrong_type.message().contains("a whole number from 1 to"));

    let variable = Settings::load(&Sources {
        environment: BTreeMap::from([("A2M_LOGGING__LEVLE".to_owned(), "debug".to_owned())]),
        ..Sources::none()
    })
    .unwrap_err();
    assert!(variable.message().contains("A2M_LOGGING__LEVLE"));

    let not_toml = Settings::load(&Sources {
        project: file("anything2mcp.toml", "[logging\nlevel = 1\n"),
        ..Sources::none()
    })
    .unwrap_err();
    assert!(not_toml.message().contains("anything2mcp.toml"));

    // A guardrail has no key: a file that tries to set one fails as an
    // unknown key.
    for guardrail in [
        "[engine]\nmodel = \"any\"\n",
        "[evidence.redaction]\nenabled = false\n",
    ] {
        assert!(
            Settings::load(&Sources {
                installation: file("installation.toml", guardrail),
                ..Sources::none()
            })
            .is_err()
        );
    }
}

#[test]
fn the_upper_layers_may_tighten_a_protected_setting_and_never_loosen_it() {
    let loosen = "[policy]\nmax_records_per_run = 500\n";
    let tighten = "[policy]\nmax_records_per_run = 5\n";
    for (name, sources) in [
        (
            "user",
            Sources {
                user: file("user.toml", loosen),
                ..Sources::none()
            },
        ),
        (
            "project",
            Sources {
                project: file("anything2mcp.toml", loosen),
                ..Sources::none()
            },
        ),
        (
            "environment",
            Sources {
                environment: BTreeMap::from([(
                    "A2M_POLICY__MAX_RECORDS_PER_RUN".to_owned(),
                    "500".to_owned(),
                )]),
                ..Sources::none()
            },
        ),
        (
            "flag",
            Sources::none().with_flag("policy.max_records_per_run", "500"),
        ),
    ] {
        let failure = Settings::load(&sources).unwrap_err();
        assert_eq!(failure.class(), FailureClass::Policy, "{name}");
        assert_eq!(failure.code().as_str(), "setting_protected", "{name}");
        assert!(
            failure.message().contains("policy.max_records_per_run"),
            "{name}"
        );
        assert!(failure.message().contains(name), "{name}: {failure}");
    }
    // Tightening is never protected, from any layer.
    let tightened = Settings::load(&Sources {
        user: file("user.toml", tighten),
        ..Sources::none()
    })
    .unwrap();
    assert_eq!(tightened.int("policy.max_records_per_run"), Some(5));
    // The installation file may loosen: there it is visible as such. A
    // higher layer may then go back down, and not above it.
    let installation = Settings::load(&Sources {
        installation: file("installation.toml", loosen),
        user: file("user.toml", "[policy]\nmax_records_per_run = 200\n"),
        ..Sources::none()
    })
    .unwrap();
    assert_eq!(installation.int("policy.max_records_per_run"), Some(200));
    // The other directions: a list that grows, a switch that turns on, a
    // word further down its rank.
    for loosening in [
        "[files]\nallowed_roots = [\"<project>\", \"elsewhere\"]\n",
        "[authoring.capture]\nvalues = true\n",
        "[policy]\napprove_writes = \"caller_confirms\"\n",
        "[evidence]\ncapture_on_writes = \"standard\"\n[policy]\npre_approval = true\n",
    ] {
        let sources = Sources {
            installation: file(
                "installation.toml",
                "[evidence]\ncapture_on_writes = \"full\"\n[policy]\npre_approval = false\n",
            ),
            user: file("user.toml", loosening),
            ..Sources::none()
        };
        let failure = Settings::load(&sources).unwrap_err();
        assert_eq!(failure.code().as_str(), "setting_protected", "{loosening}");
    }
    // Every problem is reported, not only the first.
    let (_, problems) = Settings::load_all(&Sources {
        user: file(
            "user.toml",
            "[policy]\nmax_records_per_run = 500\nnope = 1\n",
        ),
        ..Sources::none()
    });
    assert_eq!(problems.len(), 2);
}

#[test]
fn an_installation_only_setting_and_a_locked_key_are_the_installations_alone() {
    let failure = Settings::load(&Sources {
        user: file("user.toml", "[store]\ndatabase = \"elsewhere.db\"\n"),
        ..Sources::none()
    })
    .unwrap_err();
    assert_eq!(failure.code().as_str(), "setting_protected");
    assert!(failure.next_step().contains("installation"));
    // A user-scope key may not come from the project file.
    assert!(
        Settings::load(&Sources {
            project: file("anything2mcp.toml", "[logging]\nlevel = \"debug\"\n"),
            ..Sources::none()
        })
        .is_err()
    );
    // The flags of `serve` may set an installation-only key; other flags not.
    let flags = Sources::none().with_flag("server.listen", "127.0.0.1:0");
    assert!(Settings::load(&flags).is_err());
    let serve = Settings::load(&Sources {
        serve: true,
        ..flags
    })
    .unwrap();
    assert_eq!(serve.entry("server.listen").unwrap().layer, Layer::Flag);

    // A lock: no higher layer changes the key in either direction.
    let locked = "[locks]\nkeys = [\"policy.*\", \"logging.level\"]\n";
    for change in [
        "[policy]\nmax_records_per_run = 5\n",
        "[logging]\nlevel = \"debug\"\n",
    ] {
        let failure = Settings::load(&Sources {
            installation: file("installation.toml", locked),
            user: file("user.toml", change),
            ..Sources::none()
        })
        .unwrap_err();
        assert_eq!(failure.code().as_str(), "setting_locked", "{change}");
    }
    let kept = Settings::load(&Sources {
        installation: file("installation.toml", locked),
        user: file("user.toml", "[logging]\nformat = \"json\"\n"),
        ..Sources::none()
    })
    .unwrap();
    assert!(kept.entry("policy.max_records_per_run").unwrap().locked);
    assert!(!kept.entry("logging.format").unwrap().locked);
    // Only the installation file may lock.
    assert!(
        Settings::load(&Sources {
            user: file("user.toml", locked),
            ..Sources::none()
        })
        .is_err()
    );
}

#[test]
fn a_preset_is_layer_three_tightens_only_and_is_named_as_the_source() {
    let cautious = Settings::load(&Sources {
        user: file(
            "user.toml",
            "[profile]\npreset = \"cautious\"\n[sessions]\nkeep_alive_hours = 2\n",
        ),
        ..Sources::none()
    })
    .unwrap();
    let records = cautious.entry("policy.max_records_per_run").unwrap();
    assert_eq!(records.value, Value::Int(10));
    assert_eq!(records.layer, Layer::Preset);
    assert_eq!(records.origin, "the preset `cautious`");
    assert_eq!(cautious.text("policy.agent_effect_ceiling"), Some("read"));
    assert_eq!(cautious.text("authoring.scout.run"), Some("on_request"));
    // The user's file still overrides what the preset set.
    assert_eq!(cautious.int("sessions.keep_alive_hours"), Some(2));
    assert!(
        cautious
            .presets()
            .iter()
            .any(|preset| preset.name == "cautious" && preset.chosen)
    );

    let ci = Settings::load(&Sources {
        environment: BTreeMap::from([("A2M_PROFILE__PRESET".to_owned(), "ci".to_owned())]),
        ..Sources::none()
    })
    .unwrap();
    assert_eq!(ci.text("engine.person"), Some("absent"));
    assert_eq!(ci.int("sessions.login_wait_minutes"), Some(0));

    // A site's own preset: listed, and refused when it loosens.
    let site = |text: &str| Sources {
        presets: vec![SourceFile::new(
            PathBuf::from("presets").join("night.toml"),
            text,
        )],
        user: file("user.toml", "[profile]\npreset = \"night\"\n"),
        ..Sources::none()
    };
    let night = Settings::load(&site("[policy]\nmax_records_per_run = 20\n")).unwrap();
    assert_eq!(night.int("policy.max_records_per_run"), Some(20));
    let loosening = Settings::load(&site("[policy]\nmax_records_per_run = 2000\n")).unwrap_err();
    assert_eq!(loosening.code().as_str(), "setting_protected");
    assert!(Settings::load(&site("[profile]\npreset = \"ci\"\n")).is_err());
    let missing = Settings::load(&Sources {
        user: file("user.toml", "[profile]\npreset = \"nowhere\"\n"),
        ..Sources::none()
    })
    .unwrap_err();
    assert!(missing.next_step().contains("cautious"));

    // A server profile changes a default, as layer two.
    let team = Settings::load(&Sources {
        installation: file("installation.toml", "[profile]\nname = \"team\"\n"),
        ..Sources::none()
    })
    .unwrap();
    let scout = team.entry("authoring.scout.run").unwrap();
    assert_eq!(
        (scout.value.as_text(), scout.layer),
        (Some("on_request"), Layer::Profile)
    );
}

const PACK: &str = r#"
manifest_format = 1
contract = 1

[provider]
name = "example_pack"
version = "1.0.0"
publisher = "example"
kind = "pack"

[[settings]]
key = "page_budget"
type = "int"
default = 4
min = 1
max = 64
doc = "Open pages for all sessions together."

[[settings]]
key = "attach"
type = "bool"
default = false
protected = true
safer = "off"
doc = "Whether the pack may attach to a program it did not start."
"#;

#[test]
fn a_packs_group_is_checked_by_the_schema_its_manifest_declares() {
    let with_pack = |user: &str| {
        Sources {
            user: file("user.toml", user),
            ..Sources::none()
        }
        .with_pack_manifest(PACK)
        .unwrap()
    };
    let settings = Settings::load(&with_pack("[packs.example_pack]\npage_budget = 8\n")).unwrap();
    assert_eq!(settings.int("packs.example_pack.page_budget"), Some(8));
    assert_eq!(
        settings.entry("packs.example_pack.attach").unwrap().layer,
        Layer::Default
    );
    assert!(
        settings
            .entry("packs.example_pack.attach")
            .unwrap()
            .protected
    );
    // What the registry is built from: the group as it was written.
    let provider = settings.provider_settings();
    let group = &provider.packs[&a2m_provider::ProviderName::new("example_pack").unwrap()];
    assert_eq!(group.get("page_budget"), Some(&Value::Int(8)));
    assert!(group.get("attach").is_none());

    for (wrong, says) in [
        ("[packs.example_pack]\npage_budgte = 8\n", "page_budget"),
        ("[packs.example_pack]\npage_budget = 900\n", "page_budget"),
        ("[packs.example_pack]\nattach = true\n", "protected"),
        // A group for a pack that is not installed is an unknown key.
        ("[packs.other]\npage_budget = 8\n", "not a setting"),
    ] {
        let failure = Settings::load(&with_pack(wrong)).unwrap_err();
        assert!(
            failure.message().contains(says) || failure.next_step().contains(says),
            "{wrong}: {failure} / {}",
            failure.next_step()
        );
    }
}

#[test]
fn every_key_the_crate_reads_is_one_the_schema_declares() {
    // A key read anywhere that the schema does not declare fails here.
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stack = vec![src];
    let mut read = 0;
    while let Some(folder) = stack.pop() {
        for entry in std::fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for accessor in [".int(\"", ".text(\"", ".flag(\"", ".list(\""] {
                for rest in text.split(accessor).skip(1) {
                    let key = rest.split('"').next().unwrap();
                    if key.contains('.') {
                        read += 1;
                        assert!(find(key).is_some(), "{} reads `{key}`", path.display());
                    }
                }
            }
        }
    }
    assert!(read > 5, "the scan found the reads: {read}");
}
