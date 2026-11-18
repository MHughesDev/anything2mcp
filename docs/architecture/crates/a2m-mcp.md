# `a2m-mcp`

The MCP face of the kernel. It turns the action registry and a toolset's published tools into MCP tool lists, turns a tool call into a dispatch, and turns the dispatcher's response into a structured result. It serves Streamable HTTP, which the local channel and a server both carry, and it holds the stdio proxy that an MCP client on one machine starts. It knows no make of client.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 8, Roles |
| May depend on | Layers 1 to 7 |
| Depends on | `a2m-core`, `a2m-actions`, `a2m-policy` for the name of a toolset. Outside: `axum` for the HTTP types and the router; `futures-core` for one stream trait; `serde_json`; Tokio. The protocol layer is this crate's own. The official Rust SDK, `rmcp`, was built on first and left, because its tree holds a crate declared unmaintained ([decision 0130](../../decisions/0130-the-mcp-protocol-layer-is-our-own-because-the-sdk-brings-an-unmaintained-crate.md), which supersedes that part of [decision 0071](../../decisions/0071-the-mcp-server-stands-on-the-official-sdk-behind-a-seam-and-serves-two-revisions.md); [research note 18](../../research/18-mcp-server-base.md)) |
| Roadmap packages | Roadmap package 1.5 (stdio proxy, the `use` toolset, structured results); 5.1 (the `build` toolset, prompts, resources); 6.2 (Streamable HTTP on a server, as an OAuth resource server) |
| Carries out | Spec sections [221](../../spec/2-kernel/221-the-tool-surface-agents-see.md), [222](../../spec/2-kernel/222-front-doors-and-the-local-engine.md) for MCP, [220](../../spec/2-kernel/220-actions-and-the-authoring-interface.md) for the MCP face, [220](../../spec/2-kernel/220-actions-and-the-authoring-interface.md); the MCP half of [503](../../spec/5-security/503-approval-and-sign-in-surfaces.md) |

## Owns

- The MCP endpoint, one per toolset, as an HTTP service that a listener above it serves.
- The revisions served, and the difference between them, kept inside this crate.
- Tool lists: which actions and which target tools each toolset lists, and the cap on their number and size.
- The mapping from an MCP tool call to a `Request`, and from a `Response` to an MCP result: structured content, a one-line summary, resource links, annotations.
- Elicitation for sign-in and approval links, and the link in the result where a client has no elicitation.
- Progress and long jobs: progress notifications to a caller that asked, and the Tasks extension mapped onto runs.
- Server instructions, prompts, and resources generated from the same sources as the guidance file and `docs.get`.
- The stdio proxy: a thin client that copies MCP messages between standard input and output and the engine's MCP endpoint.
- Client profiles, as data: a quirk of one client, where one must be handled.

It must never: hold run state between requests; decide whether a call is allowed; validate a call against anything but the full schema the dispatcher holds; let text from a target into a tool's name or description; branch on which client is calling except through a profile.

## Interface

As built ([decision 0115](../../decisions/0115-the-mcp-endpoint-as-built-checks-in-front-of-the-sdk-and-a-redactor-on-the-way-out.md); [decision 0130](../../decisions/0130-the-mcp-protocol-layer-is-our-own-because-the-sdk-brings-an-unmaintained-crate.md), which changed nothing in it; and [decision 0151](../../decisions/0151-the-doors-take-up-the-action-registrys-second-pass.md), which added no public item and changed what a mount may put on a request).

