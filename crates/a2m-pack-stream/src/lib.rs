//! The stream pack of Anything2MCP: the `stream` mode over queues, topics,
//! and logs (spec section 309).
//!
//! It registers one mode, `stream`, of the stream archetype; the surface
//! kind `broker`; two file kinds, `channels/` and `schemas/`; the lowering
//! of the verbs `read`, `list`, `await`, `publish`, and `take`; a driver
//! that ends every read within its limit and publishes with the record's
//! token; a discoverer that only looks; and the drafting rules.
//!
//! A wire protocol is one small interface inside the pack (decision 0104).
//! Two fill it: `nats`, the NATS client protocol with JetStream, the first
//! standard one (decision 0167); and `example-broker/1`, the line protocol
//! of the test broker under `targets/example-broker`. A surface of kind
//! `broker` says which it speaks.
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

use a2m_core::{Failure, Value};
use a2m_files::{JsonSchema, SurfaceKindSpec};
use a2m_provider::{CompilerRegistrations, Pack, PackContext, Registrar, RegistryBuilder};

use crate::names::{BROKER, Names};

/// The pack's manifest, as the registry reads it.
pub const MANIFEST: &str = include_str!("manifest.toml");

/// The wire protocols this build speaks, by the name a surface of kind
/// `broker` states under `protocol`.
pub const PROTOCOLS: &[&str] = &[nats_wire::NAME, example_wire::NAME];

/// The stream pack.
#[derive(Debug, Clone, Copy, Default)]
pub struct StreamPack;

/// Hands the stream pack to the registry builder. This is the one call the
/// product crate makes for the pack.
pub fn register(builder: &mut RegistryBuilder) {
    builder.add_pack(&StreamPack);
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

        to.driver(
            names.mode.clone(),
            Arc::new(driver::StreamDriver::new(
                Arc::clone(cx.redact()),
                vec![
                    Arc::new(nats_wire::NatsProtocol),
                    Arc::new(example_wire::ExampleProtocol),
                ],
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
                        .required("protocol", JsonSchema::words(PROTOCOLS)),
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
                cx.telemetry().clock(),
            )),
            Arc::new(discover::BrokerDescriber::new(names)),
        )?;
        Ok(())
    }
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
