//! The stand's own tests. A small client, written here, speaks to the real
//! program over its standard input and output and to the real endpoint over
//! HTTP, and every write is proven by asking the control interface what the
//! stand stored.

use std::error::Error;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::time::{Duration, Instant};

use example_mcp::control::Control;
use example_mcp::faults::{Drift, Fault, FaultKind, Malformed};
use example_mcp::harness::{HttpStand, StdioStand, stdio_program};
use example_mcp::protocol::{
    CREDENTIAL_REFUSED, HANDSHAKE_REVISION, INVALID_PARAMS, INVALID_REQUEST, METHOD_NOT_FOUND,
    PARSE_ERROR, STATELESS_REVISION, STATELESS_VERSION_META_KEY,
};
use example_mcp::state::{CallOutcome, Transport};
use example_mcp::stdio::EXIT_DROPPED;
use example_mcp::store::{INSTRUCTION_TEXT, OrderStatus, WriteAction};
use example_mcp::tools::{ADDED_TOOL, TOOLS};
use reqwest::StatusCode;
use serde_json::{Value, json};

type Outcome = Result<(), Box<dyn Error>>;

// ---- a client over stdio ---------------------------------------------------

struct StdioClient {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    next_id: u64,
    /// Messages read while waiting for another: notifications, late answers.
    others: Vec<Value>,
}

impl StdioClient {
    fn start(stand: &StdioStand, token: Option<&str>) -> Result<Self, Box<dyn Error>> {
        let mut command = stand.command();
        if let Some(token) = token {
            command.envs(stand.env_with_token(token));
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().ok_or("no standard output")?);
        Ok(Self {
            child,
            input,
            output,
            next_id: 1,
            others: Vec::new(),
        })
    }

    fn send(&mut self, message: &Value) -> Outcome {
        let input = self.input.as_mut().ok_or("the input is closed")?;
        writeln!(input, "{message}")?;
        input.flush()?;
        Ok(())
    }

    /// The next line the program wrote, or `None` once it closed its output.
    fn read_line(&mut self) -> Result<Option<String>, Box<dyn Error>> {
        let mut line = String::new();
        match self.output.read_line(&mut line)? {
            0 => Ok(None),
            _ => Ok(Some(line.trim_end().to_owned())),
        }
    }

    fn read(&mut self) -> Result<Value, Box<dyn Error>> {
        let line = self.read_line()?.ok_or("the program closed its output")?;
        Ok(serde_json::from_str(&line)?)
    }

    fn post(&mut self, method: &str, params: Value) -> Result<u64, Box<dyn Error>> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        Ok(id)
    }

    /// Sends a request and reads until its answer arrives.
    fn request(&mut self, method: &str, params: Value) -> Result<Value, Box<dyn Error>> {
        let id = self.post(method, params)?;
        loop {
            let message = self.read()?;
            if message["id"] == id {
                return Ok(message);
            }
            self.others.push(message);
        }
    }

    fn notify(&mut self, method: &str) -> Outcome {
        self.send(&json!({"jsonrpc": "2.0", "method": method}))
    }

    fn handshake(&mut self) -> Result<Value, Box<dyn Error>> {
        let answer = self.request("initialize", initialize_params(HANDSHAKE_REVISION))?;
        self.notify("notifications/initialized")?;
        Ok(answer)
    }

    fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, Box<dyn Error>> {
        self.request("tools/call", json!({"name": tool, "arguments": arguments}))
    }

    /// Every tool, following the cursor from page to page.
    fn tools(&mut self) -> Result<Vec<Value>, Box<dyn Error>> {
        let mut tools = Vec::new();
        let mut cursor = Value::Null;
        loop {
            let page = self.request("tools/list", json!({"cursor": cursor}))?;
            tools.extend(
                page["result"]["tools"]
                    .as_array()
                    .ok_or("no tools")?
                    .clone(),
            );
            cursor = page["result"]["nextCursor"].clone();
            if cursor.is_null() {
                return Ok(tools);
            }
        }
    }

    /// Closes the input and waits for the program to end.
    fn finish(mut self) -> Result<Option<i32>, Box<dyn Error>> {
        self.input = None;
        Ok(self.child.wait()?.code())
    }
}

impl Drop for StdioClient {
    fn drop(&mut self) {
        self.input = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn initialize_params(revision: &str) -> Value {
    json!({
        "protocolVersion": revision,
        "capabilities": {},
        "clientInfo": {"name": "stand-test", "version": "0"}
    })
}

fn tool<'a>(tools: &'a [Value], name: &str) -> Result<&'a Value, Box<dyn Error>> {
    let found = tools.iter().find(|tool| tool["name"] == name);
    Ok(found.ok_or_else(|| format!("{name} is not listed"))?)
}