```rust
/// One endpoint per toolset. Cheap to clone; no state between requests.
pub struct McpService { /* Kernel, toolset, settings */ }
impl McpService {
    /// Refuses a toolset other than `use` and `build`, and settings with no revision.
    pub fn new(kernel: Kernel, toolset: ToolsetName, settings: McpSettings) -> Result<Self, Failure>;
    pub fn toolset(&self) -> &ToolsetName;
    /// Stateless Streamable HTTP, as a router to nest at `mcp_path(toolset)`.
    pub fn http(self) -> axum::Router;
    /// One request, answered without a router.
    pub async fn handle(&self, request: HttpRequest) -> HttpResponse;
}
pub fn mcp_path(toolset: &ToolsetName) -> String;      // "/mcp/<toolset>"
pub type HttpRequest = axum::http::Request<axum::body::Body>;
pub type HttpResponse = axum::http::Response<axum::body::Body>;

/// Who the local channel saw connect. The mount puts one in each request's extensions,
/// or a `Caller` the kernel already checked. Beside it the mount may put the
/// `ProjectFolder` of the action layer: the folder the client end works in.
pub struct McpPeer { pub user: String, pub label: String }

pub struct McpSettings {
    pub revisions: Vec<Revision>,      // mcp.revisions; default 2026-07-28 and 2025-11-25
    pub listing: Listing,              // tools.full_listing: Default (pipelines), Full
    pub listed_max: usize,             // tools.listed_limit; a full listing stops at 200
    pub allowed_hosts: Vec<String>,    // the Host values served; loopback names by default
    pub max_request_bytes: usize,      // 4 MiB by default
}
impl McpSettings {
    pub fn local() -> Self;
    pub fn from_settings(settings: &Settings) -> Result<Self, Failure>;
}
pub enum Revision { Handshake, Stateless }   // 2025-11-25, 2026-07-28
pub enum Listing { Default, Full }

/// The way to the engine's endpoint. Any function from a request to a response is one.
pub trait Upstream: Send + Sync + 'static {
    /// A failure of class `busy`, `policy`, `input`, or `transient` says the request was not
    /// handed to the engine; any other class, `possible_commit` above all, says it may have been.
    fn send(&self, request: HttpRequest) -> BoxFuture<'static, Result<HttpResponse, Failure>>;
}
/// The longest line the proxy reads (4 MiB), and the most requests it carries at once (64).
pub const LINE_LIMIT: usize;
pub const OPEN_LIMIT: usize;
/// The stdio proxy. It reads and writes MCP messages and nothing else; it has no tools of its own.
/// It ends well when its input ends.
pub async fn stdio_proxy<U, R, W>(upstream: U, toolset: &ToolsetName, input: R, output: W)
    -> Result<(), Failure>;
```

The proxy is started by `bridge`, a process command of the product crate, which connects through `a2m-local` and wraps the link's `send` in a closure. The two role crates do not depend on each other. Client profiles are not built, so no type for them exists yet.

**How the product crate composes it.** For each of `use` and `build`: `McpService::new(kernel.clone(), toolset, McpSettings::from_settings(kernel.settings())?)`, nested at `mcp_path(&toolset)` in the router the local channel serves. The mount puts the caller on each request: an `McpPeer` with the user the channel saw, or the `Caller` it already had the kernel check for the REST router. Beside the caller it puts the `ProjectFolder` the client end stated, when it stated one; the endpoint hands it to `Request::in_project_folder` with every dispatch, and nothing in a message can set it. `bridge` names the folder it was started in, or the one `--project` gives, on every request it carries. For `bridge`: `stdio_proxy(move |request| link.send(request), &toolset, stdin, stdout)`, and exit status 0 when it returns well.

## Revisions

| Revision | How it is served |
| --- | --- |
| 2026-07-28 | Stateless. No handshake, no session header. Every request carries everything; continuity is the `run_id` |
| 2025-11-25 | The initialize exchange is answered from settings. No session identifier is issued, which that revision allows, so the server stays stateless |

One service serves both, with no session for either. Both are served because a server with only one of them turns clients away: the handshake is still the default way a reference client library opens ([decision 0071](../../decisions/0071-the-mcp-server-stands-on-the-official-sdk-behind-a-seam-and-serves-two-revisions.md)).

**How a revision is agreed.**

1. A request whose `_meta` carries `io.modelcontextprotocol/protocolVersion` is served under that revision. On HTTP the `MCP-Protocol-Version` header must say the same. An unserved revision is error -32022 with `data.supported` and `data.requested`, and status 400 on HTTP.
2. `server/discover` lists the revisions served and the capabilities.
3. `initialize` selects the handshake revision: 2025-11-25 if the client asks for it, and 2025-11-25 as the answer if it asks for anything else. The client then decides.
4. A handshake-era request on HTTP names its revision in the header and is served as it comes.
5. With `mcp.revisions` set to the handshake revision alone, `server/discover` is answered as an unknown method, so that a client that speaks both falls back. With 2026-07-28 alone, `initialize` is refused with -32022 and the list, and a request that names no revision is refused with -32602.
6. A request of the handshake revision that names no revision, in its body or in a header, is served as 2025-11-25. The stdio proxy therefore remembers nothing of the handshake.

