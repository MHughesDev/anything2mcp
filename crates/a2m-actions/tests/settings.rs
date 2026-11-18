//! The settings loader: eight layers, what each may set, and where every
//! value came from (spec section 226).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "this file holds only tests; the helpers the tests share do not carry the test attribute"
)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use a2m_actions::settings::{InstallationKeeper, Layer, SCHEMA, find};
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
            "[logging]\nlevel = \"warn\"\n\n[sessions]\nkeep_alive_hours = 90\n",
        ),
        user: file("user.toml", "[logging]\nlevel = \"debug\"\n"),
        project: file(
            "anything2mcp.toml",
            "[authoring.scout]\nrun = \"on_request\"\n",
        ),
        environment: BTreeMap::from([
            ("A2M_SESSIONS__KEEP_ALIVE_HOURS".to_owned(), "7".to_owned()),
            ("A2M_HOME".to_owned(), "not a setting".to_owned()),
            ("PATH".to_owned(), "not ours".to_owned()),
        ]),
        ..Sources::none()
    }
    .with_flag("logging.level", "error");
    let settings = Settings::load(&sources).unwrap();
    assert_eq!(settings.text("logging.level"), Some("error"));
    assert_eq!(settings.int("sessions.keep_alive_hours"), Some(7));

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
        settings.entry("sessions.keep_alive_hours").unwrap().origin,
        "the variable `A2M_SESSIONS__KEEP_ALIVE_HOURS`"
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
    // The installation file an administrator keeps may loosen: there it
    // is visible as such. A higher layer may then go back down, and not
    // above it.
    let installation = Settings::load(&Sources {
        installation: file("installation.toml", loosen),
        installation_keeper: InstallationKeeper::Administrator,
        user: file("user.toml", "[policy]\nmax_records_per_run = 200\n"),
        ..Sources::none()
    })
    .unwrap();
    assert_eq!(installation.int("policy.max_records_per_run"), Some(200));
    // An installation file another account can change, or one nobody
    // checked, is held to the person's rule (decision 0241): it may
    // tighten, and its loosening is refused by name with the file.
    for keeper in [
        InstallationKeeper::default(),
        InstallationKeeper::Open {
            path: PathBuf::from("installation"),
            why: "belongs to an account that is not the administrator's",
        },
    ] {
        let failure = Settings::load(&Sources {
            installation: file("installation.toml", loosen),
            installation_keeper: keeper.clone(),
            ..Sources::none()
        })
        .unwrap_err();
        assert_eq!(failure.code().as_str(), "setting_protected");
        assert!(
            failure.message().contains("installation.toml")
                && failure
                    .message()
                    .contains("only when no account but an administrator"),
            "{failure}"
        );
        let tightened = Settings::load(&Sources {
            installation: file("installation.toml", tighten),
            installation_keeper: keeper,
            ..Sources::none()
        })
        .unwrap();
        assert_eq!(tightened.int("policy.max_records_per_run"), Some(5));
    }
    // The other directions: a list that grows, a switch that turns on, a
    // word further down its rank.
    for loosening in [
        "[files]\nallowed_roots = [\"<project>\", \"elsewhere\"]\n",
        "[policy]\npreview_freshness_seconds = 120\n",
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
        user: file("user.toml", "[mcp]\nrevisions = [\"2026-07-28\"]\n"),
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
    let flags = Sources::none().with_flag("providers.disabled", "web");
    assert!(Settings::load(&flags).is_err());
    let serve = Settings::load(&Sources {
        serve: true,
        ..flags
    })
    .unwrap();
    assert_eq!(
        serve.entry("providers.disabled").unwrap().layer,
        Layer::Flag
    );

    // A lock: no higher layer changes the key in either direction.
    let locked = "[locks]\nkeys = [\"policy.*\", \"logging.level\"]\n";
    for change in [
        "[policy]\nmax_records_per_run = 5\n",
        "[logging]\nlevel = \"debug\"\n",
    ] {
        let failure = Settings::load(&Sources {
            installation: file("installation.toml", locked),
            installation_keeper: InstallationKeeper::Administrator,
            user: file("user.toml", change),
            ..Sources::none()
        })
        .unwrap_err();
        assert_eq!(failure.code().as_str(), "setting_locked", "{change}");
    }
    let kept = Settings::load(&Sources {
        installation: file("installation.toml", locked),
        installation_keeper: InstallationKeeper::Administrator,
        user: file("user.toml", "[tools]\nlisted_limit = 10\n"),
        ..Sources::none()
    })
    .unwrap();
    assert!(kept.entry("policy.max_records_per_run").unwrap().locked);
    assert!(!kept.entry("tools.listed_limit").unwrap().locked);
    assert_eq!(kept.int("tools.listed_limit"), Some(10));
    // A lock holds a value against the person's own layers: it is taken
    // only from a file an administrator keeps.
    let failure = Settings::load(&Sources {
        installation: file("installation.toml", locked),
        ..Sources::none()
    })
    .unwrap_err();
    assert_eq!(failure.code().as_str(), "setting_protected");
    assert!(
        failure.message().contains("lock keys only when"),
        "{failure}"
    );
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

/// Every Rust file of a product crate under `crates/`, with its tests cut
/// off: a file under a `tests` folder is left out, and a file's text ends
/// where its first `#[cfg(test)]` or `#[cfg(all(test, ...))]` begins.
fn product_sources() -> Vec<(PathBuf, String)> {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let mut found = Vec::new();
    let mut stack: Vec<PathBuf> = std::fs::read_dir(&crates)
        .unwrap()
        .map(|entry| entry.unwrap().path().join("src"))
        .filter(|src| src.is_dir())
        .collect();
    while let Some(folder) = stack.pop() {
        for entry in std::fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name != "tests") {
                    stack.push(path);
                }
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let end = text
                .lines()
                .scan(0, |at, line| {
                    let start = *at;
                    *at += line.len() + 1;
                    Some((start, line.trim_start()))
                })
                .find(|(_, line)| {
                    line.starts_with("#[cfg(test)]") || line.starts_with("#[cfg(all(test")
                })
                .map_or(text.len(), |(start, _)| start);
            found.push((path, text[..end].to_owned()));
        }
    }
    found.sort();
    found
}

