# `a2m-local`

The engine on one machine, and the channel to it. This crate holds both ends of the local channel: the listener inside the engine, and the connector every thin client uses to find the engine, start it when nothing answers, and check its version. It also holds the short-lived loopback listener that serves an approval or sign-in page, and the operating system's notification and user-presence prompt: polkit on a Linux desktop, built; macOS and Windows, not built yet. Nothing reached through the channel can approve.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 8, Roles |
| May depend on | Layers 1 to 7 |
| Depends on | `a2m-core`, `a2m-actions`, `a2m-telemetry`, and for the person surface `a2m-policy`, `a2m-auth`, `a2m-vault`. Outside: Tokio, axum, `hyper`, `hyper-util`; `rustix` on macOS and Linux and `windows-sys` on Windows ([decision 0117](../../decisions/0117-the-local-channel-and-the-person-surface-on-one-machine-as-built.md)). On Linux only, for the presence prompt: `zbus` 6.3.0 (no default features; `tokio`, `proxy`) and `zbus_polkit` 6.0.0 ([decision 0142](../../decisions/0142-the-presence-prompt-interface-and-the-linux-prompt-as-built.md)). The crates for the macOS and Windows prompts are named in [decision 0133](../../decisions/0133-the-presence-prompt-on-each-operating-system-and-what-it-is-said-to-prove.md) and not in the tree yet. A crate for notifications is still to be chosen |
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

As built ([decision 0117](../../decisions/0117-the-local-channel-and-the-person-surface-on-one-machine-as-built.md), and for the presence prompt, the engine lock, and the idle hold [decision 0142](../../decisions/0142-the-presence-prompt-interface-and-the-linux-prompt-as-built.md)). What is not built is left out, and named in the build order.

```rust
// The engine end.
pub struct Channel { /* the listener; the engine lock, which serve hands back */ }
impl Channel {
    /// Takes the engine lock, removes a stale socket, binds with the user-only access rule.
    /// Refuses with busy / engine_running when another engine holds the lock.
    /// `version` is this program's, which the hello answers with.
    pub async fn bind(paths: &Paths, version: Version) -> Result<Channel, Failure>;
    pub fn control(&self) -> ChannelControl;
    /// Serves the router the product crate composed: the REST router of a2m-api and
    /// the MCP endpoint of a2m-mcp. Every connection is checked before its first byte is read.
    /// The channel answers the hello; every other request carries a `Peer` in its extensions.
    /// The engine lock comes back, still held: the caller drops it once the kernel has closed.
    pub async fn serve(self, service: axum::Router, idle: IdlePolicy) -> (StopReason, EngineLock);
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
    fn busy(&self) -> BoxFuture<'_, bool>; // a waiting page, runs in hand (a read of the store)
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
    pub async fn presence(&self) -> Availability; // what the prompt can do now; raises nothing
    pub fn waiting(&self) -> usize;      // an idle hold
    pub async fn settled(&self);         // no page waits and no listener is open
}
impl PersonSurface for LocalSurface { /* see a2m-actions */ }

// The presence prompt. One prompt of an operating system is one file beside
// `presence.rs` that implements the trait, its `mod` line, and its arm in `system_prompt`.
pub trait PresencePrompt: Send + Sync {
    /// What this prompt can do on this machine now. It raises nothing and shows nothing.
    /// Asked each time it is needed: what it reads can change while the engine runs.
    fn available(&self) -> BoxFuture<'_, Availability>;
    /// Raises the prompt and waits for its end. At `deadline`, the end of the approval
    /// window, the prompt is cancelled and the answer is `Dismissed`, whatever arrives after.
    /// The surface never calls it while an earlier ask of its own has not answered.
    fn ask(&self, text: PromptText, deadline: tokio::time::Instant) -> BoxFuture<'_, PromptAnswer>;
}
pub struct PromptText { pub title: String, pub detail: String }
pub enum PresenceKind { OsPrompt, DialogOnly, NotAvailable }            // os_prompt, dialog_only, not_available
pub enum PromptName { Polkit, LocalAuthentication, WindowsHello }       // polkit, local_authentication, windows_hello
pub enum PresenceReason {                                               // one word, for a report
    Ready, NotBuilt, NoSystemBus, NoPolkit, PolicyNotInstalled, SessionNotActive,
    NoAgent, GrantedWithoutAsking, CheckFailed,
}
pub struct Availability {
    pub kind: PresenceKind, pub prompt: Option<PromptName>, pub reason: PresenceReason,
    pub message: String, pub remedy: String,                            // the engine's own words
}
impl Availability {
    pub fn ready(prompt: PromptName, message: impl Into<String>) -> Self;
    pub fn not_available(reason: PresenceReason, message: impl Into<String>, remedy: impl Into<String>) -> Self;
    pub fn proves_presence(&self) -> bool;
}
pub enum PromptAnswer {
    Present(PromptName),          // the only answer that proves a person; names the prompt that answered
    Confirmed,                    // a dialog alone: recorded with the check not_available
    Dismissed,                    // closed, or cancelled at the deadline
    Denied,                       // answered and not passed
    NotAvailable(Availability),   // could not be raised, and why
}
pub struct NoPrompt;                                // not_available, not_built
pub fn system_prompt() -> Arc<dyn PresencePrompt>;  // polkit on Linux; NoPrompt on macOS and Windows
pub async fn presence_available() -> Availability;  // what system_prompt() says of itself, for doctor.run
```

