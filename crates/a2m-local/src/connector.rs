//! The client's end of the local channel, which commands, the stdio proxy,
//! and the SDK bridge use. It holds no engine logic: it finds the engine,
//! checks that it belongs to this user, says hello, and carries requests.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use a2m_actions::Paths;
use a2m_core::{Failure, Version};
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
    codes::failure(
        &codes::row::ENGINE_UNREACHABLE,
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
    codes::row::CONNECTION_LOST.failure(
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
    /// went away, which for a hello means it stopped while starting, and,
    /// on macOS and Linux, when it closed its channel with this connection
    /// made and not yet taken, as an engine that is stopping does;
    /// `busy`/`engine_unreachable`, pointing at the engine's log, when an
    /// engine holds the channel and does not answer the hello within
    /// [`HELLO_PATIENCE`], or, on Windows, keeps every instance of the
    /// pipe taken for as long; `input`/`socket_path_too_long`, before any
    /// connection, when the socket's path is longer than the system takes;
    /// `busy`/`channel_unavailable`, with the
    /// system's error by its number, for anything else the operating system
    /// refused.
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
                codes::failure(
                    &codes::row::LOCAL_API_MISMATCH,
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
        // A socket of a path the system does not take cannot be reached:
        // told by name, and no engine is started for it.
        if let Some((socket, length, longest)) = self.place.too_long() {
            return Err(codes::socket_path_too_long(socket, length, longest));
        }
        // The same patience as the hello's: a pipe whose every instance is
        // taken is waited for no longer than an engine that does not answer.
        let deadline = tokio::time::Instant::now() + self.hello_patience;
        let stream = os::connect(&self.place, self.expected.as_str(), deadline)
            .await
            .map_err(|error| match error {
                ConnectError::NotRunning => codes::failure(
                    &codes::row::ENGINE_UNREACHABLE,
                    "No engine answers on the local channel.",
                    "Start the engine with `anything2mcp serve`, then run the command again.",
                ),
                ConnectError::NotOurs => codes::failure(
                    &codes::row::PEER_NOT_ALLOWED,
                    "What listens on the local channel does not belong to this user, so nothing was sent to it.",
                    "Check who owns the engine's runtime directory, remove what is not yours to trust, and start the engine again.",
                ),
                // Every instance of the pipe stayed taken: an engine holds
                // the channel and does not answer, as a stalled hello.
                #[cfg(windows)]
                ConnectError::Busy => hello_unanswered(&self.log),
                // The engine closed its channel with this connection made
                // and untaken: it is stopping, and it had nothing.
                #[cfg(unix)]
                ConnectError::Lost => hello_lost(&self.log),
                ConnectError::Io(error) => codes::unavailable("The local channel", &error),
            })?;
        Link::over(stream, self.version).await
    }
}

fn not_an_engine() -> Failure {
    codes::failure(
        &codes::row::HELLO_INVALID,
        "What answers on the local channel did not answer the hello as an engine does.",
        "Stop whatever holds the engine's channel, then run the command again.",
    )
}

/// One connection to the engine, after the hello.
#[derive(Debug)]
pub struct Link {
    sender: Mutex<SendRequest<Body>>,
    /// Turns true when the connection has ended.
    gone: watch::Receiver<Option<Seen>>,
    engine_version: Version,
    /// The engine's start variables, by name and digest, as its hello
    /// named them; `None` when it named none.
    start_variables: Option<BTreeMap<String, String>>,
}

/// What the client saw when a connection ended with no whole answer. The
/// list is closed: only these words ever reach a message, never the
/// library's own text, which could hold an address or a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Seen {
    Reset,
    ClosedBeforeAnswer,
    CutShort,
    WriteFailed,
    NotWellFormed,
    NoDetail,
}

impl Seen {
    pub(crate) const fn words(self) -> &'static str {
        match self {
            Self::Reset => "the connection was reset",
            Self::ClosedBeforeAnswer => "the connection closed before any answer",
            Self::CutShort => "the answer was cut short",
            Self::WriteFailed => "the write of the request failed",
            Self::NotWellFormed => "the answer was not well formed",
            Self::NoDetail => "no detail",
        }
    }

    /// By the kind of the system's I/O error.
    pub(crate) const fn of_io(kind: std::io::ErrorKind) -> Self {
        use std::io::ErrorKind;
        match kind {
            ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted => Self::Reset,
            ErrorKind::BrokenPipe => Self::WriteFailed,
            ErrorKind::UnexpectedEof => Self::CutShort,
            _ => Self::NoDetail,
        }
    }

    /// By the HTTP library's error: an I/O error under it first, then its
    /// own kinds. Its text is never read.
    pub(crate) fn of(error: &hyper::Error) -> Self {
        let mut source = std::error::Error::source(error);
        while let Some(inner) = source {
            if let Some(io) = inner.downcast_ref::<std::io::Error>() {
                return Self::of_io(io.kind());
            }
            source = inner.source();
        }
        if error.is_parse() || error.is_parse_status() || error.is_parse_too_large() {
            Self::NotWellFormed
        } else if error.is_incomplete_message() || error.is_closed() || error.is_canceled() {
            Self::ClosedBeforeAnswer
        } else {
            Self::NoDetail
        }
    }
}

