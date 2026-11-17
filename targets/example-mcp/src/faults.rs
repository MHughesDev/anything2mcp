//! Faults and settings a test can switch on, so that a failure is reproduced
//! on demand and never waited for.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// How long the slow tool waits unless a test says otherwise.
pub const DEFAULT_SLOW_MS: u64 = 200;
/// How many tools one page of the tool list holds unless a test says otherwise.
/// Smaller than the list on purpose: a client that ignores the cursor sees
/// only the first page.
pub const DEFAULT_PAGE_SIZE: usize = 3;
/// The version the server reports unless a test says otherwise.
pub const DEFAULT_SERVER_VERSION: &str = "1.0.0";

/// Which broken answer the `malformed` fault sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Malformed {
    /// Text that stops in the middle of a JSON object.
    NotJson,
    /// A JSON object with the request's id and neither a result nor an error.
    NotAResponse,
}

/// What a fault does to the request it applies to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FaultKind {
    /// Do the work and answer correctly, after waiting this long.
    Delay {
        /// Milliseconds to wait before answering.
        ms: u64,
    },
    /// Drop the connection before doing anything. Nothing is stored and no
    /// call is recorded.
    DropBeforeHandling,
    /// Do the work, so a write is committed, then drop the connection without
    /// answering. Over stdio the program exits; over HTTP the socket closes.
    DropAfterCommit,
    /// Do the work, then answer with something that is not a response.
    Malformed {
        /// Which broken answer to send.
        shape: Malformed,
    },
    /// Do nothing and answer with this JSON-RPC error.
    ProtocolError {
        /// The error code.
        code: i64,
        /// The error message.
        message: String,
    },
    /// Do the work and add a text block holding `INSTRUCTION_TEXT` to the tool
    /// result. Applies to `tools/call` only.
    InjectText,
    /// Do the work and answer with structured content that does not fit the
    /// tool's output schema. Applies to `tools/call` only.
    ResultOffSchema,
    /// Do nothing and answer with this HTTP status. Applies over HTTP only.
    HttpStatus {
        /// The HTTP status to answer with.
        status: u16,
    },
}

/// One fault and the requests it applies to. A fault applies to requests,
/// never to notifications, and never to the control interface.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fault {
    /// What happens to a matching request.
    #[serde(flatten)]
    pub kind: FaultKind,
    /// Only requests with this method are affected. All are, if absent.
    #[serde(default)]
    pub method: Option<String>,
    /// Only calls to this tool are affected.
    #[serde(default)]
    pub tool: Option<String>,
    /// How many requests it applies to before it clears itself. Until cleared, if absent.
    #[serde(default)]
    pub times: Option<u32>,
}

impl Fault {
    /// A fault that affects the next matching request only.
    #[must_use]
    pub fn once(kind: FaultKind) -> Self {
        Self {
            kind,
            method: None,
            tool: None,
            times: Some(1),
        }
    }

    /// A fault that stays on until it is cleared.
    #[must_use]
    pub fn until_cleared(kind: FaultKind) -> Self {
        Self {
            times: None,
            ..Self::once(kind)
        }
    }

    /// The same fault, limited to requests with this method, such as `tools/list`.
    #[must_use]
    pub fn on_method(mut self, method: &str) -> Self {
        self.method = Some(method.to_owned());
        self
    }

    /// The same fault, limited to calls of this tool.
    #[must_use]
    pub fn on_tool(mut self, tool: &str) -> Self {
        self.method = Some("tools/call".to_owned());
        self.tool = Some(tool.to_owned());
        self
    }
}

/// One change to the tool list, as an upstream server makes when it is
/// updated. A change is silent: the stand sends the notification that its
/// list changed only when the control interface asks it to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Drift {
    /// The tool is no longer listed, and a call to it is refused as unknown.
    ToolRemoved {
        /// The tool that is gone.
        tool: String,
    },
    /// The tool's input schema gains a required string property `reason`,
    /// and a call without it gets an error result.
    InputSchemaChanged {
        /// The tool whose input schema changed.
        tool: String,
    },
    /// The tool's output schema gains a required property `region`. Results
    /// stay as they were.
    OutputSchemaChanged {
        /// The tool whose output schema changed.
        tool: String,
    },
    /// The tool's read-only marking is turned around.
    AnnotationsChanged {
        /// The tool whose annotations changed.
        tool: String,
    },
    /// `INSTRUCTION_TEXT` is added to the tool's description. Nothing else changes.
    DescriptionChanged {
        /// The tool whose description changed.
        tool: String,
    },
    /// A tool named `orders_archive` appears at the end of the list.
    ToolAdded,
}

/// Everything a test can set. The default is a stand with no fault.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// How long the slow tool waits, in milliseconds.
    pub slow_ms: u64,
    /// How many tools one page of the tool list holds; 0 puts all on one page.
    pub page_size: usize,
    /// The version the server reports in `initialize`.
    pub server_version: String,
    /// Answer an `initialize` for a revision the stand does not serve with an
    /// error, in place of offering the revision it does serve.
    pub refuse_unknown_revision: bool,
    /// Made-up bearer tokens the stand accepts, each with the name of the
    /// principal it stands for. Empty means no credential is asked for.
    pub tokens: BTreeMap<String, String>,
    /// The faults switched on, in the order they were set.
    pub faults: Vec<Fault>,
    /// The changes made to the tool list.
    pub drift: Vec<Drift>,
    /// How many times a test asked for the notification that the list changed.
    pub announce: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            slow_ms: DEFAULT_SLOW_MS,
            page_size: DEFAULT_PAGE_SIZE,
            server_version: DEFAULT_SERVER_VERSION.to_owned(),
            refuse_unknown_revision: false,
            tokens: BTreeMap::new(),
            faults: Vec::new(),
            drift: Vec::new(),
            announce: 0,
        }
    }
}