fn named(path: &std::path::Path, tail: &str) -> bool {
    path.ends_with(tail)
}

/// The groups of the kernel's keys, and `packs`.
fn is_setting_group(key: &str) -> bool {
    let group = key.split('.').next().unwrap_or_default();
    group == "packs"
        || SCHEMA
            .iter()
            .any(|setting| setting.key.split('.').next() == Some(group))
}

#[test]
fn every_key_read_anywhere_is_one_the_schema_declares() {
    // A key read in any product crate that the schema does not declare
    // fails here (decision 0236): through the loader's accessors.
    let mut read = 0;
    for (path, text) in product_sources() {
        for accessor in [
            ".int(\"",
            ".text(\"",
            ".flag(\"",
            ".list(\"",
            ".entry(\"",
            ".get(\"",
        ] {
            for rest in text.split(accessor).skip(1) {
                let key = rest.split('"').next().unwrap();
                if key.contains('.') && is_setting_group(key) && !key.starts_with("packs.") {
                    read += 1;
                    assert!(find(key).is_some(), "{} reads `{key}`", path.display());
                }
            }
        }
    }
    assert!(read > 30, "the scan found the reads: {read}");
}

/// Where code reads `key`: its text as an argument of a call, directly or
/// through a constant that holds it; a key with a `*` part, by the prefix
/// before it given to `strip_prefix` and its last part named. The schema's
/// own file is not a reader.
fn readers(key: &str, sources: &[(PathBuf, String)]) -> Vec<String> {
    let mut found = Vec::new();
    for (path, text) in sources {
        if named(path, "settings/schema.rs") {
            continue;
        }
        let as_argument = |needle: &str| {
            text.match_indices(needle)
                .any(|(at, _)| text[..at].trim_end().ends_with(['(', ',']))
        };
        let reads = if let Some((prefix, last)) = key.split_once('*') {
            text.contains(&format!("strip_prefix(\"{prefix}\")"))
                && (last.is_empty() || text.contains(&format!("\"{}\"", &last[1..])))
        } else {
            let quoted = format!("\"{key}\"");
            as_argument(&quoted)
                || text.lines().any(|line| {
                    line.trim_start().contains("const ")
                        && line.contains(&format!("&str = {quoted};"))
                        && line
                            .split("const ")
                            .nth(1)
                            .and_then(|rest| rest.split(':').next())
                            .is_some_and(|name| {
                                as_argument(name.trim())
                                    || text.contains(&format!("({}", name.trim()))
                            })
                })
        };
        if reads {
            found.push(path.display().to_string());
        }
    }
    found
}

#[test]
fn every_key_marked_built_has_a_reader_and_no_key_marked_not_built_has_one() {
    // Decision 0236: the mark is held to the code in both directions. A key
    // marked built that nothing reads fails, and so does a key marked not
    // built that has gained a reader without its mark changing.
    let sources = product_sources();
    let mut built = 0;
    for setting in SCHEMA {
        let found = readers(setting.key, &sources);
        if setting.built.is_built() {
            built += 1;
            assert!(
                !found.is_empty(),
                "`{}` is marked built, and no product code reads it",
                setting.key
            );
        } else {
            assert!(
                found.is_empty(),
                "`{}` is marked not built, and {found:?} read it: mark it built",
                setting.key
            );
        }
    }
    assert!(built > 30, "{built}");
}