The product crate starts the role in this order: `Channel::bind`, `LocalSurface::new` with `system_prompt()`, `Kernel::open` with it, then `Channel::serve` with one router that merges the REST router and the MCP endpoint. When `serve` returns, the product closes the kernel and only then drops the engine lock that `serve` handed back. A front door reads the `Peer` from a request's extensions and hands `Peer::credential()` to `Kernel::verify_caller`. `KernelParts::os_user` is `OsUser::current()` as text.

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
| Idle stop | After `engine.idle_stop_minutes` with no connections and `IdleHolds::busy` false, the engine saves sessions to the vault, closes the kernel, and exits | A waiting page and a run in hand are idle holds: the product's hold asks `Kernel::runs_in_hand`. The keeper's keep-alive window is to be one |
| Stop | `Channel::serve` returns the engine lock, still held; the product closes the kernel and then drops it | No second engine opens the store while the first still writes to it |
| Crash | The engine lock is an operating-system lock, so it is released when the process dies | The next start sets orphaned runs to `stopped`, as `Kernel::open` describes |
| In process | `--in-process` refuses to run while the engine lock is held | Otherwise the product crate opens the kernel inside the command, with no channel |

Two locks are used. The engine lock is held by the engine for its whole life and proves one engine per user and per data directory. It is two operating-system file locks taken together, one in the runtime directory and one in the data directory, because two runtime directories can lead to one data directory and two engines on one store must never run. The start lock is held by a client only while it starts the engine, so two clients never start two; it is a file lock in the runtime directory, and is not built yet. No lock is judged by a file merely existing.

The `engine.*` actions are carried out here, in the client end, because they act on the process: `engine.start` runs the start procedure; `engine.stop` asks the engine to drain; `engine.status` reports `not running` when nothing answers and otherwise dispatches; `engine.logs` reads the log directory from `Paths`.

## Nothing on this machine can approve

