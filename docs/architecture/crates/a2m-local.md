# `a2m-local`

The engine on one machine, and the channel to it. This crate holds both ends of the local channel: the listener inside the engine, and the connector every thin client uses to find the engine, start it when nothing answers, and check its version. It also holds the short-lived loopback listener that serves an approval or sign-in page, and the operating system's notification and user-presence prompt: polkit on a Linux desktop, LocalAuthentication on macOS, and Windows Hello on Windows; the last two are written and not yet run. Nothing reached through the channel can approve.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 8, Roles |
| May depend on | Layers 1 to 7 |
| Depends on | `a2m-core`, `a2m-actions`, `a2m-telemetry`, and for the person surface `a2m-policy`, `a2m-auth`, `a2m-vault`. Outside: Tokio, axum, `hyper`, `hyper-util`; `rustix` on macOS and Linux and `windows-sys` on Windows ([decision 0117](../../decisions/0117-the-local-channel-and-the-person-surface-on-one-machine-as-built.md)). On Linux only, for the presence prompt: `zbus` 6.3.0 (no default features; `tokio`, `proxy`) and `zbus_polkit` 6.0.0 ([decision 0142](../../decisions/0142-the-presence-prompt-interface-and-the-linux-prompt-as-built.md)). On macOS only: `objc2-local-authentication` 0.3.2, `objc2` 0.6.5, `objc2-foundation` 0.3.2, and `block2` 0.6.2, with no default features. On Windows only: `windows` 0.62.2 (`Security_Credentials_UI`, `Win32_System_WinRT`, `Win32_Foundation`) and `windows-future` 0.3.2, and more features of `windows-sys` ([decision 0146](../../decisions/0146-the-macos-and-windows-presence-prompts-as-built.md)). A crate for notifications is still to be chosen |
| Roadmap packages | Roadmap package 1.4 (the channel, the lifecycle); 3.6 (the loopback listener, the presence prompt, notifications) |
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
    /// The hello also names these variables, each by its name and a digest, never the value
    /// (decision 0192): a client tells whether its own settings variables differ.
    pub fn with_start_variables(self, variables: &BTreeMap<String, String>) -> Channel;
    pub fn control(&self) -> ChannelControl;
    /// Serves the router the product crate composed: the REST router of a2m-api and
    /// the MCP endpoint of a2m-mcp. Every connection is checked before its first byte is read.
    /// The channel answers the hello; every other request carries a `Peer` in its extensions.
    /// The engine lock comes back, still held: the caller drops it once the kernel has closed.
    pub async fn serve(self, service: axum::Router, idle: IdlePolicy) -> (StopReason, EngineLock);
    pub fn lock_place(&self) -> LockPlace;   // the engine asks it before each use it can see
}
pub struct ChannelControl { /* cheap to clone */ }
impl ChannelControl { pub fn stop(&self, why: StopReason); pub fn refused(&self) -> u64; }
pub enum StopReason { Idle, Asked, Restart, Failed, DataGone }   // DataGone: decision 0181

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

// The engine lock, taken by whatever opens the store: `serve` at the bind, and a
// command that runs the engine inside itself, held until its kernel has closed.
pub struct EngineLock { /* two file locks: runtime directory and data directory */ }
impl EngineLock {
    pub fn take(paths: &Paths) -> Result<EngineLock, Failure>;
    pub fn held(paths: &Paths) -> Result<bool, Failure>;   // a question for a report, never a guard
    pub fn place(&self) -> LockPlace;
}
// Which file the lock of the data directory is (decision 0181): `holds` asks
// whether that file still stands at its place, by device and file number on
// macOS and Linux, by the file standing there on Windows. Cheap to clone.
pub struct LockPlace; impl LockPlace { pub fn holds(&self) -> bool; }

