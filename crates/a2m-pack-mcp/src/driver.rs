//! The driver of the mode `mcp`: one connection per surface and identity,
//! the server's tool list compared with the mapping's pins, and one call of
//! one mapped tool per step.
//!
//! It never retries, never reaches a tool the mapping does not name, and
//! says of every failure whether the call was sent.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use a2m_auth::{ChallengeScheme, CredentialSink, SessionLease};
use a2m_core::{Digest, RecordKey, Redact, Secret, Step, Value, Values};
use a2m_provider::{
    BoxFuture, Capture, Cleanup, Deadline, Driver, DriverHealth, Observed, PrepareContext,
    Prepared, StepFailure, StepInput, StepOutcome, SurfaceRef,
};
use serde_json::{Value as Json, json};

use crate::client::{Client, HttpLink, Limits, Link, Revision, ToolResult};
use crate::codes::{self, unsent};
use crate::json::{
    ToolDefinition, cannot_hold_free_text, canonical, definition_digest, digest_text, fits,
    from_json, parse_digest, to_json, unchecked_keywords,
};
use crate::mapping::{CallPayload, LOCAL_ADDRESS, Mapping, Section, SurfaceFields, ToolEntry};
use crate::names;

/// What a connection reaches, as the run's own surface states it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum FarEnd {
    /// A remote server, at the surface's address in the run's environment.
    Http { address: String },
    /// A local server program, as the surface declares it.
    Program {
        path: String,
        digest: Option<String>,
        args: Vec<String>,
        environment: BTreeMap<String, String>,
        pass_environment: Vec<String>,
    },
}

impl FarEnd {
    /// The far end the run's surface names, checked against what the run
    /// may reach. It is worked out at every `prepare`, whether or not a
    /// connection is open already, so an open connection never stands in
    /// for a check.
    fn of(cx: &PrepareContext<'_>, fields: &SurfaceFields) -> Result<Self, StepFailure> {
        let address = cx.surface.surface.address.as_str();
        if let Some(program) = fields.program.as_deref() {
            if address != LOCAL_ADDRESS {
                return Err(unsent(codes::plan_unreadable(
                    "a surface with a local server program must have the address `stdio`",
                )));
            }
            return Ok(Self::Program {
                path: program.to_owned(),
                digest: fields.digest.clone(),
                args: fields.args.clone(),
                environment: fields.environment.clone(),
                pass_environment: fields.pass_environment.clone(),
            });
        }
        if address.starts_with("https://") {
            return Err(unsent(codes::tls_not_enabled()));
        }
        if !address.starts_with("http://") {
            return Err(unsent(codes::plan_unreadable(
                "a surface of the kind `mcp` has an `http://` address, or the address `stdio` with a program",
            )));
        }
        if !cx.destinations.contains(address) {
            return Err(unsent(codes::not_granted(
                "The address of the upstream server is not among the addresses this run may reach.",
            )));
        }
        Ok(Self::Http {
            address: address.to_owned(),
        })
    }
}

/// Which connection: one surface of one target in one environment, for one
/// session at one epoch of its credentials, to one far end, in one protocol
/// revision. Two identities never share one, and a plan that names another
/// address, program, or revision is never served by the connection an
/// earlier plan opened.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ConnKey {
    target: String,
    environment: String,
    surface: String,
    /// The session and the epoch of its credentials; nothing for a surface
    /// with no session.
    session: Option<(String, u64)>,
    /// The address, or the program with its digest, arguments, and
    /// environment.
    far_end: FarEnd,
    /// The protocol revision the connection speaks.
    revision: &'static str,
}

impl ConnKey {
    /// Whether `other` is a connection of the same surface and session that
    /// this key replaces: its credentials are of another epoch, or it
    /// reaches another far end, or it speaks another revision.
    fn replaces(&self, other: &Self) -> bool {
        other != self
            && other.target == self.target
            && other.environment == self.environment
            && other.surface == self.surface
            && other.session.as_ref().map(|(session, _)| session)
                == self.session.as_ref().map(|(session, _)| session)
    }
}

