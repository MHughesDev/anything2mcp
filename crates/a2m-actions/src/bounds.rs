//! The protected policy settings of spec section 226, and what the policy
//! engine has in force for each.
//!
//! Whether something may happen is the policy engine's to decide, so a
//! setting about it is in force only as far as the policy engine decides by
//! it. This module says, for every such setting, how that is so. Where it
//! cannot be made so, the setting has exactly one value that is true, the
//! one the policy engine works by, and any other value stops the kernel
//! with the setting's name: a limit is never reported that is not kept.
//!
//! | Setting | In force through |
//! | --- | --- |
//! | `policy.approve_writes`, `policy.max_records_per_run`, `policy.approval_window_minutes` | The settings of the policy set, which `Policy::open` takes from the built-in set alone |
//! | `policy.agent_effect_ceiling` | The highest effect class the built-in set's grants give; the policy engine has no rule that lowers a grant for an agent |
//! | `policy.writes_outside_tests`, `policy.two_person_irreversible` | Nothing: the policy engine has no rule for either |
//! | `policy.pre_approval` | The guard: with it off, a write is never begun on a pre-approval, whatever the set in force says |
//! | `policy.preview_freshness_seconds` | The engine's freshness window |
//! | `files.allowed_roots` | The dispatcher's check of a stated project folder |

use a2m_core::{EffectClass, Value};
use a2m_policy::BuiltIn;

use crate::codes;
use crate::settings::Settings;

/// Whether a pre-approval rule may let a write begin without a person.
pub(crate) const PRE_APPROVAL: &str = "policy.pre_approval";

/// One protected setting the policy engine works by at one value only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Held {
    /// The setting's key.
    pub(crate) key: &'static str,
    /// The one value the policy engine has in force.
    pub(crate) in_force: Value,
    /// Why no other value can be put in force, as the end of a sentence.
    pub(crate) why: &'static str,
}

/// The protected settings that are held at the value the policy engine has
/// in force, read from the set the kernel opens the policy engine with:
/// what the kernel passes to `Policy::open` is that built-in set and
/// nothing else, so these are the values in force.
pub(crate) fn held(built_in: &BuiltIn) -> Vec<Held> {
    let layers = built_in.layers();
    let set = &layers.settings;
    let by_the_set = "the policy engine takes it from the policy set, which the kernel cannot open with another value";
    // The highest effect class any grant of the set gives is the ceiling an
    // agent has: nothing lowers a grant for a caller that is an agent.
    let ceiling = layers
        .grants
        .iter()
        .map(|grant| grant.max_effect)
        .max()
        .unwrap_or(EffectClass::Irreversible);
    vec![
        Held {
            key: "policy.approve_writes",
            in_force: Value::Text(set.approve_writes.as_str().to_owned()),
            why: by_the_set,
        },
        Held {
            key: "policy.max_records_per_run",
            in_force: Value::Int(i64::from(set.max_records_per_run)),
            why: by_the_set,
        },
        Held {
            key: "policy.approval_window_minutes",
            in_force: Value::Int(i64::from(set.approval_window_minutes)),
            why: by_the_set,
        },
        Held {
            key: "policy.agent_effect_ceiling",
            in_force: Value::Text(ceiling.as_str().to_owned()),
            why: "the policy engine has no rule that lowers a grant for a caller that is an agent",
        },
        Held {
            key: "policy.writes_outside_tests",
            in_force: Value::Bool(true),
            why: "the policy engine is not told which records an environment declares as test records, and has no rule about them",
        },
        Held {
            key: "policy.two_person_irreversible",
            in_force: Value::Bool(false),
            why: "the policy engine has no rule that asks for a second approver",
        },
    ]
}

/// A protected setting that says what is not in force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NotInForce {
    /// The setting's key.
    pub(crate) key: &'static str,
    /// The refusal, which names the setting, the value it was given, the
    /// value in force, and where the setting was set.
    pub(crate) failure: a2m_core::Failure,
}

