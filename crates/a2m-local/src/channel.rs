//! The engine's end of the local channel.
//!
//! [`Channel::bind`] takes the engine lock and binds the socket or the
//! pipe. [`Channel::serve`] carries HTTP/1.1 over it to the service the
//! product crate composed. Every connection's user is read from the
//! operating system before a byte is read from it, and a connection of
//! anyone else is closed. The first request on a connection is the hello;
//! every later request carries the [`Peer`] in its extensions.

use std::collections::BTreeMap;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use a2m_actions::{BoxFuture, CallerCredential, Paths};
use a2m_core::{Digest, Failure, Version};
use axum::Router;
use axum::extract::connect_info::Connected;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::serve::{IncomingStream, Listener};
use serde_json::json;
use tokio::sync::watch;

use crate::codes;
use crate::lock::EngineLock;
use crate::os;

/// The version of the local API: what a client and an engine must agree on
/// to talk at all. Raised when the hello or the framing changes.
pub const LOCAL_API: u32 = 1;
/// The path of the hello, the first request on every connection.
pub const HELLO_PATH: &str = "/local/hello";
/// The hello's header that carries the client's version.
pub const CLIENT_VERSION_HEADER: &str = "a2m-client-version";
/// The hello's header that carries the local API version the client speaks.
pub const LOCAL_API_HEADER: &str = "a2m-local-api";
/// The hello's header that carries the client label.
pub const CLIENT_LABEL_HEADER: &str = "a2m-client-label";

/// The longest name a client may give itself in its label.
const LABEL_NAME_LIMIT: usize = 64;
/// How long the accept loop pauses after the operating system refused to
/// hand over a connection, for example for want of file handles.
const ACCEPT_PAUSE: Duration = Duration::from_millis(100);

/// An operating-system user, as the text the system identifies it by: the
/// user id on macOS and Linux, the security identifier on Windows.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OsUser(String);

impl OsUser {
    /// The user this process runs as.
    ///
    /// # Errors
    ///
    /// `busy`/`channel_unavailable` when the operating system does not say.
    pub fn current() -> Result<Self, Failure> {
        os::current_user()
            .map(Self)
            .map_err(|error| codes::unavailable("The identity of this process's user", &error))
    }

    /// The user as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn named(text: &str) -> Self {
        Self(text.to_owned())
    }
}

impl std::fmt::Display for OsUser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Which client a connection is. It goes into the audit log and fair
/// queuing and nowhere else: every local client is the same principal.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClientLabel {
    /// The command line.
    CommandLine,
    /// An MCP client, by what it says of itself.
    Mcp(String),
    /// An SDK, by its language.
    Sdk(String),
}

fn clean_name(name: &str) -> Option<String> {
    let fits = !name.is_empty()
        && name.len() <= LABEL_NAME_LIMIT
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b' '));
    fits.then(|| name.to_owned())
}

impl ClientLabel {
    /// A label for an MCP client that calls itself `name`. Characters a
    /// label does not carry are replaced, and a long name is cut.
    #[must_use]
    pub fn mcp(name: &str) -> Self {
        Self::Mcp(Self::fit(name))
    }

    /// A label for the SDK of `language`.
    #[must_use]
    pub fn sdk(language: &str) -> Self {
        Self::Sdk(Self::fit(language))
    }

    fn fit(name: &str) -> String {
        let fitted: String = name
            .chars()
            .take(LABEL_NAME_LIMIT)
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ' ') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        if fitted.is_empty() {
            "unnamed".to_owned()
        } else {
            fitted
        }
    }

    /// The label as the hello carries it.
    #[must_use]
    pub fn as_text(&self) -> String {
        match self {
            Self::CommandLine => "cli".to_owned(),
            Self::Mcp(name) => format!("mcp:{name}"),
            Self::Sdk(name) => format!("sdk:{name}"),
        }
    }

    /// Reads a label from the hello. `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if text == "cli" {
            return Some(Self::CommandLine);
        }
        if let Some(name) = text.strip_prefix("mcp:") {
            return clean_name(name).map(Self::Mcp);
        }
        text.strip_prefix("sdk:")
            .and_then(clean_name)
            .map(Self::Sdk)
    }
}

