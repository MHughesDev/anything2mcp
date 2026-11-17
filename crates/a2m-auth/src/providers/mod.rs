//! The neutral auth providers: the ones that only hold or exchange a
//! credential, and so need no surface of the system they sign in to.
//!
//! | Provider | What it does |
//! | --- | --- |
//! | `api_key` | Holds a key; places it in a named header |
//! | `bearer_static` | Holds a token; places it as a bearer token |
//! | `http_basic` | Holds a password; places the Basic header |
//! | `http_digest` | Holds a password; hands it to the driver, which answers the challenge |
//! | `windows_integrated` | Holds a password; hands it to the driver for Negotiate or NTLM |
//! | `client_certificate` | Holds a certificate and its key; places them as the client's identity |
//! | `oauth_client_credentials` | Exchanges a client secret for a token, and again when it runs out |
//! | `oauth_authorization_code` | Gives the person a link, exchanges the code they come back with, and refreshes |
//!
//! One-time codes are not a provider of sessions: [`totp`] computes a code
//! that another provider enters, through
//! [`SignInContext::one_time_code`](crate::SignInContext::one_time_code).
//!
//! Not built: `obo_token`, which needs the caller's own token carried from
//! the front door to the worker, and `os_broker`, which is per operating
//! system and belongs to Phase 7.

use std::sync::Arc;

use a2m_core::{Failure, Secret};

use crate::failure;
use crate::material::Material;
use crate::provider::{AuthProvider, ExpirySign, RawSign};

mod holder;
mod oauth;
pub mod totp;

/// The names of the neutral providers this crate builds, in order.
pub const NEUTRAL: &[&str] = &[
    "api_key",
    "bearer_static",
    "client_certificate",
    "http_basic",
    "http_digest",
    "oauth_authorization_code",
    "oauth_client_credentials",
    "windows_integrated",
];

/// One of each neutral provider.
pub(crate) fn neutral() -> Vec<Arc<dyn AuthProvider>> {
    let mut all: Vec<Arc<dyn AuthProvider>> = holder::Kind::ALL
        .iter()
        .map(|kind| Arc::new(holder::Holder::new(*kind)) as Arc<dyn AuthProvider>)
        .collect();
    all.push(Arc::new(oauth::ClientCredentials));
    all.push(Arc::new(oauth::AuthorizationCode));
    all
}

/// A part the provider put into its material and now cannot find: the
/// material is not this provider's, or is damaged.
fn part<'a>(material: &'a Material, name: &str) -> Result<&'a Secret, Failure> {
    material
        .secret(name)
        .ok_or_else(|| failure::needs_login("its saved credentials are incomplete"))
}

/// What a refusing status means for a provider whose credential is sent
/// with every request: status 401 says the credential was not accepted.
fn refused_status(sign: &RawSign) -> Option<ExpirySign> {
    match sign {
        RawSign::Status { code: 401, .. } => Some(ExpirySign::Refused),
        RawSign::PastExpiry => Some(ExpirySign::PastExpiry),
        _ => None,
    }
}

/// Whether text can be placed in a header without changing the request:
/// visible ASCII and spaces only.
fn header_safe(text: &str) -> bool {
    text.bytes().all(|byte| (0x20..0x7f).contains(&byte))
}
