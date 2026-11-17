//! The HTTP pack: a target can be a service reached by HTTP requests.
//!
//! The pack owns the endpoint catalog, the `endpoints/` file kind, and
//! compiles each entry into a typed request template, its own payload in
//! the plan. Its driver, of the mode `http`, builds every request position
//! by position from a template, sends it to the surface's own address with
//! no browser, and checks the answer against the shape the endpoint
//! declares. Its discoverers read a published description and what a
//! surface says about itself, with requests that cannot write.
//!
//! The product crate registers the pack with one call:
//! [`add_to`], or `builder.add_pack(&HttpPack)`.
//!
//! Not built yet, and so not declared in the pack's manifest: the `api`
//! mode, the `token` step and the anti-forgery bootstrap, the edge's
//! answers, uploads and downloads, pagers, identity by a stamp or a search,
//! the digest and integrated Windows exchanges, GraphQL and OData. TLS is
//! not in this build: a surface reached over HTTPS is refused with the
//! failure `input`/`tls_not_enabled`.
//!
//! A pack crate sees the kernel up to the provider contract; the layering
//! table in `docs/reference/technical-reference.md` is the rule, and this
//! crate's design is `docs/architecture/crates/a2m-pack-http.md`.

pub mod codes;
mod discover;
mod driver;
mod endpoints;
mod json;
mod lower;
mod openapi;
mod shape;
mod template;

use std::sync::Arc;

use a2m_core::{Failure, Mode};
use a2m_files::{JsonSchema, SurfaceKindSpec};
use a2m_provider::{CompilerRegistrations, Pack, PackContext, Registrar, RegistryBuilder};

pub use discover::{DESCRIPTION_FILE, DESCRIPTION_PATH};
pub use endpoints::{ENDPOINT_KIND, HTTP_SURFACE, PACK, WEB_SURFACE};
pub use lower::{EXTRACT, PROBE, REQUEST, VALIDATE};

/// The name of the mode this pack drives.
pub const HTTP_MODE: &str = "http";

/// The key of a surface of the kind `http` that holds its address.
pub const ADDRESS_KEY: &str = "base";

/// The most bytes of one answer the driver reads, unless a setting or an
/// endpoint says otherwise.
const DEFAULT_MAX_RESPONSE_BYTES: u64 = 8_000_000;

/// The HTTP pack.
#[derive(Debug, Clone, Copy, Default)]
pub struct HttpPack;

/// Hands the HTTP pack to the registry builder. This is the one call the
/// product crate makes for the pack.
pub fn add_to(builder: &mut RegistryBuilder) {
    builder.add_pack(&HttpPack);
}

impl Pack for HttpPack {
    fn manifest(&self) -> &'static str {
        include_str!("anything2mcp-provider.toml")
    }

    fn register(&self, cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        let mode = Mode::new(HTTP_MODE)?;
        let max_response_bytes = cx
            .settings()
            .get("max_response_bytes")
            .and_then(a2m_core::Value::as_int)
            .and_then(|number| u64::try_from(number).ok())
            .unwrap_or(DEFAULT_MAX_RESPONSE_BYTES);
        to.driver(
            mode.clone(),
            Arc::new(driver::HttpDriver::new(
                Arc::clone(cx.redact()),
                max_response_bytes,
            )),
        )?;
        to.compiler(
            CompilerRegistrations::new()
                .surface_kind(SurfaceKindSpec::new(
                    HTTP_SURFACE,
                    ADDRESS_KEY,
                    JsonSchema::object().required(ADDRESS_KEY, JsonSchema::text()),
                ))
                .file_kind(Arc::new(endpoints::EndpointKind::new()))
                .lowering(Arc::new(lower::HttpLowering::new(mode)))
                .lint_rule(Arc::new(endpoints::SecretLiteral))
                .lint_rule(Arc::new(endpoints::NoResponseSchema))
                .lint_rule(Arc::new(endpoints::EffectMethod))
                .lint_rule(Arc::new(endpoints::SignedEndpoint))
                .drafting_rule(Arc::new(openapi::DescribedOperationRule {
                    kind: openapi::described_operation()?,
                })),
        )?;
        let clock = cx.telemetry().clock();
        to.discoverer(
            Arc::new(discover::HttpSurface::new(
                cx.provider().clone(),
                Arc::clone(&clock),
            )?),
            Arc::new(discover::HttpDescriber),
        )?;
        to.discoverer(
            Arc::new(discover::HttpDescription::new(
                cx.provider().clone(),
                clock,
            )?),
            Arc::new(discover::HttpDescriber),
        )?;
        Ok(())
    }
}