impl Link {
    /// A link over a connection that is made: the HTTP handshake, and a
    /// task that tells the link when the connection ends and what was seen.
    pub(crate) async fn over<T>(stream: T, engine_version: Version) -> Result<Self, Failure>
    where
        T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let (sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|error| codes::lost_seeing(Seen::of(&error)))?;
        // The connection ends when the link is dropped, and this task with
        // it. When the connection ends first, the link is told, so that no
        // request waits on a connection that is gone.
        let (ended, gone) = watch::channel(None);
        tokio::spawn(async move {
            let seen = match connection.await {
                Ok(()) => Seen::ClosedBeforeAnswer,
                Err(error) => Seen::of(&error),
            };
            ended.send_replace(Some(seen));
        });
        Ok(Self {
            sender: Mutex::new(sender),
            gone,
            engine_version,
            start_variables: None,
        })
    }

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
            sender
                .ready()
                .await
                .map_err(|error| codes::lost_seeing(Seen::of(&error)))?;
            match sender.try_send_request(request).await {
                Ok(response) => Ok(response),
                // The library hands the request back when it never wrote
                // any of it; otherwise some of it may have reached the
                // engine.
                Err(mut error) => {
                    let seen = Seen::of(error.error());
                    Err(if error.take_message().is_some() {
                        codes::lost_seeing(seen)
                    } else {
                        codes::answer_lost(seen)
                    })
                }
            }
        };
        // An answer that is there is taken first. Otherwise the end of the
        // connection ends the wait: nothing can answer after it, and the
        // request may have been read before it ended.
        let response = tokio::select! {
            biased;
            answered = answered => answered?,
            ended = gone.wait_for(Option::is_some) => {
                let seen = ended.ok().and_then(|seen| *seen).unwrap_or(Seen::NoDetail);
                return Err(codes::answer_lost(seen));
            }
        };
        Ok(response.map(Body::new))
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests; clippy.toml allows them but the attribute keeps the module self-evident"
)]
mod tests {
    use std::io::ErrorKind;

    use a2m_core::FailureClass;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    use super::*;

    #[test]
    fn each_word_of_the_closed_list_comes_from_one_kind_of_what_the_client_saw() {
        assert_eq!(Seen::of_io(ErrorKind::ConnectionReset), Seen::Reset);
        assert_eq!(Seen::of_io(ErrorKind::ConnectionAborted), Seen::Reset);
        assert_eq!(Seen::of_io(ErrorKind::BrokenPipe), Seen::WriteFailed);
        assert_eq!(Seen::of_io(ErrorKind::UnexpectedEof), Seen::CutShort);
        assert_eq!(Seen::of_io(ErrorKind::PermissionDenied), Seen::NoDetail);
        let words: Vec<&str> = [
            Seen::Reset,
            Seen::ClosedBeforeAnswer,
            Seen::CutShort,
            Seen::WriteFailed,
            Seen::NotWellFormed,
            Seen::NoDetail,
        ]
        .map(Seen::words)
        .to_vec();
        assert_eq!(
            words,
            [
                "the connection was reset",
                "the connection closed before any answer",
                "the answer was cut short",
                "the write of the request failed",
                "the answer was not well formed",
                "no detail"
            ]
        );
    }

    /// A link to a peer that reads the request, then does `then` with its
    /// end of the connection.
    async fn link_to_peer<F>(then: F) -> Link
    where
        F: FnOnce(TcpStream) + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut peer, _) = listener.accept().await.unwrap();
            let mut seen = Vec::new();
            let mut chunk = [0_u8; 512];
            while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = peer.read(&mut chunk).await.unwrap();
                seen.extend_from_slice(&chunk[..n]);
            }
            then(peer);
        });
        let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        Link::over(stream, Version::new(0, 0, 0)).await.unwrap()
    }

    fn request() -> Request<Body> {
        Request::builder()
            .uri("/api/v1/tools")
            .body(Body::empty())
            .unwrap()
    }

    fn assert_lost(failure: &Failure, words: &str) {
        assert_eq!(failure.class(), FailureClass::PossibleCommit);
        assert_eq!(failure.code(), &codes::ANSWER_LOST);
        assert!(
            failure
                .message()
                .ends_with(&format!("What the client saw: {words}.")),
            "{}",
            failure.message()
        );
    }

    #[tokio::test]
    async fn a_peer_that_closes_without_answering_is_told_as_a_connection_closed_before_any_answer()
    {
        let link = link_to_peer(|peer| {
            drop(peer);
        })
        .await;
        let failure = link.send(request()).await.unwrap_err();
        assert_lost(&failure, "the connection closed before any answer");
    }

    #[tokio::test]
    async fn a_peer_that_resets_is_told_as_a_connection_reset() {
        let link = link_to_peer(|peer| {
            peer.set_zero_linger().unwrap();
            drop(peer);
        })
        .await;
        let failure = link.send(request()).await.unwrap_err();
        assert_lost(&failure, "the connection was reset");
    }

    #[tokio::test]
    async fn a_peer_that_answers_what_is_not_http_is_told_as_not_well_formed() {
        let link = link_to_peer(|mut peer| {
            tokio::spawn(async move {
                peer.write_all(b"not an answer\r\n\r\n").await.unwrap();
                tokio::time::sleep(Duration::from_secs(5)).await;
            });
        })
        .await;
        let failure = link.send(request()).await.unwrap_err();
        assert_lost(&failure, "the answer was not well formed");
    }
}