**What a handshake-era caller does not get.** A stateless server does not remember what a client declared in `initialize`. Such a caller gets the fallback every client gets: the link in the result in place of elicitation, the run identifier in place of a task.

**The protocol layer.** One module of this crate answers the methods: `initialize` and `ping` of the handshake revision, `server/discover` of the stateless revision, and `tools/list` and `tools/call` in both. It builds each result in the shape its revision wants, and answers a tool call whose caller asked for progress as a stream of events. It is about 300 lines over `serde_json`, and names no outside protocol library ([decision 0130](../../decisions/0130-the-mcp-protocol-layer-is-our-own-because-the-sdk-brings-an-unmaintained-crate.md)). Everything this crate promises is in the list of messages below, which is tested with raw messages; that list is what let the SDK be replaced in one step, with every test passing unchanged.

**In front of it.** The endpoint checks, in order: the `Host`, the HTTP method, the content types, the caller, the size of the body, that the body is JSON, that it is one request or one notification, the revision it names and that its headers agree, and the shape of a tool call. It answers a notification itself, with status 202. Each refusal of a message is a JSON-RPC error whose `data` holds the failure's class, code, message, and fix.

**On the way out.** Every text of every message passes the kernel's redactor as the message leaves. The dispatcher has already redacted a result; this pass covers what the protocol layer writes itself, such as an error that quotes a tool's name, and the id and progress token it echoes.

## What the server must pass, message by message

Each line is a test with raw messages. "H" is the handshake revision, "M" the newer one. The spike's caller, `spikes/mcp-server/callers/raw_conformance.py`, holds a first version of most of them.

| Message | The server must |
| --- | --- |
| A body or line that is not JSON | Answer -32700 with a null id, and status 400 on HTTP. On stdio this is the proxy's to do |
| A message that is not a request, or has no method | Answer -32600. A batch is not a request. Nor is a message whose version is not 2.0 or whose id is neither text nor a whole number |
| An unknown method | Answer -32601 |
| An unknown notification | Answer nothing; on HTTP, status 202 |
| A request id | Come back unchanged, text as text and number as number |
| `initialize` (H) | Answer with `protocolVersion` 2025-11-25 whatever was asked, `capabilities` that name only what is served, and `serverInfo` with a name and a version. Issue no session identifier |
| `initialize` when only M is served | Refuse with -32022 and the revisions served |
| `notifications/initialized` (H) | Answer nothing |
| `ping` (H) | Answer an empty result, before `initialize` too |
| `ping` (M) | Answer -32601 |
| `server/discover` (M) | Answer `supportedVersions`, `capabilities`, `instructions`, `resultType`, `ttlMs`, `cacheScope` |
| `server/discover` when only H is served | Answer -32601 |
| Any M request with an unserved version | Answer -32022 with `data.supported` and `data.requested`; status 400 on HTTP |
| Any M request without `io.modelcontextprotocol/clientCapabilities` | Answer -32602 |
| Any M request on HTTP whose `MCP-Protocol-Version`, `Mcp-Method`, or `Mcp-Name` header disagrees with the body | Answer status 400 with -32020 |
| Every M result | Carry `resultType`, and name the server in `_meta` under `io.modelcontextprotocol/serverInfo` |
| `tools/list` (H and M) | List each tool with its name, description, and input schema of type object; annotations from the effect class; the same tools in the same order every time; no `nextCursor` on a single page; -32602 for a cursor it did not issue |
| `tools/list` (M) | Also carry `ttlMs` and `cacheScope` |
| `tools/call` for a known tool (H and M) | Answer `structuredContent` that fits the tool's output schema, and one text item |
| `tools/call` for an unknown tool | Answer -32602, a protocol error. An action the toolset does not list is an unknown tool. So is a name nobody published: the error's data then holds the dispatcher's own failure |
| `tools/call` whose name is not text or whose arguments are not an object | Answer -32602, before any dispatch |
| `tools/call` with input that fails the schema | Answer a result with `isError` true, not a protocol error |
| `tools/call` that fails | Answer a result with `isError` true and one text item: the class, code, message, fix, and whether a retry is safe, as one JSON object. No structured content and nothing else |
| `tools/call` with a `progressToken` | Answer as a stream of events: `notifications/progress` with that token, a rising count, and a sentence of the server's own, then the result. Say nothing in progress that a system said |
| `tools/call` without a `progressToken` | Send no progress, and answer with one JSON message |
| `notifications/cancelled` on stdio, or the stream closed on HTTP | Stop, and send nothing more for that request. Other requests are still answered |
| `resources/read`, `prompts/list`, `prompts/get`, tasks, elicitation | Not built. Their lines are added by the packages that build them |
| A request over HTTP with a foreign `Host`, or none | Be refused with status 403 before it is read |
| A request over HTTP that is not a `POST` | Be refused with status 405 |
| A request over HTTP that is not `application/json`, or whose caller does not accept both `application/json` and `text/event-stream` | Be refused with status 415, or 406 |
| A request of the stateless revision over HTTP with no `MCP-Protocol-Version` header | Answer status 400 with -32020 |
| A request over HTTP that names no caller, or one the kernel does not let in | Be refused with status 401 or 403 and error -32001, before the body is read |
| Any message that leaves | Hold no registered secret, in a result or in a protocol error |
| End of input on stdio | The proxy exits with status 0 |
| Standard output on stdio | Carry one JSON message per line and nothing else |
| A line on stdio longer than 4 MiB, the endpoint's cap on a body | Answer -32600 with a null id, and keep none of the line: the rest of it is read and dropped |
| More than 64 requests open on stdio at once | The next line is read when one of them is answered |
| A request on stdio refused before it was handed to the engine: nothing answered, or the connection ended first | Answer -32000 with the failure as `data` and `retry_safe` true |
| A `tools/call` on stdio whose answer was lost after the engine may have had it: the connection broke, the answer broke off, or what came back is no MCP response | Answer -32000 with `data` of class `possible_commit`, code `answer_lost`, `retry_safe` false, and a fix that says not to call again and to find the run first. Any other request in that case writes nothing, and its `data` says `retry_safe` true |
| Two unrelated client libraries, each in its default mode | Connect, list the tools, call a read, and read the structured result. Not yet run against this service; owed before Phase 1 exits |

