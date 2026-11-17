//! The providers that exchange a credential with an identity service's
//! token endpoint (RFC 6749): client credentials, and the authorization
//! code with a link the person opens (with RFC 7636 proof keys).
//!
//! The token endpoint comes from the target's auth file and from nowhere
//! else. It must be `https`, or `http` to a loopback address written as an
//! IP address; `localhost` is a name, not one (decision 0188). Nothing an identity
//! service answers is put into a failure: only the standard's own error
//! names are read, and each maps to a fixed sentence.

use std::time::Duration;

use a2m_core::{Failure, Secret, Timestamp};
use a2m_vault::BoxFuture;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::{part, refused_status};
use crate::encode::{base64, base64_url, form, hex, percent};
use crate::failure;
use crate::key::AuthEntry;
use crate::material::Material;
use crate::name::ProviderName;
use crate::person::{PersonAnswer, PersonAsk, PersonReason};
use crate::provider::{AuthProvider, ExpirySign, ProviderNeeds, RawSign, SignInContext, SignedIn};
use crate::sink::CredentialSink;

const CLIENT_CREDENTIALS: &str = "oauth_client_credentials";
const AUTHORIZATION_CODE: &str = "oauth_authorization_code";

/// The largest answer read from a token endpoint.
const ANSWER_LIMIT: usize = 64 * 1024;

/// Whether `url` is an address a credential may be sent to: `https`, or
/// `http` to a loopback address written as an IP address (`crate::link`).
/// No user part, no spaces.
fn endpoint_ok(url: &str) -> bool {
    let clean = url
        .bytes()
        .all(|byte| byte.is_ascii_graphic() && byte != b'@');
    let (secure, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (false, rest)
    } else {
        return false;
    };
    // The address as written must name a host: the parser would otherwise
    // read `https:///token` as the host `token`.
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    clean
        && !authority.is_empty()
        && parsed.host_str().is_some_and(|host| !host.is_empty())
        && (secure || crate::link::loopback(&parsed))
}

fn endpoint<'a>(entry: &'a AuthEntry, name: &str) -> Result<&'a str, Failure> {
    let url = entry.required(name)?;
    if endpoint_ok(url) {
        Ok(url)
    } else {
        Err(failure::settings_invalid(
            entry.provider.as_str(),
            &format!(
                "the setting `{name}` is not an `https` address, nor `http` to a loopback address written as an IP address such as `127.0.0.1` (a name such as `localhost` is not one)"
            ),
        ))
    }
}

/// How the client proves itself to the token endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClientAuth {
    /// In the `Authorization` header, which every service must accept.
    Basic,
    /// In the body of the request.
    Body,
}

fn client_auth(entry: &AuthEntry) -> Result<ClientAuth, Failure> {
    match entry.optional("client_auth") {
        None | Some("basic") => Ok(ClientAuth::Basic),
        Some("body") => Ok(ClientAuth::Body),
        Some(_) => Err(failure::settings_invalid(
            entry.provider.as_str(),
            "the setting `client_auth` is neither `basic` nor `body`",
        )),
    }
}

/// What a token endpoint gave.
struct Tokens {
    access: Secret,
    refresh: Option<Secret>,
    expires_at: Option<Timestamp>,
}