/// One open connection.
struct Connection {
    client: Client,
    /// The digest of the local server program that runs, when there is one.
    program_digest: Option<Digest>,
}

impl Connection {
    /// Whether the program that runs is the one `far_end` pins: the digest
    /// the supervisor measured when it started the program is the digest
    /// the surface states. True for a remote server, and for a program the
    /// surface does not pin.
    fn runs_what(&self, far_end: &FarEnd) -> bool {
        match (far_end, &self.program_digest) {
            (
                FarEnd::Program {
                    digest: Some(pinned),
                    ..
                },
                Some(running),
            ) => parse_digest(pinned).as_ref() == Some(running),
            _ => true,
        }
    }
}

/// The open connections, by surface and identity.
#[derive(Default)]
pub(crate) struct Connections {
    open: tokio::sync::Mutex<BTreeMap<ConnKey, Arc<Connection>>>,
}

/// The server's tool list as it was last read.
#[derive(Debug, Clone)]
struct Listing {
    /// The tools in the order the server lists them.
    order: Vec<String>,
    tools: BTreeMap<String, ToolDefinition>,
    /// When it was read. It decides only whether the list is read again;
    /// it is never part of a result.
    read_at: tokio::time::Instant,
}

impl Listing {
    fn of(tools: Vec<ToolDefinition>) -> Self {
        let mut listing = Self {
            order: Vec::new(),
            tools: BTreeMap::new(),
            read_at: tokio::time::Instant::now(),
        };
        for tool in tools {
            if !listing.tools.contains_key(&tool.name) {
                listing.order.push(tool.name.clone());
            }
            listing.tools.insert(tool.name.clone(), tool);
        }
        listing
    }
}

/// How a mapped tool stands to its pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pin {
    Matches,
    Changed,
    Gone,
    Unpinned,
}

impl Pin {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Matches => "matches",
            Self::Changed => "changed",
            Self::Gone => "gone",
            Self::Unpinned => "unpinned",
        }
    }
}

fn pin_of(entry: &ToolEntry, listed: Option<&ToolDefinition>) -> Pin {
    match (entry.digest.as_deref().and_then(parse_digest), listed) {
        (None, _) => Pin::Unpinned,
        (Some(_), None) => Pin::Gone,
        (Some(pinned), Some(definition)) if definition_digest(definition) == pinned => Pin::Matches,
        (Some(_), Some(_)) => Pin::Changed,
    }
}

/// What `prepare` acquired for one run.
struct State {
    key: ConnKey,
    conn: Arc<Connection>,
    surface: String,
    mapping: Mapping,
    listing: Listing,
}

/// Collects what a session's provider places for an HTTP connection.
#[derive(Default)]
struct HeaderSink {
    headers: Vec<(String, Secret)>,
    /// Something was placed that an MCP connection has no place for.
    other: bool,
}

impl CredentialSink for HeaderSink {
    fn header(&mut self, name: &str, value: &Secret) {
        self.headers
            .push((name.to_owned(), Secret::new(value.expose())));
    }

    fn cookie(&mut self, _origin: &str, _name: &str, _value: &Secret) {
        self.other = true;
    }

    fn tls_identity(&mut self, _certificate: &[u8], _key: &Secret) {
        self.other = true;
    }

    fn challenge_credential(&mut self, _scheme: ChallengeScheme, _user: &str, _secret: &Secret) {
        self.other = true;
    }
}

/// The driver of the mode `mcp`.
pub struct McpDriver {
    conns: Connections,
    redact: Arc<dyn Redact>,
    limits: Limits,
}

impl std::fmt::Debug for McpDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpDriver").finish_non_exhaustive()
    }
}

impl McpDriver {
    pub(crate) fn new(redact: Arc<dyn Redact>, limits: Limits) -> Self {
        Self {
            conns: Connections::default(),
            redact,
            limits,
        }
    }