| Rule | How it is held |
| --- | --- |
| No approve command, tool, or route | The registry has no such action, and the router has no route to `ApprovalDesk`; `a2m-actions` tests both |
| The loopback listener carries no API | It serves the page, the stored diff it was opened for, and two presses: approve and decline. Nothing else answers |
| One prompt at a time | The surface holds one lock while a prompt is up. A second press, or a protected setting meanwhile, raises nothing: the page answers 503, and `confirm_presence` fails with `busy`/`prompt_busy` |
| A prompt ends with the window | `ask` is given the end of the page's approval window; the prompt is cancelled then and the answer is a dismissal. A press is carried out in a task of its own, so a browser that goes away leaves no page marked as pressed |
| An error is never an approval | Only `PromptAnswer::Present` records an approval with proof of presence. Every failure of the operating system's prompt is `Dismissed`, `Denied`, or `NotAvailable` with a reason |
| A press is not an approval | Approve raises the presence prompt; only its success reaches `ApprovalDesk`. Where no prompt can be raised, nothing is approved, and the page says so, with the reason and the remedy, before anything is pressed |
| The page is only for the person | A random port, an address with a one-time code, and a check of `Host` and `Origin` on every request; a press must come from the page's own origin |
| A press needs the page | Each time the page is served it carries a fresh one-time value; a press without the value of the page served last records nothing, and a press spends it |
| The approval is for what the page showed | At a press the desk is asked again; nothing is recorded unless the run still waits with the hash the page showed |
| The page shows what was hashed | It reads the stored diff through `ApprovalDesk::preview`, which reads it back and computes its hash again, and shows it only when that hash is the one the page was opened for: the target, the environment, the counts, and each record's operation, key, action, and fields, from what to what. Every value is placed as text. A change of more than 2,000 records is shown in part, says so, and cannot be approved on the page. Not built: stating uploads, sends, and anything irreversible above the records |
| Silence is a decline | At the end of the approval window the page is removed, a decline is recorded, and the listener closes when no page waits |
| An answer stands | An answered change gets no second page while the engine runs; the ask is answered with words and no link |
| Protected settings | `config.set` and `provider.enable` on a protected setting call `confirm_presence`, which is this same prompt |

Where the operating system offers no prompt a background process can raise, the fallback is a native dialog showing the tool name and the counts; it is not built ([decision 0133](../../decisions/0133-the-presence-prompt-on-each-operating-system-and-what-it-is-said-to-prove.md)). Spec section 222 states plainly what each protects against.

## The presence prompt

| System | Prompt | State |
| --- | --- | --- |
| Linux | polkit `CheckAuthorization` on the system bus, for the engine's own process and the one action `io.github.mhughesdev.anything2mcp.presence` | Built: `src/presence/polkit.rs` |
| macOS | The system's own authentication | Not built: `NoPrompt` answers `not_built` |
| Windows | Windows Hello | Not built: `NoPrompt` answers `not_built` |

**The policy file** is `crates/a2m-local/polkit/io.github.mhughesdev.anything2mcp.presence.policy`: one action, `allow_any` no, `allow_inactive` no, `allow_active` `auth_self`. An administrator copies it into `/usr/share/polkit-1/actions`; the remedy the engine gives says so. A test holds the file to the action id in the code and to those defaults. The packages do not carry the file yet.

**How the Linux prompt answers.** Every ask connects to the system bus anew, so a bus that comes or goes while the engine runs is simply found, and a connection that ends takes its dialog along.

