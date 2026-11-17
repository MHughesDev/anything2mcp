# 304. The MCP pack

The MCP pack lets a target be an MCP server that a system already provides. It reaches the server and re-exposes chosen tools of it, with this runtime's policy, approval, audit, and verification around every call. It brings one mode, `mcp`, in the request archetype.

Three ideas define it. **A mapping file names the only tools that can be reached,** each pinned by the digest of its definition, so the mapping is the upstream server's manifest. **Verification is added, not borrowed:** a server's success answer shows it accepted a call, never that the record holds what was sent. **The server's own tool list is a discoverer,** and a change to a mapped tool stops the bindings that use it.

The pack is one crate, `a2m-pack-mcp`. It has no base crate and depends on no other pack. Its [design](../../architecture/crates/a2m-pack-mcp.md) is the exact statement of how the code is arranged; this section says what the pack does and why.

## What it reaches, and what it refuses

| Reaches | How |
| --- | --- |
| A local MCP server program | Started through the supervisor the pack's context scopes to the programs the published plan declares, by path and digest, with its arguments and an allowlisted environment ([section 204](../2-kernel/204-supervising-subsystems.md)). The program, its digest, its arguments, and its environment are fields of its `mcp` surface. Not built on Windows |
| A remote MCP server | Over HTTP, at the address its `mcp` surface declares, with the protocol's own authorization. As built: HTTP with a bearer token, and HTTPS in a build with the feature `tls` ([decision 0157](../../decisions/0157-tls-is-switched-on-with-the-provider-stated-in-each-client.md)); the protocol's authorization flow is not built |
| A server used only to help discovery | A mapping with `purpose: discovery`, whose tools can never be bound to an operation |

| Refused | Why |
| --- | --- |
| A tool the mapping does not name | Only what the mapping names is reachable; nothing is reachable until a person maps it |
| Passing an upstream tool's description, or any text from the server, to a caller as anything but untrusted data | Section [502](../5-security/502-defending-against-a-manipulated-agent.md). Text from a target is never placed where it could read as an instruction |
| Believing the server's read-only marking for a write | A marking is a claim. The effect class is the developer's judgment, recorded in the mapping |
| A tool that takes one free-text string and runs it as a command or a query | It cannot be verified ([section 213](../2-kernel/213-effects-hazards-and-consent.md)). A tool whose mapping entry says it runs the text it is given is never called, and a tool whose whole input is one string is called only when its entry says the server does not run it ([decision 0136](../../decisions/0136-the-mcp-pack-refuses-free-text-reads-its-list-again-before-a-write-and-passes-on-only-listed-members.md)) |
| A member of a result that the mapping does not list | Only what the mapping names is reachable. Such a member is left out of what a step returns |
| Handing the server a credential beyond the one its mapping declares | Rule four |
| One connection shared by two identities | A user's session is theirs alone |
| Retrying inside a step | The engine decides what is tried again |
| A mapped tool's resources and prompts | Tools only |

`site_tools`, which calls tools a page registers in the browser, is not this pack's. It needs a tab that can act, so it is a mode of the web pack ([section 301](301-the-web-pack.md)). The two share ideas, a pinned list of tools and a typed call, and no code.

## The mode

| Mode | Archetype | How it reaches the system | Needs | Best for | Main risk |
| --- | --- | --- | --- | --- | --- |
| `mcp` | Request | Connects to an MCP server the system already provides and re-exposes chosen tools with this runtime's policy, approval, audit, and verification around them | The server's endpoint or program, and its authorization | Systems that ship their own MCP server | Its tools may be too broad or too thin |

The mode is from [decision 0005](../../decisions/0005-seven-modes-behind-one-driver-interface.md). Where a system's own server covers the job, `mcp` pass-through is a starting mode; where it does not, the system's published interface is reached through the HTTP pack ([section 303](303-the-http-pack.md)).

## An upstream server as a provider

An upstream MCP server is one of three ways to be a provider ([section 203](../2-kernel/203-the-provider-contract.md)), and its manifest is its mapping file.