/// Who is on the other end of a connection: the user the operating system
/// reported, its process when the system says, and the label the client
/// gave in its hello. The channel puts one into the extensions of every
/// request it hands to the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    /// The operating-system user.
    pub user: OsUser,
    /// The client's process.
    pub process: Option<u32>,
    /// Which client it says it is.
    pub label: ClientLabel,
}

impl Peer {
    /// The credential a front door hands to the kernel's one verifier for
    /// this peer. The channel checks who connected; the kernel decides who
    /// that is.
    #[must_use]
    pub fn credential(&self) -> CallerCredential {
        CallerCredential::LocalUser(self.user.as_str().to_owned())
    }
}

/// What keeps an idle engine open: runs, captures, a pending approval, a
/// hold of the session keeper. The product crate implements it over the
/// kernel and the person surface.
pub trait IdleHolds: Send + Sync {
    /// Whether something still needs the engine. It may read the store:
    /// a run in hand with nobody connected holds the engine open.
    fn busy(&self) -> BoxFuture<'_, bool>;
}

/// When an engine with no connections stops.
#[derive(Clone)]
pub struct IdlePolicy {
    /// How long the engine stays with no connection. `None` never stops.
    pub after: Option<Duration>,
    /// What keeps it open past that.
    pub holds: Arc<dyn IdleHolds>,
}

struct NoHolds;

impl IdleHolds for NoHolds {
    fn busy(&self) -> BoxFuture<'_, bool> {
        Box::pin(async { false })
    }
}

impl IdlePolicy {
    /// An engine that stops only when asked, as an installed service does.
    #[must_use]
    pub fn never() -> Self {
        Self {
            after: None,
            holds: Arc::new(NoHolds),
        }
    }

    /// An engine that stops `after` this long with no connection and
    /// nothing held.
    #[must_use]
    pub fn after(after: Duration, holds: Arc<dyn IdleHolds>) -> Self {
        Self {
            after: Some(after),
            holds,
        }
    }
}

impl std::fmt::Debug for IdlePolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdlePolicy")
            .field("after", &self.after)
            .finish_non_exhaustive()
    }
}

/// Why [`Channel::serve`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// No connection and nothing held for the idle time.
    Idle,
    /// A person or the operating system asked.
    Asked,
    /// A newer client asked this engine to make way.
    Restart,
    /// The listener failed.
    Failed,
    /// The data directory is no longer the one the engine opened: its
    /// engine lock is gone or is another file. The engine refuses work and
    /// stops, so that no second engine opens a new store beside it.
    DataGone,
}

/// A handle on a serving channel: stops it, and tells how many connections
/// it refused. Cheap to clone.
#[derive(Debug, Clone)]
pub struct ChannelControl {
    stop: watch::Sender<Option<StopReason>>,
    refused: Arc<AtomicU64>,
}

impl ChannelControl {
    /// Asks the channel to stop: it takes no new connection, lets the
    /// requests in flight end, and [`Channel::serve`] returns `why`. The
    /// first reason given stands.
    pub fn stop(&self, why: StopReason) {
        self.stop.send_if_modified(|reason| {
            if reason.is_none() {
                *reason = Some(why);
                true
            } else {
                false
            }
        });
    }

    /// How many connections were closed unread because their user was not
    /// the engine's.
    #[must_use]
    pub fn refused(&self) -> u64 {
        self.refused.load(Ordering::SeqCst)
    }
}

