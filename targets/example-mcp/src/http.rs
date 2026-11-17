//! The HTTP form: one endpoint on loopback that takes a POSTed JSON-RPC
//! message and answers with JSON. It opens no event stream, so a `GET` is
//! answered with 405 and the notification that the tool list changed is sent
//! over stdio only.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::extract::connect_info::Connected;
use axum::extract::{ConnectInfo, State as Shared};
use axum::http::header::{ALLOW, AUTHORIZATION, CONTENT_TYPE, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::serve::{IncomingStream, Listener};
use axum::{Json, Router};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};

use crate::protocol::{self, INVALID_REQUEST, Incoming, Outcome, PROTOCOL_VERSION_HEADER, Reply};
use crate::state::{Backing, Conn, Credential, State, Transport};

/// The path of the MCP endpoint.
pub const ENDPOINT_PATH: &str = "/mcp";
/// The path of the document a 401 answer points to.
pub const RESOURCE_METADATA_PATH: &str = "/.well-known/oauth-protected-resource";
/// The header that carries the session identifier.
pub const SESSION_HEADER: &str = "mcp-session-id";

/// Lets a handler close the connection its request arrived on.
#[derive(Clone, Debug, Default)]
pub(crate) struct Kill(Arc<AtomicBool>);

/// A connection that fails every read and write once it is killed, so that
/// the server closes the socket without sending a byte of an answer.
pub(crate) struct Killable {
    stream: TcpStream,
    kill: Kill,
}

impl Killable {
    fn check(&self) -> io::Result<()> {
        if self.kill.0.load(Ordering::SeqCst) {
            Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "the stand dropped this connection on purpose",
            ))
        } else {
            Ok(())
        }
    }
}

