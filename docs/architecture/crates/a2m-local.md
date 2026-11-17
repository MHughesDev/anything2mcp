# `a2m-local`

The engine on one machine, and the channel to it. This crate holds both ends of the local channel: the listener inside the engine, and the connector every thin client uses to find the engine, start it when nothing answers, and check its version. It also holds the short-lived loopback listener that serves an approval or sign-in page, and the operating system's notification and user-presence prompt. Nothing reached through the channel can approve.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 8, Roles |
| May depend on | Layers 1 to 7 |
| Depends on | `a2m-core`, `a2m-actions`, `a2m-telemetry`, and for the person surface `a2m-policy`, `a2m-auth`, `a2m-vault`. Outside: Tokio, axum, `hyper`, `hyper-util`; `rustix` on macOS and Linux and `windows-sys` on Windows ([decision 0117](../../decisions/0117-the-local-channel-and-the-person-surface-on-one-machine-as-built.md)). Crates for notifications and each operating system's presence prompt are still to be chosen by research brief 27 |
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

As built ([decision 0117](../../decisions/0117-the-local-channel-and-the-person-surface-on-one-machine-as-built.md)). What is not built is left out, and named in the build order.

```rust
// The engine end.
pub struct Channel { /* the listener; the engine lock, held until serve returns */ }
impl Channel {
    /// Takes the engine lock, removes a stale socket, binds with the user-only access rule.
    /// Refuses with busy / engine_running when another engine holds the lock.
    /// `version` is this program's, which the hello answers with.
    pub async fn bind(paths: &Paths, version: Version) -> Result<Channel, Failure>;
    pub fn control(&self) -> ChannelControl;
    /// Serves the router the product crate composed: the REST router of a2m-api and
    /// the MCP endpoint of a2m-mcp. Every connection is checked before its first byte is read.
    /// The channel answers the hello; every other request carries a `Peer` in its extensions.
    pub async fn serve(self, service: axum::Router, idle: IdlePolicy) -> StopReason;
}
pub struct ChannelControl { /* cheap to clone */ }
impl ChannelControl { pub fn stop(&self, why: StopReason); pub fn refused(&self) -> u64; }
pub enum StopReason { Idle, Asked, Restart, Failed }

pub struct OsUser(/* the user id, or the security identifier, as text */);
impl OsUser { pub fn current() -> Result<OsUser, Failure>; pub fn as_str(&self) -> &str; }
pub enum ClientLabel { CommandLine, Mcp(String), Sdk(String) }
pub struct Peer { pub user: OsUser, pub process: Option<u32>, pub label: ClientLabel }
impl Peer { pub fn credential(&self) -> CallerCredential; } // LocalUser, for Kernel::verify_caller

pub struct IdlePolicy { pub after: Option<Duration>, pub holds: Arc<dyn IdleHolds> }
impl IdlePolicy { pub fn never() -> Self; pub fn after(after: Duration, holds: Arc<dyn IdleHolds>) -> Self; }
pub trait IdleHolds: Send + Sync {   // implemented over the Kernel and the surface by the product crate
    fn busy(&self) -> bool;          // runs, captures, a waiting page, a keeper hold
}

// The engine lock, also for a command that would run the engine inside itself.
pub struct EngineLock { /* two file locks: runtime directory and data directory */ }
impl EngineLock {
    pub fn take(paths: &Paths) -> Result<EngineLock, Failure>;
    pub fn held(paths: &Paths) -> Result<bool, Failure>;
}

// The client end. Used by commands, the stdio proxy, and the SDK bridge.
pub struct Connector { /* the channel's place, this program's version, the client label */ }
impl Connector {
    pub fn new(paths: &Paths, version: Version, label: ClientLabel) -> Result<Connector, Failure>;
    /// Finds the engine, checks its owner before sending anything, and says hello.
    /// It starts nothing: when no engine answers it fails with busy / engine_unreachable.
    pub async fn connect(&self) -> Result<Link, Failure>;
}
pub struct Link { /* an HTTP/1.1 connection over the channel; the engine's hello */ }
impl Link {
    pub fn engine_version(&self) -> Version;
    pub async fn send(&self, req: http::Request<axum::body::Body>)
        -> Result<http::Response<axum::body::Body>, Failure>;
}

// The person surface on one machine.
pub struct SurfaceParts {
    pub prompt: Arc<dyn PresencePrompt>, pub clock: Arc<dyn Clock>,
    pub random: Arc<dyn RandomSource>, pub window: Duration,
}
pub struct LocalSurface { /* the desk, the loopback listener, the waiting pages */ }
impl LocalSurface {
    pub fn new(parts: SurfaceParts) -> Self;
    pub fn presence(&self) -> PresenceKind;
    pub fn waiting(&self) -> usize;      // an idle hold
    pub async fn settled(&self);         // no page waits and no listener is open
}
impl PersonSurface for LocalSurface { /* see a2m-actions */ }

pub trait PresencePrompt: Send + Sync {
    fn kind(&self) -> PresenceKind;
    fn ask(&self, text: PromptText) -> BoxFuture<'_, PromptAnswer>;
}
pub enum PresenceKind { OsPrompt, DialogOnly, NotAvailable }
pub enum PromptAnswer { Present, Confirmed, Dismissed, NotAvailable }
pub struct NoPrompt;                              // the only prompt built: NotAvailable
pub fn presence_available() -> PresenceKind;      // NotAvailable on every system, for doctor.run
pub fn system_prompt() -> Arc<dyn PresencePrompt>; // NoPrompt
```

