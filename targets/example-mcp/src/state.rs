//! Everything one stand holds: its settings, the ground truth it reports, and
//! where both are kept. The HTTP form keeps them in memory; the stdio form
//! keeps them in a state folder, so that a restarted program still holds what
//! an earlier one committed and the harness can read it from outside.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::faults::{Fault, FaultKind, Settings};
use crate::store::{Store, WriteRecord};

const STATE_FILE: &str = "state.json";
const STATE_FILE_NEW: &str = "state.json.new";
const LOCK_FILE: &str = "lock";

/// How a connection reaches the stand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    /// One started program, speaking on its standard input and output.
    Stdio,
    /// The HTTP endpoint.
    Http,
}

/// What the stand did with a tool call it received.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallOutcome {
    /// The tool ran and its result was a success.
    Answered,
    /// The tool ran and its result was an error result. Nothing was stored.
    ErrorResult,
    /// No such tool is listed. Nothing ran.
    UnknownTool,
}

/// One `tools/call` request the stand received.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CallRecord {
    /// Counted from 1 since the last reset.
    pub seq: u64,
    /// The connection it arrived on: `ConnectionRecord::id`.
    pub connection: String,
    /// The principal whose credential came with it, if one was asked for.
    pub principal: Option<String>,
    /// The tool named.
    pub tool: String,
    /// The arguments exactly as received.
    pub arguments: Value,
    /// What the stand did.
    pub outcome: CallOutcome,
    /// The fault that applied. The outcome says what the stand did; the fault
    /// says what became of the answer.
    pub fault: Option<FaultKind>,
}

/// One connection the stand has seen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionRecord {
    /// `stdio-N` for a started program, `session-N` for an HTTP session, and
    /// `stateless` for HTTP requests that carry no session.
    pub id: String,
    /// How it reaches the stand.
    pub transport: Transport,
    /// The protocol revision it speaks, once known.
    pub revision: Option<String>,
    /// Every principal whose credential arrived on it. More than one means a
    /// connection was shared between identities.
    pub principals: BTreeSet<String>,
    /// How many messages arrived on it.
    pub messages: u64,
    /// False once the client ended it: the program's input closed, or the
    /// session was deleted.
    pub open: bool,
}

/// One fault that applied to a request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FiredFault {
    /// What it did.
    pub kind: FaultKind,
    /// The method of the request it applied to.
    pub method: String,
    /// The tool, for a call.
    pub tool: Option<String>,
}

/// The ground truth: what the stand received and what it stored.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Truth {
    /// What is stored.
    pub store: Store,
    /// Every tool call received, oldest first.
    pub calls: Vec<CallRecord>,
    /// Every stored write, oldest first.
    pub writes: Vec<WriteRecord>,
    /// How many messages of each method reached the protocol layer.
    pub requests: BTreeMap<String, u64>,
    /// Every connection seen.
    pub connections: Vec<ConnectionRecord>,
    /// Every fault that applied, oldest first.
    pub fired: Vec<FiredFault>,
    /// How many requests were refused for a missing or unknown credential.
    pub unauthorized: u64,
    /// How many notifications that the tool list changed were sent.
    pub announcements: u64,
    /// Every start of the stdio program over this state folder, oldest
    /// first, as the program itself saw it.
    #[serde(default)]
    pub starts: Vec<StartRecord>,
}

/// One start of the stdio program, as the program saw itself: what a test
/// asks to prove what the program was given, never what the engine says it
/// gave.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartRecord {
    /// Its process identifier.
    pub process: u32,
    /// Its arguments after its own name, element for element, as text.
    pub args: Vec<String>,
    /// The names of every variable in its environment.
    pub env: Vec<String>,
    /// The process identifier of the process it started on purpose, with
    /// `--grandchild`, which lingers until it is ended.
    pub grandchild: Option<u32>,
}

/// Where a connection is in the handshake.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    /// Nothing agreed yet.
    New,
    /// `initialize` was answered; the client has not yet confirmed.
    Initialized,
    /// The client sent `notifications/initialized`.
    Ready,
}

/// What the stand remembers about one connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Conn {
    pub(crate) id: Option<String>,
    pub(crate) transport: Transport,
    pub(crate) phase: Phase,
    /// The value of `Settings::announce` this connection last acted on.
    pub(crate) announced: u64,
}

impl Conn {
    pub(crate) fn new(transport: Transport) -> Self {
        Self {
            id: None,
            transport,
            phase: Phase::New,
            announced: 0,
        }
    }
}

/// Settings, ground truth, and the HTTP sessions.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct State {
    pub(crate) settings: Settings,
    pub(crate) truth: Truth,
    pub(crate) sessions: BTreeMap<String, Conn>,
}

/// Whether a request's credential is accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Credential {
    /// The stand asks for none.
    NotAsked,
    /// The token stands for this principal.
    Principal(String),
    /// One is asked for and none came.
    Missing,
    /// One came and the stand does not know it.
    Refused,
}