    /// Starts the surface's local server program through the supervisor and
    /// connects over its standard streams.
    #[cfg(unix)]
    async fn open_local(
        &self,
        cx: &PrepareContext<'_>,
        fields: &SurfaceFields,
        program: &str,
    ) -> Result<Connection, StepFailure> {
        use a2m_provider::{EnvAllowlist, ForeignPipe, LaunchSpec, ProgramRef, SubsystemKind};

        let spec = LaunchSpec {
            program: ProgramRef {
                path: program.into(),
                digest: fields.digest.as_deref().and_then(parse_digest),
            },
            args: fields.args.iter().map(Into::into).collect(),
            env: EnvAllowlist {
                pass: fields.pass_environment.clone(),
                set: fields
                    .environment
                    .iter()
                    .map(|(name, value)| (name.clone(), value.into()))
                    .collect(),
            },
            cwd: None,
            kind: SubsystemKind::new("mcp_server").map_err(unsent)?,
            place: None,
        };
        // The program's own protocol runs over its standard input and
        // output, so the two ends of the pipe are placed on descriptors 0
        // and 1.
        let pipe = ForeignPipe {
            child_reads_fd: 0,
            child_writes_fd: 1,
            windows_argument: String::new(),
        };
        let link = cx
            .supervisor
            .start_foreign(spec, pipe)
            .await
            .map_err(unsent)?;
        let program_digest = link.program_digest;
        let stdio = crate::client::StdioLink::new(
            link.to_child,
            link.from_child,
            Some((cx.supervisor.clone(), link.id)),
            self.limits,
        );
        let client = Client::connect(
            Link::Stdio(stdio),
            Revision::of(fields.revision.as_deref()),
            self.limits,
            cx.deadline,
        )
        .await?;
        Ok(Connection {
            client,
            program_digest: Some(program_digest),
        })
    }

    /// On a system where the supervisor's foreign link cannot be placed on
    /// a program's standard streams, a local server program is not started.
    #[cfg(not(unix))]
    async fn open_local(
        &self,
        _cx: &PrepareContext<'_>,
        _fields: &SurfaceFields,
        _program: &str,
    ) -> Result<Connection, StepFailure> {
        Err(unsent(codes::not_built(
            "start a local server program on this operating system: the supervisor's foreign link cannot be placed on a program's standard streams here",
        )))
    }

    async fn open(
        &self,
        cx: &PrepareContext<'_>,
        lease: Option<&SessionLease>,
        fields: &SurfaceFields,
        far_end: &FarEnd,
    ) -> Result<Connection, StepFailure> {
        let mut sink = HeaderSink::default();
        if let Some(lease) = lease {
            lease.apply(&mut sink).map_err(unsent)?;
        }
        if sink.other {
            return Err(unsent(codes::not_built(
                "use a credential that is not a request header on a connection to an upstream server",
            )));
        }
        let address = match far_end {
            FarEnd::Program { path, .. } => {
                if !sink.headers.is_empty() {
                    return Err(unsent(codes::not_built(
                        "hand a credential to a local server program: the supervisor has no private way to give one to a program on a foreign link",
                    )));
                }
                return self.open_local(cx, fields, path).await;
            }
            // `FarEnd::of` checked the address against the run's
            // destinations.
            FarEnd::Http { address } => address,
        };
        let link = HttpLink::new(address, sink.headers, self.limits)?;
        let client = Client::connect(
            Link::Http(link),
            Revision::of(fields.revision.as_deref()),
            self.limits,
            cx.deadline,
        )
        .await?;
        Ok(Connection {
            client,
            program_digest: None,
        })
    }

