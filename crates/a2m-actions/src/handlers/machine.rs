//! The machine itself: the engine's status, its settings, the doctor, and
//! what the machine already knows of its own networks.

use a2m_core::DoctorState;
use a2m_ledger::ChainCheck;
use a2m_vault::StoreHealth as KeyStoreHealth;
use serde_json::{Value as Json, json};

use super::handler;
use crate::def::BuiltInToolset;
use crate::dispatch::{Next, Output};
use crate::health::HealthState;

handler!(
    /// `engine.status`: the health state and its cause. Answers in every
    /// state.
    engine_status,
    |kernel, _call| {
        let health = kernel.recheck().await;
        let mut out = health.to_json();
        if let Json::Object(map) = &mut out {
            map.insert("runner".to_owned(), kernel.runner_status());
        }
        let output = Output::ok(out);
        Ok(if health.state == HealthState::Ok {
            output
        } else {
            output.then(Next::new("doctor.run", json!({}), "for the fix"))
        })
    }
);

handler!(
    /// `config.show`: every effective setting and the layer that set it.
    config_show,
    |kernel, call| {
        let mut shown = kernel.settings().show(call.text("key"))?;
        let mut output_warnings = Vec::new();
        // A protected setting whose value is not the one the policy engine
        // works by is never shown as in force: it is marked, and the
        // reason is given. The kernel is stopped while there is one.
        if let Some(settings) = shown.get_mut("settings").and_then(Json::as_array_mut) {
            for refused in &kernel.inner.not_in_force {
                let found = settings
                    .iter_mut()
                    .find(|setting| setting["key"] == refused.key);
                if let Some(Json::Object(setting)) = found {
                    setting.insert("in_force".to_owned(), json!(false));
                    output_warnings.push(refused.failure.message().to_owned());
                }
            }
        }
        let mut output = Output::ok(shown);
        for warning in output_warnings {
            output = output.warn(warning);
        }
        Ok(output)
    }
);

fn check(
    name: &str,
    state: DoctorState,
    detail: impl Into<String>,
    fix: impl Into<String>,
) -> Json {
    json!({"check": name, "state": state.as_str(), "detail": detail.into(), "fix": fix.into()})
}

