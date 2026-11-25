//! The stream pack of Anything2MCP: the `stream` mode over queues, topics,
//! and logs (spec section 309).
//!
//! It registers one mode, `stream`, of the stream access kind; the surface
//! kind `broker`; two file kinds, `channels/` and `schemas/`; the lowering
//! of the verbs `read`, `list`, `await`, `publish`, and `take`; a driver
//! that ends every read within its limit and publishes with the record's
//! token; a discoverer that only looks; and the drafting rules.
//!
//! A wire protocol is one small interface inside the pack (decision 0104).
//! Two fill it: `nats`, the NATS client protocol with JetStream, the first
//! standard one (decision 0167); and `example-broker/1`, the line protocol
//! of the test broker under `targets/example-broker`. A surface of kind
//! `broker` says which it speaks. The second exists only to speak to that
//! stand, so the registration the product makes, [`register`], never offers
//! it: only a test's own registration does, [`register_with_test_broker`],
//! or a process in which the stand's harness has started a broker
//! (decision 0201, which replaces the feature of decision 0197). Whatever
//! a build unifies, the shipped program offers `nats` alone.
//!
//! The product crate makes one call: [`register`].

mod channel;
pub mod codes;
mod discover;
mod draft;
mod driver;
mod example_wire;
mod json;
mod lint;
mod lower;
mod names;
mod nats_wire;
mod wire;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use a2m_core::{Failure, Value};
use a2m_files::{JsonSchema, SurfaceKindSpec};
use a2m_provider::{CompilerRegistrations, Pack, PackContext, Registrar, RegistryBuilder};

use crate::names::{BROKER, Names};

/// The pack's manifest, as the registry reads it.
pub const MANIFEST: &str = include_str!("manifest.toml");

/// The wire protocols the product's registration offers, by the name a
/// surface of kind `broker` states under `protocol`. A surface naming
/// another does not compile. The test broker's protocol is not among them.
pub const PROTOCOLS: &[&str] = &[nats_wire::NAME];

/// The test broker's protocol, offered only by a test's own registration.
pub const TEST_BROKER_PROTOCOL: &str = example_wire::NAME;

/// What a registration that offers the test broker's protocol offers.
const WITH_TEST_BROKER: &[&str] = &[nats_wire::NAME, example_wire::NAME];

/// The stream pack. The value the product registers offers [`PROTOCOLS`];
/// [`StreamPack::with_test_broker`] offers the test broker's protocol too.
#[derive(Debug, Clone, Copy, Default)]
pub struct StreamPack {
    test_broker: bool,
}

impl StreamPack {
    /// The pack as a test registers it to speak to the test broker under
    /// `targets/example-broker`: the product never does.
    #[must_use]
    pub const fn with_test_broker() -> Self {
        Self { test_broker: true }
    }

    /// The wire protocols this registration offers.
    #[must_use]
    pub fn protocols(&self) -> &'static [&'static str] {
        if self.test_broker {
            WITH_TEST_BROKER
        } else {
            PROTOCOLS
        }
    }
}

/// Set once the stand's harness has started a broker in this process.
static TEST_BROKER_STARTED: AtomicBool = AtomicBool::new(false);

/// Test equipment: from now on, in this process, [`register`] offers the
/// test broker's protocol as [`register_with_test_broker`] does. Called by
/// the harness of `targets/example-broker` when it starts a broker, so that
/// a test that builds the product's own registry in its process (the
/// product's end-to-end tests) reaches the broker it started. The shipped
/// program never links the stand, so nothing in it calls this, and a rule
/// of this crate's tests holds that no other source names it. Nothing a
/// caller sends reaches it.
#[doc(hidden)]
pub fn offer_test_broker_protocol_in_this_process() {
    TEST_BROKER_STARTED.store(true, Ordering::SeqCst);
}

/// Hands the stream pack to the registry builder. This is the one call the
/// product crate makes for the pack. It offers [`PROTOCOLS`], and never the
/// test broker's protocol unless a broker of the stand was started in this
/// process.
pub fn register(builder: &mut RegistryBuilder) {
    if TEST_BROKER_STARTED.load(Ordering::SeqCst) {
        register_with_test_broker(builder);
    } else {
        builder.add_pack(&StreamPack::default());
    }
}

/// Hands the stream pack to the registry builder with the test broker's
/// protocol offered too: a test's own registration. The product never
/// calls it.
pub fn register_with_test_broker(builder: &mut RegistryBuilder) {
    builder.add_pack(&StreamPack::with_test_broker());
}

/// A whole-number setting of the pack's own group. The registry has
/// already checked the group against the manifest and filled the defaults.
fn setting(cx: &PackContext, key: &str, otherwise: u32) -> u32 {
    cx.settings()
        .get(key)
        .and_then(Value::as_int)
        .and_then(|number| u32::try_from(number).ok())
        .unwrap_or(otherwise)
}