The product crate starts the role in this order: `LocalSurface::new`, `Kernel::open` with it, `Channel::bind`, then `Channel::serve` with one router that merges the REST router and the MCP endpoint. A front door reads the `Peer` from a request's extensions and hands `Peer::credential()` to `Kernel::verify_caller`. `KernelParts::os_user` is `OsUser::current()` as text.

## The channel

| Item | macOS and Linux | Windows |
| --- | --- | --- |
| Address | A socket in the user's runtime directory, inside a directory of its own with mode 0700 | A pipe whose name carries the user's security identifier |
| Who may open it | The directory's mode; the socket's mode 0600 | An access list naming only the user, and the first-instance flag so no other process can create the pipe first |
| Server's check of the client | The peer credentials the kernel reports for the socket | The client process's token, from the pipe |
| Client's check of the server | The socket file and its directory are owned by the user | The pipe's server process runs as the user |

The client's check matters as much as the server's. Without it another program could create the socket or the pipe first and receive commands meant for the engine, or answer them.

**Protocol.** HTTP/1.1 over the channel. The paths are those of a server: `/api/v1/...` from `a2m-api` and the MCP endpoint from `a2m-mcp`. This crate does not depend on either. The product crate composes their routers into one service and hands it to `Channel::serve`, as it hands the same routers to the network listener on a server. The first request on a connection is a hello that states the client's version, the local API version, and its label; the engine answers with its own version. The hello is `GET /local/hello` with three headers, and the channel answers it itself. A request on a connection that has not said hello is refused with `hello_invalid` and does not reach the service; another local API version is `local_api_mismatch`.

On Windows the pipe's name also carries a short digest of the runtime directory's path, so that two homes of one account do not share a pipe. The engine reads the client's user from the token of the client's process, which the pipe names, because impersonating the client needs a byte to have been read first.

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

Two locks are used. The engine lock is held by the engine for its whole life and proves one engine per user and per data directory. It is two operating-system file locks taken together, one in the runtime directory and one in the data directory, because two runtime directories can lead to one data directory and two engines on one store must never run. The start lock is held by a client only while it starts the engine, so two clients never start two; it is a file lock in the runtime directory, and is not built yet. No lock is judged by a file merely existing.

The `engine.*` actions are carried out here, in the client end, because they act on the process: `engine.start` runs the start procedure; `engine.stop` asks the engine to drain; `engine.status` reports `not running` when nothing answers and otherwise dispatches; `engine.logs` reads the log directory from `Paths`.

## Nothing on this machine can approve

| Rule | How it is held |
| --- | --- |
| No approve command, tool, or route | The registry has no such action, and the router has no route to `ApprovalDesk`; `a2m-actions` tests both |
| The loopback listener carries no API | It serves the page, the stored diff it was opened for, and two presses: approve and decline. Nothing else answers |
| A press is not an approval | Approve raises the presence prompt; only its success reaches `ApprovalDesk`. With no prompt built, nothing is approved |
| The page is only for the person | A random port, an address with a one-time code, and a check of `Host` and `Origin` on every request; a press must come from the page's own origin |
| A press needs the page | Each time the page is served it carries a fresh one-time value; a press without the value of the page served last records nothing, and a press spends it |
| The approval is for what the page showed | At a press the desk is asked again; nothing is recorded unless the run still waits with the hash the page showed |
| The page shows what was hashed | It is to read the stored diff, never a rebuilt one, and state uploads, sends, and anything irreversible above the records. Today it shows the tool, the run, who started it, and the hash: `ApprovalDesk` gives no way to read the stored diff |
| Silence is a decline | At the end of the approval window the page is removed, a decline is recorded, and the listener closes when no page waits |
| An answer stands | An answered change gets no second page while the engine runs; the ask is answered with words and no link |
| Protected settings | `config.set` and `provider.enable` on a protected setting call `confirm_presence`, which is this same prompt |