impl AsyncRead for Killable {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.check() {
            Ok(()) => Pin::new(&mut self.stream).poll_read(cx, buf),
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

impl AsyncWrite for Killable {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.check() {
            Ok(()) => Pin::new(&mut self.stream).poll_write(cx, buf),
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.check() {
            Ok(()) => Pin::new(&mut self.stream).poll_flush(cx),
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

/// Accepts connections that can be killed. The "address" of a connection is
/// its kill switch, which is how a handler gets hold of it.
pub(crate) struct StandListener(TcpListener);

impl Listener for StandListener {
    type Io = Killable;
    type Addr = Kill;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            // An accept that fails says nothing about the next one.
            if let Ok((stream, _)) = self.0.accept().await {
                let kill = Kill::default();
                return (
                    Killable {
                        stream,
                        kill: kill.clone(),
                    },
                    kill,
                );
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(Kill::default())
    }
}

impl Connected<IncomingStream<'_, StandListener>> for Kill {
    fn connect_info(stream: IncomingStream<'_, StandListener>) -> Self {
        stream.remote_addr().clone()
    }
}

#[derive(Clone)]
struct App {
    backing: Arc<Backing>,
    /// The scheme, host, and port the stand is reached at.
    base: Arc<str>,
}

/// What to answer, decided while the state was held.
enum Answer {
    Unauthorized(String),
    Plain(StatusCode, &'static str),
    Handled {
        outcome: Outcome,
        new_session: Option<String>,
    },
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then_some(token.trim())
}

/// Checks the credential. A request without an accepted one gets the 401
/// answer the protocol's authorization rules name, and is counted.
fn credential(state: &mut State, base: &str, token: Option<&str>) -> Result<Credential, Answer> {
    let metadata = format!("resource_metadata=\"{base}{RESOURCE_METADATA_PATH}\"");
    match state.credential(token) {
        Credential::Missing => {
            state.truth.unauthorized += 1;
            Err(Answer::Unauthorized(format!("Bearer {metadata}")))
        }
        Credential::Refused => {
            state.truth.unauthorized += 1;
            Err(Answer::Unauthorized(format!(
                "Bearer error=\"invalid_token\", {metadata}"
            )))
        }
        accepted => Ok(accepted),
    }
}

fn handle_post(state: &mut State, base: &str, headers: &HeaderMap, body: &str) -> Answer {
    let credential = match credential(state, base, bearer(headers)) {
        Ok(credential) => credential,
        Err(answer) => return answer,
    };
    let session = header(headers, SESSION_HEADER);
    let mut conn = match session {
        None => Conn::new(Transport::Http),
        Some(id) => match state.sessions.get(id) {
            Some(conn) => conn.clone(),
            None => return Answer::Plain(StatusCode::NOT_FOUND, "Session not found"),
        },
    };
    let incoming = Incoming {
        raw: body,
        header_revision: header(headers, PROTOCOL_VERSION_HEADER),
        credential,
    };
    let outcome = protocol::dispatch(state, &mut conn, &incoming);
    let new_session = match (&conn.id, session) {
        (Some(id), None) => Some(id.clone()),
        _ => None,
    };
    if let Some(id) = &conn.id {
        state.sessions.insert(id.clone(), conn.clone());
    }
    Answer::Handled {
        outcome,
        new_session,
    }
}

fn handle_delete(state: &mut State, base: &str, headers: &HeaderMap) -> Answer {
    if let Err(answer) = credential(state, base, bearer(headers)) {
        return answer;
    }
    let Some(id) = header(headers, SESSION_HEADER) else {
        return Answer::Plain(StatusCode::BAD_REQUEST, "No session named");
    };
    if state.sessions.remove(id).is_none() {
        return Answer::Plain(StatusCode::NOT_FOUND, "Session not found");
    }
    state.record(id, Transport::Http).open = false;
    Answer::Plain(StatusCode::OK, "Session ended")
}

fn plain(status: StatusCode, message: &str) -> Response {
    let error = json!({"code": INVALID_REQUEST, "message": message});
    (
        status,
        Json(json!({"jsonrpc": "2.0", "id": null, "error": error})),
    )
        .into_response()
}

fn status(code: u16) -> StatusCode {
    StatusCode::from_u16(code).unwrap_or(StatusCode::SERVICE_UNAVAILABLE)
}

async fn respond(answer: io::Result<Answer>, kill: &Kill) -> Response {
    let (outcome, new_session) = match answer {
        Err(_) => return plain(StatusCode::INTERNAL_SERVER_ERROR, "The stand lost its state"),
        Ok(Answer::Unauthorized(challenge)) => {
            let mut response = plain(StatusCode::UNAUTHORIZED, "Unauthorized");
            if let Ok(value) = challenge.parse() {
                response.headers_mut().insert(WWW_AUTHENTICATE, value);
            }
            return response;
        }
        Ok(Answer::Plain(code, message)) => return plain(code, message),
        Ok(Answer::Handled {
            outcome,
            new_session,
        }) => (outcome, new_session),
    };
    if outcome.delay_ms > 0 {
        // Not a wait for something: being late is the behavior asked for.
        tokio::time::sleep(Duration::from_millis(outcome.delay_ms)).await;
    }
    let mut response = match outcome.reply {
        Reply::Nothing => StatusCode::ACCEPTED.into_response(),
        Reply::Message(message) => (status(outcome.http_status), Json(message)).into_response(),
        Reply::Raw(text) => ([(CONTENT_TYPE, "application/json")], text).into_response(),
        Reply::Status(code) => {
            let body: Value = json!({"error": "fault", "message": "A fault was switched on for this request."});
            (status(code), Json(body)).into_response()
        }
        Reply::Drop => {
            // Every write on this connection now fails, so the server closes
            // the socket and the answer below never leaves.
            kill.0.store(true, Ordering::SeqCst);
            StatusCode::OK.into_response()
        }
    };
    if let Some(id) = new_session
        && let Ok(value) = id.parse()
    {
        response.headers_mut().insert(SESSION_HEADER, value);
    }
    response
}

async fn post_message(
    Shared(app): Shared<App>,
    ConnectInfo(kill): ConnectInfo<Kill>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let answer = app
        .backing
        .with(|state| handle_post(state, &app.base, &headers, &body));
    respond(answer, &kill).await
}

async fn end_session(
    Shared(app): Shared<App>,
    ConnectInfo(kill): ConnectInfo<Kill>,
    headers: HeaderMap,
) -> Response {
    let answer = app
        .backing
        .with(|state| handle_delete(state, &app.base, &headers));
    respond(answer, &kill).await
}

/// The stand opens no event stream.
async fn no_stream() -> Response {
    (StatusCode::METHOD_NOT_ALLOWED, [(ALLOW, "POST, DELETE")]).into_response()
}

/// The protected resource metadata a 401 answer points to. It names no
/// authorization server, because the stand has none.
async fn resource_metadata(Shared(app): Shared<App>) -> Json<Value> {
    Json(json!({
        "resource": format!("{}{ENDPOINT_PATH}", app.base),
        "bearer_methods_supported": ["header"]
    }))
}

/// Serves a stand on the listener until the task is stopped.
pub(crate) async fn serve(listener: TcpListener, backing: Arc<Backing>) -> io::Result<()> {
    let base = format!("http://{}", listener.local_addr()?);
    let app = App {
        backing,
        base: base.into(),
    };
    let router = Router::new()
        .route(
            ENDPOINT_PATH,
            post(post_message).get(no_stream).delete(end_session),
        )
        .route(RESOURCE_METADATA_PATH, get(resource_metadata))
        .with_state(app);
    axum::serve(
        StandListener(listener),
        router.into_make_service_with_connect_info::<Kill>(),
    )
    .await
}
