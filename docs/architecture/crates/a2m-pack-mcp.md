# `a2m-pack-mcp`

The MCP pack: the `mcp` driver, which reaches an MCP server that a system already provides and re-exposes chosen tools of it with this runtime's policy, approval, audit, and verification around them. A mapping file names the only tools that can be reached. The server's own tool list is a discoverer, and a change to a mapped tool stops the calls to it. The pack is designed in [spec section 304](../../spec/3-packs/304-the-mcp-pack.md); this document says how its crate is put together and what is built. It was built as package 8.2; what building it settled is in decisions [0108](../../decisions/0108-an-upstream-mcp-server-is-reached-only-through-a-pinned-mapping-by-a-client-of-the-packs-own.md) and [0109](../../decisions/0109-the-mcp-pack-as-building-it-settled-it.md), and what a reading of its code against this document then closed is in [decision 0136](../../decisions/0136-the-mcp-pack-refuses-free-text-reads-its-list-again-before-a-write-and-passes-on-only-listed-members.md).

## Facts

| | |
| --- | --- |
| Side | Pack: MCP |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5. It has no base crate and depends on no other pack crate |
| Depends on | `a2m-core` (with `Plan` and `Step`), `a2m-files` (registration), `a2m-auth` (`AuthProvider`, `SessionLease`, `CredentialSink`), `a2m-provider` (`Driver`, `Discoverer`, `Describer`, `SandboxPort`, `ScopedSupervisor`, registration), `a2m-telemetry` (the clock a finding is dated with). Outside: `reqwest` with its default features off, so with no TLS back end; `serde`, `serde_json`; and `tokio` with its `io-util` feature. No protocol SDK: the client is the pack's own ([decision 0108](../../decisions/0108-an-upstream-mcp-server-is-reached-only-through-a-pinned-mapping-by-a-client-of-the-packs-own.md)), as the service side is ([decision 0130](../../decisions/0130-the-mcp-protocol-layer-is-our-own-because-the-sdk-brings-an-unmaintained-crate.md)) |
| Roadmap packages | Package 8.2. Package E6, the optional discovery provider, is withdrawn (decision 0279) |
| Carries out | Spec sections [201](../../spec/2-kernel/201-archetypes-and-modes.md) (`mcp`), [203](../../spec/2-kernel/203-the-provider-contract.md) (an upstream MCP server as a provider), [204](../../spec/2-kernel/204-supervising-subsystems.md) (starting a server program), [213](../../spec/2-kernel/213-effects-hazards-and-consent.md) (untrusted text), [402](../../spec/4-building-tools/402-the-scout-and-who-does-what.md) (drafting from a tool list) |

## Owns

- The mapping file kind: per `mcp` surface, the tools that may be reached, each pinned by the digest of its definition.
- The `mcp` surface kind's own fields: the address, and for a local server program the program by path and digest, its arguments, and its environment.
- Connecting to an upstream server: starting a local server program through the scoped supervisor, which starts only programs the published plan declares, or reaching a remote one over HTTP.
- A protocol client of its own: JSON-RPC 2.0 over HTTP or over a program's standard streams, in the revisions 2025-11-25, 2025-06-18 ([decision 0365](../../decisions/0365-an-mcp-servers-json-answered-as-text-is-a-declared-result-form-and-the-pack-speaks-the-earlier-handshake-revision.md)), and 2026-07-28.
- Calling a mapped tool with arguments checked against its pinned input schema, checking its result against the pinned output schema, and extracting values.
- Detecting that a mapped tool changed.
- The `upstream_mcp` auth provider: a bearer token for a remote server.
- The discoverer over the server's tool list, its describer, and its drafting rule.

It must never: reach a tool the mapping does not name; call a tool a reviewer found to run free text, or a tool whose whole input is one string that no reviewer has cleared; use a connection for a far end other than the one the run's plan names; pass an upstream tool's description, or any text from the server, to a caller as anything but marked untrusted data; pass on a member of a result that the mapping does not list; quote the server in a failure; believe the server's read-only marking for a write; retry; hand the server a credential beyond the one its session's lease places.

## Not here: `site_tools`

Spec section 605 and the roadmap's package 8.2 place the `site_tools` mode in this crate. It calls tools that a page registers in the browser, so it needs an acting tab, and only the web pack has one: a pack may not depend on another pack's crates. It is designed in `a2m-pack-web-ui`, where spec section 202 and the long-term plan already put it. The two modes share ideas, a pinned list of tools and a typed call, and no code. That placement is now settled.

## What it registers