| Provider contract | For an upstream server |
| --- | --- |
| Found | From the mapping file |
| What it may be asked to do | Exactly the tools the mapping names, each pinned by digest |
| Effects | `declared`: the server runs code this runtime cannot see. The person enabling it is told so in those words |
| Lifecycle | A local program is started on first use under the supervisor, owned as a process group, and stopped when idle; a remote server is reached on first use. Stopping an idle program is not built: a program is stopped when its connection is closed |
| Listing | The mapping is what `provider.list` shows for it |

Policy, approval, and audit wrap every call, as for any operation. The tools an agent sees are the target's own operations, named `{target}_{module}_{object}_{verb}` and described by the target's files ([section 221](../2-kernel/221-the-tool-surface-agents-see.md)). Their read-only, destructive, and idempotent hints are computed from the operation's effect class, hazard, and effects, never copied from the upstream server's annotations.

## What it registers

| Registration | Content |
| --- | --- |
| Surface kinds | `mcp` |
| Modes | `mcp`, archetype request |
| Step kinds | `call_tool`, a checked name within the mode; `list_tools`, which only the discoverer sends, through the sandbox port |
| Driver | One |
| File kinds | `mappings/`: one file per `mcp` surface |
| Binding fields | `tool`, `args`; also `surface`, `timeout_ms`, `settle_ms`, `read_back` with its own `args`, and `lookup` with a `tool`, because a read-back and a lookup are calls of their own |
| Finding kinds | `upstream_tool`, `upstream_server`, each a checked name with the schema of its `found` |
| Discoverers | `mcp_tools` |
| Describer | `mcp_describer` |
| Auth providers | `upstream_mcp` |

## The mapping file

| Field | Meaning |
| --- | --- |
| `surface` (required) | The name of the `mcp` surface. The surface in `target.yaml` holds the server address, or `stdio` and a local program by path and digest with its arguments and an allowlisted environment, so that an environment can override the address and the plan declares the program ([decision 0109](../../decisions/0109-the-mcp-pack-as-building-it-settled-it.md)) |
| `server` | The name and version the server reported when the mapping was written |
| `tools` (required) | Map of the upstream tool's name to an entry: `digest` of its definition (name, input schema, output schema, annotations), `effect` as the developer judged it, `read_back` naming a mapped read tool for a write, and `untrusted_fields`, the members of a structured result that hold text and may be passed on. An entry may also state `result` (`structured` or `text`), `absent_error` (the text of the tool's error result that means the record is not there), `records` (the field of a list tool's result that holds the records), `key` (the fields of a result that make the record's key), `key_args` (the arguments that take the parts of a record's key, in order, so that a read-back and a lookup need no arguments of their own), `stamp` (the argument that carries a record's stamp), and `free_text`: `false` is a reviewer's statement that the server does not run the text the tool is given, which a tool whose whole input is one string needs before it is called; `true` records that it does, and the tool is never called |
| `credential` | Which vault reference the server is given, and how: a header for a remote server, an environment variable for a local program. Not built: the mapping has no such field yet. A remote server is given the bearer token of the session's lease, through `upstream_mcp`, and a local program is given no credential |
| `purpose` | `tools`, or `discovery` for a server used only as a discovery provider |

A definition's digest is taken over its canonical form, with SHA-256: one JSON text of the tool's name, input schema, output schema, and annotations, object keys in byte order and no spaces. The title and the description are not part of it ([decision 0108](../../decisions/0108-an-upstream-mcp-server-is-reached-only-through-a-pinned-mapping-by-a-client-of-the-packs-own.md)).

## Verbs and how they lower

| Verb | `mcp` |
| --- | --- |
| `open`, `upload` | No |
| `read`, `list`, `search` | If offered |
| `create`, `update`, `upsert`, `delete` | If offered |
| `download`, `submit` | If offered |
| `call` | Yes |

"If offered" means the server has a tool for it and the mapping names that tool. Every verb lowers into `call_tool`, compiled with the mapping entry it uses into the pack's opaque step payload. A call to a tool mapped as a write is a commit point; a call to a mapped read tool by key is a lookup.

