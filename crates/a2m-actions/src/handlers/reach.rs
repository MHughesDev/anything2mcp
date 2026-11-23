//! The reach: a network the person names, kept as a protected setting
//! (decisions 0223, 0318, 0327, 0399).
//!
//! The reaches are a list of [`Reach`] kept as one fact of the installation's
//! store. Only the person's acts below write it, each after the operating
//! system's presence check, and no input of an agent carries a network, a
//! destination, or a gateway: the network is read from the machine at the
//! moment the person names it, and the destinations and gateway names are the
//! person's own words. What a listing shows is the person's own entries and
//! the names of the facts a fingerprint states, never a digest.

use a2m_core::Failure;
use a2m_netstate::{FingerprintFact, FingerprintReading, NetworkFingerprint, Recognition};
use a2m_provider::{GatewayName, Reach, ReachEntry};
use serde_json::{Value as Json, json};

use super::handler;
use crate::codes;
use crate::dispatch::{Call, Output};
use crate::kernel::{Kernel, fact, set_fact};

/// The name of the store fact that holds the reaches.
const FACT: &str = "net.reaches";

/// The most reaches a person may name.
const MOST: usize = 32;

/// The most entries a reach may hold on either list.
const MOST_ENTRIES: usize = 64;

/// The longest label.
const LABEL_LONGEST: usize = 64;

fn unreadable() -> Failure {
    codes::busy(
        codes::ENGINE_DEGRADED,
        "The reaches this installation keeps cannot be read, so none is in force.",
        "Call `doctor.run` for the fix.",
    )
}

/// The reaches in the store.
fn load(tx: &mut a2m_ledger::Tx<'_>) -> Result<Vec<Reach>, Failure> {
    match fact(tx, FACT)? {
        None => Ok(Vec::new()),
        Some(text) => serde_json::from_str(&text).map_err(|_| unreadable()),
    }
}

fn save(tx: &mut a2m_ledger::Tx<'_>, reaches: &[Reach]) -> Result<(), Failure> {
    let text = serde_json::to_string(reaches).map_err(|_| unreadable())?;
    set_fact(tx, FACT, &text)
}

/// The fingerprint of the network the machine is on, read on a thread that
/// may wait.
async fn read_fingerprint() -> Result<FingerprintReading, Failure> {
    tokio::task::spawn_blocking(a2m_netstate::fingerprint)
        .await
        .map_err(|_| {
            codes::busy(
                codes::ENGINE_DEGRADED,
                "The reading of the machine's network was interrupted.",
                "Try again.",
            )
        })?
        .map_err(Failure::from)
}

/// Whether a fingerprint can name a network: it states every core fact,
/// which is what recognizing it again needs.
fn can_name_a_network(fingerprint: &NetworkFingerprint) -> bool {
    !fingerprint.is_empty() && fingerprint.recognize(fingerprint) == Recognition::Same
}

fn stated_names(fingerprint: &NetworkFingerprint) -> Vec<&'static str> {
    fingerprint
        .stated()
        .into_iter()
        .map(FingerprintFact::as_str)
        .collect()
}

impl Kernel {
    /// The reaches the person has named and the fingerprint of the network
    /// the machine is on now: what [`a2m_provider::NetworkPlace::of_in_reach`]
    /// needs to derive `via_gateway`. A fingerprint that cannot be read is
    /// the empty one, which no reach is in force on.
    ///
    /// # Errors
    ///
    /// `busy`/`engine_degraded` when the store is not open or the reaches
    /// cannot be read.
    pub async fn reaches_and_network(&self) -> Result<(Vec<Reach>, NetworkFingerprint), Failure> {
        let reaches = self.store()?.read(load).await?;
        if reaches.is_empty() {
            return Ok((reaches, NetworkFingerprint::empty()));
        }
        let current = read_fingerprint()
            .await
            .map_or_else(|_| NetworkFingerprint::empty(), |read| read.fingerprint);
        Ok((reaches, current))
    }
}

fn bad_entry(why: &str) -> Failure {
    codes::input(
        codes::SCHEMA_INVALID,
        format!("An entry of a reach is not taken: {why}."),
        "Write each entry as `destination`, or as `destination via gateway` with a name you choose for the gateway.",
    )
}

/// One entry as the person wrote it: `destination`, or
/// `destination via gateway`.
fn entry_of(text: &str) -> Result<ReachEntry, Failure> {
    let words: Vec<&str> = text.split_whitespace().collect();
    match words.as_slice() {
        [destination] => Ok(ReachEntry::direct(destination)),
        [destination, "via", gateway] => GatewayName::new(gateway)
            .map(|name| ReachEntry::through(destination, name))
            .map_err(|_| {
                bad_entry(
                    "the gateway's name is not a short name of letters, digits, and underscores",
                )
            }),
        _ => Err(bad_entry("it is not in one of those forms")),
    }
}

fn entries_of(call: &Call, field: &str) -> Result<Vec<ReachEntry>, Failure> {
    let Some(items) = call.input.get(field).and_then(Json::as_array) else {
        return Ok(Vec::new());
    };
    if items.len() > MOST_ENTRIES {
        return Err(codes::input(
            codes::SCHEMA_INVALID,
            format!("A reach holds at most {MOST_ENTRIES} entries on `{field}`."),
            "Name fewer destinations, or name a second reach.",
        ));
    }
    items
        .iter()
        .map(|item| {
            item.as_str()
                .ok_or_else(|| bad_entry("it is not text"))
                .and_then(entry_of)
        })
        .collect()
}