| Registration | Content |
| --- | --- |
| Provider | `mcp` |
| Mode | `mcp`, archetype request, rank 30, with the verbs `read`, `list`, `search`, `create`, `update`, `upsert`, `delete`, `download`, `submit`, and `call`. Four reads and one write at a time on a session |
| Surface kind | `mcp`, with a session. Fields: `address`, an `http://` address, or `stdio` for a local server program, which states `program` (its absolute path), `digest`, `args`, `environment`, and `pass_environment`; `revision`, `2025-11-25` (the default, which starts with `initialize`), `2025-06-18` (the same handshake, [decision 0365](../../decisions/0365-an-mcp-servers-json-answered-as-text-is-a-declared-result-form-and-the-pack-speaks-the-earlier-handshake-revision.md)), or `2026-07-28` (no handshake); `unencrypted_credentials: accepted`, the one way to let a session's token cross plain `http` to an address that is not a loopback address ([decision 0188](../../decisions/0188-credentials-over-plain-links-drafted-workbooks-and-published-device-maps.md)). The program and its digest are declared as the surface's program, so the plan declares them ([decision 0124](../../decisions/0124-the-plan-declares-a-surfaces-programs-and-a-schema-checks-run-time-values.md)) |
| Step kinds | `call_tool`, from lowerings; `list_tools`, which only the discoverer sends, through the sandbox port |
| Driver | `McpDriver` |
| File kind | `mapping`, in `mappings/`, for `mcp` surfaces. A mapped tool is the defined name `surface/tool`, so the kernel refuses one tool mapped twice for a surface |
| What saves, for the compiler | `Lowering::commits`: `call_tool` is `AsDeclared`, because the same kind calls a tool that reads and one that writes, which is the `effect` the mapping gives the tool; the lowering marks it `commits` exactly for the binding's own act when the tool is mapped as a write, and the driver refuses a write-mapped tool from any step not so marked, in every posture. `list_tools` is `Never`. No kind always saves, so the manifest lists no `commit_steps`. `Lowering::least_effect`: `write` for `create`, `update`, and `upsert`; `irreversible` for `delete` and `submit`; `read` for the rest, where the mapping's effect holds the operation ([decision 0154](../../decisions/0154-the-http-command-and-mcp-packs-state-what-saves-what-writes-and-which-key-is-sent.md)) |
| What can describe a write | `FileKind::writes` on a mapping: it does when any tool is mapped as anything but `read`. A tool whose effect is still an open question, as the drafter leaves every tool the server does not mark read-only, does not count: the file does not compile until a person answers it, and a mapping alone calls nothing |
| A key argument | A tool's `key_args` are filled from the key the engine gives the step. A read belongs to no commit point and is given no key, so the lowering states each of its `key_args` as an argument of its step, from the operation's key inputs in order, where the binding states none of its own [decision 0365](../../decisions/0365-an-mcp-servers-json-answered-as-text-is-a-declared-result-form-and-the-pack-speaks-the-earlier-handshake-revision.md). For an operation with no key input that key is the record's place in the run, which is never sent. The lowering refuses (`mcp.binding_incomplete`) a step whose tool has a `key_args` argument that the step does not state itself and no key input fills; a read-back after a write is exempt when the written tool reports the assigned key under `key` |
| Binding fields | `tool`, `surface` (the target's first `mcp` surface when left out), `args`, `timeout_ms`, `settle_ms`, `read_back` with its own `args`, and `lookup` with a `tool` and its `args` |
| Verb lowerings | Every verb into one `call_tool` step (below) |
| Discoverer | `mcp_tools`, at `discovered` |
| Finding kinds | `upstream_tool`, `upstream_server` |
| Describer | `McpDescriber` |
| Drafting rule | From `upstream_tool` findings to mapping entries |
| Auth provider | `upstream_mcp` |
| Effects | No program of its own; the plan's programs; network to the surfaces; a session for `mcp` |
| Settings | `max_message_bytes` (4 MiB), `max_tools` (500), and `list_max_age_ms` (10 seconds): how old the tool list a read is compared with may be on a connection that gives no notice of a change |

### The mapping file

| Field | Meaning |
| --- | --- |
| `surface` (required) | The name of the `mcp` surface the mapping is for. The address and the program are fields of that surface in `target.yaml`, so an environment can override the address |
| `server` | The name and version the server reported when the mapping was written |
| `tools` (required) | Map of the upstream tool's name to an entry: `digest` of its definition, `effect` (`read`, `write`, or `irreversible`) as the developer judged it, `read_back` naming a mapped read tool for a write, and `untrusted_fields`, the members of a structured result that hold text and may be passed on. An entry may also state `result` (`structured`, the default; `text`; or `json_text`, text items that are JSON, [decision 0365](../../decisions/0365-an-mcp-servers-json-answered-as-text-is-a-declared-result-form-and-the-pack-speaks-the-earlier-handshake-revision.md)), `absent_error` (the start of the tool's error text that means the record is not there), `records` (the member of a list tool's result that holds the records), `key` (the members of a result that hold a key the server assigned), `key_args` (the arguments that take the parts of a record's key, in order), `stamp` (the argument that carries a record's stamp), and `free_text` (what a reviewer found the server does with the text the tool is given: `false`, it does not run it, which a tool whose whole input is one string needs before it is called; `true`, it runs it as a command, a query, or a prompt, and the tool is never called) |
| `purpose` | `tools`, the default, or `discovery` for a server used only as a discovery provider, whose tools can never be bound to an operation |

A mapping has no `credential` field yet. A remote server is given the bearer token of the session's lease, through `upstream_mcp`; a local program is given no credential.

The digest is SHA-256 over one canonical JSON text, object keys in byte order and no spaces, of the tool's name, input schema, output schema, and annotations. The title and the description are not part of it.

The mapping is the upstream server's manifest in the sense of [section 203](../../spec/2-kernel/203-the-provider-contract.md): what it may be asked to do is what the mapping names. Its effects are `declared`, because the server runs code this runtime cannot see, and the person enabling it is told so.

### Lint rules

The first six are registered rules that look across files. The last four are raised where a file is compiled or a binding lowered.

| Code | Level | Rule |
| --- | --- | --- |
| `mcp.tool_unmapped` | Error | A binding, or its lookup, names a tool its surface's mapping does not |
| `mcp.tool_unpinned` | Error | A mapped tool with no definition digest, or one that is not a SHA-256 digest |
| `mcp.program_unpinned` | Error | A local server program with no SHA-256 digest |
| `mcp.write_no_readback` | Error | A tool mapped `write` that names no mapped read tool, or one whose result is `text`, which gives a read-back no field ([decision 0365](../../decisions/0365-an-mcp-servers-json-answered-as-text-is-a-declared-result-form-and-the-pack-speaks-the-earlier-handshake-revision.md)). A tool mapped `irreversible` needs none |
| `mcp.discovery_bound` | Error | A binding to a tool on a `discovery` mapping |
| `mcp.environment_refused` | Error | A local server program is handed a variable that starts with `A2M_` (any letter case), that makes a program load code other than its own (the provider's closed list, `redirects_loading`, [decision 0250](../../decisions/0250-one-check-of-who-may-change-a-program-and-the-mcp-packs-local-server-on-windows.md)), or that is named for a credential, in `environment` or `pass_environment`. The driver refuses the same names again before it starts the program, so a plan changed by hand starts nothing. A credential for a local server goes through the session keeper, not the environment ([decision 0158](../../decisions/0158-the-product-crate-and-the-local-engine-after-their-safety-audit.md), finding P6) |
| `mcp.unencrypted_credentials` | Warning | A surface declares `unencrypted_credentials: accepted`: its credentials cross an unencrypted link to another machine |
| `mcp.free_text` | Warning at the mapping, error at a binding | A tool whose entry says `free_text: true` is never called. The entry may stand as the record of a review; a binding that uses the tool, as its act, its read-back, or its lookup, does not compile |
| `mcp.effect_understated` | Error | An operation states less than the tool it calls, or a reading verb calls a write tool |
| `mcp.untrusted_unmarked` | Error | An output of an operation bound in this mode that is or holds text, and whose field type is not marked untrusted. Every value of the mode comes from the server, from the act, the read-back, or the lookup. A choice is not text: the kernel admits only its codes |
| `mcp.binding_incomplete` | Error | No tool, no `mcp` surface, a lookup tool that is not a read saying how an absent record shows, or a `stamped` operation whose tool names no `stamp` |
| `mcp.mapping_conflict` | Error | Two mapping documents of one surface state different purposes, or a mapping cannot be encoded |

### Drafting rules

| From a finding of kind | The drafter writes | Left to the developer |
| --- | --- | --- |
| A tool on the server's list, marked read-only by the server | A mapping entry with its digest and `effect: read`; and, when nothing in the entry is left open, a pass-through read operation `{target}_{tool}`, its inputs the input schema's plain properties passed by name, its outputs the output schema's plain properties (text as `untrusted_text`) or `text` where it states none, and its example the schema's own `examples` or `default` for every required input ([decision 0179](../../decisions/0179-what-each-pack-drafts-so-that-a-scout-ends-with-reads-that-work.md)) | Whether the marking is believed; a nested output; the operation's name, its test and fixture |
| A tool not marked read-only | A mapping entry with its digest, and `effect` as an open question | The effect, the operation, its identity strategy, its read-back tool |

Both also propose `untrusted_fields` from the output schema's free-text members, and `result: text` for a tool with no output schema. For a tool whose whole input is one string, `free_text` is left as an open question: whether the server runs the string is not in its definition, and no read is drafted for it until the target's mapping, outside the holding folder, answers `free_text: false` at the digest the server lists. For every tool the read rule drafts nothing for, it says why, in the pack's own sentences, and it counts the properties it left out as a secret's [decision 0365](../../decisions/0365-an-mcp-servers-json-answered-as-text-is-a-declared-result-form-and-the-pack-speaks-the-earlier-handshake-revision.md). An entry that holds a question is drafted into `mappings/{surface}_undecided.yaml`, apart from the settled entries in `mappings/{surface}.yaml`, so that it does not hold up the reads beside it.

**A property named as a secret is never drafted as an input or an output.** The read rule asks the compiler's own test (`names_a_secret` in `a2m-files`) of each property of the tool's input schema and of its output schema, as the server writes the name and as the input or output name it becomes. An output so named is left out, and the last sentence of the drafted description tells the names left out; an optional input so named is no input; a tool with a required input so named is not drafted as a read ([decision 0302](../../decisions/0302-a-field-named-as-a-secret-is-never-drafted-as-an-input-or-an-output.md)).

## Interface

```rust
pub fn pack() -> McpPack;                  // for `RegistryBuilder::add_pack`
pub struct McpPack;                        // impl Pack
pub const MANIFEST: &str;                  // the manifest's text
pub mod codes;                             // the pack's failure codes in one failure_codes! table (decision 0258), and PASSED_ON

pub struct McpDriver { /* connections, redactor, limits */ }   // impl Driver
pub struct UpstreamMcp;                    // impl AuthProvider
pub struct McpTools;                       // impl Discoverer: lists, never calls
pub struct McpDescriber;                   // impl Describer
pub struct ToolDefinition { /* name, title, description, schemas, annotations */ }
pub fn definition_digest(definition: &ToolDefinition) -> Digest;   // canonical form, then SHA-256
```

The names the pack registers are public constants in the crate root: `PACK`, `SURFACE_KIND`, `MAPPING_KIND`, `CALL_TOOL`, `LIST_TOOLS`, and the `Names` it builds once.

Inside the crate, the client is the only code that knows the protocol. It lists tools, following cursors for at most 200 pages and at most `max_tools` tools; calls one tool, once; and tells the server a request is cancelled when the caller stops waiting. On the handshake revision it sends `initialize` and stops if the server offers another revision. On the other it names the revision in every request. Nothing is probed. Every failure it returns says whether the request left: not sent, answered, or unknown.

**The compiled section** (format 1) holds every surface's mapping, by surface. **The step payload** of `call_tool` is the surface and the tool; everything else about the tool is read from the mapping, so it is written once.

What the kernel gives this crate:

- The engine reaches the driver through a handle that refuses a committing step without the ledger's intent token, or in a posture that may not write. A step marked as a lookup may report a record as absent. The handle turns an unknown failure on a committing step into `possible_commit`/`no_confirmation`.
- The provider's check of who may change a program, `check_program`, which the command pack's programs are held to as well ([decision 0250](../../decisions/0250-one-check-of-who-may-change-a-program-and-the-mcp-packs-local-server-on-windows.md)). The driver asks it of a local server program, as a program and with no group named as the administrators' beyond the system's own, before every start: the file and every folder on the way must be closed to other users, and a shell, an interpreter, a batch file, or a script (`#!`) is refused by name or by its first bytes. The pack has no setting that names a group; the command pack's `packs.command.admin_groups` is that pack's.
- The scoped supervisor starts a local server program by path and digest, only as the published plan declares it, over its standard streams (`start_on_standard_streams`), owns its process group on Linux and macOS and its job object on Windows, and stops it. On Linux and macOS the two ends of its link are placed on descriptors 0 and 1; on Windows they are its standard handles and it is started with no console window. Its standard error goes to the null device on every system. Its environment is only what the surface sets and passes, the owner mark, and on Windows `SystemRoot`, which the driver adds to what passes.
- `SessionLease::apply` places the session's credential into a `CredentialSink` without returning it. The pack takes request headers only.
- `mcp_tools` runs as a discovery job inside the engine's sandbox, and is given only what `SandboxPort` offers.

### How a binding lowers

| Verb | `act` | `read_back` | `lookup` |
| --- | --- | --- | --- |
| Any | One `call_tool` of the binding's tool, with its `args`. It commits when the tool is mapped as a write; a `read` of a tool that says how an absent record shows is a lookup | For a write: one `call_tool` of the mapped read tool, with the binding's `read_back.args`, or else the record's key through the read tool's `key_args`, or else the operation's key inputs by name | The binding's `lookup` tool, a read whose entry names `records` or `absent_error`, with its `args` |

No step is idempotent, and there is no confirm step. A step's time limit is the binding's `timeout_ms`, else the target's default `step`, else 30 seconds. Identity: `natural`, `composite`, and `none` always; `stamped` and `find` when the binding names a `lookup` tool. A `stamped` write carries the run's stamp in the argument its entry names.

### How the mode fills the loop

| Part | What the driver does |
| --- | --- |
| Prepare | Works out the far end the run's surface names, and refuses an address that is not among the run's destinations, or a program the plan does not declare for the surface, whether or not a connection is open. A program that runs is reused only when the digest the supervisor measured is the one the surface pins. Takes the connection for the target, environment, surface, session, epoch of its credentials, far end, and revision, if it is still alive, or opens one; reads the server's tool list |
| Act | Refuses, unsent, a tool the mapping does not name, any tool of a `discovery` mapping, a write tool from a step that does not commit, and a tool whose entry says `free_text: true`. Reads the list again when it must (below). Compares the called tool's definition with its pin. Refuses a tool whose whole input is one string unless its entry says `free_text: false`. Builds the arguments, checks them against the pinned input schema, and calls the tool once |
| Confirm | Within the act: the result is not an error, its structured content fits the pinned output schema, and it holds what the mapping names |
| Read back | The mapped read tool, called with the record's key, compared after normalization by the engine |
| Release | Keeps the connection for the identity's next run, unless the run ended in doubt or the connection was lost; then closes it, and the program with it |

**Verification is added, not borrowed.** A server's own success answer is evidence that it accepted a call, never that the record holds what was sent.

**What a result becomes.** A tool's error result is `rejected`, or absent for a lookup whose error text starts with `absent_error`. A `result: text` tool gives one value, `text`, its text blocks joined. A `result: json_text` tool's text items are read as JSON: an empty item holds nothing; with `records` named every item is a row, an item that is a list giving each element, listed under that name, so no item is an empty list; without it the one item that is an object is the result. Text that is not JSON is `drift`/`response_schema_mismatch`. The members are then passed on as a structured result's are [decision 0365](../../decisions/0365-an-mcp-servers-json-answered-as-text-is-a-declared-result-form-and-the-pack-speaks-the-earlier-handshake-revision.md). A structured result gives the members the mapping lists, and no others: the ones the entry names under `key`, `records`, or `untrusted_fields`, and the ones the pinned output schema declares as something that cannot hold free text, which is a number, a truth value, nothing, or a value from a closed list. A member that is not listed is left out. So a tool with no output schema gives only what its entry names. What the entry names must be there: the member under `records` is a list of records, and each member under `key` is text or a whole number, or the step stops as `drift`/`response_schema_mismatch`. A lookup through a list tool takes the one record in `records`: none is absent, more than one is `possible_commit`/`ambiguous_match`. The members under `key` give the assigned key. Every value and the capture of the whole result pass the redactor.

**When the list is read again.** Before every call that commits, on every connection: a write is compared with a list read for it. That costs one more listing, as many requests as the list has pages, for each write. Before a read: on the server's notice, where the connection gives notice, which is a local program that said so in its handshake; and where it gives none, which is every HTTP connection and every connection with no handshake, when the list held is older than `list_max_age_ms`. A read may therefore run against a pin up to that old. A listing that fails before a call leaves the call unsent.

**The schema check** covers the keywords the pack reads: `type`, `enum`, `const`, properties, required members, items, and the like. Discovery names the keywords in a schema that it does not check.

### Change detection

| What changed | Found when | Result |
| --- | --- | --- |
| A mapped tool's definition | Before each call: the list read at `prepare`, read again before every write, and before a read on the server's notice or when it is older than `list_max_age_ms` | Calls to that tool stop as `drift`/`upstream_tool_changed`, unsent; calls to other tools still run. Nothing lowers a binding's level yet |
| A mapped tool is gone | The same | The same |
| An unmapped tool is added | `mcp_tools` | A finding with `pin: unmapped`. Nothing is reachable until a person maps it |
| The server's reported version | `mcp_tools` | An `upstream_server` finding that says whether it changed. Comparing it at `prepare` and lowering the bindings' levels is not built; a call still runs if every pin matches |

### The entry bar

| # | Item | This crate's answer |
| --- | --- | --- |
| 1 | A driver, no retries inside | `McpDriver` |
| 2 | Read-back for every write | The mapped read tool; without one, the tool must be mapped `irreversible` |
| 3 | Each identity strategy | `natural` and `composite`: the read tool with the record's key, through `key_args`. `stamped`: the stamp argument, and a lookup tool that lists records. `find`: a lookup tool. `none`: nothing is read back |
| 4 | A test stand | [`targets/example-mcp`](../../../targets/example-mcp/README.md), over HTTP and as a local program |
| 5 | A discoverer | `mcp_tools`. It sends one `list_tools` step and never calls a tool |
| 6 | Failures mapped | Below |
| 7 | Effects and hazards | The plan's programs; network to the surfaces; a session. The upstream's own effects are `declared`. Hazards come from operations |
| 8 | A record per new mode | None: decision 0005 |

### After the kernel's second pass (decisions 0161 and 0162)

- Every step input is built with `StepInput::new` and `for_record`. A `call_tool` and a `list_tools` push the `acting` capture: `means` the tool and whether its result was an error, `measured` the size of the result or the count of tools. Five runs of one read give the same `means`.
- An upstream tool's error text is carried by `StepFailure::with_said`: redacted, cleaned, and redacted again. The failure's sentence says only that the tool refused the call.
- `Lowering::evidence` returns `acknowledged` for an operation declared `irreversible`, with identity `none`, whose binding names no `read_back` and whose act saves. Since the lint `mcp.write_no_readback` refuses a tool mapped `write` with no read tool, such an operation calls a tool mapped `irreversible`. Every other write keeps its read-back.
- `delete` and `submit` are `irreversible`. **Not done:** a mapping entry whose effect is an open question still does not count as a write in `FileKind::writes`; the drafter leaves the effect of every tool the server does not mark read-only as one, and counting it makes the drafter refuse those drafts (see the report of the package).
- Not moved to expressions: `key_args` in a mapping entry, which names the arguments that take the parts of a record's key. It could be `args: {order_id: record.key.0}` in the binding; the entry's `key_args` is kept because it serves the lookup and read-back tools, which a binding does not give arguments to.
- Keys: a record with no key gets no key argument; the call is then checked against the tool's input schema and stops unsent, `arguments_invalid`, when a required argument is missing.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| The product crate | `pack()`, once |
| `a2m-engine`, through the registry | `McpDriver`, through its handle |
| The session keeper in `a2m-auth`, through the registry | `UpstreamMcp`, only |
| `a2m-files`, through the gathered set | The surface kind, the mapping schema, the lowering, the lint and drafting rules |
| A runner, inside the engine's sandbox | `mcp_tools`, for discovery jobs |
| `a2m-actions`, through the registry | The describer; the mapping as a provider manifest |

## State

| State | Where |
| --- | --- |
| Open connections and started programs, by target, environment, surface, session, epoch, far end (the address, or the program with its digest, arguments, and environment), and revision | Memory in the driver, and the supervisor's listing. A connection of the same surface and session under another key is closed, and its program with it, when a newer one is asked for: its credentials were replaced, or the plan now names another far end or revision |
| A run's connection, mapping, and the tool list last read, with when it was read | Memory, in the run's `Prepared` |
| The mapping and its pins | The project folder, as a target file |
| The bearer token | The keeper; the lease places it in a request header |

## Failures

| Class | Codes | When | Sent |
| --- | --- | --- | --- |
| `policy` | `not_granted` | A tool the mapping does not name, an unpinned tool, a tool of a `discovery` mapping, a write tool from a step that does not commit, a tool whose entry says `free_text: true`, a one-string tool whose entry does not say `free_text: false`; an address that is not among the run's destinations, or a local program the plan does not declare for the surface, at every `prepare` | Not sent |
| `input` | `arguments_invalid` | Arguments that do not fit the pinned input schema, or a value with no form a tool takes | Not sent |
| `policy` | `program_path_unsafe` | A local server program another user could change, through its file or a folder or link on the way to it; one whose access list cannot be read; one on another machine's share; a shell, an interpreter, a batch file, or a script. Refused before it starts, at every `prepare` that opens a connection ([decision 0250](../../decisions/0250-one-check-of-who-may-change-a-program-and-the-mcp-packs-local-server-on-windows.md)) | Not sent |
| `input` | `program_not_found` | The contract's code: a local server program that is not there, or cannot be run | Not sent |
| `policy` | `link_not_encrypted` | A session on an `http://` surface whose address is not a loopback address written plainly as an IP address, or that the URL parser cannot read, where the surface does not declare `unencrypted_credentials: accepted`; refused at every `prepare`, and by the link before each request | Not sent |
| `input` | `tls_not_enabled` | An `https://` surface in a build without the feature `tls` | Not sent |
| `input` | `tls_setup_failed` | No TLS configuration could be made on this machine | Not sent |
| `unknown_variant` | `certificate_untrusted`, `certificate_name_mismatch`, `certificate_expired`, `certificate_not_verified`, `tls_handshake_failed` | An `https://` surface whose certificate or handshake fails. The session keeper's codes, given by `tls::refusal` of `a2m-auth` | Not sent |
| `input` | `not_built` | A credential for a local program, a credential that is not a request header, the protocol's authorization flow | Not sent |
| `input` | `payload_unreadable` | The plan's section or a step's payload does not decode; an address that is neither `http://`, `https://`, nor `stdio` with a program | Not sent |
| `drift` | `upstream_tool_changed` | A pin does not match, or the tool is gone; a server that answers a call with "invalid parameters" | Not sent; answered |
| `drift` | `response_schema_mismatch` | A structured result that does not fit the pinned output schema, or that lacks what the entry names: a `records` member that is a list of records, a `key` member that is text or a whole number | Answered; unknown after a write |
| `rejected` | `app_validation_error` | The tool's own error result | Answered |
| `unknown_variant` | `unexpected_status` | A protocol error the pack has no rule for, another revision, a status with no rule, an answer that is not a response, an answer sent as an event stream, an answer longer than `max_message_bytes`, a list longer than `max_tools` | Answered; unknown |
| `possible_commit` | `ambiguous_match` | A lookup found more than one record | Answered |
| `session_expired` | `needs_login` | Status 401: the server refused the credential | Answered |
| `transient` | `timeout`, `connection_lost`, `upstream_unavailable` | No answer by the deadline; a connection or program lost, or a server that no longer knows the session; a server that cannot be reached, or answers 429, 502, 503, or 504 | Not sent, answered, or unknown |

`no_confirmation` is the contract's code: the handle raises it for an unknown failure on a committing step, so the manifest does not declare it. The supervisor's own refusals, such as a program whose digest is not the pinned one, pass through unsent.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| A registry with only this pack builds, its digest is stable, and the manifest declares exactly the codes the pack raises | `a_registry_with_only_this_pack_builds_and_its_digest_is_stable`, `every_failure_code_of_the_pack_is_declared_in_its_manifest_with_its_one_class`, in `tests/pack.rs` |
| The fixture compiles twice to the same bytes, and its pins are the stand's definitions | `the_fixture_target_compiles_twice_to_the_same_bytes`, `the_fixture_pins_are_the_stands_definitions`, in `tests/pack.rs` |
| What the mapping does not allow is refused with a file, a line, and a fix | `what_the_mapping_does_not_allow_is_refused_with_a_file_a_line_and_a_fix`, in `tests/pack.rs` |
| A digest pins the definition and not the server's words; a value is checked by the keywords the pack knows, and the others are named | `a_digest_pins_the_definition_and_not_the_servers_words`, `a_value_is_checked_by_the_keywords_the_pack_knows`, `keywords_that_are_not_checked_are_named`, in `src/json.rs` |
| Only mapped tools are reachable | `only_mapped_tools_are_reachable`, in `tests/driver.rs`; the stand counts no call |
| A tool that runs free text is never called, whatever compiled the plan, and no binding uses one | `a_tool_that_runs_free_text_is_never_called`, in `tests/driver.rs`; `a_tool_that_runs_free_text_can_be_recorded_and_never_bound`, in `tests/pack.rs` |
| A connection serves only the far end and the revision the run's plan names, and a run that may not reach an address is refused although a connection to it is open | `a_connection_is_reused_only_for_the_far_end_the_plan_names`, in `tests/driver.rs`; `a_plan_published_again_with_another_program_digest_does_not_use_the_running_program`, in `tests/stdio.rs` |
| A tool that changes within a run is not called against its old pin: never for a write, and for a read no later than the setting allows | `a_tool_that_changes_within_a_run_is_not_called_against_its_old_pin`, `a_read_is_compared_with_a_list_no_older_than_the_setting_allows`, in `tests/driver.rs` |
| A structured result gives only what the mapping lists, and stops when it lacks what the mapping names | `a_structured_result_gives_only_what_the_mapping_lists_and_is_checked_against_it`, in `tests/driver.rs` |
| An output that holds text is marked untrusted, for a read, a write, and a tool whose result is text | `what_the_mapping_does_not_allow_is_refused_with_a_file_a_line_and_a_fix`, in `tests/pack.rs` |
| A changed tool stops its calls and does not reach the stand; another tool still runs; a changed description or server version changes nothing | `a_changed_tool_is_drift_and_the_call_does_not_reach_the_stand`, in `tests/driver.rs` |
| A read returns what the stand holds, five runs observe the same, and one connection serves the identity | `a_read_returns_what_the_stand_holds_and_five_runs_observe_the_same`, in `tests/driver.rs` |
| A write is sent once, only with the ledger's token, and read back from the stand | `a_write_is_sent_once_with_the_ledgers_token_and_read_back_from_the_stand`, in `tests/driver.rs` |
| A write lost in flight is `possible_commit`, and the lookup finds the one write by its stamp | `a_write_lost_in_flight_is_possible_commit_and_the_lookup_finds_the_one_write`, in `tests/driver.rs` |
| What the server gets wrong is a named stop, no failure quotes the server, and text aimed at an agent is not among the values | `what_the_server_gets_wrong_is_a_named_stop`, in `tests/driver.rs` |
| Without the feature `tls`, an HTTPS surface is refused | `an_https_upstream_is_refused_because_tls_is_not_built`, in `tests/driver.rs` |
| With the feature `tls`, a read and a verified write work over HTTPS against a named root; an untrusted authority, a wrong name, and an expired certificate are each named and nothing reaches the server; a write lost after it was sent is in doubt | `tests/tls.rs`, through the test application's TLS front before the stand |
| The revision with no handshake is spoken when the surface states it | `the_revision_with_no_handshake_is_spoken_when_the_surface_states_it`, in `tests/driver.rs` |
| The earlier handshake revision is spoken when the surface states it, against a server that serves it alone, and the newer one is then a named stop | `the_earlier_handshake_revision_is_spoken_when_the_surface_states_it`, in `tests/answers.rs` |
| A read whose binding states no `args` sends the key its mapping names, and the stand receives it | `a_read_with_no_args_sends_the_key_its_mapping_names`, in `tests/answers.rs` |
| JSON answered as text is read one item a row, a key the server does not hold is absent, and a write is read back through such a read; a write read back through `text` alone is refused at lint | `json_as_text_is_read_one_item_a_row_and_none_is_absent`, `a_write_is_read_back_through_a_tool_that_answers_json_as_text`, `a_write_read_back_through_text_alone_is_refused_at_lint`, in `tests/answers.rs` |
| A save's offline request is recorded under the name its fixture file carries | `a_saves_offline_request_is_recorded_under_its_fixture_files_name`, in `tests/answers.rs` |
| The read rule gives a reason for each tool it drafts nothing for, and drafts a one-string read once the mapping answers `free_text: false` | `the_stands_tool_list_drafts_reads_that_read_what_it_stores`, in `tests/discover.rs` |
| Two identities never share a connection, and no secret reaches a caller | `each_identity_has_its_own_connection_and_no_secret_reaches_a_caller`, in `tests/driver.rs` |
| A local program is started by the supervisor from the plan, read, and written; one that exits after a commit is `possible_commit`; one whose digest is not the pinned one is not started | `a_local_server_program_is_started_by_the_supervisor_and_read_and_written`, `a_program_that_exits_after_a_commit_is_possible_commit`, `a_program_whose_digest_is_not_the_pinned_one_is_not_started`, in `tests/stdio.rs`, on Linux, macOS, and Windows |
| A local program ended from outside in the middle of a write call leaves the write in doubt, and it is sent once | `a_program_killed_in_the_middle_of_a_call_leaves_the_write_in_doubt_and_sends_it_once`, in `tests/stdio.rs`; the stand counts one write |
| A program that writes a megabyte to standard error is not held up; one that never answers ends at the deadline, and neither it nor a process it started still runs, as the system's own listing says | `a_program_that_writes_a_megabyte_to_standard_error_is_not_held_up_by_it`, `a_program_that_never_answers_ends_at_the_deadline_and_leaves_nothing_running`, in `tests/stdio.rs` |
| A program sees none of the engine's variables, no credential, and nothing that loads other code, whatever the engine's environment holds; the program reports its own environment | `the_program_sees_none_of_the_engines_variables_and_no_credential`, in `tests/stdio.rs` |
| A program another user could change, an interpreter by its name, a batch file, and a script are refused before they start; a program at a path with a space, letters beyond ASCII, and on Linux and macOS quotes, starts, and its arguments arrive element for element, as the program reports them | `a_program_another_user_could_change_is_refused_before_it_starts`, `a_script_is_refused_and_an_odd_path_starts_with_its_arguments_element_for_element`, in `tests/stdio.rs` |
| Discovery calls no tool, nothing that writes passes its port, the describer keeps the server's words apart, and the draft guesses nothing | `discovery_lists_the_servers_tools_and_calls_none`, in `tests/discover.rs`; the stand counts no call |
| The tool list is read again on the server's notice, once, and the changed tool is not called | `the_tool_list_is_read_again_on_the_servers_notice_that_it_changed`, in `tests/stdio.rs` |

## Build order

1. Done: the test stand, with its control interface, at `targets/example-mcp`.
2. Done: the mapping schema, `McpDriver` for reads against the stand over HTTP and as a local program, and change detection before a call.
3. Done: writes with read-back, the stamp and the lookup, and the fault cases.
4. Done: `mcp_tools`, its describer, and its drafting rule; `upstream_mcp` with a bearer token against the HTTP stand. The pass-through read operation drafted for each settled read-only tool ([decision 0179](../../decisions/0179-what-each-pack-drafts-so-that-a-scout-ends-with-reads-that-work.md)), proven against the HTTP stand by `the_stands_tool_list_drafts_reads_that_read_what_it_stores`.
5. Done: no credential crosses plain `http` to another machine unless the surface accepts it; the lint rule `mcp.unencrypted_credentials` warns in every mode ([decision 0188](../../decisions/0188-credentials-over-plain-links-drafted-workbooks-and-published-device-maps.md)). Proven by `a_credential_crosses_an_unencrypted_link_only_where_the_surface_accepts_it`, in `tests/driver.rs`. The surface kind registers the notice `SurfaceNotice::UnencryptedCredentials` for that field (`with_notice`), so a door shows it (`a_surface_that_accepts_an_unencrypted_link_carries_its_notice_and_no_other_does`). An address with a user name or a password is refused at prepare, unsent, `input`/`payload_unreadable`: the HTTP client would send it as a credential by itself, past the link check ([decision 0194](../../decisions/0194-the-packs-the-product-and-the-local-surface-take-up-the-kernels-second-pass.md); `an_address_that_holds_a_user_name_or_a_password_is_refused_unsent`).
6. Done: `mapping::plain_to_another_machine` asks `ScopedHost::may_leave_this_machine` of `a2m-core` of the host the URL parser writes, the one question every check of a plain link asks, and counts an address the parser cannot read as plain to another machine whatever it starts with, a tightening; pinned over the shared corpus of address spellings ([decision 0240](../../decisions/0240-one-typed-reading-of-where-an-address-is-and-one-question-of-a-plain-link.md)).
7. Done: a local server program on Windows, over its standard streams, in a job object, with no console window, and the provider's check of who may change a program before every start on every system ([decision 0250](../../decisions/0250-one-check-of-who-may-change-a-program-and-the-mcp-packs-local-server-on-windows.md)). Written from Linux and linted for Windows and macOS there; its tests run on all three systems through one harness in CI, which is their first run on Windows and macOS.
8. Not built: a `discovery` mapping's results taken as findings at `inferred`. A binding to its tools already does not compile (`mcp.discovery_bound`), and the driver refuses to call one. Package E6, which was to build this for the outside project of research note 16 and test hypothesis H-27, is withdrawn ([decision 0279](../../decisions/0279-what-the-three-surveys-left-open-against-decision-0276-is-settled.md)); the step has no package of its own.
9. Done ([decision 0264](../../decisions/0264-the-publish-page-renders-the-typed-review-sign-ins-first-from-one-keeper-call.md)): `Lowering::statement` for the review summary: on a remote server a binding starts nothing and sends only to its surface's address (the client follows no redirect and uses no proxy); on a local server it starts the surface's program by path and digest, with each `environment` entry as set and each `pass_environment` name as passed, and where the program sends is not stated. Test: `the_review_states_what_the_mcp_pack_runs_and_where_it_sends` (`tests/pack.rs`).

Not built:

- A server that an interpreter runs from a script (Not yet, decision 0250). The pack pins the program's digest and nothing pins a script's, so an interpreter is refused by its name, as a shell, a batch file, and a script are. The command pack's fixed pair, an interpreter in its one form with a script pinned by digest and copied for the run, is the way to build it here.
- A setting that names a group whose write counts as the administrators' (Not yet, decision 0250): only the system's own count.
- Keeping a local program's standard error: it goes to the null device, on every system.
- HTTPS in the default build. It is behind the feature `tls` ([decision 0157](../../decisions/0157-tls-is-switched-on-with-the-provider-stated-in-each-client.md)); without it the driver's health reports an `https://` surface unavailable.
- The protocol's authorization flow, in which a person opens a link. An auth entry that asks for it is refused.
- A credential for a local server program, and any credential that is not a request header.
- Answers sent as an event stream, and a notice that the tool list changed over HTTP: only a local program's notices are read. Over HTTP the list is read again instead, before every write and when it is older than `list_max_age_ms`.
- A declared shape for the structured result of a tool with no output schema. Such a tool gives only the members its entry names, whether its answer is structured or `json_text`; a typed member of it that is not a key cannot be taken yet.
- Drafting `result: json_text`: the drafter sees a tool's definition, not its answer, so it proposes `text` for a tool with no output schema and a person states the form.
- A comparison of the server's version at `prepare`, and lowering a binding's level when a tool or the version changes.
- The canary, and stopping an idle local program.
- The engine loop over these steps. The tests call the handle directly.

## Open points

- Whether a tool mapped `free_text: true` is refused outright. Settled: it is never called, and no binding may use it ([decision 0136](../../decisions/0136-the-mcp-pack-refuses-free-text-reads-its-list-again-before-a-write-and-passes-on-only-listed-members.md)). `free_text: false` is a person's statement that the server does not run the string, as `effect` is a person's judgment; the pack cannot see what a server does with a string.
- Whether a read should also be compared with a list read for it. Today a read may run against a pin up to `list_max_age_ms` old.
- Where the consent link's callback lands on a server, where the person's browser cannot reach the runner.
- Whether a mapped tool's resources and prompts are ever reachable. Not in this design: tools only.
- Whether an upstream server that holds one shared credential may serve several identities. This design gives each identity its own connection and refuses sharing.
- Hypothesis H-19 asks whether `site_tools` and `mcp` are one driver. With the two in different packs, the answer here is that they share a shape and not a driver.
- What the kernel could not give the pack is listed in [decision 0109](../../decisions/0109-the-mcp-pack-as-building-it-settled-it.md).
