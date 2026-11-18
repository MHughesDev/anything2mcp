//! Cases 13 to 15 and 29: signing in, and the signed-in copies of the JSON interface.
//!
//! Everything here lives under `/secure`, so nothing outside it changes. A person
//! signs in by form, with a username and a password, and optionally a one-time code;
//! the session is a cookie. The same JSON interface is served three more times:
//! under `/secure/api` for a session cookie, under `/secure/bearer/api` for a bearer
//! token, and under `/secure/key/api` for an API key in a header. The copy for a
//! bearer token serves its description openly, at `/secure/bearer/api/openapi.json`,
//! and the description says each operation takes the token. The credentials are made
//! up and declared in [`crate::harness`].

use std::collections::BTreeMap;
use std::hash::{BuildHasher, Hasher, RandomState};

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{AppendHeaders, Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::harness::{
    API_KEY, API_KEY_HEADER, BEARER_TOKEN, LOCKOUT_AFTER, PASSWORD, PENDING_COOKIE,
    SECOND_FACTOR_CODE, SESSION_COOKIE, USERNAME,
};
use crate::pages::escape;
use crate::{AppState, api, details};

/// Where a signed-in person lands unless the sign-in page was asked for another page.
const HOME: &str = "/secure/orders";
const SIGN_IN: &str = "/secure/sign-in";
const CODE: &str = "/secure/sign-in/code";

/// Modes a test can set for signing in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthSettings {
    /// Case 13: after the password, ask for the one-time code.
    #[serde(default)]
    pub second_factor: bool,
    /// Case 15: a new sign-in revokes every other session of the account.
    #[serde(default)]
    pub single_session: bool,
}

/// A credential of the JSON interface that the control interface can revoke.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Credential {
    /// The bearer token, [`BEARER_TOKEN`].
    BearerToken,
    /// The API key, [`API_KEY`].
    ApiKey,
}

/// What the application counted about signing in since the last reset.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthReport {
    /// Every sign-in form sent, whatever came of it, locked account included.
    pub sign_in_attempts: u64,
    /// Sign-ins that ended in a session.
    pub sign_ins: u64,
    /// Sign-in forms refused for a wrong username or password.
    pub failed_sign_ins: u64,
    /// One-time codes refused.
    pub failed_codes: u64,
    /// Whether the account is locked: after [`LOCKOUT_AFTER`] failed sign-ins in a
    /// row, every sign-in is refused, the right password too, until a reset.
    pub locked: bool,
    /// Sign-outs.
    pub sign_outs: u64,
    /// Sessions that are usable now.
    pub active_sessions: u64,
    /// Requests under `/secure` refused for a missing, expired, or revoked session.
    pub refused_sessions: u64,
    /// Requests refused for a wrong or revoked bearer token (a missing one is not counted).
    pub refused_tokens: u64,
    /// Requests refused for a wrong or revoked API key (a missing one is not counted).
    pub refused_keys: u64,
    /// Whether the control interface revoked the bearer token.
    pub bearer_token_revoked: bool,
    /// Whether the control interface revoked the API key.
    pub api_key_revoked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionState {
    Active,
    Expired,
    Revoked,
}

/// Everything the application holds about signing in.
#[derive(Debug)]
pub(crate) struct AuthState {
    salt: u64,
    issued: u64,
    sessions: BTreeMap<String, SessionState>,
    pending: BTreeMap<String, String>,
    failures_in_a_row: u64,
    settings: AuthSettings,
    report: AuthReport,
}

impl Default for AuthState {
    fn default() -> Self {
        // So that a cookie from one application is never valid in another.
        let salt = RandomState::new().build_hasher().finish();
        Self {
            salt,
            issued: 0,
            sessions: BTreeMap::new(),
            pending: BTreeMap::new(),
            failures_in_a_row: 0,
            settings: AuthSettings::default(),
            report: AuthReport::default(),
        }
    }
}

impl AuthState {
    fn new_id(&mut self, kind: &str) -> String {
        self.issued += 1;
        format!("example-{kind}-{:016x}-{}", self.salt, self.issued)
    }

    fn issue_session(&mut self) -> String {
        if self.settings.single_session {
            for state in self.sessions.values_mut() {
                if *state == SessionState::Active {
                    *state = SessionState::Revoked;
                }
            }
        }
        let id = self.new_id("session");
        self.sessions.insert(id.clone(), SessionState::Active);
        self.report.sign_ins += 1;
        id
    }

    fn end_sessions(&mut self, to: SessionState) {
        for state in self.sessions.values_mut() {
            if *state == SessionState::Active {
                *state = to;
            }
        }
    }