    /// The connection for `key`, reused while it lives, opened otherwise.
    /// A connection of the same surface and session under another key is
    /// closed, and its program with it: its credentials were replaced, or
    /// the plan now names another far end or revision.
    async fn connection(
        &self,
        key: &ConnKey,
        cx: &PrepareContext<'_>,
        lease: Option<&SessionLease>,
        fields: &SurfaceFields,
    ) -> Result<Arc<Connection>, StepFailure> {
        let mut open = self.conns.open.lock().await;
        let replaced: Vec<ConnKey> = open
            .keys()
            .filter(|other| key.replaces(other))
            .cloned()
            .collect();
        for old in replaced {
            if let Some(conn) = open.remove(&old) {
                conn.client.close().await;
            }
        }
        if let Some(conn) = open.get(key) {
            if !conn.client.lost() && conn.runs_what(&key.far_end) {
                return Ok(Arc::clone(conn));
            }
            if let Some(dead) = open.remove(key) {
                dead.client.close().await;
            }
        }
        let conn = Arc::new(self.open(cx, lease, fields, &key.far_end).await?);
        open.insert(key.clone(), Arc::clone(&conn));
        Ok(conn)
    }
}

impl State {
    fn pin(&self, tool: &str) -> Option<Pin> {
        self.mapping
            .tools
            .get(tool)
            .map(|entry| pin_of(entry, self.listing.tools.get(tool)))
    }

    /// What the `list_tools` step returns: the server's list as canonical
    /// JSON, with how each tool stands to the mapping.
    fn listing_values(&self) -> Values {
        let tools: Vec<Json> = self
            .listing
            .order
            .iter()
            .filter_map(|name| self.listing.tools.get(name))
            .map(|tool| {
                let mut unchecked = unchecked_keywords(&tool.input_schema);
                if let Some(output) = &tool.output_schema {
                    unchecked.extend(unchecked_keywords(output));
                }
                json!({
                    "name": tool.name,
                    "digest": digest_text(&definition_digest(tool)),
                    "read_only": tool.marked_read_only(),
                    "free_text": tool.takes_free_text(),
                    "unchecked_keywords": unchecked,
                    "input_schema": tool.input_schema,
                    "output_schema": tool.output_schema,
                    "annotations": tool.annotations,
                    "title": tool.title,
                    "description": tool.description,
                    "pin": self.pin(&tool.name).map_or("unmapped", Pin::as_str),
                })
            })
            .collect();
        let gone: Vec<&String> = self
            .mapping
            .tools
            .keys()
            .filter(|name| !self.listing.tools.contains_key(*name))
            .collect();
        let server = |name: &str, version: &str| json!({"name": name, "version": version});
        let listing = json!({
            "surface": self.surface,
            "server": self.conn.client.server.as_ref().map(|seen| server(&seen.name, &seen.version)),
            "mapped_server": self.mapping.server.as_ref().map(|seen| server(&seen.name, &seen.version)),
            "program_digest": self.conn.program_digest.as_ref().map(digest_text),
            "tools": tools,
            "gone": gone,
        });
        Values::new().with("listing", canonical(&listing))
    }
}

/// The earlier of the step's own timeout and the run's deadline.
fn step_deadline(step: &Step, input: &StepInput<'_>) -> Deadline {
    let own = Deadline::after(Duration::from_millis(u64::from(step.timeout_ms)));
    own.min(input.deadline)
}

/// The arguments of a call: each named value of the step, and the record's
/// stamp in the argument the mapping names for it. A value that is nothing
/// is left out.
fn arguments(entry: &ToolEntry, tool: &str, input: &StepInput<'_>) -> Result<Json, StepFailure> {
    let mut arguments = serde_json::Map::new();
    for (name, value) in input.args.iter() {
        if value.is_null() {
            continue;
        }
        let json = to_json(value).ok_or_else(|| {
            unsent(codes::arguments_invalid(
                tool,
                "a moment or a length of time has no form a tool takes without a rule",
            ))
        })?;
        arguments.insert(name.to_owned(), json);
    }
    if let (Some(argument), Some(stamp)) = (entry.stamp.as_deref(), input.stamp) {
        arguments.insert(argument.to_owned(), Json::String(stamp.to_owned()));
    }
    // The record's key, in the arguments the mapping names for it, where
    // the step gave none of its own.
    if let Some(key) = input.key {
        for (argument, part) in entry.key_args.iter().zip(key.parts()) {
            if !arguments.contains_key(argument)
                && let Some(json) = to_json(part)
            {
                arguments.insert(argument.clone(), json);
            }
        }
    }
    Ok(Json::Object(arguments))
}