// Who may change a file or a folder: on macOS and Linux, Private when its owner is
// the user or the administrator, nobody else may write, and a group may only when
// it is the user's primary group; NotChecked on Windows. The product's project walk.
pub enum Kept { Private, Open(&'static str), NotChecked }
pub fn kept_by(path: &Path) -> std::io::Result<Kept>;
// One of the engine's own folders made this user's alone, as the runtime folder is:
// 0700 on Unix, an access list naming only this user on Windows (decision 0241).
// On Unix the folder is opened without following a link at its end, and its type,
// owner, and mode are checked and set through that one handle (decision 0273).
pub fn keep_folder_private(dir: &Path) -> std::io::Result<()>;

// The client end. Used by commands, the stdio proxy, and the SDK bridge.
pub struct Connector { /* the channel's place, this program's version, the client label */ }
impl Connector {
    pub fn new(paths: &Paths, version: Version, label: ClientLabel) -> Result<Connector, Failure>;
    /// Finds the engine, checks its owner before sending anything, and says hello.
    /// It starts nothing: when no engine answers it fails with busy / engine_unreachable.
    /// The hello waits at most HELLO_PATIENCE (30 s); an engine that holds the channel
    /// and does not answer is busy / engine_unreachable, naming the engine's output file.
    pub async fn connect(&self) -> Result<Link, Failure>;
}
pub const HELLO_PATIENCE: Duration;
// Start on demand (decision 0165): the client takes the start lock, looks again, starts
// `start` detached, and waits for the channel to answer, all within `patience`.
pub struct Launch {
    pub program: PathBuf, pub args: Vec<String>, pub cwd: PathBuf,
    pub environment: BTreeMap<String, String>, // the whole environment; nothing else is passed on
    pub log: PathBuf,                          // standard output and error, appended
    pub input: Option<Secret>,                 // written once to standard input, then closed
}
impl Launch { pub fn log_of(paths: &Paths) -> PathBuf; }   // `engine.log` in the log directory
pub enum Started { Found(Link), Launched(Link), Refused(Failure) } // Refused: no process could be started
impl Connector {
    /// The `Launch` is owned by the blocking task that starts the engine (decision 0192).
    pub async fn connect_or_start(&self, paths: &Paths, start: Launch, patience: Duration)
        -> Result<Started, Failure>;
}
pub const START_LOCK_NAME: &str; pub const STARTED_LINE: &str;
pub const STOPPED_LINE: &str;              // how the line begins that an engine writes as it stops before it serves
pub fn stopped_line(failure: &Failure) -> String;  // that line: its process's number and the failure as JSON
pub const ENGINE_LOG_NAME: &str;   // "engine-output.log", not telemetry's engine.log (decision 0181)
pub struct Link { /* an HTTP/1.1 connection over the channel; the engine's hello */ }
impl Link {
    pub fn engine_version(&self) -> Version;
    /// The names of the start variables that differ from `client`'s own; None when the
    /// engine named none in its hello.
    pub fn start_variables_differing(&self, client: &BTreeMap<String, String>) -> Option<Vec<String>>;
    /// transient / connection_lost only when the request was handed back unwritten;
    /// possible_commit / answer_lost once any of it may have reached the engine.
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
    NoAgent, GrantedWithoutAsking, NoWindowStation, SessionRemote, SessionLocked,
    NotSetUp, DisabledByPolicy, DeviceBusy, LockedOut, CheckFailed,
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
pub fn system_prompt() -> Arc<dyn PresencePrompt>;  // polkit, LocalAuthentication, Windows Hello; NoPrompt elsewhere
pub fn system_prompt_for(paths: &Paths) -> Arc<dyn PresencePrompt>;  // the same; on Linux it writes the policy file's copy into the data directory (decision 0220)
pub async fn presence_available() -> Availability;  // what system_prompt() says of itself, for doctor.run
```

The product crate starts the role in this order: `Channel::bind`, `LocalSurface::new` with `system_prompt_for(&paths)`, `Kernel::open` with it, then `Channel::serve` with one router that merges the REST router and the MCP endpoint. When `serve` returns, the product closes the kernel and only then drops the engine lock that `serve` handed back. A front door reads the `Peer` from a request's extensions and hands `Peer::credential()` to `Kernel::verify_caller`. `KernelParts::os_user` is `OsUser::current()` as text.

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

A pipe has no queue of connections as a socket has. The engine always has one instance of the pipe waiting, from the moment it binds, and makes the next before it hands a connection on; an instance a client left before it was taken is replaced by a new one, made first. Between the bind and the first accept, while the engine opens its store, the one waiting instance takes one client and the pipe is busy for the rest. A client that finds the pipe busy waits on the system's own wait for a free instance (`WaitNamedPipeW`), in waits of at most 200 ms, until its patience (the hello's) ends, and then is told `engine_unreachable` with the engine's log, as for a hello that is not answered; a pipe that is not there is no engine. Anything else the system refuses is `channel_unavailable`, with the system's error by its number and its words.

On macOS and Linux the socket's path must fit the system's address of a socket: at most 103 bytes on macOS and the BSDs, 107 on Linux, which the standard library's own check of `sockaddr_un` gives. Both ends check it before anything else: the engine before it takes a lock or makes a folder, a client before it connects or starts an engine; a path too long is `input`/`socket_path_too_long`, which names the path, its length, the limit, and `A2M_HOME` as the way to a shorter one.

**Client label.** The command line, a named MCP client (from what the client says of itself), or an SDK. It goes into the audit log and fair queuing and nowhere else. Every local client is the same principal, the operating-system user, and an agent: the product's layer makes no caller a person for what its label or a header says ([decision 0158](../../decisions/0158-the-product-crate-and-the-local-engine-after-their-safety-audit.md)).

## Lifecycle

| Step | What happens | Detail |
| --- | --- | --- |
| Find | The client connects to the fixed path from `Paths` | No answer, or a refused connection on a stale socket, means not running |
| Start | The client takes the start lock, checks again, and starts the same program's `serve`, detached: on macOS and Linux in a session of its own, on Windows with no console, in a process group of its own, and out of the caller's job where the job allows it. Its environment, folder, and standard streams are what the product's `Launch` says ([decision 0165](../../decisions/0165-a-client-starts-the-engine-on-demand-detached-and-gives-it-the-passphrase-on-a-pipe.md)) | A second client arriving at once waits on the start lock and then connects. A refused start is `Started::Refused`, and the caller may run the engine inside itself |
| Ready | The engine binds the channel, which takes the engine lock, then opens the kernel, and answers a hello only once `Kernel::open` has returned, in whatever health state | The client waits on the channel answering its hello, up to a fixed deadline (30 seconds for an engine it started, and `HELLO_PATIENCE`, 30 seconds, for any hello), then exits with code 4, naming the engine's output file. A hello whose connection ends is an engine that stopped while starting: start on demand looks on, and a plain connect says so and names the file ([decision 0181](../../decisions/0181-the-product-and-the-local-engine-take-up-what-the-kernel-now-offers.md)) |
| Version | A newer client asks an older engine to drain and restart. If runs are active, the client reports that and leaves the engine alone | An older client reaching a newer engine is served while the local API version matches; otherwise it is told to upgrade |
| Idle stop | After `engine.idle_stop_minutes` with no connections and `IdleHolds::busy` false, the engine saves sessions to the vault, closes the kernel, and exits | A waiting page and a run in hand are idle holds: the product's hold asks `Kernel::runs_in_hand`. The keeper's keep-alive window is to be one |
| Stop | `Channel::serve` returns the engine lock, still held; the product closes the kernel and then drops it | No second engine opens the store while the first still writes to it |
| Requests at a stop | Once `ChannelControl::stop` is called, by `engine.stop` or by a signal, the channel takes no new connection. A request it is already answering, the one that asked for the stop included, is answered before `serve` returns. A connection with no request in hand is closed at once, so a request still on its way to the engine when the stop lands is closed unread | Its client reports `possible_commit`/`answer_lost`: it cannot tell a request the engine never read from one whose answer was lost. `serve` waits for a request in hand with no bound (not built: a bound and a named outcome when it passes) |
| Crash | The engine lock is an operating-system lock, so it is released when the process dies | The next start sets orphaned runs to `stopped`, as `Kernel::open` describes |
| Data directory gone | The lock of the data directory remembers which file it is (`LockPlace`). The product asks it before each request and at each idle look; when the file is gone or another, the request is refused `busy`/`data_directory_gone` and the channel stops with `StopReason::DataGone` | Spec section 224: fail closed. The next client starts a new engine ([decision 0181](../../decisions/0181-the-product-and-the-local-engine-take-up-what-the-kernel-now-offers.md)) |
| In process | A command that finds no engine takes the engine lock in the same step and holds it until its kernel has closed; `--in-process` is refused while an engine answers | The product crate opens the kernel inside the command, with no channel. A second command waits a bounded time for the lock; `serve` is refused at its bind |

Two locks are used. The engine lock is held by the engine for its whole life and proves one engine per user and per data directory. It is two operating-system file locks taken together, one in the runtime directory and one in the data directory, because two runtime directories can lead to one data directory and two engines on one store must never run. The start lock is held by a client only while it starts the engine, so two clients never start two; it is a file lock in the runtime directory, `start.lock`, taken without waiting and looked at again every 25 ms while the channel is looked at too. No lock is judged by a file merely existing.

The `engine.*` actions are carried out here, in the client end, because they act on the process: `engine.start` runs the start procedure; `engine.stop` asks the engine to drain; `engine.status` reports `not running` when nothing answers and otherwise dispatches; `engine.logs` reads the log directory from `Paths`.

## Nothing on this machine can approve

| Rule | How it is held |
| --- | --- |
| No approve command, tool, or route | The registry has no such action, and the router has no route to `ApprovalDesk`; `a2m-actions` tests both |
| The loopback listener carries no API | It serves the approval page, the stored diff it was opened for, and two presses: approve and decline; and the page of a person's act, what the desk holds for it, and two presses: carry out and decline. Nothing else answers |
| A person's act is carried out only from its page | A press of Carry out raises the presence prompt; only its success reaches `ApprovalDesk::carry_out`, with a proof given for that act. A press without the page's value, from another origin, with the prompt not passed, a second press, or a press after the window does nothing |
| One prompt at a time | The surface holds one lock while a prompt is up. A second press, or a protected setting meanwhile, raises nothing: the page answers 503, and `confirm_presence` fails with `busy`/`prompt_busy` |
| No prompt but a page's while a page waits | `confirm_presence` fails with `busy`/`prompt_busy` while any page waits, so no dialog comes up that the person reading a page could take for its own. The page says to answer a prompt only right after one's own press, and shows the twelve characters a prompt that carries words shows. polkit's dialog carries none, so on Linux a program that holds another waiting page's link can still press it first; the person's press is then told that another prompt is up (section 503 states the limit) |
| A prompt ends with the window | `ask` is given the end of the page's approval window; the prompt is cancelled then and the answer is a dismissal. The surface keeps the window itself too: an answer that comes back at or after the deadline is a dismissal, whatever it says. A press is carried out in a task of its own, so a browser that goes away leaves no page marked as pressed |
| An error is never an approval | Only `PromptAnswer::Present` records an approval with proof of presence. Every failure of the operating system's prompt is `Dismissed`, `Denied`, or `NotAvailable` with a reason |
| A press is not an approval | Approve raises the presence prompt; only its success reaches `ApprovalDesk`. Where no prompt can be raised, nothing is approved, and the page says so, with the reason and the remedy, before anything is pressed |
| The page is only for the person | A random port, an address with a one-time code, and a check of `Host` and `Origin` on every request; a press must come from the page's own origin. The page's referrer policy is `same-origin`: under `no-referrer` a browser sends `Origin: null` with the form's post, and no real press could work. `null` is never accepted, because a sandboxed frame or a `data:` page sends it too |
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
| macOS 13 and later | `LAContext` with the policy of the device's owner: Touch ID or a paired watch, and the account password where neither can be used | Written, not run: `src/presence/local_authentication.rs` |
| Windows 10 and 11 | Windows Hello, through `IUserConsentVerifierInterop::RequestVerificationForWindowAsync`, with `CheckAvailabilityAsync` read first | Written, not run: `src/presence/windows_hello.rs` |

**The policy file** is `crates/a2m-local/polkit/io.github.mhughesdev.anything2mcp.presence.policy`: one action, `allow_any` no, `allow_inactive` no, `allow_active` `auth_self`. It is in the binary. When polkit does not know the action, the availability writes a copy into the folder `polkit` of the data directory (made 0700 and checked to be the user's, as the runtime directory is; the file written whole under another name, never through a link, moved into place, mode 0644, and written again when it differs), and the remedy names that copy and gives the one command a person with administrator rights runs: `sudo sh -c '...'`, which copies the file to a temporary file of root's, checks its SHA-256 against the policy the binary carries, and installs it into `/usr/share/polkit-1/actions` with mode 0644 only when they match. The bytes checked are the bytes installed, so a copy that a program of the user changed is refused: the administrator never installs a policy the user's programs chose. A path with anything but letters, digits, and `/._-+,@` is not put in a command; the remedy then gives the file, the destination, and the digest. The engine never runs the command, never asks for elevation, and writes nothing outside its data directory ([decision 0220](../../decisions/0220-a-first-run-on-linux-needs-no-administrators-file.md)). A test holds the file to the action id in the code and to those defaults. The packages do not carry the file yet.

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
| The bus has no polkit, or cannot start it: the service is unknown, has no owner, or its start failed | `not_available`, `no_polkit` | the same |
| polkit refuses, and does not list the action | `not_available`, `policy_not_installed` | the same |
| polkit refuses, and lists the action | `not_available`, `check_failed` | the same |
| polkit says the check was cancelled | `not_available`, `check_failed` | `Dismissed` |
| Any other error, or no answer in ten seconds to a call that asks nobody | `not_available`, `check_failed`, with the error's name | the same |

**How the macOS and Windows prompts answer** ([decision 0146](../../decisions/0146-the-macos-and-windows-presence-prompts-as-built.md)). Each is one file with the same shape as the Linux one: a small trait over the system (`Authenticator`, `Verifier`), the real system behind it in a module compiled on its own system only, and total functions from what the system says to what the prompt answers. The files are compiled into the tests on every system, so the whole mapping is tested everywhere against a made-up system.

1. Before anybody is asked, the session is read, then the system is asked without showing anything (`canEvaluatePolicy`, `CheckAvailabilityAsync`). Only a session at the screen, not locked, whose system says it can ask, goes on.
2. The prompt carries the engine's words: `{title}: {detail}` of `PromptText`, so the tool and the first twelve characters of the change's hash. macOS shows them after "is trying to"; Windows shows them as the dialog's message.
3. Dropping the future of the system's ask takes the dialog away: macOS invalidates the context, which lives on a thread of its own because it may not move between threads; Windows cancels the operation and then closes its owner window. So the deadline, and an ask dropped for any other reason, both close the dialog. macOS sets the reuse duration to none on every context, so no earlier unlock counts.
4. Windows needs a window to own the dialog. Each ask makes one of its own: a hidden window of the system's `STATIC` class, on a thread that runs its messages until the ask ends.

| What the system says | macOS | Windows | The answer |
| --- | --- | --- | --- |
| No graphical session, or another user's at the screen | `CGSessionCopyCurrentDictionary` is null, or not on the console | | `not_available`, `session_not_active` |
| The services' session, or a window station that is not `WinSta0` | | session 0, or the station's name | `not_available`, `no_window_station` |
| Remote desktop | | `SM_REMOTESESSION` | `not_available`, `session_remote` |
| The screen is locked | the session's lock key, which Apple does not document | the session's flags | `not_available`, `session_locked` |
| Nothing set up to check | no password; no biometry, watch, or pairing | device not present; not configured for the user | `not_available`, `not_set_up` |
| Turned off by policy; busy | | disabled by policy; device busy | `not_available`, `disabled_by_policy`; `device_busy` |
| Locked out, before the prompt | biometry lockout | | `not_available`, `locked_out` |
| No interface may be shown | not interactive | | `not_available`, `session_not_active` |
| The person passed | success | verified | `Present` |
| Cancelled by the person, the system, or the engine | user cancel, user fallback, system cancel, app cancel | canceled | `Dismissed` |
| Failed, or locked out after the prompt | authentication failed, biometry lockout | retries exhausted | `Denied` |
| Anything else: an unknown code or result, an error of another domain, a failed call, an unknown lock state, no answer in ten seconds to a call that asks nobody | | | `not_available`, `check_failed`, with the code |

**What CI shows, and what only a person can.** On a macOS and a Windows runner, one test each calls the real `available()` through `system_prompt()` and asserts only that it answers one of the named outcomes within the bound, and writes the answer to the log; another holds this build's codes to the system crate's constants. The real `ask` is never called in CI. A person checks once on each system: the prompt comes to the front after a press in the browser; it shows our words with the same twelve characters the page shows; a prompt left alone closes at the deadline; synthetic input cannot press it; and, on macOS, that a program with no bundle gets the dialog at all, and what its title says.

**What `doctor.run` and `serve` say.** `LocalSurface::presence` gives the kind, the prompt's name, the reason, a sentence, and the remedy. The product's `serve` writes them as one line before it says it is ready. `doctor.run` does not report them yet: `PersonSurface` has no way to ask a surface what its presence check is.

**The record.** `PromptAnswer::Present` names the prompt that answered. The policy engine's `PresenceCheck` has one word for every operating system's prompt, `os_prompt`, so that is what is recorded today; the page's answer names the prompt. One function of the surface, `recorded_check`, maps the name to the word, for when the policy engine has the three words.

Sign-in is the keeper's work, not this crate's. `ask_sign_in` is to notify the person and serve a status page on the same loopback listener while the keeper's sign-in window, opened by the pack's auth provider, is waiting. Today it returns words and no link. `ask_session` answers a request for a one-time code (`PersonAsk::EnterCode`) on the page of the same person's press of a sign-in while that press is carried out ([decision 0205](../../decisions/0205-a-sign-in-an-agent-asks-for-is-carried-out-by-its-person-on-the-engines-page.md)); every other request, and a code asked with no such press in flight, is answered that the person cannot be reached: no notification, no status page, and no return address are built. `give_link` is for a sign-in provider outside the web pack that needs the person's consent in a browser, such as authorization-code sign-in or an upstream MCP server's: it raises a notification with the link and returns it to the calling client, and the person opens it in their own browser. Nothing here starts, reads, or drives that browser.

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
| `transient` | `connection_lost` | The connection ended before a request was sent, so the engine never had it; also a hello whose answer was lost |
| `possible_commit` | `answer_lost` | A request may have reached the engine and no whole answer came back: never said to be safe to send again. Its message, and `connection_lost`'s, ends with what the client saw, in one of six fixed phrases (the connection was reset; the connection closed before any answer; the answer was cut short; the write of the request failed; the answer was not well formed; no detail), never the HTTP library's own text |
| `policy` | `peer_not_allowed` | An engine owned by another user. A connection from another user is closed unread, so that client sees `connection_lost` |
| `policy` | `presence_failed` | The person dismissed or failed the prompt, or none can be raised on this machine; the message gives the reason |
| `busy` | `prompt_busy` | A presence prompt is already up, and one is raised at a time |
| `busy` | `engine_unreachable` | Nothing answers on the channel; or an engine started on demand ended before it answered, or did not answer within the deadline, or on Windows kept every instance of its pipe taken for as long, with the log to read; the command exits with code 4 |
| the engine's own | the code it stopped with | An engine started on demand stopped before it answered and wrote why with `stopped_line`, as one that cannot open its vault does: the client answers with that class, code, and sentence, and names the log ([decision 0220](../../decisions/0220-a-first-run-on-linux-needs-no-administrators-file.md)). A line of another process, or one that claims a possible commit, is not taken
| `busy` | `channel_unavailable` | A directory, a lock file, the channel, or the loopback listener cannot be opened; the message gives the system's error by its number and words, or its kind where it has no number |
| `input` | `socket_path_too_long` | The engine's socket would have a path longer than the system takes for a socket's address: the path, its length, and the limit are named, and `A2M_HOME` set to a shorter folder is the way out |
| `input` | `hello_invalid` | A connection's first request was not a well-formed hello |
| `input` | `local_api_mismatch` | The client and the engine speak different versions of the local API |
| `busy` | `surface_unavailable` | The surface holds no approval desk, so it shows no page |
| `busy` | `data_directory_gone` | The engine's data directory was removed or replaced while it ran: the request is not handed to the kernel, and the engine stops. `codes::data_directory_gone()` gives the failure the product answers with |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Two clients started at the same moment produce one engine | Built: two clients at once share one engine, and a second engine on the same data directory is refused; and in the product crate, `two_commands_at_once_start_one_engine`: two commands with no engine running start one, as the log's one start line and one ready line show |
| An engine started on demand holds none of its caller's handles and none of its credentials | Built, in the product crate: `a_command_with_no_engine_starts_one_detached_and_is_answered`: the command's output is read to its end while the engine runs, and on Linux the engine is in a session of its own, in its data directory, with the passphrase in neither its environment nor its arguments |
| A different user is refused, before any byte is read | Built: an engine that expects another user closes this user's connection unread, and the service sees nothing. Not built: a test run as a second account in CI |
| A client refuses an engine owned by someone else | Built: a client that expects another user's engine sends nothing. Not built: a socket or pipe planted by another account |
| A crashed engine leaves no lock that blocks the next start | Built: a stale socket is not an engine and the next engine binds over it; a dropped lock is free. Not built: a test that kills an engine process |
| No TCP port is open except while an approval or sign-in is pending | Built: the page's port refuses connections once nothing waits. Not built: a listing of the process's sockets |
| The loopback listener answers only its page and two presses | Built: a test that sends other paths and methods, a front door's among them |
| A script that presses Approve does not approve | Built: a press without the page's value, from another origin, or with the prompt not passed records nothing. Hypothesis H-12 itself needs a real prompt |
| A real browser's press works | Built, in the product crate: `a_write_through_the_served_engine_rests_is_approved_on_its_page_and_is_written_once` presses Approve in Chrome through the web pack's browser engine |
| A late answer of a prompt proves nobody | Built: `a_prompt_that_answers_after_the_window_records_nothing` |
| An error of the operating system's prompt is never an approval | Built, for polkit: every verdict and every fault, before and after the prompt, against a made-up authority behind the traits the real bus implements; exactly one combination is presence |
| A prompt is raised only when the operating system would have to ask | Built, for polkit: the same walk shows no prompt raised after any other first answer |
| One prompt is up at a time | Built: a press on a second page and a protected setting, while a prompt is up, raise nothing and are told busy |
| A prompt ends with the approval window | Built: a prompt nobody answers is dismissed at the page's deadline and the run ends as declined; for polkit, the cancellation id of the check that was raised is the one cancelled |
| The page shows the stored change as text | Built: the page shows the record's fields from what to what, and a value that holds markup and a place-holder of the page arrives as text |
| A write that will not be read back says so before it is approved | Built: `a_write_that_will_not_be_read_back_says_so_on_its_page_before_it_is_approved`: after the record's heading, "This write is accepted by the system, not read back. Nothing will read it back or compare it after it is sent." |
| The page names the binding a write is bound to | Built ([decision 0192](../../decisions/0192-a-served-engine-hands-no-root-of-the-project-that-started-it-and-clients-start-and-compare.md)): beside each record, the binding `PreviewRecord::bindings` names, by its mode, and what its evidence means for whether the write is read back; for a record whose binding is chosen later, `BINDING_LATER` and every binding it could take, and `MAY_NOT_BE_READ_BACK` in its own sentence (`a_record_whose_binding_is_chosen_later_shows_every_binding_it_could_take_and_what_each_means`) |
| What is pressed on an act's page is what it showed | Built: `a_persons_act_whose_input_the_page_cannot_show_whole_is_not_carried_out_from_it`: a value cut to the page's length is said above the input, and a press raises no prompt and carries nothing out |
| A client waits for no engine for ever | Built: `an_engine_that_binds_and_stalls_is_waited_for_no_longer_than_the_patience` and `an_engine_that_stops_while_starting_is_told_as_one_and_points_at_its_log` |
| The started engine's output goes to no file but its own | Built, on macOS and Linux: `the_started_engines_output_is_not_written_through_a_link_and_is_kept_to_the_user`. On Windows the file takes its folder's access list |
| The engine lock knows whether it is still in place | Built: `the_engine_lock_says_whether_it_is_still_the_file_at_its_place`; the removal itself is run on macOS and Linux only |
| A request in hand when the channel stops is answered | Built: `a_request_being_answered_when_the_channel_stops_is_still_answered`: the request that stops the channel is answered only once a new connection is refused, and another request held past that answer is answered before `serve` returns |
| The engine lock outlives the channel | Built: after `serve` returns the lock is held and a second bind is refused, until the lock that came back is dropped |

## Build order

1. Done: `Paths` from `a2m-actions`, the engine lock, and the socket on Linux and macOS with the peer check and the client's check of the owner. Two clients share one engine. Run on Linux only; the macOS code is compiled and linted, not run.
2. Written, not run: the named pipe on Windows, with both checks. It is compiled and linted for Windows through a crate that holds only the operating-system module, because the whole crate's tree compiles SQLite. CI must show its tests and the channel's tests passing on Windows. Hypothesis H-06 is not yet measured.
3. Partly done: the hello with the version exchange, the idle stop, the engine lock held by a command that runs the engine inside itself ([decision 0158](../../decisions/0158-the-product-crate-and-the-local-engine-after-their-safety-audit.md)), and start on demand under the start lock, detached on each system ([decision 0165](../../decisions/0165-a-client-starts-the-engine-on-demand-detached-and-gives-it-the-passphrase-on-a-pipe.md)), run on Linux; the Windows start is compiled, not run. Not built: the drain and restart of an older engine, `engine.logs`, rotation of the engine's log, and the 30 ms and cold-start numbers of spec section 222.
4. Done: the loopback listener and the approval page, which shows the stored change through `ApprovalDesk::preview`, with the prompt behind an interface, and beside a record whose write is only acknowledged, that it will not be read back ([decision 0181](../../decisions/0181-the-product-and-the-local-engine-take-up-what-the-kernel-now-offers.md)). Not built: the words above the records for uploads, sends, and anything irreversible.
4a. Done ([decision 0163](../../decisions/0163-a-persons-act-an-agent-asks-for-is-carried-out-on-a-page-of-the-person-surface.md)): the page of a person's act an agent asked for, such as a publish, served by `ask_person_act` on the same listener at `/act/<code>`. It shows what `ApprovalDesk::person_act` reads back: the action, its summary, every input field as text, who asked, and for a publish what each tool may do. A press of Carry out raises the presence prompt, one at a time with every other; only a proof from it, made with `PresenceProof::for_act`, goes to `ApprovalDesk::carry_out`, and the answer page says what the act came to. Decline and silence call `ApprovalDesk::decline_act`. The page has the approval page's guard, headers, one-time value, and window. Tests: `a_persons_act_an_agent_asks_for_is_carried_out_once_from_its_page` and `a_persons_act_nobody_carries_out_ends_with_its_window_and_nothing_is_done`, through the real loopback page.
4c. Done ([decision 0205](../../decisions/0205-a-sign-in-an-agent-asks-for-is-carried-out-by-its-person-on-the-engines-page.md)): the page of a sign-in. Inside the form of Carry out, an empty password field `secret.<role>` for each credential `PersonActAsk::secrets` names, with its vault reference, never filled in and never shown back; a press may then carry up to 16 KiB, read as a form, each such field held as a `Secret` and handed with the proof to `ApprovalDesk::carry_out_with`. While a press of a sign-in is carried out, the keeper's request for that person's one-time code makes the press's answer a page (`pages/code.html`) that posts to `/code/<code>` with its own one-time value; the post hands the code over once and answers with what the act came to; a post without the value ends the sign-in. Tests: `a_sign_in_that_asks_for_a_code_asks_on_the_persons_own_page_and_keeps_it_nowhere` with a test provider that asks for a code; the credential field through the product in `first_hour_signed_in.rs`. Not built: the code asked of a person who is not pressing a page.
4b. Done (decisions [0194](../../decisions/0194-the-packs-the-product-and-the-local-surface-take-up-the-kernels-second-pass.md) and [0199](../../decisions/0199-the-halves-left-for-each-other-are-joined.md)): what the pages say of surfaces, in `surface::version_shown`, printed from the registry's own types. The publish page lists every surface of the version (`PersonActAsk::surfaces`) with its kind and its address in each environment, named, each notice in its fixed words, and every lint warning (`PersonActAsk::warnings`) with its code and words; the registry no longer repeats these as lines of `details`, so the page prints them once. Beside each record on the approval page, after the words on how it is written and before its fields, its surfaces' notices (`PreviewRecord::notices`). The counts name the records to write whose current values cannot be read (`ApprovalPreview::write`), and each such field's Now column holds `NOW_CANNOT_BE_READ` as the program's words, set apart, never as a value or as "does not exist". Every word escaped; a character that reorders, hides, or restyles text is shown as its escape. Tests: `the_publish_page_shows_where_each_surface_leads_its_notices_and_every_warning`, `a_records_notices_stand_beside_it_before_its_fields`, and `a_write_that_will_not_be_read_back_says_so_on_its_page_before_it_is_approved`, which rests a keyed write with no read-back through the real page; the publish page's surfaces through the product in `first_hour.rs`.
5. Done: the presence prompt's interface as [decision 0133](../../decisions/0133-the-presence-prompt-on-each-operating-system-and-what-it-is-said-to-prove.md) gives it: availability asked at run time with a kind and a reason, a deadline on every ask, one prompt at a time, and the name of the prompt that answered.
6. Done, with what only a prepared machine can show: the Linux prompt over polkit. Run here: the whole mapping against a made-up authority; the real bus code against an address where nothing listens; and the real bus code against a real polkit on a bus of the test's own, for the cases that need nothing installed (no polkit on the bus, and no policy file). Not run anywhere yet: the cases that need the policy file installed and a test account, which the CI job prepares (below). Never run: a dialog on a real desktop, under Wayland and under X11, which needs a person once.
   Done here, [decision 0220](../../decisions/0220-a-first-run-on-linux-needs-no-administrators-file.md): the copy of the policy file and the remedy's command, and the remedies of every other reason, which now say what a person can do, and say plainly where nothing can be done on that machine and that this is by design. All five stages of the test against a real polkit were run in a container on 2026-11-18, with the policy file installed for the later stages by the remedy's own command; a copy changed before the command ran was refused by it and nothing was installed. The engine that stops by name when it cannot open its vault is proven here with a child that writes the line; the product crate's `serve` writes it.
7. Written, not run: the macOS and Windows prompts ([decision 0146](../../decisions/0146-the-macos-and-windows-presence-prompts-as-built.md)). Run here, on Linux: their whole mapping against a made-up system. Compiled and linted for `aarch64-apple-darwin` and `x86_64-pc-windows-msvc`, tests included, through a scratch crate that holds only `presence.rs`, its files, and their tests, because the whole crate's tree compiles SQLite. CI on macOS and Windows is the first compile of the whole crate and the first run of the real `available()`; the real `ask` needs a person once on each.
8. Not built: the fallback dialog, notifications, the sign-in status page, and the way to a session's person. H-12 is open.

**The test against a real polkit** is `against_a_real_polkit_the_prompt_answers_as_the_machine_is_prepared`. It runs only when `A2M_TEST_POLKIT` says how far the machine was prepared, and is otherwise skipped by name:

| Value | The machine | The prompt must answer |
| --- | --- | --- |
| `no_polkit` | A system bus with no polkit on it | `not_available`, `no_polkit` |
| `no_policy` | polkit runs; the policy file is not installed | `not_available`, `policy_not_installed`; the copy is written into the test's folder with the bytes the binary carries and mode 0644, and the remedy's command names it and the destination |
| `no_session` | The policy file is installed; the test runs as an ordinary account in no local, active session | `not_available`, `session_not_active` |
| `root` | The same, run as the administrator account | `not_available`, `granted_without_asking` |
| `no_agent` | A rule of the job gives the test account `auth_self` for the action, standing in for the seat a CI machine does not have; no agent runs | `not_available`, `no_agent` |
| `ready` | The same, and the test starts polkit's text agent for its own process and types the test account's made-up password | no agent first; a wrong password is not presence; the right one is `Present(polkit)`, and polkit would ask again after it; a dialog nobody answers is dismissed at the deadline, and the next ask, through an agent of its own, passes |

## Open points

- Settled by [decision 0117](../../decisions/0117-the-local-channel-and-the-person-surface-on-one-machine-as-built.md): the pipe, the peer checks, and the locks need no new crate. Settled by [decision 0165](../../decisions/0165-a-client-starts-the-engine-on-demand-detached-and-gives-it-the-passphrase-on-a-pipe.md): starting a detached process on each system, with no new crate; `setsid` between fork and exec is the crate's one `unsafe` block for it.
- Settled by [research note 20](../../research/20-operating-system-user-presence-checks.md) and decisions [0133](../../decisions/0133-the-presence-prompt-on-each-operating-system-and-what-it-is-said-to-prove.md) and [0142](../../decisions/0142-the-presence-prompt-interface-and-the-linux-prompt-as-built.md): which presence prompts a background process can raise, and the Linux one as built; [decision 0146](../../decisions/0146-the-macos-and-windows-presence-prompts-as-built.md), the macOS and Windows ones as written. Settled by [decision 0220](../../decisions/0220-a-first-run-on-linux-needs-no-administrators-file.md): a machine that installed the product through pip or npm gets the policy file from the engine, which writes a copy and names the command that installs it; whether a package should install it on its own is still open; whether a desktop's polkit agent closes its dialog when the check is cancelled; the hand check on a desktop; and on macOS and Windows the hand checks listed under the presence prompt, of which the one most likely to change the design is whether the Windows prompt comes to the front.
- How `doctor.run` learns the presence check: a method on `PersonSurface` that the dispatcher's `doctor.run` asks, most likely.
- The record of an approval names the prompt only once the policy engine's `PresenceCheck` has the three names.
- Whether an engine that is not elevated can read the user of an elevated client's process on Windows. If it cannot, that client is refused, which is the safe side.
- Settled by [decision 0132](../../decisions/0132-the-action-registrys-second-pass-a-decline-ends-the-run-and-a-wait-reads-the-schedulers-own-table.md): the page reads the stored diff through `ApprovalDesk::preview`, and a declined run ends as `expired`. Still open: a change of more than 2,000 records cannot be approved on the page; it needs paging that a person can follow.
- How a drained engine is restarted by the newer client: the client starts the new one after the old has released the engine lock, most likely.
- Whether the hardened Workstation service, under its own account, needs the access rule widened to name one other user, and how that is kept from becoming a hole.
- Where `engine.logs` reads from when the engine is not running and logs are rotated. The started engine's output is its own file, `engine-output.log`, since [decision 0181](../../decisions/0181-the-product-and-the-local-engine-take-up-what-the-kernel-now-offers.md); it is not rotated.
- From the second safety review ([decision 0181](../../decisions/0181-the-product-and-the-local-engine-take-up-what-the-kernel-now-offers.md)), built by [decision 0192](../../decisions/0192-a-served-engine-hands-no-root-of-the-project-that-started-it-and-clients-start-and-compare.md): on Windows the runtime directory is refused when it is a link or another's and is given an access list naming only the user, which its lock files take, and the output file gets one of its own (S4; compiled and linted for Windows, not run); the start runs on a blocking thread that owns the `Launch` (S5); the hello names the engine's start variables by digest, so that a client whose own differ is told (S6).