    pub(crate) fn report(&self) -> AuthReport {
        let mut report = self.report.clone();
        let active = self
            .sessions
            .values()
            .filter(|s| **s == SessionState::Active)
            .count();
        report.active_sessions = u64::try_from(active).unwrap_or(u64::MAX);
        report
    }

    pub(crate) fn set_settings(&mut self, settings: AuthSettings) {
        self.settings = settings;
    }

    pub(crate) fn expire_sessions(&mut self) {
        self.end_sessions(SessionState::Expired);
    }

    pub(crate) fn revoke_sessions(&mut self) {
        self.end_sessions(SessionState::Revoked);
    }

    pub(crate) fn revoke(&mut self, credential: Credential) {
        match credential {
            Credential::BearerToken => self.report.bearer_token_revoked = true,
            Credential::ApiKey => self.report.api_key_revoked = true,
        }
    }

    /// Checks the session cookie, counting a refusal.
    fn check_session(&mut self, headers: &HeaderMap) -> Result<(), &'static str> {
        let state = cookie(headers, SESSION_COOKIE).and_then(|id| self.sessions.get(&id).copied());
        let refusal = match state {
            Some(SessionState::Active) => return Ok(()),
            Some(SessionState::Expired) => "session_expired",
            Some(SessionState::Revoked) => "session_revoked",
            None => "needs_login",
        };
        self.report.refused_sessions += 1;
        Err(refusal)
    }
}

pub(crate) fn routes(state: &AppState) -> Router<AppState> {
    let interface = || api::routes().merge(details::api_routes());
    let pages = Router::new()
        .route(HOME, get(home))
        .route_layer(from_fn_with_state(state.clone(), page_guard));
    Router::new()
        .route("/secure", get(|| async { Redirect::to(HOME) }))
        .route(SIGN_IN, get(sign_in_page).post(sign_in))
        .route(CODE, get(code_page).post(code))
        .route("/secure/sign-out", post(sign_out))
        .merge(pages)
        .nest(
            "/secure",
            interface().route_layer(from_fn_with_state(state.clone(), session_guard)),
        )
        .nest(
            "/secure/bearer",
            interface()
                .route_layer(from_fn_with_state(state.clone(), bearer_guard))
                // The description of this copy is served openly, as many
                // systems serve theirs, and says that every operation it
                // describes takes the token.
                .merge(crate::openapi::bearer_routes()),
        )
        .nest(
            "/secure/key",
            interface().route_layer(from_fn_with_state(state.clone(), key_guard)),
        )
}

/// The value of one cookie in the request.
fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(n, _)| *n == name)
        .map(|(_, v)| v.to_owned())
}

/// Reads `application/x-www-form-urlencoded` text: the form body, or a query.
fn form_fields(text: &str) -> BTreeMap<String, String> {
    fn decode(part: &str) -> String {
        let bytes = part.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'+' => out.push(b' '),
                b'%' if i + 2 < bytes.len() => {
                    match std::str::from_utf8(&bytes[i + 1..i + 3])
                        .ok()
                        .and_then(|h| u8::from_str_radix(h, 16).ok())
                    {
                        Some(byte) => {
                            out.push(byte);
                            i += 2;
                        }
                        None => out.push(b'%'),
                    }
                }
                other => out.push(other),
            }
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }
    text.split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(name), decode(value))
        })
        .collect()
}

/// Where to go after signing in: only a page under `/secure`, never another site.
fn next_page(fields: &BTreeMap<String, String>) -> String {
    fields
        .get("next")
        .filter(|n| n.starts_with("/secure/") && !n.starts_with("//") && !n.contains('\\'))
        .cloned()
        .unwrap_or_else(|| HOME.to_owned())
}

fn query(request_query: Option<&str>) -> BTreeMap<String, String> {
    form_fields(request_query.unwrap_or(""))
}

fn set_cookie(name: &str, value: &str) -> String {
    format!("{name}={value}; Path=/secure; HttpOnly; SameSite=Lax")
}

fn clear_cookie(name: &str) -> String {
    format!("{name}=; Path=/secure; HttpOnly; SameSite=Lax; Max-Age=0")
}

fn page(title: &str, body: &str) -> Html<String> {
    Html(format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <title>{} - Example Orders</title>\n</head>\n<body>\n<main>\n{body}\n</main>\n</body>\n</html>\n",
        escape(title)
    ))
}

