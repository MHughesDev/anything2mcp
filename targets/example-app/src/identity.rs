//! Single sign-on through an imaginary identity provider, "Example ID", on a
//! second origin of its own (decision 0254).
//!
//! - `GET /secure/sso` on the application: sends the browser to the identity
//!   provider's `/authorize`, with a state the application remembers.
//! - `GET /authorize` on the identity provider: its own sign-in page, with a
//!   username and a password. `POST /authorize`: a right password issues a
//!   one-use code and sends the browser back to the application's
//!   `/secure/sso/return` with the code and the state.
//! - `GET /secure/sso/return` on the application: takes the code once, issues
//!   the session cookie, and sends the browser to the orders page. A code it
//!   did not issue, one already used, or a state it did not send is refused.
//!
//! The identity provider is served under the name `localhost`, and the
//! application under `127.0.0.1`, so that the two keep their cookies apart as
//! two hosts do. It learns where the application is from the harness that
//! started both, never from the request, so it sends the browser nowhere a
//! request names. With `identity_provider_strays` it sends it instead to its
//! own `/elsewhere` under the name `127.0.0.1`: the same server at an origin
//! nobody declared, which counts every arrival.

use std::collections::BTreeMap;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::response::{AppendHeaders, Html, IntoResponse, Redirect, Response};
use axum::routing::get;

use crate::AppState;
use crate::auth::{form_fields, page, person_types, query, set_cookie};
use crate::harness::{PASSWORD, SESSION_COOKIE, USERNAME};
use crate::pages::escape;

/// The application's page that starts a single sign-on.
pub(crate) const START: &str = "/secure/sso";
/// The application's page the identity provider sends the browser back to.
pub(crate) const RETURN: &str = "/secure/sso/return";
/// The identity provider's sign-in page.
const AUTHORIZE: &str = "/authorize";
/// Where a signed-in person lands.
const HOME: &str = "/secure/orders";
/// The identity provider's own cookie, for its own origin.
const PROVIDER_COOKIE: &str = "example_id_session";

/// What both sides of the single sign-on hold.
#[derive(Debug, Default)]
pub(crate) struct SsoState {
    /// The application's origin, when an identity provider was started.
    pub(crate) application: Option<String>,
    /// The identity provider's origin, likewise.
    pub(crate) provider: Option<String>,
    /// The states the application sent and has not seen back.
    states: BTreeMap<String, ()>,
    /// Every code the identity provider issued, and whether it was used.
    codes: BTreeMap<String, bool>,
}

impl SsoState {
    /// Where the identity provider is served from, under its other name: the
    /// origin nobody declared.
    fn stray(&self) -> String {
        self.provider
            .as_deref()
            .unwrap_or_default()
            .replace("localhost", "127.0.0.1")
    }
}

/// The identity provider's routes, served on an origin of their own.
pub(crate) fn provider_routes() -> Router<AppState> {
    Router::new()
        .route(AUTHORIZE, get(authorize_page).post(authorize))
        .route(
            "/elsewhere",
            get(|State(state): State<AppState>| async move {
                state.lock().await.auth.report_mut().strays_arrived += 1;
                "Nobody declared this origin."
            }),
        )
}

/// The application starts a sign-in: to the identity provider, with a state.
pub(crate) async fn start(State(state): State<AppState>) -> Response {
    let mut inner = state.lock().await;
    let auth = &mut inner.auth;
    let Some(provider) = auth.sso.provider.clone() else {
        return (StatusCode::NOT_FOUND, "No identity provider was started.").into_response();
    };
    let sent = auth.new_id("state");
    auth.sso.states.insert(sent.clone(), ());
    Redirect::to(&format!("{provider}{AUTHORIZE}?state={sent}")).into_response()
}

fn provider_form(state: &str, message: &str) -> String {
    let message = if message.is_empty() {
        String::new()
    } else {
        format!("<p id=\"message\" role=\"alert\">{}</p>\n", escape(message))
    };
    format!(
        "<h1>Example ID</h1>\n{message}<form id=\"provider-sign-in\" method=\"post\" action=\"{AUTHORIZE}\">\n\
         <input type=\"hidden\" name=\"state\" value=\"{}\">\n\
         <p><label for=\"username\">Username</label> \
         <input id=\"username\" name=\"username\" autocomplete=\"username\" required></p>\n\
         <p><label for=\"password\">Password</label> \
         <input id=\"password\" name=\"password\" type=\"password\" autocomplete=\"current-password\" required></p>\n\
         <p><button type=\"submit\">Continue</button></p>\n</form>",
        escape(state)
    )
}

async fn authorize_page(State(state): State<AppState>, request: Request) -> Html<String> {
    let fields = query(request.uri().query());
    let sent = fields.get("state").cloned().unwrap_or_default();
    let typed = state.lock().await.auth.settings().person_at_the_window;
    let Html(mut html) = page("Example ID", &provider_form(&sent, ""));
    if typed {
        html = html.replace(
            "</main>",
            &format!("</main>\n{}", person_types("provider-sign-in")),
        );
    }
    Html(html)
}

async fn authorize(State(state): State<AppState>, body: String) -> Response {
    let fields = form_fields(&body);
    let sent = fields.get("state").cloned().unwrap_or_default();
    let right = fields.get("username").map(String::as_str) == Some(USERNAME)
        && fields.get("password").map(String::as_str) == Some(PASSWORD);
    let mut inner = state.lock().await;
    let auth = &mut inner.auth;
    if !right {
        let form = page(
            "Example ID",
            &provider_form(&sent, "The username or password is not right."),
        );
        return (StatusCode::UNAUTHORIZED, form).into_response();
    }
    let code = auth.new_id("code");
    auth.sso.codes.insert(code.clone(), false);
    auth.report_mut().identity_provider_sign_ins += 1;
    let provider_session = auth.new_id("provider-session");
    let back = if auth.settings().identity_provider_strays {
        auth.sso.stray() + "/elsewhere"
    } else {
        format!(
            "{}{RETURN}?code={code}&state={sent}",
            auth.sso.application.as_deref().unwrap_or_default()
        )
    };
    (
        AppendHeaders([(
            header::SET_COOKIE,
            format!("{PROVIDER_COOKIE}={provider_session}; Path=/; HttpOnly; SameSite=Lax"),
        )]),
        Redirect::to(&back),
    )
        .into_response()
}

/// The browser comes back with a code: taken once, it signs the person in.
pub(crate) async fn come_back(State(state): State<AppState>, request: Request) -> Response {
    let fields = query(request.uri().query());
    let mut inner = state.lock().await;
    let auth = &mut inner.auth;
    let known_state = fields
        .get("state")
        .is_some_and(|sent| auth.sso.states.remove(sent).is_some());
    let fresh_code = fields
        .get("code")
        .and_then(|code| auth.sso.codes.get_mut(code))
        .is_some_and(|used| !std::mem::replace(used, true));
    if !(known_state && fresh_code) {
        auth.report_mut().returns_refused += 1;
        return (
            StatusCode::BAD_REQUEST,
            page(
                "Sign-in not completed",
                "<h1>Sign-in not completed</h1>\n<p>The sign-in could not be completed.</p>",
            ),
        )
            .into_response();
    }
    auth.report_mut().codes_used += 1;
    let session = auth.issue_session();
    (
        AppendHeaders([(header::SET_COOKIE, set_cookie(SESSION_COOKIE, &session))]),
        Redirect::to(HOME),
    )
        .into_response()
}