Every line but the last is a test in this crate, with raw messages over the HTTP service in process, and, for the lines that hold on both transports, through the proxy with in-memory streams as well.

## Tool lists

A toolset's list is the same for every caller connected to it; rights are checked at call time and a refused call is a clear `policy` result ([spec section 214](../../spec/2-kernel/214-access-control-toolsets-and-tenancy.md)).

| Toolset | Lists | Reaches without listing |
| --- | --- | --- |
| `use` | Actions whose `listed` names `use`; the toolset's pipelines | Every operation, through `a2m_tool_search` and `a2m_tool_call` |
| `build` | Actions whose `listed` names `build`, as the registry gives them; the pipelines | Every action, through `a2m_action_search`, `a2m_action_describe`, `a2m_action_run` |
| A named toolset on a server | Its members' pipelines and the `use` actions | Its operations, through search. Not built: the endpoint refuses any toolset but `use` and `build` |

- **Names.** An action `noun.verb` is listed as `a2m_<noun>_<verb>`. A target's tool keeps its published name, `{target}_{module}_{object}_{verb}`; its version appears in results, never in the name.
- **Descriptions** are the published ones, or the action's summary. The first 240 characters are enough to choose. A published tool with no description is listed with its title.
- **Where a list comes from.** It is built on each request: the registry's MCP faces, then the published tools, read through `tool.search` with `full`, twenty to a page, dispatched as the caller: one dispatch for each page, and one in all for a target of twenty tools or fewer. A page that the result size cap cut is asked for again at half the size. Nothing is kept between requests.
- **Schemas.** The input schema in a list may be the compact form, provided every difference is exact or wider; today the full schema is listed. Output schemas are left out of the list; `tool.describe` returns them.
- **Annotations.** `readOnlyHint` for `read`; `destructiveHint` for `irreversible` and for writes without a key; `idempotentHint` from the key.
- **Size.** A built-in toolset lists no more actions than its cap, twelve for `use` and fifteen for `build`; an action marked for it past the cap is reached through `a2m_action_run` (decisions [0094](../../decisions/0094-the-action-layers-interface-as-building-it-settled-it.md) and [0280](../../decisions/0280-the-use-toolset-lists-at-most-twelve-tools.md)). For target tools the cap is `tools.listed_limit`, which counts the built-in tools too; past it, pipelines beyond the cap move to search. The server instructions say that a tool that is not listed is found with `a2m_tool_search` and called with `a2m_tool_call`. The list's size in bytes is measured and budgeted (spec section 220 leaves the budget open).
- **Full listing** is a setting for clients with their own tool search. It lists every operation, and changes nothing about how calls are checked.

