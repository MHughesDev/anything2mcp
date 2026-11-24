//! The auth provider `upstream_mcp`: what an upstream MCP server is given
//! to know who is calling.
//!
//! Built: a bearer token. The token is a stored credential, named by the
//! auth file under the role `token`; it is placed in the `Authorization`
//! header of each request through the lease, and never returned.
//!
//! Not built: the protocol's own authorization flow, in which the person is
//! given a link to open, and the code they come back with is exchanged for
//! tokens. An entry that asks for it is refused by name.

use a2m_auth::{
    AuthEntry, AuthProvider, BoxFuture, CredentialSink, ExpirySign, Material, ProviderName,
    ProviderNeeds, RawSign, SignInContext, SignedIn,
};
use a2m_core::{Failure, FailureClass, Secret, Timestamp};

use crate::codes;
use crate::names;

/// The credential role the auth file names the token under.
const TOKEN: &str = "token";
/// The one setting: which way the server is given its credential.
const FLOW: &str = "flow";

fn invalid(why: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        a2m_auth::codes::AUTH_SETTINGS_INVALID,
        format!("The auth entry for `upstream_mcp` cannot be used: {why}."),
        "Name a stored token under `credentials: {token: ...}`, and leave `flow` out or set it to `bearer`.",
    )
}

fn incomplete() -> Failure {
    Failure::new(
        FailureClass::SessionExpired,
        a2m_auth::codes::NEEDS_LOGIN,
        "The session cannot be used: its saved credentials are incomplete.",
        "Sign in with `anything2mcp session login`, then resume the run, which reconciles any write it began before it writes again.",
    )
}

/// The auth provider `upstream_mcp`.
#[derive(Debug, Clone, Copy, Default)]
pub struct UpstreamMcp;

impl AuthProvider for UpstreamMcp {
    fn name(&self) -> ProviderName {
        names::auth_provider()
    }

    fn needs(&self) -> ProviderNeeds {
        ProviderNeeds {
            person: false,
            silent_renewal: false,
            light_prompt: false,
            movable: true,
            // The token is only held: whether the server accepts it is
            // known when the server is used.
            confirms: false,
        }
    }

    fn check(&self, entry: &AuthEntry) -> Result<(), Failure> {
        entry.only(&[FLOW], &[TOKEN])?;
        match entry.optional(FLOW) {
            None | Some("bearer") => {}
            Some("authorization_code") => {
                return Err(codes::not_built(
                    "carry out the protocol's authorization flow, in which a person opens a link; only a bearer token is built",
                ));
            }
            Some(_) => return Err(invalid("the setting `flow` is not `bearer`")),
        }
        if entry.credentials.contains_key(TOKEN) {
            Ok(())
        } else {
            Err(invalid("no credential is named for the role `token`"))
        }
    }

    fn sign_in<'a>(
        &'a self,
        cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<Material, Failure>> {
        Box::pin(async move {
            self.check(cx.entry())?;
            let token = cx.credential(TOKEN).await?;
            Ok(Material::new(self.name()).with_secret(TOKEN, token))
        })
    }

    fn is_signed_in<'a>(
        &'a self,
        _material: &'a Material,
        _now: Timestamp,
    ) -> BoxFuture<'a, SignedIn> {
        Box::pin(async { SignedIn::Yes { until: None } })
    }

    fn detect_expiry(&self, sign: &RawSign) -> Option<ExpirySign> {
        match sign {
            RawSign::Status { code: 401, .. } => Some(ExpirySign::Refused),
            RawSign::PastExpiry => Some(ExpirySign::PastExpiry),
            _ => None,
        }
    }

    fn refresh<'a>(
        &'a self,
        _material: &'a Material,
        _cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<Material, Failure>> {
        Box::pin(async { Err(incomplete()) })
    }

    fn sign_out<'a>(
        &'a self,
        _material: &'a Material,
        _cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async { Ok(()) })
    }

    fn inject(&self, material: &Material, sink: &mut dyn CredentialSink) -> Result<(), Failure> {
        let token = material.secret(TOKEN).ok_or_else(incomplete)?;
        // A token that could not travel in a header is never sent.
        if !token
            .expose()
            .bytes()
            .all(|byte| (0x21..0x7f).contains(&byte))
        {
            return Err(incomplete());
        }
        sink.header(
            "Authorization",
            &Secret::new(format!("Bearer {}", token.expose())),
        );
        Ok(())
    }
}