/// The members of a structured result that are passed on: the ones the
/// mapping lists. A member is listed when the entry names it, under `key`,
/// `records`, or `untrusted_fields`, or when the pinned output schema
/// declares it as something that cannot hold free text: a number, a truth
/// value, nothing, or a value from a closed list. Any other member is left
/// out, so text the server wrote reaches a caller only through a member a
/// person listed.
///
/// `None` when the result does not hold what the mapping names: the member
/// under `records` is not a list of records, or a member has no exact form.
fn listed_members(
    entry: &ToolEntry,
    schema: Option<&Json>,
    members: &serde_json::Map<String, Json>,
) -> Option<Values> {
    if let Some(list) = entry.records.as_deref() {
        let records = members.get(list)?.as_array()?;
        if !records.iter().all(Json::is_object) {
            return None;
        }
    }
    let properties = schema
        .and_then(|schema| schema.get("properties"))
        .and_then(Json::as_object);
    let mut values = Values::new();
    for (name, member) in members {
        let named = entry.key.contains(name)
            || entry.records.as_deref() == Some(name.as_str())
            || entry.untrusted_fields.contains(name);
        let closed = properties
            .and_then(|properties| properties.get(name))
            .is_some_and(cannot_hold_free_text);
        if named || closed {
            values.insert(name.clone(), from_json(member)?);
        }
    }
    Some(values)
}

impl McpDriver {
    /// Turns a tool's result into what the step observed.
    fn observe(
        &self,
        step: &Step,
        tool: &str,
        entry: &ToolEntry,
        definition: &ToolDefinition,
        result: &ToolResult,
    ) -> Result<StepOutcome, StepFailure> {
        // A result that cannot be trusted says nothing certain about a
        // write that was sent: its fate is unknown.
        let doubt = |failure| {
            if step.commits {
                StepFailure::unknown(failure)
            } else {
                StepFailure::answered(failure)
            }
        };
        let capture = Capture {
            name: "result".to_owned(),
            media_type: "application/json".to_owned(),
            bytes: self.redact.redact(&canonical(&result.raw)).into_bytes(),
        };
        let observed = |values: Values, assigned_key: Option<RecordKey>| Observed {
            values: self.redact.redact_values(&values),
            assigned_key,
            for_reconcile: Values::new(),
            captures: vec![capture.clone()],
        };
        if result.is_error {
            let absent = step.lookup
                && entry.absent_error.as_deref().is_some_and(|prefix| {
                    result
                        .texts
                        .first()
                        .is_some_and(|text| text.starts_with(prefix))
                });
            return if absent {
                Ok(StepOutcome::Absent(observed(Values::new(), None)))
            } else {
                Err(StepFailure::answered(codes::rejected(tool)))
            };
        }
        let mut values = if entry.result == "text" {
            // The tool's own words, under one name. They are untrusted text
            // and the operation's field type says so. Nothing else of the
            // result is passed on.
            Values::new().with("text", result.texts.join("\n"))
        } else {
            let structured = match (&definition.output_schema, &result.structured) {
                (Some(schema), Some(structured)) if fits(schema, structured).is_ok() => {
                    Some(structured)
                }
                (Some(_), _) => return Err(doubt(codes::schema_mismatch(tool))),
                // No schema is pinned, so the mapping alone says what the
                // result holds.
                (None, structured) => structured.as_ref(),
            };
            let none = serde_json::Map::new();
            let members = match structured {
                None => &none,
                Some(Json::Object(members)) => members,
                Some(_) => return Err(doubt(codes::schema_mismatch(tool))),
            };
            listed_members(entry, definition.output_schema.as_ref(), members)
                .ok_or_else(|| doubt(codes::schema_mismatch(tool)))?
        };
        // A lookup through a tool that lists records takes the one record
        // in the list.
        if step.lookup
            && let Some(list) = entry.records.as_deref()
        {
            let Some(Value::List(records)) = values.get(list) else {
                return Err(doubt(codes::schema_mismatch(tool)));
            };
            match records.as_slice() {
                [] => return Ok(StepOutcome::Absent(observed(Values::new(), None))),
                [Value::Record(record)] => values = record.clone(),
                [_] => return Err(doubt(codes::schema_mismatch(tool))),
                more => return Err(StepFailure::answered(codes::ambiguous(tool, more.len()))),
            }
        }
        // The key the mapping says the result holds must be there, each
        // part text or a whole number.
        let assigned_key = if entry.key.is_empty() {
            None
        } else {
            let key = entry
                .key
                .iter()
                .map(|part| {
                    values
                        .get(part)
                        .filter(|value| matches!(value, Value::Text(_) | Value::Int(_)))
                        .cloned()
                })
                .collect::<Option<Vec<Value>>>()
                .and_then(|parts| RecordKey::new(parts).ok())
                .ok_or_else(|| doubt(codes::schema_mismatch(tool)))?;
            Some(key)
        };
        Ok(StepOutcome::Done(observed(values, assigned_key)))
    }

