//! The HTTP pack: a target can be a service reached by HTTP requests.
//!
//! The pack owns the endpoint catalog, the `endpoints/` file kind, and
//! compiles each entry into a typed request template, its own payload in the
//! compiled target. Its driver, of the mode `http`, builds every request
//! position by position from a template, sends it to the surface's own
//! address with no browser, and checks the answer against the shape the
//! endpoint declares. Its discoverers read a published description and what a
//! surface says about itself, with requests that cannot write.
//!
//! The product crate registers the pack with one call:
//! [`add_to`], or `builder.add_pack(&HttpPack)`.
//!
//! The `api` mode binds a read to an operation of a published description
//! the import pinned, with the description's digest, in `descriptions/`;
//! a description changed since is named drift (decision 0389).
//!
//! Not built yet, and so not declared in the pack's manifest: writes
//! through `api`, a token taken from another endpoint's answer, a challenge
//! page, uploads and downloads, pagers, identity by a stamp or a search,
//! the digest and integrated Windows exchanges, a client certificate,
//! GraphQL and OData.
//!
//! HTTPS is behind the feature `tls`. With it on, an `https` surface is
//! reached through a client made by `a2m_auth::tls`, and a certificate or a
//! handshake that fails is a named failure of `unknown_variant` with nothing
//! sent. With it off, an `https` surface is refused with the failure
//! `input`/`tls_not_enabled`.
//!
//! A pack crate sees the kernel up to the provider contract; the layering
//! table in `docs/reference/technical-reference.md` is the rule, and this
//! crate's design is `docs/architecture/crates/a2m-pack-http.md`.

pub mod codes;
mod described;
mod discover;
mod draft;
mod driver;
mod endpoints;
mod forgery;
mod forms;
mod json;
mod link;
mod lower;
mod openapi;
mod template;
mod xml;

use std::sync::Arc;

use a2m_core::{Failure, Mode};
use a2m_files::{JsonSchema, SurfaceKindSpec};
/// The closed shape of a body, which this pack reads in its endpoint
/// entries and checks against answers, is the provider contract's, shared
/// with the web pack's page requests (decision 0394, point 1).
use a2m_provider::shape;
use a2m_provider::{CompilerRegistrations, Pack, PackContext, Registrar, RegistryBuilder};

pub use described::{API_MODE, DESCRIPTION_KIND};
pub use discover::{DESCRIPTION_FILE, DESCRIPTION_PATH};
pub use endpoints::{ENDPOINT_KIND, HTTP_SURFACE, PACK, WEB_SURFACE};
pub use link::{ACCEPTED, UNENCRYPTED_CREDENTIALS};
pub use lower::{CALL, EXTRACT, PROBE, REQUEST, TOKEN, VALIDATE};

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
        let api = Mode::new(API_MODE)?;
        // One driver for both modes: an `api` call is sent as an `http`
        // request is, and the reads left running are counted once for the
        // pack (decision 0389).
        let driver = Arc::new(driver::HttpDriver::new(
            Arc::clone(cx.redact()),
            max_response_bytes,
        ));
        to.driver(
            mode.clone(),
            Arc::clone(&driver) as Arc<dyn a2m_provider::Driver>,
        )?;
        to.driver(api.clone(), driver)?;
        to.compiler(
            CompilerRegistrations::new()
                .surface_kind(
                    SurfaceKindSpec::new(
                        HTTP_SURFACE,
                        ADDRESS_KEY,
                        JsonSchema::object()
                            .required(ADDRESS_KEY, JsonSchema::text())
                            .optional(
                                link::UNENCRYPTED_CREDENTIALS,
                                JsonSchema::words(&[link::ACCEPTED]),
                            ),
                    )
                    // A door shows a person that the surface sends a credential
                    // over an unencrypted link (decisions 0193 and 0194).
                    .with_notice(
                        link::UNENCRYPTED_CREDENTIALS,
                        link::ACCEPTED,
                        a2m_core::SurfaceNotice::UnencryptedCredentials,
                    ),
                )
                .file_kind(Arc::new(endpoints::EndpointKind::new()))
                .file_kind(Arc::new(described::DescriptionKind::new()))
                .lowering(Arc::new(lower::HttpLowering::new(mode)))
                .lowering(Arc::new(described::ApiLowering::new(api)))
                .lint_rule(Arc::new(endpoints::SecretLiteral))
                .lint_rule(Arc::new(endpoints::NoResponseSchema))
                .lint_rule(Arc::new(endpoints::EffectMethod))
                .lint_rule(Arc::new(endpoints::SignedEndpoint))
                .lint_rule(Arc::new(link::UnencryptedCredentials))
                .lint_rule(Arc::new(described::StatedTwice))
                .drafting_rule(Arc::new(openapi::DescribedOperationRule {
                    kind: openapi::described_operation()?,
                }))
                .drafting_rule(Arc::new(draft::DescribedReadRule {
                    kind: openapi::described_operation()?,
                }))
                .drafting_rule(Arc::new(described::PinnedDescriptionRule {
                    kind: openapi::described_operation()?,
                })),
        )?;
        let clock = cx.logs_and_traces().clock();
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