/// Decision 0236: a value for a key whose part is not built is refused by
/// name, from every layer, a value equal to the default too, with what
/// happens instead; the key stays declared and `config.show` marks it.
#[test]
fn a_key_whose_part_is_not_built_is_refused_by_name_from_every_layer() {
    let retention = "[evidence]\ntrace_retention_days = 30\n";
    let mut sources = vec![
        (
            "installation",
            Sources {
                installation: file("installation.toml", retention),
                installation_keeper: InstallationKeeper::Administrator,
                ..Sources::none()
            },
        ),
        (
            "user",
            Sources {
                user: file("user.toml", retention),
                ..Sources::none()
            },
        ),
        (
            "project",
            Sources {
                project: file(
                    "anything2mcp.toml",
                    "[authoring.scout.budget]\nseconds = 60\n",
                ),
                ..Sources::none()
            },
        ),
        (
            "variable",
            Sources {
                environment: BTreeMap::from([(
                    "A2M_EVIDENCE__TRACE_RETENTION_DAYS".to_owned(),
                    "7".to_owned(),
                )]),
                ..Sources::none()
            },
        ),
        (
            "flag",
            Sources::none().with_flag("evidence.trace_retention_days", "7"),
        ),
    ];
    sources.push((
        "a site's preset",
        Sources {
            presets: vec![SourceFile::new(
                "quiet.toml",
                "[logging]\nformat = \"json\"\n",
            )],
            user: file("user.toml", "[profile]\npreset = \"quiet\"\n"),
            ..Sources::none()
        },
    ));
    for (layer, sources) in sources {
        let failure = Settings::load(&sources).unwrap_err();
        assert_eq!(failure.code().as_str(), "settings_invalid", "{layer}");
        assert!(
            failure
                .message()
                .contains("is a setting whose part is not built yet"),
            "{layer}: {failure}"
        );
        assert!(
            failure.next_step().contains("It arrives with"),
            "{layer}: {failure}"
        );
    }
    let failure = Settings::load(&Sources {
        user: file("user.toml", retention),
        ..Sources::none()
    })
    .unwrap_err();
    assert!(
        failure
            .message()
            .contains("Traces and captures are kept; nothing is deleted yet."),
        "{failure}"
    );
    // Declared, shown, and marked.
    let shown = Settings::defaults()
        .show(Some("evidence.trace_retention_days"))
        .unwrap();
    let entry = &shown["settings"][0];
    assert_eq!(entry["built"], false, "{entry}");
    assert!(
        entry["instead"]
            .as_str()
            .unwrap()
            .contains("nothing is deleted")
    );
    assert!(entry["arrives_with"].is_string());
    let shown = Settings::defaults().show(Some("logging.level")).unwrap();
    assert_eq!(shown["settings"][0]["built"], true);
    assert!(shown["settings"][0].get("instead").is_none());
}

#[test]
fn no_built_in_preset_sets_a_key_that_is_not_built() {
    for preset in a2m_actions::settings::PRESETS {
        let chosen = Settings::load(&Sources {
            user: file(
                "user.toml",
                &format!("[profile]\npreset = \"{}\"\n", preset.name),
            ),
            ..Sources::none()
        })
        .unwrap_or_else(|failure| panic!("{}: {failure}", preset.name));
        for (key, _) in &chosen
            .presets()
            .iter()
            .find(|info| info.name == preset.name)
            .unwrap()
            .sets
        {
            assert!(
                find(key).unwrap().built.is_built(),
                "{}: {key}",
                preset.name
            );
        }
    }
}