/// The engine's end of the local channel, bound and not yet serving. It
/// holds the engine lock until it is dropped; [`Channel::serve`] hands the
/// lock back, still held, when it returns.
pub struct Channel {
    listener: os::Listener,
    lock: EngineLock,
    expected: OsUser,
    version: Version,
    /// The engine's start variables, each by its name and the digest of
    /// its value, which the hello answers with.
    start_variables: BTreeMap<String, String>,
    control: ChannelControl,
}

/// The digest of one start variable, as the hello carries it: of its name
/// and its value together, so that one value under two names differs.
fn variable_digest(name: &str, value: &str) -> String {
    Digest::of(format!("{name}={value}").as_bytes()).to_string()
}

/// Each variable by its name and the digest of its value.
pub(crate) fn variable_digests(variables: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    variables
        .iter()
        .map(|(name, value)| (name.clone(), variable_digest(name, value)))
        .collect()
}

/// What the hello answers with.
#[derive(Clone)]
struct Hello {
    version: Version,
    start_variables: Arc<BTreeMap<String, String>>,
}

impl std::fmt::Debug for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Channel")
            .field("user", &self.expected)
            .finish_non_exhaustive()
    }
}

/// Where the channel of these paths is, for the user who would serve it.
pub(crate) fn place(paths: &Paths, user: &OsUser) -> os::Place {
    // The tag tells the pipes of two runtime directories apart; a socket
    // needs none, because its file is inside the directory.
    let tag = Digest::of(paths.runtime.to_string_lossy().as_bytes()).to_string();
    os::Place::new(paths.runtime.clone(), user.as_str(), &tag[..16])
}

impl Channel {
    /// Takes the engine lock, removes a stale socket, and binds the
    /// channel so that only this process's user can open it. The engine
    /// calls this before `Kernel::open`, so that it holds the lock before
    /// it opens the store, and serves only once the kernel is open.
    /// `version` is this program's, which the hello answers with.
    ///
    /// # Errors
    ///
    /// `input`/`socket_path_too_long`, before anything is made, when the
    /// socket's path is longer than the system takes;
    /// `busy`/`engine_running` when another engine holds the lock, or
    /// another process made the pipe first; `busy`/`channel_unavailable`
    /// when the directories, the lock files, or the channel cannot be
    /// opened.
    pub async fn bind(paths: &Paths, version: Version) -> Result<Self, Failure> {
        let user = OsUser::current()?;
        Self::bind_expecting(paths, version, user).await
    }

    /// As [`Channel::bind`], with the user whose connections are let in
    /// given. The engine always expects its own user; a test expects
    /// another, and so sees its own connections refused.
    pub(crate) async fn bind_expecting(
        paths: &Paths,
        version: Version,
        expected: OsUser,
    ) -> Result<Self, Failure> {
        let me = OsUser::current()?;
        let place = place(paths, &me);
        // Before anything is made: a socket of that path cannot be bound.
        if let Some((socket, length, longest)) = place.too_long() {
            return Err(codes::socket_path_too_long(socket, length, longest));
        }
        let paths = paths.clone();
        let bound = tokio::task::spawn_blocking(move || {
            let lock = EngineLock::take(&paths)?;
            let listener = os::Listener::bind(&place)
                .map_err(|error| codes::unavailable("The local channel", &error))?;
            Ok::<_, Failure>((lock, listener))
        })
        .await;
        let (lock, listener) = match bound {
            Ok(result) => result?,
            Err(_) => {
                return Err(codes::unavailable(
                    "The local channel",
                    &io::Error::other("the bind stopped"),
                ));
            }
        };
        let (stop, _) = watch::channel(None);
        Ok(Self {
            listener,
            lock,
            expected,
            version,
            start_variables: BTreeMap::new(),
            control: ChannelControl {
                stop,
                refused: Arc::new(AtomicU64::new(0)),
            },
        })
    }