    async fn call(
        &self,
        state: &mut State,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        let call: CallPayload = step.payload.decode().map_err(unsent)?;
        let deadline = step_deadline(step, input);
        let refused = |why: &str| {
            unsent(codes::not_granted(format!(
                "The call to the upstream tool `{}` was refused before it was sent: {why}.",
                codes::shown(&call.tool)
            )))
        };
        // Only what the mapping names is reachable.
        if call.surface != state.surface {
            return Err(refused("the step belongs to another surface"));
        }
        let Some(entry) = state.mapping.tools.get(&call.tool).cloned() else {
            return Err(refused("the surface's mapping does not name the tool"));
        };
        if state.mapping.purpose == "discovery" {
            return Err(refused(
                "the surface's mapping is for discovery, and its tools are never called",
            ));
        }
        if entry.writes() && !step.commits {
            return Err(refused(
                "the tool is mapped as a write and the step is not one that commits",
            ));
        }
        // A tool the mapping says runs free text is never called, whatever
        // its input looks like.
        if entry.free_text == Some(true) {
            return Err(refused(
                "its mapping entry says, under `free_text`, that the server runs the text it is given",
            ));
        }
        // How fresh the list must be. A write is compared with a list read
        // for it, on every connection. A read is compared with the list
        // held: read again on the server's notice where the connection
        // gives notice, and when it is older than the setting allows where
        // it gives none.
        let notice = state.conn.client.take_list_changed();
        let again = step.commits
            || if state.conn.client.gives_notice() {
                notice
            } else {
                state.listing.read_at.elapsed() >= self.limits.list_max_age
            };
        if again {
            // Listing is a read. Whatever became of it, the call itself
            // has not been sent.
            let listed = state
                .conn
                .client
                .list_tools(deadline)
                .await
                .map_err(|failure| unsent(failure.failure))?;
            state.listing = Listing::of(listed);
        }
        match state.pin(&call.tool) {
            Some(Pin::Matches) => {}
            Some(Pin::Changed) => {
                return Err(unsent(codes::tool_changed(
                    &call.tool,
                    "has another definition than the one its mapping pins",
                )));
            }
            Some(Pin::Gone) => {
                return Err(unsent(codes::tool_changed(
                    &call.tool,
                    "is no longer on the server's list",
                )));
            }
            Some(Pin::Unpinned) | None => {
                return Err(refused("its mapping entry pins no digest"));
            }
        }
        let Some(definition) = state.listing.tools.get(&call.tool).cloned() else {
            return Err(unsent(codes::tool_changed(
                &call.tool,
                "is no longer on the server's list",
            )));
        };
        if definition.takes_free_text() && entry.free_text != Some(false) {
            return Err(refused(
                "its whole input is one string, and its mapping entry does not say, with `free_text: false`, that a person found the server does not run it",
            ));
        }
        let arguments = arguments(&entry, &call.tool, input)?;
        fits(&definition.input_schema, &arguments)
            .map_err(|why| unsent(codes::arguments_invalid(&call.tool, why)))?;
        let result = state
            .conn
            .client
            .call_tool(&call.tool, arguments, deadline)
            .await?;
        self.observe(step, &call.tool, &entry, &definition, &result)
    }
}