impl Credential {
    pub(crate) fn principal(&self) -> Option<&str> {
        match self {
            Self::Principal(name) => Some(name),
            _ => None,
        }
    }
}

impl State {
    pub(crate) fn credential(&self, token: Option<&str>) -> Credential {
        if !self.settings.credential_required {
            return Credential::NotAsked;
        }
        match token {
            None => Credential::Missing,
            Some(token) => match self.settings.tokens.get(token) {
                Some(principal) => Credential::Principal(principal.clone()),
                None => Credential::Refused,
            },
        }
    }

    /// Gives the connection an identifier if it has none, and a record.
    pub(crate) fn register(&mut self, conn: &mut Conn, prefix: &str) -> String {
        let id = conn
            .id
            .clone()
            .unwrap_or_else(|| format!("{prefix}-{}", self.truth.connections.len() + 1));
        conn.id = Some(id.clone());
        self.record(&id, conn.transport);
        id
    }

    /// The record of the connection with this identifier, made if it is missing.
    pub(crate) fn record(&mut self, id: &str, transport: Transport) -> &mut ConnectionRecord {
        let connections = &mut self.truth.connections;
        let index = match connections.iter().position(|c| c.id == id) {
            Some(index) => index,
            None => {
                connections.push(ConnectionRecord {
                    id: id.to_owned(),
                    transport,
                    revision: None,
                    principals: BTreeSet::new(),
                    messages: 0,
                    open: true,
                });
                connections.len() - 1
            }
        };
        &mut connections[index]
    }

    /// The first fault that applies to this request, used up by one.
    pub(crate) fn take_fault(
        &mut self,
        method: &str,
        tool: Option<&str>,
        transport: Transport,
    ) -> Option<FaultKind> {
        let applies = |fault: &Fault| {
            let kind_fits = match fault.kind {
                FaultKind::InjectText
                | FaultKind::ResultOffSchema
                | FaultKind::ResultStructured { .. } => method == "tools/call",
                FaultKind::HttpStatus { .. } => transport == Transport::Http,
                _ => true,
            };
            kind_fits
                && fault.method.as_deref().is_none_or(|m| m == method)
                && fault.tool.as_deref().is_none_or(|t| Some(t) == tool)
        };
        let faults = &mut self.settings.faults;
        let index = faults.iter().position(applies)?;
        let kind = faults[index].kind.clone();
        if let Some(times) = faults[index].times.as_mut() {
            *times = times.saturating_sub(1);
            if *times == 0 {
                faults.remove(index);
            }
        }
        self.truth.fired.push(FiredFault {
            kind: kind.clone(),
            method: method.to_owned(),
            tool: tool.map(str::to_owned),
        });
        Some(kind)
    }
}

/// Where a stand's state is kept.
#[derive(Debug)]
pub(crate) enum Backing {
    /// In this process. The HTTP form.
    Memory(Box<Mutex<State>>),
    /// In a folder: `state.json`, guarded by a lock on the file `lock`, so
    /// that several programs and the harness can share it.
    Folder(PathBuf),
}

impl Backing {
    pub(crate) fn memory() -> Self {
        Self::Memory(Box::new(Mutex::new(State::default())))
    }

    /// Runs `change` on the state, alone, and keeps what it changed.
    pub(crate) fn with<R>(&self, change: impl FnOnce(&mut State) -> R) -> io::Result<R> {
        match self {
            Self::Memory(state) => {
                // A test that panicked while holding the state left it whole:
                // every change is finished before the lock is released.
                let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
                Ok(change(&mut state))
            }
            Self::Folder(folder) => {
                fs::create_dir_all(folder)?;
                let lock: File = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(folder.join(LOCK_FILE))?;
                lock.lock()?;
                let path = folder.join(STATE_FILE);
                let before = match fs::read(&path) {
                    Ok(bytes) => Some(bytes),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                    Err(e) => return Err(e),
                };
                let mut state = match &before {
                    Some(bytes) => serde_json::from_slice(bytes).map_err(io::Error::other)?,
                    None => State::default(),
                };
                let result = change(&mut state);
                let after = serde_json::to_vec_pretty(&state).map_err(io::Error::other)?;
                if before.as_deref() != Some(after.as_slice()) {
                    // Written beside and moved into place, so that a program
                    // stopped in the middle never leaves half a file.
                    let new = folder.join(STATE_FILE_NEW);
                    fs::write(&new, &after)?;
                    if fs::rename(&new, &path).is_err() {
                        // Some systems refuse to replace a file that a
                        // scanner has open. Writing in place is as good while
                        // the lock is held.
                        fs::write(&path, &after)?;
                        let _ = fs::remove_file(&new);
                    }
                }
                // The lock is released when `lock` is dropped here.
                Ok(result)
            }
        }
    }
}