fn names(tools: &[Value]) -> Vec<&str> {
    tools.iter().filter_map(|t| t["name"].as_str()).collect()
}

// ---- a client over HTTP ----------------------------------------------------

struct HttpClient {
    http: reqwest::Client,
    url: String,
    session: Option<String>,
    token: Option<String>,
    revision: Option<String>,
    next_id: u64,
}

impl HttpClient {
    fn new(stand: &HttpStand) -> Self {
        Self {
            http: reqwest::Client::new(),
            url: stand.url(),
            session: None,
            token: None,
            revision: None,
            next_id: 1,
        }
    }

    fn with_token(stand: &HttpStand, token: &str) -> Self {
        Self {
            token: Some(token.to_owned()),
            ..Self::new(stand)
        }
    }

    async fn send(&self, message: &Value) -> Result<reqwest::Response, reqwest::Error> {
        let mut request = self
            .http
            .post(&self.url)
            .header("accept", "application/json, text/event-stream")
            .json(message);
        if let Some(session) = &self.session {
            request = request.header("mcp-session-id", session);
        }
        if let Some(revision) = &self.revision {
            request = request.header("mcp-protocol-version", revision);
        }
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        request.send().await
    }

    fn message(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
    }

    async fn request(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<(StatusCode, Value), Box<dyn Error>> {
        let message = self.message(method, params);
        let response = self.send(&message).await?;
        Ok((response.status(), response.json().await?))
    }

    async fn handshake(&mut self) -> Result<Value, Box<dyn Error>> {
        let message = self.message("initialize", initialize_params(HANDSHAKE_REVISION));
        let response = self.send(&message).await?;
        let session = response.headers().get("mcp-session-id");
        self.session = Some(session.ok_or("no session")?.to_str()?.to_owned());
        let answer: Value = response.json().await?;
        self.revision = answer["result"]["protocolVersion"]
            .as_str()
            .map(str::to_owned);
        let confirmed = self
            .send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await?;
        assert_eq!(confirmed.status(), StatusCode::ACCEPTED);
        Ok(answer)
    }

    async fn call(&mut self, tool: &str, arguments: Value) -> Result<Value, Box<dyn Error>> {
        let params = json!({"name": tool, "arguments": arguments});
        let (status, answer) = self.request("tools/call", params).await?;
        assert_eq!(status, StatusCode::OK);
        Ok(answer)
    }
}

async fn ready(stand: &HttpStand) -> Result<HttpClient, Box<dyn Error>> {
    let mut client = HttpClient::new(stand);
    client.handshake().await?;
    Ok(client)
}

fn update_note(value: &str) -> Value {
    json!({"order_id": "A-100", "field": "note", "value": value})
}

fn note_of(control: &Control, order_id: &str) -> Result<String, Box<dyn Error>> {
    Ok(control.stored_order(order_id)?.ok_or("no such order")?.note)
}

// ---- the basic path --------------------------------------------------------

/// The slice's proof over stdio: initialize, list, call a write tool, and ask
/// the stand what it received and stored.
#[test]
fn stdio_a_write_is_proven_by_asking_the_stand_what_it_stored() -> Outcome {
    let stand = StdioStand::new()?;
    let control = stand.control();
    let mut client = StdioClient::start(&stand, None)?;

    let hello = client.handshake()?;
    assert_eq!(hello["result"]["protocolVersion"], HANDSHAKE_REVISION);
    assert_eq!(hello["result"]["serverInfo"]["name"], "example-mcp");
    assert_eq!(hello["result"]["serverInfo"]["version"], "1.0.0");
    assert_eq!(
        hello["result"]["capabilities"]["tools"]["listChanged"],
        true
    );
    assert_eq!(client.request("ping", Value::Null)?["result"], json!({}));

    // The list comes in pages; a client that ignores the cursor sees three tools.
    let first = client.request("tools/list", Value::Null)?;
    assert_eq!(first["result"]["tools"].as_array().map(Vec::len), Some(3));
    assert_eq!(names(&client.tools()?), TOOLS);
    // Listing calls no tool.
    assert!(control.calls()?.is_empty());
    assert_eq!(control.request_count("tools/list")?, 4);

    let answer = client.call("orders_update", update_note("  Dock 4  "))?;
    assert_eq!(answer["result"]["isError"], false);
    // The answer has no structured content: it says the call was accepted,
    // not what was stored. The stand says what was stored.
    assert!(answer["result"].get("structuredContent").is_none());
    let stored = control.stored_order("A-100")?.ok_or("A-100 vanished")?;
    assert_eq!((stored.note.as_str(), stored.version), ("Dock 4", 2));
    assert_eq!(control.write_count("A-100")?, 1);
    let calls = control.calls()?;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool, "orders_update");
    assert_eq!(calls[0].arguments, update_note("  Dock 4  "));
    assert_eq!(calls[0].outcome, CallOutcome::Answered);
    assert_eq!(calls[0].connection, "stdio-1");
    let writes = control.writes()?;
    assert_eq!(
        (writes[0].call, writes[0].action, writes[0].field.as_deref()),
        (1, WriteAction::Updated, Some("note"))
    );