/// One request to the token endpoint. `pairs` is the grant; the client's
/// own proof is added here.
async fn exchange(
    cx: &SignInContext<'_>,
    pairs: &[(&str, &str)],
    client_secret: Option<&Secret>,
) -> Result<Tokens, Failure> {
    let entry = cx.entry();
    let url = endpoint(entry, "token_endpoint")?;
    let client_id = entry.required("client_id")?;
    let auth = client_auth(entry)?;
    let mut pairs = pairs.to_vec();
    for name in ["scope", "audience"] {
        if let Some(value) = entry.optional(name)
            && !pairs.iter().any(|(given, _)| *given == name)
        {
            pairs.push((name, value));
        }
    }
    let mut basic = None;
    match (client_secret, auth) {
        (Some(secret), ClientAuth::Basic) => {
            let mut pair = Zeroizing::new(String::new());
            percent(client_id, &mut pair);
            pair.push(':');
            percent(secret.expose(), &mut pair);
            let header = Secret::new(format!("Basic {}", base64(pair.as_bytes())));
            cx.register(&header);
            basic = Some(header);
        }
        (Some(secret), ClientAuth::Body) => {
            pairs.push(("client_id", client_id));
            pairs.push(("client_secret", secret.expose()));
        }
        (None, _) => pairs.push(("client_id", client_id)),
    }
    let body = form(&pairs);

    let http = client(url)?;
    let mut request = http
        .post(url)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("accept", "application/json")
        .body(body);
    if let Some(header) = &basic {
        request = request.header("authorization", header.expose());
    }
    let mut response = request.send().await.map_err(|error| {
        crate::tls::refusal(&error, "the identity service")
            .unwrap_or_else(|| failure::upstream_unavailable("it did not answer"))
    })?;
    let status = response.status();
    let mut answer = Zeroizing::new(Vec::new());
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| failure::upstream_unavailable("its answer was cut off"))?
    {
        if answer.len() + chunk.len() > ANSWER_LIMIT {
            return Err(failure::unexpected_response("its answer is too large"));
        }
        answer.extend_from_slice(&chunk);
    }
    if status.is_server_error() || status.as_u16() == 429 {
        return Err(failure::upstream_unavailable(
            "it answered that it cannot serve now",
        ));
    }
    let json: serde_json::Value = serde_json::from_slice(&answer)
        .map_err(|_| failure::unexpected_response("its answer is not JSON"))?;
    let text = |name: &str| json.get(name).and_then(serde_json::Value::as_str);
    if !status.is_success() {
        return Err(refusal(entry, text("error")));
    }
    let access = text("access_token")
        .filter(|token| !token.is_empty())
        .ok_or_else(|| failure::unexpected_response("its answer holds no access token"))?;
    if !text("token_type").is_some_and(|kind| kind.eq_ignore_ascii_case("bearer")) {
        return Err(failure::unexpected_response(
            "the token is not a bearer token",
        ));
    }
    let access = Secret::new(access);
    cx.register(&access);
    let refresh = text("refresh_token")
        .filter(|token| !token.is_empty())
        .map(Secret::new);
    if let Some(refresh) = &refresh {
        cx.register(refresh);
    }
    // Some services write the number of seconds as text.
    let seconds = json.get("expires_in").and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
    });
    let expires_at = seconds.filter(|seconds| *seconds >= 0).map(|seconds| {
        Timestamp::from_unix_millis(
            cx.now()
                .as_unix_millis()
                .saturating_add(seconds.saturating_mul(1000)),
        )
    });
    Ok(Tokens {
        access,
        refresh,
        expires_at,
    })
}

/// The HTTP client for one exchange. It follows no redirect: a credential
/// goes to the address the auth file names and nowhere else.
fn client(url: &str) -> Result<reqwest::Client, Failure> {
    #[cfg(not(feature = "tls"))]
    if url.starts_with("https://") {
        return Err(failure::tls_not_enabled());
    }
    #[cfg(feature = "tls")]
    let _ = url;
    // What the engine trusts, with its TLS provider stated in the client's
    // own configuration; nothing is installed for the process.
    crate::tls::client_builder(&crate::tls::installed()?)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| failure::upstream_unavailable("no client could be made to reach it"))
}

/// What a refusal by the token endpoint means, by the standard's own error
/// names (RFC 6749, section 5.2, and OpenID Connect's).
fn refusal(entry: &AuthEntry, error: Option<&str>) -> Failure {
    match error {
        Some("invalid_client" | "invalid_grant" | "unauthorized_client" | "access_denied") => {
            failure::credential_refused("the identity service")
        }
        Some("interaction_required" | "login_required" | "consent_required") => {
            failure::needs_login("the identity service wants the person to sign in again")
        }
        Some("invalid_scope") => failure::settings_invalid(
            entry.provider.as_str(),
            "the identity service does not grant the scope asked for",
        ),
        Some("invalid_request" | "unsupported_grant_type" | "invalid_target") => {
            failure::settings_invalid(
                entry.provider.as_str(),
                "the identity service does not accept this kind of request for this client",
            )
        }
        Some("temporarily_unavailable" | "server_error") => {
            failure::upstream_unavailable("it answered that it cannot serve now")
        }
        _ => failure::unexpected_response("it refused with an error this engine has no rule for"),
    }
}

fn bearer_material(name: &'static str, tokens: Tokens) -> Material {
    let mut material = Material::new(ProviderName::fixed(name))
        .with_secret("access_token", tokens.access)
        .expiring_at(tokens.expires_at);
    if let Some(refresh) = tokens.refresh {
        material = material.with_secret("refresh_token", refresh);
    }
    material
}

fn inject_bearer(material: &Material, sink: &mut dyn CredentialSink) -> Result<(), Failure> {
    let token = part(material, "access_token")?;
    sink.header(
        "Authorization",
        &Secret::new(format!("Bearer {}", token.expose())),
    );
    Ok(())
}

fn token_signed_in(material: &Material, now: Timestamp) -> SignedIn {
    match material.expires_at() {
        Some(until) if until <= now => SignedIn::No,
        until => SignedIn::Yes { until },
    }
}