handler!(
    /// `doctor.run`: every part checked, with a fix for each problem. It
    /// makes no outside connection and starts nothing.
    doctor_run,
    |kernel, _call| {
        let health = kernel.recheck().await;
        let mut checks = Vec::new();

        // The settings, as start-up loaded them.
        let refused = kernel.settings().refused();
        checks.push(match refused.first() {
            None => check(
                "settings",
                DoctorState::Healthy,
                format!(
                    "The settings loaded; their digest is {}.",
                    kernel.settings().digest()
                ),
                "",
            ),
            Some(first) => check(
                "settings",
                DoctorState::RuntimeFailure,
                first.message(),
                first.next_step(),
            ),
        });

        // The store, and the audit chain inside it.
        match kernel.store() {
            Ok(store) => {
                let state = store.health().await;
                checks.push(if state.can_commit {
                    check("store", DoctorState::Healthy, "The store commits.", "")
                } else {
                    check(
                        "store",
                        DoctorState::RuntimeFailure,
                        "The store cannot commit.",
                        "Free disk space, or restore the database from a backup.",
                    )
                });
                if let Ok(installation) = kernel.installation() {
                    let chain = store
                        .read(move |tx| tx.audit().verify(&installation.workspace))
                        .await;
                    checks.push(match chain {
                        Ok(ChainCheck::Intact { .. }) => check(
                            "audit_chain",
                            DoctorState::Healthy,
                            "The audit chain is whole.",
                            "",
                        ),
                        Ok(_) => check(
                            "audit_chain",
                            DoctorState::RuntimeFailure,
                            "The audit chain is broken: an entry was changed or removed.",
                            "Restore the store from a backup and keep the broken one for review.",
                        ),
                        Err(failure) => check(
                            "audit_chain",
                            DoctorState::Unknown,
                            failure.message(),
                            failure.next_step(),
                        ),
                    });
                }
            }
            Err(failure) => {
                checks.push(check(
                    "store",
                    DoctorState::RuntimeFailure,
                    failure.message(),
                    failure.next_step(),
                ));
            }
        }

        // The engine's own folders: closed to every other account.
        let paths = kernel.paths().clone();
        let home = tokio::task::spawn_blocking(move || crate::paths::open_to_others(&paths))
            .await
            .unwrap_or_else(|_| crate::paths::OpenFolders {
                not_checked: true,
                ..crate::paths::OpenFolders::default()
            });
        let listed = |folders: &[std::path::PathBuf]| {
            folders
                .iter()
                .map(|folder| format!("`{}`", folder.display()))
                .collect::<Vec<_>>()
                .join(", ")
        };
        checks.push(if home.not_checked {
            check(
                "home",
                DoctorState::Unknown,
                "Who may enter the engine's own folders is not read on this operating system yet; the engine gives each an access list that names only this user when it starts.",
                "",
            )
        } else if home.closed() {
            check(
                "home",
                DoctorState::Healthy,
                "The engine's own folders are this user's alone.",
                "",
            )
        } else if !home.not_ours.is_empty() {
            // The engine closes only a folder of its own user's: these it
            // could not close when it started (decision 0273).
            check(
                "home",
                DoctorState::ConfigDrift,
                format!(
                    "These of the engine's own folders are not a folder of this user's, so the engine could not close them to other accounts: {}. Another account owns each, or a link or a file stands in its place.{}",
                    listed(&home.not_ours),
                    if home.open.is_empty() {
                        String::new()
                    } else {
                        format!(" Other accounts may also enter {}.", listed(&home.open))
                    }
                ),
                "Put a folder of this user's own in the place of each, not a link, closed to every other account, then start the engine again.",
            )
        } else {
            check(
                "home",
                DoctorState::ConfigDrift,
                format!(
                    "Other accounts may enter the engine's own folders: {}. What they hold is sealed, and is still not to be left where others can list and copy it.",
                    listed(&home.open)
                ),
                format!(
                    "Close each to every account but this user's ({}), or start the engine, which closes the folders it uses.",
                    if cfg!(unix) { "`chmod 700` on each" } else { "remove every other account from its access list" }
                ),
            )
        });

        // The key store.
        checks.push(match &kernel.inner.vault {
            Some(vault) => {
                let state = vault.health().await;
                match state.store_health {
                    KeyStoreHealth::Unreachable => check(
                        "key_store",
                        DoctorState::RuntimeFailure,
                        format!("The key store `{}` does not answer.", state.store.name),
                        "Unlock or start the key store.",
                    ),
                    _ => check(
                        "key_store",
                        DoctorState::Healthy,
                        format!("The key store is `{}`.", state.store.name),
                        "",
                    ),
                }
            }
            None => check(
                "key_store",
                DoctorState::RuntimeFailure,
                "The vault is not open, so no session can be saved and nothing can be sealed.",
                "Make the operating system's key store reachable, or choose the passphrase route yourself.",
            ),
        });

        // Policy, and the presence check this machine offers.
        checks.push(
            if kernel.policy().is_some()
                && health
                    .reasons
                    .iter()
                    .all(|reason| reason.part != crate::health::Part::Policy)
            {
                check(
                    "policy",
                    DoctorState::Healthy,
                    "A valid policy set is in force.",
                    "",
                )
            } else if let Some(refused) = kernel.inner.not_in_force.first() {
                // A protected setting says what the policy engine does not
                // work by: the engine serves nothing until it is taken out.
                check(
                    "policy",
                    DoctorState::ConfigDrift,
                    refused.failure.message(),
                    refused.failure.next_step(),
                )
            } else {
                check(
                    "policy",
                    DoctorState::RuntimeFailure,
                    "No valid policy set is in force, so every decision is a denial.",
                    "Publish a policy set whose tests pass.",
                )
            },
        );
        // Whether a person can be proved present here, which an approval,
        // a publish, and the release of a kill switch need. The surface is
        // asked what its prompt can do; no prompt is raised.
        let presence = kernel.person().presence().await;
        checks.push(check(
            "presence",
            if presence.available() {
                DoctorState::Healthy
            } else if presence.reason == "unknown" {
                DoctorState::Unknown
            } else {
                DoctorState::ComponentMissing
            },
            format!(
                "{} (prompt: {}, {})",
                presence.message, presence.kind, presence.reason
            ),
            presence.fix,
        ));

        // The providers.
        for provider in kernel.providers().listing() {
            let state = if provider.availability.is_ready() && provider.problems.is_empty() {
                DoctorState::Healthy
            } else {
                DoctorState::RuntimeFailure
            };
            checks.push(check(
                &format!("provider.{}", provider.name),
                state,
                format!(
                    "{} {}: {}.",
                    provider.name, provider.version, provider.availability
                ),
                provider.fix,
            ));
        }

        // The toolsets against their caps.
        for toolset in [BuiltInToolset::Use, BuiltInToolset::Build] {
            let past = kernel.actions().past_cap(toolset);
            if !past.is_empty() {
                checks.push(check(
                    &format!("toolset.{}", toolset.as_str()),
                    DoctorState::Healthy,
                    format!(
                        "{} actions marked for the `{}` toolset are past its cap of {} and are reached through `action.run`.",
                        past.len(),
                        toolset.as_str(),
                        toolset.cap()
                    ),
                    "",
                ));
            }
        }

        // The clock: a time of day before the engine's own build cannot be
        // right, and approvals are judged by it.
        let now = kernel.inner.clock.now().as_unix_millis();
        checks.push(if now > 0 {
            check(
                "clock",
                DoctorState::Healthy,
                "The clock gives a time of day.",
                "",
            )
        } else {
            check(
                "clock",
                DoctorState::RuntimeFailure,
                "The machine's clock is set before 1970.",
                "Set the machine's clock.",
            )
        });

        for reason in &health.reasons {
            checks.push(check(
                &format!("health.{}", reason.part.as_str()),
                DoctorState::RuntimeFailure,
                reason.failure.message(),
                reason.failure.next_step(),
            ));
        }
        Ok(Output::ok(
            json!({"state": health.state.as_str(), "checks": checks}),
        ))
    }
);