    // A read by key has a structured result that fits its output schema.
    let read = client.call("orders_get", json!({"order_id": "A-100"}))?;
    assert_eq!(read["result"]["structuredContent"]["note"], "Dock 4");
    assert_eq!(
        read["result"]["structuredContent"]["reference"],
        Value::Null
    );

    assert_eq!(client.finish()?, Some(0));
    let connections = control.connections()?;
    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0].transport, Transport::Stdio);
    assert_eq!(connections[0].revision.as_deref(), Some(HANDSHAKE_REVISION));
    assert!(!connections[0].open);
    Ok(())
}

/// The same proof over HTTP, with a create: the stand assigns the identifier,
/// and the same create sent twice makes two orders.
#[tokio::test]
async fn http_a_create_is_proven_by_asking_the_stand_what_it_stored() -> Outcome {
    let stand = HttpStand::start().await?;
    let control = stand.control();
    let mut client = HttpClient::new(&stand);
    let hello = client.handshake().await?;
    assert_eq!(hello["result"]["protocolVersion"], HANDSHAKE_REVISION);
    // No stream is opened over HTTP, so the stand does not claim to send the notice.
    assert_eq!(
        hello["result"]["capabilities"]["tools"]["listChanged"],
        false
    );
    assert_eq!(client.session.as_deref(), Some("session-1"));
    let stream = client.http.get(stand.url()).send().await?;
    assert_eq!(stream.status(), StatusCode::METHOD_NOT_ALLOWED);

    let (_, first) = client.request("tools/list", Value::Null).await?;
    assert_eq!(first["result"]["nextCursor"], "offset-3");
    let (_, last) = client
        .request("tools/list", json!({"cursor": "offset-6"}))
        .await?;
    assert_eq!(
        names(last["result"]["tools"].as_array().ok_or("no tools")?),
        ["orders_delete"]
    );
    assert!(last["result"].get("nextCursor").is_none());
    let (_, bad) = client
        .request("tools/list", json!({"cursor": "nonsense"}))
        .await?;
    assert_eq!(bad["error"]["code"], INVALID_PARAMS);

    let order = json!({"customer": " Fourth Example Customer ", "reference": "REF-1"});
    let created = client.call("orders_create", order.clone()).await?;
    assert_eq!(
        created["result"]["structuredContent"],
        json!({"order_id": "A-103", "version": 1})
    );
    let stored = control
        .stored_order("A-103")?
        .ok_or("A-103 was not stored")?;
    assert_eq!(stored.customer, "Fourth Example Customer");
    assert_eq!(stored.reference.as_deref(), Some("REF-1"));
    assert_eq!(stored.status, OrderStatus::Draft);
    assert_eq!(control.write_count("A-103")?, 1);
    assert_eq!(control.calls()?[0].connection, "session-1");

    client.call("orders_create", order).await?;
    assert_eq!(control.stored_orders()?.len(), 5);
    let found = client
        .call("orders_list", json!({"reference": "REF-1"}))
        .await?;
    let found = found["result"]["structuredContent"]["orders"]
        .as_array()
        .map(Vec::len);
    assert_eq!(found, Some(2));

    // Refused writes are error results and store nothing.
    let refused = client
        .call("orders_delete", json!({"order_id": "A-100"}))
        .await?;
    assert_eq!(refused["result"]["isError"], true);
    let closed = json!({"order_id": "A-101", "field": "note", "value": "x"});
    let refused = client.call("orders_update", closed).await?;
    assert_eq!(refused["result"]["isError"], true);
    let stale = json!({"order_id": "A-100", "field": "note", "value": "x", "expected_version": 9});
    assert_eq!(
        client.call("orders_update", stale).await?["result"]["isError"],
        true
    );
    let absent = client
        .call("orders_get", json!({"order_id": "A-999"}))
        .await?;
    assert_eq!(absent["result"]["isError"], true);
    assert_eq!(control.writes()?.len(), 2);
    assert_eq!(control.calls()?[3].outcome, CallOutcome::ErrorResult);

    // The seed data holds text aimed at an agent, in a field a read returns.
    let read = client
        .call("orders_get", json!({"order_id": "A-101"}))
        .await?;
    assert_eq!(
        read["result"]["structuredContent"]["note"],
        INSTRUCTION_TEXT
    );
    let search = client
        .call("orders_search", json!({"query": "Third"}))
        .await?;
    let text = search["result"]["content"][0]["text"]
        .as_str()
        .ok_or("no text")?;
    assert!(text.contains("A-102") && search["result"].get("structuredContent").is_none());

    control.reset()?;
    assert_eq!(control.stored_orders()?.len(), 3);
    assert!(control.calls()?.is_empty() && control.connections()?.is_empty());
    Ok(())
}

