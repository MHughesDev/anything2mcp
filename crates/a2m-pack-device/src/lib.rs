//! The device pack for Anything2MCP: the `device` mode over a device's
//! link.
//!
//! The pack brings one surface kind, `device_link`; one file kind,
//! `registers/`, a map of every address it may touch; the `device` mode,
//! with the verbs `read`, `update`, and `submit`; a driver that sends one
//! frame at a time and checks every write against its register's range
//! before a frame is built; and a discoverer that asks a device to identify
//! itself and nothing else.
//!
//! The first protocol is a subset of the Modbus application protocol over
//! TCP, written here. Not built: serial links, a console on a link, writes
//! with the hazard `physical` (`confirm_by`, the safe-state operation),
//! one frame that writes several registers, reads of a register whose
//! reading changes it, firmware inspection, and the firmware update
//! operation.
//!
//! The product crate calls [`add_to`], and nothing else here.
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: layers 1 to 5.

mod codec;
pub mod codes;
mod discover;
mod driver;
mod link;
mod lower;
mod map;
mod modbus;
mod names;

use std::sync::Arc;
use std::time::Duration;

use a2m_core::{Failure, Value};
use a2m_files::{JsonSchema, SurfaceKindSpec};
use a2m_provider::{CompilerRegistrations, Pack, PackContext, Registrar, RegistryBuilder};

pub use names::PACK;

/// The pack's manifest, as the registry reads it.
pub const MANIFEST: &str = include_str!("../anything2mcp-provider.toml");

/// The device pack.
#[derive(Debug, Clone, Copy, Default)]
pub struct DevicePack;

/// Hands the device pack to the registry builder. The one call the product
/// crate makes for this pack.
pub fn add_to(builder: &mut RegistryBuilder) {
    builder.add_pack(&DevicePack);
}

fn setting_ms(cx: &PackContext, key: &str, default: u64) -> Duration {
    let millis = cx
        .settings()
        .get(key)
        .and_then(Value::as_int)
        .and_then(|millis| u64::try_from(millis).ok())
        .unwrap_or(default);
    Duration::from_millis(millis)
}

impl Pack for DevicePack {
    fn manifest(&self) -> &'static str {
        MANIFEST
    }

    fn register(&self, cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        let names = Arc::new(names::Names::new()?);
        let defaults = driver::PaceDefaults {
            reply_within: setting_ms(cx, "reply_within_ms", 1000),
            min_gap: setting_ms(cx, "min_gap_ms", 50),
        };
        to.driver(
            names.mode.clone(),
            Arc::new(driver::DeviceDriver::new(
                Arc::clone(&names),
                Arc::clone(cx.redact()),
                defaults,
            )),
        )?;
        let mut compiler = CompilerRegistrations::new()
            .surface_kind(SurfaceKindSpec::new(
                names.surface_kind.clone(),
                "address",
                JsonSchema::object()
                    .required("address", JsonSchema::text())
                    .required("protocol", JsonSchema::words(link::PROTOCOLS))
                    .required("unit", JsonSchema::int_between(0, 255)),
            ))
            .file_kind(Arc::new(map::RegisterKind::new(Arc::clone(&names))))
            .lowering(Arc::new(lower::DeviceLowering::new(Arc::clone(&names))))
            .drafting_rule(Arc::new(discover::MapFromIdentity::new(Arc::clone(&names))));
        for rule in [
            map::WritableRule::Hazard,
            map::WritableRule::Range,
            map::WritableRule::CommandHazard,
        ] {
            compiler = compiler.lint_rule(Arc::new(map::WritableRegisterRule::new(
                Arc::clone(&names),
                rule,
            )?));
        }
        to.compiler(compiler)?;
        let version = a2m_provider::Manifest::parse(MANIFEST)?
            .provider
            .version
            .to_string();
        to.discoverer(
            Arc::new(discover::IdentityDiscoverer::new(
                Arc::clone(&names),
                cx.telemetry().clock(),
                version,
            )?),
            Arc::new(discover::IdentityDescriber),
        )?;
        Ok(())
    }
}