/// Reads one of the machine's network facts on a thread that may wait (each
/// reader blocks on system calls and file reads), and shows it as the
/// reader serializes it: a hardware address as its length, a proxy value
/// with its credentials removed (decision 0243). Nothing is sent to any
/// other machine.
async fn net_read<T: serde::Serialize + Send + 'static>(
    read: fn() -> Result<T, a2m_netstate::NetError>,
) -> Result<Json, a2m_core::Failure> {
    let reading = tokio::task::spawn_blocking(read)
        .await
        .map_err(|_| {
            crate::codes::busy(
                crate::codes::ENGINE_DEGRADED,
                "The reading of the machine's network was interrupted.",
                "Try again.",
            )
        })?
        .map_err(a2m_core::Failure::from)?;
    serde_json::to_value(&reading).map_err(|_| {
        crate::codes::input(
            crate::codes::SCHEMA_INVALID,
            "The reading of the machine's network could not be written as JSON.",
            "Report this; it is a fault in the engine.",
        )
    })
}

handler!(
    /// `net.interfaces`: the interfaces and their addresses.
    net_interfaces,
    |_kernel, _call| { Ok(Output::ok(net_read(a2m_netstate::interfaces).await?)) }
);

handler!(
    /// `net.routes`: the routes and each family's default routes.
    net_routes,
    |_kernel, _call| { Ok(Output::ok(net_read(a2m_netstate::routes).await?)) }
);

handler!(
    /// `net.resolvers`: the name servers and search domains.
    net_resolvers,
    |_kernel, _call| { Ok(Output::ok(net_read(a2m_netstate::resolvers).await?)) }
);

handler!(
    /// `net.neighbours`: the neighbour table the system already holds.
    net_neighbours,
    |_kernel, _call| { Ok(Output::ok(net_read(a2m_netstate::neighbours).await?)) }
);

handler!(
    /// `net.listeners`: the listening sockets and their owners.
    net_listeners,
    |_kernel, _call| { Ok(Output::ok(net_read(a2m_netstate::listeners).await?)) }
);

handler!(
    /// `net.proxy`: the proxy setting this engine reads, without
    /// credentials. Read to be shown, never applied.
    net_proxy,
    |_kernel, _call| { Ok(Output::ok(net_read(a2m_netstate::proxy).await?)) }
);

handler!(
    /// `net.standing`: where the machine stands. The one network action an
    /// agent sees listed; it names the fuller reading of the interfaces.
    net_standing,
    |_kernel, _call| {
        Ok(Output::ok(net_read(a2m_netstate::standing).await?).then(Next::new(
            "action.run",
            json!({"action": "net.interfaces"}),
            "for every interface in full",
        )))
    }
);
