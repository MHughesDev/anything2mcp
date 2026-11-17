# 39. The local engine and its clients

> Archived on 2026-11-17. This is section 39 of the first design, kept as it was written, with the names and numbers of its time. What it says now lives in [section 222, Front doors and the local engine](../../spec/2-kernel/222-front-doors-and-the-local-engine.md).

On one machine, exactly one engine process runs per user, and everything else is a thin client of it. This section defines that process, how clients find and reach it, and what it can and cannot protect when an AI agent has a shell on the same machine.

**Why one engine.** A coding agent builds a target through many short commands: start a capture, take a snapshot, try a draft, run the tests. Each command is a new process, yet the browser, the signed-in session, the capture in progress, and the record locks must outlive all of them. The same holds when two agents and a script use the same target at once. If every MCP client and every command started its own engine, they would fight over one browser profile and one store. So the state lives in one long-lived process and the commands only talk to it.

## Clients

| Client | What it is | How it reaches the engine |
| --- | --- | --- |
| `anything2mcp` commands | The command line used by people and coding agents | Local channel, directly |
| MCP over stdio | What an MCP client starts for a local agent | A small proxy that copies MCP messages between stdio and the engine's MCP endpoint on the local channel |
| Python and TypeScript SDKs | Libraries for other software | A bridge subprocess, `anything2mcp bridge`, that carries JSON messages between the SDK and the local channel |
| Tray application, from Phase 7 | Status and prompts on a user-device runner | Local channel, directly |
| Remote callers | Agents and software on other machines | Not the local channel; they use the network front doors in [section 22](22-front-doors.md) |

The stdio proxy and the SDK bridge are the same binary. Keeping the channel code in Rust means no SDK in any language needs to implement sockets and pipes for three operating systems.

## The local channel

- **Transport.** A Unix domain socket on macOS and Linux, a named pipe on Windows. No TCP port is opened by default, so a web page, another machine, or a DNS-rebinding attack has nothing to connect to. The one exception is a short-lived loopback listener that serves an approval or sign-in status page while one is pending and carries no API ([section 49](../../spec/5-security/503-approval-and-sign-in-surfaces.md)).
- **Access.** The socket sits in a directory only the user can open; the pipe carries an access list naming only that user. On every connection the engine also asks the operating system which user is on the other end and refuses anyone else.
- **Protocol.** HTTP over the channel, carrying the same REST API as `/api/v1` and the same MCP endpoint as the network front doors. One API, three ways to reach it.
- **Client label.** Each connection states which client it is, such as the command line, a named MCP client, or an SDK. The label goes into the audit log and is used for fair queuing. All local clients are the same principal: the operating-system user.

## Lifecycle

1. **Find.** A client looks for the engine's channel at a fixed per-user path.
2. **Start on demand.** If nothing answers, the client takes a lock file, starts the engine detached, and waits for it to report ready. A second client arriving at the same moment waits on the lock instead of starting a second engine.
3. **Version check.** Client and engine exchange versions. After an upgrade, a newer client asks the older engine to finish its runs and restart; if runs are still active, the client reports that instead of interrupting them.
4. **Idle stop.** With no runs, no captures, and no clients for a set time (30 minutes by default), the engine saves session state to the vault and exits. The session keeper holds the engine open for a session that would not survive a restart, up to its keep-alive window ([section 44](44-session-keeper.md)). The next command starts it again and sessions resume from the vault.
5. **Crash.** Runs in flight are left `running` in the ledger under a lease nobody holds. The next start sets each to `stopped` with the class `possible_commit` and the code `runner_lost`, and reconciles before any resumed write, as in [section 42](42-run-ledger-design.md).

`anything2mcp engine status`, `start`, `stop`, and `logs` expose this to people. Nobody needs them in normal use.

## How this changes the deployment profiles

| Profile | Engine lifetime | Channels |
| --- | --- | --- |
| Solo | Started on demand, stops when idle | Local channel only |
| Workstation service | Installed as a service, always running | Local channel, plus local HTTP if switched on |
| Team server and larger | A server process | Network front doors; the local channel remains for administration on the host |
| Embedded | Inside the host application | None, unless the host opens one |

Solo and Workstation service are now the same architecture with different lifetimes. The earlier description of Solo as a process started by each MCP client is replaced by this one.

**In-process mode.** For CI jobs and containers that run one command and exit, `--in-process` runs the engine inside the command itself with no background process. It refuses to start if an engine is already running for that user.

## Speed

A command that reaches a running engine should add no more than 30 ms beyond the engine's own work, and a cold start should be ready within the 100 ms budget of [section 9](../../spec/2-kernel/225-performance.md), not counting browser launch. Both are targets to be measured in Phase 1 and added to the benchmark suite.

## What a local agent with a shell can and cannot do

A coding agent that runs commands does so as the same operating-system user as the person. That has consequences the design states plainly.

**What the engine enforces regardless.**

- No action approves a write. Approval is not a command, not an API route on the local channel, and not an MCP tool.
- Secrets never cross the channel. Cookies, tokens, and passwords stay inside the engine and the vault.
- Exploration is read-only by enforcement inside the engine, and draft bindings can write only to declared test records.
- Published versions are immutable and hashed; the engine runs only registered hashes for normal callers.

**How a write is approved on a local machine.**

1. The engine, not the client, raises the approval: a notification, the diff on a page it serves for that moment only, and then a prompt owned by the operating system ([section 49](../../spec/5-security/503-approval-and-sign-in-surfaces.md)).
2. Where the operating system offers a user-presence check (a fingerprint, face, PIN, or password prompt owned by the operating system), the engine requires it before recording the approval. An agent with a shell cannot answer that prompt.
3. Where no such check is available, the prompt is a native dialog alone. This stops mistakes and stops an agent that was merely told to approve, but it is not a security boundary.

**The limit.** A process with unrestricted shell access as the user can, in principle, read and change that user's files, including Anything2MCP's local store. On a single machine under a single account, Anything2MCP's protections guard against mistakes, runaway agents, and ordinary prompt injection. They do not make the machine safe from a hostile program already running as the user.

**When a real boundary is needed.**

- Run the coding agent in its own sandbox with no write access outside the project folder, which most coding agents support.
- Use the hardened Workstation service: the engine runs under its own operating-system account, so the user's agents cannot read its store or vault and reach it only through the channel. The cost is that the device's own sign-in may not carry over to a different account, so the user signs in once through the engine's browser.
- Use a server profile, where approval requires the person's own sign-in to the organization's identity provider.

## Open points

- Which Rust crates provide the socket, named pipe, and peer-identity checks on all three operating systems.
- Which user-presence checks can be called from a background process on each operating system, and from Rust.

These are research briefs 26 and 27 in [open research](../../research/11-open-research.md).
