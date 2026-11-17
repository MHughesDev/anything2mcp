# `a2m-pack-mcp`

The MCP pack: the `mcp` driver, which reaches an MCP server that a system already provides and re-exposes chosen tools of it with this runtime's policy, approval, audit, and verification around them. A mapping file names the only tools that can be reached. The server's own tool list is a discoverer, and a change to a mapped tool stops the bindings that use it.

## Facts

| | |
| --- | --- |
| Side | Pack: MCP |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5. It has no base crate and depends on no other pack crate |
| Depends on | `a2m-core` (with `Plan` and `Step`), `a2m-files` (registration), `a2m-auth` (`AuthProvider`, `SessionLease`, the neutral OAuth pieces), `a2m-provider` (`Driver`, `Discoverer`, `Describer`, `SandboxPort`, `PackContext` with its scoped supervisor, registration), `a2m-telemetry`. Outside: `rmcp`, the official Rust SDK, verified as existing ([research note 09](../../research/09-rust-building-blocks.md)); its client side and its revisions are to be confirmed |
| Built in | Package 8.2. Package E6, the optional discovery provider, comes after it |
| Carries out | Spec sections [201](../../spec/2-kernel/201-archetypes-and-modes.md) (`mcp`), [203](../../spec/2-kernel/203-the-provider-contract.md) (an upstream MCP server as a provider), [204](../../spec/2-kernel/204-supervising-subsystems.md) (starting a server program), [213](../../spec/2-kernel/213-effects-hazards-and-consent.md) (untrusted text), [402](../../spec/4-building-tools/402-the-scout-and-who-does-what.md) (drafting from a tool list) |

## Owns

- The mapping file kind: per upstream server, the tools that may be reached, each pinned by the digest of its definition.
- Connecting to an upstream server: starting a local server program through the supervisor its context scopes to the programs the mapping and the published plan declare, or reaching a remote one over HTTP.
- Calling a mapped tool with typed arguments, checking its result against the pinned output schema, and extracting values.
- Detecting that a server or a mapped tool changed.
- The `upstream_mcp` auth provider: the protocol's own authorization for a remote server.
- The discoverer over the server's tool list, its drafting rule, and its describer.

It must never: reach a tool the mapping does not name; pass an upstream tool's description, or any text from the server, to a caller as anything but marked untrusted data; believe the server's read-only marking for a write; retry; hand the server a credential beyond the one its mapping declares.

## Not here: `site_tools`

Spec section 605 and the roadmap's package 8.2 place the `site_tools` mode in this crate. It calls tools that a page registers in the browser, so it needs an acting tab, and only the web pack has one: a pack may not depend on another pack's crates. It is designed in `a2m-pack-web-ui`, where spec section 202 and the long-term plan already put it. The two modes share ideas, a pinned list of tools and a typed call, and no code. That placement is now settled.

## What it registers