1. Before anybody is asked, the authority is asked without interaction. Only "would have to ask" goes on. A yes at that point was given without asking anybody (an administrator's rule, a kept authorization, or an engine run as the administrator account) and is refused as `granted_without_asking`; a plain no means the engine is outside a local, active session.
2. polkit refuses a check for an action it does not know. Its list of actions tells the missing policy file from any other refusal.
3. The check that may ask carries no details, the flag that allows interaction, and a cancellation id. At the deadline the id cancels the dialog and the answer is a dismissal.

| What polkit or the bus says | Before anybody is asked | After the prompt |
| --- | --- | --- |
| Authorized, no challenge, not dismissed | `not_available`, `granted_without_asking` | `Present(polkit)` |
| Not authorized, challenge | goes on to the prompt | `not_available`, `no_agent` |
| Not authorized, no challenge, dismissed | `not_available`, `session_not_active` | `Dismissed` |
| Not authorized, no challenge | `not_available`, `session_not_active` | `Denied` |
| Authorized together with challenge or dismissed | `not_available`, `granted_without_asking` | `not_available`, `check_failed` |
| No system bus | `not_available`, `no_system_bus` | the same |
| The bus has no polkit, or cannot start it | `not_available`, `no_polkit` | the same |
| polkit refuses, and does not list the action | `not_available`, `policy_not_installed` | the same |
| polkit refuses, and lists the action | `not_available`, `check_failed` | the same |
| polkit says the check was cancelled | `not_available`, `check_failed` | `Dismissed` |
| Any other error, or no answer in ten seconds to a call that asks nobody | `not_available`, `check_failed`, with the error's name | the same |

**What `doctor.run` and `serve` say.** `LocalSurface::presence` gives the kind, the prompt's name, the reason, a sentence, and the remedy. The product's `serve` writes them as one line before it says it is ready. `doctor.run` does not report them yet: `PersonSurface` has no way to ask a surface what its presence check is.

**The record.** `PromptAnswer::Present` names the prompt that answered. The policy engine's `PresenceCheck` has one word for every operating system's prompt, `os_prompt`, so that is what is recorded today; the page's answer names the prompt. One function of the surface, `recorded_check`, maps the name to the word, for when the policy engine has the three words.

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
| The socket or pipe, the engine lock, the start lock | The runtime directory from `Paths`; the engine lock also in the data directory |
| Whether a prompt is up | Memory: one lock in the surface |
| Pending approval pages: one-time code, diff reference, deadline | Memory only; lost on restart, which is a decline |
| Open connections and their labels | Memory |

## Failures

| Class | Code | When |
| --- | --- | --- |
| `busy` | `engine_running` | `--in-process` or a second `serve` while the engine lock is held |
| `transient` | `connection_lost` | The engine went away during a request, or closed the connection |
| `policy` | `peer_not_allowed` | An engine owned by another user. A connection from another user is closed unread, so that client sees `connection_lost` |
| `policy` | `presence_failed` | The person dismissed or failed the prompt, or none can be raised on this machine; the message gives the reason |
| `busy` | `prompt_busy` | A presence prompt is already up, and one is raised at a time |
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
| An error of the operating system's prompt is never an approval | Built, for polkit: every verdict and every fault, before and after the prompt, against a made-up authority behind the traits the real bus implements; exactly one combination is presence |
| A prompt is raised only when the operating system would have to ask | Built, for polkit: the same walk shows no prompt raised after any other first answer |
| One prompt is up at a time | Built: a press on a second page and a protected setting, while a prompt is up, raise nothing and are told busy |
| A prompt ends with the approval window | Built: a prompt nobody answers is dismissed at the page's deadline and the run ends as declined; for polkit, the cancellation id of the check that was raised is the one cancelled |
| The page shows the stored change as text | Built: the page shows the record's fields from what to what, and a value that holds markup and a place-holder of the page arrives as text |
| The engine lock outlives the channel | Built: after `serve` returns the lock is held and a second bind is refused, until the lock that came back is dropped |

## Build order

1. Done: `Paths` from `a2m-actions`, the engine lock, and the socket on Linux and macOS with the peer check and the client's check of the owner. Two clients share one engine. Run on Linux only; the macOS code is compiled and linted, not run.
2. Written, not run: the named pipe on Windows, with both checks. It is compiled and linted for Windows through a crate that holds only the operating-system module, because the whole crate's tree compiles SQLite. CI must show its tests and the channel's tests passing on Windows. Hypothesis H-06 is not yet measured.
3. Partly done: the hello with the version exchange, the idle stop, and `EngineLock::held` for `--in-process`. Not built: start on demand and the start lock, the drain and restart of an older engine, the `engine.*` actions of the client end, and the 30 ms and cold-start numbers of spec section 222.
4. Done: the loopback listener and the approval page, which shows the stored change through `ApprovalDesk::preview`, with the prompt behind an interface. Not built: the words above the records for uploads, sends, and anything irreversible.
5. Done: the presence prompt's interface as [decision 0133](../../decisions/0133-the-presence-prompt-on-each-operating-system-and-what-it-is-said-to-prove.md) gives it: availability asked at run time with a kind and a reason, a deadline on every ask, one prompt at a time, and the name of the prompt that answered.
6. Done, with what only a prepared machine can show: the Linux prompt over polkit. Run here: the whole mapping against a made-up authority; the real bus code against an address where nothing listens; and the real bus code against a real polkit on a bus of the test's own, for the cases that need nothing installed (no polkit on the bus, and no policy file). Not run anywhere yet: the cases that need the policy file installed and a test account, which the CI job prepares (below). Never run: a dialog on a real desktop, under Wayland and under X11, which needs a person once.
7. Not built: the macOS and Windows prompts, the fallback dialog, notifications, the sign-in status page, and the way to a session's person. On macOS and Windows no approval is recorded on one machine and no protected setting is loosened. H-12 is open.

**The test against a real polkit** is `against_a_real_polkit_the_prompt_answers_as_the_machine_is_prepared`. It runs only when `A2M_TEST_POLKIT` says how far the machine was prepared, and is otherwise skipped by name:

| Value | The machine | The prompt must answer |
| --- | --- | --- |
| `no_polkit` | A system bus with no polkit on it | `not_available`, `no_polkit` |
| `no_policy` | polkit runs; the policy file is not installed | `not_available`, `policy_not_installed` |
| `no_session` | The policy file is installed; the test runs as an ordinary account in no local, active session | `not_available`, `session_not_active` |
| `root` | The same, run as the administrator account | `not_available`, `granted_without_asking` |
| `no_agent` | A rule of the job gives the test account `auth_self` for the action, standing in for the seat a CI machine does not have; no agent runs | `not_available`, `no_agent` |
| `ready` | The same, and the test starts polkit's text agent for its own process and types the test account's made-up password | no agent first; a wrong password is not presence; the right one is `Present(polkit)`, and polkit would ask again after it; a dialog nobody answers is cancelled at the deadline, and the next ask passes |

## Open points

- Settled by [decision 0117](../../decisions/0117-the-local-channel-and-the-person-surface-on-one-machine-as-built.md): the pipe, the peer checks, and the locks need no new crate. Still open from research brief 26: starting a detached process on each system.
- Settled by [research note 20](../../research/20-operating-system-user-presence-checks.md) and decisions [0133](../../decisions/0133-the-presence-prompt-on-each-operating-system-and-what-it-is-said-to-prove.md) and [0142](../../decisions/0142-the-presence-prompt-interface-and-the-linux-prompt-as-built.md): which presence prompts a background process can raise, and the Linux one as built. Still open: how the policy file reaches a machine that installed the product through pip or npm, which carry one binary; whether a desktop's polkit agent closes its dialog when the check is cancelled; and the hand check on a desktop.
- How `doctor.run` learns the presence check: a method on `PersonSurface` that the dispatcher's `doctor.run` asks, most likely.
- The record of an approval names the prompt only once the policy engine's `PresenceCheck` has the three names.
- Whether an engine that is not elevated can read the user of an elevated client's process on Windows. If it cannot, that client is refused, which is the safe side.
- Settled by [decision 0132](../../decisions/0132-the-action-registrys-second-pass-a-decline-ends-the-run-and-a-wait-reads-the-schedulers-own-table.md): the page reads the stored diff through `ApprovalDesk::preview`, and a declined run ends as `expired`. Still open: a change of more than 2,000 records cannot be approved on the page; it needs paging that a person can follow.
- How a drained engine is restarted by the newer client: the client starts the new one after the old has released the engine lock, most likely.
- Whether the hardened Workstation service, under its own account, needs the access rule widened to name one other user, and how that is kept from becoming a hole.
- Where `engine.logs` reads from when the engine is not running and logs are rotated.