// ---- revisions -------------------------------------------------------------

#[test]
fn stdio_revisions_are_negotiated_and_an_unknown_one_is_not_served() -> Outcome {
    let stand = StdioStand::new()?;
    let control = stand.control();
    let mut client = StdioClient::start(&stand, None)?;

    // Nothing but ping is served before the handshake.
    let early = client.request("tools/list", Value::Null)?;
    assert_eq!(early["error"]["code"], INVALID_REQUEST);
    assert_eq!(client.request("ping", Value::Null)?["result"], json!({}));

    // An unknown revision is answered with the one the stand serves; the
    // client is the one to end the connection if it cannot speak it.
    let offer = client.request("initialize", initialize_params("1999-01-01"))?;
    assert_eq!(offer["result"]["protocolVersion"], HANDSHAKE_REVISION);
    // The handshake is not finished until the client confirms it.
    let unconfirmed = client.request("tools/list", Value::Null)?;
    assert_eq!(unconfirmed["error"]["code"], INVALID_REQUEST);
    client.notify("notifications/initialized")?;
    assert!(client.request("tools/list", Value::Null)?["result"]["tools"].is_array());

    // A stand told to refuse answers with the error the handshake defines.
    control.refuse_unknown_revisions(true)?;
    let refused = client.request("initialize", initialize_params("1999-01-01"))?;
    assert_eq!(refused["error"]["code"], INVALID_PARAMS);
    assert_eq!(refused["error"]["data"]["requested"], "1999-01-01");
    assert_eq!(
        refused["error"]["data"]["supported"],
        json!([HANDSHAKE_REVISION])
    );

    assert_eq!(
        client.request("nonsense", Value::Null)?["error"]["code"],
        METHOD_NOT_FOUND
    );
    client.send(&json!([{"jsonrpc": "2.0", "id": 1, "method": "ping"}]))?;
    assert_eq!(client.read()?["error"]["code"], INVALID_REQUEST);
    writeln!(client.input.as_mut().ok_or("closed")?, "{{not json")?;
    assert_eq!(client.read()?["error"]["code"], PARSE_ERROR);
    drop(client);

    // The revision with no handshake: every request names it.
    let mut stateless = StdioClient::start(&stand, None)?;
    let meta = |revision: &str| json!({"_meta": {STATELESS_VERSION_META_KEY: revision}});
    let listed = stateless.request("tools/list", meta(STATELESS_REVISION))?;
    assert!(listed["result"]["tools"].is_array());
    let unknown = stateless.request("tools/list", meta("1999-01-01"))?;
    assert_eq!(unknown["error"]["code"], INVALID_PARAMS);
    assert_eq!(
        unknown["error"]["data"]["supported"],
        json!([STATELESS_REVISION, HANDSHAKE_REVISION])
    );
    let needs_handshake = stateless.request("tools/list", meta(HANDSHAKE_REVISION))?;
    assert_eq!(needs_handshake["error"]["code"], INVALID_REQUEST);
    let revisions: Vec<_> = control
        .connections()?
        .into_iter()
        .map(|c| c.revision)
        .collect();
    assert_eq!(
        revisions,
        [
            Some(HANDSHAKE_REVISION.to_owned()),
            Some(STATELESS_REVISION.to_owned())
        ]
    );
    Ok(())
}

#[tokio::test]
async fn http_revisions_sessions_and_the_request_with_no_handshake() -> Outcome {
    let stand = HttpStand::start().await?;
    let control = stand.control();

    // With no session and no revision named, a request is refused.
    let mut bare = HttpClient::new(&stand);
    let (status, answer) = bare.request("tools/list", Value::Null).await?;
    assert_eq!(
        (status, &answer["error"]["code"]),
        (StatusCode::BAD_REQUEST, &json!(INVALID_REQUEST))
    );

    // The revision with no handshake is named in the header or in `_meta`.
    bare.revision = Some(STATELESS_REVISION.to_owned());
    let (status, listed) = bare.request("tools/list", Value::Null).await?;
    assert_eq!(status, StatusCode::OK);
    assert!(listed["result"]["tools"].is_array());
    bare.revision = None;
    let named = json!({
        "name": "orders_get",
        "arguments": {"order_id": "A-100"},
        "_meta": {STATELESS_VERSION_META_KEY: STATELESS_REVISION}
    });
    let (status, read) = bare.request("tools/call", named).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read["result"]["structuredContent"]["order_id"], "A-100");
    assert_eq!(control.calls()?[0].connection, "stateless");
    bare.revision = Some("1999-01-01".to_owned());
    let (status, unknown) = bare.request("tools/list", Value::Null).await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(unknown["error"]["data"]["requested"], "1999-01-01");

    // A session lasts until the client ends it or the server forgets it.
    let mut client = ready(&stand).await?;
    let other = ready(&stand).await?;
    assert_ne!(client.session, other.session);
    control.expire_sessions()?;
    let (status, _) = client.request("tools/list", Value::Null).await?;
    assert_eq!(status, StatusCode::NOT_FOUND);
    client.session = None;
    client.revision = None;
    client.handshake().await?;
    let (status, _) = client.request("tools/list", Value::Null).await?;
    assert_eq!(status, StatusCode::OK);
    let ended = other.http.delete(stand.url());
    let ended = ended.header(
        "mcp-session-id",
        client.session.as_deref().unwrap_or_default(),
    );
    assert_eq!(ended.send().await?.status(), StatusCode::OK);
    let (status, _) = client.request("tools/list", Value::Null).await?;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let closed = control.connections()?;
    let closed = closed
        .iter()
        .find(|c| Some(&c.id) == client.session.as_ref());
    assert!(closed.is_some_and(|c| !c.open));
    Ok(())
}