    /// The same channel, whose hello names the variables the engine was
    /// started with that would change what it does, so that a client can
    /// tell whether its own differ (decision 0192). Each is answered as
    /// its name and the digest of its value, never the value; the channel
    /// does not know what any of them means.
    #[must_use]
    pub fn with_start_variables(mut self, variables: &BTreeMap<String, String>) -> Self {
        self.start_variables = variable_digests(variables);
        self
    }

    /// Where the engine lock of the data directory was taken, which the
    /// engine asks before each use of its data directory it can see.
    #[must_use]
    pub fn lock_place(&self) -> crate::LockPlace {
        self.lock.place()
    }

    /// The handle that stops this channel once it serves.
    #[must_use]
    pub fn control(&self) -> ChannelControl {
        self.control.clone()
    }

    /// Serves the service the product crate composed, until the channel is
    /// stopped or goes idle. Every connection's user is checked before a
    /// byte is read from it. The channel answers the hello itself; every
    /// other request reaches `service` with a [`Peer`] in its extensions,
    /// and only after its connection said hello.
    ///
    /// When this returns, the socket is gone and the engine lock is still
    /// held: it comes back to the caller, who drops it once the kernel has
    /// closed. Until then no second engine can bind, and so none opens the
    /// store while this one still writes to it.
    pub async fn serve(self, service: Router, idle: IdlePolicy) -> (StopReason, EngineLock) {
        let Self {
            listener,
            lock,
            expected,
            version,
            start_variables,
            control,
        } = self;
        let (open, open_seen) = watch::channel(0_usize);
        let accepting = Accepting {
            os: listener,
            expected,
            open,
            refused: Arc::clone(&control.refused),
        };
        let hello = Hello {
            version,
            start_variables: Arc::new(start_variables),
        };
        let app = service.layer(axum::middleware::from_fn_with_state(hello, front));
        let mut asked = control.stop.subscribe();
        let stop = control.stop.clone();
        let signal = async move {
            tokio::select! {
                _ = asked.wait_for(Option::is_some) => {}
                () = idle_stop(open_seen, idle) => {
                    stop.send_if_modified(|reason| {
                        if reason.is_none() {
                            *reason = Some(StopReason::Idle);
                        }
                        true
                    });
                }
            }
        };
        let served = axum::serve(accepting, app.into_make_service_with_connect_info::<Seat>())
            .with_graceful_shutdown(signal)
            .await;
        // The listener is gone with the serve. The lock goes back to the
        // caller, held, so that it outlives the kernel.
        let why = match (served, *control.stop.borrow()) {
            (Ok(()), Some(reason)) => reason,
            _ => StopReason::Failed,
        };
        (why, lock)
    }
}

/// Resolves when the engine has had no connection for the idle time and
/// nothing holds it. Never resolves for a policy that never stops.
async fn idle_stop(mut open: watch::Receiver<usize>, idle: IdlePolicy) {
    let Some(after) = idle.after else {
        return std::future::pending().await;
    };
    loop {
        if open.wait_for(|count| *count == 0).await.is_err() {
            return std::future::pending().await;
        }
        tokio::select! {
            () = tokio::time::sleep(after) => {
                if *open.borrow() == 0 && !idle.holds.busy().await && *open.borrow() == 0 {
                    return;
                }
            }
            changed = open.changed() => {
                if changed.is_err() {
                    return std::future::pending().await;
                }
            }
        }
    }
}

/// One open connection. When the last copy is dropped, the connection and
/// every request on it have ended.
struct Open(watch::Sender<usize>);

impl Drop for Open {
    fn drop(&mut self) {
        self.0.send_modify(|count| *count = count.saturating_sub(1));
    }
}

/// What the channel knows of one connection.
#[derive(Clone)]
struct Seat {
    user: OsUser,
    process: Option<u32>,
    label: Arc<OnceLock<ClientLabel>>,
    _open: Arc<Open>,
}

impl std::fmt::Debug for Seat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Seat")
            .field("user", &self.user)
            .field("process", &self.process)
            .finish_non_exhaustive()
    }
}