## The driver, part by part

| Prepare | Act | Confirm | Read back |
| --- | --- | --- | --- |
| The connection for the session's identity and for the far end the plan names, started or reused; the run's destinations are checked either way; the server's tool list. Each call then compares the definition of the tool it calls with its pin, after reading the list again before a write, and before a read on the server's notice or at a stated age, so a change to one tool stops only the calls to it | `call_tool` with arguments built from `args` and checked against the pinned input schema | The result is not an error, its structured content fits the pinned output schema, and it holds what the mapping names. Only the members the mapping lists are passed on | The mapped read tool, called with the record's key, compared after normalization by the engine |

- **One connection per surface and identity.** A local program started by the supervisor, or an HTTP session; never shared between identities. A connection serves only the far end the run's plan names: the address, or the program with its digest, and the protocol revision. A plan published again with another is not served by the old connection or the old program.
- **Every failure says whether the call was sent.** A lost connection, or a lost program, after a write call was sent is `possible_commit`. The supervisor reports whether a request on a lost connection ran.
- **A write is verified only by the read tool.** A write with no mapped read tool is `irreversible`.
- A step marked as a lookup may report a record absent.

## Change detection

| What changed | Found when | Result |
| --- | --- | --- |
| A mapped tool's definition | Before each call, from the list read at `prepare`; read again before every write, and before a read on the server's notice that its list changed or, where no notice can arrive, when the list is older than the setting `list_max_age_ms`; the canary | Calls to that tool stop as `drift`; bindings on it fall to `shadow` until the mapping is re-pinned and the gates pass |
| A mapped tool is gone | The same | The same |
| An unmapped tool is added | `mcp_tools` | A finding. Nothing is reachable until a person maps it |
| The server's reported version | `prepare` | A finding, and the bindings' evidence levels fall ([section 219](../2-kernel/219-the-evidence-ladder.md)); a call still runs if every pin matches |

Over HTTP this build hears no notice, and with no handshake a server promises none. That is why a write is always compared with a list read for it, at the cost of one more listing for each write, and why a read may run against a pin up to `list_max_age_ms` old, 10 seconds unless set ([decision 0136](../../decisions/0136-the-mcp-pack-refuses-free-text-reads-its-list-again-before-a-write-and-passes-on-only-listed-members.md)).

Not built: the canary; the bindings' fall to `shadow`, and the fall in levels, since nothing lets a driver lower a binding; and the comparison of the server's version at `prepare`. `mcp_tools` reports a changed version as a finding.

## Sessions and sign-in

The rules for sessions are the kernel's ([section 215](../2-kernel/215-sessions-and-the-session-keeper.md)). The pack brings one auth provider, `upstream_mcp`: the protocol's own authorization for a remote server. As built, it holds one stored token under the role `token` and places it in the `Authorization` header as a bearer token; an entry that asks for the authorization flow is refused. The second and the last points below are not built, nor is the environment variable of the first.

- The session keeper alone calls it. The tokens it obtains are held by the keeper; one request borrows them through `SessionLease::inject`, which places a token in a request header, or an environment variable into a local program's start, without returning it.
- When the protocol's authorization needs a person to consent in a browser, `upstream_mcp` asks through the person surface, reached through the keeper: the person is given a link and opens it in their own browser. Nothing drives that browser, and the pack borrows nothing from the web pack. The callback carries the authorization code back to the provider, which exchanges it for tokens the keeper holds.
- The server refusing the credential is `needs_login`, reported through the lease.
- **A credential never crosses an unencrypted link to another machine.** A connection with a session to a server at a plain `http` address that is not a loopback address written as an IP address is refused unsent, at prepare and again as each request is sent, with `policy`, `link_not_encrypted`, unless the surface declares `unencrypted_credentials: accepted`, which `mcp.unencrypted_credentials` reports as a warning on every lint. The standard-input transport is not a network link ([decision 0188](../../decisions/0188-credentials-over-plain-links-drafted-workbooks-and-published-device-maps.md)).
- A local program's credential is handed to it privately at start by the supervisor, as the mapping's `credential` says.

