# `a2m-local`

The engine on one machine, and the channel to it. This crate holds both ends of the local channel: the listener inside the engine, and the connector every thin client uses to find the engine, start it when nothing answers, and check its version. It also holds the short-lived loopback listener that serves an approval or sign-in page, and the operating system's notification and user-presence prompt. Nothing reached through the channel can approve.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 8, Roles |
| May depend on | Layers 1 to 7 |
| Depends on | `a2m-core`, `a2m-actions`, `a2m-telemetry`. Outside: Tokio, Hyper; crates for the named pipe, peer identity, file locks, notifications, and each operating system's presence prompt, all to be chosen by research briefs 26 and 27 |
| Built in | Roadmap package 1.4 (the channel, the lifecycle); 3.6 (the loopback listener, the presence prompt, notifications) |
| Carries out | Spec sections [222](../../spec/2-kernel/222-front-doors-and-the-local-engine.md), [503](../../spec/5-security/503-approval-and-sign-in-surfaces.md) on one machine, the protected settings of [502](../../spec/5-security/502-defending-against-a-manipulated-agent.md); the [local channel](../../reference/technical-reference.md#local-channel) and [on-disk layout](../../reference/technical-reference.md#on-disk-layout) of the technical reference |

## Owns

- The local channel: a Unix domain socket on macOS and Linux, a named pipe on Windows, and HTTP over it.
- The check of the connecting user on every connection, and the client's check that the engine it reached belongs to the same user.
- The engine's lifecycle on one machine: find, start on demand under a lock, the version exchange, idle stop, and the refusal of `--in-process` while an engine runs.
- The loopback listener for approval and sign-in pages, which exists only while something is pending.
- `LocalSurface`, its implementation of `PersonSurface` from `a2m-actions`: a notification, a link to the page or a plain link for the person's own browser, and the operating system's user-presence prompt or its fallback dialog.
- The client label each connection states.

It must never: open a TCP port other than the loopback listener; serve an API on the loopback listener; expose `ApprovalDesk` through the channel; hold engine logic in the client end; decide anything about what a caller may do.

## Interface

```rust
// The engine end.
pub struct Channel { /* the listener; the engine lock held for the engine's whole life */ }
impl Channel {
    /// Takes the engine lock, removes a stale socket, binds with the user-only access rule.
    /// Refuses with busy / engine_running when another engine holds the lock.
    pub async fn bind(paths: &Paths) -> Result<Channel, Failure>;
    /// Serves the HTTP service the product crate composed: the REST router of a2m-api and
    /// the MCP endpoint of a2m-mcp. Every connection is checked before its first byte is read.
    pub async fn serve(self, service: BoxedHttpService, idle: IdlePolicy) -> StopReason;
}

pub struct Peer { pub user: OsUser, pub process: Option<u32>, pub label: ClientLabel }
pub struct IdlePolicy { pub after: Duration, pub holds: Arc<dyn IdleHolds> }
pub trait IdleHolds: Send + Sync {   // implemented over the Kernel by the product crate
    fn busy(&self) -> bool;          // runs, captures, a pending approval, a keeper hold
}

// The client end. Used by commands, the stdio proxy, and the SDK bridge.
pub struct Connector { /* paths, this program's version, the client label */ }
impl Connector {
    /// Find the engine; if nothing answers, take the start lock, start it detached, and wait
    /// for it to report ready. Checks the engine's owner before sending anything.
    pub async fn connect(&self, start: StartPolicy) -> Result<Link, Failure>;
}
pub struct Link { /* an HTTP client over the channel; the engine's hello */ }
impl Link {
    pub fn engine_version(&self) -> &Version;
    pub async fn send(&self, req: http::Request<Body>) -> Result<http::Response<Body>, Failure>;
}

// The person surfaces on one machine.
pub struct LocalSurface { /* the loopback listener, the notifier, the presence prompt */ }
impl PersonSurface for LocalSurface { /* see a2m-actions */ }
pub fn presence_available() -> PresenceKind;  // Prompt(kind) or DialogOnly, reported by doctor.run
```

## The channel

| Item | macOS and Linux | Windows |
| --- | --- | --- |
| Address | A socket in the user's runtime directory, inside a directory of its own with mode 0700 | A pipe whose name carries the user's security identifier |
| Who may open it | The directory's mode; the socket's mode 0600 | An access list naming only the user, and the first-instance flag so no other process can create the pipe first |
| Server's check of the client | The peer credentials the kernel reports for the socket | The client process's token, from the pipe |
| Client's check of the server | The socket file and its directory are owned by the user | The pipe's server process runs as the user |

The client's check matters as much as the server's. Without it another program could create the socket or the pipe first and receive commands meant for the engine, or answer them.

**Protocol.** HTTP/1.1 over the channel. The paths are those of a server: `/api/v1/...` from `a2m-api` and the MCP endpoint from `a2m-mcp`. This crate does not depend on either. The product crate composes their routers into one service and hands it to `Channel::serve`, as it hands the same routers to the network listener on a server. The first request on a connection is a hello that states the client's version, the local API version, and its label; the engine answers with its own version.

**Client label.** The command line, a named MCP client (from what the client says of itself), or an SDK. It goes into the audit log and fair queuing and nowhere else. Every local client is the same principal, the operating-system user.

## Lifecycle

| Step | What happens | Detail |
| --- | --- | --- |
| Find | The client connects to the fixed path from `Paths` | No answer, or a refused connection on a stale socket, means not running |
| Start | The client takes the start lock, checks again, and starts the same program in the solo profile's `serve`, detached | A second client arriving at once waits on the start lock and then connects |
| Ready | The engine binds the channel only after `Kernel::open` has returned, in whatever health state | The client waits on the channel becoming connectable, up to a fixed deadline, then exits with code 4 |
| Version | A newer client asks an older engine to drain and restart. If runs are active, the client reports that and leaves the engine alone | An older client reaching a newer engine is served while the local API version matches; otherwise it is told to upgrade |
| Idle stop | After `engine.idle_stop_minutes` with no connections and `IdleHolds::busy` false, the engine saves sessions to the vault, closes the kernel, and exits | The keeper's keep-alive window is an idle hold |
| Crash | The engine lock is an operating-system lock, so it is released when the process dies | The next start sets orphaned runs to `stopped`, as `Kernel::open` describes |
| In process | `--in-process` refuses to run while the engine lock is held | Otherwise the product crate opens the kernel inside the command, with no channel |

Two locks are used. The engine lock is held by the engine for its whole life and proves one engine per user. The start lock is held by a client only while it starts the engine, so two clients never start two. Both are operating-system file locks in the runtime directory; no lock is judged by a file merely existing.

The `engine.*` actions are carried out here, in the client end, because they act on the process: `engine.start` runs the start procedure; `engine.stop` asks the engine to drain; `engine.status` reports `not running` when nothing answers and otherwise dispatches; `engine.logs` reads the log directory from `Paths`.

## Nothing on this machine can approve

| Rule | How it is held |
| --- | --- |
| No approve command, tool, or route | The registry has no such action, and the router has no route to `ApprovalDesk`; `a2m-actions` tests both |
| The loopback listener carries no API | It serves the page, the stored diff it was opened for, and two presses: approve and decline. Nothing else answers |
| A press is not an approval | Approve raises the operating system's presence prompt; only its success reaches `ApprovalDesk` |
| The page is only for the person | A random port, an address with a one-time code, and a check of `Host` and `Origin` on every request |
| The page shows what was hashed | It reads the stored diff, never a rebuilt one, and states uploads, sends, and anything irreversible above the records |
| Silence is a decline | The listener closes at the end of the approval window, and the answer recorded is decline |
| Protected settings | `config.set` and `provider.enable` on a protected setting call `confirm_presence`, which is this same prompt |

Where the operating system offers no prompt a background process can raise, the fallback is a native dialog showing the tool name and the counts. `doctor.run` says which kind this machine has. Spec section 222 states plainly what each protects against.

Sign-in is the keeper's work, not this crate's. `ask_sign_in` only notifies the person and serves a status page on the same loopback listener while the keeper's sign-in window, opened by the pack's auth provider, is waiting. `give_link` is for a sign-in provider outside the web pack that needs the person's consent in a browser, such as authorization-code sign-in or an upstream MCP server's: it raises a notification with the link and returns it to the calling client, and the person opens it in their own browser. Nothing here starts, reads, or drives that browser.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `anything2mcp` | `Channel` in the `serve` role on one machine; `Connector` for every command; `LocalSurface` handed to `Kernel::open` |
| `a2m-mcp` and the SDK bridge | `Link`, through the product crate, which hands the stdio proxy a connected link |

It calls `a2m-actions` for `Paths`, `PersonSurface`, `ApprovalDesk`, and `health`; `a2m-telemetry` for spans.

## State

| State | Where |
| --- | --- |
| The socket or pipe, the engine lock, the start lock | The runtime directory from `Paths` |
| Pending approval pages: one-time code, diff reference, deadline | Memory only; lost on restart, which is a decline |
| Open connections and their labels | Memory |

## Failures

| Class | Code | When |
| --- | --- | --- |
| `busy` | `engine_running` (new) | `--in-process` or a second `serve` while the engine lock is held |
| `transient` | `connection_lost` | The engine went away during a request |
| `policy` | `peer_not_allowed` (new) | A connection from another user, or an engine owned by another user |
| `policy` | `presence_failed` (new) | The person dismissed or failed the operating system's prompt |
| `busy` | `engine_unreachable` (new) | The engine did not become ready; the command exits with code 4 |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Two clients started at the same moment produce one engine | A test that starts many clients at once on three operating systems |
| A different user is refused, before any byte is read | A test run as a second account in CI |
| A client refuses an engine owned by someone else | A test that plants a socket or pipe owned by another account |
| A crashed engine leaves no lock that blocks the next start | A test that kills the engine and starts a client |
| No TCP port is open except while an approval or sign-in is pending | A test that lists the process's sockets in each state |
| The loopback listener answers only its page and two presses | A test that sends every other path and method |
| A script that presses Approve does not approve | The test of hypothesis H-12 |

## Build order

1. `Paths` from `a2m-actions`, the engine lock, and the socket on Linux and macOS with the peer check. Two commands share one engine.
2. The named pipe on Windows, with both checks. Hypothesis H-06 begins to be measured.
3. Start on demand, the start lock, the version exchange, idle stop, and `--in-process`. Package 1.4 ends here, with the 30 ms and cold-start numbers of spec section 222.
4. The loopback listener and the diff page, with a stub prompt. Package 3.6 begins.
5. The presence prompt on each operating system, or the fallback; notifications. Package 3.6 ends with H-12.

## Open points

- The crates for the pipe, the peer checks, and the locks: research brief 26.
- Which presence prompts a background process can raise: research brief 27.
- How a drained engine is restarted by the newer client: the client starts the new one after the old has released the engine lock, most likely.
- Whether the hardened Workstation service, under its own account, needs the access rule widened to name one other user, and how that is kept from becoming a hole.
- Where `engine.logs` reads from when the engine is not running and logs are rotated.