fn sign_in_form(next: &str, message: &str) -> Html<String> {
    let message = if message.is_empty() {
        String::new()
    } else {
        format!("<p id=\"message\" role=\"alert\">{}</p>\n", escape(message))
    };
    page(
        "Sign in",
        &format!(
            "<h1>Sign in</h1>\n{message}<form id=\"sign-in\" method=\"post\" action=\"{SIGN_IN}\">\n\
             <input type=\"hidden\" name=\"next\" value=\"{}\">\n\
             <p><label for=\"username\">Username</label> \
             <input id=\"username\" name=\"username\" autocomplete=\"username\" required></p>\n\
             <p><label for=\"password\">Password</label> \
             <input id=\"password\" name=\"password\" type=\"password\" autocomplete=\"current-password\" required></p>\n\
             <p><button type=\"submit\">Sign in</button></p>\n</form>",
            escape(next)
        ),
    )
}

async fn sign_in_page(request: Request) -> Html<String> {
    let fields = query(request.uri().query());
    let message = match fields.get("reason").map(String::as_str) {
        Some("session_expired") => "Your session has expired. Sign in again.",
        Some("session_revoked") => "Your session was ended. Sign in again.",
        _ => "",
    };
    sign_in_form(&next_page(&fields), message)
}

async fn sign_in(State(state): State<AppState>, body: String) -> Response {
    let fields = form_fields(&body);
    let next = next_page(&fields);
    let mut inner = state.lock().await;
    let auth = &mut inner.auth;
    auth.report.sign_in_attempts += 1;
    if auth.report.locked {
        let form = sign_in_form(
            &next,
            "This account is locked after too many failed sign-ins.",
        );
        return (StatusCode::LOCKED, form).into_response();
    }
    let right = fields.get("username").map(String::as_str) == Some(USERNAME)
        && fields.get("password").map(String::as_str) == Some(PASSWORD);
    if !right {
        auth.report.failed_sign_ins += 1;
        auth.failures_in_a_row += 1;
        if auth.failures_in_a_row >= LOCKOUT_AFTER {
            auth.report.locked = true;
        }
        let form = sign_in_form(&next, "The username or password is not right.");
        return (StatusCode::UNAUTHORIZED, form).into_response();
    }
    auth.failures_in_a_row = 0;
    if auth.settings.second_factor {
        let pending = auth.new_id("pending");
        auth.pending.insert(pending.clone(), next.clone());
        return (
            AppendHeaders([(header::SET_COOKIE, set_cookie(PENDING_COOKIE, &pending))]),
            Redirect::to(CODE),
        )
            .into_response();
    }
    let session = auth.issue_session();
    (
        AppendHeaders([(header::SET_COOKIE, set_cookie(SESSION_COOKIE, &session))]),
        Redirect::to(&next),
    )
        .into_response()
}

fn code_form(message: &str) -> Html<String> {
    let message = if message.is_empty() {
        String::new()
    } else {
        format!("<p id=\"message\" role=\"alert\">{}</p>\n", escape(message))
    };
    page(
        "One-time code",
        &format!(
            "<h1>One-time code</h1>\n{message}<form id=\"code\" method=\"post\" action=\"{CODE}\">\n\
             <p><label for=\"one-time-code\">Code</label> \
             <input id=\"one-time-code\" name=\"code\" inputmode=\"numeric\" autocomplete=\"one-time-code\" required></p>\n\
             <p><button type=\"submit\">Continue</button></p>\n</form>"
        ),
    )
}

async fn code_page(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let inner = state.lock().await;
    match cookie(&headers, PENDING_COOKIE) {
        Some(id) if inner.auth.pending.contains_key(&id) => code_form("").into_response(),
        _ => Redirect::to(SIGN_IN).into_response(),
    }
}

async fn code(State(state): State<AppState>, headers: HeaderMap, body: String) -> Response {
    let fields = form_fields(&body);
    let mut inner = state.lock().await;
    let auth = &mut inner.auth;
    let Some(pending) = cookie(&headers, PENDING_COOKIE).filter(|id| auth.pending.contains_key(id))
    else {
        return Redirect::to(SIGN_IN).into_response();
    };
    if fields.get("code").map(|c| c.trim()) != Some(SECOND_FACTOR_CODE) {
        auth.report.failed_codes += 1;
        return (
            StatusCode::UNAUTHORIZED,
            code_form("The code is not right."),
        )
            .into_response();
    }
    let next = auth
        .pending
        .remove(&pending)
        .unwrap_or_else(|| HOME.to_owned());
    let session = auth.issue_session();
    (
        AppendHeaders([
            (header::SET_COOKIE, clear_cookie(PENDING_COOKIE)),
            (header::SET_COOKIE, set_cookie(SESSION_COOKIE, &session)),
        ]),
        Redirect::to(&next),
    )
        .into_response()
}