## Discovery and drafting

`mcp_tools` lists the server's tools. Listing is a read the protocol defines; it never calls a tool. It runs as a discovery job on a runner, inside the engine's sandbox, and is given only what the sandbox port offers ([section 218](../2-kernel/218-discovery-and-findings.md)). A tool list is a listing the server computes about itself, so what it returns is `discovered`; what a description claims a tool does is a claim, and is marked untrusted.

| From a finding of kind | The drafter writes | Left to the developer |
| --- | --- | --- |
| A tool on the server's list, marked read-only by the server | A mapping entry with its digest, and a pass-through read operation with one binding | Its name in the language of the work; whether the marking is believed |
| A tool not marked read-only | A mapping entry only, with `effect` left as an open question | The operation, its identity strategy, its read-back tool |

As built, the drafter writes the mapping entry for either kind of tool, and the pass-through read operation for a read-only tool whose entry holds no question ([decision 0179](../../decisions/0179-what-each-pack-drafts-so-that-a-scout-ends-with-reads-that-work.md)); an entry that holds one is drafted into a mapping file of its own. It also proposes `untrusted_fields` from the output schema's free-text members, and `result: text` for a tool with no output schema. For a tool whose whole input is one string it leaves `free_text` as an open question.

`spec.import` accepts an upstream server's tool list as well as a published description. A server mapped with `purpose: discovery` is a discovery provider: its results are stored as findings at `inferred` and never bound (hypothesis H-27).

## Lint rules

| Code | Level | Rule |
| --- | --- | --- |
| `mcp.tool_unmapped` | Error | A binding names a tool its surface's mapping does not |
| `mcp.tool_unpinned` | Error | A mapped tool with no definition digest |
| `mcp.program_unpinned` | Error | A local server program with no digest |
| `mcp.write_no_readback` | Error, unless the tool is mapped `irreversible` | A write tool with no mapped read tool |
| `mcp.discovery_bound` | Error | A binding to a tool on a `discovery` mapping |
| `mcp.environment_refused` | Error | A local server program is handed a variable of the engine's (a name that starts with `A2M_`) or one named for a credential. The driver refuses them again at run time |
| `mcp.free_text` | Warning at the mapping, error at a binding | A mapped tool whose entry says `free_text: true`: a reviewer found that the server runs the text it is given. The tool is never called, and a binding that uses it does not compile. The compiler sees a tool's digest and not its schema, so the driver is what refuses a one-string tool that no reviewer has cleared |
| `mcp.untrusted_unmarked` | Error | An output of an operation bound in this mode that is or holds text, and whose field type is not marked untrusted |
| `mcp.unencrypted_credentials` | Warning | A surface declares `unencrypted_credentials: accepted`: its credentials cross an unencrypted link to another machine |

## Effects and hazards

| Effect | Declared as |
| --- | --- |
| Starts a process | The mapped program, by path and digest |
| Uses the network | The mapped server address |
| Holds a session | For surfaces of kind `mcp` |
| The upstream server's own effects | `declared` |

Hazards come from operations ([section 213](../2-kernel/213-effects-hazards-and-consent.md)).

## Failures