/// Every protected setting whose value is not the one in force.
pub(crate) fn not_in_force(settings: &Settings, built_in: &BuiltIn) -> Vec<NotInForce> {
    held(built_in)
        .into_iter()
        .filter_map(|held| {
            let entry = settings.entry(held.key)?;
            if entry.value == held.in_force {
                return None;
            }
            let shown = |value: &Value| match value {
                Value::Text(text) => text.clone(),
                Value::Int(number) => number.to_string(),
                Value::Bool(flag) => flag.to_string(),
                _ => "another value".to_owned(),
            };
            Some(NotInForce {
                key: held.key,
                failure: codes::policy(
                    codes::SETTING_NOT_IN_FORCE,
                    format!(
                        "The setting `{}` is set to `{}` by {}, and what is in force is `{}`: {}. The engine does not work under a limit it would report and not keep.",
                        held.key,
                        shown(&entry.value),
                        entry.origin,
                        shown(&held.in_force),
                        held.why
                    ),
                    format!(
                        "Take `{}` out of {}, or set it to `{}`, then start the engine again.",
                        held.key,
                        entry.origin,
                        shown(&held.in_force)
                    ),
                ),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use a2m_core::{PrincipalId, Value, WorkspaceId};
    use a2m_policy::BuiltIn;

    use super::{held, not_in_force};
    use crate::codes;
    use crate::settings::{PRESETS, Settings, SourceFile, Sources, find};

    fn local() -> BuiltIn {
        BuiltIn::local(
            PrincipalId::from_bytes([1; 16]),
            WorkspaceId::from_bytes([2; 16]),
        )
    }

    fn user(text: &str) -> Settings {
        Settings::load(&Sources {
            user: Some(SourceFile::new("settings.toml", text)),
            ..Sources::none()
        })
        .unwrap()
    }

    #[test]
    fn every_default_is_the_value_the_policy_engine_has_in_force() {
        // The schema's default of each held setting is what the built-in
        // sets work by, on one machine and on a server: the defaults are
        // true, and nothing is refused for them.
        for built_in in [local(), BuiltIn::server()] {
            for held in held(&built_in) {
                let setting = find(held.key).unwrap();
                assert_eq!(setting.default.value(), held.in_force, "{}", held.key);
                assert!(setting.kind.fits(&held.in_force), "{}", held.key);
            }
            assert!(not_in_force(&Settings::defaults(), &built_in).is_empty());
        }
    }

    #[test]
    fn a_protected_setting_the_policy_engine_does_not_work_by_is_refused_by_name() {
        for (text, key) in [
            (
                "[policy]\nmax_records_per_run = 5\n",
                "policy.max_records_per_run",
            ),
            (
                "[policy]\napproval_window_minutes = 5\n",
                "policy.approval_window_minutes",
            ),
            (
                "[policy]\napprove_writes = \"named_approver\"\n",
                "policy.approve_writes",
            ),
            (
                "[policy]\nagent_effect_ceiling = \"read\"\n",
                "policy.agent_effect_ceiling",
            ),
            (
                "[policy]\nwrites_outside_tests = false\n",
                "policy.writes_outside_tests",
            ),
            (
                "[policy]\ntwo_person_irreversible = true\n",
                "policy.two_person_irreversible",
            ),
        ] {
            let refused = not_in_force(&user(text), &local());
            assert_eq!(refused.len(), 1, "{key}");
            assert_eq!(refused[0].key, key);
            let failure = &refused[0].failure;
            assert_eq!(failure.code(), &codes::SETTING_NOT_IN_FORCE);
            assert!(failure.message().contains(key), "{failure:?}");
            assert!(failure.message().contains("settings.toml"), "{failure:?}");
        }
        // The two settings that are carried are never refused.
        let carried = user("[policy]\npre_approval = false\npreview_freshness_seconds = 5\n");
        assert!(not_in_force(&carried, &local()).is_empty());
        assert_eq!(
            carried.get("policy.pre_approval"),
            Some(&Value::Bool(false))
        );
    }

    #[test]
    fn a_preset_that_sets_what_is_not_in_force_is_refused_by_the_setting_it_sets() {
        let chosen = |name: &str| {
            Settings::load(&Sources {
                user: Some(SourceFile::new(
                    "settings.toml",
                    format!("[profile]\npreset = \"{name}\"\n"),
                )),
                ..Sources::none()
            })
            .unwrap()
        };
        let keys = |name: &str| -> Vec<&'static str> {
            not_in_force(&chosen(name), &local())
                .into_iter()
                .map(|refused| refused.key)
                .collect()
        };
        assert!(keys("standard").is_empty());
        assert_eq!(
            keys("cautious"),
            ["policy.max_records_per_run", "policy.agent_effect_ceiling"]
        );
        assert_eq!(keys("ci"), ["policy.writes_outside_tests"]);
        assert_eq!(PRESETS.len(), 3);
    }
}