/// `oauth_client_credentials`: a client secret exchanged for a token. A
/// renewal is the same exchange again.
///
/// Settings: `token_endpoint`, `client_id`; optional `scope`, `audience`,
/// `client_auth` (`basic` or `body`). Credential role: `client_secret`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ClientCredentials;

impl ClientCredentials {
    async fn tokens(cx: &SignInContext<'_>) -> Result<Material, Failure> {
        let secret = cx.credential("client_secret").await?;
        let tokens = exchange(cx, &[("grant_type", "client_credentials")], Some(&secret)).await?;
        Ok(bearer_material(CLIENT_CREDENTIALS, tokens))
    }
}

impl AuthProvider for ClientCredentials {
    fn name(&self) -> ProviderName {
        ProviderName::fixed(CLIENT_CREDENTIALS)
    }

    fn needs(&self) -> ProviderNeeds {
        ProviderNeeds {
            person: false,
            silent_renewal: true,
            light_prompt: false,
            movable: true,
            confirms: true,
        }
    }

    fn check(&self, entry: &AuthEntry) -> Result<(), Failure> {
        entry.only(
            &[
                "token_endpoint",
                "client_id",
                "scope",
                "audience",
                "client_auth",
            ],
            &["client_secret"],
        )?;
        endpoint(entry, "token_endpoint")?;
        entry.required("client_id")?;
        client_auth(entry)?;
        if !entry.credentials.contains_key("client_secret") {
            return Err(failure::settings_invalid(
                CLIENT_CREDENTIALS,
                "no credential is named for the role `client_secret`",
            ));
        }
        Ok(())
    }

    fn sign_in<'a>(
        &'a self,
        cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<Material, Failure>> {
        Box::pin(Self::tokens(cx))
    }

    fn is_signed_in<'a>(
        &'a self,
        material: &'a Material,
        now: Timestamp,
    ) -> BoxFuture<'a, SignedIn> {
        Box::pin(async move { token_signed_in(material, now) })
    }

    fn detect_expiry(&self, sign: &RawSign) -> Option<ExpirySign> {
        refused_status(sign)
    }

    fn refresh<'a>(
        &'a self,
        _material: &'a Material,
        cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<Material, Failure>> {
        Box::pin(Self::tokens(cx))
    }

    fn sign_out<'a>(
        &'a self,
        _material: &'a Material,
        _cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async { Ok(()) })
    }

    fn inject(&self, material: &Material, sink: &mut dyn CredentialSink) -> Result<(), Failure> {
        inject_bearer(material, sink)
    }
}

/// `oauth_authorization_code`: the person is given a link to the identity
/// service, opens it in their own browser, and agrees; the code their
/// browser comes back with is exchanged for tokens. Nothing drives that
/// browser. A renewal uses the refresh token, which may rotate.
///
/// Settings: `authorization_endpoint`, `token_endpoint`, `client_id`;
/// optional `scope`, `audience`, `client_auth`. Credential role:
/// `client_secret`, optional: without one the client is a public client,
/// protected by the proof key alone.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AuthorizationCode;

impl AuthorizationCode {
    async fn client_secret(cx: &SignInContext<'_>) -> Result<Option<Secret>, Failure> {
        if cx.has_credential("client_secret") {
            cx.credential("client_secret").await.map(Some)
        } else {
            Ok(None)
        }
    }

    async fn with_consent(cx: &SignInContext<'_>) -> Result<Material, Failure> {
        let entry = cx.entry();
        let authorize = endpoint(entry, "authorization_endpoint")?;
        let client_id = entry.required("client_id")?;
        let back = cx.return_address().ok_or_else(|| {
            failure::needs_login("no surface here can receive the answer to a sign-in link")
        })?;

        let mut bytes = [0u8; 48];
        cx.random(&mut bytes)?;
        let state = hex(&bytes[..16]);
        // The proof key (RFC 7636): only its hash goes into the link.
        let verifier = Secret::new(base64_url(&bytes[16..]));
        cx.register(&verifier);
        let challenge = base64_url(&Sha256::digest(verifier.expose().as_bytes()));

        let mut query = vec![
            ("response_type", "code"),
            ("client_id", client_id),
            ("redirect_uri", back.as_str()),
            ("state", state.as_str()),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
        ];
        for name in ["scope", "audience"] {
            if let Some(value) = entry.optional(name) {
                query.push((name, value));
            }
        }
        let joiner = if authorize.contains('?') { '&' } else { '?' };
        let url = format!("{authorize}{joiner}{}", form(&query));

        let answer = cx
            .ask_person(PersonAsk::OpenLink {
                url,
                reason: PersonReason::Consent,
            })
            .await;
        let returned = match answer {
            PersonAnswer::Done(returned) => returned,
            PersonAnswer::Declined => {
                return Err(failure::needs_login("the person declined the sign-in"));
            }
            PersonAnswer::NoAnswer => {
                return Err(failure::needs_login(
                    "nobody opened the sign-in link in time",
                ));
            }
            PersonAnswer::Unreachable => {
                return Err(failure::needs_login("no surface could reach the person"));
            }
        };
        if !returned
            .get("state")
            .is_some_and(|given| given.expose() == state)
        {
            return Err(failure::unexpected_response(
                "the answer does not belong to this sign-in",
            ));
        }
        if returned.get("error").is_some() {
            return Err(failure::needs_login(
                "the identity service did not grant the sign-in",
            ));
        }
        let code = returned
            .get("code")
            .ok_or_else(|| failure::unexpected_response("the answer holds no code"))?;

        let secret = Self::client_secret(cx).await?;
        let tokens = exchange(
            cx,
            &[
                ("grant_type", "authorization_code"),
                ("code", code.expose()),
                ("redirect_uri", back.as_str()),
                ("code_verifier", verifier.expose()),
            ],
            secret.as_ref(),
        )
        .await?;
        Ok(bearer_material(AUTHORIZATION_CODE, tokens))
    }