// ---- the tool list changes -------------------------------------------------

#[tokio::test]
async fn stdio_each_drift_shows_in_the_list_and_the_notice_is_sent_when_asked() -> Outcome {
    let stand = StdioStand::new()?;
    let control = stand.control();
    let mut client = StdioClient::start(&stand, None)?;
    client.handshake()?;
    let before = client.tools()?;
    let update_before = tool(&before, "orders_update")?.clone();
    let get_before = tool(&before, "orders_get")?.clone();

    for drift in [
        Drift::ToolRemoved {
            tool: "orders_report".to_owned(),
        },
        Drift::InputSchemaChanged {
            tool: "orders_update".to_owned(),
        },
        Drift::DescriptionChanged {
            tool: "orders_get".to_owned(),
        },
        Drift::OutputSchemaChanged {
            tool: "orders_create".to_owned(),
        },
        Drift::AnnotationsChanged {
            tool: "orders_list".to_owned(),
        },
        Drift::ToolAdded,
    ] {
        control.set_drift(drift)?;
    }
    control.set_server_version("1.1.0")?;
    // A change is silent: nothing was sent, and the next list shows it.
    assert_eq!(control.announcements()?, 0);
    let after = client.tools()?;

    // Removed: not listed, and a call is refused as unknown and runs nothing.
    assert!(!names(&after).contains(&"orders_report"));
    let gone = client.call("orders_report", json!({}))?;
    assert_eq!(gone["error"]["code"], INVALID_PARAMS);
    assert_eq!(control.calls()?[0].outcome, CallOutcome::UnknownTool);

    // Input schema changed: a new required argument, which the server enforces.
    let update_after = tool(&after, "orders_update")?;
    assert_ne!(update_after["inputSchema"], update_before["inputSchema"]);
    assert_eq!(update_after["inputSchema"]["required"][3], "reason");
    let old_shape = client.call("orders_update", update_note("Dock 5"))?;
    assert_eq!(old_shape["result"]["isError"], true);
    assert_eq!(control.write_count("A-100")?, 0);

    // Description changed: only the description, and it now carries the text.
    let get_after = tool(&after, "orders_get")?;
    let described = get_after["description"].as_str().ok_or("no description")?;
    assert!(described.ends_with(INSTRUCTION_TEXT));
    for part in ["name", "inputSchema", "outputSchema", "annotations"] {
        assert_eq!(get_after[part], get_before[part]);
    }

    let create = tool(&after, "orders_create")?;
    assert_eq!(create["outputSchema"]["required"][2], "region");
    assert_eq!(
        tool(&after, "orders_list")?["annotations"]["readOnlyHint"],
        false
    );

    // Added: listed last, and it works if called.
    assert_eq!(names(&after).last(), Some(&ADDED_TOOL));
    client.call(ADDED_TOOL, json!({"order_id": "A-102"}))?;
    let archived = control.stored_order("A-102")?.ok_or("A-102 vanished")?;
    assert_eq!(archived.status, OrderStatus::Archived);

    // The notice leaves when the test asks, with no request to prompt it.
    control.announce_list_changed()?;
    let notice = client.read()?;
    assert_eq!(
        notice,
        json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"})
    );
    control.wait_for_announcements(1).await?;
    drop(client);

    // A program started later reports the new version and is owed no old notice.
    let mut later = StdioClient::start(&stand, None)?;
    assert_eq!(
        later.handshake()?["result"]["serverInfo"]["version"],
        "1.1.0"
    );
    control.clear_drift()?;
    assert_eq!(later.tools()?, before);
    assert!(later.others.is_empty());
    assert_eq!(control.announcements()?, 1);
    Ok(())
}