impl Driver for McpDriver {
    fn prepare<'a>(
        &'a self,
        cx: &'a PrepareContext<'a>,
        lease: Option<&'a SessionLease>,
    ) -> BoxFuture<'a, Result<Prepared, StepFailure>> {
        Box::pin(async move {
            let surface = &cx.surface.surface;
            if surface.kind != names::SURFACE_KIND {
                return Err(unsent(codes::plan_unreadable(
                    "the surface is not of the kind `mcp`",
                )));
            }
            let fields = SurfaceFields::of(surface).map_err(unsent)?;
            // Checked for every run, before any connection is taken: what
            // the surface names now, and whether this run may reach it.
            let far_end = FarEnd::of(cx, &fields)?;
            let mapping = Section::of(cx.plan)
                .map_err(unsent)?
                .surfaces
                .remove(surface.name.as_str())
                .unwrap_or_default();
            let key = ConnKey {
                target: cx.surface.target.as_str().to_owned(),
                environment: cx.surface.environment.as_str().to_owned(),
                surface: surface.name.as_str().to_owned(),
                session: lease.map(|lease| (lease.session().to_string(), lease.epoch())),
                far_end,
                revision: Revision::of(fields.revision.as_deref()).as_str(),
            };
            let conn = self.connection(&key, cx, lease, &fields).await?;
            // The list is read at every prepare, so a change the server
            // gave no notice of is found before the next call.
            let listing = Listing::of(conn.client.list_tools(cx.deadline).await?);
            conn.client.take_list_changed();
            Ok(Prepared::new(State {
                key,
                conn,
                surface: surface.name.as_str().to_owned(),
                mapping,
                listing,
            }))
        })
    }

    fn act<'a>(
        &'a self,
        prepared: &'a mut Prepared,
        step: &'a Step,
        input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        Box::pin(async move {
            let Some(state) = prepared.state::<State>() else {
                return Err(unsent(codes::plan_unreadable(
                    "the step was given what another driver prepared",
                )));
            };
            if step.kind == names::LIST_TOOLS {
                state.listing = Listing::of(
                    state
                        .conn
                        .client
                        .list_tools(step_deadline(step, input))
                        .await?,
                );
                state.conn.client.take_list_changed();
                return Ok(StepOutcome::Done(Observed {
                    values: state.listing_values(),
                    ..Observed::default()
                }));
            }
            self.call(state, step, input).await
        })
    }

    fn health(&self, surface: &SurfaceRef) -> DriverHealth {
        if surface.surface.address.starts_with("https://") {
            DriverHealth::Unavailable(codes::TLS_NOT_ENABLED)
        } else {
            DriverHealth::Ready
        }
    }

    fn release<'a>(&'a self, prepared: Prepared, poisoned: bool) -> BoxFuture<'a, Cleanup> {
        Box::pin(async move {
            let Ok(state) = prepared.into_state::<State>() else {
                return Cleanup::complete();
            };
            if !poisoned && !state.conn.client.lost() {
                // The connection stays open for the identity's next run.
                return Cleanup::complete();
            }
            let mut open = self.conns.open.lock().await;
            if open
                .get(&state.key)
                .is_some_and(|held| Arc::ptr_eq(held, &state.conn))
            {
                open.remove(&state.key);
            }
            drop(open);
            state.conn.client.close().await
        })
    }
}
