//! Whether a credential may cross the link to a surface (decision 0188).
//!
//! A request that carries the session's credential (an `Authorization`
//! header, a cookie, a key in a header) to a plain `http` address that is
//! not a loopback address is refused, unsent, at prepare and again at the
//! point of sending. A loopback address is one written as an IP address in
//! `127.0.0.0/8` or `::1`; a name, `localhost` included, is not one, since
//! the machine's resolver decides where a name leads. The one way to allow
//! it is the surface's own field `unencrypted_credentials: accepted` in
//! `target.yaml`, which the lint reports as a warning on every lint. No
//! setting, variable, or caller's input turns it on.

use std::net::IpAddr;

use a2m_core::{Failure, FailureClass, SurfacePlan};
use a2m_files::{CompileMode, Diagnostic, Level, LintRule, ResolvedView, Stage};
use serde::Deserialize;

use crate::codes;
use crate::endpoints::{HTTP_SURFACE, code};

/// The surface field that accepts credentials over an unencrypted link.
pub const UNENCRYPTED_CREDENTIALS: &str = "unencrypted_credentials";
/// The one word it takes.
pub const ACCEPTED: &str = "accepted";

/// What an `http` surface states beside its address, as the compiler put
/// it in the surface's payload.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SurfaceFields {
    #[serde(default)]
    unencrypted_credentials: Option<String>,
}

impl SurfaceFields {
    /// Reads the fields of a surface of the plan.
    pub(crate) fn of(surface: &SurfacePlan) -> Result<Self, Failure> {
        if surface.payload.is_none() {
            Ok(Self::default())
        } else {
            surface.payload.decode()
        }
    }

    /// Whether the surface declares that its credentials may cross an
    /// unencrypted link.
    pub(crate) fn accepts_unencrypted(&self) -> bool {
        self.unencrypted_credentials.as_deref() == Some(ACCEPTED)
    }
}

/// Whether an address is plain `http` to an address that is not a
/// loopback address written as an IP address.
pub(crate) fn plain_to_another_machine(url: &reqwest::Url) -> bool {
    if url.scheme() != "http" {
        return false;
    }
    let host = url.host_str().unwrap_or_default();
    let bare = host
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(host);
    !bare.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// The refusal: a credential would cross an unencrypted link to another
/// machine. Nothing was sent.
pub(crate) fn not_encrypted(surface: &str) -> Failure {
    codes::failure(
        FailureClass::Policy,
        codes::LINK_NOT_ENCRYPTED,
        format!(
            "The surface `{surface}` is reached over plain `http` at an address that is not a loopback address, and the request would carry the session's credential over that unencrypted link; nothing was sent."
        ),
        "Reach the surface over `https`. For a system on a private network that offers nothing else, the surface may declare `unencrypted_credentials: accepted` in `target.yaml`.",
    )
}

/// `http.unencrypted_credentials`: a surface that accepts credentials over
/// an unencrypted link is reported on every lint.
pub(crate) struct UnencryptedCredentials;

impl LintRule for UnencryptedCredentials {
    fn code(&self) -> a2m_core::LintCode {
        code("unencrypted_credentials")
    }

    fn level(&self, _mode: CompileMode) -> Level {
        Level::Warning
    }

    fn summary(&self) -> &str {
        "A surface that sends its credentials over an unencrypted link says so, and is reported on every lint."
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        view.target()
            .surfaces()
            .into_iter()
            .filter(|surface| *surface.kind == HTTP_SURFACE)
            .filter_map(|surface| {
                let field = surface.doc.get(UNENCRYPTED_CREDENTIALS)?;
                (field.as_text() == Some(ACCEPTED)).then(|| {
                    Diagnostic::error(
                        code("unencrypted_credentials"),
                        Stage::Lint,
                        &field.at,
                        format!(
                            "The surface `{}` accepts that its credentials cross an unencrypted link to another machine, where anyone on the path can read them.",
                            surface.name
                        ),
                        "Reach the surface over `https` and remove `unencrypted_credentials`, once the system offers it.",
                    )
                    .as_warning()
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::plain_to_another_machine;

    #[test]
    fn only_plain_http_to_an_address_that_is_not_loopback_is_another_machine() {
        let judged = |address: &str| plain_to_another_machine(&reqwest::Url::parse(address).unwrap());
        for (address, remote) in [
            ("http://127.0.0.1:8080", false),
            ("http://127.9.0.1", false),
            ("http://[::1]:8080", false),
            ("http://192.0.2.10", true),
            ("http://[2001:db8::1]", true),
            ("http://localhost:8080", true),
            ("http://app.example.test", true),
            ("https://app.example.test", false),
            ("https://192.0.2.10", false),
        ] {
            assert_eq!(judged(address), remote, "{address}");
        }
    }
}