    async fn with_refresh_token(
        material: &Material,
        cx: &SignInContext<'_>,
    ) -> Result<Material, Failure> {
        let Some(held) = material.secret("refresh_token") else {
            return Err(failure::needs_login(
                "the identity service gave no way to renew without the person",
            ));
        };
        let secret = Self::client_secret(cx).await?;
        let mut tokens = exchange(
            cx,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", held.expose()),
            ],
            secret.as_ref(),
        )
        .await?;
        // A service that does not rotate the refresh token sends none back:
        // the one held stays the one to use.
        if tokens.refresh.is_none() {
            tokens.refresh = Some(Secret::new(held.expose()));
        }
        Ok(bearer_material(AUTHORIZATION_CODE, tokens))
    }
}

impl AuthProvider for AuthorizationCode {
    fn name(&self) -> ProviderName {
        ProviderName::fixed(AUTHORIZATION_CODE)
    }

    fn needs(&self) -> ProviderNeeds {
        ProviderNeeds {
            person: true,
            silent_renewal: true,
            light_prompt: false,
            movable: true,
            confirms: true,
        }
    }

    fn check(&self, entry: &AuthEntry) -> Result<(), Failure> {
        entry.only(
            &[
                "authorization_endpoint",
                "token_endpoint",
                "client_id",
                "scope",
                "audience",
                "client_auth",
            ],
            &["client_secret"],
        )?;
        endpoint(entry, "authorization_endpoint")?;
        endpoint(entry, "token_endpoint")?;
        entry.required("client_id")?;
        client_auth(entry).map(|_| ())
    }

    fn sign_in<'a>(
        &'a self,
        cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<Material, Failure>> {
        Box::pin(Self::with_consent(cx))
    }

    fn is_signed_in<'a>(
        &'a self,
        material: &'a Material,
        now: Timestamp,
    ) -> BoxFuture<'a, SignedIn> {
        Box::pin(async move { token_signed_in(material, now) })
    }

    fn detect_expiry(&self, sign: &RawSign) -> Option<ExpirySign> {
        refused_status(sign)
    }

    fn refresh<'a>(
        &'a self,
        material: &'a Material,
        cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<Material, Failure>> {
        Box::pin(Self::with_refresh_token(material, cx))
    }

    fn sign_out<'a>(
        &'a self,
        _material: &'a Material,
        _cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async { Ok(()) })
    }

    fn inject(&self, material: &Material, sink: &mut dyn CredentialSink) -> Result<(), Failure> {
        inject_bearer(material, sink)
    }
}

#[cfg(test)]
mod tests {
    use super::endpoint_ok;

    #[test]
    fn a_credential_is_sent_only_over_https_or_to_this_machine() {
        for good in [
            "https://id.example.test/oauth/token",
            "http://127.0.0.1:8080/token",
            "http://127.1/token",
            "http://[::1]:9000/token",
            "https://localhost/token",
        ] {
            assert!(endpoint_ok(good), "{good}");
        }
        for bad in [
            "http://id.example.test/token",
            // A name is not a loopback address (decision 0188), whatever
            // the resolver makes of it.
            "http://localhost/token",
            "http://localhost./token",
            "http://[::ffff:127.0.0.1]/token",
            "http://0.0.0.0/token",
            "http://127.0.0.1.example.test/token",
            "https://user@id.example.test/token",
            "http://localhost@id.example.test/token",
            "ftp://127.0.0.1/token",
            "https:///token",
            "https://id.example.test/to ken",
            "id.example.test/token",
        ] {
            assert!(!endpoint_ok(bad), "{bad}");
        }
    }
}
