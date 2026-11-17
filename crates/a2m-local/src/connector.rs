//! The client's end of the local channel, which commands, the stdio proxy,
//! and the SDK bridge use. It holds no engine logic: it finds the engine,
//! checks that it belongs to this user, says hello, and carries requests.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use a2m_actions::Paths;
use a2m_core::{Failure, FailureClass, Version};
use axum::body::Body;
use axum::http::{HeaderValue, Method, Request, Response, StatusCode, header};
use hyper::client::conn::http1::SendRequest;
use hyper_util::rt::TokioIo;
use serde_json::Value as Json;
use tokio::sync::{Mutex, watch};

use crate::channel::{
    CLIENT_LABEL_HEADER, CLIENT_VERSION_HEADER, ClientLabel, HELLO_PATH, LOCAL_API,
    LOCAL_API_HEADER, OsUser, place, variable_digests,
};
use crate::codes;
use crate::os::{self, ConnectError};

/// The most a hello's answer may hold.
const HELLO_LIMIT: usize = 4 * 1024;
/// The host a request names. The channel is not a network address; the
/// header is there because HTTP/1.1 asks for one.
const HOST: &str = "localhost";
/// How long a client waits for an engine to answer its hello. An engine
/// binds its channel before it opens its store and answers a hello only
/// once the store is open, so this is how long an engine may take to open
/// before a client stops waiting for it (decision 0181).
pub const HELLO_PATIENCE: Duration = Duration::from_secs(30);

/// Finds the engine of one user and one set of paths.
#[derive(Debug, Clone)]
pub struct Connector {
    place: os::Place,
    expected: OsUser,
    version: Version,
    label: ClientLabel,
    /// The engine's log, which a failure to answer points at.
    log: PathBuf,
    hello_patience: Duration,
}

pub(crate) fn shown(path: &Path) -> String {
    path.display()
        .to_string()
        .chars()
        .filter(|c| !c.is_control())
        .collect()
}

/// An engine holds the channel and did not answer the hello in time.
fn hello_unanswered(log: &Path) -> Failure {
    codes::busy(
        codes::ENGINE_UNREACHABLE,
        "An engine holds the local channel and did not answer in time: it may still be opening its store, or it is stuck while starting.",
        format!(
            "Read the engine's log, `{}`. If the engine is stuck, stop its process and run the command again.",
            shown(log)
        ),
    )
}