## Calls and results

1. `tools/call` arrives with a name and arguments. A name of an action the toolset lists is that action. Any other name is `tool.call` with the name as its `tool` input: one dispatch. When the dispatcher answers `input`, `not_found` with no result, no such tool is published, and the answer is the protocol's unknown-tool error. `a2m_tool_call` is the same action called by name, and a tool it does not find is a failed call ([decision 0151](../../decisions/0151-the-doors-take-up-the-action-registrys-second-pass.md)).
2. The front door builds a `Request`: the caller (on the local channel, the operating-system user; on a server, from the validated token), the toolset, the client label, and the project folder the mount stated, and the idempotency key of the call's `_meta`, `anything2mcp/idempotency_key`, which the dispatcher checks (decision 0160). A request's JSON-RPC id is the connection's and is never taken as a key.
3. `Kernel::dispatch` does the rest. This crate only renders the `Response`:

| Part of the response | MCP result |
| --- | --- |
| `result` | `structuredContent`, matching the action's output schema, and one text item holding the same JSON |
| `next` of a success | Inside the structured content, as `next`: the action, the name of its tool, its input, and when. Every output schema admits it. Rendered by `McpResultFace::of_in` with the endpoint's toolset: when this toolset does not list the tool, `next` names `a2m_action_run` with the action as its argument |
| Kept blobs: traces, diffs, snapshots, files | A reference in the result, never inlined. Resource links and `resources/read` are not built |
| `warnings` of a success | `_meta`, under `anything2mcp/warnings`. The output schemas take no key for them |
| `error` | `isError`, and one text item: the class, code, message, fix, and whether a retry is safe, as one JSON object. No structured content |
| Text from the target | Inside fields marked untrusted, already cut and stripped by the dispatcher |

**Long jobs.** A long-running action returns its `run_id` at once. A client that supports the Tasks extension gets a task whose `tasks/get` is answered by `run.get` and whose cancel is `run.cancel`; that is not built.

**Progress** is sent only to a caller that asked, and says what the call's run does ([decision 0151](../../decisions/0151-the-doors-take-up-the-action-registrys-second-pass.md)).

| The call | How it is made | What is followed |
| --- | --- | --- |
| Starts a run and waits for it: `tool.call`, by the control tool or by a published tool's own name | Dispatched with `wait_seconds` 0, which returns the run's identifier at once | The run's events after those the first answer counted, through `Kernel::next_events_from`, until the run rests, ends, or the wait the caller asked for (60 s when it did not say) is over |
| Continues a run the caller names and waits for it: `run.resume` | Dispatched as it was written, on a task of its own | The run's events after its last one, until the dispatch answers |
| Any other call, a call that asks for no wait, and a call whose `wait_seconds` is not what the schema takes | Dispatched as it was written | Nothing: two notes, handed to the engine and answered |

- **What is told.** A change of state, a step that began, the records planned, the stored preview; and, once for each page of events, how many records were verified so far, how many failed, and how many in doubt were read again. A lease, a lock, a chosen binding, an intent, and a need are not told.
- **What is said.** A note is `progress`, a count that rises by one, and `message`, a sentence of this crate with a count, a step's number, or the name of a state in it. Nothing an event carries as text is passed on, so nothing a system said can reach a model as an instruction. No `total` is sent: the number of events to come is not known.
- **The answer to a call that started a run.** A run that has ended, or is still worked on when the wait is over, is read with `run.get`, whose output is `tool.call`'s. A run that came to rest is answered by the caller's own call made once more with `wait_seconds` 0: the same call is the same run while the run is unfinished, and only a call that starts or continues a run tells the run's person that it waits; `run.get` asks nobody anything. So a followed call costs two dispatches. Should the second be refused with no result, the first answer stands: the run's identifier, and `a2m_run_get` as the next call.
- **A run that already rests or has ended** when its call is answered has nothing to follow, and the answer is the answer.
- **Settling.** The record that a job has ended appends no event. Each wait is `Kernel::next_events_from` with what the last page showed, a `Followed`: the last event's number and whether the run was settled. The kernel answers at once when that has changed, so a run whose state rests while its job is still in a runner's hands is told the moment the job is recorded as ended, with no timer ([decision 0159](../../decisions/0159-a-lost-answer-is-a-write-in-doubt-at-the-doors-and-in-the-clients.md)).
- **A reader that stops reading.** Each message of a stream waits at most 60 s for the reader to take it (`READER_PATIENCE`). After that the caller is treated as gone: following stops, and the dispatch, on its own task, goes on to its end.

