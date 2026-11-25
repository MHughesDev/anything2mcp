# 222. Front doors and the local engine

The same operations, pipelines, and actions are reachable several ways, all backed by one engine, one policy gate, and one audit log. A front door reads the caller's credential, has `a2m-actions` check it, builds a request, and dispatches ([section 220](220-actions-and-the-authoring-interface.md)). It decides nothing, holds no run state, and holds no engine logic. This section describes the front doors, the local engine that serves them on one machine, the thin clients that reach it, and the server's network listener. Built today: partly; [section 704](../7-project/704-readiness.md) gives the detail.

## The front doors

| Front door | For | Details |
| --- | --- | --- |
| MCP over stdio | An agent on the same machine | Started by the MCP client; a small proxy to the local engine's MCP endpoint; no network, no setup |
| MCP over Streamable HTTP | Remote and hosted agents, many at once | Stateless, one endpoint per toolset, behind TLS |
| REST API | Existing software, scripts, schedulers, agent platforms that do not speak MCP | Versioned under `/api/v1`, described by an OpenAPI 3.1 document generated from the registry |
| SDKs | Python and TypeScript applications | Thin typed clients generated from the action registry and the OpenAPI document; local through a bridge, remote over HTTPS ([decision 0025](../../decisions/0025-pip-and-npm-packages-ship-the-binary-with-a-thin-sdk-no-in-process-bindings-yet.md), [section 605](../6-running-it/605-packaging-and-release.md)) |
| Command line | People, scripts, CI | `call`, `run`, and every action as `anything2mcp <noun> <verb>` |
| Embedded library | Software that wants the engine in-process | The `anything2mcp` Rust crate; Python and Node bindings are deferred |

The MCP face is `a2m-mcp` and the REST face is `a2m-api` ([crate designs](../../architecture/crates/a2m-mcp.md), [`a2m-api`](../../architecture/crates/a2m-api.md)). Both are pure services. The product crate mounts them on the local channel on one machine and on the network listener on a server. Role crates never use each other; the product crate joins them.

## MCP

**Revisions served.**

| Revision | How it is served |
| --- | --- |
| 2026-07-28 | Stateless. No handshake and no session header ([changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog)). Every request carries everything |
| 2025-11-25 | Kept for older clients for as long as clients in use open with the handshake. The initialize exchange is answered from settings. No session identifier is issued, which that revision allows, so the server stays stateless |

One service serves both revisions with no session for either ([decision 0071](../../decisions/0071-the-mcp-server-stands-on-the-official-sdk-behind-a-seam-and-serves-two-revisions.md), [research note 18](../../research/18-mcp-server-base.md)). Its protocol layer is the project's own: the official Rust SDK, `rmcp`, was built on first and left, because its tree holds a crate declared unmaintained ([decision 0130](../../decisions/0130-the-mcp-protocol-layer-is-our-own-because-the-sdk-brings-an-unmaintained-crate.md)). A request that names a revision in its metadata is served under it; `initialize` selects 2025-11-25; an unserved revision is refused with the list of those served. The difference between revisions is kept inside `a2m-mcp`.