| Registration | Content |
| --- | --- |
| Modes | `mcp`, archetype request |
| Surface kinds | `mcp` |
| Step kinds | `call_tool` for `mcp`, a checked name registered per mode |
| Driver | `McpDriver` |
| File kinds | `mappings/`: one file per `mcp` surface (below) |
| Binding fields | `tool`, `args` |
| Verb lowerings | Every verb the [verbs by mode](../../reference/technical-reference.md#verbs-by-mode) table allows for `mcp`, into `call_tool`. Each lowering compiles the mapping entry it uses into this crate's opaque step payload. A call to a tool mapped as a write is a commit point; a call to a mapped read tool by key is a lookup |
| Discoverers | `mcp_tools` |
| Finding kinds | `upstream_tool`, `upstream_server`, each a checked name with the schema of its `found` |
| Describer | `mcp_describer` |
| Auth providers | `upstream_mcp` |

### The mapping file

| Field | Meaning |
| --- | --- |
| `surface` (required) | The `mcp` surface: a server address, or a local program by path and digest with its arguments and an allowlisted environment |
| `server` | The name and version the server reported when the mapping was written |
| `tools` (required) | Map of the upstream tool's name to an entry: `digest` of its definition (name, input schema, output schema, annotations), `effect` as the developer judged it, `read_back` naming a mapped read tool for a write, and `untrusted_fields`. An entry may also state `result` (`structured` or `text`), `absent_error` (the text of the tool's error result that means the record is not there), `records` (the field of a list tool's result that holds the records), `key` (the fields of a result that make the record's key), `key_args` (the arguments that take the parts of a record's key, in order, so that a read-back and a lookup need no arguments of their own), `stamp` (the argument that carries a record's stamp), and `free_text: false` (a reviewer's statement that a tool whose whole input is one string does not run it) |
| `credential` | Which vault reference the server is given, and how: a header for a remote server, an environment variable for a local program. Not built: the mapping has no such field yet. A remote server is given the bearer token of the session's lease, through `upstream_mcp`, and a local program is given no credential |
| `purpose` | `tools`, or `discovery` for a server used only as a discovery provider, whose tools can never be bound to an operation |

The mapping is the upstream server's manifest in the sense of [section 203](../../spec/2-kernel/203-the-provider-contract.md): what it may be asked to do is what the mapping names. Its effects are `declared`, because the server runs code this runtime cannot see, and the person enabling it is told so.

### Lint rules

| Code | Level | Rule |
| --- | --- | --- |
| `mcp_tool_unmapped` | Error | A binding names a tool its surface's mapping does not |
| `mcp_tool_unpinned` | Error | A mapped tool with no definition digest |
| `mcp_program_unpinned` | Error | A local server program with no digest |
| `mcp_write_no_readback` | Error, unless the operation is `irreversible` | A write tool with no mapped read tool |
| `mcp_discovery_bound` | Error | A binding to a tool on a `discovery` mapping |
| `mcp_free_text` | Warning | A mapped tool whose input is one free-text string that the server could run as a command or a query. A reviewer decides; a tool that runs free text is refused by section 213 |

### Drafting rules

| From a finding of kind | The drafter writes | Left to the developer |
| --- | --- | --- |
| A tool on the server's list, marked read-only by the server | A mapping entry with its digest, and a pass-through read operation with one binding | Its name in the language of the work; whether the marking is believed |
| A tool not marked read-only | A mapping entry only, with `effect` left as an open question | The operation, its identity strategy, its read-back tool |

## Interface

A sketch.

```rust
pub fn register(reg: &mut Registration, ctx: &PackContext) -> Result<(), Failure>;

pub struct McpDriver { conns: Connections }
impl Driver for McpDriver {                                        // mode and capabilities are in the manifest
    /// A connection for the session's identity, started or reused; the tool list compared with the mapping.
    async fn prepare(&self, ctx: &RunContext, lease: &SessionLease) -> Result<Prepared, Failure>;
    async fn act(&self, p: &mut Prepared, step: &Step, input: &Values) -> Result<ActOutcome, Failure>;
    async fn health(&self, surface: &Surface) -> Health;
    async fn release(&self, p: Prepared);
}

/// Inside the crate. One connection per (surface, identity): a local program started by the
/// supervisor, or an HTTP session. Never shared between identities.
pub(crate) struct Connections { /* ... */ }

pub(crate) struct Pinned { tool: String, digest: Digest }
pub(crate) fn definition_digest(def: &ToolDefinition) -> Digest;   // canonical form, then SHA-256

pub struct UpstreamMcp; impl AuthProvider for UpstreamMcp { /* sign_in, is_signed_in, detect_expiry, refresh, sign_out, inject */ }
pub struct McpTools;    impl Discoverer for McpTools { /* lists, never calls */ }
```

What the kernel gives this crate:

- The engine reaches the driver through a handle that refuses a committing step without the ledger's intent token. Every failure says whether the call was not sent, was answered, or is unknown; a lost connection after a write call was sent is unknown. A step marked as a lookup may report a record as absent.
- The scoped supervisor starts a local server program by path and digest, hands it its credential privately, owns its process group, and reports whether a request on a lost connection ran ([section 204](../../spec/2-kernel/204-supervising-subsystems.md)).
- `SessionLease::inject` places a token in a request header, or an environment variable into a program's start, without returning it.
- When the protocol's authorization needs a person to consent in a browser, `upstream_mcp` asks through `PersonSurface`, defined in `a2m-actions` and reached through the keeper: the person is given a link and opens it in their own browser. Nothing drives that browser, and this pack borrows nothing from the web pack. The callback carries the authorization code back to the provider, which exchanges it for tokens the keeper holds.
- `mcp_tools` runs as a discovery job on a runner, inside the engine's sandbox, and is given only what `SandboxPort` offers.

### How the mode fills the loop

| Prepare | Act | Confirm | Read back |
| --- | --- | --- | --- |
| The connection; the server's name and version; each mapped tool's definition, digested and compared with its pin | `call_tool` with arguments built from `args` and checked against the pinned input schema | The result is not an error and its structured content fits the pinned output schema | The mapped read tool, called with the record's key |

**Verification is added, not borrowed.** A server's own success answer is evidence that it accepted a call, never that the record holds what was sent. A write is verified only by the read tool, compared after normalization by the engine.

### Change detection

| What changed | Found when | Result |
| --- | --- | --- |
| A mapped tool's definition | `prepare`, the canary, and on the server's notice that its list changed | Calls to that tool stop as `drift`; bindings on it fall to `shadow` until the mapping is re-pinned and the gates pass |
| A mapped tool is gone | The same | The same |
| An unmapped tool is added | `mcp_tools` | A finding. Nothing is reachable until a person maps it |
| The server's reported version | `prepare` | A finding, and the bindings' levels fall ([section 401](../../spec/4-building-tools/401-overlaying-tools-on-an-existing-system.md)); a call still runs if every pin matches |

### The entry bar

| # | Item | This crate's answer |
| --- | --- | --- |
| 1 | A driver, no retries inside | `McpDriver` |
| 2 | Read-back for every write | The mapped read tool; without one, `irreversible` |
| 3 | Each identity strategy | `natural`: the read tool by key. `stamped`: a field the tool accepts and its read or list tool returns. `find`: a list or search tool with `settle`. `composite` where the read tool takes a parent and a child key. `none`: `irreversible` |
| 4 | A test stand | A small MCP server at `targets/example-mcp`, over the example data, with the control interface: inspect, count writes, change a tool's definition, announce a list change, drop the connection after a commit, return text with instructions aimed at an agent |
| 5 | A discoverer | `mcp_tools`. Listing tools is a read the protocol defines; it never calls one |
| 6 | Failures mapped | Below |
| 7 | Effects and hazards | Starts a process: the mapped program, by path and digest. Network: the mapped server address. Holds a session. The upstream's own effects are `declared`. Hazards come from operations |
| 8 | A record per new mode | None: decision 0005 |

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-engine`, through the registry | `McpDriver`, only |
| The session keeper in `a2m-auth`, through the registry | `UpstreamMcp`, only |
| `a2m-files`, through the gathered set | The mapping schema, lowerings, lint and drafting rules |
| A runner, inside the engine's sandbox | `mcp_tools`, for discovery jobs |
| `a2m-actions`, through the registry | The describer; the mapping as a provider manifest for `provider.list` |

## State

| State | Where |
| --- | --- |
| Open connections and started programs, by surface and identity | Memory and the supervisor's listing |
| The mapping and its pins | The project folder, as a target file |
| Tokens for a remote server | The keeper; one request borrows them |

## Failures

| Class | Codes | When |
| --- | --- | --- |
| `drift` | `upstream_tool_changed` | A pin does not match |
| `drift` | `response_schema_mismatch` | A result that does not fit the pinned output schema |
| `rejected` | `app_validation_error` | The tool's own error result, for a call that was well formed |
| `unknown_variant` | `unexpected_status` | A protocol error the mapping did not foresee |
| `possible_commit` | `no_confirmation` | A write call was sent and the connection or the program was lost before its result |
| `session_expired` | `needs_login` | The server refused the credential |
| `transient` | `timeout`, `connection_lost`, `upstream_unavailable` | Before a call was sent |
| `policy` | `not_granted` | A call to a tool that is not mapped, if one were ever attempted; the compiler should have refused it first |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Only mapped tools are reachable | A test calling an unmapped tool through every route; the stand counts no call |
| A changed tool stops its bindings | The stand changes a definition; the next call is `drift` and does not reach the stand |
| A write lost in flight is `possible_commit`, and reconcile leaves one write | The stand drops the connection after a commit |
| Text from the server reaches a caller only marked untrusted | Test-application case 27, played by the stand |
| Discovery calls no tool | The stand counts calls during `mcp_tools` |
| Two identities never share a connection | A test with two sessions on one server |

## Build order

1. Done: the test stand, with its control interface, at `targets/example-mcp`.
2. Package 8.2: the mapping schema, `McpDriver` for reads against a local stand program, change detection.
3. Writes with read-back, and the fault cases.
4. `mcp_tools` and its drafting rule; `upstream_mcp` for a remote stand.
5. Package E6: a `discovery` mapping, whose results are stored as findings at `inferred` and never bound. Hypothesis H-27.

## Open points

- Which protocol revisions the client side speaks, after the server-side answer of slice 0.5.
- Where the consent link's callback lands on a server, where the person's browser cannot reach the runner.
- Whether a mapped tool's resources and prompts are ever reachable. Not in this design: tools only.
- Whether an upstream server that holds one shared credential may serve several identities. This design gives each identity its own connection and refuses sharing.
- Hypothesis H-19 asks whether `site_tools` and `mcp` are one driver. With the two in different packs, the answer here is that they share a shape and not a driver.