| Class | Codes | When |
| --- | --- | --- |
| `drift` | `upstream_tool_changed` | A pin does not match |
| `drift` | `response_schema_mismatch` | A result that does not fit the pinned output schema, or that lacks a member the mapping names under `key` or `records` |
| `rejected` | `app_validation_error` | The tool's own error result, for a call that was well formed |
| `unknown_variant` | `unexpected_status` | A protocol error the mapping did not foresee; an answer in another revision, sent as an event stream, or that is not a response |
| `possible_commit` | `no_confirmation` | A write call was sent and the connection or the program was lost before its result. The contract's code: the handle raises it for a committing step whose fate is unknown |
| `possible_commit` | `ambiguous_match` | A lookup found more than one record |
| `session_expired` | `needs_login` | The server refused the credential |
| `transient` | `timeout`, `connection_lost`, `upstream_unavailable` | Before a call was sent, or on a read |
| `policy` | `not_granted` | A call to a tool that is not mapped, if one were ever attempted; the compiler refuses it first. Also an unpinned tool, a tool of a `discovery` mapping, a write tool from a step that does not commit, a tool that runs free text or whose one string no reviewer has cleared, and an address that is not among the run's destinations |
| `input` | `arguments_invalid` | Arguments that do not fit the pinned input schema; nothing is sent |
| `policy` | `link_not_encrypted` | A session's credential would go over plain `http` to an address that is not a loopback address, on a surface that does not accept it; nothing was sent |
| `unknown_variant` | `certificate_untrusted`, `certificate_name_mismatch`, `certificate_expired`, `certificate_not_verified`, `tls_handshake_failed` | An `https://` surface whose certificate or handshake fails; nothing was sent. The session keeper's codes, passed on |
| `input` | `tls_not_enabled`, `not_built`, `payload_unreadable` | An `https://` surface in a build without the feature `tls`; a part of this design that is not built; a plan compiled by another build of the pack |

## The test stand

A small MCP server at `targets/example-mcp` ([README](../../../targets/example-mcp/README.md)), over the example data, with a control interface: inspect what it stored, count writes and calls, change a tool's definition, announce a list change, drop the connection after a commit, and return text with instructions aimed at an agent.

| Must always hold | Proven by |
| --- | --- |
| Only mapped tools are reachable | A test calling an unmapped tool through every route; the stand counts no call |
| A changed tool stops its bindings | The stand changes a definition; the next call is `drift` and does not reach the stand |
| A write lost in flight is `possible_commit`, and reconcile leaves one write | The stand drops the connection after a commit |
| Text from the server reaches a caller only marked untrusted | Test-application case 27, played by the stand |
| Discovery calls no tool | The stand counts calls during `mcp_tools` |
| Two identities never share a connection | A test with two sessions on one server |

Each row is proven by a test the [crate's design](../../architecture/crates/a2m-pack-mcp.md) names.

## The entry bar

The eight items of [section 202](../2-kernel/202-kernel-and-packs.md).

| # | Item | The MCP pack's answer |
| --- | --- | --- |
| 1 | A driver, no retries inside | The `mcp` driver |
| 2 | Read-back for every write | The mapped read tool; without one, `irreversible` |
| 3 | Each identity strategy | `natural`: the read tool by key. `composite`: where the read tool takes a parent and a child key. `stamped`: a field the tool accepts and its read or list tool returns. `find`: a list or search tool with `settle`. `none`: `irreversible` |
| 4 | A test stand | The small MCP server above |
| 5 | A discoverer | `mcp_tools`, which lists and never calls |
| 6 | Failures mapped | The table above |
| 7 | Effects and hazards | The table above; the upstream's own effects are `declared` |
| 8 | A record per new mode | None needed: decision 0005 |

## Build order

The test stand with its control interface first. Then package 8.2: the mapping schema, the driver for reads against a local stand program, and change detection; writes with read-back and the fault cases; `mcp_tools` and its drafting rule; `upstream_mcp` against a remote stand. Package E6, a `discovery` mapping, comes after.

Package 8.2 is built in `a2m-pack-mcp`, except what the [crate's design](../../architecture/crates/a2m-pack-mcp.md) lists as not built. Its tests drive the driver through the kernel's handle; the engine loop has not run the mode.

## Open points

- Which protocol revisions the client side speaks. Settled: 2025-11-25, which starts with `initialize`, and 2026-07-28, which has no handshake; the surface states which, and nothing is probed ([decision 0108](../../decisions/0108-an-upstream-mcp-server-is-reached-only-through-a-pinned-mapping-by-a-client-of-the-packs-own.md)).
- Where the consent link's callback lands on a server, when the person's browser cannot reach the runner.
- Whether a mapped tool's resources and prompts are ever reachable.
- Whether an upstream server that holds one shared credential may serve several identities. This design gives each identity its own connection and refuses sharing.
- Hypothesis H-19 asks whether `site_tools` and `mcp` are one driver. With the two in different packs, they share a shape and not a driver.