- **Explicit handles.** Because the protocol has no sessions, all continuity is carried by `run_id`, which matches the ledger's design exactly.
- **One endpoint per toolset.** A toolset's list is the same for every caller connected to it; rights are checked at call time, and a refused call is a clear `policy` result. What each toolset lists, the names, descriptions, schemas, annotations, and results agents see, are in [section 221](221-the-tool-surface-agents-see.md).
- **Calls.** A tool name of the form `a2m_<noun>_<verb>` is the action `noun.verb`. Any other name is `tool.call` with that name as its tool. A request's JSON-RPC id belongs to its connection and is not an idempotency key; a call carries one as `anything2mcp/idempotency_key` in its `_meta`, or as the `idempotency_key` argument of `a2m_tool_call`, and the same call made again with it is the same run ([section 208](208-runs-the-ledger-and-resume.md)). The response is rendered as structured content matching the published output schema, a one-line summary, resource links for kept blobs, and, for a failure, the class, code, message, fix, and whether a retry is safe.
- **Long jobs.** A long-running action returns its `run_id` at once. Pipelines are exposed through the official Tasks extension, where clients poll with `tasks/get`, answered by `run.get`, and can cancel, through `run.cancel`; and equally through `a2m_run_get` for clients without the extension. Progress is sent only to a caller that asked, from the run's events: a rising count, and for each event that matters a sentence of the server's own, never text a system gave.
- **Sign-in and approval links.** When a run rests at `needs_login` or `needs_approval`, the result carries the link the person surface gave. Where the client supports URL-mode elicitation, the same link is also sent as an elicitation. Following the link is never an approval by itself, so giving it to an agent is safe ([section 503](../5-security/503-approval-and-sign-in-surfaces.md)).
- **Resources.** Traces, diffs, screenshots, and downloaded files are MCP resources, linked from results and never inlined.
- **When a list changes.** What an endpoint lists changes only when a version is published, an environment's version is switched, or a toolset's members change, and whether a listed tool is marked unavailable changes when a drift mark is made or lifted (below); a changed description is always a new version, and an environment follows a published version, never a pointer that moves without a publish. Where a connection can carry a notification, the door declares `listChanged` and sends it on each such change. Every listed tool carries a digest of its definition ([section 221](221-the-tool-surface-agents-see.md)), so a client that cannot be told can still see a change ([decision 0325](../../decisions/0325-what-a-browser-can-reach-and-what-a-tools-definition-may-hold.md)).
- **A tool a drift stopped.** It stays listed, with its schema unchanged. Its description begins with one fixed sentence, "Unavailable: the system changed and this tool was stopped until its target is repaired.", and its `_meta` carries `anything2mcp/unavailable` with the reason `drift`, the name of the knowledge marked, and when the mark was made; no word of the system's is in either. A call of it is refused as `drift`, `binding_drifted`, before anything is sent. When the mark is lifted, by a canary that passes or a published version whose knowledge differs, the next listing shows it available; no notice of the change is sent, because the door keeps no session, so a client learns by listing again ([decision 0396](../../decisions/0396-a-tool-stopped-by-a-drift-is-listed-and-marked-never-left-out.md)).
- **Found tools.** The stateless revision has no per-connection tool list. Until a client can be told of a changed list without a session, a tool found by search is called through the generic `a2m_tool_call`, and every call is checked against the full schema either way.

**Authorization for remote MCP.**