struct Accepting {
    os: os::Listener,
    expected: OsUser,
    open: watch::Sender<usize>,
    refused: Arc<AtomicU64>,
}

impl Listener for Accepting {
    type Io = os::ServerStream;
    type Addr = Seat;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            match self.os.accept().await {
                Ok((stream, peer)) => {
                    if peer.user.as_deref() == Some(self.expected.as_str()) {
                        self.open.send_modify(|count| *count += 1);
                        let seat = Seat {
                            user: self.expected.clone(),
                            process: peer.process,
                            label: Arc::new(OnceLock::new()),
                            _open: Arc::new(Open(self.open.clone())),
                        };
                        return (stream, seat);
                    }
                    // Another user's connection: closed here, unread.
                    self.refused.fetch_add(1, Ordering::SeqCst);
                    drop(stream);
                }
                Err(_) => {
                    // The system could not hand a connection over. This is
                    // a pause before asking again, not a wait for a state.
                    tokio::time::sleep(ACCEPT_PAUSE).await;
                }
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Err(io::Error::other(
            "the local channel has no address of this kind",
        ))
    }
}

impl Connected<IncomingStream<'_, Accepting>> for Seat {
    fn connect_info(stream: IncomingStream<'_, Accepting>) -> Self {
        stream.remote_addr().clone()
    }
}

/// A refusal of the channel itself, as its four parts and nothing else.
pub(crate) fn refusal(status: StatusCode, failure: &Failure) -> Response {
    let body = json!({
        "class": failure.class().to_string(),
        "code": failure.code().to_string(),
        "message": failure.message(),
        "fix": failure.next_step(),
    });
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

fn header_text<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn hello_invalid(why: &str) -> Response {
    refusal(
        StatusCode::BAD_REQUEST,
        &codes::input(
            codes::HELLO_INVALID,
            format!("The connection did not open with a well-formed hello: {why}."),
            "Reach the engine through `anything2mcp` or its SDK bridge, which say hello first.",
        ),
    )
}

/// What stands in front of the composed service: the hello, and the peer.
async fn front(
    State(hello): State<Hello>,
    ConnectInfo(seat): ConnectInfo<Seat>,
    mut request: Request,
    next: Next,
) -> Response {
    if request.uri().path() == HELLO_PATH {
        let headers = request.headers();
        let Some(_client) = header_text(headers, CLIENT_VERSION_HEADER)
            .and_then(|text| text.parse::<Version>().ok())
        else {
            return hello_invalid("the client's version is missing or is not a version");
        };
        let Some(api) =
            header_text(headers, LOCAL_API_HEADER).and_then(|text| text.parse::<u32>().ok())
        else {
            return hello_invalid("the local API version is missing or is not a number");
        };
        let Some(label) = header_text(headers, CLIENT_LABEL_HEADER).and_then(ClientLabel::parse)
        else {
            return hello_invalid("the client label is missing or is not a label");
        };
        if api != LOCAL_API {
            return refusal(
                StatusCode::BAD_REQUEST,
                &codes::input(
                    codes::LOCAL_API_MISMATCH,
                    format!(
                        "The client speaks local API version {api} and the engine speaks {LOCAL_API}."
                    ),
                    "Install the same version of the client and the engine, then run the command again.",
                ),
            );
        }
        // The first hello names the connection; a second changes nothing.
        let _ = seat.label.set(label);
        let body = json!({
            "engine_version": hello.version.to_string(),
            "local_api": LOCAL_API,
            "start_variables": *hello.start_variables,
        });
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            body.to_string(),
        )
            .into_response();
    }
    let Some(label) = seat.label.get().cloned() else {
        return hello_invalid("its first request was not the hello");
    };
    request.extensions_mut().insert(Peer {
        user: seat.user,
        process: seat.process,
        label,
    });
    next.run(request).await
}