/// The engine closed the connection before it answered the hello: nothing
/// was sent, and the engine most likely stopped while it was starting.
fn hello_lost(log: &Path) -> Failure {
    Failure::new(
        FailureClass::Transient,
        codes::CONNECTION_LOST,
        "The engine closed the connection before it answered, so nothing was sent: it stopped while starting, or it is closing.",
        format!(
            "Read the engine's log, `{}`, for why it stopped, then run the command again.",
            shown(log)
        ),
    )
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
            log: crate::Launch::log_of(paths),
            hello_patience: HELLO_PATIENCE,
        })
    }

    /// As [`Connector::new`], waiting `patience` for a hello's answer. A
    /// test waits less.
    #[cfg(test)]
    pub(crate) const fn hello_patience(mut self, patience: Duration) -> Self {
        self.hello_patience = patience;
        self
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
    /// went away, which for a hello means it stopped while starting;
    /// `busy`/`engine_unreachable`, pointing at the engine's log, when an
    /// engine holds the channel and does not answer the hello within
    /// [`HELLO_PATIENCE`]; `busy`/`channel_unavailable` for anything else
    /// the operating system refused.
    pub async fn connect(&self) -> Result<Link, Failure> {
        match tokio::time::timeout(self.hello_patience, self.say_hello()).await {
            Ok(said) => said,
            Err(_) => Err(hello_unanswered(&self.log)),
        }
    }

    async fn say_hello(&self) -> Result<Link, Failure> {
        let link = self.open().await?;
        let hello = Request::builder()
            .method(Method::GET)
            .uri(HELLO_PATH)
            .header(CLIENT_VERSION_HEADER, self.version.to_string())
            .header(LOCAL_API_HEADER, LOCAL_API.to_string())
            .header(CLIENT_LABEL_HEADER, self.label.as_text())
            .body(Body::empty())
            .map_err(|_| codes::lost())?;
        // A hello does nothing in the engine, so a hello whose answer was
        // lost is only a connection that ended.
        let answer = link.send(hello).await.map_err(|_| hello_lost(&self.log))?;
        let status = answer.status();
        let body = axum::body::to_bytes(answer.into_body(), HELLO_LIMIT)
            .await
            .map_err(|_| hello_lost(&self.log))?;
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
        // An engine that names none is told as one whose variables are not
        // known, never as one that has none.
        let start_variables = said["start_variables"].as_object().map(|named| {
            named
                .iter()
                .filter_map(|(name, digest)| Some((name.clone(), digest.as_str()?.to_owned())))
                .collect()
        });
        Ok(Link {
            engine_version,
            start_variables,
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
        // it. When the connection ends first, the link is told, so that no
        // request waits on a connection that is gone.
        let (ended, gone) = watch::channel(false);
        tokio::spawn(async move {
            let _ = connection.await;
            ended.send_replace(true);
        });
        Ok(Link {
            sender: Mutex::new(sender),
            gone,
            engine_version: self.version,
            start_variables: None,
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
    /// Turns true when the connection has ended.
    gone: watch::Receiver<bool>,
    engine_version: Version,
    /// The engine's start variables, by name and digest, as its hello
    /// named them; `None` when it named none.
    start_variables: Option<BTreeMap<String, String>>,
}

impl Link {
    /// The version of the engine that answered the hello.
    #[must_use]
    pub const fn engine_version(&self) -> Version {
        self.engine_version
    }

    /// The names of the start variables whose values differ between the
    /// engine that answered and `client`'s own: each set on one side only,
    /// or set to another value, in order of name. `None` when the engine
    /// named none in its hello, so nothing can be compared.
    #[must_use]
    pub fn start_variables_differing(
        &self,
        client: &BTreeMap<String, String>,
    ) -> Option<Vec<String>> {
        let engine = self.start_variables.as_ref()?;
        let client = variable_digests(client);
        let mut names: Vec<String> = engine
            .keys()
            .chain(client.keys())
            .filter(|name| engine.get(*name) != client.get(*name))
            .cloned()
            .collect();
        names.sort();
        names.dedup();
        Some(names)
    }

    /// Sends one request and returns the response as it arrives; the body
    /// may still be on its way. Requests on one link go one after another.
    /// The request's address is a path, such as `/api/v1/tools`.
    ///
    /// # Errors
    ///
    /// `transient`/`connection_lost` when the connection ended before the
    /// request was sent, so the engine never had it;
    /// `possible_commit`/`answer_lost` when the request may have reached
    /// the engine and no answer came back. The second is never safe to
    /// send again blindly: what the request asked may have been done.
    pub async fn send(&self, mut request: Request<Body>) -> Result<Response<Body>, Failure> {
        request
            .headers_mut()
            .entry(header::HOST)
            .or_insert(HeaderValue::from_static(HOST));
        let mut sender = self.sender.lock().await;
        let mut gone = self.gone.clone();
        let answered = async {
            // A connection that cannot take a request has been given none.
            sender.ready().await.map_err(|_| codes::lost())?;
            match sender.try_send_request(request).await {
                Ok(response) => Ok(response),
                // The library hands the request back when it never wrote
                // any of it; otherwise some of it may have reached the
                // engine.
                Err(mut error) => Err(if error.take_message().is_some() {
                    codes::lost()
                } else {
                    codes::answer_lost()
                }),
            }
        };
        // An answer that is there is taken first. Otherwise the end of the
        // connection ends the wait: nothing can answer after it, and the
        // request may have been read before it ended.
        let response = tokio::select! {
            biased;
            answered = answered => answered?,
            _ = gone.wait_for(|gone| *gone) => return Err(codes::answer_lost()),
        };
        Ok(response.map(Body::new))
    }
}