// ---- a connection lost around a write --------------------------------------

/// The possible commit over stdio: the program stores the order and exits
/// without answering. A program started again over the same stand finds it.
#[test]
fn stdio_the_program_is_lost_after_a_write_was_committed() -> Outcome {
    let stand = StdioStand::new()?;
    let control = stand.control();
    let order = json!({"customer": "Fourth Example Customer", "reference": "REF-7"});

    // Lost before the call was handled: nothing ran and nothing is stored.
    control.set_fault(Fault::once(FaultKind::DropBeforeHandling).on_tool("orders_create"))?;
    let mut client = StdioClient::start(&stand, None)?;
    client.handshake()?;
    client.post(
        "tools/call",
        json!({"name": "orders_create", "arguments": order}),
    )?;
    assert_eq!(client.read_line()?, None);
    assert_eq!(client.finish()?, Some(EXIT_DROPPED));
    assert!(control.calls()?.is_empty() && control.writes()?.is_empty());
    assert_eq!(control.fired_faults()?.len(), 1);

    // Lost after the commit: the call ran, the order is stored, no answer left.
    control.set_fault(Fault::once(FaultKind::DropAfterCommit).on_tool("orders_create"))?;
    let mut client = StdioClient::start(&stand, None)?;
    client.handshake()?;
    client.post(
        "tools/call",
        json!({"name": "orders_create", "arguments": order}),
    )?;
    assert_eq!(client.read_line()?, None);
    assert_eq!(client.finish()?, Some(EXIT_DROPPED));
    let calls = control.calls()?;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].outcome, CallOutcome::Answered);
    assert_eq!(calls[0].fault, Some(FaultKind::DropAfterCommit));
    assert_eq!(control.write_count("A-103")?, 1);
    assert!(control.faults()?.is_empty());

    // What a reconcile would do: start again, look for the reference.
    let mut client = StdioClient::start(&stand, None)?;
    client.handshake()?;
    let found = client.call("orders_list", json!({"reference": "REF-7"}))?;
    let found = &found["result"]["structuredContent"]["orders"];
    assert_eq!(found.as_array().map(Vec::len), Some(1));
    assert_eq!(found[0]["order_id"], "A-103");
    assert_eq!(control.writes()?.len(), 1);
    let ids: Vec<_> = control.connections()?.into_iter().map(|c| c.id).collect();
    assert_eq!(ids, ["stdio-1", "stdio-2", "stdio-3"]);
    Ok(())
}

#[tokio::test]
async fn http_the_connection_is_lost_after_a_write_was_committed() -> Outcome {
    let stand = HttpStand::start().await?;
    let control = stand.control();
    let mut client = ready(&stand).await?;
    let call = |client: &mut HttpClient, value: &str| {
        client.message(
            "tools/call",
            json!({"name": "orders_update", "arguments": update_note(value)}),
        )
    };

    control.set_fault(Fault::once(FaultKind::DropBeforeHandling).on_tool("orders_update"))?;
    let message = call(&mut client, "Dock 5");
    assert!(client.send(&message).await.is_err());
    assert!(control.calls()?.is_empty());
    assert_eq!(note_of(control, "A-100")?, "Deliver to dock 2");

    control.set_fault(Fault::once(FaultKind::DropAfterCommit).on_tool("orders_update"))?;
    let message = call(&mut client, "Dock 6");
    // No status and no body: the socket closed.
    assert!(client.send(&message).await.is_err());
    assert_eq!(note_of(control, "A-100")?, "Dock 6");
    // The client did not send it again by itself: one call, one write.
    assert_eq!(control.write_count("A-100")?, 1);
    let calls = control.calls()?;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].fault, Some(FaultKind::DropAfterCommit));

    // The session outlives the lost connection, and the read shows the write.
    let read = client
        .call("orders_get", json!({"order_id": "A-100"}))
        .await?;
    assert_eq!(read["result"]["structuredContent"]["note"], "Dock 6");
    assert_eq!(read["result"]["structuredContent"]["version"], 2);
    Ok(())
}

// ---- slow, broken, and steering answers ------------------------------------