/// Decision 0236, also decided: `policy.two_person_irreversible` is
/// protected with on the safer side. Once an installation an administrator
/// keeps turns it on, no lower layer turns it off; any layer turns it on.
#[test]
fn two_person_approval_once_on_is_never_turned_off_from_below() {
    let on = "[policy]\ntwo_person_irreversible = true\n";
    let off = "[policy]\ntwo_person_irreversible = false\n";
    let installed = || Sources {
        installation: file("installation.toml", on),
        installation_keeper: InstallationKeeper::Administrator,
        ..Sources::none()
    };
    for (name, sources) in [
        (
            "user.toml",
            Sources {
                user: file("user.toml", off),
                ..installed()
            },
        ),
        (
            "A2M_POLICY__TWO_PERSON_IRREVERSIBLE",
            Sources {
                environment: BTreeMap::from([(
                    "A2M_POLICY__TWO_PERSON_IRREVERSIBLE".to_owned(),
                    "false".to_owned(),
                )]),
                ..installed()
            },
        ),
        (
            "--policy.two_person_irreversible",
            installed().with_flag("policy.two_person_irreversible", "false"),
        ),
        (
            "anything2mcp.toml",
            Sources {
                project: file("anything2mcp.toml", off),
                ..installed()
            },
        ),
    ] {
        let failure = Settings::load(&sources).unwrap_err();
        assert_eq!(failure.code().as_str(), "setting_protected", "{name}");
        assert!(failure.message().contains(name), "{name}: {failure}");
    }
    // The person turns it on from their own file; an installation file
    // that is not an administrator's may turn it on too, and not off.
    let turned_on = Settings::load(&Sources {
        user: file("user.toml", on),
        ..Sources::none()
    })
    .unwrap();
    assert_eq!(turned_on.flag("policy.two_person_irreversible"), Some(true));
    assert!(
        Settings::load(&Sources {
            installation: file("installation.toml", on),
            user: file("user.toml", off),
            ..Sources::none()
        })
        .is_err()
    );
}

/// Decision 0241: the only way to take an installation file as an
/// administrator's without the check of the file on disk is the switch a
/// test of the product turns, and no product source but the one that
/// defines it names it; the only product sources that make the keeper an
/// administrator are the check and that switch.
#[test]
fn no_product_source_takes_an_installation_file_as_an_administrators_but_the_check() {
    let switch = "with_installation_kept_by_an_administrator_for_tests";
    let mut defined = 0;
    for (path, text) in product_sources() {
        if text.contains(switch) {
            assert!(
                named(&path, "anything2mcp/src/engine.rs"),
                "{} names the test's switch",
                path.display()
            );
            defined += 1;
        }
        if text.contains("InstallationKeeper::Administrator") {
            assert!(
                named(&path, "a2m-actions/src/settings/keeper.rs")
                    || named(&path, "anything2mcp/src/engine.rs"),
                "{} makes the installation's keeper an administrator",
                path.display()
            );
        }
    }
    assert_eq!(defined, 1);
}

/// No outbound proxy is built: a `network.proxy` that is set is refused by
/// name at load, where the engine starts, and never taken silently
/// (decision 0196, for 0197).
#[test]
fn a_proxy_setting_is_refused_while_no_proxy_is_supported() {
    let sources = Sources {
        installation: file(
            "settings.toml",
            "[network]\nproxy = \"http://proxy.example.invalid:3128\"\n",
        ),
        installation_keeper: InstallationKeeper::Administrator,
        ..Sources::none()
    };
    let failure = Settings::load(&sources).unwrap_err();
    assert_eq!(failure.code().as_str(), "settings_invalid", "{failure}");
    assert!(
        failure
            .message()
            .contains("No outbound proxy is supported yet"),
        "{failure}"
    );
    let (kept, problems) = Settings::load_all(&sources);
    assert_eq!(problems.len(), 1);
    assert_eq!(kept.text("network.proxy"), Some(""));
    // Empty, as by default, it is no problem.
    assert!(Settings::load(&Sources::none()).is_ok());
}

/// Decision 0220: the vault's back end defaults to what the build has, the
/// passphrase route where it has no key store, and only the installation
/// chooses it; a user's file or the environment that sets it is refused.
#[test]
fn the_vault_back_end_defaults_to_what_the_build_has_and_only_the_installation_chooses_it() {
    let none = Settings::load(&Sources::none()).unwrap();
    assert_eq!(none.text("vault.backend"), Some(a2m_vault::DEFAULT_BACKEND));
    #[cfg(target_os = "linux")]
    assert_eq!(none.text("vault.backend"), Some("passphrase"));
    for value in ["passphrase", "os_keystore"] {
        for sources in [
            Sources {
                user: file("user.toml", &format!("[vault]\nbackend = \"{value}\"\n")),
                ..Sources::none()
            },
            Sources {
                environment: BTreeMap::from([("A2M_VAULT__BACKEND".to_owned(), value.to_owned())]),
                ..Sources::none()
            },
        ] {
            let refused = Settings::load(&sources).unwrap_err();
            assert_eq!(refused.code().as_str(), "setting_protected", "{value}");
            assert!(refused.message().contains("vault.backend"), "{refused}");
        }
    }
    let installed = Settings::load(&Sources {
        installation: file("settings.toml", "[vault]\nbackend = \"passphrase\"\n"),
        ..Sources::none()
    })
    .unwrap();
    assert_eq!(installed.text("vault.backend"), Some("passphrase"));
}