async fn sign_out(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let mut inner = state.lock().await;
    if let Some(id) = cookie(&headers, SESSION_COOKIE)
        && inner.auth.sessions.remove(&id).is_some()
    {
        inner.auth.report.sign_outs += 1;
    }
    (
        AppendHeaders([(header::SET_COOKIE, clear_cookie(SESSION_COOKIE))]),
        Redirect::to(SIGN_IN),
    )
        .into_response()
}

async fn home(State(state): State<AppState>) -> Html<String> {
    let inner = state.lock().await;
    let mut rows = String::new();
    for order in inner.store.orders() {
        rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td></tr>\n",
            escape(&order.order_id),
            escape(&order.customer),
            order.items.len(),
        ));
    }
    page(
        "Orders",
        &format!(
            "<h1>Orders</h1>\n<p id=\"signed-in\">Signed in as {}.</p>\n\
             <form method=\"post\" action=\"/secure/sign-out\"><button type=\"submit\">Sign out</button></form>\n\
             <table>\n<caption>All orders</caption>\n\
             <thead><tr><th>Order</th><th>Customer</th><th>Lines</th></tr></thead>\n\
             <tbody>\n{rows}</tbody>\n</table>",
            escape(USERNAME)
        ),
    )
}

/// A page without a session redirects to the sign-in page, which returns here.
async fn page_guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let refused = state.lock().await.auth.check_session(request.headers());
    match refused {
        Ok(()) => next.run(request).await,
        Err(reason) => {
            let mut to = format!("{SIGN_IN}?next={}", request.uri().path());
            if reason != "needs_login" {
                to.push_str("&reason=");
                to.push_str(reason);
            }
            Redirect::to(&to).into_response()
        }
    }
}

fn unauthorized(error: &str, challenge: &str) -> Response {
    let mut response = (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": error,
            "message": "This request needs a valid credential.",
            "sign_in": SIGN_IN,
        })),
    )
        .into_response();
    if let Ok(value) = HeaderValue::from_str(challenge) {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, value);
    }
    response
}

/// The interface without a usable session answers 401 and does nothing.
async fn session_guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let refused = state.lock().await.auth.check_session(request.headers());
    match refused {
        Ok(()) => next.run(request).await,
        Err(reason) => unauthorized(reason, "Cookie realm=\"example\""),
    }
}

async fn bearer_guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, token)| token.trim().to_owned());
    let challenge = "Bearer realm=\"example\"";
    {
        let mut inner = state.lock().await;
        let auth = &mut inner.auth;
        match presented {
            None => return unauthorized("needs_credential", challenge),
            Some(token) if token != BEARER_TOKEN || auth.report.bearer_token_revoked => {
                auth.report.refused_tokens += 1;
                return unauthorized(
                    "invalid_credential",
                    "Bearer realm=\"example\", error=\"invalid_token\"",
                );
            }
            Some(_) => {}
        }
    }
    next.run(request).await
}

async fn key_guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let presented = request
        .headers()
        .get(API_KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_owned());
    let challenge = "ApiKey realm=\"example\", header=\"X-Api-Key\"";
    {
        let mut inner = state.lock().await;
        let auth = &mut inner.auth;
        match presented {
            None => return unauthorized("needs_credential", challenge),
            Some(key) if key != API_KEY || auth.report.api_key_revoked => {
                auth.report.refused_keys += 1;
                return unauthorized("invalid_credential", challenge);
            }
            Some(_) => {}
        }
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_text_is_decoded() {
        let fields = form_fields(
            "username=example-user&password=a+b%26c%3D&next=%2Fsecure%2Fx&bad=%zz&end=%4",
        );
        assert_eq!(fields["username"], "example-user");
        assert_eq!(fields["password"], "a b&c=");
        assert_eq!(fields["next"], "/secure/x");
        assert_eq!(fields["bad"], "%zz");
        assert_eq!(fields["end"], "%4");
    }

    #[test]
    fn the_next_page_never_leaves_the_signed_in_part() {
        let next = |value: &str| next_page(&[("next".to_owned(), value.to_owned())].into());
        assert_eq!(next("/secure/api/orders"), "/secure/api/orders");
        for away in [
            "https://elsewhere.example/",
            "//elsewhere.example/",
            "/orders",
            "/secure/\\x",
        ] {
            assert_eq!(next(away), HOME, "{away}");
        }
    }
}