#[tokio::test]
async fn http_each_fault_on_an_answer_can_be_switched_on_and_seen() -> Outcome {
    let stand = HttpStand::start().await?;
    let control = stand.control();
    let mut client = ready(&stand).await?;
    let get = json!({"order_id": "A-100"});

    // Slow: the slow tool, and a delay on any request. Only a lower bound is
    // checked, so the test does not depend on how fast the machine is.
    control.set_slow_ms(30)?;
    let started = Instant::now();
    let report = client.call("orders_report", json!({})).await?;
    assert!(started.elapsed() >= Duration::from_millis(30));
    assert_eq!(
        report["result"]["structuredContent"],
        json!({"draft": 2, "submitted": 1, "archived": 0})
    );
    control.set_fault(Fault::once(FaultKind::Delay { ms: 30 }).on_method("tools/list"))?;
    let started = Instant::now();
    let (_, listed) = client.request("tools/list", Value::Null).await?;
    assert!(started.elapsed() >= Duration::from_millis(30) && listed["result"]["tools"].is_array());

    // Malformed: the work is done, and what comes back is not a response.
    let broken = Fault::once(FaultKind::Malformed {
        shape: Malformed::NotJson,
    });
    control.set_fault(broken.on_tool("orders_update"))?;
    let message = client.message(
        "tools/call",
        json!({"name": "orders_update", "arguments": update_note("Dock 7")}),
    );
    let response = client.send(&message).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(serde_json::from_str::<Value>(&response.text().await?).is_err());
    assert_eq!(note_of(control, "A-100")?, "Dock 7");
    control.set_fault(Fault::once(FaultKind::Malformed {
        shape: Malformed::NotAResponse,
    }))?;
    let (_, empty) = client.request("ping", Value::Null).await?;
    assert!(empty.get("result").is_none() && empty.get("error").is_none());

    // A protocol error nobody foresaw, and an HTTP status: nothing runs.
    let error = FaultKind::ProtocolError {
        code: -32099,
        message: "Out of order".to_owned(),
    };
    control.set_fault(Fault::once(error).on_tool("orders_get"))?;
    assert_eq!(
        client.call("orders_get", get.clone()).await?["error"]["code"],
        -32099
    );
    control.set_fault(Fault::once(FaultKind::HttpStatus { status: 503 }).on_tool("orders_get"))?;
    let (status, _) = client
        .request(
            "tools/call",
            json!({"name": "orders_get", "arguments": get}),
        )
        .await?;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(control.calls_to("orders_get")?, 0);

    // Text aimed at an agent, added to an answer that is otherwise right.
    control.set_fault(Fault::once(FaultKind::InjectText).on_tool("orders_get"))?;
    let steered = client.call("orders_get", get.clone()).await?;
    assert_eq!(steered["result"]["content"][1]["text"], INSTRUCTION_TEXT);
    assert_eq!(steered["result"]["structuredContent"]["order_id"], "A-100");

    // Structured content that does not fit the tool's output schema.
    control.set_fault(Fault::until_cleared(FaultKind::ResultOffSchema).on_tool("orders_get"))?;
    let off = client.call("orders_get", get.clone()).await?;
    assert_eq!(
        off["result"]["structuredContent"],
        json!({"unexpected": true})
    );
    assert_eq!(control.faults()?.len(), 1);
    control.clear_faults()?;
    let fine = client.call("orders_get", get).await?;
    assert_eq!(fine["result"]["structuredContent"]["note"], "Dock 7");

    let fired: Vec<_> = control
        .fired_faults()?
        .into_iter()
        .map(|f| f.method)
        .collect();
    assert_eq!(fired.len(), 7);
    assert_eq!(fired[0], "tools/list");
    Ok(())
}

/// A slow answer does not hold up the ones behind it, and a broken line is
/// one line. The slow tool is set to a minute and never waited for.
#[test]
fn stdio_answers_leave_out_of_order_and_a_broken_answer_is_one_line() -> Outcome {
    let stand = StdioStand::new()?;
    let control = stand.control();
    let mut client = StdioClient::start(&stand, None)?;
    client.handshake()?;

    control.set_slow_ms(60_000)?;
    client.post(
        "tools/call",
        json!({"name": "orders_report", "arguments": {}}),
    )?;
    let quick = client.post("ping", Value::Null)?;
    assert_eq!(client.read()?["id"], quick);
    assert_eq!(control.calls_to("orders_report")?, 1);

    let broken = Fault::once(FaultKind::Malformed {
        shape: Malformed::NotJson,
    });
    control.set_fault(broken.on_tool("orders_update"))?;
    client.post(
        "tools/call",
        json!({"name": "orders_update", "arguments": update_note("Dock 8")}),
    )?;
    let line = client.read_line()?.ok_or("no line")?;
    assert!(serde_json::from_str::<Value>(&line).is_err());
    assert_eq!(note_of(control, "A-100")?, "Dock 8");
    // The connection is still good after the broken line.
    assert_eq!(client.request("ping", Value::Null)?["result"], json!({}));
    Ok(())
}

// ---- credentials -----------------------------------------------------------