fn entry_json(entry: &ReachEntry) -> Json {
    let mut out = json!({"destination": entry.destination});
    if let (Json::Object(map), Some(gateway)) = (&mut out, &entry.gateway) {
        map.insert("gateway".to_owned(), json!(gateway.as_str()));
    }
    out
}

fn reach_json(reach: &Reach, current: &NetworkFingerprint) -> Json {
    json!({
        "label": reach.label,
        "stated": stated_names(&reach.network),
        "in_force": reach.is_in_force(current),
        "look": reach.look.iter().map(entry_json).collect::<Vec<_>>(),
        "send": reach.send.iter().map(entry_json).collect::<Vec<_>>(),
    })
}

/// A reach is a person's act: an agent never names or withdraws one, whatever
/// the dispatcher let through.
fn person_only(call: &Call, action: &str) -> Result<(), Failure> {
    if call.caller.is_agent {
        return Err(codes::policy(
            codes::PERSON_REQUIRED,
            format!("`{action}` is a person's act, and an agent cannot do it."),
            "Ask the person to run it themselves, at a terminal.",
        ));
    }
    Ok(())
}

handler!(
    /// `reach.name`: names the network the machine is on now, with the
    /// destinations the person says may be looked at and sent to.
    reach_name,
    |kernel, call| {
        person_only(&call, "reach.name")?;
        let label = call.required("label")?.trim().to_owned();
        if label.is_empty()
            || label.chars().count() > LABEL_LONGEST
            || label.chars().any(char::is_control)
        {
            return Err(codes::input(
                codes::SCHEMA_INVALID,
                format!("A label is one to {LABEL_LONGEST} printable characters."),
                "Choose a label such as `office`.",
            ));
        }
        let look = entries_of(&call, "look")?;
        let send = entries_of(&call, "send")?;
        let reading = read_fingerprint().await?;
        if !can_name_a_network(&reading.fingerprint) {
            let absent = serde_json::to_value(&reading.absent).unwrap_or(Json::Null);
            return Err(codes::input(
                codes::NETWORK_NOT_RECOGNIZABLE,
                format!(
                    "The network the machine is on states too few facts to be recognized again: it states {:?}, and the gateway's hardware address, the gateway's address, and the subnet are all needed. Left out: {absent}.",
                    stated_names(&reading.fingerprint)
                ),
                "Connect to a network with a default route and a gateway that answers, then name it again.",
            ));
        }
        let reach = Reach {
            label: label.clone(),
            network: reading.fingerprint.clone(),
            look,
            send,
        };
        let shown = reach_json(&reach, &reading.fingerprint);
        let named = label.clone();
        kernel
            .store()?
            .write(a2m_ledger::Durability::Synced, move |tx| {
                let mut reaches = load(tx)?;
                if reaches
                    .iter()
                    .any(|held| held.label.eq_ignore_ascii_case(&named))
                {
                    return Err(codes::input(
                        codes::SCHEMA_INVALID,
                        format!("A reach named `{named}` is already kept."),
                        "Withdraw it with `reach.withdraw`, or choose another label.",
                    ));
                }
                if reaches.len() >= MOST {
                    return Err(codes::input(
                        codes::SCHEMA_INVALID,
                        format!("{MOST} reaches are already kept."),
                        "Withdraw one with `reach.withdraw`.",
                    ));
                }
                reaches.push(reach);
                save(tx, &reaches)
            })
            .await?;
        kernel
            .audit(
                &call.caller.principal.principal.id.to_string(),
                "reach.name",
                &label,
                "named",
            )
            .await?;
        Ok(Output::ok(shown))
    }
);

handler!(
    /// `reach.list`: the reaches, each with the names of the facts it
    /// states and whether it is in force on the network the machine is on.
    reach_list,
    |kernel, call| {
        let _ = &call;
        let (reaches, current) = kernel.reaches_and_network().await?;
        Ok(Output::ok(json!({
            "reaches": reaches.iter().map(|reach| reach_json(reach, &current)).collect::<Vec<_>>(),
        })))
    }
);

handler!(
    /// `reach.withdraw`: forgets one reach by its label.
    reach_withdraw,
    |kernel, call| {
        person_only(&call, "reach.withdraw")?;
        let label = call.required("label")?.to_owned();
        let named = label.clone();
        kernel
            .store()?
            .write(a2m_ledger::Durability::Synced, move |tx| {
                let mut reaches = load(tx)?;
                let before = reaches.len();
                reaches.retain(|held| !held.label.eq_ignore_ascii_case(&named));
                if reaches.len() == before {
                    return Err(codes::input(
                        codes::NOT_FOUND,
                        format!("No reach is named `{named}`."),
                        "Call `reach.list` to see the labels.",
                    ));
                }
                save(tx, &reaches)
            })
            .await?;
        kernel
            .audit(
                &call.caller.principal.principal.id.to_string(),
                "reach.withdraw",
                &label,
                "withdrawn",
            )
            .await?;
        Ok(Output::ok(json!({"label": label, "withdrawn": true})))
    }
);

handler!(
    /// `net.current`: which facts of the network the machine is on were
    /// read, and which were left out and why. It shows no digest.
    net_current,
    |_kernel, _call| {
        let reading = read_fingerprint().await?;
        let usable = can_name_a_network(&reading.fingerprint);
        Ok(Output::ok(json!({
            "stated": stated_names(&reading.fingerprint),
            "left_out": serde_json::to_value(&reading.absent).unwrap_or(Json::Null),
            "interface": reading.interface,
            "can_be_named": usable,
        })))
    }
);