Where the operating system offers no prompt a background process can raise, the fallback is a native dialog showing the tool name and the counts. `doctor.run` says which kind this machine has. Spec section 222 states plainly what each protects against.

Sign-in is the keeper's work, not this crate's. `ask_sign_in` is to notify the person and serve a status page on the same loopback listener while the keeper's sign-in window, opened by the pack's auth provider, is waiting. Today it returns words and no link, and `ask_session` answers that the person cannot be reached: no notification, no status page, and no return address are built. `give_link` is for a sign-in provider outside the web pack that needs the person's consent in a browser, such as authorization-code sign-in or an upstream MCP server's: it raises a notification with the link and returns it to the calling client, and the person opens it in their own browser. Nothing here starts, reads, or drives that browser.

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
| `busy` | `engine_running` | `--in-process` or a second `serve` while the engine lock is held |
| `transient` | `connection_lost` | The engine went away during a request, or closed the connection |
| `policy` | `peer_not_allowed` | An engine owned by another user. A connection from another user is closed unread, so that client sees `connection_lost` |
| `policy` | `presence_failed` | The person dismissed or failed the prompt, or this machine has none |
| `busy` | `engine_unreachable` | Nothing answers on the channel; the command exits with code 4 |
| `busy` | `channel_unavailable` | A directory, a lock file, the channel, or the loopback listener cannot be opened |
| `input` | `hello_invalid` | A connection's first request was not a well-formed hello |
| `input` | `local_api_mismatch` | The client and the engine speak different versions of the local API |
| `busy` | `surface_unavailable` | The surface holds no approval desk, so it shows no page |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Two clients started at the same moment produce one engine | Built: two clients at once share one engine, and a second engine on the same data directory is refused. Not built: the start on demand, so no test starts clients against no engine |
| A different user is refused, before any byte is read | Built: an engine that expects another user closes this user's connection unread, and the service sees nothing. Not built: a test run as a second account in CI |
| A client refuses an engine owned by someone else | Built: a client that expects another user's engine sends nothing. Not built: a socket or pipe planted by another account |
| A crashed engine leaves no lock that blocks the next start | Built: a stale socket is not an engine and the next engine binds over it; a dropped lock is free. Not built: a test that kills an engine process |
| No TCP port is open except while an approval or sign-in is pending | Built: the page's port refuses connections once nothing waits. Not built: a listing of the process's sockets |
| The loopback listener answers only its page and two presses | Built: a test that sends other paths and methods, a front door's among them |
| A script that presses Approve does not approve | Built: a press without the page's value, from another origin, or with the prompt not passed records nothing. Hypothesis H-12 itself needs a real prompt |

## Build order

1. Done: `Paths` from `a2m-actions`, the engine lock, and the socket on Linux and macOS with the peer check and the client's check of the owner. Two clients share one engine. Run on Linux only; the macOS code is compiled and linted, not run.
2. Written, not run: the named pipe on Windows, with both checks. It is compiled and linted for Windows through a crate that holds only the operating-system module, because the whole crate's tree compiles SQLite. CI must show its tests and the channel's tests passing on Windows. Hypothesis H-06 is not yet measured.
3. Partly done: the hello with the version exchange, the idle stop, and `EngineLock::held` for `--in-process`. Not built: start on demand and the start lock, the drain and restart of an older engine, the `engine.*` actions of the client end, and the 30 ms and cold-start numbers of spec section 222.
4. Done, with one gap: the loopback listener and the approval page, with the prompt behind an interface and a prompt that says none is available. The gap is the diff itself, which the page cannot read.
5. Not built: the presence prompt on each operating system, the fallback dialog, notifications, the sign-in status page, and the way to a session's person. Until then no approval is recorded on one machine and no protected setting is loosened. H-12 is open.

## Open points

- Settled by [decision 0117](../../decisions/0117-the-local-channel-and-the-person-surface-on-one-machine-as-built.md): the pipe, the peer checks, and the locks need no new crate. Still open from research brief 26: starting a detached process on each system.
- Which presence prompts a background process can raise: research brief 27.
- Whether an engine that is not elevated can read the user of an elevated client's process on Windows. If it cannot, that client is refused, which is the safe side.
- How the page reads the stored diff: an accessor on `ApprovalDesk`, most likely.
- A declined run still rests at `needs_approval` in the kernel. The surface's memory keeps it from being asked about again only while the engine runs.
- How a drained engine is restarted by the newer client: the client starts the new one after the old has released the engine lock, most likely.
- Whether the hardened Workstation service, under its own account, needs the access rule widened to name one other user, and how that is kept from becoming a hole.
- Where `engine.logs` reads from when the engine is not running and logs are rotated.