- Anything2MCP is an OAuth 2.1 resource server. It publishes protected resource metadata, which the MCP specification requires, and rejects any token not issued for it ([MCP authorization](https://modelcontextprotocol.io/specification/draft/basic/authorization)).
- The authorization server is the organization's identity provider. Anything2MCP never issues user credentials.
- Client registration supports Client ID Metadata Documents, which the specification prefers, and pre-registered clients. Dynamic Client Registration is deprecated in the specification but still used by some clients, so it is supported where the identity provider or a gateway offers it ([client registration](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/client-registration)).
- **API keys** are available for clients that cannot do OAuth. A key is tied to one principal and one toolset, stored hashed, and rotatable.
- A credential is checked in one place: `a2m-actions` verifies a bearer token against the configured issuer and audience, or an API key against its stored hash. Front doors extract the credential from their transport and never check one themselves, so the MCP and REST faces check tokens alike.
- A caller's token is never forwarded to a target. Reaching a target as the user goes through the session keeper and the identity kinds ([section 215](215-sessions-and-the-session-keeper.md), [section 602](../6-running-it/602-identity-on-a-shared-server.md)).

## REST

Every route is generated from an action, but two that dispatch none: the OpenAPI document and a run's events. There are two forms of a generated route: the action route, `POST /api/v1/actions/<noun>.<verb>`, for every action; and resource-shaped aliases that an action's definition declares.

| Route | Action |
| --- | --- |
| `GET /api/v1/tools` | `tool.search` with no words |
| `POST /api/v1/operations/{name}/calls`, `POST /api/v1/pipelines/{name}/runs` | `tool.call` |
| `GET /api/v1/runs/{id}` | `run.get` |
| `GET /api/v1/runs` | `run.list` |
| `POST /api/v1/runs/{id}/resume`, `/cancel` | `run.resume`, `run.cancel` |
| `GET /api/v1/sessions` | `session.status` |
| `GET /api/v1/runs/{id}/events` | A run's events, below; served by `a2m-api`, not an action. The kernel shows a caller only a run the same caller started |
| `GET /api/v1/openapi.json` | Generated from the registry; served by `a2m-api`, not an action |

The two aliases of `tool.call` are not served yet: the catalog gives `tool.call` no alias, and an action has at most one, so an operation or a pipeline is called through `POST /api/v1/actions/tool.call` for now ([`a2m-api`](../../architecture/crates/a2m-api.md)).

- **Calls.** `POST /api/v1/operations/{name}/calls` starts a call and returns a result or a `run_id`. An `Idempotency-Key` header is accepted on every call whose effect is a write: a call made again with the same key gives back the run the first one started, finished or not ([section 208](208-runs-the-ledger-and-resume.md)). A request whose answer was lost is answered with that advice when it carried a key, and with the `GET /api/v1/runs` that lists the caller's runs since it was sent when it did not.
- **Status codes** come from the response's exit class alone: 200 for success; 202 for a run that rests waiting for a person or is still running; 400 for malformed; 401 for no caller; 403 for `policy`; 409 for `stale`; 429 and 503 for `busy`, with a retry time; 500 for anything else. The body is always the full response, so a client never needs the status code to know what happened.
- **Events.** A run's events are read as one page of JSON from a sequence number, or, by a caller that accepts it, as a server-sent stream. The stream starts with every stored event after the `Last-Event-ID` the client sent, then follows new ones. Each event's identifier is its sequence number in the ledger, so a reconnecting client misses nothing and sees nothing twice. The stream ends when the run settles: when it has ended, or rests until someone acts, in which case the client opens the stream again after the run is resumed. Events pass the redactor before they are stored.
- **Webhooks.** Run events are also delivered as signed webhooks, from an outbox written in the same transaction as the event it reports, so that no event is reported that did not happen and none that happened is lost. Delivery is at least once, with growing delays up to a limit; signed over the body and a timestamp with a key from the vault; deduplicable by the event's identifier. The destination comes from settings, is checked against the egress allowlist, and never comes from an input. With no destination configured, nothing is sent.
- **Toolset export.** Any toolset can be exported as an OpenAPI document listing its target tools as operations, so platforms that consume OpenAPI actions instead of MCP can use the same tools.

## The local engine

On one machine exactly one engine process runs per user, and everything else is a thin client of it ([decision 0024](../../decisions/0024-one-local-engine-per-user-reached-over-a-socket-or-named-pipe.md)).

**Why one engine.** A coding agent builds a target through many short commands: start a capture, take a snapshot, try a draft, run the tests. Each command is a new process, yet the signed-in session, the capture in progress, the subsystems the supervisor holds, and the record locks must outlive all of them. The same holds when two agents and a script use the same target at once. If every MCP client and every command started its own engine, they would fight over one session and one store. So the state lives in one long-lived process, and the commands only talk to it.

### Thin clients

| Client | What it is | How it reaches the engine |
| --- | --- | --- |
| `anything2mcp` commands | The command line used by people and coding agents | The local channel, directly |
| MCP over stdio | What an MCP client starts for a local agent | A small proxy that copies MCP messages between standard input and output and the engine's MCP endpoint on the local channel, and adds nothing |
| Python and TypeScript SDKs | Libraries for other software | The local channel, directly: the Python client speaks HTTP over the socket on macOS and Linux; Windows is not built ([decision 0135](../../decisions/0135-the-wheel-carries-the-binary-and-a-thin-client-and-npm-uses-platform-packages.md)) |
| Tray application, from Phase 7 | Status and prompts on a user-device runner | The local channel, directly |
| Remote callers | Agents and software on other machines | Not the local channel; the network front doors |

The stdio proxy is the binary itself, started by the `bridge` process command. It writes to its standard output only complete MCP messages, and its diagnostics to standard error only, so that a client reading the output never takes a stray line for a message ([decision 0325](../../decisions/0325-what-a-browser-can-reach-and-what-a-tools-definition-may-hold.md)). An SDK that reaches the channel itself restates where the channel is and checks who owns it, as a command does; on Windows that needs calls into the operating system, which is why the Python client does not reach it there yet. Commands, the proxy, and the SDKs hold no engine logic and no state; they render the engine's response without branching on it.

### The local channel

| Item | macOS and Linux | Windows |
| --- | --- | --- |
| Transport | A Unix domain socket in the user's runtime directory, inside a directory of its own with mode 0700 | A named pipe whose name carries the user's security identifier |
| Who may open it | The directory's mode; the socket's mode 0600 | An access list naming only the user, and the first-instance flag, so no other process can create the pipe first |
| The engine's check of the client | The peer credentials the kernel reports for the socket | The client process's token, from the pipe |
| The client's check of the engine | The socket file and its directory are owned by the user | The pipe's server process runs as the user |

- **No TCP port** is opened by default, so a web page, another machine, or a DNS-rebinding attack has nothing to connect to. The one exception is a short-lived loopback listener that serves an approval page, the page of a person's act, or a sign-in page while one is pending and carries no API ([section 503](../5-security/503-approval-and-sign-in-surfaces.md)).
- **Local HTTP, where it is switched on** (the workstation profile, [section 601](../6-running-it/601-deployment-configurations.md)), is the one listener in the first release that a web page's script could reach, and is held to four rules ([decision 0325](../../decisions/0325-what-a-browser-can-reach-and-what-a-tools-definition-may-hold.md), [research note 39](../../research/39-mcp-transport-and-injection-threats.md)). It binds a loopback address only, never an unspecified one. It answers 403 to a request whose `Host` is not a loopback name with the listener's own port, and to a request that carries an `Origin` other than the listener's own, before its body is read and before any credential is looked at; a request with no `Origin` goes on to the credential check. The listener's own origin is `http://` and a loopback name it serves, with the port the request's own `Host` names, so `http://localhost:8731` and `http://127.0.0.1:8731` are its own when the request went to port 8731; `https://`, another name or port, `null`, text that is not an origin, and a second `Origin` header are refused. It needs a bearer credential on every request, which the engine issues to the person's own client files, readable by the person alone, and which is never sent in a query string. A listener with no credential check does not start; the failure code is declared in the local engine's table when local HTTP is built.
- **The check of the connecting user.** On every connection, before its first byte is read, the engine asks the operating system which user is on the other end and refuses anyone else (`peer_not_allowed`). The client's check matters as much: without it another program could create the socket or the pipe first and receive commands meant for the engine, or answer them.
- **Protocol.** HTTP over the channel, carrying the same REST API as `/api/v1` and the same MCP endpoint as the network front doors. One API, three ways to reach it. The first request on a connection is a hello that states the client's version, the local API version, and its label; the engine answers with its own version.
- **Client label.** Each connection states which client it is: the command line, an MCP client (from what the client says of itself), or an SDK. The label goes into the audit log and fair queuing and nowhere else. All local clients are the same principal: the operating-system user, and every one of them is an agent: nothing a client says of itself, its label or a header, makes it a person for any decision; a person is shown only by the presence prompt ([decision 0158](../../decisions/0158-the-product-crate-and-the-local-engine-after-their-safety-audit.md)).

The [technical reference](../../reference/technical-reference.md#local-channel) summarizes the channel, and the [on-disk layout](../../reference/technical-reference.md#on-disk-layout) gives its paths.

### Lifecycle

1. **Find.** A client connects to the engine's channel at a fixed per-user path. No answer, or a refused connection on a stale socket, means not running.
2. **Start on demand.** If nothing answers, the client takes the start lock, checks again, and starts the engine detached. A second client arriving at the same moment waits on the start lock and then connects, instead of starting a second engine.
3. **Ready.** The engine takes the engine lock and binds the channel first, then opens the kernel, and answers a hello only once the kernel has opened, in whatever health state ([section 224](224-when-the-engine-itself-fails.md)). A second engine is thus refused before it opens the store ([decision 0131](../../decisions/0131-the-front-doors-joined-serve-the-thin-client-and-the-bridge.md)). The client waits for the answer up to a fixed deadline, then exits with code 4.
4. **Version check.** After an upgrade, a newer client asks the older engine to finish its runs and restart; if runs are still active, the client reports that instead of interrupting them. An older client reaching a newer engine is served while the local API version matches; otherwise it is told to upgrade.
5. **Idle stop.** With no runs, no captures, no pending approval, and no clients for a set time, 30 minutes by default (`engine.idle_stop_minutes`), the engine saves session state to the vault and exits. The session keeper holds the engine open for a session that would not survive a restart, up to its keep-alive window ([section 215](215-sessions-and-the-session-keeper.md)). The next command starts the engine again, and sessions resume from the vault.
6. **Crash.** Runs in flight are left `running` in the ledger under a lease nobody holds. The next start sets each to `stopped` with the class `possible_commit` and the code `runner_lost`, and reconciles before any resumed write ([section 208](208-runs-the-ledger-and-resume.md)).

Two locks are used. The engine lock is held by the engine for its whole life and proves one engine per user. The start lock is held by a client only while it starts the engine, so two clients never start two. Both are operating-system file locks, released when the process dies; no lock is judged by a file merely existing. The engine lock is taken in the runtime directory and in the data directory together, so that two runtime directories never lead two engines to one store ([decision 0117](../../decisions/0117-the-local-channel-and-the-person-surface-on-one-machine-as-built.md)). An engine that stops lets go of it only after its kernel has closed, so that the next engine never opens a store the last one still writes to ([decision 0142](../../decisions/0142-the-presence-prompt-interface-and-the-linux-prompt-as-built.md)). The runtime directory follows no variable that two processes of one user need not share: on macOS it is in the data directory, and on Linux without `XDG_RUNTIME_DIR` it is the session manager's folder of the user, `/run/user/<uid>`, when that is the user's and closed. Where two processes still reach two runtime directories, the engine's record beside the data directory's lock lets the second be refused `busy`/`engine_running` with the engine's runtime directory named in the fix; nothing connects there by the record ([decision 0432](../../decisions/0432-the-runtime-folder-does-not-follow-a-variable-that-differs-between-one-users-processes.md)).

**In-process mode.** For CI jobs and containers that run one command and exit, `--in-process`, or the setting `engine.in_process` when no engine answers, runs the engine inside the command itself, with no background process and no channel; with the setting on, nothing starts an engine on demand, and the bridge, which cannot run one, tells its caller to run `serve` ([decision 0165](../../decisions/0165-a-client-starts-the-engine-on-demand-detached-and-gives-it-the-passphrase-on-a-pipe.md)). It refuses to start while an engine answers (`engine_running`). A command that runs the engine inside itself takes the engine lock as it finds no engine, and holds it until its kernel has closed; a second such command waits a bounded time for it, and `serve` is refused meanwhile ([decision 0158](../../decisions/0158-the-product-crate-and-the-local-engine-after-their-safety-audit.md)).

`engine.status`, `engine.start`, `engine.stop`, and `engine.logs` expose this to people; nobody needs them in normal use. They are carried out in the client end, because they act on the process: `engine.status` reports `not running` when nothing answers and otherwise asks the engine. `engine.stop` is sent to the engine like any action, so that the policy engine decides, and the engine's own process then stops serving ([decision 0131](../../decisions/0131-the-front-doors-joined-serve-the-thin-client-and-the-bridge.md)).

### Deployment profiles

| Profile | Engine lifetime | Channels |
| --- | --- | --- |
| Solo | Started on demand, stops when idle | The local channel only |
| Workstation service | Installed as a service, always running | The local channel, plus local HTTP if switched on |
| Team server and larger | A server process | Network front doors; the local channel remains for administration on the host |
| Embedded | Inside the host software | None, unless the host opens one |

Solo and Workstation service are the same architecture with different lifetimes. The profiles themselves are in [section 601](../6-running-it/601-deployment-configurations.md).

### Speed

A command that reaches a running engine adds no more than 30 ms beyond the engine's own work. A cold start is ready within the 100 ms budget of [section 225](225-performance.md), not counting the start of any outside program such as a browser. Both are targets, measured in Phase 1 and added to the benchmark suite.

## What may never be done from a local command

A coding agent that runs commands does so as the same operating-system user as the person. The design states the consequences plainly.

**What the engine enforces regardless.**

- **Nothing on this machine can approve.** No action approves a write: approval is not a command, not a route on the local channel, and not an MCP tool. The loopback listener serves only its page, the stored diff it was opened for, and two presses, approve and decline. A press is not an approval: Approve raises the operating system's user-presence prompt, and only its success records the approval. The page is reached at a random port and an address with a one-time code, with `Host` and `Origin` checked on every request. Silence is a decline.
- **Protected settings.** `config.set` and `provider.enable` on a protected setting need the same presence prompt.
- **A person's act an agent asks for** (publishing a version, releasing a kill switch, loosening a protected setting) is not refused: it is shown on a page of the same listener, exactly as it will be done, and a press there raises the same prompt; only its success carries the act out, once, as the person ([decision 0163](../../decisions/0163-a-persons-act-an-agent-asks-for-is-carried-out-on-a-page-of-the-person-surface.md)).
- **Secrets never cross the channel.** Cookies, tokens, and passwords stay inside the engine and the vault.
- **Exploration is read-only** by enforcement inside the engine, and draft bindings can write only to declared test records.
- **Published versions are immutable and hashed.** The engine runs only registered hashes for normal callers.

**How a write is approved on a local machine.** The engine, not the client, raises the approval: a notification, the diff on a page it serves for that moment only, and then a prompt the operating system owns ([decision 0039](../../decisions/0039-local-approval-a-diff-page-for-the-moment-then-an-operating-system-prompt.md)). Where the operating system offers a user-presence check (a fingerprint, face, PIN, or password prompt it owns), the engine requires it before recording the approval. An agent with a shell cannot answer it without the person's finger, face, PIN, or password, and cannot pass it by pressing the page or by synthetic input; on Linux, an agent that knows or has captured the password can ([research note 20](../../research/20-operating-system-user-presence-checks.md)). The check stops an agent that acts through the engine; it does not stop a program that replaces the engine or changes its files, as the limit below says. Where no such check is available, the prompt is a native dialog alone, showing the tool name and the counts. That stops mistakes and stops an agent that was merely told to approve, but it is not a security boundary: a program running as the user can press a dialog's button. `doctor.run` says which kind this machine has, why, and what to do where nothing can be raised; until it does, `serve` says so when it starts, and the approval page says so before anything is pressed. One prompt is up at a time, and it is cancelled at the end of the approval window. A yes that the operating system gives without asking anybody, such as polkit's for a rule an administrator wrote or for an engine run as the administrator account, is not taken as a person's presence ([decision 0142](../../decisions/0142-the-presence-prompt-interface-and-the-linux-prompt-as-built.md)).

**The limit.** A process with unrestricted shell access as the user can, in principle, read and change that user's files, including Anything2MCP's local store. On a single machine under a single account, Anything2MCP's protections guard against mistakes, runaway agents, and ordinary prompt injection ([section 502](../5-security/502-defending-against-a-manipulated-agent.md)). They do not make the machine safe from a hostile program already running as the user.

**When a real boundary is needed.**

- Run the coding agent in its own sandbox, with no write access outside the project folder, which most coding agents support.
- Use the hardened Workstation service: the engine runs under its own operating-system account, so the user's agents cannot read its store or vault and reach it only through the channel. The cost is that the device's own sign-in may not carry over to a different account, so the user signs in once through the engine.
- Use a server profile, where approval requires the person's own sign-in to the organization's identity provider.

## The server's front doors

On a server, `a2m-api` holds the network listener: the bind address, TLS, trusted proxies, and the base address. It serves the REST router and every other mount the product crate hands it, such as the MCP endpoint of each toolset at `/mcp/<toolset>`. It publishes the protected resource metadata, and checks every token through the kernel's one verifier.

The approval and sign-in pages on a server are the console's, reached only after the approver signs in to the organization's identity provider; that sign-in is the proof of a person, and an agent's identity can never hold the approver role. [Section 503](../5-security/503-approval-and-sign-in-surfaces.md) describes them. On the local channel the router is built without the console and without any approval page or route, and a test over that router holds it. A runner on another machine reaches the control plane over the runner link, not a front door ([section 223](223-the-runner-protocol.md)). How the listener sits behind a network is in [section 603](../6-running-it/603-networking.md).

## No client is special

Any client that speaks the protocol is served alike, whoever made it, a home-built one included ([decision 0058](../../decisions/0058-a-toolkit-for-developers-with-no-catalog-and-no-preferred-agent.md)). Nothing in the kernel knows which make of agent is calling.

| What differs between clients | Where it is handled |
| --- | --- |
| Capabilities: elicitation, tasks, progress, resource links | What the client declares. Each has a fallback that every client gets |
| Which revision | The revision the request names |
| A quirk with no declared capability | A client profile row in a data file, with a test and a note of why. It changes presentation only, never what is allowed or which tools exist |
| The client's own name | The client label, for the audit log and fair queuing only |

Conformance is tested against the protocol with at least two unrelated clients. A test dispatches with two client labels and compares the decisions recorded.

## Open points

- Which Rust crates provide the socket, the named pipe, the peer-identity checks, and the locks on all three operating systems: brief 26; hypothesis H-06.
- Settled by [research note 20](../../research/20-operating-system-user-presence-checks.md): which user-presence checks a background process can raise on each operating system, and from Rust. Still open: hypothesis H-12, which needs a person.
- How a client is told of a changed tool list with no session, if at all. Until then, the tool digest of section 221 lets a client pin a tool and see that it changed.
- How a drained engine is restarted by the newer client. Most likely the client starts the new engine after the old one has released the engine lock.
- Whether the hardened Workstation service, under its own account, needs the channel's access rule widened to name one other user, and how that is kept from becoming a hole.
- Where `engine.logs` reads from when the engine is not running and logs are rotated.
- The `/api/v1/admin/...` row of the technical reference: removed, or defined as aliases of the admin actions, which today are reached by their action routes.
- Where a remote caller fetches a kept blob: a generated alias of `trace.show`, most likely.
- The health answer for a load balancer, which has no caller: a reduced, generated alias of `engine.status` that shows only the state.
- Actions to manage webhook destinations and to export a toolset's description are not yet in the action catalog ([section 220](220-actions-and-the-authoring-interface.md)).