#[tokio::test]
async fn http_a_bearer_token_is_required_when_the_stand_is_told_to_ask() -> Outcome {
    let stand = HttpStand::start().await?;
    let control = stand.control();
    control.accept_token("token-of-ada", "ada")?;
    control.accept_token("token-of-ben", "ben")?;

    let mut nobody = HttpClient::new(&stand);
    let message = nobody.message("initialize", initialize_params(HANDSHAKE_REVISION));
    let refused = nobody.send(&message).await?;
    assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);
    let challenge = refused.headers()["www-authenticate"].to_str()?.to_owned();
    let metadata = format!("{}/.well-known/oauth-protected-resource", stand.base());
    assert_eq!(
        challenge,
        format!("Bearer resource_metadata=\"{metadata}\"")
    );
    let document: Value = nobody.http.get(&metadata).send().await?.json().await?;
    assert_eq!(document["resource"], stand.url());

    let stranger = HttpClient::with_token(&stand, "token-of-nobody");
    let refused = stranger.send(&message).await?;
    assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);
    let challenge = refused.headers()["www-authenticate"].to_str()?;
    assert!(challenge.starts_with("Bearer error=\"invalid_token\""));
    assert_eq!(control.unauthorized_count()?, 2);
    assert_eq!(control.request_count("initialize")?, 0);

    // Two identities, each on its own session.
    let mut ada = HttpClient::with_token(&stand, "token-of-ada");
    ada.handshake().await?;
    let mut ben = HttpClient::with_token(&stand, "token-of-ben");
    ben.handshake().await?;
    ada.call("orders_get", json!({"order_id": "A-100"})).await?;
    ben.call("orders_get", json!({"order_id": "A-102"})).await?;
    let who: Vec<_> = control
        .calls()?
        .into_iter()
        .map(|c| (c.connection, c.principal))
        .collect();
    assert_eq!(who[0], ("session-1".to_owned(), Some("ada".to_owned())));
    assert_eq!(who[1], ("session-2".to_owned(), Some("ben".to_owned())));
    assert!(control.shared_connections()?.is_empty());

    // The stand lets one session carry two identities, and reports it.
    ben.session.clone_from(&ada.session);
    ben.call("orders_get", json!({"order_id": "A-102"})).await?;
    assert_eq!(
        control
            .shared_connections()?
            .into_iter()
            .collect::<Vec<_>>(),
        ["session-1"]
    );

    // A credential that stops being accepted is refused on the next request.
    control.revoke_token("token-of-ada")?;
    let (status, _) = ada.request("tools/list", Value::Null).await?;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // The token never appears in what the stand reports.
    let truth = serde_json::to_string(&control.truth()?)?;
    assert!(!truth.contains("token-of-"));
    Ok(())
}

#[test]
fn stdio_the_credential_comes_from_the_environment_and_two_programs_share_one_stand() -> Outcome {
    let stand = StdioStand::new()?;
    let control = stand.control();
    control.accept_token("token-of-ada", "ada")?;
    control.accept_token("token-of-ben", "ben")?;

    let mut stranger = StdioClient::start(&stand, Some("token-of-nobody"))?;
    let refused = stranger.request("initialize", initialize_params(HANDSHAKE_REVISION))?;
    assert_eq!(refused["error"]["code"], CREDENTIAL_REFUSED);
    assert!(!refused.to_string().contains("token-of-nobody"));
    let mut nobody = StdioClient::start(&stand, None)?;
    let refused = nobody.request("initialize", initialize_params(HANDSHAKE_REVISION))?;
    assert_eq!(refused["error"]["code"], CREDENTIAL_REFUSED);
    assert_eq!(control.unauthorized_count()?, 2);

    // Two programs at once over one stand: one set of orders, two connections.
    let mut ada = StdioClient::start(&stand, Some("token-of-ada"))?;
    let mut ben = StdioClient::start(&stand, Some("token-of-ben"))?;
    ada.handshake()?;
    ben.handshake()?;
    ada.call("orders_update", update_note("Dock 9"))?;
    let read = ben.call("orders_get", json!({"order_id": "A-100"}))?;
    assert_eq!(read["result"]["structuredContent"]["note"], "Dock 9");
    let who: Vec<_> = control
        .calls()?
        .into_iter()
        .map(|c| (c.connection, c.principal))
        .collect();
    assert_eq!(who[0], ("stdio-3".to_owned(), Some("ada".to_owned())));
    assert_eq!(who[1], ("stdio-4".to_owned(), Some("ben".to_owned())));
    assert!(control.shared_connections()?.is_empty());
    Ok(())
}

// ---- finding the program ---------------------------------------------------

/// Another crate's tests get the program from `stdio_program`, which asks
/// Cargo to build it. This is the program Cargo built for these tests.
#[test]
fn the_program_the_harness_finds_is_the_one_cargo_built() -> Outcome {
    let found = stdio_program()?.canonicalize()?;
    let built = std::path::Path::new(env!("CARGO_BIN_EXE_example-mcp")).canonicalize()?;
    assert_eq!(found, built);
    Ok(())
}