**Cancelling.** A dispatch runs on its own task and is never cut off. When the caller cancels or the stream closes, the endpoint stops waiting, stops following the run, and sends nothing more for that request. The run goes on, and is stopped with `a2m_run_cancel`.

**Elicitation.** Only the fallback is built: the link in the result. When a run rests at `needs_login` or `needs_approval`, the result carries the link from `PersonSurface`; that includes a plain link from `give_link`, which a sign-in provider such as authorization-code sign-in needs the person to open in their own browser. Where the client supports URL-mode elicitation, the same link is also sent as an elicitation. On one machine the link is the loopback page, which can only raise the operating system's prompt; on a server it is the console page behind the identity provider's sign-in. Following the link is never an approval by itself, so giving it to an agent is safe.

**Found tools.** Spec section 220 asks that a tool found by search be added to the client's list. The stateless revision has no per-connection list. Until a client can be told of a changed list without a session, the generic `a2m_tool_call` is the route, and every call is checked against the full schema either way.

## No client is special

| What differs between clients | Where it is handled |
| --- | --- |
| Capabilities: elicitation, tasks, progress, resource links | What the client declares; each has a fallback that every client gets |
| Which revision | The revision the request names |
| A quirk with no declared capability | A `ClientProfile` row in a data file, with a test, and a note of why |
| The client's own name | The client label, for the audit log and fair queuing only |

The kernel never sees a client's name. Conformance is tested against the protocol with at least two unrelated clients ([decision 0058](../../decisions/0058-a-toolkit-for-developers-with-no-catalog-and-no-preferred-agent.md)).

## On a server

Not built. The same `McpService` is mounted at `/mcp/<toolset>` on the network listener. The listener authenticates: it is an OAuth 2.1 resource server, publishes protected resource metadata, and rejects a token not issued for it. Token checks use the verifier the kernel holds, so `a2m-api` and this crate check tokens in one place. A caller's token is never forwarded to a target. Today the endpoint takes a bearer token out of the request, hands it to that verifier, and answers status 401 when it is refused, which in this build is always.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `anything2mcp` | `McpService` mounted on the local channel and the network listener; `stdio_proxy` for `bridge` |

It calls `a2m-actions` for the registry, `dispatch`, the check of a caller, and the redactor.

## State

None between requests. Settings are read at start.

## Failures

Every failure of a call is the dispatcher's, rendered. This crate adds five, none of which reaches the dispatcher:

| Class and code | When |
| --- | --- |
| `input` / `schema_invalid` | A malformed MCP message. It is the `data` of the JSON-RPC error |
| `input` / `mcp_not_served` | An endpoint is asked for a toolset or a revision that is not served |
| `policy` / `caller_missing` | A request names no caller |
| `transient` / `stdio_read_failed` | The proxy cannot read its input |
| `possible_commit` / `answer_lost` | The proxy lost the answer to a `tools/call` after the engine may have had it. It is the `data` of the -32000 error, with `retry_safe` false |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Two unrelated clients list the tools and call a read | The Phase 1 exit test. Not yet run against this service |
| A built-in toolset never lists more than its cap | The registry check, in `a2m-actions` |
| A call is validated against the full schema, whatever the list showed | The checklist: an input with an unknown key, and one of the wrong type, are results with `isError` |
| No secret appears in any MCP message | The planted-secret test through both transports: results, later reads of the run, and protocol errors that quote the caller's input |
| Nothing that passed through the door is in the process's log: no secret of the engine, no caller's token, no caller's input | A test in a program of its own, with every event captured at the most verbose level |
| No client name reaches the kernel | Not proven. The label goes to the dispatcher as the caller's label and nowhere else |
| The proxy copies messages and adds nothing | A test that compares proxied and direct answers byte for byte |
| A lost answer to a tool call is a write in doubt, and a request refused before it was sent may be made again | The tests in `tests/lost.rs`, against an engine of the test's own that takes a request and goes away, never answers, breaks off a stream after a note, or answers 500 or 401 with no message |
| A line longer than a request may be is refused and not kept | The same file: a line of more than 4 MiB, then a line that is carried |
| `next` names a tool the caller's toolset lists | `a_next_step_names_a_tool_the_callers_toolset_lists` |
| A cancelled request is sent nothing more, and others are still answered | A test that holds a call open, cancels it through the proxy, and waits for the endpoint to see the stream close |
| The door offers no way to approve | A test over the tools of both toolsets, names an agent might try, and the registry |
| A list costs one dispatch for each page of published tools, and a call by a published tool's own name costs one | Tests that count what the endpoint handed the dispatcher |
| A list is whole when the result size cap is smaller than a page | A test with a cap that one tool fits under and the page does not: the same list, in more dispatches |
| The project folder is what the mount stated, and nothing a caller writes | A test with the folder stated, not stated, and written as an argument |
| A person is told once that a run waits, also when the call's progress was followed; the answer carries the link and the next call | The progress test: a write that rests, the same call again, and the approvals the person surface counted |
| A progress note holds nothing a system said | The same test: every note is one of this crate's sentences, and none holds a value the test application answered with |
| A resumed write that was followed is written once | The same test asks the test application what it stored and how often |
| What is not well formed never reaches the dispatcher | A test that counts what the endpoint handed the dispatcher |

## Build order

1. Done: one service, the stdio proxy as a line copier of our own, and the `use` toolset with `a2m_tool_call` for a read. Package 1.5.
2. Done: structured results, `next` inside them, annotations, the two revisions, cancelling, and progress from the run's events for `tool.call` and `run.resume`. Not built: resource links.
3. Not built: elicitation. The link fallback is in the result. Package 3.6.
4. Done: the `build` toolset and its action search tools, which are actions like any other; one sentence of server instructions, which names the fields of `a2m_action_run` from `action.run`'s own definition, held by a test ([decision 0219](../../decisions/0219-shadow-bindings-reach-the-product-and-the-first-hour-is-mended.md)). Not built: prompts, resources, instructions generated from the guidance file. Section 221 says nothing of the instructions; section 220 says they carry the handful of rules every caller needs and come from the same sources as the guidance file, which is now the `guidance` page of `docs.get`. The one sentence stays until that generation is built. Package 5.1.
5. Done: stateless Streamable HTTP with the `Host` check, as a router to nest at `/mcp/<toolset>`. Not built: token checks, protected resource metadata, named toolsets. Package 6.2.

Also not built: client profiles, the Tasks extension, compact schemas, and a byte budget for a list.

## Open points

- A default listing reads every published tool in full to list the pipelines among them, because `tool.search` has no filter by kind. A `kind` input on `tool.search` would make a target of five hundred operations cost one page.
- A tool call whose answer the proxy lost is answered with the exact next step (decision 0160): when it carried an idempotency key, in its `_meta` or as the argument of `a2m_tool_call`, the same call again with the same key; when it did not, `a2m_action_run` with `run.list` since a minute before the call was sent, by the proxy's clock, and the tool it named. The moment is shown and decides nothing.
- A followed call that comes to rest is made twice, so that the run's person is told. The kernel telling the person when a run comes to rest, whoever waits, would make `run.get` the answer in every case. Until then a `run.resume` that is followed is dispatched whole, so a note cannot say that the wait is over before the answer does.
- How many notes a run of very many records sends. A note for each page of events is bounded by how fast the client reads; a rate of its own is not built.
- Keep-alive comments on an event stream that stays open for minutes, which an intermediary that closes idle connections would need. The SDK sent them; this layer does not.
- Whether cancelling a request should also cancel the run it started.
- The byte budget for a tool list and for one compact schema.
- How compact schemas are derived from full ones mechanically, so that "exact or wider" is proven, not reviewed.
- How a client is told of a changed tool list with no session, if at all.
