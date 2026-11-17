//! The client's end of the local channel, which commands, the stdio proxy,
//! and the SDK bridge use. It holds no engine logic: it finds the engine,
//! checks that it belongs to this user, says hello, and carries requests.

use a2m_actions::Paths;
use a2m_core::{Failure, Version};
use axum::body::Body;
use axum::http::{HeaderValue, Method, Request, Response, StatusCode, header};
use hyper::client::conn::http1::SendRequest;
use hyper_util::rt::TokioIo;
use serde_json::Value as Json;
use tokio::sync::Mutex;

use crate::channel::{
    CLIENT_LABEL_HEADER, CLIENT_VERSION_HEADER, ClientLabel, HELLO_PATH, LOCAL_API,
    LOCAL_API_HEADER, OsUser, place,
};
use crate::codes;
use crate::os::{self, ConnectError};

/// The most a hello's answer may hold.
const HELLO_LIMIT: usize = 4 * 1024;
/// The host a request names. The channel is not a network address; the
/// header is there because HTTP/1.1 asks for one.
const HOST: &str = "localhost";

/// Finds the engine of one user and one set of paths.
#[derive(Debug, Clone)]
pub struct Connector {
    place: os::Place,
    expected: OsUser,
    version: Version,
    label: ClientLabel,
}

impl Connector {
    /// A connector for the engine of this process's user. `version` is
    /// this program's and `label` says which client it is.
    ///
    /// # Errors
    ///
    /// `busy`/`channel_unavailable` when the operating system does not say
    /// who this process's user is.
    pub fn new(paths: &Paths, version: Version, label: ClientLabel) -> Result<Self, Failure> {
        let user = OsUser::current()?;
        Ok(Self {
            place: place(paths, &user),
            expected: user,
            version,
            label,
        })
    }

    /// As [`Connector::new`], expecting the engine to belong to `user`. A
    /// test expects another user, and so sees its own engine refused.
    #[cfg(test)]
    pub(crate) fn expecting(mut self, user: OsUser) -> Self {
        self.expected = user;
        self
    }

    /// Connects to the engine and says hello. The engine's owner is
    /// checked before anything is sent. Nothing is started: when no engine
    /// answers, the failure says so.
    ///
    /// # Errors
    ///
    /// `busy`/`engine_unreachable` when nothing answers;
    /// `policy`/`peer_not_allowed` when what answers is not this user's;
    /// `input`/`local_api_mismatch` when the engine speaks another version
    /// of the local API; `transient`/`connection_lost` when the engine
    /// went away; `busy`/`channel_unavailable` for anything else the
    /// operating system refused.
    pub async fn connect(&self) -> Result<Link, Failure> {
        let link = self.open().await?;
        let hello = Request::builder()
            .method(Method::GET)
            .uri(HELLO_PATH)
            .header(CLIENT_VERSION_HEADER, self.version.to_string())
            .header(LOCAL_API_HEADER, LOCAL_API.to_string())
            .header(CLIENT_LABEL_HEADER, self.label.as_text())
            .body(Body::empty())
            .map_err(|_| codes::lost())?;
        let answer = link.send(hello).await?;
        let status = answer.status();
        let body = axum::body::to_bytes(answer.into_body(), HELLO_LIMIT)
            .await
            .map_err(|_| codes::lost())?;
        let said: Json = serde_json::from_slice(&body).map_err(|_| not_an_engine())?;
        if status != StatusCode::OK {
            return Err(if said["code"] == codes::LOCAL_API_MISMATCH.to_string() {
                codes::input(
                    codes::LOCAL_API_MISMATCH,
                    "The engine that runs speaks another version of the local API than this client.",
                    "Install the same version of the client and the engine, then run the command again.",
                )
            } else {
                not_an_engine()
            });
        }
        let engine_version = said["engine_version"]
            .as_str()
            .and_then(|text| text.parse::<Version>().ok())
            .ok_or_else(not_an_engine)?;
        Ok(Link {
            engine_version,
            ..link
        })
    }

    /// Opens a connection and checks the engine's owner, without the
    /// hello.
    pub(crate) async fn open(&self) -> Result<Link, Failure> {
        let stream = os::connect(&self.place, self.expected.as_str())
            .await
            .map_err(|error| match error {
                ConnectError::NotRunning => codes::busy(
                    codes::ENGINE_UNREACHABLE,
                    "No engine answers on the local channel.",
                    "Start the engine with `anything2mcp serve`, then run the command again.",
                ),
                ConnectError::NotOurs => codes::policy(
                    codes::PEER_NOT_ALLOWED,
                    "What listens on the local channel does not belong to this user, so nothing was sent to it.",
                    "Check who owns the engine's runtime directory, remove what is not yours to trust, and start the engine again.",
                ),
                ConnectError::Io(error) => codes::unavailable("The local channel", &error),
            })?;
        let (sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|_| codes::lost())?;
        // The connection ends when the link is dropped, and this task with
        // it.
        tokio::spawn(async move {
            let _ = connection.await;
        });
        Ok(Link {
            sender: Mutex::new(sender),
            engine_version: self.version,
        })
    }
}

fn not_an_engine() -> Failure {
    codes::input(
        codes::HELLO_INVALID,
        "What answers on the local channel did not answer the hello as an engine does.",
        "Stop whatever holds the engine's channel, then run the command again.",
    )
}

/// One connection to the engine, after the hello.
#[derive(Debug)]
pub struct Link {
    sender: Mutex<SendRequest<Body>>,
    engine_version: Version,
}

impl Link {
    /// The version of the engine that answered the hello.
    #[must_use]
    pub const fn engine_version(&self) -> Version {
        self.engine_version
    }

    /// Sends one request and returns the response as it arrives; the body
    /// may still be on its way. Requests on one link go one after another.
    /// The request's address is a path, such as `/api/v1/tools`.
    ///
    /// # Errors
    ///
    /// `transient`/`connection_lost` when the engine went away.
    pub async fn send(&self, mut request: Request<Body>) -> Result<Response<Body>, Failure> {
        request
            .headers_mut()
            .entry(header::HOST)
            .or_insert(HeaderValue::from_static(HOST));
        let mut sender = self.sender.lock().await;
        sender.ready().await.map_err(|_| codes::lost())?;
        let response = sender
            .send_request(request)
            .await
            .map_err(|_| codes::lost())?;
        Ok(response.map(Body::new))
    }
}