impl Pack for StreamPack {
    fn manifest(&self) -> &'static str {
        MANIFEST
    }

    fn register(&self, cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        let names = Names::new()?;
        let ceiling_ms = setting(cx, "wait_ceiling_seconds", 60).saturating_mul(1_000);
        let sample = setting(cx, "sample_count", 5);
        let idle_links = usize::try_from(setting(cx, "idle_links", 2)).unwrap_or(2);
        let ceiling = usize::try_from(setting(
            cx,
            "message_ceiling_bytes",
            u32::try_from(nats_wire::DEFAULT_CEILING).unwrap_or(u32::MAX),
        ))
        .unwrap_or(nats_wire::DEFAULT_CEILING);

        to.driver(
            names.mode.clone(),
            Arc::new(driver::StreamDriver::new(
                Arc::clone(cx.redact()),
                protocols(ceiling, self.test_broker),
                idle_links,
            )),
        )?;
        to.compiler(
            CompilerRegistrations::new()
                .surface_kind(SurfaceKindSpec::new(
                    BROKER,
                    "address",
                    JsonSchema::object()
                        .required("address", JsonSchema::text())
                        .required("protocol", JsonSchema::words(self.protocols())),
                ))
                .file_kind(Arc::new(channel::ChannelKind::new(ceiling_ms)))
                .file_kind(Arc::new(channel::SchemaKind::new()))
                .lowering(Arc::new(lower::StreamLowering::new(
                    names.mode.clone(),
                    ceiling_ms,
                )))
                .lint_rule(Arc::new(lint::UnknownSchema))
                .lint_rule(Arc::new(lint::ConsumersUnknown))
                .lint_rule(Arc::new(lint::UntrustedUnmarked))
                .lint_rule(Arc::new(lint::ProtocolUnfit))
                .drafting_rule(Arc::new(draft::ChannelDraft::new(names.clone())))
                .drafting_rule(Arc::new(draft::SchemaDraft::new(names.clone())))
                .drafting_rule(Arc::new(draft::ReadDraft::new(names.clone()))),
        )?;
        let version = to_version(MANIFEST);
        to.discoverer(
            Arc::new(discover::BrokerLook::new(
                names.clone(),
                sample,
                version,
                cx.logs_and_traces().clock(),
            )),
            Arc::new(discover::BrokerDescriber::new(names)),
        )?;
        Ok(())
    }
}

/// The wire protocols the driver speaks: `nats`, and the test broker's
/// only for a registration that offers it.
fn protocols(ceiling: usize, test_broker: bool) -> Vec<Arc<dyn wire::Protocol>> {
    let mut protocols: Vec<Arc<dyn wire::Protocol>> =
        vec![Arc::new(nats_wire::NatsProtocol::new(ceiling))];
    if test_broker {
        protocols.push(Arc::new(example_wire::ExampleProtocol));
    }
    protocols
}

/// The pack's version, as its manifest states it: the text of the first
/// `version = "..."` line.
fn to_version(manifest: &str) -> String {
    manifest
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or("0.0.0")
        .to_owned()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test stops when a manifest cannot be read"
    )]

    use std::path::Path;

    /// Decision 0201: only the stand's harness opens the test broker's
    /// protocol to the ordinary registration, and the product names only
    /// the ordinary registration. No manifest turns a feature on for it:
    /// there is none to turn on.
    #[test]
    fn only_the_test_broker_offers_its_protocol_to_the_ordinary_registration() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap();
        let mut naming = Vec::new();
        let mut folders = vec![
            root.join("crates"),
            root.join("targets"),
            root.join("bench"),
        ];
        while let Some(folder) = folders.pop() {
            let Ok(entries) = std::fs::read_dir(&folder) else {
                continue;
            };
            for entry in entries {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    if path.file_name().is_some_and(|name| name != "target") {
                        folders.push(path);
                    }
                    continue;
                }
                let ours = path.starts_with(root.join("crates").join("a2m-pack-stream"));
                let rust = path.extension().is_some_and(|extension| extension == "rs");
                let cargo = path.file_name().is_some_and(|name| name == "Cargo.toml");
                if !rust && !cargo {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                if rust && !ours && text.contains("offer_test_broker_protocol_in_this_process") {
                    naming.push(path.strip_prefix(root).unwrap().to_path_buf());
                }
                if cargo && text.contains("example-protocol") {
                    naming.push(path.strip_prefix(root).unwrap().to_path_buf());
                }
            }
        }
        assert_eq!(
            naming,
            [Path::new("targets")
                .join("example-broker")
                .join("src")
                .join("harness.rs")]
        );
        // What the product calls offers the real protocol alone.
        assert_eq!(super::StreamPack::default().protocols(), ["nats"]);
        assert_eq!(super::PROTOCOLS, ["nats"]);
        assert_eq!(
            super::StreamPack::with_test_broker().protocols(),
            ["nats", super::TEST_BROKER_PROTOCOL]
        );
    }
}
